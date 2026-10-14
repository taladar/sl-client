---
id: protocol-cap-offline-group-invite-answers
title: Answer offline group invitations over AcceptGroupInvite / DeclineGroupInvite
topic: protocol
status: blocked
origin: gridspec-instant-messages follow-up (2026-10-14)
refs: [protocol-cap-offline-friendship-answers, viewer-offline-im-drain,
  protocol-cap-read-offline-msgs]
blocked_by: [gridspec-groups]
---

Context: [context/protocol.md](../context/protocol.md).

Second Life grants both (`book/src/gridspec/capabilities.md`, groups and
chat row). Firestorm answers a group invitation over them only when the
invitation came offline with a nil session id — i.e. through
`ReadOfflineMsgs` (`llimprocessing.cpp` sets the notification's
`use_offline_cap`, `llviewermessage.cpp` `send_join_group_response` and
`response_group_invitation_coro`): POST `{group: <uuid>}` to
`AcceptGroupInvite` or `DeclineGroupInvite`; without the capability it logs
that the answer cannot be sent. Every other invitation keeps the UDP
`ImprovedInstantMessage` answers (`GroupInvitationAccept` /
`GroupInvitationDecline`) with its transaction id.

Add the two commands to the client (both runtimes), answer a
capability-delivered invitation over them in the viewer, and record the
reply shape in `book/src/comms/caps-reference.md`.

Every capability adopted here goes into `REQUESTED_CAPABILITIES`
(`sl-proto/src/session.rs`), is used where the region grants it, keeps the
existing path where it does not, and is served by the fake grid per flavour as
`seed-capabilities` measured it (`book/src/gridspec/capabilities.md`). Shapes
and Firestorm references: `book/src/comms/caps-reference.md`.

## Done together with [[gridspec-groups]]

This capability is adopted inside that gridspec task, which measures
invitations on both grids, including one kept while the invitee is away and
read back over both roads, makes the fake grid serve the capability per
flavour, and checks the viewer. Claim that task.
