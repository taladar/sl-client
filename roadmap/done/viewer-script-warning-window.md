---
id: viewer-script-warning-window
title: Nothing in the viewer shows a script run-time error
topic: viewer
status: done
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

## Measured (2026-10-26, [[gridspec-lsl-ossl]])

The "owner only" premise above does not hold on OpenSim for OSSL: with the
stock `PermissionErrorToOwner = false` a denied or failing OSSL call
(`OSSL Permission Error: …`, `OSSL Runtime Error: …`) reaches **every**
avatar near the object as `DEBUG_CHANNEL` chat, and a deprecated function's
warning or an `osRegexIsMatch` pattern error is a `DEBUG_CHANNEL` **shout**
that the other avatar and scripts 50 m away hear too. So the owner-only
default has to be the window's own filter (by the chat's owner id), not
something the grid already did. The two engines wrap the same text
differently — YEngine `YEngine: <text> (script: <name> event: <event>
primID: <key> at: <x,y,z>) Script must be Reset to re-enable.`, XEngine
`<text>(script: <name> event: <event> primID:<key> at <x, y, z>)` — so the
script name, prim and position the window wants to show are in the text, in
two layouts (`book/src/gridspec/lsl.md` § OSSL).

## Done (2026-10-27, with [[gridspec-lsl-runtime-errors]])

- **The window.** `sl-viewer-notices`' `script_errors` (Advanced ▸ Script
  Errors, floater `script-errors`): an All tab and a tab per speaking object
  in the order they first spoke, each line stamped `[HH:MM]` with the local
  time it arrived, a multi-line message (XEngine's .NET stack, YEngine's
  report) a row per line, at most 1000 messages, a Clear button. A new line
  opens it, as the reference does. Debug lines wear the skin's console-error
  class.
- **One routing rule.** `world_api::script_chat_route` decides for the
  window, the Nearby transcript and the chat overlay alike, so a line shows
  in exactly one place. Preferences ▸ Chat ▸ Script errors:
  `ScriptErrorsShowIn` (`window`, the reference's default; `chat`;
  `nowhere`), `ScriptErrorsFromOthers` (off: only the agent's own objects,
  by the chat's owner field — the grid does not filter for us on OpenSim),
  `ScriptErrorsOwnerSay` (`llOwnerSay` to the window instead of nearby
  chat) and `ScriptErrorsClearOnClose`.
- **In nearby chat** a debug line wears its own palette colour,
  `ChatColorScriptError` (`--chat-script-error`, the reference's
  `ScriptErrorColor`), in the transcript and the overlay.
- **Click-through.** An object's tab is headed by its name as an `objectim`
  link carrying its owner and SLURL — the object inspector, with Show on
  Map — and where it was, and a debug beacon marks the object while the tab
  is shown.
- **Tests.** Unit tests of the route and the log; `e2e_script_errors` on the
  fake grid (the window opens on the owner's object's two fault lines, with
  a tab for it; another owner's line is not shown; routed to chat it is a
  `debug` line of the Nearby transcript); `e2e_pilot`'s debug-channel test
  now opts its listener into chat. A live stage,
  `a_scripts_fault_opens_the_script_errors_window_on_a_live_grid`
  (`e2e_objects`, `SL_E2E_GRID=opensim|aditi`), saves a division by zero from
  the script editor and expects the grid's words in the window.

Not done, and why:

- **Editor at the reported line**: no grid names a line (Second Life's
  `Math Error`, OpenSim's `(script: … event: …)`), so there is nothing to
  open the editor at.
- **Firestorm's owner-say routing variant**
  (`FSllOwnerSayToScriptDebugWindowRouting`: All tab only, object tab only,
  both): an owner-say sent here goes to both, the variant's default.
- **The chat log on disk** is written by the runtime (`sl-client-bevy`'s
  `ChatLog`), below the routing, so a debug line is still logged with
  nearby chat whichever surface shows it.
