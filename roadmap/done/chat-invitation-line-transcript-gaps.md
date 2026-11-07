---
id: chat-invitation-line-transcript-gaps
title: A conference invitation's line is not transcribed, and a group one is under Do Not Disturb
topic: chat
status: done
origin: chat-group-history-server-side live verification (2026-11-07)
refs: [chat-group-history-server-side]
---

Context: [context/viewer.md](../context/viewer.md).

## Observation

Until an agent joins a chat session, the grid hands the session's lines over
as invitations (`Event::ConferenceInvited`), the first carrying the line that
opened it. The viewer shows that line. Since 2026-11-07
`ChatLog::observe_event` (`sl-client-common/src/chat_log.rs`) writes it to the
transcript, but only for a **group** session. Two cases are still wrong:

- **Ad-hoc conference**: the carried line is shown but not written, so the
  transcript and the next recall miss the conference's opening line. It was
  left out because the chat log does not know the viewer's ignore-ad-hoc
  setting (`auto_reject::ignores_ad_hoc`): a declined conference opens no tab,
  and its line must not be written either.
- **Do Not Disturb**: the reference drops a group invitation under Do Not
  Disturb before anything else (`LLViewerChatterBoxInvitation::post`), so it
  writes nothing, and the viewer drops it too. The chat log has no Do Not
  Disturb state, so it writes the line anyway.

## Fix direction

Both need a decision the viewer makes reaching the chat log: either the
runtime learns the two settings (presence and the ad-hoc policy, as it learns
the chat-log configuration through `SetChatLogConfig`), or the viewer tells
the runtime to write the carried line once it has decided to show it.

## Fixed (2026-11-07)

The second: one decision, made where the line is shown, rather than the
viewer's Do Not Disturb, blocking and ignore-ad-hoc rules copied into the
runtime.

- New `Command::TranscribeInvitationLine { session, from_agent_id,
  from_name, message }`, consumed by both runtimes' chat-log shells through
  `ChatLog::log_invitation_line` (a group to its `(group).txt`, a conference
  under its roster as the session knows it, as its later lines are). The
  REPL has it as `transcribe_invitation_line`.
- `ChatLog::observe_event` no longer writes an invitation's line on its own;
  it only takes the group's name from it.
- The viewer (`conversations.rs`) sends the command exactly where it pushes
  an invitation's line into a group or conference tab, so a line dropped
  under Do Not Disturb, from a blocked inviter, from our own invitation or by
  the ignore-ad-hoc policy is never written.
- Tests: the chat log writes a group and a conference invitation's line only
  when told; the viewer's invitation tests assert the command for a shown
  group and conference line and none for Do Not Disturb, a blocked inviter
  (the conference case shares the ignore-ad-hoc exit), our own invitation
  and a voice call. The aditi group-history stage re-passed on the new path.
