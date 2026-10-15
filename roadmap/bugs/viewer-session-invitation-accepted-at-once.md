---
id: viewer-session-invitation-accepted-at-once
title: A group or conference invitation waits for Accept / Decline; the reference joins at once
topic: viewer
status: bugs
origin: gridspec-group-chat-and-conference (2026-10-15)
refs: [gridspec-group-chat-and-conference, viewer-conference-start-ui]
---

Context: [context/viewer.md](../context/viewer.md).

## Observation

A `ChatterBoxInvitation` with an instant message opens our conversation tab as
a *pending invite* with an Accept / Decline bar
(`sl-viewer-people/src/conversations.rs`, `mark_invite`), and nothing is
answered until the user presses one. The reference shows the line and POSTs
`accept invitation` at once for a text invitation
(`LLViewerChatterBoxInvitation::post` → `chatterBoxInvitationCoro`,
`llimview.cpp:5028`, `:665`), unless the inviter is muted or ad-hoc sessions
are ignored.

## Effect

Measured on Second Life (`book/src/gridspec/chat.md` § Group and conference
sessions): until the invitee accepts, each line reaches it as a fresh
invitation, and its roster does not arrive. The line itself is shown since
the same task; the session is simply not joined.

## Fix

Accept a text invitation on arrival (`Command::AcceptChatInvite`), keeping the
ignore-ad-hoc and blocked-inviter exits, and retire the pending-invite bar and
its `pending_invite` readout field (`sl_automation_proto`), which then has no
source. Unit test through `ingest_app`.
