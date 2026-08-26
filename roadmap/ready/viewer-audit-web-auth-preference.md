---
id: viewer-audit-web-auth-preference
title: Whether the grid session cookie is injected into the browser is a CLI flag, not a preference
topic: viewer
status: ready
origin: static code audit (2026-08-26)
points: 2
refs: [viewer-audit-env-overrides-preferences]
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-media/src/web_auth.rs` — the `--no-web-auth` CLI flag gates whether
the grid session cookie is injected into the shared browser context.

That is a user-facing **privacy** choice with no preferences row. Under the
project rule (GUI options belong in preferences; CLI is for non-GUI/startup
concerns) it belongs in the Preferences shell, alongside the other media
settings.

For the record the rest of this file is careful and should be left alone: the
token is never logged (`:118` logs `token.len()`), the cookie is host-scoped,
isolated media contexts are never touched, and the store is cleared both on exit
and before each injection.

Related and worth deciding together: `SL_VIEWER_NOTIFICATION_DEMO`
(`notification_host.rs:181`) and `SL_VIEWER_DUMP_MEDIA_FRAMES`
(`media_engine.rs:494`) are correctly env-only debug knobs and their docs say so
— the problem cases are catalogued in
[[viewer-audit-env-overrides-preferences]].
