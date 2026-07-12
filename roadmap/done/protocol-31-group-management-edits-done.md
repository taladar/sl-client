---
id: protocol-31
title: Group management edits (done)
topic: protocol
status: done
origin: ROADMAP.md — Tier C
---

Context: [context/protocol.md](../context/protocol.md).

**31. Group management edits (done) ✅ — group-notice creation,
`GroupRoleUpdate`, `GroupRoleChanges`, `EjectGroupMemberRequest` · 5 pts.
(extends #7, Tier A.)** Item #7 implemented membership, roster/role/profile
reads, group IM sessions, and the join/leave/invite/contribution/accept-notices
writes, but deferred the admin edits. This completes the roster-admin surface
for an owner/officer bot. Implemented: **role create/update/delete** —
`Session::update_group_roles` (`GroupRoleUpdate`, one `RoleData` block per edit)
taking a `Vec<GroupRoleEdit>` (`role_id`, name/description/title, a `powers`
u64, and a `GroupRoleUpdateType` selecting
`Create`/`UpdateData`/`UpdatePowers`/`UpdateAll`/`Delete`, the wire bytes
matching the viewer's `LLRoleChangeType` and OpenSim's
`OpenMetaverse.GroupRoleUpdate`); **member-role assignment** —
`change_group_role_members` (`GroupRoleChanges`, `Vec<GroupRoleMemberChange>`
with a `GroupRoleChange` `Add`=0/`Remove`=1); **ejecting members** —
`eject_group_members` (`EjectGroupMemberRequest`), with the
`EjectGroupMemberReply` surfaced as `Event::EjectGroupMemberResult`; and
**group-notice creation** — `send_group_notice` (`ImprovedInstantMessage`,
`IM_GROUP_NOTICE`, subject and body joined with `|`, `from_group` false) taking
an optional `GroupNoticeAttachment` (`item_id`, `owner_id`), packed into the
binary bucket as the viewer's serialized LLSD stream — the 15-byte
`<? LLSD/XML ?>\n` header (which OpenSim's group module strips verbatim) plus an
LLSD-XML `{ item_id, owner_id }` map, with the one-byte empty bucket sent when
there is no attachment (new sl-wire `build_group_notice_bucket`). New value
types `GroupRoleEdit`, `GroupRoleUpdateType`, `GroupRoleMemberChange`,
`GroupRoleChange`, `GroupNoticeAttachment`, and a `group_powers` constants
module (the `GP_*` power bits). All wired as `Command`/`SlCommand`
(`UpdateGroupRoles`, `ChangeGroupRoleMembers`, `EjectGroupMembers`,
`SendGroupNotice`) through both runtimes. Covered by one sl-wire test (the
notice bucket's LLSD header) and five `lifecycle.rs` tests (the three send
encodings, the eject reply → event, and the notice IM with/without attachment).
*Live-verified against the local OpenSim (Groups V2) via the new `group_admin`
tokio example: created a group, posted a notice (relayed back to the agent as a
member — `"sl-client #31|group management edits work"`), then ran a full role
create → list → update → delete cycle (the new role appeared with powers
`0x4000_0000_0002` = `MEMBER_INVITE | NOTICES_SEND`, its `UpdateAll` changed the
title to "Senior Tester" and powers to `NOTICES_SEND`, and the delete dropped it
from the 4-role list back to 3). The role-member assignment and eject paths need
a second group member (`SL_MEMBER`), so they are unit-tested only. Test: local
OpenSim with the Groups V2 module (MariaDB backend).*
