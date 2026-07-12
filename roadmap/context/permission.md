# Context — PERMISSION_ROADMAP.md

Non-task preamble from `PERMISSION_ROADMAP.md` (scope, protocol/implementation
reality, locked decisions, and Phase-B consolidation notes). Tasks split out of
that file carry the `permission` topic; each Phase-A design item folds in its
own `reference (from A*)` section.

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

## Other notes

## Phase B — implementation (tasks produced by Phase A)

Each Phase A item, while it was worked, appended the concrete implementation
tasks it implied here (tagged with the producing item) as a first draft. With
Phase A complete, that draft was **consolidated** into five tasks, then the
2026-06-26 Open-questions sign-off added three more (**B1.5 / B2.5 / B6**) — see
§ Phase B consolidation for the old→new mapping and the runtime-match findings
that drove it. The **references** (`### Classification reference` … below) are
unchanged knowledge; only the task list was reordered/merged. The § Open
questions are now **signed off** (decisions recorded inline there), so Phase B
may start; tick a box only when the step builds, is clippy-clean (restriction
lints), and `cargo test` passes.
Keep `sl-client-tokio`, `sl-client-bevy`, and the REPL at feature parity; never
push client-only types into shared `sl-types`.

### Phase B consolidation (ordering, merges & runtime-match findings)

Phase A appended its implementation tasks incrementally, one batch per item, so
the first-drafted list (the former B1–B11) carried ordering inconsistencies and
dead-code windows that only surface at implementation time. Before any Phase B
code is written, the tasks were consolidated into the five below. Four findings,
verified against the runtime code, drive the merges:

1. **A new `Command` variant is wired asymmetrically.** `sl-client-bevy`'s
   `drive` match on `Command` is **exhaustive** (no wildcard; last arm
   `Command::Logout` at `sl-client-bevy/src/lib.rs:2441`), adding a variant is
   a **compile error** there until an arm is added. `sl-client-tokio` has a
   `Some(Command::Logout) | None` catch-all (`sl-client-tokio/src/lib.rs:1438`)
   that **silently swallows** an unhandled variant, and the REPL builds commands
   from a manual `CommandSpec` list (`sl-repl/src/registry.rs`); a new command
   cannot be an sl-proto-only step — it must land with all three runtime arms
   (bevy forced; tokio + REPL by parity). This folds the former B4/B9 command
   work into the same task as the former B10 wiring.
2. **A new `Event` variant breaks two exhaustive matches.**
   `sl-repl/src/format.rs:292` `event_name` (an exhaustive `const fn`) **and**
   `sl-survey/src/bin/sl-survey.rs:536` `handle_event` (exhaustive union) have
   no wildcard, so `Event::ScriptPermissionState` needs an arm in each (plus the
   REPL `format_event` body). tokio/bevy forward events without matching each
   variant, and `context.rs::apply_event` has a `_ => {}` — those are safe. The
   former B10 named only `format_event`; the new B4 lists all three.
3. **`experience_id` must travel through the answer command.** The runtime
   callers of `answer_script_permissions` do not have the experience to hand —
   `Command::AnswerScriptPermissions` (`sl-proto/src/command.rs:563`) carries
   only `{ task_id, item_id, permissions }`. So that command gains an
   `experience_id: Option<ExperienceKey>` field, filled by the driver from the
   `ScriptPermissionRequest` it answers; session keeps no request state (A2).
4. **`ScriptGrant.circuit` is read only by the circuit-retired reset**
   (`forget_sim_objects`, `sl-proto/src/session/methods.rs:1439`). Writing it at
   record time but first reading it at reset is a dead-code window. Resolved by
   landing the whole grant store — model, record, read, revoke **and** all
   region-leave resets — as one task (new B2), no field is written in one step
   and first read in another, and no `#[expect(dead_code)]` shim is needed.

The consolidation produced five tasks (was eleven): **B1** the role classifier
(unchanged, independent); **B2** the complete grant registry (former
B2+B3+B4+B6+B7); **B3** the complete taken-controls tracker (former B8+B5);
**B4** the query command, snapshot event and runtime wiring (former B9+B10);
**B5** the lifecycle test suite (former B11). The
**2026-06-26 Open-questions sign-off** then added three tasks (now eight total):
**B1.5** (own-avatar id caching + `pcode::AVATAR`, resolving blocker #1, before
B2), **B2.5** (explicit *denied* vs never-asked, resolving #2, after B2 / before
B4), and **B6** (the closed-session login guard, from #3, independent). Each is
a self-contained landing unit that builds, passes `cargo test`, and is
clippy-clean (restriction lints) on its own — no cross-task dead-code shim.
Dependencies point backwards only: B1.5 stands alone; B2 on B1's classifier +
B1.5's own-avatar id; B2.5 on B2's registry; B3 on B2's `Session` field
neighbourhood; B4 on B2's `ScriptGrantInfo` (+ B2.5's denied status) + B3's
`ScriptControlsInfo`; B5 on all of them; B6 is independent.
