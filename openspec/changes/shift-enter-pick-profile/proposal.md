## Why

Currently, entering a project always uses the bound profile if one exists, or falls back to the default environment. There is no way to temporarily pick a different profile for a one-shot session without permanently binding it via `e`. Shift+Enter provides a fast, friction-free way to launch with a profile of your choosing, just once.

## What Changes

- **New shortcut**: `p` in the Projects pane triggers a profile picker popup
- **Popup behavior**: Shows the same profile list as `BindProfile` modal; `↑↓` navigate, `Enter` select, `q/Esc` cancel
- **One-shot launch**: Selected profile is passed to the Claude CLI via `--settings <profile-path>` and is **not** persisted to the project binding
- **No binding change**: The project's existing binding (if any) is left untouched

## Capabilities

### New Capabilities
- `project-session-launch`: Add a "profile picker" launch mode that overrides the bound profile for a single session without modifying the project's permanent binding

### Modified Capabilities
- (none)

## Impact

- `app/mod.rs`: `ModalState` gets a new variant `PickProfile`; `AppState` gets `pending_profile_override: Option<PathBuf>`
- `app/project_actions.rs`: `new_session_here()` / `new_session()` inspect `pending_profile_override` and pass the profile path to the launcher
- `application.rs`: `ResumeLauncher::launch_new()` accepts an optional profile override (via existing `pending_new_session` mechanism)
- `main.rs`: passes `--settings <path>` when `pending_profile_override` is set
- `ui/mod.rs`: renders the new `PickProfile` popup using the existing `render_bind_profile_modal` layout
- No changes to `claude_env/` store layer — `activate_profile` is not called; only `--settings` CLI flag is used
