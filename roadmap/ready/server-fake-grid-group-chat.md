---
id: server-fake-grid-group-chat
title: Fake grid — group chat sessions and conferences
topic: server
status: ready
origin: server-fake-grid-groups (2026-10-15)
refs: [server-fake-grid-groups, gridspec-group-chat-and-conference, server-fake-grid-im-relay]
blocked_by: [gridspec-group-chat-and-conference]
---

Context: [context/server.md](../context/server.md).

## What

The fake grid holds groups ([[server-fake-grid-groups]]) but no group chat:
a `SessionGroupStart` and the messages sent into a group session go nowhere.
Fan group session messages out to the members as each grid does, with the
history Second Life keeps and the membership propagation delay it shows.

## How

Implement what [[gridspec-group-chat-and-conference]] measures, per
`ImitatedGrid` flavour (a field of `GroupsPolicy` per divergence), and hold
it to the measurement with that task's conformance cases run against `FakeSl`
and `FakeOpensim`.

## Measured (gridspec-group-chat-and-conference, 2026-10-15)

What to imitate per flavour is `book/src/gridspec/chat.md` § Group and
conference sessions, held by the `group-chat` census; add `FakeSl` /
`FakeOpensim` to its grids (it needs a third fake avatar) once the sessions
exist. The divergences that need a `GroupsPolicy` field:

- **The line**: Second Life passes a session line on with `from_group`
  clear, no region, estate 0, the time and the group's name as the bucket;
  OpenSim with `from_group` set, a region, estate 1, the time, the group's
  name. Both send it back to its sender. Second Life refuses a line with an
  empty bucket (`ChatterBoxSessionEventReply` "message", error "generic").
- **Before joining**: the first line reaches a member who has not joined as
  a `ChatterBoxInvitation` carrying it (dialog 17); OpenSim adds a roster
  push entering the member.
- **The start**: a `ChatterBoxSessionStartReply` under the group's id (with
  voice on Second Life), and a roster push — Second Life's lists the online
  members and marks the owner moderator.
- **`accept invitation`**: Second Life answers `200` and an empty map and
  pushes the roster; OpenSim has no `ChatSessionRequest`.
- **A non-member**: Second Life refuses its start ("unverified") and its
  lines ("generic"); OpenSim takes both and passes the lines on.
- **Moderation** (Second Life): `mute update` answers `200` `{}`, pushes a
  roster entry with `mutes.text` and no transition to the moderator and the
  muted, and refuses the muted member's lines ("generic").
- **Leave / decline**: Second Life pushes `LEAVE`, stops sending the member
  lines and tells a lone speaker "You are the only participant in this IM
  session. Your message is not being broadcast at this time." (from nil,
  "Second Life"); OpenSim ignores both and keeps sending.
- **Away**: nothing is kept on either grid.
- **History** (Second Life only): `fetch history` answers an array of `{ from,
  from_id, message, num, time }`, the live session's lines in order (not a
  refused one, not an earlier session's); the census also records it empty
  after a burst — imitate that only once what empties it is known.

## Conferences (moved from server-fake-grid-im-relay, 2026-10-23)

The instant-message relay is in (`sl-fake-grid/src/messages.rs`); the
conferences that ride it were left for this task, which builds the session
machinery they share with group sessions. As measured by
gridspec-group-chat-and-conference (2026-10-15):

Conferences ride the same relay (`book/src/gridspec/chat.md` § A conference;
the `group-chat` census states both grids' answers). On Second Life's flavour:
`start conference` over `ChatSessionRequest` answered with a start reply
moving the client's id onto one the grid mints, named "*starter's name*
Conference", with voice, and a roster push; the invitees told nothing until
the first line, which reaches each one who has not accepted as a
`ChatterBoxInvitation` (dialog 17, `from_group` clear, the conference's name
as the bucket); after `accept invitation` (`200` `{}`) the lines go over UDP,
back to the sender too. A decline or a leave ends the lines to that avatar;
`invite` tells the invitee nothing until it accepts; the conference outlives
its starter, and a lone speaker is told it is the only participant. A
conference's `fetch history` answers `403` "You cannot join chat". On
OpenSim's flavour there are no conferences: nobody is invited, the start gets
no reply, and each line comes back to its sender alone, as a group line under
the viewer's id.

The `group-chat` census holds both; once it runs on the fake flavours, its
conference legs are held there too.
