use crate::domain::{Result, StetsonError};
use crate::encoding;
use dirs::home_dir;
use serde_json::Value;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use super::parser::find_first_string;

pub(super) fn default_claude_projects_dir() -> Result<PathBuf> {
    let home = home_dir().ok_or(StetsonError::HomeDirectoryUnavailable)?;
    Ok(home.join(".claude/projects"))
}

pub(super) fn remove_dir_if_exists(path: &Path) -> Result<()> {
    if path.exists() {
        fs::remove_dir_all(path)?;
    }
    Ok(())
}

pub(super) fn normalize_path(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

pub(super) fn strip_windows_extended_prefix(path: &Path) -> PathBuf {
    let path_str = path.to_string_lossy();
    if let Some(stripped) = path_str.strip_prefix(r"\\?\") {
        return PathBuf::from(stripped);
    }

    path.to_path_buf()
}

const PROJECT_SESSION_SCAN_LIMIT: usize = 8;

pub(super) fn find_project_dir(
    store_root: &Path,
    path: &Path,
) -> Result<Option<(PathBuf, PathBuf)>> {
    let normalized_path = normalize_path(path);
    let normalized_parent = normalized_path.parent().map(normalize_path);
    let encoded_name = encoding::encode_project_dir_name(&normalized_path);
    let target_name = normalized_path
        .file_name()
        .map(|n| normalize_path(Path::new(n)));

    if !store_root.exists() {
        return Ok(None);
    }

    for project_entry in fs::read_dir(store_root)? {
        let project_entry = project_entry?;
        if !project_entry.file_type()?.is_dir() {
            continue;
        }

        let candidate_path = project_entry.path();
        let decoded_candidate =
            normalize_path(&encoding::infer_cwd_from_project_dir_name(&candidate_path));
        if candidate_path
            .file_name()
            .map(|n| n == encoded_name.as_str())
            .unwrap_or(false)
        {
            return Ok(Some((candidate_path, decoded_candidate)));
        }

        if let Some(session_project_path) =
            find_session_cwd_match(&candidate_path, &normalized_path)?
        {
            return Ok(Some((candidate_path, session_project_path)));
        }

        if decoded_candidate == normalized_path
            || normalized_parent.as_ref() == Some(&decoded_candidate)
        {
            return Ok(Some((candidate_path, decoded_candidate)));
        }

        // Fallback: match by last path component for relative paths
        if let Some(ref target) = target_name {
            let candidate_name = decoded_candidate
                .file_name()
                .map(|n| normalize_path(Path::new(n)));
            if candidate_name.as_ref() == Some(target) {
                return Ok(Some((candidate_path, decoded_candidate)));
            }
        }
    }

    Ok(None)
}

fn find_session_cwd_match(project_dir: &Path, requested_path: &Path) -> Result<Option<PathBuf>> {
    let entries = match fs::read_dir(project_dir) {
        Ok(entries) => entries,
        Err(_) => return Ok(None),
    };

    let mut scanned_sessions = 0usize;
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };

        if !entry
            .file_type()
            .map(|kind| kind.is_file())
            .unwrap_or(false)
        {
            continue;
        }

        let session_path = entry.path();
        if session_path.extension() != Some(OsStr::new("jsonl")) {
            continue;
        }

        if scanned_sessions >= PROJECT_SESSION_SCAN_LIMIT {
            break;
        }
        scanned_sessions += 1;

        if let Some(session_project_path) =
            session_cwd_matches_requested_path(&session_path, requested_path)?
        {
            return Ok(Some(session_project_path));
        }
    }

    Ok(None)
}

fn session_cwd_matches_requested_path(
    session_path: &Path,
    requested_path: &Path,
) -> Result<Option<PathBuf>> {
    let contents = match fs::read_to_string(session_path) {
        Ok(contents) => contents,
        Err(_) => return Ok(None),
    };

    for line in contents
        .lines()
        .filter(|line| !line.trim().is_empty())
        .take(PROJECT_SESSION_SCAN_LIMIT)
    {
        let value = match serde_json::from_str::<Value>(line) {
            Ok(value) => value,
            Err(_) => continue,
        };

        if let Some(cwd) = find_first_string(
            &value,
            &["cwd", "currentWorkingDirectory", "workingDirectory"],
        ) {
            let cwd_path = normalize_path(Path::new(&cwd));
            let cwd_cmp = strip_windows_extended_prefix(&cwd_path);
            let requested_cmp = strip_windows_extended_prefix(requested_path);
            if requested_cmp.starts_with(&cwd_cmp) {
                if cwd_cmp.starts_with(&requested_cmp) {
                    return Ok(Some(requested_path.to_path_buf()));
                }
                return Ok(Some(cwd_path));
            }
        }
    }

    Ok(None)
}
