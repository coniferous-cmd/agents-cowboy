# Cowboy Specification

## 1. UI Specification

### Layout

#### Main Layout

```text
┌─ Projects ─────────────────┬─ Sessions ────────────────────┐
│ (project list)             │ (session list)                │
│                            │                               │
│                            │                               │
│                            │                               │
├─ Usage Totals ─────────────┴─ Session Details ─────────────┤
│ (token stats)              │ (session details/search/rename)│
└────────────────────────────┴───────────────────────────────┘
```

- Dual-column equal-width layout, minimum 80 columns × 24 rows
- Top tabs: Projects / Sessions / Profiles
- Middle dual-column: left panel for projects, right panel for sessions
- Bottom panel: left for token stats, right for session details

#### Tab System

- `[` / `]` cycle through three tabs
- Projects / Sessions share the dual-column layout
- Profiles uses a dedicated page

### Color Themes

#### Monokai (Default)

| Semantic Use    | Color     | Hex       |
|----------------|-----------|-----------|
| Accent          | Yellow    | `#F9CE34` |
| Project name    | Green     | `#A8E6CF` |
| Session title   | White     | `#F8F8F2` |
| Token numbers   | Cyan      | `#A8E6CF` |
| Success status  | Green     | `#A8E6CF` |
| Error status    | Red       | `#FF6B6B` |
| Disabled        | Gray      | `#75715E` |

#### Nord

| Semantic Use    | Color     | Hex       |
|----------------|-----------|-----------|
| Accent          | Light blue| `#88C0D0` |
| Project name    | Green     | `#A3BE8C` |
| Session title   | White     | `#ECEFF4` |
| Token numbers   | Cyan      | `#88C0D0` |
| Success status  | Green     | `#A3BE8C` |
| Error status    | Red       | `#BF616A` |
| Disabled        | Gray      | `#4C566A` |

#### Gruvbox Dark

| Semantic Use    | Color     | Hex       |
|----------------|-----------|-----------|
| Accent          | Yellow    | `#FABD2F` |
| Project name    | Green     | `#B8BB26` |
| Session title   | White     | `#FBF1C7` |
| Token numbers   | Cyan      | `#8EC07C` |
| Success status  | Green     | `#B8BB26` |
| Error status    | Red       | `#FB4934` |
| Disabled        | Gray      | `#665C54` |

### Navigation

#### Global Keybindings

| Key             | Action                        |
|-----------------|-------------------------------|
| `j` / `↓`      | Move focus down               |
| `k` / `↑`      | Move focus up                 |
| `h` / `←`      | Left panel focus / prev tab   |
| `l` / `→`      | Right panel focus / next tab  |
| `Tab`           | Toggle panel focus            |
| `[` / `]`       | Switch tabs                   |
| `g`             | Jump to top                   |
| `G`             | Jump to bottom                |
| `?`             | Show help                     |
| `q` / `Esc`     | Quit                          |

#### Project Actions

| Key      | Action                          |
|----------|---------------------------------|
| `Enter`  | Select project, show sessions   |
| `n`      | Create new session in project   |
| `d`      | Delete project (with confirm)   |

#### Session Actions

| Key      | Action                          |
|----------|---------------------------------|
| `Enter`  | Resume session                  |
| `i`      | Show session details            |
| `/`      | Search sessions                 |
| `r`      | Rename session                  |
| `x`      | Delete session (with confirm)   |

#### Profiles Actions

| Key         | Action                          |
|-------------|---------------------------------|
| `Enter`     | Activate selected Profile       |
| `n`         | Create new Profile              |
| `Ctrl+D`    | Delete Profile (with confirm)   |

### Theme Switching

- `t` opens theme selector
- Up/down arrows to choose theme
- `Enter` confirms, `Esc` cancels
- Theme change applies immediately and persists

### Search

- `/` enters search mode
- Real-time filtering of session list
- `Esc` exits search
- Matching text highlighted in results

### Delete Confirmation

- `x` or `d` triggers delete
- Confirmation dialog appears
- `y` / `Enter` confirms, `n` / `Esc` cancels
- Other operations disabled during deletion

### Empty States

- No projects: "No projects found"
- No sessions: "No sessions in this project"
- No search results: "No matching sessions"

### Responsive

- Minimum size: 80×24
- Warning displayed below minimum
- Supports terminal window resizing

### Accessibility

- Screen reader support
- High contrast mode
- Customizable colors

---

## 2. CLI Specification

### Commands

#### `cowboy`

Launch the TUI interface.

```bash
cowboy
```

#### `cowboy project`

Project management commands.

```bash
cowboy project list                    # List all projects
cowboy project sessions <project>      # List sessions in a project
cowboy project new-session <project>   # Create new session in project
cowboy project delete <project>        # Delete project (with confirmation)
```

#### `cowboy session`

Session management commands.

```bash
cowboy session list                    # List all sessions
cowboy session list --project <name>   # List sessions for a project
cowboy session resume <session>        # Resume a session
cowboy session info <session>          # Show session details
cowboy session rename <session> <name> # Rename a session
cowboy session delete <session>        # Delete session (with confirmation)
cowboy session search <query>          # Search sessions
```

#### `cowboy config`

Profile and settings management.

```bash
cowboy config list                     # List all profiles
cowboy config create <name>            # Create new profile
cowboy config edit <name>              # Edit profile with $EDITOR
cowboy config delete <name>            # Delete profile
cowboy config activate <name>          # Activate a profile
```

#### `cowboy config history`

Snapshot management.

```bash
cowboy config history list             # List all snapshots
cowboy config history show <id>        # Show snapshot content
cowboy config history activate <id>    # Activate a snapshot
cowboy config history delete <id>      # Delete a snapshot
cowboy config history prune --keep <n> # Keep n most recent snapshots
```

#### `cowboy theme`

Theme management.

```bash
cowboy theme list                      # List available themes
cowboy theme set <theme>               # Set active theme
cowboy theme current                   # Show current theme
```

#### `cowboy stats`

Usage statistics.

```bash
cowboy stats                           # Show overall statistics
cowboy stats --project <name>          # Show project statistics
cowboy stats --session <id>            # Show session statistics
```

### Global Options

| Option              | Description                          |
|---------------------|--------------------------------------|
| `--help`            | Show help information                |
| `--version`         | Show version number                  |
| `--config-dir <path>` | Override config directory          |
| `--database <path>` | Override database path               |
| `--no-color`        | Disable colored output               |
| `--json`            | Output in JSON format                |

### Output Formats

#### Default (Human-readable)

```text
Project: agent-cowboy
  Sessions: 12
  Total tokens: 1.2M
  Estimated cost: $0.45
```

#### JSON (`--json`)

```json
{
  "project": "agent-cowboy",
  "sessions": 12,
  "total_tokens": 1200000,
  "estimated_cost": 0.45
}
```

### Exit Codes

| Code | Description                    |
|------|--------------------------------|
| 0    | Success                        |
| 1    | General error                  |
| 2    | Invalid arguments              |
| 3    | Resource not found             |
| 4    | Permission denied              |
| 5    | Database error                 |
| 6    | Network error                  |

### Environment Variables

| Variable            | Description                          |
|---------------------|--------------------------------------|
| `COWBOY_CONFIG_DIR` | Override config directory            |
| `COWBOY_DATABASE`   | Override database path               |
| `COWBOY_THEME`      | Override theme                       |
| `COWBOY_NO_COLOR`   | Disable colors (any non-empty value) |
| `EDITOR`            | Editor for profile editing           |

### Examples

#### List all projects with stats

```bash
cowboy project list
```

Output:
```text
agent-cowboy     12 sessions   1.2M tokens   $0.45
web-app          8 sessions    800K tokens   $0.30
api-server       5 sessions    500K tokens   $0.20
```

#### Create and activate a profile

```bash
cowboy config create work
cowboy config edit work
cowboy config activate work
```

#### Search sessions

```bash
cowboy session search "authentication"
```

Output:
```text
Found 3 sessions:
  [1] Add OAuth support (2 hours ago)
  [2] Fix login bug (1 day ago)
  [3] Implement JWT tokens (3 days ago)
```

#### Show session details in JSON

```bash
cowboy session info abc123 --json
```

Output:
```json
{
  "id": "abc123",
  "title": "Add OAuth support",
  "project": "web-app",
  "created_at": "2026-08-10T14:30:00Z",
  "updated_at": "2026-08-10T16:45:00Z",
  "tokens": {
    "input": 50000,
    "output": 25000
  },
  "cost": 0.15,
  "model": "claude-sonnet-4-20250514"
}
```

---

## 3. Profiles Specification

### Overview

Profiles allow users to save multiple named Claude Code setting snippets to cowboy's SQLite metadata database and activate one when needed, overwriting `~/.claude/settings.json`.

This replaces the legacy project-level config editing and environment variable metadata mechanism; Profiles is the only settings management entry point retained by the application.

### Data Model

#### Profile Table (`claude_profiles`)

| Column          | Type    | Constraints                          |
|-----------------|---------|--------------------------------------|
| `id`            | INTEGER | PRIMARY KEY                          |
| `name`          | TEXT    | NOT NULL UNIQUE                      |
| `settings_json` | TEXT    | NOT NULL                             |
| `updated_at`    | TEXT    | NOT NULL DEFAULT CURRENT_TIMESTAMP   |

##### Name Rules

- ASCII letters, digits, hyphens (`-`), underscores (`_`) only
- Maximum 64 characters
- Case-insensitive: input normalized to ASCII lowercase after validation
- Empty names rejected

##### Settings JSON Rules

- Must parse as JSON object
- Accepts any Claude Code settings fields
- Rejects: empty string, parse errors, `null`, arrays, scalars
- Accepts: `{}`, objects with `_comment`, `$schema`, etc.

#### Snapshot Table (`claude_settings_snapshots`)

| Column          | Type    | Constraints                          |
|-----------------|---------|--------------------------------------|
| `id`            | INTEGER | PRIMARY KEY                          |
| `captured_at`   | TEXT    | NOT NULL DEFAULT CURRENT_TIMESTAMP   |
| `source`        | TEXT    | Optional (e.g., `pre-activate:work`) |
| `settings_json` | TEXT    | NOT NULL                             |

- Created automatically before profile activation
- Not shown in `config list` output
- Managed via `config history` commands

#### Active Profile Setting

- Stored in `settings` table as `active_profile_name`
- Set after successful activation
- Cleared on snapshot activation or profile deletion
- Not affected by external `settings.json` modifications

#### Activation Journal (`profile_activation_journal`)

| Column            | Type    | Constraints                          |
|-------------------|---------|--------------------------------------|
| `id`              | INTEGER | PRIMARY KEY                          |
| `target_kind`     | TEXT    | NOT NULL (`profile` or `snapshot`)   |
| `target_id`       | TEXT    | NOT NULL                             |
| `target_name`     | TEXT    | NULL for snapshots                   |
| `target_json_hash`| TEXT    | NOT NULL (SHA-256)                   |
| `phase`           | TEXT    | NOT NULL (`prepared`/`file_replaced`/`failed`) |
| `error`           | TEXT    | Only for `failed` phase              |
| `created_at`      | TEXT    | NOT NULL DEFAULT CURRENT_TIMESTAMP   |

- Singleton table (fixed `id=1`)
- Coordinates SQLite and filesystem operations
- Enables crash recovery

### Activation Flow

#### Steps

1. **Acquire lock**: Application-level mutex (advisory lock on database directory)
2. **Validate target**: Read and validate target profile's `settings_json`
3. **Read current**: Read current `~/.claude/settings.json`
4. **Capture snapshot** (if current exists):
   - Parse as JSON object
   - Insert into `claude_settings_snapshots` with `source: pre-activate:<profile_name>`
   - Write `prepared` journal
5. **Atomic replace**:
   - Write to temporary file: `<path>.tmp.<pid>.<nanos>`
   - `sync_all()`
   - `rename()` (Unix) or `ReplaceFileW` (Windows)
   - Preserve file permissions/ACL
6. **Commit state**:
   - Mark journal as `file_replaced`
   - Write `active_profile_name`
   - Auto-prune snapshots (keep 100 most recent)
   - Clear journal

#### Error Handling

| Failure Point               | Behavior                                      |
|-----------------------------|-----------------------------------------------|
| Current file missing        | No snapshot created, proceed                   |
| Current file invalid JSON   | Reject activation, keep original bytes         |
| Snapshot INSERT fails       | Reject activation, keep original bytes         |
| Profile not found/invalid   | Reject activation, keep original bytes         |
| Temp file creation fails    | Reject activation, keep original bytes         |
| Atomic replace fails        | Reject activation, keep original bytes         |
| Final DB transaction fails  | Keep new file, preserve journal, recover on startup |

#### Crash Recovery

On startup, if journal exists:
- If `settings.json` SHA-256 matches `target_json_hash` → complete activation
- Otherwise → mark as `failed`, preserve snapshot

### TUI Integration

#### Tab Navigation

- `[` / `]` cycles through Projects / Sessions / Profiles tabs
- Profiles tab has dedicated page layout

#### Profiles Page Layout

```text
┌─ Profiles ──────────────────────────────────────────────────┐
│ ● work        (active)                                      │
│   personal                                                  │
│   testing                                                   │
├─ Snapshots ─────────────────────────────────────────────────┤
│ 2026-08-15 14:30  1.2KB  pre-activate:work                  │
│ 2026-08-14 10:15  1.1KB  pre-activate:personal              │
│ 2026-08-13 09:00  1.0KB  pre-activate:testing               │
└─────────────────────────────────────────────────────────────┘
```

#### Keybindings

| Key         | Action                              |
|-------------|-------------------------------------|
| `↑`/`↓`    | Move focus between profiles/snapshots |
| `Enter`     | Activate selected profile/snapshot  |
| `n`         | Create new profile                  |
| `Ctrl+D`    | Delete focused profile              |

#### Active Profile Indicator

- Displayed as `●` or `(active)` next to profile name
- Only shown in profile list, not in snapshots
- Source: `settings.active_profile_name`

### Security

#### File Permissions

- Database parent directory: user-only access
- Database file: user-only access
- WAL/SHM files: same or stricter permissions
- `settings.json`: preserve original permissions (default `0600` on Unix)

#### Sensitive Data

- Profiles may contain OAuth tokens, API keys, MCP secrets
- `config history show` warns about sensitive content
- Snapshot export includes full JSON (potential secrets)

### Testing Requirements

#### Profile Tests

- Schema initialization and migration
- Name validation (including case conflicts, 64/65 char boundary)
- Stable sorting
- CLI argument errors
- Create/edit/delete operations
- Missing profile and invalid JSON handling

#### Snapshot Tests

- Schema initialization
- Pre-activation snapshot capture (with source tag)
- History list/show/activate/delete/prune
- History activation doesn't create new snapshot
- Can overwrite corrupted settings.json

#### Activation Tests

- Single profile atomic overwrite (cross-platform)
- Target directory creation
- Pre-switch snapshot and journal writing
- Journal recovery after crash
- Global settings bytes unchanged on pre-atomic-replace errors
- Windows: no `remove` + `rename` fallback
- Post-activation permissions match or tighten

#### Migration Tests

- Migration from old schema generates `cowboy-migrated-*.json` dump
- Empty database migration doesn't generate empty dump
- Dump failure aborts migration

---

## 4. Profile Activation Specification

### Purpose

Defines how cowboy stores Claude profiles as files, swaps `~/.claude/settings.json` to a symlink, migrates existing real settings files, and keeps SQLite in sync — covering activation, snapshot activation, dual-write updates, deletion, and crash recovery.

### Requirements

#### Requirement: File-backed profile storage

The system SHALL store each profile's JSON payload as `<profiles_dir>/settings.<name>.json`, where `<profiles_dir>` is derived from cowboy's metadata database path (macOS: `~/Library/Application Support/cowboy/profiles/`, Linux/other: `<config_dir>/cowboy/profiles/`). The profile file SHALL be created with mode `0o600` on Unix and replaced atomically.

##### Scenario: Profile file is created with private mode
- **WHEN** a new profile `work` is activated and the file does not exist
- **THEN** `<profiles_dir>/settings.work.json` exists with mode `0o600` and contains exactly the profile's JSON

##### Scenario: Profile file is atomically replaced on activation
- **WHEN** profile `work` is activated and `settings.work.json` already exists
- **THEN** the file is replaced atomically (no truncated intermediate state observable to readers) and its mode is preserved

#### Requirement: Symlink to the active profile

The system SHALL make `~/.claude/settings.json` a symbolic link that points to the profile file corresponding to the active profile. Claude Code reading `~/.claude/settings.json` SHALL observe the active profile's JSON content.

##### Scenario: Activation produces a working symlink
- **WHEN** profile `work` is activated
- **THEN** `~/.claude/settings.json` is a symbolic link whose target resolves to `<profiles_dir>/settings.work.json`, and reading the symlink returns the profile JSON

##### Scenario: Re-activation swaps the symlink target
- **WHEN** profile `home` is activated while `work` is currently active
- **THEN** `~/.claude/settings.json` is a symbolic link whose target now resolves to `<profiles_dir>/settings.home.json`, and `settings.work.json` is untouched

#### Requirement: First-activation migration of an existing real settings file

The system SHALL detect, on the first profile activation in a session where the SQLite store does not yet have a `default` profile, whether `~/.claude/settings.json` exists as a regular file (not a symlink) with valid JSON. When detected, the system SHALL register the contents as a `default` profile, write them to `<profiles_dir>/settings.default.json`, and replace `~/.claude/settings.json` with a symlink to that file before performing the requested activation.

##### Scenario: Existing real settings are imported as default
- **WHEN** `~/.claude/settings.json` exists as a regular file containing valid JSON, and the user activates any profile for the first time
- **THEN** a `default` profile appears in `list_profiles`, `<profiles_dir>/settings.default.json` exists with the imported JSON, and `~/.claude/settings.json` is now a symlink

##### Scenario: Already-migrated stores do not re-import
- **WHEN** a `default` profile already exists in SQLite and `~/.claude/settings.json` is already a symlink
- **THEN** activation proceeds without re-reading or rewriting the symlink target

##### Scenario: Invalid existing settings abort activation
- **WHEN** `~/.claude/settings.json` exists as a regular file containing invalid JSON
- **THEN** the requested activation fails, no profile file is written, no symlink is replaced, and `~/.claude/settings.json` remains a regular file with its original contents

#### Requirement: Snapshot activation writes an orphan file

The system SHALL activate a historical snapshot by writing the snapshot JSON to `<profiles_dir>/settings._orphan.json` and symlinking `~/.claude/settings.json` to it. No profile name SHALL be set as active after snapshot activation.

##### Scenario: Snapshot activation clears the active profile
- **WHEN** a snapshot is activated
- **THEN** `~/.claude/settings.json` is a symbolic link whose target resolves to `<profiles_dir>/settings._orphan.json`, and `active_profile_name` returns `None`

##### Scenario: Re-activating a profile clears the orphan
- **WHEN** a profile is activated after a snapshot activation
- **THEN** the symlink target becomes `<profiles_dir>/settings.<name>.json`, and `<profiles_dir>/settings._orphan.json` is left untouched

#### Requirement: Dual-write on profile JSON edits

The system SHALL persist every successful edit to a profile's JSON to both SQLite (`claude_profiles.settings_json`) and `<profiles_dir>/settings.<name>.json` before reporting success. A failure in either write SHALL cause the operation to return an error and leave the previously persisted state unchanged.

##### Scenario: Edit updates both stores
- **WHEN** `update_profile_json("work", json)` succeeds
- **THEN** `profile("work").settings_json` equals `json` and `<profiles_dir>/settings.work.json` contains `json`

##### Scenario: File write failure aborts the edit
- **WHEN** `<profiles_dir>/settings.<name>.json` cannot be written (e.g., directory read-only)
- **THEN** `update_profile_json` returns an error and the SQLite row is unchanged

#### Requirement: Profile deletion cleans up files and active symlink

The system SHALL remove both the SQLite row and `<profiles_dir>/settings.<name>.json` when a profile is deleted. If the deleted profile is the currently active one, the system SHALL remove the symlink at `~/.claude/settings.json` and clear `active_profile_name`.

##### Scenario: Deleting an inactive profile removes only its file
- **WHEN** profile `home` is deleted while `work` is active
- **THEN** `list_profiles` no longer contains `home`, `<profiles_dir>/settings.home.json` does not exist, `~/.claude/settings.json` still resolves to `settings.work.json`, and `active_profile_name` is still `Some("work")`

##### Scenario: Deleting the active profile removes the symlink
- **WHEN** profile `work` is deleted while it is active
- **THEN** `~/.claude/settings.json` no longer exists, `active_profile_name` is `None`, and no other profile is auto-activated

#### Requirement: Crash recovery follows the symlink chain

The system SHALL record, in the activation journal, the SHA-256 hash of the profile or orphan **file** the symlink is expected to point to. On recovery, the system SHALL resolve `~/.claude/settings.json` to its target file and compare that file's hash to the journal entry.

##### Scenario: Recovery completes when the symlink target matches
- **WHEN** the process is restarted after a partial activation and `~/.claude/settings.json` is a symlink whose target file's hash matches the journal
- **THEN** the journal is cleared and the active profile name (if any) is finalized

##### Scenario: Recovery marks mismatch as failed
- **WHEN** the symlink target file's hash does not match the journal, or the symlink is missing or broken
- **THEN** the journal is marked `failed` with a descriptive message, and the system returns a `Failed` recovery outcome

#### Requirement: Profiles directory is private and co-located with the database

The system SHALL create `<profiles_dir>` on demand with mode `0o700` on Unix. `<profiles_dir>` SHALL be derived from the metadata database path (the directory containing `cowboy.db`) without requiring an additional user-facing setting.

##### Scenario: Profiles directory is created with private mode
- **WHEN** any activation operation runs and `<profiles_dir>` does not exist
- **THEN** the directory is created with mode `0o700` on Unix and the operation continues
