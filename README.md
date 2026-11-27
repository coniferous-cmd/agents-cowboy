# cowboy

`cowboy` is a Rust TUI for browsing and managing Claude Code and Pi sessions in separate agent tabs.

The two-column browser fills the terminal from the top row: Projects on the left and Sessions on the right, with status and keyboard hints at the bottom. The focused pane's border is highlighted.

## Features

- Browse Claude projects from `~/.claude/projects/` and Pi v3 sessions from `~/.pi/agent/sessions/`
- Start Pi with `pi` or resume with `pi --session <session-id>` in the project's working directory
- Resume an existing session with the configured launcher, defaulting to `claude --resume <session-id>`
- Start a new Claude session in the selected project directory with the configured launcher
- Rename sessions
- Delete a single session or an entire project group without blocking the TUI
- Search sessions within the selected project
- Inspect session metadata in a modal

## Requirements

- Rust toolchain
- `claude` available on your `PATH`, or a configured launcher alias available on your `PATH`
- For Pi launches, `pi` on your `PATH`. A missing Pi sessions directory simply leaves the Pi tab empty.

## Build and Run

```bash
cargo build
cargo run
```

Run tests:

```bash
cargo test
```

## Publishing

This repository includes an npm package wrapper that can ship prebuilt binaries
for:

- macOS x64 and arm64
- Linux x64 and arm64
- Windows x64 and arm64

The npm entrypoint selects the matching binary from `dist/<platform>-<arch>/`
at runtime.

For local smoke testing, build and pack the binary for the current platform:

```bash
npm pack --dry-run
```

### Automatic Release (Recommended)

Configure the repository secret `NPM_TOKEN`, then:

1. Update `version` in both `Cargo.toml` and `package.json` to the same new
   version.
2. Commit the changes and push to `main` (or merge a PR into `main`).
3. The `Auto Tag Release` workflow validates that both versions match, creates
   a `v<version>` tag, and pushes it to the remote.
4. The pushed tag triggers the `Release` workflow (GitHub Release with
   prebuilt binaries) and the `npm release` workflow (npm publish).

   GitHub Release attachments are named `cowboy-linux-amd64`,
   `cowboy-linux-arm64`, `cowboy-macos-amd64`, `cowboy-macos-arm64`, and
   `cowboy-windows-amd64.exe`. The npm package and CLI remain `cowboy`.

If the same version has already been published, the workflow skips tag
creation and no duplicate release occurs. To publish a new version, increment
the version in both files and push again.

### Manual Release (Fallback)

If you need to publish a release without pushing to `main` (for example, to
re-publish an existing tag), create and push a version tag directly:

```bash
git tag v0.1.0
git push origin v0.1.0
```

The same `Release` and `npm release` workflows are triggered regardless of
whether the tag was created automatically or manually.

## Keyboard Shortcuts

| Key | Action |
|---|---|
| `[` / `]` / `Tab` / `←` / `→` | Toggle focus between Projects and Sessions |
| `↑` / `↓` | Move selection |
| `Enter` in Projects | Start a new session in the selected project directory, or in the current working directory when `Open Here` is selected |
| `Enter` in Sessions | Resume selected session |
| `n` | Start a new session in the current project directory |
| `i` | Show session info |
| `r` | Rename session |
| `Ctrl+D` | Delete project or session depending on focus; press `Ctrl+D` again in the confirmation dialog to confirm |
| `/` | Search sessions |
| `a` | Cycle the agent tab (one tab per supported agent backend; exactly one is always selected, no `All`) |
| `q` / `Esc` | Quit |

After confirming a deletion, cowboy keeps rendering while the filesystem
work runs in the background. Until it completes, all application shortcuts are
disabled, including `q`, `Esc`, and `Ctrl-C`; the status bar asks the user to
wait.
Successful deletion refreshes the project list. If deletion fails, the current
list remains visible and the status bar reports the error.

## CLI Commands

```bash
cowboy                    # Launch the TUI
cowboy alias <command>           # Set Claude launcher alias
cowboy install                   # Download & install latest Claude Code
cowboy --help                    # Show help
```

Examples:

```bash
cowboy alias my-claude
cowboy install
```

`install` downloads the latest Claude Code CLI binary from the
[coniferous-cmd/claude-shop](https://github.com/coniferous-cmd/claude-shop) GitHub
releases, verifies its SHA-256 checksum, and installs it under the
`cowboy` application data directory in a version-specific subdirectory.
On macOS the installed path is:

```
~/Library/Application Support/cowboy/versions/<version>/claude
```

On Linux it is `~/.config/cowboy/versions/<version>/claude`, and on
Windows it is the corresponding app data directory with `claude.exe`.

Multiple versions can coexist — installing a new version does not remove older
ones. Reinstalling the same version atomically replaces the existing binary.
After installation, you are asked whether to update the `claude_command_alias`
setting to point to the installed binary. Answer `y` to use it for TUI resume
and new-session operations, or `n` to keep the current alias.

After configuration, session resume should launch `my-claude --resume <session-id>` and new sessions should launch `my-claude`. Without an alias, the launcher remains `claude`.

## Data Model

`cowboy` uses two storage locations:

1. `~/.claude/projects/`
   Session and project source of truth. Session files are read and mutated directly from the filesystem.
2. `~/.config/cowboy/cowboy.db`
   SQLite metadata used for configurable paths, launcher settings, and themes. On macOS this is under `~/Library/Application Support/cowboy/`.

Project and session discovery does not come from SQLite.

## Claude Settings and Environment

New and resumed Claude processes inherit the current environment and load their own settings. Cowboy does not pass `--settings` or modify Claude's global `settings.json`.

### Upgrading from a release with Profiles

`cowboy config` is retired, and project Profile bindings no longer affect launches. On first startup, metadata schema v5 permanently removes `claude_profiles`, `project_profile_bindings`, `profile_activation_journal`, and `claude_settings_snapshots` from SQLite, along with their indexes and triggers. It also removes `active_profile_name` from `settings`. Other settings (including paths and the launcher alias) and themes are retained. For v0/v1 databases, Cowboy still exports legacy project settings before deleting the old environment tables.

**Back up `cowboy.db` before first launch.** Close all Cowboy instances and use SQLite's backup command to create a consistent copy, for example `sqlite3 "/path/to/cowboy.db" ".backup '/path/to/cowboy-before-v5.db'"`. Keep the copy separate from the live database. Downgrading alone will not restore deleted records, and older Cowboy binaries may reject v5. To roll back, close Cowboy, move the upgraded database and any `-wal`, `-shm`, or `-journal` sidecars aside, restore the backup at the original database path, and run a compatible older binary. Changes made after the backup will not be in the restored database.

The SQLite cleanup does not touch legacy Profile files, Claude's `settings.json` (regular file or symlink), the link target, or any `settings.json.cowboy-backup`.

If `settings.json` is a symlink to a legacy Profile file, **do not delete the target** while Claude still uses it. To keep the settings currently in effect and remove that dependency:

1. Close Claude and copy the contents of the linked file to a new, private regular file in the same directory as `settings.json`. Verify that the copy has the expected settings and restrict its permissions to your account.
2. Replace the `settings.json` symlink itself with that regular file. Do not copy over the existing symlink in place: that can write through to the target.
3. Verify that Claude works with the new `settings.json` before removing any old Profile files. The old `settings.json.cowboy-backup` contains earlier settings; restoring it is a rollback, not a way to keep the currently effective settings.

- On Windows, Claude project directories under `~/.claude/projects/` are resolved from session `cwd` data when the directory name is lossy or normalized differently from the workspace path.

## UI Notes

- Project rows show aggregate token usage across all sessions: `name (count) usage`
- Aggregate total includes input, output, cache creation, and cache read tokens
- Partial totals show `+` (e.g. `12.3K+`); wholly unknown totals show `Unknown`; empty projects show `0`
- The `Open Here` row shows no token usage
- Session token usage is shown inline with session metadata using compact `K` and `M` units when cost is unavailable
- Session Info modal shows the selected session's four token categories and total using the same four-component definition
- LLM token prices are maintained in `data/llm_pricing.json` as USD per million tokens, grouped by provider and model prefix
- Default documented theme direction is `Dracula`
- Theme and layout rules for AI-assisted edits live under `.claude/rules/`

## Project Docs

- [docs/01-principles.md](docs/01-principles.md)
- [docs/02-architecture.md](docs/02-architecture.md)
- [docs/03-ui.md](docs/03-ui.md)
- [docs/05-decisions.md](docs/05-decisions.md)
- [docs/06-interfaces.md](docs/06-interfaces.md)

## Development Notes

- The UI is keyboard-first and intentionally minimal.
- Each backend's local transcripts remain the source of truth for its sessions.
- SQLite is metadata only.
