## Purpose

Lets users duplicate an existing profile under a new name, preserving all settings JSON, for scenarios like creating a debug variant of a working profile.

## ADDED Requirements

### Requirement: Profile copy via CLI

The system SHALL support copying an existing profile via `cowboy config copy <source> <new-name>` in the CLI. The new profile SHALL contain an identical copy of the source profile's settings JSON. The new profile SHALL NOT inherit the source's project bindings.

#### Scenario: Copy with valid new name

- **WHEN** user runs `cowboy config copy work work-debug`
- **THEN** a new profile named `work-debug` is created with the same settings JSON as `work`
- **AND** the new profile is persisted in the database and filesystem

#### Scenario: Copy to duplicate name reports error

- **WHEN** user runs `cowboy config copy work work` and `work` already exists
- **THEN** the command SHALL fail with `profile already exists: work`

#### Scenario: Copy from nonexistent profile reports error

- **WHEN** user runs `cowboy config copy nonexistent new-profile`
- **THEN** the command SHALL fail with `profile not found: nonexistent`

#### Scenario: Copy with invalid new name reports error

- **WHEN** user runs `cowboy config copy work "invalid name"`
- **THEN** the command SHALL fail with a validation error describing name constraints

### Requirement: Profile copy via TUI

The system SHALL support copying a profile in the TUI via the Profiles tab. The user SHALL select a profile, press `c`, enter a new name in a modal, and confirm with Enter. The new profile SHALL contain an identical copy of the source profile's settings JSON.

#### Scenario: Copy modal pre-fills suggestion

- **WHEN** user presses `c` with profile `work` selected
- **THEN** the copy modal input SHALL be pre-filled with `Copy of work`

#### Scenario: Copy confirmed with Enter

- **WHEN** user types a valid new name in the copy modal and presses Enter
- **THEN** a new profile with that name is created, the profile list is refreshed, and the new profile is selected

#### Scenario: Copy canceled with Escape

- **WHEN** user presses Escape in the copy modal
- **THEN** the modal is dismissed and no profile is created

#### Scenario: Copy to duplicate name shows toast

- **WHEN** user attempts to copy `work` to `work` (which already exists)
- **THEN** the modal is dismissed and a toast error `Profile already exists: work` is shown

#### Scenario: Copy source selection

- **WHEN** user is on the Snapshots list (below profiles) and presses `c`
- **THEN** no copy modal is shown (copy only applies to profiles, not snapshots)
