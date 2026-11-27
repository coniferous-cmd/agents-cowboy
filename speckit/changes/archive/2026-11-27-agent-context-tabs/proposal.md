---
create-time: 2026-11-27 13:56:11
---

## Why

Cowboy is preparing to manage projects for multiple agent backends (Claude, Codex, Pi, and others). Today the agent filter is a session-level `All | <agent>` cycle hidden behind the `a` key, and only Claude sessions are discovered. A top tab strip that selects one agent as the global working context makes agent ownership visible at a glance, filters both columns at the project level, and gives future agent backends a stable place to plug into. Deriving the tab list from a registry of supported agent strategies means a tab exists exactly when Cowboy supports that agent, so users can start the first session of a newly supported agent even before any of its sessions exist.

## What Changes

- Introduce a per-agent backend strategy abstraction. Each strategy owns its session-directory discovery, transcript parsing and usage extraction (normalized into the shared `SessionUsage`), launch/resume arguments, and rename/delete file operations. Token summation, compact formatting, and cost estimation stay shared domain logic so the documented four-component token total remains identical across agents. Implement the Claude strategy by moving the existing `ClaudeProjectsStore` and `ClaudeCliLauncher` behavior behind the abstraction; register it as the only strategy in this change.
- Add a top agent tab strip driven by the strategy registry. Exactly one agent is always selected; there is no `All` tab. The default selection is the first registered agent (Claude) and the selection is not persisted across restarts.
- The selected tab is a working context, not just a filter: the Projects column lists only projects with at least one session of the selected agent (the synthetic `Open Here` row is always shown), and the Sessions column shows only that agent's sessions of the current project. A directory with sessions from several agents appears once under each of those agents' tabs, each view showing only its own sessions.
- Project-row token summaries and session counts are computed from the selected agent's sessions only, so the numbers always match the visible list. There is no cross-agent combined view.
- Enter in Projects, `n`, and `Open Here` start a new session of the selected tab's agent; Enter in Sessions resumes the selected session of that agent. With only Claude registered, observable launch behavior is unchanged.
- The `a` key cycles the tab selection instead of the old `All | <agent>` filter. Remove the `AgentFilter::All` variant and the Sessions-title agent suffix (`Sessions · Claude`); the tab strip is the single source of truth for the active agent. Pane focus remains border-driven; the removed focus tab row does not return.
- Update `docs/03-ui.md` and `docs/05-decisions.md`: the "no tab strip" decision is revised to "one agent-context tab strip; no focus tab row", with the rationale recorded.

Out of scope: Codex/Pi strategy implementations (each lands as its own follow-up change), any cross-agent aggregate view, and persisting the selected tab.

## Capabilities

### New Capabilities

- `agent-context-tabs`: The agent tab strip as a global working context — tab list equals the supported-agent strategy registry, exactly one agent selected with a non-persisted default, project-level filtering of both columns, per-agent token summaries, and agent-following launch actions.

### Modified Capabilities

<!-- No existing requirement changes: with only the Claude strategy registered, session-launch behavior (alias, working directory, --resume, environment inheritance, no --settings) is unchanged. profile-disk-sync is untouched. -->

## Impact

- `src/app/`: `AgentFilter` replaced by a required selected `AgentId`; tab cycling and selection-fallback behavior when the current project is filtered out; key handling.
- `src/ui/`: layout gains a one-row tab strip above the two-column browser; tab rendering; Sessions title loses the agent suffix; existing row-0 layout and title tests updated.
- `src/features/session_list/`, `src/features/project_usage/`: filtering and summaries scoped to the selected agent (aggregation math itself unchanged).
- `src/infrastructure/`, `src/application.rs`: Claude discovery/parsing/launch reorganized behind the strategy abstraction and a registry; `SessionRepository`/`ResumeLauncher` traits and `SessionKey { agent_id, native_id }` are reused, not replaced.
- Docs: `docs/03-ui.md`, `docs/05-decisions.md`, and UI-facing README text; regression tests per AGENTS.md TDD requirements.
