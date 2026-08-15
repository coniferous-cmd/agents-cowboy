# Decisions

## Session source of truth

- Claude session JSONL under the configured projects directory remains the source of truth.
- SQLite is not used for session discovery.
- New and resumed Claude processes inherit the parent process environment unchanged.

## Profiles

- Profiles are the only cowboy-owned Claude settings management entry point.
- A Profile contains a complete JSON object and may use any Claude Code settings key.
- Names are normalized to ASCII lowercase and allow only letters, digits, `-`, and `_`, up to 64 characters.
- The TUI can create and delete Profiles; editing profile JSON remains CLI-only.
- The old `export` command, project settings editor, environment metadata, and runtime environment injection are removed.

## Safe activation

- Activating a Profile captures the previous valid global settings file as a SQLite snapshot.
- Snapshot activation is non-recursive and may replace a missing or damaged current file.
- Settings replacement is atomic and preserves private permissions or a stricter existing mode/ACL.
- A singleton activation journal plus a cross-process lock coordinates SQLite and filesystem state.
- Startup completes a journal whose exact-byte hash matches the settings file and records a mismatch as failed.
- `active_profile_name` records the last successful Profile activation; it is not inferred from file contents.

## Storage

- `settings` stores generic paths, launcher alias, and `active_profile_name`.
- `themes` stores presentation themes.
- `claude_profiles` stores named settings JSON.
- `claude_settings_snapshots` stores pre-activation history.
- `profile_activation_journal` stores the single recoverable activation transition.
- Schema version 1 removes the legacy env/project tables after privately dumping non-empty project settings rows.

## TUI

- Projects and Profiles are the top-level tabs selected by `[` and `]`.
- The Projects tab hosts the existing two-column browser (Projects left, Sessions right); pane focus is independent from the selected top-level tab.
- Profiles has a continuous cursor over Profile and snapshot rows. Enter activates the focused Profile or snapshot.
