---
create-time: 2026-11-27 14:36:50
---

## Context

The strategy seam is already in place: `AgentBackend: SessionRepository + ResumeLauncher` with an ordered registry driving tabs, lazy per-agent caches, and per-agent scoping (`agent-context-tabs`, implemented and synced). `ClaudeBackend` composes `ClaudeProjectsStore` + `ClaudeCliLauncher`; `main.rs` still hands concrete `ClaudeBackend` clones to the initial-load and delete workers — correct with one strategy, wrong with two.

Reconnaissance against real pi data on this machine (v3 files under `~/.pi/agent/sessions/`) established:

- Header: `{"type":"session","version":3,"id":"<uuid>","timestamp":"<ISO>","cwd":"E:\\..."}` — cwd is explicit; no directory-name decoding needed for grouping.
- Assistant usage: `message.usage = {input, output, cacheRead, cacheWrite, reasoning, totalTokens, cost:{input, output, cacheRead, cacheWrite, total, totalFromProvider}}`. Verified `totalTokens == input+output+cacheRead+cacheWrite` (reasoning excluded) — Cowboy's shared four-component sum matches pi's own total.
- **Rename spike resolved**: display names live in `{"type":"session_info","id":"<8hex>","parentId":"<8hex>","timestamp":"<ISO>","name":"..."}` records (found in real files; `pi --name` sets the same store). Observed files carry at most one `session_info`.
- `cost.total` is nonzero in real files with `totalFromProvider:false` (pi computes it from its own model catalog); no `true` instance exists on this machine. So "reported cost" means *pi-reported*, with the flag distinguishing provider origin — not "always provider-billed".
- Record-level `timestamp` fields are ISO RFC 3339 on every record; epoch-millis numbers appear only inside message objects (redundant with the record timestamp).
- Assistant messages carry a `model` string; roles seen: `user`, `assistant`, `toolResult` (plus a `system` message). `pi --help` confirms `--session <path|id>` opens by full/partial id and `--resume` is an interactive picker.

Constraints: AGENTS.md TDD (Red→Green→Refactor) and the three gates (`cargo fmt --check`, `cargo test`, `cargo clippy --all-targets --all-features -- -D warnings`). App test fakes are non-Send (`Rc<RefCell>`), so the app registry must stay `Vec<Box<dyn AgentBackend>>`.

## Goals / Non-Goals

**Goals:**
- `PiBackend` as the second registered strategy, reusing every existing tab/cache/scoping/summary behavior untouched.
- A normalization extension for reported cost that keeps Claude's estimation path bit-for-bit unchanged.
- Worker routing by agent id so mutations reload the right list with two strategies registered.

**Non-Goals:**
- Pi command-alias setting, Codex or further strategies, cross-agent aggregates, tab-semantics changes, pi sidecar cleanup (pi has no known sidecars).

## Decisions

### D1: Module layout mirrors the Claude store

New library module `src/pi/` (`mod.rs`, `parser.rs`, `store.rs`) exposing `PiProjectsStore { root: PathBuf }` with `from_home()` (`~/.pi/agent/sessions` via `dirs::home_dir`), `new(root)`, `discover_sessions`, `load_projects`, `rename_session`, `delete_session`, `delete_project`. `PiBackend` + `PiCliLauncher` live in `src/application.rs` beside their Claude counterparts, with the same explicit-trait delegation (`SessionRepository::load_projects(&self.repository)`) to avoid inherent-method shadowing. Both are `Clone` with `PathBuf`-only fields, hence `Send + Sync`.

Alternative considered: extending `infrastructure/` with a second store — rejected; that module's parser helpers are Claude-shaped (recursive key hunting), while pi v3 is a fixed record schema that deserves direct field access.

### D2: Reported cost as a new optional Session field

`domain::ReportedCost { total: f64, from_provider: bool }`; `Session.reported_cost: Option<ReportedCost>` (and the same field on `app::SessionInfo`, threaded through `build_session_info`). Pi sums `usage.cost.total` over cost-bearing assistant records; `from_provider` is true only if **every** cost-bearing record has `totalFromProvider: true` (conservative: mixed origin is pi-reported). No cost objects → `None`.

Display precedence: reported > estimated > tokens. Session row shows `$X.XX` for either cost kind (unchanged format). Session Info: reported → `Cost: $X.XXXX` + `(reported by provider)` / `(reported by pi)`; estimated → existing `Est. Cost` + `(estimate based on token usage)` untouched. Pi parser still computes `estimated_cost` via `pricing.rs` as the fallback when no cost objects exist (a pi session on a Claude model gets an estimate).

Alternative considered: reusing `estimated_cost` with an accuracy enum — rejected; it would change Claude-visible types and conflate two different provenances in one field. Cost of the new field: updating ~10 `Session` struct literals in test fixtures (mechanical).

### D3: Rename writes `session_info` (spike outcome)

Update the **last** `session_info` record's `name` in place; if none exists, append `{"type":"session_info","id":<generated 8-hex>,"parentId":<last record's id or null>,"timestamp":<now UTC ISO>,"name":<title>}`. Write via `.jsonl.tmp` + `fs::rename`, mirroring Claude's atomicity. Parser title precedence: last `session_info.name` → first user message text (first `text` content item, newlines collapsed to spaces, trimmed) → file stem.

Rationale: appending with `parentId` = last leaf mimics how pi itself chains records; updating in place avoids growing the file on repeated renames. Risk: pi's reader could reject a foreign `id`/`parentId` — mitigated by keeping the exact observed record shape; the probe/manual check against real pi is a verification step, not a blocker for the file contract.

### D4: Worker routing via `Arc<dyn AgentBackend + Send + Sync>`

- `main.rs` builds concrete `ClaudeBackend` and `PiBackend`, then a worker registry `Vec<Arc<dyn AgentBackend + Send + Sync>>` from clones; the app registry stays `Vec<Box<dyn AgentBackend>>` built from the same concrete values (cheap `PathBuf` clones; no blanket `AgentBackend for Arc` gymnastics needed for the app).
- Blanket `impl<R: SessionRepository + ?Sized> SessionRepository for Arc<R>` (4 delegating methods) in `application.rs` lets `Arc<dyn AgentBackend + Send + Sync>` satisfy the existing generic worker bound `R: SessionRepository + Send + 'static`.
- `DeleteTarget::agent_id()` accessor (`Project.agent_id` / `Session.key.agent_id`). `spawn_delete_worker` takes the worker registry + target, finds the backend by id inside the thread, and errors the completion if unregistered. `run_delete` takes `&dyn SessionRepository`. The UI is locked during deletion, so the target's agent is still the selected agent at completion — existing `complete_delete` scoping stays correct.
- Initial load: `main.rs` passes `worker_registry[0]` — the first registered agent, which is always the startup selection (selection is not persisted). `complete_initial_load` already scopes to `backends[0].id()`.

Alternative considered: a concrete `WorkerBackends { claude, pi }` struct — rejected; it re-hardcodes the strategy pair the registry exists to eliminate.

### D5: Pi parsing is direct-field, strict where Claude is strict

Require a session header. Skip a file when its header explicitly declares a non-v3 version; for v3 require nonempty `cwd` and `id` (never synthesize an id). Missing headers or required fields fail discovery with `InvalidSessionFile` identifying the path and reason. Use fixed v3 paths (`message.usage`, `message.model`, header `cwd`/`id`) instead of Claude's recursive key search. Parity kept where it matters: invalid JSON line → `StetsonError::InvalidSessionFile("{path}:{line}: ...")` aborting discovery; non-JSONL files and subdirectories ignored; blank lines skipped; timestamps through the shared `TimestampRange` (record-level ISO strings only — `collect_timestamps` already ignores numbers, so epoch-millis inner values are naturally excluded). Deliberate divergence: an assistant `usage` object with all-zero components counts as **known** usage (pi writes explicit zeros; Claude's all-zero→None heuristic would misreport them). Message count = `type:"message"` records (covers `user`, `assistant`, `toolResult`). `git_branch`: `None` (pi records none).

### D6: Launch contract

`PiCliLauncher` uses `Command::new("pi")` on every platform (no suffix probing or fallback), `current_dir(cwd)`, inherited env, no alias lookup. New: no args. Resume: `["--session", <full native id>]` — never `--resume`. Non-zero exit → `Err("pi exited with status ...")`, surfaced by the existing `resume_finished` paths with the `Pi` label. Launcher probe test reuses the `launcher_tests.rs` pattern: compile a fake `pi` executable, prepend to PATH, capture cwd/args/env marker.

### D7: Pi project deletion matches by header cwd

`delete_project(cwd)` completes discovery across all directories before unlinking anything. Collect all v3 files whose header cwd matches (the same Path equality as group_sessions_by_project; never canonicalize aliases); on a scan error leave all files untouched. Unlink only collected files, then prune only their immediate parent directories if empty, never recursively. Other sessions, unsupported versions, unrelated files and the sessions root survive. No encoded-name reconstruction — pi's `--E--foo-bar--` encoding is ambiguous for hyphenated names, and the header is authoritative. Not found → `ProjectNotFound`. `delete_session` removes the file only (no sidecars).

### D8: Registration

`main.rs`: `Stetson::new(vec![Box::new(claude), Box::new(pi)])`. `PiProjectsStore::from_home()` failure maps to the same startup-error style as the Claude root resolution. Everything downstream (tabs, lazy load, scoping, summaries, continuity) applies automatically.

## Risks / Trade-offs

- **Pi reader compatibility of appended `session_info`**: shape matches observed records, but pi could validate ids strictly. Mitigation: exact-shape append; manual smoke check with real pi after implementation. If pi rejects it, fallback is update-in-place only plus a clear error when no record exists — a spec revision, not a silent narrow.
- **`from_provider` semantics**: no `true` sample exists locally; the all-true rule is exercised by fixtures only. The wording (`reported by pi` vs `reported by provider`) keeps the display honest either way.
- **Zero-cost pi sessions** (gateway models with unknown pricing) display `$0.00` — that is what pi reports; suppressing it would hide pi's own claim. Accepted.
- **Synchronous first-visit load**: switching to Pi loads on the UI thread (existing lazy-load contract). Pi roots are small today; a worker-based lazy load is a future change if scans grow.
- **Fixture churn** from the new `Session` field is mechanical but touches several test modules; done in the first task so every later Red step compiles against the final shape.