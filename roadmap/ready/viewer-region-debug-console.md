---
id: viewer-region-debug-console
title: Region (sim) debug console
topic: viewer
status: ready
origin: Advanced/Develop menu survey (2026-07-22)
blocked_by: [viewer-ui-widget-scaffold]
refs: [viewer-region-options-debug]
---

Context: [context/viewer.md](../context/viewer.md).

The region console: a command line to the simulator for estate owners /
gods — send a console command, print the streamed reply (the OpenSim region
console exposes region admin commands this way; SL uses it for a smaller
set). Wire: the `SimConsoleAsync` capability request + the
`SimConsoleResponse` event-queue event — verify whether the caps batches
covered the pair and add it to `sl-proto` if not (small; the OpenSim server
side makes it locally testable).

UI: a monospaced scrollback + input floater with history, gated on
estate-manager/god status like the region floater's debug tab
([[viewer-region-options-debug]]).

Reference (Firestorm, read-only): `llfloaterregiondebugconsole`,
`floater_region_debug_console.xml`.

Builds on: the CAPS + event-queue layer (`caps.rs`).
