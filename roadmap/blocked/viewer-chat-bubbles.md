---
id: viewer-chat-bubbles
title: Chat bubbles
topic: viewer
status: blocked
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-name-tags-billboard-render]
---

Context: [context/viewer.md](../context/viewer.md).

Chat-bubble mode (`UseChatBubbles`): recent nearby-chat lines render
inside the speaker's name tag — whisper in italic, shout in bold, coloured
by speaker type, fading out on a timer — plus the animated typing dots
(`.` `..` `...`) while the avatar types. An alternative chat *display*
mode, off by default like the reference; the tag surface, sizing and fade
machinery come from [[viewer-name-tags-billboard-render]], which this
extends with a multi-line text body per tag.

Reference (Firestorm, read-only): `llhudnametag` (`mVisibleChat` path),
`llvoavatar::idleUpdateNameTagText`.

Deps: [[viewer-name-tags-billboard-render]] (the tag renderer).
