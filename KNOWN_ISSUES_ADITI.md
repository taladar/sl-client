# Known issues observed on the SL Beta grid (aditi)

Minor, non-blocking observations from the first live `sl-repl-tokio`
login/hold/logout smoke test against the Second Life Beta grid (aditi) on
2026-06-25. The session itself succeeded end-to-end (login, ~2-minute hold with
three `request_region_info` liveness probes, clean `logout`, exit 0). None of
the items below broke the run; they warrant further investigation while we work
on real-SL support.

The full trace from that run is saved (uncommitted) for analysis at:
`~/.claude-personal/projects/-home-taladar-devel-new-sl-client/analysis/aditi-smoke-run-2026-06-25.log`

These are SL-specific findings: OpenSim never exercised these paths, so they
only show up against a real Linden Lab simulator.

## 1. RegionInfo formatter prints `$circuitid` instead of numeric values — RESOLVED

In the `region_info_handshake(...)` event, several numeric fields rendered as
the literal placeholder `$circuitid` rather than their values
(`region_protocols: $circuitid`, `cpu_ratio: $circuitid`, `billable_factor:
$circuitid.0`).

**Cause:** the REPL formatter reverse-symbolizes bound context values back to
their `$placeholder` for diffable transcripts. The circuit-instance id
(`$circuitid`) is a small monotonic counter (here `circuit#1`, so value `1`), so
matching it by bare value clobbered every coincidental small integer in the
event's fields.

**Fix:** `$circuitid` is no longer matched by bare value. Because it genuinely
varies run-to-run (teleports/region crossings mint new circuits) it is still
worth symbolizing, so the formatter now matches its distinctive `Debug` wrapper
`CircuitId(<n>)` instead (also the form it takes inside `ScopedObjectId` /
`ScopedParcelId`) — exact, since no unrelated field renders that way. See
`sl-repl` `format::symbolize_circuit_id` and the dropped bare-value branch in
`context::SessionContext::symbolize`, with regression tests in both modules.
Verified live against aditi: the line now reads `region_protocols: 1, cpu_ratio:
1, billable_factor: 1.0`.

## 2. Unhandled inbound UDP messages (warnings)

The run logged 65 `UnhandledMessage` warnings for two message types streamed
continuously by the simulator:

| Message | Template id | Count in run |
| --- | --- | --- |
| `SimStats` | `Low(140)` | 54 |
| `SimulatorViewerTimeMessage` | `Low(150)` | 11 |

These are expected, regularly-emitted simulator messages (their steady ~2s
cadence was in fact our liveness signal). They are not errors, but:

- We currently have no handler/event for them, so useful data (region
  performance stats, in-world time/sun) is dropped.
- At `WARN` level they dominate the log (the 2-minute run produced ~15 MB of
  output), drowning out signal.

**Investigate:** decide whether to (a) implement handlers/events for `SimStats`
and `SimulatorViewerTimeMessage`, and/or (b) downgrade the "no handler" log
level for known-but-intentionally-unhandled messages so they stop flooding the
output.

## 3. Unknown CAPS event `AgentStateUpdate`

One `UnknownCapsEvent message=AgentStateUpdate` warning was logged shortly after
login. This is an event-queue (CAPS) message we do not yet recognize.

**Investigate:** add `AgentStateUpdate` to the known CAPS event set and decode
it (it carries avatar god-level / preferences / hover-height style state), or at
minimum classify it as known-and-ignored so it stops surfacing as "unknown".
