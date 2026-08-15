## 1. Profiles directory helper

- [x] 1.1 Add failing test `profiles_dir_is_cowboy_data_profiles_subdirectory` asserting the path equals `<metadata_db_parent>/profiles` for the macOS and Linux fixtures
- [x] 1.2 Implement `ClaudeEnvStore::profiles_dir()` in `src/claude_env/profiles.rs`, deriving from `self.path().parent()`
- [x] 1.3 Add `#[cfg(unix)]` test `profiles_dir_is_created_with_private_mode` asserting the directory is created with mode `0o700` on first access

## 2. Symlink replacement helper

- [x] 2.1 Add failing test `replace_with_symlink_creates_link_for_missing_target` calling `replace_with_symlink(~/.claude/settings.json, <profiles_dir>/settings.work.json)` when neither path exists
- [x] 2.2 Add failing test `replace_with_symlink_swaps_when_target_is_regular_file` writing a real file first, then asserting the symlink replaces it and the original bytes are gone
- [x] 2.3 Add failing test `replace_with_symlink_repoints_when_target_is_symlink` creating a symlink to one file, then asserting a second call repoints it
- [x] 2.4 Implement `replace_with_symlink(target_path, link_path)` using `cfg(unix)` `std::os::unix::fs::symlink`; fall back to the current `AtomicReplace::write` semantics on non-Unix with a clear error

## 3. activate_profile rewrite

- [x] 3.1 Update existing test `activation_captures_exact_snapshot_and_snapshot_restore_does_not_recurse` to assert `fs::symlink_metadata(settings.json)` is a symlink whose target is `<profiles_dir>/settings.work.json`
- [x] 3.2 Add failing test `activate_profile_writes_target_file_with_private_mode` covering the new mode guarantee on the profile file
- [x] 3.3 Add failing test `activate_profile_swaps_symlink_when_activating_a_second_profile` asserting two consecutive activations leave the symlink pointing at the second profile's file and the first file untouched
- [x] 3.4 Add failing test `activate_profile_journal_hash_targets_the_profile_file` asserting the recorded hash equals SHA-256 of the profile file, not the symlink
- [x] 3.5 Rewrite `activate_profile` in `src/claude_env/profiles.rs` to (a) write the target file via `AtomicReplace::write`, (b) call `replace_with_symlink`, (c) insert the journal with the target file's hash, (d) finish activation
- [x] 3.6 Refactor: extract the profile-file-path helper `profile_file_path(name)` so `activate_profile` and `update_profile_json` share it

## 4. activate_snapshot rewrite

- [x] 4.1 Update existing snapshot test `activate_snapshot_*` to assert the symlink target is `<profiles_dir>/settings._orphan.json` and that `active_profile_name` is `None`
- [x] 4.2 Add failing test `activate_snapshot_overwrites_prior_orphan_file` asserting a second snapshot activation overwrites `_orphan.json` via `AtomicReplace::write`
- [x] 4.3 Rewrite `activate_snapshot` in `src/claude_env/profiles.rs` to write `<profiles_dir>/settings._orphan.json` and call `replace_with_symlink` + `finish_activation(None)`
- [x] 4.4 Refactor: extract the shared "write file, symlink, journal, finish" core between `activate_profile` and `activate_snapshot`

## 5. First-activation migration

- [x] 5.1 Add failing test `first_activation_migrates_existing_real_settings_into_default_profile` asserting a real `settings.json` with valid JSON becomes a `default` profile, a `settings.default.json` file, and a symlink
- [x] 5.2 Add failing test `first_activation_skips_migration_when_default_already_exists` asserting migration is a no-op when the `default` profile is already in SQLite
- [x] 5.3 Add failing test `first_activation_aborts_when_existing_settings_are_invalid_json` asserting `activate_profile` returns an error and the real file is left untouched
- [x] 5.4 Add failing test `first_activation_aborts_when_existing_settings_root_is_not_object` asserting `[]` / `null` / primitive roots are rejected
- [x] 5.5 Implement migration as the first step inside `activate_profile` (gated on "no `default` profile exists" + "real file exists" + "valid JSON"), then continue with the rewrite from §3

## 6. update_profile_json dual-write

- [x] 6.1 Add failing test `update_profile_json_syncs_profile_file` asserting the SQLite row and `<profiles_dir>/settings.<name>.json` both equal the new JSON after a successful update
- [x] 6.2 Add failing test `update_profile_json_rolls_back_when_file_write_fails` (chmod the directory read-only, call `update_profile_json`, assert SQLite is unchanged)
- [x] 6.3 Extend `update_profile_json` to call `AtomicReplace::write(<profiles_dir>/settings.<name>.json, json)` inside the SQLite transaction window; abort the transaction if the file write fails
- [x] 6.4 Add `#[cfg(unix)]` test `update_profile_json_preserves_profile_file_mode` asserting the mode of an existing profile file is preserved on update

## 7. delete_profile cleanup

- [x] 7.1 Add failing test `delete_inactive_profile_removes_only_its_file` asserting the SQLite row and profile file are gone, but the active symlink and its target file are untouched
- [x] 7.2 Add failing test `delete_active_profile_removes_symlink_and_clears_active` asserting `~/.claude/settings.json` no longer exists and `active_profile_name` is `None`
- [x] 7.3 Add failing test `delete_active_profile_does_not_auto_activate_another` asserting no other profile is auto-promoted after deletion
- [x] 7.4 Extend `delete_profile` to remove `<profiles_dir>/settings.<name>.json` and, when the profile matches `active_profile_name`, remove the symlink and clear the `active_profile_name` setting

## 8. Crash recovery through the symlink

- [x] 8.1 Update existing test `recovery_completes_matching_journal_and_marks_mismatch_failed` so the journal points at `<profiles_dir>/settings.<name>.json` and the assertion uses `fs::read_link` then `fs::read(target)` for the hash
- [x] 8.2 Add failing test `recovery_marks_broken_symlink_failed` asserting that a symlink whose target was deleted causes `RecoveryOutcome::Failed`
- [x] 8.3 Add failing test `recovery_marks_regular_file_failed` asserting that if `~/.claude/settings.json` has been replaced by a regular file (someone bypassed cowboy), recovery marks the journal failed rather than auto-fixing it
- [x] 8.4 Update `recover_profile_activation` to resolve the symlink to its target file before computing/validating the hash; on missing/regular-file symlink mark the journal `failed`

## 9. Profiles directory mode and cross-dir integration

- [x] 9.1 Add `#[cfg(unix)]` test `profiles_dir_is_created_with_private_mode_on_first_activation` (already partly covered by §1.3; assert the directory mode after `activate_profile` runs)
- [x] 9.2 Add integration test `changing_claude_config_dir_moves_the_symlink_target` asserting that updating the `claude_config_dir` setting and activating again relocates `~/.claude/settings.json` to the new dir without touching the profile file

## 10. Validation pass

- [x] 10.1 Run `cargo fmt --check` and apply formatting if it fails
- [x] 10.2 Run `cargo test` and ensure the full suite passes (existing + new tests)
- [x] 10.3 Run `cargo clippy --all-targets --all-features -- -D warnings` and resolve any lints
- [x] 10.4 If any of the above cannot run, record the reason in the implementation summary (per AGENTS.md)
