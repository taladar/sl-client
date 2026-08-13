---
id: protocol-sim-login
title: Server-side login surface at full fidelity
topic: protocol
status: done
origin: user request (2026-07) — complete simulator protocol surface
points: 5
refs: [viewer-fake-grid]
---

Context: [context/protocol.md](../context/protocol.md).

`sl-wire/src/login.rs` already has a login *server*
(`parse_login_request`, `LoginServer::respond` with password/MFA checks,
`build_login_response`). Complete it to full fidelity against our own
client parser (`parse_login_response`) and the reference viewer's
expectations:

- every response field the client consumes: inventory-skeleton + library,
  buddy-list, gestures, login-flags, global-textures, event/classified
  categories, ui-config, home/look_at, seed_capability, helper URIs,
  agent flags/limits;
- the failure paths: bad credentials, MFA required/invalid (the existing
  `LoginServer::respond` MFA check is the seed), TOS / critical-message
  gates, "presence"/already-logged-in, login redirects;
- the LLSD login variant alongside XML-RPC if the client or reference
  viewer uses it (verify against Firestorm; SL grids accept LLSD login).

Round-trip tested against `build_login_request`/`parse_login_response`
in-memory; [[viewer-fake-grid]] consumes it as-is.
