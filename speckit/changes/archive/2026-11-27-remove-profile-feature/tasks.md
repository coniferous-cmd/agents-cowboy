---
create-time: 2026-11-27 13:11:12
---

## 1. Entry points and Claude launching

- [x] 1.1 Red: add tests for rejected Profile CLI commands, retained alias/help behavior, two-tab TUI navigation, and former Profile shortcuts. Run them and confirm the new expectations fail on the current behavior.
- [x] 1.2 Red: add launch argument tests for both new and resumed sessions with a valid legacy binding, using a fake launcher and temporary fixtures. Assert working directory, alias/default executable, resume ID, inherited environment, and the absence of `--settings`; confirm failures are due to the current injection.
- [x] 1.3 Green: remove Profile CLI/TUI entry points and application wiring, including Profile repository access, worker loading, and the one-shot override. Simplify the launcher while retaining ordinary project/session behavior, and rerun the focused tests.

## 2. Metadata and upgrade safety

- [x] 2.1 Red: add temporary-directory tests for fresh metadata and upgrades from supported old SQLite versions, including legacy bindings and pending activation. Assert no creation of fresh Profile tables and no modification of old Profile rows, bindings, activation state, settings files/symlinks, or backups; assert alias and theme operations still work. Cover a missing mirror and verify no new backup or mirror is created. Use a real symlink where supported and confirm new tests fail for the expected reasons.
- [x] 2.2 Green: remove Profile startup hooks and Profile-only schema work while preserving non-Profile settings and theme initialization. Detach shared filesystem helpers, then remove unused Profile modules and dependencies without deleting old data. Rerun focused tests.

## 3. Documentation and full verification

- [x] 3.1 Refactor covered code as needed, keeping tests green. Update active user-facing documentation/help and storage descriptions; retain historical change records. Include upgrade notes on retired commands/bindings and preserving the effective settings before removing a legacy link target.
- [x] 3.2 Run `cargo fmt --check`, `cargo test`, and `cargo clippy --all-targets --all-features -- -D warnings`; report coverage and any validation blockers.
