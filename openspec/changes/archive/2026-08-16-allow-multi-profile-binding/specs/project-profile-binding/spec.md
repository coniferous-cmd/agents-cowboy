## MODIFIED Requirements

### Requirement: Profile binding relationship
The system SHALL support binding a single profile to multiple projects (1:N relationship).

#### Scenario: Bind same profile to multiple projects
- **WHEN** user binds profile "work" to project A
- **AND** user binds profile "work" to project B
- **THEN** both projects show "[work]" in their project list
- **AND** the binding is persisted in the database

#### Scenario: Bind different profiles to same project
- **WHEN** user binds profile "work" to project A
- **AND** user binds profile "home" to project A
- **THEN** project A shows "[home]" (latest binding wins)
- **AND** the previous binding is replaced

### Requirement: Profile binding error handling
The system SHALL provide clear error messages when profile binding fails.

#### Scenario: Profile not found
- **WHEN** user attempts to bind non-existent profile "invalid"
- **THEN** system displays error "Profile 'invalid' not found"

#### Scenario: Binding succeeds
- **WHEN** user binds a valid profile to a project
- **THEN** system displays success message "Bound profile 'work' to project"
- **AND** the binding is persisted

### Requirement: Profile deletion with bindings
The system SHALL prevent deletion of profiles that have active bindings.

#### Scenario: Delete bound profile
- **WHEN** user attempts to delete profile "work" that is bound to project A
- **THEN** system displays error indicating profile is in use
- **AND** profile is not deleted

#### Scenario: Delete unbound profile
- **WHEN** user deletes profile "work" that has no bindings
- **THEN** profile is deleted successfully
- **AND** all associated files are removed
