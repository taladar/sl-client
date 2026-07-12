---
id: test-script-permissions
title: request/grant/revoke a script permission
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 9 — Scripting & permissions `[both]`
---

Context: [context/test.md](../context/test.md).

`script-permissions` — request/grant/revoke a script permission. `1av`.
A script asks the agent for LSL permissions with `llRequestPermissions`: the
simulator sends a `ScriptQuestion` naming the holding object, its script item,
the owner and the requested `PERMISSION_*` bitfield; the agent answers with a
`ScriptAnswerYes` granting a subset (empty = explicit deny) and may later
withdraw with `RevokePermissions`. `sl-proto` keeps a **local mirror** of what
the agent answered — never a security boundary — readable through
[`Command::QueryScriptPermissions`], which the runtime answers by synthesizing
an [`Event::ScriptPermissionState`] snapshot (no wire traffic). The case
exercises all three edges against the Default Region's `SLClientScriptTester`
prim (the Phase-8 #8 fixture, which calls `llRequestPermissions(av,
PERMISSION_DEBIT)` on a 4 s timer): it waits for the request, asserts the
parse (a holder, a script item, `DEBIT` in the requested set), grants exactly
that subset with [`Command::AnswerScriptPermissions`], queries the mirror and
asserts the grant is recorded (`Granted`, not `Denied`, carrying `DEBIT`),
then revokes with [`Command::RevokeScriptPermissions`] and queries once more.
The revoke is faithful to the documented mirror policy: `RevokePermissions`
puts the full bitfield on the wire, but the mirror only *follows* the
animation bits (`TRIGGER_ANIMATION`/`OVERRIDE_ANIMATIONS`) — every other
permission, `DEBIT` among them, the simulator keeps enforcing, so the
conservative mirror leaves the grant in place; the assertion records that
server-enforced behaviour rather than expecting a local clear.
`RevokePermissions` carries no application-level acknowledgement, so — as with
`script-dialog` — the circuit staying healthy (a keep-alive ping still
round-tripping) is read as "no error". No new client code — the
[`ScriptPermissionRequest`](Event::ScriptPermissionRequest) event and the
answer/query/revoke commands all existed (verified end-to-end in Phase 8's #8
setup); only the new case. On OpenSim the avatar is forced into the "Default
Region" whose test prim guarantees a request (its absence fails the case); the
fixture prim is wiped by any non-merge OAR load, so restoring it is a
`load oar --merge slclient8.oar` followed by a restart (the `scripts show`
console count is XEngine-only and reads 0 for the YEngine fixture —
"Initialized N script instances" on restart is the real signal; memory
`sl-client-opensim-scripted-object-testing`). On Second Life no scripted
object requests permissions from this avatar, so a window with no request
records `partial` rather than failed. Green on OpenSim: `DEBIT` requested and
granted, mirror keeps it across the revoke, request RTT ≈ 4.9 s (the timer),
reply ping ≈ 3.6 ms. `[both]`; the aditi run is deferred with the batch (no
aditi record this session).
