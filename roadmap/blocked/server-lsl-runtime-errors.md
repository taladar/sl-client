---
id: server-lsl-runtime-errors
title: Run-time errors where a resident can see them
topic: server
status: blocked
origin: LSL-on-the-fake-grid audit (2026-09-20)
points: 3
blocked_by: [server-lsl-vm-execution]
refs: [server-world-chat-routing, server-lsl-memory-and-limits,
  viewer-lsl-editor-save-compile]
---

Context: [context/lsl.md](../context/lsl.md).

A script that fails at run time on a real grid does three things, none of
them silent: it shouts the error on **`DEBUG_CHANNEL`**
(`0x7FFFFFFF`, 2147483647) from the object's position, it stops, and the
viewer pops its script-warning window showing object name, owner, region
position and the message. Firestorm's `llfloaterscriptdebug.cpp` is the
consumer; a `ChatFromSimulator` with `ChatType::Debug` is the wire.

Wanted, once the VM can fail ([[server-lsl-vm-execution]]):

- a single error path that formats the reference's message shapes —
  "Math Error", "Stack-Heap Collision", "Script run-time error", with
  the script name and the line the source map yields
  ([[server-lsl-compiler-ir]] carries the span);
- delivery as a `DEBUG_CHANNEL` shout through the region's chat fan-out
  ([[server-world-chat-routing]]), so a script listening on
  `DEBUG_CHANNEL` hears it exactly as on a real grid — the standard way
  content self-reports;
- the script left **stopped** in the state it failed in, so
  `llGetScriptState` and the viewer's Running checkbox agree;
- `llScriptDanger` and `llGetScriptState` answering from the same
  record.

There is a viewer half too, but it is not this task's: check whether the
Bevy viewer renders a `ChatType::Debug` message anywhere, and if not,
raise a separate `viewer-*` task for a script-warning surface rather
than widening this one. The editor's *compile* errors already have a
home ([[viewer-lsl-editor-save-compile]] renders them as a listed
report); run-time errors do not.

Acceptance: a script dividing by zero produces one `DEBUG_CHANNEL`
message naming the script and the line, stops, and reports stopped; the
region keeps ticking; and a second script listening on `DEBUG_CHANNEL`
receives it.
