---
id: viewer-windlight-bulk-import
title: Legacy Windlight bulk import
topic: viewer
status: ready
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

## Half the conversion already exists (2026-09-11)

[[viewer-environment-import-legacy-presets]] landed the **sky and water**
converters as `sl_proto::legacy_preset_from_bytes`, with the value-range and
renamed-parameter mismatches this task warns about already handled (the scalars
out of their four-real arrays, the cloud scroll rate's bias of ten, star
brightness × 250, the sun's clockwise east angle). It also landed the file
dialog, in `sl_viewer_platform::file_dialog`.

What is left for this task: the **day cycle** (`days/*.xml`), which
`legacy_preset_from_bytes` refuses outright — its keyframes name sky presets
that live in sibling files, so converting one needs a whole folder rather than
one file, which is what a bulk import has anyway. Plus the folder walk, the
per-file upload, and the success/failure summary.
