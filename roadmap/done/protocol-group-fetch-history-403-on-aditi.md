---
id: protocol-group-fetch-history-403-on-aditi
title: A group session's automatic fetch history is answered 403 on aditi
topic: protocol
status: done
origin: protocol-voice-accept-on-direct-session-becomes-conference live run (2026-11-18)
refs: [chat-group-history-server-side, test-chat-invite-accept-decline-aditi-primary-not-a-member]
---

Context: [context/protocol.md](../context/protocol.md).

## Observation

`sl-conformance run --grid aditi chat-invite-accept-decline` (2026-11-18,
passing) logged two `fetch history` POSTs for one **group** session, ten
seconds apart, each answered `403 Forbidden` "You cannot join chat":

```text
WARN sl_client_tokio::http: ChatSessionRequest refused the request
  session=Group { group_id: … } purpose=FetchHistory status=403 Forbidden
  body="You cannot join chat"
```

The gridspec measured that answer only for a **conference's** history
(`book/src/gridspec/chat.md`, Server history); a group session's history was
`200`. The requests are the automatic fetch a session makes once it is
joined (`next_server_history_fetches`), unchanged by the change the run
verified. The log does not say which avatar sent them.

## To do

- Find which avatar and which of the case's two groups (the accepted or the
  declined one) asked, and at what point of the case: an avatar that has
  just declined, or one whose join the grid has not finished, may be refused
  where a member in the session is not.
- If the fetch is premature, hold it until the grid has the agent in the
  session (the reference fetches from `LLIMModel::LLIMSession` once the
  session is initialised); if it is the declined group, do not fetch for it.
- Name the avatar in the runtimes' log line, which would have answered the
  first question here.

## Seen answered (2026-11-18)

A `sl-repl-tokio` probe of the secondary avatar joining the voice of one of
its groups (protocol-join-session-voice-posts-accept-invitation) logged that
group's automatic `fetch history` answered `200` with an empty array, so a
group member's fetch is not refused as such. Since that change a refused
fetch is handed back with its status and stores nothing (the session stays
`Requested`); what asked the second time in the original run is still open.

## Resolution (2026-11-19)

The refusals were the **primary's**, the group's founder, which opens the
session with `StartGroupSession`. The runtimes' refusal lines now name the
agent (`agent=` on both runtimes' `ChatSessionRequest` warnings), which
answered that at once. Live on aditi, with temporary logging: the
`StartGroupSession` opened the session as `Joined`, so the sweep posted
`fetch history` before the grid had answered; the `ChatterBoxSessionStartReply`
then came back `success=false error=unverified`, which drops the session; the
case's next retry sent into the group, which re-opened the session as a fresh
`Joined` one, and the sweep fetched again — ten seconds later, the case's
delivery window.

The reference fetches a session's backlog at two points only: when an inbound
message opens the session (`LLIMMgr::addMessage`), and when the start reply to
a session it started succeeds (`LLViewerChatterBoxSessionStartReply`). So a
session this client opens — by `start_group_session`, or by sending into a
group or conference with no session open — now starts in the new
`ServerHistoryState::AwaitingStart`. A successful group start reply makes the
fetch due, a refused one still drops the session, and a session opened only by
a send is never fetched. A session that inbound traffic opened first keeps its
fetch when the agent starts it afterwards. The declined group asked nothing
(declining removes the session), so nothing changed there.

Live on aditi after the change: the start was again refused `unverified`, and
no `fetch history` was posted at all, so no 403.

Tests: a started group session fetches only after a successful start reply,
and only once; a refused start followed by a send, and then by inbound
traffic, fetches nothing; starting a session inbound traffic already opened
keeps its fetch. `a_conference_we_started_fetches_no_backlog` now opens its
"someone else's" conference with an inbound message, as its comment said,
rather than with our own send.

Every aditi run of the case, including the one this was filed from, is
**partial**: the invitation never reaches the secondary. `unverified` is
Second Life's refusal of a non-member (`book/src/gridspec/chat.md`), so the
primary is apparently not a member of the group it starts. Filed as
[[test-chat-invite-accept-decline-aditi-primary-not-a-member]].
