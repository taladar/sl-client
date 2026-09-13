---
id: viewer-multiline-field-fills-its-floater
title: A multi-line field that grows with the window around it
topic: viewer
status: ready
origin: user report (2026-09-13, resizing the notecard floater)
refs: [viewer-ui-text-input-widget, viewer-notecard-editor,
  viewer-notecard-inline-items, viewer-lsl-editor-widget]
---

Context: [context/viewer.md](../context/viewer.md).

**Making the notecard floater bigger does not make its body bigger.** The window
grows and the field stays the size it was, leaving a lake of empty content slot
around it. The same is true of the script editor and of every other multi-line
field, because they share one widget — so this is `ui_text_input`'s to fix, not
any one editor's.

It is not an oversight, and the *reason* is the hard part of the task. A
multi-line field's height is a `ContentSize` measure —
`visible_lines × line_height` — and `bevy_ui` resolves a measure's constraints
as `effective = known.or(preferred.or(min).maybe_clamp(min, max))` (in
`measurement::resolve_axis`). A `min_height` added so the field can shrink does
not *floor* the intrinsic height, it **replaces** it; `min_height: 0` erases it,
and the field then lays out at zero in any container with no spare height to
grow it back. That is why `TextInputSpec::fill` exists for the single-line
(width) case and has no height counterpart, and why the notecard and script
editors are content-driven today. Anyone reaching for "just add `flex_grow`"
should read that doc comment first.

Two ways out, and the first is probably right:

- **drive `visible_lines` from the space the field is given.** The field keeps
  its measure and stays honest about its intrinsic size; a system reads the
  content slot's height each time it changes and sets `visible_lines` to fit.
  One feedback loop to be careful of (the measure changes the layout that fed
  it) — settle it by quantising to whole lines and only writing on a real
  change, the way [[viewer-notecard-inline-items]]'s object measurement does.
- **replace the measure with an explicit height** when the field is in fill
  mode: no `ContentSize` at all, `flex_grow: 1` and `min_height: 0`, so taffy
  sizes it like any other box. Simpler, but it gives up the intrinsic size that
  makes a content-driven window open at the right height — so a filling field
  would need its window to carry a `default_size`.

Whichever lands, the rich-text field ([[viewer-notecard-inline-items]]) must
follow it: its objects are positioned against the field's box, so a field that
grows moves them, and a `min_width` floor it declares today should become the
floor of a field that otherwise fills.

Do the notecard, the script editor and the prim-contents description together —
they are the same widget and the same window-shaped complaint.
