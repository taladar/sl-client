---
id: viewer-text-field-scrollbar
title: Scrollbar for a scrolled multi-line text field
topic: viewer
status: ready
origin: viewer-lsl-editor-widget (2026-10-29)
refs: [viewer-lsl-editor-widget, viewer-skin-scrollbar-shape]
---

Context: [context/viewer.md](../context/viewer.md).

An editable field scrolls through `TextScroll`, which neither
`ScrollTarget::Container` (a node's `ScrollPosition`) nor
`ScrollTarget::List` (a virtual list's offset) drives, so the code editor and
the notecard body have no scrollbar: they scroll by the wheel (the code
editor) and by following the caret. The reference's script editor and
notecard both have one.

Add a `ScrollTarget` for a text field to `sl-viewer-ui-core`'s shared
scrollbar: the thumb sized from the field's layout height against its content
box, dragging and the arrow ends writing `TextScroll`, and the thumb following
a caret-driven scroll. Then give the code editor (beside its text, inside the
well) and the notecard body one.
