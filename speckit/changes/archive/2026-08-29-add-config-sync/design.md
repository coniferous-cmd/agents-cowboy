## Context

`claude_profiles` is metadata for the per-profile JSON files under the
database parent. Existing edits, copies, and activation write files, but bare
creation only writes SQLite. The new direction makes the file mirror an
invariant and supplies the missing file-to-database import command.

## Goals / Non-Goals

**Goals:**

- Every created profile has `settings.<name>.json`.
- Legacy database-only profiles receive a missing mirror without replacing an
  existing file.
- `cowboy config sync [name]` imports disk JSON into SQLite with useful
  per-profile results.
- Preserve external edits: sync reads files but never rewrites them.

**Non-Goals:**

- Bidirectional conflict resolution, mtime comparison, or a dry-run mode.
- Changing the global settings symlink or project bindings during sync.
- Adding a TUI entry point or a schema migration.

## Decisions

### Creation is an atomic DB-and-file operation

`create_profile(name)` validates and inserts `{}` within a SQLite transaction,
then atomically writes `profile_file_path(name)`. The transaction commits only
after that write succeeds. This gives callers a single success condition:
both row and file exist.

`copy_profile`, `update_profile_json`, and activation already write their
profile targets; their behavior remains compatible with this invariant.

### Backfill is one-way and non-destructive

Startup or the profile-operation boundary enumerates legacy rows and creates
only missing mirror files from `settings_json`. It never overwrites a file:
an existing file is potentially an external change and is reconciled only by
an explicit `config sync`.

### Sync is file-to-database only

`sync_profiles_from_disk(None)` scans valid `settings.<name>.json` entries in
`profiles_dir()` in alphabetical order. `Some(name)` handles exactly that
file. It validates the name and JSON object, then uses one short transaction
per file to INSERT, UPDATE, or report unchanged. Read/validation failures are
collected in `SyncReport.invalid` and processing continues.

Sync never writes a profile file, the settings symlink, or bindings. This
keeps its meaning unambiguous: the caller elects the file as the source of
truth.

### CLI interface

`ConfigCommand::Sync { name: Option<String> }` accepts `cowboy config sync`
and `cowboy config sync work`; extra arguments are rejected. The handler
prints one line per outcome and returns success after a completed run,
including mixed valid/invalid files.

## Risks / Trade-offs

- A disk file can be stale relative to SQLite, but an explicit `sync` is the
  user's declaration that disk wins.
- Backfill can fail for filesystem permissions; surface that failure rather
  than creating a database-only profile.
- Concurrent external writes remain outside this CLI's locking model; atomic
  file replacement limits cowboy-originated partial writes.
