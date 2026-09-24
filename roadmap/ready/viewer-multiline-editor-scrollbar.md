---
id: viewer-multiline-editor-scrollbar
title: Multi-line text fields scroll with no scrollbar, and no table scrolls sideways
topic: viewer
status: ready
origin: viewer-skin-scrollbar-shape (2026-09-24)
points: 3
refs: [viewer-skin-scrollbar-shape, viewer-vintage-skin]
---

Context: [context/viewer.md](../context/viewer.md).

[[viewer-skin-scrollbar-shape]] put every scrolling surface in the viewer on
the one `sl_viewer_ui_core::scrollbar` widget. Two kinds of overflow were left
out because the widget cannot reach them yet.

## Multi-line text fields

A `TextInputKind::Multiline` field — a notecard, the script body, profile
text, a group's charter — scrolls its content vertically to follow the caret,
but the offset is the **editor's own**, inside `EditableText`, not a
`ScrollPosition` on a node. So there is no bar, a long notecard shows no sign
of how long it is, and you cannot move through it without moving the caret.

The widget needs a third `ScrollTarget` whose offset and content height come
from the editor's layout, and a way to write the offset back without moving
the caret. The reference's text editors all have a bar (`LLTextBase` is an
`LLScrollContainer`).

## Tables that are wider than their window

The reference's scroll lists grow a **horizontal** bar when their columns
outrun the width. Ours never scroll sideways: `top_objects.rs` floors a
window's width at the columns' sum instead, and other tables squeeze their
flexible column to nothing. The widget has horizontal bars now; the table's
header and rows would have to share one offset.

## Done when

A multi-line field over-full of text shows a bar that tracks and moves it, and
a table narrower than its columns can be scrolled sideways with its header
following.
