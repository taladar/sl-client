# Chat & Instant Messaging

Text communication comes in two broad forms: **local chat**, which is spatial
and public (anyone nearby hears it), and **instant messages**, which are
addressed to a specific avatar, group, or ad-hoc conference. They are different
mechanisms with different reach.

## Local chat

Local chat is heard by avatars within range in the same region. A chat message
carries:

- the **speaker** — name, source id, and the *source type* (a system message, an
  avatar, or a scripted object),
- a **chat type** that sets both intent and range: whisper, normal, shout, plus
  the typing-indicator pseudo-types (start/stop typing) and special channels
  (debug, region, owner, direct),
- an **audibility** level (fully / barely / not audible) derived from distance,
- the **channel** — a typed `ChatChannel(i32)`; channel `0` is what users see;
  scripts listen and speak on other integer channels, which is how in-world
  devices communicate.

Sending is `Command::Chat { message, chat_type, channel }`; a typing indicator
is `Command::Typing(bool)`. Incoming chat arrives as `Event::ChatReceived(..)`,
and others' typing as `Event::ChatTyping`.

## Instant messages

An instant message (IM) is point-to-point and works across regions and even to
offline avatars (stored and delivered later). The IM structure is heavily
overloaded: a **dialog** field selects what the IM actually *is*. The same
envelope carries:

- plain person-to-person messages,
- **inventory offers** (and their accept/decline replies) — the offered item is
  encoded in the IM's *binary bucket*,
- **teleport lures** (offer / accept / decline),
- **group invitations** and **group notices**,
- **friendship** offers and replies,
- **typing** start/stop,
- session control for group and conference chat (below).

Send a direct IM with `Command::InstantMessage { to_agent_id, message }` (typing
via `Command::ImTyping`); an **automatic** reply — the viewer's Do Not Disturb /
autorespond / away canned answer — goes out as `Command::AutoResponse` instead,
which sends the same envelope under the `DoNotDisturbAutoResponse` dialog so the
recipient can tell it from a typed message and never answers it in turn.
Incoming IMs arrive as
`Event::InstantMessageReceived(..)`, which the application dispatches on the
dialog type. Inventory offers carried in an IM are accepted or declined with
`Command::AcceptInventoryOffer` / `DeclineInventoryOffer` (see
[Inventory](inventory.md)).

## Group and conference sessions

Multi-party chat is modelled as a **session** keyed by an id (the group's id for
group chat, an ad-hoc id for a conference):

- **Group chat** — join the group's session and send to it
  (`Command::StartGroupSession`, `SendGroupMessage`, `LeaveGroupSession`); the
  message and roster events are `Event::GroupSessionMessage` and
  `GroupSessionParticipant`.
- **Conference (ad-hoc) chat** — start a conference with a set of avatars
  (`Command::StartConference`, `SendConferenceMessage`, `LeaveConference`);
  messages and roster are `Event::ConferenceSessionMessage` /
  `ConferenceSessionParticipant`, and being invited to one arrives as
  `Event::ConferenceInvited`.

Much of the multi-party machinery (invitations especially) is delivered through
the [event queue](../comms/caps.md#the-event-queue-eventqueueget) rather than
over UDP.

## The server side

`SimSession` keeps a real **chat-session registry** — the mirror of the
client's — as `SimChatSession` entries keyed by the wire session id: the kind
(`Group { group_id }` or `Conference`), the participant roster, and a capped
**server history** of relayed messages. That history is the backlog the
`ChatSessionRequest` capability's `fetch history` method serves; the cap
dispatch itself belongs to the CAPS surface, the state lives here.

Inbound, the session-dialog IMs decode into typed events instead of the
generic `ServerEvent::InstantMessage`: a group-session start
(`ServerEvent::GroupSessionStartRequested`) or conference start
(`ConferenceStartRequested`, its invitee list unpacked from the binary
bucket) creates the registry entry; a `SessionSend`
(`ServerEvent::SessionMessageSent`) appends to the session's history when the
session is known — a send into an *unknown* session surfaces the event but
creates no state, since the simulator is authoritative for membership and the
driver polices it; a `SessionLeave` (`SessionLeaveRequested`) drops the
sender from the roster, removing an emptied session.

Outbound, the driver relays with `send_session_message` and
`send_session_participant` (thin wrappers over the IM relay primitive that
also fold the local roster/history; their `from_group` flag selects whether
the client folds the traffic as group or conference chat), materialises a
session on a *peer's* sim with `open_chat_session` (the invitee's region
never sees the starter's conference-start IM), and delivers the invitation
itself over the event queue with `enqueue_chatterbox_invitation`.

---

> **In this codebase**
>
> - Chat/IM types are in `sl-proto/src/types/chat.rs`: `ChatMessage`,
>   `ChatType`, `ChatAudible`, `ChatSourceType`, `InstantMessage`, `ImDialog`,
>   and `InventoryOffer` (decoded from the IM binary bucket). A `ChatMessage`
>   carries its speaker as a typed `ChatSource` (system / agent / object /
>   unknown), folding the wire `SourceID` + `SourceType`; an `InventoryOffer`'s
>   `item_id` is a typed `InventoryItemOrFolderKey` (a folder offer is a folder
>   id, otherwise an item id).
> - Commands (`Chat`, `Typing`, `InstantMessage`, `AutoResponse`, `ImTyping`,
>   `StartGroupSession`, `SendGroupMessage`, `LeaveGroupSession`,
>   `StartConference`, `SendConferenceMessage`, `LeaveConference`,
>   `AcceptInventoryOffer`, `DeclineInventoryOffer`) are in
>   `sl-proto/src/command.rs`.
> - Events (`ChatReceived`, `ChatTyping`, `InstantMessageReceived`, `ImTyping`,
>   `GroupSessionMessage`, `GroupSessionParticipant`,
>   `ConferenceSessionMessage`, `ConferenceSessionParticipant`,
>   `ConferenceInvited`) are in `sl-proto/src/types/event.rs`.
> - Server side (`sl-proto/src/sim_session.rs`): `SimChatSession` /
>   `SimChatSessionKind`, the session-dialog decodes
>   (`GroupSessionStartRequested`, `ConferenceStartRequested`,
>   `SessionMessageSent`, `SessionLeaveRequested`), the relay helpers
>   `send_session_message` / `send_session_participant` /
>   `open_chat_session` / `enqueue_chatterbox_invitation`, and the
>   `chat_session` accessor. Loopback proofs are
>   `group_session_lifecycle_and_history_on_sim` and
>   `conference_relays_between_avatars` in `sl-proto/tests/sim_session.rs`.
