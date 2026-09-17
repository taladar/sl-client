---
id: viewer-audit-notifications-crate-split
title: Split the 21637-line notification catalogue and make its lookup a map
topic: viewer
status: done
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

## What was done

1. **Split along the `// ---- <family> ----` markers**, exactly as the verdict
   read. `catalogue.rs` lists 31 family modules and nothing else;
   `catalogue/<family>.rs` holds that family's `ENTRIES`, with the banner
   comment promoted to the module's own docs. The four sections that had no
   `viewer-notification-catalogue-*` task got names for what they are:
   `generic` (the fallbacks and demo exemplars), `server_alerts`,
   `confirmations` and `info_tips`. `forms.rs` took the 97 button tables.
   `lib.rs` is 1525 lines — types, lookup, runtime state and the 17 tests;
   the largest catalogue file is `objects_edit.rs` at 2840.

2. **`NOTIFICATIONS` is still one `&'static [NotificationTemplate]`**, so no
   caller changed and the catalogue stays usable from a `const` context: the
   families are flattened into one array during const evaluation
   (`flatten_families`). Rust cannot concatenate `const` slices, so that copy
   is written out as a loop; it costs nothing at run time. The two `const fn`s
   are the crate's only `indexing_slicing` exception, each with its reason.

3. **`template()` is a map lookup.** A `LazyLock<HashMap<&str, &Template>>`
   built once on first use, rather than `binary_search`: sorting the array by
   name at compile time needs the same indexing loop *and* would have put the
   entries in an order that has nothing to do with the families they are
   edited in. No new dependency (`phf` would have needed a `build.rs`).

4. The fused doc paragraph on `ignore_key_matches_ignore_kind` lost the half
   that described a test living in the binary's integration tests.

Verified by a normalised token diff of the catalogue body against
`git show HEAD:` — the only lines that moved are the 30 banner comments that
became module docs — and by the 17 invariant tests, which pin the entry names,
the 140 suppressible entries and the ignore-kind sets against the reference.

The six never-raised templates were left alone: they belong to
[[viewer-audit-preferences-restart-note]], which decides whether to wire them
up or drop them.
