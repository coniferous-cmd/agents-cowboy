use super::{ClaudeCliLauncher, ResumeLauncher, ResumeTarget, SessionKey};
use cowboy::claude_env::{ClaudeEnvStore, Setting};
use rusqlite::Connection;
use std::{env, fs, path::PathBuf, process::Command};

const SETTING_CLAUDE_COMMAND_ALIAS: &str = "claude_command_alias";

#[test]
fn launching_bound_projects_leaves_settings_to_claude() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let project = root.join("project");
    fs::create_dir(&project).unwrap();
    let db_path = root.join("cowboy.db");
    let store = ClaudeEnvStore::new(&db_path);
    store.initialize().unwrap();
    let db = Connection::open(&db_path).unwrap();
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS claude_profiles (id INTEGER PRIMARY KEY, name TEXT UNIQUE, settings_json TEXT, updated_at TEXT DEFAULT '');
         CREATE TABLE IF NOT EXISTS project_profile_bindings (project_cwd TEXT PRIMARY KEY, profile_name TEXT);
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
    let path = env::join_paths(
        std::iter::once(root.to_path_buf()).chain(
            env::var_os("PATH")
                .iter()
                .flat_map(|value| env::split_paths(value)),
        ),
    )
    .unwrap();

    let mut mismatches = Vec::new();
    for (mode, executable, expected_args) in [
        ("new", "custom-claude", &[][..]),
        ("resume", "claude", &["--resume", "session-123"][..]),
    ] {
        if mode == "new" {
            store
                .upsert_setting(&Setting {
                    key: SETTING_CLAUDE_COMMAND_ALIAS.into(),
                    value: alias.display().to_string(),
                })
                .unwrap();
        } else {
            db.execute(
                "DELETE FROM settings WHERE key=?1",
                [SETTING_CLAUDE_COMMAND_ALIAS],
            )
            .unwrap();
        }
        let capture = root.join(format!("{mode}.txt"));
        let output = Command::new(env::current_exe().unwrap())
            .args(["--exact", "application::launcher_tests::run_launcher_probe"])
            .env("PATH", &path)
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
}

#[test]
fn run_launcher_probe() {
    let Ok(mode) = env::var("COWBOY_TEST_LAUNCH_MODE") else {
        return;
    };
    let store = ClaudeEnvStore::new(PathBuf::from(env::var_os("COWBOY_TEST_DB").unwrap()));
    let cwd = PathBuf::from(env::var_os("COWBOY_TEST_PROJECT").unwrap());
    let launcher = ClaudeCliLauncher::new(store);
    if mode == "new" {
        launcher.launch_new(&cwd).unwrap();
    } else {
        launcher
            .resume(&ResumeTarget {
                key: SessionKey::claude("session-123"),
                cwd,
            })
            .unwrap();
    }
}
