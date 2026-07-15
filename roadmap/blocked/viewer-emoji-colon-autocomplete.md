---
id: viewer-emoji-colon-autocomplete
title: Colon-based emoji autocomplete
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-emoji-input
blocked_by: [viewer-emoji-data, viewer-ui-text-input-widget, viewer-chat-input-bar]
---

Context: [context/viewer.md](../context/viewer.md).

The inline `:`-completer for text fields: type `:` to open an autocomplete of
emoji short-codes (`:smile:`), filter as you type, and insert the Unicode glyph.
Reads short-codes from [[viewer-emoji-data]], renders in the
[[viewer-ui-text-input-widget]], and its primary consumer is the chat input
([[viewer-chat-input-bar]]).

Reference (Firestorm, read-only): `llemojihelper`, `llpanelemojicomplete` (the
inline `:`-completer).
