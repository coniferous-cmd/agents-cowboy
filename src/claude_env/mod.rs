use std::path::PathBuf;

mod profiles;
mod schema;
mod settings;
mod store;
mod themes;

pub use self::profiles::{
    validate_profile_name, validate_settings_json, AtomicReplace, ClaudeProfile, RecoveryOutcome,
};
pub use self::settings::default_metadata_db_path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Setting {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    pub name: String,
    pub is_active: bool,
    pub active_pane_border: String,
    pub inactive_pane_border: String,
    pub project_highlight: String,
    pub session_highlight: String,
    pub status_badge_bg: String,
    pub status_badge_fg: String,
    pub hint_key_fg: String,
    pub hint_text_fg: String,
    pub meta_text_fg: String,
    pub modal_border: String,
}

#[derive(Debug, Clone)]
pub struct ClaudeEnvStore {
    path: PathBuf,
}
