# remove-snapshot-feature — Spec Deltas

## Capability: first-launch-backup

### ADDED Requirements

#### Requirement: First-launch settings backup

The system SHALL, on its first observation of the Claude settings file, copy that file to a sibling `settings.json.cowboy-backup` if the settings file exists as a regular file. The backup is one-shot — it MUST NOT repeat on subsequent runs.

##### Scenario: Initial backup is created when settings.json exists as a regular file
- **WHEN** cowboy initializes and `claude_config_dir/settings.json` exists as a regular file and `initial_backup_done` is not yet `1` in the `settings` table
- **THEN** the system writes `claude_config_dir/settings.json.cowboy-backup` with the same bytes and stores `initial_backup_done = '1'` before any other operation touches the file

##### Scenario: Subsequent runs do not re-back up
- **WHEN** cowboy initializes and `initial_backup_done = '1'` is already stored
- **THEN** no file copy occurs regardless of whether `settings.json.cowboy-backup` still exists

##### Scenario: Missing settings.json is not an error
- **WHEN** cowboy initializes and `claude_config_dir/settings.json` does not exist
- **THEN** no backup file is created, no error is returned, and `initial_backup_done` is still written to skip future attempts

##### Scenario: Symlinked settings.json is skipped
- **WHEN** `claude_config_dir/settings.json` already exists as a symbolic link (because cowboy has already migrated the user's settings into a profile)
- **THEN** no backup is performed — the profile mechanism already preserves the contents

#### Requirement: Backup file location and permissions

The system SHALL write the backup alongside the source file at `<claude_config_dir>/settings.json.cowboy-backup`. On Unix, the backup file SHALL be created with mode `0600`.

##### Scenario: Backup file is colocated with settings.json
- **WHEN** the backup is performed
- **THEN** the resulting path is the same directory as `settings.json` and the filename is exactly `settings.json.cowboy-backup`

##### Scenario: Backup file inherits private permissions on Unix
- **WHEN** the backup is written on a Unix system
- **THEN** the resulting file has mode `0600`

#### Requirement: One-shot flag persists across runs

The system MUST use the `initial_backup_done` key in the `settings` table to record that the backup has been performed. The flag is independent of the file's existence on disk: manually deleting `settings.json.cowboy-backup` MUST NOT cause cowboy to recreate it on a subsequent run.

##### Scenario: Deleted backup file is not recreated
- **WHEN** cowboy initializes and `initial_backup_done = '1'` is stored but `settings.json.cowboy-backup` has been removed by the user
- **THEN** no backup is performed

---

## Capability: profiles

### REMOVED Requirements

#### Requirement: Snapshot Table (`claude_settings_snapshots`)

The `claude_settings_snapshots` table — including its `id`, `captured_at`, `source`, `settings_json` columns and the auto-prune-at-100 behavior — is removed. The system SHALL NOT create, read, or write any rows in this table. There is no replacement SQL-backed history; rollback to a previous configuration is provided by the `first-launch-backup` capability and by the profile mechanism itself.

##### Scenario: Activation does not write a snapshot row
- **WHEN** a profile is activated
- **THEN** no row is inserted into any snapshot table

#### Requirement: Pre-activation snapshot capture

The activation flow step that captured the current `settings.json` content into a snapshot row tagged `pre-activate:<name>` is removed. Activation proceeds from "validate target" directly to "atomic replace" without an intermediate snapshot write.

##### Scenario: Profile activation skips the capture step
- **WHEN** `activate_profile(name)` runs
- **THEN** the activation transaction contains no INSERT targeting the snapshot table, regardless of whether `settings.json` existed beforehand

#### Requirement: Auto-prune of historical snapshots

The `prune_snapshots_on` helper and the `AUTO_SNAPSHOT_LIMIT = 100` constant are removed. Activation completion MUST NOT include any SQL that deletes historical configuration rows.

##### Scenario: Activation finalization does not prune
- **WHEN** `finish_activation` runs
- **THEN** no SQL targeting the snapshot table is executed

### MODIFIED Requirements

#### Requirement: Activation Flow

The activation flow is restructured to remove the snapshot capture step. The remaining steps are renumbered.

##### Scenario: Numbered activation steps after the change
- **WHEN** the documentation or comments describe the activation flow
- **THEN** the steps read: (1) acquire lock, (2) validate target, (3) atomic replace, (4) commit state — with no step performing a snapshot insert

#### Requirement: Profile tab layout

The Profiles tab page no longer contains a Snapshots subpanel. The page SHALL contain a single Profiles list that occupies the full height allocated to the tab.

##### Scenario: Profiles page renders a single panel
- **WHEN** the Profiles tab is active
- **THEN** the layout produces a single bordered list of profiles and no second list titled "Snapshots"

#### Requirement: Profile keybindings

The profile keybinding model returns to a single-list cursor (profile index only). The `Enter` key activates the focused profile. There is no separate "activate snapshot" action and the previous disabled-on-snapshot guards are removed.

##### Scenario: Cursor stays in profile range
- **WHEN** the user moves the cursor with `j`/`k` or `↓`/`↑` on the Profiles tab
- **THEN** the cursor index is always within `[0, profiles.len())`

---

## Capability: profile-activation

### REMOVED Requirements

#### Requirement: Snapshot activation writes an orphan file

The ability to activate a historical snapshot by writing its JSON to `<profiles_dir>/settings._orphan.json` is removed. There is no `activate_snapshot(id)` method on `ClaudeEnvStore` after this change.

##### Scenario: No snapshot activation method exists
- **WHEN** the implementation is searched for snapshot activation
- **THEN** no method or function named `activate_snapshot` exists, and no path writes to `settings._orphan.json`

##### Scenario: Activating a profile never produces an orphan file
- **WHEN** any profile is activated
- **THEN** `<profiles_dir>/settings._orphan.json` is never created or rewritten

#### Requirement: Profile activation captures a snapshot of the current file

The `perform_activation` helper no longer accepts a `snapshot_source` argument and no longer INSERTs into `claude_settings_snapshots`. The implicit pre-activation backup that previously lived in this code path is replaced by the one-shot `first-launch-backup` capability.

##### Scenario: perform_activation signature has no snapshot parameter
- **WHEN** `perform_activation` is called
- **THEN** the call site passes no snapshot source argument and the helper executes no INSERT against a snapshot table

#### Requirement: Profile activation journal uses `target_kind = 'profile'` only

The `profile_activation_journal.target_kind` CHECK constraint is restricted to the single value `'profile'`. The old alternative `'snapshot'` is no longer a valid value and the CHECK constraint that conditioned `target_name` on `target_kind` is dropped.

##### Scenario: Journal CHECK constraint no longer references 'snapshot'
- **WHEN** the journal schema is read
- **THEN** `target_kind` only allows `'profile'` and there is no CHECK on `target_name IS NULL`

##### Scenario: No orphan rows remain after v4 migration
- **WHEN** the v3 → v4 migration completes
- **THEN** no row with `target_kind = 'snapshot'` exists in `profile_activation_journal`

### MODIFIED Requirements

#### Requirement: Activation journal recovery

Recovery semantics are unchanged for the `'profile'` kind: the journal is consulted on startup, the symlink target hash is compared to the recorded hash, and the journal is either finalized or marked failed.

##### Scenario: Recovery flow is unchanged for profile activations
- **WHEN** cowboy starts and a journal entry with `target_kind = 'profile'` exists
- **THEN** behavior is identical to the prior implementation: matching hash completes activation, mismatched or missing symlink marks failed
