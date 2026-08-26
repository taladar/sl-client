//! Setting **names** two layers have to agree on.
//!
//! A setting has an owner — the feature whose behaviour it changes, which
//! registers its default and its description — and a second reader, the
//! preferences floater, which binds a checkbox or a text field to it. Both
//! sides need the same `&'static str` key, and neither is beneath the other:
//! the feature does not know the panel exists, and the panel is drawn from a
//! layer above every feature it shows a control for.
//!
//! A name they both need therefore belongs in the layer beneath both, which is
//! this crate — the same move the `PipelineStats` labels made during the
//! viewer's crate split. It is not merely tidier: `sl-viewer-preferences`
//! named forty-one of these and *nothing else at all* from
//! `sl-viewer-people` and `sl-viewer-map`, so two whole crate dependencies —
//! one of them the tier's slowest crate — existed to carry string constants,
//! and the build serialised behind them.
//!
//! The owner keeps everything that is actually its own: the default value, the
//! settings section, the human-readable description, and the code that reads
//! the setting. It re-exports its keys from here, so `radar::SETTING_AGE_DAYS`
//! still resolves inside `sl-viewer-people` and the two cannot drift apart.
//!
//! Only *shared* keys live here. A key one crate reads and no other layer
//! names stays with its feature.

/// Keys owned by `sl-viewer-people`'s `presence` (away / do-not-disturb, and
/// the canned replies they send), drawn on the preferences general and chat
/// tabs.
pub mod presence {
    /// Whether an IM received while merely **away** is answered at all (the
    /// reference `FSSendAwayAvatarResponse`; default off — being away is not
    /// being busy).
    pub const SETTING_SEND_AWAY_RESPONSE: &str = "SendAwayAvatarResponse";

    /// The reply sent to an IM while away, when [`SETTING_SEND_AWAY_RESPONSE`]
    /// is on (the reference `FSAwayAvatarResponse`).
    pub const SETTING_AWAY_RESPONSE: &str = "AwayAvatarResponse";

    /// Whether a **blocked** resident's IM is answered with
    /// [`SETTING_MUTED_RESPONSE`] (the reference `FSSendMutedAvatarResponse`;
    /// default off — telling someone they are blocked is a deliberate choice).
    pub const SETTING_SEND_MUTED_RESPONSE: &str = "SendMutedAvatarResponse";

    /// The reply sent to a blocked resident's IM, when
    /// [`SETTING_SEND_MUTED_RESPONSE`] is on (the reference
    /// `FSMutedAvatarResponse`).
    pub const SETTING_MUTED_RESPONSE: &str = "MutedAvatarResponse";

    /// Whether going away sits the avatar down on the ground, standing it back
    /// up on return (the reference `AvatarSitOnAway`, an anti-grief habit).
    /// Default off.
    pub const SETTING_SIT_ON_AWAY: &str = "AvatarSitOnAway";

    /// Seconds of *being away* after which the viewer logs out by itself; `0` =
    /// never (the reference `QuitAfterSecondsOfAFK`). Distinct from
    /// `sl-viewer-world-api`'s `SETTING_AFK_TIMEOUT`, which is the idle time
    /// before going away.
    pub const SETTING_QUIT_AFTER_AFK: &str = "QuitAfterSecondsOfAFK";
}

/// Keys owned by `sl-viewer-people`'s `people` (the friends list), drawn on the
/// preferences alerts tab.
pub mod people {
    /// The account setting gating the friend online / offline toasts (the
    /// reference `ChatOnlineNotification`): while on, a friend's presence
    /// change raises a `FriendOnlineOffline` tip. Lives in the
    /// `[notifications]` section with the other notification preferences.
    pub const SETTING_FRIEND_NOTIFY: &str = "ChatOnlineNotification";

    /// The account setting that lets a **contact set** ask for its members'
    /// online / offline toasts even while [`SETTING_FRIEND_NOTIFY`] is off (the
    /// reference `FSContactSetsNotificationToast`, default off — one opts in to
    /// the per-set path deliberately). The per-set flag itself lives on the set
    /// (`sl-viewer-people`'s `contact_sets::ContactSets::notifies`); this is the
    /// master switch over all of them, so the feature can be turned off without
    /// editing every set.
    pub const SETTING_CONTACT_SET_NOTIFY: &str = "ContactSetsNotificationToast";
}

/// Keys owned by `sl-viewer-people`'s `group_notice`, drawn on the preferences
/// alerts tab.
pub mod group_notice {
    /// The account setting gating group-notice toasts (our own name — the
    /// reference has no single global gate). While off, a received notice
    /// raises no card and is not persisted for relogin re-raise (it stays
    /// readable in the group's Notices tab, which pulls from the server). Lives
    /// in the `[notifications]` section with the other notification
    /// preferences.
    pub const SETTING_GROUP_NOTICE_TOASTS: &str = "ShowGroupNoticeToasts";
}

/// Keys owned by `sl-viewer-people`'s `offers_invites`, drawn on the
/// preferences alerts tab.
pub mod offers_invites {
    /// The account setting for silently accepting inventory offers (the
    /// reference `AutoAcceptNewInventory`; default **off**). While on, an
    /// inventory offer is filed into its type folder with no offer card; an
    /// offer whose destination cannot be resolved yet (the inventory skeleton
    /// still loading) falls back to the card — an offer is never dropped. Lives
    /// in the `[notifications]` section with the other notification
    /// preferences.
    pub const SETTING_AUTO_ACCEPT_INVENTORY: &str = "AutoAcceptNewInventory";
}

/// Keys owned by `sl-viewer-people`'s `auto_reject` (the standing refuse-this
/// modes), drawn on the preferences chat tab and the menu bar's Online Status
/// submenu.
pub mod auto_reject {
    /// Whether incoming teleport offers and requests are rejected unanswered
    /// (the reference `FSRejectTeleportOffersMode`). Account-scoped and
    /// persisted.
    pub const SETTING_REJECT_TELEPORT_OFFERS: &str = "RejectTeleportOffersMode";

    /// Whether a **friend's** teleport offer is exempt from
    /// [`SETTING_REJECT_TELEPORT_OFFERS`] (the reference
    /// `FSDontRejectTeleportOffersFromFriends`).
    pub const SETTING_DONT_REJECT_TELEPORT_FROM_FRIENDS: &str =
        "DontRejectTeleportOffersFromFriends";

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
    /// still shown (the reference `FSShowJoinedGroupInvitations`; default off,
    /// so the redundant re-invite is dropped).
    pub const SETTING_SHOW_JOINED_GROUP_INVITATIONS: &str = "ShowJoinedGroupInvitations";

    /// Whether an ad-hoc conference invitation is silently declined (the
    /// reference `FSIgnoreAdHocSessions`). Group IMs are never touched by it —
    /// only the multi-resident conferences a griefer can pull anyone into.
    pub const SETTING_IGNORE_AD_HOC_SESSIONS: &str = "IgnoreAdHocSessions";

    /// Whether a **friend's** conference invitation is exempt from
    /// [`SETTING_IGNORE_AD_HOC_SESSIONS`] (the reference
    /// `FSDontIgnoreAdHocFromFriends`).
    pub const SETTING_DONT_IGNORE_AD_HOC_FROM_FRIENDS: &str = "DontIgnoreAdHocFromFriends";
}

/// Keys owned by `sl-viewer-people`'s `radar` (the nearby-avatar list's alerts),
/// drawn on the preferences alerts tab. All account-scoped, in the `[radar]`
/// section.
pub mod radar {
    /// Report entering chat (say) range.
    pub const SETTING_CHAT_ENTER: &str = "RadarReportChatRangeEnter";
    /// Report leaving chat (say) range.
    pub const SETTING_CHAT_LEAVE: &str = "RadarReportChatRangeLeave";
    /// Report entering draw distance.
    pub const SETTING_DRAW_ENTER: &str = "RadarReportDrawRangeEnter";
    /// Report leaving draw distance.
    pub const SETTING_DRAW_LEAVE: &str = "RadarReportDrawRangeLeave";
    /// Report entering the own region.
    pub const SETTING_SIM_ENTER: &str = "RadarReportSimRangeEnter";
    /// Report leaving the own region.
    pub const SETTING_SIM_LEAVE: &str = "RadarReportSimRangeLeave";
    /// Where alerts go: `"chat"` (Nearby Chat line) or `"toast"`.
    pub const SETTING_ALERT_OUTPUT: &str = "RadarAlertOutput";
    /// Arm the young-account alert.
    pub const SETTING_AGE_ALERT: &str = "RadarAgeAlert";
    /// The young-account threshold, days.
    pub const SETTING_AGE_DAYS: &str = "RadarAgeAlertDays";
}

/// Keys owned by `sl-viewer-map`'s `minimap`, drawn on the preferences move &
/// view tab. All global, in the `[minimap]` section.
pub mod minimap {
    /// The map scale setting (pixels per 256 m region), shared by all
    /// instances.
    pub const SETTING_SCALE: &str = "MiniMapScale";

    /// Whether the map rotates so the camera heading points up.
    pub const SETTING_ROTATE: &str = "MiniMapRotate";

    /// Whether the pan offset eases back to centre each frame.
    pub const SETTING_AUTO_CENTER: &str = "MiniMapAutoCenter";

    /// The minimap surface opacity.
    pub const SETTING_OPACITY: &str = "MiniMapOpacity";

    /// Whether the object layer draws at all.
    pub const SETTING_OBJECTS: &str = "MiniMapObjects";

    /// Whether the parcel layer (property lines) draws at all.
    pub const SETTING_PROPERTY_LINES: &str = "MiniMapShowPropertyLines";

    /// Whether for-sale / auction parcels are filled.
    pub const SETTING_FOR_SALE: &str = "MiniMapForSaleParcels";

    /// Master toggle for the chat-range rings.
    pub const SETTING_CHAT_RING: &str = "MiniMapChatRing";
}

/// Keys owned by `sl-viewer-map`'s `world_map`, drawn on the preferences move &
/// view tab. All global, in the `[worldmap]` section.
pub mod world_map {
    /// Layer toggle: avatar ("people") markers.
    pub const SETTING_PEOPLE: &str = "WorldMapShowPeople";

    /// Layer toggle: telehub / infohub markers.
    pub const SETTING_INFOHUBS: &str = "WorldMapShowInfohubs";

    /// Layer toggle: land-for-sale markers.
    pub const SETTING_LAND_SALE: &str = "WorldMapShowLandForSale";

    /// Layer toggle: PG event markers.
    pub const SETTING_EVENTS: &str = "WorldMapShowEvents";

    /// Whether region-name labels draw in the detail regime.
    pub const SETTING_REGION_NAMES: &str = "WorldMapShowRegionNames";
}
