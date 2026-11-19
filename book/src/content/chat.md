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
and others' typing as `Event::ChatTyping`, which states how audible the grid
says the typist is and where.

Both are read on a child circuit as well as the root one: OpenSim sends a
line said just over a border down the child circuit of the speaker's region.
What each grid passes on, how far, and what it cuts — and that neither ever
marks a *line* less than fully audible, while Second Life marks a distant
typist's notices so — is measured in
[Grid Behaviour: Local chat](../gridspec/chat.md).

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

Every IM states a `Position` for its sender, and the session fills it as the
reference viewer does: the agent's own region-local position (its avatar
object, composed with the seat it sits on, or where the last
`AgentMovementComplete` placed it before that object arrives) for offers and
their answers, a teleport request, a friendship offer, a group notice or
invitation answer and a session start, and zero for the conversational
dialogs — an ordinary message, a session's messages, typing, leaving a session
and the automatic reply. OpenSim passes a request's position on as sent;
Second Life states none.

Messages sent while the avatar was away are kept by the grid until asked
for: `Command::RetrieveInstantMessages` on both grids, or
`Command::RequestOfflineMessages`, which reads Second Life's
`ReadOfflineMsgs` capability where the region grants it with
`AcceptFriendship` and `AcceptGroupInvite` (`OFFLINE_DRAIN_CAPABILITIES`),
and falls back to UDP otherwise and when the GET fails, as the reference
does. They arrive as ordinary `Event::InstantMessageReceived` with
`offline` set; a record the capability hands over is read as the same
message over UDP (the group of an invitation or a notice, an inventory
offer's transaction id, all from its `asset_id`), and
`InstantMessage::answered_over_capabilities` says which offers are to be
answered with `use_offline_cap`. What
each grid passes on, rewrites, keeps and tells the sender is measured in
[Grid Behaviour: Instant messages](../gridspec/chat.md#instant-messages).

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

Starting a conference has **two wire paths**, and the runtimes pick by
capability presence: where the region publishes `ChatSessionRequest` they POST
its `start conference` method (`{ method, session-id, params: [<invitees>] }`,
the reference's `startConferenceCoro`); on a grid without the cap — OpenSim —
they send the deprecated `IM_SESSION_CONFERENCE_START` instant message with the
invitees packed into its binary bucket (`Session::start_conference`). Either way
the session opens locally under the id the *client* minted.

Adding people to a conference that is already open is a *different* request:
`Command::InviteToChatSession`, the cap's `invite` method (the reference's
`LLFloaterIMSession::inviteToSession`). It names a real session, so no start
reply follows; without the cap it falls back to re-sending the conference-start
IM, the only invite the legacy path has.

That id is **temporary**. The grid answers over the event queue with a
`ChatterBoxSessionStartReply` — surfaced as `Event::ChatSessionStarted`, and on
Second Life it names a *different* session id, which is the one every later
message, invitation and roster update uses. `Session` re-keys its own registry
entry onto the real id before the event is surfaced (merging into an entry the
invitation may already have created), so a driver only has to move what *it*
keyed by the temporary id — a conversation tab, say. A reply with
`success: false` drops the session instead.

Much of the multi-party machinery (invitations especially) is delivered through
the [event queue](../comms/caps.md#the-event-queue-eventqueueget) rather than
over UDP: the roster as `Event::ChatSessionAgentsUpdated` (folded into the
session's participants), a refused line or start as
`Event::ChatSessionEventReply`, a session the grid closes as
`Event::ChatSessionForceClosed`.

Second Life sends group chat — the lines and the invitations — with
`from_group` **clear**, so `Session` reads a session line as a group's by its
id: one of the groups the agent is in (it keeps the list from
`AgentGroupDataUpdate`, joins, leaves and drops), or a group session that is
open. And Second Life refuses a session line with an empty bucket; every
session line goes out with the reference's one zero byte. What each grid does
with a session is measured in [Grid Behaviour: Group and conference
sessions](../gridspec/chat.md#group-and-conference-sessions).

An invitation names the session it is to as a `ChatSessionKind` — the
`session` of `Event::ConferenceInvited` — and is answered with that kind
(`Command::AcceptChatInvite` / `DeclineChatInvite { session }`). A voice
invitation whose `voice.invitation_type` is 2, the reference's
`P2P_CHAT_SESSION`, is a **1:1 call** with the inviter, so its kind is the
`Direct` session with that avatar rather than a conference under the call's
id; otherwise a `from_group` invitation is a group's and anything else a
conference (still read as a group's by its id, as above). A group or a
conference is accepted with the cap's `accept invitation` and declined with
`decline invitation`, or left with a UDP `SessionLeave` where there is no cap;
a 1:1 call is accepted with **no POST**, as the reference accepts one, and
declined with `decline p2p voice`. Declining a call leaves a conversation
already open with the caller in place.

Joining a session's voice (`Command::JoinSessionVoice`) asks the cap for the
session's **channel** only when the session does not have one, as the
reference does: an invitation that carried a channel needs no request. A group
or a conference asks with `call`, naming the backend in
`alt_params.preferred_voice_server_type`, and the reply's `voice_credentials`
become the session's channel. A 1:1 call is run the way the reference runs one
under WebRTC, which relays no call itself: as a two-person ad-hoc channel,
started with `start p2p voice` (the other agent as a single uuid in `params`,
the backend in `alt_params.voice_server_type`), whose
`ChatterBoxSessionStartReply` comes under the 1:1 session's own id and makes
the `call` due. `Session` records what is due and the runtimes POST it from
`Session::next_session_voice_requests`; a `call` answered `403`, any other
refusal, a `call` with no channel in it or a refused start ends the join with
`Event::SessionVoiceFailed`, clearing the optimistic `voice.joined`. Leaving a
1:1 call forgets its channel, so calling again rings again; a group keeps its
channel. On OpenSim there is no cap, and nothing is asked.

A join provisions **no voice account**: the reference, under WebRTC, connects
to a session's channel only once it has one, with a `multiagent`
`ProvisionVoiceAccountRequest` carrying its JSEP offer and the channel's id
(`channel`) and `credentials` (`LLVoiceWebRTCAdHocConnection`). The offer is
the WebRTC engine's, so `Session` reports the channel as
`Event::SessionVoiceChannelReady { session, channel }` — at the join when the
session already knows it, else when a `call` or an accept brings it — once per
join and again only when the channel changes, and never for a channel with no
id or a session whose voice is not joined. The engine answers with
`Command::RequestVoiceAccount` and a `VoiceProvisionRequest::webrtc_channel`.

The cap's replies — an accept's roster and `voice_channel_info`, a
`fetch history`'s backlog, a `call`'s channel — name no session of their own:
the reference keeps the session id in the coroutine that posted. So the
runtimes carry the kind from the request to its reply as a typed
`ChatSessionReply { session, purpose, status, body }` on a channel of its own,
handed to `Session::handle_chat_session_reply`, rather than writing it into
the reply body. A refusal is handed back too, with its status (and an
undefined body when it was not LLSD); a refused accept folds no roster and a
refused `fetch history` stores nothing and is not asked again.

The automatic `fetch history` (`Session::next_server_history_fetches`) asks
only for a session the grid has put the agent in, at the two points the
reference does: a group or a conference that an inbound message or an
accepted invitation opens, and a group session the agent started once its
`ChatterBoxSessionStartReply` succeeds. Asked any earlier, Second Life
answers `403` "You cannot join chat". A session the agent opened by sending
into it with no start is never fetched, nor is a conference it started
(nothing was said in it before), and a refused start (`unverified` for a
non-member) drops the session, so a later send re-opens it without a fetch.

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

The modern conference start arrives over the cap instead
(`ChatSessionRequest`'s `start conference`), and lands on the same
`ConferenceStartRequested` event — one registry, two doors.

Outbound, the driver relays with `send_session_message` and
`send_session_participant` (thin wrappers over the IM relay primitive that
also fold the local roster/history; their `from_group` flag selects whether
the client folds the traffic as group or conference chat), materialises a
session on a *peer's* sim with `open_chat_session` (the invitee's region
never sees the starter's conference-start IM), and delivers the invitation
itself over the event queue with `enqueue_chatterbox_invitation`. A simulator
that mints its own session id for a conference tells the starter so with
`enqueue_chatterbox_session_start_reply`, the inverse of the client's
`Event::ChatSessionStarted`.

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
>   `AcceptChatInvite`, `DeclineChatInvite`, `AcceptInventoryOffer`,
>   `DeclineInventoryOffer`) are in
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
