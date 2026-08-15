-- Reference for the version-0 -> version-1 cleanup transaction.
-- The application first exports non-empty claude_project_settings rows to a
-- private cowboy-migrated-*.json file. It aborts before this transaction if
-- that export cannot be completed.

BEGIN IMMEDIATE;

DROP INDEX IF EXISTS idx_claude_env_value_env_name;
DROP INDEX IF EXISTS idx_claude_env_values_env_name;
DROP INDEX IF EXISTS idx_claude_env_values_env_id;

DROP TABLE IF EXISTS claude_env_value;
DROP TABLE IF EXISTS claude_env_values;
DROP TABLE IF EXISTS claude_env_value_legacy;
DROP TABLE IF EXISTS claude_env_values_legacy;
DROP TABLE IF EXISTS claude_project_settings;
DROP TABLE IF EXISTS claude_project_settings_legacy;
DROP TABLE IF EXISTS claude_env;
DROP TABLE IF EXISTS claude_envs;
DROP TABLE IF EXISTS claude_env_legacy;
DROP TABLE IF EXISTS claude_envs_legacy;

-- Create the version-1 tables from 01_metadata_schema.sql before committing.
PRAGMA foreign_key_check;
PRAGMA user_version = 1;

COMMIT;
