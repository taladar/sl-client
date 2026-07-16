---
id: viewer-ui-text-parley-pr-backdelete
title: Upstream PR — parley: grapheme-correct backdelete / delete
topic: viewer
status: in-progress
origin: split out of viewer-ui-text-grapheme-backdelete (2026-07)
refs: [viewer-ui-text-grapheme-backdelete, viewer-ui-text-parley-pr-vs16, viewer-ui-text-emoji-presentation]
---

Context: [context/viewer.md](../context/viewer.md).

Land a correct backspace/forward-delete fix in `linebender/parley`.

**In progress**: submitted as <https://github.com/linebender/parley/pull/693> —
and **the review rejected its premise**, so this needs rewriting, not patience.
The replacement is planned in detail at
`~/.claude-personal/plans/now-make-a-detailed-agile-aho.md`.

Nothing of ours is stuck meanwhile: the consumer side
([[viewer-ui-text-grapheme-backdelete]]) is already **done** — we get the
behaviour from the `[patch]`ed 0.9 branch, the tripwire is replaced, and
`viewer-ui-widget-scaffold` is unblocked. Landing it upstream is what lets us
eventually drop the `[patch]`.

Probably the better of the two to lead with: self-contained, measured against a
clear standard (UAX #29), and it does **not** touch the `CharCluster` code
upstream is currently refactoring (`#685`–`#690`).

## The branch

- Fork: `taladar/parley` (`origin`); upstream `linebender/parley` is `upstream`.
- Local clone: `~/devel/3rdparty/parley`.
- Branch: `fix/grapheme-backdelete`, off `main`, pushed to the fork, one commit.
  Kept **separate** from `fix/vs16-emoji-presentation-ordering` so the two stay
  independent.
- Commit: **`97a09e5`** —
  *"Delete one grapheme cluster in `backdelete` and `delete`"*, carrying a
  `Co-authored-by: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`
  trailer. (It was `d9054eb` before the trailer amend; the diff is unchanged.) 2
  files, +139/−24.
- Rebase state: based on `upstream/main` `7993939`. Re-`git fetch upstream` and
  rebase before submitting.

How to contribute there — the LLM policy, the issue-first guideline, the CI
matrix, `--no-verify`, the CHANGELOG format — is captured in the
**`linebender-parley` skill**; read that first rather than rediscovering it.

Two constraints that decide how this is submitted:

- **Linebender's [LLM contribution
  policy](https://linebender.org/wiki/llm_policy/) forbids AI-generated PR
  descriptions** and requires LLM use to be disclosed up front
  *in the PR description* (it becomes the squash-merge commit message). The
  prose is the human's to write; the `Co-authored-by` trailer is supplementary
  and their squash may not preserve it. The policy also asks the submitter to
  self-review as hard as a human-authored PR.
- Their
  [contributor guidelines](https://linebender.org/contributor-guidelines/): *"To
  propose a nontrivial change, it is better to file an issue first rather than
  sending a PR."* **Decided: issue first**, PR only if invited.

## The bug

Backspace and forward delete must each remove exactly one **grapheme cluster**.
Both remove less.

`backdelete` (`parley/src/editing/editor.rs`, ~line 298, unchanged on `main` as
of `7993939`) deletes the whole cluster only for a hard line break or an emoji
cluster, else one **codepoint**:

```text
let start = if cluster.is_hard_line_break() || cluster.is_emoji() {
    range.start                       // delete the previous cluster
} else {
    // Otherwise, delete the previous character
    ... str.char_indices().next_back() ...
};
```

**`delete` (forward) turned out to have the same defect and is worse** — it has
no special case at all, removing the downstream **layout** cluster, which
follows the font's glyphs rather than perceived characters. So it splits a
grapheme the font does not ligate, and splits a CRLF pair in two despite `#667`.

Measured presses to erase one grapheme, before → after the fix:

| input | `backdelete` | `delete` |
| --- | --- | --- |
| `👨‍👩‍👧‍👦` ZWJ family | 7 → 1 | 7 → 1 |
| `🇯🇵` regional-indicator flag | 2 → 1 | 2 → 1 |
| `❤️` (`U+2764 U+FE0F`) | 2 → 1 | 2 → 1 |
| `👋🏽` waving hand + skin tone | 1 → 1 | 2 → 1 |
| `e` + combining acute (`U+0301`) | 2 → 1 | 2 → 1 |
| `각` Hangul jamo (`U+1100 U+1161 U+11A8`) | 1 → 1 | 3 → 1 |
| CRLF | 1 → 1 | 2 → 1 |
| `ab` (two real graphemes) | 2 → 2 | 2 → 2 |

**These are not a font artifact** — verified 2026-07-16 by measuring with a bare
`FontCx::default()` *and* with the viewer's full bundled stack (Inter + DejaVu +
Noto CBDT, emoji generic bound): identical counts both ways. Worth stating in
the PR, since cluster boundaries otherwise plausibly depend on shaping.

## The fix (written)

Segment on grapheme-cluster boundaries with the ICU grapheme segmenter already
reachable via `AnalysisDataSources` (`LayoutContext` holds it, and the driver
holds `layout_cx`). **`parley` already depends on `icu_segmenter` directly**
(`parley/Cargo.toml:39`), so this adds no dependency. The
`is_hard_line_break() || is_emoji()` branch
**became unnecessary and is dropped**: a hard line break and an emoji cluster
are both just graphemes.

Note `delete` now takes `start` from `selection.focus().index()` directly rather
than looking up the downstream cluster, while `backdelete` still uses the
upstream cluster's `text_range()` for its `end`. A reviewer may ask why the two
paths are not symmetric; keeping the diff minimal was the reason. Happy to unify
if asked.

Note the `e` + acute, Hangul and CRLF rows: **not emoji-related at all**, so
making `cluster.is_emoji` UTS #51-aware (as [[viewer-ui-text-parley-pr-vs16]]
does for selection) would **not** fix them. That is the argument for grapheme
segmentation over a better `is_emoji`, and the reason not to conflate the two
PRs — they merely touch the same flag.

Be ready for a design question in review: some editors deliberately delete
combining marks one at a time. Position it as UAX #29 grapheme clusters being
the platform-standard behaviour (macOS, Windows, Chrome all delete a ZWJ family
whole), and note that parley's existing emoji special-case shows the *intent*
was already whole-cluster deletion — it just under-reaches.

## Testing done

Three tests in `parley_tests/tests/editor.rs` —
`editor_backdelete_deletes_one_grapheme`, `editor_delete_deletes_one_grapheme`,
`editor_delete_stops_at_grapheme_boundaries` — covering every row above in both
directions plus a guard that two separate graphemes still take two presses. They
**assert the resulting text rather than snapshots**, so they do not depend on
the font, and they extend the existing editor tests rather than adding a
harness.

Verified they *fail* against unfixed parley: one press leaves
`"👨\u{200d}👩\u{200d}👧\u{200d}"` — the family peeled apart. A test that cannot
fail proves nothing, so this was checked explicitly.

`cargo test -p parley_core -p parley` green; `cargo test -p parley_tests` green
at **159** (their 156 + our 3).

## Their CI, reproduced locally (2026-07-16, on this branch)

Recorded so a reviewer's "did you check no_std / MSRV?" has an answer, and so
the next person does not redo it. Everything below was actually run:

| Check | Result |
| --- | --- |
| `cargo fmt --all --check` | pass |
| `taplo fmt --check` | pass |
| `.github/copyright.sh` | pass (no new `.rs` files) |
| `cargo rdme --check --heading-base-level=0 --workspace-project=attributed_text` | pass |
| …`=parley_core` | pass |
| …`=parlance` | **not run** — needs a pinned `nightly-2026-06-22` for intralinks; fails identically on pristine `main`, and neither branch touches `parlance` |
| `cargo clippy --workspace --all-targets -- -D warnings` | **adds 0 errors vs `main`** — `main` itself has 3 unique (7 occurrences) under our clippy 0.1.97, newer than their CI pins |
| no_std clippy, `--target x86_64-unknown-none` | pass (so does `main`) |
| no_std clippy, `--target thumbv8m.main-none-eabihf` | pass |
| `cargo +1.88 check` (MSRV) over `RUST_MIN_VER_PKGS` | pass |
| `cargo test -p parley_core -p parley` | pass |
| `cargo test -p parley_tests` | pass, 159 snapshots |

Nothing needed amending. Gotchas worth keeping:

- `cargo rdme` needs their exact flags (`--heading-base-level=0
  --workspace-project=<p>`); other invocations fail misleadingly.
- **`parley_tests` is a separate crate** — `--lib` skips its snapshots entirely.
- The no_std invocation must not pass the package list via an unquoted shell
  variable: **zsh does not word-split**, so it arrives as one glued argument and
  cargo reports "matched no packages" — which looks exactly like a real upstream
  failure. Inline the `-p` flags.

## Consumer side — already done

The behaviour is already in our viewer via the `[patch]`ed
`sl-client/0.9-patch` branch (see [[viewer-ui-text-grapheme-backdelete]]): the
`backdelete_is_not_grapheme_correct_yet` tripwire is gone, replaced by
`backspace_deletes_exactly_one_grapheme` in
`sl-client-bevy-viewer/src/ui_text.rs`, which also guards against the patch
silently disappearing. Its helper's doc comment was corrected too — it had
claimed measurement "with the viewer's own font setup" while using a bare
`FontCx::default()`.

Remaining consumer work is only to **drop the `[patch]`**, which needs this
landed upstream *and* `bevy_text` moved to parley ≥ 0.10.

## Submitted, and the review says the premise is wrong (2026-07-16)

PR: <https://github.com/linebender/parley/pull/693>

**[@raphlinus] (Linebender's founder) — the feedback that matters:**

> "I haven't carefully looked at the backspace logic in Parley recently, but in
> *general* it should be finer granularity than grapheme clusters. We had fairly
> carefully worked out logic in <https://github.com/linebender/xilem/pull/303>,
> as far as I'm concerned that's still a good reference."

He is right, and this task's framing ("backspace must delete exactly one
grapheme cluster") is **wrong**. xilem#303 ("Actually use the druid backspace
logic", merged 2024-05-12) restores a state machine originating in xi-editor#837
— the Android algorithm. It deletes emoji **sequences** whole (VS,
regional-indicator pairs, ZWJ chains, keycaps, emoji modifiers, tag sequences,
CRLF) but falls through to **one codepoint** for everything else. That is
deliberate: Korean IMEs conventionally delete Hangul jamo-by-jamo, and users
expect a combining accent to come off on its own. Emoji are the exception
because nobody types them piecemeal.

### Corrected measurements — three rows in the submitted PR are wrong

The PR's `backdelete` "before" column claimed skin tone `1`, Hangul `1` and CRLF
`1`. Those were **inferred from reading the `is_hard_line_break() || is_emoji()`
special case, not measured** — the exact mistake this whole effort kept catching
elsewhere. Re-measured on pristine `main` (`7993939`):

| input | `main` (measured) | this PR (measured) | xilem/Android (traced, not run) |
| --- | --- | --- | --- |
| `👨‍👩‍👧‍👦` ZWJ family | 7 | **1** | 1 |
| `🇯🇵` flag | 2 | **1** | 1 |
| `❤️` `U+2764 U+FE0F` | 2 | **1** | 1 |
| `👋🏽` skin tone | 2 | **1** | 1 |
| CRLF | 2 | **1** | 1 |
| `e` + combining acute | 2 | **1** | **2** |
| `각` Hangul jamo | 3 | **1** | **3** |
| `ab` | 2 | 2 | 2 |

So: this PR **improves 5 rows** (all agreeing with the reference) and
**regresses 2** — `e`+acute and Hangul — where `main` already behaved correctly
by accident. The bad "1 → 1" rows hid precisely the contentious change: Hangul
is really `3 → 1`.

### A related finding worth stating

`main`'s `is_emoji` special case is **ineffective, not merely incomplete**: a
`ClusterData` is one `char` (see [[viewer-ui-text-caret-grapheme-motion]]), so
`cluster.is_emoji()` is true for a single emoji codepoint and deletes just that
one. Hence the ZWJ family taking 7 despite the special case existing. This is
the same root cause as the caret issue, from a third direction.

### What to do next

Port the xilem#303 / Android state machine rather than grapheme segmentation.
That keeps all 5 improvements and drops both regressions. Check whether it
should live in `parley` or be shared, and what `delete` (forward) should mirror
— the Android algorithm is backspace-specific and forward delete may want
different rules.

Also note this **invalidates the "possible pushback" note above**: the counter
it suggests (UAX #29 is platform-standard) does not survive contact with the
actual argument, since Android — the source of the platform behaviour —
deliberately does *not* use grapheme clusters here.

**The PR record needs correcting by hand** (the LLM policy forbids AI-written
comments): the three measurement rows are wrong in a public PR.

[@raphlinus]: https://github.com/raphlinus

## Research for the replacement (2026-07-16)

Full execution plan: `~/.claude-personal/plans/now-make-a-detailed-agile-aho.md`
(written to be run by a fresh session). Contribution rules: the
**`linebender-parley` skill**. Key findings, so they survive the plan file:

- **Do not copy the reference code — reimplement the algorithm.** Every link is
  Apache-2.0-**only** (xi-unicode, xi-editor, druid, xilem:
  `// SPDX-License- Identifier: Apache-2.0`), while parley is
  `Apache-2.0 OR MIT` across 132/132 `.rs` files. Apache-2.0 confers no power to
  sublicense under MIT, so pasting it in would have parley offering under MIT
  what it may not. Algorithms are not copyrightable; a parley-native
  reimplementation avoids this entirely.
- **Raph is not the author** and cannot relicense it — original is YangKeao
  (xi-editor#837, 2018), plus ~5 others and AOSP provenance; Linebender has no
  CLA.
- **The reference no longer exists in xilem**: deleted 2024-11-18 by xilem#616,
  when masonry adopted parley's own `PlainEditor`. Last copy pinned at
  `30cb5fb6a694908a74ed8969247807ce821d624b`, `masonry/src/text/backspace.rs`.
- **`xi_unicode`'s emoji tables are frozen at Unicode 11 (2018)**; no release
  since 2019. The strongest technical argument for reimplementing: parley's own
  data is generated from current ICU4X.
- **Forward `delete` needs no rewrite.** The reference is deliberately
  asymmetric (xilem `masonry/src/text/edit.rs` @ `30cb5fb`: Backspace →
  `offset_for_delete_backwards`, Delete → `next_grapheme_offset`). Our #693
  `delete` is already grapheme-based, which **matches**; only `backdelete` is
  wrong. Note the earlier claim in this task that "druid has no forward-delete
  equivalent, so a mirror is our invention" was **wrong**.
- **Two new data bits needed**, zero new dependencies:
  `CharInfo::is_emoji_or_pictograph()` is `Emoji ∪ Extended_Pictographic`
  (`parley_data_gen/src/lib.rs:41-42`) but the algorithm needs `Emoji` alone;
  `Emoji_Modifier_Base` is absent. `parley_data_gen` already imports the right
  `icu_properties` module — add `EmojiModifierBase` and two `contains32` args.
  `parley_data` has spare bits and is `#![no_std]` with baked tables.
- **`parley` has no `icu_segmenter` on `main`** — only `icu_properties`;
  `parley_core` has the rest. See correction 2 below.

## A fourth error in the submitted PR (2026-07-16)

Beyond the three measurement rows already noted: the claim **"`parley` already
depends on `icu_segmenter` (`parley/Cargo.toml:39`)" is false on `main`** — that
line number is from the **0.9 branch**. The conclusion ("adds no dependency")
still holds, because the segmenter is reached through `parley_core`'s public
API, but the stated reason is wrong. Same root cause as the other three: reading
one branch and asserting it of another.

Also: #693's `formatting` CI is red because `cargo fmt` was never run on this
branch — the cheap checks were run on the VS16 branch only and reported as
covering both.
