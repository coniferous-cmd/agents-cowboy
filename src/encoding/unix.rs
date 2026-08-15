use std::path::PathBuf;

/// Encode a Unix path into a project directory name.
///
/// `/tmp/demo/repo` → `-tmp-demo-repo`
/// Relative paths: `foo/bar` → `-foo-bar`
pub(super) fn encode(path_str: &str) -> String {
    if let Some(stripped) = path_str.strip_prefix('/') {
        return format!("-{}", stripped.replace('/', "-"));
    }
    // Relative fallback
    format!("-{}", path_str.replace('/', "-"))
}

/// Decode a Unix-style project directory name back into a path.
///
/// `tmp-demo-repo` (after stripping the leading `-`) → `/tmp/demo/repo`
pub(super) fn decode(stripped: &str) -> PathBuf {
    PathBuf::from(format!("/{}", stripped.replace('-', "/")))
}
