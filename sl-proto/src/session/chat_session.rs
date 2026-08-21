//! The chat-session registry's typed discriminator and per-session value.
//!
//! All IM traffic — 1:1 direct, group, and ad-hoc conference — rides one wire
//! message (`ImprovedInstantMessage`); [`ChatSessionKind`] names which of the
//! three kinds a session is and carries that kind's *typed* canonical id (never a
//! raw `Uuid`), so it doubles as the key of the
//! [`Session::chat_sessions`](crate::Session) registry. The simulator stays
//! authoritative throughout; this registry is an API-convenience read model that
//! mirrors what the IM wire reports and never routes or gates traffic.

use super::conversions::compute_im_session_id;
use crate::bookkeeping_ids::ImSessionId;
use crate::types::{Friend, ImDialog};
use sl_types::key::{AgentKey, GroupKey};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::time::{Duration, Instant};
use uuid::Uuid;

/// The most recent messages retained per chat session. Older messages are
/// dropped front-first once the log exceeds this many entries — the log is an
/// in-memory display convenience, not the durable store (that is the optional
/// on-disk chat log), so a fixed bound keeps a busy session from growing without
/// limit. Matches the order of magnitude viewers keep in a conversation pane.
pub(crate) const HISTORY_CAP: usize = 256;

/// How long a remote "X is typing…" entry survives without a refresh before it
/// is pruned (see [`ChatSession::typing`]). A lost `TypingStop` (packet loss, a
/// crashed peer) would otherwise strand the indicator forever; senders re-emit a
/// typing-start every ~4 s, so this tolerates a couple of missed refreshes. The
/// value matches Firestorm's `OTHER_TYPING_TIMEOUT` (`fsfloaterim.cpp:88`).
pub(crate) const TYPING_TIMEOUT: Duration = Duration::from_secs(9);

/// The most server-history messages retained per session (see
/// [`ChatSession::server_history`]). The server already bounds a
/// `fetch history` reply to its own recent window; this cap only guards against
/// a pathological oversized reply, keeping the **newest** entries when it
/// trims. Deliberately the same order of magnitude as [`HISTORY_CAP`].
pub(crate) const SERVER_HISTORY_CAP: usize = 256;

/// The timestamp tolerance when de-duplicating a fetched server-history entry
/// against the live ring: two messages with equal sender and text are the same
/// message when their Unix timestamps differ by at most this many seconds (or
/// when either side carries no timestamp at all — live lines log
/// `timestamp: None`). Firestorm's merge effectively compares at datetime
/// granularity (`llimview.cpp:1385`), so one minute of slack is faithful.
pub(crate) const SERVER_HISTORY_TIMESTAMP_SLACK_SECONDS: u32 = 60;

/// Which of the three IM-session kinds a chat session is, carrying that kind's
/// *typed* canonical id. This is the key of the chat-session registry: the enum
/// discriminant keeps the three id spaces disjoint, so a group id never aliases a
/// conference id or a 1:1 XOR id in the map (the bug a bare-`Uuid` key would
/// have).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum ChatSessionKind {
    /// A 1:1 instant-message conversation, keyed by the **peer** avatar (the
    /// human-meaningful, stable identity — a conversation is "with this avatar").
    /// The wire-correlation session id is the byte-wise XOR of the two agent ids,
    /// derivable on demand via [`ChatSessionKind::canonical_session_id`].
    Direct {
        /// The other avatar in the conversation.
        peer: AgentKey,
    },
    /// A group IM session, keyed by the group id (which *is* the session id on
    /// the wire).
    Group {
        /// The group whose IM session this is.
        group_id: GroupKey,
    },
    /// An ad-hoc conference / multi-party IM session, keyed by the caller-minted
    /// conference id.
    Conference {
        /// The conference session id.
        id: ImSessionId,
    },
}

impl ChatSessionKind {
    /// The canonical IM session id this kind uses on the wire: for a group the
    /// group id, for a conference the minted conference id, and for a 1:1 the
    /// viewer's XOR of the two agent ids (which also handles the self-IM special
    /// case). `own_agent` is this session's own avatar id, needed only for the
    /// `Direct` XOR.
    #[must_use]
    pub fn canonical_session_id(self, own_agent: AgentKey) -> Uuid {
        match self {
            Self::Direct { peer } => compute_im_session_id(own_agent, peer),
            Self::Group { group_id } => group_id.uuid(),
            Self::Conference { id } => id.get(),
        }
    }
}

/// Which channel(s) a chat-session invitation offers. A group or conference can
/// expose both a text channel and a voice channel under one session id, so the
/// two are tracked together rather than as separate sessions: a text-only invite
/// is [`Text`](Self::Text), a voice-call invite is [`Voice`](Self::Voice), and an
/// invite to both is [`Both`](Self::Both). Classified from the `ChatterBoxInvitation`
/// body — an `instantmessage` sub-map is a text invite, a `voice` sub-map a voice
/// invite (Firestorm `llimview.cpp:5047`/`:5196`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum InviteChannel {
    /// A text-session invite (the viewer auto-joins these).
    Text,
    /// A voice-call invite (the viewer prompts the user).
    Voice,
    /// An invite to both the text and the voice channel of one session.
    Both,
}

/// The payload an [`Invited`](ChatSessionLifecycle::Invited) chat session carries:
/// who invited us, the session's display name, and which channel(s) the invite is
/// for. There is no separate pending-invitation registry — a pending invitation is
/// exactly a chat-session entry whose lifecycle is `Invited`, so the registry is
/// self-describing.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PendingInvite {
    /// The inviting agent (the `ConferenceInvited.from_agent_id`).
    pub inviter: AgentKey,
    /// The session's human-readable name (the group or conference name).
    pub session_name: String,
    /// Which channel(s) the invitation is to.
    pub channel: InviteChannel,
}

/// Whether a chat session is a still-pending invitation or one we have joined.
/// Born here with its only constructor (the invite path sets
/// [`Invited`](Self::Invited)) and the promotion rule (any session message or
/// participant traffic, and an explicit accept, set [`Joined`](Self::Joined)), so
/// the `Invited` variant is never a dead, never-constructed state.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ChatSessionLifecycle {
    /// A session we were invited to but have not yet joined; carries the invite.
    Invited(PendingInvite),
    /// A session we are in — opened by our own send, by inbound traffic, or by an
    /// explicit accept. A 1:1 direct session is always `Joined`.
    Joined,
}

/// One logged conversation message in a chat session's history — a 1:1 IM, a
/// group-session message, or a conference message, plus our own outbound sends.
/// Read back via [`Session::history`](crate::Session::history). Distinct from the
/// nearby-chat [`ChatMessage`](crate::ChatMessage) (region-local spoken chat);
/// this is the IM/session conversation log entry.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SessionMessage {
    /// Who sent the message: the remote avatar for inbound traffic, or our own
    /// agent for messages we sent.
    pub sender: AgentKey,
    /// The IM dialog the message arrived on (`Message` for a 1:1, `SessionSend`
    /// for a group / conference message).
    pub dialog: ImDialog,
    /// The message text (trailing NUL padding already stripped).
    pub text: String,
    /// The sender's wire Unix timestamp, when the simulator supplied one (notably
    /// for replayed offline IMs). `None` for our own sends and for live messages
    /// that carry no timestamp — the sans-IO layer has no wall-clock of its own,
    /// so insertion order is the authoritative sequence.
    pub timestamp: Option<u32>,
}

/// One message of the **server-side recent-message backlog** of a group /
/// conference session, fetched via the `ChatSessionRequest` capability's
/// `fetch history` method and carried by
/// [`Event::SessionServerHistory`](crate::Event::SessionServerHistory).
/// Distinct from the live-ring [`SessionMessage`]: the backlog record carries
/// the sender's **display name** as the server rendered it (a consumer showing
/// history from before it was listening has no roster to resolve the key
/// against), and it is what was said *before* this client joined — it is never
/// written to the on-disk transcript (transcript = what this client heard
/// live).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ServerHistoryMessage {
    /// Who sent the message (the record's `from_id`).
    pub sender: AgentKey,
    /// The sender's display name as the server rendered it (the record's
    /// `from`).
    pub sender_name: String,
    /// The message text (the record's `message`).
    pub text: String,
    /// The message's Unix timestamp (the record's `time`, which Second Life
    /// sends as an integer or a real), or `None` when absent/zero.
    pub timestamp: Option<u32>,
}

/// Where a session stands in the once-per-login server-history fetch cycle —
/// the state the [`Session::next_server_history_fetches`](crate::Session::next_server_history_fetches)
/// scheduler flips so each group / conference session is fetched exactly once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ServerHistoryState {
    /// No fetch has been issued for this session yet; the scheduler will
    /// return it once the session is joined (and the cap is known).
    Unfetched,
    /// A fetch was handed to the runtime. A reply flips this to
    /// [`Fetched`](Self::Fetched); a failed POST deliberately leaves it here —
    /// the backlog is a convenience, so there is no retry storm.
    Requested,
    /// A reply was received and folded into
    /// [`ChatSession::server_history`].
    Fetched,
}

/// Whether the automatic server-side chat-backlog fetch is enabled — the
/// `Session`-level gate behind
/// [`Session::set_fetch_server_chat_history`](crate::Session::set_fetch_server_chat_history).
/// A two-variant enum rather than a bare bool so the `Session` struct stays
/// within the workspace's `struct_excessive_bools` budget; the public setter /
/// getter API stays `bool`-shaped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ServerHistoryFetch {
    /// Newly joined group / conference sessions are swept by
    /// [`Session::next_server_history_fetches`](crate::Session::next_server_history_fetches)
    /// (the default, matching the reference `FetchGroupChatHistory`).
    Enabled,
    /// The sweep returns nothing; only the explicit
    /// [`Command::FetchSessionHistory`](crate::Command::FetchSessionHistory)
    /// fetches.
    Disabled,
}

impl ServerHistoryFetch {
    /// The enum for a bool-shaped setter argument.
    pub(crate) const fn from_enabled(enabled: bool) -> Self {
        if enabled {
            Self::Enabled
        } else {
            Self::Disabled
        }
    }

    /// Whether the auto-fetch is enabled (the bool-shaped getter view).
    pub(crate) const fn is_enabled(self) -> bool {
        matches!(self, Self::Enabled)
    }
}

/// One recalled line of **nearby (local) chat** history, read back from the
/// on-disk transcript in reply to
/// [`Command::QueryNearbyChatHistoryPage`](crate::Command::QueryNearbyChatHistoryPage).
///
/// Nearby chat is deliberately **not** a [`ChatSessionKind`] — it has no
/// participant roster, no session id, and no in-memory ring in the sans-IO
/// [`Session`](crate::Session); it is only spoken live (surfaced as
/// [`ChatMessage`](crate::ChatMessage)) and appended to a flat transcript file.
/// So its recall is a separate, simpler value type than the keyed
/// [`SessionMessage`]: the transcript stores the speaker's **display name** (a
/// string, not a resolvable key — most nearby speakers are avatars/objects we
/// hold no key for), so that is what a recalled line carries.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NearbyHistoryLine {
    /// The speaker's display name as written in the transcript, or `None` for a
    /// plain-text fallback line that carried no recognisable `Name:` separator.
    pub speaker: Option<String>,
    /// The message text, with any folded multi-line continuations rejoined.
    pub text: String,
    /// The line's local wall-clock as a Unix timestamp, recovered from the
    /// transcript's `[…]` prefix when it carried (and we could parse) one.
    pub timestamp: Option<u32>,
}

/// The coordinates of a chat session's voice channel — the SL `voice_channel_info`
/// block carried by a voice invitation and the `ChatSessionRequest "accept
/// invitation"` reply. A small **client-local** struct (not a reuse of
/// [`sl_wire::ParcelVoiceInfo`], whose `parcel_local_id` / `region_name` are
/// spatial-voice-only): it mirrors only the per-session room's connection
/// coordinates. All fields are optional, so it [`Default`]s to an empty,
/// no-coordinates channel. Signalling only — never the audio stream.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct VoiceChannelInfo {
    /// The voice room to connect to (`sip:…` for Vivox, the session's channel
    /// id for WebRTC — see [`VoiceChannelUri`](sl_wire::VoiceChannelUri)), or
    /// `None` when the grid carried an empty/absent uri.
    pub channel_uri: Option<sl_wire::VoiceChannelUri>,
    /// Optional per-channel credentials (a token the voice client presents when
    /// connecting; rarely sent — OpenSim leaves it unset).
    pub channel_credentials: Option<String>,
    /// The backend the channel uses (`"vivox"` | `"webrtc"`), when the grid
    /// echoes it.
    pub voice_server_type: Option<String>,
    /// The SL voice session handle the signalling correlates on, when present.
    pub session_handle: Option<String>,
}

/// The per-session **voice** facet — at the SL *signalling* level only. A group,
/// conference, or 1:1 session can carry a voice channel beside its text channel;
/// this records *that the session offers voice*, the channel coordinates, whether
/// we have joined (optimistically, at the signalling level), and who is voice-
/// connected. It **never** models the audio stream nor the talk-activity /
/// "who is speaking" state (the standing project rule — that lives in an external
/// voice client). [`Default`]s to an empty, no-voice facet (all `false` / `None` /
/// empty), so a freshly opened chat session starts without voice.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct VoiceChannelState {
    /// Whether this session offers a voice channel at all (set from a voice
    /// invitation or an accept reply that carried channel coordinates).
    pub has_voice: bool,
    /// The channel coordinates, once known (from the invite's `voice` body or the
    /// accept reply's `voice_channel_info`).
    pub channel: Option<VoiceChannelInfo>,
    /// Whether *we* have joined the voice channel, tracked **optimistically** at
    /// the signalling level (set by [`Session::join_session_voice`](crate::Session::join_session_voice)
    /// / a voice-accept, cleared by [`Session::leave_session_voice`](crate::Session::leave_session_voice)
    /// / a voice-decline). There is no audio ack — this is the request state, not
    /// a confirmed media connection.
    pub joined: bool,
    /// The voice-connected subset of the text roster — who is currently in voice,
    /// folded from the `ChatterBoxSessionAgentListUpdates` agent-list voice flag.
    /// Strictly a membership set, **never** the speaking / talk-activity state.
    pub members: BTreeSet<AgentKey>,
}

/// The mutable per-session state mirror — the value half of the chat-session
/// registry (the kind/id lives in the [`ChatSessionKind`] key). It grows
/// additively as later chat tasks land their facets (participants/typing,
/// history/unread, lifecycle, voice-channel state); for now it carries the
/// activity stamp, the roster, the typing set, the message log, the lifecycle,
/// and the voice-channel facet.
///
/// No `Default`: [`Instant`] has none, so the value is built by
/// [`ChatSession::new`].
#[derive(Debug)]
pub(crate) struct ChatSession {
    /// Monotonic time of the last message / typing / roster change in this
    /// session (the crate's sans-IO clock). Drives display ordering and any
    /// future idle handling; it **never** drives presence (presence comes only
    /// from the authoritative friend notifications).
    pub(crate) last_activity: Instant,
    /// The session roster: who the simulator reports is in this group /
    /// conference (it **includes self** once we have joined). Folded from the
    /// `SessionAdd` / `SessionLeave` participant events. A 1:1 `Direct` session
    /// never materialises a roster — its participants are implicitly
    /// `{ self, peer }` and the accessor synthesises `{ peer }` from the key —
    /// so this set stays empty for `Direct`.
    pub(crate) participants: BTreeSet<AgentKey>,
    /// Remote typers in this session, each mapped to the monotonic time its last
    /// typing-start was seen. Holds **other** avatars only (never our own
    /// outbound typing). Entries older than [`TYPING_TIMEOUT`] are pruned on the
    /// timed loop so a lost `TypingStop` cannot strand the indicator; an explicit
    /// `TypingStop` removes immediately.
    pub(crate) typing: BTreeMap<AgentKey, Instant>,
    /// The bounded conversation log, oldest-first. Capped at [`HISTORY_CAP`]
    /// entries; the oldest is dropped front-first once the cap is exceeded. Holds
    /// only conversation messages (inbound 1:1 / group / conference and our own
    /// outbound sends) — typing, participant, offer, and notice dialogs are not
    /// logged.
    pub(crate) history: VecDeque<SessionMessage>,
    /// The number of inbound messages received since the session was last read.
    /// Bumped per inbound message from another agent; reset to zero by our own
    /// outbound send and by [`Session::mark_session_read`](crate::Session::mark_session_read).
    pub(crate) unread: u32,
    /// Whether this session is a still-pending invitation or one we have joined.
    /// A session opened by traffic (the `chat_session_mut` lazy-open) or an
    /// explicit accept is [`Joined`](ChatSessionLifecycle::Joined); only the
    /// invite path sets [`Invited`](ChatSessionLifecycle::Invited).
    pub(crate) lifecycle: ChatSessionLifecycle,
    /// The per-session voice-channel facet (signalling only): whether the session
    /// offers voice, its channel coordinates, whether we have joined, and who is
    /// voice-connected. Empty / no-voice until a voice invite, accept reply, or
    /// join sets it. Persists across teleport with the rest of the session and is
    /// folded by the presence-driven reset (an offlined friend leaves `members`).
    pub(crate) voice: VoiceChannelState,
    /// The server-side recent-message backlog (`fetch history`), oldest-first,
    /// already de-duplicated against the live [`history`](Self::history) ring —
    /// **separate** from that ring so its unread / [`HISTORY_CAP`] semantics
    /// stay untouched. Capped at [`SERVER_HISTORY_CAP`] keeping the newest;
    /// replaced wholesale by each fetch. Empty on grids without the cap.
    pub(crate) server_history: Vec<ServerHistoryMessage>,
    /// Where this session stands in the once-per-login server-history fetch
    /// cycle (see [`ServerHistoryState`]).
    pub(crate) server_history_state: ServerHistoryState,
}

impl ChatSession {
    /// Creates a session whose last activity is `now`, with an empty roster, no
    /// typers, an empty log, nothing unread, a [`Joined`](ChatSessionLifecycle::Joined)
    /// lifecycle (the invite path overrides this to `Invited` before any traffic),
    /// and an empty no-voice channel facet.
    pub(crate) const fn new(now: Instant) -> Self {
        Self {
            last_activity: now,
            participants: BTreeSet::new(),
            typing: BTreeMap::new(),
            history: VecDeque::new(),
            unread: 0,
            lifecycle: ChatSessionLifecycle::Joined,
            // `VoiceChannelState::default()` is not `const`; spell out the empty
            // no-voice facet so the constructor stays `const`.
            voice: VoiceChannelState {
                has_voice: false,
                channel: None,
                joined: false,
                members: BTreeSet::new(),
            },
            server_history: Vec::new(),
            server_history_state: ServerHistoryState::Unfetched,
        }
    }

    /// Appends `message` to the log, dropping the oldest entry if that pushes the
    /// log past [`HISTORY_CAP`]. Shared by the inbound and outbound log paths;
    /// the unread bookkeeping is the caller's (it differs between the two).
    fn push_history(&mut self, message: SessionMessage) {
        self.history.push_back(message);
        while self.history.len() > HISTORY_CAP {
            self.history.pop_front();
        }
    }

    /// Logs an inbound message and, unless it is our own echo (`own_agent` equals
    /// the sender), bumps the unread counter. Offline-IM replays ride this same
    /// path, carrying their original wire timestamp.
    pub(crate) fn log_inbound(&mut self, message: SessionMessage, own_agent: Option<AgentKey>) {
        if own_agent != Some(message.sender) {
            self.unread = self.unread.saturating_add(1);
        }
        self.push_history(message);
    }

    /// Logs one of our own outbound messages and clears the unread counter
    /// (sending implies we have seen the conversation).
    pub(crate) fn log_outbound(&mut self, message: SessionMessage) {
        self.unread = 0;
        self.push_history(message);
    }

    /// Whether a fetched server-history entry duplicates a message already in
    /// the live ring: equal sender and text, with compatible timestamps —
    /// compatible meaning either side carries none (live lines log
    /// `timestamp: None`) or they differ by at most
    /// [`SERVER_HISTORY_TIMESTAMP_SLACK_SECONDS`]. This is how the message
    /// that *triggered* a lazy-open fetch, which arrives both live and inside
    /// the fetched backlog, is dropped from the backlog.
    fn duplicates_ring(&self, message: &ServerHistoryMessage) -> bool {
        self.history.iter().any(|live| {
            live.sender == message.sender
                && live.text == message.text
                && match (live.timestamp, message.timestamp) {
                    (Some(a), Some(b)) => a.abs_diff(b) <= SERVER_HISTORY_TIMESTAMP_SLACK_SECONDS,
                    _ => true,
                }
        })
    }

    /// Replaces the stored server-history backlog with `messages`
    /// (oldest-first, as fetched), dropping entries that duplicate a live-ring
    /// message ([`Self::duplicates_ring`]) and trimming to the **newest**
    /// [`SERVER_HISTORY_CAP`] entries, then marks the fetch cycle
    /// [`Fetched`](ServerHistoryState::Fetched). Replacing wholesale (rather
    /// than appending) makes an explicit re-fetch idempotent. The live ring,
    /// the unread counter, and the on-disk transcript are untouched.
    pub(crate) fn store_server_history(&mut self, messages: Vec<ServerHistoryMessage>) {
        let mut kept: Vec<ServerHistoryMessage> = messages
            .into_iter()
            .filter(|message| !self.duplicates_ring(message))
            .collect();
        if kept.len() > SERVER_HISTORY_CAP {
            kept.drain(..kept.len().saturating_sub(SERVER_HISTORY_CAP));
        }
        self.server_history = kept;
        self.server_history_state = ServerHistoryState::Fetched;
    }
}

/// A flattened, read-model view of a chat session's lifecycle — the public
/// counterpart of the internal [`ChatSessionLifecycle`], carried by
/// [`ChatSessionInfo::lifecycle`]. The `Invited` variant inlines the
/// [`PendingInvite`] fields rather than nesting them, so a consumer reads
/// `inviter` / `session_name` / `channel` directly off the view.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ChatLifecycleView {
    /// A session we are in (the common case for everything we have sent into or
    /// received traffic from).
    Joined,
    /// A still-pending invitation we have not yet accepted or declined.
    Invited {
        /// The inviting agent.
        inviter: AgentKey,
        /// The session's human-readable name (the group or conference name).
        session_name: String,
        /// Which channel(s) the invitation is to.
        channel: InviteChannel,
    },
}

impl ChatLifecycleView {
    /// Flattens the internal [`ChatSessionLifecycle`] into the public view,
    /// cloning the invite's `session_name` (the only owned field).
    pub(crate) fn from_lifecycle(lifecycle: &ChatSessionLifecycle) -> Self {
        match lifecycle {
            ChatSessionLifecycle::Joined => Self::Joined,
            ChatSessionLifecycle::Invited(invite) => Self::Invited {
                inviter: invite.inviter,
                session_name: invite.session_name.clone(),
                channel: invite.channel,
            },
        }
    }
}

/// A light, owned snapshot of one chat session — the element of the
/// [`Session::chat_sessions_info`](crate::Session::chat_sessions_info) list and
/// the [`Event::ChatSessions`](crate::Event::ChatSessions) reply. Deliberately
/// **omits the history and the activity stamp**: the list stays cheap to ship,
/// history is fetched separately and one bounded page at a time via
/// [`Event::ChatHistoryPage`](crate::Event::ChatHistoryPage), and the monotonic
/// `last_activity` is meaningless across a process boundary (it only orders the
/// list newest-first before it ships).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ChatSessionInfo {
    /// The typed session id (1:1 direct, group, or conference).
    pub kind: ChatSessionKind,
    /// Whether the session is joined or a still-pending invitation.
    pub lifecycle: ChatLifecycleView,
    /// The session roster: the group / conference participants, or the implicit
    /// `{ peer }` for a `Direct` session.
    pub participants: Vec<AgentKey>,
    /// The avatars currently typing (remote typers only, stale entries pruned).
    pub typing: Vec<AgentKey>,
    /// The number of unread inbound messages.
    pub unread: u32,
    /// Whether the session offers a voice channel (signalling only).
    pub has_voice: bool,
    /// Whether *we* have joined the session's voice channel (optimistic, at the
    /// signalling level — there is no audio ack).
    pub voice_joined: bool,
    /// Who is currently voice-connected (never the speaking state). Empty when the
    /// session has no voice or no agent-list voice update has been seen.
    pub voice_members: Vec<AgentKey>,
}

/// A friend paired with whether they are currently known-online — the element of
/// the [`Session::friends_presence`](crate::Session::friends_presence) snapshot
/// and the [`Event::FriendsSnapshot`](crate::Event::FriendsSnapshot) reply.
/// `online` follows the same visibility caveat as
/// [`Session::is_online`](crate::Session::is_online): `false` is "offline or not
/// visible to us", never provably offline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FriendPresence {
    /// The friend, with the friendship rights in both directions.
    pub friend: Friend,
    /// Whether the friend is currently known-online.
    pub online: bool,
}

/// A page token for [`Session::history_page`](crate::Session::history_page) — a
/// `prev` cursor returned by one page is fed back as the `before` argument of the
/// next to walk older windows. It counts how many messages, from the newest end, a
/// page already consumed over the **unified** memory→archive view: the in-memory
/// ring supplies the newest messages, and the runtime's on-disk chat log continues
/// older pages from the same count once the ring is exhausted. The count is exposed
/// (rather than fully
/// opaque) precisely so the runtime can cross that boundary; ordinary in-memory
/// consumers still need not interpret it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MessageCursor(usize);

impl MessageCursor {
    /// Wraps a "messages already consumed from the newest end" count as a cursor.
    pub(crate) const fn new(consumed: usize) -> Self {
        Self(consumed)
    }

    /// The number of newest in-memory messages this cursor skips past.
    pub(crate) const fn consumed(self) -> usize {
        self.0
    }

    /// Builds a cursor from a newest-first consumed count — the constructor the
    /// runtime's file-backed paging uses to continue older pages past the in-memory
    /// ring.
    #[must_use]
    pub const fn from_consumed(consumed: usize) -> Self {
        Self(consumed)
    }

    /// This cursor's newest-first consumed count, for the runtime to resume
    /// file-backed paging at the right offset.
    #[must_use]
    pub const fn consumed_count(self) -> usize {
        self.0
    }
}

#[cfg(test)]
mod server_history_tests {
    use super::{
        ChatSession, SERVER_HISTORY_CAP, SERVER_HISTORY_TIMESTAMP_SLACK_SECONDS,
        ServerHistoryMessage, ServerHistoryState, SessionMessage,
    };
    use crate::types::ImDialog;
    use pretty_assertions::assert_eq;
    use sl_types::key::AgentKey;
    use std::time::Instant;
    use uuid::Uuid;

    /// A live-ring entry from `sender` with `text` and `timestamp`.
    fn ring_message(sender: AgentKey, text: &str, timestamp: Option<u32>) -> SessionMessage {
        SessionMessage {
            sender,
            dialog: ImDialog::SessionSend,
            text: text.to_owned(),
            timestamp,
        }
    }

    /// A fetched backlog record from `sender` with `text` and `timestamp`.
    fn server_message(
        sender: AgentKey,
        text: &str,
        timestamp: Option<u32>,
    ) -> ServerHistoryMessage {
        ServerHistoryMessage {
            sender,
            sender_name: "Some Speaker".to_owned(),
            text: text.to_owned(),
            timestamp,
        }
    }

    /// The message that triggered a lazy-open fetch arrives both live (with
    /// `timestamp: None` — live lines carry no wire timestamp) and inside the
    /// fetched backlog (with the server's epoch): the either-side-`None`
    /// timestamp rule de-duplicates it on sender + text alone, while the other
    /// backlog entries survive.
    #[test]
    fn store_drops_the_live_trigger_duplicate() {
        let now = Instant::now();
        let mut session = ChatSession::new(now);
        let sender = AgentKey::from(Uuid::from_u128(0xC1));
        session.log_inbound(ring_message(sender, "trigger", None), None);

        session.store_server_history(vec![
            server_message(sender, "older line", Some(1_700_000_100)),
            server_message(sender, "trigger", Some(1_700_000_200)),
        ]);
        assert_eq!(
            session.server_history,
            vec![server_message(sender, "older line", Some(1_700_000_100))]
        );
        assert!(matches!(
            session.server_history_state,
            ServerHistoryState::Fetched
        ));
        // The live ring and its unread bookkeeping are untouched by a store.
        assert_eq!(session.history.len(), 1);
        assert_eq!(session.unread, 1);
    }

    /// Timestamped-both-sides de-dup honours the slack boundary: equal sender +
    /// text within the slack is the same message, one second past it is not —
    /// and the same text from a *different* sender is never de-duplicated.
    #[test]
    fn store_dedup_respects_slack_and_sender() {
        let now = Instant::now();
        let mut session = ChatSession::new(now);
        let sender = AgentKey::from(Uuid::from_u128(0xC2));
        let other = AgentKey::from(Uuid::from_u128(0xC3));
        session.log_inbound(ring_message(sender, "hello", Some(1_000_000)), None);

        session.store_server_history(vec![
            server_message(
                sender,
                "hello",
                Some(1_000_000 + SERVER_HISTORY_TIMESTAMP_SLACK_SECONDS),
            ),
            server_message(
                sender,
                "hello",
                Some(1_000_000 + SERVER_HISTORY_TIMESTAMP_SLACK_SECONDS + 1),
            ),
            server_message(other, "hello", Some(1_000_000)),
        ]);
        let kept: Vec<(AgentKey, Option<u32>)> = session
            .server_history
            .iter()
            .map(|entry| (entry.sender, entry.timestamp))
            .collect();
        assert_eq!(
            kept,
            vec![
                (
                    sender,
                    Some(1_000_000 + SERVER_HISTORY_TIMESTAMP_SLACK_SECONDS + 1)
                ),
                (other, Some(1_000_000)),
            ]
        );
    }

    /// A store replaces the previous backlog wholesale (an explicit re-fetch is
    /// idempotent, never accumulating), and an oversized backlog is trimmed to
    /// the **newest** [`SERVER_HISTORY_CAP`] entries.
    #[test]
    fn store_replaces_and_caps_keeping_newest() {
        let now = Instant::now();
        let mut session = ChatSession::new(now);
        let sender = AgentKey::from(Uuid::from_u128(0xC4));
        session.store_server_history(vec![server_message(sender, "first fetch", None)]);
        assert_eq!(session.server_history.len(), 1);

        let oversized: Vec<ServerHistoryMessage> = (0..SERVER_HISTORY_CAP + 10)
            .map(|index| server_message(sender, &format!("line {index}"), None))
            .collect();
        session.store_server_history(oversized);
        assert_eq!(session.server_history.len(), SERVER_HISTORY_CAP);
        // The oldest overflow (0..=9) is what was trimmed; the newest survive.
        assert_eq!(
            session
                .server_history
                .first()
                .map(|entry| entry.text.clone()),
            Some("line 10".to_owned())
        );
        assert_eq!(
            session
                .server_history
                .last()
                .map(|entry| entry.text.clone()),
            Some(format!("line {}", SERVER_HISTORY_CAP + 9))
        );
    }
}
