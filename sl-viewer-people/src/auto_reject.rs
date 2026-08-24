//! **Auto-reject modes** (`viewer-auto-reject-offers`): the standing policies
//! that answer a whole *class* of incoming offer before it ever reaches the
//! screen — reject every teleport offer, every friendship request, every group
//! invite, and ignore every ad-hoc conference.
//!
//! # The modes
//!
//! Each is a persisted per-account flag, toggled from Comm ▸ Online Status
//! beside the presence modes ([`crate::presence`]) and, unlike them, purely
//! **local**: the grid is told nothing, the offer is simply answered by the
//! viewer instead of by the user.
//!
//! - **Reject teleport offers and requests**
//!   ([`SETTING_REJECT_TELEPORT_OFFERS`]), optionally sparing friends
//!   ([`SETTING_DONT_REJECT_TELEPORT_FROM_FRIENDS`]), with its own canned reply
//!   ([`SETTING_REJECT_TELEPORT_RESPONSE`]).
//! - **Reject all friendship requests**
//!   ([`SETTING_REJECT_FRIENDSHIP_REQUESTS`]) with its reply
//!   ([`SETTING_REJECT_FRIENDSHIP_RESPONSE`]).
//! - **Reject all group invites** ([`SETTING_REJECT_ALL_GROUP_INVITES`]) — no
//!   reply: a group invitation is sent by the *group*, and a canned IM back to
//!   the inviter is not part of the reference's behaviour either.
//! - **Ignore ad-hoc conferences** ([`SETTING_IGNORE_AD_HOC_SESSIONS`]),
//!   optionally sparing friends ([`SETTING_DONT_IGNORE_AD_HOC_FROM_FRIENDS`]) —
//!   a conference invite is declined and its tab never opens.
//!
//! Alongside them sits one narrower suppression that is not a mode at all:
//! [`SETTING_SHOW_JOINED_GROUP_INVITATIONS`] (default **off**, the reference's
//! `FSShowJoinedGroupInvitations`) drops an invitation to a group the agent is
//! *already in* — a common re-invite that can only ever be noise.
//!
//! # What a rejection does
//!
//! `reject_for` is the whole decision, a pure function of the mode flags and
//! two facts about the offer (is the sender a friend, is the group one we are
//! already in). Its caller — the offers host ([`crate::offers_invites`]) — then:
//!
//! 1. sends the mode's canned reply, if it has one and the user has not blanked
//!    it, as an [`ImDialog::DoNotDisturbAutoResponse`](sl_client_bevy::ImDialog)
//!    IM ([`Command::AutoResponse`](sl_client_bevy::Command)), the same envelope
//!    the presence replies use, so the sender's viewer marks it automatic;
//! 2. **declines the offer on the wire** — and this is a deliberate departure
//!    from the reference, which sends the canned reply and then simply *drops*
//!    the offer. Leaving it unanswered leaves the sender's request pending on
//!    the simulator for nothing; declining it is what the user's Decline button
//!    would have done, and the sender learns no more than the canned reply
//!    already tells them;
//! 3. raises no card, queues nothing, and logs the rejection so the user can see
//!    in the log what their mode swallowed.
//!
//! Reference (Firestorm, read-only): `llagent.cpp`
//! (`selectRejectTeleportOffers` / `selectRejectFriendshipRequests` /
//! `selectRejectAllGroupInvites`), `llimprocessing.cpp` (the `IM_LURE_USER` /
//! `IM_FRIENDSHIP_OFFERED` / `IM_GROUP_INVITATION` arms),
//! `llviewermessage.cpp` (`send_rejecting_tp_offers_message`,
//! `send_rejecting_friendship_requests_message`), `llimview.cpp` (the
//! `FSIgnoreAdHocSessions` leave), `menu_viewer.xml` (Comm ▸ Online Status).

use sl_settings::SettingValue;

use crate::notifications::ShowNotification;
use crate::presence::PRESENCE_SECTION;
use crate::settings::ViewerSettings;
use crate::world_api::SETTING_AUTORESPONSE_ITEM;

/// Whether incoming teleport offers and requests are rejected unanswered (the
/// reference `FSRejectTeleportOffersMode`). Account-scoped and persisted.
pub const SETTING_REJECT_TELEPORT_OFFERS: &str = "RejectTeleportOffersMode";

/// Whether a **friend's** teleport offer is exempt from
/// [`SETTING_REJECT_TELEPORT_OFFERS`] (the reference
/// `FSDontRejectTeleportOffersFromFriends`).
pub const SETTING_DONT_REJECT_TELEPORT_FROM_FRIENDS: &str = "DontRejectTeleportOffersFromFriends";

/// The canned reply sent to a rejected teleport offer (the reference
/// `FSRejectTeleportOffersResponse`).
pub const SETTING_REJECT_TELEPORT_RESPONSE: &str = "RejectTeleportOffersResponse";

/// Whether incoming friendship requests are rejected (the reference
/// `FSRejectFriendshipRequestsMode`). Account-scoped and persisted.
pub const SETTING_REJECT_FRIENDSHIP_REQUESTS: &str = "RejectFriendshipRequestsMode";

/// The canned reply sent to a rejected friendship request (the reference
/// `FSRejectFriendshipRequestsResponse`).
pub const SETTING_REJECT_FRIENDSHIP_RESPONSE: &str = "RejectFriendshipRequestsResponse";

/// Whether incoming group invitations are rejected (the reference
/// `FSRejectAllGroupInvitesMode`). Account-scoped and persisted.
pub const SETTING_REJECT_ALL_GROUP_INVITES: &str = "RejectAllGroupInvitesMode";

/// Whether an invitation to a group the agent is **already a member of** is
/// still shown (the reference `FSShowJoinedGroupInvitations`; default off, so
/// the redundant re-invite is dropped).
pub const SETTING_SHOW_JOINED_GROUP_INVITATIONS: &str = "ShowJoinedGroupInvitations";

/// Whether an ad-hoc conference invitation is silently declined (the reference
/// `FSIgnoreAdHocSessions`). Group IMs are never touched by it — only the
/// multi-resident conferences a griefer can pull anyone into.
pub const SETTING_IGNORE_AD_HOC_SESSIONS: &str = "IgnoreAdHocSessions";

/// Whether a **friend's** conference invitation is exempt from
/// [`SETTING_IGNORE_AD_HOC_SESSIONS`] (the reference
/// `FSDontIgnoreAdHocFromFriends`).
pub const SETTING_DONT_IGNORE_AD_HOC_FROM_FRIENDS: &str = "DontIgnoreAdHocFromFriends";

/// The default rejected-teleport reply (the reference
/// `RejectTeleportOffersResponseDefault`, without its `[APP_NAME]`
/// interpolation).
const REJECT_TELEPORT_RESPONSE_DEFAULT: &str = "The Resident you messaged has activated 'reject all teleport offers and requests' mode, \
     which means they have requested not to be disturbed with teleport offers and requests. You \
     may still send an IM message manually.";

/// The default rejected-friendship reply (the reference
/// `RejectFriendshipRequestsResponseDefault`).
const REJECT_FRIENDSHIP_RESPONSE_DEFAULT: &str = "The Resident you messaged has activated 'reject all friendship requests' mode, which means \
     they have requested not to be disturbed with friendship requests. You may still send an IM \
     message manually.";

/// Register the auto-reject settings. They share the `[presence]` section with
/// the modes they sit beside in the menu, and every one of them defaults
/// **off** — a viewer that silently swallowed offers out of the box would be a
/// trap.
pub fn register_settings(settings: &mut ViewerSettings) {
    settings.register_in(
        PRESENCE_SECTION,
        SETTING_REJECT_TELEPORT_OFFERS,
        SettingValue::Bool(false),
        "Reject every incoming teleport offer and request",
    );
    settings.register_in(
        PRESENCE_SECTION,
        SETTING_DONT_REJECT_TELEPORT_FROM_FRIENDS,
        SettingValue::Bool(false),
        "Exempt friends from the teleport-offer rejection",
    );
    settings.register_in(
        PRESENCE_SECTION,
        SETTING_REJECT_TELEPORT_RESPONSE,
        SettingValue::String(REJECT_TELEPORT_RESPONSE_DEFAULT.to_owned()),
        "The automatic reply sent to a rejected teleport offer",
    );
    settings.register_in(
        PRESENCE_SECTION,
        SETTING_REJECT_FRIENDSHIP_REQUESTS,
        SettingValue::Bool(false),
        "Reject every incoming friendship request",
    );
    settings.register_in(
        PRESENCE_SECTION,
        SETTING_REJECT_FRIENDSHIP_RESPONSE,
        SettingValue::String(REJECT_FRIENDSHIP_RESPONSE_DEFAULT.to_owned()),
        "The automatic reply sent to a rejected friendship request",
    );
    settings.register_in(
        PRESENCE_SECTION,
        SETTING_REJECT_ALL_GROUP_INVITES,
        SettingValue::Bool(false),
        "Reject every incoming group invitation",
    );
    settings.register_in(
        PRESENCE_SECTION,
        SETTING_SHOW_JOINED_GROUP_INVITATIONS,
        SettingValue::Bool(false),
        "Show invitations to groups I am already a member of",
    );
    settings.register_in(
        PRESENCE_SECTION,
        SETTING_IGNORE_AD_HOC_SESSIONS,
        SettingValue::Bool(false),
        "Silently decline every ad-hoc conference invitation",
    );
    settings.register_in(
        PRESENCE_SECTION,
        SETTING_DONT_IGNORE_AD_HOC_FROM_FRIENDS,
        SettingValue::Bool(false),
        "Exempt friends from the ad-hoc conference rejection",
    );
    settings.register_in(
        PRESENCE_SECTION,
        SETTING_AUTORESPONSE_ITEM,
        SettingValue::String(String::new()),
        "The inventory item sent with every autoresponse (item id; empty = none)",
    );
}

/// Which class of offer arrived — the granularity the reject modes act at. A
/// teleport *offer* (someone offering to bring us to them) and a teleport
/// *request* (someone asking to be brought to us) share one mode, exactly as
/// the reference's single "teleport offers and requests" toggle does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OfferClass {
    /// A teleport offer or a teleport request.
    Teleport,
    /// A friendship offer.
    Friendship,
    /// A group-membership invitation.
    GroupInvite,
}

/// Why an offer was rejected without ever being shown — what `reject_for`
/// decided, so the caller can pick the reply (if any) and log the reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RejectKind {
    /// The reject-teleport-offers mode is on.
    Teleport,
    /// The reject-friendship-requests mode is on.
    Friendship,
    /// The reject-all-group-invites mode is on.
    GroupInvite,
    /// The invitation is to a group the agent is already a member of, and
    /// [`SETTING_SHOW_JOINED_GROUP_INVITATIONS`] is off.
    AlreadyJoinedGroup,
}

impl RejectKind {
    /// The setting holding this rejection's canned reply, or `None` when the
    /// rejection is silent (both group-invite cases: the invitation comes from
    /// a group, and the reference answers neither).
    #[must_use]
    pub(crate) const fn response_setting(self) -> Option<&'static str> {
        match self {
            Self::Teleport => Some(SETTING_REJECT_TELEPORT_RESPONSE),
            Self::Friendship => Some(SETTING_REJECT_FRIENDSHIP_RESPONSE),
            Self::GroupInvite | Self::AlreadyJoinedGroup => None,
        }
    }
}

/// The mode flags a reject decision reads, lifted out of the settings store so
/// the decision itself is a pure function.
#[expect(
    clippy::struct_excessive_bools,
    reason = "a direct mirror of the independent per-account mode flags the reference keeps; \
              folding them into enums would invent states the settings cannot express"
)]
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct RejectPolicy {
    /// Reject teleport offers and requests.
    pub(crate) reject_teleport: bool,
    /// …but not a friend's.
    pub(crate) spare_friends_teleport: bool,
    /// Reject friendship requests.
    pub(crate) reject_friendship: bool,
    /// Reject group invitations.
    pub(crate) reject_group_invites: bool,
    /// Show an invitation to an already-joined group instead of dropping it.
    pub(crate) show_joined_group_invitations: bool,
}

impl RejectPolicy {
    /// Read the policy out of the settings store (everything defaults off).
    #[must_use]
    pub(crate) fn from_settings(settings: Option<&ViewerSettings>) -> Self {
        let flag = |name: &str| {
            settings.is_some_and(|settings| settings.store().get_bool(name).unwrap_or(false))
        };
        Self {
            reject_teleport: flag(SETTING_REJECT_TELEPORT_OFFERS),
            spare_friends_teleport: flag(SETTING_DONT_REJECT_TELEPORT_FROM_FRIENDS),
            reject_friendship: flag(SETTING_REJECT_FRIENDSHIP_REQUESTS),
            reject_group_invites: flag(SETTING_REJECT_ALL_GROUP_INVITES),
            show_joined_group_invitations: flag(SETTING_SHOW_JOINED_GROUP_INVITATIONS),
        }
    }
}

/// Decide whether an offer of the given class is auto-rejected — the whole
/// policy, as a pure function. `is_friend` is about the *sender*;
/// `already_member` only means anything for a group invitation (the agent is
/// already in the inviting group).
#[must_use]
pub(crate) const fn reject_for(
    policy: RejectPolicy,
    class: OfferClass,
    is_friend: bool,
    already_member: bool,
) -> Option<RejectKind> {
    match class {
        OfferClass::Teleport => {
            if policy.reject_teleport && !(policy.spare_friends_teleport && is_friend) {
                Some(RejectKind::Teleport)
            } else {
                None
            }
        }
        OfferClass::Friendship => {
            if policy.reject_friendship {
                Some(RejectKind::Friendship)
            } else {
                None
            }
        }
        OfferClass::GroupInvite => {
            if policy.reject_group_invites {
                Some(RejectKind::GroupInvite)
            } else if already_member && !policy.show_joined_group_invitations {
                // The reference checks this one first; either way it is the same
                // suppression, and keeping it last means the explicit mode is
                // what gets logged when both apply.
                Some(RejectKind::AlreadyJoinedGroup)
            } else {
                None
            }
        }
    }
}

/// Whether an **ad-hoc conference** invitation from this sender is ignored: the
/// mode is on and the sender is not a spared friend. A group IM is never
/// ignored by this policy — the caller passes only conference invitations.
#[must_use]
pub(crate) fn ignores_ad_hoc(settings: Option<&ViewerSettings>, is_friend: bool) -> bool {
    let flag = |name: &str| {
        settings.is_some_and(|settings| settings.store().get_bool(name).unwrap_or(false))
    };
    flag(SETTING_IGNORE_AD_HOC_SESSIONS)
        && !(flag(SETTING_DONT_IGNORE_AD_HOC_FROM_FRIENDS) && is_friend)
}

/// A rejection's canned reply text: the configured string, or `None` when the
/// kind has no reply or the user blanked the field (a blank field means "reject
/// them, but say nothing").
#[must_use]
pub(crate) fn response_text(settings: Option<&ViewerSettings>, kind: RejectKind) -> Option<String> {
    let name = kind.response_setting()?;
    let text = settings?.store().get_str(name).ok()?;
    if text.is_empty() {
        None
    } else {
        Some(text.to_owned())
    }
}

/// Toggle an auto-reject mode from the Comm ▸ Online Status menu, raising the
/// reference's "mode is on" notification on the rising edge. Returns whether
/// the action was one of ours, so the caller's dispatch can fall through.
///
/// The sibling of [`crate::presence::toggle_presence_mode`], and split from it
/// for the same reason the modes are separate: these three are pure settings
/// with no session state and no wire representation at all.
pub fn toggle_reject_mode(
    action: &str,
    settings: &mut ViewerSettings,
    notify: &mut bevy::prelude::MessageWriter<ShowNotification>,
) -> bool {
    let (name, template) = match action {
        "reject-teleport-offers" => (
            SETTING_REJECT_TELEPORT_OFFERS,
            "RejectTeleportOffersModeSet",
        ),
        "reject-group-invites" => (
            SETTING_REJECT_ALL_GROUP_INVITES,
            "RejectAllGroupInvitesModeSet",
        ),
        "reject-friendship-requests" => (
            SETTING_REJECT_FRIENDSHIP_REQUESTS,
            "RejectFriendshipRequestsModeSet",
        ),
        _ => return false,
    };
    let on = !settings.store().get_bool(name).unwrap_or(false);
    settings.set_account(name, SettingValue::Bool(on));
    settings.save_async();
    if on {
        notify.write(ShowNotification::new(template));
    }
    true
}

#[cfg(test)]
mod tests {
    use super::{OfferClass, RejectKind, RejectPolicy, reject_for};
    use pretty_assertions::assert_eq;

    /// With every mode off, nothing is rejected — the shipped default, and the
    /// only one where the user still sees each offer.
    #[test]
    fn nothing_is_rejected_by_default() {
        let policy = RejectPolicy::default();
        for class in [
            OfferClass::Teleport,
            OfferClass::Friendship,
            OfferClass::GroupInvite,
        ] {
            assert_eq!(reject_for(policy, class, false, false), None);
            assert_eq!(reject_for(policy, class, true, false), None);
        }
    }

    /// The teleport mode rejects everyone, unless the friends exemption is on —
    /// in which case a friend's offer comes through and a stranger's does not.
    #[test]
    fn the_friends_exemption_only_spares_friends() {
        let all = RejectPolicy {
            reject_teleport: true,
            ..RejectPolicy::default()
        };
        assert_eq!(
            reject_for(all, OfferClass::Teleport, true, false),
            Some(RejectKind::Teleport),
            "without the exemption a friend is rejected too"
        );
        let spare = RejectPolicy {
            spare_friends_teleport: true,
            ..all
        };
        assert_eq!(reject_for(spare, OfferClass::Teleport, true, false), None);
        assert_eq!(
            reject_for(spare, OfferClass::Teleport, false, false),
            Some(RejectKind::Teleport)
        );
        // The exemption alone, with the mode off, rejects nothing at all.
        let off = RejectPolicy {
            reject_teleport: false,
            ..spare
        };
        assert_eq!(reject_for(off, OfferClass::Teleport, false, false), None);
    }

    /// Each mode answers only its own class of offer.
    #[test]
    fn each_mode_answers_only_its_own_class() {
        let friendship = RejectPolicy {
            reject_friendship: true,
            ..RejectPolicy::default()
        };
        assert_eq!(
            reject_for(friendship, OfferClass::Friendship, true, false),
            Some(RejectKind::Friendship)
        );
        assert_eq!(
            reject_for(friendship, OfferClass::Teleport, false, false),
            None
        );
        assert_eq!(
            reject_for(friendship, OfferClass::GroupInvite, false, false),
            None
        );
        let groups = RejectPolicy {
            reject_group_invites: true,
            ..RejectPolicy::default()
        };
        assert_eq!(
            reject_for(groups, OfferClass::GroupInvite, false, false),
            Some(RejectKind::GroupInvite)
        );
        assert_eq!(
            reject_for(groups, OfferClass::Friendship, false, false),
            None
        );
    }

    /// An invitation to a group we are already in is dropped even with every
    /// mode off — and shown again once the user asks for those invitations.
    #[test]
    fn an_already_joined_group_invite_is_dropped() {
        let policy = RejectPolicy::default();
        assert_eq!(
            reject_for(policy, OfferClass::GroupInvite, false, true),
            Some(RejectKind::AlreadyJoinedGroup)
        );
        let shown = RejectPolicy {
            show_joined_group_invitations: true,
            ..policy
        };
        assert_eq!(
            reject_for(shown, OfferClass::GroupInvite, false, true),
            None
        );
        // The explicit mode still wins over the exemption for joined groups.
        let rejecting = RejectPolicy {
            reject_group_invites: true,
            ..shown
        };
        assert_eq!(
            reject_for(rejecting, OfferClass::GroupInvite, false, true),
            Some(RejectKind::GroupInvite)
        );
    }

    /// Only the two personal rejections carry a canned reply; a group
    /// invitation is answered by nobody.
    #[test]
    fn only_the_personal_rejections_reply() {
        assert!(RejectKind::Teleport.response_setting().is_some());
        assert!(RejectKind::Friendship.response_setting().is_some());
        assert_eq!(RejectKind::GroupInvite.response_setting(), None);
        assert_eq!(RejectKind::AlreadyJoinedGroup.response_setting(), None);
    }
}
