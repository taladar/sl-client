---
id: test-script-running
title: query/toggle script running, reset
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 9 — Scripting & permissions `[both]`
---

Context: [context/test.md](../context/test.md).

`script-running` — query/toggle script running, reset. `1av`.
The three commands that drive a script *after* it compiles: the viewer's
object-Contents "Running" checkbox reads the state with `GetScriptRunning`
([`Command::RequestScriptRunning`] → [`Event::ScriptRunning`]) and writes it
with `SetScriptRunning`, and the "Reset" button re-initialises it with
`ScriptReset`. Only the *get* draws a reply — set/reset are fire-and-forget —
so every mutation is verified by a follow-up query. Rather than borrow a
fixture prim, the case owns a script it can freely toggle: it rezzes a
container cube and creates a **new script directly in it** with
[`Command::RezScript`] + [`RestoreItem::new_script`] (the object-Contents "New
Script"). OpenSim's `RezNewScript` fills a default body **and starts it**, so
the script is running the moment it appears in the task inventory. The case
then: queries → asserts **running** (auto-start); `SetScriptRunning(false)`
→ queries → asserts **stopped**; `SetScriptRunning(true)` → queries → asserts
**running** again; `ResetScript` → queries → asserts still **running** and the
circuit healthy (a keep-alive ping still round-trips — the reset leaves a
running script running and carries no reply, so "no error" is read the way
`script-dialog` / `script-permissions` read their reply-less commands). Each
query polls across the engine's asynchronous compile/start/stop:
`GetScriptRunning` returns *nothing* while the engine has no live instance yet
(still compiling), so a per-attempt timeout re-queries rather than failing.
**Surfaced & fixed a real client gap** (behavioural, in `sl-proto` so both
runtimes get it): OpenSim answers `GetScriptRunning` over the **CAPS event
queue** (`ScriptRunningReply`, `{ Script: [ { ObjectID, ItemID, Running, Mono
} ] }`) whenever the region has an event queue — its default, and modern SL —
rather than the UDP `ScriptRunningReply` the client parsed, so no reply
reached [`Event::ScriptRunning`]; `handle_caps_event` now decodes that event
(helper `script_running_from_caps_llsd`, regression test
`script_running_reply_caps_surfaces_run_state`). The case lives in a prim's
*task* inventory, reached through the same `RezScript` task-write Second Life
silently drops (the open investigation tracked with `script-upload` in Phase
Z), so `[opensim]` only — the SL variant defers with the task-inventory batch.
No other new client surface (the query/set/reset commands and the event all
existed). Green on OpenSim: create+query ≈ 69 ms, stop ≈ 100 ms, start
≈ 100 ms, reset ping ≈ 0.5 ms loopback. `[opensim]` only.
