---
id: viewer-environment-import-legacy-presets
title: Import a legacy WindLight preset from disk
topic: viewer
status: ready
origin: Split out of viewer-environment-fixed-editor (2026-09-09)
refs: [viewer-environment-fixed-editor, viewer-environment-my-environments]
---

Context: [context/viewer.md](../context/viewer.md).

The reference's settings editors carry an **Import** button that reads a
legacy WindLight `.xml` preset off disk — the `windlight/skies/*.xml` and
`windlight/water/*.xml` files a decade of Second Life photographers have
collections of — converts it to an EEP frame, and hands it to the editor as
an unsaved asset.

Scope:

- the legacy preset parser (the reference's
  `LLSettingsSky::buildFromLegacyPreset` /
  `translateLegacyHazeSettings`, which is where the `legacy_haze` block of
  a modern sky comes from in the first place — the decoder already reads
  that block, so the conversion is the missing half);
- a file-open dialog and the Import button in the sky and water editors
  ([[viewer-environment-fixed-editor]]), seeding a session with no
  inventory item behind it, so Save As is what files it;
- the reference also imports a legacy *day cycle* (`days/*.xml`), which
  belongs with the day-cycle editor rather than here.

Left out of the fixed-editor task deliberately: reading somebody's preset
folder is a file-format job with its own test corpus, not a knob on a
window.

Reference (Firestorm, read-only): `llfloaterfixedenvironment.cpp`
(`onButtonImport`, `doImportFromDisk`), `llsettingssky.cpp`
(`buildFromLegacyPreset`), `llsettingswater.cpp`.
