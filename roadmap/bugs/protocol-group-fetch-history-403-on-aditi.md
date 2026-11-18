---
id: protocol-group-fetch-history-403-on-aditi
title: A group session's automatic fetch history is answered 403 on aditi
topic: protocol
status: bugs
origin: protocol-voice-accept-on-direct-session-becomes-conference live run (2026-11-18)
refs: [chat-group-history-server-side]
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
