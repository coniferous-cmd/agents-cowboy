use crate::domain::{Result, StetsonError};
#[cfg(not(target_os = "macos"))]
use dirs::config_dir;
use dirs::home_dir;
use std::path::PathBuf;

pub(super) const SETTING_CLAUDE_CONFIG_DIR: &str = "claude_config_dir";
pub(super) const SETTING_CLAUDE_PROJECTS_DIR: &str = "claude_projects_dir";
pub(super) const SETTING_METADATA_DB_PATH: &str = "metadata_db_path";
pub(super) const SETTING_CLAUDE_COMMAND_ALIAS: &str = "claude_command_alias";

pub(super) fn default_claude_config_dir() -> Result<PathBuf> {
    let home = home_dir().ok_or(StetsonError::HomeDirectoryUnavailable)?;
    Ok(home.join(".claude"))
}

pub(super) fn default_claude_projects_dir() -> Result<PathBuf> {
    Ok(default_claude_config_dir()?.join("projects"))
}

pub fn default_metadata_db_path() -> Result<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let home = home_dir().ok_or(StetsonError::HomeDirectoryUnavailable)?;
        Ok(home
            .join("Library")
            .join("Application Support")
            .join("cowboy")
            .join("cowboy.db"))
    }

    #[cfg(not(target_os = "macos"))]
    {
        let base_dir = match config_dir() {
            Some(dir) => dir,
            None => {
                let home = home_dir().ok_or(StetsonError::HomeDirectoryUnavailable)?;
                home.join(".config")
            }
        };
        Ok(base_dir.join("cowboy").join("cowboy.db"))
    }
}
