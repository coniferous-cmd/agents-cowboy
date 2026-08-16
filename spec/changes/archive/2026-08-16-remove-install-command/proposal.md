## Why

The `cowboy install` command downloads and installs Claude Code from the coniferous-cmd/claude-shop GitHub releases. This capability is no longer needed — users who want Claude Code should install it directly through Claude's official channels.

## What Changes

- **Remove** `cowboy install` CLI command entirely
- **Remove** `src/cmd/install.rs` (~1900 lines, including ~60 tests)
- **Remove** `CommandMode::Install` variant from `CommandMode` enum
- **Remove** `install` from CLI parser in `src/cmd/mod.rs`
- **Remove** `handle_install` from `src/cmd/mod.rs` exports
- **Remove** `cmd::CommandMode::Install` match arm from `src/main.rs`
- **Update** `src/cmd/help.rs` to remove install from usage and command list
- **Remove** `ureq = "3"` from `Cargo.toml` (only used by install)

## Capabilities

No spec-level behavior changes — this is pure removal of a CLI command with no impact on any remaining capabilities.

## Impact

- `cowboy install` will return "Unknown command: install" after this change
- Binary size decreases (removal of ureq dependency + install code)
- `sha2 = "0.10"` remains (used by `profiles.rs` for profile hashing)
- All other commands (`config`, `alias`, TUI) are unaffected
