---
id: viewer-audit-notification-store-overwrite
title: A malformed notification store is overwritten, destroying unanswered notices
topic: viewer
status: bugs
origin: static code audit (2026-08-26)
points: 2
refs: [viewer-audit-settings-write-race]
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-notices/src/notification_persist.rs:236` — `read_store` warns and
returns `Vec::new()` on a parse failure. `load_persisted` then sets
`store.path`, and the first later `flush_persistent_notifications` rewrites the
file from the now-empty map. A parse error is silently downgraded into **data
loss** of every unanswered notification.

Same file, `:265` — the store is written with a whole-file `fs_err::write` on
the **main thread inside `Update`** every time `dirty` flips. No `save_async`
equivalent, and the same no-read-modify-write clobber hazard as
[[viewer-audit-settings-write-race]].

Fix: on a parse failure, refuse to write rather than overwriting (rename the bad
file aside), and move the write off the frame thread with the atomic
temp-and-rename helper. The 2 existing tests cover record/forget and a JSON
round-trip; the reload path — a `PersistedKind::Catalogue` naming a template
that no longer exists, and whether surviving entries are still written back — is
the untested case.
