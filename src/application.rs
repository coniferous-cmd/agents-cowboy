use std::path::{Path, PathBuf};
use std::process::Command;

use cowboy::claude_env::{ClaudeEnvStore, ClaudeProfile, ClaudeSettingsSnapshot};
use cowboy::domain::{Project, Session, SessionKey};
use cowboy::infrastructure::ClaudeProjectsStore;

use crate::app::ResumeTarget;

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

pub trait ProfileRepository {
    fn list_profiles(&self) -> AppResult<Vec<ClaudeProfile>>;
    fn list_snapshots(&self) -> AppResult<Vec<ClaudeSettingsSnapshot>>;
    fn active_profile_name(&self) -> AppResult<Option<String>>;
    fn activate_profile(&self, name: &str) -> AppResult<()>;
    fn activate_snapshot(&self, id: i64) -> AppResult<()>;
    fn create_profile(&self, name: &str) -> AppResult<ClaudeProfile>;
    fn create_snapshot(&self, profile_id: i64, settings_json: &str) -> AppResult<()>;
    fn update_profile_json(&self, name: &str, settings_json: &str) -> AppResult<ClaudeProfile>;
    fn delete_profile(&self, name: &str) -> AppResult<()>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct NoProfileRepository;

impl ProfileRepository for NoProfileRepository {
    fn list_profiles(&self) -> AppResult<Vec<ClaudeProfile>> {
        Ok(Vec::new())
    }

    fn list_snapshots(&self) -> AppResult<Vec<ClaudeSettingsSnapshot>> {
        Ok(Vec::new())
    }

    fn active_profile_name(&self) -> AppResult<Option<String>> {
        Ok(None)
    }

    fn activate_profile(&self, _name: &str) -> AppResult<()> {
        Err("Profiles are unavailable".to_string())
    }

    fn activate_snapshot(&self, _id: i64) -> AppResult<()> {
        Err("Profiles are unavailable".to_string())
    }

    fn create_profile(&self, _name: &str) -> AppResult<ClaudeProfile> {
        Err("Profiles are unavailable".to_string())
    }

    fn create_snapshot(&self, _profile_id: i64, _settings_json: &str) -> AppResult<()> {
        Err("Profiles are unavailable".to_string())
    }

    fn update_profile_json(&self, _name: &str, _settings_json: &str) -> AppResult<ClaudeProfile> {
        Err("Profiles are unavailable".to_string())
    }

    fn delete_profile(&self, _name: &str) -> AppResult<()> {
        Err("Profiles are unavailable".to_string())
    }
}

pub struct StetsonApplication<R, L, P = NoProfileRepository> {
    repository: R,
    _launcher: L,
    profiles: P,
}

#[cfg(test)]
impl<R, L> StetsonApplication<R, L, NoProfileRepository>
where
    R: SessionRepository,
    L: ResumeLauncher,
{
    pub fn new(repository: R, launcher: L) -> Self {
        Self {
            repository,
            _launcher: launcher,
            profiles: NoProfileRepository,
        }
    }
}

impl<R, L, P> StetsonApplication<R, L, P>
where
    R: SessionRepository,
    L: ResumeLauncher,
    P: ProfileRepository,
{
    pub fn with_profiles(repository: R, launcher: L, profiles: P) -> Self {
        Self {
            repository,
            _launcher: launcher,
            profiles,
        }
    }

    pub fn load_projects(&self) -> AppResult<Vec<Project>> {
        self.repository.load_projects()
    }

    pub fn rename_session(&self, key: &SessionKey, new_title: &str) -> AppResult<Vec<Project>> {
        self.repository.rename_session(key, new_title)?;
        self.load_projects()
    }

    pub fn load_profile_data(
        &self,
    ) -> AppResult<(
        Vec<ClaudeProfile>,
        Vec<ClaudeSettingsSnapshot>,
        Option<String>,
    )> {
        Ok((
            self.profiles.list_profiles()?,
            self.profiles.list_snapshots()?,
            self.profiles.active_profile_name()?,
        ))
    }

    pub fn activate_profile(&self, name: &str) -> AppResult<()> {
        self.profiles.activate_profile(name)
    }

    pub fn activate_snapshot(&self, id: i64) -> AppResult<()> {
        self.profiles.activate_snapshot(id)
    }

    pub fn create_profile(&self, name: &str) -> AppResult<ClaudeProfile> {
        self.profiles.create_profile(name)
    }

    pub fn create_snapshot(&self, profile_id: i64, settings_json: &str) -> AppResult<()> {
        self.profiles.create_snapshot(profile_id, settings_json)
    }

    pub fn update_profile_json(&self, name: &str, settings_json: &str) -> AppResult<ClaudeProfile> {
        self.profiles.update_profile_json(name, settings_json)
    }

    pub fn delete_profile(&self, name: &str) -> AppResult<()> {
        self.profiles.delete_profile(name)
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
}

impl ResumeLauncher for ClaudeCliLauncher {
    fn resume(&self, target: &ResumeTarget) -> AppResult<()> {
        let command_name = self
            .env_store
            .claude_command_alias()
            .map_err(|error| format!("Failed to read claude command alias: {error}"))?;
        let status = Command::new(&command_name)
            .arg("--resume")
            .arg(&target.key.native_id)
            .current_dir(&target.cwd)
            .status()
            .map_err(|error| format!("Failed to launch {command_name}: {error}"))?;

        if status.success() {
            Ok(())
        } else {
            Err(format!("{command_name} exited with status {status}"))
        }
    }

    fn launch_new(&self, cwd: &Path) -> AppResult<()> {
        let command_name = self
            .env_store
            .claude_command_alias()
            .map_err(|error| format!("Failed to read claude command alias: {error}"))?;
        let status = Command::new(&command_name)
            .current_dir(cwd)
            .status()
            .map_err(|error| format!("Failed to launch {command_name}: {error}"))?;

        if status.success() {
            Ok(())
        } else {
            Err(format!("{command_name} exited with status {status}"))
        }
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

impl ProfileRepository for ClaudeEnvStore {
    fn list_profiles(&self) -> AppResult<Vec<ClaudeProfile>> {
        ClaudeEnvStore::list_profiles(self).map_err(|error| error.to_string())
    }

    fn list_snapshots(&self) -> AppResult<Vec<ClaudeSettingsSnapshot>> {
        ClaudeEnvStore::list_snapshots(self).map_err(|error| error.to_string())
    }

    fn active_profile_name(&self) -> AppResult<Option<String>> {
        ClaudeEnvStore::active_profile_name(self).map_err(|error| error.to_string())
    }

    fn activate_profile(&self, name: &str) -> AppResult<()> {
        ClaudeEnvStore::activate_profile(self, name)
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    fn activate_snapshot(&self, id: i64) -> AppResult<()> {
        ClaudeEnvStore::activate_snapshot(self, id)
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    fn create_profile(&self, name: &str) -> AppResult<ClaudeProfile> {
        ClaudeEnvStore::create_profile(self, name).map_err(|error| error.to_string())
    }

    fn create_snapshot(&self, profile_id: i64, settings_json: &str) -> AppResult<()> {
        ClaudeEnvStore::create_snapshot(self, profile_id, settings_json)
            .map_err(|error| error.to_string())
    }

    fn update_profile_json(&self, name: &str, settings_json: &str) -> AppResult<ClaudeProfile> {
        ClaudeEnvStore::update_profile_json(self, name, settings_json)
            .map_err(|error| error.to_string())
    }

    fn delete_profile(&self, name: &str) -> AppResult<()> {
        ClaudeEnvStore::delete_profile(self, name).map_err(|error| error.to_string())
    }
}
