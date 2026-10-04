---
id: viewer-i18n-agent-language
title: Send agent-language preference to the grid
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-i18n-localization
blocked_by: [viewer-i18n-locale-selection]
---

Context: [context/viewer.md](../context/viewer.md).

Send the agent's language preference to the grid so region/object scripts and
services can localise for the user. Report the locale chosen in
[[viewer-i18n-locale-selection]] (and whether the user permits sharing it) via
the agent-language update, and re-send it when the locale changes at runtime.

Reference (Firestorm, read-only): `llagentlanguage` (the `UpdateAgentLanguage`
cap and the "share my language with objects" setting).

## Capability (triage 2026-10-04)

`UpdateAgentLanguage` is granted by both grids: POST `{language,
language_is_public}` at login and whenever the setting changes; the reply is
ignored.

Shapes and Firestorm references: `book/src/comms/caps-reference.md`; which grid
grants it: `book/src/gridspec/capabilities.md`.
