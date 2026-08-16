## Why

Users need to duplicate an existing profile to create a variation — e.g., copy `work` to `work-debug`, tweak settings, and activate the debug version — without losing the original or manually replicating its JSON content.

Currently there is no copy operation; users must create a new profile and manually copy-paste settings via `$EDITOR`, which is error-prone and tedious.

## What Changes

- **CLI**: `cowboy config copy <source> <new-name>` — reads source profile's settings JSON, creates a new profile with that JSON under the new name
- **TUI**: Profiles tab — select a profile, press `c`, enter new name in modal, confirm to duplicate
- Both interfaces validate the new name (1–64 ASCII chars, not duplicate) and report errors clearly
- Copying does NOT transfer project bindings; the new profile starts unbound

## Capabilities

### New Capabilities

- `profile-copy`: Add support for duplicating an existing profile with a new name, preserving all settings JSON. Supported in both CLI (`cowboy config copy`) and TUI (Profiles tab, `c` key + modal). Duplicate names are rejected — no auto-suffix behavior.

## Impact

- **New CLI command**: `copy` added to `ConfigCommand` enum; `parse_config_args` handles it
- **New store method**: `copy_profile(source, new_name)` in `ClaudeEnvStore`
- **New TUI state**: `ModalState::CopyProfile` (mirrors `NewProfile` modal flow)
- **New key handler**: `c` key in Profiles tab triggers copy modal
- **No breaking changes** to existing commands, APIs, or data formats
