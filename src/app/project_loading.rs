use super::Stetson;
use crate::application::{AgentBackend, AppResult};
use cowboy::domain::{Project, ProjectCatalog};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Clone)]
pub(crate) struct LoadRequest {
    pub agent: usize,
    generation: u64,
    kind: LoadKind,
}

#[derive(Clone)]
enum LoadKind {
    Catalog,
    Project { cwd: PathBuf, sources: Vec<PathBuf> },
}

pub(crate) struct LoadCompletion {
    agent: usize,
    generation: u64,
    result: LoadResult,
}

pub(crate) enum LoadFeedback<'a> {
    Ready,
    Catalog,
    CatalogFailed(&'a str),
    Sessions,
    Failed(&'a str),
}

enum LoadResult {
    Catalog(AppResult<ProjectCatalog>),
    Project(PathBuf, AppResult<Project>),
}

impl LoadRequest {
    pub fn run(self, backend: &dyn AgentBackend) -> LoadCompletion {
        let result = match self.kind {
            LoadKind::Catalog => LoadResult::Catalog(backend.discover_projects()),
            LoadKind::Project { cwd, sources } => {
                let result = backend.load_project(&cwd, &sources);
                LoadResult::Project(cwd, result)
            }
        };
        LoadCompletion {
            agent: self.agent,
            generation: self.generation,
            result,
        }
    }

    pub fn failed(&self, error: String) -> LoadCompletion {
        let result = match &self.kind {
            LoadKind::Catalog => LoadResult::Catalog(Err(error)),
            LoadKind::Project { cwd, .. } => LoadResult::Project(cwd.clone(), Err(error)),
        };
        LoadCompletion {
            agent: self.agent,
            generation: self.generation,
            result,
        }
    }
}

#[derive(Default)]
struct AgentProjects {
    catalog: Option<AppResult<ProjectCatalog>>,
    projects: BTreeMap<PathBuf, AppResult<Project>>,
    generation: u64,
}

pub(super) struct ProjectLoading {
    agents: Vec<AgentProjects>,
    busy: bool,
    preferred: Option<PathBuf>,
}

impl Stetson {
    pub fn enable_project_loading(&mut self) {
        self.loading = Some(ProjectLoading {
            agents: (0..self.backends.len())
                .map(|_| AgentProjects::default())
                .collect(),
            busy: false,
            preferred: None,
        });
        self.state.status = "Discovering projects…".into();
    }

    pub fn load_feedback(&self) -> LoadFeedback<'_> {
        let Some(loading) = &self.loading else {
            return LoadFeedback::Ready;
        };
        let slot = &loading.agents[self.state.selected_agent];
        let result = self
            .current_project()
            .and_then(|project| slot.projects.get(&project.cwd));
        match (&slot.catalog, result) {
            (_, Some(Ok(_))) => LoadFeedback::Ready,
            (_, Some(Err(error))) => LoadFeedback::Failed(error),
            (None, None) => LoadFeedback::Catalog,
            (Some(Err(error)), None) => LoadFeedback::CatalogFailed(error),
            (Some(Ok(_)), None) if self.current_project().is_some() => LoadFeedback::Sessions,
            (Some(Ok(_)), None) => LoadFeedback::Ready,
        }
    }

    pub fn take_load_request(&mut self) -> Option<LoadRequest> {
        if self.should_quit() || self.delete_in_progress() {
            return None;
        }
        let agent = self.state.selected_agent;
        let project = self.state.projects.get(self.state.selected_project);
        let loading = self.loading.as_mut()?;
        if loading.busy {
            return None;
        }
        let slot = &loading.agents[agent];
        let kind = match &slot.catalog {
            None => LoadKind::Catalog,
            Some(Ok(catalog)) => {
                let cwd = &project?.cwd;
                if slot.projects.contains_key(cwd) {
                    return None;
                }
                LoadKind::Project {
                    cwd: cwd.clone(),
                    sources: catalog.get(cwd)?.clone(),
                }
            }
            Some(Err(_)) => return None,
        };
        loading.busy = true;
        Some(LoadRequest {
            agent,
            generation: slot.generation,
            kind,
        })
    }

    pub fn complete_project_load(&mut self, completion: LoadCompletion) {
        let Some(loading) = self.loading.as_mut() else {
            return;
        };
        loading.busy = false;
        let slot = &mut loading.agents[completion.agent];
        if slot.generation != completion.generation {
            return;
        }
        let visible = completion.agent == self.state.selected_agent;
        match completion.result {
            LoadResult::Catalog(result) => {
                slot.catalog = Some(result);
                if visible {
                    self.show_catalog();
                }
            }
            LoadResult::Project(cwd, mut result) => {
                if let (Some(id), Ok(project)) = (
                    self.state
                        .agent_tabs
                        .get(completion.agent)
                        .map(|tab| &tab.id),
                    &mut result,
                ) {
                    project
                        .sessions
                        .retain(|session| &session.key.agent_id == id);
                }
                if visible {
                    if let Ok(project) = &result {
                        if let Some(row) = self.state.projects.iter_mut().find(|row| row.cwd == cwd)
                        {
                            row.sessions.clone_from(&project.sessions);
                        }
                    }
                }
                slot.projects.insert(cwd, result);
                if visible {
                    self.clamp_selections();
                }
            }
        }
    }

    pub(super) fn refresh_project_catalog(&mut self, cwd: &Path) {
        let current = self.current_project().map(|project| project.cwd.clone());
        let Some(loading) = self.loading.as_mut() else {
            return;
        };
        loading.preferred = current;
        loading.agents[self.state.selected_agent].catalog = None;
        self.invalidate_project_sessions(cwd);
    }

    pub(super) fn invalidate_project_sessions(&mut self, cwd: &Path) {
        let Some(loading) = self.loading.as_mut() else {
            return;
        };
        let slot = &mut loading.agents[self.state.selected_agent];
        slot.generation += 1;
        slot.projects.remove(cwd);
        if let Some(row) = self.state.projects.iter_mut().find(|row| row.cwd == cwd) {
            row.sessions.clear();
        }
    }

    pub(super) fn switch_loading_agent(&mut self, agent: usize) {
        let current = self.current_project().map(|project| project.cwd.clone());
        let loading = self.loading.as_mut().expect("project loading enabled");
        if current.is_some() {
            loading.preferred = current;
        }
        let target = &mut loading.agents[agent];
        if target.catalog.as_ref().is_some_and(Result::is_err) {
            target.catalog = None;
        }
        self.state.selected_agent = agent;
        self.show_catalog();
        self.retry_selected_project();
    }

    fn show_catalog(&mut self) {
        let loading = self.loading.as_mut().expect("project loading enabled");
        let slot = &loading.agents[self.state.selected_agent];
        self.state.projects = match &slot.catalog {
            Some(Ok(catalog)) => catalog
                .keys()
                .map(|cwd| {
                    slot.projects
                        .get(cwd)
                        .and_then(|p| p.as_ref().ok())
                        .cloned()
                        .unwrap_or_else(|| Project {
                            cwd: cwd.clone(),
                            sessions: Vec::new(),
                        })
                })
                .collect(),
            _ => Vec::new(),
        };
        self.state.selected_project = loading
            .preferred
            .as_ref()
            .and_then(|cwd| self.state.projects.iter().position(|p| &p.cwd == cwd))
            .unwrap_or(0);
        self.state.selected_session = 0;
        self.state.status = match &slot.catalog {
            None => "Discovering projects…".into(),
            Some(Err(error)) => format!("Could not discover projects: {error}"),
            Some(Ok(catalog)) if catalog.is_empty() => "No projects found".into(),
            Some(Ok(_)) => format!(
                "Agent: {}",
                self.state.agent_tabs[self.state.selected_agent].label
            ),
        };
    }

    pub(super) fn retry_selected_project(&mut self) {
        if let (Some(loading), Some(project)) = (
            &mut self.loading,
            self.state.projects.get(self.state.selected_project),
        ) {
            let slot = &mut loading.agents[self.state.selected_agent];
            if slot.projects.get(&project.cwd).is_some_and(Result::is_err) {
                slot.projects.remove(&project.cwd);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::app::{ResumeTarget, Stetson};
    use crate::application::{AgentBackend, AppResult, ResumeLauncher, SessionRepository};
    use cowboy::domain::{AgentId, Project, ProjectCatalog, Session, SessionKey};
    use std::{
        cell::RefCell,
        path::{Path, PathBuf},
    };

    struct RecordingBackend {
        catalog: ProjectCatalog,
        calls: RefCell<Vec<Option<PathBuf>>>,
        title: &'static str,
    }

    impl RecordingBackend {
        fn new(paths: &[&str]) -> Self {
            Self {
                catalog: paths
                    .iter()
                    .map(|p| (PathBuf::from(p), Vec::new()))
                    .collect(),
                calls: RefCell::new(Vec::new()),
                title: "Initial title",
            }
        }
    }

    impl SessionRepository for RecordingBackend {
        fn load_projects(&self) -> AppResult<Vec<Project>> {
            panic!("must not load all sessions")
        }
        fn rename_session(&self, key: &SessionKey, title: &str) -> AppResult<()> {
            assert_eq!(key.native_id, "session");
            assert_eq!(title, "New title");
            Ok(())
        }
        fn delete_session(&self, _: &SessionKey) -> AppResult<()> {
            unreachable!()
        }
        fn delete_project(&self, _: &Path) -> AppResult<()> {
            unreachable!()
        }
    }

    impl ResumeLauncher for RecordingBackend {
        fn resume(&self, _: &ResumeTarget) -> AppResult<()> {
            unreachable!()
        }
        fn launch_new(&self, _: &Path) -> AppResult<()> {
            unreachable!()
        }
    }

    impl AgentBackend for RecordingBackend {
        fn id(&self) -> AgentId {
            AgentId::new("claude").unwrap()
        }
        fn label(&self) -> &str {
            "Claude"
        }
        fn discover_projects(&self) -> AppResult<ProjectCatalog> {
            self.calls.borrow_mut().push(None);
            Ok(self.catalog.clone())
        }
        fn load_project(&self, cwd: &Path, _: &[PathBuf]) -> AppResult<Project> {
            self.calls.borrow_mut().push(Some(cwd.to_path_buf()));
            Ok(Project {
                cwd: cwd.to_path_buf(),
                sessions: vec![Session {
                    key: SessionKey::claude("session"),
                    title: self.title.into(),
                    cwd: cwd.to_path_buf(),
                    source_location: None,
                    git_branch: None,
                    created_at: None,
                    updated_at: None,
                    message_count: None,
                    usage: None,
                    model: None,
                    estimated_cost: None,
                    reported_cost: None,
                    capabilities: Default::default(),
                }],
            })
        }
    }

    fn dispatch(app: &mut Stetson, backend: &RecordingBackend) {
        let request = app.take_load_request().expect("pending load");
        app.complete_project_load(request.run(backend));
    }

    #[test]
    fn loads_only_the_first_project_then_loads_on_visit_and_reuses_results() {
        let backend = RecordingBackend::new(&["zeta", "alpha"]);
        let mut app = Stetson::new(vec![Box::new(RecordingBackend::new(&[]))]);
        app.enable_project_loading();
        dispatch(&mut app, &backend);
        assert_eq!(app.current_project().unwrap().cwd, Path::new("alpha"));
        dispatch(&mut app, &backend);
        assert!(app.take_load_request().is_none());

        app.move_selection(1);
        dispatch(&mut app, &backend);
        app.move_selection(-1);
        assert!(app.take_load_request().is_none());
        app.move_selection(2);
        assert!(app.is_open_here_selected());
        assert!(app.take_load_request().is_none());
        assert_eq!(
            *backend.calls.borrow(),
            vec![None, Some("alpha".into()), Some("zeta".into())]
        );
    }

    #[test]
    fn coalesces_navigation_and_keeps_a_late_result_out_of_another_agent() {
        let backend = RecordingBackend::new(&["a", "b", "c"]);
        let other = RecordingBackend::new(&["c", "d"]);
        let mut app = Stetson::new(vec![
            Box::new(RecordingBackend::new(&[])),
            Box::new(RecordingBackend::new(&[])),
        ]);
        app.enable_project_loading();
        dispatch(&mut app, &backend);
        let first = app.take_load_request().unwrap();
        app.move_selection(1);
        app.move_selection(1);
        assert!(app.take_load_request().is_none());
        app.complete_project_load(first.run(&backend));
        assert_eq!(app.current_project().unwrap().cwd, Path::new("c"));
        let latest = app.take_load_request().unwrap();
        app.switch_to_agent(1);
        app.complete_project_load(latest.run(&backend));
        assert!(app.state().projects.is_empty());
        dispatch(&mut app, &other);
        dispatch(&mut app, &other);
        assert_eq!(
            *backend.calls.borrow(),
            vec![None, Some("a".into()), Some("c".into())]
        );
        assert_eq!(*other.calls.borrow(), vec![None, Some("c".into())]);
    }

    #[test]
    fn discards_a_completion_invalidated_by_a_catalog_refresh() {
        let backend = RecordingBackend::new(&["a"]);
        let mut app = Stetson::new(vec![Box::new(RecordingBackend::new(&[]))]);
        app.enable_project_loading();
        dispatch(&mut app, &backend);
        let obsolete = app.take_load_request().unwrap();
        app.refresh_project_catalog(Path::new("a"));
        assert!(app.take_load_request().is_none());
        app.complete_project_load(obsolete.run(&backend));
        dispatch(&mut app, &backend);
        dispatch(&mut app, &backend);
        assert_eq!(
            *backend.calls.borrow(),
            vec![None, Some("a".into()), None, Some("a".into())]
        );
    }

    #[test]
    fn deletion_refreshes_membership_without_reloading_unrelated_cached_projects() {
        use crate::app::{DeleteCompletion, DeleteOutcome, DeleteTarget};

        let backend = RecordingBackend::new(&["a", "b"]);
        let after_delete = RecordingBackend::new(&["b"]);
        let mut app = Stetson::new(vec![Box::new(RecordingBackend::new(&[]))]);
        app.enable_project_loading();
        dispatch(&mut app, &backend);
        dispatch(&mut app, &backend);
        app.move_selection(1);
        dispatch(&mut app, &backend);
        app.move_selection(-1);
        app.complete_delete(DeleteCompletion {
            target: DeleteTarget::Project {
                agent_id: backend.id(),
                name: "a".into(),
                cwd: "a".into(),
            },
            outcome: DeleteOutcome::Applied,
        });
        dispatch(&mut app, &after_delete);
        assert_eq!(app.current_project().unwrap().cwd, Path::new("b"));
        assert!(app.take_load_request().is_none());
        assert_eq!(*after_delete.calls.borrow(), vec![None]);
    }

    #[test]
    fn rename_and_resume_refresh_only_the_affected_sessions() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut backend = RecordingBackend::new(&["a", "b"]);
        let mut app = Stetson::new(vec![Box::new(RecordingBackend::new(&[]))]);
        app.enable_project_loading();
        dispatch(&mut app, &backend);
        dispatch(&mut app, &backend);
        app.move_selection(1);
        dispatch(&mut app, &backend);
        app.move_selection(-1);
        backend.calls.borrow_mut().clear();

        app.begin_rename();
        app.state.input_buffer = "New title".into();
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        backend.title = "New title";
        dispatch(&mut app, &backend);
        assert_eq!(app.current_session().unwrap().title, "New title");
        app.resume_finished("done");
        backend.title = "After resume";
        dispatch(&mut app, &backend);
        dispatch(&mut app, &backend);
        assert_eq!(app.current_session().unwrap().title, "After resume");
        app.move_selection(1);
        assert_eq!(app.current_session().unwrap().title, "Initial title");
        assert!(app.take_load_request().is_none());
        assert_eq!(
            *backend.calls.borrow(),
            vec![Some("a".into()), None, Some("a".into())]
        );
    }

    #[test]
    fn a_query_entered_during_loading_filters_only_the_selected_project() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let backend = RecordingBackend::new(&["a", "b"]);
        let mut app = Stetson::new(vec![Box::new(RecordingBackend::new(&[]))]);
        app.enable_project_loading();
        dispatch(&mut app, &backend);
        let pending = app.take_load_request().unwrap();
        let mut key = |code| app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
        key(KeyCode::Char('/'));
        for c in "no match".chars() {
            key(KeyCode::Char(c));
        }
        key(KeyCode::Enter);
        app.complete_project_load(pending.run(&backend));
        assert_eq!(app.state().search_query, "no match");
        assert!(app.filtered_sessions().is_empty());
        assert_eq!(app.current_project().unwrap().sessions.len(), 1);
        assert!(app.take_load_request().is_none());
        assert_eq!(*backend.calls.borrow(), vec![None, Some("a".into())]);
    }

    #[test]
    fn empty_catalog_does_not_start_a_session_load() {
        let backend = RecordingBackend::new(&[]);
        let mut app = Stetson::new(vec![Box::new(RecordingBackend::new(&[]))]);
        app.enable_project_loading();
        dispatch(&mut app, &backend);
        assert!(app.is_open_here_selected());
        assert!(app.take_load_request().is_none());
        assert_eq!(*backend.calls.borrow(), vec![None]);
    }
}
