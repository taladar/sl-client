---
id: viewer-name-tags-billboard-render
title: Name tags — world-space billboard renderer
topic: viewer
status: ready
origin: user request (2026-07); split from viewer-name-tags
refs: [viewer-name-tags-display-names]
---

Context: [context/viewer.md](../context/viewer.md).

Promote the debug name tag to the reference feature's **rendering**. The viewer
today projects a screen-space `Text2d` with `Camera::world_to_viewport` and
hides it when off-screen — no styling, no culling, no occlusion.

The tags are already **out of `bevy_ui`**: since
[[viewer-perf-ui-layout-per-frame-relayout]] each tag is a `Text2d` on a
dedicated overlay `Camera2d` (`name_tag_overlay.rs`), because the old
absolutely-positioned UI nodes dirtied taffy layout for every avatar every
frame. That resolves half of what used to be this task's first architectural
decision; the remaining choice is **screen-space overlay vs true world-space
billboards**. The reference draws tags in-world (`LLHUDNameTag`), which is what
makes occlusion, depth and size clamping natural — screen-space rendering makes
each of those a special case. Pick the world-space path so the same machinery
serves object hover text ([[viewer-hover-text]]) too; the overlay-camera
projection machinery is small and disposable, while the `Text2d`-per-tag
entities, the name plumbing and the `NameTagHitTest` cursor resolution carry
over.

Deliver the tag **behaviour**:

- a **backdrop bubble** and **outline** so tags read against any background;
- **occlusion / depth** behaviour against world geometry (the tag reads as
  attached in the world, not floating over everything);
- **on-screen size clamping** so a distant tag stays legible without dominating;
- **distance-based fade** and a **hide-beyond-N-metres** cut-off (deliberately
  kept in this task's scope, not the perf task's: culling far tags also caps the
  tag population, but it belongs with the rest of the distance behaviour).

This task renders whatever text the resolver
([[viewer-name-tags-display-names]]) supplies; the decorations (title line,
states, colouring), click-to-select and the preference toggles are separate
follow-ups ([[viewer-name-tags-decorations]], [[viewer-name-tags-click-select]],
[[viewer-name-tags-preferences]]).

**Standing hazard** (from `viewer-name-tags-lost-to-probe-cameras`): every
camera query in this code must stay qualified `With<ViewerCamera>`, or the
P33.2 reflection-probe cameras make `Query::single()` fail every frame and the
tags vanish. (A `Text2d` tag is structurally invisible to the probes' 3D
cameras; a world-space billboard mesh will need explicit probe-layer exclusion
again.)

Reference (Firestorm, read-only): `llhudnametag`, `llhudtext`,
`llvoavatar::idleUpdateNameTag`.

Builds on: the existing `avatars.rs` tags (`NameTag`, `spawn_label`,
`position_name_tags`) and the `name_tag_overlay.rs` overlay camera + hit test.
