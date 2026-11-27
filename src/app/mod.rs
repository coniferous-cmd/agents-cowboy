mod project_actions;
mod project_loading;
mod session_actions;

// ── Internal modules ─────────────────────────────────────────────────────────

mod state {
    use cowboy::domain::{AgentId, SessionKey};
    use cowboy::theme::ThemePalette;
    use std::path::PathBuf;
    use std::time::Instant;

    pub const OPEN_HERE_PROJECT_LABEL: &str = "Open Here";

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum FocusPane {
        Projects,
        Sessions,
    }

    /// One tab in the agent strip: a registered backend strategy. Exactly one
    /// tab is always selected; there is no `All` variant.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct AgentTab {
        pub id: AgentId,
        pub label: String,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum ModalState {
        None,
        Search,
        Rename,
        DeleteConfirm,
        Info,
    }

    /// Result of a worker-thread deletion, paired with its originating target.
    ///
    /// The worker owns the filesystem I/O and returns one of these back to the
    /// main loop. Thread and channel plumbing lives in `main.rs`; this type only
    /// carries data across the boundary.
    #[derive(Debug)]
    pub struct DeleteCompletion {
        pub target: DeleteTarget,
        pub outcome: DeleteOutcome,
    }

    #[derive(Debug)]
    pub enum DeleteOutcome {
        /// Deletion succeeded; refresh the affected project asynchronously.
        Applied,
        #[cfg(test)]
        Success(Vec<cowboy::domain::Project>),
        /// Deletion (or the reload after it) failed; carries an error message.
        Failure(String),
    }

    #[derive(Debug, Clone)]
    pub struct Toast {
        pub message: String,
        pub created_at: Instant,
    }

    // ── DeleteTarget with internal Kind enum ───────────────────────────────

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum DeleteTarget {
        Project {
            agent_id: AgentId,
            name: String,
            cwd: PathBuf,
        },
        Session {
            title: String,
            key: SessionKey,
        },
    }

    impl DeleteTarget {
        pub fn agent_id(&self) -> &AgentId {
            match self {
                Self::Project { agent_id, .. } => agent_id,
                Self::Session { key, .. } => &key.agent_id,
            }
        }
        /// Internal kind tag — not exposed outside this module.
        pub(crate) fn kind(&self) -> Kind {
            match self {
                Self::Project { .. } => Kind::Project,
                Self::Session { .. } => Kind::Session,
            }
        }

        pub fn name(&self) -> &str {
            match self {
                Self::Project { name, .. } => name,
                Self::Session { title, .. } => title,
            }
        }

        /// Human-readable kind label: "Project" or "Session".
        pub fn label(&self) -> &'static str {
            self.kind().label()
        }

        pub fn prompt_status(&self) -> String {
            format!("Confirm delete {}...", self.label().to_ascii_lowercase())
        }

        pub fn success_status(&self) -> String {
            format!("{} deleted", self.label())
        }

        pub fn confirmation_lines(&self) -> [String; 4] {
            [
                format!(
                    "Delete {} '{}'?",
                    self.label().to_ascii_lowercase(),
                    self.name()
                ),
                String::new(),
                "Enter, y, or Ctrl+D to confirm".to_string(),
                "q, Esc, or n to cancel".to_string(),
            ]
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum Kind {
        Project,
        Session,
    }

    impl Kind {
        pub(crate) fn label(self) -> &'static str {
            match self {
                Self::Project => "Project",
                Self::Session => "Session",
            }
        }
    }

    // ── SessionInfo (moved from features/session_info) ─────────────────────

    #[derive(Debug, Clone)]
    pub struct SessionInfo {
        pub key: SessionKey,
        pub title: String,
        pub project_name: String,
        pub working_dir: String,
        pub source_location: Option<String>,
        pub git_branch: Option<String>,
        pub created_at: Option<String>,
        pub updated_at: Option<String>,
        pub message_count: Option<usize>,
        pub usage: Option<cowboy::domain::SessionUsage>,
        pub model: Option<String>,
        pub estimated_cost: Option<cowboy::domain::CostEstimate>,
        pub reported_cost: Option<cowboy::domain::ReportedCost>,
    }

    pub fn build_session_info(
        project: &cowboy::domain::Project,
        session: &cowboy::domain::Session,
    ) -> SessionInfo {
        SessionInfo {
            key: session.key.clone(),
            title: session.title.clone(),
            project_name: project.name(),
            working_dir: project.cwd.display().to_string(),
            source_location: session.source_location.clone(),
            git_branch: session.git_branch.clone(),
            created_at: session.created_at.clone(),
            updated_at: session.updated_at.clone(),
            message_count: session.message_count,
            usage: session.usage.clone(),
            model: session.model.clone(),
            estimated_cost: session.estimated_cost.clone(),
            reported_cost: session.reported_cost,
        }
    }

    // ── ResumeTarget (moved from features/session_resume) ──────────────────

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct ResumeTarget {
        pub key: SessionKey,
        pub cwd: PathBuf,
    }

    pub fn session_resume_target(session: &cowboy::domain::Session) -> ResumeTarget {
        ResumeTarget {
            key: session.key.clone(),
            cwd: session.cwd.clone(),
        }
    }

    // ── AppState ────────────────────────────────────────────────────────────

    pub struct AppState {
        pub projects: Vec<cowboy::domain::Project>,
        pub agent_tabs: Vec<AgentTab>,
        pub selected_agent: usize,
        /// Per-agent project caches, parallel to the registry. `None` means
        /// the agent has not been visited yet (lazy loading).
        pub project_cache: Vec<Option<Vec<cowboy::domain::Project>>>,
        pub selected_project: usize,
        pub selected_session: usize,
        pub focus: FocusPane,
        pub modal: ModalState,
        pub status: String,
        pub search_query: String,
        pub input_buffer: String,
        pub theme: ThemePalette,
        pub should_quit: bool,
        pub pending_resume: Option<ResumeTarget>,
        pub pending_new_session: Option<PathBuf>,
        pub info: Option<SessionInfo>,
        pub delete_target: Option<DeleteTarget>,
        pub pending_delete: Option<DeleteTarget>,
        pub delete_in_progress: Option<DeleteTarget>,
        pub toast: Option<Toast>,
    }

    impl Default for AppState {
        fn default() -> Self {
            Self {
                projects: Vec::new(),
                agent_tabs: Vec::new(),
                selected_agent: 0,
                project_cache: Vec::new(),
                selected_project: 0,
                selected_session: 0,
                focus: FocusPane::Projects,
                modal: ModalState::None,
                status: String::from("Loading ~/.claude..."),
                search_query: String::new(),
                input_buffer: String::new(),
                should_quit: false,
                pending_resume: None,
                pending_new_session: None,
                info: None,
                delete_target: None,
                pending_delete: None,
                delete_in_progress: None,
                toast: None,
                theme: ThemePalette::default(),
            }
        }
    }

    impl AppState {
        /// The selected tab's agent id, or `None` when no tabs are registered
        /// (default-constructed state used in rendering tests).
        pub fn selected_agent_id(&self) -> Option<&AgentId> {
            self.agent_tabs.get(self.selected_agent).map(|tab| &tab.id)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::{build_session_info, session_resume_target, DeleteTarget};
        use cowboy::domain::{Project, Session, SessionCapabilities, SessionKey, SessionUsage};
        use std::path::PathBuf;

        fn make_session(usage: Option<SessionUsage>) -> Session {
            Session {
                key: SessionKey::claude("s1"),
                title: "Test Session".to_string(),
                cwd: PathBuf::from("/work/repo"),
                git_branch: Some("main".to_string()),
                created_at: Some("2025-01-01T00:00:00Z".to_string()),
                updated_at: Some("2025-01-02T00:00:00Z".to_string()),
                source_location: Some("/tmp/s1.jsonl".into()),
                message_count: Some(5),
                usage,
                model: Some("claude-sonnet-5".to_string()),
                estimated_cost: None,
                reported_cost: None,
                capabilities: SessionCapabilities::default(),
            }
        }

        fn make_project() -> Project {
            Project {
                cwd: PathBuf::from("/work/repo"),
                sessions: vec![],
            }
        }

        #[test]
        fn build_session_info_passes_through_all_usage_categories() {
            let usage = SessionUsage {
                input_tokens: 100,
                output_tokens: 200,
                cache_creation_tokens: 300,
                cache_read_tokens: 400,
            };
            let session = make_session(Some(usage));
            let project = make_project();

            let info = build_session_info(&project, &session);

            let u = info.usage.expect("usage should be present");
            assert_eq!(u.input_tokens, 100);
            assert_eq!(u.output_tokens, 200);
            assert_eq!(u.cache_creation_tokens, 300);
            assert_eq!(u.cache_read_tokens, 400);
            assert_eq!(u.total_tokens(), 1000);
        }

        #[test]
        fn reported_cost_passes_through_without_an_estimate() {
            let mut session = make_session(None);
            let cost = cowboy::domain::ReportedCost {
                total: 0.25,
                from_provider: true,
            };
            session.reported_cost = Some(cost);
            let info = build_session_info(&make_project(), &session);
            assert_eq!(info.reported_cost, Some(cost));
            assert!(info.estimated_cost.is_none());
        }

        #[test]
        fn build_session_info_handles_missing_usage() {
            let session = make_session(None);
            let project = make_project();

            let info = build_session_info(&project, &session);
            assert!(info.usage.is_none());
        }

        #[test]
        fn build_session_info_usage_total_matches_domain_definition() {
            let usage = SessionUsage {
                input_tokens: 10,
                output_tokens: 20,
                cache_creation_tokens: 30,
                cache_read_tokens: 40,
            };
            let session = make_session(Some(usage.clone()));
            let project = make_project();

            let info = build_session_info(&project, &session);

            let info_total = info.usage.unwrap().total_tokens();
            let domain_total = usage.total_tokens();
            assert_eq!(info_total, domain_total);
        }

        #[test]
        fn delete_target_prompt_status() {
            let target = DeleteTarget::Project {
                agent_id: cowboy::domain::AgentId::new("claude").unwrap(),
                name: "my-project".to_string(),
                cwd: PathBuf::from("/work/repo"),
            };
            assert_eq!(target.prompt_status(), "Confirm delete project...");
        }

        #[test]
        fn delete_target_success_status() {
            let target = DeleteTarget::Session {
                title: "My Session".to_string(),
                key: SessionKey::claude("s1"),
            };
            assert_eq!(target.success_status(), "Session deleted");
        }

        #[test]
        fn delete_target_confirmation_lines() {
            let target = DeleteTarget::Project {
                agent_id: cowboy::domain::AgentId::new("claude").unwrap(),
                name: "my-project".to_string(),
                cwd: PathBuf::from("/work/repo"),
            };
            let lines = target.confirmation_lines();
            assert_eq!(lines[0], "Delete project 'my-project'?");
        }

        #[test]
        fn resume_target_from_session() {
            let session = make_session(None);
            let target = session_resume_target(&session);
            assert_eq!(target.key.native_id, "s1");
            assert_eq!(target.cwd, PathBuf::from("/work/repo"));
        }
    }
}

// ── Internal module: navigation ──────────────────────────────────────────────

mod navigation {
    use crate::application::{project_name_owned, project_working_dir, session_key, session_title};
    use cowboy::features::session_list::projects_for_agent;

    use super::state::{FocusPane, OPEN_HERE_PROJECT_LABEL};
    use super::Stetson;

    impl Stetson {
        pub fn toggle_focus(&mut self) {
            self.state.focus = match self.state.focus {
                FocusPane::Projects => FocusPane::Sessions,
                FocusPane::Sessions => FocusPane::Projects,
            };
            self.state.status = match self.state.focus {
                FocusPane::Projects => "Focus: Projects".to_string(),
                FocusPane::Sessions => "Focus: Sessions".to_string(),
            };
        }

        /// Cycle the agent tab selection in registration order, wrapping to
        /// the first. With a single registered agent this is a no-op that
        /// still reports the active agent.
        pub fn cycle_agent_tab(&mut self) {
            let next = (self.state.selected_agent + 1) % self.backends.len();
            if next == self.state.selected_agent {
                self.state.status = format!("Agent: {}", self.state.agent_tabs[next].label);
                return;
            }
            self.switch_to_agent(next);
        }

        /// Select an agent tab and show its projects. The search query is a
        /// view filter and survives the switch; session selection resets.
        pub fn switch_to_agent(&mut self, index: usize) {
            if index >= self.backends.len() {
                return;
            }
            if self.loading.is_some() {
                self.switch_loading_agent(index);
                return;
            }
            let projects = if let Some(Some(cached)) = self.state.project_cache.get(index) {
                cached.clone()
            } else {
                match self.backends[index].load_projects() {
                    Ok(projects) => {
                        let agent_id = self.backends[index].id();
                        let projects =
                            cowboy::features::session_list::projects_for_agent(projects, &agent_id);
                        if let Some(slot) = self.state.project_cache.get_mut(index) {
                            *slot = Some(projects.clone());
                        }
                        projects
                    }
                    Err(error) => {
                        self.show_toast(error);
                        return;
                    }
                }
            };
            self.state.selected_agent = index;
            let status = format!("Agent: {}", self.state.agent_tabs[index].label);
            self.replace_projects(projects, &status);
            self.state.selected_session = 0;
        }

        pub fn move_selection(&mut self, delta: isize) {
            match self.state.focus {
                FocusPane::Projects => {
                    let next = offset_index(
                        self.state.selected_project,
                        delta,
                        self.project_item_count(),
                    );
                    if next != self.state.selected_project {
                        self.state.selected_project = next;
                        self.state.selected_session = 0;
                        self.state.status = format!("Project: {}", self.selected_project_label());
                        self.retry_selected_project();
                    }
                }
                FocusPane::Sessions => {
                    let next = offset_index(
                        self.state.selected_session,
                        delta,
                        self.filtered_sessions().len(),
                    );
                    if next != self.state.selected_session {
                        self.state.selected_session = next;
                        self.state.status = format!(
                            "Session: {}",
                            self.current_session()
                                .map(session_title)
                                .unwrap_or("No sessions")
                        );
                    }
                }
            }
        }

        pub fn project_item_count(&self) -> usize {
            self.state.projects.len() + 1
        }

        pub fn is_open_here_selected(&self) -> bool {
            self.state.selected_project == self.state.projects.len()
        }

        pub fn selected_project_label(&self) -> String {
            if self.is_open_here_selected() {
                OPEN_HERE_PROJECT_LABEL.to_string()
            } else {
                self.current_project()
                    .map(project_name_owned)
                    .unwrap_or_else(|| "No projects".to_string())
            }
        }

        pub(super) fn replace_projects(
            &mut self,
            projects: Vec<cowboy::domain::Project>,
            status: &str,
        ) {
            let current_project_key = self
                .current_project()
                .map(project_working_dir)
                .map(|value| value.to_string());
            let current_session_key = self.current_session().map(session_key).cloned();

            // Scope to the selected agent before anything renders or sums:
            // directories without this agent's sessions disappear, and the
            // remaining rows count only this agent's sessions.
            let projects = match self.state.selected_agent_id() {
                Some(agent_id) => projects_for_agent(projects, agent_id),
                None => projects,
            };
            self.state.projects = projects;
            if let Some(slot) = self.state.project_cache.get_mut(self.state.selected_agent) {
                *slot = Some(self.state.projects.clone());
            }
            self.state.delete_target = None;
            self.state.selected_project = current_project_key
                .as_deref()
                .and_then(|key| {
                    self.state
                        .projects
                        .iter()
                        .position(|project| project_working_dir(project) == key)
                })
                .unwrap_or(0);
            self.state.selected_session = current_session_key
                .as_ref()
                .and_then(|key| {
                    self.filtered_sessions()
                        .iter()
                        .position(|session| session_key(session) == key)
                })
                .unwrap_or(0);
            self.clamp_selections();
            self.state.status = if self.state.projects.is_empty() {
                "No projects found under ~/.claude".to_string()
            } else {
                status.to_string()
            };
        }

        pub(super) fn clamp_selections(&mut self) {
            if self.state.selected_project >= self.project_item_count() {
                self.state.selected_project = self.project_item_count().saturating_sub(1);
            }

            let session_count = self.filtered_sessions().len();
            if self.state.selected_session >= session_count {
                self.state.selected_session = session_count.saturating_sub(1);
            }
        }
    }

    pub fn offset_index(current: usize, delta: isize, len: usize) -> usize {
        if len == 0 {
            return 0;
        }
        let max = len.saturating_sub(1) as isize;
        (current as isize + delta).clamp(0, max) as usize
    }
}

// ── Internal module: modal ───────────────────────────────────────────────────

mod modal {
    use crate::application::session_key;
    use cowboy::domain::validate_rename_title;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::state::ModalState;
    use super::Stetson;

    impl Stetson {
        pub fn handle_key(&mut self, key: KeyEvent) {
            if self.delete_in_progress() {
                let is_quit = matches!(key.code, KeyCode::Char('q'))
                    || (matches!(key.code, KeyCode::Char('c'))
                        && key.modifiers.contains(KeyModifiers::CONTROL));
                if is_quit {
                    self.state.status = "Deleting... please wait".to_string();
                }
                return;
            }

            self.expire_toast();
            self.state.toast = None;
            match self.state.modal {
                ModalState::Search => self.handle_search_key(key),
                ModalState::Rename => self.handle_rename_key(key),
                ModalState::DeleteConfirm => self.handle_delete_key(key),
                ModalState::Info => self.handle_info_key(key),
                ModalState::None => self.handle_normal_key(key),
            }
        }

        fn handle_normal_key(&mut self, key: KeyEvent) {
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => self.state.should_quit = true,
                KeyCode::Tab | KeyCode::Right | KeyCode::Left | KeyCode::Char('[' | ']') => {
                    self.toggle_focus()
                }
                KeyCode::Down => self.move_selection(1),
                KeyCode::Up => self.move_selection(-1),
                KeyCode::Char('a') => self.cycle_agent_tab(),
                KeyCode::Char('/') => self.open_search(),
                KeyCode::Char('i') => self.open_info(),
                KeyCode::Char('r') => self.begin_rename(),
                KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.begin_delete()
                }
                KeyCode::Char('n') if self.state.focus == super::state::FocusPane::Sessions => {
                    self.new_session()
                }
                KeyCode::Enter => self.activate_selection(),
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.state.should_quit = true;
                }
                _ => {}
            }
        }

        fn handle_search_key(&mut self, key: KeyEvent) {
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => {
                    self.state.modal = ModalState::None;
                    self.state.input_buffer.clear();
                    self.state.status = "Search canceled".to_string();
                }
                KeyCode::Enter => {
                    let submission = apply_search(&self.state.input_buffer);
                    self.state.search_query = submission.query;
                    self.state.selected_session = 0;
                    self.state.modal = ModalState::None;
                    self.state.status = submission.status;
                }
                KeyCode::Backspace => {
                    self.state.input_buffer.pop();
                }
                KeyCode::Char(ch) => self.state.input_buffer.push(ch),
                _ => {}
            }
        }

        fn handle_rename_key(&mut self, key: KeyEvent) {
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => {
                    self.state.modal = ModalState::None;
                    self.state.input_buffer.clear();
                    self.state.status = "Rename canceled".to_string();
                }
                KeyCode::Enter => {
                    let Some(session_key) = self.current_session().map(session_key).cloned() else {
                        self.show_toast("No session selected");
                        self.state.modal = ModalState::None;
                        self.state.input_buffer.clear();
                        return;
                    };

                    let new_title: String = match validate_rename_title(&self.state.input_buffer) {
                        Ok(title) => title,
                        Err(error) => {
                            self.show_toast(error.to_string());
                            return;
                        }
                    };

                    let backend = self.selected_backend();
                    if self.loading.is_some() {
                        let cwd = self
                            .current_session()
                            .expect("session selected")
                            .cwd
                            .clone();
                        match backend.rename_session(&session_key, &new_title) {
                            Ok(()) => {
                                self.state.modal = ModalState::None;
                                self.state.input_buffer.clear();
                                self.invalidate_project_sessions(&cwd);
                                self.state.status = "Session renamed".into();
                            }
                            Err(error) => self.show_toast(error),
                        }
                        return;
                    }
                    match backend
                        .rename_session(&session_key, &new_title)
                        .and_then(|()| backend.load_projects())
                    {
                        Ok(projects) => {
                            self.state.modal = ModalState::None;
                            self.state.input_buffer.clear();
                            self.replace_projects(projects, "Session renamed");
                        }
                        Err(error) => self.show_toast(error),
                    }
                }
                KeyCode::Backspace => {
                    self.state.input_buffer.pop();
                }
                KeyCode::Char(ch) => self.state.input_buffer.push(ch),
                _ => {}
            }
        }

        fn handle_delete_key(&mut self, key: KeyEvent) {
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => {
                    self.state.modal = ModalState::None;
                    self.state.status = "Delete canceled".to_string();
                    self.state.delete_target = None;
                }
                KeyCode::Char('y') | KeyCode::Enter => self.confirm_delete(),
                KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.confirm_delete()
                }
                KeyCode::Char('n') => {
                    self.state.modal = ModalState::None;
                    self.state.status = "Delete canceled".to_string();
                    self.state.delete_target = None;
                }
                _ => {}
            }
        }

        fn confirm_delete(&mut self) {
            let Some(target) = self.state.delete_target.take() else {
                self.show_toast("Nothing selected to delete");
                self.state.modal = ModalState::None;
                return;
            };

            self.state.modal = ModalState::None;
            self.state.status = format!("Deleting {}...", target.name());
            self.state.pending_delete = Some(target.clone());
            self.state.delete_in_progress = Some(target);
        }

        fn handle_info_key(&mut self, key: KeyEvent) {
            if matches!(key.code, KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q')) {
                self.state.modal = ModalState::None;
                self.state.status = "Closed session info".to_string();
            }
        }
    }

    // ── Private helpers ────────────────────────────────────────────────────

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct SearchSubmission {
        pub query: String,
        pub status: String,
    }

    fn apply_search(input: &str) -> SearchSubmission {
        let query = input.trim().to_string();
        let status = if query.is_empty() {
            "Search cleared".to_string()
        } else {
            format!("Filtering sessions by '{query}'")
        };
        SearchSubmission { query, status }
    }

    #[cfg(test)]
    mod tests {
        use super::apply_search;

        #[test]
        fn apply_search_trims_query() {
            let submission = apply_search("  abc  ");
            assert_eq!(submission.query, "abc");
            assert_eq!(submission.status, "Filtering sessions by 'abc'");
        }

        #[test]
        fn apply_search_empty_clears() {
            let submission = apply_search("");
            assert_eq!(submission.query, "");
            assert_eq!(submission.status, "Search cleared");
        }
    }
}

// ── Public re-exports ────────────────────────────────────────────────────────

#[cfg(test)]
pub(crate) use state::build_session_info;

pub(crate) use project_loading::{LoadCompletion, LoadFeedback, LoadRequest};
pub use state::{
    AgentTab, AppState, DeleteCompletion, DeleteOutcome, DeleteTarget, FocusPane, ModalState,
    ResumeTarget, Toast,
};

// ── Stetson main impl ────────────────────────────────────────────────────────

use crate::application::AgentBackend;
#[cfg(test)]
use crate::application::AppResult;
use std::path::PathBuf;

#[cfg(test)]
#[derive(Debug)]
pub struct InitialLoadCompletion {
    pub projects: AppResult<Vec<cowboy::domain::Project>>,
}

pub struct Stetson {
    backends: Vec<Box<dyn AgentBackend>>,
    state: AppState,
    loading: Option<project_loading::ProjectLoading>,
}

impl Stetson {
    pub fn new(backends: Vec<Box<dyn AgentBackend>>) -> Self {
        assert!(
            !backends.is_empty(),
            "agent registry must contain at least one backend"
        );
        let mut state = AppState::default();
        state.agent_tabs = backends
            .iter()
            .map(|backend| state::AgentTab {
                id: backend.id(),
                label: backend.label().to_string(),
            })
            .collect();
        state.project_cache = vec![None; backends.len()];
        Self {
            backends,
            state,
            loading: None,
        }
    }

    /// The backend of the currently selected agent tab.
    pub fn selected_backend(&self) -> &dyn AgentBackend {
        self.backends
            .get(self.state.selected_agent)
            .expect("selected agent is registered")
            .as_ref()
    }

    pub fn state(&self) -> &AppState {
        &self.state
    }

    pub fn show_toast(&mut self, message: impl Into<String>) {
        self.state.toast = Some(Toast {
            message: message.into(),
            created_at: std::time::Instant::now(),
        });
    }

    fn expire_toast(&mut self) {
        if let Some(toast) = &self.state.toast {
            if toast.created_at.elapsed() > std::time::Duration::from_secs(3) {
                self.state.toast = None;
            }
        }
    }

    pub fn set_theme(&mut self, theme: cowboy::theme::ThemePalette) {
        self.state.theme = theme;
    }

    pub fn should_quit(&self) -> bool {
        self.state.should_quit
    }

    pub fn take_pending_resume(&mut self) -> Option<ResumeTarget> {
        self.state.pending_resume.take()
    }

    pub fn take_pending_new_session(&mut self) -> Option<PathBuf> {
        self.state.pending_new_session.take()
    }

    /// Take the confirmed-but-unstarted delete task for the main loop to run on
    /// a worker thread. Returns `Some` at most once per queued task.
    pub fn take_pending_delete(&mut self) -> Option<DeleteTarget> {
        self.state.pending_delete.take()
    }

    /// Whether a deletion worker currently owns a target. While true the app
    /// must block exit and every conflicting action.
    pub fn delete_in_progress(&self) -> bool {
        self.state.delete_in_progress.is_some()
    }

    /// Apply a worker's delete result: refresh + success status on success,
    /// or retain the list and surface an error toast on failure. Either way the
    /// UI is unlocked for a subsequent exit.
    pub fn complete_delete(&mut self, completion: DeleteCompletion) {
        self.state.delete_in_progress = None;
        self.state.pending_delete = None;

        match completion.outcome {
            DeleteOutcome::Applied => {
                let cwd = match &completion.target {
                    DeleteTarget::Project { cwd, .. } => Some(cwd.clone()),
                    DeleteTarget::Session { .. } => self.current_project().map(|p| p.cwd.clone()),
                };
                if let Some(cwd) = cwd {
                    self.refresh_project_catalog(&cwd);
                }
                self.state.status = completion.target.success_status();
            }
            #[cfg(test)]
            DeleteOutcome::Success(projects) => {
                let target = completion.target;
                self.replace_projects(projects, &target.success_status());
            }
            DeleteOutcome::Failure(error) => {
                self.show_toast(error);
            }
        }
    }

    #[cfg(test)]
    pub fn complete_initial_load(&mut self, completion: InitialLoadCompletion) {
        match completion.projects {
            Ok(projects) => {
                // The startup worker loads the first registered agent.
                let agent_id = self.backends[0].id();
                let projects =
                    cowboy::features::session_list::projects_for_agent(projects, &agent_id);
                if let Some(slot) = self.state.project_cache.get_mut(0) {
                    *slot = Some(projects.clone());
                }
                if self.state.selected_agent == 0 {
                    self.replace_projects(projects, "Loaded ~/.claude");
                }
            }
            Err(error) => self.show_toast(error),
        }
    }

    #[cfg(test)]
    pub fn resume_finished(&mut self, status: impl Into<String>) {
        let cwd = self
            .current_project()
            .map(|project| project.cwd.clone())
            .unwrap_or_default();
        self.resume_finished_for(&cwd, status);
    }

    pub fn resume_finished_for(&mut self, cwd: &std::path::Path, status: impl Into<String>) {
        self.state.should_quit = false;
        self.state.pending_resume = None;
        self.state.pending_new_session = None;
        self.state.modal = ModalState::None;
        self.state.info = None;
        self.state.input_buffer.clear();
        self.state.delete_target = None;

        if self.loading.is_some() {
            self.refresh_project_catalog(cwd);
            self.state.status = status.into();
            return;
        }
        match self.selected_backend().load_projects() {
            Ok(projects) => self.replace_projects(projects, &status.into()),
            Err(error) => self.show_toast(error),
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::state::{DeleteTarget, FocusPane, ModalState, ResumeTarget};
    use super::{DeleteCompletion, DeleteOutcome, InitialLoadCompletion, Stetson};
    use crate::application::{AgentBackend, AppResult, ResumeLauncher, SessionRepository};
    use cowboy::domain::{Project, Session, SessionCapabilities, SessionKey};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::cell::RefCell;
    use std::path::{Path, PathBuf};
    use std::rc::Rc;
    use std::sync::Mutex;

    static CURRENT_DIR_LOCK: Mutex<()> = Mutex::new(());

    struct FakeRepository;

    impl SessionRepository for FakeRepository {
        fn load_projects(&self) -> AppResult<Vec<Project>> {
            Ok(Vec::new())
        }
        fn rename_session(&self, _session_id: &SessionKey, _new_title: &str) -> AppResult<()> {
            Ok(())
        }
        fn delete_session(&self, _session_id: &SessionKey) -> AppResult<()> {
            Ok(())
        }
        fn delete_project(&self, _project_cwd: &Path) -> AppResult<()> {
            Ok(())
        }
    }

    #[derive(Clone, Default)]
    struct SpyRepository {
        delete_session_calls: Rc<RefCell<Vec<String>>>,
        delete_project_calls: Rc<RefCell<Vec<PathBuf>>>,
        load_calls: Rc<RefCell<usize>>,
        projects: Vec<Project>,
    }

    impl SessionRepository for SpyRepository {
        fn load_projects(&self) -> AppResult<Vec<Project>> {
            *self.load_calls.borrow_mut() += 1;
            Ok(self.projects.clone())
        }
        fn rename_session(&self, _session_id: &SessionKey, _new_title: &str) -> AppResult<()> {
            Ok(())
        }
        fn delete_session(&self, session_id: &SessionKey) -> AppResult<()> {
            self.delete_session_calls
                .borrow_mut()
                .push(session_id.native_id.clone());
            Ok(())
        }
        fn delete_project(&self, project_cwd: &Path) -> AppResult<()> {
            self.delete_project_calls
                .borrow_mut()
                .push(project_cwd.to_path_buf());
            Ok(())
        }
    }

    /// Test strategy: any repository plus a no-op launcher, registered under a
    /// chosen agent id so multi-agent behavior can be exercised with fakes.
    struct TestBackend<R> {
        id: &'static str,
        repository: R,
    }

    impl<R: SessionRepository> SessionRepository for TestBackend<R> {
        fn load_projects(&self) -> AppResult<Vec<Project>> {
            self.repository.load_projects()
        }
        fn rename_session(&self, key: &SessionKey, new_title: &str) -> AppResult<()> {
            self.repository.rename_session(key, new_title)
        }
        fn delete_session(&self, key: &SessionKey) -> AppResult<()> {
            self.repository.delete_session(key)
        }
        fn delete_project(&self, cwd: &Path) -> AppResult<()> {
            self.repository.delete_project(cwd)
        }
    }

    impl<R: SessionRepository> ResumeLauncher for TestBackend<R> {
        fn resume(&self, _target: &ResumeTarget) -> AppResult<()> {
            Ok(())
        }
        fn launch_new(&self, _cwd: &Path) -> AppResult<()> {
            Ok(())
        }
    }

    impl<R: SessionRepository> AgentBackend for TestBackend<R> {
        fn id(&self) -> cowboy::domain::AgentId {
            cowboy::domain::AgentId::new(self.id).expect("test agent id is valid")
        }
        fn label(&self) -> &str {
            match self.id {
                "claude" => "Claude",
                "codex" => "Codex",
                other => other,
            }
        }
    }

    fn app_with_spy(projects: Vec<Project>, repository: SpyRepository) -> Stetson {
        let mut app = Stetson::new(vec![Box::new(TestBackend {
            id: "claude",
            repository,
        })]);
        app.state.projects = projects;
        app
    }

    fn app_with_projects(projects: Vec<Project>) -> Stetson {
        let mut app = Stetson::new(vec![Box::new(TestBackend {
            id: "claude",
            repository: FakeRepository,
        })]);
        app.state.projects = projects;
        app
    }

    fn project(cwd: &str, sessions: Vec<Session>) -> Project {
        Project {
            cwd: PathBuf::from(cwd),
            sessions,
        }
    }

    fn session(id: &str, cwd: &str) -> Session {
        Session {
            key: SessionKey::claude(id),
            title: format!("Session {id}"),
            cwd: PathBuf::from(cwd),
            git_branch: None,
            created_at: None,
            updated_at: None,
            source_location: Some(format!("/tmp/{id}.jsonl")),
            message_count: Some(0),
            usage: None,
            model: None,
            estimated_cost: None,
            reported_cost: None,
            capabilities: SessionCapabilities::default(),
        }
    }

    fn session_for_agent(agent_id: &str, id: &str, cwd: &str) -> Session {
        let mut session = session(id, cwd);
        session.key = SessionKey::new(
            cowboy::domain::AgentId::new(agent_id).expect("test agent id is valid"),
            id,
        )
        .expect("test session key is valid");
        session
    }

    fn enter_key() -> KeyEvent {
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
    }

    fn key_char(ch: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE)
    }

    fn ctrl_c() -> KeyEvent {
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)
    }

    fn ctrl_d() -> KeyEvent {
        KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL)
    }

    #[test]
    fn completing_initial_load_replaces_loading_state() {
        let mut app = app_with_projects(Vec::new());

        app.complete_initial_load(InitialLoadCompletion {
            projects: Ok(vec![project(
                "/work/repo",
                vec![session("session-1", "/work/repo")],
            )]),
        });

        assert_eq!(app.state.projects.len(), 1);
        assert_eq!(app.state.status, "Loaded ~/.claude");
        assert!(app.state.toast.is_none());
    }

    // ── helpers shared across tests ───────────────────────────────────────

    fn session_target() -> DeleteTarget {
        DeleteTarget::Session {
            title: "Session s1".to_string(),
            key: SessionKey::claude("s1"),
        }
    }

    // ── Focus switching ───────────────────────────────────────────────────

    #[test]
    fn tab_toggles_focus_from_projects_to_sessions() {
        let mut app = app_with_projects(vec![project(
            "/work/repo",
            vec![session("session-1", "/work/repo")],
        )]);
        assert_eq!(app.state.focus, FocusPane::Projects);

        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

        assert_eq!(app.state.focus, FocusPane::Sessions);
    }

    #[test]
    fn tab_toggles_focus_from_sessions_to_projects() {
        let mut app = app_with_projects(vec![project(
            "/work/repo",
            vec![session("session-1", "/work/repo")],
        )]);
        app.state.focus = FocusPane::Sessions;

        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

        assert_eq!(app.state.focus, FocusPane::Projects);
    }

    // ── Navigation ────────────────────────────────────────────────────────

    #[test]
    fn down_arrow_moves_selection_in_projects() {
        let mut app = app_with_projects(vec![
            project("/work/a", vec![session("s1", "/work/a")]),
            project("/work/b", vec![session("s2", "/work/b")]),
        ]);
        assert_eq!(app.state.selected_project, 0);

        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.state.selected_project, 1);
    }

    #[test]
    fn down_arrow_beyond_last_project_clamps() {
        let mut app = app_with_projects(vec![project("/work/a", vec![session("s1", "/work/a")])]);
        app.state.selected_project = 2;

        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));

        assert_eq!(app.state.selected_project, 1);
    }

    #[test]
    fn up_arrow_at_first_project_does_not_underflow() {
        let mut app = app_with_projects(vec![project("/work/a", vec![session("s1", "/work/a")])]);
        assert_eq!(app.state.selected_project, 0);

        app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));

        assert_eq!(app.state.selected_project, 0);
    }

    #[test]
    fn down_arrow_moves_selection_in_sessions() {
        let mut app = app_with_projects(vec![project(
            "/work/repo",
            vec![session("s1", "/work/repo"), session("s2", "/work/repo")],
        )]);
        app.state.focus = FocusPane::Sessions;

        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));

        assert_eq!(app.state.selected_session, 1);
    }

    #[test]
    fn filtered_sessions_returns_all_when_no_search_query() {
        let app = app_with_projects(vec![project(
            "/work/repo",
            vec![session("s1", "/work/repo"), session("s2", "/work/repo")],
        )]);
        assert_eq!(app.filtered_sessions().len(), 2);
    }

    #[test]
    fn sessions_are_scoped_to_the_selected_agent() {
        let app = app_with_projects(vec![project(
            "/work/repo",
            vec![
                session_for_agent("claude", "claude-1", "/work/repo"),
                session_for_agent("codex", "codex-1", "/work/repo"),
            ],
        )]);

        assert_eq!(app.filtered_sessions().len(), 1);
        assert_eq!(app.filtered_sessions()[0].native_id(), "claude-1");
    }

    // ── Agent tab selection ───────────────────────────────────────────────

    struct StubRepository {
        projects: Vec<Project>,
    }

    impl SessionRepository for StubRepository {
        fn load_projects(&self) -> AppResult<Vec<Project>> {
            Ok(self.projects.clone())
        }
        fn rename_session(&self, _: &SessionKey, _: &str) -> AppResult<()> {
            Ok(())
        }
        fn delete_session(&self, _: &SessionKey) -> AppResult<()> {
            Ok(())
        }
        fn delete_project(&self, _: &Path) -> AppResult<()> {
            Ok(())
        }
    }

    fn claude_and_codex_app() -> Stetson {
        Stetson::new(vec![
            Box::new(TestBackend {
                id: "claude",
                repository: StubRepository {
                    projects: vec![project(
                        "/work/repo",
                        vec![session_for_agent("claude", "claude-1", "/work/repo")],
                    )],
                },
            }),
            Box::new(TestBackend {
                id: "codex",
                repository: StubRepository {
                    projects: vec![project(
                        "/work/repo",
                        vec![session_for_agent("codex", "codex-1", "/work/repo")],
                    )],
                },
            }),
        ])
    }

    #[test]
    fn startup_selects_the_first_registered_agent() {
        let app = claude_and_codex_app();

        assert_eq!(app.state.selected_agent, 0);
        assert_eq!(app.state.agent_tabs.len(), 2);
        assert_eq!(app.state.agent_tabs[0].label, "Claude");
        assert_eq!(app.state.agent_tabs[1].label, "Codex");
    }

    #[test]
    fn a_cycles_tabs_in_registration_order_with_wraparound() {
        let mut app = claude_and_codex_app();

        app.handle_key(key_char('a'));
        assert_eq!(app.state.selected_agent, 1);
        assert_eq!(app.state.projects[0].sessions[0].native_id(), "codex-1");

        app.handle_key(key_char('a'));
        assert_eq!(app.state.selected_agent, 0);
        assert_eq!(app.state.projects[0].sessions[0].native_id(), "claude-1");
    }

    #[test]
    fn a_with_one_agent_keeps_selection_and_reports_status() {
        let mut app = app_with_projects(vec![project(
            "/work/repo",
            vec![session("s1", "/work/repo")],
        )]);

        app.handle_key(key_char('a'));

        assert_eq!(app.state.selected_agent, 0);
        assert_eq!(app.state.status, "Agent: Claude");
    }

    #[test]
    fn selection_resets_to_default_on_fresh_start() {
        let mut app = claude_and_codex_app();
        app.handle_key(key_char('a'));
        assert_eq!(app.state.selected_agent, 1);

        let fresh = claude_and_codex_app();
        assert_eq!(fresh.state.selected_agent, 0);
    }

    // ── Lazy per-agent loading ────────────────────────────────────────────

    fn two_spy_agents() -> (Stetson, SpyRepository, SpyRepository) {
        let claude_spy = SpyRepository {
            projects: vec![project(
                "/work/repo",
                vec![session_for_agent("claude", "claude-1", "/work/repo")],
            )],
            ..SpyRepository::default()
        };
        let codex_spy = SpyRepository {
            projects: vec![project(
                "/work/repo",
                vec![session_for_agent("codex", "codex-1", "/work/repo")],
            )],
            ..SpyRepository::default()
        };
        let app = Stetson::new(vec![
            Box::new(TestBackend {
                id: "claude",
                repository: claude_spy.clone(),
            }),
            Box::new(TestBackend {
                id: "codex",
                repository: codex_spy.clone(),
            }),
        ]);
        (app, claude_spy, codex_spy)
    }

    #[test]
    fn first_visit_loads_and_revisits_reuse_the_cache() {
        let (mut app, claude_spy, codex_spy) = two_spy_agents();
        app.complete_initial_load(InitialLoadCompletion {
            projects: Ok(vec![project(
                "/work/repo",
                vec![session("s1", "/work/repo")],
            )]),
        });

        app.handle_key(key_char('a')); // first visit to codex
        assert_eq!(*codex_spy.load_calls.borrow(), 1);
        assert_eq!(app.state.projects[0].sessions[0].native_id(), "codex-1");

        app.handle_key(key_char('a')); // back to claude — cached
        assert_eq!(*claude_spy.load_calls.borrow(), 0);
        assert_eq!(app.state.projects[0].sessions[0].native_id(), "s1");

        app.handle_key(key_char('a')); // codex again — cached
        assert_eq!(*codex_spy.load_calls.borrow(), 1);
    }

    #[test]
    fn complete_delete_updates_only_the_selected_agents_cache() {
        let (mut app, claude_spy, _codex_spy) = two_spy_agents();
        app.complete_initial_load(InitialLoadCompletion {
            projects: Ok(vec![project(
                "/work/repo",
                vec![session("s1", "/work/repo")],
            )]),
        });
        app.handle_key(key_char('a')); // select codex

        app.complete_delete(DeleteCompletion {
            target: DeleteTarget::Session {
                title: "codex-1".to_string(),
                key: SessionKey::new(cowboy::domain::AgentId::new("codex").unwrap(), "codex-1")
                    .unwrap(),
            },
            outcome: DeleteOutcome::Success(vec![project(
                "/new",
                vec![session_for_agent("codex", "codex-2", "/new")],
            )]),
        });
        assert_eq!(app.state.projects[0].cwd, PathBuf::from("/new"));

        app.handle_key(key_char('a')); // back to claude — untouched cache
        assert_eq!(app.state.projects[0].cwd, PathBuf::from("/work/repo"));
        assert_eq!(app.state.projects[0].sessions[0].native_id(), "s1");
        assert_eq!(*claude_spy.load_calls.borrow(), 0);
    }

    #[test]
    fn resume_finished_reloads_only_the_selected_backend() {
        let (mut app, claude_spy, codex_spy) = two_spy_agents();
        app.handle_key(key_char('a')); // select codex (load 1)

        app.resume_finished("done");

        assert_eq!(*codex_spy.load_calls.borrow(), 2);
        assert_eq!(*claude_spy.load_calls.borrow(), 0);
    }

    // ── Project-level agent scoping ───────────────────────────────────────

    #[test]
    fn projects_column_scopes_to_selected_agent_and_drops_foreign_projects() {
        let mut app = app_with_projects(Vec::new());

        app.complete_initial_load(InitialLoadCompletion {
            projects: Ok(vec![
                project(
                    "/work/mixed",
                    vec![
                        session_for_agent("claude", "claude-1", "/work/mixed"),
                        session_for_agent("codex", "codex-1", "/work/mixed"),
                    ],
                ),
                project(
                    "/work/codex-only",
                    vec![session_for_agent("codex", "codex-2", "/work/codex-only")],
                ),
                project("/work/empty", Vec::new()),
            ]),
        });

        assert_eq!(app.state.projects.len(), 1);
        assert_eq!(app.state.projects[0].cwd, PathBuf::from("/work/mixed"));
        assert_eq!(app.state.projects[0].sessions.len(), 1);
        assert_eq!(app.state.projects[0].sessions[0].native_id(), "claude-1");
        assert_eq!(app.project_item_count(), 2); // mixed + Open Here
    }

    #[test]
    fn open_here_is_present_under_an_agent_with_zero_projects() {
        let mut app = Stetson::new(vec![
            Box::new(TestBackend {
                id: "claude",
                repository: StubRepository {
                    projects: vec![project("/work/repo", vec![session("s1", "/work/repo")])],
                },
            }),
            Box::new(TestBackend {
                id: "codex",
                repository: StubRepository {
                    projects: Vec::new(),
                },
            }),
        ]);

        app.handle_key(key_char('a')); // codex has no projects

        assert!(app.state.projects.is_empty());
        assert_eq!(app.project_item_count(), 1);
        assert!(app.is_open_here_selected());
    }

    #[test]
    fn project_row_summary_counts_only_selected_agent_sessions() {
        let mut claude_session = session_for_agent("claude", "claude-1", "/work/mixed");
        claude_session.usage = Some(cowboy::domain::SessionUsage {
            input_tokens: 100,
            output_tokens: 0,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
        });
        let mut codex_session = session_for_agent("codex", "codex-1", "/work/mixed");
        codex_session.usage = Some(cowboy::domain::SessionUsage {
            input_tokens: 999,
            output_tokens: 0,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
        });
        let mut app = app_with_projects(Vec::new());

        app.complete_initial_load(InitialLoadCompletion {
            projects: Ok(vec![project(
                "/work/mixed",
                vec![claude_session, codex_session],
            )]),
        });

        let summary =
            cowboy::features::project_usage::summarize_project_usage(&app.state.projects[0]);
        assert_eq!(summary.session_count, 1);
        assert_eq!(summary.known_total, 100);
    }

    // ── Tab-switch continuity ─────────────────────────────────────────────

    fn continuity_app(codex_projects: Vec<Project>) -> Stetson {
        let mut app = Stetson::new(vec![
            Box::new(TestBackend {
                id: "claude",
                repository: StubRepository {
                    projects: vec![
                        project(
                            "/work/a",
                            vec![session_for_agent("claude", "a1", "/work/a")],
                        ),
                        project(
                            "/work/b",
                            vec![
                                session_for_agent("claude", "b1", "/work/b"),
                                session_for_agent("claude", "b2", "/work/b"),
                            ],
                        ),
                    ],
                },
            }),
            Box::new(TestBackend {
                id: "codex",
                repository: StubRepository {
                    projects: codex_projects,
                },
            }),
        ]);
        // Populate the claude list the way the startup load would, so
        // selection indices in these tests refer to real rows.
        app.switch_to_agent(0);
        app
    }

    #[test]
    fn switching_tabs_keeps_the_same_directory_selected_when_present() {
        let mut app = continuity_app(vec![
            project(
                "/work/b",
                vec![session_for_agent("codex", "bx1", "/work/b")],
            ),
            project("/work/c", vec![session_for_agent("codex", "c1", "/work/c")]),
        ]);
        app.state.selected_project = 1; // /work/b under claude

        app.handle_key(key_char('a'));

        assert_eq!(app.state.selected_agent, 1);
        assert_eq!(app.current_project().unwrap().cwd, PathBuf::from("/work/b"));
    }

    #[test]
    fn switching_tabs_falls_back_to_the_first_project_when_directory_absent() {
        let mut app = continuity_app(vec![project(
            "/work/c",
            vec![session_for_agent("codex", "c1", "/work/c")],
        )]);
        app.state.selected_project = 1; // /work/b — absent under codex

        app.handle_key(key_char('a'));

        assert_eq!(app.state.selected_project, 0);
        assert_eq!(app.current_project().unwrap().cwd, PathBuf::from("/work/c"));
    }

    #[test]
    fn switching_to_an_empty_agent_selects_open_here() {
        let mut app = continuity_app(Vec::new());
        app.state.selected_project = 1;

        app.handle_key(key_char('a'));

        assert!(app.state.projects.is_empty());
        assert!(app.is_open_here_selected());
    }

    #[test]
    fn switching_tabs_resets_session_selection_to_the_first_row() {
        let mut app = continuity_app(vec![project(
            "/work/b",
            vec![
                session_for_agent("codex", "bx1", "/work/b"),
                session_for_agent("codex", "bx2", "/work/b"),
            ],
        )]);
        app.state.selected_project = 1;
        app.state.selected_session = 1; // second claude session

        app.handle_key(key_char('a'));

        assert_eq!(app.state.selected_session, 0);
        assert_eq!(app.current_session().unwrap().native_id(), "bx1");
    }

    #[test]
    fn switching_tabs_preserves_the_search_query() {
        let mut app = continuity_app(vec![project(
            "/work/b",
            vec![session_for_agent("codex", "bx1", "/work/b")],
        )]);
        app.state.search_query = "b".to_string();

        app.handle_key(key_char('a'));

        assert_eq!(app.state.search_query, "b");
        assert_eq!(app.filtered_sessions().len(), 1);
    }

    // ── Quit ──────────────────────────────────────────────────────────────

    #[test]
    fn q_quits_the_application() {
        let mut app = app_with_projects(Vec::new());
        app.handle_key(key_char('q'));
        assert!(app.state.should_quit);
    }

    #[test]
    fn escape_quits_the_application_from_the_normal_view() {
        let mut app = app_with_projects(Vec::new());

        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        assert!(app.state.should_quit);
    }

    // ── Search modal ──────────────────────────────────────────────────────

    #[test]
    fn forward_slash_opens_search_modal() {
        let mut app = app_with_projects(vec![project(
            "/work/repo",
            vec![session("s1", "/work/repo")],
        )]);

        app.handle_key(key_char('/'));

        assert_eq!(app.state.modal, ModalState::Search);
    }

    #[test]
    fn search_modal_escape_returns_to_normal() {
        let mut app = app_with_projects(Vec::new());
        app.state.modal = ModalState::Search;

        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        assert_eq!(app.state.modal, ModalState::None);
    }

    #[test]
    fn search_modal_enter_applies_query() {
        let mut app = app_with_projects(vec![project(
            "/work/repo",
            vec![session("s1", "/work/repo"), session("s2", "/work/repo")],
        )]);
        app.state.modal = ModalState::Search;
        app.state.input_buffer = "s1".to_string();

        app.handle_key(enter_key());

        assert_eq!(app.state.modal, ModalState::None);
        assert_eq!(app.state.search_query, "s1");
        assert_eq!(app.filtered_sessions().len(), 1);
        assert_eq!(app.filtered_sessions()[0].native_id(), "s1");
    }

    #[test]
    fn search_modal_backspace_removes_last_char() {
        let mut app = app_with_projects(Vec::new());
        app.state.modal = ModalState::Search;
        app.state.input_buffer = "abc".to_string();

        app.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));

        assert_eq!(app.state.input_buffer, "ab");
    }

    #[test]
    fn search_modal_typed_char_appends_to_buffer() {
        let mut app = app_with_projects(Vec::new());
        app.state.modal = ModalState::Search;

        app.handle_key(key_char('x'));

        assert_eq!(app.state.input_buffer, "x");
    }

    // ── Rename modal ──────────────────────────────────────────────────────

    #[test]
    fn r_opens_rename_modal() {
        let mut app = app_with_projects(vec![project(
            "/work/repo",
            vec![session("s1", "/work/repo")],
        )]);
        app.state.focus = FocusPane::Sessions;

        app.handle_key(key_char('r'));

        assert_eq!(app.state.modal, ModalState::Rename);
        assert_eq!(app.state.input_buffer, "Session s1");
    }

    #[test]
    fn rename_escape_returns_to_normal() {
        let mut app = app_with_projects(Vec::new());
        app.state.modal = ModalState::Rename;
        app.state.input_buffer = "new name".to_string();

        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        assert_eq!(app.state.modal, ModalState::None);
        assert!(app.state.input_buffer.is_empty());
    }

    #[test]
    fn rename_backspace_removes_last_char() {
        let mut app = app_with_projects(Vec::new());
        app.state.modal = ModalState::Rename;
        app.state.input_buffer = "abc".to_string();

        app.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));

        assert_eq!(app.state.input_buffer, "ab");
    }

    // ── Delete modal ──────────────────────────────────────────────────────

    #[test]
    fn ctrl_d_in_projects_starts_delete_project() {
        let mut app = app_with_projects(vec![project(
            "/work/repo",
            vec![session("s1", "/work/repo")],
        )]);

        app.handle_key(ctrl_d());

        assert_eq!(app.state.modal, ModalState::DeleteConfirm);
        assert!(app.state.delete_target.is_some());
        assert!(matches!(
            app.state.delete_target.as_ref().unwrap(),
            DeleteTarget::Project { .. }
        ));
    }

    #[test]
    fn ctrl_d_in_sessions_starts_delete_session() {
        let mut app = app_with_projects(vec![project(
            "/work/repo",
            vec![session("s1", "/work/repo")],
        )]);
        app.state.focus = FocusPane::Sessions;

        app.handle_key(ctrl_d());

        assert_eq!(app.state.modal, ModalState::DeleteConfirm);
        assert!(app.state.delete_target.is_some());
        assert!(matches!(
            app.state.delete_target.as_ref().unwrap(),
            DeleteTarget::Session { .. }
        ));
    }

    #[test]
    fn delete_modal_escape_returns_to_normal() {
        let mut app = app_with_projects(Vec::new());
        app.state.modal = ModalState::DeleteConfirm;
        app.state.delete_target = Some(DeleteTarget::Session {
            title: "test".to_string(),
            key: SessionKey::claude("s1"),
        });

        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        assert_eq!(app.state.modal, ModalState::None);
        assert!(app.state.delete_target.is_none());
    }

    #[test]
    fn delete_modal_n_cancels_delete() {
        let mut app = app_with_projects(Vec::new());
        app.state.modal = ModalState::DeleteConfirm;
        app.state.delete_target = Some(DeleteTarget::Session {
            title: "test".to_string(),
            key: SessionKey::claude("s1"),
        });

        app.handle_key(key_char('n'));

        assert_eq!(app.state.modal, ModalState::None);
        assert!(app.state.delete_target.is_none());
    }

    #[test]
    fn ctrl_d_twice_confirms_a_session_delete() {
        let spy = SpyRepository::default();
        let mut app = app_with_spy(
            vec![project("/work/repo", vec![session("s1", "/work/repo")])],
            spy.clone(),
        );
        app.state.focus = FocusPane::Sessions;

        app.handle_key(ctrl_d());
        assert_eq!(app.state.modal, ModalState::DeleteConfirm);

        app.handle_key(ctrl_d());

        assert_eq!(app.state.modal, ModalState::None);
        assert!(app.state.delete_target.is_none());
        assert!(matches!(
            app.state.pending_delete,
            Some(DeleteTarget::Session { ref key, .. }) if key.native_id == "s1"
        ));
        assert!(app.state.delete_in_progress.is_some());
        assert!(spy.delete_session_calls.borrow().is_empty());
    }

    #[test]
    fn confirming_session_delete_queues_task_without_touching_repository() {
        let spy = SpyRepository::default();
        let mut app = app_with_spy(
            vec![project("/work/repo", vec![session("s1", "/work/repo")])],
            spy.clone(),
        );
        app.state.focus = FocusPane::Sessions;

        app.handle_key(ctrl_d());
        app.handle_key(enter_key());

        assert_eq!(app.state.modal, ModalState::None);
        assert!(app.state.delete_target.is_none());
        assert!(matches!(
            app.state.pending_delete,
            Some(DeleteTarget::Session { ref key, .. }) if key.native_id == "s1"
        ));
        assert!(app.state.delete_in_progress.is_some());
        assert!(spy.delete_session_calls.borrow().is_empty());
        assert_eq!(*spy.load_calls.borrow(), 0);
    }

    #[test]
    fn take_pending_delete_returns_task_exactly_once() {
        let mut app = app_with_projects(Vec::new());
        app.state.pending_delete = Some(session_target());

        let first = app.take_pending_delete();
        let second = app.take_pending_delete();

        assert_eq!(first, Some(session_target()));
        assert!(second.is_none());
    }

    #[test]
    fn quit_keys_during_deletion_do_not_exit_and_keep_task_active() {
        let mut app = app_with_projects(Vec::new());
        app.state.delete_in_progress = Some(session_target());

        app.handle_key(key_char('q'));
        assert!(!app.state.should_quit);
        assert!(app.state.delete_in_progress.is_some());

        app.handle_key(ctrl_c());
        assert!(!app.state.should_quit);
        assert!(app.state.delete_in_progress.is_some());
    }

    #[test]
    fn other_keys_during_deletion_cannot_start_conflicting_actions() {
        let mut app = app_with_projects(vec![project(
            "/work/repo",
            vec![session("s1", "/work/repo")],
        )]);
        app.state.focus = FocusPane::Projects;
        app.state.delete_in_progress = Some(session_target());

        app.handle_key(ctrl_d());
        assert_eq!(app.state.modal, ModalState::None);

        app.handle_key(key_char('r'));
        assert_eq!(app.state.modal, ModalState::None);

        app.handle_key(enter_key());
        assert!(app.state.pending_new_session.is_none());

        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.state.focus, FocusPane::Projects);

        assert!(app.state.delete_in_progress.is_some());
    }

    #[test]
    fn successful_completion_replaces_list_reports_success_and_permits_exit() {
        let mut app = app_with_projects(vec![project("/old", Vec::new())]);
        app.state.delete_in_progress = Some(DeleteTarget::Session {
            title: "Session s1".to_string(),
            key: SessionKey::claude("s1"),
        });

        app.complete_delete(DeleteCompletion {
            target: DeleteTarget::Session {
                title: "Session s1".to_string(),
                key: SessionKey::claude("s1"),
            },
            outcome: DeleteOutcome::Success(vec![project("/new", vec![session("s2", "/new")])]),
        });

        assert!(app.state.delete_in_progress.is_none());
        assert!(app.state.pending_delete.is_none());
        assert_eq!(app.state.projects.len(), 1);
        assert_eq!(app.state.projects[0].cwd, PathBuf::from("/new"));
        assert_eq!(app.state.status, "Session deleted");
        assert!(app.state.toast.is_none());

        app.handle_key(key_char('q'));
        assert!(app.state.should_quit);
    }

    #[test]
    fn failed_completion_keeps_list_shows_toast_and_permits_exit() {
        let mut app = app_with_projects(vec![project("/old", Vec::new())]);
        app.state.delete_in_progress = Some(DeleteTarget::Session {
            title: "Session s1".to_string(),
            key: SessionKey::claude("s1"),
        });

        app.complete_delete(DeleteCompletion {
            target: DeleteTarget::Session {
                title: "Session s1".to_string(),
                key: SessionKey::claude("s1"),
            },
            outcome: DeleteOutcome::Failure("disk on fire".to_string()),
        });

        assert!(app.state.delete_in_progress.is_none());
        assert!(app.state.pending_delete.is_none());
        assert_eq!(app.state.projects.len(), 1);
        assert_eq!(app.state.projects[0].cwd, PathBuf::from("/old"));
        assert_eq!(
            app.state.toast.as_ref().map(|toast| toast.message.as_str()),
            Some("disk on fire")
        );

        app.handle_key(key_char('q'));
        assert!(app.state.should_quit);
    }

    // ── resume_finished ───────────────────────────────────────────────────

    #[test]
    fn resume_finished_clears_all_pending_state_and_reloads() {
        use crate::application::AppResult;
        use std::path::Path;

        struct ReloadingFakeRepository;
        impl SessionRepository for ReloadingFakeRepository {
            fn load_projects(&self) -> AppResult<Vec<Project>> {
                Ok(Vec::new())
            }
            fn rename_session(&self, _: &SessionKey, _: &str) -> AppResult<()> {
                Ok(())
            }
            fn delete_session(&self, _: &SessionKey) -> AppResult<()> {
                Ok(())
            }
            fn delete_project(&self, _: &Path) -> AppResult<()> {
                Ok(())
            }
        }

        let mut stetson = Stetson::new(vec![Box::new(TestBackend {
            id: "claude",
            repository: ReloadingFakeRepository,
        })]);
        stetson.state.pending_resume = Some(ResumeTarget {
            key: SessionKey::claude("s1"),
            cwd: PathBuf::from("/"),
        });
        stetson.state.pending_new_session = Some(PathBuf::from("/"));
        stetson.state.should_quit = true;

        stetson.resume_finished("done");

        assert!(!stetson.state.should_quit);
        assert!(stetson.state.pending_resume.is_none());
        assert!(stetson.state.pending_new_session.is_none());
        assert_eq!(stetson.state.modal, ModalState::None);
        assert!(stetson.state.info.is_none());
        assert!(stetson.state.input_buffer.is_empty());
        assert!(stetson.state.delete_target.is_none());
    }

    // ── No-session guards ─────────────────────────────────────────────────

    #[test]
    fn resume_without_sessions_shows_status() {
        let mut app = app_with_projects(vec![project("/work/repo", Vec::new())]);
        app.state.focus = FocusPane::Sessions;

        app.handle_key(enter_key());

        assert_eq!(app.state.pending_resume, None);
        assert!(!app.state.should_quit);
    }

    #[test]
    fn delete_without_project_shows_status() {
        let mut app = app_with_projects(Vec::new());

        app.handle_key(ctrl_d());

        assert_eq!(app.state.modal, ModalState::None);
        assert!(app.state.delete_target.is_none());
    }

    // ── Enter / navigation shortcuts ─────────────────────────────────────

    #[test]
    fn enter_in_projects_starts_new_session_for_selected_project() {
        let mut app = app_with_projects(vec![project(
            "/work/repo",
            vec![session("session-1", "/work/repo")],
        )]);
        app.state.focus = FocusPane::Projects;

        app.handle_key(enter_key());

        assert_eq!(
            app.state.pending_new_session,
            Some(PathBuf::from("/work/repo"))
        );
        assert_eq!(app.state.pending_resume, None);
        assert!(app.state.should_quit);
    }

    #[test]
    fn enter_in_sessions_resumes_selected_session() {
        let mut app = app_with_projects(vec![project(
            "/work/repo",
            vec![session("session-1", "/work/repo")],
        )]);
        app.state.focus = FocusPane::Sessions;

        app.handle_key(enter_key());

        assert_eq!(app.state.pending_new_session, None);
        assert_eq!(
            app.state.pending_resume,
            Some(ResumeTarget {
                key: SessionKey::claude("session-1"),
                cwd: PathBuf::from("/work/repo"),
            })
        );
        assert!(app.state.should_quit);
    }

    #[test]
    fn enter_on_open_here_uses_current_directory() {
        let _lock = CURRENT_DIR_LOCK.lock().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let current_dir = std::env::current_dir().unwrap();
        let mut app = app_with_projects(vec![project(
            "/work/repo",
            vec![session("session-1", "/work/repo")],
        )]);
        app.state.focus = FocusPane::Projects;
        app.state.selected_project = 1;
        std::env::set_current_dir(temp.path()).unwrap();
        let expected = std::env::current_dir().unwrap();

        app.handle_key(enter_key());

        std::env::set_current_dir(current_dir).unwrap();

        assert_eq!(app.state.pending_new_session, Some(expected));
    }

    #[test]
    fn projects_list_has_open_here_after_real_projects() {
        let mut app = app_with_projects(vec![
            project("/work/a", vec![session("s1", "/work/a")]),
            project("/work/b", vec![session("s2", "/work/b")]),
        ]);
        assert_eq!(app.project_item_count(), 3);

        app.state.selected_project = 2;
        assert!(app.is_open_here_selected());
    }

    #[test]
    fn bracket_navigation_cycles_only_projects_and_sessions() {
        let mut app = app_with_projects(Vec::new());

        for (key, expected) in [
            (']', FocusPane::Sessions),
            (']', FocusPane::Projects),
            ('[', FocusPane::Sessions),
            ('[', FocusPane::Projects),
        ] {
            app.handle_key(key_char(key));
            assert_eq!(app.state.focus, expected);
        }
    }

    #[test]
    fn former_profile_shortcuts_leave_project_actions_available() {
        let mut app = app_with_projects(vec![project(
            "/work/repo",
            vec![session("s1", "/work/repo")],
        )]);

        for ch in ['e', 'u', 'p'] {
            let status = app.state.status.clone();
            app.handle_key(key_char(ch));
            assert_eq!(app.state.modal, ModalState::None);
            assert_eq!(app.state.status, status, "former shortcut: {ch}");
            assert!(!app.state.should_quit);
        }
        app.handle_key(enter_key());
        assert_eq!(
            app.state.pending_new_session,
            Some(PathBuf::from("/work/repo"))
        );
    }
}
