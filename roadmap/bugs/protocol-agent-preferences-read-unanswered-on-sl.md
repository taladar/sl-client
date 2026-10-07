---
id: protocol-agent-preferences-read-unanswered-on-sl
title: Reading the agent preferences gets no answer on Second Life
topic: protocol
status: bugs
origin: gridspec-teleport (2026-10-07)
refs: [gridspec-teleport]
---

Context: [context/protocol.md](../context/protocol.md).

## Observation

`Command::RequestAgentPreferences` POSTs an `AgentPreferences` body that sets
nothing and expects the stored set echoed back. OpenSim echoes it. On aditi
(2026-10-07, `teleport-access-refused`, straight after login in a mainland
region) the request produced `CAPS request failed; no reply surfaced
capability="AgentPreferences"` and a `Diagnostic::ExpectedReplyMissing`, and
no `Event::AgentPreferences` ever came. A POST that *sets* `access_prefs` is
answered with the whole stored set on the same capability, in the same
session.

## To do

- Read the response Second Life gives the empty POST (status and body; the
  trace log shows neither) and what the reference viewer does to read the
  stored preferences — it learns them from the `AgentStateUpdate` event the
  region pushes on arrival, which also carries `access_prefs`.
- Either make the read work on Second Life (surface the pushed
  `AgentStateUpdate` preferences as `Event::AgentPreferences`, or send what
  the grid accepts), or refuse the command there with an error instead of
  failing silently.
- `teleport-access-refused` takes the preference to restore from the login
  response because of this; switch it back to the capability once the read
  works.
