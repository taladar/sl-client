---
id: viewer-session-invitation-accepted-at-once
title: A group or conference invitation waits for Accept / Decline; the reference joins at once
topic: viewer
status: done
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

## Done

`ingest_conversation_events` (`sl-viewer-people/src/conversations.rs`) now
answers a text invitation itself: it shows the line the invitation carries in
the tab it opens and sends `Command::AcceptChatInvite` with no button,
as `LLViewerChatterBoxInvitation::post` does. The exits are the reference's:

- **Our own invitation** (OpenSim routes the line we opened a session with
  back through one): the tab opens, nothing is answered. We are in that
  session already, and the reference returns before it accepts.
- **A blocked resident's group session**: no tab, no answer either way, as
  before. The group is still the user's.
- **A blocked resident's ad-hoc conference, or one the ignore-conferences
  setting refuses**: declined, as before.

The Accept / Decline bar went with it: the `pending_invite` flag, the
`RespondToInvite` message, its system, the bar's nodes and colours, the three
`conversations-invite-*` Fluent strings, `ConversationReadout::pending_invite`
in `sl_automation_proto` and the "(invitation pending)" line `sl-viewer-ctl`
printed for it.

**A voice invitation is no longer treated as a text one.** It cannot be
accepted unseen: in the reference, accepting a voice invitation joins the
call. `Event::ConferenceInvited` now names the channel it invites to
(`channel: InviteChannel`, from the body's `instantmessage` / `voice`
sub-maps). The session layer had already classified this for its registry,
and now reads it off the event. `chatterbox_invitation_to_llsd` writes the
voice body back, a voice-only invitation with its session and inviter at the
top level. A voice-only invitation is left alone (logged at info), which is
what the reference does while voice is not working. This viewer has no voice
calls yet, so the incoming-call prompt belongs to [[viewer-voice-call-dialogs]],
which now notes it. An invitation to both channels is a text one first, as
the reference reads it. The `group-chat` conformance census records the channel
in each invitation's metric.

`book/src/gridspec/chat.md` § Group and conference sessions says what the
panel does now.

Found in the reference while doing this, filed separately: under Do Not
Disturb the reference neither shows nor accepts an invitation, and ours does
both ([[viewer-session-invitation-under-do-not-disturb]]).

## Verified

`cargo test --release -p sl-viewer-people --lib -- conversations`: 37 green.
The new tests cover a group invitation accepted at once with its line, a
conference one (to both channels) accepted as a conference, our own not
answered, a voice-only one opening no tab and sending nothing, a blocked
inviter's group left unanswered, and their conference declined.
`cargo test --release -p sl-proto --lib -- chatterbox`: the text round trip
and a new voice one (both channels, and voice-only with no `instantmessage`).
`cargo test --release -p sl-client-bevy-viewer --test e2e_two_avatars
a_conference_invitation`: the new fake-grid stage
`a_conference_invitation_is_joined_without_a_click`, with the grid playing a
host account. Its conference invitation shows "Join us?" in the viewer's
conference tab, the viewer's `ChatSessionRequest` accept puts it in the grid's
roster with no click, and it accepts exactly once.

Not run live. That `accept invitation` joins the session on Second Life, roster
and all, is the `group-chat` and `chat-invite-accept-decline` conformance
cases' ground. This change only decides when the viewer sends it.
