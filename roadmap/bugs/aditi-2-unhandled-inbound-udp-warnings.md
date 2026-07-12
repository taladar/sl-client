---
id: aditi-2
title: Unhandled inbound UDP messages (SimStats, SimulatorViewerTimeMessage) log warnings
topic: aditi
status: bugs
origin: KNOWN_ISSUES_ADITI.md — issue 2 (planned)
refs: [missing-batch-1]
---

Context: [context/aditi-issues.md](../context/aditi-issues.md).

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

**Status — planned.** This turned out to be the tip of a broader gap: the client
handles only a subset of the messages a real simulator sends (and sends only a
subset of those it could). Rather than patch these two in isolation, the full
bidirectional coverage is catalogued and batched under the `missing` topic.
`SimStats` and `SimulatorViewerTimeMessage` are batch 1 there
([[missing-batch-1]]); implementing it
closes this issue (the two messages become typed `Event`s instead of
`WARN UnhandledMessage` noise).
