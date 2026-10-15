---
id: protocol-sim-group-messages
title: SimSession — decode group requests and send group replies
topic: protocol
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey]
blocked_by: [gridspec-groups]
---

Context: [context/protocol.md](../context/protocol.md).

## What

Group requests reach the server side only as raw `ClientMessage`s and
`SimSession` has no group profile / members / roles / notices senders. Add them,
loopback-tested like the other `protocol-sim-*` surfaces.

## How

Implement the behaviour the discovery task measured and the book's
`gridspec` chapter records, per `ImitatedGrid` flavour (a row in
`sl-fake-grid/src/imitates.rs` for every divergence), and hold it to the
measurement with the discovery task's conformance cases run against
`FakeSl` and `FakeOpensim`. See `roadmap/context/gridspec.md`.

## Done (2026-10-15)

Done with [[server-fake-grid-groups]]. `SimSession` decodes every group
request the census sends into a typed `ServerEvent` (founding, joining,
leaving, ejection, invitations, the active group, notices taken, profile,
members, roles, role members, titles, notices, role edits and assignments;
the account, proposal, profile-edit and title requests were typed already),
and `sim_groups.rs` sends their replies — `CreateGroupReply`,
`JoinGroupReply`, `LeaveGroupReply`, `EjectGroupMemberReply`,
`AgentDropGroup` over UDP, `AgentGroupDataUpdate`, `GroupProfileReply`,
`GroupMembersReply`, `GroupRoleDataReply`, `GroupRoleMembersReply`,
`GroupTitlesReply`, `GroupNoticesListReply` — plus `MoneyBalanceReply`. The
four group capabilities are served (`GroupMemberData` and `GroupAPIv1` handed
to the driver as `CapsDispatch` variants, the two invitation answers as a
server event), with the `GroupAPIv1` and invitation bodies' readers and the
ban list's writer beside the client's codecs. Loopback-tested in
`group_requests_and_replies_round_trip`.
