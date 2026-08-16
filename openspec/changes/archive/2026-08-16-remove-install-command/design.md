## Context

See proposal.md. The `cowboy install` command is a self-contained CLI entry point with no coupling to the rest of the application beyond the command routing in `mod.rs` and `main.rs`.

## Goals / Non-Goals

**Goals:**
- Remove `cowboy install` from the CLI surface
- Remove all code paths that only exist to support install
- Remove the `ureq` HTTP dependency (only used by install)

**Non-Goals:**
- No changes to `sha2` — still used by `profiles.rs` for profile hashing
- No changes to any remaining CLI commands or TUI behavior

## Decisions

### 1. Remove `src/cmd/install.rs` entirely

The file is entirely dedicated to install. No shared code would be lost by deleting it wholesale rather than surgically removing individual functions.

### 2. Keep `sha2` in `Cargo.toml`

`sha2 = "0.10"` is used by `src/claude_env/profiles.rs` for profile snapshot hashing. Removing it would break profiles functionality.

### 3. Remove `ureq` from `Cargo.toml`

`ureq` is only used by `install.rs`. After removal, `Cargo.toml` should be updated to remove the `ureq = "3"` line.

### 4. Remove `mod install;` and related entries from `src/cmd/mod.rs`

- Delete `mod install;`
- Delete `install` from `CommandMode` enum
- Delete `"install" => install::parse_install_args(args)` match arm
- Delete `pub(crate) use install::handle_install;`
- Remove `parse_install_args` and `rejects_install_with_extra_args` tests

### 5. Remove install match arm from `src/main.rs`

Delete `cmd::CommandMode::Install => cmd::handle_install(&env_store),`

### 6. Update `src/cmd/help.rs`

Remove the `cowboy install` line from usage and the `install  Download...` line from the command list.

## Risks / Trade-offs

- **Risk:** Any external script or documentation referencing `cowboy install` will break silently. → No mitigation needed — this is the intended behavior.
- **Risk:** The `ureq` crate may have been a transitive dependency for something else. → Verified: `ureq` appears only in `install.rs` and `Cargo.toml`.
