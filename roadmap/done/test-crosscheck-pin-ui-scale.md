---
id: test-crosscheck-pin-ui-scale
title: A chrome pair pins one UI scale on both viewers
topic: test
status: done
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

## Findings (2026-09-27): the premise was wrong

Both viewers already drew the recorded pair at an effective scale of **1.0**.
A UI capture routes our UI camera into the off-screen capture image, and an
`ImageRenderTarget` made from a handle has `scale_factor` 1.0, so bevy_ui's
scale is 1.0 × `UiScale` (1.0 in a fresh run directory). The output's 1.5
never reaches a capture. Firestorm's is `UIScaleFactor` (1.0) ×
`getSystemUISize()` (1.0 under Xwayland). The 38 px against 19 px is therefore
our own layout, filed as [[viewer-menu-bar-twice-reference-height]]. It cannot
be closed by pinning a scale.

## Built

- `SL_VIEWER_CAPTURE_UI_SCALE`, read by both viewers and refused outside
  0.75–2 (our `UiScale` range):
  - ours: `--capture-ui-scale`, a run-scoped `UiScaleOverride` resource that
    `apply_ui_scale` prefers over the stored preference, which it leaves
    alone;
  - Firestorm (fork `test-harness`, `65df871d0a`): `UIScaleFactor` forced
    non-persistently before the window exists, with `ResetUIScaleOnFirstRun`
    forced off so a fresh user directory cannot reset it.
- `sl-crosscheck --ui-scale`. A `--capture-ui` run that names none pins 1
  (`DEFAULT_UI_SCALE`), which replaces the planned "warn when they differ".
  The summary prints "interface drawn at UI scale N in both viewers", and
  `run.json` records it.
- Both scene dumps report `render.ui_scale`: ours read back from a UI root's
  `ComputedUiRenderTargetInfo`, theirs from `LLUI::getScaleFactor()`.
  `sl-crosscheck-report` lists it with the other render settings.

## Verified live

A Vintage chrome pair at 3840×2160:

| `--ui-scale` | dumps' `ui_scale` | our bar | reference's bar |
| --- | --- | --- | --- |
| unset (pins 1) | 1.0 / 1.0 | 38 px | 19 px |
| 1.5 | 1.5 / 1.5 | 57 px | 28 px |

The report shows `ui_scale: 1.0000 vs 1.0000`.

The original "done when" (both menu bars at their skins' height) now belongs
to [[viewer-menu-bar-twice-reference-height]].
