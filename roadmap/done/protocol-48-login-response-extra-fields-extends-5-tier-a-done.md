---
id: protocol-48
title: Login-response extra fields (extends #5, Tier A). Done
topic: protocol
status: done
origin: ROADMAP.md — Tier E
---

Context: [context/protocol.md](../context/protocol.md).

**48. Login-response extra fields (extends #5, Tier A). ✅ Done.**
`handle_login_response` (`session.rs`) plus the requested options and parser
(`sl-wire/src/login.rs`) previously captured only
`inventory-root`/`inventory-skeleton`/`buddy-list`. Now the login request also
asks for the Library options
(`inventory-lib-root`/`inventory-lib-owner`/`inventory-skel-lib`), and
[`LoginSuccess`](../../sl-wire/src/login.rs) carries the broadly-useful extras:
the **`home`** location (a new
`HomeLocation { region_handle, position, look_at }`, parsed from the quasi-LLSD
`r`-prefixed string), the start **`look_at`**,
**`agent_access`/`agent_access_max`** (the account maturity short codes), the
**`max-agent-groups`** join limit, and the **Library** root/owner ids and folder
skeleton. `sl-proto` classifies the access codes into the typed `Maturity`
(`Maturity::from_login_access`), stores the lot in a new `LoginAccount`
reachable via `Session::login_account()`, and emits it once as `Event::Account`
(plus `Event::LibraryInventory` for the library tree) right after
`Event::CircuitEstablished`. Both runtimes forward the new events. Verified
against local OpenSim:
`access Mature/Adult, max groups 42, region_handle (256000, 256000)`, library
skeleton of 19 folders. The lower-value
`gestures`/`global-textures`/`login-flags`/category lists in the same response
were left out. *Test: local OpenSim.*
