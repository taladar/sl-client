---
id: viewer-ui-skin-tokens
title: Skin system — design tokens (bevy_flair CSS)
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-ui-framework
blocked_by: [viewer-ui-widget-scaffold]
---

Context: [context/viewer.md](../context/viewer.md).

The skin / theming system, implemented with **`bevy_flair`** — real CSS
(selectors, pseudo-classes, `@keyframes`, `var()`, `@font-face`, hot-reloaded
`.css`), a better skin language than XUI and the natural home for the design
tokens. The reference's skinning is in practice a **design-token exercise**: its
6 skins × 21 themes are almost entirely `colors.xml` (447 named colours) + named
textures (939); **no theme overrides a layout**. So model skins as named colour
/ texture / font tokens with an id-keyed recursive merge for overlays, and
hot-reload the stylesheets at runtime.

For bidi we go beyond the reference: use CSS **logical properties**
(`inset-inline-start`, `margin-inline`, `padding-block`, `text-align: start`)
and a per-locale `direction`, so an RTL locale mirrors the layout with
**no separate skin**. No physical `left`/`right` in tokens or stylesheets.

**Do not copy** whole-file skin replacement (the reason a reference skin forks a
3,500-line `floater_tools.xml` and then breaks every release) — overrides are
token-level only.

Reference (Firestorm, read-only): `newview/skins/*/colors.xml` + textures,
`llui` colour/texture lookups. The [[viewer-ui-notification-host]] and every
panel consume these tokens.
