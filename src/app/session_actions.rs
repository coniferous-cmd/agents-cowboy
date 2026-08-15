use cowboy::domain::{Project, Session};
use cowboy::features::session_list::filter_project_sessions_for_agent;
use std::path::PathBuf;

use super::state::{
    build_session_info, session_resume_target, DeleteTarget, FocusPane, ModalState,
};
use super::Stetson;
use crate::application::{
    project_name_owned, session_key, session_title, ProfileRepository, ResumeLauncher,
    SessionRepository,
};

impl<R: SessionRepository, L: ResumeLauncher, P: ProfileRepository> Stetson<R, L, P> {
    pub fn filtered_sessions(&self) -> Vec<&Session> {
        filter_project_sessions_for_agent(
            self.current_project(),
            &self.state.search_query,
            self.state.agent_filter.agent_id(),
        )
    }

    pub fn current_project(&self) -> Option<&Project> {
        self.state.projects.get(self.state.selected_project)
    }

    pub fn current_session(&self) -> Option<&Session> {
        let selected = self.state.selected_session;
        self.filtered_sessions().get(selected).copied()
    }

    pub fn open_info(&mut self) {
        let Some(project_index) = self
            .state
            .projects
            .get(self.state.selected_project)
            .map(|_| self.state.selected_project)
        else {
            self.show_toast("No project selected");
            return;
        };
        let filtered = self.filtered_sessions();
        let Some(session_index) = filtered
            .get(self.state.selected_session)
            .map(|_| self.state.selected_session)
        else {
            self.show_toast("No session selected");
            return;
        };

        let project = &self.state.projects[project_index];
        let session = filtered[session_index];
        self.state.info = Some(build_session_info(project, session));
        self.state.modal = ModalState::Info;
        self.state.status = "Viewing session info".to_string();
    }

    pub fn begin_rename(&mut self) {
        let Some(title) = self.current_session().map(|s| s.title.clone()) else {
            self.show_toast("No session selected");
            return;
        };
        self.state.modal = ModalState::Rename;
        self.state.input_buffer = title;
        self.state.status = "Rename session".to_string();
    }

    pub fn begin_delete(&mut self) {
        let target = match self.state.focus {
            FocusPane::Projects => {
                let Some(project) = self.current_project() else {
                    self.show_toast("No project selected");
                    return;
                };
                DeleteTarget::Project {
                    agent_id: project
                        .sessions
                        .first()
                        .map(|session| session.key.agent_id.clone())
                        .unwrap_or_else(|| cowboy::domain::AgentId::new("claude").unwrap()),
                    name: project_name_owned(project),
                    cwd: project.cwd.clone(),
                }
            }
            FocusPane::Sessions => {
                let Some(session) = self.current_session() else {
                    self.show_toast("No session selected");
                    return;
                };
                DeleteTarget::Session {
                    title: session_title(session).to_string(),
                    key: session_key(session).clone(),
                }
            }
        };

        self.state.delete_target = Some(target.clone());
        self.state.modal = ModalState::DeleteConfirm;
        self.state.status = target.prompt_status();
    }

    pub fn open_search(&mut self) {
        self.state.modal = ModalState::Search;
        self.state.input_buffer = self.state.search_query.clone();
        self.state.status = "Search current project".to_string();
    }

    pub(super) fn resume(&mut self) {
        let Some(session) = self.current_session() else {
            self.show_toast("No session selected");
            return;
        };
        let target = session_resume_target(session);
        self.state.pending_resume = Some(target.clone());
        self.state.status = format!("Ready to resume {}", target.key.native_id);
        self.state.should_quit = true;
    }

    pub(super) fn new_session(&mut self) {
        let cwd = self
            .current_project()
            .map(|p| p.cwd.clone())
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        self.queue_new_session(cwd);
    }

    pub(super) fn queue_new_session(&mut self, cwd: PathBuf) {
        self.state.pending_new_session = Some(cwd.clone());
        self.state.status = format!("Ready to create new session in {}", cwd.display());
        self.state.should_quit = true;
    }
}
