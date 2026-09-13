---
id: viewer-notecard-inline-items
title: A rich-text field, and a notecard body with its items in it
topic: viewer
status: done
origin: user request (2026-09)
refs: [viewer-notecard-editor, viewer-lsl-editor-widget,
  viewer-url-linkification, viewer-ui-text-input-widget]
---

Context: [context/viewer.md](../context/viewer.md).

A notecard's embedded items belong **in** its text, whether the resident is
reading it or writing it — that is what the reference viewer draws
(`llviewertexteditor`'s embedded-item segments) and what the asset means. The
notecard editor instead carried a **toggle**: a plain edit field where each item
was a placeholder glyph, and a separate read-only preview where the items were
legible and clickable. That toggle was a stand-in for a missing widget, and this
task is the widget.

## Why the toggle existed

Bevy 0.19's editable text *is* `parley::PlainEditor`: one style for the whole
buffer, no inline boxes. Parley's layout builders model both already —
`RangedBuilder::push_inline_box` reserves width and height at a byte offset, and
a ranged style scopes a property to a byte range — the plain editor simply owns
its builder and exposes neither.

## What was built

**Two upstream patches**, both small, both in forks this workspace already
pins (the root `Cargo.toml` documents each):

- **parley** (`sl-client/0.9-patch`): `PlainEditor::set_inline_boxes` and
  `set_range_styles`, pushed into the builder when the layout is rebuilt. Both
  compare before invalidating, so a caller may hand its model over every frame;
  an offset that lands out of bounds or inside a character is skipped rather
  than panicking, because the caller's offsets can lag an edit by a frame.
- **bevy** (`sl-client-externally-posed-skin`):
  `ComputedTextBlock::set_entities`. The renderer resolves a glyph's colour
  (and its underline) through the entity
  a brush's section index names in that list, which the text pipeline fills from
  a `Text` block's spans — an editable field has none, so every per-range brush
  resolved to nothing and a rich field could not colour a range.

**The widget**: `sl-viewer-ui-widgets/src/ui_rich_text.rs` — a multi-line field
whose flow carries **objects** (a whole `bevy_ui` subtree laid out *in* the
text) and whose byte ranges wear **style classes** (a colour, an underline, and
whether a press on one activates it) or are **hidden** (kept in the buffer,
taking no room and drawing nothing). Three things it had to get right:

- an object is an **overlay sibling** of the field, not a child: a node's
  intrinsic size is a `ContentSize` measure, taffy measures only childless
  nodes, and a field with children silently loses the "eighteen lines tall" that
  sizes the window around it;
- an object is **hidden, not undisplayed**, when it has no box — an undisplayed
  node is never laid out, so it would never be measured, so its box would stay
  at zero and it would never appear at all;
- the root is a **column**, so the field stretches across it. In a row it is a
  main-axis item sized by its own measure, and a multi-line field measured
  against no definite width comes out as wide as its padding;
- the node that **sizes** and the node that **clips** cannot be the same one.
  `overflow: clip` zeroes a box's automatic minimum size, so a clipping root
  stops being pushed out by the field inside it and collapses to whatever its
  parent allots — in the content-sized notecard window, to its padding, with
  the field hanging out of it. So the root hugs the field and does not clip,
  and the clipping is done by an **overlay** absolutely positioned at inset
  zero: it fills the root's padding box exactly while contributing nothing to
  its size, and the objects live in it. (Caught by the element sweeps, which
  measured the root at 36 px around a 103 px field — invisible in the gallery,
  whose cards have a definite width.);
- a rich-text field needs an **intrinsic width floor**. A multi-line field has
  an intrinsic *height* (`visible_lines`) and no intrinsic width at all — its
  measure takes the width it is offered, and a container that offers none
  leaves it as wide as its own padding. Plain text survives that illegibly; an
  object cannot, because its box is a real node that then hangs out of a field
  narrower than one item. So the field declares a floor, the way a single-line
  field declares a width in glyph advances.

And one that cost the most to find, because it fails as "clicking the field does
nothing": **naming brush sections takes a field's presses away from it.**
`bevy_ui` resolves a press on a text node to the *section* the glyph under the
pointer belongs to — that is how a `Text` block's spans are individually
pickable — so a field that names sections hands every press on its own prose to
an inert style carrier, and `bevy_ui_widgets`' caret observer on the field never
fires. Section 0 is therefore the **field itself** (it already carries the
colour section 0 should be drawn in); a press on a *styled* range does land on
that range's section node and is resolved back to the field, which is exactly
the press a link click wants. Two dead ends on the way: section nodes as
children of the field fix the bubbling and destroy the field's height, and
`Pickable::IGNORE` on them leaves nothing hit at all.

**The notecard body**: one flowing text with its items in it, read or written.
The buffer keeps each item's private-use marker code point — it is what a
deletion deletes and what `Notecard::with_edited_text` reconciles against on
save — the field takes the marker off the screen, and the item's box is laid out
exactly where the marker sits. Boxes are reused by what they stand for, so
typing prose around an item moves its box rather than despawning and respawning
it (the floaters' build-once rule, and what keeps a link box's in-flight name
lookup alive).

Links differ by mode, deliberately: a **read-only** body draws each as the
resolved chip the rest of the viewer draws (an avatar's name, an icon, the
hover tooltip — `linkified_text`'s own node over the hidden URL text), while an
**editable** body leaves the URL as the text it is and styles it in place, with
a press on it re-read from the buffer and dispatched as a `LinkActivated`.

**Read-only** is no longer a different widget: the body drops the edits that
would change the buffer and keeps every motion and selection edit, so a
no-modify notecard stays selectable, copyable and clickable — the reference's
own behaviour — instead of being a separate non-editable block.

And one that only a pair of eyes could find: **parley puts an inline box's
bottom on the baseline**, which is right for a box of arbitrary content and
wrong for this one. The object here is a pill *with text in it*, so
bottom-aligned its name floats a descent above the prose around it — visibly,
and only on lines that have text, because a line holding nothing but the item
has its baseline set by the item. The object's top is placed at the line's
**text ascent** instead — taken from a glyph run on the line, not from the line
metrics, which a tall box grows — so the pill's own first baseline lands on the
line's.

And the one that every layout assertion in the suite was blind to: **the
overlay has to be spawned after the field.** `bevy_ui` stacks siblings in tree
order and a decorated field paints a background, so an overlay spawned first is
painted over — its objects invisible and unclickable, while sitting exactly
where they should be and passing every geometric check. The live symptom is
distinctive and worth recognising: *the text leaves a hole of the right size
with nothing in it, and clicking the hole does nothing*. Pinned now by a test on
the two nodes' `ComputedStackIndex` (verified against the broken order, not just
the fixed one).

One addition to the **layout harness** fell out of it. The element sweeps hold
every node to "your content fits your box", which stops having an answer for a
node whose children are absolutely positioned by something other than taffy —
the harness already skips an open popover's ancestors for exactly that reason.
`sl-viewer-ui-core`'s new `ContentMayOverflow` lets a widget make that statement
about itself, with a reason, the way `TextMayClip` does for clipped text; the
rich-text overlay is its first (and so far only) user.

## What this leaves for the script editor

[[viewer-lsl-editor-widget]] wanted "per-range colour plus inline boxes, built
knowing the second is coming". Both halves are now in the widget and in the
forks; what that task still owns is the editor's own surface — undo/redo, a
gutter and line numbers, current-line highlight, find/replace and go-to-line —
plus the colour *source* (the lexer) in [[viewer-lsl-editor-highlight]].

Dropping an item **at the caret** rather than appending its marker still waits
on the field reporting the caret's byte offset.
