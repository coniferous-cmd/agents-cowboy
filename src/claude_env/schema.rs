use crate::domain::{Result, StetsonError};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use std::fs;
use std::path::{Path, PathBuf};

use super::profiles::{ensure_private_dir, AtomicReplace};

const SCHEMA_VERSION: i64 = 1;
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
CREATE TABLE IF NOT EXISTS claude_settings_snapshots (
    id INTEGER PRIMARY KEY,
    captured_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    source TEXT,
    settings_json TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS profile_activation_journal (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    target_kind TEXT NOT NULL CHECK (target_kind IN ('profile', 'snapshot')),
    target_id TEXT NOT NULL,
    target_name TEXT,
    target_json_hash TEXT NOT NULL,
    phase TEXT NOT NULL CHECK (phase IN ('prepared', 'file_replaced', 'failed')),
    error TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CHECK ((target_kind = 'profile' AND target_name IS NOT NULL) OR
           (target_kind = 'snapshot' AND target_name IS NULL)),
    CHECK ((phase = 'failed' AND error IS NOT NULL) OR
           (phase != 'failed' AND error IS NULL))
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
    fn fresh_database_is_version_one_without_legacy_tables() {
        let temp = tempdir().unwrap();
        let store = ClaudeEnvStore::new(temp.path().join("db/cowboy.db"));
        store.initialize().unwrap();
        let connection = Connection::open(store.path()).unwrap();
        assert_eq!(
            connection
                .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            1
        );
        for table in [
            "settings",
            "themes",
            "claude_profiles",
            "claude_settings_snapshots",
            "profile_activation_journal",
        ] {
            assert!(exists(&connection, table));
        }
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
}
