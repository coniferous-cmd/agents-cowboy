use crate::domain::{Result, StetsonError};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use std::fs;
use std::path::{Path, PathBuf};

use super::profiles::{ensure_private_dir, AtomicReplace};

const SCHEMA_VERSION: i64 = 4;
const SETTINGS_SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS settings (
    id INTEGER PRIMARY KEY,
    key TEXT NOT NULL UNIQUE,
    value TEXT NOT NULL
);";
const THEMES_SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS themes (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    is_active INTEGER NOT NULL DEFAULT 0,
    active_pane_border TEXT NOT NULL DEFAULT 'Cyan',
    inactive_pane_border TEXT NOT NULL DEFAULT 'DarkGray',
    project_highlight TEXT NOT NULL DEFAULT 'Yellow',
    session_highlight TEXT NOT NULL DEFAULT 'Magenta',
    status_badge_bg TEXT NOT NULL DEFAULT 'LightMagenta',
    status_badge_fg TEXT NOT NULL DEFAULT 'Black',
    hint_key_fg TEXT NOT NULL DEFAULT 'White',
    hint_text_fg TEXT NOT NULL DEFAULT 'Gray',
    meta_text_fg TEXT NOT NULL DEFAULT 'DarkGray',
    modal_border TEXT NOT NULL DEFAULT 'Cyan'
);";
const PROFILES_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS claude_profiles (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    settings_json TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE IF NOT EXISTS profile_activation_journal (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    target_kind TEXT NOT NULL CHECK (target_kind IN ('profile')),
    target_id TEXT NOT NULL,
    target_name TEXT NOT NULL,
    target_json_hash TEXT NOT NULL,
    phase TEXT NOT NULL CHECK (phase IN ('prepared', 'file_replaced', 'failed')),
    error TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CHECK ((phase = 'failed' AND error IS NOT NULL) OR
           (phase != 'failed' AND error IS NULL))
);"#;
const PROJECT_BINDINGS_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS project_profile_bindings (
    project_cwd TEXT PRIMARY KEY,
    profile_name TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (profile_name) REFERENCES claude_profiles(name) ON DELETE RESTRICT
);"#;

pub(super) fn initialize_schema(
    connection: &mut Connection,
    default_config_dir: &Path,
) -> Result<()> {
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version > SCHEMA_VERSION {
        return Err(StetsonError::UnsupportedSchemaVersion(version));
    }
    if version == SCHEMA_VERSION {
        create_current_schema(connection)?;
        return Ok(());
    }

    // Migration from version 3 to 4: drop snapshot table, simplify journal,
    // mark first-launch backup as done.
    if version == 3 {
        migrate_v3_to_v4(connection)?;
        return Ok(());
    }

    // Migration from version 2 to 3: remove UNIQUE constraint on profile_name
    if version == 2 {
        migrate_v2_to_v3(connection)?;
        return Ok(());
    }

    let config_dir =
        configured_claude_dir(connection)?.unwrap_or_else(|| default_config_dir.to_path_buf());
    let projects = legacy_project_rows(connection)?;
    if !projects.is_empty() {
        write_legacy_dump(&config_dir, &projects)?;
    }

    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    normalize_settings_table(&transaction)?;
    transaction.execute_batch(THEMES_SCHEMA)?;
    transaction.execute_batch(PROFILES_SCHEMA)?;
    transaction.execute_batch(PROJECT_BINDINGS_SCHEMA)?;

    for index in [
        "idx_claude_env_value_env_name",
        "idx_claude_env_values_env_name",
        "idx_claude_env_values_env_id",
    ] {
        transaction.execute(&format!("DROP INDEX IF EXISTS {index}"), [])?;
    }
    for table in [
        "claude_env_value",
        "claude_env_values",
        "claude_env_value_legacy",
        "claude_env_values_legacy",
        "claude_project_settings",
        "claude_project_settings_legacy",
        "claude_env",
        "claude_envs",
        "claude_env_legacy",
        "claude_envs_legacy",
    ] {
        transaction.execute(&format!("DROP TABLE IF EXISTS {table}"), [])?;
    }

    let violations = {
        let mut statement = transaction.prepare("PRAGMA foreign_key_check")?;
        let mut rows = statement.query([])?;
        let mut count = 0;
        while rows.next()?.is_some() {
            count += 1;
        }
        count
    };
    if violations != 0 {
        return Err(StetsonError::MigrationFailed(format!(
            "foreign_key_check reported {violations} violation(s)"
        )));
    }
    transaction.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    transaction.commit()?;
    Ok(())
}

fn create_current_schema(connection: &Connection) -> Result<()> {
    connection.execute_batch(SETTINGS_SCHEMA)?;
    connection.execute_batch(THEMES_SCHEMA)?;
    connection.execute_batch(PROFILES_SCHEMA)?;
    connection.execute_batch(PROJECT_BINDINGS_SCHEMA)?;
    Ok(())
}

/// Migrate from schema version 2 to 3: remove UNIQUE constraint on profile_name
/// in project_profile_bindings table to allow 1:N relationship.
fn migrate_v2_to_v3(connection: &mut Connection) -> Result<()> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;

    // Create new table without UNIQUE constraint on profile_name
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS project_profile_bindings_v2 (
            project_cwd TEXT PRIMARY KEY,
            profile_name TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (profile_name) REFERENCES claude_profiles(name) ON DELETE RESTRICT
        );",
    )?;

    // Copy data from old table to new table
    transaction.execute_batch(
        "INSERT OR IGNORE INTO project_profile_bindings_v2 (project_cwd, profile_name, created_at)
         SELECT project_cwd, profile_name, created_at FROM project_profile_bindings;",
    )?;

    // Drop old table and rename new table
    transaction.execute_batch("DROP TABLE project_profile_bindings;")?;
    transaction.execute_batch(
        "ALTER TABLE project_profile_bindings_v2 RENAME TO project_profile_bindings;",
    )?;

    // Update schema version
    transaction.pragma_update(None, "user_version", 3)?;
    transaction.commit()?;
    Ok(())
}

/// Migrate from schema version 3 to 4: drop the snapshot table and rebuild
/// the activation journal (target_kind can only be 'profile', target_name is
/// now NOT NULL). The first-launch backup flag is intentionally NOT set here so
/// that the startup flow's `perform_initial_backup` call still runs once after
/// the upgrade — v3 users get the same `settings.json.cowboy-backup` file that
/// fresh installs get.
fn migrate_v3_to_v4(connection: &mut Connection) -> Result<()> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;

    // Drop the snapshot table outright; SQLite cannot ALTER it and the data is
    // intentionally discarded. Users who need history can rely on
    // `settings.json.cowboy-backup` (written by perform_initial_backup right
    // after initialize() returns).
    transaction.execute("DROP TABLE IF EXISTS claude_settings_snapshots", [])?;

    // Rebuild the journal: clear any in-flight row (id=1), drop the old table,
    // recreate it with the v4 schema (profile-only, target_name NOT NULL).
    transaction.execute("DELETE FROM profile_activation_journal WHERE id=1", [])?;
    transaction.execute("DROP TABLE profile_activation_journal", [])?;
    transaction.execute_batch(
        "CREATE TABLE profile_activation_journal (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            target_kind TEXT NOT NULL CHECK (target_kind IN ('profile')),
            target_id TEXT NOT NULL,
            target_name TEXT NOT NULL,
            target_json_hash TEXT NOT NULL,
            phase TEXT NOT NULL CHECK (phase IN ('prepared', 'file_replaced', 'failed')),
            error TEXT,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            CHECK ((phase = 'failed' AND error IS NOT NULL) OR
                   (phase != 'failed' AND error IS NULL))
        );",
    )?;

    transaction.pragma_update(None, "user_version", 4)?;
    transaction.commit()?;
    Ok(())
}

fn configured_claude_dir(connection: &Connection) -> Result<Option<PathBuf>> {
    if !table_exists(connection, "settings")? {
        return Ok(None);
    }
    let value = connection
        .query_row(
            "SELECT value FROM settings WHERE key='claude_config_dir'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    Ok(value.and_then(|value| {
        let value = value.trim();
        let path = PathBuf::from(value);
        (!value.is_empty() && path.is_absolute()).then_some(path)
    }))
}

fn legacy_project_rows(connection: &Connection) -> Result<Vec<(String, String)>> {
    let table = if table_exists(connection, "claude_project_settings")? {
        Some("claude_project_settings")
    } else if table_exists(connection, "claude_project_settings_legacy")? {
        Some("claude_project_settings_legacy")
    } else {
        None
    };
    let Some(table) = table else {
        return Ok(Vec::new());
    };
    let mut statement = connection.prepare(&format!(
        "SELECT project_path,settings_json FROM {table} ORDER BY project_path"
    ))?;
    let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn write_legacy_dump(config_dir: &Path, projects: &[(String, String)]) -> Result<PathBuf> {
    let projects = projects.iter().map(|(path, settings_json)| {
        serde_json::json!({"path": path, "settings_json": settings_json})
    }).collect::<Vec<_>>();
    let bytes = serde_json::to_vec_pretty(&serde_json::json!({"projects": projects}))?;
    ensure_private_dir(config_dir)?;

    for entry in fs::read_dir(config_dir)?.flatten() {
        if entry
            .file_name()
            .to_string_lossy()
            .starts_with("cowboy-migrated-")
            && fs::read(entry.path()).ok().as_deref() == Some(bytes.as_slice())
        {
            return Ok(entry.path());
        }
    }

    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%S.%9fZ");
    for suffix in 0u32.. {
        let suffix = if suffix == 0 {
            String::new()
        } else {
            format!("-{suffix}")
        };
        let path = config_dir.join(format!("cowboy-migrated-{stamp}{suffix}.json"));
        if !path.exists() {
            AtomicReplace::write(&path, &bytes)?;
            return Ok(path);
        }
    }
    unreachable!()
}

fn normalize_settings_table(connection: &Connection) -> Result<()> {
    if table_exists(connection, "settings")? && !column_exists(connection, "settings", "id")? {
        connection.execute("ALTER TABLE settings RENAME TO settings_profiles_v0", [])?;
        connection.execute_batch(SETTINGS_SCHEMA)?;
        connection.execute(
            "INSERT INTO settings (key,value) SELECT key,value FROM settings_profiles_v0",
            [],
        )?;
        connection.execute("DROP TABLE settings_profiles_v0", [])?;
    } else {
        connection.execute_batch(SETTINGS_SCHEMA)?;
    }
    Ok(())
}

fn table_exists(connection: &Connection, name: &str) -> Result<bool> {
    Ok(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
        [name],
        |row| row.get(0),
    )?)
}

fn column_exists(connection: &Connection, table: &str, column: &str) -> Result<bool> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
    for name in rows {
        if name? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::claude_env::ClaudeEnvStore;
    use rusqlite::Connection;
    use tempfile::tempdir;

    fn exists(connection: &Connection, name: &str) -> bool {
        table_exists(connection, name).unwrap()
    }

    #[test]
    fn fresh_database_is_version_four_without_snapshot_or_legacy_tables() {
        let temp = tempdir().unwrap();
        let store = ClaudeEnvStore::new(temp.path().join("db/cowboy.db"));
        store.initialize().unwrap();
        let connection = Connection::open(store.path()).unwrap();
        assert_eq!(
            connection
                .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            4
        );
        for table in [
            "settings",
            "themes",
            "claude_profiles",
            "profile_activation_journal",
            "project_profile_bindings",
        ] {
            assert!(exists(&connection, table));
        }
        assert!(!exists(&connection, "claude_settings_snapshots"));
        for table in [
            "claude_env",
            "claude_envs",
            "claude_env_value",
            "claude_env_values",
            "claude_project_settings",
        ] {
            assert!(!exists(&connection, table));
        }
    }

    #[test]
    fn migration_dumps_raw_text_and_preserves_custom_setting() {
        let temp = tempdir().unwrap();
        let db = temp.path().join("cowboy.db");
        let config = temp.path().join("custom-claude");
        let connection = Connection::open(&db).unwrap();
        connection.execute_batch("CREATE TABLE settings(key TEXT PRIMARY KEY,value TEXT NOT NULL); CREATE TABLE claude_env(name TEXT PRIMARY KEY,category TEXT NOT NULL); CREATE TABLE claude_env_value(scope TEXT,env_name TEXT,env_value TEXT); CREATE TABLE claude_project_settings(project_path TEXT PRIMARY KEY,settings_json TEXT NOT NULL);").unwrap();
        connection
            .execute(
                "INSERT INTO settings VALUES('claude_config_dir',?1)",
                [config.display().to_string()],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO claude_project_settings VALUES('/repo','not-json')",
                [],
            )
            .unwrap();
        drop(connection);
        let store = ClaudeEnvStore::new(&db);
        store.initialize().unwrap();
        assert_eq!(
            store.get_setting("claude_config_dir").unwrap().as_deref(),
            config.to_str()
        );
        store.seed_default_settings().unwrap();
        assert_eq!(
            store.get_setting("claude_config_dir").unwrap().as_deref(),
            config.to_str()
        );
        let dump = fs::read_dir(&config)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let value: serde_json::Value = serde_json::from_slice(&fs::read(dump).unwrap()).unwrap();
        assert_eq!(value["projects"][0]["settings_json"], "not-json");
        let connection = Connection::open(&db).unwrap();
        assert!(!exists(&connection, "claude_project_settings"));
    }

    #[test]
    fn empty_project_table_produces_no_dump() {
        let temp = tempdir().unwrap();
        let db = temp.path().join("cowboy.db");
        let config = temp.path().join("claude");
        let connection = Connection::open(&db).unwrap();
        connection.execute_batch("CREATE TABLE settings(key TEXT PRIMARY KEY,value TEXT NOT NULL); CREATE TABLE claude_project_settings(project_path TEXT PRIMARY KEY,settings_json TEXT NOT NULL);").unwrap();
        connection
            .execute(
                "INSERT INTO settings VALUES('claude_config_dir',?1)",
                [config.display().to_string()],
            )
            .unwrap();
        drop(connection);
        ClaudeEnvStore::new(&db).initialize().unwrap();
        assert!(!config.exists());
    }

    #[test]
    fn dump_failure_does_not_advance_schema() {
        let temp = tempdir().unwrap();
        let db = temp.path().join("cowboy.db");
        let blocked = temp.path().join("file");
        fs::write(&blocked, "x").unwrap();
        let connection = Connection::open(&db).unwrap();
        connection.execute_batch("CREATE TABLE settings(key TEXT PRIMARY KEY,value TEXT NOT NULL); CREATE TABLE claude_project_settings(project_path TEXT PRIMARY KEY,settings_json TEXT NOT NULL); INSERT INTO claude_project_settings VALUES('/repo','{}');").unwrap();
        connection
            .execute(
                "INSERT INTO settings VALUES('claude_config_dir',?1)",
                [blocked.display().to_string()],
            )
            .unwrap();
        drop(connection);
        assert!(ClaudeEnvStore::new(&db).initialize().is_err());
        let connection = Connection::open(&db).unwrap();
        assert_eq!(
            connection
                .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert!(exists(&connection, "claude_project_settings"));
    }

    #[test]
    fn migration_v2_to_v3_removes_unique_constraint_on_profile_name() {
        let temp = tempdir().unwrap();
        let db = temp.path().join("cowboy.db");
        let connection = Connection::open(&db).unwrap();

        // Create schema version 2 with UNIQUE constraint on profile_name
        connection
            .execute_batch(
                "PRAGMA user_version = 2;
                 CREATE TABLE settings(id INTEGER PRIMARY KEY, key TEXT NOT NULL UNIQUE, value TEXT NOT NULL);
                 CREATE TABLE themes(id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE, is_active INTEGER NOT NULL DEFAULT 0);
                 CREATE TABLE claude_profiles(id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE, settings_json TEXT NOT NULL, updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);
                 CREATE TABLE claude_settings_snapshots(id INTEGER PRIMARY KEY, captured_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP, source TEXT, settings_json TEXT NOT NULL);
                 CREATE TABLE profile_activation_journal(id INTEGER PRIMARY KEY CHECK (id = 1), target_kind TEXT NOT NULL, target_id TEXT NOT NULL, target_name TEXT, target_json_hash TEXT NOT NULL, phase TEXT NOT NULL, error TEXT, created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);
                 CREATE TABLE project_profile_bindings(project_cwd TEXT PRIMARY KEY, profile_name TEXT NOT NULL UNIQUE, created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP, FOREIGN KEY (profile_name) REFERENCES claude_profiles(name) ON DELETE RESTRICT);",
            )
            .unwrap();

        // Insert test data
        connection
            .execute_batch(
                "INSERT INTO claude_profiles (name, settings_json) VALUES ('work', '{}');
                 INSERT INTO claude_profiles (name, settings_json) VALUES ('home', '{}');
                 INSERT INTO project_profile_bindings (project_cwd, profile_name) VALUES ('/project/a', 'work');
                 INSERT INTO project_profile_bindings (project_cwd, profile_name) VALUES ('/project/b', 'home');",
            )
            .unwrap();

        drop(connection);

        // Initialize store - this should trigger migration
        let store = ClaudeEnvStore::new(&db);
        store.initialize().unwrap();

        let connection = Connection::open(&db).unwrap();
        assert_eq!(
            connection
                .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            3
        );

        // Verify data was migrated
        let mut statement = connection
            .prepare("SELECT project_cwd, profile_name FROM project_profile_bindings ORDER BY project_cwd")
            .unwrap();
        let rows: Vec<(String, String)> = statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], ("/project/a".to_string(), "work".to_string()));
        assert_eq!(rows[1], ("/project/b".to_string(), "home".to_string()));
    }

    #[test]
    fn migration_v2_to_v3_allows_same_profile_bound_to_multiple_projects() {
        let temp = tempdir().unwrap();
        let db = temp.path().join("cowboy.db");
        let connection = Connection::open(&db).unwrap();

        // Create schema version 2
        connection
            .execute_batch(
                "PRAGMA user_version = 2;
                 CREATE TABLE settings(id INTEGER PRIMARY KEY, key TEXT NOT NULL UNIQUE, value TEXT NOT NULL);
                 CREATE TABLE themes(id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE, is_active INTEGER NOT NULL DEFAULT 0);
                 CREATE TABLE claude_profiles(id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE, settings_json TEXT NOT NULL, updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);
                 CREATE TABLE claude_settings_snapshots(id INTEGER PRIMARY KEY, captured_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP, source TEXT, settings_json TEXT NOT NULL);
                 CREATE TABLE profile_activation_journal(id INTEGER PRIMARY KEY CHECK (id = 1), target_kind TEXT NOT NULL, target_id TEXT NOT NULL, target_name TEXT, target_json_hash TEXT NOT NULL, phase TEXT NOT NULL, error TEXT, created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);
                 CREATE TABLE project_profile_bindings(project_cwd TEXT PRIMARY KEY, profile_name TEXT NOT NULL UNIQUE, created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP, FOREIGN KEY (profile_name) REFERENCES claude_profiles(name) ON DELETE RESTRICT);",
            )
            .unwrap();

        connection
            .execute_batch(
                "INSERT INTO claude_profiles (name, settings_json) VALUES ('work', '{}');",
            )
            .unwrap();

        drop(connection);

        // Initialize store - this should trigger migration
        let store = ClaudeEnvStore::new(&db);
        store.initialize().unwrap();

        // Now we should be able to bind the same profile to multiple projects
        store
            .bind_profile(&PathBuf::from("/project/a"), "work")
            .unwrap();
        store
            .bind_profile(&PathBuf::from("/project/b"), "work")
            .unwrap();

        // Verify both bindings exist
        let binding_a = store
            .project_binding(&PathBuf::from("/project/a"))
            .unwrap()
            .unwrap();
        let binding_b = store
            .project_binding(&PathBuf::from("/project/b"))
            .unwrap()
            .unwrap();

        assert_eq!(binding_a.profile_name, "work");
        assert_eq!(binding_b.profile_name, "work");

        // Verify profile_bindings returns both projects
        let bindings = store.profile_bindings("work").unwrap();
        assert_eq!(bindings.len(), 2);
    }

    #[test]
    fn v3_to_v4_migration_drops_snapshot_table_and_writes_flag() {
        let temp = tempdir().unwrap();
        let db = temp.path().join("cowboy.db");
        let config_dir = temp.path().join("claude");
        let connection = Connection::open(&db).unwrap();

        // Seed v3 schema with one snapshot row and a non-trivial settings file.
        connection
            .execute_batch(
                "PRAGMA user_version = 3;
                 CREATE TABLE settings(id INTEGER PRIMARY KEY, key TEXT NOT NULL UNIQUE, value TEXT NOT NULL);
                 CREATE TABLE themes(id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE, is_active INTEGER NOT NULL DEFAULT 0);
                 CREATE TABLE claude_profiles(id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE, settings_json TEXT NOT NULL, updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);
                 CREATE TABLE claude_settings_snapshots(id INTEGER PRIMARY KEY, captured_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP, source TEXT, settings_json TEXT NOT NULL);
                 CREATE TABLE profile_activation_journal(id INTEGER PRIMARY KEY CHECK (id = 1), target_kind TEXT NOT NULL, target_id TEXT NOT NULL, target_name TEXT, target_json_hash TEXT NOT NULL, phase TEXT NOT NULL, error TEXT, created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);
                 CREATE TABLE project_profile_bindings(project_cwd TEXT PRIMARY KEY, profile_name TEXT NOT NULL, created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP, FOREIGN KEY (profile_name) REFERENCES claude_profiles(name) ON DELETE RESTRICT);",
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO claude_settings_snapshots (settings_json) VALUES ('{\"old\":true}')",
                [],
            )
            .unwrap();
        fs::create_dir_all(&config_dir).unwrap();
        let original = "{\"key\":\"value\"}";
        fs::write(config_dir.join("settings.json"), original).unwrap();
        connection
            .execute(
                "INSERT INTO settings (key,value) VALUES ('claude_config_dir',?1)",
                [config_dir.display().to_string()],
            )
            .unwrap();

        drop(connection);

        // Trigger migration.
        let store = ClaudeEnvStore::new(&db);
        store.initialize().unwrap();
        // Main.rs calls perform_initial_backup right after initialize(); mirror
        // that here so the test exercises the full upgrade flow.
        store.perform_initial_backup().unwrap();

        let connection = Connection::open(&db).unwrap();
        assert_eq!(
            connection
                .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            4
        );
        assert!(!exists(&connection, "claude_settings_snapshots"));
        let flag: String = connection
            .query_row(
                "SELECT value FROM settings WHERE key='initial_backup_done'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(flag, "1");
        assert_eq!(
            fs::read_to_string(config_dir.join("settings.json.cowboy-backup")).unwrap(),
            original
        );
    }

    #[test]
    fn v3_to_v4_migration_removes_snapshot_journal_kind() {
        let temp = tempdir().unwrap();
        let db = temp.path().join("cowboy.db");
        let connection = Connection::open(&db).unwrap();

        connection
            .execute_batch(
                "PRAGMA user_version = 3;
                 CREATE TABLE settings(id INTEGER PRIMARY KEY, key TEXT NOT NULL UNIQUE, value TEXT NOT NULL);
                 CREATE TABLE themes(id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE, is_active INTEGER NOT NULL DEFAULT 0);
                 CREATE TABLE claude_profiles(id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE, settings_json TEXT NOT NULL, updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);
                 CREATE TABLE claude_settings_snapshots(id INTEGER PRIMARY KEY, captured_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP, source TEXT, settings_json TEXT NOT NULL);
                 CREATE TABLE profile_activation_journal(id INTEGER PRIMARY KEY CHECK (id = 1), target_kind TEXT NOT NULL, target_id TEXT NOT NULL, target_name TEXT, target_json_hash TEXT NOT NULL, phase TEXT NOT NULL, error TEXT, created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);
                 CREATE TABLE project_profile_bindings(project_cwd TEXT PRIMARY KEY, profile_name TEXT NOT NULL, created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP, FOREIGN KEY (profile_name) REFERENCES claude_profiles(name) ON DELETE RESTRICT);",
            )
            .unwrap();

        drop(connection);

        let store = ClaudeEnvStore::new(&db);
        store.initialize().unwrap();

        let connection = Connection::open(&db).unwrap();
        // The new schema's CHECK should reject the old 'snapshot' kind.
        let result = connection.execute(
            "INSERT INTO profile_activation_journal \
             (id,target_kind,target_id,target_name,target_json_hash,phase) \
             VALUES (1,'snapshot','1',NULL,'abc','prepared')",
            [],
        );
        assert!(
            result.is_err(),
            "expected CHECK constraint to reject 'snapshot' kind after v4 migration"
        );
        // And target_name must be NOT NULL.
        let result = connection.execute(
            "INSERT INTO profile_activation_journal \
             (id,target_kind,target_id,target_name,target_json_hash,phase) \
             VALUES (1,'profile','1',NULL,'abc','prepared')",
            [],
        );
        assert!(
            result.is_err(),
            "expected NOT NULL constraint to reject NULL target_name after v4 migration"
        );
    }
}
