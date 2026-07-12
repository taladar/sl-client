---
id: protocol-10
title: Seamless teleport via child-agent circuits
topic: protocol
status: done
origin: ROADMAP.md
---

Context: [context/protocol.md](../context/protocol.md).

**10. ✅ Seamless teleport via child-agent circuits — `EnableSimulator` → child
`UseCircuitCode`, `EstablishAgentCommunication` (CAPS), `CrossedRegion`,
`TeleportFinish` handover · 8 pts. (done)** Not a new surface but a quality
upgrade that *adds value to the Tier-A clients*: replaced the re-login
workaround with real child→root handover so a roaming bot keeps one continuous
session (open IMs, group sessions, agent state) across teleports and region
crossings. `Session` now holds a root circuit plus a `BTreeMap` of child-agent
circuits keyed by simulator address; neighbours are opened with a child
`UseCircuitCode` (no `CompleteAgentMovement`) so they hold the agent's presence
*before* a crossing, and a crossing promotes the pre-opened child to root
(swapping the old root back down to a child — shared neighbours are **not**
dropped, so the general any-side topology keeps its circuits). Datagrams are
routed per-circuit by source address; both runtimes already multiplex circuits
over one socket via `Transmit.destination` / `recv_from`, so neither needed
changes. **Key live finding:** OpenSim (and SL) deliver `EnableSimulator`,
`EstablishAgentCommunication` **and** `CrossedRegion` over the **CAPS event
queue**, not UDP — and the CAPS `Port`/`SimPort` is a plain integer (no
byte-swap, unlike the UDP `IPPORT`). Both UDP and CAPS paths are handled.
*Live-verified: a bot flew east across the Default→East border on one
continuous login (3 neighbours enabled, `RegionChanged` to the East sim, no
re-login) against the local 2×2 multi-region OpenSim.*
