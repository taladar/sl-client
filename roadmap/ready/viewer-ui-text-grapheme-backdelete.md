---
id: viewer-ui-text-grapheme-backdelete
title: Grapheme-correct backspace (parley backdelete deletes codepoints)
topic: viewer
status: ready
origin: gap surfaced by viewer-ui-text-foundation (2026-07)
refs: [viewer-ui-text-foundation, viewer-ui-text-input-widget]
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
