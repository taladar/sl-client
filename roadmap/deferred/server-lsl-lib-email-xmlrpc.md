---
id: server-lsl-lib-email-xmlrpc
title: Library tranche — email and XML-RPC, parked
topic: server
status: deferred
origin: LSL-on-the-fake-grid audit (2026-09-20)
blocked_by: [server-lsl-vm-execution]
refs: [server-lsl-lib-http-url]
---

Context: [context/lsl.md](../context/lsl.md).

Two legacy inter-region / off-world channels that exist in the language
and are almost never what modern content uses, both of which need
infrastructure a loopback test grid has no business running.

- **Email**: `llEmail`, `llGetNextEmail`, and the
  `email(string time, string address, string subject, string message, integer num_left)`
  event. OpenSim implements it with a real SMTP client and an in-memory inbox
  per object (`CoreModules/Scripting/EMailModules`). `llEmail` also carries a
  20-second implicit sleep, which is a real part of its semantics.
- **XML-RPC**: `llOpenRemoteDataChannel`, `llRemoteDataReply`,
  `llCloseRemoteDataChannel`, `llRemoteDataSetRegion`, and the
  `remote_data` event (type, channel, message id, sender, and an
  integer and a string payload) — an inbound XML-RPC endpoint per channel
  (`CoreModules/Scripting/XMLRPC`). Superseded in practice by `llRequestURL`
  ([[server-lsl-lib-http-url]]) a decade ago.

**Parked, not refused.** The reason is not difficulty: an in-process
mailbox with no SMTP, and an XML-RPC endpoint on the grid's existing
HTTP service, would both be small and could be entirely deterministic.
It is priority — no content the fake grid needs to run uses either, and
every hour here is an hour not spent on the tranches that unblock a
scripted scenario.

Unpark this when a script we actually want to run calls one of them, or
when the coverage harness's "missing" list is short enough that these
eight functions are the remainder. Until then the right behaviour is a
**stub that fails visibly** — the call compiles, returns the type's
default, and says once on `DEBUG_CHANNEL` that this grid does not
implement it — rather than a silent no-op that makes a script look
broken for the wrong reason.

Acceptance (when unparked): `llEmail` between two objects in one region
delivered through an in-process mailbox with no SMTP; a `remote_data`
channel opened, called from a test client and answered, with no network
beyond loopback.
