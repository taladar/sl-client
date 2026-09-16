---
id: viewer-floater-z-climbs-over-fixed-planes
title: A window's z-order climbed without end, over the lists, bars and menus
topic: viewer
status: done
origin: found while investigating [[viewer-combo-stops-opening]] (2026-09-16)
refs: [viewer-combo-stops-opening]
---

Context: [context/viewer.md](../context/viewer.md).

The floater manager raises a window on **every press** inside it
(`FloaterOp::BringToFront`, plus a second one from each chrome button) by
handing out the next value of a high-water mark, `FloaterZTop`. The mark only
ever went up; its doc said `i32` was "far more headroom than any session of
clicks could exhaust". Headroom was never the limit: everything painted *over*
the windows sits on **fixed** planes of the same number line — the top and
bottom bars at 9 000, the toast channel and every combo's list at 9 500, the
menus at 10 000.

So a long enough session put windows over the bars, then over their own
combos' lists (the press builds a list under its window, the next press
closes the invisible list, and the combo reads as dead until a restart), then
over the menus. It takes thousands of presses, so no short session showed it —
which is also why it is not the cause of [[viewer-combo-stops-opening]].

Reproduced headless before the fix: a combo in a floater whose mark sits at
9 600, clicked through the real pointer stack, opens its list and the click on
an option lands on the window instead.

## Fix

- `FloaterZTop::next` stops at `FLOATER_Z_CEILING` (`BOTTOM_BAR_Z - 1`) — a
  backstop for a single frame's raises.
- `renumber_floater_z`, chained after `raise_floaters_on_open` and before the
  UI stack pass, packs the free windows back down to `1..=N` in their current
  order once the mark passes `FLOATER_Z_RENUMBER_AT` (4 096). A never-raised
  window (0) and a docked one (its host's plane) are left alone.
- Each raise logs its z at debug under `sl_viewer_ui_widgets::floater`.

Tests (`sl-viewer-ui-widgets/src/floater.rs`):
`a_combo_list_opens_over_a_window_raised_all_session` (through hit-testing;
fails with the bound disabled),
`a_long_session_of_raises_is_packed_back_down_in_order`,
`the_mark_never_climbs_over_the_bars`.
