## Why

Profile settings currently have two representations: a SQLite row and a
per-profile JSON file. External edits to the file are invisible to cowboy
until they are imported, and creating a profile can leave it with no file at
all. That makes the documented file mirror incomplete and risks a later
cowboy edit overwriting an external change.

## What Changes

- Add `cowboy config sync [name]` to import JSON from
  `profiles/settings.<name>.json` into `claude_profiles`.
- Make profile creation atomically create `profiles/settings.<name>.json`,
  initially containing `{}`. A successfully created profile therefore always
  has its matching mirror file.
- Preserve the one-way meaning of `sync`: files are the source for this
  command and sync never rewrites them, the global settings symlink, or
  bindings.
- Backfill a missing mirror for legacy database profiles before they are
  exposed for normal profile operations, using their stored JSON. This is the
  only DB-to-file repair path; it does not overwrite an existing file.
- Update CLI help and README documentation.

## Capabilities

### New Capabilities

- `profile-disk-sync`: import per-profile settings files into the profile
  database and report each reconcile outcome.
- `profile-file-invariant`: ensure every successfully created or legacy
  profile has a `settings.<name>.json` mirror.

### Modified Capabilities

None. The repository has no established main specification to amend for the
existing profile lifecycle; the new invariant is captured as an added delta.

## Impact

- `src/claude_env/profiles.rs`: creation, legacy backfill, and disk-to-DB
  reconcile behavior.
- `src/cmd/config.rs`, `src/cmd/mod.rs`, and `src/cmd/help.rs`: `sync`
  command parsing, handling, and help.
- `src/claude_env/mod.rs` and `README.md`: public report types and user
  documentation.
