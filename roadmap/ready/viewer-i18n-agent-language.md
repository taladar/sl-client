---
id: viewer-i18n-agent-language
title: Send agent-language preference to the grid
topic: viewer
status: ready
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-i18n-localization
refs: [viewer-i18n-locale-selection]
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

## Unblocked (audit 2026-10-15)

The language this reports already exists: the persisted `UiLanguage` setting
and the live `UiLocale` resource (`sl-viewer-ui-core/src/i18n.rs`), switched at
runtime from the preferences General tab. What
[[viewer-i18n-locale-selection]] still owes (OS-locale detection) only changes
*which* locale is active, so it no longer blocks this. Nothing sends
`UpdateAgentLanguage` yet, and there is no "share my language with objects"
setting.
