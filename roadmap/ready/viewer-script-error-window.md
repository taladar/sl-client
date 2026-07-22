---
id: viewer-script-error-window
title: Script warning / error window
topic: viewer
status: ready
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-ui-widget-scaffold]
refs: [viewer-chat-channel-and-commands]
---

Context: [context/viewer.md](../context/viewer.md).

The script-error window: runtime script errors and `llOwnerSay`-style debug
output arrive as chat on `DEBUG_CHANNEL` (2147483647) typed
`ChatType::DebugMessage` — today they would drown in nearby chat. This
floater collects them instead: a combined tab plus one tab per source
object (as the reference does), each line timestamped with the object name,
a jump-to-object beacon action, and a cap on retained lines. Includes the
"show script errors in chat vs window" preference the reference offers.

Reference (Firestorm, read-only): `llfloaterscriptdebug`,
`floater_script_debug.xml`.

Builds on: the chat pipeline (`protocol-1`, channel handling in
[[viewer-chat-channel-and-commands]]).
