---
id: permission-a2
title: Design the state model & keying
topic: permission
status: done
origin: PERMISSION_ROADMAP.md
---

Context: [context/permission.md](../context/permission.md).

**A2. Design the state model & keying.** Specify what `Session` stores
(in `session.rs`, beside `TeleportPhase` / `SitState`): grants keyed by the
holding script `(task_id: ObjectKey, item_id: InventoryKey)` → granted
`ScriptPermissions`, plus the holder **kind** (attachment vs in-world object)
and its circuit/region for reset scoping, the optional `experience_id`, and
the per-holder set of **taken controls** (`ScriptControl`). Decide whether to
also track outstanding (un-answered) requests. Reuse the typed keys
(`ObjectKey` / `InventoryKey`). Define how the attachment-vs-in-world kind is
determined (the object cache `objects` / attachment tracking). **Done — see
§ State-model reference (from A2) + task B2 in § Phase B.** Decided: one
`BTreeMap<ScriptHolder, ScriptGrant>` field on `Session`, keyed by
`(task_id, item_id)`; each grant carries the raw granted bits, a `HolderKind`
(attachment vs in-world, with a conservative in-world default when the holder
is not in the object cache), the holder's `CircuitId` for reset scoping, and
the optional `experience_id`. **Correction to this item's premise:** taken
controls are **not** per-holder — `ScriptControlChange` carries no object id
(see § Protocol reality), so taken controls become a *separate session-global*
tracker, not a registry field (designed under A6, not A2). Outstanding
(un-answered) requests are **not** tracked — the driver already holds the
`Event::ScriptPermissionRequest` until it answers.

## State-model reference (from A2)

The permission mirror is one new `Session` field (in `session.rs`), beside
`sit: SitState` / `teleport: TeleportPhase`, private like them and reached only
through accessors. The simulator stays authoritative; this is an API-convenience
mirror, not a security boundary.

**The grant registry.**

    script_grants: BTreeMap<ScriptHolder, ScriptGrant>

- **`ScriptHolder`** — the key, the script that holds the grant:
  `{ task_id: ObjectKey, item_id: InventoryKey }`. A script is uniquely a
  `(holding object, inventory item within it)` pair — one object may run several
  scripts, each with its own grant — so both halves are needed. Both fields come
  straight off the `ScriptQuestion` / `ScriptPermissionRequest`, reusing the
  existing typed keys (no new key newtype). `BTreeMap` (not `HashMap`) keeps the
  crate's deterministic-iteration convention; derive `Ord` on `ScriptHolder`.
- **`ScriptGrant`** — the value:
  - `granted: ScriptPermissions` — the granted subset, stored **wholesale** as
    the raw bitfield (per B1: 8 record-only flags need no handler, 3 cooperation
    flags reuse existing event surfaces, only `TELEPORT` carries an action). A
    partial grant is just a subset of bits; an explicit deny is the absence of a
    registry entry (or an entry with empty `granted` — A3 decides which).
    Replace the whole entry on a re-grant for the same holder (a later
    `llRequestPermissions` supersedes the earlier grant — matches the sim, which
    keeps only the latest per script).
  - `kind: HolderKind` — drives the A5 reset (attachments cross with the avatar;
    in-world objects are left behind). See below.
  - `circuit: Option<CircuitId>` — the circuit the holder was last seen on, for
    scoping the `DisableSimulator` and in-world-teleport resets (A5). `None`
    when the holder was not in the object cache at grant time (kind `InWorld`
    fallback). Stale for an attachment once it crosses a border, but attachments
    are kept on those resets regardless of circuit, so the staleness is inert.
  - `experience_id: Option<ExperienceKey>` — the experience the grant was made
    under, copied from the request (`None` outside an experience).

**`HolderKind`** — a private enum:

    enum HolderKind { Attachment, InWorld }

- **`Attachment`** — the script lives in an attachment worn by *this* agent; the
  grant crosses regions with the avatar (kept on teleport, cleared on detach).
- **`InWorld`** — the script lives in an in-world object (or another avatar's
  attachment, which is in-world from our frame); the grant is region/circuit
  scoped and dropped when the agent leaves it.

No `Unknown` variant: detection failure falls back to `InWorld`, the
conservative direction (an unrecognised holder is cleared on the next teleport
rather than kept forever — losing a mirror entry is cheap; the sim still
enforces).

**Determining the kind** (at grant time, in `answer_script_permissions`): look
up the holder by `task_id` in the `objects` cache. There is no by-`full_id`
index, so scan `self.objects` values for `full_id == task_id` (small N — only
nearby objects are cached; a helper `object_by_full_id(ObjectKey) ->
Option<&Object>` is worth adding). Classify as `Attachment` **iff** the found
object `attachment_point().is_some()` **and** it is parented to *our own* avatar
— i.e. its `parent_id` resolves, within the same circuit, to an avatar object
whose `full_id` equals `self.agent_id()`. Otherwise `InWorld` (including the
not-found case). Record the `CircuitId` it was found on into `circuit`. The
exact own-avatar-parentage plumbing (the session does not yet cache its own
avatar's region-local id) is the open detail A8 flags for sign-off; the fallback
keeps the design safe until it is resolved.

**Not stored here:** the **taken-controls** state. `ScriptControlChange` carries
no object id (see § Protocol reality), so taken controls are agent-global, not
per-holder; that tracker is a *separate* session field designed under A6, keyed
by nothing (a single agent-wide `ControlFlags` set, or per-control counts
mirroring the viewer). Also **not** stored: outstanding un-answered requests —
the driver holds the `Event::ScriptPermissionRequest` until it calls
`answer_script_permissions`, so the registry records only answered grants.
