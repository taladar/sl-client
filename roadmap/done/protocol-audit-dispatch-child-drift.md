---
id: protocol-audit-dispatch-child-drift
title: dispatch_child is a hand-copied subset of dispatch
topic: protocol
status: done
origin: static code audit (2026-08-26)
points: 5
refs: [protocol-audit-region-handshake-mid-session]
---

Context: [context/protocol.md](../context/protocol.md).

`sl-proto/src/session/methods.rs:1926` — `dispatch_child` mirrors the root
dispatcher by hand, and its own comment admits it (`:2038`, "Mirror the
root-circuit handlers so it animates"). Verified byte-identical pairs:
`AvatarAnimation` `2040` = `3871`, `ObjectAnimation` `2051` = `3885`,
`AvatarAppearance` `2070` = `3828`, `ParcelOverlay` `2022` = `2863`, plus
`SoundTrigger` / `AttachedSound` / `AttachedSoundGainChange` / `PreloadSound`
(`2081-2123` = `4083-4123`, differing only by a comment).

Every fix has to be made twice, and the `RegionHandshake` pair has **already
diverged** — see [[protocol-audit-region-handshake-mid-session]].

Scope: factor the shared handlers into functions parameterised by which circuit
raised the message, so the root and child arms call one implementation. The
mirror is the point; hand-copying is what makes it fragile.

Related, and probably the same change: `dispatch` itself is 2155 lines with 141
`AnyMessage::` arms (`methods.rs:2786`) and its server mirror
(`sim_session.rs:7534`) is 1494 lines with 132. `sim_caps.rs:622 handler_for`
already demonstrates the fn-pointer-table pattern that applies. Note the
workspace enables neither `clippy::too_many_lines` nor `cognitive_complexity`,
which is why these survive an otherwise very strict lint set.

## Fixed (2026-09-20)

The sixteen arms both dispatchers carried are now one `Session::dispatch_shared`
taking a `CircuitRole` (`Root` / `Child`): the three link-level ones
(`StartPingCheck`, `CompletePingCheck`, `PacketAck`), `RegionHandshake`,
`ParcelOverlay`, `AvatarAnimation`, `ObjectAnimation`, `AvatarAppearance`, the
four sound messages, `CoarseLocationUpdate`, `AlertMessage` and the plain
`GenericMessage` / `LargeGenericMessage` envelopes. The role decides exactly
three things, and nothing else branches on it:

- which circuit answers — `circuit_in_role` resolves `self.circuit` or
  `self.children[&from]`, which is also what `handle_datagram` now uses instead
  of its own `is_root` bool and its own if/else;
- how `Event::Ping` is tagged (`child: role.is_child()`);
- whether `complete_arrival` runs after a handshake. A neighbour's handshake is
  never an arrival, and on the root only the arrival transition is once-only —
  `complete_arrival` already guards itself, which is why the *reply* stays
  ungated for both (the fix from
  [[protocol-audit-region-handshake-mid-session]], now unduplicatable).

`dispatch_child` keeps the two arms that mean something only to a child —
`AgentMovementComplete` (commit the deferred teleport handover) and
`DisableSimulator` (retire the circuit) — and calls `dispatch_shared` **before**
them. `dispatch` calls it from its `_` fallback arm instead, **after** its own
arms, so its three root-only refinements of `GenericMessage` still match first:
`emptymutelist` and the two experience features are the agent's, not a region's,
and only the region hosting the agent has them to report. That ordering
difference is the one thing about the two callers that is not symmetric, and it
is what makes the root's extra arms possible without re-copying the plain one.

`handle_datagram`'s `is_root` bool is gone: `circuit_role(from)` returns
`Option<CircuitRole>`, so "traffic from a stranger is ignored" and "which
dispatcher runs" are one decision rather than three places agreeing.

One test, driving six shared messages (`SoundTrigger`, `AttachedSound`,
`PreloadSound`, `ObjectAnimation`, `GenericMessage`, `LargeGenericMessage`)
down the root circuit and then the child circuit and asserting the two event
lists are **equal**. The per-arm tests around it each pin one message on one
circuit; this one pins the mirror, so an arm that grows a root-only quirk fails
even if nobody writes the child half of its test. `ParcelOverlay` and
`CoarseLocationUpdate` are deliberately excluded: they are tagged with the
region that sent them, so their two events differ by design.

### What the "related" paragraph asked for, and why it is not here

The audit expected the `sim_caps.rs handler_for` pattern to apply to `dispatch`
itself. It does not. `handler_for` maps a capability **string** to a
`CapHandler` variant because several distinct URL names route to one handler and
the name carries no type; `AnyMessage`'s variant *is* the discriminant, and each
arm destructures a differently-typed payload. Reproducing `handler_for` here
would mean matching the message to a handler enum and then matching the handler
enum to a body — two matches where there is now one, with the payload type lost
in between. The duplication was the defect and it is gone: `dispatch` went from
2,253 lines to 2,069 and `dispatch_child` from 246 to 57, with one 230-line
`dispatch_shared` standing in for both halves of what they used to each carry.
Splitting what remains across modules is a readability
change, not a de-duplication, and would want its own item (and its own
`multiple_inherent_impl` answer — every `impl Session` in this crate lives in
`methods.rs`, which is why `dispatch_shared` does too and only `CircuitRole`
sits next to `Circuit` in `session.rs`).
