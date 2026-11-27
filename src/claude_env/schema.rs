use crate::domain::{Result, StetsonError};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use std::fs;
use std::path::{Path, PathBuf};

use super::files::{ensure_private_dir, AtomicReplace};

const SCHEMA_VERSION: i64 = 5;
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
pub(super) fn initialize_schema(
    connection: &mut Connection,
    default_config_dir: &Path,
) -> Result<()> {
    if supported_version(connection)? == SCHEMA_VERSION {
        return Ok(());
    }

    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    // Another initializer may have finished while we waited for the write lock.
    let version = supported_version(&transaction)?;
    if version == SCHEMA_VERSION {
        return Ok(());
    }

    if version < 2 {
        migrate_legacy_environment(&transaction, default_config_dir)?;
    } else {
        transaction.execute_batch(SETTINGS_SCHEMA)?;
    }
    transaction.execute_batch(THEMES_SCHEMA)?;
    transaction.execute_batch(
        "DROP TABLE IF EXISTS project_profile_bindings;
         DROP TABLE IF EXISTS profile_activation_journal;
         DROP TABLE IF EXISTS claude_settings_snapshots;
         DROP TABLE IF EXISTS claude_profiles;
         DELETE FROM settings WHERE key='active_profile_name';",
    )?;

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

fn supported_version(connection: &Connection) -> Result<i64> {
    let version = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version > SCHEMA_VERSION {
        return Err(StetsonError::UnsupportedSchemaVersion(version));
    }
    Ok(version)
}

fn migrate_legacy_environment(connection: &Connection, default_config_dir: &Path) -> Result<()> {
    let projects = legacy_project_rows(connection)?;
    if !projects.is_empty() {
        let config_dir =
            configured_claude_dir(connection)?.unwrap_or_else(|| default_config_dir.to_path_buf());
        write_legacy_dump(&config_dir, &projects)?;
    }
    normalize_settings_table(connection)?;

    for index in [
        "idx_claude_env_value_env_name",
        "idx_claude_env_values_env_name",
        "idx_claude_env_values_env_id",
    ] {
        connection.execute(&format!("DROP INDEX IF EXISTS {index}"), [])?;
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
        connection.execute(&format!("DROP TABLE IF EXISTS {table}"), [])?;
    }
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
    fn fresh_database_is_version_five_without_retired_tables() {
        let temp = tempdir().unwrap();
        let store = ClaudeEnvStore::new(temp.path().join("db/cowboy.db"));
        store.initialize().unwrap();
        let connection = Connection::open(store.path()).unwrap();
        assert_eq!(
            connection
                .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            5
        );
        for table in ["settings", "themes"] {
            assert!(exists(&connection, table));
        }
        for table in [
            "claude_profiles",
            "profile_activation_journal",
            "project_profile_bindings",
            "claude_settings_snapshots",
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
    fn v0_migration_dumps_raw_text_and_preserves_custom_setting() {
        assert_legacy_migration(0, "claude_project_settings");
    }

    #[test]
    fn v1_migration_dumps_raw_text_and_preserves_custom_setting() {
        assert_legacy_migration(1, "claude_project_settings_legacy");
    }

    fn assert_legacy_migration(version: i64, table: &str) {
        let temp = tempdir().unwrap();
        let db = temp.path().join("cowboy.db");
        let config = temp.path().join("custom-claude");
        let connection = Connection::open(&db).unwrap();
        connection
            .pragma_update(None, "user_version", version)
            .unwrap();
        connection.execute_batch(&format!("CREATE TABLE settings(key TEXT PRIMARY KEY,value TEXT NOT NULL); CREATE TABLE claude_env(name TEXT PRIMARY KEY,category TEXT NOT NULL); CREATE TABLE claude_env_value(scope TEXT,env_name TEXT,env_value TEXT); CREATE TABLE {table}(project_path TEXT PRIMARY KEY,settings_json TEXT NOT NULL); INSERT INTO settings VALUES('active_profile_name','work'), ('claude_command_alias','keep-claude'); CREATE TABLE claude_profiles (name TEXT PRIMARY KEY);")).unwrap();
        connection
            .execute(
                "INSERT INTO settings VALUES('claude_config_dir',?1)",
                [config.display().to_string()],
            )
            .unwrap();
        connection
            .execute(
                &format!("INSERT INTO {table} VALUES('/repo','not-json')"),
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
        assert_eq!(value["projects"][0]["path"], "/repo");
        assert_eq!(value["projects"][0]["settings_json"], "not-json");
        assert_eq!(store.claude_command_alias().unwrap(), "keep-claude");
        assert_eq!(store.get_setting("active_profile_name").unwrap(), None);
        let connection = Connection::open(&db).unwrap();
        let tables = connection
            .prepare("SELECT name FROM sqlite_schema WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(tables, ["settings", "themes"]);
        assert_eq!(
            connection
                .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            5
        );
    }

    #[test]
    fn future_schema_is_rejected_without_initialization() {
        let temp = tempdir().unwrap();
        let mut connection = Connection::open_in_memory().unwrap();
        connection.pragma_update(None, "user_version", 6).unwrap();
        assert!(matches!(
            initialize_schema(&mut connection, temp.path()),
            Err(StetsonError::UnsupportedSchemaVersion(6))
        ));
        assert!(!exists(&connection, "settings"));
        assert!(!exists(&connection, "themes"));
        assert_eq!(
            connection
                .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            6
        );
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
    fn legacy_export_is_reused_after_sql_rollback() {
        let temp = tempdir().unwrap();
        let mut connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(SETTINGS_SCHEMA).unwrap();
        connection.execute_batch(
            "PRAGMA user_version=1;
             CREATE TABLE claude_project_settings(project_path TEXT PRIMARY KEY, settings_json TEXT NOT NULL);
             INSERT INTO claude_project_settings VALUES('/repo','{}');
             INSERT INTO settings (key,value) VALUES('active_profile_name','work');
             CREATE TRIGGER block_cleanup BEFORE DELETE ON settings
             BEGIN SELECT RAISE(ABORT, 'cleanup blocked'); END;",
        ).unwrap();
        let error = initialize_schema(&mut connection, temp.path()).unwrap_err();
        assert!(error.to_string().contains("cleanup blocked"));
        assert!(exists(&connection, "claude_project_settings"));
        let dumps = || {
            fs::read_dir(temp.path())
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .collect::<Vec<_>>()
        };
        let first = dumps();
        assert_eq!(first.len(), 1);
        let contents = fs::read(&first[0]).unwrap();

        connection
            .execute_batch("DROP TRIGGER block_cleanup")
            .unwrap();
        initialize_schema(&mut connection, temp.path()).unwrap();

        assert_eq!(dumps(), first);
        assert_eq!(fs::read(&first[0]).unwrap(), contents);
        assert!(!exists(&connection, "claude_project_settings"));
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
}
