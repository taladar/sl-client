---
id: server-fake-grid-groups
title: Fake grid — a group store: membership, roles, notices, invitations and group sessions
topic: server
status: blocked
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey]
blocked_by: [gridspec-groups, gridspec-group-chat-and-conference,
  protocol-sim-group-messages]
---

Context: [context/server.md](../context/server.md).

## What

No group exists on the fake grid. Store groups, members, roles and notices; fan
out group session messages; each grid's quirks as flavour rows (activation,
`AgentDropGroup`, name cap, propagation delay).

## How

Implement the behaviour the discovery task measured and the book's
`gridspec` chapter records, per `ImitatedGrid` flavour (a row in
`sl-fake-grid/src/imitates.rs` for every divergence), and hold it to the
measurement with the discovery task's conformance cases run against
`FakeSl` and `FakeOpensim`. See `roadmap/context/gridspec.md`.

## Measured (gridspec-groups, 2026-10-14)

What to imitate per flavour is `book/src/gridspec/groups.md`, held by the
`groups` census; add `FakeSl` / `FakeOpensim` to its grids once the store
exists. The divergences that need a row in `imitates.rs`: founding cost (L$100
vs none) and the 35-character cut; OpenSim activating a group on founding and
joining; Second Life's empty UDP roster (the roster only over
`GroupMemberData`) and stale profile counts; the placeholder blocks of an
empty answer; notice ids (one per recipient on Second Life, the notice's on
OpenSim) and the requested-notice dialog 37; who is told of a role change;
the ejectee told only by message on Second Life; an invitation to a member
sent or not; bans over `GroupAPIv1` (Second Life only, `201` "Created" /
`200` "OK", `group_id` a string); the kept invitation over
`ReadOfflineMsgs` / `AcceptGroupInvite` / `DeclineGroupInvite`; accounts and
proposals answered (Second Life) or silent (OpenSim); and a deed of an
unshared object ignored (Second Life) or taken (OpenSim).

`FakeSl` grants `ReadOfflineMsgs` but not `AcceptGroupInvite` /
`DeclineGroupInvite`; the client drains over the capability only with both
answer capabilities granted ([[protocol-cap-read-offline-msgs]]), so the kept
invitation needs them too, and a record shaped as Second Life's (the inviter
as sender, the group in `asset_id`, a nil `transaction-id`).
