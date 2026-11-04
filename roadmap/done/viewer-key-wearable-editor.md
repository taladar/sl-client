---
id: viewer-key-wearable-editor
title: One wearable-editor window per bodypart / clothing item
topic: viewer
status: done
origin: split out of [[viewer-keyed-floater-audit]] (2026-09-07)
points: 3
refs: [viewer-keyed-floater-audit, viewer-profile-floater-single-instance,
  viewer-key-texture-preview]
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-asset-editors/src/edit_wearable.rs` (`"wearable-editor"`) is a
singleton holding **one** `WearEditState::active` edit. Opening a second
wearable therefore replaces the first — including its unsaved parameter
changes. That is the same edit-losing shape the notecard and script editors had
before they were keyed, and it is worse here: editing a shape while looking at
the skin it sits under is the ordinary way to work on an avatar.

Convert it onto the keyed scaffold ([[viewer-profile-floater-single-instance]]),
with `edit_notecard.rs` as the worked example.

## What moves

- **Key** by the wearable's inventory item id (`OpenWearableEditor::item`) — a
  `FloaterKey::subject`, so nothing persists per instance.
- `WearEditorUi` (`panel` / `content` / `title`) and `WearEditState` become
  components on the window root. `WearEdit` stops being "the active edit" and
  becomes "this window's edit"; the `Option` around it goes away with the
  window's own lifetime.
- The open path goes through `KeyedFloaters::open`, content is built at spawn
  instead of by rebuilding a shared column, and the open system runs
  `.after(FloaterSystems::Commands)` (the inventory row that opens the editor
  also raises the inventory window; the later raise wins).
- Re-opening an item already up **raises** it and must not re-fetch — that
  re-fetch is what replaced typed edits with the grid's copy in the notecard
  editor (`reopening_a_notecard_does_not_refetch_it`).

## The gotchas this one has

- **Picker replies.** Texture picks are matched by the swatch entity
  (`pick.requester`), which already names a widget — resolve its window with
  `host_floater` rather than reaching for a resource. The tint colour pick is
  matched against `WearEdit::tint_swatch`, a per-edit entity: with two windows
  that comparison has to run per window, not against "the" active edit.
- **Save in flight.** The pending-save match (`InventoryAssetSaved`) must
  belong to the window that saved; see how the notecard editor rebinds its
  saved asset (`rebind_saved_asset`) from the editor entity rather than from a
  reply that does not name it.
- **Bake / preview.** `shape_dirty` / `bake_dirty` / `pending_textures` drive a
  re-composite of the **worn** avatar, which is global state, not per window.
  Decide explicitly what two open editors mean for the preview — the honest
  answer is probably that only the window whose item is currently worn drives
  the bake — and write that decision into the module header.

## How to verify

Open two wearables (a shape and a skin): two windows, each on its own item,
each with its own parameter values; a change in one must not appear in the
other; closing one leaves the other; re-opening keeps unsaved edits. Pin the
window count and the no-refetch rule with `instances` unit tests mirroring
`edit_notecard.rs`.

## Done (2026-11-04)

`WearEditorUi` and the `WearEditState` resource are gone; `WearEdit` is a
component on its window, which `open_wearable_editor` spawns through
`KeyedFloaters::open` (ordered after `FloaterSystems::Commands`) under the
item's `FloaterKey::subject`. Every observer and system reaches its edit
through the window its control lives in (`FloaterHost`): slider drags, the
gender radio, texture and tint picks (the tint is still checked against the
window's own `tint_swatch`), Save / Save As / Revert. `save_wearable` takes the
`SaveEditorWindow` it names, the in-place report finds the window by its
transaction and the Save As report by its creation ticket. A re-open only
raises the window and never re-seeds it. The status line gained the test id
`wearable-status`.

What did not fall out of the pattern:

- **The bake preview, decided.** The worn avatar is global, so the honest
  question was what two editors mean for it. The answer is *both preview*: the
  bake inputs (`sl-viewer-world-avatar` `OwnBakeInputs`) now hold the worn
  assets **by item** (`WornAsset { item, asset }`), `set_preview_asset` takes
  the item, and a new `worn_asset_of(item)` seeds a window from its own item.
  So a shape window and a skin window both show on the avatar, and a window
  closing puts back only its own item. Closing is an `On<Remove, WearEdit>`
  observer (`restore_preview_on_close`), since a keyed close despawns the
  window and there is no hidden window left for a system to notice.
- **The composite drew one layer per type.** `sl-bake`'s `region_layers` took
  each clothing slot's texture, tint and garment masks from the *first* worn
  wearable of its type, so a second shirt (or an edit of the lower of two)
  never showed — on OpenSim's client bake, in this editor's preview, and in
  the fake grid's bake service, which shares `assemble_layers`. It now draws a
  planned wearable layer once per worn wearable of its type, bottom first, each
  copy tinted and cut by its own wearable (`WornLayer { wearable, index }`),
  as the reference's `LLTexLayerTemplate::render` does; `region_texture_ids`
  lists every worn wearable's texture likewise. Pinned by
  `layered_shirts_composite_each_bottom_first`.
- **Layered wearables were mixed up.** The editor used to seed from the
  *first worn asset of the item's type*, so Edit on the second of two worn
  shirts showed (and Save wrote onto the second shirt) the first shirt's
  content. Keyed windows make opening both the ordinary case, so this is fixed
  with the by-item store above. The store is also kept in worn order now; it
  used to be in fetch-completion order despite its doc saying otherwise.
- **A Current Outfit Folder link** row opens the item it links to
  (`resolve_wearable`), so a link row and the item's own row are one window
  and Save writes onto the wearable rather than the link.
- A landed in-place save moves the window's item onto the stored asset and
  rebinds it in the inventory model.

Pinned by `edit_wearable` `instances` unit tests (two windows, re-open keeps
the edit, seeding by item, link resolution, close restores only its own item,
a tint pick and a save each landing in their own window), two `bake_inputs`
`by_item` tests, and the e2e stage `wearable_window_per_wearable` in
`tests/e2e_windows.rs` (wear two layers, move a slider, open the other, re-open,
Save, close one).
