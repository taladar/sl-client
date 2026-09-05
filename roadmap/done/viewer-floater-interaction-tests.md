---
id: viewer-floater-interaction-tests
title: Floater chrome under a real pointer
topic: viewer
status: done
origin: user request (2026-07) — end manual re-testing of UI interactions
points: 5
blocked_by: [viewer-ui-interaction-harness, viewer-floater-registry]
---

Context: [context/viewer.md](../context/viewer.md).

Drive `floater.rs`'s real observers headlessly:

- title-bar `Pointer<Drag>` moves the floater — assert
  `FloaterGeometry.position` tracks the drag, clamped to the viewport;
- resize-handle drags respect `min_size` and the content reflows without
  `layout_violations`;
- dock/minimize/close buttons emit their `FloaterOp`s;
- press-anywhere brings to front (`FloaterZTop` ordering);
- `floater_persist.rs` round-trips geometry.

Swept over the whole `FLOATERS` registry so a new floater inherits chrome
coverage by registering.

## Landed (2026-09-05)

`sl-client-bevy-viewer/src/floater_chrome.rs` — the registry sweep, eight
checks over all 38 windows: each one lays out at rest, follows its title bar
to a definite destination, keeps a grabbable sliver on screen when thrown at
the corner, grows with its grip, stops at its own declared floor, collapses
and restores, docks and tears off, closes on its ×, and raises + highlights
itself under a press. Each button's `FloaterOp` is asserted from a recorded
`FloaterCommand` stream rather than inferred from the state it left.

What a single fixture window cannot show went beside the widget instead:
`floater.rs`'s `scenarios` gained the two-window z-order (a press raises *its*
window and lights only that title bar) and the docked tear-off slop — a
distance-since-press rule that a hand-triggered `Pointer<Drag>` can only pin
by making the number up. `floater_persist.rs` gained the lifecycle, driven:
a window dragged and resized by the pointer, its stored values read back out,
and a **second session** that registers first, is handed those values at its
own login, and opens the window where the first one left it.

Two findings, both in the gap this tier exists to close — the layout matrix
has no editable-text stack, so a body field sized in `visible_lines` is
measured there at nothing:

- the **notecard** and **script** editors each opened with a body taller than
  the `default_size` their own window declared (42 px and 52 px over), and a
  floater's content slot clips — so the Save button under it was cut off the
  bottom of the window, unreachable by any click, in two windows the layout
  sweep had reported clean in eight scripts. Both windows are **content-driven**
  now (the scaffold's convention 2): the window follows the field rather than a
  rect measured by eye against the *read-only* block, which is what those
  numbers were. A bigger number would have broken again at the next font size.
- the script editor's **diagnostics list** was unbounded, so a compile with a
  dozen errors would have pushed the same rows out of the same clip. Bounded
  and wheel-scrollable, like the notecard reader beside it.

The first fix attempted was to flex the field into the window's rect, and it is
worth recording why that cannot work — it looks obviously right and it silently
destroys the field. `EditableText`'s height comes from a `ContentSize` measure
(`TextInputMeasure`: `visible_lines` × line height) and `bevy_ui` resolves its
constraints as `effective = known.or(preferred.or(min).maybe_clamp(min, max))`
(`measurement::resolve_axis`), so a `min_height` added to let the field shrink
**replaces** the intrinsic height instead of flooring it. `min_height: 0` erases
it: the field then has whatever a `flex_grow` can win back, which in a floater
is the spare room and in a gallery card is nothing at all. Headlessly this
passes every check in the harness — the box is inside its parent, inside the
viewport, overflowing nothing — and on screen the editors showed no text. **The
gallery caught it, not the suite**, which is what the gallery is for.

Two things came back from that. The harness gained **`field_violations`** — a
laid-out `EditableText` shorter than its own font size is a finding, because a
field of no height passes every containment check there is. It would not have
caught this instance (the `flex_grow` beside the zero minimum refilled the
height in every headless fixture) but it does catch the trap, since a
`min_height` on a field now lays it out at exactly that and nothing else. And
`TextInputSpec::fill` carries the note, so the next person to reach for the same
lever reads why it is not there.

It lives in a new **`interaction_violations`** (everything `layout_violations`
has, plus the checks that need a stack the layout harness does without) rather
than in `layout_violations` itself, which the two interaction sweeps now call.
The split is not tidiness: a field's height only exists once
`update_editable_text_content_size` has run, so in a plain `LayoutTest` app the
check would fail every text field in the viewer for want of a system the harness
deliberately omits — "the harness left it out" and "the widget is broken" are
indistinguishable from inside a check.

The first attempt gated it on a marker resource left by `install_text_editing`
instead, and that broke `a_click_focuses_the_field_it_lands_on`: inserting
**any** resource while that installer runs leaves the click's focus unset
(verified with an unrelated empty resource, three runs out of three). Filed as
[[viewer-testkit-click-focus-resource-sensitive]] — a test standing on
`ComponentId` allocation order that nothing names, which makes the harness
un-extendable and may mean the live viewer's focus-on-click is order-sensitive
too.

The gallery's cards are `flex_shrink: 0` for the same episode: its page is a
definite-height scroll container holding more cards than fit, so the first card
whose content *could* shrink was asked to absorb the whole shortfall. A card
showing an element at a height the viewer never gives it is the one thing that
gallery must not do.

`install_element_hosting` moved out of `ui_contract`'s test module to the
module body: both sweeps spawn the same content specimens under a pointer, so
what a specimen needs to be live is answered once. `sl-viewer-testkit` gained
`interact::centre_of_entity` — the by-entity half of `centre_of`, because two
live floaters carry the same chrome names and a name lookup always answers
with the first.
