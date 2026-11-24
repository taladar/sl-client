---
id: viewer-teleport-back-gets-no-answer
title: A teleport back to a recently left region can get no answer from the grid
topic: viewer
status: in-progress
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

## Fix (not yet confirmed live)

A distant teleport moves the old root and its children into
`Session::departed` instead of dropping them. There they run at the transport
level only: acks, duplicate suppression, an answer to the region's ping, our
own ping on the usual cadence, and resends. A departed circuit ends on
`DisableSimulator` or after the 45 s inactivity timeout. The next circuit to
the same simulator (`open_child_circuit`, which a teleport back uses) takes it
back under a fresh circuit id. Its sequence numbers continue and it still
sends `UseCircuitCode`.

Test: `a_distant_teleport_keeps_the_circuit_it_left` in
`sl-proto/tests/lifecycle.rs`.

Each circuit's sends are now traced under `sl_proto::wire::sent` (book:
`comms/lludp-transport.md`).

Two runs to confirm the fix on aditi failed at login, before any teleport.
The texture service answered 503 89 and 96 times, and the stage's settle wait
timed out.

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

If it passes, move this item to `done/`. If it stalls with the circuit kept,
the cause is elsewhere. The trace of the stalled step is then the starting
point, and Firestorm making the same trip at the same pace is the next
comparison.
