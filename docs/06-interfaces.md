# Interfaces

## Application boundaries

- `SessionRepository`
  - `load_projects()`
  - `rename_session(session_id, new_title)`
  - `delete_session(session_id)`
  - `delete_project(project_cwd)`
- `ResumeLauncher`
  - `resume(target)`
  - `launch_new(cwd)`
  - child processes inherit the current process environment
- `ProfileRepository`
  - `list_profiles()`
  - `active_profile_name()`
  - `activate_profile(name)`

## Metadata store

`ClaudeEnvStore` retains its historical name but now owns generic settings,
themes, Profiles, and activation recovery. The public Profile API is:

- `list_profiles`, `create_profile`, `profile`, `update_profile_json`, `delete_profile`
- `activate_profile`, `active_profile_name`
- `recover_profile_activation`
- `perform_initial_backup` — one-shot file backup of `settings.json` to
  `settings.json.cowboy-backup`, gated by the `initial_backup_done` flag in
  the `settings` table
- `claude_config_dir`, `global_settings_path`

All Profile names and settings JSON are validated by shared helpers.

## CLI

```text
config list
config create <name>
config edit <name>
config delete <name>
config activate <name>
config bind <project-path> <profile-name>
config unbind <project-path>
config copy <source> <new-name>
```

`config edit` updates SQLite only after `$EDITOR` exits successfully and the
result parses as a JSON object. Invalid edits remain in a private temporary file.

## TUI state

- `MainTab`: `Projects` or `Profiles`
- `FocusPane`: `Projects` or `Sessions`, independent of `MainTab`
- Profiles state: rows and active Profile name (no snapshot subpanel)
- `[`/`]` changes `MainTab`; browser `Tab`/`←`/`→` changes `FocusPane`; those keys are no-ops in Profiles

## SQL references

- `docs/sql/01_metadata_schema.sql`: canonical version-1 schema
- `docs/sql/02_legacy_migration.sql`: legacy cleanup transaction reference
- `docs/07-profiles-plan.md`: complete migration, activation, CLI, TUI, and acceptance semantics (note: snapshot-related sections are now historical)
