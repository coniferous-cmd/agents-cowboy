---
create-time: 2026-11-27 14:07:39
---

## 1. Strategy abstraction (behavior-preserving refactor)

- [x] 1.1 Red: add tests asserting an `AgentBackend` registry exposes the Claude strategy with id `claude` and label `Claude`, and that its repository/launcher behavior delegates to the existing `ClaudeProjectsStore` / `ClaudeCliLauncher` (reuse existing launcher and store tests via fakes/temp dirs).
- [x] 1.2 Green: define `trait AgentBackend: SessionRepository + ResumeLauncher { fn id(&self) -> AgentId; fn label(&self) -> &str; }` in `src/application.rs`; implement `ClaudeBackend` composing the existing store and launcher; build the ordered registry (Claude only) at startup.
- [x] 1.3 Refactor: route existing load/rename/delete/resume/launch call sites through the registry's Claude entry without changing observable behavior; run the full suite.

## 2. Required agent selection (remove `All`)

- [x] 2.1 Red: tests for startup selection defaulting to the first registered agent, `a` cycling in registration order with wraparound, single-agent `a` no-op that still reports the active agent in the status bar, and selection resetting to the default on a fresh app start (no persistence).
- [x] 2.2 Green: replace `AgentFilter` (delete the `All` variant and the enum's filter role) with a required selected agent in `src/app/mod.rs`; rewire `cycle_agent_filter` to cycle registry tabs; update `session_actions.rs` to scope by the required agent.
- [x] 2.3 Refactor: remove dead `All`-related code paths and update existing filter tests to the new state model; run the full suite.

## 3. Lazy per-agent loading with cache

- [x] 3.1 Red: with a fake second backend, tests asserting an agent's projects load on first tab selection, revisits reuse the cache without reloading, and a completed mutation reloads only the selected agent's list.
- [x] 3.2 Green: add per-agent in-memory project caches to app state; load through the selected backend on first visit; invalidate/reload only the active agent after rename, delete, and session-launch return.

## 4. Project-level filtering and per-agent summaries

- [x] 4.1 Red: tests asserting the Projects column lists only directories with ≥1 session of the selected agent, `Open Here` is always present (including under an agent with zero projects), a mixed-agent directory appears under each of its agents' tabs, and project-row counts/token summaries include only the selected agent's sessions.
- [x] 4.2 Green: filter projects by the selected agent before rendering and before `summarize_project_usage` (aggregation math unchanged); keep `Open Here` unfiltered; scope the Sessions column by agent then query via `filter_project_sessions_for_agent`.

## 5. Selection continuity and query preservation

- [x] 5.1 Red: tests for tab-switch behavior — same working directory stays selected when present under the new tab; otherwise selection falls back to the first project; empty list selects `Open Here`; session selection resets to the first row; the search query is preserved.
- [x] 5.2 Green: implement the switch logic in app state (directory match by cwd, fallbacks, session reset, query untouched).

## 6. Tab strip UI and Sessions title

- [x] 6.1 Red: update `src/ui/layout.rs` tests — one strip row at the top, both panes start below it, single-cell gap and status bar unchanged; add `TestBackend` render tests asserting the strip shows exactly the registered tabs with the selected one highlighted, no `All` tab, and no focus tab row.
- [x] 6.2 Red: update `session_list` title tests to the two remaining forms `Sessions` and `Sessions / query` (agent suffix removed).
- [x] 6.3 Green: add the top `Constraint::Length(1)` strip row to `compute_main_layout`; render tabs from registry labels with the theme's highlight; strip the agent suffix from `sessions_panel_title` and its callers.

## 7. Docs and final verification

- [x] 7.1 Update `docs/03-ui.md` (tab strip, title forms, per-agent summaries), `docs/05-decisions.md` (explicit revision: one agent-context tab row returns, focus row stays removed, rationale), and UI-facing README text.
- [x] 7.2 Run `cargo fmt --check`, `cargo test`, and `cargo clippy --all-targets --all-features -- -D warnings`; report results and the tests added/changed per AGENTS.md.
