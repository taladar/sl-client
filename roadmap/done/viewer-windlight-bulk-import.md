---
id: viewer-windlight-bulk-import
title: Legacy Windlight bulk import
topic: viewer
status: done
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

## Done (2026-09-11)

**The day-cycle converter is in the pure crate**, beside the two it joins:
`sl_proto::legacy_day_cycle_from_bytes`. A legacy day file is an LLSD array of
`[keyframe, sky-preset-name]` pairs — nothing else — so the function takes the
day file's bytes plus a `FnMut(SettingsKind, &str) -> Option<Vec<u8>>` that
hands back a *named* sibling preset, which keeps `sl-proto` free of the
filesystem while letting the caller resolve a name to a file however it likes.
The keyframes land sorted on the surface sky track; a preset named twice is one
frame referenced twice (the reference's `std::set<std::string> framenames`); the
frames are keyed `sky:`/`water:` as the reference keys them, and are *named* for
those keys rather than for the bare preset, because `day_cycle_to_llsd` writes
the map key — so a cycle named any other way is one that reads back differently
from how it was built.

A **missing** water preset is not a failure (the reference falls back to the
`water/` folder the viewer ships; this workspace ships none, so the fallback is
`WaterSettings::legacy_default`, which *is* the reference's `defaults()`), but a
missing or unconvertible **sky** fails the whole cycle by name — a day two
thirds of whose skies are defaults is not the day its author wrote.

**The chooser asks for a folder**, where the reference takes a multi-file
selection (`LLFilePickerReplyThread::startPicker(…, FFLOAD_XML, true)`). A
WindLight collection *is* folders, and a day cycle needs its siblings anyway.
`sl_viewer_platform::file_dialog` grew `FileDialogSelection::{File, Folder}`
for it (`rfd`'s `pick_folder`); the remembered directory is the picked path's
parent either way, so a folder purpose reopens looking *at* what it chose last
time rather than inside it.

**Two divergences in finding a day cycle's siblings**, both because the
reference's own path does not work on a real collection:

- `skies/` and `water/` are looked for **beside** the chosen folder first and
  inside it second. `LLSettingsVODay::buildFromLegacyPreset` derives its base
  path with one `getDirName` on the day file's full path — which is the days
  folder — so it looks for `days/skies`, misses, and falls back to the presets
  the viewer itself ships.
- Within a folder a preset is found by **name**: every `.xml` stem is
  percent-unescaped and indexed under what that yields (and under the raw stem
  too). The reference re-escapes the wanted name and tries three spellings
  (`legacy_name_to_filename`, "a disturbing hack" in its own words), which
  cannot find the viewer's *own* shipped `%28SS%29%20Atmos%2023%2E30%202.xml` —
  no current `LLURI::escape` produces that `%2E`.

**Filing takes the Save As path**, not an upload: `CreateInventoryItem` per
converted preset, with the body written onto the item when the reply names it,
through the one ordered `PendingSettingsCreations` queue — so a bulk import and
an editor's Save As cannot claim each other's items. The folder walk and the
conversion run on the `IoTaskPool`, so four hundred files do not cost a frame.

**The summary is one notification, not four hundred.** The reference raises a
modal `WLImportFail` per file that failed plus its `WindlightBulkImportFinished`
tip at the end; a folder of the wrong kind is then one modal per file. A clean
run raises the reference's tip; a run with failures raises a new
`WindlightBulkImportSummary` naming the counts and up to eight failing files
(every failure is also logged at `warn!`). A run that stops hearing replies is
ended by a 120 s watchdog saying how many items were never confirmed, rather
than greying the menu entries for the rest of the session.

A run says it has started, too: the reference puts up a modal `LLUploadDialog`
("Importing Windlights…") for the length of a run that may be hundreds of
uploads long, which is exactly the wrong shape for it — so this is a tip
(`WindlightBulkImportStarted`) naming the count, and the viewer stays usable.

The three menu entries are greyed while a run is going, as the reference's
`File.EnableImportWindlightBulk` greys them.

## Verified

`cargo test --release -p sl-proto --lib -- legacy_preset` — the day cycle
converting into sorted keyframes and shared frames, every frame named for its
key, a missing water preset falling back to the default, a missing sky failing
by name, a sibling that will not convert failing with its own reason, and the
four not-a-day-cycle refusals.

`cargo test --release -p sl-viewer-environment --lib -- bulk_import` — the
folder walk against real temporary directories: a folder of skies converting
file by file (`.XML` included, a non-preset failing alone, a `.txt` skipped), a
day cycle finding its skies beside its folder and failing by name without them,
the sibling-folder search preferring beside to inside, and the index answering
to both the unescaped name and the raw stem.

Not verified live: the chooser itself is the compositor's, and the filing needs
a grid. Worth one manual run — World ▸ Environment ▸ Bulk Import ▸ Skies over a
`windlight/skies` folder, then My Environments.
