use std::path::PathBuf;

/// Encode a Windows path into a project directory name.
///
/// `C:\Users\test` → `C:-Users-test`
pub(super) fn encode(path_str: &str) -> String {
    let drive = &path_str[..2];
    let rest = &path_str[2..];
    format!("{}{}", drive, rest.replace(['\\', '/'], "-"))
}

/// Decode a Windows-style project directory name back into a path.
///
/// `C:-Users-test-demo` → `C:\Users\test\demo`
/// `D--kingdom-workstation` → `D:\kingdom\workstation`
pub(super) fn decode(decoded: &str) -> PathBuf {
    let bytes = decoded.as_bytes();
    if bytes.len() < 3 || !bytes[0].is_ascii_alphabetic() {
        return PathBuf::from(decoded);
    }

    if !matches!(bytes[1], b':' | b'-') {
        return PathBuf::from(decoded);
    }

    let rest = &decoded[3..];
    let mut path = String::with_capacity(decoded.len() + 2);
    path.push(bytes[0] as char);
    path.push(':');
    path.push('\\');
    path.push_str(&rest.replace(['-', '/'], "\\"));
    PathBuf::from(path)
}

/// Check if a string looks like a Windows path (drive letter followed by `-`, `\`, or `/`).
pub(super) fn looks_like_project_hint(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && matches!(
            (bytes[1], bytes[2]),
            (b':', b'-' | b'\\' | b'/') | (b'-', b'-')
        )
}
