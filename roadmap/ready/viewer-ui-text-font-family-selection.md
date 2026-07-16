---
id: viewer-ui-text-font-family-selection
title: Deliberate UI font selection (generic families shadow colour emoji)
topic: viewer
status: ready
origin: gap surfaced by viewer-ui-text-foundation (2026-07)
refs: [viewer-ui-text-foundation, viewer-ui-widget-scaffold]
---

Context: [context/viewer.md](../context/viewer.md).

**A trap the whole UI cluster inherits**, found while proving requirement 4 of
[[viewer-ui-text-foundation]]: using a `FontSource` **generic** (`SansSerif`,
`Serif`, `Monospace`, …) silently destroys colour emoji on a typical Linux host.

Why: `fontique` expands a generic through fontconfig's alias list — ~150
families on this host — and most distros add the system
**`Noto Color Emoji` (COLRv1)** to the `sans-serif` alias so emoji render in any
text. parley appends the `Emoji` generic **after** the primary family stack
(`select_font` in `parley::shape`), so that COLRv1 face matches the emoji
codepoints and stops the query *before* the `Emoji` generic — i.e. before our
bundled `CBDT` font is ever offered as a candidate. `swash` cannot paint
`COLRv1`, so the emoji render blank.

Measured with `FontSource::SansSerif` the emoji run resolves to the 4 991
984-byte system COLRv1 font; with a single unpolluted primary family it resolves
to the 10 673 480-byte bundled CBDT font and paints in colour. Per-script
fallback (CJK, Arabic, Hebrew, Cyrillic) is unaffected either way, so nothing is
lost by avoiding the generic.

[[viewer-ui-text-foundation]] worked around this by leaving the demo editor on
the default single-font primary. That is a stopgap, not a font strategy.

Do:

- Decide the viewer's actual UI font stack (bundle our own text faces, à la the
  reference viewer's `DejaVu`/`Roboto`, rather than depending on host aliases —
  this also makes the UI look the same everywhere).
- Give [[viewer-ui-widget-scaffold]] a single font-selection helper every widget
  uses, so no widget hand-picks a `FontSource` generic.
- Add a guard (test or lint) that fails if a generic `FontSource` is used for UI
  text while `swash` still lacks `COLRv1`.
- Re-check if `swash` gains `COLRv1` support: that would remove the whole hazard
  and let the host's emoji font be used directly.
