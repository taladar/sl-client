---
id: viewer-multiline-field-fills-its-floater
title: A multi-line field that grows with the window around it
topic: viewer
status: done
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

**Unit-verified, not yet eyeballed**: the widget tests pin both axes (a filling
field grows past its declared lines and takes its panel's width; one in a panel
with no width does *not* take the screen), and the floater sweeps pass — but
nobody has yet dragged the notecard or script window and watched the body
follow.

## What it actually took (2026-09-13)

Neither of the two ways below: **`flex_grow: 1.0` and nothing else.**

The trap is real but narrower than it looked. `min_height` becomes the
`effective` size because the style carries no explicit height, so `preferred` is
`None` and the min is all `resolve_axis` has to go on — that is what lays a
field out at zero. `flex_grow` touches none of it: the measure stays the flex
**base** size, so the field still opens at its declared lines in a
content-driven window and grows past them only where a container has spare
height to give. A floater hands its content slot a definite size once it has
one, so the growth starts exactly when the resident resizes the window.

The width half came with it, and cost a second round. Dropping the
`MULTILINE_MAX_WIDTH` wrapping bound is *not* enough on its own — it is a
regression: with no bound the measure answers "whatever is available", and in a
content-driven window that is the width of the **display**, so the floater
shrink-wraps to its content, the content asks for everything, and the window
opens across the screen. (The floater sweeps caught it: every resizable floater
test failed at once, with the notecard window laid out `[400, 100]..[2000,
521]`.) What a filling field needs is an **intrinsic** width instead of a bound
— a reading measure in `"0"`-glyph advances, the same vocabulary a single-line
field's width already uses. An unsized window then opens at that measure, and a
sized one stretches the field past it, which a `max_width` could never do.

Both axes are one opt-in flag (`TextInputSpec::fill`, already the vocabulary
for the single-line case), so nothing that did not ask for it moved: the
profile, group and preferences fields still stand at their declared size.

Wired for the notecard body (through the rich-text field, whose root grows too
— the field has nothing to grow into otherwise) and for the shared
`spawn_body_field`, which is the script editor's body.

One duplicate fell out on the way. `ui_text_input` had meanwhile grown a
`ReadOnlyField` stance — greyed, focusable, selectable, copyable, refusing only
the edits that would change the buffer — which is exactly what the rich-text
field had built for itself a day earlier. The widget now asks for that stance
instead of keeping its own filter, and brings `TextInputPlugin` with it rather
than trusting a consumer to remember: without that plugin a read-only body looks
right and silently takes every keystroke.

The two designs that were considered and not needed:

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
