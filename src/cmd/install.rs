//! `cowboy install` — Download and install the latest Claude Code binary
//! from coniferous-cmd/claude-shop GitHub Releases.
//!
//! ## Phases (TDD)
//!
//! 1. CLI parsing — `install` → `CommandMode::Install`, reject extra args
//! 2. Platform detection — map OS+arch+libc to release platform string
//! 3. Manifest parsing — extract version, size, SHA-256 from manifest.json
//! 4. Path planning — determine install directory (versions/<version> under app data)
//! 5. Download, verify, and atomic replace — stream + SHA-256 + atomic swap
//! 6. Alias confirmation — ask before persisting the installed binary path
//! 7. Documentation — help text and README

use std::{
    error::Error,
    fs,
    io::{self, BufRead, Write},
    path::{Path, PathBuf},
};

use cowboy::claude_env::ClaudeEnvStore;

#[cfg(test)]
use cowboy::claude_env::Setting;

use super::CommandMode;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const MANIFEST_URL: &str =
    "https://github.com/coniferous-cmd/claude-shop/releases/latest/download/manifest.json";

const MANIFEST_MAX_SIZE: u64 = 10 * 1024 * 1024; // 10 MiB — small JSON payloads only

#[cfg(test)]
const SETTING_CLAUDE_COMMAND_ALIAS: &str = "claude_command_alias";

// ---------------------------------------------------------------------------
// Public API — CLI entry points
// ---------------------------------------------------------------------------

pub(crate) fn handle_install(env_store: &ClaudeEnvStore) -> Result<(), Box<dyn Error>> {
    let installed_path = do_install(env_store, default_fetch)?;
    let stdin = io::stdin();
    let stdout = io::stdout();
    confirm_alias(
        env_store,
        &installed_path,
        &mut stdin.lock(),
        &mut stdout.lock(),
    )
}

fn prompt_for_alias<R: BufRead, W: Write>(input: &mut R, output: &mut W) -> io::Result<bool> {
    loop {
        write!(
            output,
            "Use the installed Claude Code binary as the command alias? [y/n] "
        )?;
        output.flush()?;

        let mut answer = String::new();
        if input.read_line(&mut answer)? == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "No alias confirmation received",
            ));
        }

        match answer.trim().to_ascii_lowercase().as_str() {
            "y" => return Ok(true),
            "n" => return Ok(false),
            _ => writeln!(output, "Please enter y or n.")?,
        }
    }
}

fn confirm_alias<R: BufRead, W: Write>(
    env_store: &ClaudeEnvStore,
    installed_path: &Path,
    input: &mut R,
    output: &mut W,
) -> Result<(), Box<dyn Error>> {
    if prompt_for_alias(input, output)? {
        super::handle_alias(env_store, installed_path.to_string_lossy().to_string())?;
    }
    Ok(())
}

pub(super) fn parse_install_args<I>(args: I) -> Result<CommandMode, String>
where
    I: IntoIterator<Item = String>,
{
    let mut args = args.into_iter();
    if args.next().is_some() {
        return Err("Usage: cowboy install".to_string());
    }
    Ok(CommandMode::Install)
}

// ---------------------------------------------------------------------------
// Platform detection
// ---------------------------------------------------------------------------

/// Map `(os, arch, optional libc)` to a release platform string.
///
/// Supported platforms:
///
/// | OS      | Arch    | libc   | Platform string    |
/// |---------|---------|--------|--------------------|
/// | macos   | aarch64 | —      | `darwin-arm64`     |
/// | macos   | x86_64  | —      | `darwin-x64`       |
/// | linux   | aarch64 | gnu    | `linux-arm64`      |
/// | linux   | aarch64 | musl   | `linux-arm64-musl` |
/// | linux   | x86_64  | gnu    | `linux-x64`        |
/// | linux   | x86_64  | musl   | `linux-x64-musl`   |
/// | windows | aarch64 | —      | `win32-arm64`      |
/// | windows | x86_64  | —      | `win32-x64`        |
pub(super) fn detect_platform(os: &str, arch: &str, libc: Option<&str>) -> Result<String, String> {
    match (os, arch, libc.unwrap_or("gnu")) {
        ("macos", "aarch64", _) => Ok("darwin-arm64".to_string()),
        ("macos", "x86_64", _) => Ok("darwin-x64".to_string()),
        ("linux", "aarch64", "gnu") => Ok("linux-arm64".to_string()),
        ("linux", "x86_64", "gnu") => Ok("linux-x64".to_string()),
        ("linux", "aarch64", "musl") => Ok("linux-arm64-musl".to_string()),
        ("linux", "x86_64", "musl") => Ok("linux-x64-musl".to_string()),
        ("windows", "aarch64", _) => Ok("win32-arm64".to_string()),
        ("windows", "x86_64", _) => Ok("win32-x64".to_string()),
        _ => Err(format!("Unsupported platform: {os}-{arch}")),
    }
}

/// Return the target binary file name for the given platform string.
pub(super) fn target_file_name(platform: &str) -> &'static str {
    if platform.starts_with("win32") {
        "claude.exe"
    } else {
        "claude"
    }
}

// ---------------------------------------------------------------------------
// Manifest types & parsing
// ---------------------------------------------------------------------------

/// A single platform entry inside the manifest.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct ManifestEntry {
    pub size: u64,
    pub sha256: String,
}

/// The parsed release manifest.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Manifest {
    pub version: String,
    pub platforms: Vec<(String, ManifestEntry)>,
}

impl Manifest {
    /// Look up the entry for a given platform string.
    pub fn entry_for_platform(&self, platform: &str) -> Option<&ManifestEntry> {
        self.platforms
            .iter()
            .find(|(p, _)| p == platform)
            .map(|(_, e)| e)
    }
}

/// Parse the release manifest JSON bytes.
///
/// Expected JSON shape:
/// ```json
/// {
///   "version": "0.4.20",
///   "platforms": {
///     "darwin-arm64": { "size": 12345678, "checksum": "abc..." },
///     ...
///   }
/// }
/// ```
pub(super) fn parse_manifest(data: &[u8]) -> Result<Manifest, String> {
    use serde_json::Value;

    let root: Value =
        serde_json::from_slice(data).map_err(|e| format!("Invalid manifest JSON: {e}"))?;

    let version = root
        .get("version")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing 'version' field in manifest".to_string())?
        .to_string();

    let platforms_obj = root
        .get("platforms")
        .and_then(|v| v.as_object())
        .ok_or_else(|| "Missing 'platforms' field in manifest".to_string())?;

    let mut platforms = Vec::with_capacity(platforms_obj.len());
    for (key, val) in platforms_obj {
        let entry = parse_platform_entry(val)?;
        platforms.push((key.clone(), entry));
    }

    Ok(Manifest { version, platforms })
}

fn parse_platform_entry(val: &serde_json::Value) -> Result<ManifestEntry, String> {
    let obj = val
        .as_object()
        .ok_or_else(|| "Platform entry must be a JSON object".to_string())?;

    // --- size ---
    let size_val = obj
        .get("size")
        .ok_or_else(|| "Missing 'size' in platform entry".to_string())?;

    let size: u64 = match size_val {
        serde_json::Value::Number(n) => n
            .as_u64()
            .ok_or_else(|| format!("Invalid numeric 'size' value: {n}"))?,
        serde_json::Value::String(s) => s
            .parse::<u64>()
            .map_err(|_| format!("Invalid string 'size' value: {s}"))?,
        _ => return Err("'size' must be a number or string".to_string()),
    };

    // --- checksum ---
    let sha256 = obj
        .get("checksum")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing 'checksum' in platform entry".to_string())?
        .to_string();

    validate_sha256(&sha256)?;

    Ok(ManifestEntry { size, sha256 })
}

/// Validate that a string is a 64-char hex SHA-256 digest.
fn validate_sha256(hex: &str) -> Result<(), String> {
    if hex.len() != 64 {
        return Err(format!(
            "SHA-256 must be 64 hex characters, got {}",
            hex.len()
        ));
    }
    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("SHA-256 contains non-hex characters: {hex}"));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Install path planning
// ---------------------------------------------------------------------------

/// Computed paths for an install operation.
#[derive(Debug)]
pub(super) struct InstallPaths {
    pub install_dir: PathBuf,
    pub target_bin: PathBuf,
    pub temp_bin: PathBuf,
}

/// Validate that a manifest version is a safe single path component.
///
/// Rejects empty values, `.`/`..`, and any component containing `/` or `\`
/// to prevent directory traversal out of the `versions/` tree.
fn validate_version_component(version: &str) -> Result<(), String> {
    if version.is_empty() {
        return Err("Invalid manifest version: must not be empty".to_string());
    }
    if version == "." || version == ".." {
        return Err(format!(
            "Invalid manifest version: must not be '.' or '..': {version}"
        ));
    }
    if version.contains('/') || version.contains('\\') {
        return Err(format!(
            "Invalid manifest version: must not contain path separators: {version}"
        ));
    }
    Ok(())
}

/// Compute install paths based on the ClaudeEnvStore database path and version.
///
/// `db_path` — path to the `cowboy.db` SQLite file.
/// `version` — the manifest version string (e.g. "1.2.3").
/// `file_name` — the target binary name (e.g. "claude" or "claude.exe").
///
/// The install directory is `<db_parent>/versions/<version>/`.
pub(super) fn plan_install_path(
    db_path: &Path,
    version: &str,
    file_name: &str,
) -> Result<InstallPaths, String> {
    validate_version_component(version)?;

    let parent = db_path.parent().ok_or_else(|| {
        format!(
            "Cannot determine app data directory from database path: {}",
            db_path.display()
        )
    })?;

    if !parent.is_dir() {
        return Err(format!(
            "Database parent directory does not exist: {}",
            parent.display()
        ));
    }

    let version_dir = parent.join("versions").join(version);
    let target_bin = version_dir.join(file_name);
    let temp_bin = version_dir.join(format!(".{}.download", file_name));

    Ok(InstallPaths {
        install_dir: version_dir,
        target_bin,
        temp_bin,
    })
}

// ---------------------------------------------------------------------------
// SHA-256 helper
// ---------------------------------------------------------------------------

/// Decode a 64-char hex string into a 32-byte array.
fn sha256_hex_to_bytes(hex: &str) -> Result<[u8; 32], String> {
    validate_sha256(hex)?;
    let mut out = [0u8; 32];
    for i in 0..32 {
        out[i] = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)
            .map_err(|_| format!("Invalid hex at position {}", i * 2))?;
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Binary verification
// ---------------------------------------------------------------------------

/// Verify downloaded data matches expected size and SHA-256 digest.
pub(super) fn verify_binary(
    data: &[u8],
    expected_size: u64,
    expected_sha256: &str,
) -> Result<(), String> {
    let actual_size = data.len() as u64;
    if actual_size != expected_size {
        return Err(format!(
            "Size mismatch: expected {expected_size} bytes, got {actual_size}"
        ));
    }

    use sha2::{Digest, Sha256};
    let actual_digest = Sha256::digest(data);
    let expected_digest = sha256_hex_to_bytes(expected_sha256)?;

    if actual_digest.as_slice() != expected_digest.as_slice() {
        return Err("SHA-256 mismatch".to_string());
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Atomic file replacement
// ---------------------------------------------------------------------------

/// Atomically replace `target` with `source`.
///
/// - On Unix: `rename` overwrites atomically.
/// - On Windows: backup the target first, rename, then remove backup.
///   If rename fails the backup is restored.
pub(super) fn atomic_replace(target: &Path, source: &Path) -> Result<(), String> {
    #[cfg(not(windows))]
    {
        fs::rename(source, target)
            .map_err(|e| format!("Failed to replace '{}': {e}", target.display()))?;
    }

    #[cfg(windows)]
    {
        let backup = target.with_extension("bak");
        let had_backup = target.exists();
        if had_backup {
            fs::rename(target, &backup)
                .map_err(|e| format!("Failed to backup '{}': {e}", target.display()))?;
        }
        if let Err(e) = fs::rename(source, target) {
            // Restore backup
            if had_backup {
                let _ = fs::rename(&backup, target);
            }
            return Err(format!("Failed to replace '{}': {e}", target.display()));
        }
        // Success — remove backup if it exists
        let _ = fs::remove_file(&backup);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// URL construction
// ---------------------------------------------------------------------------

fn manifest_download_url() -> String {
    MANIFEST_URL.to_string()
}

fn binary_download_url(version: &str, platform: &str) -> String {
    let base = format!(
        "https://github.com/coniferous-cmd/claude-shop/releases/latest/download/claude-{version}-{platform}"
    );
    if platform.starts_with("win32") {
        format!("{base}.exe")
    } else {
        base
    }
}

// ---------------------------------------------------------------------------
// Core install logic (testable with injected HTTP fetch)
// ---------------------------------------------------------------------------

fn do_install<F>(env_store: &ClaudeEnvStore, fetch: F) -> Result<PathBuf, Box<dyn Error>>
where
    F: Fn(&str, u64) -> Result<Vec<u8>, String>,
{
    // 1. Detect platform
    let libc = if cfg!(target_env = "musl") {
        Some("musl")
    } else {
        None
    };
    let platform = detect_platform(std::env::consts::OS, std::env::consts::ARCH, libc)
        .map_err(|e| io::Error::new(io::ErrorKind::Unsupported, e))?;
    let file_name = target_file_name(&platform);

    // 2. Fetch manifest (small bound — manifest is a tiny JSON file)
    let manifest_data = fetch(&manifest_download_url(), MANIFEST_MAX_SIZE)
        .map_err(|e| io::Error::other(format!("Failed to download manifest: {e}")))?;

    // 3. Parse manifest
    let manifest = parse_manifest(&manifest_data).map_err(|e| {
        io::Error::new(io::ErrorKind::InvalidData, format!("Invalid manifest: {e}"))
    })?;

    let entry = manifest.entry_for_platform(&platform).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("No release asset for platform: {platform}"),
        )
    })?;

    // 4. Plan & create version directory
    let paths = plan_install_path(env_store.path(), &manifest.version, file_name)?;
    fs::create_dir_all(&paths.install_dir)?;

    if !paths.install_dir.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "Install directory not found: {}",
                paths.install_dir.display()
            ),
        )
        .into());
    }

    // 5. Download binary (bounded by manifest-declared size + 1 to account
    //    for ureq's LimitReader erroring when the body exactly equals the limit)
    let binary_url = binary_download_url(&manifest.version, &platform);
    println!(
        "Downloading Claude Code {} for {platform}...",
        manifest.version
    );
    let binary_data = fetch(&binary_url, entry.size + 1)
        .map_err(|e| io::Error::other(format!("Failed to download binary: {e}")))?;

    // 6. Verify size and SHA-256
    verify_binary(&binary_data, entry.size, &entry.sha256)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    // 7. Write to temp file
    let _ = fs::remove_file(&paths.temp_bin);
    {
        let mut tmp = fs::File::create(&paths.temp_bin).map_err(|e| {
            format!(
                "Cannot create temp file '{}': {e}",
                paths.temp_bin.display()
            )
        })?;
        tmp.write_all(&binary_data)
            .map_err(|e| format!("Cannot write temp file '{}': {e}", paths.temp_bin.display()))?;
    }

    // 8. Set executable permissions on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&paths.temp_bin, fs::Permissions::from_mode(0o755)).map_err(|e| {
            format!(
                "Cannot set permissions on '{}': {e}",
                paths.temp_bin.display()
            )
        })?;
    }

    // 9. Atomic replace (this is the point of no return for the old binary)
    atomic_replace(&paths.target_bin, &paths.temp_bin)?;

    // 10. Clean up temp file
    let _ = fs::remove_file(&paths.temp_bin);

    println!(
        "Claude Code {} installed to {}",
        manifest.version,
        paths.target_bin.display()
    );

    Ok(paths.target_bin)
}

// ---------------------------------------------------------------------------
// Production HTTP fetch
// ---------------------------------------------------------------------------

fn default_fetch(url: &str, max_size: u64) -> Result<Vec<u8>, String> {
    let response = ureq::get(url)
        .call()
        .map_err(|e| format!("HTTP request failed: {e}"))?;

    let status = response.status();
    if status != 200 {
        return Err(format!("HTTP {status}"));
    }

    let body = response
        .into_body()
        .with_config()
        .limit(max_size)
        .read_to_vec()
        .map_err(|e| format!("Failed to read response body: {e}"))?;

    Ok(body)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use cowboy::claude_env::ClaudeEnvStore;
    use std::path::Path;
    use tempfile::TempDir;

    // ======================================================================
    // Phase 1: CLI parsing
    // ======================================================================

    #[test]
    fn parses_install_command() {
        let mode = parse_install_args([]).unwrap();
        assert_eq!(mode, CommandMode::Install);
    }

    #[test]
    fn rejects_install_with_extra_args() {
        let result = parse_install_args(["extra".to_string()]);
        assert!(result.is_err());
        assert!(
            result.unwrap_err().contains("Usage:"),
            "expected usage error"
        );
    }

    #[test]
    fn alias_prompt_accepts_y() {
        let mut input = "y\n".as_bytes();
        let mut output = Vec::new();

        assert!(prompt_for_alias(&mut input, &mut output).unwrap());
        assert!(String::from_utf8(output).unwrap().contains("[y/n]"));
    }

    #[test]
    fn alias_prompt_accepts_n() {
        let mut input = "n\n".as_bytes();
        let mut output = Vec::new();

        assert!(!prompt_for_alias(&mut input, &mut output).unwrap());
    }

    #[test]
    fn alias_prompt_retries_after_invalid_input() {
        let mut input = "maybe\ny\n".as_bytes();
        let mut output = Vec::new();

        assert!(prompt_for_alias(&mut input, &mut output).unwrap());
        assert!(String::from_utf8(output)
            .unwrap()
            .contains("Please enter y or n"));
    }

    // ======================================================================
    // Phase 2: Platform detection
    // ======================================================================

    #[test]
    fn platform_macos_arm64() {
        assert_eq!(
            detect_platform("macos", "aarch64", None).unwrap(),
            "darwin-arm64"
        );
    }

    #[test]
    fn platform_macos_x64() {
        assert_eq!(
            detect_platform("macos", "x86_64", None).unwrap(),
            "darwin-x64"
        );
    }

    #[test]
    fn platform_linux_arm64_gnu() {
        assert_eq!(
            detect_platform("linux", "aarch64", Some("gnu")).unwrap(),
            "linux-arm64"
        );
    }

    #[test]
    fn platform_linux_arm64_musl() {
        assert_eq!(
            detect_platform("linux", "aarch64", Some("musl")).unwrap(),
            "linux-arm64-musl"
        );
    }

    #[test]
    fn platform_linux_x64_gnu() {
        assert_eq!(
            detect_platform("linux", "x86_64", Some("gnu")).unwrap(),
            "linux-x64"
        );
    }

    #[test]
    fn platform_linux_x64_musl() {
        assert_eq!(
            detect_platform("linux", "x86_64", Some("musl")).unwrap(),
            "linux-x64-musl"
        );
    }

    #[test]
    fn platform_windows_arm64() {
        assert_eq!(
            detect_platform("windows", "aarch64", None).unwrap(),
            "win32-arm64"
        );
    }

    #[test]
    fn platform_windows_x64() {
        assert_eq!(
            detect_platform("windows", "x86_64", None).unwrap(),
            "win32-x64"
        );
    }

    #[test]
    fn platform_unsupported_os() {
        let result = detect_platform("freebsd", "x86_64", None);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unsupported platform"));
    }

    #[test]
    fn platform_unsupported_arch() {
        let result = detect_platform("linux", "riscv64", None);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unsupported platform"));
    }

    #[test]
    fn platform_linux_arm64_defaults_to_gnu() {
        assert_eq!(
            detect_platform("linux", "aarch64", None).unwrap(),
            "linux-arm64"
        );
    }

    #[test]
    fn platform_linux_x64_defaults_to_gnu() {
        assert_eq!(
            detect_platform("linux", "x86_64", None).unwrap(),
            "linux-x64"
        );
    }

    #[test]
    fn target_file_name_unix() {
        assert_eq!(target_file_name("darwin-arm64"), "claude");
        assert_eq!(target_file_name("linux-x64"), "claude");
        assert_eq!(target_file_name("linux-arm64-musl"), "claude");
    }

    #[test]
    fn target_file_name_windows() {
        assert_eq!(target_file_name("win32-x64"), "claude.exe");
        assert_eq!(target_file_name("win32-arm64"), "claude.exe");
    }

    // ======================================================================
    // Phase 3: Manifest parsing
    // ======================================================================

    #[test]
    fn parse_valid_manifest() {
        let json = br#"{
            "version": "1.2.3",
            "platforms": {
                "darwin-arm64": {
                    "size": 12345678,
                    "checksum": "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890"
                }
            }
        }"#;

        let manifest = parse_manifest(json).unwrap();
        assert_eq!(manifest.version, "1.2.3");

        let entry = manifest.entry_for_platform("darwin-arm64").unwrap();
        assert_eq!(entry.size, 12_345_678);
        assert_eq!(
            entry.sha256,
            "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890"
        );
    }

    #[test]
    fn parse_manifest_size_as_string() {
        let json = br#"{
            "version": "1.0.0",
            "platforms": {
                "linux-x64": {
                    "size": "9876543",
                    "checksum": "1111111111111111111111111111111111111111111111111111111111111111"
                }
            }
        }"#;

        let manifest = parse_manifest(json).unwrap();
        let entry = manifest.entry_for_platform("linux-x64").unwrap();
        assert_eq!(entry.size, 9_876_543);
    }

    #[test]
    fn parse_manifest_multiple_platforms() {
        let json = br#"{
            "version": "2.0.0",
            "platforms": {
                "darwin-arm64": { "size": 1, "checksum": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" },
                "darwin-x64": { "size": 2, "checksum": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb" }
            }
        }"#;

        let manifest = parse_manifest(json).unwrap();
        assert_eq!(manifest.entry_for_platform("darwin-arm64").unwrap().size, 1);
        assert_eq!(manifest.entry_for_platform("darwin-x64").unwrap().size, 2);
        assert!(manifest.entry_for_platform("linux-x64").is_none());
    }

    #[test]
    fn parse_manifest_missing_version() {
        let json = br#"{ "platforms": {} }"#;
        let err = parse_manifest(json).unwrap_err();
        assert!(err.contains("version"), "error mentions 'version': {err}");
    }

    #[test]
    fn parse_manifest_missing_platforms() {
        let json = br#"{ "version": "1.0.0" }"#;
        let err = parse_manifest(json).unwrap_err();
        assert!(
            err.contains("platforms"),
            "error mentions 'platforms': {err}"
        );
    }

    #[test]
    fn parse_manifest_missing_size() {
        let json = br#"{
            "version": "1.0.0",
            "platforms": {
                "darwin-arm64": { "checksum": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" }
            }
        }"#;
        let err = parse_manifest(json).unwrap_err();
        assert!(err.contains("size"), "error mentions 'size': {err}");
    }

    #[test]
    fn parse_manifest_missing_checksum() {
        let json = br#"{
            "version": "1.0.0",
            "platforms": {
                "darwin-arm64": { "size": 123 }
            }
        }"#;
        let err = parse_manifest(json).unwrap_err();
        assert!(err.contains("checksum"), "error mentions 'checksum': {err}");
    }

    #[test]
    fn parse_manifest_invalid_checksum_length() {
        let json = br#"{
            "version": "1.0.0",
            "platforms": {
                "darwin-arm64": { "size": 123, "checksum": "tooshort" }
            }
        }"#;
        let err = parse_manifest(json).unwrap_err();
        assert!(err.contains("64"), "error mentions length: {err}");
    }

    #[test]
    fn parse_manifest_non_hex_checksum() {
        let json = br#"{
            "version": "1.0.0",
            "platforms": {
                "darwin-arm64": { "size": 123, "checksum": "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz" }
            }
        }"#;
        let err = parse_manifest(json).unwrap_err();
        assert!(err.contains("non-hex"), "error mentions non-hex: {err}");
    }

    #[test]
    fn parse_manifest_negative_size() {
        let json = br#"{
            "version": "1.0.0",
            "platforms": {
                "darwin-arm64": { "size": -1, "checksum": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" }
            }
        }"#;
        let err = parse_manifest(json).unwrap_err();
        assert!(err.contains("size"), "error mentions size: {err}");
    }

    #[test]
    fn parse_manifest_invalid_json() {
        let err = parse_manifest(b"not json").unwrap_err();
        assert!(err.contains("JSON"), "error mentions JSON: {err}");
    }

    #[test]
    fn parse_manifest_empty_json() {
        let err = parse_manifest(b"{}").unwrap_err();
        assert!(err.contains("version"), "error mentions version: {err}");
    }

    #[test]
    fn parse_manifest_official_format_with_extra_fields() {
        // Official manifest format uses `checksum` and may include
        // `binary`, `commit`, `buildDate`, and `sdkCompat` fields.
        let json = br#"{
            "version": "0.42.0",
            "platforms": {
                "darwin-arm64": {
                    "size": 9876543,
                    "checksum": "fedcba0987654321fedcba0987654321fedcba0987654321fedcba0987654321",
                    "binary": "claude-0.42.0-darwin-arm64",
                    "commit": "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2",
                    "buildDate": "2026-07-10T12:00:00Z",
                    "sdkCompat": "0.5.0"
                },
                "linux-x64": {
                    "size": 1111111,
                    "checksum": "1111111111111111111111111111111111111111111111111111111111111111"
                }
            }
        }"#;

        let manifest = parse_manifest(json).unwrap();
        assert_eq!(manifest.version, "0.42.0");

        let entry = manifest.entry_for_platform("darwin-arm64").unwrap();
        assert_eq!(entry.size, 9_876_543);
        assert_eq!(
            entry.sha256,
            "fedcba0987654321fedcba0987654321fedcba0987654321fedcba0987654321"
        );

        let linux_entry = manifest.entry_for_platform("linux-x64").unwrap();
        assert_eq!(linux_entry.size, 1_111_111);
        assert_eq!(
            linux_entry.sha256,
            "1111111111111111111111111111111111111111111111111111111111111111"
        );
    }

    #[test]
    fn validate_sha256_rejects_short_string() {
        assert!(validate_sha256("abc").is_err());
    }

    #[test]
    fn validate_sha256_rejects_non_hex() {
        let hex = "z".repeat(64);
        assert!(validate_sha256(&hex).is_err());
    }

    #[test]
    fn validate_sha256_accepts_valid() {
        let hex = "a".repeat(64);
        assert!(validate_sha256(&hex).is_ok());
    }

    // ======================================================================
    // Phase 4: Path planning (version-based)
    // ======================================================================

    #[test]
    fn plan_install_path_uses_db_parent_and_version() {
        let tmp = TempDir::new().unwrap();
        let db_path = tmp.path().join("cowboy.db");
        std::fs::write(&db_path, b"fake db").unwrap();

        let paths = plan_install_path(&db_path, "1.2.3", "claude").unwrap();
        assert_eq!(paths.install_dir, tmp.path().join("versions").join("1.2.3"));
        assert_eq!(
            paths.target_bin,
            tmp.path().join("versions").join("1.2.3").join("claude")
        );
        assert_eq!(
            paths.temp_bin,
            tmp.path()
                .join("versions")
                .join("1.2.3")
                .join(".claude.download")
        );
    }

    #[test]
    fn plan_install_path_windows_uses_claude_exe() {
        let tmp = TempDir::new().unwrap();
        let db_path = tmp.path().join("cowboy.db");
        std::fs::write(&db_path, b"fake db").unwrap();

        let paths = plan_install_path(&db_path, "2.0.0", "claude.exe").unwrap();
        assert_eq!(
            paths.target_bin,
            tmp.path().join("versions").join("2.0.0").join("claude.exe")
        );
        assert_eq!(
            paths.temp_bin,
            tmp.path()
                .join("versions")
                .join("2.0.0")
                .join(".claude.exe.download")
        );
    }

    #[test]
    fn plan_install_path_no_longer_depends_on_current_exe() {
        // The function signature takes db_path, not exe_path.
        // Any path with a valid parent works — no exe required.
        let tmp = TempDir::new().unwrap();
        let db_path = tmp.path().join("some.db");
        std::fs::write(&db_path, b"db").unwrap();

        let paths = plan_install_path(&db_path, "0.1.0", "claude").unwrap();
        assert_eq!(paths.install_dir, tmp.path().join("versions").join("0.1.0"));
    }

    #[test]
    fn plan_install_path_errors_when_db_parent_missing() {
        let result = plan_install_path(Path::new("/nonexistent/dir/db.db"), "1.0.0", "claude");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("parent directory"),
            "error should mention parent directory: {err}"
        );
    }

    #[test]
    fn plan_install_path_rejects_empty_version() {
        let tmp = TempDir::new().unwrap();
        let db_path = tmp.path().join("cowboy.db");
        std::fs::write(&db_path, b"fake db").unwrap();

        let result = plan_install_path(&db_path, "", "claude");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("Invalid manifest version"),
            "error should mention invalid version: {err}"
        );
    }

    #[test]
    fn plan_install_path_rejects_dot_dot_version() {
        let tmp = TempDir::new().unwrap();
        let db_path = tmp.path().join("cowboy.db");
        std::fs::write(&db_path, b"fake db").unwrap();

        let result = plan_install_path(&db_path, "..", "claude");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("Invalid manifest version"),
            "error should mention invalid version: {err}"
        );
    }

    #[test]
    fn plan_install_path_rejects_version_with_slash() {
        let tmp = TempDir::new().unwrap();
        let db_path = tmp.path().join("cowboy.db");
        std::fs::write(&db_path, b"fake db").unwrap();

        let result = plan_install_path(&db_path, "1.0/../../etc", "claude");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("Invalid manifest version"),
            "error should mention invalid version: {err}"
        );
    }

    #[test]
    fn plan_install_path_rejects_version_with_backslash() {
        let tmp = TempDir::new().unwrap();
        let db_path = tmp.path().join("cowboy.db");
        std::fs::write(&db_path, b"fake db").unwrap();

        let result = plan_install_path(&db_path, "1.0\\evil", "claude");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("Invalid manifest version"),
            "error should mention invalid version: {err}"
        );
    }

    #[test]
    fn plan_install_path_rejects_dot_component() {
        let tmp = TempDir::new().unwrap();
        let db_path = tmp.path().join("cowboy.db");
        std::fs::write(&db_path, b"fake db").unwrap();

        // "." as a version component would resolve to current dir
        let result = plan_install_path(&db_path, ".", "claude");
        assert!(result.is_err());
    }

    #[test]
    fn plan_install_path_accepts_release_versions() {
        let tmp = TempDir::new().unwrap();
        let db_path = tmp.path().join("cowboy.db");
        std::fs::write(&db_path, b"fake db").unwrap();

        // Typical release versions should all be accepted
        for version in &["1.0.0", "0.42.0", "10.20.30", "1.0.0-beta.1"] {
            let paths = plan_install_path(&db_path, version, "claude").unwrap();
            assert_eq!(
                paths.install_dir,
                tmp.path().join("versions").join(version),
                "version {version} should be accepted"
            );
        }
    }

    // ======================================================================
    // Phase 5: Download, verify, and atomic replace
    // ======================================================================

    #[test]
    fn verify_binary_matches() {
        let data = b"hello world";
        let size = data.len() as u64;
        // SHA-256 of "hello world"
        let sha256 = "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";
        verify_binary(data, size, sha256).unwrap();
    }

    #[test]
    fn verify_binary_size_mismatch() {
        let data = b"hello world";
        let err = verify_binary(
            data,
            999,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9",
        )
        .unwrap_err();
        assert!(
            err.contains("Size mismatch"),
            "error mentions size mismatch: {err}"
        );
    }

    #[test]
    fn verify_binary_sha256_mismatch() {
        let data = b"hello world";
        let fake_sha256 = "a".repeat(64);
        let err = verify_binary(data, 11, &fake_sha256).unwrap_err();
        assert!(err.contains("SHA-256"), "error mentions SHA-256: {err}");
    }

    #[test]
    fn verify_binary_invalid_sha256_string() {
        let data = b"hello";
        let err = verify_binary(data, 5, "short").unwrap_err();
        assert!(err.contains("64"), "error mentions length: {err}");
    }

    #[test]
    fn atomic_replace_unix_replaces_target() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("target");
        let source = tmp.path().join("source");

        std::fs::write(&target, b"old content").unwrap();
        std::fs::write(&source, b"new content").unwrap();

        atomic_replace(&target, &source).unwrap();

        assert_eq!(std::fs::read_to_string(&target).unwrap(), "new content");
        assert!(!source.exists(), "source should be gone after rename");
    }

    #[test]
    fn atomic_replace_creates_new_file_when_target_missing() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("new-file");
        let source = tmp.path().join("source");

        std::fs::write(&source, b"data").unwrap();

        atomic_replace(&target, &source).unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "data");
    }

    // ======================================================================
    // Phase 6: Full install flow (with mock HTTP fetch)
    // ======================================================================

    /// Build a fake manifest JSON for the given platform with version "9.9.9".
    fn fake_manifest_json(platform: &str, sha256: &str, size: u64) -> Vec<u8> {
        fake_manifest_json_versioned("9.9.9", platform, sha256, size)
    }

    /// Build a fake manifest JSON for the given platform and version.
    fn fake_manifest_json_versioned(
        version: &str,
        platform: &str,
        sha256: &str,
        size: u64,
    ) -> Vec<u8> {
        format!(
            r#"{{
                "version": "{version}",
                "platforms": {{
                    "{platform}": {{
                        "size": {size},
                        "checksum": "{sha256}"
                    }}
                }}
            }}"#
        )
        .into_bytes()
    }

    /// Build a mock HTTP fetcher that returns manifest and binary.
    fn mock_fetch(
        manifest_body: Vec<u8>,
        binary_body: Vec<u8>,
    ) -> impl Fn(&str, u64) -> Result<Vec<u8>, String> {
        move |url: &str, _max_size: u64| {
            if url.contains("manifest.json") {
                Ok(manifest_body.clone())
            } else if url.contains("claude-") {
                Ok(binary_body.clone())
            } else {
                Err(format!("unexpected URL: {url}"))
            }
        }
    }

    /// Helper to create a temporary ClaudeEnvStore for testing.
    fn temp_env_store() -> (ClaudeEnvStore, TempDir) {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test.db");
        let store = ClaudeEnvStore::new(db_path);
        store.initialize().unwrap();
        store.seed_default_settings().unwrap();
        (store, dir)
    }

    #[test]
    fn install_success_does_not_save_alias_before_confirmation() {
        let (store, _store_dir) = temp_env_store();

        let binary_body = b"fake claude binary".to_vec();
        let sha256: String = {
            use sha2::{Digest, Sha256};
            format!("{:x}", Sha256::digest(&binary_body))
        };
        let size = binary_body.len() as u64;

        // The platform string must match the test environment.
        // We detect it using the same logic the real code uses.
        let libc = if cfg!(target_env = "musl") {
            Some("musl")
        } else {
            None
        };
        let platform = detect_platform(std::env::consts::OS, std::env::consts::ARCH, libc).unwrap();
        let manifest = fake_manifest_json(&platform, &sha256, size);
        let fetch = mock_fetch(manifest, binary_body.clone());

        do_install(&store, fetch).unwrap();

        // Installing alone must not change the alias before user confirmation.
        let alias = store.claude_command_alias().unwrap();
        assert_eq!(alias, "claude");

        // Verify the binary was installed under versions/<version>/
        let file_name = target_file_name(&platform);
        let app_data_dir = store.path().parent().unwrap();
        let installed_path = app_data_dir.join("versions").join("9.9.9").join(file_name);
        assert!(
            installed_path.exists(),
            "binary should be installed at {}",
            installed_path.display()
        );
        assert_eq!(
            std::fs::read(&installed_path).unwrap(),
            binary_body,
            "installed binary content should match"
        );

        // Check permissions on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = std::fs::metadata(&installed_path).unwrap();
            assert!(
                meta.permissions().mode() & 0o111 != 0,
                "binary should be executable"
            );
        }
    }

    #[test]
    fn confirming_alias_saves_installed_binary_path() {
        let (store, _store_dir) = temp_env_store();
        let installed_path = PathBuf::from("/tmp/claude");
        let mut input = "y\n".as_bytes();
        let mut output = Vec::new();

        confirm_alias(&store, &installed_path, &mut input, &mut output).unwrap();

        assert_eq!(
            store.claude_command_alias().unwrap(),
            installed_path.to_string_lossy()
        );
    }

    #[test]
    fn declining_alias_preserves_existing_alias() {
        let (store, _store_dir) = temp_env_store();
        store
            .upsert_setting(&Setting {
                key: SETTING_CLAUDE_COMMAND_ALIAS.to_string(),
                value: "my-claude".to_string(),
            })
            .unwrap();
        let mut input = "n\n".as_bytes();
        let mut output = Vec::new();

        confirm_alias(&store, Path::new("/tmp/claude"), &mut input, &mut output).unwrap();

        assert_eq!(store.claude_command_alias().unwrap(), "my-claude");
    }

    #[test]
    fn install_failure_does_not_change_old_alias() {
        let (store, _store_dir) = temp_env_store();

        // Set an existing alias first
        let old_alias = "claude".to_string();
        store
            .upsert_setting(&Setting {
                key: SETTING_CLAUDE_COMMAND_ALIAS.to_string(),
                value: old_alias.clone(),
            })
            .unwrap();

        // Make the fetch return an error to simulate download failure
        let fetch = |_url: &str, _max_size: u64| Err("network error".to_string());

        let result = do_install(&store, fetch);
        assert!(result.is_err(), "install should fail");

        // Alias should still be the old value
        let alias = store.claude_command_alias().unwrap();
        assert_eq!(alias, old_alias);
    }

    #[test]
    fn install_failure_due_to_checksum_does_not_install_binary() {
        let (store, _store_dir) = temp_env_store();

        let binary_body = b"fake claude binary".to_vec();
        let wrong_sha256 = "a".repeat(64); // wrong checksum
        let size = binary_body.len() as u64;

        let libc = if cfg!(target_env = "musl") {
            Some("musl")
        } else {
            None
        };
        let platform = detect_platform(std::env::consts::OS, std::env::consts::ARCH, libc).unwrap();
        let manifest = fake_manifest_json(&platform, &wrong_sha256, size);
        let fetch = mock_fetch(manifest, binary_body.clone());

        let result = do_install(&store, fetch);
        assert!(
            result.is_err(),
            "install should fail due to SHA-256 mismatch"
        );

        // The binary should NOT be installed under versions/
        let file_name = target_file_name(&platform);
        let app_data_dir = store.path().parent().unwrap();
        let installed_path = app_data_dir.join("versions").join("9.9.9").join(file_name);
        assert!(!installed_path.exists(), "binary should not be installed");

        // Temp file should have been cleaned up
        let version_dir = app_data_dir.join("versions").join("9.9.9");
        let temp_path = version_dir.join(format!(".{file_name}.download"));
        assert!(!temp_path.exists(), "temp file should be cleaned up");
    }

    #[test]
    fn install_with_sha256_size_mismatch_cleans_up_temp() {
        let (store, _store_dir) = temp_env_store();

        let binary_body = b"fake claude binary".to_vec();
        let sha256: String = {
            use sha2::{Digest, Sha256};
            format!("{:x}", Sha256::digest(&binary_body))
        };

        let libc = if cfg!(target_env = "musl") {
            Some("musl")
        } else {
            None
        };
        let platform = detect_platform(std::env::consts::OS, std::env::consts::ARCH, libc).unwrap();
        let manifest = fake_manifest_json(&platform, &sha256, 999_999_999); // wrong size
        let fetch = mock_fetch(manifest, binary_body.clone());

        let result = do_install(&store, fetch);
        assert!(result.is_err(), "install should fail due to size mismatch");

        let file_name = target_file_name(&platform);
        let app_data_dir = store.path().parent().unwrap();
        let version_dir = app_data_dir.join("versions").join("9.9.9");
        let temp_path = version_dir.join(format!(".{file_name}.download"));
        assert!(!temp_path.exists(), "temp file should be cleaned up");
    }

    // ======================================================================
    // URL construction
    // ======================================================================

    #[test]
    fn binary_download_url_unix() {
        let url = binary_download_url("1.2.3", "darwin-arm64");
        assert_eq!(
            url,
            "https://github.com/coniferous-cmd/claude-shop/releases/latest/download/claude-1.2.3-darwin-arm64"
        );
        assert!(!url.ends_with(".exe"));
    }

    #[test]
    fn binary_download_url_windows() {
        let url = binary_download_url("1.2.3", "win32-x64");
        assert_eq!(
            url,
            "https://github.com/coniferous-cmd/claude-shop/releases/latest/download/claude-1.2.3-win32-x64.exe"
        );
    }

    #[test]
    fn manifest_download_url_format() {
        let url = manifest_download_url();
        assert!(url.contains("manifest.json"));
        assert!(url.contains("coniferous-cmd/claude-shop"));
    }

    // ======================================================================
    // SHA-256 hex helper
    // ======================================================================

    #[test]
    fn sha256_hex_to_bytes_valid() {
        let hex = "a94a8fe5ccb19ba61c4c0873d391e987982fbbd3";
        // Pad to 64 chars
        let hex = format!("{:0>64}", hex);
        let result = sha256_hex_to_bytes(&hex).unwrap();
        assert_eq!(result.len(), 32);
    }

    #[test]
    fn sha256_hex_to_bytes_invalid_length() {
        assert!(sha256_hex_to_bytes("abc").is_err());
    }

    #[test]
    fn sha256_hex_to_bytes_non_hex() {
        assert!(sha256_hex_to_bytes(&"z".repeat(64)).is_err());
    }

    // ======================================================================
    // default_fetch body size limiting
    // ======================================================================

    #[test]
    fn default_fetch_rejects_response_above_limit() {
        // Regression: default_fetch with a small limit rejects larger bodies.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();

        std::thread::scope(|s| {
            s.spawn(|| {
                ready_tx.send(()).unwrap();
                if let Ok((mut stream, _)) = listener.accept() {
                    let mut resp = Vec::new();
                    resp.extend_from_slice(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 200\r\n\
                          Connection: close\r\n\r\n",
                    );
                    resp.extend_from_slice(&[0xA5u8; 200]);
                    let _ = stream.write_all(&resp);
                    // Drop stream naturally — avoids Shutdown::Write RST on Windows.
                }
            });

            let _ = ready_rx.recv();
            let url = format!("http://127.0.0.1:{port}/test");
            let result = default_fetch(&url, 100); // limit 100 < body 200
            assert!(
                result.is_err(),
                "default_fetch should reject body exceeding limit"
            );
        });
    }

    #[test]
    fn default_fetch_succeeds_with_matched_limit() {
        // With limit matching body size, downloads succeed.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();

        std::thread::scope(|s| {
            s.spawn(|| {
                ready_tx.send(()).unwrap();
                if let Ok((mut stream, _)) = listener.accept() {
                    let mut resp = Vec::new();
                    resp.extend_from_slice(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 200\r\n\
                          Connection: close\r\n\r\n",
                    );
                    resp.extend_from_slice(&[0xA5u8; 200]);
                    let _ = stream.write_all(&resp);
                    // Drop stream naturally — avoids Shutdown::Write RST on Windows.
                }
            });

            let _ = ready_rx.recv();
            // ureq's LimitReader errors when body exactly equals limit,
            // so we pass limit = body_size + 1.
            let url = format!("http://127.0.0.1:{port}/test");
            let result = default_fetch(&url, 201);
            assert!(
                result.is_ok(),
                "default_fetch should succeed with limit > body size"
            );
            assert_eq!(result.unwrap().len(), 200);
        });
    }

    #[test]
    fn default_fetch_rejects_non_200_status() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();

        std::thread::scope(|s| {
            s.spawn(|| {
                ready_tx.send(()).unwrap();
                if let Ok((mut stream, _)) = listener.accept() {
                    let _ = stream.write_all(
                        b"HTTP/1.1 404 Not Found\r\n\
                          Content-Length: 0\r\n\
                          Connection: close\r\n\r\n",
                    );
                }
            });

            let _ = ready_rx.recv();
            let url = format!("http://127.0.0.1:{port}/notfound");
            let result = default_fetch(&url, 1024);
            assert!(result.is_err(), "default_fetch should reject 404");
            assert!(
                result.unwrap_err().contains("404"),
                "error should mention status"
            );
        });
    }

    // ======================================================================
    // Large binary downloads (>10 MiB)
    // ======================================================================

    #[test]
    fn install_success_large_binary() {
        let (store, _store_dir) = temp_env_store();

        // Binary larger than 10 MiB — exercises limit plumbing through do_install
        let binary_body = vec![0xABu8; 15 * 1024 * 1024]; // 15 MiB
        let sha256: String = {
            use sha2::{Digest, Sha256};
            format!("{:x}", Sha256::digest(&binary_body))
        };
        let size = binary_body.len() as u64;

        let libc = if cfg!(target_env = "musl") {
            Some("musl")
        } else {
            None
        };
        let platform = detect_platform(std::env::consts::OS, std::env::consts::ARCH, libc).unwrap();
        let manifest = fake_manifest_json(&platform, &sha256, size);
        let fetch = mock_fetch(manifest, binary_body.clone());

        do_install(&store, fetch).unwrap();

        let file_name = target_file_name(&platform);
        let app_data_dir = store.path().parent().unwrap();
        let installed_path = app_data_dir.join("versions").join("9.9.9").join(file_name);
        assert!(installed_path.exists(), "large binary should be installed");
        assert_eq!(
            std::fs::read(&installed_path).unwrap(),
            binary_body,
            "installed large binary content should match"
        );

        let alias = store.claude_command_alias().unwrap();
        assert_eq!(alias, "claude");
    }

    // ======================================================================
    // Phase 7: Version-based install behavior
    // ======================================================================

    /// Helper: compute the SHA-256 hex digest of data.
    fn sha256_hex(data: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        format!("{:x}", Sha256::digest(data))
    }

    /// Helper: detect the current platform string for tests.
    fn test_platform() -> String {
        let libc = if cfg!(target_env = "musl") {
            Some("musl")
        } else {
            None
        };
        detect_platform(std::env::consts::OS, std::env::consts::ARCH, libc).unwrap()
    }

    #[test]
    fn install_creates_version_directory_and_places_binary() {
        let (store, _dir) = temp_env_store();
        let platform = test_platform();
        let binary_body = b"v1 binary content";
        let sha256 = sha256_hex(binary_body);
        let manifest =
            fake_manifest_json_versioned("1.0.0", &platform, &sha256, binary_body.len() as u64);
        let fetch = mock_fetch(manifest, binary_body.to_vec());

        do_install(&store, fetch).unwrap();

        let file_name = target_file_name(&platform);
        let app_data_dir = store.path().parent().unwrap();
        let installed = app_data_dir.join("versions").join("1.0.0").join(file_name);
        assert!(
            installed.exists(),
            "binary should be at versions/1.0.0/{file_name}"
        );
        assert_eq!(std::fs::read(&installed).unwrap(), binary_body);
    }

    #[test]
    fn new_version_preserves_old_versions() {
        let (store, _dir) = temp_env_store();
        let platform = test_platform();
        let file_name = target_file_name(&platform);
        let app_data_dir = store.path().parent().unwrap();

        // Install v1
        let body_v1 = b"version 1 binary";
        let sha_v1 = sha256_hex(body_v1);
        let manifest_v1 =
            fake_manifest_json_versioned("1.0.0", &platform, &sha_v1, body_v1.len() as u64);
        do_install(&store, mock_fetch(manifest_v1, body_v1.to_vec())).unwrap();

        // Install v2
        let body_v2 = b"version 2 binary";
        let sha_v2 = sha256_hex(body_v2);
        let manifest_v2 =
            fake_manifest_json_versioned("2.0.0", &platform, &sha_v2, body_v2.len() as u64);
        do_install(&store, mock_fetch(manifest_v2, body_v2.to_vec())).unwrap();

        // Both versions should exist
        let v1_path = app_data_dir.join("versions").join("1.0.0").join(file_name);
        let v2_path = app_data_dir.join("versions").join("2.0.0").join(file_name);
        assert!(v1_path.exists(), "v1 should still exist");
        assert!(v2_path.exists(), "v2 should exist");
        assert_eq!(std::fs::read(&v1_path).unwrap(), body_v1);
        assert_eq!(std::fs::read(&v2_path).unwrap(), body_v2);
    }

    #[test]
    fn reinstall_same_version_replaces_atomically() {
        let (store, _dir) = temp_env_store();
        let platform = test_platform();
        let file_name = target_file_name(&platform);
        let app_data_dir = store.path().parent().unwrap();

        // Install v1 first time
        let body_old = b"old v1 binary";
        let sha_old = sha256_hex(body_old);
        let manifest_old =
            fake_manifest_json_versioned("1.0.0", &platform, &sha_old, body_old.len() as u64);
        do_install(&store, mock_fetch(manifest_old, body_old.to_vec())).unwrap();

        // Reinstall v1 with different content
        let body_new = b"new v1 binary content updated";
        let sha_new = sha256_hex(body_new);
        let manifest_new =
            fake_manifest_json_versioned("1.0.0", &platform, &sha_new, body_new.len() as u64);
        do_install(&store, mock_fetch(manifest_new, body_new.to_vec())).unwrap();

        let installed = app_data_dir.join("versions").join("1.0.0").join(file_name);
        assert_eq!(
            std::fs::read(&installed).unwrap(),
            body_new,
            "reinstall should replace with new content"
        );
    }

    #[test]
    fn install_does_not_modify_alias() {
        let (store, _dir) = temp_env_store();
        let platform = test_platform();
        let binary_body = b"binary";
        let sha256 = sha256_hex(binary_body);
        let manifest =
            fake_manifest_json_versioned("1.0.0", &platform, &sha256, binary_body.len() as u64);

        do_install(&store, mock_fetch(manifest, binary_body.to_vec())).unwrap();

        // Alias should remain unchanged (default "claude")
        assert_eq!(store.claude_command_alias().unwrap(), "claude");
    }

    #[test]
    fn alias_saves_versioned_path_after_confirmation() {
        let (store, _dir) = temp_env_store();
        let platform = test_platform();
        let file_name = target_file_name(&platform);
        let app_data_dir = store.path().parent().unwrap();

        let binary_body = b"binary";
        let sha256 = sha256_hex(binary_body);
        let manifest =
            fake_manifest_json_versioned("1.0.0", &platform, &sha256, binary_body.len() as u64);
        do_install(&store, mock_fetch(manifest, binary_body.to_vec())).unwrap();

        let expected_path = app_data_dir.join("versions").join("1.0.0").join(file_name);

        // Simulate user confirming alias
        let mut input = "y\n".as_bytes();
        let mut output = Vec::new();
        confirm_alias(&store, &expected_path, &mut input, &mut output).unwrap();

        assert_eq!(
            store.claude_command_alias().unwrap(),
            expected_path.to_string_lossy()
        );
    }

    #[test]
    fn alias_decline_preserves_original() {
        let (store, _dir) = temp_env_store();
        store
            .upsert_setting(&Setting {
                key: SETTING_CLAUDE_COMMAND_ALIAS.to_string(),
                value: "my-custom-claude".to_string(),
            })
            .unwrap();

        let mut input = "n\n".as_bytes();
        let mut output = Vec::new();
        confirm_alias(&store, Path::new("/tmp/claude"), &mut input, &mut output).unwrap();

        assert_eq!(store.claude_command_alias().unwrap(), "my-custom-claude");
    }

    #[test]
    fn download_failure_does_not_create_target() {
        let (store, _dir) = temp_env_store();
        let platform = test_platform();
        let file_name = target_file_name(&platform);
        let app_data_dir = store.path().parent().unwrap();

        let fetch = |_url: &str, _max_size: u64| Err("network error".to_string());
        let result = do_install(&store, fetch);
        assert!(result.is_err());

        let version_dir = app_data_dir.join("versions").join("9.9.9");
        assert!(
            !version_dir.join(file_name).exists(),
            "no target should be created"
        );
    }

    #[test]
    fn invalid_manifest_does_not_create_target() {
        let (store, _dir) = temp_env_store();
        let app_data_dir = store.path().parent().unwrap();

        let fetch = |url: &str, _max_size: u64| {
            if url.contains("manifest") {
                Ok(b"not valid json".to_vec())
            } else {
                Ok(b"binary".to_vec())
            }
        };
        let result = do_install(&store, fetch);
        assert!(result.is_err());

        assert!(
            !app_data_dir.join("versions").exists(),
            "no versions dir should be created for invalid manifest"
        );
    }

    #[test]
    fn checksum_mismatch_does_not_corrupt_existing_version() {
        let (store, _dir) = temp_env_store();
        let platform = test_platform();
        let file_name = target_file_name(&platform);
        let app_data_dir = store.path().parent().unwrap();

        // First, install v1 successfully
        let body_good = b"good binary";
        let sha_good = sha256_hex(body_good);
        let manifest_good =
            fake_manifest_json_versioned("1.0.0", &platform, &sha_good, body_good.len() as u64);
        do_install(&store, mock_fetch(manifest_good, body_good.to_vec())).unwrap();

        // Now try to reinstall v1 with bad checksum
        let body_bad = b"bad binary data";
        let wrong_sha = "a".repeat(64);
        let manifest_bad =
            fake_manifest_json_versioned("1.0.0", &platform, &wrong_sha, body_bad.len() as u64);
        let result = do_install(&store, mock_fetch(manifest_bad, body_bad.to_vec()));
        assert!(result.is_err());

        // The original good binary must still be intact
        let installed = app_data_dir.join("versions").join("1.0.0").join(file_name);
        assert!(installed.exists(), "old binary should still exist");
        assert_eq!(
            std::fs::read(&installed).unwrap(),
            body_good,
            "old binary content should be unchanged"
        );
    }

    #[test]
    fn failure_cleans_up_temp_file() {
        let (store, _dir) = temp_env_store();
        let platform = test_platform();
        let file_name = target_file_name(&platform);
        let app_data_dir = store.path().parent().unwrap();

        let body = b"binary";
        let sha256 = sha256_hex(body);
        // Wrong size to trigger verification failure
        let manifest = fake_manifest_json_versioned("1.0.0", &platform, &sha256, 999_999);
        let result = do_install(&store, mock_fetch(manifest, body.to_vec()));
        assert!(result.is_err());

        let version_dir = app_data_dir.join("versions").join("1.0.0");
        let temp_file = version_dir.join(format!(".{file_name}.download"));
        assert!(!temp_file.exists(), "temp file should be cleaned up");
    }

    #[test]
    fn failure_preserves_other_versions() {
        let (store, _dir) = temp_env_store();
        let platform = test_platform();
        let file_name = target_file_name(&platform);
        let app_data_dir = store.path().parent().unwrap();

        // Install v1 successfully
        let body_v1 = b"v1 binary";
        let sha_v1 = sha256_hex(body_v1);
        let manifest_v1 =
            fake_manifest_json_versioned("1.0.0", &platform, &sha_v1, body_v1.len() as u64);
        do_install(&store, mock_fetch(manifest_v1, body_v1.to_vec())).unwrap();

        // Try to install v2 with bad checksum
        let body_v2 = b"v2 binary";
        let wrong_sha = "b".repeat(64);
        let manifest_v2 =
            fake_manifest_json_versioned("2.0.0", &platform, &wrong_sha, body_v2.len() as u64);
        let result = do_install(&store, mock_fetch(manifest_v2, body_v2.to_vec()));
        assert!(result.is_err());

        // v1 should be untouched
        let v1_path = app_data_dir.join("versions").join("1.0.0").join(file_name);
        assert!(v1_path.exists(), "v1 should still exist");
        assert_eq!(std::fs::read(&v1_path).unwrap(), body_v1);

        // v2 should not have a binary (only v1 remains)
        let v2_path = app_data_dir.join("versions").join("2.0.0").join(file_name);
        assert!(
            !v2_path.exists(),
            "v2 binary should not exist after failed install"
        );
    }

    #[test]
    fn installed_binary_has_exec_permissions() {
        let (store, _dir) = temp_env_store();
        let platform = test_platform();
        let binary_body = b"executable content";
        let sha256 = sha256_hex(binary_body);
        let manifest =
            fake_manifest_json_versioned("1.0.0", &platform, &sha256, binary_body.len() as u64);

        do_install(&store, mock_fetch(manifest, binary_body.to_vec())).unwrap();

        let file_name = target_file_name(&platform);
        let app_data_dir = store.path().parent().unwrap();
        let installed = app_data_dir.join("versions").join("1.0.0").join(file_name);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&installed).unwrap().permissions().mode();
            assert!(
                mode & 0o111 != 0,
                "binary should be executable (mode: {mode:#o})"
            );
        }
    }

    #[test]
    fn install_large_binary_to_versioned_path() {
        let (store, _dir) = temp_env_store();
        let platform = test_platform();

        let binary_body = vec![0xCDu8; 15 * 1024 * 1024]; // 15 MiB
        let sha256 = sha256_hex(&binary_body);
        let manifest =
            fake_manifest_json_versioned("3.0.0", &platform, &sha256, binary_body.len() as u64);

        do_install(&store, mock_fetch(manifest, binary_body.clone())).unwrap();

        let file_name = target_file_name(&platform);
        let app_data_dir = store.path().parent().unwrap();
        let installed = app_data_dir.join("versions").join("3.0.0").join(file_name);
        assert!(installed.exists(), "large binary should be installed");
        assert_eq!(std::fs::read(&installed).unwrap(), binary_body);
    }
}
