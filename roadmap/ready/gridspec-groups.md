---
id: gridspec-groups
title: Groups: membership, roles, notices, invitations and accounting on each grid
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, viewer-social-group-extras,
  viewer-group-notice-attachments, gridspec-object-edit]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

OpenSim auto-activates at creation, sends `AgentDropGroup` on leave / eject,
has no accounting backend; SL caps names at 35, drops back-to-back creates,
charges L$100. The fake grid has no group store and `SimSession` no group
senders.

An object's group and a deed were measured with no group and with a plain
member ([[gridspec-object-edit]], `book/src/gridspec/building.md` § Editing
and § Somebody else's object): a set to a group the agent is not in does
nothing, a member's set is taken, and a deed without the power is refused —
with an alert on Second Life, with the unchanged record on OpenSim. Left
for this task: a deed by a member **with** the power (what the object's
record, owner and masks read afterwards, and who may edit it), which needs
a throwaway object and a group whose owner we are. The fake grid refuses
every deed and ignores every group set until it has groups to ask.

## Discover

`group_*` cases on aditi (pre-made fixture group) and OpenSim; role changes,
invites, bans, notices with attachments, ejection from the ejectee's side,
`GroupMemberData` cap vs UDP.

## Document

`book/src/gridspec/groups.md`.

## Fake grid

Large — [[protocol-sim-group-messages]] then [[server-fake-grid-groups]].

## Viewer

Ejectee list on SL (no `AgentDropGroup`); 35-character limit; finance tab
tolerates silence.

## Capabilities done in this task

[[protocol-cap-group-bans]]: group ban lists over `GroupAPIv1` (Second Life
only).

[[protocol-cap-offline-group-invite-answers]]: answering offline group
invitations over `AcceptGroupInvite` / `DeclineGroupInvite`, measured with
an invitation kept while the invitee is away.
