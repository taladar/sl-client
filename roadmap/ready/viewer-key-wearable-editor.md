---
id: viewer-key-wearable-editor
title: One wearable-editor window per bodypart / clothing item
topic: viewer
status: ready
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
