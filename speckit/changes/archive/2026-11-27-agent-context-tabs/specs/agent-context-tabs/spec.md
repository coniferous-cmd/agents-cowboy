---
create-time: 2026-11-27 14:07:07
---

## ADDED Requirements

### Requirement: Tab strip mirrors the agent strategy registry

Cowboy SHALL render a single top tab strip whose tabs are exactly the registered agent backend strategies, in registration order. A tab SHALL exist if and only if its strategy is registered, regardless of whether any of that agent's sessions exist. This change registers only the Claude strategy. The strip SHALL NOT contain an `All` tab, and no focus tab row SHALL be rendered; pane focus remains indicated by the highlighted border and status badge.

#### Scenario: Only Claude is registered
- **WHEN** the TUI renders with only the Claude strategy registered
- **THEN** the top strip shows exactly one tab, `Claude`, and no `All` tab
- **AND** the two-column browser renders below the strip without a focus tab row

#### Scenario: A registered agent has no sessions
- **WHEN** a strategy is registered but its agent has no discovered sessions
- **THEN** its tab is still shown and selectable

### Requirement: Exactly one agent is selected, defaulting to the first registered

Cowboy SHALL always have exactly one selected agent tab. The selection SHALL default to the first registered agent on every start and SHALL NOT be persisted across restarts. The `a` key SHALL cycle the selection through the tabs in registration order, wrapping to the first. With a single registered agent, `a` SHALL leave the selection unchanged and report the active agent in the status bar.

#### Scenario: Startup selection
- **WHEN** Cowboy starts
- **THEN** the first registered agent (Claude) is selected

#### Scenario: Selection is not persisted
- **WHEN** the user switches to another agent tab and later restarts Cowboy
- **THEN** the selection is back on the first registered agent

#### Scenario: Cycling with one agent
- **WHEN** the user presses `a` while only Claude is registered
- **THEN** Claude remains selected and the status bar reports the active agent

### Requirement: Selected agent filters both columns at the project level

The Projects column SHALL list only directories with at least one session of the selected agent, plus the synthetic `Open Here` row, which SHALL always be present regardless of the selected agent. The Sessions column SHALL show only the current project's sessions of the selected agent, narrowed further by the active search query. A directory with sessions from multiple agents SHALL appear once under each of those agents' tabs, each view showing only that agent's sessions.

#### Scenario: Project without the selected agent's sessions
- **WHEN** a directory has sessions of another agent but none of the selected agent
- **THEN** it does not appear in the Projects column under the selected tab
- **AND** it appears under the tab of the agent whose sessions it has

#### Scenario: Open Here survives every tab
- **WHEN** the selected agent has no projects at all
- **THEN** the Projects column still shows `Open Here`

#### Scenario: Search combines with the agent scope
- **WHEN** a search query is active under a selected agent tab
- **THEN** the Sessions column shows only that agent's sessions of the current project matching the query

### Requirement: Token summaries follow the selected agent

Project-row session counts and token summaries SHALL be computed from the selected agent's sessions only, using the existing shared four-component total and compact formatter. No cross-agent combined view SHALL be offered.

#### Scenario: Mixed-agent project under one tab
- **WHEN** a directory has 5 Claude sessions totaling 8.1K tokens and 3 sessions of another agent
- **THEN** its row under the Claude tab shows the count and total of the 5 Claude sessions only

### Requirement: Launch actions follow the selected tab

Enter in Projects, `n` in Sessions, and Enter on `Open Here` SHALL start a new session of the selected tab's agent. Enter in Sessions SHALL resume the selected session through its agent's launcher. With only the Claude strategy registered, observable launch behavior SHALL remain unchanged: the configured alias or default `claude`, the intended working directory, `--resume <session-id>` for resume, inherited environment, and no `--settings`.

#### Scenario: New session under the Claude tab
- **WHEN** the user presses Enter on a project under the Claude tab
- **THEN** Cowboy launches the configured Claude command in that directory without `--settings`

#### Scenario: Open Here under the Claude tab
- **WHEN** the user presses Enter on `Open Here` under the Claude tab
- **THEN** Cowboy launches a new Claude session in the current working directory

### Requirement: Selection and query behavior across tab switches

When switching tabs, Cowboy SHALL keep the same working directory selected if it exists under the new tab; otherwise it SHALL select the first project; if the new tab has no projects, it SHALL select `Open Here`. Session selection SHALL reset to the first row. The search query SHALL be preserved across tab switches.

#### Scenario: Same directory exists under the new tab
- **WHEN** the user switches tabs and the currently selected directory also has sessions of the new agent
- **THEN** that directory stays selected and the session selection resets to its first session

#### Scenario: Directory absent under the new tab
- **WHEN** the user switches tabs and the selected directory has no sessions of the new agent
- **THEN** selection falls back to the first project, or to `Open Here` when the list is empty

#### Scenario: Query survives a switch
- **WHEN** a search query is active and the user switches tabs
- **THEN** the same query applies under the new tab

### Requirement: Lazy per-agent loading with cache

Cowboy SHALL load an agent's projects only when its tab is first selected, cache the result in memory for subsequent switches, and reload only the selected agent's list after a mutation (rename, delete, or returning from a launched session).

#### Scenario: First visit loads, revisits reuse
- **WHEN** the user selects an agent tab for the first time
- **THEN** that agent's projects are loaded from its store
- **AND** switching away and back does not reload them

#### Scenario: Mutation reloads only the active agent
- **WHEN** a rename or delete completes under the selected tab
- **THEN** only the selected agent's project list is reloaded

### Requirement: Sessions title carries only the search query

The Sessions pane title SHALL NOT carry an agent suffix; the tab strip is the single source of truth for the active agent. The title SHALL be `Sessions` without a query and `Sessions / query` with one.

#### Scenario: Title under a selected agent
- **WHEN** the Claude tab is selected and no search query is active
- **THEN** the Sessions pane title is `Sessions`

#### Scenario: Title with a query
- **WHEN** a search query `foo` is active under any tab
- **THEN** the Sessions pane title is `Sessions / foo`
