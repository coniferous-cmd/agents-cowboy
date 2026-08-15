## 1. Database Schema

- [x] 1.1 Add `project_profile_bindings` table to schema migration in `claude_env/schema.rs` (column: `project_cwd TEXT PRIMARY KEY`, `profile_name TEXT NOT NULL UNIQUE`, `created_at TEXT`, FK to `claude_profiles(name) ON DELETE RESTRICT`)
- [x] 1.2 Bump schema version and add migration logic for the new table

## 2. Domain Layer

- [x] 2.1 Add `ProjectProfileBinding` struct to `domain.rs` (fields: `project_cwd: PathBuf`, `profile_name: String`)
- [x] 2.2 Add error variants `BindingNotFound` and `ProfileAlreadyBound` to `StetsonError`

## 3. Infrastructure — Binding CRUD

- [x] 3.1 Add `bind_profile(project_cwd: &Path, profile_name: &str) -> Result<()>` to `ClaudeEnvStore` (insert or replace, validate profile exists first)
- [x] 3.2 Add `unbind_profile(project_cwd: &Path) -> Result<()>` to `ClaudeEnvStore`
- [x] 3.3 Add `project_binding(project_cwd: &Path) -> Result<Option<ProjectProfileBinding>>` to `ClaudeEnvStore`
- [x] 3.4 Add `profile_bindings(profile_name: &str) -> Result<Vec<ProjectProfileBinding>>` to `ClaudeEnvStore` (for delete protection check)
- [x] 3.5 Write unit tests for all binding CRUD operations (create, read, update, delete, not-found cases)

## 4. Application Layer — Trait + Launcher

- [x] 4.1 Extend `ProfileRepository` trait with `bind_profile`, `unbind_profile`, `project_binding`, `profile_bindings` methods
- [x] 4.2 Implement new trait methods on `ClaudeEnvStore` (delegation layer in `application.rs`)
- [x] 4.3 Add stub implementations to `NoProfileRepository` (return errors)
- [x] 4.4 Modify `ClaudeCliLauncher::resume()` to query binding and pass `--settings <path>` when binding exists
- [x] 4.5 Modify `ClaudeCliLauncher::launch_new()` to query binding and pass `--settings <path>` when binding exists
- [x] 4.6 Write tests for launcher with mocked profile repository (verify `--settings` is passed when bound, omitted when unbound)

## 5. TUI — Binding UI

- [x] 5.1 Add `ModalState::BindProfile` variant with profile cursor state
- [x] 5.2 Add `b` keybinding in Projects tab to open the bind profile modal
- [x] 5.3 Implement profile picker modal (list available profiles, navigate with up/down, Enter to bind, Esc to cancel)
- [x] 5.4 Display bound profile name indicator next to project names in the Projects tab (e.g., `[work] project-name`)
- [x] 5.5 Add `u` keybinding in Projects tab to unbind (with confirmation or direct action)
- [x] 5.6 Write tests for bind modal navigation and binding action

## 6. CLI — Bind/Unbind Subcommands

- [x] 6.1 Add `cowboy bind <project-path> <profile-name>` subcommand parsing in `cmd/config.rs`
- [x] 6.2 Add `cowboy unbind <project-path>` subcommand parsing in `cmd/config.rs`
- [x] 6.3 Implement bind command handler (validate project path exists, validate profile exists, create binding)
- [x] 6.4 Implement unbind command handler (validate binding exists, remove it)
- [x] 6.5 Write tests for CLI bind/unbind commands

## 7. Delete Protection

- [x] 7.1 Modify `delete_profile` in `ClaudeEnvStore` to check for active bindings before deletion
- [x] 7.2 Return descriptive error when attempting to delete a bound profile
- [x] 7.3 Write tests for delete protection (delete bound profile fails, delete unbound profile succeeds)

## 8. Verification

- [x] 8.1 Run `cargo fmt --check`
- [x] 8.2 Run `cargo test`
- [x] 8.3 Run `cargo clippy --all-targets --all-features -- -D warnings`
