---
id: viewer-quick-prefs-environment-presets
title: Quick preferences — sky / water / day-cycle combos over inventory
topic: viewer
status: done
origin: split from viewer-environment-settings-index (2026-09-08) — the consumer
  the index was built for
refs: [viewer-quick-preferences, viewer-quick-preferences-editor,
  viewer-environment-settings-index, viewer-environment-my-environments,
  viewer-ui-combo-widget, viewer-environment-personal-lighting]
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

## Done (2026-09-08)

Three rows added under the group / time pair, in
`sl-viewer-preferences/src/quick_prefs_environment.rs`. The three answers this
task had to give rather than inherit:

**The four times of day stay.** The group / time pair is kept beside the three
combos, not replaced by them. It is the only environment control that works
before inventory loads (the reference's three combos are empty until the walk
finds something), and it is the only way to the Day-Cycle-frozen and
Modern-library groups, for which the reference has no row at all. Where the two
surfaces overlap they agree: the sky combo appends the four ported legacy
WindLight presets as `<Time> [WL]` rows — the reference's own
`mLegacySkies` tail, same suffix — and picking one *is*
`FixedEnvironment::Legacy(time)`, so each surface shows the other's choice.
Unlike the reference the `[WL]` rows are not gated on OpenSim: the World ▸
Environment menu offers those presets on every grid, so a grid test here would
withdraw a control the viewer already has.

**The local layer became three tracks.** `EnvironmentState::local` was one
`Option<EnvironmentAsset>`; it is now a `LocalEnvironment` of a day, a sky and a
water track, each carrying the asset id it came from. That is what
`LLEnvironment`'s `ENV_LOCAL` is (`DayInstance::setSky` / `setWater` replace one
and leave the rest; `setDay` clears both), and without it picking a water preset
would drop the sky picked a moment earlier. The asset id per track is what
`setSelectedEnvironment` reads to decide which row is selected — a sky is not
identifiable by its contents, and a `@setenv_*` edit has no asset at all, which
is exactly the reference's null-asset-id case. `set_fixed(Some(..))` now takes
back only the **sky** track (the one thing a menu pin and a local sky both are);
`set_fixed(None)` still empties the whole layer, which is
`setSharedEnvironment`.

**The sentinels are states, not choices.** `ui_combo` grew `ComboRow`
(Selectable / Disabled / Separator) and `ComboRowStates`, carried with the
labels in `SetComboOptions` so a separator's index can never lag its list. A
disabled row can be *shown* as the selection while refusing the pointer — which
is what "Region default" has to do — and both it and a separator swallow their
press so a stray click does not shut the list. Prev / next step over both,
wrapping, as `stepComboBox` does, and raise the reference's
`NoValidEnvSettingFound` when a list holds nothing pickable (an inventory with
no water settings, say).

Applying a pick is asynchronous, so it goes through a new
`LocalEnvironmentPick` resource in `sl-viewer-world-scene`:
the panel records the asset id, `resolve_local_environment_pick` fetches and
installs it. That is deliberately a resource and not a call, so the
settings picker and [[viewer-environment-my-environments]] reach the same path
without a dependency on the asset store — and so the sync can leave a row the
user just clicked alone while its fetch is still out.

**The preset box stopped claiming the region's environment was rendering when
it was not.** A fifth `Custom` row reports a settings asset in force. That was
two faults, not one: a combo only announces a pick when its index *moves*, so
with the selection parked on "Shared (region)" the one control that empties the
local layer could not be chosen at all.

## Two floater-manager fixes found on the way

Neither is this task's subject; both were in its path.

**Floaters are now confined to a snap rect** — the screen minus every band of
fixed chrome (`ScreenChrome`: the top menu bar, the bottom toolbar), the
reference's `LLFloaterView` snap rect. The clamp reserved nothing, so a window
at block offset zero was *on screen and unreachable*: its title bar, the only
part a drag can grab, sat behind an opaque bar drawn above it. The
quick-preferences panel had persisted `[16, 0]` on one grid and opened there,
undraggable, every session after. Transient overlays are deliberately excluded
— a menu popup, a context menu and another floater all cover a title bar for a
moment, and reserving for them would shove windows aside whenever one opened.

The first cut of that fix **reproduced the bug it fixed**, which is worth
recording. The menu bar is spawned content-sized and stretched to the window
width a frame later, so for exactly one frame it measures as a tall narrow
column — `4x316` in an 800x600 harness whose settled bar is `800x38`. Reserving
off that frame shoved every window a third of the way down the screen and left
it there, because the clamp only ever pushes. A band now has to *span* the edge
it claims (90% of it) and the floater has to be laid out. The threshold started
at 50% and a test caught that too: the same transient column spans 53% of the
viewport height, so it was read as a **side** bar and reserved 4px inline.

**The panel's anchor measures instead of guessing.** It placed the window by
subtracting a constant `300x232`, measured once against the English strings; the
three new rows would have hung it 130px past the bottom edge. It now waits for a
laid-out size and subtracts the real one, in UI-logical units — the same unit
mix-up the clamp was once caught making.

Not done here, and not this task's: the panel does not yet *edit* an
environment ([[viewer-environment-personal-lighting]]), and there is no
inventory settings picker floater.
