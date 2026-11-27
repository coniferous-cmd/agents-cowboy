# Decisions

## Session source of truth

- Claude session JSONL under the configured projects directory remains the source of truth.
- SQLite is not used for session discovery.
- New and resumed Claude processes inherit the parent process environment unchanged and load their own settings. Cowboy does not add `--settings`.

## Storage and upgrades

- `settings` stores configurable paths and launcher settings; `themes` stores presentation themes.
- New databases do not create Profile tables. Existing Profile tables and records are left in place and unused, including pending activations and bindings.
- Cowboy does not change a legacy settings symlink or restore its backup. See the [upgrade notes](../README.md#upgrading-from-a-release-with-profiles) before cleaning up old Profile files.
- The earlier migration for pre-Profile environment/project tables remains: it privately exports non-empty project settings before removing the obsolete env/project tables.

## TUI

- Projects and Sessions share a two-column browser that starts at the top terminal row; there is no tab strip. The highlighted pane border and the status badge track focus, and the Sessions pane title carries the agent filter (omitted for the default `All`) and search query.
- `[` / `]`, `Tab`, and the left/right arrows move focus between the two panes.
