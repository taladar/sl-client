---
id: protocol-agent-preferences-read-unanswered-on-sl
title: Reading the agent preferences gets no answer on Second Life
topic: protocol
status: done
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

## Done (2026-11-18)

Measured on aditi with a one-shot `sl-repl-tokio` probe: the empty POST is
answered `400 Bad Request` with the body `Bad Request: InvalidArguments`, and
the `AgentStateUpdate` pushed 0.5 s after login carries the whole stored set
under `preferences` (beside `can_modify_navmesh` and `has_modified_navmesh`).
The reference viewer never reads the preferences: it is told them that way.

- The session decodes the push's `preferences` into `Event::AgentPreferences`
  and keeps the last set the grid stated, pushed or echoed after a change.
  `Session::answer_agent_preferences_request` answers a read from it; both
  runtimes call it and POST only when it holds nothing (OpenSim before a
  change, which echoes the empty POST).
- The fake grid imitates both halves: `SimSession::enqueue_agent_state_update`
  pushes the stored set, and the Second Life flavour's `AgentPreferences`
  refuses an empty POST (`SimCaps::with_empty_preferences_post`,
  `ImitatedGrid::empty_preferences_post`).
- `teleport-access-refused` and `teleport-lure-rated` read the preference to
  restore with `Command::RequestAgentPreferences` again, and a set waits for an
  answer naming the value it asked for, since an arrival's push still names the
  old one. Live on aditi: passes, no refused POST.
- Tests: `agent_state_update_preferences_answer_a_later_read` (`sl-proto`),
  `the_stored_preferences_are_read_on_both_flavours` (`sl-fake-grid`).
