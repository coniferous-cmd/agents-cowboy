# Architecture

## Overview

cowboy is a keyboard-first Rust TUI for discovering and managing local
Claude Code sessions. Claude's filesystem data remains the source of truth;
SQLite stores cowboy metadata and configuration only.

The main dependency direction is:

```text
Presentation -> Application -> Domain <- Infrastructure
```

## Layers

### Presentation

- `src/main.rs` owns terminal setup, teardown, the foreground application loop,
  and coordination of background deletion workers.
- `src/app/` (directory module) owns interactive state, selection, modal state,
  and key-driven actions:
  - `mod.rs` — `Stetson` facade, lifecycle methods, and tests
  - `state.rs` — `AppState`, `FocusPane`, `ModalState`, `DeleteTarget` types
  - `navigation.rs` — project/session selection, focus toggle, clamping
  - `session_actions.rs` — resume, info, search, rename, delete
  - `project_actions.rs` — new session, project delete, and Open Here
  - `modal.rs` — `handle_key` dispatch and modal input processing
- `src/ui/mod.rs`, `src/ui/layout.rs`, and `src/ui/colors.rs` render the TUI.
- `src/theme.rs` maps stored theme values to Ratatui colors.

Presentation may format domain data for display, but must not own persistence,
pricing rules, session parsing, or process-launch policy.

### Application

- `src/application.rs` coordinates session repositories, launchers, and Profile activation.
- `SessionRepository`, `ResumeLauncher`, and `ProfileRepository` are the primary application boundaries.
- Application errors are converted into user-facing toast messages before
  returning control to the TUI.

### Domain

- `src/domain.rs` defines projects, sessions, token usage, cost estimates, and
  shared error types.
- Domain helpers group sessions into projects and calculate values that do not
  depend on terminal or filesystem APIs.
- Domain types must not depend on Ratatui, Crossterm, SQLite, or process APIs.

### Infrastructure

- `src/infrastructure/` (directory module) discovers, parses, renames, and
  deletes Claude session files under `~/.claude/projects/`:
  - `mod.rs` — `ClaudeProjectsStore` struct and tests
  - `store.rs` — store construction, session discovery, project queries
  - `parser.rs` — JSONL parsing, title/usage/model extraction
  - `timestamps.rs` — RFC 3339 timestamp collection and UTC normalisation
  - `project_paths.rs` — project directory encoding and path matching
  - `mutation.rs` — rename and delete operations
- `src/claude_env/` (directory module) owns the SQLite metadata store:
  - `mod.rs` — Profile, setting, and theme types
    module declarations, and tests
  - `store.rs` — `ClaudeEnvStore` main implementation
  - `schema.rs` — versioned database schema, private legacy dump, and migration
  - `settings.rs` — `Setting` data, default paths, launcher alias
  - `profiles.rs` — Profile repositories, activation journal, locking, permissions, and atomic replacement
  - `themes.rs` — theme CRUD and activation
- `src/pricing.rs` loads the embedded pricing table from
  `data/llm_pricing.json`.
- `src/encoding/` maps workspace paths to Claude project-directory names on
  Unix and Windows.

## Feature Layout

Feature-specific pure helpers live under `src/features/<feature>/`. Shared
orchestration stays in `src/app.rs`, `src/application.rs`, and `src/ui/`.

Current feature modules are:

- `project_usage`: aggregate session usage and format project totals.
- `session_delete`: confirmation and deletion status helpers.
- `session_info`: build the session detail view model.
- `session_list`: filter sessions and construct panel titles.
- `session_rename`: validate rename input and construct status messages.
- `session_resume`: construct resume targets.
- `session_search`: manage search input and submission state.

New feature modules should contain behavior specific to one capability. They
should not become alternate owners of global application state or persistence.

## Runtime Flow

The interactive flow is:

```text
Terminal event -> App action -> Application boundary -> State update -> Render
```

Deletion is the exception to the otherwise foreground action flow. After the
user confirms a session or project deletion, the app records a pending task and
the main loop starts one standard-library worker. The worker owns filesystem
deletion and the following project reload; the main thread continues rendering
and polling input. A completion message returns either the refreshed project
list or a formatted error to the app state. Thread and channel objects remain
in the runtime loop rather than in `AppState`.

```text
Confirm deletion -> queue task -> worker deletes and reloads -> completion
                                      |                         |
                                      +-- UI remains responsive --+
```

Only one deletion worker may be active. While it is active, app state rejects
exit and conflicting shortcuts so no action can race with filesystem mutation.
Success replaces the displayed list; failure preserves it and exposes the
error. This protects normal application-level exit paths, not forced process
termination.

Launching Claude temporarily exits the alternate screen and raw mode. The CLI
Profile editor runs `$EDITOR` outside the TUI. Profile activation uses the
shared repository directly and reloads the Profiles view afterward.

## Storage Boundaries

### Claude Filesystem

- `~/.claude/projects/**/*.jsonl` is the source of truth for sessions.
- The global `settings.json` is atomically replaced only by Profile activation.
- Rename and delete operations mutate Claude-owned session data and companion
  sidecar directories.
- Project discovery does not depend on SQLite.

### SQLite Metadata

- The default database is stored in the platform configuration directory for
  cowboy.
- SQLite stores configurable paths, Profiles, journal state,
  launcher settings, and themes.
- SQL reference files live under `docs/sql/`.
- SQLite must not become a second source of truth for session discovery.

## Session Discovery

`ClaudeProjectsStore` scans Claude project directories and parses JSONL records
into `Session` values. Session identity falls back to the file stem when a
record does not expose an id. The parser extracts titles, cwd, branch, message
count, model, usage, and timestamps.

Valid RFC 3339 timestamps are compared as instants. The earliest value becomes
`created_at`, the latest becomes `updated_at`, and both are normalized to UTC.
Local-time conversion happens only at the UI boundary.

Discovery should degrade at the narrowest practical boundary. A malformed or
partially written session must eventually be isolated from healthy sessions
rather than making the entire project list unavailable.

## Token Usage And Cost Estimation

- Token accounting reads the `usage` payload already written to session JSONL.
- cowboy does not re-tokenize transcript text.
- Session usage is aggregated before project usage is calculated.
- Cost is an estimate derived from usage, model, and the embedded pricing table.
- Missing usage or model data produces an unknown or token-only state rather
  than a fabricated zero-cost estimate.
- API interception and billing reconciliation are outside the current design.

## Claude Process Environment

New and resumed Claude processes inherit the parent process environment without
project- or session-specific overrides from cowboy.

## Profile Activation

The `config edit` CLI validates edited content as a JSON object before updating
SQLite. Profile activation serializes journal preparation in SQLite,
atomically replaces the configured global settings file, then commits active
state and clears the journal. A cross-process lock prevents CLI/TUI races; startup
recovers a matching pending journal by hashing the exact target bytes.

### First-Launch Backup

On the first launch that observes an existing `settings.json`, the startup flow
copies it to `<claude_config_dir>/settings.json.cowboy-backup` with mode 0600
and persists an `initial_backup_done` flag in the `settings` table. Subsequent
runs skip the copy regardless of whether the backup file still exists. The
backup is the only rollback path now that the SQLite-backed snapshot history
has been removed.

## Cross-Platform Constraints

- Filesystem replacement behavior must be validated on Unix and Windows.
- Claude project-directory decoding must not rely only on lossy directory-name
  reconstruction; session `cwd` records are the stronger Windows fallback.
- Terminal raw mode and alternate-screen state must be restored on every error
  path.
- UI timestamp tests must not assume the developer machine's time zone.

## Testing Strategy

- Follow TDD: add a failing focused test before production changes.
- Keep pure domain and feature behavior covered by unit tests.
- Use temporary directories and databases for filesystem and SQLite tests.
- Run the complete Rust suite on Linux, macOS, and Windows.
- Release readiness also requires formatting, Clippy, and npm distribution
  validation.
