---
id: viewer-gallery-scrollbar-and-page-keys
title: The gallery can only be crossed by mouse wheel
topic: viewer
status: done
origin: user, during the viewer-skin-list-row-striping live look (2026-09-23)
points: 2
refs: [viewer-ui-gallery-tab-order, viewer-ui-virtualized-list]
---

Context: [context/viewer.md](../context/viewer.md).

The UI gallery's page is a plain `Overflow::scroll` node whose offset only the
**wheel** moves: no scrollbar, no `Home` / `End` / `PageUp` / `PageDown`. It is
a list of a hundred-odd cards that a person reads looking for one of them, and
a wheel is the slowest way across it — and with no bar there is nothing saying
how far down that one is, either.

## Done (2026-09-23)

- **A scrollbar**, the widget set's own (`bevy_ui_widgets::Scrollbar` pointed at
  the page, `.sk-scrollbar-track` / `.sk-scrollbar-thumb`, the same thickness
  the tab strip and the windowed list use) — so the thumb sizes and drags
  itself, and the bar a person sees in the gallery is the bar the viewer draws.
  It sits in a **row beside** the page rather than floating over its trailing
  edge, holding its own width open, so it cannot cover whatever a card put
  there.
- **`Home` / `End` / `PageUp` / `PageDown`**, the page keys jumping by a
  viewport less one wheel notch of overlap, so the eye keeps an anchor across
  the jump. Announced in the header legend, which is where the gallery's other
  keys are.
- **A focused editor keeps these keys.** All four are text motions and the
  gallery is full of live fields, so the system stands down when `InputFocus`
  is on an `EditableText`. (`drive_gallery_keys` deliberately does *not* do
  this: `D` / `L` / `S` are letters, and typing one into a field is a nuisance
  rather than a lost caret motion. Worth revisiting; it is not this task.)

### The bug under the bug

`bevy_ui` clamps a scroll offset into **`ComputedNode::scroll_position`** and
leaves the `ScrollPosition` component exactly as the app wrote it
(`layout/mod.rs`: `node.bypass_change_detection().scroll_position = …`). The
wheel handler floors at zero and left the far end "to bevy", which is true of
what gets *drawn* and not of the number: scrolling hard at the bottom banked an
offset far past the content, and the next several notches upward did nothing at
all while that slack came back down. `End` would have made it trivial to
reproduce — one press, then a dead `PageUp`.

`clamp_gallery_scroll` runs after layout and pulls the stored offset back to
`content_size - size + scrollbar_size`, which is bevy's own
`max_possible_offset`.
The keys clamp to the same figure rather than writing a big number and hoping.

## Done when

The gallery has a working scrollbar and the four page keys move it, a focused
text field still gets its own `Home`, and the stored offset never runs past the
end.
