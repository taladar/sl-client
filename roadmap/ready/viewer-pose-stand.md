---
id: viewer-pose-stand
title: Pose Stand floater
topic: viewer
status: ready
origin: main-menu survey (2026-07-23)
refs: [viewer-poser, viewer-animation-overrider]
---

Context: [context/viewer.md](../context/viewer.md).

Avatar ▸ Pose Stand…: freeze the own avatar into a neutral T/A-pose by
suspending the AO and idle animations — the standard way to fit rigged
clothing and attachments or take reference photos without idle motion.
Distinct from the full joint [[viewer-poser]].

Scope:

- A small on/off floater; while on, stop/suppress the active animation
  set (including the AO, [[viewer-animation-overrider]]) and play the
  reference pose-stand animation.
- Pose choice (T-pose / A-pose variants) as in the FS floater.
- Restore the previous animation state (AO resumes) on close.

Reference (Firestorm, read-only): `Floater.Show fs_posestand`
(`menu_viewer.xml` Avatar section), `fsposestand` floater.

Builds on: the animation driver (done); interacts with the AO when that
lands.
