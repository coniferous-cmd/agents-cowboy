use crate::domain::{Result, StetsonError};
use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};

use super::schema::initialize_schema;
use super::settings::{
    default_claude_config_dir, default_claude_projects_dir, default_metadata_db_path,
    SETTING_CLAUDE_COMMAND_ALIAS, SETTING_CLAUDE_CONFIG_DIR, SETTING_CLAUDE_PROJECTS_DIR,
    SETTING_METADATA_DB_PATH,
};
use super::ClaudeEnvStore;

impl ClaudeEnvStore {
    pub fn from_home() -> Result<Self> {
        Ok(Self::new(default_metadata_db_path()?))
    }

    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn initialize(&self) -> Result<()> {
        let mut connection = self.connection()?;
        initialize_schema(&mut connection, &default_claude_config_dir()?)
    }

    pub fn seed_default_settings(&self) -> Result<usize> {
        let defaults = [
            super::Setting {
                key: SETTING_CLAUDE_CONFIG_DIR.into(),
                value: default_claude_config_dir()?.display().to_string(),
            },
            super::Setting {
                key: SETTING_CLAUDE_PROJECTS_DIR.into(),
                value: default_claude_projects_dir()?.display().to_string(),
            },
            super::Setting {
                key: SETTING_METADATA_DB_PATH.into(),
                value: self.path.display().to_string(),
            },
        ];
        let connection = self.connection()?;
        let mut inserted = 0;
        for setting in defaults {
            inserted += connection.execute(
                "INSERT INTO settings (key,value) VALUES (?1,?2) ON CONFLICT(key) DO NOTHING",
                params![setting.key, setting.value],
            )?;
        }
        Ok(inserted)
    }

    pub fn upsert_setting(&self, setting: &super::Setting) -> Result<()> {
        self.connection()?.execute(
            "INSERT INTO settings (key,value) VALUES (?1,?2) \
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![setting.key, setting.value],
        )?;
        Ok(())
    }

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare("SELECT value FROM settings WHERE key=?1")?;
        let mut rows = statement.query([key])?;
        Ok(rows.next()?.map(|row| row.get(0)).transpose()?)
    }

    pub fn list_settings(&self) -> Result<Vec<super::Setting>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare("SELECT key,value FROM settings ORDER BY key")?;
        let rows = statement.query_map([], |row| {
            Ok(super::Setting {
                key: row.get(0)?,
                value: row.get(1)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(StetsonError::from)
    }

    pub fn claude_projects_dir(&self) -> Result<PathBuf> {
        match self.get_setting(SETTING_CLAUDE_PROJECTS_DIR)? {
            Some(path) if !path.trim().is_empty() => Ok(PathBuf::from(path)),
            _ => default_claude_projects_dir(),
        }
    }

    pub fn claude_command_alias(&self) -> Result<String> {
        match self.get_setting(SETTING_CLAUDE_COMMAND_ALIAS)? {
            Some(alias) if !alias.trim().is_empty() => Ok(alias),
            _ => Ok("claude".to_string()),
        }
    }

    pub(super) fn connection(&self) -> Result<Connection> {
        if let Some(parent) = self.path.parent() {
            super::profiles::ensure_private_dir(parent)?;
        }
        drop(super::profiles::private_open(&self.path)?);
        super::profiles::ensure_private_file(&self.path)?;
        for suffix in ["-wal", "-shm"] {
            super::profiles::ensure_private_file(&PathBuf::from(format!(
                "{}{suffix}",
                self.path.display()
            )))?;
        }
        let connection = Connection::open(&self.path)?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        Ok(connection)
    }
}
