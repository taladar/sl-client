---
id: protocol-audit-session-god-object
title: Session and SimSession are god objects with 12k-line impl blocks
topic: protocol
status: ready
origin: static code audit (2026-08-26)
points: 13
refs: [protocol-audit-extract-lludp-transport]
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
