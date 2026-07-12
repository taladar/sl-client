---
id: permission-a5
title: Design the client-mirror reset (the crux)
topic: permission
status: done
origin: PERMISSION_ROADMAP.md
---

Context: [context/permission.md](../context/permission.md).

**A5. Design the client-mirror reset (the crux).** Per the decided
client-mirror model and the `SitState` precedent, define which signal clears
which state, distinguishing an attachment (crosses with the avatar) from an
in-world object (left behind):

- **Real teleport** (`begin_handover`, `TeleportLocal` — the
  `self.sit = SitState::NotSitting` sites): drop all **in-world-object**
  grants and their taken controls; **keep attachment** grants.
- **Neighbour region crossing** (`promote_child_to_root`, which keeps the
  seat): **keep** all grants — an in-world object may still be visible in a
  neighbour, and a vehicle / attachment crosses with the agent.
- **`DisableSimulator`** (a child/neighbour circuit retired — the
  `forget_sim_objects` site): drop grants scoped to that circuit's objects.
- **Detach** (`detach_objects` / `remove_attachment`, and the inbound removal
  of an attachment): clear that attachment's grants and controls — this
  mirrors the sim's auto-revoke on detach, which also arrives as a
  `ScriptControlChange(release)`.
- The session does **not** message the simulator on teleport; it just clears
  its own tracking (the left-behind object is unreachable anyway).
**Done — see § Client-mirror reset reference (from A5) + task B2 in
§ Phase B.**
Decided: the **grant registry** and the (A6) **taken-controls tracker** reset
on *different* signals. Grant registry — real teleport drops every
`HolderKind::InWorld` grant (keeps `Attachment`); a neighbour crossing keeps
all; `forget_sim_objects` (DisableSimulator + child-circuit expiry, both
child-only, never the root) drops grants scoped to that circuit; an object
going away (inbound `KillObject`) drops grants on that object's `full_id`,
which is the *detach* path too (the sim echoes `KillObject` for the detached
attachment).
**Two corrections to this item's wording, grounded in Firestorm:** (1) the
teleport reset clears **only in-world grants, not taken controls** — taken
controls are agent-global (A6), unattributable to a holder, and the viewer
keeps `mControlsTakenCount` across teleport (only a
`ScriptControlChange(release)` decrements it), so the tracker resets on the
release echo / explicit `release_script_controls`, never on teleport; (2)
detach is mirrored from the **inbound** echoes (`KillObject` clears the grant,
`ScriptControlChange(release)` clears controls), **not** by hooking the
outbound `detach_objects` / `remove_attachment` — the mirror follows the wire
(A3), and `remove_attachment`'s `item_id` is the *worn* item, not the grant's
script-item key, so it could not match a grant entry anyway.

## Client-mirror reset reference (from A5)

When and how the mirror is cleared as the agent moves. Per § Protocol reality
the simulator **never** pushes a "permissions revoked" on a region change, so
every reset here is a pure *client-mirror* policy: the session clears its own
tracking and sends nothing to the sim (a left-behind in-world object is in the
old simulator, unreachable; an attachment crosses with the agent). This follows
the exact `SitState` precedent — a real teleport resets, a neighbour crossing
deliberately keeps (`7bc19b4`).

**Two states, two reset rules.** The reset touches two *separate* stores (A2 vs
A6), and they do **not** clear on the same signals:

- the **grant registry** `script_grants` (A2/B2) — keyed per script, each entry
  carrying its `HolderKind` and `circuit`;
- the **taken-controls tracker** (A6, agent-global, **no** holder/object
  attribution — `ScriptControlChange` carries no object id, see § Protocol
  reality).

**Grant-registry resets** (each maps onto an existing reset site in
`session/methods.rs`):

| Signal | Site | Action on `script_grants` |
|--------|------|---------------------------|
| Real teleport | `begin_handover` (`:696`), `TeleportLocal` (`:1960`) — the two **teleport** `SitState::NotSitting` sites | drop every `HolderKind::InWorld` grant; **keep** `Attachment` |
| Neighbour crossing | `promote_child_to_root` (`:790`) | **keep all** — no change |
| Circuit retired | `forget_sim_objects` (`:1439`) | drop grants whose `circuit == Some(circuit_id)` |
| Object gone (incl. detach) | inbound `KillObject` (`:1180`) | drop grants whose `task_id == removed.full_id` |

- **Real teleport** drops in-world grants via
  `script_grants.retain(|_, g| matches!(g.kind, HolderKind::Attachment))` at the
  two **teleport** `SitState::NotSitting` sites *only* — not the sit-timeout
  (`:3072`) or `stand` (`:3427`) sites, which are seating changes, not region
  leaves. (A small `drop_inworld_grants()` helper called from both keeps the two
  sites in lockstep, like the shared `self.sit = …` lines.)
- **Neighbour crossing** keeps every grant: an in-world object may still be
  visible from the neighbour, and a vehicle/attachment crosses with the agent.
  Its in-world grants stay circuit-scoped to the old root (now demoted to a
  child) and are dropped later, when that child is retired (next row).
- **Circuit retired** hooks `forget_sim_objects`, beside its existing
  per-circuit object/terrain/region drops:
  `script_grants.retain(|_, g| g.circuit != Some(circuit_id))`. Both callers —
  the `DisableSimulator` handler (`:1089`) and the child-inactivity expiry
  (`:3150`) — are **child/neighbour** circuits; the root is never retired this
  way (it changes via teleport/crossing), so this never drops an
  attachment grant (attachments are root-scoped, never a disabled child id).
- **Object gone** hooks the inbound `KillObject` handler. It already resolves
  the removed `Object` (to read its `region_handle`); also read its `full_id`
  and `script_grants.retain(|h, _| h.task_id != full_id)`. This is the
  **detach** path too: detaching an attachment makes the sim send a `KillObject`
  for the rezzed object, so the attachment's grant is cleared by following that
  echo — no outbound hook on `detach_objects` / `remove_attachment` (the mirror
  follows the wire, per A3; and `remove_attachment` names the *worn* item, not
  the script's inventory item, so it could not key a grant anyway).

**Taken-controls-tracker resets** (policy here; the concrete clear lands with
the A6 tracker, sequenced like B3):

- **Not** cleared on real teleport, neighbour crossing, or `DisableSimulator`.
  The tracker is agent-global and cannot be attributed to the in-world holder
  being left behind; and the viewer is faithful here — Firestorm resets
  `mControlsTakenCount` **only** in its constructor and mutates it **only** in
  `processScriptControlChange` (Take `++`, Release `--`); `resetControlFlags`
  touches the ephemeral input flags, **not** the taken counts. So taken controls
  survive a teleport in the viewer and must in the mirror.
- Cleared **per-bit** by an inbound `ScriptControlChange(release)` (A6) — the
  only revoke the sim pushes — and **wholesale** by the explicit
  `release_script_controls` send (B3). **Detach** needs no dedicated controls
  clear: the sim auto-revokes `TAKE_CONTROLS | CONTROL_CAMERA` on detach and
  echoes a `ScriptControlChange(release)`, so the A6 tracker self-corrects on
  that echo (matching the `KillObject` that clears the grant).

**Not reset here:** `close` / logout / relogin. The existing caches (`objects`
et al.) are **not** cleared on `close` either — a `Closed` session is dead and a
relogin builds fresh state through the constructor — so `script_grants` follows
the same convention and adds no `close` hook. (A8 lists this as a sign-off
checkpoint in case a relogin is later made to reuse a live `Session`.)
