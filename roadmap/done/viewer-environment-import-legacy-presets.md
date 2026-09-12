---
id: viewer-environment-import-legacy-presets
title: Import a legacy WindLight preset from disk
topic: viewer
status: done
origin: Split out of viewer-environment-fixed-editor (2026-09-09)
refs:
  [
    viewer-environment-fixed-editor,
    viewer-environment-my-environments,
    viewer-os-portals-linux,
    viewer-windlight-bulk-import,
  ]
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

## Done

**The conversion is in the pure crate**, beside the decoder it feeds:
`sl_proto::legacy_preset_from_bytes` (and the two per-kind
`sky_settings_from_legacy_preset` / `water_settings_from_legacy_preset`).
It starts from `SkySettings::legacy_windlight_default` /
`WaterSettings::legacy_default` — which already *were* the reference's
`defaults()` — and takes each key the preset carries over the top, which is
exactly `translateLegacySettings`'s shape and is why a setting the old format
never had keeps the reference's value rather than a zero.

The three conversions that are not copies are the ones worth naming: the
scalars come out of the four-real arrays WindLight wrote every slider as, the
cloud scroll rate loses its bias of ten (and a disabled axis becomes a literal
zero, since EEP has no `enable_cloud_scroll` to remember the rate in), and star
brightness is rescaled by 250 from the old `0..2` slider. The sun is two Euler
angles — east angle runs *clockwise*, hence the negation — and the moon is
placed opposite it, because the old format stored no moon of its own.

A preset carries **no `type` tag**, so which kind a file is comes from the
editor that asked; a water preset picked in the sky editor is caught by
converting nothing (`converted_something` in the reference) rather than
importing as a sky of pure defaults.

One deliberate loosening: the reference reads a sky scalar out of `legacy[k][0]`
and a water scalar out of `legacy[k]`, so each would read `0.0` from the other's
shape. Both are accepted here — the files in the wild do not honour that
distinction, and a zero is not what the author meant.

**The file-open dialog is a new platform service**, not a knob on this window:
`sl_viewer_platform::file_dialog` is an `OpenFileDialog` → `FileDialogClosed`
message pair over `rfd` (the XDG FileChooser portal on Linux, native dialogs
elsewhere), single-flight, driven on the `IoTaskPool`, with the last-used
directory remembered **per purpose** so the sky editor reopens in
`windlight/skies` and the water editor in its sibling. That is the FileChooser
bullet of [[viewer-os-portals-linux]] landed for all three platforms; the
uploaders and the snapshot save are its next callers. It does **not** parent the
chooser to the viewer's window — Bevy only hands out a window handle through an
`unsafe fn`, and this workspace forbids `unsafe_code`; that costs placement and
modality, not function.

**An imported frame has nothing behind it.** `EditSession::item` became
`Option<EditedItem>` — the reference's `loadInventoryItem(LLUUID::null)` — so
Save says the preset is not in inventory yet, and a Save As files it in the
Settings folder (falling back to the agent root), where a brand-new settings
item goes. The session starts *modified*, as the reference's `setDirtyFlag()`
does, because the frame on screen exists nowhere else. The unsaved-work
confirmation is raised **before** the dialog rather than after the file is
chosen, which is what `onButtonImport` wrapping the whole of `doImportFromDisk`
in `checkAndConfirmSettingsLoss` means; the existing `SettingsConfirmLoss` stash
grew a second arm for it. A failure raises the reference's own `WLImportFail`,
already in the catalogue.

## Not done — and why

- **No legacy day cycle.** `windlight/days/*.xml` names sky presets that live in
  sibling files, so one file is not enough to convert one;
  `SettingsKind::DayCycle` is refused outright rather than half-answered. It
  belongs with
  [[viewer-windlight-bulk-import]], which walks a whole folder and so has the
  siblings in hand.
- **No validation pass.** The reference runs its `validationList` over the
  converted map and warns. There is no equivalent validator in this workspace to
  run, and every field this writes comes from a checked default or the file.

## Verified

`cargo test --release -p sl-proto --lib -- legacy_preset` — the conversion
field by field over the stock `Default.xml` sky and water presets (the real
files, verbatim), an absent setting keeping the reference default, the sun and
moon placement, a disabled scroll axis, star brightness rescaled by 250, the
wrong kind of preset being refused both ways, a non-preset and a day cycle
being refused, a bare scalar still being read, and the escaped-filename
unescaping.

`cargo test --release -p sl-viewer-platform --lib -- file_dialog` — the
single-flight refusal, the per-purpose directory memory, and an idle frame
opening nothing.

`cargo test --release -p sl-viewer-environment --lib` — the import seeding an
unfiled, modified session named after the unescaped file stem; a file that is
not a preset of that kind being refused with `WLImportFail` and the window left
as it was; and another window's file-dialog reply being left alone.

Not verified live: the dialog itself was not driven against a desktop — the
chooser is the compositor's, and what it hands back is a path. Worth one
manual run: press Import in the sky editor, pick a `windlight/skies/*.xml`, and
see the sky change and the name field fill in.
