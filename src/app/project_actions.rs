use std::path::PathBuf;

use super::state::FocusPane;
use super::Stetson;
use crate::application::{ProfileRepository, ResumeLauncher, SessionRepository};

impl<R: SessionRepository, L: ResumeLauncher, P: ProfileRepository> Stetson<R, L, P> {
    pub fn activate_selection(&mut self) {
        match self.state.focus {
            FocusPane::Projects if self.is_open_here_selected() => self.new_session_here(),
            FocusPane::Projects => self.new_session(),
            FocusPane::Sessions => self.resume(),
        }
    }

    pub fn new_session_here(&mut self) {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        self.queue_new_session(cwd);
    }
}
