---
id: viewer-crash-reporter
title: Crash capture & opt-in reporting
topic: viewer
status: ideas
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-about-floater, viewer-settings-backup]
---

Context: [context/viewer.md](../context/viewer.md).

Firestorm ships a whole crash-reports preferences tab
(`panel_preferences_crashreports.xml`): send crash reports (never /
ask each time / always), include settings, include username. Reporting
to firestormviewer.org is FS-project self-reference and stays out of
scope; what we lack is any crash handling at all — an abnormal exit
today leaves nothing but whatever made it into the log.

Scope for us: a panic/crash handler that writes a report bundle on the
way down — backtrace, session-log tail, GPU/adapter info, and the
viewer version/build info the done [[viewer-about-floater]] already
collects — plus a next-start "the viewer crashed last time — view
report?" dialog, and an opt-in destination (a local directory by
default; at most a grid- or user-configured endpoint, no phone-home).
Preferences UI: the ask/always mode and the include-settings /
include-username toggles (include-settings pairs naturally with the
[[viewer-settings-backup]] export format).

Reference (Firestorm, read-only):
`indra/newview/skins/default/xui/en/panel_preferences_crashreports.xml`,
`indra/newview/llfloaterpreference.cpp`.
