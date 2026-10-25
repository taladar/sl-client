---
id: server-lsl-lib-experience
title: Library tranche — experiences and the experience key-value store
topic: server
status: ideas
origin: LSL-on-the-fake-grid audit (2026-09-20)
blocked_by: [server-lsl-vm-execution, server-lsl-lib-money-permissions,
  gridspec-lsl-experience]
refs: [protocol-experience-search-paging, server-fake-grid-agent-experiences,
  viewer-experience-event-stream]
---

Context: [context/lsl.md](../context/lsl.md).

Experiences are the one big script-facing surface where the fake grid is
*ahead* of the engine: it already has an experience catalogue, the
agent's five experience lists ([[server-fake-grid-agent-experiences]]),
region experience settings, the event stream
([[viewer-experience-event-stream]]) and the search paging
([[protocol-experience-search-paging]]). `sl-wire` even has an
`experience` module with the `ScriptQuestion` experience block. What is
missing is the script side.

- **Permissions**: `llRequestExperiencePermissions`,
  `llAgentInExperience`, `llGetExperienceDetails`,
  `llGetExperienceErrorMessage`, and the
  `experience_permissions(key agent_id)` /
  `experience_permissions_denied(key agent_id, integer reason)` events.
  The point of an experience is that a resident grants **once, for the
  experience**, and every script in it is then already permitted — which
  is a different grant lifetime from
  [[server-lsl-lib-money-permissions]]'s per-script one, and the reason
  the two are separate tasks.
- **The key-value store**: `llKeyCountKeyValue`, `llKeysKeyValue`,
  `llReadKeyValue`, `llCreateKeyValue`, `llUpdateKeyValue`,
  `llDeleteKeyValue`, each answering over `dataserver` with a request
  key. This is a *grid-side* store scoped to the experience, not to the
  object — so it survives a rez, a take and a region restart, and two
  objects in one experience share it. The fake grid needs to own it
  alongside the experience catalogue.
- **`llSitOnLink`, `llTeleportAgent`, `llAttachToAvatarTemp` and the
  other experience-only powers**, which are ordinary functions whose
  gate is an experience grant rather than a script permission.

Second Life is the reference; OpenSim's experience support is partial,
so the local grid is a weak oracle and aditi is the real one.

Acceptance: a script in a scenario experience requests permission, the
viewer shows the experience-flavoured question (the notices crate
already has `experience_permission.rs`), and a second script in the same
experience finds itself already permitted; a key written by one object
is read by another in the same experience and not by one outside it.

## From gridspec-lsl-experience (2026-10-25)

Measured from a script in no experience (`book/src/gridspec/lsl.md`
§ Experiences and the key-value store); the in-experience half is
[[gridspec-lsl-in-experience]].

- Second Life: `llRequestExperiencePermissions` answers
  `experience_permissions_denied(agent, 5)` at once, no question to the
  viewer; each key-value call returns a request key and its `dataserver`
  answer is `0,5`, the seven answers not in call order;
  `llAgentInExperience` 0; `llGetExperienceDetails(NULL_KEY)` empty.
- `llGetExperienceDetails` returns name, owner, **experience id**, state,
  state message, **group** — the library table's tooltip puts the group
  third. An unknown id answers empty first, then six fields with state 7.
- `llGetExperienceErrorMessage`: codes 0–18 have texts (in the chapter);
  anything else is `unknown error id`.
- Outside an experience `llTeleportAgent` moves the owner only on Second
  Life (a granting non-owner stays put, silently); OpenSim teleports a
  granting non-owner too. Second Life routes a script teleport through the
  region's telehub and alerts the avatar; OpenSim lands it at the aim.
- OpenSim compiles none of the experience functions or events, so the
  OpenSim flavour should refuse them at compile time.
