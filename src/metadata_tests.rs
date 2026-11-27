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
    store.initialize().unwrap();
    store.seed_default_theme().unwrap();
    let db = Connection::open(store.path()).unwrap();
    db.pragma_update(None, "foreign_keys", "ON").unwrap();
    let uniqueness = if version == 2 { "UNIQUE" } else { "" };
    db.execute_batch(&format!(
        "CREATE TABLE claude_profiles (id INTEGER PRIMARY KEY, name TEXT UNIQUE, settings_json TEXT, updated_at TEXT);
         CREATE TABLE project_profile_bindings (project_cwd TEXT PRIMARY KEY, profile_name TEXT NOT NULL {uniqueness}, created_at TEXT, FOREIGN KEY(profile_name) REFERENCES claude_profiles(name) ON DELETE RESTRICT);
         CREATE TABLE profile_activation_journal (id INTEGER PRIMARY KEY, target_kind TEXT NOT NULL, target_id TEXT NOT NULL, target_name TEXT, target_json_hash TEXT, phase TEXT, error TEXT, created_at TEXT);
         INSERT INTO settings (key,value) VALUES ('active_profile_name','home'), ('claude_command_alias','keep-claude');
         INSERT INTO claude_profiles VALUES (1, 'work', '{{}}', 'preserve');
         INSERT INTO project_profile_bindings VALUES ('/project','work','preserve');
         INSERT INTO profile_activation_journal VALUES (1,'profile','1','work','not-a-real-hash','prepared',NULL,'preserve');
         CREATE INDEX retired_journal_phase ON profile_activation_journal(phase);
         CREATE TRIGGER retired_profile_trigger AFTER UPDATE ON claude_profiles BEGIN SELECT 1; END;
         INSERT INTO settings (key,value) VALUES ('claude_projects_dir','keep-projects'), ('metadata_db_path','keep-db');"
    )).unwrap();
    if version < 4 {
        db.execute_batch("CREATE TABLE claude_settings_snapshots (id INTEGER PRIMARY KEY, settings_json TEXT); INSERT INTO claude_settings_snapshots VALUES (1, '{}');").unwrap();
    }
    let config = root.join("claude");
    fs::create_dir(&config).unwrap();
    db.execute(
        "INSERT INTO settings(key,value) VALUES ('claude_config_dir',?1)",
        [config.to_string_lossy()],
    )
    .unwrap();
    db.execute(
        "UPDATE themes SET project_highlight='Magenta' WHERE is_active=1",
        [],
    )
    .unwrap();
    db.pragma_update(None, "user_version", version).unwrap();
    (store, db)
}

fn assert_retired_state_removed(store: &ClaudeEnvStore, db: &Connection) {
    let objects = db
        .prepare("SELECT name FROM sqlite_schema WHERE type='table' OR name IN ('retired_journal_phase', 'retired_profile_trigger') ORDER BY name")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(objects, ["settings", "themes"]);
    assert_eq!(store.get_setting("active_profile_name").unwrap(), None);
    assert_eq!(
        db.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .unwrap(),
        5
    );
}

fn assert_stored_profile_state_is_removed(version: i64) {
    let temp = tempfile::tempdir().unwrap();
    let (store, db) = legacy_fixture(temp.path(), version);
    let settings = temp.path().join("claude/settings.json");
    fs::write(&settings, br#"{"current":true}"#).unwrap();
    let store = initialize_env_store(store).unwrap();

    assert_eq!(store.claude_command_alias().unwrap(), "keep-claude");
    assert_eq!(
        store.active_theme().unwrap().unwrap().project_highlight,
        "Magenta"
    );
    assert_eq!(
        store.claude_config_dir().unwrap(),
        temp.path().join("claude")
    );
    assert_eq!(
        store.claude_projects_dir().unwrap(),
        Path::new("keep-projects")
    );
    assert_eq!(
        store.get_setting("metadata_db_path").unwrap().as_deref(),
        Some("keep-db")
    );
    assert_eq!(fs::read(settings).unwrap(), br#"{"current":true}"#);
    assert!(!temp.path().join("profiles/settings.work.json").exists());
    assert_retired_state_removed(&store, &db);
    let store = initialize_env_store(store).unwrap();
    assert_retired_state_removed(&store, &db);
    assert!(!temp
        .path()
        .join("claude/settings.json.cowboy-backup")
        .exists());
}

#[test]
fn version_two_removes_legacy_profile_data() {
    assert_stored_profile_state_is_removed(2);
}

#[test]
fn version_three_removes_legacy_profile_data() {
    assert_stored_profile_state_is_removed(3);
}

#[test]
fn version_four_removes_legacy_profile_data() {
    assert_stored_profile_state_is_removed(4);
}

#[test]
fn profile_cleanup_sql_failure_rolls_back_schema_and_data() {
    let temp = tempfile::tempdir().unwrap();
    let (store, db) = legacy_fixture(temp.path(), 3);
    db.execute_batch("CREATE TRIGGER block_cleanup BEFORE DELETE ON settings BEGIN SELECT RAISE(ABORT, 'cleanup blocked'); END;").unwrap();
    let snapshot = |sql| {
        let mut statement = db.prepare(sql).unwrap();
        let columns = statement.column_count();
        statement
            .query_map([], |row| {
                (0..columns)
                    .map(|index| row.get::<_, rusqlite::types::Value>(index))
                    .collect::<Result<Vec<_>, _>>()
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    };
    let queries = [
        "SELECT * FROM sqlite_schema ORDER BY name",
        "SELECT * FROM settings ORDER BY id",
        "SELECT * FROM themes ORDER BY id",
        "SELECT * FROM claude_profiles",
        "SELECT * FROM project_profile_bindings",
        "SELECT * FROM profile_activation_journal",
        "SELECT * FROM claude_settings_snapshots",
        "PRAGMA user_version",
    ];
    let before = queries.map(snapshot);
    assert!(store
        .initialize()
        .unwrap_err()
        .to_string()
        .contains("cleanup blocked"));
    assert_eq!(queries.map(snapshot), before);
}

#[test]
fn startup_does_not_back_up_regular_claude_settings() {
    let temp = tempfile::tempdir().unwrap();
    let (store, db) = legacy_fixture(temp.path(), 4);
    let settings = temp.path().join("claude/settings.json");
    fs::write(&settings, "{}").unwrap();

    let store = initialize_env_store(store).unwrap();

    assert!(fs::symlink_metadata(&settings).unwrap().is_file());
    assert_eq!(fs::read_to_string(&settings).unwrap(), "{}");
    assert!(!temp.path().join("profiles").exists());
    assert_retired_state_removed(&store, &db);
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
    let store = initialize_env_store(store).unwrap();

    assert_eq!(fs::read_link(&settings).unwrap(), profile_path);
    assert_eq!(fs::read(&settings).unwrap(), contents);
    assert!(fs::symlink_metadata(&profile_path).unwrap().is_file());
    assert_eq!(fs::read(&profile_path).unwrap(), contents);
    assert!(fs::symlink_metadata(&backup).unwrap().is_file());
    assert_eq!(fs::read(&backup).unwrap(), b"previous settings");
    assert_retired_state_removed(&store, &db);
}
