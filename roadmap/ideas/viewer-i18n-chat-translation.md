---
id: viewer-i18n-chat-translation
title: Machine translation of chat / IM
topic: viewer
status: ideas
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-i18n-localization
blocked_by: [viewer-i18n-fluent-scaffold, viewer-chat-history-panel]
---

Context: [context/viewer.md](../context/viewer.md).

Machine-translate incoming chat / IM: send text to a translation service, show
the original + translation together, with per-conversation toggles.

**Kept as an idea pending research** into the translation model / API options —
a self-hosted model vs. a cloud translation API — weighing cost, privacy
(chat/IM is sensitive), offline capability and language coverage. That decision
should be made before this is moved to `ready/`/`blocked/`. The `blocked_by`
records the technical prerequisites (the i18n scaffold and a chat surface to
render the dual text into); it stays in `ideas/` regardless, which is allowed.

Reference (Firestorm, read-only): `lltranslate`, `llfloatertranslationsettings`
(the Google/Azure/DeepL-style provider selection is exactly the research topic
here).
