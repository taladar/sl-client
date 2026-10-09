---
id: test-e2e-ground-double-click-fails-under-suite-load
title: The ground double-click e2e test flies its camera by frames and failed once in the full suite
topic: test
status: bugs
origin: gridspec-terrain commit runs (2026-10-08)
refs: [gridspec-terrain, viewer-automation-ground-aim,
  test-scripted-environment-change-marker-timeout]
---

Context: [context/testing.md](../context/testing.md).

## Observation

`e2e_double_click`'s `a_ground_double_click_lands_there_and_keeps_the_camera`
failed in the first of three pre-commit runs of the full suite on
2026-10-08, after 162 s: the agent was still at 128, 128 and the teleport
state idle, where the double-click had landed on 142, 132. It passed in the
other two runs and alone (14 to 17 s).

The readout of the failing run has the flycam's eye at 66.9, 128.1, 42.0:
61 m from the avatar. The test parks the camera by holding the backward key
for `FLY_FRAMES` (30) frames, which at the flycam's 10 m/s and the headless
viewer's 60 Hz is 5 m.

## What is known

A flight counted in frames covers a distance that depends on how long the
frames took. Sixty-one metres in thirty frames is frames of 0.2 s: the
viewer was running at about 5 Hz in that window, where three long UI
contract tests and four GPU avatar tests were running beside it.

From 61 m the double-click's point was still found (the action returned a
hit), and the agent did not move.

## Not known

Why no teleport followed: whether the double-click's second press came too
late to count as one at 5 Hz, or the gesture refuses a point that far from
the camera, or the request went out and was lost.

## What to do

Park the camera by distance, not by frames — fly until the eye has moved
`FLOWN` metres, or place it — so the test aims from where it means to
whatever the frame rate. Then see whether a double-click still fails at a
low frame rate, which would be the viewer's bug and not the test's.
