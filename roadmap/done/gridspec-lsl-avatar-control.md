---
id: gridspec-lsl-avatar-control
title: LSL library behaviour on each grid — animations, sitting, controls and camera
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, server-lsl-lib-avatar-control]
blocked_by: [gridspec-lsl-live-differential-runner]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

No aditi or OpenSim measurements for this tranche beyond the library table's
documented values.

## Discover

Probes for: Permission prompts, `llTakeControls`, camera parameters, animation
overrides (second avatar). Run on OpenSim YEngine / XEngine and aditi.

## Document

`book/src/gridspec/lsl.md` § Animations, sitting, controls and camera.

## Fake grid

Large — [[server-lsl-lib-avatar-control]].

## Viewer

Permission card, control capture, scripted camera.

## Done (2026-10-24)

- **Discover.** Five probes, `sl-lsl-runtime/probes/avatar/`:
  `permissions` (the question, partial / denied / over-granted answers, a
  mask of 0, `CONTROL_CAMERA` unseated, a question to the other avatar,
  calls made without a grant), `sit` (sit target and camera offsets, the
  seated avatar's implicit grants, `llUnSit`, the other avatar sitting, a
  cleared target), `controls` (take, press, release, accept off,
  `llReleaseControls`), `camera` (refused standing, granted seated, in- and
  out-of-range rules, clear, tracking) and `animation` (start, stop, a
  missing name, the standing override with a built-in, the other avatar).
  All complete on aditi, YEngine and XEngine; the two OpenSim engines agree
  on every row.
- **Runner.** Four new drives — `answer grant=<mask>`, `sit`, `stand`,
  `control keys=<mask>`, each `by=owner|other` — and notes of what the grid
  sends the two viewers meanwhile (questions, sit responses,
  `ScriptControlChange`, follow cameras, built-in animation lists, alerts,
  stray owner-say lines), with both avatars' names scrubbed to placeholders.
- **Document.** `book/src/gridspec/lsl.md` § Animations, sitting, controls
  and camera (a new chapter, linked from `content/scripts.md`); the drives
  and notes in `book/src/tools/lsl-probes.md`; the seated-animation row of
  `gridspec/movement.md` corrected (aditi now sends the built-in `sit`).
- **Fake grid.** Large, as planned: [[server-lsl-lib-avatar-control]] now
  carries the measured rows a flavour must split on. Nothing small: every
  divergence is in what a script does, and the fake grid runs none.
- **Viewer.** `e2e_script_permissions` holds the permission card to both
  fake flavours (*Yes* grants the mask asked, *No* answers 0); the
  question's shape is the same on both live grids, and where they differ
  Second Life asks nothing. Control capture and the scripted follow camera
  are the existing [[viewer-input-script-control-capture]] and
  [[viewer-scripted-followcam-llsetcameraparams]], which now carry the
  measured shapes (Second Life's release-all and nudge bit, unclamped
  camera rules, camera control only for a seat or attachment).
