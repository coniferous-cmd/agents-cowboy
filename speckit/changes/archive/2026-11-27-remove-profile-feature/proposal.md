---
create-time: 2026-11-27 13:09:50
---

## Why

Cowboy currently manages Claude settings as named Profiles, in addition to browsing and launching sessions. Removing Profile management simplifies Cowboy and makes Claude responsible for loading its own configuration. Upgrading must not silently rewrite or discard configuration that users already rely on.

## What Changes

- Remove the Profiles tab, Profile management actions, project bindings, and the `Shift+Enter` Profile picker. Remove the `cowboy config` command, all of whose subcommands manage Profiles.
- Stop supplying a Cowboy-generated `--settings` argument when starting or resuming Claude. Keep the launcher alias, working directory, session ID, and existing project/session workflows.
- Stop Profile-related startup work, including mirror backfill, initial Claude settings backup, and activation recovery. Do not migrate, restore, or purge existing Profile data or settings files.
- Document that existing project bindings stop taking effect and that an existing `settings.json` symlink may still depend on its old Profile file.

## Capabilities

### New Capabilities

- `session-launch`: Specify how Cowboy starts and resumes Claude without Cowboy-managed Profiles, and what remains accessible in the UI and CLI.

### Modified Capabilities

- `profile-disk-sync`: Replace the runtime sync behavior with a retirement contract: no sync or backfill, and no automatic changes to legacy Profile data.

## Impact

- CLI argument parsing and help; TUI navigation, dialogs, rendering, and startup loading; launcher and Profile service APIs; SQLite initialization; user documentation and regression tests.
- Keep SQLite for launcher settings and themes. Existing Profile tables and records are left in place and unused; their presence is not required for a fresh install.
- This intentionally breaks scripts using `cowboy config` and changes Claude settings selection for projects that relied on a Cowboy binding. Existing global settings remain under the user's control.
