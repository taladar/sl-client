---
id: viewer-ui-text-caret-grapheme-motion
title: Upstream issue — parley caret motion steps one codepoint, not one grapheme
topic: viewer
status: deferred
origin: found by hand-testing the F4 demo after the grapheme-delete fix (2026-07)
refs: [viewer-ui-text-grapheme-backdelete, viewer-ui-text-parley-pr-backdelete, viewer-ui-text-foundation, viewer-ui-text-renderability-axis]
---

Context: [context/viewer.md](../context/viewer.md).

Caret motion (`move_left` / `move_right`) steps one **codepoint** at a time, not
one **grapheme cluster**. Found by hand-testing the F4 demo panel: the caret
walks *into the middle* of a ZWJ family, a Hangul syllable, `e` + a combining
acute, and so on.

**Raise this as an issue against `linebender/parley` first, before writing any
code.** Unlike [[viewer-ui-text-parley-pr-vs16]] and
[[viewer-ui-text-parley-pr-backdelete]] — self-contained bug fixes we could just
submit — every plausible fix here changes parley's cluster data model, and that
representation is upstream's design decision to make, not ours. Same reasoning
as
[[viewer-ui-text-renderability-axis]]: propose, let them choose, then offer to
implement.

Deferred, not blocked: nothing of ours waits on it, and it is pre-existing
behaviour rather than a regression.

## Measured

Caret steps to cross a single grapheme (each should be **1**), via `move_right`
from the text start, against the patched `parley` 0.9 branch with Inter + Noto
CBDT and the emoji generic bound:

| input | steps | want |
| --- | --- | --- |
| `👨‍👩‍👧‍👦` ZWJ family | 7 | 1 |
| `각` Hangul jamo (`U+1100 U+1161 U+11A8`) | 3 | 1 |
| `🇯🇵` regional-indicator flag | 2 | 1 |
| `❤️` (`U+2764 U+FE0F`) | 2 | 1 |
| `👋🏽` waving hand + skin tone | 2 | 1 |
| `e` + combining acute (`U+0301`) | 2 | 1 |
| `कि` Devanagari consonant + vowel sign | 2 | 1 |
| `ab` (two real graphemes) | 2 | 2 ✓ |

Note these are exactly the counts `backdelete` had before
[[viewer-ui-text-parley-pr-backdelete]] — the same wrong unit, reached for by a
different operation.

## Root cause

`ClusterData` is **one `char` by construction**, in `push_cluster`
(`parley/src/layout/data.rs`):

```text
text_len: cluster_start_char.1.len_utf8() as u8,
```

Ligatures are not merged clusters: they are per-char clusters tagged
`LigatureStart` / `LigatureComponent`. So a layout cluster is a codepoint, and
`Cursor::next_visual` / `previous_visual`
(`parley/src/editing/cursor.rs`) — which step to the adjacent cluster's
`text_range()` — are codepoint-granular *by design*. harfrust's cluster level
never enters into it (parley does not set one, so it is the `MonotoneGraphemes`
default, and it is irrelevant here).

This is why the fix cannot mirror
[[viewer-ui-text-parley-pr-backdelete]]: that one segments the *buffer* with the
ICU grapheme segmenter reached via `LayoutContext`, but `Cursor::next_visual`
receives only `&Layout` and has no access to `LayoutContext`, so it cannot
segment anything.

## Routes to propose (upstream picks)

- **A grapheme-start bit on `ClusterData`**, computed during analysis (where the
  segmenter already lives) and consumed by `next_visual` / `previous_visual` to
  skip non-starts. Smaller, but touches the analysis→layout data model and every
  consumer of cluster navigation: selection, word motion, hit-testing,
  accessibility.
- **Merge clusters to graphemes** in the layout model outright. Conceptually
  cleaner, much more invasive, and changes ligature representation.

Mention that `main` already computes grapheme boundaries in the shaper
(`grapheme_cluster_boundaries` in `parley_core/src/shape/shaper.rs`), so the
data exists; the question is where it should be surfaced.

## Scope to check when it is worked

Caret motion is unlikely to be the only casualty of the per-codepoint cluster —
audit these together rather than one at a time:

- shift+arrow selection extension,
- word motion (`next_visual_word` loops over `next_visual`),
- hit-testing / click-to-place-caret,
- accessibility cluster reporting,
- `Cluster::from_byte_index` consumers generally.

Also: caret motion must stay **visual** (bidi-aware) while becoming
grapheme-granular. A grapheme is contiguous and single-direction, so the two
should compose, but say so explicitly in the issue — it is the obvious review
question.

## Corrects an overclaim

[[viewer-ui-text-foundation]]'s "requirement 2 (grapheme editing)" is **not**
fully met even with the backdelete fix landed: backspace is grapheme-correct,
caret motion is not. The foundation's follow-up
[[viewer-ui-text-grapheme-backdelete]] only ever covered `backdelete` — caret
motion was never checked, by anyone, until the F4 panel was driven by hand.
Worth remembering that the headless tests all passed while this was broken:
`caret_moves_in_visual_order_across_a_bidi_boundary` in
`sl-client-bevy-viewer/src/ui_text.rs` steps `text.chars().count()` times over
text whose graphemes *are* all single codepoints, so it could never have caught
it.
