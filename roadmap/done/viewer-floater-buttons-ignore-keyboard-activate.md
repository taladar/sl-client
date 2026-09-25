---
id: viewer-floater-buttons-ignore-keyboard-activate
title: The material editor, wearable editor and object contents buttons act on a mouse press only
topic: viewer
status: done
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

## Done (2026-09-25)

All six buttons were already spawned by `ui_spawn::spawn_button`; what was
wrong was the kind and the observer. The material editor's Save / Revert and
alpha-mode buttons and the wearable editor's Save / Save As / Revert were
`ButtonKind::Interaction` (`bevy_ui`'s marker, which nothing activates from the
keyboard) with a `Pointer<Press>` observer; they are `ButtonKind::Headless` now
and observe `Activate`. The object contents buttons were already headless but
observed `Pointer<Press>` anyway; they observe `Activate`. A pointer
activation now fires on release, like every other button in the viewer, rather
than on press.

The wearable editor's handler required the preview resources (texture manager,
bake inputs, local bake) before looking at which button it was, though only
Revert reads them. It now asks for them in the Revert arm only.

**Not contract rows.** Contract rows cover registered `ELEMENTS`, and these
three windows are `FLOATERS`. Their specimens act on window state an element
host has none of, so a row there could only say `inert`, which would not show
`Enter` doing anything. Instead each crate has an interaction test that drives
a primary click, `Enter` and `Space` through the real input and focus stack
(`sl-viewer-testkit`), each in a fresh app, and asserts the same effect:

- `edit_contents::tests::a_contents_button_acts_on_enter_space_and_a_click`
  (one `ContentsActionRequest`);
- `edit_material_asset::tests::revert_answers_enter_space_and_a_click` and
  `…::the_alpha_mode_button_answers_enter_space_and_a_click`;
- `edit_wearable::tests::the_save_buttons_answer_enter_space_and_a_click`
  (Save asks for its window once; Save As sends one upload).

Their inline colours are still there. They belong to
[[viewer-skin-floater-controls-outside-the-skin]].
