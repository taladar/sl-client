---
id: viewer-key-material-editor
title: One material-editor window per material item
topic: viewer
status: ready
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
