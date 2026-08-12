---
id: viewer-settings-file-resilience
title: Settings store — survive a broken settings file without data loss
topic: viewer
status: ideas
origin: graphics-tab live verification (2026-08-12) lost the global overrides
---

Context: [context/viewer.md](../context/viewer.md).

During the graphics-tab live verification, repeated hand-edits of
`viewer-settings.toml` between headless runs (the harness A/B idiom)
ended with the file reduced to only the `[render]` section and the
other sections' overrides (`[input]`, `[minimap]`, `[statusbar]`,
`[ui]`, `[world]`, `[worldmap]`) silently gone. The suspected chain: a
hand-append created a duplicate `[render]` table header → the next
load hit a TOML parse error (or partial parse) → the session continued
with an empty/partial override set → the exit save rewrote the file
from that state, discarding everything else. Not reproduced under
controlled conditions yet — verify the failure mode first.

## Task

Make the store's load / save path lose nothing when the file is bad:

- On a parse failure, do **not** continue silently with an empty store
  and then overwrite the file on exit. Keep the unreadable file aside
  (e.g. rename to `viewer-settings.toml.broken-<timestamp>`), log
  loudly, and start clean — the user's data stays on disk.
- Consider a one-deep backup on every save (`.bak`), so any surprise
  rewrite is reversible.
- A duplicate-table or unknown-key file section should surface as a
  warning naming the file and line, not vanish.

Applies to both the global and the account-scope files (the account
file carries all floater geometry, so losing it hurts more).
