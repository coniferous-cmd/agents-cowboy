---
create-time: 2026-11-27 14:37:27
---

## 1. Domain normalization extension

- [x] 1.1 TDD: add `domain::ReportedCost { total: f64, from_provider: bool }` and `Session.reported_cost: Option<ReportedCost>`; thread the field through `app::SessionInfo` and `build_session_info`. Red: domain test for the new type plus a `build_session_info` pass-through test; update every `Session` struct literal fixture so the suite compiles and stays green.

## 2. Pi v3 parser (`src/pi/parser.rs`)

- [x] 2.1 TDD: parse the v3 header and record stream — skip explicitly unsupported versions; reject missing headers and missing/empty v3 `id` or `cwd` with file and reason (test each case; no file-stem id fallback). Use the header id and cwd, created/updated via the shared `TimestampRange` over record-level ISO timestamps (epoch-millis inner values ignored), message count = `type:"message"` records, model from the first assistant `message.model`. Red: temp-dir fixture tests for each field including offset normalization and epoch exclusion.
- [x] 2.2 TDD: title precedence — latest `session_info.name` wins; else first user message text (first `text` item, newlines collapsed, trimmed); else file stem. Red: tests for all three tiers plus latest-of-two name records.
- [x] 2.3 TDD: usage extraction — sum assistant `message.usage` `{input, output, cacheRead→cache_read, cacheWrite→cache_creation}`; exclude `reasoning`; an all-zero usage object is known usage; no usage objects → `None`. Red: multi-record sum, reasoning-exclusion, zero-known, and unknown tests.
- [x] 2.4 TDD: reported cost — sum `usage.cost.total` over cost-bearing records; `from_provider` true only when every cost-bearing record has `totalFromProvider: true`; no cost objects → `None`; `estimated_cost` still computed via `pricing.rs` from usage+model as the fallback. Red: sum, all-true, mixed-flag, absent-cost, and known-model-estimate tests.

## 3. Pi store (`src/pi/store.rs`)

- [x] 3.1 TDD: discovery and grouping — scan `<root>/<dir>/*.jsonl`, group by header cwd via `group_sessions_by_project`, missing root → empty list, non-JSONL files and subdirectories ignored, blank lines skipped, invalid JSON line → `InvalidSessionFile("{path}:{line}")` aborting the scan. Red: fixture tests for each behavior.
- [x] 3.2 TDD: rename — update the last `session_info.name` in place, else append a `session_info` record with generated 8-hex `id`, `parentId` = last record id (null when empty), current UTC ISO `timestamp`, and `name`; atomic `.jsonl.tmp` + rename; empty title rejected; missing file errors. Red: tests asserting file contents (old name gone, appended shape) and re-parsed title.
- [x] 3.3 TDD: delete — `delete_session` removes the file (missing → `SessionNotFound`); `delete_project` finishes scanning before mutation, unlinks all and only matching v3 sessions by header cwd, then prunes emptied parents (no match → `ProjectNotFound`). Red: cover a hyphenated cwd spanning directories, a path alias to the same directory that groups separately and must survive, a shared directory with another project, unsupported sessions and unrelated files, an emptied directory, and a scan error leaving every file intact.

## 4. Pi backend strategy (`src/application.rs`)

- [x] 4.1 TDD: `PiBackend` (composing `PiProjectsStore` + `PiCliLauncher`, `Clone`, explicit-trait delegation) with id `pi` / label `Pi`; `PiCliLauncher` runs bare `pi` from PATH for new and `pi --session <full-id>` for resume, inheriting env. Red: id/label + delegation tests on temp fixtures, plus a launcher probe test (fake `pi` executable on PATH capturing cwd, args, env marker) mirroring `launcher_tests.rs`. Assert the requested program name is exactly `pi` for both launches.

## 5. Cost display precedence

- [x] 5.1 TDD: session row shows reported cost total when present (same `$X.XX` format); Session Info shows `Cost: $X.XXXX` with `(reported by provider)` when `from_provider` else `(reported by pi)`, falling back to the unchanged `Est. Cost` + estimate note, else no cost line. Red: UI render tests for reported/provider/pi-labeled/estimated-only cases.

## 6. Worker routing and registration

- [x] 6.1 TDD: `DeleteTarget::agent_id()` accessor; blanket `impl<R: SessionRepository + ?Sized> SessionRepository for Arc<R>`; `spawn_delete_worker` takes `Vec<Arc<dyn AgentBackend + Send + Sync>>` and routes by target agent id (unregistered id → failure completion); `run_delete` takes `&dyn SessionRepository`. Red: worker-routing test with two fake Send backends proving the pi target hits the pi backend and the reload comes from it.
- [x] 6.2 Register `PiBackend` after `ClaudeBackend` in `main.rs` (`PiProjectsStore::from_home()`), build the worker registry from clones, pass `worker_registry[0]` to the initial-load worker; verify startup tests still pass and the app registry drives `[Claude] [Pi]`.

## 7. Docs and verification

- [x] 7.1 Update `docs/03-ui.md` (Pi tab, cost display + accuracy wording), `docs/05-decisions.md` (reported-cost precedence, pi rename via `session_info`, worker routing), and README/roadmap notes where they claim Claude-only.
- [x] 7.2 Run all gates: `cargo fmt --check`, `cargo test`, `cargo clippy --all-targets --all-features -- -D warnings`; `speckit validate pi-agent-backend --type change`; fix any fallout and report results honestly.