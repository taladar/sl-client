---
id: viewer-render-matrix-hud-over-world
title: Render matrix — a prim on a HUD composited over a prim in the world
topic: viewer
status: blocked
origin: viewer-render-context-matrix (2026-11-08) — curated pair not built
points: 2
refs: [viewer-render-context-matrix, viewer-hud-attachments-not-composited]
blocked_by: [viewer-render-hud-and-actor-axes]
---

Context: [context/testing.md](../context/testing.md).

The "HUD over world" curated pair: a subject on `Screen::Hud` in front of
an opaque world prim at the same screen position. The HUD must draw over
the world prim whatever their world depths are, because it composites after
the world frame. The world prim must still show around the HUD subject.
The HUD regression in [[viewer-hud-attachments-not-composited]] (built,
seated, visible, never in the frame) is the failure this pins.

- Add the cell to `CURATED` once [[viewer-render-hud-and-actor-axes]] gives
  `ContextSet` its `screen` slot.
- Teeth: the same cell with the HUD camera's order below the world camera's
  must lose the subject.
