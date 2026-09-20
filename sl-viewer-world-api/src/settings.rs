//! Settings the behaviour reads, not the tab that shows them.
//!
//! A preferences tab draws a control; the setting it writes is read somewhere
//! else entirely -- the chat overlay sizes itself from the font setting, the
//! auto-reply picks its text by mode, the idle timer reads the AFK seconds.
//! The keys live with the behaviour, so a tab is never something the behaviour
//! has to depend on.

use bevy::prelude::*;
use sl_viewer_settings::ViewerSettings;

/// Whether the autorespond mode is on (the reference `FSAutorespondMode`).
/// Account-scoped and persisted.
pub const SETTING_AUTORESPOND_MODE: &str = "AutorespondMode";

/// Whether the autorespond-to-non-friends mode is on (the reference
/// `FSAutorespondNonFriendsMode`). Account-scoped and persisted.
pub const SETTING_AUTORESPOND_NON_FRIENDS_MODE: &str = "AutorespondNonFriendsMode";

/// Whether either autorespond mode is on, which is the one question three
/// different tiers ask: the IM auto-reply decides whether to answer, the
/// Comm menu ticks its toggles, and a name tag badges the own avatar. Kept
/// here so the "either mode counts" rule has exactly one statement.
#[must_use]
pub fn shows_autoresponse(settings: Option<&ViewerSettings>) -> bool {
    [
        SETTING_AUTORESPOND_MODE,
        SETTING_AUTORESPOND_NON_FRIENDS_MODE,
    ]
    .into_iter()
    .any(|name| settings.is_some_and(|settings| settings.store().get_bool(name).unwrap_or(false)))
}

/// The chat font-size step: `0` small, `1` medium, `2` large. Consumed by the
/// overlay (`crate::chat`) and the conversations transcript
/// (`crate::conversations`); the reference `ChatFontSize` radio group.
pub const SETTING_CHAT_FONT_SIZE: &str = "ChatFontSize";

/// Seconds a nearby-chat overlay line lives before it has fully faded (the
/// reference `NearbyToastLifeTime`); the fade itself takes the last
/// `crate::chat` fade-duration seconds of it.
pub const SETTING_NEARBY_TOAST_LIFETIME: &str = "NearbyChatToastLifetime";

/// The most lines the nearby-chat overlay shows at once (the burst safety
/// valve; the reference console's `ConsoleMaxLines`).
pub const SETTING_CHAT_MAX_LINES: &str = "ChatOverlayMaxLines";

/// The auto-reply sent to an IM sender while in Do Not Disturb (busy) mode.
/// Account-scoped; consumed by `viewer-do-not-disturb-away`.
pub const SETTING_BUSY_RESPONSE: &str = "BusyResponse";

/// The auto-reply sent while in autorespond mode (the Firestorm extension).
/// Account-scoped; consumed by `viewer-do-not-disturb-away`.
pub const SETTING_AUTORESPOND_RESPONSE: &str = "AutorespondResponse";

/// The auto-reply sent to non-friends while in autorespond-to-non-friends
/// mode. Account-scoped; consumed by `viewer-do-not-disturb-away`.
pub const SETTING_AUTORESPOND_NON_FRIENDS_RESPONSE: &str = "AutorespondNonFriendsResponse";

/// Seconds of inactivity before the viewer marks the avatar away; `0` = never.
/// Registered here, consumed by the away-mode task
/// (`viewer-do-not-disturb-away`).
pub const SETTING_AFK_TIMEOUT: &str = "AfkTimeoutSeconds";

/// Tracks whether the local avatar is **ground-sitting**.
///
/// The session records object-sits (`SlAgentParcel::seated_on`) but keeps *no*
/// ground-sit state — `sit_on_ground` sends only a transient control bit, so
/// there is nothing on the wire to read back. The viewer therefore tracks it
/// here: set when this menu sends Sit Down, cleared when it sends Stand Up or the
/// avatar walks (which stands it up). Best-effort — a ground sit begun or ended
/// by something other than this menu or ordinary locomotion is not observed, and
/// the worst case is a momentarily wrong Stand Up / Sit Down enable that the next
/// sit / stand / step corrects.
#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct SelfGroundSit {
    /// Whether the local avatar is currently sitting on the ground.
    pub sitting: bool,
}

/// The settings key (under the `[statusbar]` section) gating the agent-position
/// coordinates in the location read-out, mirroring the reference viewer's
/// `NavBarShowCoordinates`. Bare, like the floater-geometry keys — the section
/// only shapes the persisted file, not the lookup. `pub(crate)` for the
/// preferences floater's bound checkbox.
pub const SHOW_COORDINATES_KEY: &str = "statusbar_show_coordinates";

/// The in-world double-click action setting name (mirrors the reference's
/// `DoubleClickAction`). Bound by the preferences camera & movement tab
/// (`preferences_camera_move`).
pub const SETTING_DOUBLE_CLICK_ACTION: &str = "DoubleClickAction";

/// How many times smaller than the window the **3D world** is rendered before
/// being stretched back over it (the reference `RenderResolutionDivisor`,
/// `pipeline.cpp`): `1` — the default — renders the world at the window's own
/// resolution and costs nothing.
///
/// Declared here rather than beside the render path that obeys it because two
/// layers have to agree on the name and neither may depend on the other: the
/// scene layer (`sl_viewer_world_scene::resolution_divisor`) registers it and
/// resizes the world's render target from it, and the RLV surface below that
/// ([`crate::rlv::ViewerRlvExt`]) reads and writes it as the one writable row of
/// the `@getdebug_*` / `@setdebug_*` allowlist.
pub const SETTING_RENDER_RESOLUTION_DIVISOR: &str = "RenderResolutionDivisor";

/// The agent-frame rear-view camera offset (forward, left, up metres), the
/// reference's `CameraOffsetRearView`: three metres behind and 0.75 m above the
/// focus. Its length is the default zoom distance and its elevation the default
/// tilt.
pub const CAMERA_OFFSET: Vec3 = Vec3::new(-3.0, 0.0, 0.75);

/// The closest the third-person camera zooms before it crosses into mouselook —
/// the reference's `LAND_MIN_ZOOM`, near enough to the head that the transition
/// reads as "stepping inside".
pub const MOUSELOOK_CROSS_DISTANCE: f32 = 0.5;

/// The farthest the third-person camera zooms from the avatar
/// (`MAX_CAMERA_DISTANCE_FROM_AGENT`).
pub const MAX_DISTANCE: f32 = 50.0;

/// Pitch clamp (just under a quarter turn) so the view never flips over the pole.
pub const MAX_PITCH: f32 = 1.54;

/// The inventory item sent along with every autoresponse, as its item id in
/// text form; empty = none (the reference `FSAutoresponseItemUUID`). Consumed
/// by the presence auto-reply (`sl_viewer_social::PresenceState`), which is why it is
/// account-scoped like the replies themselves.
pub const SETTING_AUTORESPONSE_ITEM: &str = "AutoresponseItemUUID";
