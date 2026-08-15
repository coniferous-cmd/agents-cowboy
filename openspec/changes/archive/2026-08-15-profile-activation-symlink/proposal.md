## Why

Today, activating a Claude profile rewrites `~/.claude/settings.json` in place — the JSON payload travels from SQLite through `AtomicReplace::write` into a single global file. This makes profile state invisible on disk, blocks any tooling that wants to read or diff individual profiles, and forces every activation to either rewrite or back up the global file. We want each profile to live as its own `settings.<name>.json` file inside cowboy's data directory, with `~/.claude/settings.json` becoming a symlink that always points at the active profile's file. This makes activation a cheap `ln -sf` swap, lets users browse and edit profile JSON directly, and keeps the SQLite row as a redundant mirror for queries and history.

## What Changes

- **Add** a new `profiles/` directory under cowboy's data directory (macOS: `~/Library/Application Support/cowboy/profiles/`, Linux: `~/.config/cowboy/profiles/`). Each profile lives as `settings.<name>.json` inside it with mode `0o600`.
- **Modify** `activate_profile(name)` so it (1) writes the profile JSON to `<profiles_dir>/settings.<name>.json` via `AtomicReplace::write`, then (2) replaces `~/.claude/settings.json` with a symlink pointing at that file. The activation journal now records the hash of the **target file** (not the symlink itself).
- **Modify** `activate_snapshot(id)` to write the snapshot JSON to `<profiles_dir>/settings._orphan.json` and symlink `~/.claude/settings.json` to it; `active_profile_name` is cleared as today.
- **Modify** `update_profile_json(name, json)` to dual-write: SQLite first, then `<profiles_dir>/settings.<name>.json` via `AtomicReplace::write`.
- **Modify** `delete_profile(name)` to remove both the SQLite row and the corresponding profile file. If the deleted profile is the active one, the symlink is removed (no auto-reactivation).
- **Add** first-activation migration: when `~/.claude/settings.json` exists as a regular file (not a symlink) and contains valid JSON, import it as a `default` profile (written to `<profiles_dir>/settings.default.json` and registered in SQLite), then replace the global file with a symlink. Migration runs lazily on the first `activate_profile` call after upgrade.
- **Modify** `recover_profile_activation` to follow the symlink at `~/.claude/settings.json` and validate the hash of the symlink target.
- **BREAKING** for the on-disk shape of `~/.claude/settings.json`: it transitions from a regular file to a symlink. Existing users must run any `cowboy config activate <name>` once after upgrading; before that, Claude Code still reads the (now-relocated) file as before.
- **BREAKING** for any external tooling that opens `~/.claude/settings.json` and expects to write back to it; such writes will replace the symlink with a regular file and must be re-imported.

## Capabilities

### New Capabilities

- `profile-activation`: Covers file-backed profile storage, symlink management, snapshot-to-orphan handling, dual-write semantics, first-activation migration, and crash recovery through the symlink chain.

### Modified Capabilities

_None._ This project has no existing specs to modify — `openspec/specs/` is empty.

## Impact

- `src/claude_env/profiles.rs` — rewrite `activate_profile`, `activate_snapshot`, `update_profile_json`, `delete_profile`, `recover_profile_activation`; add `profiles_dir()` helper.
- `src/claude_env/settings.rs` — derive the profiles directory from `default_metadata_db_path` (no new setting; computed).
- `src/claude_env/store.rs` — unchanged API; transactions still cover SQLite writes.
- `src/claude_env/schema.rs` — unchanged; `claude_profiles.settings_json` remains the SQLite mirror.
- `src/cmd/config.rs` — CLI surface unchanged; outputs now reference the symlink path (same string, but `ls -la` will reveal the chain).
- Tests in `src/claude_env/profiles.rs` — existing activation/snapshot/recovery/permissions tests must be adapted to assert on the symlink and target file; new tests cover migration, dual-write, delete-with-active-symlink, and orphan snapshot handling.
- `~/.claude/settings.json` — transitions from regular file to symlink for every user after first activation. The file's mode is no longer cowboy-controlled once it becomes a symlink (it follows the target's mode, which is `0o600`).
- New `profiles/` directory under cowboy's data dir — added on demand by `ensure_private_dir`; mode `0o700`.
