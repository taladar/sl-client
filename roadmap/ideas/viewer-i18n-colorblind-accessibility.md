---
id: viewer-i18n-colorblind-accessibility
title: Colour-blind-accessible UI (no colour-as-sole-signal)
topic: viewer
status: ideas
origin: raised during viewer-i18n-fluent-scaffold (2026-07)
blocked_by: [viewer-ui-skin-tokens]
---

Context: [context/viewer.md](../context/viewer.md).

An accessibility concern adjacent to the culture-colour work
([[viewer-i18n-cultural-color-meanings]]): roughly 8% of men (and ~0.5% of
women) have some colour-vision deficiency, most commonly red-green
(deuteranopia / protanopia), and a UI that encodes meaning **in colour alone**
is unreadable to them. This is the single most common accessibility failure in
game / viewer UIs.

The rule this task enforces: **colour is never the only channel.** Every
meaning-bearing colour must be doubled by a shape, icon, label, or pattern:

- **Presence / status dots** (online / away / busy / offline) — pair each colour
  with a distinct **glyph or shape**, not just a hue.
- **Red/green anything** — friend online vs offline, gain vs loss, valid vs
  invalid field, permission granted vs denied — the exact pair deuteranopes
  cannot separate. Add an icon (✓/✗), a `+`/`-`, or text.
- **Minimap / net map** — avatar dots, parcel ownership bands, faction colours:
  add shape or a hatch pattern.
- **Warning vs. error vs. info chrome** — differentiate by icon and border
  style, not colour temperature alone.
- **Charts / meters** the diagnostics HUD or an economy panel might draw — use
  texture/dash patterns and direct labels, not colour-only series.

Deliverables sketch:

- A palette check: every semantic colour pair must be distinguishable under
  simulated deuteranopia/protanopia/tritanopia, or carry a second channel. A
  small simulation/contrast helper (unit-testable: transform a colour through
  each CVD matrix and assert a minimum perceptual distance between a semantic
  pair) so this is enforced, not aspirational.
- Meaning-bearing widgets ship a non-colour channel by construction (the status
  dot is a shaped glyph, the delta carries a sign, etc.).
- A user toggle for high-contrast / CVD-friendly palettes, resolved through the
  skin-token layer ([[viewer-ui-skin-tokens]]) — the same indirection the
  culture-colour task uses, so the two compose rather than fight.
- WCAG-style contrast minimums for text on its background.

Reference (Firestorm, read-only): its UI is colour-heavy with little
non-colour redundancy — a feature-gap to fill, not a design to copy.
