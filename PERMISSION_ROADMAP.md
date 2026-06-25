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
  § Phase B** (drafted as 8 record-only, 3 cooperation, 1 API action `TELEPORT`;
  **A4 later reclassified `TELEPORT` as record-only → 9 record-only,
  3 cooperation, 0 API action** — see the A4 correction).
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
- [x] **A4. Decide the auto-act policy (API-concept grants).** Using A1, decide
  which granted permissions the session acts on autonomously versus surfaces for
  the library user: record-only flags need no action; controls/camera are
  surfaced (the user cooperates) and the session only tracks the taken-controls
  set from `ScriptControlChange`; for `TELEPORT`, decide whether a granted
  script-teleport (`Event::ScriptTeleport`) may auto-call `teleport_to` or stays
  user-driven. Keep the library a conduit where it must be, a convenience where
  the API already covers the action. **Done — see § Auto-act policy reference
  (from A4) + the B1 amendment in § Phase B.** Decided: the session takes **no
  autonomous action** on any granted permission — every flag is either a
  *record-only* mirror (the sim enforces) or a *cooperation* surface (the
  runtime routes inputs / camera). **Correction to A1's premise:** `TELEPORT` is
  **not** client-actionable. A granted `llTeleportAgent` is executed
  *server-side* (`DoLLTeleport → World.RequestTeleportLocation`) and reaches the
  client as an ordinary teleport already handled by `TeleportPhase`;
  `Event::ScriptTeleport` is `llMapDestination` — a map beacon that needs **no**
  permission and must **not** auto-call `teleport_to`. So `TELEPORT` is
  reclassified *record-only*, there are **zero** auto-act flags, and B1's
  `PermissionRole` drops its `ApiAction` variant (now two roles).
- [x] **A5. Design the client-mirror reset (the crux).** Per the decided
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
  **Done — see § Client-mirror reset reference (from A5) + task B7 in
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
- [x] **A6. Design inbound control-change & de-facto revoke handling.** Specify
      how `ScriptControlChange` (already `Event::ScriptControlChange`) updates
      the taken-controls registry (a `Take` adds, a `Release` removes), and
      record that a `ScriptControlChange(release)` is the only revoke the sim
      pushes (no inbound `RevokePermissions`). Decide whether a client-sent
      `release_script_controls` / `RevokePermissions` updates the mirror
      immediately. **Done — see § Inbound control-change reference (from A6) +
      task B8 in § Phase B (and the now-unblocked B5).** Decided: the
      taken-controls tracker is a **session-global** `TakenControls` field (no
      holder/object attribution — `ScriptControlChange` carries no object id),
      modelled exactly as the viewer's per-control-bit
      **counts split by `PassToAgent`** (Firestorm `mControlsTakenCount` /
      `mControlsTakenPassedOnCount`): a `Take` block increments each named bit's
      count, a `Release` block saturating-decrements it (removed at 0), so two
      scripts taking the same control survive one releasing — the count model is
      required, a single `ControlFlags` union would not suffice. The inbound
      `ScriptControlChange` handler folds every block into the tracker
      **in addition to** still emitting `Event::ScriptControlChange` (the driver
      routes inputs). `ScriptControlChange(release)` is recorded as the **only**
      revoke the sim pushes (verified: no inbound `RevokePermissions`; OpenSim's
      detach / script release echo a release `ScriptControlChange`, see §
      Inbound control-change reference). Client-sent updates:
      `release_script_controls` clears the tracker to empty
      **immediately on send** (B5; does not wait for the echo — OpenSim's echo
      is `Controls = 0xFFFFFFFF, PassToAgent = false`, which would miss the
      passed-on counts, so clear-on-send is the robust choice and the later
      echo's clamped decrement is a harmless no-op); `RevokePermissions` does
      **not** touch the tracker (it honours only the animation bits, never
      controls — verified in OpenSim's `HandleRevokePermissions`). The
      A3-reserved `script_controls()` accessor return type is finalized as a
      public `ScriptControlsInfo` view (two `ControlFlags` unions, `taken` /
      `passed_to_agent`; counts stay private).
- [x] **A7. Specify the API-surface delta & driver/REPL exposure.** Enumerate
  the new/changed `Command`s (`RevokePermissions`, an optional grant
  convenience), any `Event` changes (inbound likely unchanged), and the new
  `Session` accessors; and how `sl-client-tokio`, `sl-client-bevy`, and the REPL
  expose the commands and a way to query the granted state, at feature parity.
  Draw the boundary: what is sl-proto `Session` state versus what stays
  application policy. **Done — see § API-surface & exposure reference
  (from A7) + tasks B9–B10 in § Phase B.** Decided: two new `Command`s
  (`RevokePermissions` from A3, and a new unit `QueryScriptPermissions`); the
  *optional grant convenience* is **dropped** (a grant only ever *answers* a
  pending request — `AnswerScriptPermissions` is that path). **Key discovery:**
  no runtime can call a live `Session` accessor (`tokio`'s `run` loop owns the
  `Session`; `bevy` boxes it privately in `SlState`; the REPL only caches event
  bindings), and a `Command` cannot carry a `oneshot` reply (it is `Clone` /
  REPL-parsed), so read-only state can reach an application **only as an
  `Event`**. Therefore the grant mirror is exposed by a **query command answered
  by a reply event** (`QueryScriptPermissions` → a new *synthesized*
  `Event::ScriptPermissionState(ScriptPermissionState)`, the crate's first
  local-reply event), keeping all three runtimes at parity via the same
  command-in / event-out path they already use. The **inbound** event surface is
  unchanged (as predicted). New `Session` accessors:
  `granted_permissions` / `script_grants` / `script_controls` (A3/A6) plus an
  A7 `script_permission_state()` snapshot convenience.
- [x] **A8. Define the test & verification strategy.** Plan the
  `sl-proto/tests/lifecycle.rs` and `sim_session.rs` cases (mirroring the new
  `teleport_clears_seat` test): feed a `ScriptQuestion` →
  `answer_script_permissions` → assert the registry; feed a real teleport →
  assert in-world grants cleared but attachment grants kept; feed a neighbour
  crossing → assert grants kept; feed `DisableSimulator` / a detach → assert the
  scoped clears; feed `ScriptControlChange` `Take` / `Release` → assert the
  taken-controls tracking. List the remaining open questions for sign-off before
  implementation (the exact attachment-detection source; whether to expose an
  explicit deny). **Done — see § Test & verification strategy reference,
  task B11 in § Phase B, and § Open questions.** Decided: the
  per-task tests embedded in B3–B10 stay (each lands with its own unit test); A8
  adds **one** consolidated lifecycle suite (B11) of the cross-cutting
  reset/recording cases that mirror `teleport_clears_seat`, all built from the
  **existing** test helpers — `established` / `server_message` / `drain` /
  `drain_events`, `object_update[_in]` to seed the `objects` cache, the
  `enable_neighbour_b` + `CrossedRegion` + `AgentMovementComplete` crossing
  fixture (`lifecycle.rs:1048`), `DisableSimulator` from `sim_b()`
  (`:10551`), `KillObject` (`:10133`), and `sim.send_script_control_change`
  (`sim_session.rs:1856`) — so the suite needs **no new harness**. **Key finding
  the strategy surfaces:** the only case that cannot be written against the
  current code is the **`HolderKind::Attachment` detection**: the session caches
  no own-avatar region-local id (no `pcode::AVATAR` handling in
  `session/methods.rs`), so `holder_kind` cannot yet classify a holder parented
  to our own avatar. This is promoted from a B2 footnote to the **#1 sign-off
  blocker** (§ Open questions): the attachment-vs-in-world teleport-reset test
  (the heart of A5/B7) is unwritable until that plumbing is decided, so B2's
  detection rule must be pinned down before B7 lands. A8 produces **no new
  protocol** — only the test task and the sign-off list.

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
boundary. Roles (final, after A4): **record-only** — the sim enforces
end-to-end, the client only mirrors the grant and takes no action (any effect
arrives later on the ordinary message path) · **cooperation** — inert unless the
runtime routes control inputs or applies camera params; `sl-proto` surfaces the
grant and tracks the live state. There is **no** autonomous-action role — A4
established that no granted permission triggers a client-initiated `Session`
method (see § Auto-act policy reference).

| Flag | Bit | Role |
|------|-----|------|
| `DEBIT` | `1<<1` | record-only |
| `TAKE_CONTROLS` | `1<<2` | cooperation |
| `TRIGGER_ANIMATION` | `1<<4` | record-only |
| `ATTACH` | `1<<5` | record-only |
| `CHANGE_LINKS` | `1<<7` | record-only |
| `TRACK_CAMERA` | `1<<10` | cooperation |
| `CONTROL_CAMERA` | `1<<11` | cooperation |
| `TELEPORT` | `1<<12` | record-only (was "API action" — see A4) |
| `EXPERIENCE` | `1<<13` | record-only |
| `SILENT_ESTATE_MANAGEMENT` | `1<<14` | record-only |
| `OVERRIDE_ANIMATIONS` | `1<<15` | record-only |
| `RETURN_OBJECTS` | `1<<16` | record-only |

`TRIGGER_ANIMATION` / `OVERRIDE_ANIMATIONS` are record-only (the sim plays them;
this refines the A1 draft, which listed them as client-actionable but noted
"nothing client-side"). The 3 cooperation flags reuse event surfaces `sl-proto`
already emits — `TAKE_CONTROLS` via `Event::ScriptControlChange` /
`ScriptControl`, `TRACK_CAMERA` / `CONTROL_CAMERA` via the follow-cam events
(`FollowCamProperty` / `FollowCamPropertyValue`). **`TELEPORT` is record-only,
not an action** (the A1 draft misclassified it): a granted `llTeleportAgent`
teleports the agent *server-side* and arrives as a normal teleport handled by
`TeleportPhase`, so the client only mirrors the grant. `Event::ScriptTeleport`
(`llMapDestination`) is a **separate, permission-less** map beacon — not the
`TELEPORT` grant — and is left as a passthrough (A4).

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

### Auto-act policy reference (from A4)

The decision: **the session takes no autonomous action on any granted
permission.** Every one of the 12 flags is either *record-only* (the sim
enforces end-to-end; the client mirrors the grant and any effect arrives on the
ordinary message path) or *cooperation* (inert until the runtime routes control
inputs or applies camera params). No grant maps onto a client-initiated
`Session` method — so there is nothing for A4 to "auto-act". This keeps the
library a pure conduit/mirror and leaves all policy (whether to cooperate, when
to revoke) to the driver.

**Why `TELEPORT` is not an action (the A1 correction).** A1 drafted `TELEPORT`
as "API action → `Session::teleport_to` via `Event::ScriptTeleport`". The
protocol disproves both halves:

- A granted `llTeleportAgent` / `llTeleportAgentGlobalCoords` runs
  **server-side**: OpenSim's `DoLLTeleport` calls
  `World.RequestTeleportLocation` / `RequestTeleportLandmark`, i.e. the sim
  teleports the agent itself. The client receives a normal teleport
  (`TeleportStart` → `TeleportLocal` / region handoff → `TeleportFinish`)
  already driven by `TeleportPhase`. There is no client-initiated step, so
  a granted `TELEPORT` needs **no** auto-act — it is *record-only*.
- `Event::ScriptTeleport` (`ScriptTeleportRequest`, from `llMapDestination`) is
  a **map beacon that requires no permission at all** — Firestorm's
  `process_script_teleport_request` only tracks the location on the world-map
  floater (gated on `ScriptsCanShowUI`), it does **not** teleport. It is
  unrelated to the `TELEPORT` grant. The session must therefore **not**
  auto-call `teleport_to` on it; it stays a passthrough event the driver may act
  on (open a map, offer a teleport) entirely at its discretion.

**Cooperation flags — surfaced, never auto-acted.** `TAKE_CONTROLS` is surfaced
via `Event::ScriptControlChange`; the runtime routes the avatar's control inputs
and `sl-proto` only mirrors the live *taken-controls* set (the A6 tracker, fed
by `ScriptControlChange` Take/Release). `TRACK_CAMERA` / `CONTROL_CAMERA` are
surfaced via the follow-cam events (`FollowCamProperty` /
`FollowCamPropertyValue`); the runtime applies the camera params. The session
records the grant but initiates nothing.

**Consequence for the registry.** A4 changes only *roles/policy*, not storage:
the registry still stores all granted bits wholesale (B2/B3 unchanged). Because
there are now **zero** auto-act flags, B1's `PermissionRole` enum collapses from
three variants to two (`RecordOnly` / `Cooperation`) — see the B1 amendment.

A4 produces **no new implementation task**: "no autonomous action" is the
absence of code. Its only code-facing output is the B1 amendment below; the
`Event::ScriptTeleport` passthrough already exists and is intentionally left
untouched.

### Client-mirror reset reference (from A5)

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
the A6 tracker, sequenced like B5):

- **Not** cleared on real teleport, neighbour crossing, or `DisableSimulator`.
  The tracker is agent-global and cannot be attributed to the in-world holder
  being left behind; and the viewer is faithful here — Firestorm resets
  `mControlsTakenCount` **only** in its constructor and mutates it **only** in
  `processScriptControlChange` (Take `++`, Release `--`); `resetControlFlags`
  touches the ephemeral input flags, **not** the taken counts. So taken controls
  survive a teleport in the viewer and must in the mirror.
- Cleared **per-bit** by an inbound `ScriptControlChange(release)` (A6) — the
  only revoke the sim pushes — and **wholesale** by the explicit
  `release_script_controls` send (B5). **Detach** needs no dedicated controls
  clear: the sim auto-revokes `TAKE_CONTROLS | CONTROL_CAMERA` on detach and
  echoes a `ScriptControlChange(release)`, so the A6 tracker self-corrects on
  that echo (matching the `KillObject` that clears the grant).

**Not reset here:** `close` / logout / relogin. The existing caches (`objects`
et al.) are **not** cleared on `close` either — a `Closed` session is dead and a
relogin builds fresh state through the constructor — so `script_grants` follows
the same convention and adds no `close` hook. (A8 lists this as a sign-off
checkpoint in case a relogin is later made to reuse a live `Session`.)

### Inbound control-change reference (from A6)

The *taken-controls* tracker — the second of the two permission stores (the
grant registry is A2; this one is agent-global). It mirrors which movement
controls scripts are currently holding, fed by the inbound `ScriptControlChange`
and cleared by `release_script_controls`. The simulator stays authoritative;
this is an API-convenience mirror.

**Why it is session-global and count-based (the protocol constraint).**
`ScriptControlChange` (wire `Low 189`) carries **no object/holder id** — only a
`Data` array of `{ TakeControls: BOOL, Controls: U32, PassToAgent: BOOL }` (see
§ Protocol reality). So taken controls cannot be attributed to a holder and do
**not** live in the per-script grant registry (A2); they are a separate
session-global field. Firestorm tracks them as a **per-control-bit count**, in
two arrays split by `PassToAgent` — `mControlsTakenCount` (consumed; the avatar
does not move from the input) and `mControlsTakenPassedOnCount` (also passed to
the agent) — incremented on a take block, decremented (clamped at 0) on a
release block (`LLAgent::processScriptControlChange`). The count model is
**required**, not cosmetic: two scripts may take the same control bit, and one
releasing must not clear it for the other; a single `ControlFlags` union would
lose that. The mirror reproduces this exactly.

**The field** (on `Session`, in `session.rs`, beside `script_grants` / `sit` /
`teleport`, private, reached only through the accessor):

    taken_controls: TakenControls

    struct TakenControls {
        /// Per-control-bit take count for controls the script *consumes*
        /// (PassToAgent = false; the avatar does not move from the input).
        consumed: BTreeMap<u32, u32>,    // single-bit mask -> count
        /// Per-control-bit take count for controls *also* passed to the agent
        /// (PassToAgent = true).
        passed_on: BTreeMap<u32, u32>,
    }

- Keyed by the **single-bit mask** (`u32`, e.g. `0x1`, `0x2`, …), not by
  `ControlFlags` (which derives no `Ord`) — `BTreeMap` keeps the crate's
  deterministic-iteration convention and a sparse map (entries removed at 0) so
  "is this control taken" ≡ "key present". The split mirrors the viewer's two
  arrays; an `iter_bits(controls: ControlFlags) -> impl Iterator<Item = u32>`
  helper (yield each set bit as its own mask) replaces the viewer's
  `for i in 0..TOTAL_CONTROLS { if controls & (1<<i) }` loop without raw
  indexing (clippy restriction lints).

**Inbound update — folding `ScriptControlChange`.** The existing handler
(`session/methods.rs:2676`) already parses each block into a `ScriptControl` and
emits `Event::ScriptControlChange(controls)`. A6 adds, *in the same handler*
(keeping it the single update site), a fold of each block into `taken_controls`
**before** pushing the event (the event is still emitted unchanged — the driver
routes the actual inputs; A7 confirms the inbound event surface is unchanged):

- pick the map by `block.pass_to_agent` (`passed_on` if set, else `consumed`);
- for each set bit in `block.controls` (`iter_bits`): on
  `ScriptControlAction::Take`, `*entry.or_insert(0) += 1`; on `Release`,
  decrement and **remove** the key when it reaches 0 (saturating — never go
  negative, matching the viewer's clamp; a release for an untracked bit is a
  no-op).

A take and a release block can arrive in the same message; they are applied in
order. A `ScriptControlChange` never touches `script_grants` — "permission
granted" (registry) and "controls currently taken" (this tracker) stay separate
(the `TAKE_CONTROLS` grant persists across a release; the script may re-take).

**De-facto revoke — the only revoke the sim pushes.** There is **no** inbound
`RevokePermissions`; a `ScriptControlChange(release)` is the *only* control
revoke the simulator pushes, and the tracker self-corrects from it. Verified in
OpenSim: a per-script release / detach
(`ScenePresence.UnRegisterControlEvents- ToScript`, reached via
`UnRegisterSeatControls` on detach) sends **two** release blocks for that
script's controls — `PassToAgent = false` *and* `true` — so both maps decrement;
this is the same `ScriptControlChange(release)` echo A5 relies on to
self-correct the tracker on detach (no dedicated detach hook needed).

**Client-sent updates to the mirror.**

- **`release_script_controls` (`ForceScriptControlRelease`)** — clears the
  tracker to empty (`consumed` *and* `passed_on`) **immediately on send**, after
  queuing the message, without waiting for the echo (the A3 policy; B5 carries
  it). Clearing on send is the *robust* choice, not merely eager: OpenSim's
  `HandleForceReleaseControls` echoes a single release block with
  `Controls = int.MaxValue (0xFFFFFFFF), PassToAgent = false`, which by the fold
  rule above would decrement only `consumed` and **miss** `passed_on` — so
  trusting the echo alone would leak passed-on counts. Clearing both maps on
  send fixes that, and the later echo's clamped decrement from an already-empty
  map is a harmless no-op. Does **not** touch `script_grants` (the
  `TAKE_CONTROLS` grant persists).
- **`RevokePermissions`** — does **not** touch `taken_controls` at all. The
  command is object-scoped and the sim honours only `TRIGGER_ANIMATION |
  OVERRIDE_ANIMATIONS` (verified in OpenSim's `HandleRevokePermissions`, which
  early-returns unless an animation bit is set and never clears controls);
  `TAKE_CONTROLS` is not revocable this way. Its only mirror effect is on
  `script_grants` (A3/B4).

**Reset on region-leave signals** — none. Per § Client-mirror reset reference
(A5), the tracker is **not** cleared on real teleport, neighbour crossing, or
`DisableSimulator`: it is agent-global (unattributable to the left-behind
in-world holder) and the viewer keeps `mControlsTakenCount` across a teleport
(reset only in its constructor, mutated only in `processScriptControlChange`).
It clears **only** via the inbound release echo (above) and the explicit
`release_script_controls` send.

**The accessor (finalizing A3's reservation).** A3 reserved
`script_controls(&self) -> …`; A6 fixes the return type as a small public view
(the internal `TakenControls` / its counts stay private):

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct ScriptControlsInfo {
        /// Controls scripts hold and *consume* (avatar does not move from
        /// them) — union of `consumed` bits with count > 0.
        pub taken: ControlFlags,
        /// Controls scripts hold that are *also* passed to the agent — union
        /// of `passed_on` bits with count > 0.
        pub passed_to_agent: ControlFlags,
    }

    Session::script_controls(&self) -> ScriptControlsInfo

Two `ControlFlags` unions (fold each map's keys with `|`), mirroring the
viewer's two arrays without exposing the per-bit counts. No new `Event` is
emitted on a client-sent clear — `release_script_controls` is a local call the
driver made; the inbound path already emits `Event::ScriptControlChange` for
every change.

### API-surface & exposure reference (from A7)

The complete public API delta the permission system adds, and how each of the
three runtimes (`sl-client-tokio`, `sl-client-bevy`, the REPL) surfaces it at
feature parity. The boundary: everything that *records, resets, and computes*
the mirror is sl-proto `Session` state; everything that *decides* (cooperate?
revoke? display?) is application/driver policy. The runtimes stay pure conduits
— command-in, event-out — and add no policy.

**The exposure constraint (discovered here, the crux of A7).** None of the three
runtimes can call a *live* `Session` accessor:

- `sl-client-tokio`: `Client::run` takes `self`, and the driver loop then
  **owns** the `Session` (`sl-client-tokio/src/lib.rs` — no `Arc<Mutex>`, no
  snapshot channel, no shared state). The pre-`run` accessors (`agent_id`,
  `session_id`, …) are unreachable once running.
- `sl-client-bevy`: the `Session` is boxed **privately** inside the `SlState`
  resource (`SlInner::Running { session: Box<Session>, … }`); there is no
  `Res<Session>` and no public read accessor on the resource.
- The REPL: `SessionContext` (`sl-repl/src/context.rs`) caches only bindings it
  scrapes from the event stream in `apply_event`; it never reads the `Session`.

So today read-only state reaches an application **only as an `Event`**. A
`Command` cannot carry a `oneshot` reply channel either — `Command` is
`Clone`/`Debug` and is *parsed* by the REPL (`registry.rs`), so it must stay a
plain data enum. **Therefore the only parity-preserving way to expose the grant
mirror is a query `Command` answered by a reply `Event` over the existing events
channel.** That is the design below; it adds no `Arc<Mutex>` to tokio and no
`Res<Session>` to bevy.

**New / changed `Command`s** (`sl-proto/src/command.rs`):

1. `RevokePermissions { object_id: ObjectKey, permissions: ScriptPermissions }`
   — the granular revoke (A3/B4), object-scoped; a struct variant modelled on
   `AnswerScriptPermissions`.
2. `QueryScriptPermissions` — a **unit** variant (modelled on
   `ReleaseScriptControls`) requesting a snapshot of the whole mirror.
   Fire-and-forget *to the session*; the reply rides back as
   `Event::ScriptPermissionState`.
3. **No grant-convenience command.** A1/A4 confirm a grant only ever *answers* a
   pending `llRequestPermissions`; `AnswerScriptPermissions` (gaining
   `experience_id`, B3) is that path. Nothing is granted out of band, so the
   "optional grant convenience" A7 floated is **dropped**.
4. `AnswerScriptPermissions` is unchanged *on the wire*, but its driver dispatch
   gains the `experience_id` argument plumbed from the `ScriptPermissionRequest`
   (B3) — a runtime-wiring change, not a new command.

**`Event` changes** (`sl-proto/src/types/event.rs`):

- **Inbound surface unchanged** (as A7/A3/A6 predicted):
  `ScriptPermissionRequest` (already carries `experience_id`),
  `ScriptControlChange`, `ScriptTeleport`, and the follow-cam events
  (`SetFollowCamProperties` / `ClearFollowCamProperties`) stay exactly as
  they are. No grant/deny/revoke recording emits an event (A3).
- **One new *synthesized* event:**
  `ScriptPermissionState(ScriptPermissionState)` — the reply to
  `QueryScriptPermissions`. It is **not** produced by `Session::poll` from the
  wire; each runtime's command dispatch builds it by reading the session and
  pushes it onto its event sink. This is the crate's **first** synthesized /
  local-reply event — note the precedent it sets (an `Event` that does not
  originate from an inbound message). `ScriptPermissionState` is a new public
  struct:

      #[derive(Debug, Clone)]
      pub struct ScriptPermissionState {
          pub grants: Vec<ScriptGrantInfo>,        // the A3/B6 view
          pub controls: ScriptControlsInfo,        // the A6/B8 view
      }

**New `Session` accessors** (`sl-proto/src/session/methods.rs`), all read-only,
following the `seat()` precedent (public signature, public return types, the
private internal state stays private):

- `granted_permissions(task_id, item_id) -> ScriptPermissions` (A3/B6) — one
  script's granted subset (empty when absent).
- `script_grants() -> impl Iterator<Item = ScriptGrantInfo>` (A3/B6) — every
  current grant.
- `script_controls() -> ScriptControlsInfo` (A6/B8) — the taken-controls union.
- `script_permission_state() -> ScriptPermissionState` (A7) — a convenience that
  collects the two stores into one snapshot (`grants` from `script_grants`,
  `controls` from `script_controls`); the value each runtime wraps in
  `Event::ScriptPermissionState` to answer the query.

**Runtime exposure, at parity** — three identical command-in / event-out shapes:

| Concern | sl-client-tokio | sl-client-bevy | REPL |
|---------|-----------------|----------------|------|
| Send `RevokePermissions` | `match` arm → `session.revoke_permissions(…)`, beside the `AnswerScriptPermissions` arm | `drive` `match` arm → `session.revoke_permissions(…)` | `CommandSpec { name: "revoke_permissions", usage: "<object_id> <permissions-i32>" }`, beside `release_script_controls` |
| Send `QueryScriptPermissions` | `match` arm → push `session.script_permission_state()` onto the events `mpsc` | `drive` `match` arm → `write(SlEvent(Event::ScriptPermissionState(session.script_permission_state())))` | `CommandSpec { name: "query_script_permissions", usage: "" }` |
| Receive the snapshot | the `ScriptPermissionState` event on the events `mpsc::Receiver` | `SlEvent(Event::ScriptPermissionState(..))` Bevy event | a `format_event` arm prints the grants + controls |

- Each runtime already forwards every sl-proto `Event` (tokio over
  `mpsc::Sender<Event>`; bevy as `SlEvent(SessionEvent)`; the REPL via
  `format_event`), so the snapshot reply travels the same path and needs
  **no new transport** — only a `format_event` / `format_command` arm for the
  REPL and the two `match` arms per driver.
- No runtime exposes a live accessor directly (the constraint above); they
  all go command-in / event-out, which keeps them at parity without
  bevy needing a `Res<Session>` or tokio an `Arc<Mutex<Session>>`.

**The boundary — sl-proto `Session` state vs. application policy.**

- **sl-proto `Session` owns:** the grant registry + the taken-controls tracker;
  all recording (B3), revoke-mirroring (B4), region-leave resets (B7) and
  inbound control folds (B8); the four accessors and the
  `script_permission_state` snapshot. It is the single source of mirror truth
  and stays sans-IO; the simulator remains authoritative (the mirror is a
  convenience, never a security boundary).
- **The application / driver owns:** every *decision* — whether to answer a
  request and with which bits, whether/when to `revoke_permissions` or
  `release_script_controls`, whether to cooperate with a `TAKE_CONTROLS` /
  camera grant (route the avatar's inputs, apply the follow-cam params), and
  when to `QueryScriptPermissions` and how to display the snapshot. The session
  **never auto-acts** (A4); the runtimes add **no** policy, only transport.

### Test & verification strategy reference (from A8)

How the permission mirror is proved correct before sign-off, and how the suite
is built. The guiding rule mirrors the rest of the crate: **the test drives the
`Session` exactly as the wire does** — inbound state via `handle_datagram`
(decoded `AnyMessage`s through `server_message`), outbound via `drain` /
`drain_events` — and asserts the public accessors (`granted_permissions`,
`script_grants`, `script_controls`, `script_permission_state`), never the
private fields. This is the `teleport_clears_seat` pattern (`lifecycle.rs:1271`)
applied to grants.

**Two layers, matching the two existing test files.**

- **`sl-proto/tests/lifecycle.rs`** — the *client* `Session` in isolation: feed
  raw server datagrams, drive the answer/revoke/release commands, assert the
  mirror. This is where every recording and reset case lives (it already hosts
  `script_question_surfaces_permission_request` `:3399`,
  `answer_script_permissions_packs_message` `:3467`,
  `script_control_change_surfaces_event` `:2936`, and `teleport_clears_seat`).
- **`sl-proto/tests/sim_session.rs`** — the *paired* `Session` ⇄ `SimSession`
  round-trip via `pump` / `setup`: the sim **produces** the inbound message and
  the client folds it. Reuse `sim.send_script_control_change`
  (`sim_session.rs:1856`) to drive the taken-controls tracker end-to-end (a
  `Take` then a `Release`), proving the fold against a real server-built block,
  not a hand-rolled `AnyMessage`.

**The existing helpers cover every case — no new harness.** Each scenario maps
onto a fixture already in `lifecycle.rs`:

| Scenario | Built from | Asserts |
|----------|-----------|---------|
| Record a grant | `ScriptQuestion` datagram → `answer_script_permissions(.., experience_id, now)` | `granted_permissions(task,item)` == the subset; `script_grants()` yields it |
| Deny = no entry | answer with `ScriptPermissions::empty()` | the holder absent from `script_grants()`; `granted_permissions` empty |
| Re-grant replaces | answer twice for one holder | only the latest subset present |
| Revoke (animation) | `revoke_permissions(obj, TRIGGER_ANIMATION)` after a grant of anim+teleport | anim bit cleared, `TELEPORT` kept; revoke last bit → entry gone |
| Real teleport | `teleport_to` → `TeleportFinish` (as `teleport_clears_seat`) | in-world grant gone, attachment grant kept |
| Neighbour crossing | `enable_neighbour_b` + `CrossedRegion` + `AgentMovementComplete` (`:1048`) | **all** grants kept |
| Circuit retired | `DisableSimulator` from `sim_b()` (`:10551`) | that circuit's grants gone; root's kept |
| Object gone / detach | `KillObject` for the granted object (`:10133`) | that object's grants gone |
| Controls Take/Release | `sim.send_script_control_change` or a `ScriptControlChange` datagram | `script_controls().taken` reflects Take, empties on Release |
| Count model | two Takes of one bit, one Release | bit still in `taken` (count > 0) |
| Pass-to-agent split | Take with `pass_to_agent: true` | lands in `passed_to_agent`, not `taken` |
| Release-on-send | `release_script_controls` after a Take | `script_controls()` empty, the `TAKE_CONTROLS` grant unchanged |
| Snapshot | `script_permission_state()` with a grant + a taken control | both stores reflected |

Seeding the `objects` cache (for `holder_kind` and the `KillObject` reset) uses
`object_update[_in]` (`:9316`): the holder must be present so the reset's
`task_id == full_id` match (B7) and the kind detection (B2) have something to
read. `object_update_in` scopes the object to a chosen `region_handle` /
circuit, which is exactly what the **circuit-retired** test needs to put a grant
on the neighbour circuit and assert `DisableSimulator` drops only it.

**The one case that is not yet writable — and why it gates sign-off.** The
`HolderKind::Attachment` branch of `holder_kind` (B2) classifies a holder as an
attachment *iff* the cached object `attachment_point().is_some()` **and** it is
parented to **our own** avatar. The session caches `agent_id: AgentKey`
(`session.rs:678`) but **not** its own avatar's region-local id, and there is no
`pcode::AVATAR` handling in `session/methods.rs`, so "parented to our avatar"
cannot be evaluated today. Consequently the **attachment-kept-on-teleport** half
of the teleport-reset test (the crux of A5/B7) cannot be written against the
current code — only the in-world-cleared half can. The test strategy therefore
**blocks on resolving the attachment-detection source first** (see § Open
questions): once B2 pins down how the own-avatar parentage is read (cache the
avatar's region-local id at `AgentMovementComplete`, or derive `Attachment` from
a different signal), the attachment object is seeded with `object_update_in`
carrying the attachment `state` nibble and a `parent_id` resolving to the
own-avatar object, and the test becomes a straightforward extension of
`teleport_clears_seat`. Until then, B7's attachment assertion is the suite's
single known gap, called out so it is not silently skipped.

**Coverage discipline.** B3–B10 each carry their own focused unit test (in their
task bodies); B11 is the *integration* layer that exercises the resets and
the two-store interaction together (a grant **and** a taken control surviving /
clearing across the same teleport), the cross-cutting behaviour no single B-task
owns. The suite asserts the **conservative-mirror** invariants throughout: a
revoke clears only the honoured bits (never `TELEPORT`), a teleport clears only
in-world grants (never controls), and no empty grant entry is ever observable.

### Open questions for sign-off (from A8)

Resolve these before starting Phase B implementation; the first blocks B2/B7.

1. **Attachment-detection source (blocker).** `holder_kind` needs to know which
   region-local id is our own avatar to classify a holder as `Attachment`. The
   session does not cache it today. **Proposed:** record the own avatar's
   region-local id when `AgentMovementComplete` / the object cache first sees an
   object whose `full_id == agent_id`, and resolve a holder's `parent_id`
   against it. Decide this (or an alternative signal) before B2 lands, since the
   `HolderKind` default is `InWorld` — shipping without it means **every**
   attachment grant is wrongly dropped on the next teleport. Acceptable interim:
   land B2 with the `InWorld` fallback and a tracked follow-up, but only if
   sign-off accepts attachments not surviving teleport until then.
2. **Explicit deny exposure.** The design represents a deny as the *absence* of
   a registry entry (A3); `granted_permissions` returns empty for both "never
   asked" and "denied". Confirm no caller needs to distinguish the two (i.e. no
   "denied" surface on `ScriptGrantInfo`). The decision so far is **no**: a deny
   is indistinguishable from never-granted, matching the sim.
3. **`close` / relogin reset.** `script_grants` and `taken_controls` add **no**
   `close` hook (A5), matching the `objects` cache convention (a closed session
   is dead; relogin rebuilds via the constructor). Confirm this holds, or add a
   reset if a relogin is ever made to reuse a live `Session`.
4. **First synthesized event precedent.** `Event::ScriptPermissionState` (A7/B9)
   is the crate's first `Event` not produced from an inbound wire message (the
   runtimes synthesize it from a query). Confirm this precedent is acceptable
   before B9/B10 set it; the alternative (a live accessor) is ruled out by the
   exposure constraint in § API-surface & exposure reference.

### Tasks

- [ ] **B1 (from A1, amended by A4). Encode the per-flag role classifier in
      `sl-proto`.** Add a `PermissionRole` enum with **two** variants —
      `RecordOnly` / `Cooperation` (A4 dropped the planned `ApiAction`: no
      granted permission is client-actionable) — plus a total mapping from each
      `ScriptPermissions` bit to its role, per the table above (note `TELEPORT`
      is `RecordOnly`, not an action), in a client-side module (e.g.
      `sl-proto/src/types/script.rs`) — kept in `sl-proto`, never pushed to
      shared `sl-types` (the flags themselves stay client-agnostic there). This
      is the canonical encoding of the A1/A4 classification; the grant registry
      (A2) still stores the raw granted `ScriptPermissions` bitfield wholesale,
      because the 9 record-only flags need no handler and the 3 cooperation
      flags reuse existing event surfaces (`Event::ScriptControlChange` for
      `TAKE_CONTROLS`, the follow-cam events for the camera flags). The session
      takes no autonomous action on any flag, so the classifier exists for the
      driver's benefit (deciding what to surface), not to branch session
      behaviour.
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
- [ ] **B5 (from A3, lands with A6/B8). Reset the taken-controls mirror on
      `release_script_controls`.** Once B8's `taken_controls` field exists, have
      `release_script_controls` (`session/methods.rs`) clear **both** maps
      (`consumed` and `passed_on`) to empty after queuing
      `ForceScriptControlRelease`, *without* touching `script_grants` (the
      `TAKE_CONTROLS` grant persists). Clear on send, not on the echo (per
      § Inbound control-change reference: OpenSim's echo is
      `Controls = 0xFFFFFFFF, PassToAgent = false` and would miss `passed_on`).
      Depends on B8. Test: feed two `ScriptControlChange` takes (one consumed,
      one passed-on) → assert `script_controls()` reflects both → call
      `release_script_controls` → assert `script_controls()` is empty but the
      `TAKE_CONTROLS` grant in `script_grants` is unchanged.
- [ ] **B6 (from A3). Add the grant query accessors.** Add public
      `Session::granted_permissions(task_id, item_id) -> ScriptPermissions`
      (empty when absent) and
      `Session::script_grants() -> impl Iterator<Item = ScriptGrantInfo>`, plus
      the public `#[derive(Clone, Copy)] ScriptGrantInfo` view (`task_id`,
      `item_id`, `granted`, `is_attachment` flattening `HolderKind`,
      `experience_id`; the internal `circuit` is not surfaced).
      `Session::script_controls() -> ScriptControlsInfo` is finalized by A6 and
      ships in **B8** (the taken-controls tracker), not here. Depends on B2.
      This is also what makes B2's `ScriptGrant` / `HolderKind`
      non-`dead_code`, so it can land alongside B3 to drop the
      `#[expect(dead_code)]` shim B2 notes.
- [ ] **B7 (from A5). Reset the grant registry on region-leave signals.** Clear
      `script_grants` following the § Client-mirror reset reference, at the
      existing reset sites in `sl-proto/src/session/methods.rs` (no message is
      sent to the sim):
      - **Real teleport** — add a private `drop_inworld_grants(&mut self)`
      (`script_grants.retain(|_, g| matches!(g.kind, HolderKind::Attachment))`)
      and call it at the two **teleport** `SitState::NotSitting` sites,
      `begin_handover` (`:696`) and `TeleportLocal` (`:1960`) — **not** the
      sit-timeout (`:3072`) or `stand` (`:3427`) sites.
      - **Circuit retired** — in `forget_sim_objects` (`:1439`), beside the
      existing per-circuit drops, add
      `self.script_grants.retain(|_, g| g.circuit != Some(circuit_id))`
      (covers both the `DisableSimulator` and child-expiry callers; both
      child-only).
      - **Object gone / detach** — in the inbound `KillObject` handler
      (`:1180`), read the removed object's `full_id` (already resolved there
      for `region_handle`) and
      `self.script_grants.retain(|h, _| h.task_id != full_id)`.
      - **Neighbour crossing** — `promote_child_to_root` is left untouched
      (keep all grants); assert this in a test.

      Depends on B2 (the `HolderKind` / `circuit` fields) and B3 (so there are
      grants to clear). The taken-controls-tracker resets are **not** in B7 —
      per the reference, the tracker is untouched by these signals and is cleared
      only by the inbound `ScriptControlChange(release)` and
      `release_script_controls` (A6 / B5). Tests (mirroring `teleport_clears_seat`,
      in `sl-proto/tests/lifecycle.rs`): grant an in-world script + an attachment
      script → feed a real teleport → assert the in-world grant gone, the
      attachment grant kept; feed a neighbour crossing → assert both kept; feed
      `DisableSimulator` for a child circuit → assert that circuit's grants gone;
      feed a `KillObject` for a granted object → assert its grant gone.
- [ ] **B8 (from A6). Add the taken-controls tracker + inbound fold +
      `script_controls` accessor.** Per § Inbound control-change reference:
      - **State.** Add the private `TakenControls` struct (two
      `BTreeMap<u32, u32>` fields, `consumed` / `passed_on`, single-bit-mask
      key → take count) and the `taken_controls: TakenControls` field on
      `Session` (`session.rs`), beside `script_grants` / `sit` / `teleport`;
      init empty in the constructor (`methods.rs:138`). Add a private
      `iter_bits(controls: ControlFlags) -> impl Iterator<Item = u32>` helper
      (yield each set bit as its own mask — no raw indexing, clippy-clean).
      - **Inbound fold.** In the existing `AnyMessage::ScriptControlChange`
      handler (`session/methods.rs:2676`), fold each block into `taken_controls`
      **before** the existing `Event::ScriptControlChange` push (the event still
      emits unchanged): select the map by `pass_to_agent`; for each set bit, on
      `Take` increment, on `Release` saturating-decrement and remove the key at
      0. Do **not** touch `script_grants`.
      - **Accessor.** Add the public `#[derive(Clone, Copy)] ScriptControlsInfo`
      view (`taken` / `passed_to_agent`, each a `ControlFlags` union of its
      map's keys) and `Session::script_controls(&self) -> ScriptControlsInfo`
      (folds the counts' keys with `|`; counts stay private). This finalizes
      B6's reservation.
      - **No resets here.** The tracker is untouched by the B7 region-leave
      signals (A5): it self-corrects only on the inbound release echo (this
      handler) and is cleared on `release_script_controls` (B5).
      Depends on B2 (the `Session` field neighbourhood). Surface
      `ScriptControlsInfo` / `script_controls` through `sl-client-tokio`,
      `sl-client-bevy`, and the REPL at feature parity (see B-tasks from A7).
      Tests (`sl-proto/tests/lifecycle.rs` / `sim_session.rs`, mirroring the
      `SimSession::send_script_control_change` path): feed a `Take` for a
      control set → assert `script_controls().taken` contains them; feed a
      matching `Release` → assert empty; feed two takes of the same bit then one
      release → assert still taken (count model); feed a take with
      `PassToAgent = true` → assert it lands in `passed_to_agent`, not `taken`.
- [ ] **B9 (from A7). Add the query command, snapshot type, reply event &
      snapshot accessor in `sl-proto`.** Add the public
      `ScriptPermissionState { grants: Vec<ScriptGrantInfo>, controls:
      ScriptControlsInfo }` struct (`#[derive(Debug, Clone)]`); the
      `Command::QueryScriptPermissions` **unit** variant (`command.rs`, modelled
      on `ReleaseScriptControls`); the
      `Event::ScriptPermissionState(ScriptPermissionState)` variant
      (`types/event.rs`) — documented as a locally-*synthesized* query reply,
      not a wire event (the first such `Event` in the crate); and the
      `Session::script_permission_state(&self) -> ScriptPermissionState`
      accessor collecting `script_grants()` + `script_controls()`. **No
      `Session::poll` change** — the event is emitted by the runtimes (B10), not
      by the session. Depends on B6 (`ScriptGrantInfo` / `script_grants`) and B8
      (`ScriptControlsInfo` / `script_controls`). Test: build a `Session` with a
      recorded grant + a taken control, call `script_permission_state()`, assert
      both stores are reflected in the snapshot.
- [ ] **B10 (from A7). Wire `RevokePermissions` + `QueryScriptPermissions` +
      the snapshot reply through all three runtimes at parity.**
      - **sl-client-tokio** (`src/lib.rs`): add the `Command::RevokePermissions`
      arm (→ `session.revoke_permissions(…)`) and the
      `Command::QueryScriptPermissions` arm (push
      `session.script_permission_state()` onto the events `mpsc::Sender`),
      beside the existing `AnswerScriptPermissions` /
      `ReleaseScriptControls` arms.
      - **sl-client-bevy** (`src/lib.rs`): add the same two arms in the `drive`
      system's `match`; the query arm writes
      `SlEvent(Event::ScriptPermissionState(session.script_permission_state()))`.
      - **REPL** (`sl-repl/src/registry.rs`): add `CommandSpec`s
      `revoke_permissions` (`<object_id> <permissions-i32>`, beside
      `release_script_controls`) and `query_script_permissions` (no args); add
      the matching `format_event` / `format_command` arms
      (`sl-repl/src/format.rs`) for the new event and commands. Optionally
      cache the snapshot in `SessionContext::apply_event`.
      Depends on B4 (the `Session::revoke_permissions` method) and B9 (the query
      command, reply event, snapshot accessor). Keep all three runtimes in
      lockstep — the parity rule. Verify: in the REPL, `revoke_permissions` then
      `query_script_permissions`, and confirm the printed snapshot reflects the
      change (live-grid smoke per the test-avatar setup).
- [ ] **B11 (from A8). Add the lifecycle test suite.** In
      `sl-proto/tests/lifecycle.rs` (and one round-trip in
      `sl-proto/tests/sim_session.rs`), add the cross-cutting reset/recording
      cases from § Test & verification strategy reference, built from the
      existing helpers (`established`, `server_message`, `drain` /
      `drain_events`, `object_update[_in]`, the `enable_neighbour_b` +
      `CrossedRegion` + `AgentMovementComplete` crossing fixture, `KillObject`,
      `DisableSimulator` from `sim_b()`, `sim.send_script_control_change`) — no
      new harness. Cover, at minimum, the rows of the reference table: grant /
      deny-as-absence / re-grant-replaces, the animation-only revoke, the
      teleport reset (in-world cleared, attachment kept — see the gate below),
      the neighbour-crossing keep-all, the circuit-retired and `KillObject`
      scoped drops, the controls Take/Release fold incl. the count model and the
      pass-to-agent split, release-on-send, and the `script_permission_state`
      snapshot. Add at least one **two-store** integration case (a grant **and**
      a taken control surviving / clearing across the same teleport) — the
      behaviour no single B-task owns. Assert the conservative-mirror invariants
      (a revoke clears only the honoured bits, a teleport clears only in-world
      grants never controls, no empty grant entry observable). Depends on
      B2–B10 (it exercises the whole surface). **Gate:** the
      attachment-kept-on-teleport assertion is **blocked** on the
      attachment-detection sign-off (§ Open questions #1) — until that lands,
      write the in-world-cleared half and mark the attachment half with a
      `// TODO(B2-attachment-detection)` rather than silently omitting it. Run
      the full `cargo test -p sl-proto`; clippy-clean (restriction lints) and
      `cargo fmt` before commit.
