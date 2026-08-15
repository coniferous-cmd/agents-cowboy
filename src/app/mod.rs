mod project_actions;
mod session_actions;

// ── Internal modules ─────────────────────────────────────────────────────────

mod state {
    use cowboy::claude_env::{ClaudeProfile, ClaudeSettingsSnapshot};
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

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum MainTab {
        Projects,
        Profiles,
    }

    /// Limits the sessions shown in the Projects tab to one Agent backend.
    #[derive(Debug, Clone, PartialEq, Eq, Default)]
    pub enum AgentFilter {
        #[default]
        All,
        Agent(AgentId),
    }

    impl AgentFilter {
        pub fn label(&self) -> &str {
            match self {
                Self::All => "All",
                Self::Agent(agent_id) => match agent_id.as_str() {
                    "claude" => "Claude",
                    "codex" => "Codex",
                    value => value,
                },
            }
        }

        pub fn agent_id(&self) -> Option<&AgentId> {
            match self {
                Self::All => None,
                Self::Agent(agent_id) => Some(agent_id),
            }
        }
    }

    pub fn agent_filters(projects: &[cowboy::domain::Project]) -> Vec<AgentFilter> {
        let mut agent_ids: Vec<_> = projects
            .iter()
            .flat_map(|project| project.sessions.iter())
            .map(|session| session.key.agent_id.clone())
            .collect();
        agent_ids.sort();
        agent_ids.dedup();

        std::iter::once(AgentFilter::All)
            .chain(agent_ids.into_iter().map(AgentFilter::Agent))
            .collect()
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum ModalState {
        None,
        Search,
        Rename,
        NewProfile,
        DeleteConfirm,
        Info,
        EditProfile { profile_id: i64 },
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
        /// Deletion succeeded; carries the freshly reloaded project list.
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
        Profile {
            agent_id: AgentId,
            name: String,
        },
    }

    impl DeleteTarget {
        /// Internal kind tag — not exposed outside this module.
        pub(crate) fn kind(&self) -> Kind {
            match self {
                Self::Project { .. } => Kind::Project,
                Self::Session { .. } => Kind::Session,
                Self::Profile { .. } => Kind::Profile,
            }
        }

        pub fn name(&self) -> &str {
            match self {
                Self::Project { name, .. } => name,
                Self::Session { title, .. } => title,
                Self::Profile { name, .. } => name,
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
        Profile,
    }

    impl Kind {
        pub(crate) fn label(self) -> &'static str {
            match self {
                Self::Project => "Project",
                Self::Session => "Session",
                Self::Profile => "Profile",
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
        pub profiles: Vec<ClaudeProfile>,
        pub snapshots: Vec<ClaudeSettingsSnapshot>,
        pub active_profile_name: Option<String>,
        pub main_tab: MainTab,
        pub agent_filter: AgentFilter,
        pub profile_cursor: usize,
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
        pub pending_profile_edit: Option<String>,
        /// Set after an external editor temporarily owns the terminal. The
        /// ratatui `Terminal` must then clear its cached previous frame before
        /// drawing again, or unchanged cells are not repainted.
        pub terminal_refresh_pending: bool,
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
                profiles: Vec::new(),
                snapshots: Vec::new(),
                active_profile_name: None,
                main_tab: MainTab::Projects,
                agent_filter: AgentFilter::All,
                profile_cursor: 0,
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
                pending_profile_edit: None,
                terminal_refresh_pending: false,
                info: None,
                delete_target: None,
                pending_delete: None,
                delete_in_progress: None,
                toast: None,
                theme: ThemePalette::default(),
            }
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
    use crate::application::{
        project_name_owned, project_working_dir, session_key, session_title, ProfileRepository,
        ResumeLauncher, SessionRepository,
    };

    use super::state::{
        agent_filters, AgentFilter, DeleteTarget, FocusPane, OPEN_HERE_PROJECT_LABEL,
    };
    use super::Stetson;
    use super::{MainTab, ModalState};

    impl<R, L, P> Stetson<R, L, P>
    where
        R: SessionRepository,
        L: ResumeLauncher,
        P: ProfileRepository,
    {
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

        pub fn cycle_agent_filter(&mut self) {
            let filters = agent_filters(&self.state.projects);
            let current = filters
                .iter()
                .position(|filter| filter == &self.state.agent_filter)
                .unwrap_or(0);
            self.state.agent_filter = filters
                .get(current + 1)
                .cloned()
                .unwrap_or(AgentFilter::All);
            self.state.selected_session = 0;
            self.clamp_selections();
            self.state.status = format!("Agent: {}", self.state.agent_filter.label());
        }

        pub fn switch_tab(&mut self, delta: isize) {
            let index = match self.state.main_tab {
                MainTab::Projects => 0isize,
                MainTab::Profiles => 1,
            };
            self.state.main_tab = match (index + delta).rem_euclid(2) {
                0 => MainTab::Projects,
                _ => MainTab::Profiles,
            };
            match self.state.main_tab {
                MainTab::Projects => self.state.focus = FocusPane::Projects,
                MainTab::Profiles => {}
            }
            self.state.status = format!("Tab: {:?}", self.state.main_tab);
        }

        pub fn move_profile_cursor(&mut self, delta: isize) {
            let len = self.state.profiles.len() + self.state.snapshots.len();
            self.state.profile_cursor = offset_index(self.state.profile_cursor, delta, len);
        }

        pub fn activate_profile_row(&mut self) {
            if self.state.profile_cursor < self.state.profiles.len() {
                let Some(name) = self
                    .state
                    .profiles
                    .get(self.state.profile_cursor)
                    .map(|profile| profile.name.clone())
                else {
                    return;
                };
                match self.application.activate_profile(&name) {
                    Ok(()) => self.reload_profiles(format!("Activated profile: {name}")),
                    Err(error) => self.show_toast(error),
                }
                return;
            }

            let snapshot_index = self
                .state
                .profile_cursor
                .saturating_sub(self.state.profiles.len());
            let Some(snapshot_id) = self.state.snapshots.get(snapshot_index).map(|item| item.id)
            else {
                return;
            };
            match self.application.activate_snapshot(snapshot_id) {
                Ok(()) => self.reload_profiles(format!("Activated snapshot: {snapshot_id}")),
                Err(error) => self.show_toast(error),
            }
        }

        /// Enter the new-profile name modal. The name is captured here so it
        /// stays out of the raw settings JSON edited in `$EDITOR`.
        pub fn begin_new_profile(&mut self) {
            self.state.modal = ModalState::NewProfile;
            self.state.input_buffer.clear();
            self.state.status = "New profile: enter name".to_string();
        }

        pub fn begin_profile_delete(&mut self) {
            let Some(profile) = self.state.profiles.get(self.state.profile_cursor) else {
                self.show_toast("No profile selected");
                return;
            };

            let target = DeleteTarget::Profile {
                agent_id: cowboy::domain::AgentId::new("claude")
                    .expect("built-in agent id is valid"),
                name: profile.name.clone(),
            };
            self.state.delete_target = Some(target.clone());
            self.state.modal = ModalState::DeleteConfirm;
            self.state.status = target.prompt_status();
        }

        pub fn begin_profile_edit(&mut self) {
            // Only works on profile list, not snapshot list
            if self.state.profile_cursor >= self.state.profiles.len() {
                return;
            }

            let Some(profile) = self.state.profiles.get(self.state.profile_cursor) else {
                self.show_toast("No profile selected");
                return;
            };

            let profile_id = profile.id;
            let profile_name = profile.name.clone();
            let settings_json = profile.settings_json.clone();

            // Create snapshot before editing
            if let Err(error) = self.application.create_snapshot(profile_id, &settings_json) {
                self.show_toast(format!("Failed to create snapshot: {error}"));
                return;
            }

            // Set modal state
            self.state.modal = ModalState::EditProfile { profile_id };
            self.state.status = format!("Editing profile: {profile_name}");

            // Hand the terminal over to the editor with full state
            // management — the helper snapshots line discipline, runs
            // vim, then resets vim's residual escape-sequence state
            // before re-entering the TUI. Without the reset, vim's
            // leftover character-set switches, SGR attributes, and
            // mouse-tracking modes make the next ratatui draw render
            // garbled / flickering frames.
            let outcome = cowboy::features::profile_editor::edit_profile_json_with_terminal_reset(
                &settings_json,
            );
            self.state.terminal_refresh_pending = true;

            // Process outcome
            match outcome {
                Ok(cowboy::features::profile_editor::EditOutcome::Saved(new_json)) => {
                    // Update profile and activate it
                    if let Err(error) = self
                        .application
                        .update_profile_json(&profile_name, &new_json)
                    {
                        self.show_toast(format!("Failed to update profile: {error}"));
                    } else if let Err(error) = self.application.activate_profile(&profile_name) {
                        self.show_toast(format!("Failed to activate profile: {error}"));
                    } else {
                        self.reload_profiles(format!("Profile '{profile_name}' updated"));
                    }
                }
                Ok(cowboy::features::profile_editor::EditOutcome::NoEditorConfigured) => {
                    self.show_toast("EDITOR environment variable not set".to_string());
                }
                Ok(cowboy::features::profile_editor::EditOutcome::EditorExitedWithError(msg)) => {
                    self.show_toast(msg);
                }
                Ok(cowboy::features::profile_editor::EditOutcome::ValidationError {
                    error,
                    ..
                }) => {
                    self.show_toast(format!("Invalid JSON: {error}"));
                }
                Err(error) => {
                    self.show_toast(format!("Editor error: {error}"));
                }
            }

            self.state.modal = ModalState::None;
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

            self.state.projects = projects;
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
    use crate::application::{session_key, ProfileRepository, ResumeLauncher, SessionRepository};
    use cowboy::claude_env::validate_profile_name;
    use cowboy::domain::validate_rename_title;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::state::{DeleteTarget, ModalState};
    use super::Stetson;

    impl<R: SessionRepository, L: ResumeLauncher, P: ProfileRepository> Stetson<R, L, P> {
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
                ModalState::NewProfile => self.handle_new_profile_key(key),
                ModalState::DeleteConfirm => self.handle_delete_key(key),
                ModalState::Info => self.handle_info_key(key),
                ModalState::EditProfile { .. } => {} // Editor owns the terminal
                ModalState::None => self.handle_normal_key(key),
            }
        }

        fn handle_normal_key(&mut self, key: KeyEvent) {
            if matches!(key.code, KeyCode::Char('[')) {
                self.switch_tab(-1);
                return;
            }
            if matches!(key.code, KeyCode::Char(']')) {
                self.switch_tab(1);
                return;
            }
            if self.state.main_tab == super::MainTab::Profiles {
                match key.code {
                    KeyCode::Esc | KeyCode::Char('q') => self.state.should_quit = true,
                    KeyCode::Down => self.move_profile_cursor(1),
                    KeyCode::Up => self.move_profile_cursor(-1),
                    KeyCode::Enter => self.activate_profile_row(),
                    KeyCode::Char('n') => self.begin_new_profile(),
                    KeyCode::Char('e') => self.begin_profile_edit(),
                    KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        self.begin_profile_delete()
                    }
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        self.state.should_quit = true;
                    }
                    _ => {}
                }
                return;
            }
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => self.state.should_quit = true,
                KeyCode::Tab | KeyCode::Right | KeyCode::Left => self.toggle_focus(),
                KeyCode::Down => self.move_selection(1),
                KeyCode::Up => self.move_selection(-1),
                KeyCode::Char('a') => self.cycle_agent_filter(),
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

                    match self.application.rename_session(&session_key, &new_title) {
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

        fn handle_new_profile_key(&mut self, key: KeyEvent) {
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => {
                    self.state.modal = ModalState::None;
                    self.state.input_buffer.clear();
                    self.state.status = "New profile canceled".to_string();
                }
                KeyCode::Enter => {
                    let normalized = match validate_profile_name(&self.state.input_buffer) {
                        Ok(name) => name,
                        Err(error) => {
                            self.show_toast(error.to_string());
                            return;
                        }
                    };
                    if self
                        .state
                        .profiles
                        .iter()
                        .any(|profile| profile.name == normalized)
                    {
                        self.show_toast(format!("Profile already exists: {normalized}"));
                        return;
                    }
                    self.state.pending_profile_edit = Some(normalized);
                    self.state.input_buffer.clear();
                    self.state.modal = ModalState::None;
                    self.state.status = "Opening editor…".to_string();
                    self.state.should_quit = true;
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
            if let DeleteTarget::Profile { name, .. } = &target {
                match self.application.delete_profile(name) {
                    Ok(()) => self.reload_profiles(target.success_status()),
                    Err(error) => self.show_toast(error),
                }
                return;
            }
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

pub use state::{
    agent_filters, AppState, DeleteCompletion, DeleteOutcome, DeleteTarget, FocusPane, MainTab,
    ModalState, ResumeTarget, Toast,
};

// ── Stetson main impl ────────────────────────────────────────────────────────

use crate::application::{
    AppResult, NoProfileRepository, ProfileRepository, ResumeLauncher, SessionRepository,
    StetsonApplication,
};
use cowboy::features::profile_editor::EditOutcome;
use std::path::PathBuf;

pub type LoadedProfiles = (
    Vec<cowboy::claude_env::ClaudeProfile>,
    Vec<cowboy::claude_env::ClaudeSettingsSnapshot>,
    Option<String>,
);

#[derive(Debug)]
pub struct InitialLoadCompletion {
    pub projects: AppResult<Vec<cowboy::domain::Project>>,
    pub profiles: AppResult<LoadedProfiles>,
}

pub struct Stetson<R, L, P = NoProfileRepository> {
    application: StetsonApplication<R, L, P>,
    state: AppState,
}

impl<R, L, P> Stetson<R, L, P>
where
    R: SessionRepository,
    L: ResumeLauncher,
    P: ProfileRepository,
{
    pub fn new(application: StetsonApplication<R, L, P>) -> Self {
        Self {
            application,
            state: AppState::default(),
        }
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

    /// Take the name of a profile whose `$EDITOR` session is waiting to run.
    /// Returns `Some` at most once per queued creation.
    pub fn take_pending_profile_edit(&mut self) -> Option<String> {
        self.state.pending_profile_edit.take()
    }

    /// Consume the request to invalidate ratatui's cached previous frame after
    /// an external editor has modified the terminal contents.
    pub fn take_terminal_refresh_request(&mut self) -> bool {
        std::mem::take(&mut self.state.terminal_refresh_pending)
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
            DeleteOutcome::Success(projects) => {
                let target = completion.target;
                self.replace_projects(projects, &target.success_status());
            }
            DeleteOutcome::Failure(error) => {
                self.show_toast(error);
            }
        }
    }

    pub fn complete_initial_load(&mut self, completion: InitialLoadCompletion) {
        let projects_loaded = match completion.projects {
            Ok(projects) => {
                self.replace_projects(projects, "Loaded ~/.claude");
                true
            }
            Err(error) => {
                self.show_toast(error);
                false
            }
        };

        let profiles_loaded = match completion.profiles {
            Ok((profiles, snapshots, active_profile_name)) => {
                self.state.profiles = profiles;
                self.state.snapshots = snapshots;
                self.state.active_profile_name = active_profile_name;
                true
            }
            Err(error) => {
                self.show_toast(error);
                false
            }
        };

        if projects_loaded && profiles_loaded && !self.state.projects.is_empty() {
            self.state.status = "Loaded ~/.claude".to_string();
        }
    }

    /// Run the `$EDITOR` session for a new profile, then persist the result into
    /// SQLite. Must be called *after* the TUI has left raw mode (mirrors how
    /// main.rs runs the blocking launch between `leave_tui` / `enter_tui`).
    pub(crate) fn run_new_profile_editor(&mut self, name: &str) -> AppResult<String> {
        // main.rs has already left raw mode and the alternate screen for this
        // flow, so the low-level helper must not perform a second terminal
        // leave/re-enter cycle.
        match cowboy::features::profile_editor::edit_profile_json("{}")? {
            EditOutcome::Saved(json) => {
                self.application.create_profile(name)?;
                if let Err(error) = self.application.update_profile_json(name, &json) {
                    let rollback = self.application.delete_profile(name);
                    return Err(match rollback {
                        Ok(()) => error,
                        Err(rollback_error) => format!(
                            "Failed to save profile '{name}': {error}; rollback also failed: {rollback_error}"
                        ),
                    });
                }
                Ok(format!("Created profile: {name}"))
            }
            EditOutcome::NoEditorConfigured => {
                Err("$EDITOR is not set or empty; profile not created".into())
            }
            EditOutcome::EditorExitedWithError(message) => Err(message),
            EditOutcome::ValidationError { error, .. } => {
                Err(format!("Invalid profile JSON: {error}"))
            }
        }
    }

    /// Apply the outcome of a new-profile editor session: refresh the profile
    /// list and surface either a status line (success) or a toast (failure).
    pub fn profile_edit_finished(&mut self, outcome: AppResult<String>) {
        self.state.should_quit = false;
        self.state.pending_profile_edit = None;
        self.state.modal = ModalState::None;
        self.state.input_buffer.clear();
        self.reload_profiles(match &outcome {
            Ok(success) => success.clone(),
            Err(_) => "Profile edit failed".to_string(),
        });
        if let Err(error) = outcome {
            self.show_toast(error);
        }
    }

    pub fn resume_finished(&mut self, status: impl Into<String>) {
        self.state.should_quit = false;
        self.state.pending_resume = None;
        self.state.pending_new_session = None;
        self.state.pending_profile_edit = None;
        self.state.modal = ModalState::None;
        self.state.info = None;
        self.state.input_buffer.clear();
        self.state.delete_target = None;

        match self.application.load_projects() {
            Ok(projects) => self.replace_projects(projects, &status.into()),
            Err(error) => self.show_toast(error),
        }
    }

    fn reload_profiles(&mut self, status: impl Into<String>) {
        match self.application.load_profile_data() {
            Ok((profiles, snapshots, active_profile_name)) => {
                self.state.profiles = profiles;
                self.state.snapshots = snapshots;
                self.state.active_profile_name = active_profile_name;
                let len = self.state.profiles.len() + self.state.snapshots.len();
                self.state.profile_cursor = self.state.profile_cursor.min(len.saturating_sub(1));
                self.state.status = status.into();
            }
            Err(error) => self.show_toast(error),
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::state::{AgentFilter, DeleteTarget, FocusPane, MainTab, ModalState, ResumeTarget};
    use super::{DeleteCompletion, DeleteOutcome, InitialLoadCompletion, Stetson};
    use crate::application::{
        AppResult, ProfileRepository, ResumeLauncher, SessionRepository, StetsonApplication,
    };
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
    }

    impl SessionRepository for SpyRepository {
        fn load_projects(&self) -> AppResult<Vec<Project>> {
            *self.load_calls.borrow_mut() += 1;
            Ok(Vec::new())
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

    fn app_with_spy(
        projects: Vec<Project>,
        repository: SpyRepository,
    ) -> Stetson<SpyRepository, FakeLauncher> {
        let application = StetsonApplication::new(repository, FakeLauncher);
        let mut app = Stetson::new(application);
        app.state.projects = projects;
        app
    }

    #[derive(Clone)]
    struct FakeLauncher;

    impl ResumeLauncher for FakeLauncher {
        fn resume(&self, _target: &ResumeTarget) -> AppResult<()> {
            Ok(())
        }
        fn launch_new(&self, _cwd: &Path) -> AppResult<()> {
            Ok(())
        }
    }

    #[derive(Clone)]
    struct SpyProfileRepository {
        profiles: Vec<cowboy::claude_env::ClaudeProfile>,
        activated_profiles: Rc<RefCell<Vec<String>>>,
    }

    impl ProfileRepository for SpyProfileRepository {
        fn list_profiles(&self) -> AppResult<Vec<cowboy::claude_env::ClaudeProfile>> {
            Ok(self.profiles.clone())
        }

        fn list_snapshots(&self) -> AppResult<Vec<cowboy::claude_env::ClaudeSettingsSnapshot>> {
            Ok(Vec::new())
        }

        fn active_profile_name(&self) -> AppResult<Option<String>> {
            Ok(None)
        }

        fn activate_profile(&self, name: &str) -> AppResult<()> {
            self.activated_profiles.borrow_mut().push(name.to_string());
            Ok(())
        }

        fn activate_snapshot(&self, _id: i64) -> AppResult<()> {
            Ok(())
        }

        fn create_profile(&self, _name: &str) -> AppResult<cowboy::claude_env::ClaudeProfile> {
            Err("not used in this test".to_string())
        }

        fn create_snapshot(&self, _profile_id: i64, _settings_json: &str) -> AppResult<()> {
            Ok(())
        }

        fn update_profile_json(
            &self,
            _name: &str,
            _settings_json: &str,
        ) -> AppResult<cowboy::claude_env::ClaudeProfile> {
            Err("not used in this test".to_string())
        }

        fn delete_profile(&self, _name: &str) -> AppResult<()> {
            Err("not used in this test".to_string())
        }
    }

    fn app_with_profile_spy(
        profile: cowboy::claude_env::ClaudeProfile,
        repository: SpyProfileRepository,
    ) -> Stetson<FakeRepository, FakeLauncher, SpyProfileRepository> {
        let application =
            StetsonApplication::with_profiles(FakeRepository, FakeLauncher, repository);
        let mut app = Stetson::new(application);
        app.state.main_tab = MainTab::Profiles;
        app.state.profiles = vec![profile];
        app
    }

    fn app_with_projects(projects: Vec<Project>) -> Stetson<FakeRepository, FakeLauncher> {
        let application = StetsonApplication::new(FakeRepository, FakeLauncher);
        let mut app = Stetson::new(application);
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
            profiles: Ok((Vec::new(), Vec::new(), None)),
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
    fn agent_tab_cycles_and_filters_sessions_in_the_current_project() {
        let mut app = app_with_projects(vec![project(
            "/work/repo",
            vec![
                session_for_agent("claude", "claude-1", "/work/repo"),
                session_for_agent("codex", "codex-1", "/work/repo"),
            ],
        )]);

        app.handle_key(key_char('a'));
        assert_eq!(
            app.state.agent_filter,
            AgentFilter::Agent(cowboy::domain::AgentId::new("claude").unwrap())
        );
        assert_eq!(app.filtered_sessions()[0].native_id(), "claude-1");

        app.handle_key(key_char('a'));
        assert_eq!(
            app.state.agent_filter,
            AgentFilter::Agent(cowboy::domain::AgentId::new("codex").unwrap())
        );
        assert_eq!(app.filtered_sessions()[0].native_id(), "codex-1");

        app.handle_key(key_char('a'));
        assert_eq!(app.state.agent_filter, AgentFilter::All);
        assert_eq!(app.filtered_sessions().len(), 2);
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
            outcome: DeleteOutcome::Success(vec![project("/new", Vec::new())]),
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

        let app = StetsonApplication::new(ReloadingFakeRepository, FakeLauncher);
        let mut stetson = Stetson::new(app);
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
    fn bracket_keys_cycle_tabs_and_align_browser_focus() {
        let mut app = app_with_projects(Vec::new());

        app.handle_key(key_char(']'));
        assert_eq!(app.state.main_tab, MainTab::Profiles);

        app.handle_key(key_char(']'));
        assert_eq!(app.state.main_tab, MainTab::Projects);
        assert_eq!(app.state.focus, FocusPane::Projects);

        app.handle_key(key_char('['));
        assert_eq!(app.state.main_tab, MainTab::Profiles);
    }

    #[test]
    fn profile_tab_ignores_pane_keys_and_space_has_no_effect() {
        use cowboy::claude_env::{ClaudeProfile, ClaudeSettingsSnapshot};

        let mut app = app_with_projects(Vec::new());
        app.state.main_tab = MainTab::Profiles;
        app.state.profiles = vec![ClaudeProfile {
            id: 7,
            name: "work".to_string(),
            settings_json: "{}".to_string(),
            updated_at: "2026-08-08 00:00:00".to_string(),
        }];
        app.state.snapshots = vec![ClaudeSettingsSnapshot {
            id: 9,
            captured_at: "2026-08-08 00:00:00".to_string(),
            source: None,
            settings_json: "{}".to_string(),
        }];
        let focus = app.state.focus;

        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        assert_eq!(app.state.focus, focus);

        app.handle_key(key_char(' '));
        assert_eq!(app.state.profile_cursor, 0);
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        app.handle_key(key_char(' '));
        assert_eq!(app.state.profile_cursor, 1);
    }

    #[test]
    fn ctrl_d_in_profiles_starts_delete_profile() {
        use cowboy::claude_env::ClaudeProfile;

        let mut app = app_with_projects(Vec::new());
        app.state.main_tab = MainTab::Profiles;
        app.state.profiles = vec![ClaudeProfile {
            id: 7,
            name: "work".to_string(),
            settings_json: "{}".to_string(),
            updated_at: "2026-08-08 00:00:00".to_string(),
        }];

        app.handle_key(ctrl_d());

        assert_eq!(app.state.modal, ModalState::DeleteConfirm);
        assert!(matches!(
            app.state.delete_target.as_ref(),
            Some(DeleteTarget::Profile { name, .. }) if name == "work"
        ));
    }

    #[test]
    fn enter_in_profiles_activates_the_focused_profile() {
        use cowboy::claude_env::ClaudeProfile;

        let profile = ClaudeProfile {
            id: 7,
            name: "work".to_string(),
            settings_json: "{}".to_string(),
            updated_at: "2026-08-08 00:00:00".to_string(),
        };
        let activated_profiles = Rc::new(RefCell::new(Vec::new()));
        let repository = SpyProfileRepository {
            profiles: vec![profile.clone()],
            activated_profiles: activated_profiles.clone(),
        };
        let mut app = app_with_profile_spy(profile, repository);

        app.handle_key(enter_key());

        assert_eq!(*activated_profiles.borrow(), vec!["work"]);
        assert_eq!(app.state.status, "Activated profile: work");
    }

    #[test]
    fn terminal_refresh_request_is_consumed_once() {
        let mut app = app_with_projects(Vec::new());
        app.state.terminal_refresh_pending = true;

        assert!(app.take_terminal_refresh_request());
        assert!(!app.take_terminal_refresh_request());
    }
}
