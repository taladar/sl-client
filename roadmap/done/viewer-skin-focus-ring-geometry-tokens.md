---
id: viewer-skin-focus-ring-geometry-tokens
title: The focus ring's shape becomes a value, not three repeated literals
topic: viewer
status: done
origin: asked while reviewing viewer-skin-search-box-focused-fill (2026-09-24)
points: 1
refs: [viewer-skin-search-box-focused-fill, viewer-ui-focus-ring-visible,
  viewer-ui-skin-tokens]
---

Context: [context/viewer.md](../context/viewer.md).

The ring's **colour** has been `--focus-ring` since it was written; its
**geometry** was `outline-width: 2px` / `outline-offset: 1px` spelled out by
each of the three rules that ring — `.sk-focusable:focus-visible`, a focused
editor (`.sk-text-field:focus`), and now a focused search box
([[viewer-skin-search-box-focused-fill]]). A skin could still change it, since
a skin's own rules beat the `common.css` it imports into a layer, but only by
**restating all three rules** and knowing they are three.

That is the wrong shape for a skin vocabulary: a ring is as much a shape as a
colour — a classic skin draws a 1 px hairline tight against the control where a
modern one draws a 2 px halo standing off it — and the question a skin author
asks is "how thick", not "which three selectors".

## Done (2026-09-24)

`--focus-ring-width` and `--focus-ring-offset`, defined in the fallback sheet
and both shipped skins at the values the literals had (2px / 1px), so nothing
moves in a graphite or azure viewer. All three ringing rules read them, as does
the resting offset of the two baselines. The resting **width** stays a literal
`0px`: that is not a choice a skin makes, it is what "not focused" means.

**The test is a fixture, not an assertion against the shipped values.** Both
skins give the tokens the numbers the literals had, so neither can tell a live
token from a literal that happens to agree with it — and a `var()` bevy_flair
failed to parse in a *length* property leaves the ring at nothing with no log,
which is exactly the silent failure the palette tests exist for. So
`a_skin_can_reshape_the_focus_ring_by_value` loads a test-only
`hairline-ring.css` that sets 1px / 0px and touches nothing else, and reads the
ring back off a focusable widget **and** a search box — the second because a
pair of tokens that only half the rules read is a skin that reshapes the ring
in some places and not others.
