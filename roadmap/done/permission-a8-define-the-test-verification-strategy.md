---
id: permission-a8
title: Define the test & verification strategy
topic: permission
status: done
origin: PERMISSION_ROADMAP.md
---

Context: [context/permission.md](../context/permission.md).

**A8. Define the test & verification strategy.** Plan the
`sl-proto/tests/lifecycle.rs` and `sim_session.rs` cases (mirroring the new
`teleport_clears_seat` test): feed a `ScriptQuestion` →
`answer_script_permissions` → assert the registry; feed a real teleport →
assert in-world grants cleared but attachment grants kept; feed a neighbour
crossing → assert grants kept; feed `DisableSimulator` / a detach → assert the
scoped clears; feed `ScriptControlChange` `Take` / `Release` → assert the
taken-controls tracking. List the remaining open questions for sign-off before
implementation (the exact attachment-detection source; whether to expose an
explicit deny). **Done — see § Test & verification strategy reference,
task B5 in § Phase B, and § Open questions.** Decided: the
per-task tests embedded in B2–B4 stay (each lands with its own unit test); A8
adds **one** consolidated lifecycle suite (B5) of the cross-cutting
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
(the heart of A5/B2) is unwritable until that plumbing is decided, so B2's
detection rule must be pinned down before B2 lands. A8 produces **no new
protocol** — only the test task and the sign-off list.

Phase A scopes the planning only; the implementation tasks each Phase A item
produces are appended to **Phase B** below as that item is worked.

## Test & verification strategy reference (from A8)

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
`task_id == full_id` match (B2) and the kind detection (B2) have something to
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
of the teleport-reset test (the crux of A5/B2) cannot be written against the
current code — only the in-world-cleared half can. The test strategy therefore
**blocks on resolving the attachment-detection source first** (see § Open
questions): once B2 pins down how the own-avatar parentage is read (cache the
avatar's region-local id at `AgentMovementComplete`, or derive `Attachment` from
a different signal), the attachment object is seeded with `object_update_in`
carrying the attachment `state` nibble and a `parent_id` resolving to the
own-avatar object, and the test becomes a straightforward extension of
`teleport_clears_seat`. Until then, B5's attachment assertion is the suite's
single known gap, called out so it is not silently skipped.

**Coverage discipline.** B2–B4 each carry their own focused unit test (in their
task bodies); B5 is the *integration* layer that exercises the resets and
the two-store interaction together (a grant **and** a taken control surviving /
clearing across the same teleport), the cross-cutting behaviour no single B-task
owns. The suite asserts the **conservative-mirror** invariants throughout: a
revoke clears only the honoured bits (never `TELEPORT`), a teleport clears only
in-world grants (never controls), and no empty grant entry is ever observable.

### Open questions for sign-off (from A8) — RESOLVED 2026-06-26

All four are decided; each produced a task (see § Tasks). The sign-off is
recorded inline below.

1. **Attachment-detection source (was the blocker) — RESOLVED.** `holder_kind`
   needs to know which region-local id is our own avatar to classify a holder as
   `Attachment`. The session does not cache it today. **Decided: implement the
   proposed caching** — a per-circuit `Option<region-local id>`, `None`
   initially, set the first time either `AgentMovementComplete` fires or the
   object cache sees an object whose `full_id == agent_id` (whichever happens
   first while it is still `None`); resolve a holder's `parent_id` against it.
   This (plus `pcode::AVATAR` handling) is the new **task B1.5**, sequenced
   **before B2** so B2's `holder_kind` does real attachment detection and B5's
   attachment-kept-on-teleport test is writable from the start (no `InWorld`
   interim, no `// TODO(attachment-detection)`).
2. **Explicit deny exposure — RESOLVED: distinguish the two.** A "denied" script
   is **not** the same as a "never-asked" one — the driver's UI that prompts the
   user may want to know it previously denied a script, so the mirror must
   record an explicit *denied* state, distinct from absence. This **reverses**
   A3's "deny is the absence of an entry". It is the new **task B2.5** (a
   tri-state permission status — never-asked / denied / granted-subset —
   recorded at answer time and exposed to the driver), sequenced **before B4**
   (the query surface the UI reads).
3. **`close` / relogin reset — RESOLVED.** A relogin uses a **new** `Session`
   (the existing caches are not reset on `close`, matching the `objects`
   convention). To make that safe, **guard login against a `Session` that has
   already logged out / disconnected** — a closed session must reject a new
   login rather than half-reuse stale state. That guard is the new **task B6**.
   No `close` hook is added to `script_grants` / `taken_controls`.
4. **First synthesized event precedent — RESOLVED: acceptable.**
   `Event::ScriptPermissionState` (A7/B4) is the crate's first `Event` not
   produced from an inbound wire message (the runtimes synthesize it from a
   query). **Decided: synthesized / local-reply events are acceptable**; B4 sets
   the precedent. The alternative (a live accessor) stays ruled out by the
   exposure constraint in § API-surface & exposure reference.
