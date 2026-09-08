---
id: viewer-quick-prefs-environment-presets
title: Quick preferences — sky / water / day-cycle combos over inventory
topic: viewer
status: ready
origin: split from viewer-environment-settings-index (2026-09-08) — the consumer
  the index was built for
refs: [viewer-quick-preferences, viewer-quick-preferences-editor,
  viewer-environment-settings-index, viewer-environment-my-environments]
blocked_by: [viewer-environment-settings-index]
---

Context: [context/viewer.md](../context/viewer.md).

The Quick Preferences panel's environment row is **two** combos here — a group
(shared / day cycle / legacy / modern) and a time of day (sunrise / midday /
sunset / midnight) — driving `FixedEnvironment`. The reference has **three**,
one per settings kind, each listing *every* settings asset in inventory with
prev / next buttons either side (`FloaterQuickPrefs::loadPresets` →
`loadSkyPresets` / `loadWaterPresets` / `loadDayCyclePresets`).

The lookup those three need now exists —
[[viewer-environment-settings-index]] holds every settings item grouped by kind,
name-ordered with duplicate names kept, `SettingsIndex::of_kind`. What is left
is the panel:

- **Three combos, not two**, and a water one where there is none today. Ours
  models the environment as one choice; the reference models it as three
  independent tracks, which is also what `EnvironmentState`'s local layer can
  already hold one of each of.
- **The two sentinel rows each combo prepends**, before a separator: day cycle
  offers *Region default* and *No day cycle*; sky and water offer *Region
  default* and *Day-cycle based*. `setDefaultPresetsEnabled` greys the pair
  while they do not apply.
- **Prev / next buttons** either side of each combo, stepping the list.
- **The `#ifdef OPENSIM` legacy WindLight names** the reference appends after a
  second separator, from `LLEnvironment::mLegacyDayCycles` / `mLegacySkies` /
  `mLegacyWater` — keyed by *name* rather than asset id, with `"[WL]"` appended
  to a sky's label. These are not inventory items, so they are the panel's own
  and not the index's; ours would come from `sl_viewer_kit::sky_presets`.

Where the four fixed times of day we offer today should end up is a decision
this task has to make rather than inherit: the reference has no such row, but it
is the only environment control that works before inventory loads.

Reference (Firestorm, read-only): `indra/newview/quickprefs.cpp`
(`FloaterQuickPrefs::loadPresets`, `loadSkyPresets`, `loadWaterPresets`,
`loadDayCyclePresets`, `setDefaultPresetsEnabled`, `onChangeSkyPreset` and
siblings), `panel_quick_prefs.xml`.
