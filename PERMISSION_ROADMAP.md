# permission system road map

A plan to give the SL client a *stateful* script-permission system. Today the
permission surface is a stateless pass-through (`ScriptQuestion` →
`Event::ScriptPermissionRequest`, `answer_script_permissions` →
`ScriptAnswerYes`, `ScriptControlChange` → `Event::ScriptControlChange`,
`release_script_controls` → `ForceScriptControlRelease`) with **no `Session`
state** recording what the agent has granted. This roadmap plans a system that
keeps the grant state for the library user, acts on grants as far as the API can
(controls and camera still need the library user's cooperation), and resets that
state as regions are left. Work these top-to-bottom; tick a box only when the
step builds, is clippy-clean (restriction lints), and `cargo test` passes. Add
sub-tasks as you discover them.

Phase A is **planning only** — its items produce design decisions, not code.
Phases B+ (implementation) are defined once Phase A is signed off.

Scope reminders:

- Commit on the current branch only (never auto-create a feature branch).
- `Session` (sl-proto) is sans-IO: the permission state lives there, beside
  `TeleportPhase` / `SitState`, driven by inbound messages and the answer/revoke
  commands. The simulator stays authoritative for actual enforcement — the
  client mirror is an API convenience, **not** a security boundary.
- Keep `sl-client-tokio` and `sl-client-bevy` (and the REPL) at feature parity.
- Never push client-only protocol types into the shared `sl-types` crate.
- Wrap this file at 80 columns; fmt/clippy/rumdl green before commit (the ggh
  hook rejects MD013 and re-runs clippy).

## Protocol reality (constraints Phase A must respect)

- The permission flags are `sl_types::lsl::ScriptPermissions` (re-exported at
  `sl-proto/src/types/script.rs`): `DEBIT`, `TAKE_CONTROLS`,
  `TRIGGER_ANIMATION`, `ATTACH`, `CHANGE_LINKS`, `TRACK_CAMERA`,
  `CONTROL_CAMERA`, `TELEPORT`, `EXPERIENCE`, `SILENT_ESTATE_MANAGEMENT`,
  `OVERRIDE_ANIMATIONS`, `RETURN_OBJECTS`.
- **The simulator does not revoke permissions on a region change** and never
  pushes a "permissions revoked" message. Server-side it revokes only on
  **detach** (`AttachmentsModule.cs` clears `TAKE_CONTROLS | CONTROL_CAMERA`),
  on an explicit `ForceScriptControlRelease`, or on an unsecure sit. So a
  "reset on region leave" is a **client-mirror** policy (the decided model): an
  in-world object the agent teleports away from is simply unreachable (it is in
  the old simulator), whereas an **attachment** crosses with the agent and keeps
  its grant.
- A `ScriptControlChange` carrying the release flag is the **only** revoke
  signal the simulator pushes (there is no inbound `RevokePermissions`).
- **`ScriptControlChange` carries no object id** (wire `Low 189`: a `Data` array
  of `{ TakeControls: BOOL, Controls: U32, PassToAgent: BOOL }`, with no
  task/holder field). Firestorm's `LLAgent::processScriptControlChange`
  accordingly tracks controls **agent-globally**, as a per-control-bit *count*
  (`mControlsTakenCount` / `mControlsTakenPassedOnCount`, split by
  `PassToAgent`, incremented on take and decremented on release). So
  taken-controls state cannot be attributed to a specific holder and lives at
  the **session level**, not in the per-holder grant registry (A2). This refutes
  A2's "per-holder set of taken controls" wording; the taken-controls tracker is
  designed under A6.
- `RevokePermissions` (wire `message_template.msg`, `Low 193`, client→server,
  `ObjectID` + `ObjectPermissions`; the server honours only the animation /
  override-animation bits per Firestorm) exists on the wire but has **no
  `sl-proto` command** today.
- Reset-point precedents already in `sl-proto/src/session/methods.rs`: a real
  teleport (`begin_handover`, `TeleportLocal`) resets `SitState`; a neighbour
  crossing (`promote_child_to_root`) deliberately **keeps** it;
  `DisableSimulator` drops a retired circuit's caches via `forget_sim_objects`.
  The `SitState` / `Session::seat` reset added in commit `7bc19b4` is the exact
  pattern the permission reset should follow.

## Phase A — plan the permission system (design only; no code yet)

- [x] **A1. Inventory & classify the permission set.** Enumerate every
  `ScriptPermissions` flag and assign the client's role: *record-only* (the sim
  enforces it; the client only mirrors the grant — `DEBIT`, `ATTACH`,
  `CHANGE_LINKS`, `RETURN_OBJECTS`, `SILENT_ESTATE_MANAGEMENT`, `EXPERIENCE`),
  *needs library-user cooperation* (`TAKE_CONTROLS` → the user routes the
  control inputs; `CONTROL_CAMERA` / `TRACK_CAMERA` → the user drives the
  camera), or *client-actionable via existing API* (`TELEPORT` →
  `Session::teleport_to`; `TRIGGER_ANIMATION` / `OVERRIDE_ANIMATIONS` → the sim
  plays them, nothing client-side). Output: a per-flag responsibility table that
  drives A4. **Done — produced the classification reference + task B1 in
  § Phase B** (8 record-only, 3 cooperation, 1 API action `TELEPORT`).
- [x] **A2. Design the state model & keying.** Specify what `Session` stores
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
- [x] **A3. Design grant/deny recording & the revoke command.** How
  `answer_script_permissions` records the granted subset into the registry
  (and how a partial grant or an explicit deny is represented). Add the
  missing granular revoke: a new `RevokePermissions` `Command` + `Session`
  method (the wire message exists; wire it through `command.rs` /
  `session/methods.rs` / `session/circuit.rs`), and define how
  `release_script_controls` (`ForceScriptControlRelease`) updates the
  mirror. Define the library-user query accessors (e.g.
  `granted_permissions(holder) -> ScriptPermissions`,
  `script_controls() -> …`). **Done — see § Grant/deny & revoke reference
  (from A3) + tasks B3–B6 in § Phase B.** Decided: recording happens *after*
  the wire send inside `answer_script_permissions` (gaining one new param,
  `experience_id`, passed back from the request — the only grant datum not
  derivable at answer time, keeping the "no outstanding-request tracking"
  decision); a non-empty grant **replaces** the holder's entry, an explicit
  deny (empty grant) **removes** it, so a deny is the *absence* of a
  registry entry (this resolves A2's open question). The new object-scoped
  `RevokePermissions` command clears, from the mirror, **only** the bits the
  sim honours (`TRIGGER_ANIMATION | OVERRIDE_ANIMATIONS`) across every grant
  on that object; `release_script_controls` clears the *taken-controls*
  tracker (A6) but leaves the grant registry untouched (the `TAKE_CONTROLS`
  grant persists; only the live taken set resets). Accessors return public
  types only (the A2 registry types stay private):
  `granted_permissions(task_id, item_id)`, a `script_grants` iterator
  yielding a new public `ScriptGrantInfo` view, and the A6-finalized
  `script_controls`.
- [ ] **A4. Decide the auto-act policy (API-concept grants).** Using A1, decide
  which granted permissions the session acts on autonomously versus surfaces for
  the library user: record-only flags need no action; controls/camera are
  surfaced (the user cooperates) and the session only tracks the taken-controls
  set from `ScriptControlChange`; for `TELEPORT`, decide whether a granted
  script-teleport (`Event::ScriptTeleport`) may auto-call `teleport_to` or stays
  user-driven. Keep the library a conduit where it must be, a convenience where
  the API already covers the action.
- [ ] **A5. Design the client-mirror reset (the crux).** Per the decided
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
- [ ] **A6. Design inbound control-change & de-facto revoke handling.** Specify
  how `ScriptControlChange` (already `Event::ScriptControlChange`) updates the
  taken-controls registry (a `Take` adds, a `Release` removes), and record that
  a `ScriptControlChange(release)` is the only revoke the sim pushes (no inbound
  `RevokePermissions`). Decide whether a client-sent `release_script_controls` /
  `RevokePermissions` updates the mirror immediately.
- [ ] **A7. Specify the API-surface delta & driver/REPL exposure.** Enumerate
  the new/changed `Command`s (`RevokePermissions`, an optional grant
  convenience), any `Event` changes (inbound likely unchanged), and the new
  `Session` accessors; and how `sl-client-tokio`, `sl-client-bevy`, and the REPL
  expose the commands and a way to query the granted state, at feature parity.
  Draw the boundary: what is sl-proto `Session` state versus what stays
  application policy.
- [ ] **A8. Define the test & verification strategy.** Plan the
  `sl-proto/tests/lifecycle.rs` and `sim_session.rs` cases (mirroring the new
  `teleport_clears_seat` test): feed a `ScriptQuestion` →
  `answer_script_permissions` → assert the registry; feed a real teleport →
  assert in-world grants cleared but attachment grants kept; feed a neighbour
  crossing → assert grants kept; feed `DisableSimulator` / a detach → assert the
  scoped clears; feed `ScriptControlChange` `Take` / `Release` → assert the
  taken-controls tracking. List the remaining open questions for sign-off before
  implementation (the exact attachment-detection source; whether to expose an
  explicit deny).

Phase A scopes the planning only; the implementation tasks each Phase A item
produces are appended to **Phase B** below as that item is worked.

## Phase B — implementation (tasks produced by Phase A)

Each Phase A item, once checked, appends the concrete implementation tasks it
implies here (tagged with the producing item). These are *not* started until
Phase A is signed off; tick a box only when the step builds, is clippy-clean
(restriction lints), and `cargo test` passes. Keep `sl-client-tokio`,
`sl-client-bevy`, and the REPL at feature parity; never push client-only types
into shared `sl-types`.

### Classification reference (from A1)

The 12 grantable `ScriptPermissions` flags by the client's responsibility. The
simulator stays authoritative; every client record is a mirror, not a security
boundary. Roles: **record-only** — the sim enforces end-to-end, the client only
mirrors the grant and takes no action (any effect arrives later on the ordinary
message path) · **cooperation** — inert unless the runtime routes control inputs
or applies camera params; `sl-proto` surfaces the grant and tracks the live
state · **API action** — maps onto an existing `Session` method.

| Flag | Bit | Role |
|------|-----|------|
| `DEBIT` | `1<<1` | record-only |
| `TAKE_CONTROLS` | `1<<2` | cooperation |
| `TRIGGER_ANIMATION` | `1<<4` | record-only |
| `ATTACH` | `1<<5` | record-only |
| `CHANGE_LINKS` | `1<<7` | record-only |
| `TRACK_CAMERA` | `1<<10` | cooperation |
| `CONTROL_CAMERA` | `1<<11` | cooperation |
| `TELEPORT` | `1<<12` | API action |
| `EXPERIENCE` | `1<<13` | record-only |
| `SILENT_ESTATE_MANAGEMENT` | `1<<14` | record-only |
| `OVERRIDE_ANIMATIONS` | `1<<15` | record-only |
| `RETURN_OBJECTS` | `1<<16` | record-only |

`TRIGGER_ANIMATION` / `OVERRIDE_ANIMATIONS` are record-only (the sim plays them;
this refines the A1 draft, which listed them as client-actionable but noted
"nothing client-side"). The 3 cooperation flags reuse event surfaces `sl-proto`
already emits — `TAKE_CONTROLS` via `Event::ScriptControlChange` /
`ScriptControl`, `TRACK_CAMERA` / `CONTROL_CAMERA` via the follow-cam events
(`FollowCamProperty` / `FollowCamPropertyValue`). `TELEPORT` is the only flag
with an autonomous action, routed through the existing `Event::ScriptTeleport`
(→ `Session::teleport_to`); its auto-vs-manual policy is A4's call.

### State-model reference (from A2)

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

### Grant/deny & revoke reference (from A3)

How the registry (A2) is written, revoked, and read. The simulator stays
authoritative; every write here mirrors a wire action the client just took, and
the mirror is deliberately kept *conservative* (it never reports a permission
gone unless the sim actually drops it).

**Recording a grant (`answer_script_permissions`).** Recording happens at the
*end* of the existing method, after the `ScriptAnswerYes` is queued — the mirror
follows the wire, never leads it. The method gains one parameter,
`experience_id: Option<ExperienceKey>`: it is the only `ScriptGrant` datum not
derivable at answer time (`task_id` / `item_id` are already params; `kind` /
`circuit` come from the A2 `holder_kind(task_id)` helper), and the driver
already holds it on the `ScriptPermissionRequest` it is answering — passing it
back keeps A2's "no outstanding-request tracking" decision intact. New shape:

    answer_script_permissions(task_id, item_id, permissions, experience_id, now)

Recording rule, on the holder `ScriptHolder { task_id, item_id }`:

- **Non-empty grant** → build `ScriptGrant { granted: permissions, kind,
  circuit, experience_id }` (kind/circuit from `holder_kind`) and **insert**,
  *replacing* any prior entry for that holder — a later `llRequestPermissions`
  supersedes the earlier grant, matching the sim (one live grant per script).
- **Empty grant** (`permissions.is_empty()`, i.e. an explicit deny / "grant
  nothing") → **remove** any existing entry. A `ScriptAnswerYes` with
  `Questions = 0` denies/replaces server-side, so the faithful mirror is
  *no entry*. **This resolves A2's open question: a deny is the absence of a
  registry entry — empty entries are never stored**, so accessors need no
  "entry-but-empty" case (absent ≡ empty granted).

**The granular revoke (`RevokePermissions`, wire `Low 193`).** The wire message
is **object-scoped** (`ObjectID` + `ObjectPermissions: U32`), not script-scoped,
and the sim **honours only the animation bits** (`TRIGGER_ANIMATION |
OVERRIDE_ANIMATIONS`) per Firestorm. New command + method:

    Command::RevokePermissions {
        object_id: ObjectKey, permissions: ScriptPermissions,
    }
    Session::revoke_permissions(object_id, permissions, now) -> Result<(), Error>

- **Send:** a new `circuit.send_revoke_permissions(object_id, permissions, now)`
  builds `AnyMessage::RevokePermissions` (`AgentData` + `Data { object_id,
  object_permissions: permissions.0 }`), `Reliability::Reliable` — same pattern
  as `send_force_script_control_release` / `send_script_dialog_reply`. The full
  requested bitfield goes on the wire (faithful to the caller's request).
- **Mirror update:** for every grant whose `holder.task_id == object_id`
  (object-scoped → may touch several scripts in one object), clear from
  `granted` only `permissions & (TRIGGER_ANIMATION | OVERRIDE_ANIMATIONS)` — the
  bits the sim will actually drop. Clearing the *requested* bits instead would
  desync the mirror (the sim still enforces e.g. `TELEPORT`), so the mirror only
  follows what the server honours. A grant whose `granted` becomes empty is
  **removed** (same invariant as the deny path: no empty entries).
  `TAKE_CONTROLS` is *not* revocable this way — releasing controls is
  `release_script_controls`.

**`release_script_controls` (`ForceScriptControlRelease`) mirror update.** This
forcibly releases **all** taken controls agent-globally (the sim echoes a
`ScriptControlChange(release)`). On send, the session resets its
*taken-controls* tracker (the session-global set designed in A6) to empty
immediately, rather than waiting for the echo. It does **not** touch
`script_grants`: the `TAKE_CONTROLS` *grant* persists (the script may re-take),
so "permission granted" (registry) and "controls currently taken" (A6 tracker)
stay separate concerns. The concrete clear lands with the A6 tracker; A3 only
fixes the policy (B5 below carries it).

**Query accessors (public; A2 registry types stay private).** The library user
reads the mirror through public signatures returning public types —
`ScriptHolder` / `ScriptGrant` / `HolderKind` are never exposed:

- `granted_permissions(&self, task_id: ObjectKey, item_id: InventoryKey) ->
  ScriptPermissions` — the granted subset for one script, `ScriptPermissions`
  empty when there is no grant (absent ≡ empty, per the deny rule).
- `script_grants(&self) -> impl Iterator<Item = ScriptGrantInfo> + '_` — every
  current grant, as a new small `#[derive(Clone, Copy)]` **public** view:

      struct ScriptGrantInfo {
          task_id: ObjectKey, item_id: InventoryKey,
          granted: ScriptPermissions,
          is_attachment: bool,            // HolderKind::Attachment, flattened
          experience_id: Option<ExperienceKey>,
      }

  The internal `circuit: Option<CircuitId>` (reset-scoping only) is **not**
  surfaced; `is_attachment` flattens `HolderKind` without leaking the enum.
- `script_controls(&self) -> …` — reserved here, **return type finalized in A6**
  (the taken-controls tracker, likely the live `ControlFlags` or an iterator of
  `ScriptControl`). A3 fixes only the name and the "reads the A6 tracker"
  intent.

No new `Event` is emitted on recording: a grant/deny/revoke is a *local* API
call the driver itself just made, so there is nothing inbound to report (A7
confirms the inbound event surface is unchanged).

### Tasks

- [ ] **B1 (from A1). Encode the per-flag role classifier in `sl-proto`.** Add a
      `PermissionRole` enum (`RecordOnly` / `Cooperation` / `ApiAction`) plus a
      total mapping from each `ScriptPermissions` bit to its role, per the table
      above, in a client-side module (e.g. `sl-proto/src/types/script.rs`) —
      kept in `sl-proto`, never pushed to shared `sl-types` (the flags
      themselves stay client-agnostic there). This is the canonical encoding of
      the A1 classification that A4's auto-act policy branches on; the grant
      registry (A2) still stores the raw granted `ScriptPermissions` bitfield
      wholesale, because the 8 record-only flags need no handler, the 3
      cooperation flags reuse existing event surfaces, and only `TELEPORT`
      carries an action (via `Event::ScriptTeleport`).
- [ ] **B2 (from A2). Add the grant-registry state model to `Session`.** In
      `sl-proto/src/session.rs`, add the private types `ScriptHolder`
      (`{ task_id: ObjectKey, item_id: InventoryKey }`, deriving `Ord` for the
      `BTreeMap` key) and `ScriptGrant` with fields
      `granted: ScriptPermissions`, `kind: HolderKind`,
      `circuit: Option<CircuitId>`, and `experience_id: Option<ExperienceKey>`;
      plus the private `HolderKind` enum (`Attachment` / `InWorld`), and the
      field `script_grants: BTreeMap<ScriptHolder, ScriptGrant>` beside `sit` /
      `teleport` (init empty in the constructor at `methods.rs:138`). Add a
      private `object_by_full_id(&self, ObjectKey) -> Option<&Object>` helper
      (scan `self.objects`) and a private
      `holder_kind(&self, task_id: ObjectKey) -> (HolderKind, Option<CircuitId>)`
      that applies the § State-model reference detection rule (attachment iff
      the cached object `attachment_point().is_some()` and parented to our own
      avatar object whose `full_id == agent_id`; else in-world / not-found).
      Per-flag *behaviour* (recording grants, accessors, the taken-controls
      tracker, resets) is **not** in B2 — it lands with B1/A3/A4/A5/A6 once
      those items are signed off; B2 only introduces the data model and the
      detection helpers, with at least a smoke test constructing a `Session` and
      asserting `script_grants` starts empty. Clippy note: `HolderKind` /
      `ScriptGrant` will be `dead_code` until A3 records into them — land B2
      together with the A3 recording task (or `#[expect(dead_code)]` with a
      reason) to keep the tree warning-clean, per the no-bare-`#[allow]`
      convention.
- [ ] **B3 (from A3). Record grants in `answer_script_permissions`.** Add the
      `experience_id: Option<ExperienceKey>` parameter to
      `answer_script_permissions` (`session/methods.rs`), keeping the existing
      `ScriptAnswerYes` send first, then append the recording: compute
      `ScriptHolder { task_id, item_id }`, and — using the A2 `holder_kind`
      helper for `kind` / `circuit` — **insert**
      `ScriptGrant { granted: permissions, kind, circuit, experience_id }`
      (replacing any prior entry) when `permissions` is non-empty, or **remove**
      the holder's entry when `permissions.is_empty()` (the deny path; never
      store an empty entry). Depends on B2's data model + helper. Update every
      caller for the new parameter (`sl-client-tokio`, `sl-client-bevy`, the
      REPL — pass the `experience_id` carried on the `ScriptPermissionRequest`),
      keeping feature parity (see B-tasks from A7). Test: feed a
      `ScriptQuestion` → answer with a subset → assert `granted_permissions`
      returns it; answer again with empty → assert the entry is gone (the A8
      cases).
- [ ] **B4 (from A3). Add the `RevokePermissions` command (wire `Low 193`).**
      Add the object-scoped
      `Command::RevokePermissions { object_id, permissions }` (`command.rs`,
      with the field types from the reference above), dispatch it in
      `session/methods.rs` to a new
      `Session::revoke_permissions(object_id, permissions, now)`, and add
      `circuit.send_revoke_permissions(...)` (`session/circuit.rs`) building
      `AnyMessage::RevokePermissions` (`AgentData` +
      `Data { object_id, object_permissions: permissions.0 }`,
      `Reliability::Reliable`) — mirroring `send_force_script_control_release`.
      After the send, update the mirror: across all grants with
      `holder.task_id == object_id`, clear
      `permissions & (TRIGGER_ANIMATION | OVERRIDE_ANIMATIONS)` from `granted`,
      removing any grant left empty. Depends on B2/B3. Test: grant animation +
      teleport → revoke animation → assert the animation bit cleared but
      teleport kept; revoke the last bit → assert the entry removed.
- [ ] **B5 (from A3, lands with A6). Reset the taken-controls mirror on
      `release_script_controls`.** When the A6 taken-controls tracker exists,
      have `release_script_controls` clear it to empty after queuing
      `ForceScriptControlRelease`, *without* touching `script_grants` (the
      `TAKE_CONTROLS` grant persists). Sequence this task with the A6 tracker
      implementation; A3 fixes only the policy.
- [ ] **B6 (from A3). Add the grant query accessors.** Add public
      `Session::granted_permissions(task_id, item_id) -> ScriptPermissions`
      (empty when absent) and
      `Session::script_grants() -> impl Iterator<Item = ScriptGrantInfo>`, plus
      the public `#[derive(Clone, Copy)] ScriptGrantInfo` view (`task_id`,
      `item_id`, `granted`, `is_attachment` flattening `HolderKind`,
      `experience_id`; the internal `circuit` is not surfaced). Reserve
      `Session::script_controls(...)` for A6 (return type finalized there).
      Depends on B2. This is also what makes B2's `ScriptGrant` / `HolderKind`
      non-`dead_code`, so it can land alongside B3 to drop the
      `#[expect(dead_code)]` shim B2 notes.
