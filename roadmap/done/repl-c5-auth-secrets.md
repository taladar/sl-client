---
id: repl-c5
title: Auth + secrets
topic: repl
status: done
origin: SL_REPL_ROAD_MAP.md — Phase C — shared library `sl-repl`
---

Context: [context/repl.md](../context/repl.md).

**C5. Auth + secrets.** `auth.rs`: TOML credentials (multi-avatar,
optional `mfa_command`, `mfa_window_guard_secs`), `Secret` redacting newtype,
and `acquire_mfa_token` with the **wall-clock-aligned 30s-window wait**
(`remaining = 30-(unix%30)`; if `< guard` sleep to next boundary, then run
command). Tests: window math + `Secret` redaction.
