# profile-disk-sync — Spec Deltas

## ADDED Requirements

#### Requirement: Every profile has a per-profile settings file

For every successfully created profile, cowboy SHALL atomically create
`<metadata_db_parent>/profiles/settings.<name>.json` containing the profile's
initial settings JSON. A failed file write SHALL fail profile creation and
leave no new database row. Before normal profile operations use legacy rows,
cowboy SHALL create a missing mirror from that row's stored JSON; it SHALL
never overwrite an existing mirror during this backfill.

##### Scenario: creating a profile creates its mirror
- **WHEN** the user creates profile `work`
- **THEN** `claude_profiles` contains `work` and
  `profiles/settings.work.json` exists with `{}`

##### Scenario: a failed mirror write rolls back creation
- **WHEN** cowboy cannot create `profiles/settings.work.json` while creating
  `work`
- **THEN** the command fails and no `work` row is committed to
  `claude_profiles`

##### Scenario: legacy profile is backfilled without overwriting a file
- **WHEN** a legacy `work` database row has no mirror file
- **THEN** cowboy creates `profiles/settings.work.json` from its stored JSON
- **AND WHEN** that file already exists
- **THEN** cowboy leaves its contents unchanged

#### Requirement: `cowboy config sync` subcommand

The `cowboy config` subcommand SHALL accept `sync [name]`. Without `<name>`,
it reconciles every valid `settings.<name>.json` file in `profiles_dir()`;
with `<name>`, it reconciles only that file. Extra positional arguments MUST
produce a usage error without touching storage.

##### Scenario: sync limits work to a named file
- **WHEN** the user runs `cowboy config sync work`
- **THEN** only `settings.work.json` is considered

#### Requirement: Disk files are imported into the database

For each valid profile file, sync SHALL validate that its content is a JSON
object, then insert a missing row, update a differing row, or report an equal
row as unchanged. Invalid or unreadable files SHALL be reported and SHALL not
stop other profiles. Sync SHALL not write profile files, alter the global
`~/.claude/settings.json` symlink, or modify profile bindings.

##### Scenario: a disk-only profile is imported
- **WHEN** `profiles/settings.newproj.json` contains valid JSON and no
  `newproj` row exists
- **THEN** sync inserts `newproj` with that JSON and reports it as `inserted`

##### Scenario: external file changes update the database
- **WHEN** `settings.work.json` differs from the `work` row
- **THEN** sync updates the row to the file content and reports `updated`

##### Scenario: invalid files do not stop other imports
- **WHEN** `settings.broken.json` is invalid JSON and
  `settings.home.json` is valid
- **THEN** `broken` is reported as invalid and `home` is still reconciled

#### Requirement: Sync output summarizes outcomes

The CLI SHALL write a human-readable stdout summary distinguishing `inserted`,
`updated`, `unchanged`, and `invalid`. It SHALL exit zero after a completed
reconcile even when individual files are invalid.

##### Scenario: mixed results are all visible
- **WHEN** sync imports, updates, leaves unchanged, and rejects invalid files
- **THEN** stdout lists every outcome and the command exits zero
