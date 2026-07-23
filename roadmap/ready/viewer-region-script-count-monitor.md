---
id: viewer-region-script-count-monitor
title: Announce region script-count changes to chat
topic: viewer
status: ready
origin: debug-settings/chat-lines survey (2026-07-23)
refs:
  [viewer-script-limits, viewer-statistics-floater, viewer-region-top-objects]
---

Context: [context/viewer.md](../context/viewer.md).

An early-warning lag/griefing detector: watch the region's total active
script count (from the sim-stats feed) and print a nearby-chat line when
it changes by more than a threshold between samples — a sudden jump of
hundreds of scripts usually means someone rezzed something heavy.

Scope:

- Sample the active-script total from the `SimStats` ingest.
- When |delta| exceeds the configured threshold
  (`FSReportTotalScriptCountChangesThreshold`), emit a system chat line
  with old → new counts (`FSReportTotalScriptCountChanges`).
- Reset the baseline on region change/teleport so arrival never
  false-positives.

Reference (Firestorm, read-only): the `FSReportTotalScriptCountChanges*`
settings and their consumer (fsfloater/statistics glue).

Builds on: sim-stats ingest (done; the statistics floater task
[[viewer-statistics-floater]] reads the same feed) and the shared system
chat-notice emitter ([[viewer-generated-chat-notices]]).
