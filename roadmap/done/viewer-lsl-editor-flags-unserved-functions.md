---
id: viewer-lsl-editor-flags-unserved-functions
title: The script editor marks five functions Second Life compiles as undefined
topic: viewer
status: done
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

## Resolution (2026-11-12)

The list moved to `sl-lsl` (`sl-lsl/keywords_lsl_unserved.xml`, exposed as
`sl_lsl::SECOND_LIFE_UNSERVED`) and gained an `llsd-lsl-syntax-version`, so
it is an `LSLSyntax` document of its own: `sl-wire`'s
`second_life_unserved_syntax` decodes it, and `sl-lsl-runtime`'s `build.rs`
reads it through a build-dependency on `sl-lsl` (its supplement check now
allows the version key). `LslSyntax::supplement` adds the symbols a table
lacks, keeping its own entries.

The script editor's `GridLslSyntax` is now the served table supplemented
with the five unless the region's simulator features carry `OpenSimExtras`
(Second Life until the first features arrive, as the inventory's Restore
check reads it). A table not yet served stays empty, since the semantic pass
checks symbols only against a non-empty table. Unit tests in `edit_script`
pin that a call to `llPointAt` / `llStopPointAt` is undefined against the
served table alone and accepted on Second Life, that OpenSim keeps its
document in either arrival order, and that nothing served stays empty.
