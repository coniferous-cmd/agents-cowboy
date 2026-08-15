use crate::domain::{group_sessions_by_project, Project, Result, Session};
use crate::encoding;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use super::parser::parse_session_file;
use super::project_paths::{default_claude_projects_dir, strip_windows_extended_prefix};
use super::ClaudeProjectsStore;

impl ClaudeProjectsStore {
    pub fn from_home() -> Result<Self> {
        Ok(Self::new(default_claude_projects_dir()?))
    }

    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn discover_sessions(&self) -> Result<Vec<Session>> {
        let mut sessions = Vec::new();
        if !self.root.exists() {
            return Ok(sessions);
        }

        for project_entry in fs::read_dir(&self.root)? {
            let project_entry = project_entry?;
            let file_type = project_entry.file_type()?;
            if !file_type.is_dir() {
                continue;
            }

            let project_dir = project_entry.path();
            let project_hint = encoding::infer_cwd_from_project_dir_name(&project_dir);
            for session_entry in fs::read_dir(&project_dir)? {
                let session_entry = session_entry?;
                let session_type = session_entry.file_type()?;
                if !session_type.is_file() {
                    continue;
                }

                let session_path = session_entry.path();
                if session_path.extension() != Some(OsStr::new("jsonl")) {
                    continue;
                }

                sessions.push(parse_session_file(&session_path, Some(&project_hint))?);
            }
        }

        Ok(sessions)
    }

    pub fn load_projects(&self) -> Result<Vec<Project>> {
        Ok(group_sessions_by_project(self.discover_sessions()?))
    }

    pub fn resolve_project_path(&self, path: &Path) -> Result<Option<PathBuf>> {
        Ok(self
            .find_project_dir(path)?
            .map(|(_, project_path)| strip_windows_extended_prefix(&project_path)))
    }

    fn find_project_dir(&self, path: &Path) -> Result<Option<(PathBuf, PathBuf)>> {
        super::project_paths::find_project_dir(&self.root, path)
    }
}
