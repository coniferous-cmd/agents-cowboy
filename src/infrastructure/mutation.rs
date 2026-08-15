use crate::domain::{Result, Session, StetsonError};
use crate::encoding;
use serde_json::{Map, Value};
use std::ffi::OsStr;
use std::fs;
use std::path::Path;

use super::parser::{parse_session_file, set_title_in_value};
use super::project_paths::{find_project_dir, remove_dir_if_exists};
use super::ClaudeProjectsStore;

impl ClaudeProjectsStore {
    pub fn rename_session(
        &self,
        session_path: impl AsRef<Path>,
        new_title: impl AsRef<str>,
    ) -> Result<Session> {
        let session_path = session_path.as_ref();
        let new_title = new_title.as_ref().trim();
        if new_title.is_empty() {
            return Err(StetsonError::InvalidSessionFile(
                "session title cannot be empty".into(),
            ));
        }

        let contents = fs::read_to_string(session_path)?;
        let mut lines: Vec<String> = if contents.is_empty() {
            Vec::new()
        } else {
            contents.lines().map(str::to_owned).collect()
        };

        let mut updated = false;
        for line in &mut lines {
            let mut value = match serde_json::from_str::<Value>(line) {
                Ok(value) => value,
                Err(_) => continue,
            };

            if set_title_in_value(&mut value, new_title) {
                *line = serde_json::to_string(&value)?;
                updated = true;
                break;
            }
        }

        if !updated {
            let mut value = Map::new();
            value.insert("type".into(), Value::String("custom-title".into()));
            value.insert("customTitle".into(), Value::String(new_title.to_string()));
            lines.push(serde_json::to_string(&Value::Object(value))?);
        }

        let mut updated_contents = lines.join("\n");
        if !updated_contents.is_empty() {
            updated_contents.push('\n');
        }

        let temp_path = session_path.with_extension("jsonl.tmp");
        fs::write(&temp_path, updated_contents)?;
        fs::rename(&temp_path, session_path)?;

        parse_session_file(
            session_path,
            encoding::parent_project_hint(session_path).as_deref(),
        )
    }

    pub fn delete_session(&self, session_id: &str, session_path: impl AsRef<Path>) -> Result<()> {
        let session_path = session_path.as_ref();
        if !session_path.exists() {
            return Err(StetsonError::SessionNotFound(session_id.to_string()));
        }

        fs::remove_file(session_path)?;

        let sidecar_dir = session_path.with_extension("");
        remove_dir_if_exists(&sidecar_dir)?;

        if let Some(claude_root) = self.root.parent() {
            for folder in ["session-env", "file-history", "tasks"] {
                remove_dir_if_exists(&claude_root.join(folder).join(session_id))?;
            }
        }

        Ok(())
    }

    pub fn delete_project(&self, project_cwd: &Path) -> Result<()> {
        let (project_dir, _) = find_project_dir(&self.root, project_cwd)?
            .ok_or_else(|| StetsonError::ProjectNotFound(project_cwd.display().to_string()))?;

        let mut sessions = Vec::new();
        for session_entry in fs::read_dir(&project_dir)? {
            let session_entry = session_entry?;
            if !session_entry.file_type()?.is_file() {
                continue;
            }

            let session_path = session_entry.path();
            if session_path.extension() != Some(OsStr::new("jsonl")) {
                continue;
            }

            sessions.push(parse_session_file(&session_path, Some(project_cwd))?);
        }

        for session in sessions {
            let source_location = session.source_location.as_deref().ok_or_else(|| {
                StetsonError::InvalidSessionFile("Claude session has no source location".into())
            })?;
            self.delete_session(session.native_id(), source_location)?;
        }

        remove_dir_if_exists(&project_dir)?;
        Ok(())
    }
}
