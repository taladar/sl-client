---
id: viewer-key-color-picker
title: One colour-picker window per field, like the texture picker
topic: viewer
status: ready
origin: split out of [[viewer-keyed-floater-audit]] (2026-09-07)
points: 2
refs: [viewer-keyed-floater-audit, viewer-profile-floater-single-instance]
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-ui-widgets/src/ui_color_picker.rs` (`"color-picker"`) is a singleton
holding one `ColorPickerState`. The texture picker had the same shape and was
converted (see [[viewer-keyed-floater-audit]]): it is now keyed by the **field**
being picked for, so each field's window remembers its own position and size,
and picking a tint for one swatch does not steal the window open on another.
This task does the same for colour.

## What moves

- **Key** by the field: a `FloaterKey::named`, the scaffold's persisting half,
  so each field's window keeps its own rect under
  `color-picker_<field>_rect`. Two swatches sharing a field name share a
  window, deliberately — they are the same field.
- That means `OpenColorPicker` needs a **field name** beside its `requester`
  entity, exactly as `OpenTexturePicker::field` and the `TextureSwatchField`
  component do today. Give `spawn_color_swatch` the same treatment so a
  caller's element id flows through to the key; the call sites
  (`edit_wearable.rs`'s tint swatch, the build tools, the skin editor) each
  pass their own name.
- `ColorPickerState` and `ColorPickerUi` become components on the window root;
  content is built at spawn, and the open system runs
  `.after(FloaterSystems::Commands)`.

## The gotcha the texture picker found

Closing a keyed window **ends** it, so the revert-on-close path must read the
manager's close **command** (before the pass that carries it out) rather than
noticing a hidden panel afterwards — and OK / Cancel must clear the requester
first, which is what stops the revert from undoing the choice just made. See
`revert_on_close` / `close_picker` in `ui_texture_picker.rs`; the live-preview
`ColorPicked` stream (emitted continuously while dragging) makes getting this
wrong more visible here, not less.

## How to verify

Open the colour picker from two different fields (a wearable tint and a build
tool colour): two windows, each seeded with its own colour, each replying to
its own requester; Cancel in one reverts only its field; closing one leaves the
other. Pin it with `instances` unit tests mirroring the texture picker's.
