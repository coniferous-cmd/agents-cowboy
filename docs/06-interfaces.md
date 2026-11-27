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
Legacy Profile data is no longer used and is left untouched.

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
