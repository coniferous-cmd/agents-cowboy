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
  - `list_snapshots()`
  - `active_profile_name()`
  - `activate_profile(name)`
  - `activate_snapshot(id)`

## Metadata store

`ClaudeEnvStore` retains its historical name but now owns generic settings,
themes, Profiles, snapshots, and activation recovery. The public Profile API is:

- `list_profiles`, `create_profile`, `profile`, `update_profile_json`, `delete_profile`
- `activate_profile`, `active_profile_name`
- `list_snapshots`, `snapshot`, `delete_snapshot`, `prune_snapshots`, `activate_snapshot`
- `recover_profile_activation`
- `claude_config_dir`, `global_settings_path`

All Profile names and settings JSON are validated by shared helpers. History is
ordered by `captured_at DESC, id DESC`; displayed size is the UTF-8 byte length.

## CLI

```text
config list
config create <name>
config edit <name>
config delete <name>
config activate <name>
config history list
config history show <id>
config history activate <id>
config history delete <id>
config history prune --keep <n>
```

`config edit` updates SQLite only after `$EDITOR` exits successfully and the
result parses as a JSON object. Invalid edits remain in a private temporary file.
`history show` may print secrets and the help text warns about this.

## TUI state

- `MainTab`: `Projects` or `Profiles`
- `FocusPane`: `Projects` or `Sessions`, independent of `MainTab`
- Profiles state: rows, snapshots, continuous cursor, and active Profile name
- `[`/`]` changes `MainTab`; browser `Tab`/`←`/`→` changes `FocusPane`; those keys are no-ops in Profiles

## SQL references

- `docs/sql/01_metadata_schema.sql`: canonical version-1 schema
- `docs/sql/02_legacy_migration.sql`: legacy cleanup transaction reference
- `docs/07-profiles-plan.md`: complete migration, activation, CLI, TUI, and acceptance semantics
