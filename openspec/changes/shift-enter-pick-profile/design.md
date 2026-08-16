## Context

See proposal.md — Why section for motivation. This design covers the implementation approach for adding a one-shot profile picker launched via `Shift+Enter`.

## Goals / Non-Goals

**Goals:**
- Add `p` as a shortcut to pick a profile before launching a new session
- Reuse existing UI components (render_bind_profile_modal) for the picker popup
- Pass the picked profile via `--settings <path>` CLI flag — no store changes needed
- Keep the existing binding intact after the session

**Non-Goals:**
- Changing how `e` / `BindProfile` works — that remains a permanent binding operation
- Supporting `Shift+Enter` on the Sessions pane or in the Profiles tab
- Persisting the picked profile in any way

## Decisions

### Modal reuse: `PickProfile` vs. extending `BindProfile`

**Decision**: Add a new `ModalState::PickProfile { profile_cursor }` variant rather than reusing `BindProfile`.

**Rationale**: `BindProfile` carries semantics of "bind on Enter" — its Enter handler calls `bind_profile()`. A one-shot picker needs the same navigation but a different Enter outcome (launch, not bind). Mixing these in one modal risks subtle bugs. Separating them keeps each modal's purpose clear.

### Profile path resolution

**Decision**: Resolve the profile's settings file path via `ClaudeEnvStore::profile_file_path(name)` when the popup's Enter is pressed, and store that `PathBuf` in `pending_profile_override`.

**Rationale**: The path is needed by `main.rs` at launch time. Storing it avoids re-resolving after the TUI exits. The path is stable for the duration of the session launch.

### `pending_profile_override` lives in `AppState`

**Decision**: Add `pending_profile_override: Option<PathBuf>` to `AppState`, alongside `pending_new_session: Option<PathBuf>`.

**Rationale**: Both fields are consumed by `main.rs` in the same launch codepath. Having them together makes the intent clear — `pending_new_session` says "launch here" and `pending_profile_override` says "use this profile instead of the bound one". The `ResumeLauncher` interface does not change; the override is handled at the `main.rs` call site.

### `launch_new` signature unchanged

**Decision**: Keep `ResumeLauncher::launch_new(&self, cwd: &Path) -> AppResult<()>` unchanged. The profile override is NOT passed through the trait — it is handled at the call site in `main.rs`.

**Rationale**: The trait is implemented by `ClaudeCliLauncher` which already has access to `env_store`. `main.rs` resolves the override path and passes `--settings <path>` directly to the `Command`. This avoids threading a new parameter through the trait and all its implementations.

## Risks / Trade-offs

[Risk] **Profile deleted between picker and launch** → If the profile file is deleted between the time the user selects it and the time `main.rs` runs, the `--settings` path will be stale. **Mitigation**: The CLI will fail with a non-zero exit; cowboy surfaces this as an error. Acceptable — this is an edge case and not worse than the current binding behavior if a profile is deleted after being bound.

[Risk] **No profiles available** → If the profile list is empty, `Shift+Enter` should fall back gracefully (same as `Enter`). **Mitigation**: The handler checks if `profiles.is_empty()` and falls through to normal Enter behavior.

[Trade-off] **Duplicate popup code** → `PickProfile` and `BindProfile` render identically. This is intentional — they share the same widget-building logic (`render_bind_profile_modal`) but differ in their Enter handler. Future refactoring could extract a shared helper, but the current approach is simpler.
