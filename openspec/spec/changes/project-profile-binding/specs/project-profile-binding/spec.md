## Purpose

Lets users bind a profile to a project directory so that Claude sessions launched in that project automatically use the bound profile's settings via `--settings`, without requiring manual global profile activation.

## ADDED Requirements

### Requirement: Bind a profile to a project

The system SHALL allow users to associate a single profile with a project directory. Each project MAY have at most one bound profile. Binding a new profile to a project that already has one SHALL replace the previous binding.

#### Scenario: Bind profile to a project with no existing binding

- **WHEN** user binds profile "work" to project "/work/api"
- **THEN** the binding is persisted in the database
- **AND** subsequent Claude launches in "/work/api" use `--settings` pointing to the "work" profile's settings file

#### Scenario: Replace an existing binding

- **WHEN** project "/work/api" is bound to profile "work" and user binds profile "staging" to the same project
- **THEN** the binding is updated to "staging"
- **AND** the old binding to "work" is removed

#### Scenario: Bind a non-existent profile

- **WHEN** user attempts to bind profile "nonexistent" to a project
- **THEN** the system SHALL return a `ProfileNotFound` error
- **AND** no binding is created

### Requirement: Unbind a profile from a project

The system SHALL allow users to remove the profile binding from a project. After unbinding, Claude sessions in that project SHALL fall back to the global profile (symlink mechanism).

#### Scenario: Unbind an existing binding

- **WHEN** project "/work/api" is bound to profile "work" and user unbinds it
- **THEN** the binding is removed from the database
- **AND** subsequent Claude launches in "/work/api" do NOT pass `--settings`

#### Scenario: Unbind a project with no binding

- **WHEN** user attempts to unbind a project that has no binding
- **THEN** the system SHALL return a `BindingNotFound` error

### Requirement: Launch Claude with project settings

When launching a Claude session (new or resumed), the launcher SHALL check for a project binding. If a binding exists, the launcher SHALL pass `--settings <profile-file-path>` to the Claude CLI, where `<profile-file-path>` is the filesystem path to the bound profile's settings JSON file.

#### Scenario: Launch in a project with a binding

- **WHEN** user launches Claude in project "/work/api" bound to profile "work"
- **THEN** the command is `claude --settings <path-to-settings.work.json> --resume <id>` (or without `--resume` for new sessions)

#### Scenario: Launch in a project without a binding

- **WHEN** user launches Claude in project "/work/api" with no binding
- **THEN** the command is `claude --resume <id>` (or bare for new sessions)
- **AND** Claude reads settings from the global `~/.claude/settings.json` symlink as before

### Requirement: Prevent deletion of bound profiles

The system SHALL prevent deletion of a profile that has active project bindings. The user MUST unbind all projects from a profile before deleting it.

#### Scenario: Delete a profile with active bindings

- **WHEN** user attempts to delete profile "work" which is bound to project "/work/api"
- **THEN** the system SHALL return an error indicating the profile has active bindings
- **AND** the profile is NOT deleted

#### Scenario: Delete a profile after unbinding

- **WHEN** user unbinds all projects from profile "work" and then deletes it
- **THEN** the profile is deleted successfully

### Requirement: Display binding in TUI

The system SHALL display the bound profile name next to project names in the Projects tab when a binding exists. Projects without bindings SHALL display without a profile indicator.

#### Scenario: Project with binding is displayed

- **WHEN** project "/work/api" is bound to profile "work"
- **THEN** the project row shows the profile indicator (e.g., `[work]`)

#### Scenario: Project without binding is displayed

- **WHEN** project "/home/blog" has no binding
- **THEN** the project row shows no profile indicator

### Requirement: Bind via TUI

The system SHALL provide a keyboard shortcut in the Projects tab to bind the selected project to a profile. The user SHALL be able to select a profile from the list of available profiles.

#### Scenario: Bind via TUI

- **WHEN** user presses `e` on a selected project in the Projects tab
- **THEN** a profile selection modal appears listing available profiles
- **AND** upon selection, the binding is created

### Requirement: Bind via CLI

The system SHALL provide CLI subcommands `cowboy bind <project-path> <profile-name>` and `cowboy unbind <project-path>` for managing bindings outside the TUI.

#### Scenario: Bind via CLI

- **WHEN** user runs `cowboy bind /work/api work`
- **THEN** the binding is created

#### Scenario: Unbind via CLI

- **WHEN** user runs `cowboy unbind /work/api`
- **THEN** the binding is removed
