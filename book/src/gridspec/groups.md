# Groups

A group is founded with `CreateGroupRequest`, joined with `JoinGroupRequest`
(or an invitation, dialog 3, answered with dialog 35 or 36), left with
`LeaveGroupRequest` and emptied with `EjectGroupMemberRequest`. Its profile,
roles, members, titles and notices are read with requests of their own; a
notice is posted as an instant message (dialog 32). What each avatar is
told of a membership arrives as `AgentDataUpdate` (the active group) and
`AgentGroupDataUpdate` (every membership), and a drop as `AgentDropGroup`.
Second Life adds three capabilities: `GroupMemberData` (the roster),
`GroupAPIv1` (bans) and `AcceptGroupInvite` / `DeclineGroupInvite` (an
invitation kept while the invitee was away).

Measured on 2026-10-14 by the conformance case `groups`: avatar **A** owns
the census group and does what an owner does; avatar **B** is the member it
acts on and goes away three times. Two runs on aditi (Mauve) and two on the
local OpenSim (Groups V2); no answer changed between two runs of a grid, and
every run is held to them (**held**). The fake grid has no groups yet, so
it is not measured here (`server-fake-grid-groups`).

A role's powers are written as the hex of the `GP_*` bitfield; a group's
name and the avatars' names are left out.

## Founding

| behaviour | Second Life | OpenSim |
| --- | --- | --- |
| the founder is told (**held**) | `CreateGroupReply` taken, **no text**; two balance updates "You paid Governor Linden L$100." (type 1002) | `CreateGroupReply` taken, "Group created successfully"; **the new group becomes the active one**, and a membership list follows |
| what it costs (**held**) | **L$100** | nothing |
| the new group's profile, read by the founder (**held**) | **1 member, 2 roles**, the founder's title "Member" and powers `0x80018010000` (those of everyone) | 1 member, 3 roles, the founder's title "Owner of …" and the owners' powers |
| its roles (**held**) | Everyone (title "Member", `0x80018010000`), Officers ("Officer", `0x1ccfffffff8ce`), Owners ("Owner", **every bit**, `0xffffffffffffffff`) | Everyone ("Member of …", `0x390018010000`), Officers ("Officer of …", `0x1fdffff7ff8ce`), Owners ("Owner of …", `0xffffffffffffe`) |
| its members over UDP (**held**) | **none** (a count of 0 and one block naming nobody) | the founder |
| its role members (**held**) | the founder in the owners' role only | the founder in the owners' role and in the Everyone role |
| its titles (**held**) | "Member" and "Owner", Owner worn | "Everyone" and "Owners", Owners worn |
| a name of 36 characters (**held**) | founded, **cut to 35**, and charged | founded as it is |
| the founder leaves, the only member (**held**) | alerts "0 users remain in group. Group will be disbanded by nightly cleanup." and "You have left the group '…'."; the leave taken | the leave taken; `AgentDropGroup`; no active group; a membership list without it |

Second Life's profile and roster of a group read like a cache: the profile
of the census group, with two members, says 1 member and 2 roles to
everyone, and names the reader's title and powers as a member's even to an
outsider.

## Membership

| behaviour | Second Life | OpenSim |
| --- | --- | --- |
| B leaves: B is told (**held**) | alert "You have left the group '…'."; the leave taken; `AgentDropGroup` **over the event queue**; two failed balance updates | the leave taken; `AgentDropGroup` over UDP; no active group; a membership list without it |
| … A is told (**held**) | nothing | nothing |
| an outsider reads the profile (**held**) | as a member does, stale | the profile, its own title empty and powers 0 |
| … the roles (**held**) | nothing | every role |
| … the members (**held**) | none | every member |
| A edits the charter: who is told (**held**) | nobody; an outsider reads the new charter at once | the same |
| B joins: B is told (**held**) | alert "You have been added to the group."; the join taken | the join taken; **the group becomes B's active group**; a membership list |
| … A is told (**held**) | nothing | nothing |
| the members over UDP (**held**) | **none** | both, with titles, powers and "Online" |
| the members over `GroupMemberData` (**held**) | both, with titles, powers and "Online" | both, the same as over UDP |
| B makes it its active group, and none (**held**) | `AgentDataUpdate` naming it, title "Member"; then none; a membership list each time | the same, title "Member of …" |

## Roles and titles

| behaviour | Second Life | OpenSim |
| --- | --- | --- |
| A makes a role: the grid's id for it (**held**) | its own, not the one sent | its own, not the one sent |
| A makes, changes, assigns, unassigns and deletes a role: who is told (**held**) | nobody | A, each time, an `AgentDataUpdate` of its active group; B nothing |
| the role members once B is in the role (**held**) | A in the owners' role, B in the new role — **no Everyone pairs** | the same with each in the Everyone role too |
| B's titles while in the role (**held**) | "Member" (worn) and the role's title | "Everyone" and the role's **name**, the role worn |
| B wears the role's title: B is told (**held**) | an `AgentDataUpdate` with no active group (the group is not B's active one) | the same |
| … A's roster then (**held**) | none over UDP | B with the new title and the role's powers |

## Notices

| behaviour | Second Life | OpenSim |
| --- | --- | --- |
| A posts a notice: B is sent (**held**) | dialog 32 **from A's id** with the group flag, A's name, not kept, no region, `subject\|body`, a bucket of 19 bytes (no inventory, the group's id) | dialog 32 **from the group's id**, A's name, **marked kept**, no region, the same text and bucket |
| … A is sent (**held**) | the same notice — **under another id**: each recipient's message has an id of its own | the same notice under the same id, the notice's |
| the notice list, 10 s later (**held**) | the notice, A's name, the subject, a time | the same |
| B asks for the notice again (by the listed id) (**held**) | dialog **37** (`IM_GROUP_NOTICE_REQUESTED`) from A's id, the sender's name **an id**, under yet another id | dialog 37 from the group, A's name, under the notice's id |
| B stops taking notices; A posts (**held**) | B is sent nothing; A still is | the same |
| a notice with a notecard (**held**) | a bucket of 32 bytes: inventory 1, type 7, the group, the item's name; listed with an attachment of type 7 | the same |
| a group without notices, listed | one entry of no id, "No notices." (seen once, on a group founded that run) | not seen |

## Accounts and proposals

| request | Second Life | OpenSim |
| --- | --- | --- |
| summary, details, transactions of the current week (**held**) | answered: 7 days, interval 0, dates given, balance 0, 2 members taxed, no entries | **nothing** |
| active proposals, vote history (**held**) | answered: none | **nothing** |

## Ejection

| behaviour | Second Life | OpenSim |
| --- | --- | --- |
| A ejects B: A is told (**held**) | a message from A to A, "… has  been ejected from '…' by ….", and the ejection taken | the same without the doubled space |
| … B is told (**held**) | a message "You have been ejected from '…' by …."; two failed balance updates; **nothing else, then or 20 s later** — no drop, no membership list | the message; `AgentDropGroup`; no active group; a membership list |

## Invitations

| behaviour | Second Life | OpenSim |
| --- | --- | --- |
| the invitation as B is sent it (**held**) | dialog 3 from the group, **A's user name**, its own id, no region, "… has invited you to join a group.\nThere is no cost to join this group.\nGroup:\n…: <charter>", a bucket of 20 bytes: fee 0, the Everyone role | dialog 3 from the group, A's name, its own id, a region, "… has invited you to join a group called …. There is no cost to join this group.", the same bucket |
| … A is told (**held**) | nothing | nothing |
| B declines: who is told (**held**) | nobody | nobody |
| B accepts: B is told (**held**) | alert "You have been added to the group."; the join taken; the group made active; a membership list | a message box from "Groups", "You have been added to the group."; the group made active; a membership list |
| … A is told (**held**) | nothing | nothing |
| an invitation to a member (**held**) | **sent**, as any | **not sent** |

## Bans

Second Life only: OpenSim grants no `GroupAPIv1`.

| behaviour | Second Life |
| --- | --- |
| the list, read with a GET (**held**) | `200`, `{ban_list: {}, group_id}` — the `group_id` **a string** |
| a ban, POSTed (**held**) | `201` with the text "Created", not LLSD; nobody told |
| the list then (**held**) | B, with a `ban_date` |
| B, banned, joins (**held**) | alert "This group has banned you from joining."; the join refused |
| A invites B, banned (**held**) | A: alert "Some residents have not been sent an invite due to being banned from the group."; B: nothing |
| an unban (**held**) | `200` with the text "OK"; nobody told; the list empty again |

The census bans B only once B is out: what a ban does to a member is not
measured. The reference ejects a member it bans as well.

## An invitation kept for an avatar that is away

B logs out and A invites it; A is told nothing on either grid (**held**).
B comes back three times and answers each way.

| behaviour | Second Life | OpenSim |
| --- | --- | --- |
| what B is sent unasked (**held**) | nothing of it | nothing of it |
| asked with `RetrieveInstantMessages` (**held**) | the invitation, kept, under its own id, no region | the same with a region |
| accepted over UDP with that id (**held**) | taken: as a live accept | taken |
| accepted as the client accepts a kept one (`AcceptGroupInvite`) (**held**) | taken: as a live accept | no such capability: our client answers over UDP, taken |
| read over `ReadOfflineMsgs` (**held**) | dialog 3, `from_agent_id` **A** (not the group), `from_group` false, `asset_id` **the group**, `transaction-id` nil, the bucket, kept | no such capability |
| declined through `DeclineGroupInvite` (`{group}`) (**held**) | `200`, `{success: true}`; nobody told; nothing kept afterwards | no such capability |

So an invitation read over the capability names its group only in
`asset_id`, and can be answered only through the capabilities — which take
an invitation however it was read.

## A deed by the owner

The owner rezzes a cube, sets it to the group and deeds it.

| behaviour | Second Life | OpenSim |
| --- | --- | --- |
| the group set (**held**) | taken; no alert | the same |
| a deed of the cube **not shared** with the group (**held**) | **ignored**: the owner stays A, nothing said | taken: the group owns it; base `0x00092000`, owner `0x00082000` |
| a deed once shared with the group (move, modify, copy) (**held**) | taken: the group owns it; base and owner `0x00082000`, group and everyone 0 | (taken already) |
| the owner moves it (**held**) | taken | taken |
| a member without the power moves it (**held**) | refused: alert "Can't reposition -- permission denied" | refused, nothing said |
| the owner takes it to its Trash, under the group (**held**) | taken: the item in A's **Trash**, owned by A | taken: the item in A's **Lost And Found**, owned by A |

The reference viewer offers a deed only for an object shared with the
group (`LLPanelPermissions`), which is why Second Life's silence was never
seen there. A deed without the power is in [Building](building.md).

## What the viewer does with it

- **The Groups list asks for the memberships again** each time it comes
  into view: Second Life tells an ejected member nothing it could act on.
- **A re-requested notice** (dialog 37) fills the notice panel and raises no
  toast, on both grids.
- **Empty answers** — a roster, a role list or a notice list of one entry
  naming nobody — read as empty.
- **A kept invitation is answered over the capabilities** where the region
  grants them, over UDP otherwise, and a refusal is logged.
- **Bans** are read and changed over `GroupAPIv1` (`Command::RequestGroupBans`,
  `BanGroupMembers`, `UnbanGroupMembers`, `Event::GroupBans`); the panel is
  `viewer-social-group-extras`, as are a founding dialog that warns of the
  35-character cut and the L$100, and a money tab that tolerates OpenSim's
  silence.

## Not measured

- Group chat (`gridspec-group-chat-and-conference`).
- A ban of a member.
- A join fee, a closed group, an invitation to a role other than Everyone.
- Accepting a notice's attachment (`viewer-group-notice-attachments`).
- Land deeded to a group, contributions, and a group with money.
- Proposals that exist: Second Life lists none and offers no way to start
  one any more.
