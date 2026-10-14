---
id: gridspec-groups
title: Groups: membership, roles, notices, invitations and accounting on each grid
topic: gridspec
status: done
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

## Done (2026-10-14)

**Discover**: the census `groups` (`sl-conformance/src/cases/groups.rs`, 2av,
both grids) — founding, leaving and outsider reads, joining and both roster
roads, activation, roles and titles, notices (with a notecard, refused, asked
for again), accounts and proposals, ejection, invitations, bans over
`GroupAPIv1`, three invitations kept while B is away (answered over UDP, as
the client does through `AcceptGroupInvite`, and read over `ReadOfflineMsgs`
and declined through `DeclineGroupInvite`), and a deed by the owner. It
measures in the fixtures' `census_group` (founding one where none is named)
and leaves B a member of it. 145 answers held; two runs per grid agreed, and
a verification pass held both.

**Document**: `book/src/gridspec/groups.md`, linked from `content/groups.md`,
`gridspec/building.md`, `gridspec/chat.md` and `comms/caps-reference.md`.

**Fake grid**: large — [[protocol-sim-group-messages]] (now ready) then
[[server-fake-grid-groups]], which carries the measured list.

**Viewer**: fixed here — the Groups list asks for the memberships again each
time it comes into view
([[viewer-groups-list-keeps-a-group-the-agent-was-ejected-from]]); a
re-requested notice (dialog 37, new `ImDialog::GroupNoticeRequested`) fills the
notice panel without a toast; an empty roster, role list or notice list (one
block naming nobody) reads as empty. The founding dialog's 35-character cut and
L$100 and the money tab's OpenSim silence are notes on
[[viewer-social-group-extras]], which has neither yet.

**Capabilities**: [[protocol-cap-group-bans]] and
[[protocol-cap-offline-group-invite-answers]] done here.

### Differences from the plan

- A deed by a member *with* the power is measured with the owner, not with
  a member granted the power; a deed of an object not shared with the group
  turned out to be its own row (Second Life ignores it).
- Ejection "from the ejectee's side" found nothing on Second Life, so the
  viewer refreshes on view rather than on an ejection.
- Not measured: group chat (its own task), a join fee or closed group, an
  invitation to another role, accepting a notice attachment, a ban of a
  member, group land and money.
