---
id: viewer-key-material-editor
title: One material-editor window per material item
topic: viewer
status: done
origin: split out of [[viewer-keyed-floater-audit]] (2026-09-07)
points: 3
refs: [viewer-keyed-floater-audit, viewer-profile-floater-single-instance,
  viewer-key-wearable-editor]
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-edit/src/edit_material_asset.rs` (`"material-editor"`) holds **one**
`MatEditState::active` edit, so opening a second material replaces the first
along with whatever was changed and not saved. Materials are exactly the kind
of asset a resident edits by comparison — two variants of the same surface side
by side — and the single window makes that impossible.

Convert it onto the keyed scaffold ([[viewer-profile-floater-single-instance]]),
following `edit_notecard.rs`.

## What moves

- **Key** by the material's inventory item id (`OpenMaterialEditor::item`) — a
  `FloaterKey::subject`.
- `MatEditorUi` (`panel` / `content` / `title`) and `MatEditState` become
  components on the window root; `MatEdit` becomes this window's edit, and its
  `Option` goes away with the window's lifetime. `MatPhase` (`Loading` /
  `Ready` / `Rebuild`) is per window and stays as it is.
- Open through `KeyedFloaters::open`, build content at spawn, and order the
  open system `.after(FloaterSystems::Commands)`.
- Re-opening an item already up **raises** it and must not re-fetch, so a
  pending edit survives (`reopening_a_notecard_does_not_refetch_it` is the
  shape of the test).

## The gotchas this one has

- **Picker replies** are matched by the swatch entity (`pick.requester`), which
  names a widget — resolve its window with `host_floater`.
- **The asset decode** (`AssetReceived` → `Loading` → `Ready`) must be matched
  per window by the asset id the window asked for, not by "the" active edit;
  two materials decoding at once is the normal case once this lands.
- **The preview sphere** is a world-space node per edit (`MatEdit::preview`).
  Two editors mean two previews — check they do not land on top of each other,
  and decide what the second window's preview looks like before wiring it.
- **Save in flight** (`saving`) must be resolved back to the window that
  saved; see `rebind_saved_asset` in the notecard/script conversion for how a
  reply that does not name the editor is re-driven from the editor that knows.

## How to verify

Open two materials: two windows, each on its own material, each with its own
preview and status line; a colour or alpha change in one must not touch the
other; closing one leaves the other; re-opening keeps unsaved edits. Pin the
window count and the no-refetch rule with `instances` unit tests.

## Done (2026-11-03)

`MatEditorUi` and the `MatEditState` resource are gone; `MatEdit` is a
component on each window, carrying its content column, and the window spawns
through `KeyedFloaters::open` (ordered after `FloaterSystems::Commands`). The
picker replies, the factor commits, the alpha / double-sided toggles and the
Save / Revert buttons all reach their edit through `FloaterHost` from the
control they name. How each gotcha came out:

- **Key.** The item id, as planned — plus the notecard (and the prim holding
  it) for a material **embedded in a notecard**, the way About Landmark keys an
  embedded landmark: its item id may be that of the resident's own inventory
  copy, and the two are different windows, one saveable and one not.
- **Decode.** Already per asset in `MaterialManager`; each window waits on its
  own asset id, and two windows on one asset share one fetch.
- **Preview.** Nothing to decide: `material_preview` already binds every
  `MaterialPreview` node to a studio on its own render layer, so the second
  window's sphere is exactly the first's and neither shows in the other's pane.
- **Save.** Matched by the item the reply names (a reply naming none is taken
  as the window's own, as the notecard editor does). A landed save now also
  **rebinds** the item's asset in `InventoryModel` from the item the window
  saved, and moves the window itself onto the new asset — the material editor
  had the same stale-reopen gap [[viewer-saved-asset-reopens-stale]] fixed for
  notecards and scripts, and keying makes close-and-reopen the ordinary way to
  hit it. A failed upload names nothing, so every window with a save in flight
  reports it.

Pinned by `edit_material_asset::tests::instances`: two windows under their own
keys and content columns, a re-open keeping an unsaved edit, the embedded copy
as its own unsaveable window, closing one leaving the other, a colour pick
landing only in its swatch's window, and a save reply landing only in the
window that saved. The existing keyboard-activation tests now run their
buttons inside a real window.

Live through the viewer by the e2e stage `material_window_per_material`
(`e2e_windows::each_material_edits_in_a_window_of_its_own`): two materials
from inventory open two windows, an unsaved metallic edit survives opening the
other and re-opening its own, Save reports only in its window, closing one
leaves the other, and the saved material re-opens with the saved value.
