---
id: viewer-skin-scrollbar-shape
title: The scrollbar is a bar — the reference's has ends, a track and a shape
topic: viewer
status: done
origin: Vintage skin fidelity audit (2026-09-20)
points: 3
refs: [viewer-ui-virtualized-list, viewer-vintage-skin, viewer-skin-image-backed-widgets]
---

Context: [context/viewer.md](../context/viewer.md),
[context/vintage-skin.md](../context/vintage-skin.md).

`virtual_list.rs` draws a scrollbar as two rectangles: a 10 px track in
`srgb(0.12, 0.14, 0.18)` and a thumb in `srgb(0.40, 0.48, 0.60)`, both
hardcoded, with a 24 px minimum thumb. That is every scrollbar in the viewer.

The reference's (`llscrollbar.cpp`) is four parts — a decrement button, a
track, a thumb and an increment button — and every skin dresses all four.
Vintage ships `ScrollArrow_{Up,Down,Left,Right}`, `ScrollThumb_{Vert,Horiz}`
and `ScrollTrack_{Vert,Horiz}`, nine-sliced, and then *tints* them from
`colors.xml`: `ScrollbarThumbColor` white (so the art shows through) over
`ScrollbarTrackColor` `#999999`. The measured art is a dark blue thumb
(`#3c4c7c`) in a mid-grey groove.

Two separate gaps, and they are worth keeping separate:

1. **Shape and colour are unskinnable.** Even the flat rectangles we draw
   should take their colours from role tokens, and be able to take a
   nine-sliced image instead ([[viewer-skin-image-backed-widgets]]).
2. **The end buttons do not exist.** A classic skin without them does not read
   as classic, and clicking an arrow to step a list is behaviour a Second Life
   user of long standing has in their hands. This is a small widget feature,
   not only a paint job: press-and-hold repeat, a step size, and the
   interaction rules the rest of the widget set already follows
   ([[viewer-ui-interaction-contracts]]).

## What to do

- `--scrollbar-track`, `--scrollbar-thumb`, `--scrollbar-thumb-hover`,
  `--scrollbar-arrow` roles; `virtual_list` consumes them.
- Optional end buttons on the scrollbar, on by default when the active skin
  says so — an arrow glyph or a skin-supplied image, with click-to-step and
  hold-to-repeat.
- Keep the thumb's minimum length and the existing scroll maths untouched; the
  ends take their thickness out of the track, not out of the content.

## Done when

The scrollbar's colours come from the skin, a skin can give it arrow ends, the
arrows step and repeat, and the list's scroll maths is unchanged (its existing
tests still pass with the ends both on and off).

## Done (2026-09-24)

**Every scrollbar is one widget now**, `sl_viewer_ui_core::scrollbar`, in both
orientations, and every scrolling surface that had no bar has one.

The widget is the reference's four parts: a frame (`.sk-scrollbar-vertical` /
`-horizontal`) holding the start arrow, the groove (`.sk-scrollbar-track`) with
its thumb, and the end arrow. `spawn_scrollbar` is the vertical bar, for a
windowed list (`ScrollTarget::List`) or a `ScrollPosition` container
(`ScrollTarget::Container`); `spawn_horizontal_scrollbar` the horizontal one,
containers only; `scrollbar_corner` the square where the two meet.

- **The arrows are always spawned and the skin shows them.**
  `.sk-scrollbar-arrow { display: var(--scrollbar-arrows) }` — `none` in both
  flat skins, `flex` in Graphite's **Relief** theme. The first tokens that feed
  a keyword property; the resolve tests pin that the `var()` parses there.
- **The ends take their length out of the groove.** The thumb is sized against
  the groove's measured length — bevy's `Scrollbar` did that for a container
  already, and the list driver does now — so the scroll range is untouched and
  the thumb stops on the end arrow.
- **Press steps, hold repeats.** A new `hold_repeat` module: `HoldToRepeat` on
  a `bevy_ui_widgets` `Button` + `ActivateOnPress` re-fires `Activate` after
  the reference's 0.5 s `held_down_delay`, at a fixed 20 per second rather than
  the reference's once per frame, so the button's own observer is its whole
  behaviour. A step is one row for a list and 16 px (`VERTICAL_MULTIPLE`) for
  a container.
- **A horizontal bar is physical.** It maps `ScrollPosition.x` and bevy places
  its thumb from the left, so its ◀ stays on the left under RTL
  (`keep_horizontal_bars_physical` reverses the row the scaffold would
  otherwise mirror).
- **Every bar hides while there is nothing to scroll** — the widget's
  default, not an opt-in: a thumb filling its whole groove says nothing. A
  container's bar also hides while the container is itself `Hidden` (an
  unselected tab page). `Visibility`, so the content never reflows by a bar's
  width at the threshold. Its runtime half registers itself from every widget
  plugin that spawns a bar (`ensure_scrollbar_widget`, from the list's and the
  tab strip's plugins): the first live look found the conversation strip's
  bar full-length and showing, because the viewer had never registered it.
- **The glyphs are the skin's**, the checkbox tick's way: an empty text host
  whose `::before` `content` is `▲` / `▼` / `◀` / `▶` in `common.css`.

Tokens: `--scrollbar-track` (its own role now, not the slider's `--track-bg` —
the reference names `ScrollbarTrackColor` separately),
`--scrollbar-thumb-hover`, `--scrollbar-arrows`, `--scrollbar-arrow-bg` /
`-hover`, `--scrollbar-arrow`, and `--scrollbar-thickness` — a classic bar is 15
px and a 10 px arrow is barely a target. The windowed list reads the bar's
laid-out width back for its row gutter, so a thicker bar moves the rows rather
than covering them. Relief sets 14 px.

### Where the bars are

Already on it through the table widget: all 21 table consumers and the RLV
console. Converted from a hand-built `bevy_ui_widgets` bar: the vertical tab
strip, a filling tab page, the gallery page, the avatar profile's group list and
the About box (the last two in colour constants no skin could reach).

**Given a bar where there was none** (wheel only before):

- windowed lists — the inventory, the groups list, object contents, the emoji
  grid (its viewport widened by the bar, and its cells now shrink rather than
  lose the ninth column to a thicker skin's bar);
- containers, each now a row of [content, bar] — the
  conversation tab strip (the width and persistence key moved to the row, so
  the divider resizes both), the conversation transcript, the search details
  pane (the row is what the selection shows and hides), the world map's
  results, the group picker, the texture picker's tree, the script editor's
  diagnostics, the wearable editor's parameter list and the inventory gallery's
  grid (which wraps, so it scrolls on one axis now, not two);
- horizontal — the UI gallery's page, the one surface that genuinely scrolls
  sideways (a wide card, a long translation), with the corner square.

### The tab strip's overflow buttons

A **horizontal** tab strip that overflows now grows the reference tab
container's four buttons (`mJumpPrevArrowBtn`, `mPrevArrowBtn`,
`mNextArrowBtn`, `mJumpNextArrowBtn`) instead of its old two: jump to the first
tab, back one, on one, jump to the last. A step moves by a **tab** — the
reference's `mScrollPos` counts tabs — lining the next tab's leading edge up
with the strip's (`tab_scroll_target`), and back / on repeat while held. The
glyphs are the skin's (`.sk-tab-scroll-*`), named in reading order and turned
round under `:root[dir="rtl"]`, which retires `apply_tab_arrow_glyphs`;
`--tab-jump-buttons` (a `display` value, `flex` in every shipped skin) lets a
skin drop the two jumps; `--tab-scroll-bg` / `-hover` / `--tab-scroll-arrow`
dress them.

A **vertical** strip keeps the scrollbar (decided with the user, 2026-09-24):
the reference gives it ▲ / ▼ step buttons instead, but a bar also says where
in a long list you are and drags, and a skin that wants the step buttons turns
on the bar's ends.

### A defect the resolve test found

**No container scrollbar's thumb was ever skinned.** `bevy_flair` makes
`Styled` a required component of `Node` and `TextSpan` only, and a
`bevy_ui_widgets` `ScrollbarThumb` has no `Node` (bevy places it by hand after
layout) — so the `.sk-scrollbar-thumb` class on the tab strip's, the gallery's
and every tab page's thumb resolved nothing, and they all drew the Rust
fallback in every skin. The thumb now carries `Styled` itself.

### Tests

- `ui_table` scenarios (the real pointer stack): the thumb drag with the ends
  off *and* on; the thumb starts under the up arrow and stops on the down arrow
  with the scroll range unchanged; a press steps one row, the up arrow steps
  back, a held arrow repeats and letting go stops it (clock driven by hand); a
  flat bar's ends are not laid out.
- `ui_tab`: the overflow buttons walk a 24-tab strip by tabs (a tab flush with
  the leading edge after *on*, the last tab at the trailing edge after *last*,
  *back* held keeps stepping and stops on release, *first* returns to 0), the
  pure step table in both directions, and a vertical strip's bar hidden at two
  tabs and shown at twenty-four.
- `skin_palette_resolves`: a fixture (`vintage-scrollbar.css`) turns the ends
  on, thickens the bar and recolours all four parts by tokens alone; a
  horizontal bar takes the thickness as its height and draws ◀ / ▶; the tab
  glyphs turn round under `dir="rtl"` and the fixture can drop the jumps; both
  flat skins keep the plain 10 px bar.
- `scrollbar` / `hold_repeat` units: the container step clamps both ends on
  either axis; the hold schedule.

### Not done

- **Multi-line text fields have no bar**: notecards, the script body, profile
  text. They scroll themselves to follow the caret, but the offset is the
  editor's own rather than a `ScrollPosition`, so they need a third
  `ScrollTarget` — [[viewer-multiline-editor-scrollbar]].
- **No table scrolls sideways**, where the reference's scroll lists grow a
  horizontal bar when the columns outrun the width — noted in the same task.
- **RTL horizontal scrolling is untested live.** Taffy reports content
  overflow only in the positive direction, so a right-to-left row that
  overflows to the left may not be scrollable at all — the tab step logic
  handles RTL by construction (unit-tested), but whether bevy lets the strip
  get there is unverified.
- **No image-backed arrow art.** `-bevy-image` on `.sk-scrollbar-arrow` works
  the way it does on `.sk-button` (the arrow is a real `Node`), but Relief
  ships no art for it, so its arrows are the glyph on a flat face.
- **Top Objects' width floor** still adds the unskinned 10 px for the bar: it
  is computed when the window is built, before any stylesheet is asked.
- **Clicking the groove** pages a container (bevy's widget does it) but not a
  windowed list, which never did.
