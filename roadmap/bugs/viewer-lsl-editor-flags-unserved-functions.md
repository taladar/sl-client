---
id: viewer-lsl-editor-flags-unserved-functions
title: The script editor marks five functions Second Life compiles as undefined
topic: viewer
status: bugs
origin: server-lsl-library-unserved-functions (2026-10-30)
refs: [server-lsl-library-unserved-functions, viewer-lsl-oracle-misses]
---

Context: [context/viewer.md](../context/viewer.md).

## Observation

`sl-lsl`'s semantic pass resolves a call against the grid's served
`LSLSyntax` document (`semantics.rs`, the `UndefinedFunction` diagnostic),
and the document Second Life serves leaves out `llPointAt`,
`llStopPointAt`, `llRemoteLoadScript`, `llClearExperiencePermissions` and
`llGetExperienceList` — all five of which aditi compiles under Mono and
LSL2 (`script-compile`, 2026-10-30). So the editor underlines a call to one
as undefined, and the save then succeeds.

## Wanted

The editor accepts what the grid accepts. The five, with their signatures,
are in `sl-lsl-runtime/keywords_lsl_unserved.xml`; whether the viewer reads
that list (a dependency on `sl-lsl-runtime`, or the list moved to `sl-lsl`)
or the pass learns them another way is the design question. Only on a
Second Life grid: OpenSim serves its own document.
