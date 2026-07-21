---
id: viewer-chat-input-world-autostart
title: World keypress auto-starts nearby chat
topic: viewer
status: ready
origin: split from viewer-chat-input-bar during the chat-input widget work (2026-07)
blocked_by: [viewer-chat-input-bar, viewer-ui-settings-store]
---

Context: [context/viewer.md](../context/viewer.md).

The reference viewer's **"letter keys start local chat"** affordance
(Firestorm's setting): while the **World** input context owns the keyboard, a
printable keypress opens the nearby-chat bar ([[viewer-chat-input-bar]]) and
forwards that character into it, so a user can just start typing without first
clicking the bar.

This belongs to the **nearby-chat bar only** — not the reusable widgets
([[viewer-ui-text-input-emoji]] / [[viewer-chat-channel-and-commands]]) and not
the conversations-floater's local tab ([[viewer-social-im-conversations]]),
which should not hijack world keypresses. So it is wired at the bar, gated on a
`gSavedSettings`-style bool ([[viewer-ui-settings-store]], default on).

Deferred out of the initial chat-input work at the user's request; the widgets
and the bar's own focus flow (Enter focuses, Esc blurs) land first.

Reference (Firestorm, read-only): `fsnearbychatcontrol` / `llchatbar` keystroke
start, the `LetterKeysFocusChatBar`-style setting.
