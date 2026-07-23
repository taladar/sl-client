---
id: viewer-windlight-bulk-import
title: Legacy Windlight bulk import
topic: viewer
status: blocked
origin: main-menu survey (2026-07-23)
blocked_by: [viewer-environment-fixed-editor]
refs: [viewer-environment-day-cycle-editor]
---

Context: [context/viewer.md](../context/viewer.md).

World ▸ Environment ▸ Bulk Import ▸ Days / Skies / Water: convert the
old pre-EEP Windlight preset files (`.xml` sky/water/day descriptors,
which many users still have in folders from the WL era) into EEP
settings assets in inventory, in bulk.

Scope:

- Parse the legacy WL XML schemas (sky, water, day cycle) and map their
  parameters onto EEP settings (the reference ships this conversion —
  reuse its mapping).
- Batch flow: pick a folder, convert every recognised file, upload each
  as an inventory settings asset, and report a per-file success/failure
  summary.
- Handle the WL→EEP mismatches the reference documents (value ranges,
  renamed parameters) rather than importing silently wrong.

Reference (Firestorm, read-only): `File.ImportWindlightBulk`
(`menu_viewer.xml` World ▸ Environment ▸ Bulk Import),
`llenvironment`/`fsimportwindlight` conversion code.

Builds on: the fixed environment editor (blocked task) — it owns the
settings-asset create/upload path this import feeds.
