---
id: viewer-ui-color-picker
title: Color picker floater + swatch widget
topic: viewer
status: ready
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-ui-widget-scaffold]
refs: [viewer-ui-texture-picker, viewer-prim-texture-editing, viewer-contact-sets]
---

Context: [context/viewer.md](../context/viewer.md).

The reference's colour picker (`LLFloaterColorPicker`, opened by every
`color_swatch` widget): a saturation/value area + hue strip, RGB (and
hex) fields, a current-vs-original comparison swatch, and a palette of saved
swatches. Ship it exactly like the texture picker
([[viewer-ui-texture-picker]]): a reusable **swatch widget** any panel can
host, an `OpenColorPicker { requester, current }` message and a
`ColorPicked { requester, color }` reply, with live-preview updates while
dragging (the reference applies continuously and reverts on cancel — keep
that, it is how people tune face colours).

Consumers waiting: prim colour in [[viewer-prim-texture-editing]], contact
set colours ([[viewer-contact-sets]]), chat/nametag colour preferences, the
environment editors, particle and beam colours.

Reference (Firestorm, read-only): `llfloatercolorpicker`,
`floater_color_picker.xml`, `llcolorswatch`.

Builds on: the widget scaffold and the skin token system (palette storage in
the settings store).
