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
- the **channel** — channel `0` is what users see; scripts listen and speak on
  other integer channels, which is how in-world devices communicate.

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
via `Command::ImTyping`); incoming IMs arrive as
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

---

> **In this codebase**
>
> - Chat/IM types are in `sl-proto/src/types/chat.rs`: `ChatMessage`,
>   `ChatType`, `ChatAudible`, `ChatSourceType`, `InstantMessage`, `ImDialog`,
>   and `InventoryOffer` (decoded from the IM binary bucket).
> - Commands (`Chat`, `Typing`, `InstantMessage`, `ImTyping`,
>   `StartGroupSession`, `SendGroupMessage`, `LeaveGroupSession`,
>   `StartConference`, `SendConferenceMessage`, `LeaveConference`,
>   `AcceptInventoryOffer`, `DeclineInventoryOffer`) are in
>   `sl-proto/src/command.rs`.
> - Events (`ChatReceived`, `ChatTyping`, `InstantMessageReceived`, `ImTyping`,
>   `GroupSessionMessage`, `GroupSessionParticipant`,
>   `ConferenceSessionMessage`, `ConferenceSessionParticipant`,
>   `ConferenceInvited`) are in `sl-proto/src/types/event.rs`.
