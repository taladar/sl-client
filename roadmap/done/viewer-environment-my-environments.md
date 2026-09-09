---
id: viewer-environment-my-environments
title: My Environments library
topic: viewer
status: done
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-environment-fixed-editor]
refs: [viewer-environment-personal-lighting, viewer-region-environment-panel]
---

Context: [context/viewer.md](../context/viewer.md).

The "My Environments" library floater: every settings asset in inventory
(sky / water / day cycle, filterable), with **apply to self** (the local
override layer of [[viewer-environment-personal-lighting]]), edit (opens
the matching editor), rename / delete, and the settings-asset **picker**
widget other panels summon (the day-cycle track editor, the region/parcel
environment panel [[viewer-region-environment-panel]]). The Linden library
folder's stock environments appear alongside the user's own.

Reference (Firestorm, read-only): `llfloatermyenvironment`,
`floater_my_environments.xml`, `floater_settings_picker.xml`.

Deps: [[viewer-environment-fixed-editor]] (asset model + editors).

## Done

**Two windows and one list.** `my_environments` is the library the user browses
(World ▸ Environment ▸ My Environments…); `settings_picker` is the chooser a
panel summons for one field. They differ in their chrome and in what a pick
does, and not at all in what a row *is*, so the row, the filters, the folder
resolution and the ordering are one module (`settings_list`) and each window
keeps only its own table spec and its own actions.

**The rows come from the index that already existed.** `SettingsIndex` — the
reference's `FSSettingsCollector`, built once per change of the inventory mirror
— is what Quick Preferences' three preset combos are already filled from, so a
window that opens costs a filter and a sort rather than an inventory walk, and
the library cannot disagree with the combos about what exists.

**The list is flat, and that is a deliberate divergence.** The reference embeds
an `asset_filtered_inv_panel` — the inventory *tree* filtered to settings items
— so its rows sit under their folders, with a Show All Folders toggle for the
empty ones. Going flat costs the hierarchy and buys the collector's
de-duplication: two items of one asset are one row, which is what a library of
environments is for. The context the tree gave by position is a **Where** cell
instead, naming the folder and prefixing *Library* for one of the shared
read-only ones — without that prefix a stock sky and one of the user's own in
same-named folders read identically, and only one of them can be saved over.

**The row actions are the reference's gear menu**, on a right-click rather than
behind a toolbar button: Edit (greyed for a day cycle, which has no editor yet),
Apply Only To Myself, Copy UUID, Delete. Under the list are a rename row and the
reference's bottom panel — New Sky, New Water, New Day Cycle (greyed, as it is
in the inventory's create menu) and the trash.

Four of those reuse what already exists rather than growing a second path:

- **Edit** writes the same `OpenSettingsEditor` the inventory's Open sends.
- **Apply Only To Myself** is a `LocalEnvironmentPick`, the request the preset
  combos already make: the window never touches the asset store, and two
  surfaces asking for the same asset is one fetch.
- **New Sky / New Water** call `new_settings_item`, extracted out of the
  inventory's `dispatch_create` so the default frame, the permission mask and
  the stamped subtype byte have one definition. A settings item's kind *is* its
  flags byte, and the stamp rides the viewer's one creation queue.
- **Rename / Delete** are `MoveInventoryItem`, spelled the way the inventory
  floater spells them, with the reference's `DeleteItems` confirmation in front
  of the delete.

**The `@setenv` gate is on the action, not the window.** The World ▸ Environment
presets and the Personal Lighting entry are greyed while an object holds the
environment, and the obvious thing would have been to grey this entry too. It
would be wrong: most of what this window does is inventory work, and a collar
holding the sky has nothing to say about renaming a sky you own. Only Apply Only
To Myself changes what the user is standing under, so only that carries the
restriction.

**The picker's kind is the opener's.** `setSettingsFilter` fixes it in the
reference and it is fixed here — a water field being handed a day cycle is not a
choice worth offering — so the picker has no kind checkboxes, and its title says
which kind it is asking for. Its reply protocol is the texture picker's, which
means a consumer written against one is written against the other: a
**non-final** `SettingsPicked` on each selection so the panel can preview it,
the committed one on OK, and what the picker opened on when Cancel (or the
chrome's ✕) closes it. One window rather than the texture picker's per-field
keyed ones, because nothing compares two settings assets side by side; an open
while another pick is outstanding **answers** that one first rather than leaving
its panel waiting.

## What running it found

Every one of these was found by launching the viewer, not by a test, and four of
them predate this task. They are recorded because the pattern is the point: none
of them was visible to `cargo test`, and several were silent on the grid as
well.

- **A first-frame panic.** `bind_picker_rows` took `Query<&mut Text>` beside
  `Query<(&mut Text, &mut TextColor)>` — Bevy's `B0001`, raised the first time
  the system runs, which took the whole viewer down before anything drew.
  Nothing in the workspace had ever *scheduled* these plugins: the floater sweep
  builds chrome from the specs without them. There is now a smoke test that
  schedules `EnvironmentUiPlugins` in a bare app and runs two frames, and it was
  A/B'd — re-introducing the conflicting query makes it fail with the same
  `B0001`.
- **An unregistered message.** `CreateSettingsItem` was never `add_message`d, so
  the add row was dead. In this Bevy version that is a panic rather than a skip,
  which the same smoke test now catches.
- **Settings items were created with the wrong protocol** — see
  [[viewer-settings-save-as-create-then-put]] for the half still outstanding.
  The creators uploaded through `NewFileAgentInventory`, which has no settings
  arm on either grid: OpenSim files the item as a **Texture**
  (`UploadCompleteHandler` leaves `assType`/`inType` at their `0` defaults) and
  Second Life creates nothing. The reference asks the *simulator* to mint it
  (`create_inventory_settings`), which authors the default asset for the kind
  and stamps the subtype — so `new_settings_item` is now a `CreateInventoryItem`
  and the flags stamp is gone. A test pins the wire shape for all three kinds.
- **The editor opened nameless.** `read_editor_names` reads the name field back
  on `Changed<EditableText>`, and a freshly spawned field counts as changed — so
  the first pass read the empty widget over the name the asset arrived with. It
  now skips while a reseed is outstanding.
- **The editor opened behind the window that asked for it**, because showing an
  already-open floater does not raise it. It sends `BringToFront` now, after the
  manager's command pass so its raise outlives the opening click's.
- **A shared table oscillated.** `column_cell_node` gave Flex columns flex-basis
  `auto`, so a cell's width followed its own content — and the ellipsis marker
  lives inside the cell. Showing the marker widened the basis, which widened the
  clip, which made the value fit, which hid the marker: a per-frame flip, seen
  as a neighbouring column's text jumping in and out from under its ellipsis.
  Flex columns now use `flex_basis: 0`. All 296 layout sweeps pass, and they run
  in 169 s rather than 404 s.
- **The index reported only what had been browsed to.** Folder contents are
  fetched lazily and `SettingsIndex` walks only loaded folders, so a created
  item was invisible until its folder was opened by hand. Two fixes: the index
  prefetches the agent's `Settings` folder as well as the Library's
  `Environments`, and the **viewer now runs the background inventory crawl**
  (`background_inventory_fetch: true`) — it was off while the on-disk cache was
  on, so the viewer cached a tree it never fetched. Inventory search had the
  same silent gap.

## A divergence that had to be undone

The list was built on `SettingsIndex`, which de-duplicates by **asset** id as
`FSSettingsCollector` does. That is right for the quick-preferences combos —
they pick an *environment*, and two items of one asset are one choice — and
wrong here: the simulator gives every fresh sky the same default asset, so two
New Skies collapsed into one row and the item without a row could not be renamed
or deleted. The index now holds one entry per **item**, and the combos
de-duplicate where they build their rows. The reference lands in the same place
by a different route: its My Environments and its picker both embed an inventory
panel, which shows every item.

## Not done — and why

- **Apply To Parcel / Apply To Region.** Publishing to land is
  [[viewer-region-environment-panel]]'s, which owns the permission tests the
  reference guards those two entries with (`canAgentUpdateRegionEnvironment` /
  `canAgentUpdateParcelEnvironment`) and the altitude-track scoping that goes
  with them. Offering the verb here without those tests would be a menu entry
  that fails on most land.
- **Copy / Paste.** The reference's gear menu forwards them to the hosted
  inventory panel's clipboard, which this window is not hosting one of.
- **No track combo in the picker.** The reference's *Select Track* combo appears
  only in `TRACK_WATER` / `TRACK_SKY` mode, to import one track out of the day
  cycle being chosen. Its only caller is the day-cycle editor's track import,
  and the combo needs the chosen asset **fetched and decoded** to know how many
  tracks it has — so it belongs with [[viewer-environment-day-cycle-editor]],
  which has something to do with the answer.
- **No `SettingsUnsuported` gate.** The reference greys its add menu on
  `LLEnvironment::isInventoryEnabled()`. Filed as
  [[viewer-environment-settings-unsupported-gate]], with the note that it would
  not have caught what the live runs did: OpenSim advertises both settings caps
  and fails elsewhere.
- **The picker has no in-viewer caller yet.** Both of its consumers are the
  tasks this one unblocks. It is registered in `FLOATERS` (so the layout sweep
  covers its chrome) and its reply protocol is unit-tested, but nothing opens it
  until the day-cycle editor or the region panel does.

## Verified

**Live on aditi.** Create a sky and a water (each lands selected, scrolled to,
named), edit one (the editor comes to the front with its name filled in), and
the Library's `Environments` rows list with their folder and a *Library* prefix.
The two windows were driven on OpenSim first, which is what turned up the
creation protocol — OpenSim accepts the upload and silently files it as a
Texture, so the DB was the only place the failure was visible.

`cargo clippy` clean on every touched crate.

`cargo test --release -p sl-viewer-environment --lib` — 33 green, 11 of them
new: six over the shared list (each kind flag hides only its own kind, a
picker's fixed filter shows exactly one, the name filter is a case-insensitive
substring, a row carries its folder and its Library flag, a Library row with no
folder still says *Library*, a row addresses the **link** while carrying the
**target's** asset, each sort column orders by what it shows with the name as
tie-break) and six over the picker (the answer carries both ids and the name, a
selection the filter has hidden is not an answer, every kind titles the window
apart, the opener's kind is the only one offered, the columns match the spec,
the two reply buttons are named apart).

`cargo test --release -p sl-client-bevy-viewer --lib` — 296 green, including
every floater and element through the layout matrix after the `flex_basis`
change.

Not verified live: the **picker**, which nothing opens yet — its two consumers
are the tasks this one unblocks. Its chrome is in the layout sweep and its reply
protocol is unit-tested.
