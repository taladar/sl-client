//! The **receive-side choke point** — what a restriction lets *in*.
//!
//! The send side — the rest of [`RlvActions`] — refuses to issue what is
//! forbidden. This half is the mirror: chat and instant messages arrive
//! whether or not a restriction wants them, so the filter sits between the
//! wire and the display and decides what the user is allowed to see. The
//! reference puts both halves on the same `RlvActions` façade — `canReceiveIM`
//! sits three functions below `canSendIM` — and so do we, because a consumer
//! that already holds an [`RlvActions`] to ask whether it may *say* something
//! should not have to build a second thing to ask whether it may *hear* it.
//!
//! Four families live here:
//!
//! - **incoming chat** — `@recvchat` / `@recvchatfrom`, `@recvemote` /
//!   `@recvemotefrom`. What arrives from a blocked source is dropped, or
//!   replaced by an ellipsis when the user asked to be told that something was
//!   held back. The line itself goes through the very same filter the send
//!   side uses ([`RlvActions::filter_chat`]), called the other way round: an
//!   arriving emote is either heard or not, never shortened;
//! - **incoming IMs** — `@recvim` / `@recvimfrom` with their distance
//!   modifiers, plus the two group-and-conference cases the reference treats
//!   differently from a one-to-one message;
//! - **auto-accept** — `@accepttp`, `@accepttprequest` and
//!   `@acceptpermission`, which answer a dialog the user would otherwise have
//!   been asked. `@acceptpermission` shares its call site with the two
//!   permissions RLV *refuses* outright, so both verdicts come from one
//!   question here;
//! - **idling** — `@allowidle`, which stops the viewer flipping to Away
//!   behind the user's back.
//!
//! What is deliberately *not* here is the anonymisation layer — `@shownames`,
//! `@showloc` and friends rewrite what a name or a location reads as rather
//! than whether a message arrives, and every display surface has to pass
//! through it, not just chat.
//!
//! ```
//! use sl_rlv::{parse_chat_line, RlvActionSource, RlvChatKind, RlvChatSource,
//!              RlvIncomingChat, RlvState};
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
//! let (collar, stranger) = (Uuid::from_u128(1), Uuid::from_u128(2));
//! let mut state = RlvState::new();
//! state.apply(collar, parse_chat_line("@recvchat=n").unwrap()[0].as_ref().unwrap());
//!
//! // Nearby chat from another avatar is replaced by the ellipsis the user
//! // asked for, rather than silently vanishing.
//! let heard = state.actions(&Standing).incoming_chat(
//!     stranger, RlvChatSource::Agent, RlvChatKind::Nearby, "hello there",
//! );
//! assert_eq!(heard, RlvIncomingChat::Replace("...".to_owned()));
//!
//! // An emote is not chat: `@recvemote` is what would have stopped it.
//! let emoted = state.actions(&Standing).incoming_chat(
//!     stranger, RlvChatSource::Agent, RlvChatKind::Nearby, "/me waves",
//! );
//! assert_eq!(emoted, RlvIncomingChat::Show);
//! ```
//!
//! Reference (Firestorm, read-only): `rlvactions.cpp:172` (`canReceiveIM`),
//! `llviewermessage.cpp:2982-3020` (the incoming-chat filter),
//! `llimprocessing.cpp:1167` (a blocked IM) and `:1975` (auto-accepted
//! teleports), `llimview.cpp:5068` (group and conference invites),
//! `llviewermessage.cpp:7513-7534` and `rlvcommon.cpp:619`
//! (`@acceptpermission` and `RlvUtil::filterScriptQuestions`),
//! `llappviewer.cpp:531` / `llagent.cpp:3252` (`@allowidle`).

use uuid::Uuid;

use crate::actions::{RlvActionSource, RlvActions, RlvObject, is_emote};
use crate::behaviour::RlvBehaviour;
use crate::modifier::RlvModifier;
use crate::state::RlvExceptionOption;

/// The away timeout `@allowidle` imposes, in seconds — half an hour, long
/// enough that the viewer will not announce an idle user who was told to stay
/// put (`llappviewer.cpp:531`).
pub const ALLOWIDLE_AWAY_TIMEOUT_SECONDS: u32 = 30 * 60;

/// What the ellipsis a blocked emote is replaced with reads as
/// (`llviewermessage.cpp:3018`).
const BLOCKED_EMOTE: &str = "/me ...";

/// What said a line of incoming chat — the three facts the reference reads off
/// the chatter to decide whether the filter applies at all
/// (`CHAT_SOURCE_*` plus `permYouOwner` and `isAttachment`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RlvChatSource {
    /// Another avatar.
    Agent,
    /// This avatar. Its own chat is never filtered — a restriction that hid
    /// what the user just typed would only confuse them.
    OwnAgent,
    /// An object or somebody else's attachment.
    Object {
        /// Whether the agent owns it (`permYouOwner`).
        owned_by_agent: bool,
        /// Whether it is worn rather than rezzed (`isAttachment`).
        is_attachment: bool,
    },
    /// The viewer itself, which no restriction gags.
    System,
}

impl RlvChatSource {
    /// An object the viewer has not rezzed yet, which the reference sees as a
    /// null `LLViewerObject*` and therefore as neither owned nor worn — so its
    /// chat is filtered.
    #[must_use]
    pub const fn unknown_object() -> Self {
        Self::Object {
            owned_by_agent: false,
            is_attachment: false,
        }
    }

    /// Whether chat from this source is filtered at all when it is `kind`.
    ///
    /// An avatar's own words are always its own; an object's are exempt only
    /// when it is the agent's own attachment talking, or when it used
    /// `llOwnerSay` / `llRegionSayTo` — the two channels RLV itself travels
    /// on, which is why they must never be swallowed.
    const fn is_filtered(self, kind: RlvChatKind) -> bool {
        match self {
            Self::Agent => true,
            Self::OwnAgent | Self::System => false,
            Self::Object {
                owned_by_agent,
                is_attachment,
            } => {
                (!owned_by_agent || !is_attachment)
                    && !matches!(kind, RlvChatKind::OwnerSay | RlvChatKind::Direct)
            }
        }
    }
}

/// How a line of chat was said — the `EChatType` cases the incoming filter
/// tells apart (`llchat.h:41`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RlvChatKind {
    /// Ordinary nearby chat: whispered, said, shouted, or on the debug
    /// channel.
    Nearby,
    /// `llOwnerSay` — the channel RLV commands themselves arrive on.
    OwnerSay,
    /// `llRegionSayTo` addressed at this agent.
    Direct,
    /// A typing indicator starting or stopping, which carries no words to
    /// filter.
    Typing,
}

/// What the receive filter decided about one line of incoming chat.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RlvIncomingChat {
    /// Show it as it arrived. No restriction touched it.
    Show,
    /// Show this in its place — the ellipsis, or what the chat filter left.
    Replace(String),
    /// Show nothing at all: the line never happened as far as the user is
    /// concerned.
    Hide,
}

/// What the receive filter decided about one incoming instant message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RlvImDecision {
    /// Deliver it as it arrived.
    Show,
    /// Deliver it with its text replaced by the `blocked_recvim` string, so
    /// the user knows a conversation is happening they may not read.
    Censor {
        /// Whether the sender is told, with the `blocked_recvim_remote`
        /// string, that their message went nowhere. The reference skips this
        /// when the sender is muted, which is the consumer's fact to check.
        tell_sender: bool,
    },
    /// Drop it: no session is opened and nothing is shown.
    Hide,
}

/// Which kind of many-party session an invitation is for — the two cases the
/// reference treats differently (`llimview.cpp:5068`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RlvSessionKind {
    /// A group chat, addressed by the group's id.
    Group,
    /// An ad-hoc conference, addressed by the avatar who started it.
    Conference,
}

/// One of the three script permissions RLV has an opinion about
/// (`SCRIPT_PERMISSION_*`, `llviewermessage.cpp:7121`).
///
/// Every other permission a script may ask for is none of RLV's business and
/// reaches the user unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RlvScriptPermission {
    /// `PERMISSION_TAKE_CONTROLS` — drive the avatar's movement keys.
    TakeControls,
    /// `PERMISSION_ATTACH` — attach itself to the avatar.
    Attach,
    /// `PERMISSION_TELEPORT` — teleport the avatar.
    Teleport,
}

/// What happens to one permission a script asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RlvPermissionVerdict {
    /// Put it to the user, as usual.
    Ask,
    /// Grant it without asking. When nothing is left to ask about, the dialog
    /// itself never appears.
    Grant,
    /// Refuse it without asking, and tell the user which object was refused —
    /// the `blocked_permattach` / `blocked_permteleport` strings.
    Refuse,
}

impl<S: RlvActionSource + ?Sized> RlvActions<'_, S> {
    // ------------------------------------------------------------------- chat

    /// Whether public chat from `sender` may be heard (`@recvchat`,
    /// `@recvchatfrom`).
    ///
    /// `sender` is the avatar that spoke, or the *object* that did — object
    /// chat is exempted by what said it, not by who owns it, so an exception
    /// naming a noisy vendor is spelled with the vendor's own id.
    #[must_use]
    pub fn can_receive_chat(&self, sender: Uuid) -> bool {
        self.can_receive(RlvBehaviour::Recvchat, RlvBehaviour::Recvchatfrom, sender)
    }

    /// Whether an emote from `sender` may be heard (`@recvemote`,
    /// `@recvemotefrom`).
    #[must_use]
    pub fn can_receive_emote(&self, sender: Uuid) -> bool {
        self.can_receive(RlvBehaviour::Recvemote, RlvBehaviour::Recvemotefrom, sender)
    }

    /// The shared shape of the two receive pairs: a blanket restriction that
    /// an exception opens, and a targeted one that an exception closes
    /// (`llviewermessage.cpp:2999-3012`).
    fn can_receive(&self, blanket: RlvBehaviour, targeted: RlvBehaviour, sender: Uuid) -> bool {
        let option = RlvExceptionOption::Avatar(sender);
        !((self.state.has_behaviour(blanket) && !self.is_exception(blanket, option))
            || (self.state.has_behaviour(targeted) && self.is_exception(targeted, option)))
    }

    /// The whole incoming-chat decision, in the order the reference makes it
    /// (`llviewermessage.cpp:2982-3020`).
    ///
    /// This is the choke point a chat overlay calls and nothing else. A line
    /// that is not filtered at all — the user's own words, an owned
    /// attachment, `llOwnerSay`, a typing indicator — comes straight back as
    /// [`RlvIncomingChat::Show`], so the caller may hand everything it hears
    /// to this one method.
    ///
    /// Whether a blocked line leaves a visible `"..."` behind or vanishes
    /// outright is the user's `RestrainedLoveShowEllipsis` choice, read from
    /// the [`RlvActionSource`]; the reference makes it for chat and emotes
    /// alike.
    #[must_use]
    pub fn incoming_chat(
        &self,
        sender: Uuid,
        source: RlvChatSource,
        kind: RlvChatKind,
        text: &str,
    ) -> RlvIncomingChat {
        if matches!(kind, RlvChatKind::Typing) || !source.is_filtered(kind) {
            return RlvIncomingChat::Show;
        }

        if is_emote(text) {
            if self.can_receive_emote(sender) {
                RlvIncomingChat::Show
            } else if self.source.show_ellipsis() {
                RlvIncomingChat::Replace(BLOCKED_EMOTE.to_owned())
            } else {
                RlvIncomingChat::Hide
            }
        } else if self.can_receive_chat(sender) {
            RlvIncomingChat::Show
        } else {
            // The send-side filter, run the other way round: an arriving emote
            // is never shortened, and a `/`-prefixed line or an `((OOC))`
            // aside survives here for the same reasons it does going out.
            let filtered = self.filter_chat(text, false);
            if filtered.blanked && !self.source.show_ellipsis() {
                RlvIncomingChat::Hide
            } else {
                RlvIncomingChat::Replace(filtered.text)
            }
        }
    }

    // --------------------------------------------------------------------- IM

    /// Whether an IM from `sender` may be read — an avatar or a group
    /// (`@recvim`, `@recvimfrom`, `RlvActions::canReceiveIM`).
    ///
    /// The distance pair is an exclusion range, the same hole in the block
    /// that [`RlvActions::can_send_im`] honours: `@recvim=n` with a
    /// `RecvIMDistMin` of ten metres lets through exactly the avatars further
    /// away than that, and one this viewer cannot see counts as infinitely
    /// far.
    #[must_use]
    pub fn can_receive_im(&self, sender: Uuid) -> bool {
        let option = RlvExceptionOption::Avatar(sender);
        (!self.state.has_behaviour(RlvBehaviour::Recvim)
            || self.is_exception(RlvBehaviour::Recvim, option)
            || self.within_im_range(
                sender,
                RlvModifier::RecvImDistMin,
                RlvModifier::RecvImDistMax,
            ))
            && (!self.state.has_behaviour(RlvBehaviour::Recvimfrom)
                || !self.is_exception(RlvBehaviour::Recvimfrom, option))
    }

    /// What becomes of one incoming one-to-one IM (`llimprocessing.cpp:1167`).
    ///
    /// A blocked message is **censored, not dropped**: the conversation still
    /// appears, with the `blocked_recvim` string where the words were, because
    /// a user who cannot read their IMs should still know they have them. The
    /// sender is told the same, so they do not talk to a wall.
    ///
    /// `offline` is an IM that was stored and delivered late, and `exempt` is
    /// a sender the viewer never blocks — the reference exempts Lindens, on
    /// the grounds that a restriction must not cut the user off from support.
    #[must_use]
    pub fn incoming_im(&self, sender: Uuid, offline: bool, exempt: bool) -> RlvImDecision {
        if offline || exempt || self.can_receive_im(sender) {
            RlvImDecision::Show
        } else {
            RlvImDecision::Censor { tell_sender: true }
        }
    }

    /// What becomes of an invitation to a group chat or a conference
    /// (`llimview.cpp:5068`).
    ///
    /// The two are not treated alike. A group session is addressed by the
    /// group's own id, so a restriction can be lifted for one group by naming
    /// it, and an invitation that is not excepted is **declined outright** —
    /// joining a session the user may not read would leak their presence into
    /// it. A conference is addressed by whoever started it, and is only
    /// censored, because the other participants are already talking.
    #[must_use]
    pub fn incoming_session_invite(
        &self,
        session: Uuid,
        sender: Uuid,
        kind: RlvSessionKind,
    ) -> RlvImDecision {
        if !self.state.has_behaviour(RlvBehaviour::Recvim)
            && !self.state.has_behaviour(RlvBehaviour::Recvimfrom)
        {
            return RlvImDecision::Show;
        }
        match kind {
            RlvSessionKind::Group => {
                if self.can_receive_im(session) {
                    RlvImDecision::Show
                } else {
                    RlvImDecision::Hide
                }
            }
            RlvSessionKind::Conference => {
                if self.can_receive_im(sender) {
                    RlvImDecision::Show
                } else {
                    RlvImDecision::Censor { tell_sender: false }
                }
            }
        }
    }

    // ------------------------------------------------------------ auto-accept

    /// Whether a teleport offer from `sender` is accepted without asking
    /// (`@accepttp`, `RlvActions::autoAcceptTeleportOffer`).
    ///
    /// This overrides Do Not Disturb and a "reject all offers" preference
    /// alike: an object that holds `@accepttp` is entitled to move the agent,
    /// and the user's own settings are not a way out of it. Naming an avatar
    /// — `@accepttp:<uuid>=add` — narrows that to one person's offers.
    ///
    /// Note that this is not the same question as
    /// [`RlvActions::can_accept_teleport_offer`], which is whether the offer
    /// may be accepted *at all*; an offer can be acceptable without being
    /// automatic, and — since the two behaviours are separate — automatic
    /// without being acceptable.
    #[must_use]
    pub fn auto_accept_teleport_offer(&self, sender: Uuid) -> bool {
        self.auto_accept(RlvBehaviour::Accepttp, sender)
    }

    /// Whether a teleport *request* from `requester` — "may I come to you?" —
    /// is answered without asking (`@accepttprequest`).
    #[must_use]
    pub fn auto_accept_teleport_request(&self, requester: Uuid) -> bool {
        self.auto_accept(RlvBehaviour::Accepttprequest, requester)
    }

    /// The shape both auto-accepts share: in force for everybody, or granted
    /// for this one avatar (`rlvactions.cpp:323`).
    ///
    /// The nil id is nobody, and never matches an exception — the reference
    /// guards on that because it reaches this with ids read back out of
    /// notification payloads.
    fn auto_accept(&self, behaviour: RlvBehaviour, avatar: Uuid) -> bool {
        (!avatar.is_nil() && self.is_exception(behaviour, RlvExceptionOption::Avatar(avatar)))
            || self.state.has_behaviour(behaviour)
    }

    /// What happens to one permission a script asked the user for
    /// (`RlvUtil::filterScriptQuestions`, `rlvcommon.cpp:619`;
    /// `@acceptpermission`, `llviewermessage.cpp:7517`).
    ///
    /// Refusing comes first and granting second, which is the order the
    /// reference applies them in and the only order that is safe: a
    /// `@acceptpermission` must not hand out the very permission an
    /// attachment lock exists to withhold.
    ///
    /// The two refusals close loopholes rather than add restrictions.
    /// A script that could attach itself would walk around
    /// [`RlvLocks`](crate::RlvLocks), and one granted `PERMISSION_TELEPORT`
    /// would walk around `@tploc`.
    ///
    /// `@acceptpermission` then answers what is left. Taking controls is
    /// always granted — that is the point of the behaviour — while attaching
    /// is granted only for a rezzed object the agent owns, because an object
    /// belonging to somebody else, or one already worn, is not something the
    /// user should find attached without having agreed to it.
    #[must_use]
    pub fn script_permission(
        &self,
        permission: RlvScriptPermission,
        object: &RlvObject,
    ) -> RlvPermissionVerdict {
        let accepts = self.state.has_behaviour(RlvBehaviour::Acceptpermission);
        match permission {
            RlvScriptPermission::Attach => {
                if !self.state.locks().can_attach_anywhere() {
                    RlvPermissionVerdict::Refuse
                } else if accepts && object.is_owned_by_agent() && !object.kind.is_attachment() {
                    RlvPermissionVerdict::Grant
                } else {
                    RlvPermissionVerdict::Ask
                }
            }
            RlvScriptPermission::Teleport => {
                if self.state.has_behaviour(RlvBehaviour::Tploc) {
                    RlvPermissionVerdict::Refuse
                } else {
                    RlvPermissionVerdict::Ask
                }
            }
            RlvScriptPermission::TakeControls => {
                if accepts {
                    RlvPermissionVerdict::Grant
                } else {
                    RlvPermissionVerdict::Ask
                }
            }
        }
    }

    /// Whether the user is told in chat what a script was granted, even though
    /// no permission it asked for was one the viewer cautions about
    /// (`llviewermessage.cpp:7531`).
    ///
    /// `@acceptpermission` answers dialogs silently, which is fine for the
    /// user's own things. Somebody else's object taking the agent's controls
    /// without a word is not, so that one is announced.
    #[must_use]
    pub fn notify_script_permission(&self, object: &RlvObject) -> bool {
        self.state.has_behaviour(RlvBehaviour::Acceptpermission) && !object.is_owned_by_agent()
    }

    // ----------------------------------------------------------------- idling

    /// How long the agent may be idle before the viewer sets it Away, given
    /// the `AFKTimeout` the user configured (`llappviewer.cpp:531`).
    ///
    /// `@allowidle` does not switch the away state off; it pushes the timeout
    /// out to [`ALLOWIDLE_AWAY_TIMEOUT_SECONDS`], so an agent left standing
    /// where it was told to stand does not advertise that nobody is watching.
    #[must_use]
    pub fn away_timeout_seconds(&self, configured: u32) -> u32 {
        if self.state.has_behaviour(RlvBehaviour::Allowidle) {
            ALLOWIDLE_AWAY_TIMEOUT_SECONDS
        } else {
            configured
        }
    }

    /// Whether the away state is cleared when the away animation stops
    /// (`llagent.cpp:3252`).
    ///
    /// Ordinarily the animation ending means the user came back. Under
    /// `@allowidle` it means nothing of the sort — the animation may have been
    /// stopped by whatever else is animating the agent — so the away state
    /// stays until something else clears it.
    #[must_use]
    pub fn clears_away_on_animation_stop(&self) -> bool {
        !self.state.has_behaviour(RlvBehaviour::Allowidle)
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use uuid::Uuid;

    use super::{
        ALLOWIDLE_AWAY_TIMEOUT_SECONDS, RlvChatKind, RlvChatSource, RlvImDecision, RlvIncomingChat,
        RlvPermissionVerdict, RlvScriptPermission, RlvSessionKind,
    };
    use crate::actions::{RlvActionSource, RlvCurrentCommand, RlvObject, RlvObjectKind};
    use crate::state::RlvState;
    use crate::{RlvParseError, parse_chat_line};

    /// What a test can go wrong with.
    #[derive(Debug, thiserror::Error)]
    enum TestError {
        /// A command a test typed did not decode.
        #[error("parsing an RLV command: {0}")]
        Parse(#[from] RlvParseError),
        /// A line a test typed was not an RLV line at all.
        #[error("not an RLV line: {0}")]
        NotRlv(String),
    }

    /// The collar every test's restrictions come from.
    fn collar() -> Uuid {
        Uuid::from_u128(0xc0_11a2)
    }

    /// A state holding everything `line` says, all of it from [`collar`].
    fn state_of(line: &str) -> Result<RlvState, TestError> {
        let mut state = RlvState::new();
        let commands = parse_chat_line(line).ok_or_else(|| TestError::NotRlv(line.to_owned()))?;
        for command in commands {
            state.apply(collar(), &command?);
        }
        Ok(state)
    }

    /// A world where the agent stands at the origin, sees nobody, and has no
    /// conversation open.
    #[derive(Debug, Default)]
    struct World {
        /// Where the avatars this viewer can see are.
        avatars: Vec<(Uuid, [f64; 3])>,
        /// Whether a blocked line leaves a visible `"..."` behind.
        show_ellipsis: bool,
    }

    impl World {
        /// The reference's default: a blocked line is replaced by an ellipsis.
        fn showing_ellipsis() -> Self {
            Self {
                show_ellipsis: true,
                ..Self::default()
            }
        }

        /// The same world, but with `avatar` standing at `position`.
        fn with_avatar(mut self, avatar: Uuid, position: [f64; 3]) -> Self {
            self.avatars.push((avatar, position));
            self
        }
    }

    impl RlvActionSource for World {
        fn agent_position(&self) -> [f64; 3] {
            [0.0, 0.0, 0.0]
        }

        fn avatar_position(&self, avatar: Uuid) -> Option<[f64; 3]> {
            self.avatars
                .iter()
                .find(|&&(id, _)| id == avatar)
                .map(|&(_, position)| position)
        }

        fn object_root(&self, object: Uuid) -> Uuid {
            object
        }

        fn is_sitting(&self) -> bool {
            false
        }

        fn has_open_session(&self, _id: Uuid) -> bool {
            false
        }

        fn current_command(&self) -> Option<RlvCurrentCommand> {
            None
        }

        fn show_ellipsis(&self) -> bool {
            self.show_ellipsis
        }
    }

    #[test]
    fn nothing_restricted_lets_every_line_through() -> Result<(), TestError> {
        let state = RlvState::new();
        let world = World::showing_ellipsis();
        let actions = state.actions(&world);
        let stranger = Uuid::from_u128(2);
        assert!(actions.can_receive_chat(stranger));
        assert!(actions.can_receive_emote(stranger));
        assert!(actions.can_receive_im(stranger));
        assert_eq!(
            actions.incoming_chat(stranger, RlvChatSource::Agent, RlvChatKind::Nearby, "hi"),
            RlvIncomingChat::Show
        );
        assert_eq!(
            actions.incoming_im(stranger, false, false),
            RlvImDecision::Show
        );
        Ok(())
    }

    #[test]
    fn blocked_chat_becomes_an_ellipsis() -> Result<(), TestError> {
        let state = state_of("@recvchat=n")?;
        let world = World::showing_ellipsis();
        assert_eq!(
            state.actions(&world).incoming_chat(
                Uuid::from_u128(2),
                RlvChatSource::Agent,
                RlvChatKind::Nearby,
                "hello there",
            ),
            RlvIncomingChat::Replace("...".to_owned())
        );
        Ok(())
    }

    #[test]
    fn blocked_chat_vanishes_without_the_ellipsis_setting() -> Result<(), TestError> {
        let state = state_of("@recvchat=n")?;
        let world = World::default();
        assert_eq!(
            state.actions(&world).incoming_chat(
                Uuid::from_u128(2),
                RlvChatSource::Agent,
                RlvChatKind::Nearby,
                "hello there",
            ),
            RlvIncomingChat::Hide
        );
        Ok(())
    }

    #[test]
    fn an_exception_is_still_heard() -> Result<(), TestError> {
        let friend = Uuid::from_u128(0xf2_1e0d);
        let state = state_of(&format!("@recvchat=n,recvchat:{friend}=add"))?;
        let world = World::showing_ellipsis();
        let actions = state.actions(&world);
        assert!(actions.can_receive_chat(friend));
        assert!(!actions.can_receive_chat(Uuid::from_u128(2)));
        Ok(())
    }

    #[test]
    fn recvchatfrom_blocks_only_who_it_names() -> Result<(), TestError> {
        let bore = Uuid::from_u128(0xb0_2e);
        let state = state_of(&format!("@recvchatfrom:{bore}=n"))?;
        let world = World::showing_ellipsis();
        let actions = state.actions(&world);
        assert!(!actions.can_receive_chat(bore));
        assert!(actions.can_receive_chat(Uuid::from_u128(2)));
        Ok(())
    }

    #[test]
    fn an_emote_is_stopped_by_recvemote_not_recvchat() -> Result<(), TestError> {
        let stranger = Uuid::from_u128(2);
        let world = World::showing_ellipsis();

        let chat_blocked = state_of("@recvchat=n")?;
        assert_eq!(
            chat_blocked.actions(&world).incoming_chat(
                stranger,
                RlvChatSource::Agent,
                RlvChatKind::Nearby,
                "/me waves",
            ),
            RlvIncomingChat::Show
        );

        let emote_blocked = state_of("@recvemote=n")?;
        assert_eq!(
            emote_blocked.actions(&world).incoming_chat(
                stranger,
                RlvChatSource::Agent,
                RlvChatKind::Nearby,
                "/me waves",
            ),
            RlvIncomingChat::Replace("/me ...".to_owned())
        );
        Ok(())
    }

    #[test]
    fn an_arriving_emote_is_never_shortened() -> Result<(), TestError> {
        // Going out, `@sendchat` would cut this to its first sentence. Coming
        // in, `@recvchat` has no say over an emote at all.
        let state = state_of("@recvchat=n")?;
        let world = World::showing_ellipsis();
        assert_eq!(
            state.actions(&world).incoming_chat(
                Uuid::from_u128(2),
                RlvChatSource::Agent,
                RlvChatKind::Nearby,
                "/me waves at everybody in the room. Twice.",
            ),
            RlvIncomingChat::Show
        );
        Ok(())
    }

    #[test]
    fn a_short_slash_command_survives_the_receive_filter() -> Result<(), TestError> {
        let state = state_of("@recvchat=n")?;
        let world = World::showing_ellipsis();
        assert_eq!(
            state.actions(&world).incoming_chat(
                Uuid::from_u128(2),
                RlvChatSource::Agent,
                RlvChatKind::Nearby,
                "/hug",
            ),
            RlvIncomingChat::Replace("/hug".to_owned())
        );
        Ok(())
    }

    #[test]
    fn the_agents_own_chat_is_never_filtered() -> Result<(), TestError> {
        let state = state_of("@recvchat=n")?;
        let world = World::showing_ellipsis();
        assert_eq!(
            state.actions(&world).incoming_chat(
                Uuid::from_u128(1),
                RlvChatSource::OwnAgent,
                RlvChatKind::Nearby,
                "hello there",
            ),
            RlvIncomingChat::Show
        );
        Ok(())
    }

    #[test]
    fn an_owned_attachment_talks_but_a_rezzed_object_does_not() -> Result<(), TestError> {
        let state = state_of("@recvchat=n")?;
        let world = World::showing_ellipsis();
        let actions = state.actions(&world);
        let object = Uuid::from_u128(0x0b1e);

        let hud = RlvChatSource::Object {
            owned_by_agent: true,
            is_attachment: true,
        };
        assert_eq!(
            actions.incoming_chat(object, hud, RlvChatKind::Nearby, "hello there"),
            RlvIncomingChat::Show
        );

        let vendor = RlvChatSource::Object {
            owned_by_agent: false,
            is_attachment: false,
        };
        assert_eq!(
            actions.incoming_chat(object, vendor, RlvChatKind::Nearby, "hello there"),
            RlvIncomingChat::Replace("...".to_owned())
        );

        // An object the viewer has not rezzed yet is neither owned nor worn.
        assert_eq!(
            actions.incoming_chat(
                object,
                RlvChatSource::unknown_object(),
                RlvChatKind::Nearby,
                "hello there",
            ),
            RlvIncomingChat::Replace("...".to_owned())
        );
        Ok(())
    }

    #[test]
    fn the_channels_rlv_travels_on_are_never_swallowed() -> Result<(), TestError> {
        let state = state_of("@recvchat=n")?;
        let world = World::showing_ellipsis();
        let actions = state.actions(&world);
        let object = Uuid::from_u128(0x0b1e);
        let vendor = RlvChatSource::Object {
            owned_by_agent: false,
            is_attachment: false,
        };
        for kind in [
            RlvChatKind::OwnerSay,
            RlvChatKind::Direct,
            RlvChatKind::Typing,
        ] {
            assert_eq!(
                actions.incoming_chat(object, vendor, kind, "@version=2222"),
                RlvIncomingChat::Show,
                "{kind:?} must reach the command parser"
            );
        }
        Ok(())
    }

    #[test]
    fn a_typing_indicator_is_never_filtered() -> Result<(), TestError> {
        let state = state_of("@recvchat=n")?;
        let world = World::showing_ellipsis();
        assert_eq!(
            state.actions(&world).incoming_chat(
                Uuid::from_u128(2),
                RlvChatSource::Agent,
                RlvChatKind::Typing,
                "",
            ),
            RlvIncomingChat::Show
        );
        Ok(())
    }

    #[test]
    fn a_blocked_im_is_censored_and_the_sender_told() -> Result<(), TestError> {
        let state = state_of("@recvim=n")?;
        let world = World::showing_ellipsis();
        let stranger = Uuid::from_u128(2);
        assert_eq!(
            state.actions(&world).incoming_im(stranger, false, false),
            RlvImDecision::Censor { tell_sender: true }
        );
        Ok(())
    }

    #[test]
    fn an_offline_or_exempt_im_is_never_blocked() -> Result<(), TestError> {
        let state = state_of("@recvim=n")?;
        let world = World::showing_ellipsis();
        let actions = state.actions(&world);
        let stranger = Uuid::from_u128(2);
        assert_eq!(
            actions.incoming_im(stranger, true, false),
            RlvImDecision::Show
        );
        assert_eq!(
            actions.incoming_im(stranger, false, true),
            RlvImDecision::Show
        );
        Ok(())
    }

    #[test]
    fn recvimfrom_blocks_what_recvim_would_allow() -> Result<(), TestError> {
        let bore = Uuid::from_u128(0xb0_2e);
        let state = state_of(&format!("@recvimfrom:{bore}=n"))?;
        let world = World::showing_ellipsis();
        let actions = state.actions(&world);
        assert!(!actions.can_receive_im(bore));
        assert!(actions.can_receive_im(Uuid::from_u128(2)));
        Ok(())
    }

    #[test]
    fn the_recvim_range_is_a_hole_in_the_block() -> Result<(), TestError> {
        // The range is a modifier, not a restriction of its own: the block
        // comes from `@recvim=n` and the range punches a hole in it, letting
        // through everybody more than ten metres away.
        let (near, far) = (Uuid::from_u128(2), Uuid::from_u128(3));
        let state = state_of("@recvim=n,recvim:10=n")?;
        let world = World::showing_ellipsis()
            .with_avatar(near, [5.0, 0.0, 0.0])
            .with_avatar(far, [50.0, 0.0, 0.0]);
        let actions = state.actions(&world);
        assert!(!actions.can_receive_im(near));
        assert!(actions.can_receive_im(far));
        // An avatar this viewer cannot see is infinitely far away, so an
        // open-ended hole covers it too.
        assert!(actions.can_receive_im(Uuid::from_u128(4)));

        // Give the hole a ceiling and the unseen avatar falls out of it again.
        let bounded = state_of("@recvim=n,recvim:10;100=n")?;
        let actions = bounded.actions(&world);
        assert!(actions.can_receive_im(far));
        assert!(!actions.can_receive_im(Uuid::from_u128(4)));
        Ok(())
    }

    #[test]
    fn a_group_invite_is_declined_and_a_conference_censored() -> Result<(), TestError> {
        let (group, sender) = (Uuid::from_u128(0x9207), Uuid::from_u128(2));
        let state = state_of("@recvim=n")?;
        let world = World::showing_ellipsis();
        let actions = state.actions(&world);
        assert_eq!(
            actions.incoming_session_invite(group, sender, RlvSessionKind::Group),
            RlvImDecision::Hide
        );
        assert_eq!(
            actions.incoming_session_invite(group, sender, RlvSessionKind::Conference),
            RlvImDecision::Censor { tell_sender: false }
        );
        Ok(())
    }

    #[test]
    fn a_group_named_as_an_exception_may_still_be_joined() -> Result<(), TestError> {
        let (group, sender) = (Uuid::from_u128(0x9207), Uuid::from_u128(2));
        let state = state_of(&format!("@recvim=n,recvim:{group}=add"))?;
        let world = World::showing_ellipsis();
        assert_eq!(
            state
                .actions(&world)
                .incoming_session_invite(group, sender, RlvSessionKind::Group),
            RlvImDecision::Show
        );
        Ok(())
    }

    #[test]
    fn an_invite_is_untouched_while_no_recvim_restriction_is_held() -> Result<(), TestError> {
        // `@recvimfrom` alone is enough to make the check run; nothing at all
        // means the invite never reaches it.
        let (group, sender) = (Uuid::from_u128(0x9207), Uuid::from_u128(2));
        let state = state_of("@sendim=n")?;
        let world = World::showing_ellipsis();
        assert_eq!(
            state
                .actions(&world)
                .incoming_session_invite(group, sender, RlvSessionKind::Group),
            RlvImDecision::Show
        );
        Ok(())
    }

    #[test]
    fn accepttp_answers_for_everybody_or_for_one() -> Result<(), TestError> {
        let (friend, stranger) = (Uuid::from_u128(0xf2_1e0d), Uuid::from_u128(2));

        let blanket = state_of("@accepttp=n")?;
        let world = World::showing_ellipsis();
        assert!(blanket.actions(&world).auto_accept_teleport_offer(stranger));

        let named = state_of(&format!("@accepttp:{friend}=add"))?;
        let actions = named.actions(&world);
        assert!(actions.auto_accept_teleport_offer(friend));
        assert!(!actions.auto_accept_teleport_offer(stranger));
        // Nobody is not somebody named as an exception.
        assert!(!actions.auto_accept_teleport_offer(Uuid::nil()));
        Ok(())
    }

    #[test]
    fn accepttprequest_is_a_separate_answer() -> Result<(), TestError> {
        let stranger = Uuid::from_u128(2);
        let state = state_of("@accepttp=n")?;
        let world = World::showing_ellipsis();
        let actions = state.actions(&world);
        assert!(actions.auto_accept_teleport_offer(stranger));
        assert!(!actions.auto_accept_teleport_request(stranger));
        Ok(())
    }

    #[test]
    fn acceptpermission_grants_controls_and_owned_attachments() -> Result<(), TestError> {
        let state = state_of("@acceptpermission=n")?;
        let world = World::showing_ellipsis();
        let actions = state.actions(&world);
        let mine = RlvObject::world(Uuid::from_u128(2), [0.0; 3]).owned_by_agent();
        let theirs = RlvObject::world(Uuid::from_u128(3), [0.0; 3]);
        let worn =
            RlvObject::world(Uuid::from_u128(4), [0.0; 3]).of_kind(RlvObjectKind::AttachmentSelf);

        assert_eq!(
            actions.script_permission(RlvScriptPermission::TakeControls, &theirs),
            RlvPermissionVerdict::Grant
        );
        assert_eq!(
            actions.script_permission(RlvScriptPermission::Attach, &mine),
            RlvPermissionVerdict::Grant
        );
        // Somebody else's object, and one already worn, are still asked about.
        assert_eq!(
            actions.script_permission(RlvScriptPermission::Attach, &theirs),
            RlvPermissionVerdict::Ask
        );
        assert_eq!(
            actions.script_permission(RlvScriptPermission::Attach, &worn),
            RlvPermissionVerdict::Ask
        );
        Ok(())
    }

    #[test]
    fn nothing_is_answered_without_acceptpermission() -> Result<(), TestError> {
        let state = RlvState::new();
        let world = World::showing_ellipsis();
        let actions = state.actions(&world);
        let mine = RlvObject::world(Uuid::from_u128(2), [0.0; 3]).owned_by_agent();
        for permission in [
            RlvScriptPermission::TakeControls,
            RlvScriptPermission::Attach,
            RlvScriptPermission::Teleport,
        ] {
            assert_eq!(
                actions.script_permission(permission, &mine),
                RlvPermissionVerdict::Ask,
                "{permission:?} is nobody's business unopposed"
            );
        }
        assert!(!actions.notify_script_permission(&mine));
        Ok(())
    }

    #[test]
    fn a_lock_refuses_what_acceptpermission_would_have_granted() -> Result<(), TestError> {
        // Every attachment point add-locked: nothing may be attached at all,
        // so the script is refused rather than quietly granted.
        let state = state_of("@addattach=n,acceptpermission=n")?;
        let world = World::showing_ellipsis();
        let mine = RlvObject::world(Uuid::from_u128(2), [0.0; 3]).owned_by_agent();
        assert_eq!(
            state
                .actions(&world)
                .script_permission(RlvScriptPermission::Attach, &mine),
            RlvPermissionVerdict::Refuse
        );
        Ok(())
    }

    #[test]
    fn tploc_refuses_a_scripted_teleport() -> Result<(), TestError> {
        let state = state_of("@tploc=n")?;
        let world = World::showing_ellipsis();
        let theirs = RlvObject::world(Uuid::from_u128(2), [0.0; 3]);
        assert_eq!(
            state
                .actions(&world)
                .script_permission(RlvScriptPermission::Teleport, &theirs),
            RlvPermissionVerdict::Refuse
        );
        Ok(())
    }

    #[test]
    fn somebody_elses_object_taking_controls_is_announced() -> Result<(), TestError> {
        let state = state_of("@acceptpermission=n")?;
        let world = World::showing_ellipsis();
        let actions = state.actions(&world);
        let mine = RlvObject::world(Uuid::from_u128(2), [0.0; 3]).owned_by_agent();
        let theirs = RlvObject::world(Uuid::from_u128(3), [0.0; 3]);
        assert!(actions.notify_script_permission(&theirs));
        assert!(!actions.notify_script_permission(&mine));
        Ok(())
    }

    #[test]
    fn allowidle_stretches_the_away_timeout() -> Result<(), TestError> {
        let world = World::showing_ellipsis();

        let unrestricted = RlvState::new();
        let actions = unrestricted.actions(&world);
        assert_eq!(actions.away_timeout_seconds(300), 300);
        assert!(actions.clears_away_on_animation_stop());

        let idle = state_of("@allowidle=n")?;
        let actions = idle.actions(&world);
        assert_eq!(
            actions.away_timeout_seconds(300),
            ALLOWIDLE_AWAY_TIMEOUT_SECONDS
        );
        assert!(!actions.clears_away_on_animation_stop());
        Ok(())
    }
}
