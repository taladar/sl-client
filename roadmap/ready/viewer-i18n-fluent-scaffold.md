---
id: viewer-i18n-fluent-scaffold
title: i18n scaffold (Project Fluent via bevy_fluent)
topic: viewer
status: ready
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-i18n-localization
blocked_by: [viewer-ui-widget-scaffold]
---

Context: [context/viewer.md](../context/viewer.md).

The internationalisation foundation, sequenced deliberately right after the UI
scaffold and **ahead of every UI-bearing panel**, so panels are authored
translatable from day one rather than retrofitted. Integrate **`bevy_fluent`**
(Project Fluent `.ftl` behind Bevy assets, runtime locale switching), load the
string bundles, and expose a string-lookup API usable by every panel.

Critically, the lookup must pass **typed named arguments** (numbers via Fluent
`NUMBER()`, gender, names) into Fluent so `.ftl` selectors resolve
singular/plural/gender correctly — e.g. a field label that reflects a count —
`{ $count -> [one] … *[other] … }`. The API takes named typed args,
**never a pre-formatted string**. Expose the current locale as a resource that
also carries the locale's **LTR/RTL direction**, to drive the layout and skin
([[viewer-ui-skin-tokens]] logical properties + `direction`).

**Do not copy** `LLTrans::getCountString` (a hardcoded if-ladder over three
languages, wrong for Polish — which ships); Fluent's plural rules are per-locale
and correct.

The bundle must also carry the **locale's typographic conventions**, not just
prose — punctuation the UI inserts itself, which differs by language and is a
translator's call, not a hardcoded literal. The first concrete case already
exists: the **truncation ellipsis** the tab widget appends to a clipped label
([[viewer-ui-tab-widget]] `TabSpec::ellipsis`, defaulting to Latin `…`) —
Chinese and Japanese conventionally use a centred six-dot `……` instead. So
expose it as a translatable key (e.g. `ui-ellipsis`) that widgets read from the
bundle rather than a per-call literal, and audit for the same shape as more
chrome lands (quotation marks, list separators, the `:` after a field label).

Locale detection/override is [[viewer-i18n-locale-selection]]; sending the
language to the grid is [[viewer-i18n-agent-language]]; chat MT is
[[viewer-i18n-chat-translation]].

Reference (Firestorm, read-only): `newview/skins/default/xui/<lang>/strings.xml`
(18+ languages), `lltrans`.
