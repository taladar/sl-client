---
id: repl-e3
title: Live aditi run
topic: repl
status: done
origin: SL_REPL_ROAD_MAP.md — Phase E — docs & live verification
---

Context: [context/repl.md](../context/repl.md).

**E3. Live aditi run.** TOML `mfa_command`, window-aligned wait; capture
every `DecodeFailed`/`UnhandledMessage`/`ExpectedReplyMissing`/`Disconnected`
as the follow-up fix list for `sl-proto`/the runtimes (append those as new
roadmap items).

**Done** (closed retroactively during the 2026-08-13 protocol/repl audit —
every piece landed earlier without this file moving):

- TOML `mfa_command` plus the TOTP window-aligned wait are implemented in
  `sl-repl/src/auth.rs` (`acquire_mfa_token`, `mfa_window_guard_secs`).
- The live aditi run happened 2026-06-25: an `sl-repl-tokio`
  login/hold/logout smoke test against the SL Beta grid, documented in
  [context/aditi-issues.md](../context/aditi-issues.md).
- Its diagnostics were harvested as follow-up roadmap items `aditi-1`,
  `aditi-2` and `aditi-3` — all done since.
- Ongoing diagnostic capture on aditi is superseded by the sl-conformance
  harness (built on sl-repl), which collects every `Diagnostic` per
  session (`sl-conformance/src/context.rs`) across the committed
  `records/aditi/` cases.
