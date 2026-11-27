---
create-time: 2026-11-27 14:05:30
---

## Context

The domain already carries agent identity (`SessionKey { agent_id, native_id }`), and the application layer already abstracts persistence and launching behind two traits: `SessionRepository` (`load_projects`, `rename_session`, `delete_session`, `delete_project`) and `ResumeLauncher` (`resume`, `launch_new`) in `src/application.rs`. The concrete implementations (`ClaudeProjectsStore`, `ClaudeCliLauncher`) are Claude-specific. The current agent filter is an `AgentFilter::All | Agent(AgentId)` enum cycled with `a`, scoped to sessions within the current project, derived from loaded sessions, and surfaced only in the Sessions pane title. The two-column layout starts at terminal row 0 by explicit decision (`docs/05-decisions.md`), with tests asserting it.

Real pi data on the development machine confirms the second strategy's shape: sessions live under `~/.pi/agent/sessions/<encoded-cwd>/<timestamp>_<uuid>.jsonl`; the header record carries `cwd` directly; assistant messages carry `usage { input, output, cacheRead, cacheWrite, reasoning, totalTokens }` whose `totalTokens` equals the four-component sum Cowboy already documents; a real `cost.total` is included; there is no title record; resume is `pi --session <path|id>` (`--resume` is an interactive picker and must not be used).

## Goals / Non-Goals

**Goals:**

- One per-agent strategy abstraction whose registry drives the tab strip: a tab exists exactly when its strategy is registered.
- Selected agent as a required global working context: filters both columns, scopes token summaries, and determines which agent Enter / `n` / `Open Here` launch.
- Claude strategy as a thin reorganization of existing behavior; observable Claude launch behavior unchanged.
- Keep the four-component token total, compact formatting, and aggregation shared across agents.

**Non-Goals:**

- Pi (or Codex) strategy implementation — pi is the agreed second strategy and lands as its own change.
- Cross-agent combined views, tab-selection persistence, configurable tab lists.
- Reintroducing a focus tab row; pane focus stays border-driven.

## Decisions

### D1: Strategy shape — supertrait over the existing seams

```rust
trait AgentBackend: SessionRepository + ResumeLauncher {
    fn id(&self) -> AgentId;    // "claude"
    fn label(&self) -> &str;    // tab text, e.g. "Claude"
}
```

The registry is an ordered `Vec<Box<dyn AgentBackend>>`; tab order is registration order. `ClaudeBackend` composes the existing `ClaudeProjectsStore` and `ClaudeCliLauncher` and delegates, keeping the refactor diff small and existing tests meaningful.

Alternative considered: a registry mapping `AgentId` to separate repository and launcher trait objects. Rejected — an agent would register in two places, allowing a half-registered agent (repository without launcher). The supertrait makes one agent = one struct = one registration, checked at compile time.

### D2: Strategy boundary — extraction per agent, computation shared

A strategy owns: session-directory discovery, transcript parsing, usage extraction normalized into `SessionUsage`, launch/resume arguments, and rename/delete file operations. `SessionUsage::total_tokens()`, `summarize_project_usage`, the compact formatter, and `pricing.rs` stay shared domain logic. Rationale: `docs/03-ui.md` defines one four-component token total for every session; pushing summation into strategies would let that invariant drift per agent. Verified against real pi data: pi's own `totalTokens` already equals the shared definition.

### D3: Selection state — required agent, no `All`

Replace `AgentFilter` with a required selected `AgentId` (index into the registry). Default is the first registered agent (Claude); the selection is not persisted — every start returns to the default. `a` cycles the registry. With one registered agent, `a` is a no-op that still reports the active agent in the status bar. The Sessions-title agent suffix is removed; the tab strip is the single source of truth. `AgentFilter::All` is deleted, not deprecated.

### D4: Loading — lazy per agent with in-memory cache

Only the selected agent's projects are loaded. Switching to an agent loads its projects on first visit and caches them in memory; subsequent switches reuse the cache. Mutations (rename/delete/new-session return) reload only the current agent's list. Rationale: with no `All` tab there is never a need for two agents' lists at once; eager loading multiplies startup I/O by the number of registered agents for data nobody is looking at. Cost: a first visit to a tab pays one directory scan — the same scan startup already pays today, moved to switch time.

### D5: Selection continuity across tab switches

- Project: keep the same working directory selected if it exists under the new tab; otherwise select the first project; if the list is empty, select `Open Here` (always present, never filtered).
- Session: reset selection to the first row (the session lists are disjoint across agents).
- Search query: preserved across tab switches; it is a view filter, not agent state.

### D6: Filtering and summaries

Projects column: directories with at least one session of the selected agent, plus the synthetic `Open Here` row. Sessions column: the current project's sessions of the selected agent, then the search query. Project-row token summaries and counts are computed by the existing `summarize_project_usage` over the already-filtered project — the aggregation code itself does not change; its input is scoped. A directory with sessions from several agents appears once under each of those agents' tabs.

### D7: Layout — one tab row above the browser

`compute_main_layout` gains a `Constraint::Length(1)` (or minimal height for the strip) top row; the two-column browser starts below it. The focus tab row does not return. Row-0 layout tests and title tests are updated as the Red step of TDD, per AGENTS.md.

### D8 (memo for the pi change): provider-reported cost

pi files carry a real `cost.total` (with `totalFromProvider`). When the pi strategy lands, the normalization contract should allow a strategy to supply an optional provider-reported cost that takes precedence over `pricing.rs` estimation, surfaced through Session Info's accuracy status. Not implemented in this change; recorded here so the follow-up spec starts from evidence.

## Risks / Trade-offs

- **Single-tab interim**: until pi lands, the strip shows only `[Claude]`. Accepted deliberately: the strip is the extension point, and the registry semantics (tab exists ⇔ strategy registered) must hold from day one so the pi change is purely additive.
- **First-switch latency**: lazy loading moves a directory scan to the first visit of a tab. Bounded by the same scan cost startup pays today; if it ever becomes noticeable, a background prewarm can be added without changing the contract.
- **Decision reversal surface**: `docs/05-decisions.md` says "there is no tab strip". The revision must be explicit — one agent-context row returns, the focus row stays removed — so the history reads as a refinement, not a silent flip.
- **Cache staleness**: per-agent caches can go stale if session files change outside Cowboy while another tab is active. Same exposure as today's single-list reload model; mutations through Cowboy always reload the active list. A manual refresh key can be considered later if needed.
