# Decisions

## Session source of truth

- Claude session JSONL under the configured projects directory remains the source of truth for Claude. Pi reads v3 JSONL under `~/.pi/agent/sessions/` and renames via the latest `session_info` record (append one if absent).
- Pi groups and deletes by the same header `cwd` path equality, without canonicalization. Project deletion scans before unlinking, never recursively removes a directory, and preserves other projects and unsupported files.
- Background deletion and reload use the target agent's backend; startup loads the first registered agent. Pi-reported cost takes precedence over estimates, with provider origin shown only for uniformly provider-reported records.
- SQLite is not used for session discovery.
- New and resumed Claude processes inherit the parent process environment unchanged and load their own settings. Cowboy does not add `--settings`.

## Storage and upgrades

- `settings` stores configurable paths and launcher settings; `themes` stores presentation themes.
- New databases do not create Profile tables. Existing Profile tables and records are left in place and unused, including pending activations and bindings.
- Cowboy does not change a legacy settings symlink or restore its backup. See the [upgrade notes](../README.md#upgrading-from-a-release-with-profiles) before cleaning up old Profile files.
- The earlier migration for pre-Profile environment/project tables remains: it privately exports non-empty project settings before removing the obsolete env/project tables.

## TUI

- A one-row agent tab strip sits above the two-column browser. This revises the earlier "no tab strip" decision: the agent row returns as a global working context (one tab per registered backend strategy, exactly one selected, no `All`), because Cowboy is preparing to manage projects for multiple agent backends and agent ownership must be visible at a glance. The focus tab row stays removed — the highlighted pane border and the status badge track focus. The Sessions pane title carries only the search query; the tab strip is the single source of truth for the active agent.
- `[` / `]`, `Tab`, and the left/right arrows move focus between the two panes.
