---
id: protocol-14
title: Estate/region management (done)
topic: protocol
status: done
origin: ROADMAP.md — Tier B
---

Context: [context/protocol.md](../context/protocol.md).

**14. Estate/region management (done) ✅ — `EstateOwnerMessage`
(kick/ban/restart/teleport-home/manage), `GodlikeMessage` · 5 pts.**
Region/estate admin for owners — a restart/ban/management bot. `Session` gains
`request_estate_info` (`getinfo` → `Event::EstateInfo` +
`Event::EstateAccessList` per category),
`update_estate_access(EstateAccessDelta, target)` (ban/manager/allowed
agent/group add+remove via `estateaccessdelta`), `kick_estate_user`,
`teleport_home_user`/`teleport_home_all_users`, `restart_region` (`-1` delays),
`send_estate_message` (estate-wide blue box),
`set_region_info(&RegionInfoUpdate)`
(maturity/agent-limit/object-bonus/toggles), plus the god-level `god_kick_user`
(`GodKickUser`) and a generic `send_godlike_message`. Incoming
`EstateOwnerMessage` `estateupdateinfo`/`setaccess` replies are parsed (the
access-list UUIDs are raw 16-byte params, one category per message). Added
`Maturity::to_sim_access`. Wired through both runtimes. *Live-verified logged in
as the estate owner: `getinfo` returned the estate config ("My Estate", id 101,
flags) and all four access lists, and a ban add + re-`getinfo` round-trip showed
the banned agent (then removed it). The kick/teleport-home/restart/setregioninfo
and god ops are unit-tested only (disruptive or god-gated to live-test).*
