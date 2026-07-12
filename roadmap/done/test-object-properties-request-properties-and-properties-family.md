---
id: test-object-properties
title: request properties and properties-family
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 8 — Objects & scene graph `[both]`
---

Context: [context/test.md](../context/test.md).

`object-properties` — request properties and properties-family. `1av`.
The two ways a viewer learns an object's administrative facts, exercised back
to back against one primitive: the selection-based full path
([`Command::RequestObjectProperties`] → `ObjectSelect` → the full
[`Event::ObjectProperties`] with creator/last-owner/perm-block/task-serial/
texture-ids) and the selection-free condensed path
([`Command::RequestObjectPropertiesFamily`] with no request flags → the hover
summary [`Event::ObjectPropertiesFamily`]). The case first watches the same
interest-list stream `object-update-decode` decodes for a primitive, issues
both requests for it, then `DeselectObjects` to leave the scene as found. It
asserts the two replies describe the *same* object consistently (identical
`object_id`, owner, group, sale type, name, and base permission mask) — the
cross-check that both decode paths agree — plus a non-empty name proving a
real object rather than a placeholder. Reuses the `start_location` hook to
force the OpenSim Default Region (no primitive there fails; on SL the
uncontrolled landing region records `partial`). No new client code — the
`RequestObjectProperties*`/`ObjectProperties`/`ObjectPropertiesFamily` surface
all existed; only the new case. Green on OpenSim against the Phase 9 scripted
prim (`SLClientSoundTester`): both replies matched, RTT ≈ 30 ms. `[both]`;
the aditi run is deferred with the batch (no aditi record this session).
