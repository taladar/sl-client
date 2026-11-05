---
id: viewer-session-invitation-under-do-not-disturb
title: Under Do Not Disturb a group or conference invitation is shown and joined; the reference ignores it
topic: viewer
status: done
origin: viewer-session-invitation-accepted-at-once (2026-11-05)
refs: [viewer-session-invitation-accepted-at-once, gridspec-group-chat-and-conference]
---

Context: [context/viewer.md](../context/viewer.md).

## Observation

`LLViewerChatterBoxInvitation::post` (`llimview.cpp`) returns first thing for
a text invitation when `gAgent.isDoNotDisturb()`. It shows no line, opens no
session and sends no `accept invitation`. Our conversations panel
(`sl-viewer-people/src/conversations.rs`, the `ConferenceInvited` arm of
`ingest_conversation_events`) does not look at Do Not Disturb, so it shows the
line and accepts.

## Effect

Under Do Not Disturb the user is drawn into every group chat that starts, and
the reference keeps them out. On Second Life an unaccepted session hands each
further line over as a fresh invitation (`gridspec/chat.md`), so the
reference's DnD user sees none of that group's lines until DnD is off and
someone speaks again.

## Wanted

Decide whether to match the reference here (drop the invitation under DnD,
with or without a line in the tab), and do it with a unit test through
`ingest_app`. The offers in `offers_invites.rs` already read the DnD state
(`judge_calling_card`), so the fact is available.

## Done (2026-11-05)

Matched the reference: under Do Not Disturb the `ConferenceInvited` arm of
`ingest_conversation_events` drops a text (or text-and-voice) invitation
before anything else. It shows no line, opens no tab and sends neither
accept nor decline, whoever sent it. It reads the presence's Do Not Disturb
through `ConversationFacts`. A voice-only invitation keeps its own earlier
exit. Second Life hands the session's next line to an unaccepted participant
as a fresh invitation, so the first line after Do Not Disturb ends joins the
session, as in the reference. `gridspec/chat.md` says so.

## Verified

The unit test `an_invitation_under_do_not_disturb_is_dropped` (through
`ingest_app`) covers a group invitation and a conference invitation under Do
Not Disturb: neither is answered and neither opens a tab. Once Do Not Disturb
is off, the next invitation is accepted and only its line shows.
