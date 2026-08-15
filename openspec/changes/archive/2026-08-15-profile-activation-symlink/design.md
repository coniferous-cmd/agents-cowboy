## Context

Today `activate_profile` does `AtomicReplace::write(~/.claude/settings.json, json)`. After this change, that target becomes a symlink and the real bytes live in `<profiles_dir>/settings.<name>.json`. SQLite (`claude_profiles.settings_json`) stays as the queryable mirror but is no longer the only place profile JSON exists. The crash-recovery journal already records a SHA-256 of "what should be on disk"; under the new layout the journal must reference the **target file's** hash, not the symlink's contents. The first activation after upgrade must transparently convert any pre-existing real `~/.claude/settings.json` into a `default` profile so users do not lose their current config.

The profile data path is fixed by `default_metadata_db_path()` (`src/claude_env/settings.rs:21`) and the `claude_config_dir` setting (`src/claude_env/store.rs:86`) controls where the symlink lives. These two paths are decoupled — `<profiles_dir>` is always under cowboy's data dir; the symlink is always at `<claude_config_dir>/settings.json`.

## Goals / Non-Goals

**Goals:**
- Move profile JSON out of a single shared file and into one file per profile, kept under cowboy's data directory.
- Make `~/.claude/settings.json` a transparent pointer to the active profile's file.
- Preserve atomicity for the target file write (the file Claude Code reads).
- Preserve crash safety through the existing journal by validating the symlink target's hash.
- Auto-import any pre-existing real settings file on first activation.
- Keep the SQLite schema and CLI surface unchanged so existing tests, history, and `config edit` keep working.

**Non-Goals:**
- Replicating the symlink to additional Claude-style settings files (`settings.local.json`, keybindings, MCP config). Only `settings.json` is in scope.
- Supporting Windows symlinks. The implementation uses `std::os::unix::fs::symlink`; Windows is documented as unsupported for activation via symlink (existing platform-specific code in `AtomicReplace` already gates on `cfg(unix)`).
- Switching `claude_profiles.settings_json` to a view or computed column. The SQLite column remains authoritative for queries; the file is the writable source for activation.
- Adding a `profiles_dir` user setting. The path is derived; users never configure it.

## Decisions

### 1. Derive `profiles_dir` from the metadata database path — no new setting

`default_metadata_db_path()` already returns the canonical cowboy DB location per OS. The profiles directory is its parent directory + `profiles/`:
- macOS: `~/Library/Application Support/cowboy/profiles/`
- Linux/other: `<config_dir>/cowboy/profiles/`

**Why:** The DB and the profile files belong to cowboy and should be co-located for backup, mode (`0o700`/`0o600`), and migration. Exposing another user setting creates configuration drift (the profiles dir could be moved out from under the DB). Derivation also means `ensure_private_dir` on the parent of the DB already covers the new directory's parent.

**Alternatives considered:**
- *New `profiles_dir` setting.* Rejected: another knob that can drift; the DB location already pins the right scope.
- *Inside `~/.claude/`.* Rejected: that dir is Claude Code's, not cowboy's; cowboy should not write into it beyond the symlink.

### 2. Activate via `AtomicReplace::write` on the target file, then `remove_file` + `symlink` on `~/.claude/settings.json`

`activate_profile` is decomposed into three ordered steps:

1. `AtomicReplace::write(<profiles_dir>/settings.<name>.json, profile.settings_json)` — preserves existing mode, atomic on Unix via `rename`, fsyncs the directory.
2. Replace `~/.claude/settings.json` with a symlink pointing at step 1's path. If the path exists as a symlink, `remove_file` then `symlink`. If it exists as a regular file, `remove_file` (migration handled the contents first) then `symlink`. If it does not exist, just `symlink`.
3. Insert the journal row keyed on the **target file's hash**, then `finish_activation(Some(name))`.

**Why:** The two writes are separated because atomic rename across the profile file and the symlink is not possible across filesystems or symlink/regular boundaries on Linux (no portable `renameat2(RENAME_EXCHANGE)`). Step 1 is the durable part; step 2 is best-effort with crash recovery to fix it. The journal's hash is computed on step 1's bytes so recovery can validate the right thing regardless of whether step 2 completed.

**Alternatives considered:**
- *Write symlink first, then target file.* Rejected: a symlink to a missing/incomplete target would let Claude Code see truncated JSON during the activation window.
- *Use a swap file pattern (`ln -sf` style `tmp_symlink` + `rename`).* Considered; rejected because `rename` over an existing regular file or symlink behaves differently across platforms. The current `unlink` + `symlink` sequence keeps the platform-specific bits inside `std::os::unix::fs::symlink`, matching the existing `AtomicReplace` style.

### 3. Migration runs lazily on the first `activate_profile` call

`activate_profile` checks three things at the top:
- `~/.claude/settings.json` exists as a regular file (not a symlink),
- its bytes parse as a JSON object via `validate_settings_json`,
- no `default` profile already exists in SQLite.

When all three hold, the contents are written to `<profiles_dir>/settings.default.json`, a `default` profile row is created, the real file is removed, and the symlink is installed. Then the requested activation continues.

**Why:** Lazy migration keeps the cold-start cost low (no scan on every CLI invocation) and ties the side effect to an explicit user action. A migration on first activation is also the first time cowboy would touch `~/.claude/settings.json` anyway, so there is no extra surface area.

**Alternatives considered:**
- *Eager migration at `initialize`.* Rejected: `initialize` runs on every CLI invocation including read-only commands; side effects there are surprising.
- *A `cowboy migrate` command.* Rejected as the primary path; users should not have to learn about it. Could be added later if rollback/inspection is needed.
- *Migrate on first `connect` of the store.* Rejected: same surprise concern as `initialize`.

### 4. Snapshot activation always uses `settings._orphan.json`

`activate_snapshot(id)` writes the snapshot JSON to `<profiles_dir>/settings._orphan.json` (overwriting any prior orphan) and symlinks `~/.claude/settings.json` to it. `finish_activation(None)` clears `active_profile_name`.

**Why:** Snapshots are not profiles; they have no name to expose via `list_profiles`. A single fixed orphan file keeps the directory tidy and makes "is the current state a snapshot?" answerable by checking whether the symlink target ends in `_orphan.json`. Subsequent profile activation does not delete the orphan — it's overwritten on the next snapshot activation.

**Alternatives considered:**
- *Per-snapshot file `settings.<snapshot_id>.json`.* Rejected: pollutes `list_profiles`-adjacent directory listings and confuses the "every profile has a name" invariant.
- *Remove the symlink entirely.* Rejected: breaks the atomic-replace narrative and would require `RecoveryOutcome` to model a "no file" state distinct from "broken symlink".

### 5. Dual-write ordering: SQLite first, file second; roll back SQLite on file failure

`update_profile_json(name, json)`:
1. `validate_settings_json(json)`.
2. Open a SQLite transaction and `UPDATE claude_profiles SET settings_json=?1`.
3. If row count is 0, abort with `ProfileNotFound` (no file touch).
4. `AtomicReplace::write(<profiles_dir>/settings.<name>.json, json)`. On error, the SQLite change has not been committed yet — drop the transaction and return the error.

The transaction commits only after step 4 succeeds, so SQLite and the file either both reflect the new JSON or neither does.

**Why:** Writing SQLite first makes the failure mode simpler — if the file write fails, we never advance the SQLite state. Putting file write inside the transaction window is fine because we are not committing SQLite work between them.

**Alternatives considered:**
- *File first, SQLite second.* Rejected: a SQLite failure after a successful file write would leave the on-disk file ahead of the database; recovery would need a separate reconciliation step.
- *Two separate transactions with explicit compensation.* Rejected: more code, harder to reason about, no benefit.

### 6. `delete_profile` removes the symlink when the active profile is removed

When the deleted profile matches `active_profile_name`, the function additionally calls `remove_file(~/.claude/settings.json)` and clears the `active_profile_name` setting. The deleted profile's file in `profiles/` is removed first; if the symlink removal fails, the user is left with a dangling symlink that recovery would mark failed on next launch.

**Why:** A dangling symlink is detectable by recovery (target file missing → `Failed` outcome). Leaving the symlink pointing at a now-deleted profile file would silently feed Claude Code a removed config. Removing it is the safer default.

**Alternatives considered:**
- *Auto-activate another profile (e.g., `default`).* Rejected: hides user intent; the user just deleted their active profile and may want to pick a new one explicitly.
- *Repoint symlink at `_orphan.json`.* Rejected: implies "snapshot semantics" for a deletion; confusing.

### 7. Recovery validates through the symlink

`recover_profile_activation`:
1. Read `~/.claude/settings.json` with `fs::symlink_metadata` to distinguish "missing", "regular file", "symlink". If regular file → mark `failed` (migration expected to run before activation).
2. If symlink, `fs::read_link` to get the target, then `fs::read(target)` and SHA-256.
3. Compare with the journal's `target_json_hash`. Match → `finish_activation(name)`. Mismatch → mark `failed`.

**Why:** The journal already records the expected hash; the only change is the read path — through the symlink instead of reading `settings.json` directly. Keeping the same journal shape avoids touching the schema or the migration code that created the table.

**Alternatives considered:**
- *Store the target path in the journal and resolve on recovery.* Rejected: the hash alone is sufficient and the target path is recoverable from the symlink itself; storing both is redundant.
- *Skip validation if the symlink is broken.* Rejected: a broken symlink is exactly what recovery needs to detect.

### 8. Tests follow the existing `tempdir` style with explicit assertions on symlink and target

The existing `profiles.rs` tests already use `tempdir` and write to a configurable `claude_config_dir`. They will be updated to:
- Assert on `fs::symlink_metadata` (file type) for `~/.claude/settings.json`.
- Read the symlink target via `fs::read_link` and verify it points at `<profiles_dir>/settings.<name>.json`.
- Verify the file's contents via `fs::read(target)`, not via `fs::read(~/.claude/settings.json)`.

New tests cover: first-activation migration, dual-write success and rollback, delete-active-removes-symlink, snapshot orphan, recovery through symlink, and the `profiles_dir` derivation. Platform-specific Unix mode checks remain gated on `#[cfg(unix)]` and are extended to also cover `profiles_dir`'s `0o700` mode.

## Risks / Trade-offs

- **Symlink briefly absent during step 2 of activation** → Claude Code reading `~/.claude/settings.json` mid-activation sees "file not found" for one syscall window. Mitigation: `activate_profile` is gated by `activation_lock`, and `AtomicReplace::write` on the target file is the only durable side effect; recovery on next launch handles the gap.
- **Cross-filesystem symlink failure** if `<profiles_dir>` and `~/.claude/` are on different filesystems → `std::os::unix::fs::symlink` would return `EXDEV`. Mitigation: derive `<profiles_dir>` from the metadata DB path which lives on the user's home filesystem; on macOS that is the same filesystem as `~/.claude/`. Document the assumption; if it ever breaks, surface it as a clear activation error.
- **External tooling overwriting the symlink** (e.g., a user or another tool writes `~/.claude/settings.json` as a regular file) → next `activate_profile` will re-run migration, creating a second `default` profile. Mitigation: gate migration on "no `default` profile yet" so duplicate imports are skipped; surface a warning if a regular file is detected post-upgrade.
- **Migration writes the user's current settings into `default` even if the user never wants to use them** → the `default` profile shows up in `list_profiles` and `delete_profile` works on it. Mitigation: document this in user-facing release notes; users who want it gone can `cowboy config delete default` after upgrade.
- **Snapshot activations accumulate orphan overwrites without cleanup** → multiple snapshot activations only ever have one `settings._orphan.json`. Mitigation: not a real risk; the file is overwritten in place via `AtomicReplace::write`.
- **Recovery on Windows** is not implemented for the symlink path → existing `replace_file` already gates on `cfg(unix)`; the new code paths will `cfg(unix)`-gate symlink creation and fall back to the current `AtomicReplace::write` semantics on Windows. Out of scope per Non-Goals.
- **Test fixtures that touch `claude_config_dir`** need to also create `profiles/` → the test helper `store()` already calls `upsert_setting(SETTING_CLAUDE_CONFIG_DIR)`; new fixtures additionally write a stub profile file or rely on `ensure_private_dir` creating the dir.

## Migration Plan

The change is delivered as a single OpenSpec change with tasks ordered for TDD:

1. Add `profiles_dir()` helper + derivation tests.
2. Rewrite `activate_profile` against target-file + symlink, update existing activation tests.
3. Rewrite `activate_snapshot` to write `settings._orphan.json`, update tests.
4. Add first-activation migration path + tests.
5. Extend `update_profile_json` to dual-write + tests.
6. Extend `delete_profile` to clean files and the active symlink + tests.
7. Adapt `recover_profile_activation` to follow the symlink + tests.
8. Add `profiles_dir` mode test and integration test that toggles `claude_config_dir`.
9. Run `cargo fmt --check`, `cargo test`, `cargo clippy --all-targets --all-features -- -D warnings`.

Rollback: revert the commit. No DB schema change; users who upgraded and activated at least once will have their real `~/.claude/settings.json` turned into a symlink, so a rollback should remind them to `mv <profiles_dir>/settings.default.json ~/.claude/settings.json` if they want the pre-change shape back. Document this in the release notes.

## Open Questions

- Should `update_profile_json` warn or refuse when the profile file is currently the symlink target and another process holds an open file descriptor? *Deferred:* `AtomicReplace::write` uses `rename` which on Unix replaces the inode, so existing readers see the old content; this is the same behavior as today and is fine. No spec or task changes.
- Do we want a `cowboy config export <name>` to dump a profile JSON to stdout for shell scripting? *Deferred to a follow-up change.* Out of scope here.
