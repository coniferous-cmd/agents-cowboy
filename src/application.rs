use std::path::{Path, PathBuf};
use std::process::Command;

use cowboy::claude_env::ClaudeEnvStore;
use cowboy::domain::{Project, Session, SessionKey};
use cowboy::infrastructure::ClaudeProjectsStore;

use crate::app::ResumeTarget;

#[cfg(test)]
#[path = "launcher_tests.rs"]
mod launcher_tests;

pub type AppResult<T> = Result<T, String>;

pub trait SessionRepository {
    fn load_projects(&self) -> AppResult<Vec<Project>>;
    fn rename_session(&self, key: &SessionKey, new_title: &str) -> AppResult<()>;
    fn delete_session(&self, key: &SessionKey) -> AppResult<()>;
    fn delete_project(&self, project_cwd: &Path) -> AppResult<()>;
}

pub trait ResumeLauncher {
    fn resume(&self, target: &ResumeTarget) -> AppResult<()>;
    fn launch_new(&self, cwd: &Path) -> AppResult<()>;
}

pub struct StetsonApplication<R, L> {
    repository: R,
    _launcher: L,
}

impl<R: SessionRepository, L: ResumeLauncher> StetsonApplication<R, L> {
    pub fn new(repository: R, launcher: L) -> Self {
        Self {
            repository,
            _launcher: launcher,
        }
    }

    pub fn load_projects(&self) -> AppResult<Vec<Project>> {
        self.repository.load_projects()
    }

    pub fn rename_session(&self, key: &SessionKey, new_title: &str) -> AppResult<Vec<Project>> {
        self.repository.rename_session(key, new_title)?;
        self.load_projects()
    }
}

#[derive(Debug, Clone)]
pub struct ClaudeCliLauncher {
    env_store: ClaudeEnvStore,
}

impl ClaudeCliLauncher {
    pub fn new(env_store: ClaudeEnvStore) -> Self {
        Self { env_store }
    }

    fn launch(&self, cwd: &Path, args: &[&str]) -> AppResult<()> {
        let command_name = self
            .env_store
            .claude_command_alias()
            .map_err(|error| format!("Failed to read claude command alias: {error}"))?;
        let status = Command::new(&command_name)
            .current_dir(cwd)
            .args(args)
            .status()
            .map_err(|error| format!("Failed to launch {command_name}: {error}"))?;

        if status.success() {
            Ok(())
        } else {
            Err(format!("{command_name} exited with status {status}"))
        }
    }
}

impl ResumeLauncher for ClaudeCliLauncher {
    fn resume(&self, target: &ResumeTarget) -> AppResult<()> {
        self.launch(&target.cwd, &["--resume", &target.key.native_id])
    }

    fn launch_new(&self, cwd: &Path) -> AppResult<()> {
        self.launch(cwd, &[])
    }
}

pub fn project_name_owned(project: &Project) -> String {
    project.name()
}

pub fn project_working_dir(project: &Project) -> String {
    project.cwd.display().to_string()
}

pub fn session_key(session: &Session) -> &SessionKey {
    &session.key
}

pub fn session_title(session: &Session) -> &str {
    &session.title
}

impl SessionRepository for ClaudeProjectsStore {
    fn load_projects(&self) -> AppResult<Vec<Project>> {
        ClaudeProjectsStore::load_projects(self).map_err(|error| error.to_string())
    }

    fn rename_session(&self, key: &SessionKey, new_title: &str) -> AppResult<()> {
        let session = self
            .discover_sessions()
            .map_err(|error| error.to_string())?
            .into_iter()
            .find(|session| &session.key == key)
            .ok_or_else(|| format!("Session not found: {}", key.native_id))?;

        let source_location = session
            .source_location
            .as_deref()
            .ok_or_else(|| "Claude session has no source location".to_string())?;
        ClaudeProjectsStore::rename_session(self, source_location, new_title)
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    fn delete_session(&self, key: &SessionKey) -> AppResult<()> {
        let session = self
            .discover_sessions()
            .map_err(|error| error.to_string())?
            .into_iter()
            .find(|session| &session.key == key)
            .ok_or_else(|| format!("Session not found: {}", key.native_id))?;

        let source_location = session
            .source_location
            .as_deref()
            .ok_or_else(|| "Claude session has no source location".to_string())?;
        ClaudeProjectsStore::delete_session(self, &key.native_id, source_location)
            .map_err(|error| error.to_string())
    }

    fn delete_project(&self, project_cwd: &Path) -> AppResult<()> {
        let project_path = PathBuf::from(project_cwd);
        ClaudeProjectsStore::delete_project(self, &project_path).map_err(|error| error.to_string())
    }
}
