# profile-disk-sync — Spec Deltas

## ADDED Requirements

#### Requirement: `cowboy config sync` subcommand

The `cowboy config` subcommand SHALL accept `sync` as a subcommand verb, sibling to `list`, `create`, `edit`, `delete`, `activate`, `bind`, `unbind`, and `copy`. The subcommand accepts an optional positional argument `<name>` that names a single profile. If `<name>` is omitted, the sync operation covers every profile file discovered on disk. Extra positional arguments after `<name>` MUST be rejected with a usage error.

##### Scenario: sync with no name reconciles every profile on disk
- **WHEN** the user runs `cowboy config sync` and the `profiles_dir` contains `settings.work.json` and `settings.home.json`
- **THEN** both `work` and `home` are reconciled against their on-disk files according to the per-file reconcile rules

##### Scenario: sync with a name limits reconciliation to that profile
- **WHEN** the user runs `cowboy config sync work` and `settings.work.json` is present on disk
- **THEN** only `work` is reconciled and `home` is not touched, regardless of any other files in `profiles_dir`

##### Scenario: sync subcommand rejects extra positional arguments
- **WHEN** the user runs `cowboy config sync work extra` or any form with more than one positional argument after `sync`
- **THEN** the CLI rejects the invocation with a usage error and exits non-zero without touching the database or the filesystem

#### Requirement: Source directory is the per-profile mirror next to the database

The sync operation SHALL read files from `profiles_dir()`, which is `<metadata_db_parent>/profiles/`, and SHALL read any file whose name matches `settings.<name>.json` where `<name>` is a valid profile name (lowercase ASCII letters, digits, `-`, `_`, length 1–64). Files that do not match this pattern SHALL be ignored.

##### Scenario: Files outside the mirror directory are not read
- **WHEN** the sync operation runs
- **THEN** no file outside `profiles_dir()` is read, including the global `~/.claude/settings.json` symlink, the cowboy-backup file, or any other path

##### Scenario: Files with non-conforming names in profiles_dir are ignored
- **WHEN** `profiles_dir()` contains both `settings.work.json` and an unrelated file such as `notes.txt` or `settings..json`
- **THEN** only `settings.work.json` is reconciled; `notes.txt` and `settings..json` are silently ignored

#### Requirement: Per-file reconcile behavior

For each profile file discovered, the sync operation SHALL apply the following rules in order:

1. If the file content does not parse as a JSON object: the profile is added to a `SyncReport.invalid` list with the parse error and no database write is performed for that name; processing continues with the next file.
2. If the file content parses as a valid JSON object:
   - If `claude_profiles` does not contain a row with that `name`, a new row SHALL be inserted with the file's content as `settings_json`.
   - If `claude_profiles` contains a row with that `name` and `settings_json` differs from the file content, the row SHALL be updated to match the file content.
   - If the row's `settings_json` already equals the file content, no write is performed.
3. If the file is missing from `profiles_dir()` for a name that does exist in the database, the database row SHALL be left untouched and no entry SHALL be added to any list in `SyncReport`.

##### Scenario: A file with no matching DB row results in an insert
- **WHEN** `profiles_dir/settings.newproj.json` exists, parses as a JSON object, and `claude_profiles` has no row named `newproj`
- **THEN** a row for `newproj` is inserted with `settings_json` equal to the file's parsed value, and `newproj` appears in `SyncReport.inserted`

##### Scenario: A file whose content differs from the DB row results in an update
- **WHEN** `profiles_dir/settings.work.json` parses as `{"key":"new"}` and the `work` row in `claude_profiles` has `settings_json` equal to `{"key":"old"}`
- **THEN** the `work` row is updated to `{"key":"new"}` and `work` appears in `SyncReport.updated`

##### Scenario: A file whose content equals the DB row is a no-op
- **WHEN** `profiles_dir/settings.work.json` parses as a JSON object whose textual form equals the `work` row's `settings_json`
- **THEN** no SQL write is performed for `work` and it appears in `SyncReport.unchanged`

##### Scenario: An unparseable file is skipped and recorded
- **WHEN** `profiles_dir/settings.broken.json` does not parse as a JSON object (e.g. trailing comma, root array, syntax error)
- **THEN** no SQL write is performed for `broken`, the parse error is captured in `SyncReport.invalid`, and reconciliation continues for any remaining files

##### Scenario: A missing file is not a reason to delete the DB row
- **WHEN** the `work` row exists in `claude_profiles` and `profiles_dir/settings.work.json` does not exist
- **THEN** the `work` row is left unchanged and `work` does not appear in any list of `SyncReport`

#### Requirement: Sync does not touch the global settings symlink

The sync operation SHALL NOT read, write, symlink, or unsymlink `~/.claude/settings.json` or any path outside `profiles_dir()`. Drift between the on-disk profile file and the symlink target is not addressed by sync; users address such drift by running `cowboy config activate <name>`.

##### Scenario: Symlink target is not modified by sync
- **WHEN** the user runs `cowboy config sync work`
- **THEN** `~/.claude/settings.json`'s symlink target is exactly what it was before the command, even if the file pointed at was renamed or deleted

#### Requirement: Sync does not modify profile bindings

The sync operation SHALL NOT read, insert into, update, or delete rows in `project_profile_bindings`. Sync is a content-only reconcile.

##### Scenario: Bindings are preserved across sync
- **WHEN** the `work` profile is bound to project `/repos/a` and `/repos/b` in `project_profile_bindings`, and the user runs `cowboy config sync work`
- **THEN** both bindings remain in `project_profile_bindings` after the command completes, regardless of whether the `work` JSON content changed

#### Requirement: Reconcile is per-profile transactional

Each profile's reconcile SHALL run inside its own SQLite transaction. A failure in one profile's reconcile (for example, a disk-write error during INSERT, or a constraint violation) MUST NOT cause other profiles in the same invocation to be skipped — each profile's outcome SHALL be recorded independently in `SyncReport`.

##### Scenario: A failure on one profile does not abort the rest
- **WHEN** the sync operation encounters an unexpected error on `work` while attempting to INSERT it
- **THEN** the error is recorded in `SyncReport`, `home` and any other profiles in the same invocation are still reconciled, and the command exits with a non-zero status only when at least one error is recorded AND no successful reconcile occurred (in practice: exit code is always zero when the CLI ran to completion, because per-file errors land in the report and the report itself is the result)

#### Requirement: CLI output summarizes reconcile results

The CLI SHALL print, on stdout, a human-readable summary of each profile reconciled and its outcome. The summary SHALL distinguish at minimum: `inserted`, `updated`, `unchanged`, and `invalid` (with the parse error). The exit code SHALL be zero whenever the operation reached completion, regardless of how many entries fell into the `invalid` list.

##### Scenario: A clean sync prints unchanged names and exits zero
- **WHEN** `cowboy config sync` runs and every disk file already matches its DB row
- **THEN** every name is listed as `unchanged` in stdout, no row is written, and the exit code is zero

##### Scenario: Mixed outcomes are all reported
- **WHEN** `cowboy config sync` runs and `newproj` is inserted, `work` is updated, `home` is unchanged, and `broken` is invalid JSON
- **THEN** stdout lists each of those four outcomes and the exit code is zero
