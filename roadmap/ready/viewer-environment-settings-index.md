---
id: viewer-environment-settings-index
title: Settings assets by name — the inventory index the Library Environments folder needs
topic: viewer
status: ready
origin: split from viewer-rlv-environment-commands (2026-09-08) — the name lookup that task could not do
refs: [viewer-rlv-environment-commands, viewer-quick-preferences, viewer-environment-my-environments, viewer-environment-fixed-editor, viewer-region-environment-panel]
---

Context: [context/viewer.md](../context/viewer.md).

An index over every **settings asset** (`AssetType::Settings`) the inventory
mirror holds — the agent's own tree *and* the read-only Library — grouped by
kind (sky / water / day cycle) and addressable **by name**. Three separate
surfaces want the same lookup and none of them can do it today:

- **`@setenv_preset:<name>=force` / `@setenv_daycycle:<name>=force`**
  ([[viewer-rlv-environment-commands]]) resolve a name against the Library's
  `Environments` folder specifically (`rlvGetLibraryEnvironmentsFolder`, a
  `LLNameCategoryCollector` for the category named `Environments` under the
  library root). Only the asset-id form works today; a name is refused.
- **The quick-preferences sky / water / day-cycle preset combos**
  ([[viewer-quick-preferences]]). Ours offers the four ported fixed times of day
  in three groups; the reference offers *every settings asset in inventory*,
  three combos with prev / next buttons either side
  (`FloaterQuickPrefs::loadPresets` → `loadSkyPresets` / `loadWaterPresets` /
  `loadDayCyclePresets`).
- **The settings picker** other panels summon — My Environments
  ([[viewer-environment-my-environments]]), the region / parcel environment
  panel ([[viewer-region-environment-panel]]), the day-cycle track editor.

## What has to be built

- **The kind.** A settings item's sky / water / day-cycle kind is carried in its
  **inventory flags** (`LLSettingsType::fromInventoryFlags`), and nothing here
  decodes them yet — `InventoryType::Settings` is as far as the mirror goes. It
  belongs in the pure crate beside the other inventory-flag readers, not in the
  index.
- **The index itself**, built over the inventory mirror
  (`sl_viewer_inventory::inventory`), which already holds the library tree and
  tags its folders (`is_library`). Rebuilt when the mirror changes rather than
  walked per query, because the quick-prefs combos would otherwise walk the
  whole tree on every open.
- **The reference's own collector rules**, which are not obvious:
  duplicate names are kept (it is a `std::multimap`, and two skies really can
  share a name), items are **de-duplicated by asset id** rather than by item id,
  and anything under **Marketplace Listings** or the **Trash** is excluded
  (`FSSettingsCollector`).
- **The Library `Environments` folder** as a named lookup of its own, because
  that is the scope RLV searches — not the whole inventory. A grid whose library
  has no such folder must resolve nothing rather than fall back to the agent's
  tree.

## Not part of this

Fetching the library tree is done (`inventory-a9`, `inventory-b7`): the mirror
already holds it. This task is the *index over* it, and the flag decode that
tells one settings item from another.

Reference (Firestorm, read-only): `quickprefs.cpp` (`FSSettingsCollector`,
`FloaterQuickPrefs::loadPresets` and the three `load*Presets`),
`rlvenvironment.cpp` (`rlvGetLibraryEnvironmentsFolder`, `RlvIsOfSettingsType`),
`llsettingstype.cpp` (`LLSettingsType::fromInventoryFlags`),
`llfloatermyenvironment.cpp`.
