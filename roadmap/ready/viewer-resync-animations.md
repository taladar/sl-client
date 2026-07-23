---
id: viewer-resync-animations
title: Resync animations action
topic: viewer
status: ready
origin: main-menu survey (2026-07-23)
refs: [viewer-p18-3, viewer-render-animation-coverage]
---

Context: [context/viewer.md](../context/viewer.md).

World ▸ Resync Animations (Ctrl+S in Firestorm): restart all currently
playing animations so out-of-sync looped animations — couple dances,
furniture sync, machinima shots — realign to a common start time.

Scope:

- Enumerate the active animations on all rendered avatars and restart
  their motions from time zero locally (the reference implementation is
  client-side: it resets each motion's activation timestamp; no wire
  traffic).
- Menu entry + rebindable shortcut; safe to invoke repeatedly.

Reference (Firestorm, read-only): `Tools.ResyncAnimations`
(`menu_viewer.xml` World section) → `fs_resync_animations` handling in
the FS agent/motion glue.

Builds on: the skeleton animation driver ([[viewer-p18-3]], done).
