---
id: server-lsl-library-table-refresh
title: The vendored library document lacks 50 current functions
topic: server
status: ready
origin: server-lsl-compiler-ir scale oracle run (2026-09-27)
points: 3
refs: [server-lsl-library-surface-table, server-lsl-compiler-ir,
  protocol-sim-lsl-syntax-document]
---

Context: [context/lsl.md](../context/lsl.md).

The library table is generated from `sl-lsl-runtime/keywords_lsl_default.xml`,
the viewer's static `LSLSyntax` file as of Firestorm `684bc1d1` (2024-06).
tailslide's `builtins.txt` knows 50 functions it does not, so a script calling
one fails to compile on the fake grid with "Name not defined within scope"
while Second Life accepts it:

`llChar`, `llOrd`, `llHash`, `llSHA256String`, `llName2Key`, `llRequestUserKey`,
`llLinear2sRGB`, `llsRGB2Linear`, the eleven `llLinksetData*` functions, the
environment family (`llGetEnvironment`, `llSetEnvironment`,
`llReplaceEnvironment`, `llSetAgentEnvironment`, `llReplaceAgentEnvironment`,
`llGetDayLength`, `llGetDayOffset`, `llGetRegionDayLength`,
`llGetRegionDayOffset`, `llGetMoonDirection`, `llGetMoonRotation`,
`llGetSunRotation`, `llGetRegion{Sun,Moon}{Direction,Rotation}`,
`llGetRegionTimeOfDay`), `llGetExperienceList`, `llClearExperiencePermissions`,
`llGetInventoryAcquireTime`, `llGetObjectAnimationNames`,
`llStartObjectAnimation`, `llStopObjectAnimation`, `llGetObjectLinkKey`,
`llGetVisualParams`, `llOpenFloater`, `llSitOnLink`, `llTargetedEmail`, and the
legacy `llPointAt`, `llStopPointAt`, `llRemoteLoadScript` (which the grid still
compiles).

The scale run of `tests/compile_corpus.rs` reports the two corpus scripts
this breaks as "not in the library table".

Wanted: vendor a current document — the one Second Life serves from the
`LSLSyntax` capability (fetch it on aditi, where the client already decodes
it), or a newer viewer copy — with its provenance in the crate `README.md`,
and let the table tests say what moved. Check that the legacy three are in
it; if the served document omits functions the grid still compiles, add
them from a second source rather than leave them uncompilable. Afterwards
the scale run's "not in the library table" list should be empty.
