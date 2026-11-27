use crate::domain::{group_sessions_by_project, Project, ProjectCatalog, Result, Session};
use crate::encoding;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use super::parser::{discover_session_cwd, parse_session_file};
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
        self.visit_session_files(|path, hint| {
            sessions.push(parse_session_file(&path, Some(hint))?);
            Ok(())
        })?;
        Ok(sessions)
    }

    pub fn discover_projects(&self) -> Result<ProjectCatalog> {
        let mut catalog = ProjectCatalog::new();
        self.visit_session_files(|path, hint| {
            let cwd = discover_session_cwd(&path, hint)?;
            catalog.entry(cwd).or_default().push(path);
            Ok(())
        })?;
        Ok(catalog)
    }

    pub fn load_project(&self, cwd: &Path, sources: &[PathBuf]) -> Result<Project> {
        let mut sessions = Vec::new();
        for path in sources {
            let hint = path.parent().map(encoding::infer_cwd_from_project_dir_name);
            let session = parse_session_file(path, hint.as_deref())?;
            if session.cwd == cwd {
                sessions.push(session);
            }
        }
        Ok(group_sessions_by_project(sessions)
            .pop()
            .unwrap_or_else(|| Project {
                cwd: cwd.to_path_buf(),
                sessions: Vec::new(),
            }))
    }

    fn visit_session_files(
        &self,
        mut visit: impl FnMut(PathBuf, &Path) -> Result<()>,
    ) -> Result<()> {
        if !self.root.exists() {
            return Ok(());
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

                visit(session_path, &project_hint)?;
            }
        }

        Ok(())
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
