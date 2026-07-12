---
id: protocol-42
title: Group-reply pagination totals (extends #7, Tier A). Done
topic: protocol
status: done
origin: ROADMAP.md — Tier E
---

Context: [context/protocol.md](../context/protocol.md).

**42. Group-reply pagination totals (extends #7, Tier A). ✅ Done.**
`GroupRoleDataReply` dropped the `RoleCount` header and `GroupRoleMembersReply`
dropped `TotalPairs`. These replies are multi-packet, and `GroupMembersReply`
already surfaces its `member_count`, so a client could tell when the member set
was complete but *not* the role or role-member sets. Surfaced both totals as new
fields on the existing events: **`Event::GroupRoleData.role_count`** (`i32`,
from the `GroupData` block) and **`Event::GroupRoleMembers.total_pairs`**
(`u32`, from the `AgentData` block) — the simulator-reported totals across all
packets of the
reply, so a client comparing them against the accumulated `roles.len()` /
`pairs.len()` knows when a (potentially multi-packet) set is complete. Both
fields flow through both runtimes unchanged (the events are shared `sl-proto`
types; every consumer binds them with `{ .. }`, so no command wiring was
needed). Covered by two new `sl-proto` lifecycle tests
(`group_role_data_reply_surfaces_role_count` and
`group_role_members_reply_surfaces_total_pairs`, each asserting the header total
decodes alongside a single-entry packet). *Live-verified against the local
OpenSim (Groups V2) via the `group_admin` tokio example, extended to fetch role
members and log both totals: a freshly-created group's `GroupRoleDataReply`
surfaced `role_count=4` (Everyone/Officers/Owners + the new role; dropping to 3
after the role delete) and its `GroupRoleMembersReply` surfaced `total_pairs=2`
(the owner in Everyone + Owners) — both previously dropped. Test: local OpenSim
with the Groups V2 module.*
