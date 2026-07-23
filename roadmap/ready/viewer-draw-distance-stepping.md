---
id: viewer-draw-distance-stepping
title: Progressive draw-distance ramp after teleport
topic: viewer
status: ready
origin: debug-settings/chat-lines survey (2026-07-23)
refs: [viewer-p1-4, viewer-teleport-flow-progress]
---

Context: [context/viewer.md](../context/viewer.md).

Firestorm's draw-distance stepping trades a long post-teleport stall for
progressive loading: on region entry the effective draw distance starts
small and steps up to the user's target over timed intervals, so nearby
content rezzes first and the scene stays responsive.

Scope:

- On teleport/region entry with stepping enabled
  (`FSRenderFarClipStepping`), set the effective far clip low and step it
  up on a timer (`FSRenderFarClipSteppingInterval`) until the saved
  target (`FSSavedRenderFarClip`) is restored.
- Each step re-issues the agent draw-distance update the handshake
  already sends ([[viewer-p1-4]] wired the wire side) so interest-list
  and fetch priority follow.
- Cancel the ramp if the user changes draw distance manually mid-ramp.

Reference (Firestorm, read-only): the `FSRenderFarClipStepping*`
settings and their consumer in the FS render/teleport glue.

Builds on: the draw-distance wire plumbing (done) and the teleport flow
([[viewer-teleport-flow-progress]] for the surrounding UX).
