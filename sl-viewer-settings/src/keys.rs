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

/// Keys owned by `sl-viewer-world-objects`' `hover_text`, drawn on the
/// preferences general tab. Global, in the `[hovertext]` section.
pub mod hover_text {
    /// Master toggle: show floating object text at all (the reference's
    /// `RenderHUDText` / hover-text preference; default on).
    pub const SETTING_SHOW_HOVER_TEXT: &str = "ShowHoverText";
}

/// Keys owned by `sl-viewer-world-objects`' `name_tag_billboard` — the tag
/// surface itself, as against the [`name_tag_content`] lines drawn on it —
/// bound by the preferences general tab.
pub mod name_tag_billboard {
    /// The distance at which tags start fading, metres (a float setting).
    pub const SETTING_FADE_START: &str = "FadeStartDistance";

    /// The fade range, metres past the fade start at which tags are gone (a
    /// float setting).
    pub const SETTING_FADE_RANGE: &str = "FadeRange";

    /// The bubble backdrop opacity (the reference `ChatBubbleOpacity`,
    /// default 0.5).
    pub const SETTING_BUBBLE_OPACITY: &str = "BubbleOpacity";

    /// The master name-tag toggle (the preferences general tab's headline
    /// switch; the reference `AvatarNameTagMode` off/on axis).
    pub const SETTING_SHOW_NAME_TAGS: &str = "ShowNameTags";

    /// Whether the logged-in avatar's own tag is shown (the reference
    /// `RenderNameShowSelf`).
    pub const SETTING_SHOW_OWN_NAME_TAG: &str = "ShowOwnNameTag";
}

/// The object mesh-detail key owned by `sl-viewer-world-objects`'
/// `render_priority`, with the bounds every slider over it uses — the
/// preferences graphics tab, the quick-preferences panel and the photographer's
/// window all draw one, and a slider whose range disagreed with another's would
/// clamp the same setting to two different maxima.
pub mod render_priority {
    /// The object LOD-factor setting key (the reference `RenderVolumeLODFactor`,
    /// its "Mesh Detail: Objects" slider): a detail multiplier — a larger value
    /// keeps finer mesh / prim / tree geometry to a greater distance.
    pub const SETTING_LOD_FACTOR: &str = "RenderVolumeLODFactor";

    /// The smallest accepted LOD factor (the stock default — the reference
    /// slider also starts at 1).
    pub const LOD_FACTOR_MIN: f32 = 1.0;

    /// The largest accepted LOD factor (the reference slider's maximum; its
    /// graphics presets push to ~4× on Ultra).
    pub const LOD_FACTOR_MAX: f32 = 4.0;
}

/// The key owned by `sl-viewer-world-avatar`'s `derender` that the preferences
/// graphics tab and the quick-preferences panel both bind. The rest of the
/// derender knobs are read by that crate alone and stay there.
pub mod derender {
    /// The **friends-only** filter (the reference's `FSRenderFriendsOnly`):
    /// draw only friends' avatars. Per avatar, because it is a per-avatar habit
    /// — and because the reference keeps it per account too.
    pub const SETTING_FRIENDS_ONLY: &str = "RenderFriendsOnly";
}

/// Keys owned by `sl-viewer-world-avatar`'s `name_tag_content` — which lines a
/// name tag carries — drawn on the preferences general tab.
pub mod name_tag_content {
    /// Show display names on tags (the reference `NameTagShowDisplayNames`,
    /// default on). Off = legacy names only.
    pub const SETTING_SHOW_DISPLAY_NAMES: &str = "ShowDisplayNames";

    /// Show the small `first.last` username line under a custom display name
    /// (the reference `NameTagShowUsernames`, default on).
    pub const SETTING_SHOW_USERNAMES: &str = "ShowUsernames";

    /// Show the group-title line (the reference `NameTagShowGroupTitles`,
    /// default on).
    pub const SETTING_SHOW_GROUP_TITLES: &str = "ShowGroupTitles";

    /// Colour friends' tags (the reference `NameTagShowFriends`, default on).
    pub const SETTING_SHOW_FRIEND_COLOR: &str = "ShowFriendColor";

    /// Show the avatar-distance line (Firestorm `FSTagShowDistance`), measured
    /// from the **own** avatar.
    pub const SETTING_SHOW_DISTANCE: &str = "ShowDistance";

    /// Show the Typing status (Firestorm `FSShowTypingStateInNameTag`).
    pub const SETTING_SHOW_TYPING: &str = "ShowTyping";

    /// Tint the whole tag by chat-range band (Firestorm
    /// `FSTagShowDistanceColors`, default off).
    pub const SETTING_COLOR_BY_DISTANCE: &str = "ColorByDistance";

    /// Show the render-cost (ARC) line at all (Firestorm `FSTagShowARW`,
    /// default on) — the master switch over the two that follow.
    pub const SETTING_SHOW_COMPLEXITY: &str = "ShowComplexity";

    /// Show the render-cost line on your **own** tag (Firestorm
    /// `FSTagShowOwnARW`, default off).
    pub const SETTING_SHOW_OWN_COMPLEXITY: &str = "ShowOwnComplexity";

    /// Show other avatars' render cost **only** when the viewer is limiting
    /// them (Firestorm `FSTagShowTooComplexOnlyARW`, default on).
    pub const SETTING_SHOW_COMPLEXITY_WHEN_LIMITED_ONLY: &str = "ShowComplexityWhenLimitedOnly";
}

/// The avatar complexity budget owned by `sl-viewer-world-avatar`'s
/// `avatar_complexity`: its keys, the bounds of the sliders over them, and the
/// [`ComplexityMode`](avatar_complexity::ComplexityMode) numbering a combo has
/// to agree with to write a mode the feature will read back.
pub mod avatar_complexity {
    /// The complexity budget (the reference's `RenderAvatarMaxComplexity`): an
    /// avatar scoring above it is drawn as a jellydoll. `0` disables the limit
    /// — and, as in the reference, the surface-area trigger with it.
    pub const SETTING_MAX_COMPLEXITY: &str = "RenderAvatarMaxComplexity";

    /// How the budget is applied (the reference's
    /// `RenderAvatarComplexityMode`); see [`ComplexityMode`].
    pub const SETTING_COMPLEXITY_MODE: &str = "RenderAvatarComplexityMode";

    /// The attachment surface-area trigger (the reference's
    /// `RenderAutoMuteSurfaceAreaLimit`), in square metres; `0` turns it off.
    pub const SETTING_SURFACE_AREA_LIMIT: &str = "RenderAutoMuteSurfaceAreaLimit";

    /// The largest budget the sliders offer. Above roughly this an avatar is
    /// unrenderable on any hardware, so a higher setting would only mean "off",
    /// which `0` already says.
    pub const MAX_COMPLEXITY_SLIDER_MAX: f32 = 500_000.0;

    /// The budget slider's step — fine enough to tune, coarse enough that
    /// dragging it does not re-decide every avatar on every pixel.
    pub const MAX_COMPLEXITY_SLIDER_STEP: f32 = 5_000.0;

    /// The largest surface-area limit the slider offers.
    pub const SURFACE_AREA_SLIDER_MAX: f32 = 5000.0;

    /// The surface-area slider's step.
    pub const SURFACE_AREA_SLIDER_STEP: f32 = 100.0;

    /// How the complexity budget is applied to friends — the reference's
    /// `RenderAvatarComplexityMode`, whose stored numbering this keeps. The
    /// numbering is the shared part: the combo writes it and the feature reads
    /// it, so it lives with the key rather than with either.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub enum ComplexityMode {
        /// Judge everyone by the budget alone, friends included.
        #[default]
        ByComplexity,
        /// Friends are always drawn in full, whatever they cost.
        AlwaysShowFriends,
        /// Only friends are drawn in full; everyone else is a jellydoll.
        /// Distinct from Show Friends Only ([`super::derender`]), which does
        /// not draw the non-friends at all — this keeps their silhouette.
        OnlyShowFriends,
    }

    impl ComplexityMode {
        /// The mode for a stored setting value, defaulting to
        /// [`ByComplexity`](Self::ByComplexity) for anything unrecognised.
        #[must_use]
        pub const fn from_stored(value: u32) -> Self {
            match value {
                1 => Self::AlwaysShowFriends,
                2 => Self::OnlyShowFriends,
                _other => Self::ByComplexity,
            }
        }

        /// The stored setting value for this mode.
        #[must_use]
        pub const fn stored(self) -> u32 {
            match self {
                Self::ByComplexity => 0,
                Self::AlwaysShowFriends => 1,
                Self::OnlyShowFriends => 2,
            }
        }
    }
}

/// Keys owned by `sl-viewer-world-scene`'s `glow`, drawn on the preferences
/// graphics tab and the photographer's window. In the `[render.glow]` section.
pub mod glow {
    /// The reference `RenderGlow` setting name (the master enable).
    pub const SETTING_ENABLED: &str = "RenderGlow";
    /// The reference `RenderGlowStrength` setting name.
    pub const SETTING_STRENGTH: &str = "RenderGlowStrength";
    /// The reference `RenderGlowIterations` setting name.
    pub const SETTING_ITERATIONS: &str = "RenderGlowIterations";
    /// The reference `RenderGlowWidth` setting name.
    pub const SETTING_WIDTH: &str = "RenderGlowWidth";
}

/// Keys owned by `sl-viewer-world-scene`'s `exposure` (the dynamic exposure
/// adaptation), drawn on the photographer's window. In the `[render.exposure]`
/// section.
pub mod exposure {
    /// The reference `RenderDynamicExposureEnabled` setting name.
    pub const SETTING_ENABLED: &str = "RenderDynamicExposureEnabled";

    /// The reference `RenderSkyAutoAdjustLegacy` setting name (treat a legacy
    /// sky as if it carried the auto-adjust probe ambiance, so it adapts too).
    pub const SETTING_AUTO_ADJUST_LEGACY: &str = "RenderSkyAutoAdjustLegacy";
}

/// Keys owned by `sl-viewer-world-scene`'s `tonemap`, drawn on the
/// photographer's window, plus the curve numbering its combo writes. In the
/// `[render.tonemap]` section.
pub mod tonemap {
    /// The reference `RenderTonemapType` setting name.
    pub const SETTING_TONEMAP_TYPE: &str = "RenderTonemapType";
    /// The reference `RenderTonemapMix` setting name.
    pub const SETTING_TONEMAP_MIX: &str = "RenderTonemapMix";
    /// The reference `RenderExposure` setting name.
    pub const SETTING_EXPOSURE: &str = "RenderExposure";

    /// The reference `RenderTonemapType` value selecting the Khronos PBR
    /// Neutral curve.
    pub const TONEMAP_KHRONOS_NEUTRAL: u32 = 0;
    /// The reference `RenderTonemapType` value selecting the ACES (Hill) curve
    /// — the reference's default, and so this viewer's.
    pub const TONEMAP_ACES: u32 = 1;
    /// Not a reference value: no tone curve at all (exposure and clamp only,
    /// the reference's `NO_POST` path), so a capture can A/B what the curve is
    /// doing.
    pub const TONEMAP_NONE: u32 = 2;
}

/// Keys owned by `sl-viewer-world-scene`'s `probes` (reflection probes and the
/// mirrors built on them), drawn on the preferences graphics tab and the
/// photographer's window. In the `[render]` section.
pub mod probes {
    /// Toggle dynamic-content capture in local probes.
    pub const PROBE_DYNAMIC_SETTING: &str = "render_reflection_probe_dynamic_content";

    /// The mirror (hero probe) master toggle.
    pub const RENDER_MIRRORS_SETTING: &str = "render_mirrors";

    /// See [`RENDER_MIRRORS_SETTING`] — the hero-probe cube resolution.
    pub const HERO_RESOLUTION_SETTING: &str = "render_hero_probe_resolution";

    /// See [`RENDER_MIRRORS_SETTING`] — the hero-probe re-render cadence in
    /// frames.
    pub const HERO_UPDATE_RATE_SETTING: &str = "render_hero_probe_update_rate";
}

/// The particle cap owned by `sl-viewer-world-scene`'s `particles`, drawn on
/// the preferences graphics tab and the quick-preferences panel. In the
/// `[render]` section.
pub mod particles {
    /// The maximum number of live particles across all sources (the reference
    /// `RenderMaxPartCount`).
    pub const SETTING_MAX_PARTICLES: &str = "RenderMaxPartCount";
}

/// The in-world property-line toggle owned by `sl-viewer-world-scene`'s
/// `parcel_borders`, drawn on the preferences move & view tab. Its sibling
/// `ShowParcelOwners` is named by the scene and the land tool only, so it stays
/// with the feature.
pub mod parcel_borders {
    /// The setting name gating the in-world property lines (the reference
    /// viewer's `ShowPropertyLines`).
    pub const SETTING_SHOW_PROPERTY_LINES: &str = "ShowPropertyLines";
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
