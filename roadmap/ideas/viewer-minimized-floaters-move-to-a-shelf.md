---
id: viewer-minimized-floaters-move-to-a-shelf
title: A minimized floater should move to a shelf of minimized windows, not collapse where it stands
topic: viewer
status: ideas
origin: the user comparing minimize against the reference while checking viewer-skin-glyphs-from-content (2026-09-24)
refs: [viewer-floater-minimize-caps-follow-no-pattern, viewer-vintage-bottom-bar]
---

Context: [context/viewer.md](../context/viewer.md).

## Observation

Our minimize collapses a floater **in place**. The content goes and the title
bar stays exactly where the window was. In the reference a minimized floater
also **moves**: its title bar goes to a slot with the other minimized
windows, and restore puts the window back where it was.

## What the reference does

`LLFloater::setMinimized(true)` (`llui/llfloater.cpp`) saves the expanded
rect, then moves the floater to `LLFloaterView::getMinimizePosition`. That is
the first free cell in a grid of `UIMinimizedWidth` (160 px) by one header
height, scanning the snap rect. Two layouts, chosen by Firestorm's
`FSLegacyMinimize`:

- **off (the default):** columns from the **top left**, down each column
  first, offset below the navigation / favourites bars when those are shown;
- **on:** rows from the **bottom left**, across each row first. This is what
  the user saw, so their install has it on, or the skin they compared
  against implies it.

Further rules to port along with it:

- a floater **dragged while minimized** remembers that spot
  (`mHasBeenDraggedWhileMinimized`) and minimizes back to it next time,
  instead of asking for a slot;
- a minimized floater is drawn at the minimized width, not its expanded width;
- minimizing a floater minimizes its dependents too, or hides those that
  cannot minimize;
- restore returns the window to the saved expanded rect.

## Open question, to decide before implementing

Where the bottom-left shelf lives. The reference puts it just above its bottom
toolbar. We removed the gap between our bottom bar and the Conversations
area in that corner, so no free strip remains there to put minimized title bars
in. Options to weigh:

- the reference's default top-left column, which avoids the corner;
- a shelf strip that appears above the bottom bar only while something is
  minimized, and pushes or overlays the Conversations area;
- a shelf inside the bottom bar itself (an existing viewer idiom, but not the
  reference's);
- both reference layouts, behind a preference mirroring `FSLegacyMinimize`.

Whichever option is chosen, the slot scan must stay inside the floater snap
rect: the screen minus `ScreenChrome`, as `clamp_floaters_on_screen` uses
it.

## Done when

Minimizing a floater moves its title bar to a predictable slot shared with the
other minimized windows. Restoring puts it back where it was. The placement
works with the bottom bar as it is now.
