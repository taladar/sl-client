---
id: viewer-object-moved-region-reuse
title: Reuse an object that arrives from another region under a new id
topic: viewer
status: ready
origin: found while fixing viewer-inventory-worn-before-attach-confirmed (2026-09-27)
refs: [viewer-inventory-worn-before-attach-confirmed, viewer-teleport-never-resets-the-world]
---

Context: [context/viewer.md](../context/viewer.md).

An object that moves to another region, such as our own avatar and its
attachments on a neighbour teleport or a crossing, arrives from the new
region under a new region-local id and the same full id. Since
[[viewer-inventory-worn-before-attach-confirmed]], the session recognises
this through `WorldCache`'s full-id index (`copy_elsewhere`). It announces
the move as an `ObjectAdded` for the new copy and an `ObjectRemoved` for
the stale one. The avatar store only re-keys (`AvatarState::rekey_avatar`),
but every other store despawns the old copy and builds the new one from
scratch: entities, tessellation, materials, deferred mesh builds. Every
neighbour teleport rebuilds all our attachments for no reason, and anyone
else's attachments that cross a border. Throwing an object away and
rebuilding it the moment it comes back is the wasted effort this task
removes. Firestorm does the same waste on Second Life by honouring the old
region's kills (below).

## Move, don't respawn

- **Session.** A superseding arrival emits one `Event::ObjectMoved { from,
  to, object }` (the new snapshot), not an add and a remove. Wire it
  through both runtimes (`sl-client-bevy`, `sl-client-tokio`) and the REPL
  formatter. Checklist: `sl-client-adding-an-event-variant`.
- **`ObjectState`.** Re-key the tracked object from `from` to `to`, keeping
  its entity, geometry holder and face entities. Fix the children index,
  both the moved object's own children and its entry under its parent,
  since the parent (the avatar, or a linkset root that also moved) is
  re-keyed by its own move. Then apply the snapshot as an ordinary update,
  so only what changed is rebuilt. An object queued in
  `PendingObjectEvents` under `from` must follow.
- **Every other store keyed by scoped id** follows the move rather than
  forgetting: `AvatarState`'s `object_parents` / `baked_hides` /
  `scanned_objects`, object cost, derender suppression, edit selection,
  media, lights, sounds, particles, and whatever else listens to
  `ObjectRemoved`. Audit them all. One that is missed falls back to
  forget-and-rebuild, which is today's behaviour, not a break.
- **Children arrive in any order.** A moved linkset's child can arrive
  before or after its root. Each move is per object, so each re-keys
  itself, and the parent links resolve once both sides have moved.

## Second Life: the old region's kill

OpenSim never kills what moves to a neighbour the viewer can see
(`ScenePresence.MakeChildAgent`). Second Life does: the region we left
sends `KillObject` for our attachments, and Firestorm even guards its own
avatar against that kill ("never kill our avatar"). The kill probably
arrives **before** the new region streams the copies, so without help it
still means forget-and-rebuild.

Firestorm's workaround (`FSExperimentalLostAttachmentsFix`, FIRE-12004)
ignores kills of our attachments during a teleport or crossing. That is
needed there because its object list is keyed by full id, so a late kill
from the old region deletes the object that has already moved. We key by
region-local id, so a late kill is harmless. The problem is an early one.

The session should hold a kill for **our own** avatar and attachments that
arrives on a circuit we are leaving or have just left, for a short grace
period. If the same full id arrives on another circuit within it, that is a
move (above), and the held kill is dropped. If not, the kill applies late,
so a real detach or derez still takes effect. Measure the kill-to-arrival
gap on aditi before choosing the grace.

## Verify

- Session tests for both orders: an arrival before the old copy's kill (the
  OpenSim case, no kill at all), and a kill before the arrival (the Second
  Life case), each ending in one `ObjectMoved` and no despawn.
- A world-tier test that the tracked object keeps its entity across a move.
- Live on OpenSim and aditi: no attachment rebuild on a neighbour teleport
  or a crossing, a trace showing moves rather than respawns, and a detach
  right after a teleport still working.
