use super::{ClaudeBackend, ClaudeCliLauncher, ResumeLauncher, ResumeTarget, SessionKey};
use cowboy::claude_env::{ClaudeEnvStore, Setting};
use cowboy::infrastructure::ClaudeProjectsStore;
use rusqlite::Connection;
use std::{env, fs, path::PathBuf, process::Command};

const SETTING_CLAUDE_COMMAND_ALIAS: &str = "claude_command_alias";

#[test]
fn launching_previously_bound_projects_uses_alias_without_profile_arguments() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let project = root.join("project with spaces");
    fs::create_dir(&project).unwrap();
    let db_path = root.join("cowboy.db");
    let store = ClaudeEnvStore::new(&db_path);
    store.initialize().unwrap();
    let db = Connection::open(&db_path).unwrap();
    db.execute_batch(
        "PRAGMA user_version=4;
         CREATE TABLE claude_profiles (id INTEGER PRIMARY KEY, name TEXT UNIQUE, settings_json TEXT, updated_at TEXT DEFAULT '');
         CREATE TABLE project_profile_bindings (project_cwd TEXT PRIMARY KEY, profile_name TEXT);
         INSERT INTO claude_profiles (name, settings_json) VALUES ('work', '{}');",
    ).unwrap();
    db.execute(
        "INSERT INTO project_profile_bindings (project_cwd, profile_name) VALUES (?1, 'work')",
        [project.to_string_lossy()],
    )
    .unwrap();
    fs::create_dir(root.join("profiles")).unwrap();
    fs::write(root.join("profiles/settings.work.json"), "{}").unwrap();

    let source = root.join("fake.rs");
    fs::write(
        &source,
        r#"
fn main() {
    use std::env;
    if env::current_exe().unwrap().file_stem().unwrap() == "bash" {
        let args: Vec<_> = env::args().skip(1).collect();
        assert_eq!(&args[..3], &["-c", "exec pi \"$@\"", "pi"]);
        let trace = env::var_os("COWBOY_TEST_BASH_TRACE").unwrap();
        std::fs::OpenOptions::new().write(true).create_new(true).open(trace).unwrap();
        std::process::exit(std::process::Command::new("pi.exe").args(&args[3..]).status().unwrap().code().unwrap());
    }
    let mut lines = vec![env::current_dir().unwrap().display().to_string(),
        env::var("COWBOY_TEST_MARKER").unwrap(),
        env::current_exe().unwrap().file_stem().unwrap().to_string_lossy().into_owned()];
    lines.extend(env::args().skip(1));
    std::fs::write(env::var_os("COWBOY_TEST_CAPTURE").unwrap(), lines.join("\n")).unwrap();
}
"#,
    )
    .unwrap();
    let fake = root.join(format!("claude{}", env::consts::EXE_SUFFIX));
    let compile = Command::new(env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
        .arg(&source)
        .arg("-o")
        .arg(&fake)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let alias = root.join(format!("custom-claude{}", env::consts::EXE_SUFFIX));
    fs::copy(&fake, &alias).unwrap();
    fs::copy(&fake, root.join(format!("pi{}", env::consts::EXE_SUFFIX))).unwrap();
    store
        .upsert_setting(&Setting {
            key: SETTING_CLAUDE_COMMAND_ALIAS.into(),
            value: alias.display().to_string(),
        })
        .unwrap();
    store.initialize().unwrap();
    let path = env::join_paths(
        std::iter::once(root.to_path_buf()).chain(
            env::var_os("PATH")
                .iter()
                .flat_map(|value| env::split_paths(value)),
        ),
    )
    .unwrap();

    #[cfg(windows)]
    let git_bin = {
        let git = root.join("Git");
        let bin = git.join("bin");
        fs::create_dir_all(&bin).unwrap();
        fs::copy(&fake, bin.join("bash.exe")).unwrap();
        fs::write(git.join("git-bash.exe"), []).unwrap();
        bin
    };
    let mut mismatches = Vec::new();
    for (mode, executable, expected_args) in [
        ("pi-new", "pi", &[][..]),
        ("pi-resume", "pi", &["--session", "session-123"][..]),
        ("new", "custom-claude", &[][..]),
        ("resume", "custom-claude", &["--resume", "session-123"][..]),
        ("resume-default", "claude", &["--resume", "session-123"][..]),
    ] {
        if mode == "resume-default" {
            db.execute(
                "DELETE FROM settings WHERE key=?1",
                [SETTING_CLAUDE_COMMAND_ALIAS],
            )
            .unwrap();
        }
        let capture = root.join(format!("{mode}.txt"));
        #[cfg(windows)]
        let path = if mode.starts_with("pi-") {
            env::join_paths([root, git_bin.as_path()]).unwrap()
        } else {
            path.clone()
        };
        let trace = root.join(format!("{mode}.bash"));
        let output = Command::new(env::current_exe().unwrap())
            .args(["--exact", "application::launcher_tests::run_launcher_probe"])
            .env("PATH", &path)
            .env("COWBOY_TEST_BASH_TRACE", &trace)
            .env("COWBOY_TEST_CAPTURE", &capture)
            .env("COWBOY_TEST_MARKER", "inherited")
            .env("COWBOY_TEST_LAUNCH_MODE", mode)
            .env("COWBOY_TEST_DB", &db_path)
            .env("COWBOY_TEST_PROJECT", &project)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        if cfg!(windows) && mode.starts_with("pi-") {
            assert!(trace.exists(), "{mode}: Git Bash was not invoked");
        }
        let captured = fs::read_to_string(capture).unwrap();
        let lines: Vec<_> = captured.lines().collect();
        assert_eq!(
            fs::canonicalize(lines[0]).unwrap(),
            fs::canonicalize(&project).unwrap()
        );
        assert_eq!(&lines[1..3], &["inherited", executable]);
        if &lines[3..] != expected_args {
            mismatches.push(format!(
                "{mode}: got {:?}, expected {expected_args:?}",
                &lines[3..]
            ));
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
    assert_eq!(
        fs::read(root.join("profiles/settings.work.json")).unwrap(),
        b"{}"
    );
}

#[cfg(windows)]
#[test]
fn windows_pi_fallback_selects_one_launcher_and_reports_failures() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let exe = dir.join("pi.exe");
    let cmd = dir.join("pi.cmd");
    let capture = dir.join("cmd-used");
    let source = dir.join("exit.rs");
    fs::write(&source, "fn main() { std::process::exit(23); }").unwrap();
    assert!(
        Command::new(env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
            .arg(source)
            .arg("-o")
            .arg(&exe)
            .status()
            .unwrap()
            .success()
    );
    fs::write(&cmd, "@echo off\r\n>cmd-used echo %1 %2\r\nexit /b 0\r\n").unwrap();
    let probe = || {
        Command::new(env::current_exe().unwrap())
            .args(["--exact", "application::launcher_tests::run_launcher_probe"])
            .env("PATH", dir)
            .env("ProgramFiles", dir)
            .env("ProgramFiles(x86)", dir)
            .env("LOCALAPPDATA", dir)
            .env("COWBOY_TEST_LAUNCH_MODE", "pi-resume")
            .env("COWBOY_TEST_PROJECT", dir)
            .env("COWBOY_TEST_DB", dir.join("unused"))
            .output()
            .unwrap()
    };
    let failure = |output: std::process::Output, expected: &str| {
        assert!(!output.status.success());
        let log = String::from_utf8_lossy(&output.stdout);
        assert!(log.contains(expected), "{log}");
        assert!(!capture.exists());
    };
    failure(probe(), "Pi exited with status");
    fs::write(&exe, b"invalid exe").unwrap();
    failure(probe(), "Failed to launch Pi");
    fs::remove_file(&exe).unwrap();
    let output = probe();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        fs::read_to_string(&capture).unwrap().trim(),
        "--session session-123"
    );
    fs::remove_file(&capture).unwrap();
    fs::remove_file(&cmd).unwrap();
    failure(probe(), "Failed to launch Pi");
}

#[test]
fn run_launcher_probe() {
    let Ok(mode) = env::var("COWBOY_TEST_LAUNCH_MODE") else {
        return;
    };
    let store = ClaudeEnvStore::new(PathBuf::from(env::var_os("COWBOY_TEST_DB").unwrap()));
    let cwd = PathBuf::from(env::var_os("COWBOY_TEST_PROJECT").unwrap());
    if mode.starts_with("pi-") {
        use super::{AgentBackend, PiBackend, SessionRepository};
        let backend = PiBackend::new(cowboy::pi::PiProjectsStore::new(cwd.join("sessions")));
        assert_eq!((backend.id().as_str(), backend.label()), ("pi", "Pi"));
        assert!(backend.load_projects().unwrap().is_empty());
        let target = ResumeTarget {
            key: SessionKey::new(backend.id(), "session-123").unwrap(),
            cwd,
        };
        #[cfg(not(windows))]
        let args = if mode == "pi-new" {
            vec![]
        } else {
            vec!["--session", "session-123"]
        };
        #[cfg(not(windows))]
        assert_eq!(
            super::PiCliLauncher::command(&target.cwd, &args).get_program(),
            "pi"
        );
        if mode == "pi-new" {
            backend.launch_new(&target.cwd).unwrap();
        } else {
            backend.resume(&target).unwrap();
        }
        return;
    }
    let launcher = ClaudeCliLauncher::new(store);
    // Launch through the backend strategy so the delegation to
    // ClaudeCliLauncher is covered by this probe.
    let backend = ClaudeBackend::new(ClaudeProjectsStore::new(cwd.join("projects")), launcher);
    if mode == "new" {
        backend.launch_new(&cwd).unwrap();
    } else {
        backend
            .resume(&ResumeTarget {
                key: SessionKey::claude("session-123"),
                cwd,
            })
            .unwrap();
    }
}
