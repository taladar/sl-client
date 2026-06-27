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

/// Which of the three IM-session kinds a chat session is, carrying that kind's
/// *typed* canonical id. This is the key of the chat-session registry: the enum
/// discriminant keeps the three id spaces disjoint, so a group id never aliases a
/// conference id or a 1:1 XOR id in the map (the bug a bare-`Uuid` key would
/// have).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
#[derive(Debug, Clone, PartialEq, Eq)]
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
#[derive(Debug, Clone, PartialEq, Eq)]
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
#[derive(Debug, Clone, PartialEq, Eq)]
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

/// The mutable per-session state mirror — the value half of the chat-session
/// registry (the kind/id lives in the [`ChatSessionKind`] key). It grows
/// additively as later chat tasks land their facets (participants/typing,
/// history/unread, lifecycle, voice-channel state); for now it carries the
/// activity stamp, the roster, the typing set, and the message log.
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
}

impl ChatSession {
    /// Creates a session whose last activity is `now`, with an empty roster, no
    /// typers, an empty log, nothing unread, and a [`Joined`](ChatSessionLifecycle::Joined)
    /// lifecycle (the invite path overrides this to `Invited` before any traffic).
    pub(crate) const fn new(now: Instant) -> Self {
        Self {
            last_activity: now,
            participants: BTreeSet::new(),
            typing: BTreeMap::new(),
            history: VecDeque::new(),
            unread: 0,
            lifecycle: ChatSessionLifecycle::Joined,
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
}

/// A flattened, read-model view of a chat session's lifecycle — the public
/// counterpart of the internal [`ChatSessionLifecycle`], carried by
/// [`ChatSessionInfo::lifecycle`]. The `Invited` variant inlines the
/// [`PendingInvite`] fields rather than nesting them, so a consumer reads
/// `inviter` / `session_name` / `channel` directly off the view.
#[derive(Debug, Clone, PartialEq, Eq)]
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
#[derive(Debug, Clone, PartialEq, Eq)]
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
}

/// A friend paired with whether they are currently known-online — the element of
/// the [`Session::friends_presence`](crate::Session::friends_presence) snapshot
/// and the [`Event::FriendsSnapshot`](crate::Event::FriendsSnapshot) reply.
/// `online` follows the same visibility caveat as
/// [`Session::is_online`](crate::Session::is_online): `false` is "offline or not
/// visible to us", never provably offline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FriendPresence {
    /// The friend, with the friendship rights in both directions.
    pub friend: Friend,
    /// Whether the friend is currently known-online.
    pub online: bool,
}

/// An opaque page token for [`Session::history_page`](crate::Session::history_page)
/// — a `prev` cursor returned by one page is fed back as the `before` argument of
/// the next to walk older windows. Consumers never interpret it; the inner
/// representation is private so the memory→archive boundary (the on-disk chat log
/// added later) can change it transparently. Today it is a count of how many of
/// the newest in-memory messages a page already consumed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
}
