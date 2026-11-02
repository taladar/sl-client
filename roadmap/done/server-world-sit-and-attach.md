---
id: server-world-sit-and-attach
title: Sitting and attaching as world state a script can read
topic: server
status: done
origin: LSL-on-the-fake-grid audit (2026-09-20)
points: 8
blocked_by: [server-world-ecs-store, gridspec-attachments]
refs: [server-lsl-lib-avatar-control, server-world-link-sets,
  viewer-seated-region-crossing, gridspec-sit-stand]
---

Context: [context/lsl.md](../context/lsl.md).

The fake grid already answers a sit: `ServerEvent::SitRequested` is
handled, `send_avatar_sit_response` exists, `FakeAgent::seat_on` seats
the avatar from a test, and the crossing code even moves a seated avatar
between regions ([[test-fake-grid-seated-crossing]]). What it does not
have is a *model*: seating is a scripted fixture effect, not a fact the
region stores and other readers can query.

A script reads it constantly — `llAvatarOnSitTarget`,
`llAvatarOnLinkSitTarget`, `llGetNumberOfPrims` (which counts seated
avatars), `llUnSit`, `llSitTarget`, `llSetSitText`, `llForceMouselook`,
`llSetCameraEyeOffset` / `llSetCameraAtOffset`, and the link numbering
rule that puts sitters after the prims ([[server-world-link-sets]]). So
does `changed(CHANGED_LINK)`, which fires on the *object* when somebody
sits on it — the standard way scripted furniture notices an occupant.

Attachments are the same shape on the other side: `llAttachToAvatar`,
`llDetachFromAvatar`, `llGetAttached`, `attach(key id)`, and the fact
that an attached object's `llGetPos` is the *avatar's* region position
while `llGetLocalPos` is its offset on the attach point. The grid
already receives `RezAttachment`, `AttachObject`, `DropAttachments` and
`DetachAttachmentIntoInventory`, and the NPC fixtures already carry
attachments as objects — what is missing is the parent relation in the
store and the position derivation that follows from it.

Wanted:

- a `SeatedOn { entity, link, offset, rotation }` component on the
  avatar and the inverse index on the seat, so both directions are a
  lookup;
- sit-target semantics: a prim with a non-zero `llSitTarget` seats the
  avatar at the stated offset; one without uses the fall-back placement,
  and `llAvatarOnSitTarget` answers `NULL_KEY` for a fall-back sit —
  the distinction content checks;
- an `AttachedTo { agent, point }` component with the same inverse
  index, and `llGetPos` / `llGetLocalPos` / `llGetRootPosition` deriving
  from it;
- both surviving a region crossing and a teleport, which the seated
  crossing work already exercises;
- `CHANGED_LINK` raised on sit, unsit, attach and detach.

Acceptance: a script on a prim sees `llAvatarOnSitTarget` return the
avatar that sat on its sit target and `NULL_KEY` for one that sat
without a target; `llGetNumberOfPrims` counts the sitter;
`llGetAttached` on an attachment returns its point; and the existing
seated-crossing case still passes.

Measured by [[gridspec-sit-stand]] (2026-10-08), in
`book/src/gridspec/movement.md` § Sitting — build to these, per flavour,
beside the `SitPolicy` rows that task added:

- **the request seats the avatar**: both grids send the avatar as a child
  of the seat with (OpenSim) or a frame after (Second Life) the
  `AvatarSitResponse`, without waiting for the `AgentSit` the fake grid
  waits for today;
- a scriptless seat's placement: at an edge of the prim chosen by the
  point clicked, 0.88 m (Second Life) or 0.90 m (OpenSim) above the centre
  of a half-metre cube, facing outwards — and Second Life's response
  states it 0.33 m lower than the avatar's update does;
- a sit target seats 0.35 m above the target on both; Second Life's
  response states that position and OpenSim's the target;
- a taken seat is shared: a second avatar is seated on the same prim
  (0.54 m higher on Second Life), and one asking for a taken sit target is
  seated as on a scriptless prim — where `llAvatarOnSitTarget` must go on
  answering the first;
- distance: Second Life answers a scriptless seat from 7.5 m and is silent
  from 9.3 m; a sit target is answered from anywhere tried on both, and so
  is a scriptless seat on OpenSim;
- standing up, a teleport and a logout all unseat; standing puts the
  avatar in front of where it sat and lets it drop, which needs
  [[server-world-agent-movement]].

`sit-stand` skips its second-resident, scripted-seat and distance legs on a
fake grid: take them offline here.

## From gridspec-lsl-events (2026-10-25)

`events/attach` (`book/src/gridspec/lsl.md` § Events): an attach from the
ground (`ObjectAttach`) raises `attach` with the owner's key alone on Second
Life; OpenSim raises two `changed(32768)` (its own `POSITION` bit) first. A
drop (`ObjectDrop`) raises `attach(NULL_KEY)`, after one `changed(32768)` on
OpenSim. `llAttachToAvatar` after `PERMISSION_ATTACH`: `run_time_permissions`,
then the same as the attach from the ground. The prim keeps its key through
both, the timer runs and a listen hears while worn, on every grid.

## From server-fake-grid-script-engine-wiring (2026-11-02)

Scripts run on the fake grid, and three of their host calls wait for sitting
to be world state (`sl_fake_grid::scripts`, `RegionHost`):

- **`seated_on`** answers `false` for every agent; answer it from the sit.
- **`sit_on_link`** (`llSitOnLink`) refuses every agent with
  `SitRefusal::NoSitTarget`: search for a sit target down the linkset from
  the prim named, with parcel access and the attachment refusal.
- **The lapses**: a drop revokes `PERMISSION_TAKE_CONTROLS`, a sitter
  standing `PERMISSION_TAKE_CONTROLS | PERMISSION_CONTROL_CAMERA`, each
  through `Engine::revoke` with the agent as granter
  (`book/src/gridspec/lsl.md`: 1040 held after a stand).

## Built (2026-11-02)

- **The model** (`sl-fake-grid/src/seating.rs`, `scene_store.rs`): a prim's
  `SeatSettings` (the runtime's `vm::seating::Seat`: sit target, sitter
  camera, mouselook), its `Sitters` in sit order and a worn root's
  `AttachedTo { agent, point, at }` are components; the store's indices give
  an avatar's seat and what it wears, so both directions are a lookup. The
  avatar's side is an index entry rather than a component, avatars being
  sessions and not entities.
- **Sitting**: the request seats the avatar at once (response, then the avatar
  as the seat's child, `SimSession::seat_agent`); a free target seats 0.35 m
  above it along its up, the response stating that on Second Life and the
  target on OpenSim; otherwise each flavour's `PlainSeat` from the half-metre
  cube (Second Life −0.34 m, 0.88 m, turned half round, stated 0.33 m lower;
  OpenSim 0.415 m, 0.90 m), a second sitter 0.54 m higher on Second Life; a
  seat without a free target unanswered past 8.4 m on Second Life.
  `CHANGED_LINK` in every script of the object on sit and stand; a stand, an
  `llUnSit`, a teleport, a logout and the seat's leaving the region stand the
  avatar up and lapse `TAKE_CONTROLS | CONTROL_CAMERA`; `llSitOnLink` seats on
  the next free target down the linkset.
- **Wearing**: `ObjectAttach` from the world and `llAttachToAvatar` (owner's
  `PERMISSION_ATTACH`) re-send the object worn under the same ids, `ObjectDrop`
  puts it back where the wearer stands; `attach(key)` raised, on OpenSim after
  two (wear) or one (drop) `changed(32768)`; a drop lapses `TAKE_CONTROLS`.
  `ObjectDetach` and `llDetachFromAvatar` take it into the Objects folder. A
  worn object stands, hears and speaks where its wearer does, and is shown to
  its wearer alone, residents not being shown to each other.
- **An avatar's own region-local id**: every avatar arriving in a region
  after its first is minted an id of its own (`SceneFixtures::arrive_avatar`,
  read by `avatar_id`), given up when it leaves and never handed out again,
  so it comes back under a new one, as on both grids. Before, every avatar
  of a region was local id 1, which would have hung one avatar's attachment
  off another's body.
- **The library**: `llSitTarget`, `llLinkSitTarget`, `llAvatarOnSitTarget`,
  `llAvatarOnLinkSitTarget`, `llGetNumberOfPrims`, `llGetLinkNumber`,
  `llGetLinkKey` (sitters after the prims, each flavour's link 0/1 rule),
  `llGetObjectPrimCount`, `llUnSit`, `llSetSitText`, `llSetTouchText`,
  `llSetCameraEyeOffset`, `llSetCameraAtOffset`, `llForceMouselook`,
  `llGetAttached`, `llAttachToAvatar`, `llDetachFromAvatar`, `llGetPos`,
  `llGetLocalPos`, `llGetRootPosition`; `Engine::attach`.
- **The client**: `Session` follows its own avatar's parent from the root
  region, as the reference viewer does — the grid standing it up (`llUnSit`)
  or seating it (`llSitOnLink`) without a request.
- **Found on the way**: `SceneFixtures::default()` gave the arriving avatar
  local id 0 (no parent), now `new()`'s 1; an OpenSim-flavoured region's
  engine merged events by Second Life's rule, now `Coalescing::Never`.
- **Held by** `sl-fake-grid/tests/scripts.rs` (a scripted seat, a taken target,
  Second Life's reach, a wear, a drop and a scripted wear, on both flavours),
  `sl-lsl-runtime/src/vm/tests/sitting.rs`, the `sl-proto` lifecycle test of
  the parent, and `sit-stand` offline on both flavours with its second
  resident, scripted seat and seated teleport.

## Handed on

- **The inventory half of wearing** — the item an attach from the world
  makes, the item a drop removes, `ObjectDetach` restating the worn item
  (here a new one is filed), `DetachAttachmentIntoInv`, a wear from the
  inventory and its add/replace modes, and attachments carried across a
  crossing or teleport (here a teleport away or a logout takes them out of the
  region) — to [[fake-grid-own-attachments-and-region-moves]]. So is the
  `attach(NULL_KEY)` a detach into the inventory raises before the object
  goes, which needs the object kept for a tick.
- **The distance legs** of `sit-stand` and the drop after a stand need an
  avatar that moves: [[server-world-agent-movement]].
- **The point clicked and the prim's surface** choosing a scriptless seat's
  spot, and the size of the seat, are not modelled: every such seat places as
  the measured half-metre cube did. Second Life's reach edge (between 7.5 and
  9.3 m) is put at 8.4 m.
