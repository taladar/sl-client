---
id: viewer-window-title-unread-count
title: Unread IM/chat count in the window title
topic: viewer
status: ready
origin: debug-settings/chat-lines survey (2026-07-23)
refs: [chat-b4]
---

Context: [context/viewer.md](../context/viewer.md).

Surface pending messages on a minimised/backgrounded viewer: put the
unread IM/chat count into the OS window title (and thereby the taskbar /
window list), updating live as counts change.

Scope:

- Aggregate the per-session unread counts ([[chat-b4]], done) into a
  total and set the Bevy window title to `(<n>) <base title>` when
  non-zero, restoring the plain title at zero
  (`FSShowMessageCountInWindowTitle`).
- Separate toggles for counting nearby chat vs. IMs
  (`FSNotifyUnreadChatMessages`, `FSNotifyUnreadIMMessages`).
- Counts clear when a session is marked read (existing mark-read flow).

Reference (Firestorm, read-only): `FSShowMessageCountInWindowTitle` and
its consumer in the conversation floater glue.

Builds on: per-session unread tracking (done) and the window handle
already owned by the Bevy app.
