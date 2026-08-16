## 1. Store Layer

- [x] 1.1 Add `copy_profile(source, new_name)` method to `ClaudeEnvStore` in `src/claude_env/profiles.rs` — reads source settings_json, validates new name, creates new profile with same JSON in a transaction
- [x] 1.2 Add unit tests for `copy_profile`: success case, duplicate name, nonexistent source, invalid name

## 2. CLI Command Parsing

- [x] 2.1 Add `Copy { source: String, new_name: String }` variant to `ConfigCommand` enum in `src/cmd/mod.rs`
- [x] 2.2 Add `"copy"` case to `parse_config_args` in `src/cmd/config.rs` — accepts exactly two positional args: `<source>` and `<new-name>`
- [x] 2.3 Add `ConfigCommand::Copy` handler in `handle_config_with_writer` — calls `store.copy_profile()` and prints success message
- [x] 2.4 Add parse test: valid `copy work work-debug`, invalid/missing args

## 3. CLI Integration Tests

- [x] 3.1 Add `profile_copy_handlers_create_and_error` test — create profile, copy it, verify new profile exists with identical JSON, verify copy to duplicate name fails

## 4. Application Trait Layer

- [x] 4.1 Add `copy_profile(source, new_name)` to `ProfileRepository` trait in `src/application.rs`
- [x] 4.2 Add `copy_profile` implementation in `StetsonApplication` delegating to the store

## 5. TUI State

- [x] 5.1 Add `CopyProfile` variant to `ModalState` enum in `src/app/mod.rs`
- [x] 5.2 Add `c` key case in `handle_normal_key` for Profiles tab — calls `begin_profile_copy()`
- [x] 5.3 Implement `begin_profile_copy()` — pre-fills `input_buffer` with `"Copy of {source_name}"`, sets `ModalState::CopyProfile`, sets status
- [x] 5.4 Implement `handle_copy_key()` — Backspace/Char input, Enter validates and calls `application.copy_profile()`, Esc cancels and closes modal

## 6. TUI Rendering

- [x] 6.1 Add `ModalState::CopyProfile` case in `render()` in `src/ui/mod.rs` — calls `render_input_modal`
- [x] 6.2 Add `"c", "Copy"` to `shortcuts_for` for Profiles tab mode
- [x] 6.3 Add CopyProfile mode label in `render_status` mode detection

## 7. TUI Integration Tests

- [x] 7.1 Add test: `c` key in Profiles tab opens CopyProfile modal with pre-filled buffer
- [x] 7.2 Add test: Enter in CopyProfile modal with valid new name calls `copy_profile` and reloads
- [x] 7.3 Add test: Escape in CopyProfile modal cancels and closes modal
- [x] 7.4 Add test: `c` key while on snapshot list does nothing

## 8. Pre-commit Verification

- [x] 8.1 Run `cargo fmt --check && cargo test && cargo clippy --all-targets --all-features -- -D warnings`
