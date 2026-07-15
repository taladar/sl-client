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
today projects a plain `bevy_ui` text node with `Camera::world_to_viewport` and
hides it when off-screen — no styling, no culling, no occlusion.

The first decision is architectural: whether tags stay `bevy_ui` nodes or become
**world-space billboarded text**. The reference draws them in-world
(`LLHUDNameTag`), which is what makes occlusion, depth and size clamping natural
— screen-space nodes make each of those a special case. Pick the world-space
path so the same machinery serves object hover text
([[viewer-hover-text]]) too.

Deliver the tag **behaviour**:

- a **backdrop bubble** and **outline** so tags read against any background;
- **occlusion / depth** behaviour against world geometry (the tag reads as
  attached in the world, not floating over everything);
- **on-screen size clamping** so a distant tag stays legible without dominating;
- **distance-based fade** and a **hide-beyond-N-metres** cut-off.

This task renders whatever text the resolver
([[viewer-name-tags-display-names]]) supplies; the decorations (title line,
states, colouring), click-to-select and the preference toggles are separate
follow-ups ([[viewer-name-tags-decorations]], [[viewer-name-tags-click-select]],
[[viewer-name-tags-preferences]]).

**Standing hazard** (from `viewer-name-tags-lost-to-probe-cameras`): every
camera query in this code must stay qualified `With<FlyCamera>`, or the P33.2
reflection-probe cameras make `Query::single()` fail every frame and the tags
vanish.

Reference (Firestorm, read-only): `llhudnametag`, `llhudtext`,
`llvoavatar::idleUpdateNameTag`.

Builds on: the existing `avatars.rs` tags (`NameTag`, `spawn_label`,
`position_name_tags`).
