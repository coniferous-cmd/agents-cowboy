---
create-time: 2026-11-27 14:32:05
---

## MODIFIED Requirements

### Requirement: Tab strip mirrors the agent strategy registry

Cowboy SHALL render a single top tab strip whose tabs are exactly the registered agent backend strategies, in registration order. A tab SHALL exist if and only if its strategy is registered, regardless of whether any of that agent's sessions exist. The strip SHALL NOT contain an `All` tab, and no focus tab row SHALL be rendered; pane focus remains indicated by the highlighted border and status badge.

#### Scenario: Registered strategies appear in registration order
- **WHEN** the TUI renders with the Claude and Pi strategies registered in that order
- **THEN** the top strip shows exactly the tabs `Claude` and `Pi`, and no `All` tab
- **AND** the two-column browser renders below the strip without a focus tab row

#### Scenario: A registered agent has no sessions
- **WHEN** a strategy is registered but its agent has no discovered sessions
- **THEN** its tab is still shown and selectable