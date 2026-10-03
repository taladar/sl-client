---
id: gridspec-groups
title: Groups: membership, roles, notices, invitations and accounting on each grid
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, viewer-social-group-extras,
  viewer-group-notice-attachments]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

OpenSim auto-activates at creation, sends `AgentDropGroup` on leave / eject,
has no accounting backend; SL caps names at 35, drops back-to-back creates,
charges L$100. The fake grid has no group store and `SimSession` no group
senders.

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
