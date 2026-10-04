---
id: protocol-cap-get-metadata
title: Read a script's experience over GetMetadata
topic: protocol
status: blocked
origin: protocol-reference-capabilities triage (2026-10-04)
refs: [protocol-reference-capabilities, viewer-lsl-editor-save-compile]
blocked_by: [gridspec-task-inventory]
---

Context: [context/protocol.md](../context/protocol.md).

Second Life grants `GetMetadata`. POST `{"object-id":<task>, "item-id":<script>,
"fields":["experience"]}` → `{experience:<uuid>}`, resolved through
`GetExperienceInfo`. Firestorm uses it to show a script's experience in the
script editor and item properties, and to keep it on a recompile (it skips the
lookup without the capability, FIRE-17688).

Our task-script upload always sends `experience: None`, dropping a script's
experience on save. Add the command, show the experience, and keep it on
recompile ([[viewer-lsl-editor-save-compile]]).

Every capability adopted here goes into `REQUESTED_CAPABILITIES`
(`sl-proto/src/session.rs`), is used where the region grants it, keeps the
existing path where it does not, and is served by the fake grid per flavour as
`seed-capabilities` measured it (`book/src/gridspec/capabilities.md`). Shapes
and Firestorm references: `book/src/comms/caps-reference.md`.

## Done together with [[gridspec-task-inventory]]

This capability is adopted inside that gridspec task, which measures the
feature on both grids over both paths, makes the fake grid serve the
capability per flavour, and checks the viewer. Claim that task.
