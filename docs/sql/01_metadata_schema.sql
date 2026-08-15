PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS settings (
    id INTEGER PRIMARY KEY,
    key TEXT NOT NULL UNIQUE,
    value TEXT NOT NULL
);

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
);

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
);

PRAGMA user_version = 1;
