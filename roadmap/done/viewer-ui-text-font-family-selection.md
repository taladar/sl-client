---
id: viewer-ui-text-font-family-selection
title: Deliberate UI font selection (generic families shadow colour emoji)
topic: viewer
status: done
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

## Outcome (2026-07-16)

The stack lives in `sl-client-bevy-viewer/src/ui_font.rs`, the one place fonts
are chosen. All four asks are done.

**The stack**: **Inter** (variable, upright + italic — one file spans weight
100–900, which is what `TextFont::weight` wants) is the UI body face;
**DejaVu Sans Mono** (regular + bold) is the tabular face, mirroring the
reference's `fonts.xml` `Monospace`. Inter covers Latin/Greek/Cyrillic but not
Hebrew/Arabic/Armenian/Georgian, so a bundled **DejaVu Sans** (four faces) is
wired up *only* as the fontique **script fallback** for those four scripts — not
a selectable role — keeping the text foundation's hard bidi requirement
rendering from bundled faces on every host. This deliberately *replaces* the
host's fallback list for those scripts (the trade: a host with a better Naskh
Arabic no longer wins); scripts we bundle nothing for (CJK, Thai, …) still
resolve through the host.

Every face is registered under a **private family name** (`SL Viewer Sans` /
`SL Viewer Sans Fallback` / `SL Viewer Mono` / `SL Viewer Emoji`) via fontique's
`FontInfoOverride`, not its embedded name — a host that also has Inter (or,
fatally, `Noto Color Emoji`) installed would otherwise merge its copy into the
same family and change which face wins. Registering a family's faces under one
name is also what makes `style` pick a **real** italic rather than a synthesised
slant.

Note bevy's `FontSource::Family` holds a *single* family name, so "Inter with
DejaVu behind it" is not expressible as a family stack — fontique's script
fallback (`FallbackKey::new(script, locale)` + `set_fallbacks`) is the designed
mechanism, and `Query::matches_with` chains `fallback_families` after the
primary. Merging both into one family would **not** work: it tries only the best
attribute match per family, not every face on a coverage miss.

**The helper**: `UiFont::{Sans, Mono}` → `UiFont::Sans.at(13.0)`, always a
`FontSource::Family`. No emoji role: emoji in text of either role already
resolve to the bundled colour font through the `Emoji` generic, and an emoji
role would **not** fix the one case that is broken (`❤️`) — see below.

**Two more defences beyond the helper**, since a stray generic is a silent
failure: the generics we bundle a font for (`SansSerif`, `UiSansSerif`,
`SystemUi`, `Monospace`, `UiMonospace`, `Emoji`) are **re-pointed** at the
private families, so `set_generic_family` replaces the fontconfig alias list
outright and defuses the trap at the root — including for text Bevy shapes
itself; and Bevy's built-in default font is **replaced** with the upright Inter
face.

**A bug this exposed**: every viewer overlay used
`TextFont { font_size, ..default() }`, i.e. `FontSource::Handle(default)` —
Bevy's `FiraMono-subset`. All viewer UI text was rendering in a Latin-only
*monospace subset*. Chat and name tags now go through `UiFont::Sans`,
diagnostics and the pipeline panel through `UiFont::Mono`.

**The guard** is `no_generic_font_source_outside_this_module`: it scans the
crate's sources and fails on any generic `FontSource` outside `ui_font.rs`,
naming file, line and text. Verified to bite by planting one. The ban stays
blanket rather than tracking which generics are bound, because the unbound rest
(`Serif`, `Cursive`, …) still walk the alias list.

**The measurement is now a test.**
`emoji_in_ui_text_resolves_to_the_bundled_ colour_font` shapes `"hi 🎉"` through
a real parley layout on a `FontCx` that has the host's fonts too, and asserts
the emoji run resolves to the bundled CBDT blob. Falsified by unbinding the
emoji generic, which reproduces the figures recorded above exactly: the emoji
run falls to the **4 991 984**-byte host COLRv1 font instead of the
**10 673 480**-byte bundled one. `rtl_scripts_fall_back_to_the_bundled_dejavu`
and `sans_spans_the_weight_axis` cover the Inter/DejaVu split the same way.

**`swash` re-check — still no `COLRv1` as of 0.2.9**, so the hazard stands.
`scale::color` reads the `COLR` **v0** header only (`numBaseGlyphRecords` at
offset 2, `baseGlyphRecordsOffset` at 4, `layerRecordsOffset` at 8) without
checking the table's version field; in a `COLRv1`-only font those counts are
zero, so the lookup finds no layers and the glyph paints as nothing. Its
`Source` enum offers only `ColorOutline` (`COLRv0`) and `ColorBitmap`
(`CBDT`/`sbix`). If a later `swash` adds it, the bundled emoji font and the
emoji binding could go.

**A wrong diagnosis corrected — `❤️` is not fixable from here.**
[[viewer-ui-text-emoji-presentation]] claimed VS16 hearts render monochrome
because `select_font` walks the primary family before the `Emoji` generic,
fixable by reordering. That is **wrong**: parley counts a font a `Complete`
match only if *every* char of the cluster is in its `cmap`, and `U+FE0F`
(`General_Category = Mn`) is filtered out by neither `map_len` nor
`contributes_to_shaping`. No emoji font carries `FE0F` (Noto CBDT does not; nor
does Inter — DejaVu does), so `[2764, FE0F]` maps 1/2 → `0.5` → **rejected**,
even when the emoji family is the *sole* requested family (verified). Reordering
cannot help. That task has been rewritten with the real root cause, and it is
the **same parley `is_emoji` / VS16 area** as
[[viewer-ui-text-grapheme-backdelete]] — one upstream fork fixes both. Switching
the primary to Inter does not change this: Inter has `U+2764` too, and lacks
`FE0F` like every emoji font.

**Simplification**: `viewer-ui-text-foundation`'s deferred `bind_emoji_family`
system, its `EmojiFont` resource and its force-re-shape hack are all gone.
Registering directly into the fontique collection at `Startup` needs no asset
round-trip, so every family exists before the first `PostUpdate` layout and
nothing already-shaped needs invalidating.

`parley` is now a direct dependency of the viewer (at `bevy_text`'s own spec, so
the two unify on one build): `bevy_text` wraps parley but does not re-export it,
and `Blob` / `FontInfoOverride` / `GenericFamily` are only reachable from parley
itself.
