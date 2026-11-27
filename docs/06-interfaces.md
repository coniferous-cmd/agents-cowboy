# Interfaces

## Application boundaries

- `SessionRepository`
  - `load_projects()`
  - `rename_session(session_key, new_title)`
  - `delete_session(session_key)`
  - `delete_project(project_cwd)`
- `ResumeLauncher`
  - `resume(target)`
  - `launch_new(cwd)`
  - Child processes inherit the current environment and load their own settings.

## Metadata store

`ClaudeEnvStore` owns SQLite-backed paths, launcher settings, and themes.
`claude_config_dir` and `claude_projects_dir` resolve configured paths.
Schema v5 keeps `settings` and `themes` as Cowboy's only supported metadata tables. The one-time upgrade drops `claude_profiles`, `project_profile_bindings`, `profile_activation_journal`, and `claude_settings_snapshots` with their table-owned indexes and triggers, and removes only `active_profile_name` from `settings`. Existing paths, aliases, and themes remain. The schema changes and version bump commit together; an SQL failure leaves the previous committed database intact. For v0/v1, a legacy project-settings export can remain on disk if a later SQL step rolls back.

The migration does not alter regular Claude settings, a settings symlink or its target, legacy Profile files, or existing settings backups. Back up `cowboy.db` before first launch. Downgrading alone does not restore removed records; restore the pre-upgrade database with Cowboy closed and use a compatible binary. See the [upgrade notes](../README.md#upgrading-from-a-release-with-profiles) for backup, restore, and symlink guidance.

## CLI

```text
cowboy
cowboy alias <command>
cowboy --help
```

Without a configured alias the launcher defaults to `claude`.

## TUI state

- `FocusPane`: `Projects` or `Sessions`; the highlighted pane border and the status badge reflect the current focus.
- `[` / `]`, `Tab`, and the left/right arrows toggle focus.

## Historical references

- `docs/sql/01_metadata_schema.sql`: historical version-1 schema, not the current initialization SQL.
- `docs/sql/02_legacy_migration.sql`: legacy environment cleanup transaction reference.
- `docs/07-profiles-plan.md`: historical Profile design; see the [upgrade notes](../README.md#upgrading-from-a-release-with-profiles) for current behavior.
