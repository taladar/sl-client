---
id: viewer-ui-tab-widget
title: Reusable tab widget (horizontal + vertical)
topic: viewer
status: done
origin: noticed while building viewer-inventory-folder-tree, whose tabs are
  ad-hoc (2026-07)
blocked_by: [viewer-ui-widget-scaffold]
refs: [viewer-inventory-folder-tree, viewer-preferences-floater]
---

Context: [context/viewer.md](../context/viewer.md).

A **reusable tabbed container**: a strip of tab buttons that switches which one
of a set of panels is shown, with exactly one active. The reference viewer uses
both layouts, so the widget must support **both**:

- **Horizontal** tabs along the **top or bottom** edge (floater tabs).
- **Vertical** tabs along the **left or right** edge (the sidebar / preferences
  style).

Per the UI conventions it sizes to content (no fixed rects), reflows on a
font-size / locale change, and the left/right placement **mirrors under RTL**
([[viewer-ui-skin-tokens]] / the scaffold's `UiDirection`). Active-tab state,
`Tab`/arrow focus, and show/hide via `UiPanelShown` come from the scaffold.

**Adopt it in the existing ad-hoc tabbed panels:** the inventory window
([[viewer-inventory-folder-tree]], whose Everything / Recent / Worn tabs are
hand-rolled today) and the preferences floater ([[viewer-preferences-floater]]).

Reference (Firestorm, read-only): `lltabcontainer`.

## What landed (2026-07-19)

`crate::ui_tab`. Two constructors over a `TabSpec`: `spawn_tab_strip` (a bare
single-select strip — all the inventory needs, since its three tabs drive one
shared list) and `spawn_tab_container` (strip + one panel per tab, only the
active shown). The strip is a `bevy_ui_widgets` `RadioGroup` of `RadioButton`s,
so exactly-one-active, arrow-key navigation and `Tab` focus (the group is the
one focus stop, per the ARIA tablist pattern) come from upstream; we own the
`Checked` state, keyed off the single source of truth `TabStrip::active`.
Placement is logical (`BlockStart`/`BlockEnd`/`InlineStart`/`InlineEnd`), so the
vertical strip mirrors under RTL for free — and `InlineEnd` is a first-class
LTR-usable placement the reference (`TOP`/`BOTTOM`/`LEFT` only) cannot express.

**Adopted in the inventory window** (the ad-hoc Everything/Recent/Worn button
row is gone; a `bridge_tab_selection` system turns the strip's selection into
the existing `SelectTab` action). The preferences floater is still blocked, so
not adopted there yet. The gallery registers **one element per placement**
(`tabs-top`/`-bottom`/`-leading`/`-trailing`), all swept by `crate::ui_test`
across every script, direction, scale and font size.

**Beyond the brief — a resizable divider for vertical tabs (requested during
implementation).** A `TabSpec::strip_width` pins a vertical strip to a width,
clips over-long labels (declaring `TextMayClip`) and adds a draggable divider so
the split can be moved — for data-valued tabs like group or avatar names that
would otherwise force the strip absurdly wide. The width is a `TabStripWidth`
component (one source of truth: the drag writes it, `apply_tab_strip_width`
reflects it to the node) and is **persisted per host floater** in
`crate::floater_persist` (`{floater}_{widget}_split`), sharing the geometry
dirty-clock and flush. The drag sign folds in placement × direction, so the
handle is correct under a mirrored layout with no per-side code. The gallery
elements stay content-sized; the divider is exercised by unit tests (no live
vertical-tab floater consumer exists yet — that lands with
[[viewer-preferences-floater]] or the first data-tab window, which will be its
first real persistence exercise).

**Beyond the brief — the rest, all requested during implementation and reviewed
in the gallery:**

- **Overflow scrolling** (reference `LLTabContainer` parity). A strip is bounded
  to the available space (a horizontal strip to `PANEL_MAX_WIDTH`, a vertical
  one to `TAB_STRIP_MAX_HEIGHT` — a content-sized container otherwise just grows
  to fit every tab and nothing ever scrolls). The buttons live in a scroll
  viewport; when they overflow, a control appears
  **from available space, not a flag** (`apply_tab_scroll_controls` measures the
  viewport post-layout): a vertical strip gets a draggable `bevy_ui_widgets`
  scrollbar (plus wheel scroll), a horizontal one gets `◀`/`▶` arrow buttons at
  the trailing edge (direction-aware glyph and scroll sign). The gallery hosts
  few-vs-many demo pairs.
- **Single-line labels + truncation.** A tab label is `LineBreak::NoWrap`; a
  clipped one shows a trailing ellipsis only when actually truncated
  (`apply_tab_ellipsis`). The ellipsis is **configurable** (`TabSpec::ellipsis`,
  default `…`) because the convention is a translator's call — CJK uses `……`;
  the i18n scaffold task ([[viewer-i18n-fluent-scaffold]]) is annotated to
  source it. The label sits in a clip *container* (a node clips descendants, not
  its own glyphs), and leading-alignment + the ellipsis side both come from the
  container's mirrored flow, so RTL clips the end and puts the `…` on the left.
- **Stable sizing.** Panels are grid-stacked in one cell with `Visibility`
  toggling (not `Display::None`), so the widget stays the size of the *largest*
  tab rather than shrinking when a lighter tab is selected.
- **Reference tab shape.** Rounded corners only on the edge away from the
  content (`apply_tab_corner_radius`, RTL-mirrored for inline strips); the
  active tab shares the panel's shade so it reads as merging into its content,
  with a bright active border. Fixed a latent bug where the *initial* active tab
  was never highlighted (only after the first switch).
- **A real `bevy_ui` picking bug, fixed here.** Scrolled-out tabs stayed
  clickable and, scrolled far enough, covered a sibling widget — clicking there
  switched the scrolled widget. `clip_check_recursive` stops at the first
  `Overflow::Visible` ancestor, so a tab *label* (its own pick target) under a
  non-clipping button was picked outside the viewport. Fix: tab buttons clip, so
  the label's clip chain reaches the viewport. Pinned by a regression test.

Also fixed in the gallery while exercising the above: mouse-wheel scrolling (its
page had `overflow: scroll` but no bound and no wheel handler, so it never
scrolled) and a sticky key-legend bar that stays put while the list scrolls.
