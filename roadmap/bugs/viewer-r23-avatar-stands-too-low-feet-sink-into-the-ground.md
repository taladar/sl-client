---
id: viewer-r23
title: Avatar stands too low — feet sink into the ground
topic: viewer
status: bugs
origin: VIEWER_ROADMAP.md — Known rendering issues (to fix)
---

Context: [context/viewer.md](../context/viewer.md).

**R23. Avatar stands too low — feet sink into the ground.** Our viewer
renders the avatar with its feet buried below the terrain surface; in
Firestorm the same avatar's feet rest *on* the ground. The avatar root is
placed at too low a Z by roughly the ankle-to-sole height, so the whole body
is offset downward. Candidates: a missing hover-height / foot-to-root offset
(the reference positions the avatar so the *soles* meet the ground, not the
pelvis-derived root), or the base-mesh / collision-volume foot offset not
applied. Cosmetic but consistently visible. **Open.**
