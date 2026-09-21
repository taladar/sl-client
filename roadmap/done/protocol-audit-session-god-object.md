---
id: protocol-audit-session-god-object
title: Session and SimSession are god objects with 12k-line impl blocks
topic: protocol
status: done
origin: static code audit (2026-08-26)
points: 13
refs: [protocol-audit-extract-lludp-transport, protocol-audit-sim-session-stores]
---

Context: [context/protocol.md](../context/protocol.md).

`sl-proto/src/session/methods.rs:185` —
**one `impl Session` block spans 12696 lines** (185 to 12881) and is the only
impl block in the file. `Session` itself (`session.rs:1200`) is a ~50-field
struct mixing the login response, the reliable transport circuit, six
download/upload registries, five per-circuit world caches, the inventory model,
the chat-session registry and two event queues.

`SimSession` (`sim_session.rs:2342`) is the mirror image: **54 fields**, about
35 of them driver-populated serving stores (`region_materials`, `object_media`,
`object_costs`, `environments`, `parcels`, `experiences`, two inventory trees)
sharing a struct with `unacked` / `seen` / `pending_acks` / `out` / timers.

Two concrete decompositions the code already points at:

- the transport half comes out first — see
  [[protocol-audit-extract-lludp-transport]];
- `handle_caps_event` (`methods.rs:375`) is ~795 lines matching on a raw `&str`,
  with 58 arms mixing string literals (`"ParcelProperties"`, `"TeleportFinish"`)
  and `CAP_*` consts. A mistyped tag falls to `_ =>` (`:1161`) and becomes a
  `Diagnostic::UnknownCapsEvent` rather than a compile error. A typed CAPS-event
  enum makes the surface exhaustive.

Also: `run_timeout` (`:4951`) is a 204-line linear chain of ~10 unrelated
`if now >= timer` blocks (typing prune, inactivity, logout, teleport, resends,
sit, ack flush, agent update, ping, child sweep) with early returns between them
— and three of those branches `return Ok(())` mid-tick (`:5011`, `:4971`,
`:5031`), skipping `process_resends`, the ack flush, `agent_update` and the
whole child loop for that tick.

`poll_timeout` (`:12847`) has a related hole:
`let circuit = self.circuit.as_ref()?;` returns early, so children get no
wakeups when the root is absent, and for children only inactivity / ack-flush /
resend are merged — `agent_update` and `ping` are not, even though `run_timeout`
services them. They only ever fire piggybacked on the root's 1 s wake.

And the `ChatSession` state machine (`chat_session.rs:334-377`) is driven by ~35
scattered `pub(crate)` field writes from `methods.rs` (`session.lifecycle =
Joined` at `:5922`, `:6029`, `:6034`; `typing.insert/remove` at `:3099-3101`,
`:4877`; `participants` at `:3146`, `:3189`, `:4877`, `:7605`; `unread = 0` at
`:6147`). Those transitions belong on `ChatSession`.

For the record, the good parts: only **4 functions >= 200 lines** in the whole
72.5k-line crate, exactly one substantive TODO, zero FIXME/HACK/XXX, zero
`#[allow]`, and `conversions.rs` is 227 small pure functions rather than another
god module. `SESSION_FLOW_COVERAGE` (`sim_session.rs:478`) is a pinned
client-to-server flow-parity table with a `Mirrored` / `Pending` / `Legacy`
status per flow — a genuinely good pattern.

## Done (2026-09-20)

Client side. `Session` is 56 fields → 38, and each of the five findings above
is closed:

- **The CAPS tag space is a type.** `session/caps_event.rs` names all 65 tags
  as `CapsEvent`, `CapsEvent::from_tag` is the only place a wire string is
  compared, and the handler's match over it is exhaustive — a tag added to the
  enum cannot be left unhandled. The two grouped arms that had to re-compare
  the raw string to tell the agent tree from the Library (`message ==
  CAP_FETCH_LIBRARY_ITEM`) read the variant instead.
- **The timer tick is nine named phases.** `run_timeout` reads as the sequence
  it always was; only the two phases that can *end* the session say so, with a
  `Liveness` return, rather than returning from the middle of a long body. The
  timed-out teleport's mid-tick `return` is gone, so a failing teleport no
  longer costs that tick its retransmissions, its owed acks, its agent update
  and the whole child-circuit loop
  (`handover_timeout_still_services_the_rest_of_the_tick`).
- **`poll_timeout` wakes for everything it services.** No deadline is gated on
  the root circuit any more, the root's ping is merged (it never was), and each
  child's agent update and ping are merged instead of riding whatever woke the
  root next (`poll_timeout_merges_a_child_circuits_agent_update`).
- **A child circuit cannot fail the session.** A failed agent-update encode on
  a child was `?`-propagated, which aborted the tick and closed the session
  over a neighbour; it is reported and stepped over, like the ack flush beside
  it.
- **`ChatSession` owns its transitions.** The ~35 scattered field writes are
  named methods on the type (`note_joined`, `note_reinvited`, `note_typing`,
  `note_participant`, `forget_agent`, `mark_read`, `note_voice_offered`,
  `note_voice_membership`, `note_voice_joined` / `_left`,
  `note_server_history`), and `ChatSession::invited` is the one constructor
  that starts a session as an invitation.

Two field groups became types, the way
[[protocol-audit-extract-lludp-transport]] did with `ReliableLink`:

- **`WorldCache`** (`session/world_cache.rs`) — the nine per-circuit mirrors of
  the streamed world (objects, terrain, region handle and flags, parcels, time
  dilation, own-avatar id, linkset-root re-asks, script-request circuits). The
  two operations that must touch all of them at once are methods now instead of
  a checklist: `reset()` (world-resetting teleport, fresh login) and
  `forget_circuit()` (`DisableSimulator`, handover, a child gone quiet). Both
  previously open-coded a *subset* — the world reset left the parcels, region
  flags, own-avatar ids and parent re-asks of circuits that were being torn
  down behind.
- **`Transfers`** (`session/transfers.rs`) — the nine in-flight asset
  registries plus the two id counters. The registries stay directly readable
  (each is a plain per-request store); what the type owns is what is true of
  all of them: the id allocation, the earliest sweep deadline, and the
  task-inventory claim expiry whose two halves have to be swept together.

Not done, and now its own task: the `SimSession` half
([[protocol-audit-sim-session-stores]]).

**The 12k-line `impl` block stays 12k lines**, and that is a workspace
constraint rather than an oversight — splitting an inherent impl across
modules fails the commit hook. See "Decomposing a god object here" in
[context/protocol.md](../context/protocol.md).
