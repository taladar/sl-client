---
id: viewer-ui-text-grapheme-backdelete
title: Grapheme-correct backspace (parley backdelete deletes codepoints)
topic: viewer
status: done
origin: gap surfaced by viewer-ui-text-foundation (2026-07)
refs: [viewer-ui-text-foundation, viewer-ui-text-input-widget, viewer-ui-text-emoji-presentation, viewer-ui-text-caret-grapheme-motion]
---

Context: [context/viewer.md](../context/viewer.md).

**A hard requirement of [[viewer-ui-text-foundation]] that `parley` 0.9 fails.**
Backspace must delete exactly one **grapheme cluster**; it currently deletes one
**codepoint** in every case except a hard line break or a single emoji cluster.

Measured (headless, with the viewer's own font setup — not a font/ligature
artifact); each case should take **1** press:

| input | presses | wanted |
| --- | --- | --- |
| `👨‍👩‍👧‍👦` (ZWJ family) | 7 | 1 |
| `🇯🇵` (regional-indicator flag) | 2 | 1 |
| `❤️` (`U+2764 U+FE0F`) | 2 | 1 |
| `e` + combining acute (`U+0301`) | 2 | 1 |
| `🎉` (standalone emoji) | 1 | 1 ✓ |

The cause is explicit in `parley::editing::editor`'s `backdelete`: it takes the
upstream cluster and, unless
`cluster.is_hard_line_break() || cluster.is_emoji()`, deletes only
`char_indices().next_back()` — one codepoint. So the ZWJ family peels apart one
member per press, and even a plain combining mark splits.

Note this contradicts [[viewer-ui-text-input-widget]]'s stated assumption that
"grapheme-correct editing (`backdelete()`) is inherited from parley" — it is
not, so that task must not rely on it.

Do: fix within the bevy/parley stack — segment on grapheme-cluster boundaries
(the ICU segmenter parley already depends on) rather than codepoints, and delete
the whole cluster. Upstream to `parley` if possible; otherwise pre-empt the edit
in our own `TextEdit::Backspace` handling. `delete()` (forward) almost certainly
needs the same treatment — check it too.

A tripwire test (`backdelete_is_not_grapheme_correct_yet` in
`sl-client-bevy-viewer/src/ui_text.rs`) asserts the current *wrong* counts, so
it fails loudly once this is fixed; delete it as part of this task.

## Shares its fix with the emoji-presentation task (2026-07-16)

[[viewer-ui-text-emoji-presentation]] turns out to be the **same** area of
`parley`, and the two should probably be worked together:

- `backdelete` (`editing/editor.rs`) deletes a whole cluster only when
  `cluster.is_emoji()`; `select_font` (`shape/mod.rs`) appends the `Emoji`
  generic only when `cluster.is_emoji` — the **same flag**, and it is not UTS
  #51 aware (it is the raw `Emoji`/`Extended_Pictographic` property, true even
  for `5`, `#` and `▶`). Making it correct improves both.
- The `❤️` row in the table above and the emoji task's monochrome heart are two
  faces of the same VS16 mishandling.

Both fixes are in `parley` alone (**not** swash). The agreed approach is to fork
`linebender/parley`, point the workspace at the fork with `[patch.crates-io]` —
which transparently redirects `bevy_text`'s parley too — fix and test locally,
then submit upstream and drop the `[patch]` once released. See
[[viewer-ui-text-emoji-presentation]] for the full root-cause analysis.

## Scope: this task is backspace only (2026-07-16)

`delete()` (forward) had the same defect and is fixed alongside `backdelete` —
but **caret motion is a separate, larger bug** and is *not* covered here: see
[[viewer-ui-text-caret-grapheme-motion]]. `move_left`/`move_right` step one
codepoint because a `ClusterData` is one `char` by construction, which no amount
of fixing the delete operations addresses.

So closing this task does **not** discharge [[viewer-ui-text-foundation]]'s
requirement 2 in full. Deleting is grapheme-correct; navigating is not.

## Outcome (2026-07-16)

Backspace and forward delete each remove exactly one grapheme cluster. Fixed in
parley itself — not worked around here — and consumed via the `[patch]`ed fork
(`taladar/parley`, `sl-client/0.9-patch`). Submitting it upstream is
[[viewer-ui-text-parley-pr-backdelete]].

The fix segments with the ICU grapheme segmenter parley already depends on, and
drops the `is_hard_line_break() || is_emoji()` special case entirely: a line
break and an emoji cluster are both just graphemes. `delete()` (forward) had the
same defect and was worse — with no special case at all it split even a CRLF
pair and a Hangul syllable.

The tripwire is gone, replaced by `backspace_deletes_exactly_one_grapheme` in
`sl-client-bevy-viewer/src/ui_text.rs`, which asserts the *right* behaviour and
doubles as a guard against the parley patch being dropped (its failure message
says so). Its helper's doc comment was also corrected: it claimed to measure
"with the viewer's own font setup" while using a bare `FontCx::default()` — the
counts are in fact font-independent, verified by measuring both ways.

**Caret motion remains broken** and is not covered here — see
[[viewer-ui-text-caret-grapheme-motion]].
