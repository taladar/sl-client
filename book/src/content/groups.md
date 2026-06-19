# Groups

Groups are the protocol's membership organizations: a named group an avatar can
join, with **roles** that bundle **powers**, a **member** roster, **titles**,
and **notices**. Group chat is covered in the
[Chat](chat.md#group-and-conference-sessions) chapter; this chapter is about the
group data and administration.

## The model

- A **group profile** holds the group-wide settings: founder, insignia, fees,
  whether it is open enrollment, the charter, and member/role counts.
- A **role** is a named set of **powers** (a bitmask — invite members, eject,
  send notices, manage roles, manage land, …). Every group has an "Everyone"
  role; an "Owners" role holds the dangerous powers.
- A **member** has a set of roles, a chosen **title** (drawn from their roles),
  contribution, and notice/online flags.
- A **notice** is a group-wide message, optionally with an inventory attachment;
  the list of notices is fetched separately from any one notice's full body.

One group can be the avatar's **active** group, which sets the group tag shown
over the avatar and the group used for land actions.

## Reading group data

Each facet is requested and returned separately, because they can be large:

| Request command | Result event |
|-----------------|--------------|
| `RequestGroupProfile` | `GroupProfileReceived` |
| `RequestGroupMembers` / `FetchGroupMembers` | `GroupMembers` |
| `RequestGroupRoles` | `GroupRoleData` |
| `RequestGroupRoleMembers` | `GroupRoleMembers` |
| `RequestGroupTitles` | `GroupTitles` |
| `RequestGroupNotices` / `RequestGroupNotice` | `GroupNotices` |

The avatar's own memberships arrive as `Event::GroupMemberships`, and changing
the active group yields `Event::ActiveGroupChanged`. On Second Life some of this
(notably bulk member data) comes through the `GroupMemberData`
[capability](../comms/caps.md) rather than UDP.

## Administering a group

The write side covers the full lifecycle, gated by the caller's powers:

- **Lifecycle** — `CreateGroup`, `JoinGroup`, `LeaveGroup`, `ActivateGroup`,
  `InviteToGroup`, `EjectGroupMembers`.
- **Roles** — `UpdateGroupRoles` (create/edit/delete roles and their powers),
  `ChangeGroupRoleMembers` (assign/remove members to/from roles).
- **Member settings** — `SetGroupAcceptNotices`, `SetGroupContribution`.
- **Notices** — `SendGroupNotice` (optionally with an inventory attachment).

Results come back as `CreateGroupResult`, `JoinGroupResult`, `LeaveGroupResult`,
`EjectGroupMemberResult`, and `DroppedFromGroup`.

---

> **In this codebase**
>
> - Types are in `sl-proto/src/types/group.rs`: `GroupProfile`,
>   `GroupMembership`, `GroupMember`, `GroupRole`, `GroupRoleMember`,
>   `GroupTitle`, `GroupNotice`, `GroupNoticeAttachment`, `CreateGroupParams`,
>   the role-edit helpers (`GroupRoleEdit`, `GroupRoleMemberChange`), and
>   `ActiveGroup`; group powers live with `group_powers` (re-exported from
>   `sl-proto`).
> - Commands and events are the `*Group*` variants in `sl-proto/src/command.rs`
>   and `sl-proto/src/types/event.rs`; the `GroupMemberData` cap is
>   `CAP_GROUP_MEMBER_DATA`.
> - Worked example: `sl-client-tokio/examples/group_admin.rs`.
