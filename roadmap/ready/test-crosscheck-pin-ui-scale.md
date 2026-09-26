---
id: test-crosscheck-pin-ui-scale
title: A chrome pair pins one UI scale on both viewers
topic: test
status: ready
origin: viewer-vintage-ui-chrome-crosscheck recorded pair (2026-09-27)
refs: [viewer-vintage-ui-chrome-crosscheck, viewer-vintage-skin,
       test-crosscheck-ui-scenes, test-firestorm-harness-skin-selection]
---

Context: [context/testing.md](../context/testing.md),
[context/vintage-skin.md](../context/vintage-skin.md).

The first recorded Vintage chrome pair (`--capture-ui --capture-size
3840x2160 --sl-client-skin vintage --firestorm-skin vintage`, catalogue scene)
draws the two interfaces at different scales in frames of the same size. Our
menu bar is **38 px** tall and the reference's is **19 px**, and text and
buttons scale with it. Until both are drawn at one scale, no shape or spacing
claim about a skin can be read off a pair.

## Not a viewer bug

Supporting UI scale is intended behaviour, and ours follows the output's.
Firestorm has no Wayland support and runs through Xwayland, where it sees a
scale factor of 1.0. Ours is a native Wayland client on the same output
(3840×2160 at **scale 1.5**) and scales its interface to match. Each viewer
is behaving correctly for itself; the pair differs because nothing tells both
the same scale. That makes this a cross-check problem, like the field of view
was before `--fov`.

The output's 1.5 covers most of the gap but not all of it: 38 / 1.5 ≈ 25 px
against 19 px. Measure our bar at scale 1.0 before deciding whether our base
sizes (font size, menu-bar padding) account for the rest.

## The work

- A capture-block variable, `SL_VIEWER_CAPTURE_UI_SCALE`, and
  `sl-crosscheck --ui-scale`, read by both viewers:
  - Firestorm: `UIScaleFactor`, forced non-persistently, as `CameraAngle` is
    for the FOV.
  - Ours: the effective UI scale for the run, overriding the window's scale
    factor for the capture without rewriting the saved preference (the
    `CameraFovOverride` pattern).
- Unset, a UI run warns when the two viewers' effective scales differ. Better
  still, it defaults to one value on both sides, since a pair at two scales is
  never what a chrome comparison wants.
- Both scene dumps report the effective UI scale in `render`, and
  `sl-crosscheck-report` lists it among the settings that decide what a frame
  can contain.

## Done when

A chrome pair on this machine draws both menu bars at their skins' height for
one stated scale, and the report shows that scale for both viewers.
