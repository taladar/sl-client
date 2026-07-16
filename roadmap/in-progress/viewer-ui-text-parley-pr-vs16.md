---
id: viewer-ui-text-parley-pr-vs16
title: Upstream PR — parley: honour the emoji presentation selector (VS16)
topic: viewer
status: in-progress
origin: split out of viewer-ui-text-emoji-presentation (2026-07)
refs: [viewer-ui-text-emoji-presentation, viewer-ui-text-font-family-selection, viewer-ui-text-parley-pr-backdelete]
---

Context: [context/viewer.md](../context/viewer.md).

Submit the VS16 font-selection fix to `linebender/parley`. **The code is already
written, committed and tested** — this task is the submission itself.

**In progress**: submitted as <https://github.com/linebender/parley/pull/692>,
awaiting review. Nothing of ours waits on it — the viewer gets the behaviour
from the `[patch]`ed 0.9 branch regardless (see
[[viewer-ui-text-emoji-presentation]]) — but a design flaw has since surfaced
(see below), so this needs a decision rather than just patience.

## The branch

- Fork: `taladar/parley` (`origin`); upstream `linebender/parley` is `upstream`.
- Local clone: `~/devel/3rdparty/parley`.
- Branch: `fix/vs16-emoji-presentation-ordering`, based on `main`, pushed to the
  fork, one commit.
- Commit: **`6fcfbe2`** —
  *"Honour the emoji presentation selector (VS16) in font selection"*, carrying
  a `Co-authored-by: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`
  trailer. (It was `5f74cd3` before the trailer amend; the diff is unchanged.)
- Rebase state: based on `upstream/main` `7993939` ("Correctly handle \"doesn't
  compose\" in `CharCluster` (#690)"). Re-`git fetch upstream` and rebase before
  submitting — they are **actively churning `CharCluster`** (`#685`–`#690`).

How to contribute there — the LLM policy, the issue-first guideline, the CI
matrix, `--no-verify`, the CHANGELOG format — is captured in the
**`linebender-parley` skill**; read that first rather than rediscovering it.

Two constraints that decide how this is submitted:

- **Linebender's [LLM contribution
  policy](https://linebender.org/wiki/llm_policy/) forbids AI-generated PR
  descriptions** and requires LLM use to be disclosed up front
  *in the PR description* (it becomes the squash-merge commit message). So the
  prose is the human's to write; the `Co-authored-by` trailer is supplementary
  and their squash may not even preserve it. The policy also asks the submitter
  to self-review as hard as a human-authored PR.
- Their
  [contributor guidelines](https://linebender.org/contributor-guidelines/): *"To
  propose a nontrivial change, it is better to file an issue first rather than
  sending a PR."* **Decided: issue first**, PR only if invited.

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
- Full suite green on the branch: `parley_core` + `parley` unit tests and
  `parley_tests` (156 snapshots, incl. all `draw_colr_emoji*`) — no regressions.
- The *behavioural* selection change was verified **externally**, not in-tree.
  Which font each cluster resolves to, `Inter` primary + bundled Noto `CBDT`
  bound to the `Emoji` generic:

  | text | before | after |
  | --- | --- | --- |
  | `❤️` `U+2764 U+FE0F` | Inter | **emoji font** |
  | `❤` bare (no selector) | Inter | Inter |
  | `🎉` | emoji font | emoji font |
  | `5` | Inter | Inter |
  | `#` | Inter | Inter |
  | Latin | Inter | Inter |

  The `❤`/`5` rows are the other half of the contract: with no selector nothing
  is requested, so the text font must still win.

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

## Their CI, reproduced locally (2026-07-16, on this branch)

Recorded so a reviewer's "did you check no_std / MSRV?" has an answer, and so
the next person does not redo it. Everything below was actually run, not
assumed:

| Check | Result |
| --- | --- |
| `cargo fmt --all --check` | pass |
| `taplo fmt --check` | pass |
| `.github/copyright.sh` | pass (we add no new `.rs` files) |
| `cargo rdme --check --heading-base-level=0 --workspace-project=attributed_text` | pass |
| …`=parley_core` | pass |
| …`=parlance` | **not run** — needs a pinned `nightly-2026-06-22` for intralinks; fails identically on pristine `main`, and neither branch touches `parlance` |
| `cargo clippy --workspace --all-targets -- -D warnings` | **adds 0 errors vs `main`** — `main` itself has 3 unique (7 occurrences) under our clippy 0.1.97, which is newer than their CI pins; their `ci.yml` even notes "different clippy versions can disagree on goals" |
| no_std clippy, `--target x86_64-unknown-none` | pass (so does `main`) |
| no_std clippy, `--target thumbv8m.main-none-eabihf` | pass |
| `cargo +1.88 check` (MSRV) over `RUST_MIN_VER_PKGS` | pass |
| `cargo test -p parley_core -p parley` | pass |
| `cargo test -p parley_tests` | pass, 156 snapshots |

Nothing needed amending. Gotchas worth keeping:

- `cargo rdme` needs their exact flags (`--heading-base-level=0
  --workspace-project=<p>`); other invocations fail misleadingly.
- **`parley_tests` is a separate crate** — `--lib` skips its snapshots entirely.
- The no_std invocation must not pass the package list via an unquoted shell
  variable: **zsh does not word-split**, so it arrives as one glued argument and
  cargo reports "matched no packages" — which looks exactly like a real failure.
  Inline the `-p` flags.

## Submitted (2026-07-16)

PR: <https://github.com/linebender/parley/pull/692>. No review feedback yet.

Watch for the review question flagged above (no in-tree behavioural test), and
for interaction with the `CharCluster` refactor (`#685`–`#690`).

Note the sibling PR <https://github.com/linebender/parley/pull/693> drew a
design objection that changed its premise entirely — see
[[viewer-ui-text-parley-pr-backdelete]]. Nothing in that feedback touches this
one: the two only share the `cluster.is_emoji` flag, and this PR does not change
deletion.

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
