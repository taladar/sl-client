---
id: viewer-i18n-cultural-color-meanings
title: Culturally-aware UI colour semantics
topic: viewer
status: ideas
origin: raised during viewer-i18n-fluent-scaffold (2026-07)
blocked_by: [viewer-ui-skin-tokens]
---

Context: [context/viewer.md](../context/viewer.md).

An l10n concern the string scaffold ([[viewer-i18n-fluent-scaffold]]) does not
touch: **colour carries different meaning in different cultures**, and a UI that
hardcodes Western associations reads wrong — or backwards — elsewhere.

The load-bearing examples:

- **Red / green for gain/loss.** Western finance is red = loss / down, green =
  gain / up. In China (and several East-Asian markets) it is **reversed**: red
  = up / lucky / gain, green = down. A viewer that colours an L$ balance delta,
  a market/economy panel, or a rising/falling indicator green-for-good will
  invert its meaning for a large user base.
- **Red as danger vs. celebration.** Western red = warning / error / stop; in
  China red = luck / joy / celebration. A destructive-action button or an error
  chrome that leans on "red = bad" carries a different affect.
- **White / black.** White = mourning in much of East Asia (vs. Western black);
  affects any "memorial", "offline", or empty-state styling that reaches for it.
- **Status/presence dots, minimap factions, warning banners** — anywhere colour
  *is* the message rather than decoration.

This is distinct from raw theming: the fix is not "let the user pick colours"
but "let the *semantic* of a
colour be a locale/culture choice". The natural home is the skin-token system
([[viewer-ui-skin-tokens]]): give **semantic** tokens (`--gain`, `--loss`,
`--danger`, `--celebrate`, `--mourning`, `--positive-delta`) rather than literal
ones, and let a locale/culture profile remap which physical colour each resolves
to — the same indirection the ellipsis key uses for punctuation, applied to
colour. Panels must then reference the semantic token, never a literal
`Color::srgb(...)` for a meaning-bearing surface.

Deliverables sketch:

- A set of **semantic colour tokens** in the skin, separate from literal palette
  tokens.
- A per-culture override map (at least a Western default and an East-Asian
  finance profile that swaps gain/loss), selectable independently of the UI
  language (a user may want English text but their own colour conventions).
- An audit pass: no meaning-bearing UI surface reads a literal colour.
- Interacts with [[viewer-i18n-colorblind-accessibility]] — both argue against
  colour-as-sole-signal, and both resolve through the same semantic-token layer.

Reference (Firestorm, read-only): `colors.xml` (its palette is literal, not
semantic — the gap this task fills), and how nothing there varies by culture.
