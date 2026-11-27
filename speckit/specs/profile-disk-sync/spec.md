# profile-disk-sync Specification

## Purpose
TBD - created by archiving change add-config-sync. Update Purpose after archive.

## Requirements

### Requirement: Retired Profile data is left in place

Cowboy SHALL NOT sync, backfill, activate, recover, migrate, or clean up legacy Profile data. Cowboy SHALL NOT create an initial Claude settings backup, rewrite or restore the effective Claude `settings.json`, or change its symlink target. Existing Profile files, settings backups, and Profile-related database rows SHALL be left intact, including project bindings and activation state. Fresh installations SHALL NOT require Profile storage.

#### Scenario: Upgrade with an active Profile link and pending activation
- **WHEN** Cowboy starts with a legacy settings symlink, its target Profile file, an existing backup, and a pending Profile activation in SQLite
- **THEN** the symlink, target contents, backup, and Profile-related database rows remain unchanged
- **AND** Cowboy does not run Profile activation recovery

#### Scenario: Upgrade with a regular settings file and a missing mirror
- **WHEN** Cowboy starts with a regular Claude settings file, no Cowboy backup, and a legacy Profile row whose mirror is missing
- **THEN** Cowboy leaves the settings file and row unchanged and creates neither the backup nor the mirror

#### Scenario: Fresh metadata store
- **WHEN** Cowboy initializes a fresh metadata store
- **THEN** launcher alias and theme settings work without creating Profile tables or files

### Requirement: Upgrade documentation identifies legacy configuration dependencies

Cowboy's user documentation SHALL explain that existing project bindings no longer affect launch, and that an existing Claude settings symlink may still depend on the legacy Profile file. It SHALL document how users can manually preserve the currently effective settings as a regular file before discarding a linked Profile file.

#### Scenario: User reviews the removal notes before cleaning up old files
- **WHEN** a user with an existing Profile link reads the upgrade notes
- **THEN** the notes warn against deleting the link target before preserving the settings and distinguish preserving current settings from restoring the old backup
