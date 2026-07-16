---
id: viewer-ui-text-parley-pr-backdelete
title: Upstream PR — parley: grapheme-correct backdelete / delete
topic: viewer
status: deferred
origin: split out of viewer-ui-text-grapheme-backdelete (2026-07)
refs: [viewer-ui-text-grapheme-backdelete, viewer-ui-text-parley-pr-vs16, viewer-ui-text-emoji-presentation]
---

Context: [context/viewer.md](../context/viewer.md).

Write **and** submit the grapheme-correct backspace fix to `linebender/parley`.
Unlike [[viewer-ui-text-parley-pr-vs16]], the code does **not** exist yet — this
task is write, test, then submit.

Deferred, not blocked: [[viewer-ui-text-grapheme-backdelete]] is the
consumer-side task (it also has to drop our tripwire test once this lands, and
unblocks `viewer-ui-widget-scaffold`). Do them together.

## The branch

- Fork: `taladar/parley` (`origin`); upstream `linebender/parley` is `upstream`.
- Local clone: `~/devel/3rdparty/parley`.
- New branch off `main`, kept **separate** from
  `fix/vs16-emoji-presentation-ordering` so the two PRs stay independent.
**Commit *and push* in that repo with `--no-verify`.** Our global `ggh` hooks
are not set up for it: on commit they lint *upstream's* tree (shellcheck on
their `.github/copyright.sh`, typos, tombi), and on push `convco check` rejects
the commit message — parley does **not** use conventional commits, their style
is a plain sentence (e.g. "Correctly handle \"doesn't compose\" in `CharCluster`
(#690)"), which is what our messages deliberately match. Their CI and their
`.rustfmt.toml` / `.clippy.toml` are the authority there; never reformat their
files or restyle our commit messages to satisfy our hooks.

## The bug

Backspace must delete exactly one **grapheme cluster**; parley deletes one
**codepoint** in every case except a hard line break or a single emoji cluster.
`parley/src/editing/editor.rs` (~line 298, unchanged on `main` as of `7993939`):

```text
let start = if cluster.is_hard_line_break() || cluster.is_emoji() {
    range.start                       // delete the previous cluster
} else {
    // Otherwise, delete the previous character
    ... str.char_indices().next_back() ...
};
```

Measured presses to clear (each should be **1**):

| input | presses |
| --- | --- |
| `👨‍👩‍👧‍👦` ZWJ family | 7 |
| `🇯🇵` regional-indicator flag | 2 |
| `❤️` (`U+2764 U+FE0F`) | 2 |
| `e` + combining acute (`U+0301`) | 2 |
| `🎉` standalone emoji | 1 ✓ |

**These are not a font artifact** — verified 2026-07-16 by measuring with a bare
`FontCx::default()` *and* with the viewer's full bundled stack (Inter + DejaVu +
Noto CBDT, emoji generic bound): identical counts both ways. Worth stating in
the PR, since cluster boundaries otherwise plausibly depend on shaping.

## The fix

Segment on grapheme-cluster boundaries — parley already depends on ICU
(`icu_properties` / `icu_normalizer`, and `Analyzer` already computes
grapheme-cluster boundaries, which the shaper consumes) — rather than
special-casing emoji at all. The `is_hard_line_break() || is_emoji()` branch
should *become unnecessary*: a hard line break and an emoji cluster are both
just graphemes.

Note the `e` + combining acute row: it is **not** emoji-related, so making
`cluster.is_emoji` UTS #51-aware (as [[viewer-ui-text-parley-pr-vs16]] does for
selection) would **not** fix it. Do not conflate the two PRs — they merely touch
the same flag.

Check **`delete()`** (forward delete) for the same defect and fix it in the same
PR if so.

Be ready for a design question in review: some editors deliberately delete
combining marks one at a time. Position it as UAX #29 grapheme clusters being
the platform-standard behaviour (macOS, Windows, Chrome all delete a ZWJ family
whole), and note that parley's existing emoji special-case shows the *intent*
was already whole-cluster deletion — it just under-reaches.

## Testing to do

- Unit tests asserting 1 press for each row above; they will read as the
  inverse of our tripwire.
- Full suite must stay green: `cargo test -p parley_core -p parley` and
  `cargo test -p parley_tests` (156 snapshots — note `parley_tests` is a
  separate crate and is **not** covered by `--lib`, which is how the VS16 work
  nearly missed the `draw_colr_emoji*` snapshots).
- `parley_tests/tests/editor.rs` and `cursor.rs` are the existing editing tests
  — check for neighbours to extend rather than adding a new harness.

## Consumer side, once it lands

Backport onto the combined `v0.9.0`-based branch we `[patch]` with (see
[[viewer-ui-text-emoji-presentation]]), then delete the
`backdelete_is_not_grapheme_correct_yet` tripwire in
`sl-client-bevy-viewer/src/ui_text.rs` and fix its doc comment, which currently
claims it was "measured with the viewer's own font setup" while actually using a
bare `FontCx::default()`.
