# pi-agent-backend Specification

## Purpose
TBD — created by archiving change pi-agent-backend. Update Purpose after archive.

## Requirements

### Requirement: Pi is a registered agent strategy with its own discovery

Cowboy SHALL register a Pi backend strategy (id `pi`, label `Pi`) after the Claude strategy. Pi session discovery SHALL scan `~/.pi/agent/sessions/<project-dir>/*.jsonl`. Each session's working directory SHALL come from the session header record (`type: "session"`, version 3, `cwd` field); the encoded project-directory name SHALL NOT be decoded for grouping. A missing sessions root SHALL yield an empty project list, not an error.

#### Scenario: Pi tab appears
- **WHEN** the TUI renders with the Pi strategy registered
- **THEN** the top strip shows `Claude` and `Pi` in registration order

#### Scenario: Projects group by header cwd
- **WHEN** two pi session files in different encoded directories carry the same header `cwd`
- **THEN** both sessions appear under one project with that cwd

#### Scenario: No pi sessions on this machine
- **WHEN** `~/.pi/agent/sessions` does not exist
- **THEN** the Pi tab loads an empty project list (plus `Open Here`) without an error toast

### Requirement: Pi transcript parsing follows the v3 record format

The Pi parser SHALL read v3 JSONL records. The session id SHALL come from the header `id`, never the file stem. A file with a session header explicitly declaring another version SHALL be skipped. A missing header or a v3 header without a nonempty `cwd` or `id` SHALL fail discovery with an invalid-session-file error identifying the file and reason.

The title SHALL resolve in order: the latest `session_info` record's `name`, then the first user message's text content (newlines collapsed to spaces), then the file stem. Created/updated timestamps SHALL normalize through the shared timestamp path using record-level RFC 3339 `timestamp` fields; numeric epoch-millis timestamps inside message objects SHALL be ignored. Message count SHALL count `type: "message"` records. The model SHALL come from the first assistant message's `model` field.

#### Scenario: Unsupported version alongside v3
- **WHEN** the root contains a session of another declared version and a valid v3 session
- **THEN** only the v3 session appears and discovery succeeds

#### Scenario: Header metadata is missing
- **WHEN** a file lacks a session header or its v3 header lacks `cwd` or `id`
- **THEN** discovery fails with an error identifying the file and missing metadata

#### Scenario: Named session shows its name
- **WHEN** a pi session file contains a `session_info` record with `name: "My task"`
- **THEN** the session row title is `My task`

#### Scenario: Latest name wins
- **WHEN** a file contains two `session_info` records with different names
- **THEN** the later record's name is the title

#### Scenario: Unnamed session falls back to first user message
- **WHEN** a pi session has no `session_info` record and its first user message text is `Fix the build`
- **THEN** the title is `Fix the build`

#### Scenario: No user text falls back to the file stem
- **WHEN** a pi session file has a header but no `session_info` and no user message text
- **THEN** the title is the file stem

#### Scenario: Timestamps normalize to UTC
- **WHEN** records carry ISO timestamps with offsets and inner epoch-millis message timestamps
- **THEN** created/updated are the earliest/latest record-level instants as UTC RFC 3339, unaffected by the epoch values

### Requirement: Pi usage extraction maps into the shared four-component total

The Pi parser SHALL sum assistant-message `usage` objects into the shared `SessionUsage`: `input` → input, `output` → output, `cacheWrite` → cache creation, `cacheRead` → cache read. The `reasoning` field SHALL NOT be counted. An assistant `usage` object counts as known usage even when all four components are zero. Summation, compact formatting, and per-agent token summaries SHALL reuse the existing shared domain logic unchanged.

#### Scenario: Usage sums across assistant records
- **WHEN** two assistant records carry usage `{input: 100, output: 10, cacheRead: 5, cacheWrite: 2}` and `{input: 200, output: 20, cacheRead: 0, cacheWrite: 3}`
- **THEN** the session usage is input 300, output 30, cache read 5, cache creation 5, total 340

#### Scenario: Reasoning tokens are excluded
- **WHEN** an assistant usage object has `reasoning: 500` alongside the four components
- **THEN** the total equals the four-component sum without the reasoning value

#### Scenario: Zero usage is still known
- **WHEN** an assistant record has a usage object with all four components zero
- **THEN** the session shows a known total of 0 rather than unknown usage

#### Scenario: No assistant usage means unknown
- **WHEN** a pi session has no assistant usage objects
- **THEN** usage is unknown and the row shows no token summary

### Requirement: Reported cost takes precedence over token-based estimation

The session normalization contract SHALL allow a strategy to supply an optional reported cost (total plus a provider-origin flag). The Pi parser SHALL sum assistant `usage.cost.total` into the reported cost; the provider-origin flag SHALL be true only when every cost-bearing record has `totalFromProvider: true`. When a reported cost exists, the session row SHALL show its total and Session Info SHALL show `Cost: $X` with `(reported by provider)` when the flag is true and `(reported by pi)` otherwise. When no reported cost exists, the existing estimation path applies unchanged: `pricing.rs` estimate shown as `Est. Cost` with the estimate note, else token summary only. Claude sessions SHALL keep their current estimated-cost behavior.

#### Scenario: Pi session with reported cost
- **WHEN** a pi session's cost records sum to 0.013308 with `totalFromProvider: false`
- **THEN** Session Info shows `Cost: $0.0133` and `(reported by pi)`, not `Est. Cost`

#### Scenario: Provider-origin cost is labeled distinctly
- **WHEN** every cost-bearing record has `totalFromProvider: true`
- **THEN** Session Info shows `(reported by provider)`

#### Scenario: Mixed origin is not provider-labeled
- **WHEN** one cost-bearing record has `totalFromProvider: true` and another has false
- **THEN** Session Info shows `(reported by pi)`

#### Scenario: Claude sessions stay estimated
- **WHEN** a Claude session has usage and a known model
- **THEN** Session Info still shows `Est. Cost` with `(estimate based on token usage)`

#### Scenario: Pi session without cost falls back
- **WHEN** a pi session has usage but no cost objects and its model matches no pricing entry
- **THEN** the row shows the token summary and Session Info shows no cost line

### Requirement: Pi launch and resume use the pi CLI contract

Under the Pi tab, starting a new session (Enter in Projects, `n` in Sessions, Enter on `Open Here`) SHALL run bare `pi` resolved from PATH in the intended working directory. Resuming SHALL run `pi --session <full-session-id>` in the session's cwd. Cowboy SHALL NOT use `--resume` (an interactive picker). Pi processes SHALL inherit Cowboy's environment unchanged. No pi command-alias setting exists in this change. Launch failures and non-zero exits SHALL surface through the existing status/toast paths with the Pi label.

#### Scenario: New pi session from a project
- **WHEN** the user presses Enter on a project under the Pi tab
- **THEN** Cowboy runs `pi` with no arguments in that project directory

#### Scenario: Launch command is platform-independent
- **WHEN** Cowboy starts or resumes a Pi session on any platform
- **THEN** the program name passed to the process launcher is exactly `pi`, without appending or retrying with a file extension

#### Scenario: Resume passes the full id
- **WHEN** the user presses Enter on a pi session with id `01a1cd8c-6a76-7754-8de8-687520f7649f`
- **THEN** Cowboy runs `pi --session 01a1cd8c-6a76-7754-8de8-687520f7649f` in the session's cwd

#### Scenario: Open Here under the Pi tab
- **WHEN** the user presses Enter on `Open Here` under the Pi tab
- **THEN** Cowboy runs bare `pi` in Cowboy's current working directory

### Requirement: Pi rename and delete adapt to pi's transcript format

Renaming a pi session SHALL set the display name in the transcript: update the latest `session_info` record's `name` in place, or append a new `session_info` record (with `id`, `parentId` referencing the last record, `timestamp`, and `name`) when none exists, written atomically via temp-file rename. Empty titles SHALL be rejected by the shared validation. Deleting a pi session SHALL remove its file. Project deletion SHALL complete discovery before modifying any files, remove only v3 session files matching the target header cwd across all directories, and remove only their parent directories that become empty. Missing sessions/projects SHALL produce the existing not-found error behavior.

#### Scenario: Rename updates an existing name record
- **WHEN** the user renames a pi session that already has a `session_info` record
- **THEN** that record's `name` is replaced and the reloaded list shows the new title

#### Scenario: Rename appends a name record
- **WHEN** the user renames a pi session with no `session_info` record
- **THEN** a `session_info` record with the new name is appended and the reloaded list shows it

#### Scenario: Delete removes the session file
- **WHEN** the user confirms deletion of a pi session
- **THEN** its JSONL file is removed and only the Pi list reloads

#### Scenario: Project spans directories with mixed contents
- **WHEN** matching sessions occur in two directories, one also containing another project's session and unrelated files
- **THEN** all matching sessions are removed, the other contents survive, and only the emptied directory is removed

#### Scenario: Distinct project paths resolve to the same directory
- **WHEN** two header cwds compare unequal under the list's Path equality but resolve to the same directory
- **THEN** deleting one listed project preserves the other project's sessions; deletion does not canonicalize the paths

#### Scenario: Pre-deletion scan fails
- **WHEN** an invalid file prevents discovery from completing
- **THEN** deletion reports the error and leaves all files untouched

#### Scenario: Missing session reports not found
- **WHEN** a rename or delete targets a pi session whose file no longer exists
- **THEN** the operation fails with a not-found error surfaced as a toast

### Requirement: Mutation workers route by the target agent

The deletion worker SHALL select the backend whose id matches the delete target's agent id, perform the deletion through it, and reload the project list through the same backend. The initial-load worker SHALL load the first registered agent's backend (the startup selection). Completion handling SHALL keep the existing selected-agent cache scoping.

#### Scenario: Delete under the Pi tab reloads pi only
- **WHEN** a pi session deletion completes while the Pi tab is selected
- **THEN** the refreshed list comes from the Pi backend and only the Pi cache slot is replaced

#### Scenario: Initial load stays on the first agent
- **WHEN** Cowboy starts with Claude registered first
- **THEN** the initial-load worker loads Claude's projects regardless of Pi's registration

### Requirement: Malformed pi files behave like malformed Claude files

A pi JSONL file containing an invalid JSON line SHALL fail discovery with an invalid-session-file error naming the file and line, consistent with the Claude store. Non-JSONL files and subdirectories SHALL be ignored during discovery.

#### Scenario: Invalid JSON line surfaces an error
- **WHEN** a pi session file contains a non-JSON line
- **THEN** loading the Pi list fails with an invalid-session-file error naming that file

#### Scenario: Non-session files are ignored
- **WHEN** a pi project directory contains non-JSONL files or subdirectories
- **THEN** they are skipped without error