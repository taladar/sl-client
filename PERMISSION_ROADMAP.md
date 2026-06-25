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
- [ ] **A2. Design the state model & keying.** Specify what `Session` stores
  (in `session.rs`, beside `TeleportPhase` / `SitState`): grants keyed by the
  holding script `(task_id: ObjectKey, item_id: InventoryKey)` → granted
  `ScriptPermissions`, plus the holder **kind** (attachment vs in-world object)
  and its circuit/region for reset scoping, the optional `experience_id`, and
  the per-holder set of **taken controls** (`ScriptControl`). Decide whether to
  also track outstanding (un-answered) requests. Reuse the typed keys
  (`ObjectKey` / `InventoryKey`). Define how the attachment-vs-in-world kind is
  determined (the object cache `objects` / attachment tracking).
- [ ] **A3. Design grant/deny recording & the revoke command.** How
  `answer_script_permissions` records the granted subset into the registry (and
  how a partial grant or an explicit deny is represented). Add the missing
  granular revoke: a new `RevokePermissions` `Command` + `Session` method (the
  wire message exists; wire it through `command.rs` / `session/methods.rs` /
  `session/circuit.rs`), and define how `release_script_controls`
  (`ForceScriptControlRelease`) updates the mirror. Define the library-user
  query accessors (e.g. `granted_permissions(holder) -> ScriptPermissions`,
  `script_controls() -> …`).
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
