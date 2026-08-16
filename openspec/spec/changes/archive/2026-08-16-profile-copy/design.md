## Context

Cowboy manages Claude Code profiles for users. Profiles store JSON settings (env vars, rules, etc.) and can be activated, edited, deleted, and bound to projects. There is currently no way to duplicate a profile — users must manually recreate and copy-paste JSON via `$EDITOR`.

See `proposal.md` for motivation.

## Goals / Non-Goals

**Goals:**
- Add `cowboy config copy <source> <new-name>` CLI command
- Add `c` key + copy modal in TUI Profiles tab
- Reuse existing JSON validation, profile name validation, and error handling patterns

**Non-Goals:**
- Auto-renaming on conflict (duplicate names are rejected, consistent with `create`)
- Copying snapshots (they are read-only history)
- Transferring project bindings to the new profile

## Decisions

### 1. CLI argument order: `<source> <new-name>`

Consistent with `create <name>`, `edit <name>`, `delete <name>` — all single positional name args. Copy needs two, and `<source>` first mirrors the English "copy X to Y" phrasing.

**Alternatives considered:**
- `--from <source> --into <new-name>` flags: more explicit but verbose; inconsistent with existing single-name commands

### 2. Store method: `copy_profile(source, new_name)`

The store already has `create_profile` and `profile`. A new `copy_profile` method reads the source profile's `settings_json`, validates the new name, then calls `create_profile` + `update_profile_json` atomically (within a transaction).

**Alternatives considered:**
- Add `copy_profile` that duplicates the row directly: would bypass `update_profile_json` and not write the profile file — less consistent with existing flow
- Have the CLI handler call `profile()` then `create_profile()` then `update_profile_json()`: leaks DB transaction across multiple store calls; the store-level method keeps it atomic

### 3. TUI modal state: `ModalState::CopyProfile`

Mirrors the existing `ModalState::NewProfile` pattern:
- `input_buffer` holds the typed name (pre-filled with `"Copy of {source}"`)
- `handle_copy_key` handles input (Enter/Esc/Backspace/Char)
- On Enter: validate name → call `application.copy_profile()` → reload profiles on success or show toast on error

**Alternatives considered:**
- Inline rename-style modal without pre-fill: user loses the source name reference; pre-fill reduces typing
- Separate `CopyProfile` struct with its own `source_name` field: more state than needed; `input_buffer` + a comment in the handler is sufficient

### 4. New key: `c` for copy

Chosen as the Copy action's first letter,左手易按,不与现有键冲突 (`n`=new, `e`=edit, `d`=delete).

## Risks / Trade-offs

- **Risk**: User types a duplicate name in the TUI modal → mitigated by validation and toast error; no silent override
- **Risk**: Very large `settings_json` (MB-scale) → `copy_profile` reads it into memory then writes it back; profile JSON in Claude settings is typically <100KB so this is not a concern in practice
