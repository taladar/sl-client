---
id: viewer-i18n-localization
title: Internationalisation & translation
topic: viewer
status: ideas
origin: reference-viewer feature-cluster survey (2026-07)
---

Context: [context/viewer.md](../context/viewer.md).

String catalog / language packs for the UI, locale selection, and machine
translation of incoming chat / IM. Pick a Rust i18n stack (e.g. Fluent) and a
catalog format, and send the agent-language preference to the grid.

**Sequenced deliberately right after the UI framework and ahead of every
UI-bearing stub:** the localization / translation approach — how strings are
declared, catalogued, and looked up — must be decided *before* any UI element
with user-visible text is built, so panels are authored translatable from day
one rather than retrofitted.

Scope: a string-lookup API usable by every panel; pluralisation / gender /
argument interpolation; locale detection + override; optional right-to-left
handling; and a chat/IM translation path (send text to a translation service,
show original + translation) with per-conversation toggles.

Reference (Firestorm, read-only): `newview/skins/default/xui/<lang>/strings.xml`
(18+ languages), `lltranslate`, `llfloatertranslationsettings`,
`llagentlanguage`.

Deps: [[viewer-ui-framework]]; a soft prerequisite for the other UI stubs.
