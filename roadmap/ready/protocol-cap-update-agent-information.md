---
id: protocol-cap-update-agent-information
title: Set the maturity preference over UpdateAgentInformation
topic: protocol
status: ready
origin: protocol-reference-capabilities triage (2026-10-04)
refs: [protocol-reference-capabilities, viewer-region-entry-maturity-gate]
---

Context: [context/protocol.md](../context/protocol.md).

Both grids grant `UpdateAgentInformation`. Firestorm sets the maturity
preference by POSTing `{access_prefs:{max:"PG"|"M"|"A"}}` and checks the echoed
`{access_prefs:{max}}`. Our viewer sends `access_prefs.max` through
`AgentPreferences` instead, which Firestorm never uses for maturity — and
nothing has checked that Second Life honours it there (OpenSim serves both from
one handler).

Measure first (a conformance case setting the preference both ways on aditi and
reading it back), then send it the way Firestorm does where granted.

Every capability adopted here goes into `REQUESTED_CAPABILITIES`
(`sl-proto/src/session.rs`), is used where the region grants it, keeps the
existing path where it does not, and is served by the fake grid per flavour as
`seed-capabilities` measured it (`book/src/gridspec/capabilities.md`). Shapes
and Firestorm references: `book/src/comms/caps-reference.md`.
