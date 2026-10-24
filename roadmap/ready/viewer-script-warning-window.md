---
id: viewer-script-warning-window
title: Nothing in the viewer shows a script run-time error
topic: viewer
status: ready
origin: LSL-on-the-fake-grid audit (2026-09-20)
points: 3
refs: [server-lsl-runtime-errors, viewer-lsl-editor-save-compile,
  viewer-script-error-window, viewer-chat-channel-and-commands]
---

Context: [context/viewer.md](../context/viewer.md).

`ChatType::DebugChannel` is decoded (`sl-proto`'s `ChatType`, code 6).
Audit 2026-10-15: since this was written such lines do reach the Nearby
transcript, as automation kind `debug` (`sl-viewer-people/src/conversations.rs`
`chat_kind`, pinned by `e2e_pilot.rs`'s debug-channel test), but nothing
gives them a surface of their own, a preference or a distinct look. So a
script that errors at run time on any grid drowns in nearby chat.

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

Folded in from [[viewer-script-error-window]] (the same window, filed twice):
a **combined tab plus one tab per source object**, as the reference does;
each line timestamped with the object name; a jump-to-object **beacon**
action; a cap on retained lines. Parity-audit extensions: an option to route
`llOwnerSay` output into this window instead of (or as well as) nearby chat
(`FSllOwnerSayToScriptDebugWindow` and its routing-variant setting), and
clear-window-on-close (`FSScriptDebugWindowClearOnClose`).

This is independently useful: a run-time error today is invisible
against the *live* grids too, which makes any scripted content
misbehaviour a silent mystery.

Acceptance: a script erroring on the local OpenSim shows its message,
object and position in the viewer within one frame of the chat arriving;
the preference silences it; and a `ChatType::DebugChannel` message
routed to nearby chat is visibly distinguished from ordinary chat.

## Measured (2026-10-24, [[gridspec-lsl-comms]])

What this window will show, as each grid words it
(`book/src/gridspec/lsl.md`): a run-time error on Second Life is two lines
from the prim, `Object [script:<name>] Script run-time error` then the
error (`Too Many Listens`, `Math Error`, …), while OpenSim sends the owner
one line naming script, prim and place; a refused library call is one line
on both, in each grid's words (`llDialog: button list too long, must be 12
or fewer entries` against `llDialog: No more than 12 buttons can be shown`,
`Invalid Key passed to instant message.` against `llInstantMessage: An
invalid key  was passed to llInstantMessage`).
