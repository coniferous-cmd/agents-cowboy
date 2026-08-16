## Context

Cowboy currently manages Claude profiles as a global concept: activating a profile swaps the `~/.claude/settings.json` symlink. The `ClaudeCliLauncher` builds `claude` commands without any `--settings` flag. Claude CLI supports `--settings <file-or-json>` for per-session settings injection.

The project has a well-established layered architecture:
- **Domain** (`domain.rs`): Core types and errors
- **Infrastructure** (`infrastructure/`): SQLite store, project path resolution
- **Claude Env** (`claude_env/`): Profile CRUD, settings, store initialization
- **Application** (`application.rs`): Trait-based abstractions (`ProfileRepository`, `ResumeLauncher`)
- **UI** (`app/`): TUI state, navigation, modals
- **CLI** (`cmd/`): Subcommand parsing and handling

## Goals / Non-Goals

**Goals:**
- Bind a profile to a project so Claude launches with `--settings` automatically
- Preserve global profile activation as fallback for unbound projects
- Surface bindings in both TUI and CLI
- Prevent accidental deletion of bound profiles

**Non-Goals:**
- Supporting multiple profiles per project (one binding per project only)
- Automatic profile switching when `cd`-ing between directories (this is a launcher-time feature, not a shell hook)
- Syncing bindings across machines
- Changing the global profile activation mechanism

## Decisions

### 1. Binding storage: SQLite table in existing cowboy.db

**Choice**: New table `project_profile_bindings` in the existing database.

**Rationale**: Profiles, snapshots, and settings already live in the same DB. Adding another table keeps all state co-located. The alternative (project-local `.cowboy/config` files) would scatter state across the filesystem and complicate the "list all bindings" operation.

**Schema**:
```sql
CREATE TABLE project_profile_bindings (
    project_cwd TEXT PRIMARY KEY,
    profile_name TEXT NOT NULL UNIQUE,
    created_at TEXT DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (profile_name) REFERENCES claude_profiles(name)
        ON DELETE RESTRICT
);
```

`profile_name` is `UNIQUE` to enforce one-binding-per-profile (a profile can only be bound to one project at a time). This is a deliberate constraint — if the same profile is bound to two projects, `--settings` works fine, but the mental model of "this profile IS this project" is cleaner with 1:1 mapping. If this proves too restrictive, the constraint can be relaxed later.

### 2. Settings path resolution

**Choice**: Pass the absolute path to the profile's settings JSON file via `--settings`.

**Profile file paths**:
- macOS: `~/Library/Application Support/cowboy/profiles/settings.<name>.json`
- Linux: `~/.config/cowboy/profiles/settings.<name>.json`

**Rationale**: Claude CLI's `--settings` accepts a file path. The profile JSON files already exist on disk (written by `AtomicReplace::write` during profile creation/update). No new file I/O needed — just resolve the path.

**Alternative considered**: Inline JSON via `--settings '{"key":"value"}'`. Rejected because it requires escaping JSON on the command line and doesn't benefit from the existing atomic file write infrastructure.

### 3. Launcher modification

**Choice**: `ClaudeCliLauncher` queries bindings before building the command.

**Flow**:
```
ClaudeCliLauncher::resume(target) / launch_new(cwd)
  → env_store.project_binding(cwd)?
  → Some(binding) → Command.arg("--settings").arg(profile_file_path(&binding.profile_name))
  → None → no --settings flag (global symlink fallback)
```

**Rationale**: The launcher already wraps `ClaudeEnvStore`. Adding one query per launch is negligible. The binding lookup happens on the main thread during the brief TUI-exit gap, not during the TUI render loop.

### 4. UI: Bind via profile picker modal

**Choice**: Pressing `b` on a project opens a modal listing available profiles (similar to the existing profile activation list). Selecting a profile creates the binding.

**Rationale**: Reuses the existing profile list data already loaded into `AppState`. The modal pattern is established (search, rename, new profile all use `ModalState` variants).

**New modal state**: `ModalState::BindProfile` with a cursor over available profiles.

### 5. Delete protection

**Choice**: `ON DELETE RESTRICT` foreign key + application-layer check.

**Rationale**: Database-level constraint prevents orphaned bindings. Application-layer check provides a clear error message ("profile 'work' is bound to project '/work/api'; unbind first").

### 6. One binding per project, one project per profile

**Choice**: `project_cwd PRIMARY KEY` + `profile_name UNIQUE`.

**Rationale**: Enforces 1:1 mapping. A project can only use one profile. A profile can only serve one project. This is the simplest mental model. If a user wants two projects to share a profile, they should create two profiles with the same settings (or we relax the UNIQUE constraint later).

## Risks / Trade-offs

**[Risk] `--settings` and global `settings.json` interaction**
Claude CLI may merge `--settings` with the global `~/.claude/settings.json`. If so, the global symlink's settings could leak into bound sessions.
→ **Mitigation**: Test empirically. If they merge, we may need to pass `--bare` or adjust the approach. The spec says `--settings` is passed, but the exact Claude CLI behavior needs verification during implementation.

**[Risk] Profile file path stability**
If the DB path changes (e.g., user reconfigures `metadata_db_path`), profile file paths change too.
→ **Mitigation**: Profile file paths are derived from `profiles_dir()` which derives from `self.path().parent()`. This is already the established pattern — no new risk.

**[Risk] Binding stale after profile rename**
If a profile is renamed, the binding references the old name.
→ **Mitigation**: Profile names are immutable in the current system (create + delete + recreate). No rename operation exists, so this is not a real risk today.

**[Trade-off] 1:1 constraint**
The UNIQUE constraint on `profile_name` means a profile can only bind to one project. This is simpler but less flexible.
→ **Future**: If needed, drop the UNIQUE constraint and allow N:M. The schema change is a single `ALTER TABLE`.

## Migration Plan

1. Add `project_profile_bindings` table in the schema migration (new version bump)
2. No data migration needed — new table starts empty
3. Rollback: Drop the table (no existing data affected)

## Open Questions

- Does Claude CLI's `--settings` merge with or override the global `settings.json`? Needs empirical testing during implementation.
