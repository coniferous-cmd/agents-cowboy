# cowboy

`cowboy` is a Rust TUI for browsing and managing Claude Code sessions stored under `~/.claude/projects/`.

It presents three top-level tabs:

- Projects and Sessions share the existing two-column session browser
- Profiles lists named global settings
- Bottom: status and keyboard hints

## Features

- Browse projects and sessions from `~/.claude/projects/`
- Resume an existing session with the configured launcher, defaulting to `claude --resume <session-id>`
- Start a new Claude session in the selected project directory with the configured launcher
- Rename sessions
- Delete a single session or an entire project group without blocking the TUI
- Search sessions within the selected project
- Inspect session metadata in a modal
- Create and edit named Claude settings Profiles from the CLI
- Atomically activate Profiles, with a one-shot backup of the prior `settings.json` written to `settings.json.cowboy-backup` on first launch

## Requirements

- Rust toolchain
- `claude` available on your `PATH`, or a configured launcher alias available on your `PATH`
- Existing Claude Code data under `~/.claude/`

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
| `[` / `]` | Cycle Projects, Sessions, and Profiles tabs |
| `Tab` / `←` / `→` | Toggle focus between Projects and Sessions |
| `↑` / `↓` | Move selection |
| `Enter` in Projects | Start a new session in the selected project directory, or in the current working directory when `Open Here` is selected |
| `Enter` in Sessions | Resume selected session |
| `n` | Start a new session in the current project directory |
| `i` | Show session info |
| `r` | Rename session |
| `Ctrl+D` | Delete project or session depending on focus; press `Ctrl+D` again in the confirmation dialog to confirm |
| `/` | Search sessions |
| `Enter` in Profiles | Activate the focused Profile |
| `Ctrl+D` in Profiles | Delete the focused Profile; press `Ctrl+D` again to confirm |
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
cowboy config list               # List settings Profiles
cowboy config create <name>      # Create an empty Profile
cowboy config edit <name>        # Edit a Profile with $EDITOR
cowboy config activate <name>    # Activate a Profile
cowboy config sync [name]        # Reconcile profile files on disk into the DB
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
   SQLite metadata used for settings Profiles, launcher settings, and themes. On macOS this is under `~/Library/Application Support/cowboy/`.

Project and session discovery does not come from SQLite.

## Profiles and Environment

Profiles store complete Claude Code settings JSON objects and atomically replace the configured global `settings.json` when activated. On the first launch that observes an existing `settings.json`, cowboy copies it to `settings.json.cowboy-backup` next to it so users always have a one-shot rollback point.

Each profile is mirrored as `~/.config/cowboy/profiles/settings.<name>.json`. If you edit one of those files directly (for example with `vim`), the metadata in cowboy's SQLite database will be stale until you run `cowboy config sync [name]` to reconcile the file back into the database. With no argument, `cowboy config sync` reconciles every profile file on disk.

New and resumed Claude processes inherit cowboy's current process environment unchanged; project/session environment overrides are no longer stored or injected.

- On Windows, Claude project directories under `~/.claude/projects/` are resolved from session `cwd` data when the directory name is lossy or normalized differently from the workspace path.

## UI Notes

- Projects and Sessions retain the two-column browser; Profiles is a separate tab
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
- `~/.claude` remains the source of truth for session data.
- SQLite is metadata only.
