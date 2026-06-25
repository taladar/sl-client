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

## 1. RegionInfo formatter prints `$circuitid` instead of numeric values

In the `region_info_handshake(...)` event, several numeric fields render as the
literal placeholder string `$circuitid` rather than their actual values:

```text
region_protocols: $circuitid,
cpu_ratio: $circuitid,
billable_factor: $circuitid.0
```

Meanwhile sibling fields render correctly (e.g. `region_flags: 336626214`,
`water_height: 20.0`). The `.0` tail on `billable_factor` suggests only the
integer portion was substituted.

**Hypothesis:** the REPL event formatter performs reverse placeholder
substitution (replacing values that match a bound context variable with the
variable's name), and these fields hold a value that coincides with the bound
`$circuitid`. This is a display-only artifact — the underlying decoded values
are almost certainly fine — but it makes the formatted output misleading.

**Investigate:** the event-formatting / placeholder-substitution path in
`sl-repl` (`format_event` and the `SessionContext` placeholder binding) and how
`$circuitid` is bound and applied. Confirm whether the raw decoded RegionInfo
values are correct (they should be) and scope the substitution so it does not
rewrite unrelated numeric fields.

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
