---
id: viewer-audit-notifications-crate-split
title: Split the 21637-line notification catalogue and make its lookup a map
topic: viewer
status: ready
origin: static code audit (2026-08-26)
points: 8
refs: [viewer-audit-preferences-restart-note]
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-notifications/src/lib.rs` is **21637 lines in one file**, and about
86% of it is a single const array: `NOTIFICATIONS: &[NotificationTemplate]`
spans `:1876-20429` and holds **1310** entries. Lines 142-1875 are 96
`pub const ...: &[NotificationButton]` form tables. Only ~1900 lines are logic
and types.

It is **hand-written, not generated** — no `build.rs`, no `include!`, no
`OUT_DIR` — with per-entry prose comments and 30 hand-curated family section
headers, each naming an existing `viewer-notification-catalogue-*` roadmap task.

Duplication is low and the invariants are well tested: all 96 form tables are
structurally distinct, 17 tests pin uniqueness, default-button count,
ignore-kind to ignore-key agreement and a reference-drift count, and
`sl-client-bevy-viewer/tests/notification_ftl_coverage.rs:46` verifies every key
resolves in `en/main.ftl` (independently re-checked: zero missing).

**Verdict: split by file, keep it in Rust.** The compile-time typing is what
makes those 17 invariant tests possible, so a RON/TOML data file would be a
downgrade. The seams are already drawn by the `// ---- <family> ----` markers:

- `lib.rs` — types, `NotificationManager`, `substitute`, tests (~1.9k);
- `forms.rs` — the 96 button tables (`:142-1875`, ~1.7k);
- `catalogue/<family>.rs` x 30, re-joined by a `NOTIFICATIONS` built from
  per-family consts.

The three that most need it: `objects_edit` (`:8034-10859`, **2826** lines),
`estate_region` (`:4371-5964`, 1594), `preferences` (`:11320-12835`, 1516).

Separately, `:20430` — `template()` is
`NOTIFICATIONS.iter().find(|c| c.name == name)`, an **O(1310) linear scan on
every raise and every `NotificationResponse` route**. A sorted array plus
`binary_search_by_key` (a test already sorts the names at `:20790`) or a `phf`
map is a two-line change.

Cosmetic while there: `:20850-20856` fuses two unrelated doc paragraphs onto
`ignore_key_matches_ignore_kind`, and the first describes a test that now lives
in the binary's integration tests — so it reads as a guarantee that test does
not make.

Six templates are also declared and **never raised by any code** —
`:11336`, `:11406`, `:12008` plus `ChangeSkin`, `ChangeLanguage` and
`CacheWillBeMoved`. They are the missing restart-note idiom that
`sl-viewer-preferences/src/preferences_graphics.rs:40` says does not exist; wire
them up or drop them (see [[viewer-audit-preferences-restart-note]]).
