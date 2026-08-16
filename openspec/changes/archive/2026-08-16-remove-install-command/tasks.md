## 1. Remove install module and CLI routing

- [ ] 1.1 Delete `src/cmd/install.rs`
- [ ] 1.2 Remove `mod install;` from `src/cmd/mod.rs`
- [ ] 1.3 Remove `Install` variant from `CommandMode` enum in `src/cmd/mod.rs`
- [ ] 1.4 Remove `"install" => install::parse_install_args(args)` match arm in `src/cmd/mod.rs`
- [ ] 1.5 Remove `pub(crate) use install::handle_install;` from `src/cmd/mod.rs`
- [ ] 1.6 Remove `parses_install_command` and `rejects_install_with_extra_args` tests from `src/cmd/mod.rs`

## 2. Remove install from main.rs

- [ ] 2.1 Remove `cmd::CommandMode::Install => cmd::handle_install(&env_store),` match arm from `src/main.rs`

## 3. Update help text

- [ ] 3.1 Remove `cowboy install` from usage string in `src/cmd/help.rs`
- [ ] 3.2 Remove `install  Download and install the latest Claude Code binary` from command list in `src/cmd/help.rs`

## 4. Remove ureq dependency

- [ ] 4.1 Remove `ureq = "3"` from `Cargo.toml`

## 5. Verify

- [ ] 5.1 Run `cargo fmt --check`
- [ ] 5.2 Run `cargo test`
- [ ] 5.3 Run `cargo clippy --all-targets --all-features -- -D warnings`
- [ ] 5.4 Verify `cowboy install` returns "Unknown command: install"
