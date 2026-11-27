---
create-time: 2026-11-27 13:10:50
---

## Context

Profile support spans `src/cmd/`, `src/app/`, `src/ui/`, and `src/application.rs`. Both launcher paths consult project bindings; the one-shot override separately adds `--settings`. `src/main.rs` loads Profile state, creates a one-time backup, and recovers pending activation. `src/claude_env/` shares a database and filesystem helpers between Profile code and the settings and theme code that must remain. Existing global `settings.json` may be a symlink to a Cowboy Profile mirror.

## Goals / Non-Goals

**Goals:**
- Remove the complete Profile feature and its launch side effects while preserving project/session actions, launcher aliases, themes, and Claude configuration directory resolution.
- Keep any legacy Profile files, database data, and effective Claude global settings exactly as they are.

**Non-Goals:**
- No automatic conversion of a symlink, restoration of a settings backup, or cleanup of stored Profile data.
- No replacement for project-scoped Profile selection and no changes to Claude's own settings resolution.

## Decisions

1. **Delete the entry points and simplify the launcher.** Remove the Profile CLI parser/handler and help entries; remove the Profile tab, overlays, actions, state, and worker loading. Remove Profile repository APIs and one-shot override plumbing. Retain the normal new/resume launcher paths, which supply the alias, working directory, and resume ID as applicable without setting overrides. Former Profile shortcuts must no longer perform Profile actions; do not assign a new shortcut behavior as part of the removal.
2. **Separate retained metadata setup from retired Profile operations.** Drop Profile-specific startup work and detach the shared private filesystem helpers from the Profile module. Review every supported database version path for Profile writes: continuing an old migration that clears activation records or rebuilds the bindings table would violate the preservation contract. Initialize or upgrade the retained settings/themes structures as needed without touching Profile data. Do not require or create Profile tables for a fresh database. Preserve unrelated metadata compatibility and verify it against representative old database fixtures.
3. **Leave filesystem state alone.** Cowboy will not inspect and rewrite `settings.json` to restore or replace a Profile. The upgrade documentation will explain that preserving the current effective settings differs from restoring the old backup, and that the link target must not be removed until the user has verified that a regular settings file contains the desired content.
4. **Record the current spec baseline.** The active `profile-disk-sync` spec has no requirements to delete; its archived change is historical evidence, not the active baseline. Add the retirement requirements to that capability and leave historical archives unchanged.

## Risks / Trade-offs

- **Previously bound projects may start with different settings.** This is intentional; call it out in the upgrade notes and test new/resume with legacy bindings that would previously have supplied `--settings`.
- **A legacy link can remain dependent on a Profile file.** Leaving it alone avoids silent rewrites and rollback to stale settings; a user who chooses to clean up must first preserve the effective configuration.
- **Shared database bootstrap can hide destructive Profile work.** Test old schema versions independently, not just a current-version database. Assert that Profile records and related on-disk artifacts stay unchanged while settings and themes continue to work.
