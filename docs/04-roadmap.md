# Roadmap

This roadmap tracks the next reliability and release work. Completed product
foundations are recorded briefly; active items include explicit acceptance
criteria and should be implemented with TDD.

## Completed Foundations

- [x] Discover projects and sessions from `~/.claude/projects/`.
- [x] Provide the two-column keyboard-first TUI.
- [x] Resume, create, rename, search, inspect, and delete sessions.
- [x] Persist cowboy metadata and themes in SQLite.
- [x] Resolve project and session environment precedence for launches.
- [x] Aggregate token usage and estimate model cost from local session data.
- [x] Normalize session lifecycle timestamps and display local time.
- [x] Edit workspace and global Claude configuration through `$EDITOR`.
- [x] Build npm release artifacts for the supported platform matrix.
- [x] Split oversized modules: `infrastructure`, `claude_env`, and `app` into
  focused directory modules with stable public API surface.

## P0: Release Integrity

### Synchronize Package Versions

- Keep `Cargo.toml`, `Cargo.lock`, and `package.json` on the same release version.
- Add an early CI check so a mismatch fails before tagging or binary builds.
- Treat the Git tag as a release trigger, not as the first point of validation.

Acceptance criteria:

- A version mismatch fails on pull requests and pushes to `main`.
- The documented release command updates or validates every package manifest.
- A dry-run package reports the same version as the Rust binary release.

### Harden The Release Matrix

- Run MSVC environment setup only for Windows targets.
- Validate every expected binary path before npm packing.
- Keep Linux, macOS, and Windows x64/arm64 jobs independently diagnosable.

Acceptance criteria:

- All release matrix jobs reach the Rust build step on their native runner.
- Missing or incorrectly named artifacts fail before `npm publish`.
- The npm entrypoint smoke test selects the correct current-platform binary.

## P1: Filesystem Reliability

### Make Deletion Non-Blocking And Exit-Safe

- Run session and project deletion, followed by project reload, in one
  background worker managed by the TUI runtime.
- Allow only one deletion at a time; keep rendering and input polling active
  while it runs.
- Block application exit and all conflicting shortcuts during deletion. `q` and
  `Ctrl-C` must display a waiting message instead of exiting; `Esc` is also
  disabled.
- Replace the list only after success. On failure, retain the current list,
  show an error toast, and restore normal interaction.

Acceptance criteria:

- Confirming deletion does not perform repository I/O on the UI thread.
- The TUI remains responsive while a delete worker is active, but cannot start
  a competing operation or exit through its normal shortcuts.
- Success refreshes the project list and failure leaves the previous list
  intact with a visible error.
- Tests cover task queueing, exit/shortcut locking, success, failure, and a
  worker-start failure without relying on sleeps.

### Isolate Malformed Session Files

- Do not let one malformed or partially written JSONL file hide healthy
  projects and sessions.
- Preserve enough file and line context to make failures diagnosable.
- Define whether an invalid trailing line is ignored or the affected session is
  skipped; do not silently accept corruption in the middle of a transcript.

Acceptance criteria:

- Tests cover a truncated final line, malformed middle line, unreadable file,
  and a healthy sibling session.
- Healthy sessions remain visible when another session cannot be parsed.
- The TUI reports degraded discovery without terminating.

### Make Session Mutation Cross-Platform

- Replace session files using semantics that work when the destination already
  exists on Windows.
- Use collision-resistant temporary files in the destination directory.
- Clean up temporary files on failed rename operations where safe.

Acceptance criteria:

- Rename tests cover an existing destination on Windows and Unix.
- A failed replacement leaves the original transcript readable.
- Concurrent or stale temporary files do not overwrite one another.

### Guarantee Terminal Restoration

- Audit terminal entry, child-process launch, editor launch, and error paths.
- Restore raw mode, alternate screen, and cursor visibility after failures.

Acceptance criteria:

- Terminal lifecycle logic has focused tests around recoverable boundaries.
- Forced render, input, editor, and launcher failures do not leave the shell in
  raw mode.

## P1: Quality Gates

- Add `cargo fmt --check` to CI.
- Add `cargo clippy --all-targets --all-features -- -D warnings` to CI after
  resolving the current findings.
- Keep `cargo test` on Linux, macOS, and Windows.
- Add npm wrapper and distribution checks to pull-request validation where they
  do not require release artifacts.

Acceptance criteria:

- The same validation commands are documented for local and CI use.
- CI rejects formatting, lint, test, package-version, and wrapper regressions.
- No quality check exists only in the release workflow.

## P2: Module Boundaries

Reduce the size and mixed responsibilities of the largest modules without
changing behavior:

- Split `src/claude_env/mod.rs` by schema/migration, settings, environment
  values, and themes.
- Split `src/infrastructure.rs` by discovery/parser and filesystem mutation.
- Split `src/app.rs` by state transitions or action families while keeping one
  owner for global interactive state.

Acceptance criteria:

- Existing public interfaces remain stable unless a separate design decision
  approves a change.
- Each extraction begins with characterization tests.
- No new circular dependencies are introduced between layers.

## P2: Documentation And Operations

- Keep feature lists synchronized with `src/features/`.
- Correct stale absolute paths in interface and SQL documentation.
- Document recovery for invalid session data and failed configuration edits.
- Add a concise release checklist covering versioning, tests, artifacts, npm
  dry run, tagging, and publication verification.

## Deferred

- Official billing-export reconciliation.
- API proxying or traffic interception.
- Re-tokenizing historical transcripts.
- Replacing Claude's filesystem with SQLite as the session source of truth.
- Adding nested navigation or a plugin system before the reliability backlog is
  complete.
