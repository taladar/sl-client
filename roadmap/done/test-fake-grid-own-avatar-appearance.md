---
id: test-fake-grid-own-avatar-appearance
title: The arriving agent never got its own AvatarAppearance
topic: test
status: done
origin: reported while live-verifying viewer-static-asset-library (2026-09-01)
points: 2
refs: [test-fake-grid-npc-avatars, viewer-static-asset-library]
---

Context: [context/testing.md](../context/testing.md).

Against the fake grid the **own** avatar was completely invisible — a
name tag hanging in mid-air over nothing — and had been for every
fake-grid login since the harness existed. The NPCs rendered fine, which
is what hid it: the region clearly *could* draw avatars.

`push_arrival_world` rezzed the arriving agent's avatar **object** and
stopped there. `push_npcs` sends each NPC an `AvatarAppearance`; nothing
sent the agent its own. A real simulator does — an agent's appearance is
broadcast to everyone in the region including the agent — and a viewer
that never receives one has no visual params and no texture entry for
itself, so it spawns the avatar, poses its skeleton (the animation half
worked all along) and draws no body.

It was invisible rather than untextured because the viewer builds the
body meshes *from* the appearance record; without one there is nothing to
build.

Done (2026-09-01): `world::push_own_appearance`, called from
`push_arrival_world` right after the avatar object goes out. The fake
grid runs no bake service, so it bakes the arriving agent exactly the way
it bakes an NPC — `NpcAppearance::solid` — in **green**
(`world::OWN_AVATAR_BAKE_COLOR`), so a fixture session tells its own body
from the catalogue NPC's blue one at a glance.

The bake bytes go into *this session's* asset store rather than the
scenario's, because the ids derive from the agent id and that is only
known at arrival; `push_arrival_world` therefore takes the session's
`InMemoryAssetSource`. The `AvatarAppearance` record itself moved from
`NpcFixture::appearance_record` down to `NpcAppearance::record(avatar,
attachments)`, since it is no longer only the NPCs that need one.

Covered by `the_arriving_agent_gets_its_own_appearance` (tokio): the
client receives an appearance for its own agent id, no baked slot is left
at the `IMG_DEFAULT_AVATAR` sentinel, and all three bakes fetch over
`GetTexture`.

Live-verified against the standalone grid (`--catalogue`): the own avatar
arrives with a body.
