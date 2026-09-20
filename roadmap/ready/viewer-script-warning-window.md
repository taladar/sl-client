---
id: viewer-script-warning-window
title: Nothing in the viewer shows a script run-time error
topic: viewer
status: ready
origin: LSL-on-the-fake-grid audit (2026-09-20)
points: 3
refs: [server-lsl-runtime-errors, viewer-lsl-editor-save-compile]
---

Context: [context/viewer.md](../context/viewer.md).

`ChatType::DebugChannel` is decoded (`sl-proto`'s `ChatType`, code 6)
and **no viewer crate mentions it**: the only reference to
`DebugChannel` outside `sl-proto` in the whole workspace is `sl-repl`'s
chat-type argument parser. So a script that errors at run time on any
grid — the local OpenSim today, the fake grid once it runs scripts —
says so into a channel this viewer drops on the floor.

The reference has a dedicated window for it
(`llfloaterscriptdebug.cpp`, Advanced ▸ Show Script Errors), with a
per-object tab and the choice of showing errors in a window or in
nearby chat, because a broken script in a crowded region is otherwise
unfindable. Firestorm adds the object name, the owner and a clickable
SLURL to its position.

What it needs here:

- a surface for `ChatType::DebugChannel` messages: object name, owner,
  region position, message, most recent first;
- routed by a **preference** — window, nearby chat, or nothing — since
  the messages are mostly other people's broken content and some
  residents want them silenced; the reference's default is a window and
  Firestorm's is nearby chat;
- **only the owner's own objects by default**, which is the rule the
  grid applies anyway (`DEBUG_CHANNEL` output goes to the owner), so the
  window should not pretend to more reach than it has;
- a click-through to the object (camera, or the object menu) and, when
  the script is modifiable, to the editor at the reported line —
  [[viewer-lsl-editor-save-compile]] already lists compile errors with
  line and column, so the two error surfaces should read alike.

This is independently useful: a run-time error today is invisible
against the *live* grids too, which makes any scripted content
misbehaviour a silent mystery.

Acceptance: a script erroring on the local OpenSim shows its message,
object and position in the viewer within one frame of the chat arriving;
the preference silences it; and a `ChatType::DebugChannel` message
routed to nearby chat is visibly distinguished from ordinary chat.
