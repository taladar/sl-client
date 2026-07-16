---
id: viewer-ui-text-renderability-axis
title: Upstream issue — font selection cannot see glyph-format renderability
topic: viewer
status: in-progress
origin: found while working viewer-ui-text-font-family-selection (2026-07)
refs: [viewer-ui-text-font-family-selection, viewer-ui-text-emoji-presentation]
---

Context: [context/viewer.md](../context/viewer.md).

File an **issue** (not a PR) against `linebender/parley` proposing that font
selection be able to account for whether the consumer's rasteriser can actually
paint the glyph format a candidate font uses.

**In progress**: filed as <https://github.com/linebender/parley/issues/695>,
awaiting a design decision from upstream. Nothing of ours waits on it — we
already sidestep the problem by bundling a `CBDT` emoji font (see
[[viewer-ui-text-font-family-selection]]). It is filed because it is the honest
**root cause** of a whole class of silent-blank bugs, it affects every
swash-based consumer (including `bevy_text`, i.e. every Bevy app), and it is a
*design* decision for upstream rather than a bug we can just fix and submit.

## The claim

Font selection matches on exactly three axes:

1. family name,
2. attributes (weight / width / style),
3. **`cmap` coverage** — "does this font map these codepoints?"

**Renderability is not an input.** Nothing asks "can whatever rasterises this
actually paint the glyph *format* this font uses for these codepoints?" So a
font is a `Status::Complete` match on coverage and then paints nothing.
Coverage ≠ renderability.

Not emoji-specific in principle: it applies to any colour/vector glyph format a
given rasteriser lacks (`COLRv0`, `COLRv1`, `CBDT`/`CBLC`, `sbix`,
`SVG`-in-OpenType). Emoji is just where it bites hardest, because that is where
formats diverge.

## Evidence to include

- **The selection path is cmap-only.** `parley`'s `select_font`
  (`parley/src/shape/mod.rs`) drives `fontique`'s `Query::matches_with`, and the
  closure it passes only consults `font.charmap()` and cluster mapping;
  `Status::Complete` means "every char mapped", nothing more.
- **The seam already exists.** `Query::matches_with` is *already* a
  caller-supplied predicate returning `QueryStatus::Continue`/`Stop`
  (`fontique/src/collection/query.rs`). parley simply owns that closure
  internally today. A capability/filter hook would fit there without inventing a
  new mechanism.
- **fontique already parses the tables** it would need (it reads attributes and
  variation axes per face — `fontique/src/font.rs`), so exposing "which colour
  formats does this face use" is cheap; reading the `COLR` table's version field
  is a couple of bytes.
- **Concrete failure, measured** (this workspace, 2026-07-16): with a generic
  `FontSource`, an emoji run resolved to the host's **4 991 984**-byte `COLRv1`
  Noto Color Emoji instead of our **10 673 480**-byte `CBDT` build, and painted
  **blank**. `swash` 0.2.9's `scale::color` reads the `COLR` **v0** header only
  (`numBaseGlyphRecords` at offset 2, `baseGlyphRecordsOffset` at 4,
  `layerRecordsOffset` at 8) without checking the table's version, so a
  `COLRv1`-only font yields zero layers. Its `Source` enum offers only
  `ColorOutline` (`COLRv0`) and `ColorBitmap` (`CBDT`/`sbix`).
- **Why parley itself never sees it:** parley's own test renderer uses
  `vello_cpu` + `glifo` (`parley_tests/tests/util/renderer.rs`), which handles
  COLR — and that file even notes "Emoji rendering is not currently implemented
  in this example. See the swash example". So the gap is invisible from inside
  the repo's own tests; it only appears for consumers with a
  less-capable rasteriser.
- **The silence is the problem.** The glyph is not tofu and not an error — it is
  nothing at all, so it reads as a layout or styling bug and costs real time to
  localise.

## Design sketch to propose (leave the decision to them)

- `fontique` exposes per-face colour-glyph formats on `FontInfo`.
- `parley` lets a consumer declare its rasteriser's capabilities — or, more
  cheaply, pass a font-acceptance predicate that selection consults alongside
  cmap coverage, so an unpaintable face is skipped and selection falls through
  to the next candidate.

Be explicit in the issue that this is a **design question**, not a patch: it
widens what "font selection" means, it has a per-glyph wrinkle (one face can
carry colour glyphs for some codepoints and plain outlines for others), and
capability data has to come from the consumer because the renderer is their
choice.

## Alternatives worth naming in the issue

- **Fix the rasteriser instead:** `swash` gains `COLRv1` (`dfrg/swash`, a
  separate repo — a big spec: gradients, composites, transforms). That removes
  today's most common instance but not the general axis.
- **Report rather than select:** `swash` could signal "cannot paint this", and a
  consumer like `bevy_text` could then fall back. Turns a silent blank into a
  recoverable one without touching selection.
- **What we do now:** bundle a font in a format the rasteriser supports,
  register it under a private family, bind the `Emoji` generic to it, and ban
  generic `FontSource` (guard test). Works, but every consumer must rediscover
  it — which is the argument for the issue.

## Where

<https://github.com/linebender/parley/issues> (fontique lives in the same repo).
Our fork with the two related fixes is `taladar/parley`; link the VS16 PR from
[[viewer-ui-text-emoji-presentation]] as related context if it is open by then.

## Submitted (2026-07-16)

Issue: <https://github.com/linebender/parley/issues/695>. No feedback yet.

## How to submit (2026-07-16)

Read the **`linebender-parley` skill** first — it captures the contribution
rules so they need not be rediscovered. The two that decide this task:

- **Linebender's [LLM contribution
  policy](https://linebender.org/wiki/llm_policy/)**:
  *"we do not allow ... AI-generated PR descriptions"*, and *"In discussion
  spaces like Github comments and the Zulip server, please avoid posting
  AI-generated analyses, even if you vetted them."* So
  **this task's prose is not issue text** — it is internal notes. The issue must
  be written by hand, from the facts here, with **LLM use disclosed up front**
  in it.
- Their
  [contributor guidelines](https://linebender.org/contributor-guidelines/): *"To
  propose a nontrivial change, it is better to file an issue first rather than
  sending a PR."* This is a design question, so it is issue-only by nature —
  propose, let them choose the representation, then offer to implement.

A working copy of these facts also sits **uncommitted** in the parley clone at
`~/devel/3rdparty/parley/sl-client-notes/` (excluded via `.git/info/exclude`,
since they *"will not merge agentic markdown files"*).
