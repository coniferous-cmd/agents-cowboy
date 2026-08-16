## 1. State Changes

- [x] 1.1 Add `PickProfile { profile_cursor: usize }` variant to `ModalState` in `app/mod.rs`
- [x] 1.2 Add `pending_profile_override: Option<PathBuf>` field to `AppState` in `app/mod.rs`
- [x] 1.3 Update `ModalState::default()` — `PickProfile` is never the initial state, no change needed

## 2. Key Handling

- [x] 2.1 In `handle_normal_key`, add `p` key: if a real project is selected (not "Open Here"), enter `ModalState::PickProfile { profile_cursor: 0 }`
- [x] 2.2 Add `handle_pick_profile_key` method in `modal` module: handles `↑↓` navigation, `q/Esc` cancel, and `Enter` to launch
- [x] 2.3 On `Enter` in `PickProfile`: resolve profile path via `application.profile_file_path(selected_profile_name)`, set `pending_profile_override = Some(path)`, set `pending_new_session = Some(cwd)`, set `should_quit = true`

## 3. UI Rendering

- [x] 3.1 In `render()` in `ui/mod.rs`, add `ModalState::PickProfile { .. }` arm that calls `render_bind_profile_modal` (same widget as BindProfile, different title)
- [x] 3.2 Add `(Shift+Enter, "Pick Profile")` hint entry to `shortcuts_for` for `FocusPane::Projects` state

## 4. Launch Integration

- [x] 4.1 In `main.rs`, after TUI exits, check if both `pending_new_session` and `pending_profile_override` are set; if so, build `Command` with `--settings <profile_override_path>` added to the `claude` invocation
- [x] 4.2 Ensure `pending_profile_override` is cleared after consumption (in `resume_finished` / `new_session_finished` path)

## 5. Tests

- [x] 5.1 Add test: `Shift+Enter` on a real project enters `PickProfile` modal
- [x] 5.2 Add test: `Shift+Enter` on "Open Here" falls through to normal Enter (opens new session in current dir)
- [x] 5.3 Add test: `Enter` in `PickProfile` sets `pending_profile_override` and `pending_new_session`
- [x] 5.4 Add test: `q` in `PickProfile` cancels and returns to `ModalState::None`
- [x] 5.5 Add test: `pending_profile_override` is cleared after `resume_finished`
