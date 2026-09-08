---
id: viewer-environment-settings-index
title: Settings assets by name — the inventory index the Library Environments folder needs
topic: viewer
status: done
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

## Done

`sl_viewer_inventory::settings_index` — a `SettingsIndex` resource rebuilt
whole whenever the mirror's change tick moves, holding every settings item the
inventory has fetched, grouped by kind and name-ordered, plus the Library
`Environments` folder projected as a name lookup.

**The kind is `SettingsKind` in the pure crate**, beside `EnvironmentAsset`
rather than beside `InventoryType`, because it is the same distinction the asset
body already carries — an `EnvironmentAsset::kind()` ties the two together, so
"what a decoded asset is" and "what an item's flags say it is" are one type
answered two ways. It reads the same `II_FLAGS_SUBTYPE_MASK` low byte
`ScriptLanguage::from_item_flags` does, and is the *only* way to tell one
settings item from another without downloading it — which is the whole reason
the index can exist. It is deliberately **not** `#[non_exhaustive]`, unlike its
neighbours: these three are the whole of `LLSettingsType::type_e` (its
`ST_INVALID` / `ST_NONE` sentinels being the `None` the constructors return),
every consumer files an asset under exactly one of them, and a fourth kind ought
to break each of those matches rather than fall into a wildcard that drops it.

**The reference has two collectors and they disagree on purpose**, so the index
holds both answers:

- `FSSettingsCollector` is the **list** — the whole inventory, agent tree *and*
  Library, outside the Trash and outside Marketplace Listings, de-duplicated by
  **asset** id rather than item id, name-ordered with duplicate names intact
  (a `std::multimap`, and two skies really can share a name).
- `RlvIsOfSettingsType` is the **name lookup** — the Library `Environments`
  folder only, `getActualType()` so a link is not a settings item, matched
  case-insensitively, first match winning.

So the list resolves links and the name lookup refuses them. Neither is an
oversight: the list shows the user what they have and a link is a thing they
have; the lookup answers a script, and the scope it is given is the point.

**Links are resolved the way `LLViewerInventoryItem` resolves them** —
`getType`, `getAssetUUID`, `getFlags` *and* `getName` all follow the link, so a
link
contributes its target's name, kind and asset and is then usually de-duplicated
away against the target itself. Our mirror does none of that: a link arrives as
`AssetType::Other(24)` with its own flags and an `asset_id` that is the target
**item**'s id. The follow costs a hash lookup, not `InventoryModel::find_item` —
that is a linear scan of every fetched folder, and the Current Outfit Folder is
a folder of nothing but links, so a scan per link would have made one rebuild
quadratic in the size of the inventory.

**Three new mirror accessors**: `library_root`, `folder_by_name_under` (the
`LLNameCategoryCollector` rule — exact, case-sensitive, and *descendants* only,
so the root it is given is never its own answer), and `needs_fetch` /
`request_folder` widened to the crate.

**The Library subtree is prefetched**, one folder at a time as pages arrive.
`request_all_agent_folders` deliberately skips the Library — a user who never
opens it should not pay for it — but a script's `@setenv_preset:<name>` cannot
wait for the user to expand a folder, so the `Environments` subtree gets the
same targeted eager fetch the Current Outfit Folder does.

**`@setenv_preset:<name>` and `@setenv_daycycle:<name>` now resolve.** That was
this task's origin — the one form [[viewer-rlv-environment-commands]] could not
honour. `RlvEnvironmentSlot` gained an `RlvLibraryEnvironments` projection the
index publishes, because `RlvEnvSource::apply_environment` is called
synchronously from inside the command parser and holds no world. The two arms
that were collapsed into one are now separate: the reference searches `ST_SKY`
for `preset` and `ST_DAYCYCLE` for `daycycle`, so the same name can mean two
assets and answering the wrong one would be worse than refusing.

## Not done — and why

- **The two remaining consumers are their own tasks, and were before this one.**
  The quick-preferences sky / water / day-cycle combos
  ([[viewer-quick-prefs-environment-presets]], split out today — ours offers
  four fixed times in three groups where the reference offers every settings
  asset, which is a redesign of that panel) and the settings picker
  ([[viewer-environment-my-environments]],
  [[viewer-region-environment-panel]]). So `SettingsIndex::of_kind` has no
  caller yet; it is the same walk the library lookup already needs rather than a
  second structure built on speculation, and both fields a row carries
  (`item`, `library`) are what a picker addresses and labels with.
- **A settings asset's kind is trusted from its flags, never checked against the
  asset.** The reference does the same, and the alternative is downloading every
  settings item in inventory to build a list of them.
- **The `#ifdef OPENSIM` legacy WindLight preset names** the reference appends
  to each combo (from `LLEnvironment::mLegacyDayCycles` / `mLegacySkies` /
  `mLegacyWater`, keyed by name rather than by asset id) are not here. They are
  a property of the *combo*, not of inventory — there is no inventory item to
  index — so they belong with whoever builds the combos.

## Verified

`cargo test --release` and `cargo clippy --release --all-targets` clean over
`sl-proto`, `sl-client-bevy`, `sl-viewer-world-api`, `sl-viewer-inventory` and
`sl-client-bevy-viewer`.

Twenty new tests. In the pure crate: both directions of the subtype byte, that
the mask ignores every other item flag, that a byte no kind claims is refused
rather than cast into one, and that a decoded asset reports the kind its flags
would have carried. Over the index: that the flag byte and not the folder
decides the kind; that the Trash and Marketplace Listings are excluded at any
depth; that two items of one asset are one entry while two assets of one name
are two; that a link reads as its target and a dangling one is dropped; that a
link's *own* folder decides whether it is collected, so a link outside the Trash
to a settings item inside it does surface that item (which is where the
reference lands too, `EXCLUDE_TRASH` being a flag on a traversal that never
reaches the target); that the `Environments` folder is the Library's own and a
same-named folder in the agent tree cannot answer for it; and that the name
lookup is case-insensitive, kind-exact, library-scoped, link-refusing and
first-wins. Over the RLV slot: that an id still wins over a name, that a name
resolves within its own kind only, and that water is indexed but no command
searches it.

Not verified live: no grid session drove it. The interactive check is the RLVa
console against aditi (whose library has a populated `Environments` folder,
unlike the local OpenSim) — type `@setenv_preset:Sunrise=force` and watch the
sky change, then `@setenv_daycycle:Sunrise=force` and confirm it answers a
*different* asset.
