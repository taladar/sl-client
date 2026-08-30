---
id: test-fake-grid-npc-avatars
title: NPC avatars with appearance, animations and attachments
topic: test
status: blocked
origin: test-harness plan (2026-08-30)
points: 5
refs: [viewer-fake-grid-login-smoke]
blocked_by: [test-fake-grid-render-fixtures]
---

Context: [context/testing.md](../context/testing.md).

The fake grid rezzes only the arriving agent's own avatar object and has
no inter-session broadcast, so a second avatar is invisible. Model other
avatars as scripted NPCs: `NpcFixture { agent_id, names, position,
appearance: NpcAppearance { visual_params (sized for the mini LAD),
bakes }, animations, attachments }`; `world::avatar_object` becomes a
public `avatar_prim`; new `SimSession::send_avatar_appearance`,
`send_avatar_animation` and `send_terse_update` helpers with loopback
tests. Bake bytes live under the texture entry's baked-slot UUIDs in the
asset source (the OpenSim path — no server-bake service advertised), so
the viewer fetches them by plain `GetTexture`. Attachments are child
objects with the wearer's local id as parent and the attachment point in
the state byte.

Arrival burst appends: NPC objects → their `AvatarAppearance` →
`AvatarAnimation` → attachments.

Acceptance: the tokio suite sees `AvatarAppearance`/`AvatarAnimation`
events for the NPC; the Bevy smoke sees an `SlAvatar` entity for it.
