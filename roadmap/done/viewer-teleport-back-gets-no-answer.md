---
id: viewer-teleport-back-gets-no-answer
title: A teleport back to a recently left region can get no answer from the grid
topic: viewer
status: done
origin: viewer-own-avatar-broken-after-teleport live runs on aditi (2026-11-23)
refs: [viewer-own-avatar-broken-after-teleport]
---

Context: [context/viewer.md](../context/viewer.md).

## Observation

Teleporting by the world map between Mauve and Ahern on aditi, two of the five
runs that made more than one teleport stalled on the same step: the third
teleport, from Ahern back to Mauve. The viewer logged `teleport timed out: the
grid neither carried it out nor refused it`. In the run that retried, the second
and third requests timed out the same way, so the session did not recover by
asking again.

| Time spent in each region | Runs | Result |
| --- | --- | --- |
| 60 s | 2 | both stalled on the third teleport |
| 30 s | 3 | 34 teleports, none stalled |

Two runs are too few to say the time spent in a region is the cause.

On 2026-11-24 a third 60 s run stalled at the same step, three requests in a
row, after the event-queue worker was changed to switch queues without waiting
for the old long poll ([[protocol-event-queue-switch-waits-on-old-poll]]). The
queue re-targeted within a second of both earlier arrivals, so a stale queue
is not the cause. A 30 s run on the same build made 14 teleports without a
stall.

## To find out

- What the viewer sent for the stalled request, and whether the simulator
  answered anything (`TeleportStart`, `TeleportProgress`, `TeleportFailed`),
  with a wire log of the stalled step.
- Whether Firestorm can make the same trip at the same pace.
- Whether a teleport to a third region still works from the stalled state.

## Found (2026-11-24)

A 60 s run with the outgoing-message trace reproduced the stall on the third
teleport. The world-map request went to Ahern, the current region, and Ahern
answered nothing: no `TeleportStart`, no `TeleportProgress`, no
`TeleportFailed`, although its circuit was alive and pinging. Asking twice more
got the same silence.

The second Ahern visit had already gone wrong when it began. Ahern's HTTP
capabilities answered 404 straight after arrival (`SimulatorFeatures`,
`ExtEnvironment`, `AgentProfile`, `AvatarRenderInfo`). The return had been
handed the same seed and event-queue URLs as the first visit.

Between the visits (64 s) this session sent Ahern nothing. A distant teleport
dropped the source's circuit outright, so Ahern got no acks and no ping
answers. Firestorm never tears a circuit down when it drops a region: its
`disableCircuit` finds no circuit code on the viewer side, so the circuit goes
on acking, answering pings and pinging, and a later visit reuses it. A 30 s
round trip never stalled, which fits Ahern clearing the silent agent's state
after a while. That part is inferred: the simulator side cannot be seen.

## Kept circuits (2026-11-24/25): parity, but not the cause

A distant teleport moves the old root and its children into
`Session::departed` instead of dropping them, and since 2026-11-25 so does a
neighbour retired by `DisableSimulator` or an abandoned transfer. There they
run at the transport level only: acks, duplicate suppression, an answer to the
region's ping, our own ping on the usual cadence, and resends. The next circuit
to the same simulator takes one back under a fresh circuit id, its numbering
continuing, and still sends `UseCircuitCode`.

The first live run (2026-11-25) showed the 2026-11-24 version ending a kept
circuit on `DisableSimulator`. Aditi sends one to the region left behind about
50 s after the agent goes, so a 64 s return still opened a fresh circuit. The
reference keeps its circuit through that: `process_disable_simulator` calls
`disableCircuit`, which finds no circuit code on the viewer side. It drops a
circuit only after 100 s without an answer to its own pings (`circuit_timeout`
in `llstartup.cpp`, `LLCircuitData::checkCircuitTimeout`). Kept circuits now
follow the same rule.

Tests: `a_distant_teleport_keeps_the_circuit_it_left` and
`disable_simulator_keeps_the_retired_child_circuit` in
`sl-proto/tests/lifecycle.rs`.

The second live run (2026-11-25, 60 s per region) **reused** Mauve's kept
circuit on the return (`reusing the circuit kept from an earlier visit`), and
the stall happened anyway, at the same step:

- The return's `TeleportFinish` handed over the **same seed as the login
  visit** (`…/cap/54a9cd1e…` both times). The seed POST answered, but
  `SimulatorFeatures`, `ExtEnvironment` and `AvatarRenderInfo` answered 404.
- The next `TeleportLocationRequest` to Mauve got no `TeleportStart`, and the
  request timed out after 30 s.

So keeping the circuit is not what the region misses.

## Fix (2026-11-25): three gaps to the reference, confirmed live

1. **Every held region's event queue is polled.** Both drivers polled only the
   root's queue and abandoned the old one on a region change. The reference
   starts an `LLEventPoll` for every region whose capabilities name
   `EventQueueGet` (`LLViewerRegion::setCapability`), neighbours included.
   It stops a region's poll only when it removes the region, which for one
   left by teleport is its `DisableSimulator`. The plan now lives in
   `sl-client-common`'s `event_queues::EventQueues`, carried out by both
   drivers. `Session::holds_region` says which regions to poll, and
   `Session::handle_caps_event_from` takes each event with its region. With
   this, a return to a region got a fresh seed, and the neighbours stopped
   repeating `EstablishAgentCommunication` every five seconds (30 in a run
   of 9 arrivals, where earlier runs had hundreds).
2. **A teleport destination's seed is fetched at `TeleportFinish`**, before
   the arrival is confirmed, as `process_teleport_finish` does right after
   `CompleteAgentMovement` (`Session::handover_destination`).
3. **A duplicate is dropped only when flagged `RESENT`**, as the reference's
   `isDuplicateResend`, and a reused kept circuit forgets the inbound numbers
   it saw. The simulator numbers a return visit's circuit from 1 again. With
   the old rule the return's reliable `RegionHandshake` and
   `AgentMovementComplete` read as duplicates of the last visit's. The region
   made the agent root (its queue sent `ParcelProperties` and
   `EnableSimulator`), the client never saw it, and the teleport timed out.
   This was the fourth teleport's stall once the first two fixes were in.

Tests: `sl-client-common` `event_queues` tests, the bevy runner's stub-grid
tests, and in `sl-proto/tests/lifecycle.rs`
`only_a_resend_of_a_packet_already_taken_is_dropped`,
`regions_are_held_until_they_disable_the_agent`,
`a_region_left_by_teleport_is_held_until_disabled`,
`a_neighbours_parcel_event_is_not_taken_as_the_agents` and
`a_neighbours_queue_event_is_taken`.

Live (2026-11-25, aditi): `e2e_teleport_back` passed, 8 teleports at 60 s
per region, with no capability answering 404 and a fresh seed on every
visit. A headless login to the local OpenSim, next to three neighbours,
polled their queues cleanly and logged out. The fake-grid `e2e_arrival`
teleports and crossing pass.

## To test

Run the live check once aditi serves textures again:

```sh
TRACE=sl_proto::session::methods=trace,sl_proto::wire::sent=trace
SL_E2E_BACKEND=process SL_E2E_GRID=aditi \
  SL_E2E_ARTIFACTS="${PWD}/target/live-logs/teleport-back" \
  RUST_LOG="info,sl_proto=debug,${TRACE}" \
  roadmap/coord.sh heavy --exclusive --label teleport-back -- sh -c \
  'cargo test --release -p sl-client-bevy-viewer --test e2e_teleport_back \
   -- --nocapture --test-threads=1 > target/live-logs/teleport-back.log 2>&1'
```

`e2e_teleport_back` makes 8 world-map teleports between Mauve and Ahern with
60 s in each region. A stall fails the test after 120 s.

Read the viewer log under
`target/live-logs/teleport-back/teleport_back/aditi/process/Alpha/viewer.log`:

- `keeping the circuit to a region left behind` on each distant teleport.
- `reusing the circuit kept from an earlier visit` on each return.
- `inbound message on a departed circuit` lines show the region left behind
  still talking.
- No `CAPS GET was rejected … 404` after a return.
- Every `UDP message sent name="TeleportLocationRequest"` followed by
  `TeleportStart` from the same address.

The 2026-11-25 run with all three fixes passed.
