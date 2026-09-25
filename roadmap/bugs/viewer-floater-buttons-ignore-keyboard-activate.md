---
id: viewer-floater-buttons-ignore-keyboard-activate
title: The material editor, wearable editor and object contents buttons act on a mouse press only
topic: viewer
status: bugs
origin: viewer-gallery-floaters-are-mostly-stubs — found while extracting their content builders (2026-09-25)
refs: [viewer-gallery-floaters-are-mostly-stubs, viewer-skin-floater-controls-outside-the-skin]
---

Context: [context/viewer.md](../context/viewer.md).

## Observation

The Save / Revert buttons of the material editor
(`sl-viewer-edit/src/edit_material_asset.rs`) and the wearable editor
(`sl-viewer-asset-editors/src/edit_wearable.rs`), and the object contents
action buttons (`sl-viewer-edit/src/edit_contents.rs`), observe
`Pointer<Press>` rather than `Activate`. Focusing one with `Tab` and pressing
`Enter` or `Space` does nothing, while every button built by the button
widget answers both.

## What to do

Observe `Activate` (which the pointer path also produces), ideally by moving
these buttons onto the button widget
([[viewer-skin-floater-controls-outside-the-skin]]), and add the contract rows
(`Enter` / `Space` emit) that pin it.

## Done when

Each of these buttons does the same thing from the keyboard as from the mouse,
and a contract row says so.
