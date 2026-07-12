---
id: aditi-1
title: RegionInfo formatter prints $circuitid instead of numeric values
topic: aditi
status: done
origin: KNOWN_ISSUES_ADITI.md — issue 1 (RESOLVED)
---

Context: [context/aditi-issues.md](../context/aditi-issues.md).

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
