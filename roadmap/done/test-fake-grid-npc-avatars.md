---
id: test-fake-grid-npc-avatars
title: NPC avatars with appearance, animations and attachments
topic: test
status: done
origin: test-harness plan (2026-08-30)
points: 5
refs: [viewer-fake-grid-login-smoke]
blocked_by: [test-fake-grid-render-fixtures]
---

Context: [context/testing.md](../context/testing.md).

Unblocked (2026-09-01): [[test-fake-grid-render-fixtures]] shipped
`PrimFixture` (including `.attached_to`, which is how an NPC's
attachments are built) and the `RegionFixture` an `npcs` field belongs
on — it was deliberately left off until this task defines `NpcFixture`.

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

Done (2026-09-01): `sl-fake-grid/src/fixtures/npcs.rs` — `NpcFixture`
(`new(local_id, AvatarIdentity, position).looking(..).rotated(..)
.animating(..).wearing(PrimFixture, point, item, offset, rotation)`),
`NpcAppearance { visual_params, bakes }` with `::default_avatar()` /
`::solid`,
and `NpcBake`. `world::avatar_object` is now the public
`world::avatar_prim`, `AvatarIdentity` is public with a constructor, and
`push_arrival_world` appends the burst the plan describes through a new
`push_npcs`. `SceneFixtures` grew `npcs` and `all_objects()` (an object
refetch answers for an NPC's body and attachments too);
`RegionFixture::into_scenario` registers each NPC's bake bytes.
`sl-proto` grew `SimSession::send_avatar_appearance`,
`send_avatar_animation` and `send_terse_update`, all three covered by
`avatar_appearance_animation_and_terse_motion_reach_client` in
`sl-proto/tests/sim_session.rs`. The catalogue gained
`catalogue::npc()` — blue bakes, the built-in `stand` animation, a
checker box on its skull — and the binary logs it under `--catalogue`.
Acceptance met by `the_catalogue_npc_arrives_with_appearance_and_attachment`
(tokio, which also fetches a bake over `GetTexture`) and
`bevy_client_sees_the_catalogue_npc`.

Four deviations from the plan, all deliberate:

- `npcs` sits on `SceneFixtures`, not directly on `RegionFixture` (which
  reaches it through its `world`): the arrival burst and the refetch path
  both take `&SceneFixtures`, and an NPC is world state exactly like a
  prim. `RegionFixture` still owns the *asset* half, registering the
  bakes in `into_scenario`.
- The Bevy half asserts the session events (avatar-pcode `ObjectAdded`,
  `AvatarAppearance`, `AvatarAnimation`, the parented attachment), not an
  `SlAvatar` entity — `sl-client-bevy` spawns no avatar entities at all;
  the entity is the viewer's, so that assertion belongs to
  [[viewer-fake-grid-render-harness]].
- The visual-param vector is `DEFAULT_VISUAL_PARAMS`, OpenSim's own
  `AvatarAppearance.SetDefaultParams` table (218 bytes, the "Ruth" default
  body), not "sized for the mini LAD" — the mini LAD fixture defines no
  visual params at all. The first shot at this used the midpoint of every
  param's range and rendered a badly distorted avatar; the ranges are not
  centred on anything a body wants to be. The standard `avatar_lad.xml`
  transmits 253 params — exactly those 218 classic ones (id < 10000) and
  then the 33 physics params plus two more — so OpenSim's vector lands
  slot for slot and the rest falls back to each param's default.
- An `AvatarAnimation`'s source list stamps a sourceless animation with
  the **avatar's own id**, mirroring OpenSim's `SendAnimations`, so a
  round trip never yields `source_id: None` for it.

Follow-up, now [[test-fake-grid-animation-assets]]: nothing in the fake
grid serves an **animation asset**, so the NPC's `stand` is signalled but
not fetchable — the render-catalogue check that "two captures a second
apart differ" needs a synthetic `.anim` in `sl-test-assets` first.

Live-verified against the standalone grid (`--catalogue`) with the real
viewer: the NPC's body reads as a normal default avatar, its bakes are
blue, and its box sits on its head. Two things the session cost, both
fixed here and written up in the book chapter:

- Every fixture texture was **64²**, which renders as a stuck low-LOD blur
  on a one metre prim face — the LOD driver asks for discard 0 and there
  is nothing finer to fetch. `TEXTURE_SIZE` and the NPC bakes are now 512
  (what real content is; ~13 kB for the checker, ~300 bytes for a solid at
  any size), with `SCULPT_MAP_SIZE` split off at 64 because a sculpt map
  is geometry the viewer reads as a 64² vertex grid.
- The first attempt at that fix *looked* like it changed nothing: the
  viewer's texture disk cache is keyed by **UUID** and is not per-account,
  so it kept serving the old bytes under the unchanged id — through three
  different fake accounts. A fixture-content A/B needs a cold cache
  (`XDG_CACHE_HOME` at a scratch dir); the tell is the LOD driver's
  `native WxH`, which read `64x64` while the grid served 512².

Also noticed and filed rather than fixed here:
[[test-fake-grid-default-wearable-textures]] — the *own* avatar's default
wearable textures 404 against the fake grid.
