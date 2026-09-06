//! The **send-side choke point** — the one predicate every outgoing action asks
//! before it happens.
//!
//! A restriction only means something if the code that would break it asks
//! first, and an RLV-compliant viewer must not offer a bypass. The reference
//! solves that with a single developer-facing façade, `RlvActions`, whose
//! `canX()` predicates are called from all over `llviewer*` — the restriction
//! logic lives in one file and a call site can only spell the question one way.
//! [`RlvActions`] is that façade: it is the *only* thing a session, a chat bar
//! or a build tool has to consult, and it never re-derives an answer the state
//! machine already holds.
//!
//! What it covers is the **outgoing** half of enforcement — refusing to issue
//! what is forbidden. The receiving half (dropping incoming chat, hiding names)
//! is a different family with a different filter. The split matters because
//! this half is exactly the part a *headless* bot has to honour too, which is
//! why it lives in this pure crate rather than in a viewer system: no Bevy, no
//! session, no I/O.
//!
//! Three things are needed to answer a question here:
//!
//! - the [`RlvState`] — which restrictions are in force, with which exceptions
//!   and which [modifier](crate::RlvModifier) values;
//! - an [`RlvActionSource`] — the handful of facts about the world the pure
//!   crate cannot know (where the agent is, whether it is sitting, whether an
//!   IM session is already open, and the four settings the reference reads);
//! - for a world-interaction question, an [`RlvObject`] describing the thing
//!   being touched, sat on or edited. The caller has those facts at the click
//!   site, so they are passed in rather than looked up.
//!
//! ```
//! use sl_rlv::{parse_chat_line, RlvActionSource, RlvBehaviour, RlvState};
//! use uuid::Uuid;
//!
//! /// A world where the agent stands at the origin and sits on nothing.
//! struct Standing;
//! impl RlvActionSource for Standing {
//!     fn agent_position(&self) -> [f64; 3] { [0.0, 0.0, 0.0] }
//!     fn avatar_position(&self, _avatar: Uuid) -> Option<[f64; 3]> { None }
//!     fn object_root(&self, object: Uuid) -> Uuid { object }
//!     fn is_sitting(&self) -> bool { false }
//!     fn has_open_session(&self, _id: Uuid) -> bool { false }
//!     fn current_command(&self) -> Option<sl_rlv::RlvCurrentCommand> { None }
//! }
//!
//! let collar = Uuid::from_u128(1);
//! let mut state = RlvState::new();
//! state.apply(collar, parse_chat_line("@fly=n").unwrap()[0].as_ref().unwrap());
//!
//! // The choke point refuses, and nothing else has to know why.
//! assert!(!state.actions(&Standing).can_fly());
//! assert!(state.actions(&Standing).can_jump());
//! ```
//!
//! Reference (Firestorm, read-only): `rlvactions.h` / `rlvactions.cpp`
//! (`RlvActions::canX()`), `rlvhandler.cpp` (`filterChat`,
//! `redirectChatOrEmote`), `llfloaterimnearbychat.cpp:890-928` (the outgoing
//! chat choke point), `llagent.cpp:3993-4002` (`@alwaysrun` / `@temprun`),
//! `llagent.cpp:5131-5140` (`@tplm`).

use uuid::Uuid;

use crate::behaviour::RlvBehaviour;
use crate::modifier::{RlvModifier, TPLOCAL_DEFAULT};
use crate::query::{CHAT_CHANNEL_DEBUG, RlvReply, split_chat, truncate_chat};
use crate::state::{RlvExceptionCheck, RlvExceptionOption, RlvState};

/// The command being executed right now, if one is
/// (`RlvHandler::getCurrentCommand`, `rlvhandler.h:100`).
///
/// A restriction must not block the very command that is carrying it out: an
/// object holding `@sittp=n` may still `@sit:<uuid>=force` the agent onto
/// something across the region, and an object holding `@fly=n` may still
/// `@fly=force`. The reference achieves that by remembering which object's
/// command is on the stack and skipping that object's restrictions; this is
/// that memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct RlvCurrentCommand {
    /// The object whose command is being executed.
    pub object: Uuid,
    /// The behaviour it named.
    pub behaviour: RlvBehaviour,
}

impl RlvCurrentCommand {
    /// The command `behaviour` sent by `object`.
    #[must_use]
    pub const fn new(object: Uuid, behaviour: RlvBehaviour) -> Self {
        Self { object, behaviour }
    }
}

/// What kind of thing an [`RlvObject`] is, which is what decides *which* touch
/// and edit restrictions apply to it.
///
/// The reference asks three separate questions of an `LLViewerObject`
/// (`isAttachment`, `permYouOwner`, `isHUDAttachment`) and branches on them in
/// that order; these are the four cases that fall out
/// (`RlvActions::canTouch`, `rlvactions.cpp:585-624`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RlvObjectKind {
    /// A rezzed in-world object.
    World,
    /// A non-HUD attachment on the agent's own avatar.
    AttachmentSelf,
    /// An attachment worn by somebody else, and who wears it.
    AttachmentOther {
        /// The avatar wearing it — the root of the object's link tree, which
        /// for an attachment is the avatar itself.
        wearer: Uuid,
    },
    /// A HUD attachment on the agent.
    Hud,
}

impl RlvObjectKind {
    /// Whether this is worn rather than rezzed — a HUD counts.
    #[must_use]
    pub const fn is_attachment(&self) -> bool {
        !matches!(self, Self::World)
    }

    /// Whether this is a HUD attachment, which most restrictions leave alone
    /// because a locked-on HUD the agent could not touch would be a trap.
    #[must_use]
    pub const fn is_hud(&self) -> bool {
        matches!(self, Self::Hud)
    }
}

/// The object a world-interaction question is about.
///
/// The reference passes an `LLViewerObject*` and reads what it needs off it;
/// this pure crate cannot, so the caller — which is holding the object at the
/// click site anyway — fills these in.
///
/// Touch and edit restrictions apply **linkset-wide** and are tested against
/// [`RlvObject::root`], while `@fartouch` distance is measured to
/// [`RlvObject::position`] — the clicked prim, not the linkset root. Keeping
/// both fields is what makes that distinction expressible.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RlvObject {
    /// The object's own id.
    pub id: Uuid,
    /// The id of its link tree's root (`getRootEdit()->getID()`). For a
    /// single-prim object this is [`RlvObject::id`].
    pub root: Uuid,
    /// What kind of object it is.
    pub kind: RlvObjectKind,
    /// Where it is, in global coordinates.
    pub position: [f64; 3],
    /// Whether it is a prim (`LL_PCODE_VOLUME`). Only a prim can be sat on.
    pub is_volume: bool,
}

impl RlvObject {
    /// A rezzed single-prim in-world object at `position`.
    #[must_use]
    pub const fn world(id: Uuid, position: [f64; 3]) -> Self {
        Self {
            id,
            root: id,
            kind: RlvObjectKind::World,
            position,
            is_volume: true,
        }
    }

    /// The same object, but part of the link tree rooted at `root`.
    #[must_use]
    pub const fn rooted_at(mut self, root: Uuid) -> Self {
        self.root = root;
        self
    }

    /// The same object, but of `kind`.
    #[must_use]
    pub const fn of_kind(mut self, kind: RlvObjectKind) -> Self {
        self.kind = kind;
        self
    }

    /// The same object, but not a prim — an avatar or a particle system, which
    /// cannot be sat on.
    #[must_use]
    pub const fn non_volume(mut self) -> Self {
        self.is_volume = false;
        self
    }
}

/// Which volume a line of nearby chat is said at (`EChatType`, `llchat.h:41`).
///
/// `@chatwhisper`, `@chatnormal` and `@chatshout` do not block chat; they
/// *clamp* it, which is why this is a value the choke point rewrites rather
/// than a yes/no answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum RlvChatVolume {
    /// Whispered — audible for 10 m.
    Whisper,
    /// Ordinary chat — audible for 20 m.
    #[default]
    Normal,
    /// Shouted — audible for 100 m.
    Shout,
}

/// How much of the edit restrictions are in force
/// (`ERlvCheckType`, `rlvactions.h:36`).
///
/// A build tool asks this before it does the expensive part: with
/// [`RlvCheckType::Nothing`] it can grey itself out without walking the
/// selection, and with [`RlvCheckType::All`] it can skip the per-object checks
/// entirely.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RlvCheckType {
    /// Nothing is restricted — every object may be edited.
    All,
    /// Something might be — ask per object.
    Some,
    /// Nothing may be edited at all.
    Nothing,
}

/// What the send-side chat filter did to a line
/// (`RlvHandler::filterChat`, `rlvhandler.cpp:1263`).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RlvFilteredChat {
    /// The text to actually say — the original, a truncated emote, `"..."` or
    /// nothing at all.
    pub text: String,
    /// Whether the line was **blanked**, which is the reference's return value.
    /// A truncated emote is rewritten but not blanked, so this stays `false`
    /// for it — and `@redirchat` leans on exactly that distinction, redirecting
    /// only what `@sendchat` would have swallowed.
    pub blanked: bool,
}

/// What the choke point decided about one line of outgoing chat.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RlvChatDecision {
    /// Say it — at this volume, on this channel, with this text. Any of the
    /// three may differ from what was asked for.
    Send {
        /// The channel to say it on.
        channel: i32,
        /// The volume to say it at, clamped by `@chatnormal` and friends.
        volume: RlvChatVolume,
        /// The text to say, filtered by `@sendchat` and `@emote`.
        text: String,
    },
    /// Say nothing at all.
    Blocked,
    /// Do not say it publicly; chat these lines instead. The list is empty when
    /// `@redirchat` named only channels that `@sendchannel` blocks — the line
    /// is still swallowed, it just goes nowhere.
    Redirected(Vec<RlvReply>),
}

/// The facts about the world an action check needs and the state machine
/// cannot know.
///
/// This is the [`RlvQuerySource`](crate::RlvQuerySource) of the enforcement
/// side: a small, synchronous, read-only view of the viewer. The four settings
/// have the reference's defaults so a headless bot only has to answer the
/// world questions.
pub trait RlvActionSource {
    /// Where the agent is, in global coordinates.
    fn agent_position(&self) -> [f64; 3];

    /// Where `avatar` is, in global coordinates, or `None` when it is not in a
    /// region this viewer knows about. Out of sight is treated as infinitely
    /// far away, which is what makes an IM exclusion range fail closed.
    fn avatar_position(&self, avatar: Uuid) -> Option<[f64; 3]>;

    /// The root of the link tree `object` belongs to, or `object` itself when
    /// it is a single prim (`RlvObject::getRootID`).
    ///
    /// Only `@touchme` needs this: "may I touch this?" is answered `yes` when
    /// *any* prim of the asking linkset said `@touchme=n`.
    fn object_root(&self, object: Uuid) -> Uuid;

    /// Whether the agent is sitting on something.
    fn is_sitting(&self) -> bool;

    /// Whether an IM session — P2P or group — is already open with `id`.
    fn has_open_session(&self, id: Uuid) -> bool;

    /// The command being executed right now, if one is. See
    /// [`RlvCurrentCommand`].
    fn current_command(&self) -> Option<RlvCurrentCommand>;

    /// `RestrainedLoveCanOOC` — whether `((out of character))` chat is let
    /// through `@sendchat`. On by default.
    fn can_ooc(&self) -> bool {
        true
    }

    /// `RestrainedLoveShowEllipsis` — whether a blanked line is said as `"..."`
    /// rather than not said at all. On by default.
    fn show_ellipsis(&self) -> bool {
        true
    }

    /// `RLVaSplitRedirectChat` — whether a redirected line longer than one chat
    /// message is split across several instead of truncated. Off by default.
    fn split_redirect_chat(&self) -> bool {
        false
    }

    /// `RLVaShowRedirectChatTyping` — whether the typing indicator is still
    /// sent while `@redirchat` is in force. Off by default, because a typing
    /// indicator with no chat following it gives the redirect away.
    fn show_redirect_chat_typing(&self) -> bool {
        false
    }
}

/// The squared distance between two global positions.
fn distance_squared(from: [f64; 3], to: [f64; 3]) -> f64 {
    let [fx, fy, fz] = from;
    let [tx, ty, tz] = to;
    let (dx, dy, dz) = (fx - tx, fy - ty, fz - tz);
    dx * dx + dy * dy + dz * dz
}

/// The squared horizontal distance between two global positions — what
/// `@tplocal` measures, because a teleport straight up is still local.
fn distance_squared_xy(from: [f64; 3], to: [f64; 3]) -> f64 {
    let [fx, fy, _] = from;
    let [tx, ty, _] = to;
    let (dx, dy) = (fx - tx, fy - ty);
    dx * dx + dy * dy
}

/// `position` displaced by a region-frame `offset` — an object's centre plus
/// the point on it that was actually clicked.
fn offset_position(position: [f64; 3], offset: [f32; 3]) -> [f64; 3] {
    let [px, py, pz] = position;
    let [ox, oy, oz] = offset;
    [px + f64::from(ox), py + f64::from(oy), pz + f64::from(oz)]
}

/// Whether `text` is an emote (`RlvUtil::isEmote`, `rlvcommon.h:334`).
///
/// The reference's byte-length test is kept: `"/me "` alone is not an emote,
/// it is four bytes of nothing to say.
#[must_use]
pub fn is_emote(text: &str) -> bool {
    text.len() > 4 && (text.starts_with("/me ") || text.starts_with("/me'"))
}

/// The characters that make an emote too expressive to be let through
/// `@sendchat` un-truncated (`rlvhandler.cpp:1273`).
const EMOTE_ILLEGAL_CHARS: [char; 9] = ['"', '(', ')', '*', '=', '^', '_', '?', '~'];

/// How many characters of an emote survive `@sendchat` when `@emote` is not
/// held (`rlvhandler.cpp:1281`).
const EMOTE_TRUNCATE_CHARS: usize = 20;

/// How long a `/`-prefixed line may be before `@sendchat` blanks it — six
/// characters, so that a gesture trigger still fires (`rlvhandler.cpp:1288`).
const SLASH_COMMAND_MAX_CHARS: usize = 7;

/// The enforcement façade: the one place a call site asks whether it may act.
///
/// Build one where the question is asked — it borrows the state and the world
/// rather than copying them, so it is free to make and must not be held across
/// a change to either.
#[derive(Debug)]
pub struct RlvActions<'state, S: ?Sized> {
    /// The restrictions in force.
    state: &'state RlvState,
    /// The world the restrictions are being enforced in.
    source: &'state S,
}

impl<'state, S: RlvActionSource + ?Sized> RlvActions<'state, S> {
    /// Ask `state`'s restrictions about the world `source` describes.
    #[must_use]
    pub const fn new(state: &'state RlvState, source: &'state S) -> Self {
        Self { state, source }
    }

    /// The restrictions being enforced, for the rare call site that needs to
    /// ask something this façade does not cover.
    #[must_use]
    pub const fn state(&self) -> &'state RlvState {
        self.state
    }

    // ------------------------------------------------------------- internals

    /// The object whose command is executing, or the nil id when none is.
    ///
    /// The nil id is never an object id, so passing it to
    /// [`RlvState::has_behaviour_except`] excludes nothing — which is exactly
    /// what the reference relies on (`getCurrentObject` returns a null UUID).
    fn current_object(&self) -> Uuid {
        self.source
            .current_command()
            .map_or_else(Uuid::nil, |command| command.object)
    }

    /// Whether `option` is let through `behaviour`, decided from the
    /// restrictions in force.
    fn is_exception(&self, behaviour: RlvBehaviour, option: RlvExceptionOption) -> bool {
        self.state
            .is_exception(behaviour, option, RlvExceptionCheck::Automatic)
    }

    /// Whether `option` is let through `behaviour` on one object's word alone
    /// — the check the touch and hover-text families use, where an exception
    /// names an object rather than an avatar.
    fn is_permissive_exception(&self, behaviour: RlvBehaviour, option: RlvExceptionOption) -> bool {
        self.state
            .is_exception(behaviour, option, RlvExceptionCheck::Permissive)
    }

    /// The float in force on `modifier`, or its default when nothing set it.
    fn modifier_float(&self, modifier: RlvModifier) -> f64 {
        f64::from(
            self.state
                .modifiers()
                .value(modifier)
                .as_float()
                .unwrap_or(f32::MAX),
        )
    }

    /// Whether `target` is within `modifier` metres of the agent.
    fn within(&self, modifier: RlvModifier, target: [f64; 3]) -> bool {
        let limit = self.modifier_float(modifier);
        distance_squared(self.source.agent_position(), target) < limit * limit
    }

    /// Whether `target` is no further than `@fartouch` allows.
    ///
    /// The reference uses `<=` here and `<` for the sit and local-teleport
    /// radii; both are kept as they are, because a script that pins a radius to
    /// the exact distance of a prim is relying on which one it is.
    fn within_fartouch(&self, target: [f64; 3]) -> bool {
        let limit = self.modifier_float(RlvModifier::FartouchDist);
        distance_squared(self.source.agent_position(), target) <= limit * limit
    }

    /// Whether `avatar` sits inside the exclusion range a distance-modifier
    /// pair describes (`rlvCheckAvatarIMDistance`, `rlvactions.cpp:150`).
    ///
    /// An exclusion range is a hole in a communication block: `@sendim=n` with
    /// a `SendIMDistMin` of 10 blocks everyone *except* the avatars more than
    /// 10 m away. With no minimum set there is no hole, so the answer is `no`
    /// — and an avatar this viewer cannot see is treated as infinitely far,
    /// which puts it outside any range that has a maximum.
    ///
    /// These two slots hold **squared** metres (`rlvhandler.cpp:2260` writes
    /// `nDistMin * nDistMin`), so the comparison skips the square root.
    fn within_im_range(&self, avatar: Uuid, min: RlvModifier, max: RlvModifier) -> bool {
        let modifiers = self.state.modifiers();
        if !modifiers.has_value(min) {
            return false;
        }
        let min_distance = self.modifier_float(min);
        let max_distance = if modifiers.has_value(max) {
            self.modifier_float(max)
        } else {
            f64::MAX
        };
        let distance = self
            .source
            .avatar_position(avatar)
            .map_or(f64::MAX, |position| {
                distance_squared(self.source.agent_position(), position)
            });
        min_distance < max_distance && min_distance <= distance && distance <= max_distance
    }

    // -------------------------------------------- communication / interaction

    /// Whether the active group may be changed (`@setgroup`).
    ///
    /// `except` is the object asking on its own behalf: a `@setgroup=force`
    /// from the very object holding `@setgroup=n` is allowed, because the
    /// restriction is there to stop the *user* changing groups.
    #[must_use]
    pub fn can_change_active_group(&self, except: Option<Uuid>) -> bool {
        match except {
            None => !self.state.has_behaviour(RlvBehaviour::Setgroup),
            Some(object) => !self
                .state
                .has_behaviour_except(RlvBehaviour::Setgroup, "", object),
        }
    }

    /// Whether inventory may be given to *anybody* (`@share`) — what a blanket
    /// "Share" button asks before greying itself out.
    #[must_use]
    pub fn can_give_inventory(&self) -> bool {
        !self.state.has_behaviour(RlvBehaviour::Share)
            || self.state.has_exception(RlvBehaviour::Share)
    }

    /// Whether inventory may be given to `agent` (`@share`).
    #[must_use]
    pub fn can_give_inventory_to(&self, agent: Uuid) -> bool {
        !self.state.has_behaviour(RlvBehaviour::Share)
            || self.is_exception(RlvBehaviour::Share, RlvExceptionOption::Avatar(agent))
    }

    /// Whether gestures may be played (`@sendgesture`).
    #[must_use]
    pub fn can_play_gestures(&self) -> bool {
        !self.state.has_behaviour(RlvBehaviour::Sendgesture)
    }

    /// Whether chat may be said on `channel` (`@sendchannel`,
    /// `@sendchannel_except`).
    ///
    /// The two are opposites and both apply: `@sendchannel` blocks every
    /// channel but its exceptions, `@sendchannel_except` blocks only its own.
    #[must_use]
    pub fn can_send_channel(&self, channel: i32) -> bool {
        let option = RlvExceptionOption::Channel(channel);
        (!self.state.has_behaviour(RlvBehaviour::Sendchannel)
            || self.is_exception(RlvBehaviour::Sendchannel, option))
            && (!self.state.has_behaviour(RlvBehaviour::SendchannelExcept)
                || !self.is_exception(RlvBehaviour::SendchannelExcept, option))
    }

    /// Whether an IM may be sent to `recipient` — an avatar or a group
    /// (`@sendim`, `@sendimto`).
    #[must_use]
    pub fn can_send_im(&self, recipient: Uuid) -> bool {
        let option = RlvExceptionOption::Avatar(recipient);
        (!self.state.has_behaviour(RlvBehaviour::Sendim)
            || self.is_exception(RlvBehaviour::Sendim, option)
            || self.within_im_range(
                recipient,
                RlvModifier::SendImDistMin,
                RlvModifier::SendImDistMax,
            ))
            && (!self.state.has_behaviour(RlvBehaviour::Sendimto)
                || !self.is_exception(RlvBehaviour::Sendimto, option))
    }

    /// Whether an IM session may be *started* with `recipient` (`@startim`,
    /// `@startimto`).
    ///
    /// A session that is already open stays usable unless `ignore_open` says to
    /// disregard that — which is how the restriction closes the door without
    /// stranding a conversation in progress.
    #[must_use]
    pub fn can_start_im(&self, recipient: Uuid, ignore_open: bool) -> bool {
        let option = RlvExceptionOption::Avatar(recipient);
        let allowed = (!self.state.has_behaviour(RlvBehaviour::Startim)
            || self.is_exception(RlvBehaviour::Startim, option)
            || self.within_im_range(
                recipient,
                RlvModifier::StartImDistMin,
                RlvModifier::StartImDistMax,
            ))
            && (!self.state.has_behaviour(RlvBehaviour::Startimto)
                || !self.is_exception(RlvBehaviour::Startimto, option));
        allowed || (!ignore_open && self.source.has_open_session(recipient))
    }

    /// Whether the region may be told the agent is typing (`@redirchat`).
    ///
    /// Redirected chat never reaches nearby chat, so a typing indicator in
    /// front of it announces a conversation that will not arrive.
    #[must_use]
    pub fn can_send_typing_start(&self) -> bool {
        !self.state.has_behaviour(RlvBehaviour::Redirchat)
            || self.source.show_redirect_chat_typing()
    }

    /// The volume chat is actually said at (`@chatwhisper`, `@chatnormal`,
    /// `@chatshout`).
    ///
    /// Each of the three clamps one step, and they compose: with `@chatnormal`
    /// in force a shout comes out whispered, because normal is not quiet
    /// enough for a restriction that says "no louder than a whisper".
    #[must_use]
    pub fn check_chat_volume(&self, volume: RlvChatVolume) -> RlvChatVolume {
        let has = |behaviour| self.state.has_behaviour(behaviour);
        match volume {
            RlvChatVolume::Shout | RlvChatVolume::Normal if has(RlvBehaviour::Chatnormal) => {
                RlvChatVolume::Whisper
            }
            RlvChatVolume::Shout if has(RlvBehaviour::Chatshout) => RlvChatVolume::Normal,
            RlvChatVolume::Whisper if has(RlvBehaviour::Chatwhisper) => RlvChatVolume::Normal,
            unchanged => unchanged,
        }
    }

    /// What `@sendchat` leaves of one line (`RlvHandler::filterChat`,
    /// `rlvhandler.cpp:1263`).
    ///
    /// Only call this when `@sendchat` (outgoing) or `@recvchat` (incoming) is
    /// actually in force — the reference guards every call site that way, and
    /// the filter itself does not check.
    ///
    /// Four kinds of line get four answers: an emote is truncated to
    /// twenty characters (or to its first sentence) unless
    /// `@emote` allows it in full, but blanked outright if it contains
    /// punctuation that could smuggle words through; a short `/`-prefixed line
    /// is let past so gesture triggers still fire; `((OOC))` is let past when
    /// the setting allows it; everything else is blanked.
    ///
    /// `filter_emote` is `true` on the way out and `false` on the way in — an
    /// arriving emote is either heard or not, never shortened.
    #[must_use]
    pub fn filter_chat(&self, text: &str, filter_emote: bool) -> RlvFilteredChat {
        if text.is_empty() {
            return RlvFilteredChat {
                text: String::new(),
                blanked: false,
            };
        }

        let mut filtered = text.to_owned();
        let mut blanked = false;

        if is_emote(text) {
            if filter_emote {
                if text.contains(EMOTE_ILLEGAL_CHARS)
                    || text.contains(" -")
                    || text.contains("- ")
                    || text.contains("''")
                {
                    blanked = true;
                } else if !self.state.has_behaviour(RlvBehaviour::Emote) {
                    filtered = truncate_emote(text);
                }
            }
        } else if text.starts_with('/') {
            blanked = text.chars().count() > SLASH_COMMAND_MAX_CHARS;
        } else if !self.source.can_ooc() || !is_ooc(text) {
            blanked = true;
        }

        if blanked {
            filtered = if self.source.show_ellipsis() {
                "...".to_owned()
            } else {
                String::new()
            };
        }

        RlvFilteredChat {
            text: filtered,
            blanked,
        }
    }

    /// Where `@redirchat` / `@rediremote` send `text` instead of nearby chat,
    /// or `None` when it is not redirected at all
    /// (`RlvHandler::redirectChatOrEmote`, `rlvhandler.cpp:1306`).
    ///
    /// `Some(lines)` means the line must **not** be said publicly even when
    /// `lines` is empty — `@sendchannel` can block every channel the redirect
    /// names, and the words are dropped rather than leaking out loud.
    ///
    /// Only chat that `@sendchat` would have swallowed is redirected: an emote
    /// short enough to survive the filter is still an emote, and saying it
    /// twice would be worse than not redirecting it.
    #[must_use]
    pub fn redirect_chat(&self, text: &str) -> Option<Vec<RlvReply>> {
        let behaviour = if is_emote(text) {
            RlvBehaviour::Rediremote
        } else {
            RlvBehaviour::Redirchat
        };
        if text.is_empty() || !self.state.has_behaviour(behaviour) {
            return None;
        }
        if behaviour == RlvBehaviour::Redirchat && !self.filter_chat(text, false).blanked {
            return None;
        }

        let mut replies = Vec::new();
        for exception in self.state.exceptions() {
            if exception.behaviour != behaviour {
                continue;
            }
            let RlvExceptionOption::Channel(channel) = exception.option else {
                continue;
            };
            if !self.can_send_channel(channel) {
                continue;
            }
            if self.source.split_redirect_chat() {
                for line in split_chat(text, ' ') {
                    replies.push(RlvReply {
                        channel,
                        message: line,
                    });
                }
            } else {
                replies.push(RlvReply {
                    channel,
                    message: truncate_chat(text).to_owned(),
                });
            }
        }
        Some(replies)
    }

    /// The whole outgoing-chat decision, in the order the reference makes it
    /// (`llfloaterimnearbychat.cpp:890-928`).
    ///
    /// This is the choke point the chat bar calls and nothing else: volume
    /// clamp, then redirect, then filter on channel `0`; and on any other
    /// channel a plain `@sendchannel` yes/no, plus the rule that the debug
    /// channel counts as public chat because that is how viewers display it.
    #[must_use]
    pub fn outgoing_chat(
        &self,
        channel: i32,
        volume: RlvChatVolume,
        text: &str,
    ) -> RlvChatDecision {
        if channel != 0 {
            if !self.can_send_channel(channel) {
                return RlvChatDecision::Blocked;
            }
            if channel == CHAT_CHANNEL_DEBUG {
                let redirected = if is_emote(text) {
                    RlvBehaviour::Rediremote
                } else {
                    RlvBehaviour::Redirchat
                };
                if self.state.has_behaviour(RlvBehaviour::Sendchat)
                    || self.state.has_behaviour(redirected)
                {
                    return RlvChatDecision::Blocked;
                }
            }
            return RlvChatDecision::Send {
                channel,
                volume,
                text: text.to_owned(),
            };
        }

        let volume = self.check_chat_volume(volume);
        if let Some(replies) = self.redirect_chat(text) {
            return RlvChatDecision::Redirected(replies);
        }
        let text = if self.state.has_behaviour(RlvBehaviour::Sendchat) {
            self.filter_chat(text, true).text
        } else {
            text.to_owned()
        };
        RlvChatDecision::Send {
            channel,
            volume,
            text,
        }
    }

    // ------------------------------------------------------------- teleporting

    /// Whether a landmark teleport may be started (`@tplm`, `@tploc`,
    /// `@unsit`).
    ///
    /// `home` is the teleport-home case, which the reference deliberately
    /// leaves open unless *both* `@tplm` and `@tploc` are held — going home is
    /// the escape hatch of last resort.
    #[must_use]
    pub fn can_teleport_via_landmark(&self, home: bool) -> bool {
        let blocked = if home {
            self.state.has_behaviour(RlvBehaviour::Tplm)
                && self.state.has_behaviour(RlvBehaviour::Tploc)
        } else {
            self.state.has_behaviour(RlvBehaviour::Tplm)
        };
        !blocked && !(self.state.has_behaviour(RlvBehaviour::Unsit) && self.source.is_sitting())
    }

    /// Whether a teleport to an arbitrary location may be started (`@tploc`).
    ///
    /// A `@tpto=force` from an object that also holds `@tploc=n` is allowed:
    /// the restriction stops the *user* going places, not the object.
    #[must_use]
    pub fn can_teleport_to_location(&self) -> bool {
        let except = self.current_object();
        !self
            .state
            .has_behaviour_except(RlvBehaviour::Tploc, "", except)
            && self.can_stand_except(except)
    }

    /// Whether a short-range teleport to `target` may be started (`@sittp`,
    /// `@tplocal`).
    ///
    /// Double-clicking the ground is a teleport too, and the two restrictions
    /// that bound it measure different things: `@sittp` a sphere around the
    /// agent, `@tplocal` a horizontal radius capped at one region
    /// ([`TPLOCAL_DEFAULT`]) however large an object asks for.
    #[must_use]
    pub fn can_teleport_to_local(&self, target: [f64; 3]) -> bool {
        let except = self.current_object();
        if !self.can_stand_except(except) {
            return false;
        }
        if self
            .state
            .has_behaviour_except(RlvBehaviour::Sittp, "", except)
            && !self.within(RlvModifier::SittpDist, target)
        {
            return false;
        }
        if self
            .state
            .has_behaviour_except(RlvBehaviour::Tplocal, "", except)
        {
            let limit = self
                .modifier_float(RlvModifier::TplocalDist)
                .min(f64::from(TPLOCAL_DEFAULT));
            if distance_squared_xy(self.source.agent_position(), target) >= limit * limit {
                return false;
            }
        }
        true
    }

    /// Whether a teleport to `target` counts as local at all — the plain
    /// one-region radius, with no restriction involved.
    #[must_use]
    pub fn is_local_teleport(&self, target: [f64; 3]) -> bool {
        let limit = f64::from(TPLOCAL_DEFAULT);
        distance_squared_xy(self.source.agent_position(), target) < limit * limit
    }

    /// Whether a teleport offer from `sender` may be accepted (`@tplure`).
    #[must_use]
    pub fn can_accept_teleport_offer(&self, sender: Uuid) -> bool {
        (!self.state.has_behaviour(RlvBehaviour::Tplure)
            || self.is_exception(RlvBehaviour::Tplure, RlvExceptionOption::Avatar(sender)))
            && self.can_stand()
    }

    /// Whether a teleport *request* from `sender` may be answered
    /// (`@tprequest`).
    #[must_use]
    pub fn can_accept_teleport_request(&self, sender: Uuid) -> bool {
        !self.state.has_behaviour(RlvBehaviour::Tprequest)
            || self.is_exception(RlvBehaviour::Tprequest, RlvExceptionOption::Avatar(sender))
    }

    // ---------------------------------------------------------------- movement

    /// Whether the agent may fly (`@fly`).
    ///
    /// While a command is executing this disregards the issuing object's own
    /// restriction, so `@fly=force` works from an object holding `@fly=n`.
    #[must_use]
    pub fn can_fly(&self) -> bool {
        !self
            .state
            .has_behaviour_except(RlvBehaviour::Fly, "", self.current_object())
    }

    /// Whether the agent may fly, disregarding `except`'s restriction.
    #[must_use]
    pub fn can_fly_except(&self, except: Uuid) -> bool {
        !self
            .state
            .has_behaviour_except(RlvBehaviour::Fly, "", except)
    }

    /// Whether the agent may jump (`@jump`).
    #[must_use]
    pub fn can_jump(&self) -> bool {
        !self.state.has_behaviour(RlvBehaviour::Jump)
    }

    /// Whether always-run may be switched on (`@alwaysrun`).
    #[must_use]
    pub fn can_always_run(&self) -> bool {
        !self.state.has_behaviour(RlvBehaviour::Alwaysrun)
    }

    /// Whether the temporary run of a held movement key applies (`@temprun`).
    #[must_use]
    pub fn can_temp_run(&self) -> bool {
        !self.state.has_behaviour(RlvBehaviour::Temprun)
    }

    // ----------------------------------------------------------------- posture

    /// Whether the agent may stand up (`@unsit`).
    ///
    /// An agent that is not sitting can always "stand": the restriction has
    /// nothing to hold on to, and answering `no` would block every caller that
    /// asks this on the way to something else.
    #[must_use]
    pub fn can_stand(&self) -> bool {
        !self.state.has_behaviour(RlvBehaviour::Unsit) || !self.source.is_sitting()
    }

    /// Whether the agent may stand up, disregarding `except`'s restriction.
    #[must_use]
    pub fn can_stand_except(&self, except: Uuid) -> bool {
        !self
            .state
            .has_behaviour_except(RlvBehaviour::Unsit, "", except)
            || !self.source.is_sitting()
    }

    /// Whether the agent may sit on the ground (`@sit`, `@unsit`).
    #[must_use]
    pub fn can_ground_sit(&self) -> bool {
        !self.state.has_behaviour(RlvBehaviour::Sit) && self.can_stand()
    }

    /// Whether the agent may sit on the ground, disregarding `except`'s
    /// restrictions — what `@sitground=force` asks.
    #[must_use]
    pub fn can_ground_sit_except(&self, except: Uuid) -> bool {
        !self
            .state
            .has_behaviour_except(RlvBehaviour::Sit, "", except)
            && self.can_stand_except(except)
    }

    /// Whether the agent may sit on `object`, clicked at `offset` from its
    /// centre (`@sit`, `@unsit`, `@standtp`, `@sittp`, `@fartouch`).
    ///
    /// Sitting is also a teleport, which is why the range restrictions apply to
    /// it: `@sittp=n` stops a click from moving the agent across the region.
    /// `@standtp` joins `@unsit` in blocking a *change* of seat, because
    /// standing to sit elsewhere would trigger the teleport back.
    #[must_use]
    pub fn can_sit(&self, object: &RlvObject, offset: [f32; 3]) -> bool {
        if !object.is_volume || self.state.has_behaviour(RlvBehaviour::Sit) {
            return false;
        }
        let seated = self.source.is_sitting();
        if seated
            && (self.state.has_behaviour(RlvBehaviour::Unsit)
                || self.state.has_behaviour(RlvBehaviour::Standtp))
        {
            return false;
        }
        if self
            .source
            .current_command()
            .is_some_and(|command| command.behaviour == RlvBehaviour::Sit)
        {
            return true;
        }
        let target = offset_position(object.position, offset);
        (!self.state.has_behaviour(RlvBehaviour::Sittp)
            || self.within(RlvModifier::SittpDist, target))
            && (!self.state.has_behaviour(RlvBehaviour::Fartouch)
                || self.within(RlvModifier::FartouchDist, target))
    }

    // ------------------------------------------------------- world interaction

    /// Whether the build tools may be opened at all (`@edit`, `@rez`).
    ///
    /// The floater is worth opening if *either* half of it still works.
    #[must_use]
    pub fn can_build(&self) -> bool {
        !self.state.has_behaviour(RlvBehaviour::Edit)
            || !self.state.has_behaviour(RlvBehaviour::Rez)
    }

    /// Whether objects may be rezzed (`@rez`).
    #[must_use]
    pub fn can_rez(&self) -> bool {
        !self.state.has_behaviour(RlvBehaviour::Rez)
    }

    /// Whether an object set for sale may be bought (`@buy`).
    #[must_use]
    pub fn can_buy_object(&self) -> bool {
        !self.state.has_behaviour(RlvBehaviour::Buy)
    }

    /// Whether an object — a vendor — may be paid (`@buy`).
    #[must_use]
    pub fn can_pay_object(&self) -> bool {
        !self.state.has_behaviour(RlvBehaviour::Buy)
    }

    /// Whether an avatar may be paid (`@pay`).
    #[must_use]
    pub fn can_pay_avatar(&self) -> bool {
        !self.state.has_behaviour(RlvBehaviour::Pay)
    }

    /// How much of the edit restrictions are in force (`@edit`, `@editobj`,
    /// `@editattach`, `@editworld`).
    ///
    /// A caller asks this to bail out early — [`RlvCheckType::All`] to skip the
    /// per-object checks, [`RlvCheckType::Nothing`] to skip walking the
    /// selection at all.
    #[must_use]
    pub fn can_edit(&self, check: RlvCheckType) -> bool {
        let has = |behaviour| self.state.has_behaviour(behaviour);
        match check {
            RlvCheckType::All => {
                !has(RlvBehaviour::Edit)
                    && !has(RlvBehaviour::Editobj)
                    && !has(RlvBehaviour::Editattach)
                    && !has(RlvBehaviour::Editworld)
            }
            RlvCheckType::Some => {
                (!has(RlvBehaviour::Edit) || self.state.has_exception(RlvBehaviour::Edit))
                    && (!has(RlvBehaviour::Editattach) || has(RlvBehaviour::Editworld))
            }
            RlvCheckType::Nothing => {
                (has(RlvBehaviour::Edit) && !self.state.has_exception(RlvBehaviour::Edit))
                    || (has(RlvBehaviour::Editattach) && has(RlvBehaviour::Editworld))
            }
        }
    }

    /// Whether `object` may be edited (`@edit`, `@editobj`, `@editattach`,
    /// `@editworld`).
    ///
    /// Deliberately not subject to `@fartouch`: an edit has no pick offset to
    /// measure to, and the reference leans on a selection having to exist
    /// first instead.
    #[must_use]
    pub fn can_edit_object(&self, object: &RlvObject) -> bool {
        let root = RlvExceptionOption::Avatar(object.root);
        (!self.state.has_behaviour(RlvBehaviour::Edit)
            || self.is_exception(RlvBehaviour::Edit, root))
            && (!self.state.has_behaviour(RlvBehaviour::Editobj)
                || !self.is_exception(RlvBehaviour::Editobj, root))
            && if object.kind.is_attachment() {
                !self.state.has_behaviour(RlvBehaviour::Editattach)
            } else {
                !self.state.has_behaviour(RlvBehaviour::Editworld)
            }
    }

    /// Whether `object` may be interacted with in any way at all — touched,
    /// edited, grabbed, clicked (`@interact`, `@fartouch`).
    ///
    /// `None` answers `true`, so a call site that has not resolved an object
    /// yet is not short-circuited into refusing. HUD attachments are exempt:
    /// `@interact=n` that reached the HUD would lock the agent out of the very
    /// controls the restriction is worn with.
    #[must_use]
    pub fn can_interact(&self, object: Option<&RlvObject>, offset: [f32; 3]) -> bool {
        let Some(object) = object else {
            return true;
        };
        if object.kind.is_hud() {
            return true;
        }
        (!self.state.has_behaviour(RlvBehaviour::Interact))
            && (!self.state.has_behaviour(RlvBehaviour::Fartouch)
                || self.within_fartouch(offset_position(object.position, offset)))
    }

    /// Whether `object` may be touched, clicked at `offset` from its centre
    /// (`@touchall`, `@touchthis`, `@touchworld`, `@touchattach`,
    /// `@touchattachself`, `@touchattachother`, `@touchhud`, `@touchme`,
    /// `@fartouch`).
    ///
    /// The granularity is the point: an object can be shut out of touch as a
    /// class (world / attachment / own attachment / other's attachment / HUD),
    /// individually by id, or by distance — and `@touchme` overrides all of it
    /// for the linkset that asked, so a locked-on collar stays operable however
    /// wide the block around it is.
    ///
    /// Touch restrictions apply linkset-wide and are tested against
    /// [`RlvObject::root`], but `@fartouch` measures to the clicked prim.
    #[must_use]
    pub fn can_touch(&self, object: &RlvObject, offset: [f32; 3]) -> bool {
        let touched = self.touch_allowed(object, offset);
        if touched || !self.state.has_behaviour(RlvBehaviour::Touchme) {
            return touched;
        }
        // `@touchme=n` from any prim of the linkset re-opens the whole linkset.
        self.state
            .objects_holding(RlvBehaviour::Touchme)
            .any(|holder| self.source.object_root(holder) == object.root)
    }

    /// [`RlvActions::can_touch`] without the `@touchme` override — the part
    /// that is a plain restriction check.
    fn touch_allowed(&self, object: &RlvObject, offset: [f32; 3]) -> bool {
        if object.root.is_nil() {
            return false;
        }
        let root = RlvExceptionOption::Avatar(object.root);
        // `@touchall` reaches world objects and worn attachments, but not HUDs.
        if !object.kind.is_hud() && self.state.has_behaviour(RlvBehaviour::Touchall) {
            return false;
        }
        // A `@touchthis` "exception" names what is blocked, not what is let by.
        if self.state.has_behaviour(RlvBehaviour::Touchthis)
            && self.is_permissive_exception(RlvBehaviour::Touchthis, root)
        {
            return false;
        }
        let in_range = || {
            !self.state.has_behaviour(RlvBehaviour::Fartouch)
                || self.within_fartouch(offset_position(object.position, offset))
        };

        match object.kind {
            RlvObjectKind::World => {
                (!self.state.has_behaviour(RlvBehaviour::Touchworld)
                    || self.is_permissive_exception(RlvBehaviour::Touchworld, root))
                    && in_range()
            }
            RlvObjectKind::AttachmentOther { wearer } => {
                let wearer_option = RlvExceptionOption::Avatar(wearer);
                let blocked = self.state.has_behaviour(RlvBehaviour::Touchattach)
                    || self.state.has_behaviour(RlvBehaviour::Touchattachother);
                let excepted = self.is_permissive_exception(RlvBehaviour::Touchattach, root)
                    || self.is_permissive_exception(RlvBehaviour::Touchattach, wearer_option);
                (!blocked || excepted)
                    // Naming an avatar under `@touchattachother` blocks it
                    // outright, whatever the general attachment exceptions say.
                    && !self.state.is_exception(
                        RlvBehaviour::Touchattachother,
                        wearer_option,
                        RlvExceptionCheck::Automatic,
                    )
                    && in_range()
            }
            RlvObjectKind::AttachmentSelf => {
                let excepted = self.is_permissive_exception(RlvBehaviour::Touchattach, root);
                (!self.state.has_behaviour(RlvBehaviour::Touchattach) || excepted)
                    && (!self.state.has_behaviour(RlvBehaviour::Touchattachself) || excepted)
            }
            RlvObjectKind::Hud => {
                !self.state.has_behaviour(RlvBehaviour::Touchhud)
                    || self.is_permissive_exception(RlvBehaviour::Touchhud, root)
            }
        }
    }
}

/// Whether `text` is out-of-character chat — wrapped in double parentheses
/// (`rlvhandler.cpp:1292`).
fn is_ooc(text: &str) -> bool {
    text.len() >= 4 && text.starts_with("((") && text.ends_with("))")
}

/// An emote cut down to what `@sendchat` lets through: its first sentence, or
/// [`EMOTE_TRUNCATE_CHARS`] characters, whichever is shorter
/// (`rlvhandler.cpp:1281`).
///
/// The reference measures the dot's position in *bytes* and then truncates by
/// *characters*; this measures both in characters, which only differs for an
/// emote whose first sentence carries non-ASCII before the dot.
fn truncate_emote(text: &str) -> String {
    let dot = text
        .chars()
        .position(|character| character == '.')
        .filter(|&index| index > 0 && index < EMOTE_TRUNCATE_CHARS);
    let keep = dot.map_or(EMOTE_TRUNCATE_CHARS, |index| index.saturating_add(1));
    text.chars().take(keep).collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::{RlvCommand, parse_chat_line};
    use pretty_assertions::assert_eq;

    /// A boxed error so tests can use `?` on `Result` and `Option` instead of
    /// the disallowed `unwrap` / `expect` / indexing.
    type TestError = Box<dyn core::error::Error>;

    /// One of the four viewer settings the façade reads.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum TestSetting {
        /// `RestrainedLoveCanOOC`.
        Ooc,
        /// `RestrainedLoveShowEllipsis`.
        Ellipsis,
        /// `RLVaSplitRedirectChat`.
        SplitRedirect,
        /// `RLVaShowRedirectChatTyping`.
        ShowTyping,
    }

    /// A world the tests drive by hand.
    #[derive(Debug, Default)]
    struct TestWorld {
        /// Where the agent is.
        agent: [f64; 3],
        /// Where the avatars the tests care about are.
        avatars: Vec<(Uuid, [f64; 3])>,
        /// Which objects belong to which link tree.
        roots: Vec<(Uuid, Uuid)>,
        /// Whether the agent is sitting.
        sitting: bool,
        /// Which sessions are already open.
        sessions: Vec<Uuid>,
        /// The command being executed, if any.
        current: Option<RlvCurrentCommand>,
        /// The settings that are on.
        settings: BTreeSet<TestSetting>,
    }

    impl TestWorld {
        /// A world with the reference's default settings and nothing in it.
        fn new() -> Self {
            Self {
                settings: [TestSetting::Ooc, TestSetting::Ellipsis]
                    .into_iter()
                    .collect(),
                ..Self::default()
            }
        }

        /// The same world with `setting` switched on.
        fn with(mut self, setting: TestSetting) -> Self {
            self.settings.insert(setting);
            self
        }

        /// The same world with `setting` switched off.
        fn without(mut self, setting: TestSetting) -> Self {
            self.settings.remove(&setting);
            self
        }
    }

    impl RlvActionSource for TestWorld {
        fn agent_position(&self) -> [f64; 3] {
            self.agent
        }

        fn avatar_position(&self, avatar: Uuid) -> Option<[f64; 3]> {
            self.avatars
                .iter()
                .find(|&&(id, _)| id == avatar)
                .map(|&(_, position)| position)
        }

        fn object_root(&self, object: Uuid) -> Uuid {
            self.roots
                .iter()
                .find(|&&(id, _)| id == object)
                .map_or(object, |&(_, root)| root)
        }

        fn is_sitting(&self) -> bool {
            self.sitting
        }

        fn has_open_session(&self, id: Uuid) -> bool {
            self.sessions.contains(&id)
        }

        fn current_command(&self) -> Option<RlvCurrentCommand> {
            self.current
        }

        fn can_ooc(&self) -> bool {
            self.settings.contains(&TestSetting::Ooc)
        }

        fn show_ellipsis(&self) -> bool {
            self.settings.contains(&TestSetting::Ellipsis)
        }

        fn split_redirect_chat(&self) -> bool {
            self.settings.contains(&TestSetting::SplitRedirect)
        }

        fn show_redirect_chat_typing(&self) -> bool {
            self.settings.contains(&TestSetting::ShowTyping)
        }
    }

    /// The object every test issues its restrictions from.
    fn collar() -> Uuid {
        Uuid::from_u128(0x0c01_1a12)
    }

    /// Apply every command on `line` to `state`, as `object`.
    fn apply(state: &mut RlvState, object: Uuid, line: &str) -> Result<(), TestError> {
        for command in parse_chat_line(line).ok_or("not an RLV line")? {
            let command: RlvCommand = command?;
            state.apply(object, &command);
        }
        Ok(())
    }

    /// A state holding `line`, issued by [`collar`].
    fn state_of(line: &str) -> Result<RlvState, TestError> {
        let mut state = RlvState::new();
        apply(&mut state, collar(), line)?;
        Ok(state)
    }

    #[test]
    fn fly_and_jump_are_plain_yes_no() -> Result<(), TestError> {
        let world = TestWorld::new();
        let state = state_of("@fly=n")?;
        let actions = state.actions(&world);
        assert!(!actions.can_fly());
        assert!(actions.can_jump());
        assert!(actions.can_always_run());
        Ok(())
    }

    #[test]
    fn an_object_is_not_blocked_by_its_own_restriction() -> Result<(), TestError> {
        let state = state_of("@fly=n")?;
        let world = TestWorld {
            current: Some(RlvCurrentCommand::new(collar(), RlvBehaviour::Fly)),
            ..TestWorld::new()
        };
        // `@fly=force` from the very object holding `@fly=n` still flies.
        assert!(state.actions(&world).can_fly());
        // Anyone else's is still blocked.
        assert!(!state.actions(&world).can_fly_except(Uuid::from_u128(9)));
        Ok(())
    }

    #[test]
    fn a_second_holder_keeps_the_block_up() -> Result<(), TestError> {
        let mut state = state_of("@fly=n")?;
        apply(&mut state, Uuid::from_u128(7), "@fly=n")?;
        let world = TestWorld {
            current: Some(RlvCurrentCommand::new(collar(), RlvBehaviour::Fly)),
            ..TestWorld::new()
        };
        assert!(!state.actions(&world).can_fly());
        Ok(())
    }

    #[test]
    fn standing_is_free_when_not_sitting() -> Result<(), TestError> {
        let state = state_of("@unsit=n")?;
        assert!(state.actions(&TestWorld::new()).can_stand());
        let seated = TestWorld {
            sitting: true,
            ..TestWorld::new()
        };
        assert!(!state.actions(&seated).can_stand());
        Ok(())
    }

    #[test]
    fn sitting_needs_a_prim_within_the_sittp_radius() -> Result<(), TestError> {
        let state = state_of("@sittp=n")?;
        let world = TestWorld::new();
        let near = RlvObject::world(Uuid::from_u128(2), [1.0, 0.0, 0.0]);
        let far = RlvObject::world(Uuid::from_u128(3), [10.0, 0.0, 0.0]);
        assert!(state.actions(&world).can_sit(&near, [0.0; 3]));
        assert!(!state.actions(&world).can_sit(&far, [0.0; 3]));
        // An avatar is not a prim, however close it is.
        assert!(!state.actions(&world).can_sit(&near.non_volume(), [0.0; 3]));
        Ok(())
    }

    #[test]
    fn a_forced_sit_ignores_the_sittp_radius() -> Result<(), TestError> {
        let state = state_of("@sittp=n")?;
        let world = TestWorld {
            current: Some(RlvCurrentCommand::new(collar(), RlvBehaviour::Sit)),
            ..TestWorld::new()
        };
        let far = RlvObject::world(Uuid::from_u128(3), [100.0, 0.0, 0.0]);
        assert!(state.actions(&world).can_sit(&far, [0.0; 3]));
        Ok(())
    }

    #[test]
    fn the_sittp_radius_is_the_modifier_the_object_named() -> Result<(), TestError> {
        let state = state_of("@sittp:20=n")?;
        let world = TestWorld::new();
        let far = RlvObject::world(Uuid::from_u128(3), [10.0, 0.0, 0.0]);
        assert!(state.actions(&world).can_sit(&far, [0.0; 3]));
        Ok(())
    }

    #[test]
    fn fartouch_measures_to_the_clicked_point() -> Result<(), TestError> {
        let state = state_of("@fartouch=n")?;
        let world = TestWorld::new();
        // The centre is out of reach but the near face is not.
        let object = RlvObject::world(Uuid::from_u128(2), [2.0, 0.0, 0.0]);
        assert!(!state.actions(&world).can_touch(&object, [0.0; 3]));
        assert!(state.actions(&world).can_touch(&object, [-1.0, 0.0, 0.0]));
        Ok(())
    }

    #[test]
    fn touchall_spares_the_hud() -> Result<(), TestError> {
        let state = state_of("@touchall=n")?;
        let world = TestWorld::new();
        let world_object = RlvObject::world(Uuid::from_u128(2), [0.0; 3]);
        let hud = world_object.of_kind(RlvObjectKind::Hud);
        assert!(!state.actions(&world).can_touch(&world_object, [0.0; 3]));
        assert!(state.actions(&world).can_touch(&hud, [0.0; 3]));
        Ok(())
    }

    #[test]
    fn touchme_reopens_the_whole_linkset() -> Result<(), TestError> {
        let root = Uuid::from_u128(0x0a11);
        let child = Uuid::from_u128(0x0a12);
        let mut state = state_of("@touchall=n")?;
        // The restriction is issued by a *child* prim of the linkset.
        apply(&mut state, child, "@touchme=n")?;
        let world = TestWorld {
            roots: vec![(child, root)],
            ..TestWorld::new()
        };
        let object = RlvObject::world(root, [0.0; 3]);
        assert!(state.actions(&world).can_touch(&object, [0.0; 3]));
        // A different linkset is still shut out.
        let other = RlvObject::world(Uuid::from_u128(0x0b11), [0.0; 3]);
        assert!(!state.actions(&world).can_touch(&other, [0.0; 3]));
        Ok(())
    }

    #[test]
    fn touchthis_blocks_the_object_it_names() -> Result<(), TestError> {
        let target = Uuid::from_u128(0x0d0d);
        let state = state_of(&format!("@touchthis:{target}=n"))?;
        let world = TestWorld::new();
        let blocked = RlvObject::world(target, [0.0; 3]);
        let other = RlvObject::world(Uuid::from_u128(0x0e0e), [0.0; 3]);
        assert!(!state.actions(&world).can_touch(&blocked, [0.0; 3]));
        assert!(state.actions(&world).can_touch(&other, [0.0; 3]));
        Ok(())
    }

    #[test]
    fn touchattachother_names_an_avatar_to_shut_out() -> Result<(), TestError> {
        let wearer = Uuid::from_u128(0x0f0f);
        let state = state_of(&format!("@touchattachother:{wearer}=n"))?;
        let world = TestWorld::new();
        let theirs = RlvObject::world(Uuid::from_u128(2), [0.0; 3])
            .of_kind(RlvObjectKind::AttachmentOther { wearer });
        let someone_elses = RlvObject::world(Uuid::from_u128(3), [0.0; 3]).of_kind(
            RlvObjectKind::AttachmentOther {
                wearer: Uuid::from_u128(0x0f10),
            },
        );
        assert!(!state.actions(&world).can_touch(&theirs, [0.0; 3]));
        assert!(state.actions(&world).can_touch(&someone_elses, [0.0; 3]));
        Ok(())
    }

    #[test]
    fn send_channel_and_its_inverse() -> Result<(), TestError> {
        let world = TestWorld::new();
        let state = state_of("@sendchannel=n,sendchannel:5=add")?;
        let actions = state.actions(&world);
        assert!(actions.can_send_channel(5));
        assert!(!actions.can_send_channel(6));

        // Naming a channel alone never blocks anything: the bare restriction is
        // what the exceptions poke holes in, on either side.
        let bare = state_of("@sendchannel_except:5=add")?;
        assert!(bare.actions(&world).can_send_channel(5));

        let except = state_of("@sendchannel_except=n,sendchannel_except:5=add")?;
        let actions = except.actions(&world);
        assert!(!actions.can_send_channel(5));
        assert!(actions.can_send_channel(6));
        Ok(())
    }

    #[test]
    fn an_im_exception_lets_one_avatar_through() -> Result<(), TestError> {
        let friend = Uuid::from_u128(0x1234);
        let stranger = Uuid::from_u128(0x5678);
        let state = state_of(&format!("@sendim=n,sendim:{friend}=add"))?;
        let world = TestWorld::new();
        let actions = state.actions(&world);
        assert!(actions.can_send_im(friend));
        assert!(!actions.can_send_im(stranger));
        Ok(())
    }

    #[test]
    fn sendimto_blocks_the_avatar_it_names() -> Result<(), TestError> {
        let blocked = Uuid::from_u128(0x1234);
        let state = state_of(&format!("@sendimto:{blocked}=n"))?;
        let world = TestWorld::new();
        assert!(!state.actions(&world).can_send_im(blocked));
        assert!(state.actions(&world).can_send_im(Uuid::from_u128(0x5678)));
        Ok(())
    }

    #[test]
    fn the_im_exclusion_range_is_a_hole_in_the_block() -> Result<(), TestError> {
        let near = Uuid::from_u128(1);
        let far = Uuid::from_u128(2);
        let unseen = Uuid::from_u128(3);
        let state = state_of("@sendim=n,sendim:10;100=n")?;
        let world = TestWorld {
            avatars: vec![(near, [5.0, 0.0, 0.0]), (far, [50.0, 0.0, 0.0])],
            ..TestWorld::new()
        };
        let actions = state.actions(&world);
        // Inside the minimum: still blocked. Between min and max: let through.
        assert!(!actions.can_send_im(near));
        assert!(actions.can_send_im(far));
        // Out of sight is infinitely far, which is outside the maximum.
        assert!(!actions.can_send_im(unseen));
        Ok(())
    }

    #[test]
    fn a_bare_sendim_has_no_exclusion_range() -> Result<(), TestError> {
        let avatar = Uuid::from_u128(1);
        let state = state_of("@sendim=n")?;
        let world = TestWorld {
            avatars: vec![(avatar, [5.0, 0.0, 0.0])],
            ..TestWorld::new()
        };
        assert!(!state.actions(&world).can_send_im(avatar));
        Ok(())
    }

    #[test]
    fn an_open_session_survives_startim() -> Result<(), TestError> {
        let friend = Uuid::from_u128(0x1234);
        let state = state_of("@startim=n")?;
        let world = TestWorld {
            sessions: vec![friend],
            ..TestWorld::new()
        };
        let actions = state.actions(&world);
        assert!(actions.can_start_im(friend, false));
        assert!(!actions.can_start_im(friend, true));
        assert!(!actions.can_start_im(Uuid::from_u128(0x5678), false));
        Ok(())
    }

    #[test]
    fn chat_volume_clamps_one_step_at_a_time() -> Result<(), TestError> {
        let world = TestWorld::new();
        let shout = state_of("@chatshout=n")?;
        assert_eq!(
            shout
                .actions(&world)
                .check_chat_volume(RlvChatVolume::Shout),
            RlvChatVolume::Normal
        );
        assert_eq!(
            shout
                .actions(&world)
                .check_chat_volume(RlvChatVolume::Whisper),
            RlvChatVolume::Whisper
        );

        let normal = state_of("@chatnormal=n")?;
        assert_eq!(
            normal
                .actions(&world)
                .check_chat_volume(RlvChatVolume::Shout),
            RlvChatVolume::Whisper
        );

        let whisper = state_of("@chatwhisper=n")?;
        assert_eq!(
            whisper
                .actions(&world)
                .check_chat_volume(RlvChatVolume::Whisper),
            RlvChatVolume::Normal
        );
        Ok(())
    }

    #[test]
    fn ordinary_chat_is_blanked_to_an_ellipsis() -> Result<(), TestError> {
        let state = state_of("@sendchat=n")?;
        let world = TestWorld::new();
        let filtered = state.actions(&world).filter_chat("hello there", true);
        assert_eq!(filtered.text, "...");
        assert!(filtered.blanked);
        Ok(())
    }

    #[test]
    fn ooc_chat_is_let_through_until_the_setting_says_otherwise() -> Result<(), TestError> {
        let state = state_of("@sendchat=n")?;
        let world = TestWorld::new();
        let filtered = state.actions(&world).filter_chat("((be right back))", true);
        assert_eq!(filtered.text, "((be right back))");
        assert!(!filtered.blanked);

        let strict = TestWorld::new().without(TestSetting::Ooc);
        assert!(
            state
                .actions(&strict)
                .filter_chat("((be right back))", true)
                .blanked
        );
        Ok(())
    }

    #[test]
    fn a_short_slash_command_survives_the_filter() -> Result<(), TestError> {
        let state = state_of("@sendchat=n")?;
        let world = TestWorld::new();
        assert!(!state.actions(&world).filter_chat("/hug", true).blanked);
        assert!(
            state
                .actions(&world)
                .filter_chat("/a rather long gesture", true)
                .blanked
        );
        Ok(())
    }

    #[test]
    fn an_emote_is_truncated_rather_than_blanked() -> Result<(), TestError> {
        let state = state_of("@sendchat=n")?;
        let world = TestWorld::new();
        let filtered = state
            .actions(&world)
            .filter_chat("/me waves at everybody in the room", true);
        assert_eq!(filtered.text, "/me waves at everybo");
        assert!(!filtered.blanked);

        // A full stop ends it sooner.
        let filtered = state
            .actions(&world)
            .filter_chat("/me waves. Then leaves.", true);
        assert_eq!(filtered.text, "/me waves.");
        Ok(())
    }

    #[test]
    fn emote_lets_a_long_emote_through_whole() -> Result<(), TestError> {
        let state = state_of("@sendchat=n,emote=add")?;
        let world = TestWorld::new();
        let text = "/me waves at everybody in the room";
        assert_eq!(state.actions(&world).filter_chat(text, true).text, text);
        Ok(())
    }

    #[test]
    fn an_emote_with_smuggled_punctuation_is_blanked() -> Result<(), TestError> {
        let state = state_of("@sendchat=n")?;
        let world = TestWorld::new();
        assert!(
            state
                .actions(&world)
                .filter_chat("/me says \"help me\"", true)
                .blanked
        );
        assert!(
            state
                .actions(&world)
                .filter_chat("/me waves - and leaves", true)
                .blanked
        );
        Ok(())
    }

    #[test]
    fn an_arriving_emote_is_never_shortened() -> Result<(), TestError> {
        let state = state_of("@recvchat=n")?;
        let world = TestWorld::new();
        let text = "/me waves at everybody in the room";
        let filtered = state.actions(&world).filter_chat(text, false);
        assert_eq!(filtered.text, text);
        assert!(!filtered.blanked);
        Ok(())
    }

    #[test]
    fn redirected_chat_goes_to_the_channel_it_named() -> Result<(), TestError> {
        let state = state_of("@redirchat:2222=n")?;
        let world = TestWorld::new();
        let replies = state
            .actions(&world)
            .redirect_chat("hello there")
            .ok_or("not redirected")?;
        let reply = replies.first().ok_or("no reply")?;
        assert_eq!(reply.channel, 2222);
        assert_eq!(reply.message, "hello there");
        Ok(())
    }

    #[test]
    fn a_short_emote_is_not_redirected_by_redirchat() -> Result<(), TestError> {
        // `@redirchat` only redirects what `@sendchat` would have swallowed,
        // and a clean emote survives the filter untouched.
        let state = state_of("@redirchat:2222=n")?;
        let world = TestWorld::new();
        assert_eq!(state.actions(&world).redirect_chat("/me waves"), None);
        Ok(())
    }

    #[test]
    fn rediremote_takes_the_emotes() -> Result<(), TestError> {
        let state = state_of("@rediremote:2222=n")?;
        let world = TestWorld::new();
        assert_eq!(state.actions(&world).redirect_chat("hello there"), None);
        let replies = state
            .actions(&world)
            .redirect_chat("/me waves")
            .ok_or("not redirected")?;
        assert_eq!(replies.len(), 1);
        Ok(())
    }

    #[test]
    fn a_redirect_to_a_blocked_channel_still_swallows_the_line() -> Result<(), TestError> {
        let state = state_of("@redirchat:2222=n,sendchannel=n")?;
        let world = TestWorld::new();
        let replies = state
            .actions(&world)
            .redirect_chat("hello there")
            .ok_or("not redirected")?;
        assert!(replies.is_empty());
        Ok(())
    }

    #[test]
    fn a_long_redirected_line_is_split_rather_than_cut() -> Result<(), TestError> {
        let state = state_of("@redirchat:2222=n")?;
        let long = "word ".repeat(300);

        // Truncated by default: one message, and the tail is simply lost.
        let world = TestWorld::new();
        let replies = state
            .actions(&world)
            .redirect_chat(&long)
            .ok_or("not redirected")?;
        assert_eq!(replies.len(), 1);

        // With the setting on, every word arrives — across several messages.
        let split = TestWorld::new().with(TestSetting::SplitRedirect);
        let replies = state
            .actions(&split)
            .redirect_chat(&long)
            .ok_or("not redirected")?;
        assert!(replies.len() > 1);
        assert!(replies.iter().all(|reply| reply.channel == 2222));
        Ok(())
    }

    #[test]
    fn the_outgoing_choke_point_clamps_then_filters() -> Result<(), TestError> {
        let state = state_of("@sendchat=n,chatnormal=n")?;
        let world = TestWorld::new();
        assert_eq!(
            state
                .actions(&world)
                .outgoing_chat(0, RlvChatVolume::Shout, "hello there"),
            RlvChatDecision::Send {
                channel: 0,
                volume: RlvChatVolume::Whisper,
                text: "...".to_owned(),
            }
        );
        Ok(())
    }

    #[test]
    fn the_debug_channel_counts_as_public_chat() -> Result<(), TestError> {
        let state = state_of("@sendchat=n")?;
        let world = TestWorld::new();
        let actions = state.actions(&world);
        assert_eq!(
            actions.outgoing_chat(CHAT_CHANNEL_DEBUG, RlvChatVolume::Normal, "hi"),
            RlvChatDecision::Blocked
        );
        // Any other channel is untouched by `@sendchat`.
        assert_eq!(
            actions.outgoing_chat(5, RlvChatVolume::Normal, "hi"),
            RlvChatDecision::Send {
                channel: 5,
                volume: RlvChatVolume::Normal,
                text: "hi".to_owned(),
            }
        );
        Ok(())
    }

    #[test]
    fn typing_is_hidden_while_chat_is_redirected() -> Result<(), TestError> {
        let state = state_of("@redirchat:2222=n")?;
        assert!(!state.actions(&TestWorld::new()).can_send_typing_start());
        let shown = TestWorld::new().with(TestSetting::ShowTyping);
        assert!(state.actions(&shown).can_send_typing_start());
        Ok(())
    }

    #[test]
    fn teleport_home_survives_tplm_alone() -> Result<(), TestError> {
        let world = TestWorld::new();
        let state = state_of("@tplm=n")?;
        assert!(state.actions(&world).can_teleport_via_landmark(true));
        assert!(!state.actions(&world).can_teleport_via_landmark(false));

        let both = state_of("@tplm=n,tploc=n")?;
        assert!(!both.actions(&world).can_teleport_via_landmark(true));
        Ok(())
    }

    #[test]
    fn a_locked_seat_blocks_every_teleport() -> Result<(), TestError> {
        let state = state_of("@unsit=n")?;
        let seated = TestWorld {
            sitting: true,
            ..TestWorld::new()
        };
        let actions = state.actions(&seated);
        assert!(!actions.can_teleport_via_landmark(true));
        assert!(!actions.can_teleport_to_location());
        assert!(!actions.can_teleport_to_local([1.0, 0.0, 0.0]));
        assert!(!actions.can_accept_teleport_offer(Uuid::from_u128(1)));
        Ok(())
    }

    #[test]
    fn tplocal_is_capped_at_one_region() -> Result<(), TestError> {
        // An object asking for a bigger radius than a region does not get one.
        let state = state_of("@tplocal:1000=n")?;
        let world = TestWorld::new();
        let actions = state.actions(&world);
        assert!(actions.can_teleport_to_local([200.0, 0.0, 0.0]));
        assert!(!actions.can_teleport_to_local([300.0, 0.0, 0.0]));
        Ok(())
    }

    #[test]
    fn tplocal_ignores_height() -> Result<(), TestError> {
        let state = state_of("@tplocal:10=n")?;
        let world = TestWorld::new();
        assert!(
            state
                .actions(&world)
                .can_teleport_to_local([0.0, 0.0, 500.0])
        );
        Ok(())
    }

    #[test]
    fn a_teleport_offer_exception_names_the_sender() -> Result<(), TestError> {
        let friend = Uuid::from_u128(0x1234);
        let state = state_of(&format!("@tplure=n,tplure:{friend}=add"))?;
        let world = TestWorld::new();
        assert!(state.actions(&world).can_accept_teleport_offer(friend));
        assert!(
            !state
                .actions(&world)
                .can_accept_teleport_offer(Uuid::from_u128(0x5678))
        );
        Ok(())
    }

    #[test]
    fn edit_reports_how_much_is_left() -> Result<(), TestError> {
        let world = TestWorld::new();
        let none = RlvState::new();
        assert!(none.actions(&world).can_edit(RlvCheckType::All));
        assert!(!none.actions(&world).can_edit(RlvCheckType::Nothing));

        let blanket = state_of("@edit=n")?;
        assert!(!blanket.actions(&world).can_edit(RlvCheckType::All));
        assert!(blanket.actions(&world).can_edit(RlvCheckType::Nothing));

        let target = Uuid::from_u128(0x2222);
        let with_exception = state_of(&format!("@edit=n,edit:{target}=add"))?;
        assert!(
            !with_exception
                .actions(&world)
                .can_edit(RlvCheckType::Nothing)
        );
        assert!(with_exception.actions(&world).can_edit(RlvCheckType::Some));
        Ok(())
    }

    #[test]
    fn editattach_and_editworld_split_by_object_kind() -> Result<(), TestError> {
        let state = state_of("@editattach=n")?;
        let world = TestWorld::new();
        let rezzed = RlvObject::world(Uuid::from_u128(2), [0.0; 3]);
        let worn = rezzed.of_kind(RlvObjectKind::AttachmentSelf);
        assert!(state.actions(&world).can_edit_object(&rezzed));
        assert!(!state.actions(&world).can_edit_object(&worn));
        Ok(())
    }

    #[test]
    fn interact_spares_the_hud() -> Result<(), TestError> {
        let state = state_of("@interact=n")?;
        let world = TestWorld::new();
        let rezzed = RlvObject::world(Uuid::from_u128(2), [0.0; 3]);
        let hud = rezzed.of_kind(RlvObjectKind::Hud);
        assert!(!state.actions(&world).can_interact(Some(&rezzed), [0.0; 3]));
        assert!(state.actions(&world).can_interact(Some(&hud), [0.0; 3]));
        // Nothing resolved yet is not a refusal.
        assert!(state.actions(&world).can_interact(None, [0.0; 3]));
        Ok(())
    }

    #[test]
    fn build_stays_open_while_half_of_it_works() -> Result<(), TestError> {
        let world = TestWorld::new();
        assert!(state_of("@edit=n")?.actions(&world).can_build());
        assert!(state_of("@rez=n")?.actions(&world).can_build());
        assert!(!state_of("@edit=n,rez=n")?.actions(&world).can_build());
        Ok(())
    }

    #[test]
    fn money_is_two_separate_restrictions() -> Result<(), TestError> {
        let world = TestWorld::new();
        let buy = state_of("@buy=n")?;
        assert!(!buy.actions(&world).can_buy_object());
        assert!(!buy.actions(&world).can_pay_object());
        assert!(buy.actions(&world).can_pay_avatar());

        let pay = state_of("@pay=n")?;
        assert!(!pay.actions(&world).can_pay_avatar());
        assert!(pay.actions(&world).can_buy_object());
        Ok(())
    }

    #[test]
    fn share_asks_two_questions() -> Result<(), TestError> {
        let friend = Uuid::from_u128(0x1234);
        let state = state_of(&format!("@share=n,share:{friend}=add"))?;
        let world = TestWorld::new();
        let actions = state.actions(&world);
        // The blanket button stays live because *somebody* can be given to.
        assert!(actions.can_give_inventory());
        assert!(actions.can_give_inventory_to(friend));
        assert!(!actions.can_give_inventory_to(Uuid::from_u128(0x5678)));
        Ok(())
    }

    #[test]
    fn setgroup_lets_its_own_holder_through() -> Result<(), TestError> {
        let state = state_of("@setgroup=n")?;
        let world = TestWorld::new();
        let actions = state.actions(&world);
        assert!(!actions.can_change_active_group(None));
        assert!(actions.can_change_active_group(Some(collar())));
        assert!(!actions.can_change_active_group(Some(Uuid::from_u128(9))));
        Ok(())
    }

    #[test]
    fn gestures_stop_with_sendgesture() -> Result<(), TestError> {
        let world = TestWorld::new();
        assert!(
            !state_of("@sendgesture=n")?
                .actions(&world)
                .can_play_gestures()
        );
        assert!(RlvState::new().actions(&world).can_play_gestures());
        Ok(())
    }

    #[test]
    fn an_unrestricted_state_allows_everything() -> Result<(), TestError> {
        let state = RlvState::new();
        let world = TestWorld::new();
        let actions = state.actions(&world);
        let object = RlvObject::world(Uuid::from_u128(2), [100.0, 0.0, 0.0]);
        assert!(actions.can_fly());
        assert!(actions.can_jump());
        assert!(actions.can_rez());
        assert!(actions.can_stand());
        assert!(actions.can_ground_sit());
        assert!(actions.can_sit(&object, [0.0; 3]));
        assert!(actions.can_touch(&object, [0.0; 3]));
        assert!(actions.can_interact(Some(&object), [0.0; 3]));
        assert!(actions.can_edit_object(&object));
        assert!(actions.can_send_channel(42));
        assert!(actions.can_send_im(Uuid::from_u128(1)));
        assert!(actions.can_start_im(Uuid::from_u128(1), true));
        assert!(actions.can_send_typing_start());
        assert!(actions.can_teleport_to_location());
        assert!(actions.can_teleport_to_local([1000.0, 0.0, 0.0]));
        assert!(actions.can_accept_teleport_request(Uuid::from_u128(1)));
        assert_eq!(actions.redirect_chat("hello"), None);
        Ok(())
    }
}
