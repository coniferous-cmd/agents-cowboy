//! Shared `$EDITOR`-on-temp-file profile JSON editor.
//!
//! Both the CLI (`config edit`) and the TUI (new-profile creation) share this
//! implementation rather than duplicating it.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crossterm::{
    cursor::{Hide, MoveTo, Show},
    terminal::{
        disable_raw_mode, enable_raw_mode, Clear, ClearType, EnterAlternateScreen,
        LeaveAlternateScreen,
    },
};

// ── Public API ────────────────────────────────────────────────────────────────

/// Outcome of an interactive `$EDITOR` session on a profile's settings JSON.
#[derive(Debug, PartialEq, Eq)]
pub enum EditOutcome {
    /// Editor saved; carries the new JSON text.
    Saved(String),
    /// `$EDITOR` is unset or empty.
    NoEditorConfigured,
    /// Editor process exited non-zero; `String` describes the error and the
    /// temp file where edits were preserved.
    EditorExitedWithError(String),
    /// The saved file was not valid JSON (or root was not a JSON object);
    /// `temp_file` points to the preserved file.
    ValidationError { error: String, temp_file: PathBuf },
}

/// Launch `$EDITOR` on a temp file seeded with `raw_json`.  On a successful
/// editor exit the file is read back, validated as a JSON object, and returned
/// as `EditOutcome::Saved`.  All other paths (no editor, non-zero exit,
/// invalid JSON) are returned as the corresponding `EditOutcome` variant so the
/// caller can decide how to surface them.
///
/// The editor has a 60-second timeout.  If it exceeds this, the process is
/// killed and `EditOutcome::EditorExitedWithError` is returned.
pub fn edit_profile_json(raw_json: &str) -> Result<EditOutcome, String> {
    let editor = match std::env::var("EDITOR") {
        Ok(value) if !value.trim().is_empty() => value,
        _ => return Ok(EditOutcome::NoEditorConfigured),
    };
    let temp_path = create_private_temp_file(raw_json.as_bytes())?;

    // Spawn editor as a child process (not .status() which blocks indefinitely)
    let mut child = spawn_editor(&editor, &temp_path).map_err(|error| {
        format!(
            "Failed to launch editor for {}: {error}",
            temp_path.display()
        )
    })?;

    let child_id = child.id();

    // Wait with timeout using a separate thread
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let result = child.wait();
        let _ = tx.send(result);
    });

    // Wait for either editor exit or timeout (60 seconds)
    let timeout = Duration::from_secs(60);
    match rx.recv_timeout(timeout) {
        Ok(Ok(status)) => {
            // Editor exited (success or failure)
            if !status.success() {
                return Ok(EditOutcome::EditorExitedWithError(format!(
                    "Editor exited with status {status}; edits preserved in {}",
                    temp_path.display()
                )));
            }
        }
        Ok(Err(error)) => {
            return Err(format!("Failed to wait for editor: {error}"));
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            // Timeout — kill the editor process
            kill_process(child_id);
            return Ok(EditOutcome::EditorExitedWithError(format!(
                "Editor timed out after 60 seconds; edits preserved in {}",
                temp_path.display()
            )));
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            // Thread panicked — should not happen
            return Err("Editor thread panicked unexpectedly".to_string());
        }
    }

    let edited = fs::read_to_string(&temp_path)
        .map_err(|error| format!("Failed to read {}: {error}", temp_path.display()))?;
    let validation = serde_json::from_str::<serde_json::Value>(&edited)
        .map_err(|error| error.to_string())
        .and_then(|value| {
            if value.is_object() {
                Ok(())
            } else {
                Err("settings root must be a JSON object".to_string())
            }
        });

    if let Err(error) = validation {
        return Ok(EditOutcome::ValidationError {
            error,
            temp_file: temp_path,
        });
    }

    fs::remove_file(&temp_path)
        .map_err(|error| format!("Failed to remove {}: {error}", temp_path.display()))?;
    Ok(EditOutcome::Saved(edited))
}

// ── Private helpers ───────────────────────────────────────────────────────────

fn create_private_temp_file(contents: &[u8]) -> Result<PathBuf, String> {
    let temp_dir = std::env::temp_dir();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();

    for sequence in 0u32..100 {
        let path = temp_dir.join(format!(
            "claude-cowboy-profile-{}-{nanos}-{sequence}.json",
            std::process::id()
        ));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&path) {
            Ok(mut file) => {
                file.write_all(contents)
                    .map_err(|error| format!("Failed to write {}: {error}", path.display()))?;
                file.sync_all()
                    .map_err(|error| format!("Failed to sync {}: {error}", path.display()))?;
                return Ok(path);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(format!("Failed to create profile edit file: {error}"));
            }
        }
    }

    Err("Failed to create a unique profile edit file".to_string())
}

fn spawn_editor(editor: &str, path: &Path) -> std::io::Result<std::process::Child> {
    #[cfg(unix)]
    {
        Command::new("sh")
            .arg("-c")
            .arg("exec $EDITOR \"$1\"")
            .arg("claude-cowboy-editor")
            .arg(path)
            .env("EDITOR", editor)
            .spawn()
    }
    #[cfg(windows)]
    {
        Command::new("cmd")
            .arg("/C")
            .arg(format!("{editor} \"{}\"", path.display()))
            .spawn()
    }
}

fn kill_process(child_id: u32) {
    #[cfg(unix)]
    {
        // SAFETY: We're sending SIGTERM to a process we own
        unsafe {
            libc::kill(child_id as i32, libc::SIGTERM);
        }
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Threading::{
            OpenProcess, TerminateProcess, PROCESS_TERMINATE,
        };
        // SAFETY: We're terminating a process we own
        unsafe {
            let handle = OpenProcess(PROCESS_TERMINATE, 0, child_id);
            if !handle.is_null() {
                TerminateProcess(handle, 1);
                windows_sys::Win32::Foundation::CloseHandle(handle);
            }
        }
    }
}

// ── Terminal-state wrapper for the TUI ───────────────────────────────────────
//
// Background: the prior three commits tried to fix garbled-TUI-after-editor
// by adding `stty sane` and screen-clears. They didn't help because
// `stty sane` only resets the *line discipline* (cooked-mode flags); it
// never touches the terminal's escape-sequence state machine. After vim
// exits, the terminal can still be holding vim's G0 character set switch
// (`\x1b(0` / DEC Special Graphics), its SGR attributes, mouse-tracking
// mode, and bracketed-paste mode. The next ratatui draw then renders
// box-drawing characters into the wrong charset and flickers between
// frames whose internal buffer and on-screen buffer no longer agree.
//
// The wrapper below snapshots the line discipline with `stty -g`,
// hands control to vim, then resets *everything* before re-entering
// the TUI: line discipline is restored from the snapshot (more precise
// than `stty sane`), and the escape-sequence state machine is wiped via
// the explicit sequences in `write_terminal_reset_escapes`.

/// TUI-aware wrapper around [`edit_profile_json`] that manages the full
/// terminal lifecycle around the editor invocation. This is the function
/// the TUI calls.
///
/// Pipeline:
///   1. Snapshot terminal line discipline via `stty -g` (Unix).
///   2. Exit raw mode, leave the alternate screen, show the cursor.
///   3. Run the editor (delegates to the lower-level helper, preserving
///      its 60 s timeout, JSON validation, and temp-file behavior).
///   4. Write the reset escape sequences described in
///      [`write_terminal_reset_escapes`] — clears everything vim may
///      have switched (character set, SGR, mouse tracking, bracketed
///      paste).
///   5. Restore the snapshotted line discipline (more precise than
///      `stty sane`, which only restores defaults).
///   6. Re-enter raw mode and the alternate screen, then wipe and
///      recenter.
pub fn edit_profile_json_with_terminal_reset(raw_json: &str) -> Result<EditOutcome, String> {
    // 1. Snapshot terminal state (Unix only — Windows has no `stty`).
    #[cfg(unix)]
    let saved_state = save_terminal_state();
    #[cfg(not(unix))]
    let saved_state = ();

    // 2. Leave the TUI so the editor can take over the main screen.
    // `LeaveAlternateScreen` only *switches* the buffer — it does NOT clear
    // its content. The shell inherits a dirty alt-screen buffer, and on
    // re-entry `EnterAlternateScreen` reveals that stale content.  We must
    // clear the alt screen BEFORE we switch away from it (while it's still
    // the active buffer). The clear + move is the last thing ratatui drew,
    // so wiping it prevents garbage from flashing on re-entry.
    let mut stdout = std::io::stdout();
    let _ = disable_raw_mode();
    let _ = crossterm::execute!(stdout, Clear(ClearType::All), MoveTo(0, 0));
    let _ = crossterm::execute!(stdout, LeaveAlternateScreen, Show);
    let _ = stdout.flush();

    // 3. Run the editor.
    let result = edit_profile_json(raw_json);

    // 4. Wipe vim's residual escape-sequence state.
    if let Err(error) = write_terminal_reset_escapes(&mut stdout) {
        eprintln!("Warning: failed to write terminal reset escapes: {error}");
    }
    let _ = stdout.flush();

    // 5. Restore the snapshotted line discipline (no-op on Windows).
    #[cfg(unix)]
    if let Some(state) = saved_state {
        if let Err(error) = restore_terminal_state(&state) {
            eprintln!("Warning: failed to restore terminal state: {error}");
        }
    }

    // 6. Re-enter the TUI.
    let _ = enable_raw_mode();
    let _ = crossterm::execute!(
        stdout,
        EnterAlternateScreen,
        Hide,
        Clear(ClearType::All),
        MoveTo(0, 0)
    );
    let _ = stdout.flush();

    result
}

/// Write the explicit escape sequences that reset everything a full-screen
/// editor (vim, nvim, nano with line-drawing, etc.) might have switched
/// while it owned the terminal. Kept as a free function that takes any
/// `impl Write` so it can be tested without touching the real terminal.
///
/// Each sequence is documented inline because future readers will wonder
/// "why is this here?" — the answer is always "vim toggled it".
pub(crate) fn write_terminal_reset_escapes(sink: &mut impl Write) -> std::io::Result<()> {
    // G0 → USASCII — restores the default character set after vim may
    //   have switched to DEC Special Graphics for line-drawing glyphs.
    sink.write_all(b"\x1b(B")?;
    // Designate UTF-8 mode — `man terminfo` lists this as the way to
    //   ask the terminal to interpret subsequent output as UTF-8.
    sink.write_all(b"\x1b%G")?;
    // Restore G1 to USASCII. `ESC ) 0` does the opposite: it designates
    // DEC Special Graphics and is itself enough to cause garbled text when
    // the editor leaves G1 selected.
    sink.write_all(b"\x1b)B")?;
    // Shift in to G0 in case the editor exited while G1 was active.
    sink.write_all(b"\x0f")?;
    // Reset SGR — clears color, bold, underline, reverse, blink, etc.
    sink.write_all(b"\x1b[0m")?;
    // Show the cursor and enable line wrap (matches a fresh terminal).
    sink.write_all(b"\x1b[?25h")?;
    sink.write_all(b"\x1b[?7h")?;
    // Hide the cursor again — the TUI takes over cursor control.
    sink.write_all(b"\x1b[?25l")?;
    // Disable mouse-tracking modes vim may have enabled (normal,
    //   SGR-encoded, and any-event variants).
    sink.write_all(b"\x1b[?1000l")?;
    sink.write_all(b"\x1b[?1006l")?;
    sink.write_all(b"\x1b[?1015l")?;
    // Disable bracketed paste — vim enables this for safe pasting.
    sink.write_all(b"\x1b[?2004l")?;
    // Belt-and-suspenders: force-exit the alternate screen in case the
    //   terminal tracked it differently than expected.
    sink.write_all(b"\x1b[?1049l")?;
    // Newline so a line-buffered stdout (e.g. when piped) actually
    //   delivers the bytes.
    sink.write_all(b"\n")?;
    sink.flush()
}

/// Save the current terminal line-discipline state as a string suitable
/// for passing back to `stty <state>`. Returns `None` if `stty -g` fails
/// (e.g. when stdin/stdout isn't a tty in a test harness); callers
/// should treat that as "no snapshot, fall back to whatever restoration
/// path remains".
#[cfg(unix)]
fn save_terminal_state() -> Option<String> {
    Command::new("stty")
        .arg("-g")
        .output()
        .ok()
        .and_then(|output| {
            if output.status.success() {
                String::from_utf8(output.stdout)
                    .ok()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
            } else {
                None
            }
        })
}

/// Restore a previously-snapshotted terminal state. The argument must be
/// a value previously produced by [`save_terminal_state`] on a
/// compatible `stty`.
#[cfg(unix)]
fn restore_terminal_state(snapshot: &str) -> std::io::Result<()> {
    let status = Command::new("stty").arg(snapshot).status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "stty exited with status {status}"
        )))
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::{edit_profile_json, EditOutcome};
    use std::sync::Mutex;

    static EDITOR_ENV_LOCK: Mutex<()> = Mutex::new(());

    fn restore_editor(previous: Option<std::ffi::OsString>) {
        match previous {
            Some(value) => std::env::set_var("EDITOR", value),
            None => std::env::remove_var("EDITOR"),
        }
    }

    #[test]
    fn missing_editor_does_not_create_an_edit() {
        let _guard = EDITOR_ENV_LOCK.lock().unwrap();
        let previous = std::env::var_os("EDITOR");
        std::env::remove_var("EDITOR");
        let outcome = edit_profile_json("{}").unwrap();
        restore_editor(previous);
        assert_eq!(outcome, EditOutcome::NoEditorConfigured);
    }

    #[cfg(unix)]
    #[test]
    fn editor_can_save_valid_object_json() {
        let _guard = EDITOR_ENV_LOCK.lock().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let editor = write_editor(temp.path(), "printf '{\"answer\":42}' > \"$1\"");
        let previous = std::env::var_os("EDITOR");
        std::env::set_var("EDITOR", &editor);
        let outcome = edit_profile_json("{}").unwrap();
        restore_editor(previous);
        assert_eq!(outcome, EditOutcome::Saved("{\"answer\":42}".to_string()));
    }

    #[cfg(unix)]
    #[test]
    fn invalid_json_is_preserved_in_private_temp_file() {
        use std::os::unix::fs::PermissionsExt;

        let _guard = EDITOR_ENV_LOCK.lock().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let editor = write_editor(temp.path(), "printf '[]' > \"$1\"");
        let previous = std::env::var_os("EDITOR");
        std::env::set_var("EDITOR", &editor);
        let outcome = edit_profile_json("{}").unwrap();
        restore_editor(previous);

        let EditOutcome::ValidationError { temp_file, .. } = outcome else {
            panic!("expected validation error");
        };
        assert_eq!(std::fs::read_to_string(&temp_file).unwrap(), "[]");
        assert_eq!(
            std::fs::metadata(&temp_file).unwrap().permissions().mode() & 0o777,
            0o600
        );
        std::fs::remove_file(temp_file).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn nonzero_editor_exit_preserves_temp_file() {
        let _guard = EDITOR_ENV_LOCK.lock().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let editor = write_editor(temp.path(), "exit 9");
        let previous = std::env::var_os("EDITOR");
        std::env::set_var("EDITOR", &editor);
        let outcome = edit_profile_json("{\"before\":true}").unwrap();
        restore_editor(previous);

        let EditOutcome::EditorExitedWithError(message) = outcome else {
            panic!("expected editor error");
        };
        let path = message
            .split("edits preserved in ")
            .nth(1)
            .expect("preserved path");
        assert_eq!(std::fs::read_to_string(path).unwrap(), "{\"before\":true}");
        std::fs::remove_file(path).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn editor_timeout_returns_error() {
        let _guard = EDITOR_ENV_LOCK.lock().unwrap();
        let temp = tempfile::tempdir().unwrap();
        // Create an editor that sleeps for 120 seconds (longer than 60s timeout)
        let editor = write_editor(temp.path(), "sleep 120");
        let previous = std::env::var_os("EDITOR");
        std::env::set_var("EDITOR", &editor);
        let outcome = edit_profile_json("{}").unwrap();
        restore_editor(previous);

        let EditOutcome::EditorExitedWithError(message) = outcome else {
            panic!("expected editor timeout error, got: {:?}", outcome);
        };
        assert!(
            message.contains("timed out"),
            "Expected timeout message, got: {}",
            message
        );
    }

    #[cfg(unix)]
    fn write_editor(directory: &std::path::Path, body: &str) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let path = directory.join("editor.sh");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        let mut permissions = std::fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&path, permissions).unwrap();
        path
    }

    // ── Tests for edit_profile_json_with_terminal_reset ─────────────────

    #[test]
    fn with_terminal_reset_missing_editor_returns_no_editor_configured() {
        let _guard = EDITOR_ENV_LOCK.lock().unwrap();
        let previous = std::env::var_os("EDITOR");
        std::env::remove_var("EDITOR");
        let outcome = super::edit_profile_json_with_terminal_reset("{}").unwrap();
        restore_editor(previous);
        assert_eq!(outcome, EditOutcome::NoEditorConfigured);
    }

    #[cfg(unix)]
    #[test]
    fn with_terminal_reset_saves_valid_object_json() {
        let _guard = EDITOR_ENV_LOCK.lock().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let editor = write_editor(temp.path(), "printf '{\"answer\":42}' > \"$1\"");
        let previous = std::env::var_os("EDITOR");
        std::env::set_var("EDITOR", &editor);
        let outcome = super::edit_profile_json_with_terminal_reset("{}").unwrap();
        restore_editor(previous);
        assert_eq!(outcome, EditOutcome::Saved("{\"answer\":42}".to_string()));
    }

    #[cfg(unix)]
    #[test]
    fn with_terminal_reset_validation_error_preserves_temp_file() {
        let _guard = EDITOR_ENV_LOCK.lock().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let editor = write_editor(temp.path(), "printf '[]' > \"$1\"");
        let previous = std::env::var_os("EDITOR");
        std::env::set_var("EDITOR", &editor);
        let outcome = super::edit_profile_json_with_terminal_reset("{}").unwrap();
        restore_editor(previous);

        let EditOutcome::ValidationError { temp_file, .. } = outcome else {
            panic!("expected validation error");
        };
        assert_eq!(std::fs::read_to_string(&temp_file).unwrap(), "[]");
        std::fs::remove_file(temp_file).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn with_terminal_reset_nonzero_editor_exit_preserves_temp_file() {
        let _guard = EDITOR_ENV_LOCK.lock().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let editor = write_editor(temp.path(), "exit 9");
        let previous = std::env::var_os("EDITOR");
        std::env::set_var("EDITOR", &editor);
        let outcome = super::edit_profile_json_with_terminal_reset("{\"before\":true}").unwrap();
        restore_editor(previous);

        let EditOutcome::EditorExitedWithError(message) = outcome else {
            panic!("expected editor error");
        };
        let path = message
            .split("edits preserved in ")
            .nth(1)
            .expect("preserved path");
        assert_eq!(std::fs::read_to_string(path).unwrap(), "{\"before\":true}");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn terminal_reset_escapes_include_required_sequences() {
        let mut buffer = Vec::new();
        super::write_terminal_reset_escapes(&mut buffer).unwrap();
        let bytes = String::from_utf8_lossy(&buffer);

        // Must reset character set (G0 → USASCII)
        assert!(
            bytes.contains("\x1b(B"),
            "missing G0 reset sequence, got: {bytes:?}"
        );
        // Must reset SGR (colors/attributes)
        assert!(
            bytes.contains("\x1b[0m"),
            "missing SGR reset, got: {bytes:?}"
        );
        // Both character-set slots must be restored to ASCII. In particular,
        // ESC ) 0 would *enable* DEC Special Graphics in G1 and can turn
        // ordinary letters into line-drawing glyphs if the editor leaves G1
        // selected.
        assert!(
            bytes.contains("\x1b)B"),
            "missing G1 ASCII reset, got: {bytes:?}"
        );
        assert!(
            bytes.contains('\x0f'),
            "missing shift-in to select G0, got: {bytes:?}"
        );
        assert!(
            !bytes.contains("\x1b)0"),
            "reset must not designate G1 as DEC Special Graphics, got: {bytes:?}"
        );
        // Must disable mouse tracking that vim may have enabled
        assert!(
            bytes.contains("\x1b[?1000l"),
            "missing mouse tracking disable, got: {bytes:?}"
        );
        // Must disable bracketed paste
        assert!(
            bytes.contains("\x1b[?2004l"),
            "missing bracketed paste disable, got: {bytes:?}"
        );
        // Must end with flushable newline (so output actually reaches terminal
        // even if stdout is line-buffered)
        assert!(
            bytes.ends_with('\n'),
            "reset sequence should end with newline for flush, got: {bytes:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn save_terminal_state_does_not_panic() {
        // stty -g returns None when there is no controlling TTY (e.g. in
        // cargo's test runner). Either outcome is acceptable; we just
        // verify the helper doesn't panic.
        let _ = super::save_terminal_state();
    }

    #[cfg(unix)]
    #[test]
    fn restore_terminal_state_does_not_panic_on_garbage_input() {
        // stty may error on unparseable input, which is fine. The
        // contract is "no panic, return a Result".
        let _ = super::restore_terminal_state("garbage_input_stty_cannot_parse");
    }

    #[cfg(unix)]
    #[test]
    fn save_and_restore_round_trip_when_tty_available() {
        // If a TTY is available, both should succeed and the snapshot
        // should be non-empty.
        let Some(saved) = super::save_terminal_state() else {
            // No TTY (cargo test runner). Nothing to verify.
            return;
        };
        assert!(
            !saved.is_empty(),
            "stty -g snapshot must be non-empty when TTY is available"
        );
        let result = super::restore_terminal_state(&saved);
        assert!(
            result.is_ok(),
            "restoring the snapshot we just saved should succeed, got: {result:?}"
        );
    }
}
