---
id: viewer-environment-fixed-editor
title: Environment editors — sky & water settings assets
topic: viewer
status: done
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-ui-widget-scaffold]
refs: [viewer-environment-personal-lighting, viewer-environment-day-cycle-editor, viewer-environment-my-environments, test-assets-settings-encoder]
---

Context: [context/viewer.md](../context/viewer.md).

The fixed-environment editors: create and edit **sky** and **water**
settings as EEP **inventory assets** (`AssetType::Settings`, flags sky /
water / daycycle). Tabbed panels mirroring the reference — sky: atmosphere
& haze, clouds (texture, coverage, scroll), sun & moon (textures, position,
brightness) and the density sections; water: fog, fresnel, normal map,
wave directions — every field the `SkySettings` / water types already
ingest, now editable with live preview through the local-override layer of
[[viewer-environment-personal-lighting]].

Save path: settings assets serialize as LLSD and upload via the standard
asset/inventory create-update flow (`sl-llsd` + `upload.rs`); load path:
apply from inventory. The library ships Linden defaults to start from.

The serialisation half is already done: `environment_asset_to_bytes`
([[test-assets-settings-encoder]]) writes a sky, water or day-cycle asset
in the notation LLSD the reference uploads, so this task's save path is the
*upload* around it, not a second encoder.

The day-cycle editor and the environments library build on this
([[viewer-environment-day-cycle-editor]],
[[viewer-environment-my-environments]]).

Reference (Firestorm, read-only): `llfloaterfixedenvironment`,
`llfloatereditextdaycycle` (shared panels), `panel_settings_sky_*.xml`,
`panel_settings_water.xml`, `llsettingsvo` (asset serialisation).

Builds on: EEP ingest types, `sl-llsd`, the asset upload path.

## Done

Two windows in `sl-viewer-environment`, one per frame kind, as the reference has
them: a name field, the knobs on tabs, and Save / Save As / Revert. **Open** is
the inventory's Open (or the settings row's new **Edit**), which fetches the
item through the settings-asset manager and seeds the window from the decoded
frame; **New Sky** / **New Water** in the inventory's create menus now mint one
from the built-in default rather than staying greyed out.

**The knobs are one table for the whole crate now.** `SkyKnob` / `WaterKnob` /
`ColorKnob` / `TextureKnob` moved out of the Personal Lighting window into
`knobs.rs` and grew the fields the reference's fixed panels have and the
personal one does not: the moisture / droplet / ice trio, the two multipliers
and the maximum altitude, cloud variance and scroll, both cloud position-density
triples, moon scale and brightness, and the sun and moon images. Each knob
carries a `slug` that is both its Fluent key's tail (`env-knob-*`, shared by
every window that shows it) and the tail of the element id each window names its
control by, so two windows cannot label one value two ways. The controls
themselves are in `rows.rs`, and the thumb-and-readout system that follows a
slider is one system for the crate rather than one per window.

**The tabs are a table too**, and a test walks it against `SkyKnob::ALL` and
friends: every knob is on exactly one tab, and a tab holds only its own editor's
kind. The failure it pins is silent both ways — a knob on no tab is a value
nobody can edit, and one on two tabs is two controls fighting over a field.

**The preview is a layer of its own.** `EnvironmentState` grew `edit`, above the
local layer — the reference's `ENV_EDIT`, which
`LLFloaterFixedEnvironment::onOpen` installs and `onClose` clears. Writing the
*local* layer instead would have been the cheaper thing to do and it is wrong:
whatever personal environment the Personal Lighting window built has to still be
there, unsaved and untouched, when the editor closes. Snapshot-and-restore only
reaches the same place while nothing else writes that layer in between, and a
worn collar's `@setenv_*` does.

**Saving a sky needed the protocol to stop dropping fields.** A Save re-encodes
the *whole* asset, so every key the decoder ignored was a key the save deleted.
Three findings, from diffing our decoder against `LLSettingsSky`'s own key list:

- The three atmospheric-density profiles (`rayleigh_config`, `mie_config`,
  `absorption_config`) are now `DensityLayer` stacks on `SkySettings`. Without
  them a save replaced the author's atmosphere with the reference's defaults for
  everyone who opened the item next. An absent profile stays absent through the
  round trip — the reference substitutes its own defaults for a missing one, so
  an emitted empty array is a third state neither side means.
- `dome_offset` and `dome_radius` are carried the same way. The reference
  stopped *reading* them (`getSkyDomeOffset` is commented out and the dome is a
  constant now) but still ships them in `defaults()`, so every sky it saves has
  them and ours would have dropped them.
- **A decode bug, not a save one.** The seven legacy-haze values live either in
  the `legacy_haze` sub-map *or* at the top level — the reference reads the
  sub-map, then the top level, then its own defaults (`get_float` /
  `get_color`), and writes each back wherever it read it (`set_legacy`). We read
  only the sub-map and fell back to zero, so a sky asset holding them at the top
  level decoded to a **black, hazeless sky**. Now three-step, with the
  reference's defaults as the floor.

The keys still not modelled are `lightnorm` (derived, never authored) and
`east_angle` / `sun_angle` / `enable_cloud_scroll`, which are legacy WindLight
*preset* keys that `buildFromLegacyPreset` folds into `sun_rotation` and
`cloud_scroll_rate` — they never appear in an EEP asset, and belong to
[[viewer-environment-import-legacy-presets]].

**One creation queue for the viewer.** `NewFileAgentInventory` mints an item
with empty flags, and for a settings item that byte *is* its kind — so a Save As
is followed by a `ChangeInventoryItemFlags`, matched FIFO against a reply that
carries no correlation id. The wearable creators already had such a queue; a
second one beside it would have popped on the first's uploads and stamped items
with each other's subtypes. `PendingItemCreations` in `sl-viewer-world-api` is
now the one queue, the inventory owns the one consumer (finishing a creation is
half a folder refresh), and the wearable path moved onto it. An **in-place**
save needs none of that: the reply names the item it wrote, which is a real
correlation.

**The Density tab edits what this viewer does not draw.** The fourth sky tab is
the reference's density panel: sixteen terms over the three scattering profiles.
Our sky is the legacy WindLight formula and reads none of them, so those sliders
change the *asset* and not the preview — which is worth having anyway, because a
settings asset is authored for the grid and every other viewer does read them.
The renderer half is [[viewer-environment-density-profiles]]. Two divergences
from the reference here, both deliberate: a term is written into **layer 0 in
place**, where the reference replaces the whole profile with a single layer
(`createSingleLayerDensityProfile`) and so discards the second layer of the
ozone ramp its own default ships; and a term written into a frame carrying no
profile at all materialises the reference's default profile first, rather than
storing one term beside four zeroes. (The reference's own panel also reads its
absorption *constant* term out of `exp_term` — a copy-paste slip in
`llpaneleditsky.cpp` that is not worth copying.)

Its sliders read at up to eight decimals: a Rayleigh linear term runs
`0.0 .. 0.000004`, and at the two decimals every other knob uses it is the
string `0.00` at every position of its slider. The precision is per knob, so
nothing else's readout changed.

**Every field the types ingest is now editable**, which is more than the
reference's panels offer. The four the reference exposes nowhere — the sun
disc's angular radius and the planet / atmosphere radii — take their ranges from
its own *validator* (`LLSettingsSky::validationList`), which is a better source
than a number chosen here; the sun disc sits with the sun, the three radii with
the density model they belong to. The bloom, halo and rainbow images join the
sun and moon images, and the water's transparent texture joins its normal map.
What is left is `dome_offset` and `dome_radius`, carried but not offered,
because the reference does not read them at all.

## Not done — and why

- **No Import.** The reference's `Import` reads a legacy WindLight `.xml` preset
  off disk, which is the legacy-preset importer's job rather than this window's.
  Filed as [[viewer-environment-import-legacy-presets]].
- **No day-cycle creator.** New Day Cycle stays greyed out: an item nothing can
  open is worse than a menu entry that says so. It comes with
  [[viewer-environment-day-cycle-editor]].

## Verified

`cargo check --workspace --all-targets` clean, no warnings.

`cargo test --release -p sl-viewer-environment --lib` — 18 green: the knob
sweeps (every knob round-trips, a knob leaves its siblings alone, the colour
scales, the built-in texture defaults, every slug unique), the three over the
scattering profiles (editing one materialises only that one, editing one keeps
the layers it is not showing, an absent Mie anisotropy reads as the reference's
default) and the five editor ones (every knob on exactly one tab, a tab holds
only its own kind, each settings kind routes to its editor, renaming touches
only the name, a save keeps the fields the editor cannot show).

`cargo test --release -p sl-viewer-world-scene --lib` — 175 green, including
three new ones over the edit layer: an edited frame renders over the local one
and gives it back, closing one editor leaves the other's preview, and a preview
is never saved as the personal environment.

`cargo test --release -p sl-proto --test lifecycle` — 458 green, including the
settings-asset round trip (now carrying the density profiles and the two dome
values), "a sky without them stays without them", and the new legacy-haze
lookup test (either place, or the reference's defaults).

`cargo test --release -p sl-viewer-inventory --lib` — 79 green, including the
blessed context-menu and create-menu tables.

Not verified live: no settings item has been opened, edited and saved against a
grid. The save path is the part worth driving — `UpdateSettingsAgentInventory`
and the flags stamp are both grid behaviour, and OpenSim's Default Region has an
inventory to make one in.
