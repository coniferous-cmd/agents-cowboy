# session-launch Specification

## Purpose
TBD — the introducing delta does not define a purpose.

## Requirements

### Requirement: Cowboy delegates configuration loading to Claude

Cowboy SHALL start and resume Claude in the intended working directory using the configured launcher alias or the existing default `claude`. Cowboy SHALL preserve `--resume <session-id>` for resume and SHALL NOT supply a Profile-derived `--settings` argument, including when legacy project bindings exist. Claude processes SHALL continue to inherit Cowboy's environment.

#### Scenario: Start a session in a previously bound project
- **WHEN** the user starts a new session in a project with a legacy binding and an existing Profile file
- **THEN** Cowboy invokes the configured launcher from the project directory without adding `--settings`

#### Scenario: Resume a session in a previously bound project
- **WHEN** the user resumes a session in a previously bound project without a custom launcher alias
- **THEN** Cowboy invokes `claude --resume <session-id>` from the session's working directory without adding `--settings`

### Requirement: Profile actions are no longer exposed

Cowboy SHALL offer Projects and Sessions navigation without the Profiles tab, Profile management dialogs, project binding controls, or a `Shift+Enter` Profile picker. The CLI SHALL remove `cowboy config` and its Profile subcommands from parsing and help. Cowboy SHALL retain `cowboy alias` and existing non-Profile project and session actions.

#### Scenario: User navigates the TUI
- **WHEN** the user cycles through Cowboy tabs or uses the former Profile shortcuts
- **THEN** only Projects and Sessions are available and no Profile picker or binding action opens
- **AND** the normal project and session actions remain available

#### Scenario: User calls a removed command
- **WHEN** the user invokes an old Profile command such as `cowboy config sync`
- **THEN** the CLI rejects it as unsupported without starting Profile operations
- **AND** `cowboy --help` no longer advertises `config` and still advertises `alias`
