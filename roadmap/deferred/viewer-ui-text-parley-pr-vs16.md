---
id: viewer-ui-text-parley-pr-vs16
title: Upstream PR — parley: honour the emoji presentation selector (VS16)
topic: viewer
status: deferred
origin: split out of viewer-ui-text-emoji-presentation (2026-07)
refs: [viewer-ui-text-emoji-presentation, viewer-ui-text-font-family-selection, viewer-ui-text-parley-pr-backdelete]
---

Context: [context/viewer.md](../context/viewer.md).

Submit the VS16 font-selection fix to `linebender/parley`. **The code is already
written, committed and tested** — this task is the submission itself.

Deferred, not blocked: our viewer gets the behaviour from the `[patch]`ed 0.9
branch regardless (see [[viewer-ui-text-emoji-presentation]]), so nothing waits
on upstream's review. Submit it because it is a real bug that affects every
parley consumer.

## The branch

- Fork: `taladar/parley` (`origin`); upstream `linebender/parley` is `upstream`.
- Local clone: `~/devel/3rdparty/parley`.
- Branch: `fix/vs16-emoji-presentation-ordering`, based on `main`.
- Commit: `5f74cd3` — *"Honour the emoji presentation selector (VS16) in font
  selection"*. Its message is written to serve as the PR body.

The branch is already pushed to the fork (`5f74cd3`); open the PR against
`linebender/parley` `main`.

**Commit *and push* in that repo with `--no-verify`.** Our global `ggh` hooks
are not set up for it: on commit they lint *upstream's* tree (shellcheck on
their `.github/copyright.sh`, typos, tombi), and on push `convco check` rejects
the commit message — parley does **not** use conventional commits, their style
is a plain sentence (e.g. "Correctly handle \"doesn't compose\" in `CharCluster`
(#690)"), which is what our messages deliberately match. Their CI and their
`.rustfmt.toml` / `.clippy.toml` are the authority there; never reformat their
files or restyle our commit messages to satisfy our hooks.

## What the change is

A cluster carrying `U+FE0F` (VS16) requests the emoji presentation per UTS #51,
but selection ignored the request: the emoji family is appended *after* the
requested families, so a text font covering the base codepoint wins and the
cluster renders as a monochrome text glyph. `❤️` (`U+2764 U+FE0F`) is the
motivating case — `U+2764` has a dingbat glyph in DejaVu Sans, Inter and
friends.

The emoji family cannot simply be tried first in general: `is_emoji` is the raw
`Emoji`/`Extended_Pictographic` property, true for `5`, `#` and `▶` too, whose
default presentation is text. Verified: with the emoji family unconditionally
first, `5` renders as an emoji. So the request must be honoured only where it is
actually made.

The diff (3 files, +125/-3):

- `parley_core/src/shape/cluster.rs` — add `Presentation`
  (`Unspecified`/`Text`/`Emoji`), record it on `CharCluster` during `fill`, and
  expose `CharCluster::presentation()`. Finer-grained than the existing
  `is_variation_selector()`, which cannot tell the two emoji variation selectors
  apart even though they request *opposite* presentations.
- `parley_core/src/shape/mod.rs` — re-export `Presentation`.
- `parley/src/shape/mod.rs` — in `select_font`, order the emoji family *before*
  the requested families when the cluster requests `Presentation::Emoji`. VS15
  (`U+FE0E`) keeps the existing ordering, which already prefers the text font
  whenever one covers the codepoint.

## Testing done — and the gap to be honest about in the PR

- Added three unit tests in `parley_core/src/shape/cluster.rs`
  (`emoji_presentation_selector_is_detected`,
  `text_presentation_selector_is_detected`, `no_selector_requests_nothing`).
  These cover *detection* and are platform-independent.
- Full suite green on the branch: `parley_core` + `parley` unit tests (78) and
  `parley_tests` (156 snapshots, incl. all `draw_colr_emoji*`) — no regressions.
- The *behavioural* selection change was verified **externally**, not in-tree:
  with `Inter` as primary and a bundled Noto `CBDT` emoji font bound to the
  `Emoji` generic, `❤️` now resolves to the emoji font while `❤` (no selector),
  `5`, `#` and Latin all still resolve to Inter.

Why no in-tree behavioural test: their bundled text fonts have **no overlap**
with their emoji subset — `Arimo` and `Roboto` contain none of `U+270C`,
`U+2705`, `U+2764` (checked by parsing the `cmap`s), so there is no text font
in-tree for the emoji font to compete against, and the change is a no-op in that
setup. That is also why their existing
`draw_colr_emoji_with_non_printing_variation_selector_16` is gated
`#[cfg(all(target_os = "macos", feature = "system"))]` and leans on system
fonts.

Offer in the PR to add a portable behavioural test if they will take a small
text-font asset covering one of the emoji subset's codepoints (or point us at a
preferred approach). Expect this to be the main review question.

## Context worth linking

- Upstream fixed the *adjacent* half in 0.10.0 (`#685`,
  `is_emoji_with_non_printing_variation_selector`), which stops VS16 from
  disqualifying the emoji font on `cmap` coverage. This PR is the other half:
  without it, that fix alone still yields a monochrome `❤️` whenever the primary
  font covers `U+2764`. Verified on `main` before writing the change.
- Their own `#[allow(dead_code, reason = "To be used in more complete emoji
  checking, in select_font")]` on `VARIATION_SELECTOR_MASK` names exactly this
  work.
- Related: [[viewer-ui-text-parley-pr-backdelete]] (same fork, same
  `cluster.is_emoji` area) and [[viewer-ui-text-renderability-axis]].
