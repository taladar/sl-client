//! Settings the behaviour reads, not the tab that shows them.
//!
//! A preferences tab draws a control; the setting it writes is read somewhere
//! else entirely -- the chat overlay sizes itself from the font setting, the
//! auto-reply picks its text by mode, the idle timer reads the AFK seconds.
//! The keys live with the behaviour, so a tab is never something the behaviour
//! has to depend on.

use bevy::prelude::*;
use sl_client_bevy::{AgentKey, ChatMessage, ChatType};
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

/// Whether a double-click teleport may land on an object that takes a click
/// itself — a scripted touch handler or a click action other than sit
/// (mirrors Firestorm's `FSAllowDoubleClickOnScriptedObjects`, on by default
/// there too). Bound beside [`SETTING_DOUBLE_CLICK_ACTION`].
pub const SETTING_DOUBLE_CLICK_SCRIPTED_OBJECTS: &str = "FSAllowDoubleClickOnScriptedObjects";

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

/// Where a script's `DEBUG_CHANNEL` lines are shown (the reference
/// `ShowScriptErrors` and `ShowScriptErrorsLocation` together): `"window"` —
/// the default, the reference's — for the Script Errors window, `"chat"` for
/// nearby chat, `"nowhere"` for neither. Global.
pub const SETTING_SCRIPT_ERRORS_SHOW_IN: &str = "ScriptErrorsShowIn";

/// Whether the `DEBUG_CHANNEL` lines of objects somebody else owns are shown
/// too. Off by default, which is the reference's rule on Second Life: a
/// script error is its owner's business, and in a crowded region somebody
/// else's broken content would bury your own. OpenSim sends an OSSL denial
/// to everybody near the object (`book/src/gridspec/lsl.md` § OSSL), so there
/// this is what tells them apart. Global.
pub const SETTING_SCRIPT_ERRORS_FROM_OTHERS: &str = "ScriptErrorsFromOthers";

/// Whether an object's `llOwnerSay` goes to the Script Errors window instead
/// of nearby chat (Firestorm's `FSllOwnerSayToScriptDebugWindow`). Off by
/// default. Global.
pub const SETTING_SCRIPT_ERRORS_OWNER_SAY: &str = "ScriptErrorsOwnerSay";

/// Whether closing the Script Errors window empties it (Firestorm's
/// `FSScriptDebugWindowClearOnClose`). Off by default. Global.
pub const SETTING_SCRIPT_ERRORS_CLEAR_ON_CLOSE: &str = "ScriptErrorsClearOnClose";

/// Where a line of nearby chat is shown, as far as the Script Errors window is
/// concerned ([`script_chat_route`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptChatRoute {
    /// In nearby chat, as any other line.
    Chat,
    /// In the Script Errors window, and not in nearby chat.
    Window,
    /// Nowhere.
    Hidden,
}

/// Where `message`, heard by the agent `own_agent`, is shown: every surface
/// that displays nearby chat asks this, so a line the Script Errors window
/// takes is not also shown in the transcript or the overlay.
///
/// A `DEBUG_CHANNEL` line goes where [`SETTING_SCRIPT_ERRORS_SHOW_IN`] says —
/// unless an object somebody else owns said it and
/// [`SETTING_SCRIPT_ERRORS_FROM_OTHERS`] is off, which hides it. An owner-say
/// goes to the window when [`SETTING_SCRIPT_ERRORS_OWNER_SAY`] is on (the grid
/// sends an owner-say to the owner alone, so it is always the agent's own).
/// Every other line is chat. A viewer with no settings store shows debug lines
/// in the window and everything else in chat.
#[must_use]
pub fn script_chat_route(
    settings: Option<&ViewerSettings>,
    own_agent: Option<AgentKey>,
    message: &ChatMessage,
) -> ScriptChatRoute {
    let store = settings.map(ViewerSettings::store);
    match message.chat_type {
        ChatType::DebugChannel => {
            let own = own_agent.is_some_and(|agent| message.owner_id == Some(agent.uuid()));
            let others = store
                .and_then(|store| store.get_bool(SETTING_SCRIPT_ERRORS_FROM_OTHERS).ok())
                .unwrap_or(false);
            if !own && !others {
                return ScriptChatRoute::Hidden;
            }
            match store.and_then(|store| store.get_str(SETTING_SCRIPT_ERRORS_SHOW_IN).ok()) {
                Some("chat") => ScriptChatRoute::Chat,
                Some("nowhere") => ScriptChatRoute::Hidden,
                _window => ScriptChatRoute::Window,
            }
        }
        ChatType::Owner
            if store
                .and_then(|store| store.get_bool(SETTING_SCRIPT_ERRORS_OWNER_SAY).ok())
                .unwrap_or(false) =>
        {
            ScriptChatRoute::Window
        }
        _chat => ScriptChatRoute::Chat,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        SETTING_SCRIPT_ERRORS_FROM_OTHERS, SETTING_SCRIPT_ERRORS_OWNER_SAY,
        SETTING_SCRIPT_ERRORS_SHOW_IN, ScriptChatRoute, script_chat_route,
    };
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{
        AgentKey, ChatAudible, ChatMessage, ChatSource, ChatType, ObjectKey, RegionCoordinates,
        Uuid,
    };
    use sl_settings::{Scope, SettingValue, SettingsStore};
    use sl_viewer_settings::ViewerSettings;

    /// The agent whose viewer it is.
    const OWN: u128 = 7;

    /// A settings store with the four Script Errors settings at their
    /// defaults.
    fn settings() -> ViewerSettings {
        let mut settings = ViewerSettings::from_store_for_test(SettingsStore::new());
        settings.register_in(
            &["chat"],
            SETTING_SCRIPT_ERRORS_SHOW_IN,
            SettingValue::String("window".to_owned()),
            "",
        );
        for name in [
            SETTING_SCRIPT_ERRORS_FROM_OTHERS,
            SETTING_SCRIPT_ERRORS_OWNER_SAY,
        ] {
            settings.register_in(&["chat"], name, SettingValue::Bool(false), "");
        }
        settings
    }

    /// A line of `chat_type` from an object owned by `owner`.
    fn line(chat_type: ChatType, owner: u128) -> ChatMessage {
        ChatMessage {
            from_name: "Object".to_owned(),
            source: ChatSource::Object(ObjectKey::from(Uuid::from_u128(3))),
            owner_id: Some(Uuid::from_u128(owner)),
            chat_type,
            audible: ChatAudible::Fully,
            position: RegionCoordinates::new(1.0, 2.0, 3.0),
            message: "Math Error".to_owned(),
        }
    }

    /// Where `message` goes under `settings`, heard by [`OWN`].
    fn route(settings: &ViewerSettings, message: &ChatMessage) -> ScriptChatRoute {
        script_chat_route(
            Some(settings),
            Some(AgentKey::from(Uuid::from_u128(OWN))),
            message,
        )
    }

    #[test]
    fn an_own_objects_debug_line_goes_where_the_preference_says() {
        let mut settings = settings();
        let own = line(ChatType::DebugChannel, OWN);
        assert_eq!(route(&settings, &own), ScriptChatRoute::Window);
        for (place, expected) in [
            ("chat", ScriptChatRoute::Chat),
            ("nowhere", ScriptChatRoute::Hidden),
            ("window", ScriptChatRoute::Window),
        ] {
            settings.set(
                Scope::Global,
                SETTING_SCRIPT_ERRORS_SHOW_IN,
                SettingValue::String(place.to_owned()),
            );
            assert_eq!(route(&settings, &own), expected, "{place}");
        }
    }

    #[test]
    fn somebody_elses_debug_line_is_hidden_unless_asked_for() {
        let mut settings = settings();
        let theirs = line(ChatType::DebugChannel, 9);
        assert_eq!(route(&settings, &theirs), ScriptChatRoute::Hidden);
        settings.set(
            Scope::Global,
            SETTING_SCRIPT_ERRORS_FROM_OTHERS,
            SettingValue::Bool(true),
        );
        assert_eq!(route(&settings, &theirs), ScriptChatRoute::Window);
        let ownerless = ChatMessage {
            owner_id: None,
            ..line(ChatType::DebugChannel, 9)
        };
        assert_eq!(route(&settings, &ownerless), ScriptChatRoute::Window);
    }

    #[test]
    fn an_owner_say_goes_to_the_window_only_when_asked() {
        let mut settings = settings();
        let owner_say = line(ChatType::Owner, OWN);
        assert_eq!(route(&settings, &owner_say), ScriptChatRoute::Chat);
        settings.set(
            Scope::Global,
            SETTING_SCRIPT_ERRORS_OWNER_SAY,
            SettingValue::Bool(true),
        );
        assert_eq!(route(&settings, &owner_say), ScriptChatRoute::Window);
        assert_eq!(
            route(&settings, &line(ChatType::Normal, OWN)),
            ScriptChatRoute::Chat
        );
    }

    #[test]
    fn with_no_settings_a_debug_line_is_the_windows_and_the_rest_is_chat() {
        let own = Some(AgentKey::from(Uuid::from_u128(OWN)));
        assert_eq!(
            script_chat_route(None, own, &line(ChatType::DebugChannel, OWN)),
            ScriptChatRoute::Window
        );
        assert_eq!(
            script_chat_route(None, own, &line(ChatType::Owner, OWN)),
            ScriptChatRoute::Chat
        );
    }
}
