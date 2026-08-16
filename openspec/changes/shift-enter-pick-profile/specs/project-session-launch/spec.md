## Purpose

Allows launching a new session in a project with a one-shot profile override, without permanently binding that profile to the project.

## ADDED Requirements

### Requirement: `p` key triggers profile picker

When the user presses `p` while a project (not "Open Here") is selected in the Projects pane, the system SHALL display a profile picker popup.

#### Scenario: `p` shows popup
- **WHEN** user focuses the Projects pane and presses `p` with a real project selected
- **THEN** a popup appears listing all available profiles with the active profile marked

#### Scenario: `p` on Open Here is ignored
- **WHEN** user presses `p` with "Open Here" selected
- **THEN** no popup appears and no action is taken

### Requirement: Popup navigation

The profile picker popup SHALL support navigation with `↑` and `↓` keys, and cancel with `q` or `Esc`.

#### Scenario: Navigate profiles
- **WHEN** the popup is displayed and user presses `↓`
- **THEN** the selection cursor moves to the next profile
- **WHEN** user presses `↑`
- **THEN** the selection cursor moves to the previous profile

#### Scenario: Cancel popup
- **WHEN** the popup is displayed and user presses `q` or `Esc`
- **THEN** the popup closes and no session is launched

### Requirement: Enter confirms selection and launches session

When the user presses `Enter` on a selected profile in the popup, the system SHALL launch a new Claude session in the selected project using that profile's settings file via the `--settings` CLI flag.

#### Scenario: Launch with picked profile
- **WHEN** popup is displayed, a profile is selected, and user presses `Enter`
- **THEN** the system launches `claude --settings <profile-path>` in the project's working directory
- **AND** the profile is NOT bound to the project afterward

### Requirement: Profile override does not persist

Launching a session through the profile picker SHALL NOT modify the project's stored profile binding. The project's binding remains whatever it was before the one-shot launch.

#### Scenario: One-shot does not change binding
- **GIVEN** project P is bound to profile "work"
- **WHEN** user launches project P via the profile picker and selects profile "personal"
- **THEN** project P is still bound to "work" after the session exits
