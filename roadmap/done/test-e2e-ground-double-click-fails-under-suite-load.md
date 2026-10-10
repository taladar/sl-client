---
id: test-e2e-ground-double-click-fails-under-suite-load
title: The ground double-click e2e test flies its camera by frames and failed once in the full suite
topic: test
status: done
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

## Again (2026-10-09)

Failed in the same full-suite run as
[[test-scripted-environment-change-marker-timeout]]'s latest, after 176 s,
in process, with the three `ui_contract` tests past 300 s beside it; it had
passed an hour earlier among the viewer's packages alone.

## Fixed (2026-10-10)

Why no teleport followed, which was the unknown: the double click was not
one. The automation plays it a step a frame, so its two presses are two
frames apart, and the viewer's world gesture asked for them within 0.4 s of
its own clock. At the five frames a second measured in the failing run that
is 0.4 to 0.5 s — a second single click. A person's double click is as
fast as their hand whatever the frame rate, so this was the input's
fault and not the gesture's. The widgets' click counter has the same
shape (wall clock, half a second) and the same exposure.

- A synthetic double click now pins the app's multi-click interval for as
  long as it plays (`InputAction::double_click`, to `FOREVER`), the mirror
  of what a single click already did to keep two of them from merging.
- The world double-click reads that same interval
  (`PickingSettings::multi_click_interval`) instead of a 0.4 s of its own,
  so the pin reaches it, two single clicks on one spot are never a
  teleport, and the world and the widgets agree on what a double click is.
  For a person the window is now the half-second the widgets use.
- The test parks its flycam by distance: two frames at a time until the eye
  has flown three metres, where thirty frames were five metres on an idle
  machine and sixty-one on a busy one.

New tests: `a_double_click_counts_two_whatever_the_frames_took` (testkit —
an app whose interval is zero stands in for the slow machine; it fails
without the pin) and `a_double_click_is_one_however_long_its_frames_took`
(the pin and its restoring).

Not shown: the e2e test failing before and passing after under one load.
Four runs with every core busy passed after the fix, but that load did not
reach five frames a second, so it would have passed before too. The world
gesture's own reading of the interval has no test of its own below the e2e
one.
