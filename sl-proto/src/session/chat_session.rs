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
use sl_types::key::{AgentKey, GroupKey};
use std::time::Instant;
use uuid::Uuid;

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

/// The mutable per-session state mirror — the value half of the chat-session
/// registry (the kind/id lives in the [`ChatSessionKind`] key). It grows
/// additively as later chat tasks land their facets (participants/typing,
/// history/unread, lifecycle, voice-channel state); for now it carries only the
/// activity stamp.
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
}

impl ChatSession {
    /// Creates a session whose last activity is `now`.
    pub(crate) const fn new(now: Instant) -> Self {
        Self { last_activity: now }
    }
}
