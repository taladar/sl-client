---
id: viewer-p10-1
title: Spheres
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 10 — Avatar placeholders
---

Context: [context/viewer.md](../context/viewer.md).

**P10.1. Spheres.** Track avatars from `ObjectAdded` (pcode 47) and
`CoarseLocationUpdate`; render each as a ~2 m UV-sphere `StandardMaterial` at
the (converted) position; despawn on removal or when dropped from the coarse
locations. No rig, baked textures, or animation. Verify with a second
logged-in avatar. **Done.** A new `avatars.rs` module owns an `AvatarState`
resource keyed by `AgentKey`, fed by two independent systems chained after the
object/texture pipeline: `update_avatar_objects` folds the `ObjectAdded` /
`ObjectUpdated` / `ObjectRemoved` stream for `pcode == 47` objects (the
precise, per-frame source — including the agent's own avatar) into one
placeholder sphere per avatar, and `update_coarse_avatars` renders a sphere
for every *coarse-only* avatar in each `CoarseLocationUpdate` (one already
tracked as a full object is skipped, and the agent's own `you` entry is left
to the object path), despawning a coarse sphere the moment its avatar drops
from the list. A full object supersedes a coarse dot for the same agent. Both
sources share one lazily-built ~2 m UV-sphere mesh + soft-blue material; the
spheres are plain world-space marker entities (not the avatar object root, so
they are not scaled by the avatar's bounding box and carry no attachment
children — attachment parenting stays with the object entity in `objects.rs`,
unchanged). The spheres sit in the root region's frame like `objects.rs` (no
multi-region origin offset yet). New re-export: `CoarseLocation` from
`sl-client-bevy`. Verified live on OpenSim with a second avatar (a
`sl-repl-tokio` login of `avatar2`): the viewer spawns a sphere for its
own avatar and one for the second avatar. **Added on user request (beyond the
base sphere spec):** a floating **name tag** per avatar — a `bevy_ui` text
node anchored bottom-centre over the sphere each frame by projecting the
sphere's head point with `Camera::world_to_viewport` (centred via the tag's
`ComputedNode` size), hidden when off-screen / behind the camera. Names
resolve once per agent through a `UUIDNameRequest`
(`Command::RequestAvatarNames` → `Event::AvatarNames`) and are held in a small
per-agent name cache (plus an "already requested" set) so a frequently-updated
avatar is never re-requested; the tag shows a short id fragment until the real
legacy name arrives. New re-export: `AvatarName` from `sl-client-bevy`.
Verified live: the two tags resolve to `avatar1` and `avatar2` and
render centred over their spheres (user-confirmed).
