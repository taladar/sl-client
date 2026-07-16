---
id: viewer-ui-text-emoji-presentation
title: Honour emoji-presentation (VS16) over a text font's own glyph
topic: viewer
status: done
origin: gap surfaced by viewer-ui-text-foundation (2026-07)
refs: [viewer-ui-text-foundation, viewer-ui-text-font-family-selection, viewer-ui-text-grapheme-backdelete]
---

Context: [context/viewer.md](../context/viewer.md).

A codepoint carrying the **emoji-presentation selector** (`U+FE0F`, VS16)
renders as its monochrome *text* glyph. Concretely `❤️` (`U+2764 U+FE0F`) is a
flat white heart. Emoji with no text-font glyph — `🎉`, `🔥`, `👨‍👩‍👧‍👦`, `🇯🇵` — are
unaffected and paint in colour.

Per UTS #51 a VS16 sequence requests **emoji presentation** and should prefer an
emoji font even when a text glyph exists (and, symmetrically, `U+FE0E` VS15
requests text presentation and should prefer the text font).

## Root cause (2026-07-16) — not what this task originally said

This task used to claim the cause was font *ordering*: that `select_font` walks
the primary family before the `Emoji` generic, so a text font carrying `U+2764`
wins. **That diagnosis is wrong**, and the fix it proposed ("make VS16 clusters
prefer the emoji family, upstream in `parley::shape::select_font`") **would not
work on its own.** Measured through real parley layouts (see below), there are
*two* independent blockers:

1. **The emoji font is rejected outright for a VS16 cluster.** `parley`'s
   `analysis/cluster.rs` counts a font a `Complete` match only when *every* char
   in the cluster is in its `cmap` (`ratio >= 1.0`). `U+FE0F` is
   `General_Category = Mn`, so it is excluded neither by the control-character
   filter that builds `map_len` (`setup()`) nor by `contributes_to_shaping`
   (`analysis/mod.rs`, which only drops `Control` / `LineSeparator` /
   `ParagraphSeparator` / non-inherited `Format`). **No emoji font has `FE0F` in
   its `cmap`** — Noto's `CBDT` build does not, nor does `Inter`; `DejaVu Sans`
   does. So for `[U+2764, U+FE0F]` the emoji font maps 1 of 2 → `0.5` →
   rejected, while DejaVu maps both → `1.0` → wins. The selector that *requests*
   emoji presentation is precisely what disqualifies the emoji font. Reordering
   cannot help: **the emoji font is rejected even when it is the sole requested
   family** (verified).
2. **`cluster.is_emoji` is not UTS #51-aware.** It is the raw
   `Emoji`/`Extended_Pictographic` property, so it is true for `5`, `#` and `▶`
   as well as `❤` — far too broad to mean "should be coloured". parley already
   tracks `is_variation_selector` in `CharInfo` but leaves it behind
   `#[allow(dead_code, reason = "To be used in more complete emoji checking, in
   select_font")]`.

`U+FE0F` is `Default_Ignorable_Code_Point`; requiring it in the `cmap` is
arguably a plain bug (HarfBuzz hides default-ignorables rather than failing font
selection over them).

## The fix is upstream, and it is shared with the backdelete task

Both blockers are in **`parley`** — *not* swash (which is only the COLRv1
rasteriser we already sidestep by bundling `CBDT`). Better still,
[[viewer-ui-text-grapheme-backdelete]] is the **same** area: `backdelete`
(`editing/editor.rs`) deletes a whole cluster only `if
cluster.is_hard_line_break() || cluster.is_emoji()` — consuming the *same*
`is_emoji` flag that `select_font` (`shape/mod.rs`) uses. One upstream fix
serves both tasks; consider working them together.

Do:

- Fork `linebender/parley` (fontique lives in the same repo) and point the
  workspace at it with `[patch.crates-io]` — that transparently redirects
  `bevy_text`'s parley too, so the whole stack picks it up.
- Fix: exclude default-ignorables (VS16 at minimum) from the cluster mapping
  ratio, and use the `is_variation_selector` flag in `select_font` so VS16
  prefers the emoji family and VS15 the text family.
- Test locally against the viewer, then submit upstream and drop the `[patch]`
  once the fixes land in a release.

Note the possible *third* problem to check once the first two are fixed: with
VS16 no longer required to map, `is_emoji` correct, and the emoji family still
appended *after* the primary, `❤️` may still lose to a primary that covers
`U+2764` (both `Inter` and `DejaVu Sans` do). Ordering may need to change for
VS16 clusters specifically — which is what the original diagnosis guessed at,
and is the *third* step rather than the first.

## Reproducing

`sl-client-bevy-viewer/src/ui_font.rs`'s test module has `resolved_run_lengths`,
which shapes a string through a real parley layout and reports which font blob
each run resolved to (identified by byte length). Measured on the current stack:

| Text | Resolves to |
| --- | --- |
| `❤️` (`2764 FE0F`) | DejaVu Sans (757 076 B) — monochrome, **the bug** |
| `❤` (bare) | bundled CBDT emoji (10 673 480 B) — colour |
| `🎉`, `🇯🇵` | bundled CBDT emoji — colour |

The reference viewer, for comparison, sidesteps all of this by looking glyphs up
**per character** rather than per cluster (`llfontfreetype.cpp`'s `addGlyph`),
so its `FE0F` simply finds no glyph and is skipped; it renders a colour heart
whenever the base font and the monochrome fallbacks lack `U+2764`.

## Outcome (2026-07-16)

`❤️` renders in colour. Confirmed live in the F4 demo panel and guarded by
`emoji_presentation_selector_beats_the_text_font` in
`sl-client-bevy-viewer/src/ui_font.rs`.

Both blockers were in parley, and both are now in the fork we `[patch]` with
(`taladar/parley`, branch `sl-client/0.9-patch`, pinned by rev in the workspace
`Cargo.toml`):

1. the `cmap`/VS16 rejection — **already fixed upstream** in 0.10.0
   (linebender/parley#685); backported here because `bevy_text` 0.19 pins parley
   `0.9.0` and a `[patch]` must stay semver-compatible;
2. the selection *ordering* — written by us, and **not** fixed upstream.
   Verified on parley `main` first that (1) alone still yields a monochrome
   heart.

Submitting (2) upstream is [[viewer-ui-text-parley-pr-vs16]]; dropping the patch
once `bevy_text` moves to parley >= 0.10 and it lands is tracked there too.

The third gap this task speculated about — "ordering may need to change for VS16
clusters specifically" — turned out to be the *actual* fix, not a footnote.
