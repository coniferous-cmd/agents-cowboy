## Purpose

Defines how cowboy stores Claude profiles as files, swaps `~/.claude/settings.json` to a symlink, migrates existing real settings files, and keeps SQLite in sync — covering activation, snapshot activation, dual-write updates, deletion, and crash recovery.

## ADDED Requirements

### Requirement: File-backed profile storage

The system SHALL store each profile's JSON payload as `<profiles_dir>/settings.<name>.json`, where `<profiles_dir>` is derived from cowboy's metadata database path (macOS: `~/Library/Application Support/cowboy/profiles/`, Linux/other: `<config_dir>/cowboy/profiles/`). The profile file SHALL be created with mode `0o600` on Unix and replaced atomically.

#### Scenario: Profile file is created with private mode
- **WHEN** a new profile `work` is activated and the file does not exist
- **THEN** `<profiles_dir>/settings.work.json` exists with mode `0o600` and contains exactly the profile's JSON

#### Scenario: Profile file is atomically replaced on activation
- **WHEN** profile `work` is activated and `settings.work.json` already exists
- **THEN** the file is replaced atomically (no truncated intermediate state observable to readers) and its mode is preserved

### Requirement: Symlink to the active profile

The system SHALL make `~/.claude/settings.json` a symbolic link that points to the profile file corresponding to the active profile. Claude Code reading `~/.claude/settings.json` SHALL observe the active profile's JSON content.

#### Scenario: Activation produces a working symlink
- **WHEN** profile `work` is activated
- **THEN** `~/.claude/settings.json` is a symbolic link whose target resolves to `<profiles_dir>/settings.work.json`, and reading the symlink returns the profile JSON

#### Scenario: Re-activation swaps the symlink target
- **WHEN** profile `home` is activated while `work` is currently active
- **THEN** `~/.claude/settings.json` is a symbolic link whose target now resolves to `<profiles_dir>/settings.home.json`, and `settings.work.json` is untouched

### Requirement: First-activation migration of an existing real settings file

The system SHALL detect, on the first profile activation in a session where the SQLite store does not yet have a `default` profile, whether `~/.claude/settings.json` exists as a regular file (not a symlink) with valid JSON. When detected, the system SHALL register the contents as a `default` profile, write them to `<profiles_dir>/settings.default.json`, and replace `~/.claude/settings.json` with a symlink to that file before performing the requested activation.

#### Scenario: Existing real settings are imported as default
- **WHEN** `~/.claude/settings.json` exists as a regular file containing valid JSON, and the user activates any profile for the first time
- **THEN** a `default` profile appears in `list_profiles`, `<profiles_dir>/settings.default.json` exists with the imported JSON, and `~/.claude/settings.json` is now a symlink

#### Scenario: Already-migrated stores do not re-import
- **WHEN** a `default` profile already exists in SQLite and `~/.claude/settings.json` is already a symlink
- **THEN** activation proceeds without re-reading or rewriting the symlink target

#### Scenario: Invalid existing settings abort activation
- **WHEN** `~/.claude/settings.json` exists as a regular file containing invalid JSON
- **THEN** the requested activation fails, no profile file is written, no symlink is replaced, and `~/.claude/settings.json` remains a regular file with its original contents

### Requirement: Snapshot activation writes an orphan file

The system SHALL activate a historical snapshot by writing the snapshot JSON to `<profiles_dir>/settings._orphan.json` and symlinking `~/.claude/settings.json` to it. No profile name SHALL be set as active after snapshot activation.

#### Scenario: Snapshot activation clears the active profile
- **WHEN** a snapshot is activated
- **THEN** `~/.claude/settings.json` is a symbolic link whose target resolves to `<profiles_dir>/settings._orphan.json`, and `active_profile_name` returns `None`

#### Scenario: Re-activating a profile clears the orphan
- **WHEN** a profile is activated after a snapshot activation
- **THEN** the symlink target becomes `<profiles_dir>/settings.<name>.json`, and `<profiles_dir>/settings._orphan.json` is left untouched

### Requirement: Dual-write on profile JSON edits

The system SHALL persist every successful edit to a profile's JSON to both SQLite (`claude_profiles.settings_json`) and `<profiles_dir>/settings.<name>.json` before reporting success. A failure in either write SHALL cause the operation to return an error and leave the previously persisted state unchanged.

#### Scenario: Edit updates both stores
- **WHEN** `update_profile_json("work", json)` succeeds
- **THEN** `profile("work").settings_json` equals `json` and `<profiles_dir>/settings.work.json` contains `json`

#### Scenario: File write failure aborts the edit
- **WHEN** `<profiles_dir>/settings.<name>.json` cannot be written (e.g., directory read-only)
- **THEN** `update_profile_json` returns an error and the SQLite row is unchanged

### Requirement: Profile deletion cleans up files and active symlink

The system SHALL remove both the SQLite row and `<profiles_dir>/settings.<name>.json` when a profile is deleted. If the deleted profile is the currently active one, the system SHALL remove the symlink at `~/.claude/settings.json` and clear `active_profile_name`.

#### Scenario: Deleting an inactive profile removes only its file
- **WHEN** profile `home` is deleted while `work` is active
- **THEN** `list_profiles` no longer contains `home`, `<profiles_dir>/settings.home.json` does not exist, `~/.claude/settings.json` still resolves to `settings.work.json`, and `active_profile_name` is still `Some("work")`

#### Scenario: Deleting the active profile removes the symlink
- **WHEN** profile `work` is deleted while it is active
- **THEN** `~/.claude/settings.json` no longer exists, `active_profile_name` is `None`, and no other profile is auto-activated

### Requirement: Crash recovery follows the symlink chain

The system SHALL record, in the activation journal, the SHA-256 hash of the profile or orphan **file** the symlink is expected to point to. On recovery, the system SHALL resolve `~/.claude/settings.json` to its target file and compare that file's hash to the journal entry.

#### Scenario: Recovery completes when the symlink target matches
- **WHEN** the process is restarted after a partial activation and `~/.claude/settings.json` is a symlink whose target file's hash matches the journal
- **THEN** the journal is cleared and the active profile name (if any) is finalized

#### Scenario: Recovery marks mismatch as failed
- **WHEN** the symlink target file's hash does not match the journal, or the symlink is missing or broken
- **THEN** the journal is marked `failed` with a descriptive message, and the system returns a `Failed` recovery outcome

### Requirement: Profiles directory is private and co-located with the database

The system SHALL create `<profiles_dir>` on demand with mode `0o700` on Unix. `<profiles_dir>` SHALL be derived from the metadata database path (the directory containing `cowboy.db`) without requiring an additional user-facing setting.

#### Scenario: Profiles directory is created with private mode
- **WHEN** any activation operation runs and `<profiles_dir>` does not exist
- **THEN** the directory is created with mode `0o700` on Unix and the operation continues
