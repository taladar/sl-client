---
id: server-world-sit-and-attach
title: Sitting and attaching as world state a script can read
topic: server
status: blocked
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
