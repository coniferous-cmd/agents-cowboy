mod unix;
mod win;

use std::path::{Path, PathBuf};

/// Encode a filesystem path into a `~/.claude/projects/` directory name.
///
/// Dispatches to platform-specific encoding based on the path format:
/// - Windows (`C:\Users\test` → `C:-Users-test`)
/// - Unix (`/tmp/demo/repo` → `-tmp-demo-repo`)
pub(crate) fn encode_project_dir_name(path: &Path) -> String {
    let path_str = path.to_string_lossy();
    let bytes = path_str.as_bytes();

    // Windows: C:\Users\... or C:/Users/...
    if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
    {
        return win::encode(&path_str);
    }

    unix::encode(&path_str)
}

/// Decode a `~/.claude/projects/` directory name back into a filesystem path.
///
/// Handles URL-encoded characters (`%2F` → `/`, `%5C` → `\`), then
/// dispatches to platform-specific decoding based on the pattern:
/// - Starts with `-`: Unix-style (`-tmp-demo-repo` → `/tmp/demo/repo`)
/// - Drive letter (`C:...`): Windows-style (`C:-Users-test-demo` → `C:\Users\test\demo`)
pub(crate) fn decode_project_hint(dir_name: &str) -> PathBuf {
    let url_decoded = dir_name
        .replace("%2F", "/")
        .replace("%2f", "/")
        .replace("%5C", "\\")
        .replace("%5c", "\\");

    if let Some(stripped) = url_decoded.strip_prefix('-') {
        return unix::decode(stripped);
    }

    if win::looks_like_project_hint(&url_decoded) {
        return win::decode(&url_decoded);
    }

    PathBuf::from(url_decoded)
}

/// Extract the project directory name from a `Path` and decode it into a filesystem path.
pub(crate) fn infer_cwd_from_project_dir_name(project_dir: &Path) -> PathBuf {
    let dir_name = project_dir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| project_dir.display().to_string());
    decode_project_hint(&dir_name)
}

/// Get the project cwd hint from a session file's parent directory.
pub(crate) fn parent_project_hint(session_path: &Path) -> Option<PathBuf> {
    let parent = session_path.parent()?;
    Some(infer_cwd_from_project_dir_name(parent))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn decodes_windows_project_dir_hint() {
        let decoded = decode_project_hint("C:-Users-test-demo");
        assert_eq!(decoded, PathBuf::from(r"C:\Users\test\demo"));
    }

    #[test]
    fn decodes_current_windows_project_dir_hint() {
        let decoded = decode_project_hint("D--kingdom-workstation");
        assert_eq!(decoded, PathBuf::from(r"D:\kingdom\workstation"));
    }

    #[test]
    fn encodes_project_dir_name() {
        assert_eq!(
            encode_project_dir_name(Path::new("/tmp/demo/repo")),
            "-tmp-demo-repo".to_string()
        );
        assert_eq!(
            encode_project_dir_name(Path::new("/tmp/claude-cap")),
            "-tmp-claude-cap".to_string()
        );
        assert_eq!(
            encode_project_dir_name(Path::new("/a-b/c-d")),
            "-a-b-c-d".to_string()
        );
    }
}
