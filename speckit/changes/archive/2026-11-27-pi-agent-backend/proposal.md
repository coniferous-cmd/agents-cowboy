---
create-time: 2026-11-27 14:26:52
---

## Why

The agent tab strip currently shows only `[Claude]` because Claude is the only registered backend strategy. Pi is the agreed second agent: registering a `PiBackend` makes the strip real multi-agent (`[Claude] [Pi]`) and lets users browse, resume, and start pi sessions per project without leaving Cowboy. The extension point is already in place — one struct implementing `AgentBackend` plus one registration line — and the on-disk format has been verified against real pi data on the development machine, so this change starts from evidence rather than guesswork.

## What Changes

- Implement `PiBackend` as the second registered agent strategy (id `pi`, label `Pi`), covering the full `AgentBackend` contract:
  - **Discovery**: scan `~/.pi/agent/sessions/<encoded-cwd>/<timestamp>_<uuid>.jsonl`. The session header record (`type: "session"`, version 3) carries `cwd` directly, so pi needs no directory-name decoding; directory names are not authoritative for grouping or deletion. Skip sessions with an explicitly unsupported version; missing headers or missing v3 cwd/id are errors.
  - **Parsing and usage extraction**: read v3 JSONL records; extract usage from assistant messages (`input`, `output`, `cacheRead`, `cacheWrite`) into the shared `SessionUsage`. Verified: pi's own `totalTokens` equals Cowboy's four-component sum, so shared aggregation stays unchanged. The `reasoning` field is not part of the four-component total. Titles fall back to the first user message text when no `session_info.name` exists; timestamps normalize through the shared path (pi mixes ISO record timestamps with epoch-millis message timestamps).
  - **Launch**: new session runs `pi` in the project directory; resume runs `pi --session <id>` (`--resume` is an interactive picker and must not be used). Pi processes inherit Cowboy's environment unchanged, mirroring the Claude delegation contract. No pi command-alias setting in this change; the launcher always invokes `pi` without a suffix and leaves PATH lookup to the platform.
  - **Rename/delete**: adapt to pi's transcript format. Rename updates or appends `session_info`; session delete removes the session file. Project deletion first scans the full root, then removes only matching v3 session files across directories and prunes only emptied parent directories.
- Provider-reported cost: pi files carry a real `cost.total` (with a `totalFromProvider` flag). Extend the session normalization contract so a strategy may supply an optional provider-reported cost that takes precedence over `pricing.rs` estimation; Session Info's accuracy status distinguishes a provider-reported value from a token-based estimate. Claude behavior is unchanged (still estimated).
- Route the deletion and initial-load workers in `main.rs` by the selected agent instead of the hardcoded Claude backend, so mutations reload the active agent's list once two strategies are registered.
- Register `PiBackend` after `ClaudeBackend`; the tab strip, lazy loading, per-agent filtering, token summaries, and switch continuity all apply to pi automatically through the existing registry contract.

Out of scope: a pi command-alias setting, Codex or other strategies, cross-agent aggregate views, and any change to the tab-strip semantics themselves.

## Capabilities

### New Capabilities

- `pi-agent-backend`: Pi session discovery, parsing, usage extraction, launch/resume, rename/delete, and provider-reported cost — the observable behavior of the Pi tab as a working context.

### Modified Capabilities

- `agent-context-tabs`: the registry requirement's change-scoped sentence "This change registers only the Claude strategy" is removed — the strip now shows Claude and Pi. The registry contract itself (a tab exists iff its strategy is registered) is unchanged. Depends on `agent-context-tabs` being synced to main specs first so this capability path exists.

## Impact

- New pi infrastructure module (discovery, v3 parser, usage/cost extraction) alongside the Claude store; `src/application.rs` registration; `src/main.rs` worker routing by selected agent.
- Domain `Session` gains an optional provider-reported cost field (or equivalent normalization extension); Session Info rendering and accuracy wording; `pricing.rs` stays the fallback for Claude.
- Dependency: `agent-context-tabs` must be archived/synced first — its registry, lazy-loading, and scoping contracts are the foundation this change plugs into.
- Docs: `docs/03-ui.md` (cost display and accuracy), `docs/05-decisions.md` (provider cost precedence), roadmap; regression tests with temp-dir pi fixtures per AGENTS.md TDD requirements, including a malformed-file isolation check consistent with the Claude store's behavior.
