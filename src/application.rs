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

impl<R: SessionRepository + ?Sized> SessionRepository for std::sync::Arc<R> {
    fn load_projects(&self) -> AppResult<Vec<Project>> {
        self.as_ref().load_projects()
    }
    fn rename_session(&self, key: &SessionKey, title: &str) -> AppResult<()> {
        self.as_ref().rename_session(key, title)
    }
    fn delete_session(&self, key: &SessionKey) -> AppResult<()> {
        self.as_ref().delete_session(key)
    }
    fn delete_project(&self, cwd: &Path) -> AppResult<()> {
        self.as_ref().delete_project(cwd)
    }
}

pub trait ResumeLauncher {
    fn resume(&self, target: &ResumeTarget) -> AppResult<()>;
    fn launch_new(&self, cwd: &Path) -> AppResult<()>;
}

/// One supported agent backend = one strategy = one tab.
///
/// The registry of implementors drives the agent tab strip: a tab exists
/// exactly when its strategy is registered. Implementations own their
/// session-directory discovery, transcript parsing and usage extraction,
/// launch/resume arguments, and rename/delete file operations; token
/// summation, formatting, and cost estimation stay shared domain logic.
pub trait AgentBackend: SessionRepository + ResumeLauncher {
    fn id(&self) -> cowboy::domain::AgentId;
    fn label(&self) -> &str;
}

/// The Claude strategy: composes the existing store and launcher so the
/// refactor stays a thin reorganization of proven behavior.
#[derive(Clone)]
pub struct ClaudeBackend {
    repository: ClaudeProjectsStore,
    launcher: ClaudeCliLauncher,
}

impl ClaudeBackend {
    pub fn new(repository: ClaudeProjectsStore, launcher: ClaudeCliLauncher) -> Self {
        Self {
            repository,
            launcher,
        }
    }
}

impl SessionRepository for ClaudeBackend {
    fn load_projects(&self) -> AppResult<Vec<Project>> {
        SessionRepository::load_projects(&self.repository)
    }

    fn rename_session(&self, key: &SessionKey, new_title: &str) -> AppResult<()> {
        SessionRepository::rename_session(&self.repository, key, new_title)
    }

    fn delete_session(&self, key: &SessionKey) -> AppResult<()> {
        SessionRepository::delete_session(&self.repository, key)
    }

    fn delete_project(&self, project_cwd: &Path) -> AppResult<()> {
        SessionRepository::delete_project(&self.repository, project_cwd)
    }
}

impl ResumeLauncher for ClaudeBackend {
    fn resume(&self, target: &ResumeTarget) -> AppResult<()> {
        self.launcher.resume(target)
    }

    fn launch_new(&self, cwd: &Path) -> AppResult<()> {
        self.launcher.launch_new(cwd)
    }
}

impl AgentBackend for ClaudeBackend {
    fn id(&self) -> cowboy::domain::AgentId {
        cowboy::domain::AgentId::new("claude").expect("built-in agent id is valid")
    }

    fn label(&self) -> &str {
        "Claude"
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

#[derive(Clone)]
pub struct PiBackend {
    repository: cowboy::pi::PiProjectsStore,
    launcher: PiCliLauncher,
}
impl PiBackend {
    pub fn new(repository: cowboy::pi::PiProjectsStore) -> Self {
        Self {
            repository,
            launcher: PiCliLauncher,
        }
    }
}
impl SessionRepository for PiBackend {
    fn load_projects(&self) -> AppResult<Vec<Project>> {
        self.repository.load_projects().map_err(|e| e.to_string())
    }
    fn rename_session(&self, key: &SessionKey, title: &str) -> AppResult<()> {
        self.repository
            .rename_session(key, title)
            .map_err(|e| e.to_string())
    }
    fn delete_session(&self, key: &SessionKey) -> AppResult<()> {
        self.repository
            .delete_session(key)
            .map_err(|e| e.to_string())
    }
    fn delete_project(&self, cwd: &Path) -> AppResult<()> {
        self.repository
            .delete_project(cwd)
            .map_err(|e| e.to_string())
    }
}
#[derive(Clone)]
pub struct PiCliLauncher;
impl PiCliLauncher {
    fn command(cwd: &Path, args: &[&str]) -> Command {
        let mut cmd = Command::new("pi");
        cmd.current_dir(cwd).args(args);
        cmd
    }
    fn launch(&self, cwd: &Path, args: &[&str]) -> AppResult<()> {
        let status = Self::command(cwd, args)
            .status()
            .map_err(|e| format!("Failed to launch Pi: {e}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("Pi exited with status {status}"))
        }
    }
}
impl ResumeLauncher for PiBackend {
    fn resume(&self, t: &ResumeTarget) -> AppResult<()> {
        self.launcher
            .launch(&t.cwd, &["--session", &t.key.native_id])
    }
    fn launch_new(&self, cwd: &Path) -> AppResult<()> {
        self.launcher.launch(cwd, &[])
    }
}
impl AgentBackend for PiBackend {
    fn id(&self) -> cowboy::domain::AgentId {
        cowboy::domain::AgentId::new("pi").unwrap()
    }
    fn label(&self) -> &str {
        "Pi"
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

#[cfg(test)]
mod backend_tests {
    use super::{AgentBackend, ClaudeBackend, ClaudeCliLauncher, SessionRepository};
    use cowboy::claude_env::ClaudeEnvStore;
    use cowboy::domain::SessionKey;
    use cowboy::infrastructure::ClaudeProjectsStore;
    use std::fs;
    use std::path::Path;

    fn backend_with_projects_root(root: &Path) -> ClaudeBackend {
        ClaudeBackend::new(
            ClaudeProjectsStore::new(root.join("projects")),
            ClaudeCliLauncher::new(ClaudeEnvStore::new(root.join("cowboy.db"))),
        )
    }

    fn write_session_fixture(projects_root: &Path) {
        let project_dir = projects_root.join("-tmp-demo-repo");
        fs::create_dir_all(&project_dir).unwrap();
        fs::write(
            project_dir.join("session-1.jsonl"),
            [
                r#"{"type":"meta","sessionId":"session-1","cwd":"/tmp/demo/repo","timestamp":"2025-07-05T08:00:00Z"}"#,
                r#"{"type":"agent-name","agentName":"Original Title"}"#,
                r#"{"role":"user","message":{"content":"hello"}}"#,
            ]
            .join("\n"),
        )
        .unwrap();
    }

    #[test]
    fn claude_backend_reports_id_and_label() {
        let temp = tempfile::tempdir().unwrap();
        let backend = backend_with_projects_root(temp.path());

        assert_eq!(backend.id().as_str(), "claude");
        assert_eq!(backend.label(), "Claude");
    }

    #[test]
    fn claude_backend_load_projects_delegates_to_store() {
        let temp = tempfile::tempdir().unwrap();
        let projects_root = temp.path().join("projects");
        write_session_fixture(&projects_root);
        let backend = backend_with_projects_root(temp.path());

        let via_backend = SessionRepository::load_projects(&backend).unwrap();
        let via_store = ClaudeProjectsStore::new(&projects_root)
            .load_projects()
            .unwrap();

        assert_eq!(via_backend, via_store);
        assert_eq!(via_backend.len(), 1);
        assert_eq!(via_backend[0].sessions.len(), 1);
        assert_eq!(via_backend[0].sessions[0].key.agent_id.as_str(), "claude");
    }

    #[test]
    fn claude_backend_rename_and_delete_delegate_to_store() {
        let temp = tempfile::tempdir().unwrap();
        let projects_root = temp.path().join("projects");
        write_session_fixture(&projects_root);
        let backend = backend_with_projects_root(temp.path());
        let key = SessionKey::claude("session-1");

        backend.rename_session(&key, "Renamed Title").unwrap();
        let projects = SessionRepository::load_projects(&backend).unwrap();
        assert_eq!(projects[0].sessions[0].title, "Renamed Title");

        backend.delete_session(&key).unwrap();
        let projects = SessionRepository::load_projects(&backend).unwrap();
        assert!(projects.is_empty() || projects[0].sessions.is_empty());
    }
}
