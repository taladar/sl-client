---
id: viewer-emoji-colon-autocomplete
title: Colon-based emoji autocomplete
topic: viewer
status: done
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-emoji-input
blocked_by: [viewer-emoji-data, viewer-ui-text-input-widget]
---

Context: [context/viewer.md](../context/viewer.md).

The inline `:`-completer for text fields: type `:` to open an autocomplete of
emoji short-codes (`:smile:`), filter as you type, and insert the Unicode glyph.
Reads short-codes from [[viewer-emoji-data]], renders in the
[[viewer-ui-text-input-widget]], and its primary consumer is the chat input
([[viewer-chat-input-bar]]).

Reference (Firestorm, read-only): `llemojihelper`, `llpanelemojicomplete` (the
inline `:`-completer).

## Done (2026-07-20)

Done ahead of its blocker `viewer-chat-input-bar` — the completer only needs a
field, not the live bar — as `sl-client-bevy-viewer/src/emoji_complete.rs`.
`attach_colon_complete(field, anchor)` hangs a popup as an **absolute child of
the field's own container** (above it), so no screen-space maths. A chat line is
typed left to right, so the pure core reads only the **trailing** `:`-token
(`trailing_colon_prefix`): the run of short-code chars back to a `:` at the
string start or after whitespace — which also means a closed `:smile:` does not
re-trigger. Past two chars, `sl_emoji::complete` fills the popup (capped);
`Up`/`Down` move the selection, `Enter`/`Tab`/click accept it (replacing the
token with the glyph, `replace_trailing_token`), `Escape` closes it.

Key coordination: the completer's key system lives in `ColonCompleteSet` and
**clears the consumed key** from the frame, and the chat input's own
Enter-to-send ([[viewer-ui-text-input-emoji]]) is ordered *after* that set — so
a press that accepted a suggestion is never also a send. Attached by the
chat-input widget and swept live in its specimen; the trailing-token,
match-gating and replacement are unit-tested.
