//! UI sound effects (`viewer-ui-sound-effects`): the viewer's own 2-D feedback
//! sounds — the typing chirp, money paid / received, teleport, the snapshot
//! shutter — played on the mixer's [`Bus::Ui`] with no position and no
//! attenuation, the non-spatial half of sound.
//!
//! The reference viewer models each feedback sound as an overridable asset UUID
//! (`UISnd*`) with a per-sound enable toggle (`PlayModeUISnd*`) under a shared
//! volume category. This mirrors that: [`UiSound`] enumerates the sounds, each
//! with its reference default asset id and default-enabled state; a persisted
//! `<key>_asset` (UUID string) and `<key>_enabled` (bool) settings pair lets a
//! user re-point or silence any one; and they all share the **UI** volume bus the
//! volume panel already exposes (so there is no parallel notion of "UI sound
//! volume").
//!
//! Any surface raises a sound by writing a [`PlayUiSound`] message — it never
//! reaches into the audio engine. The clips are fetched and decoded once through
//! the shared [`SoundCache`](crate::sound_cache::SoundCache) (over the region's
//! `ViewerAsset` capability, so a grid that lacks a given asset simply plays
//! nothing) and prefetched at login so a trigger is not late.
//!
//! # Overriding a sound: user, then skin, then default
//!
//! Each sound resolves in priority order (see [`resolve`]): an explicit **user**
//! setting (a non-blank `<key>_asset` UUID) wins; otherwise a **skin/theme** may
//! override it through a `-sk-uisnd-<key>` CSS property — a `url("file.ogg")`
//! bundled with the skin (loaded as a Bevy [`AudioSource`] and decoded through
//! *our* pipeline onto the UI bus, not `bevy_audio`) or a `"uuid"` grid asset;
//! otherwise the reference **default** UUID. The CSS side ([`SkinUiSounds`]) is
//! `bevy_flair`, registered from the skin plugin.
//!
//! Wired emitters today: the **typing** chirp (closing the gap `typing.rs`
//! recorded — P31.9 shipped the animation without the sound), **money up /
//! down** on a balance change, **teleport out** at a teleport's start, and the
//! **snapshot** shutter. The remaining catalogue entries (click, alert, chat /
//! IM chimes, window open / close, offers) are registered and playable through
//! [`PlayUiSound`] for the surfaces that will raise them — the gesture runtime's
//! sound steps ([[viewer-gesture-runtime]]) being the next planned caller — but
//! are intentionally not auto-emitted yet, to avoid a chime on every widget
//! interaction before there is a preferences surface to tune them.

use std::any::TypeId;
use std::collections::{HashMap, HashSet};

use bevy::asset::ParseAssetPathError;
use bevy::prelude::*;
use bevy_flair::parser::{CssError, Parser, ReflectParseCss, parse_property_value_with};
use bevy_flair::prelude::*;
use bevy_flair::style::placeholder::{
    AssetPathPlaceholder, Placeholder, ReflectPlaceholder, ResolvePlaceholderContext,
};
use sl_audio::{AudioMixer as _, Bus, ClipParams, DecodedClip, Importance, Mixer, decode_clip};
use sl_client_bevy::{AssetKey, Uuid};
use sl_settings::SettingValue;

use crate::settings::ViewerSettings;
use crate::sound_cache::SoundCache;
use crate::ui::UiRoot;

/// The persisted-settings section the UI-sound overrides live under
/// (`[audio.ui_sounds]`), kept distinct from the bus levels (`[audio.bus]`) and
/// the parcel stream (`[audio]`).
const UI_SOUND_SECTION: &[&str] = &["audio", "ui_sounds"];

/// One of the viewer's own feedback sounds. Each maps to a reference `UISnd*`
/// asset and a `PlayModeUISnd*` enable toggle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum UiSound {
    /// A generic button click (`UISndClick`).
    Click,
    /// The typing chirp played while entering local chat (`UISndTyping`).
    Typing,
    /// A generic alert / notification (`UISndAlert`).
    Alert,
    /// An invalid operation / rejected input (`UISndInvalidOp`).
    InvalidOp,
    /// Money received — a credit (`UISndMoneyChangeUp`).
    MoneyUp,
    /// Money paid — a debit (`UISndMoneyChangeDown`).
    MoneyDown,
    /// Teleport initiated (`UISndTeleportOut`).
    TeleportOut,
    /// The snapshot shutter (`UISndSnapshot`).
    Snapshot,
    /// A window / floater opening (`UISndWindowOpen`).
    WindowOpen,
    /// A window / floater closing (`UISndWindowClose`).
    WindowClose,
    /// A new incoming IM session (`UISndNewIncomingIMSession`).
    IncomingIm,
    /// A nearby-chat message arrived (`UISndNearbyChat`).
    NearbyChat,
    /// An inventory offer arrived (`UISndInventoryOffer`).
    InventoryOffer,
    /// A teleport offer arrived (`UISndTeleportOffer`).
    TeleportOffer,
}

impl UiSound {
    /// Every UI sound, for registering settings and prefetching clips.
    const ALL: [Self; 14] = [
        Self::Click,
        Self::Typing,
        Self::Alert,
        Self::InvalidOp,
        Self::MoneyUp,
        Self::MoneyDown,
        Self::TeleportOut,
        Self::Snapshot,
        Self::WindowOpen,
        Self::WindowClose,
        Self::IncomingIm,
        Self::NearbyChat,
        Self::InventoryOffer,
        Self::TeleportOffer,
    ];

    /// A short stable identifier used for the setting keys and logs.
    const fn key(self) -> &'static str {
        match self {
            Self::Click => "click",
            Self::Typing => "typing",
            Self::Alert => "alert",
            Self::InvalidOp => "invalid_op",
            Self::MoneyUp => "money_up",
            Self::MoneyDown => "money_down",
            Self::TeleportOut => "teleport_out",
            Self::Snapshot => "snapshot",
            Self::WindowOpen => "window_open",
            Self::WindowClose => "window_close",
            Self::IncomingIm => "incoming_im",
            Self::NearbyChat => "nearby_chat",
            Self::InventoryOffer => "inventory_offer",
            Self::TeleportOffer => "teleport_offer",
        }
    }

    /// The reference viewer's default asset UUID for this sound (its `UISnd*`
    /// settings default). These are the built-in SL UI sound assets, fetched over
    /// `ViewerAsset` like any other asset.
    const fn default_asset(self) -> &'static str {
        match self {
            Self::Click => "4c8c3c77-de8d-bde2-b9b8-32635e0fd4a6",
            Self::Typing => "5e191c7b-8996-9ced-a177-b2ac32bfea06",
            Self::Alert => "ed124764-705d-d497-167a-182cd9fa2e6c",
            Self::InvalidOp => "4174f859-0d3d-c517-c424-72923dc21f65",
            Self::MoneyUp => "77a018af-098e-c037-51a6-178f05877c6f",
            Self::MoneyDown => "104974e3-dfda-428b-99ee-b0d4e748d3a3",
            Self::TeleportOut => "d7a9a565-a013-2a69-797d-5332baa1a947",
            Self::Snapshot => "3d09f582-3851-c0e0-f5ba-277ac5c73fb4",
            Self::WindowOpen => "c80260ba-41fd-8a46-768a-6bf236360e3a",
            Self::WindowClose => "2c346eda-b60c-ab33-1119-b8941916a499",
            Self::IncomingIm | Self::InventoryOffer | Self::TeleportOffer => {
                "67cc2844-00f3-2b3c-b991-6418d01e1bb7"
            }
            Self::NearbyChat => "a3f48b85-c29f-1f97-ebb6-644b7c053512",
        }
    }

    /// Whether this sound plays by default. The auto-emitted feedback sounds are
    /// on; the rest are registered but default-off until a surface raises them.
    const fn default_enabled(self) -> bool {
        matches!(
            self,
            Self::Typing | Self::MoneyUp | Self::MoneyDown | Self::TeleportOut | Self::Snapshot
        )
    }

    /// The setting key for this sound's overridable asset UUID.
    fn asset_key(self) -> String {
        format!("{}_asset", self.key())
    }

    /// The setting key for this sound's enable toggle.
    fn enabled_key(self) -> String {
        format!("{}_enabled", self.key())
    }
}

/// Raise a UI feedback sound. Any surface writes this; the driver resolves the
/// asset, honours the per-sound enable toggle, and plays it on the UI bus.
#[derive(Message, Debug, Clone, Copy)]
pub(crate) struct PlayUiSound(pub(crate) UiSound);

/// Register every UI sound's persisted enable toggle and overridable asset UUID.
/// Called from [`ViewerSettings::load`](crate::settings::ViewerSettings).
pub(crate) fn register_settings(settings: &mut ViewerSettings) {
    for sound in UiSound::ALL {
        settings.register_in(
            UI_SOUND_SECTION,
            &sound.enabled_key(),
            SettingValue::Bool(sound.default_enabled()),
            "Whether this UI feedback sound plays",
        );
        // Empty by default so the resolution order can tell "user has not
        // overridden this" (fall through to a skin override, then the reference
        // default) from an explicit user UUID.
        settings.register_in(
            UI_SOUND_SECTION,
            &sound.asset_key(),
            SettingValue::String(String::new()),
            "Override the sound asset UUID for this UI feedback sound (blank = default)",
        );
    }
}

/// Where a UI sound's clip comes from once resolved: a grid asset fetched by the
/// [`SoundCache`], or a skin-bundled file loaded as a Bevy [`AudioSource`] and
/// decoded through the same pipeline (its clip cached in [`SkinSoundClips`]).
enum ResolvedUiSound {
    /// A grid asset (the reference default, a user override, or a skin's UUID).
    Grid(AssetKey),
    /// A skin-bundled audio file, keyed by its loaded-asset id.
    Skin(AssetId<AudioSource>),
}

/// Resolve a UI sound to its clip source, in priority order: an explicit user
/// setting (a non-blank, valid UUID) wins; then a skin/theme override
/// ([`SkinUiSounds`], `url()` file or UUID); then the reference default UUID.
fn resolve(
    settings: &ViewerSettings,
    skin: Option<&SkinUiSounds>,
    sound: UiSound,
) -> Option<ResolvedUiSound> {
    // 1. Explicit user override.
    if let Ok(raw) = settings.store().get_str(&sound.asset_key())
        && !raw.is_empty()
        && let Ok(uuid) = Uuid::parse_str(raw)
        && !uuid.is_nil()
    {
        return Some(ResolvedUiSound::Grid(AssetKey::from(uuid)));
    }
    // 2. Skin / theme override.
    if let Some(skin) = skin {
        match skin.get(sound) {
            SkinUiSound::Grid(bits) => {
                let uuid = Uuid::from_u128(*bits);
                if !uuid.is_nil() {
                    return Some(ResolvedUiSound::Grid(AssetKey::from(uuid)));
                }
            }
            SkinUiSound::File(handle) => return Some(ResolvedUiSound::Skin(handle.id())),
            SkinUiSound::Unset => {}
        }
    }
    // 3. Reference default.
    let uuid = Uuid::parse_str(sound.default_asset()).ok()?;
    (!uuid.is_nil()).then(|| ResolvedUiSound::Grid(AssetKey::from(uuid)))
}

/// Whether a UI sound is enabled (its persisted toggle, defaulting to the
/// sound's built-in default).
fn is_enabled(settings: &ViewerSettings, sound: UiSound) -> bool {
    settings
        .store()
        .get_bool(&sound.enabled_key())
        .unwrap_or_else(|_| sound.default_enabled())
}

/// Prefetch every enabled UI sound's clip once, after login, so a trigger is not
/// late. The [`SoundCache`] parks requests until the `ViewerAsset` capability and
/// the device sample rate are known and retries them, so this only needs to run
/// once the account settings (which may override an asset) have loaded.
pub(crate) fn prefetch_ui_sounds(
    settings: Option<Res<ViewerSettings>>,
    skin: Query<&SkinUiSounds, With<UiRoot>>,
    mut cache: ResMut<SoundCache>,
    mut done: Local<bool>,
) {
    if *done {
        return;
    }
    let Some(settings) = settings else {
        return;
    };
    if !settings.account_loaded() {
        return;
    }
    let skin = skin.single().ok();
    for sound in UiSound::ALL {
        // Only grid assets are prefetched here; skin-bundled files are decoded by
        // `decode_skin_sounds` once Bevy has loaded them.
        if is_enabled(&settings, sound)
            && let Some(ResolvedUiSound::Grid(asset)) = resolve(&settings, skin, sound)
        {
            cache.request(asset);
        }
    }
    *done = true;
}

/// Decode each skin-bundled UI sound (a `url()` in the active skin) once Bevy has
/// loaded its [`AudioSource`], caching the decoded clip in [`SkinSoundClips`] so
/// the driver can play it through the mixer like any other clip.
pub(crate) fn decode_skin_sounds(
    skin: Query<&SkinUiSounds, With<UiRoot>>,
    audio_assets: Res<Assets<AudioSource>>,
    cache: Res<SoundCache>,
    mut clips: ResMut<SkinSoundClips>,
) {
    let Ok(skin) = skin.single() else {
        return;
    };
    let Some(rate) = cache.device_sample_rate() else {
        return;
    };
    for sound in UiSound::ALL {
        let SkinUiSound::File(handle) = skin.get(sound) else {
            continue;
        };
        let id = handle.id();
        if clips.clips.contains_key(&id) || clips.unavailable.contains(&id) {
            continue;
        }
        let Some(source) = audio_assets.get(handle) else {
            continue; // not loaded yet
        };
        match decode_clip(source.bytes.to_vec(), rate) {
            Ok(clip) => {
                let _previous = clips.clips.insert(id, clip);
            }
            Err(error) => {
                warn!("decoding skin UI sound {id:?}: {error}");
                let _inserted = clips.unavailable.insert(id);
            }
        }
    }
}

/// Play each raised UI sound: honour its enable toggle, resolve its source
/// (user / skin / default), and trigger it on the UI bus once the clip is ready
/// (a not-yet-fetched grid clip is requested and the trigger dropped — prefetch
/// makes this rare).
pub(crate) fn drive_ui_sounds(
    mut events: MessageReader<PlayUiSound>,
    settings: Option<Res<ViewerSettings>>,
    skin: Query<&SkinUiSounds, With<UiRoot>>,
    skin_clips: Res<SkinSoundClips>,
    mut cache: ResMut<SoundCache>,
    mixer: Option<NonSendMut<Mixer>>,
) {
    let (Some(settings), Some(mut mixer)) = (settings, mixer) else {
        events.clear();
        return;
    };
    let skin = skin.single().ok();
    let params = ClipParams {
        bus: Bus::Ui,
        gain: 1.0,
        importance: Importance::Ui,
        looped: false,
    };
    for PlayUiSound(sound) in events.read() {
        if !is_enabled(&settings, *sound) {
            continue;
        }
        match resolve(&settings, skin, *sound) {
            Some(ResolvedUiSound::Grid(asset)) => match cache.clip(asset) {
                Some(clip) => {
                    let _voice = mixer.play_clip(clip, params);
                }
                // Not fetched yet (played before prefetch completed): request it
                // so a repeat plays, and drop this one.
                None => cache.request(asset),
            },
            Some(ResolvedUiSound::Skin(id)) => {
                if let Some(clip) = skin_clips.clips.get(&id) {
                    let _voice = mixer.play_clip(clip, params);
                }
            }
            None => {}
        }
    }
}

/// The UI-sounds plugin: the [`PlayUiSound`] message, the login prefetch, the
/// skin-sound decode, and the per-frame driver. Settings are registered from
/// [`ViewerSettings::load`]; the skin CSS properties are registered from the skin
/// plugin (which owns `bevy_flair`).
pub(crate) struct UiSoundsPlugin;

impl Plugin for UiSoundsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<PlayUiSound>()
            .init_resource::<SkinSoundClips>()
            .add_systems(
                Update,
                (prefetch_ui_sounds, decode_skin_sounds, drive_ui_sounds),
            );
    }
}

// ---------------------------------------------------------------------------
// Skin / theme overrides.
//
// A skin sets a `-sk-uisnd-<key>` CSS property to a `url("file")` (bundled with
// the skin, decoded through our own pipeline onto the UI bus) or a `"uuid"`
// (a grid asset). bevy_flair parses the value (resolving `url()` relative to the
// skin's stylesheet) into a `SkinUiSounds` component on the `:root` (`UiRoot`).
// ---------------------------------------------------------------------------

/// A skin/theme override for one UI sound: a grid asset by UUID, a skin-bundled
/// file loaded as a Bevy [`AudioSource`], or unset (fall through to the default).
#[derive(Reflect, Debug, Clone, PartialEq, Default)]
pub(crate) enum SkinUiSound {
    /// No skin override for this sound.
    #[default]
    Unset,
    /// A grid asset, the UUID stored as raw bits (`Uuid` is not `Reflect`).
    Grid(u128),
    /// A skin-bundled audio file, resolved to a loaded-asset handle.
    File(Handle<AudioSource>),
}

/// The placeholder a `url(...)` parses into: an asset path resolved (relative to
/// the skin's stylesheet) to a [`Handle<AudioSource>`] at `bevy_flair` resolve
/// time, then wrapped as [`SkinUiSound::File`].
#[derive(Reflect, Debug, Clone, PartialEq)]
#[reflect(Placeholder)]
struct SkinUiSoundFile(AssetPathPlaceholder<AudioSource>);

impl Placeholder for SkinUiSoundFile {
    type Error = ParseAssetPathError;
    type ResolvedValue = SkinUiSound;

    fn resolve_placeholder(
        &self,
        context: &mut ResolvePlaceholderContext,
    ) -> Result<Option<SkinUiSound>, ParseAssetPathError> {
        Ok(self.0.resolve_placeholder(context)?.map(SkinUiSound::File))
    }
}

/// The per-UI-sound skin overrides, a `bevy_flair` component written onto the
/// `:root` (`UiRoot`) from the `-sk-uisnd-<key>` CSS properties. One field per
/// [`UiSound`]; unset fields fall through to the reference default.
#[derive(Component, ComponentProperties, Reflect, Default)]
#[properties(auto_insert_remove)]
#[reflect(Default)]
pub(crate) struct SkinUiSounds {
    /// `-sk-uisnd-click`.
    click: SkinUiSound,
    /// `-sk-uisnd-typing`.
    typing: SkinUiSound,
    /// `-sk-uisnd-alert`.
    alert: SkinUiSound,
    /// `-sk-uisnd-invalid-op`.
    invalid_op: SkinUiSound,
    /// `-sk-uisnd-money-up`.
    money_up: SkinUiSound,
    /// `-sk-uisnd-money-down`.
    money_down: SkinUiSound,
    /// `-sk-uisnd-teleport-out`.
    teleport_out: SkinUiSound,
    /// `-sk-uisnd-snapshot`.
    snapshot: SkinUiSound,
    /// `-sk-uisnd-window-open`.
    window_open: SkinUiSound,
    /// `-sk-uisnd-window-close`.
    window_close: SkinUiSound,
    /// `-sk-uisnd-incoming-im`.
    incoming_im: SkinUiSound,
    /// `-sk-uisnd-nearby-chat`.
    nearby_chat: SkinUiSound,
    /// `-sk-uisnd-inventory-offer`.
    inventory_offer: SkinUiSound,
    /// `-sk-uisnd-teleport-offer`.
    teleport_offer: SkinUiSound,
}

impl SkinUiSounds {
    /// The skin override for `sound`.
    const fn get(&self, sound: UiSound) -> &SkinUiSound {
        match sound {
            UiSound::Click => &self.click,
            UiSound::Typing => &self.typing,
            UiSound::Alert => &self.alert,
            UiSound::InvalidOp => &self.invalid_op,
            UiSound::MoneyUp => &self.money_up,
            UiSound::MoneyDown => &self.money_down,
            UiSound::TeleportOut => &self.teleport_out,
            UiSound::Snapshot => &self.snapshot,
            UiSound::WindowOpen => &self.window_open,
            UiSound::WindowClose => &self.window_close,
            UiSound::IncomingIm => &self.incoming_im,
            UiSound::NearbyChat => &self.nearby_chat,
            UiSound::InventoryOffer => &self.inventory_offer,
            UiSound::TeleportOffer => &self.teleport_offer,
        }
    }
}

/// The decoded clips of the active skin's bundled UI sounds, keyed by their
/// loaded-[`AudioSource`] id (see [`decode_skin_sounds`]).
#[derive(Resource, Default)]
pub(crate) struct SkinSoundClips {
    /// Successfully decoded skin sounds.
    clips: HashMap<AssetId<AudioSource>, DecodedClip>,
    /// Skin sounds whose file failed to decode, so it is not retried each frame.
    unavailable: HashSet<AssetId<AudioSource>>,
}

/// The `-sk-uisnd-<key>` CSS property name for each [`SkinUiSounds`] field.
const UI_SOUND_CSS_PROPERTIES: [(&str, &str); 14] = [
    ("-sk-uisnd-click", "click"),
    ("-sk-uisnd-typing", "typing"),
    ("-sk-uisnd-alert", "alert"),
    ("-sk-uisnd-invalid-op", "invalid_op"),
    ("-sk-uisnd-money-up", "money_up"),
    ("-sk-uisnd-money-down", "money_down"),
    ("-sk-uisnd-teleport-out", "teleport_out"),
    ("-sk-uisnd-snapshot", "snapshot"),
    ("-sk-uisnd-window-open", "window_open"),
    ("-sk-uisnd-window-close", "window_close"),
    ("-sk-uisnd-incoming-im", "incoming_im"),
    ("-sk-uisnd-nearby-chat", "nearby_chat"),
    ("-sk-uisnd-inventory-offer", "inventory_offer"),
    ("-sk-uisnd-teleport-offer", "teleport_offer"),
];

/// Parse a `-sk-uisnd-*` value: a valid UUID string is a grid asset; anything
/// else (`url(...)` or a path string) is a skin-bundled file. `bevy_flair`
/// resolves the file placeholder to a handle later, against the skin stylesheet.
fn parse_skin_ui_sound(parser: &mut Parser) -> Result<ReflectValue, CssError> {
    let raw = parser.expect_url_or_string()?;
    let raw = raw.as_ref();
    match Uuid::parse_str(raw) {
        Ok(uuid) => Ok(ReflectValue::new(SkinUiSound::Grid(uuid.as_u128()))),
        Err(_not_a_uuid) => Ok(ReflectValue::new(SkinUiSoundFile(
            AssetPathPlaceholder::new(raw),
        ))),
    }
}

/// Register the `-sk-uisnd-<key>` CSS properties on the `bevy_flair` registries,
/// mapping each onto a [`SkinUiSounds`] field. Called from the skin plugin's
/// `build` (which owns `bevy_flair`), before the CSS loader snapshots the
/// registry.
pub(crate) fn register_skin_sound_properties(app: &mut App) {
    {
        let parse =
            ReflectParseCss(|parser| parse_property_value_with(parser, parse_skin_ui_sound));
        let type_registry = app.world().resource::<AppTypeRegistry>();
        let mut registry = type_registry.write();
        registry.register::<SkinUiSound>();
        if let Some(registration) = registry.get_mut(TypeId::of::<SkinUiSound>()) {
            registration.insert(parse);
        }
    }
    app.register_type::<SkinUiSoundFile>();
    app.register_type::<AssetPathPlaceholder<AudioSource>>();
    app.register_component_properties::<SkinUiSounds>();
    let css = app.world().resource::<CssPropertyRegistry>();
    for (property, field) in UI_SOUND_CSS_PROPERTIES {
        css.register_property(property, SkinUiSounds::property_field_ref(field));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    /// Every UI sound has a unique setting key and a parseable default asset.
    #[test]
    fn keys_unique_and_assets_parse() {
        let mut keys: Vec<&str> = UiSound::ALL.iter().map(|sound| sound.key()).collect();
        let count = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), count, "UI sound keys are unique");
        for sound in UiSound::ALL {
            assert!(
                Uuid::parse_str(sound.default_asset()).is_ok(),
                "default asset for {sound:?} parses"
            );
        }
    }

    /// One `-sk-uisnd-<key>` CSS property per UI sound, names unique, and a fresh
    /// skin override leaves every sound unset (falls through to the default).
    #[test]
    fn skin_css_properties_cover_every_sound() {
        assert_eq!(UI_SOUND_CSS_PROPERTIES.len(), UiSound::ALL.len());
        let mut names: Vec<&str> = UI_SOUND_CSS_PROPERTIES
            .iter()
            .map(|(property, _)| *property)
            .collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), UiSound::ALL.len(), "CSS property names unique");
        let skin = SkinUiSounds::default();
        for sound in UiSound::ALL {
            assert_eq!(*skin.get(sound), SkinUiSound::Unset);
        }
    }

    /// Only the auto-emitted feedback sounds default to enabled.
    #[test]
    fn default_enabled_is_the_wired_set() {
        let enabled: Vec<UiSound> = UiSound::ALL
            .into_iter()
            .filter(|sound| sound.default_enabled())
            .collect();
        assert_eq!(
            enabled,
            vec![
                UiSound::Typing,
                UiSound::MoneyUp,
                UiSound::MoneyDown,
                UiSound::TeleportOut,
                UiSound::Snapshot,
            ]
        );
    }
}
