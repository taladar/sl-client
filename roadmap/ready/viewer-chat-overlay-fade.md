---
id: viewer-chat-overlay-fade
title: Nearby-chat overlay fade / decay
topic: viewer
status: ready
origin: user request (2026-07-21); follow-up to viewer-p11-1 on-screen chat
refs: [viewer-p11-1, viewer-chat-history-panel]
---

Context: [context/viewer.md](../context/viewer.md).

The bottom-left **nearby-chat overlay** (`crate::chat`, from [[viewer-p11-1]])
is the transparent-background list of recent local chat that shows over the
world even when the chat history panel is closed. Today it keeps the last N
lines indefinitely and rewrites one joined-string `Text` node;
**old lines never decay** — the corner holds a frozen block of stale chat long
after the conversation moved on.

Make it behave like the reference viewer's floating nearby-chat toasts: each
line appears fully opaque, lingers for a hold time, then **fades out** over a
short interval and is removed, so the overlay empties itself again once chat
goes quiet. A newly arriving line does not disturb the ages of lines already
fading; each line's own age drives its alpha independently.

This needs per-line entities (or a per-line alpha the renderer can animate)
rather than the single joined-string `Text` node the current overlay uses, plus
a timed system that advances each line's age, drives its `TextColor` alpha, and
despawns a line once fully transparent. Keep it read-only and input-free — the
interactive, persistent scrollback lives in the Conversations Nearby tab
([[viewer-chat-history-panel]]); this overlay is the transient heads-up display.

Tunables (hold time, fade duration, max visible lines) start as constants; a
preferences surface (fade on/off, timings) can come later. Frame-time (not
wall-clock) drives the age so it is deterministic under the screenshot harness.

Reference (Firestorm, read-only): `llfloaternearbychat` toast mode,
`LLNearbyChatToastPanel`, `LLScreenChannel` fade timers.

Builds on: the `chat.rs` overlay ([[viewer-p11-1]]).
