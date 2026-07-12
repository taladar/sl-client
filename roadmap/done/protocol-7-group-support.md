---
id: protocol-7
title: Group support
topic: protocol
status: done
origin: ROADMAP.md
---

Context: [context/protocol.md](../context/protocol.md).

**7. Group support · 8 pts. ✅ Done.** A group chat relay / roster tool.
Implemented across the full UDP surface: **membership & active group** —
`AgentDataUpdate` → `Event::ActiveGroupChanged` (active group, title, powers)
and `AgentGroupDataUpdate` → `Event::GroupMemberships`, with
`Session::activate_group` (`ActivateGroup`). **Roster/roles/profile** —
`request_group_members`, `request_group_roles`, `request_group_role_members`,
`request_group_titles`, `request_group_profile`,
`request_group_notices`/`request_group_notice` (the
`GroupMembersReply`/`GroupRoleDataReply`/`GroupRoleMembersReply`/
`GroupTitlesReply`/`GroupProfileReply`/`GroupNoticesListReply` round-trips →
`Event::Group{Members,RoleData,RoleMembers,Titles,ProfileReceived,Notices}`).
**Group IM sessions** — `start_group_session`/`send_group_message`/
`leave_group_session` over `ImprovedInstantMessage` (session id = group id;
`IM_SESSION_GROUP_START`/`SEND`/`LEAVE`), with incoming group chat surfaced as
`Event::GroupSessionMessage` and join/leave as `Event::GroupSessionParticipant`
(new `ImDialog` session variants 13–18). **Group management** — `create_group`
(`CreateGroupParams`), `join_group`, `leave_group`, `invite_to_group`,
`set_group_accept_notices`, `set_group_contribution`, plus
`Event::{CreateGroupResult, JoinGroupResult, LeaveGroupResult, DroppedFromGroup}`.
All wired as `Command`/`SlCommand` variants through both runtimes. Built on #2's
IM multiplexing. Verified live against the local OpenSim (Groups V2) with two
accounts: create group → fetch profile/roster → second avatar joins (open
enrollment) → roster shows both → group-chat message round-trips between them.
Also implemented the **CAPS group APIs** (the modern Second Life path): the
event-queue `AgentGroupDataUpdate` (memberships; the UDP one is `UDPDeprecated`)
→ `Event::GroupMemberships`, and the `GroupMemberData` capability POST
(`Command::FetchGroupMembers`, hex-powers / titles-by-index LLSD) →
`Event::GroupMembers` — both decoded by `Session::handle_caps_event`, wired
through both runtimes' cap-POST machinery, and covered by `parse_llsd_xml` →
`handle_caps_event` tests (SL-only, so not live-verified on OpenSim, whose UDP
path is the testable one). Deferred follow-ups: group-notice *creation*, role
create/delete and member-role assignment edits, and ejecting members. *Test:
local OpenSim with the Groups V2 module enabled (needs a MySQL/MariaDB ≤10.x
backend; OpenSim's bundled connector can't talk to MariaDB 12).*
