## Why

Profiles are currently global — activating a profile swaps the `~/.claude/settings.json` symlink, affecting all Claude sessions. When a user works on multiple projects that need different settings (e.g., different API keys, different MCP configs, different permissions), they must manually activate the correct profile before launching Claude in each project. This is error-prone and tedious.

The goal is to bind a profile to a project so that launching Claude in that project automatically uses the bound profile's settings via `claude --settings <file>`, without requiring global symlink swaps.

## What Changes

- Add a `project_profile_bindings` table to the SQLite database, mapping `project_cwd` → `profile_name`
- Add bind/unbind CRUD operations to `ClaudeEnvStore` and `ProfileRepository` trait
- Modify `ClaudeCliLauncher` to check for a project binding before launching Claude, and pass `--settings <profile-file-path>` when a binding exists
- Add UI keybinding (`b`) in the Projects tab to bind a profile to the selected project
- Add CLI subcommands: `cowboy bind <project> <profile>` and `cowboy unbind <project>`
- Display bound profile name next to project names in the TUI
- Prevent deletion of profiles that have active bindings

The global profile activation mechanism (symlink swap) is preserved as the fallback for projects without bindings.

## Capabilities

### New Capabilities

- `project-profile-binding`: Bind a profile to a project directory so that Claude sessions launched in that project automatically use the bound profile's settings via `--settings`

### Modified Capabilities

(none — no existing specs)

## Impact

- **Database**: New table `project_profile_bindings` with migration in `schema.rs`
- **Core logic**: `claude_env/profiles.rs` — new CRUD methods for bindings
- **Application layer**: `application.rs` — `ProfileRepository` trait gains binding methods; `ClaudeCliLauncher` gains settings-path lookup logic
- **TUI**: `app/mod.rs` and `app/project_actions.rs` — new keybinding, display binding indicator, bind/unbind modal flow
- **CLI**: `cmd/config.rs` — new `bind`/`unbind` subcommands
- **Domain**: `domain.rs` — new error variants (`BindingNotFound`, `ProfileAlreadyBound`)
