---
id: gridspec-group-chat-and-conference
title: Group chat sessions and ad-hoc conferences on each grid
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, chat-group-history-server-side,
  viewer-group-session-moderation,
  test-conference-roster,
  viewer-conference-start-ui, protocol-agent-list-voice-transition-lossy,
  viewer-session-invitation-accepted-at-once, server-fake-grid-group-chat]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

OpenSim has no `ChatSessionRequest` cap and no ad-hoc conferences, delivers
group messages inline to non-joined members; SL drops messages into a fresh
group until membership propagates, keeps server history, re-keys conferences.

## Discover

The group-session cases with a propagated pre-made group on aditi;
`test-conference-roster` (3 avatars); moderation; agent-list transitions;
history size.

## Document

`book/src/gridspec/chat.md` § Group and conference sessions.

## Fake grid

Large — [[server-fake-grid-groups]] (sessions) and [[server-fake-grid-im-relay]]
(conferences).

## Viewer

History fetch degrades on OpenSim; inline delivery without invitation; the SL
re-key.

## Done (2026-10-15)

**Discover**: the census `group-chat` (`sl-conformance/src/cases/group_chat.rs`,
3av, both grids) — a group session opened, spoken into before and after B
joins, by C who is no member, with an empty bucket; moderation over `mute
update`; leaving, declining and opening again; the server history read over
the capability and by the client, again, and after bursts of 10, 20 and 30
lines; B away and back; and a conference started, spoken into, declined,
re-invited, left by an invitee and by its starter. It measures in the
fixtures' `census_group`. 132 answers held; five runs a grid, a
verification pass held both.

**Document**: `book/src/gridspec/chat.md` § Group and conference sessions,
linked from `content/chat.md` and `gridspec/groups.md`.

**Fake grid**: large — [[server-fake-grid-group-chat]] (now ready, carrying
the measured list) for group sessions and [[server-fake-grid-im-relay]] for
conferences.

**Viewer / client**, fixed here:

- **Second Life refused every session line we sent**: the bucket was empty,
  where the reference packs one zero byte (`EMPTY_BINARY_BUCKET`); the grid
  answers such a line with a `ChatterBoxSessionEventReply` "generic". Every
  session line, start and leave now carries the byte. Before this our client
  could not speak in a group or a conference on Second Life.
- **Second Life's group chat arrived as a conference**: it sends group lines
  and invitations with `from_group` clear. `Session` keeps the agent's groups
  and reads session traffic under a group's id as the group's.
- A group session's start reply opened a phantom conference of the group's
  id; a refused start reply (no `session_id`) failed to decode.
- New events: `ChatSessionAgentsUpdated` (the roster push, folded into the
  participants: `ENTER` adds, `LEAVE` drops — the reference's speaker list),
  `ChatSessionEventReply`, `ChatSessionForceClosed`. The session message
  events carry the whole `InstantMessage`.
- The conversations panel shows the line an invitation carries.
- A failing `ChatSessionRequest` POST logs its status and body.

**Capabilities**: none new; `ChatSessionRequest` was already requested.

### Differences from the plan

- The agent-list encoder bug [[protocol-agent-list-voice-transition-lossy]]
  was fixed here (the census needed the decoder that keeps transition and
  voice apart; the encoder followed).
- [[test-conference-roster]] is covered by the census and closed with it.
- [[viewer-conference-start-ui]]'s outstanding live check (a conference line
  round-tripping between three avatars under the grid's id) is answered by
  the census, and it is closed.
- History size is not pinned: the history read empty after only ten more
  lines and stayed empty; what empties it is listed as not measured.
- The reference accepts a text invitation at once; ours waits for Accept /
  Decline — filed as [[viewer-session-invitation-accepted-at-once]].
- Not measured: typing and voice in a session, a role without chat, a
  session across regions, `ForceCloseChatterBoxSession`.
