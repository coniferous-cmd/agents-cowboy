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

- Settings replacement is atomic and preserves private permissions or a stricter existing mode/ACL.
- A singleton activation journal plus a cross-process lock coordinates SQLite and filesystem state.
- Startup completes a journal whose exact-byte hash matches the settings file and records a mismatch as failed.
- `active_profile_name` records the last successful Profile activation; it is not inferred from file contents.

## First-launch backup (v4)

- The SQLite-backed snapshot history was removed in schema v4. Activation no
  longer inserts a row into `claude_settings_snapshots`; the table itself is
  dropped on upgrade.
- Replaced by a one-shot file backup: on the first launch that observes an
  existing `settings.json`, cowboy copies it to
  `<claude_config_dir>/settings.json.cowboy-backup` with mode `0600` and writes
  an `initial_backup_done` flag in the `settings` table. Subsequent runs skip
  the copy regardless of whether the backup file still exists.
- The decision trades fine-grained rollback history for code simplicity. Users
  who need finer history can keep their own profile JSON backups.

## Storage

- `settings` stores generic paths, launcher alias, `initial_backup_done`, and
  `active_profile_name`.
- `themes` stores presentation themes.
- `claude_profiles` stores named settings JSON.
- `profile_activation_journal` stores the single recoverable activation transition
  (target_kind is now `'profile'` only; target_name is `NOT NULL`).
- Schema version 1 removes the legacy env/project tables after privately dumping non-empty project settings rows.
- Schema version 4 drops `claude_settings_snapshots` and the `'snapshot'`
  alternative of `profile_activation_journal.target_kind`.

## TUI

- Projects and Profiles are the top-level tabs selected by `[` and `]`.
- The Projects tab hosts the existing two-column browser (Projects left, Sessions right); pane focus is independent from the selected top-level tab.
- Profiles tab renders a single panel listing all Profiles; `Enter` activates the focused Profile.
