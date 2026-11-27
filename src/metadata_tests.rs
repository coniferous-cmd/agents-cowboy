use super::initialize_env_store;
use cowboy::claude_env::ClaudeEnvStore;
use rusqlite::Connection;
use std::{fs, path::Path};

fn fresh_store(root: &Path) -> ClaudeEnvStore {
    ClaudeEnvStore::new(root.join("cowboy.db"))
}

#[test]
fn fresh_metadata_only_creates_retained_tables() {
    let temp = tempfile::tempdir().unwrap();
    let store = fresh_store(temp.path());
    store.initialize().unwrap();
    store.seed_default_theme().unwrap();
    let db = Connection::open(store.path()).unwrap();
    let mut query = db
        .prepare("SELECT name FROM sqlite_schema WHERE type='table' ORDER BY name")
        .unwrap();
    let tables = query
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(store.claude_command_alias().unwrap(), "claude");
    assert!(store.active_theme().unwrap().is_some());
    assert!(!temp.path().join("profiles").exists());
    assert_eq!(tables, ["settings", "themes"]);
}

fn legacy_fixture(root: &Path, version: i64) -> (ClaudeEnvStore, Connection) {
    let store = fresh_store(root);
    let db = Connection::open(store.path()).unwrap();
    let uniqueness = if version == 2 { "UNIQUE" } else { "" };
    db.execute_batch(&format!(
        "CREATE TABLE settings (id INTEGER PRIMARY KEY, key TEXT NOT NULL UNIQUE, value TEXT NOT NULL);
         CREATE TABLE claude_profiles (id INTEGER PRIMARY KEY, name TEXT UNIQUE, settings_json TEXT, updated_at TEXT);
         CREATE TABLE project_profile_bindings (project_cwd TEXT PRIMARY KEY, profile_name TEXT NOT NULL {uniqueness}, created_at TEXT, FOREIGN KEY(profile_name) REFERENCES claude_profiles(name));
         CREATE TABLE profile_activation_journal (id INTEGER PRIMARY KEY, target_kind TEXT NOT NULL, target_id TEXT NOT NULL, target_name TEXT, target_json_hash TEXT, phase TEXT, error TEXT, created_at TEXT);
         INSERT INTO settings (key,value) VALUES ('active_profile_name','home'), ('claude_command_alias','keep-claude');
         INSERT INTO claude_profiles VALUES (1, 'work', '{{}}', 'preserve');
         INSERT INTO project_profile_bindings VALUES ('/project','work','preserve');
         INSERT INTO profile_activation_journal VALUES (1,'profile','1','work','not-a-real-hash','prepared',NULL,'preserve');"
    )).unwrap();
    if version < 4 {
        db.execute_batch("CREATE TABLE claude_settings_snapshots (id INTEGER PRIMARY KEY, settings_json TEXT); INSERT INTO claude_settings_snapshots VALUES (1, '{}');").unwrap();
    }
    db.pragma_update(None, "user_version", version).unwrap();
    let config = root.join("claude");
    fs::create_dir(&config).unwrap();
    db.execute(
        "INSERT INTO settings(key,value) VALUES ('claude_config_dir',?1)",
        [config.to_string_lossy()],
    )
    .unwrap();
    // Released databases of these versions already have a themes table. Create it using
    // the existing initializer, while it cannot run a legacy Profile migration.
    db.pragma_update(None, "user_version", 4).unwrap();
    store.initialize().unwrap();
    store.seed_default_theme().unwrap();
    db.execute(
        "UPDATE themes SET project_highlight='Magenta' WHERE is_active=1",
        [],
    )
    .unwrap();
    db.pragma_update(None, "user_version", version).unwrap();
    (store, db)
}

fn profile_state(db: &Connection, version: i64) -> Vec<String> {
    let mut state = vec![db
        .query_row(
            "SELECT value FROM settings WHERE key='active_profile_name'",
            [],
            |row| row.get(0),
        )
        .unwrap()];
    let mut tables = vec![
        "claude_profiles",
        "project_profile_bindings",
        "profile_activation_journal",
    ];
    if version < 4 {
        tables.push("claude_settings_snapshots");
    }
    for table in tables {
        let sql: Option<String> = db
            .query_row(
                "SELECT (SELECT sql FROM sqlite_schema WHERE name=?1)",
                [table],
                |row| row.get(0),
            )
            .unwrap();
        let Some(sql) = sql else {
            state.push(format!("missing table: {table}"));
            continue;
        };
        state.push(sql);
        let mut query = db
            .prepare(&format!("SELECT * FROM {table} ORDER BY 1"))
            .unwrap();
        let width = query.column_count();
        let rows = query
            .query_map([], |row| {
                (0..width)
                    .map(|index| row.get::<_, rusqlite::types::Value>(index))
                    .collect::<Result<Vec<_>, _>>()
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        state.push(format!("{rows:?}"));
    }
    state
}

fn assert_stored_profile_state_is_unchanged(version: i64) {
    let temp = tempfile::tempdir().unwrap();
    let (store, db) = legacy_fixture(temp.path(), version);
    let settings = temp.path().join("claude/settings.json");
    fs::write(&settings, br#"{"current":true}"#).unwrap();
    let before = profile_state(&db, version);

    let store = initialize_env_store(store).unwrap();

    assert_eq!(store.claude_command_alias().unwrap(), "keep-claude");
    assert_eq!(
        store.active_theme().unwrap().unwrap().project_highlight,
        "Magenta"
    );
    assert_eq!(fs::read(settings).unwrap(), br#"{"current":true}"#);
    assert!(!temp.path().join("profiles/settings.work.json").exists());
    assert_eq!(profile_state(&db, version), before);
    assert!(!temp
        .path()
        .join("claude/settings.json.cowboy-backup")
        .exists());
}

#[test]
fn version_two_keeps_legacy_profile_data() {
    assert_stored_profile_state_is_unchanged(2);
}

#[test]
fn version_three_keeps_legacy_profile_data() {
    assert_stored_profile_state_is_unchanged(3);
}

#[test]
fn version_four_keeps_legacy_profile_data() {
    assert_stored_profile_state_is_unchanged(4);
}

#[test]
fn startup_does_not_back_up_regular_claude_settings() {
    let temp = tempfile::tempdir().unwrap();
    let (store, db) = legacy_fixture(temp.path(), 4);
    db.execute("DELETE FROM profile_activation_journal", [])
        .unwrap();
    let settings = temp.path().join("claude/settings.json");
    fs::write(&settings, "{}").unwrap();

    initialize_env_store(store).unwrap();

    assert_eq!(fs::read_to_string(&settings).unwrap(), "{}");
    assert!(!settings
        .with_file_name("settings.json.cowboy-backup")
        .exists());
}

#[cfg(any(unix, windows))]
#[test]
fn startup_leaves_settings_symlink_target_and_backup_intact() {
    let temp = tempfile::tempdir().unwrap();
    let (store, db) = legacy_fixture(temp.path(), 4);
    let profile_path = temp.path().join("profiles/settings.work.json");
    fs::create_dir(profile_path.parent().unwrap()).unwrap();
    let contents = br#"{"from-profile":true}"#;
    fs::write(&profile_path, contents).unwrap();
    let settings = temp.path().join("claude/settings.json");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&profile_path, &settings).unwrap();
    #[cfg(windows)]
    if let Err(error) = std::os::windows::fs::symlink_file(&profile_path, &settings) {
        if matches!(error.raw_os_error(), Some(5 | 1314)) {
            eprintln!("Skipping symlink fixture: Windows does not permit symlink creation");
            return;
        }
        panic!("Could not create symlink fixture: {error}");
    }
    let backup = settings.with_file_name("settings.json.cowboy-backup");
    fs::write(&backup, "previous settings").unwrap();
    let before = profile_state(&db, 4);

    initialize_env_store(store).unwrap();

    assert_eq!(fs::read_link(&settings).unwrap(), profile_path);
    assert_eq!(fs::read(&settings).unwrap(), contents);
    assert_eq!(fs::read(&backup).unwrap(), b"previous settings");
    assert_eq!(profile_state(&db, 4), before);
}
