---
id: viewer-report-abuse
title: Report Abuse floater
topic: viewer
status: ready
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-ui-widget-scaffold]
refs: [api-df1]
---

Context: [context/viewer.md](../context/viewer.md).

The abuse-report floater: pick the reportee (prefilled when opened from an
avatar / object context menu), category, location (prefilled from the current
position), summary and details, attach the automatic screenshot, and submit.
The protocol is already done — [[api-df1]] wired the
`SendUserReportWithScreenshot` capability (screenshot upload + `UserReport`) —
so this is the form UI, the screenshot capture hook (grab the frame *before*
the floater opens over it, as the reference does), and the confirmation /
failure toasts.

Also reached from here in the reference: reporting an object or a parcel, so
the context menus for those get "Report Abuse" entries opening this floater
prefilled.

Reference (Firestorm, read-only): `llfloaterreporter`,
`floater_report_abuse.xml`.

Builds on: [[api-df1]] and the screenshot capture path (`screenshot.rs`).
