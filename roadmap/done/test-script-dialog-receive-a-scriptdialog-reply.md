---
id: test-script-dialog
title: receive a ScriptDialog, reply
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 9 — Scripting & permissions `[both]`
---

Context: [context/test.md](../context/test.md).

Needs XEngine + a scripted-object OAR (appendix). Note SL enforces god-bit;
OpenSim may not.

`script-dialog` — receive a `ScriptDialog`, reply. `1av`. A script raises a menu
on an avatar's viewer with `llDialog` (or a free-text prompt with `llTextBox`):
the simulator sends a `ScriptDialog` naming the object, its owner, the prompt
text, the button labels and a hidden negative chat channel, and the avatar
answers by chatting the chosen button's label on that channel — a
`ScriptDialogReply` the script hears on its `llListen`. The case exercises both
edges: it waits for the dialog the Default Region's scripted test prim
(`SLClientScriptTester`, the Phase-8 #8 dialog fixture — `llDialog` on channel
`-4242` with `Yes`/`No` buttons, fired on a 4 s timer) raises on login, asserts
the parse (a hidden channel, at least one button), then answers it with
[`Command::ReplyScriptDialog`] choosing the first button (an `llTextBox` prompt
would carry typed text in place of a real label). The reply carries no
application-level acknowledgement — the only observer of a `ScriptDialogReply`
is the script's own `llListen`, whose reaction a stock prim need not expose —
so, like `object-touch-grab`, "no error" is read from the circuit staying
healthy: a keep-alive ping still round-tripping after the reply is enqueued (the
reliable reply's encode/enqueue failure would propagate from `send` first). No
new client code — the [`ScriptDialog`](Event::ScriptDialog) event and
[`ReplyScriptDialog`](Command::ReplyScriptDialog) command surface all
existed (verified end-to-end in Phase 8's #8 setup); only the new case. On
OpenSim the avatar is forced into the "Default Region" whose test prim
guarantees a dialog (its absence fails the case); the fixture prim is wiped by
any non-merge OAR load, so restoring it is a `load oar --merge slclient8.oar` —
the ARCHIVER starts its script on load, no restart needed (memory
`sl-client-opensim-scripted-object-testing`). On Second Life no scripted object
menus this avatar, so a window with no dialog records `partial` rather than
failed. Green on OpenSim: dialog on channel `-4242`, 2 buttons, reply RTT ≈ 0.5
ms ping. `[both]`; the aditi run is deferred with the batch (no aditi record
this session).
