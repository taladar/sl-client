---
id: viewer-name-tags-lost-to-probe-cameras
title: Avatar name tags (and render priority, and object pick) lost to the probe cameras
topic: viewer
status: done
origin: spotted while live-testing viewer-p34-3 on aditi
refs: [viewer-p33-2, viewer-p34-3]
---

Context: [context/viewer.md](../context/viewer.md).

**Done.** Avatar **name tags** (with the distance annotation) had stopped
rendering entirely. Nothing about the name-tag code changed: it was knocked out
by [[viewer-p33-2]], which spawns a `Camera3d` **per probe-capture face**
(`ProbeCaptureCamera`). Any system resolving "the" camera with an unqualified
`single()` then fails — `Query::single` errors when *several* entities match —
and returns early, every frame, silently.

Three sites had the bug; all now qualify the query with `With<FlyCamera>` (the
main viewpoint), as `water` / `lights` / `sky` already did and as `reach` had
already worked around:

- `position_name_tags` (`avatars.rs`) — no name tag was ever positioned or
  shown.
- `drive_render_priority` (`render_priority.rs`) — the whole periodic
  re-prioritisation (texture discard levels + mesh/prim LOD targets by screen
  coverage) was dead, so nothing was being re-prioritised as the camera moved.
  The costlier of the two: this one is not cosmetic.
- `pick_object` (`objects.rs`) — the `P` debug pick could not resolve a camera.

**Watch for this whenever a feature adds a camera.** A `Camera3d`/`Camera` query
that means *the viewpoint* must say so (`With<FlyCamera>`); an unqualified one
compiles, type-checks, and silently degrades to a no-op the moment a second
camera exists anywhere in the world.
