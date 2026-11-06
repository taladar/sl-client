//! **Legacy whole-parcel media** (`viewer-video-playback`): the parcel's media
//! URL played onto every face that wears the parcel's *media texture* — the
//! pre-media-on-a-prim way a land owner put a film or a web page on a screen,
//! and the reference viewer's `LLViewerParcelMedia` with
//! `LLViewerParcelMediaAutoPlay`.
//!
//! # What plays
//!
//! The agent parcel's media: its URL, MIME type, replaced texture, size and
//! looping, as the parcel's `ParcelProperties` stated them and any later
//! `ParcelMediaUpdate` changed them (the reference writes such an update into
//! its agent parcel, so the newer of the two wins here too). The MIME type picks
//! the engine ([`sl_cef::classify_media`]): a type the reference's
//! `mime_types.xml` knows wins, so `video/*` and the legacy QuickTime default
//! play on the video engine and `text/html` in the browser; an empty one falls
//! back to the URL, where the reference would probe the served type; and
//! `none/none` never plays.
//!
//! While a surface is live, every face whose texture is the media texture shows
//! it instead, fullbright, with the face's own placement — a [`ParcelMediaFace`]
//! on the face entity holds the material it wore, put back when the media stops
//! or the face stops wearing the texture. A face that shows its own media on a
//! prim keeps that. The surface is 2-D audio, as the reference's parcel media
//! is not positional.
//!
//! # When it plays
//!
//! - **Autoplay** — once per parcel entry, after five seconds on the parcel,
//!   with the agent (nearly) standing still and the media texture covering more
//!   than 24 × 24 pixels on screen, if the *Auto-play parcel media* preference is
//!   on (it is off by default, as in the reference). The reference counts the
//!   attempt as made whether or not the preference let it play, and so does this.
//! - **A click** — a *play* click action pauses, resumes or starts it; an *open
//!   media* click on a face showing it does the same (`left_click_actions`).
//! - **The Nearby Media window** — its *Parcel Streaming Media* row.
//! - **A script** — `llParcelMediaCommandList` arrives as a
//!   `ParcelMediaCommandMessage`: stop, pause, unload and a seek always apply;
//!   play and loop (and a seek with nothing loaded) start it only while
//!   *Allow scripts to play parcel media* is on, which Firestorm leaves off by
//!   default.
//!
//! Leaving the parcel (or the region) stops it unless the new parcel names the
//! same media; a change of URL, texture or type restarts it if the type stayed
//! the same, and stops it otherwise — the reference's `update`.
//!
//! # Not here
//!
//! The reference's first-use *ParcelCanPlayMedia* warning, its *ask* autoplay
//! mode (its own settings comment calls it unsupported), and the media filter
//! (`viewer-media-playback-policies`). The *auto scale* flag is not consulted:
//! it chooses the resolution the reference renders a page at (the replaced
//! texture's or the parcel's width and height), not how much of the face the
//! media covers, and the surface here always fills the face.
//!
//! Reference (Firestorm, read-only): `llviewerparcelmedia.cpp`,
//! `llviewerparcelmediaautoplay.cpp`, `lltoolpie.cpp`
//! (`handle_click_action_play`, `handle_click_action_open_media`).

use bevy::camera::visibility::RenderLayers;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use sl_cef::{MediaKind, SurfaceConfig, SurfaceTrust, ValidatedMediaUrl, classify_media};
use sl_client_bevy::{
    ParcelInfo, ParcelMediaCommand, ParcelMediaUpdateInfo, RegionHandle, RegionLocalParcelId,
    ScreenMetrics, SlAgentParcel, SlCurrentRegion, SlEvent, SlIdentity, SlRegion, SlSessionEvent,
    TextureKey, texture_face_uv_transform,
};
use sl_viewer_kit::face_material::FaceMaterial;
use sl_viewer_media::media_engine::{
    MediaEngine, MediaEngineKind, MediaEngineSystems, MediaSurfaceId, MediaSurfaces,
};
use sl_viewer_world_api::{
    AvatarMotion, AvatarState, ParcelMediaControl, ParcelMediaShown, ViewerCamera, on_hud_layer,
};
use sl_viewer_world_objects::objects::FaceTextureDebug;

use crate::media_prim::{MediaFace, media_material, validated_media_url};

/// The settings section the parcel media settings register under (the media
/// section the media-on-a-prim settings share).
const MEDIA_SECTION: &[&str] = &["media"];

/// Whether parcel media starts by itself on arriving at a parcel — the
/// reference's `ParcelMediaAutoPlayEnable` (0 = off, its default; 1 = on; its
/// unsupported *ask* is not offered).
pub const PARCEL_MEDIA_AUTO_PLAY_SETTING: &str = "ParcelMediaAutoPlayEnable";

/// Whether a script may **start** the parcel's media (`llParcelMediaCommandList`
/// play / loop, or a seek with nothing loaded) — Firestorm's
/// `PermAllowScriptedMedia`, off by default. Stop, pause and unload always
/// apply.
pub const SCRIPTED_MEDIA_SETTING: &str = "PermAllowScriptedMedia";

/// Seconds on a parcel before its media may autoplay (`AUTOPLAY_TIME`).
const AUTOPLAY_SECONDS: f32 = 5.0;

/// The on-screen pixel area the media texture must exceed before its media
/// autoplays (`AUTOPLAY_SIZE`, 24 × 24).
const AUTOPLAY_PIXEL_AREA: f32 = 24.0 * 24.0;

/// The agent speed (metres per second) under which media may autoplay
/// (`AUTOPLAY_SPEED`).
const AUTOPLAY_SPEED: f32 = 0.1;

/// How often the faces are re-scanned and autoplay is re-checked, in seconds.
const DRIVE_PERIOD: f32 = 0.5;

/// The paint rate of the parcel's surface: one screen, usually the one being
/// watched, so the media-on-a-prim driver's top tier.
const PARCEL_MEDIA_FPS: u8 = 30;

/// The surface size when the parcel names none (the media-on-a-prim default).
const DEFAULT_SIZE: UVec2 = UVec2::new(1024, 768);

/// `PARCEL_MEDIA_COMMAND_TIME`'s bit in a command's flags.
const COMMAND_TIME_BIT: u32 = 1 << 6;

/// The command bits whose `Command` field is a transport command
/// (`PARCEL_MEDIA_COMMAND_STOP`, `_PAUSE`, `_PLAY`, `_LOOP`, `_UNLOAD`).
const TRANSPORT_BITS: u32 = (1 << 0) | (1 << 1) | (1 << 2) | (1 << 3) | (1 << 8);

// --- Pure model -------------------------------------------------------------

/// A parcel's media, as the player sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParcelMediaParams {
    /// The media URL, `None` when the parcel has none.
    pub url: Option<url::Url>,
    /// The declared MIME type (empty when unset).
    pub mime: String,
    /// The texture the media replaces.
    pub texture: Option<TextureKey>,
    /// The media size the parcel names, `None` for the media's own.
    pub size: Option<UVec2>,
    /// Whether the media loops.
    pub looping: bool,
}

impl ParcelMediaParams {
    /// The media `parcel` states. With no `MediaData` block (the UDP path) the
    /// reference's legacy assumptions hold: the QuickTime type, looping.
    #[must_use]
    pub fn from_parcel(parcel: &ParcelInfo) -> Self {
        let data = parcel
            .media_data
            .clone()
            .unwrap_or_else(sl_client_bevy::ParcelMediaData::legacy_default);
        Self {
            url: parcel.media_url.clone(),
            mime: data.media_type,
            texture: parcel.media_id,
            size: size_from(Some(data.width), Some(data.height)),
            looping: data.looping,
        }
    }

    /// The media a `ParcelMediaUpdate` states.
    #[must_use]
    pub fn from_update(update: &ParcelMediaUpdateInfo) -> Self {
        Self {
            url: update.media_url.clone(),
            mime: update.media_type.clone(),
            texture: update.media_id,
            size: size_from(update.media_width, update.media_height),
            looping: update.media_loop,
        }
    }

    /// The engine the media plays on, `None` when it cannot play — no URL, a
    /// URL the media scheme allowlist refuses, or a `none/none` type.
    #[must_use]
    pub fn engine(&self) -> Option<(ValidatedMediaUrl, MediaEngineKind)> {
        let url = validated_media_url(self.url.as_ref()?)?;
        let kind = match classify_media(&self.mime, &url)? {
            MediaKind::Web => MediaEngineKind::Web,
            MediaKind::Video | MediaKind::Audio => MediaEngineKind::Video,
        };
        Some((url, kind))
    }

    /// Whether the parcel has media that could play (a URL and a type other
    /// than `none/none`).
    #[must_use]
    pub fn has_media(&self) -> bool {
        self.url.is_some() && sl_cef::classify_mime(&self.mime) != sl_cef::MimeDispatch::Never
    }

    /// Whether `other` is the same media: the URL, texture and type the
    /// reference compares (`update`, `play`).
    #[must_use]
    pub fn same_media(&self, other: &Self) -> bool {
        self.url == other.url && self.texture == other.texture && self.mime == other.mime
    }
}

/// A width and height in pixels, `None` unless both are positive.
fn size_from(width: Option<i32>, height: Option<i32>) -> Option<UVec2> {
    let width = u32::try_from(width?).ok().filter(|width| *width > 0)?;
    let height = u32::try_from(height?).ok().filter(|height| *height > 0)?;
    Some(UVec2::new(width.min(4096), height.min(4096)))
}

/// What the player does about live media when the agent's parcel, or its media,
/// changed — the reference's `LLViewerParcelMedia::update`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParcelChange {
    /// Leave it as it is.
    Keep,
    /// Play the new media (restarting the surface when it differs).
    Play,
    /// Stop it.
    Stop,
}

/// What a parcel change means for live media: nothing when none plays (autoplay
/// takes it from there), nothing when the new parcel names the same media, a
/// restart when the media changed but its type did not and the agent did not
/// move to another parcel, and a stop otherwise — including a parcel with no
/// media at all.
#[must_use]
pub fn on_parcel_change(
    live: Option<&ParcelMediaParams>,
    new: Option<&ParcelMediaParams>,
    location_changed: bool,
) -> ParcelChange {
    let Some(live) = live else {
        return ParcelChange::Keep;
    };
    let Some(new) = new.filter(|new| new.has_media()) else {
        return ParcelChange::Stop;
    };
    if live.same_media(new) {
        ParcelChange::Keep
    } else if live.mime == new.mime && !location_changed {
        ParcelChange::Play
    } else {
        ParcelChange::Stop
    }
}

/// One step a script's `ParcelMediaCommandMessage` asks of the player.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CommandStep {
    /// Stop and unload.
    Stop,
    /// Pause.
    Pause,
    /// Resume a paused surface.
    Resume,
    /// Start the parcel's media (a script start, gated by
    /// [`SCRIPTED_MEDIA_SETTING`]).
    Play,
    /// Seek to this many seconds.
    Seek(f32),
}

/// The steps a `ParcelMediaCommandMessage` of `flags`, `command` and `time`
/// asks for, given whether the surface is `paused` and whether one is `live` —
/// the reference's `processParcelMediaCommandMessage`. Play and loop resume a
/// paused surface and start anything else; a seek starts the media first when
/// nothing is loaded.
#[must_use]
pub fn command_steps(
    flags: u32,
    command: ParcelMediaCommand,
    time: f32,
    paused: bool,
    live: bool,
) -> Vec<CommandStep> {
    let mut steps = Vec::new();
    if flags & TRANSPORT_BITS != 0 {
        match command {
            ParcelMediaCommand::Stop | ParcelMediaCommand::Unload => steps.push(CommandStep::Stop),
            ParcelMediaCommand::Pause => steps.push(CommandStep::Pause),
            ParcelMediaCommand::Play | ParcelMediaCommand::Loop => steps.push(if paused {
                CommandStep::Resume
            } else {
                CommandStep::Play
            }),
            _other => {}
        }
    }
    if flags & COMMAND_TIME_BIT != 0 {
        if !live {
            steps.push(CommandStep::Play);
        }
        steps.push(CommandStep::Seek(time));
    }
    steps
}

/// Where the agent is, as far as parcel media cares: the region and the parcel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParcelLocation {
    /// The agent's region.
    pub region: RegionHandle,
    /// The parcel within it.
    pub parcel: RegionLocalParcelId,
}

/// The once-per-parcel autoplay bookkeeping — the reference's
/// `LLViewerParcelMediaAutoPlay`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct AutoplayClock {
    /// The parcel the clock runs for.
    location: Option<ParcelLocation>,
    /// Seconds on it so far.
    seconds: f32,
    /// Whether this parcel's attempt has been made (or its media played).
    played: bool,
}

/// What the autoplay check looks at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AutoplayView {
    /// Whether the parcel media is already loaded.
    pub live: bool,
    /// The agent's speed in metres per second.
    pub speed: f32,
    /// The largest on-screen pixel area any face wearing the media texture
    /// covers.
    pub texture_area: f32,
}

impl AutoplayClock {
    /// Count `seconds` more on `location`, starting over on a new parcel.
    pub fn tick(&mut self, location: Option<ParcelLocation>, seconds: f32) {
        if self.location != location {
            *self = Self {
                location,
                seconds: 0.0,
                played: false,
            };
        }
        self.seconds += seconds;
    }

    /// Note that the parcel's media played, so it does not autoplay again here.
    pub const fn mark_played(&mut self) {
        self.played = true;
    }

    /// Whether the conditions for this parcel's attempt hold, and if so make it:
    /// returns `true` exactly once per parcel, when the agent has been on it long
    /// enough, stands (nearly) still, and sees enough of the media texture, while
    /// nothing plays. Whether it then plays is the preference's call.
    pub fn attempt(&mut self, params: Option<&ParcelMediaParams>, view: AutoplayView) -> bool {
        let ready = !self.played
            && self.seconds > AUTOPLAY_SECONDS
            && !view.live
            && params.is_some_and(|params| params.has_media() && params.texture.is_some())
            && view.speed < AUTOPLAY_SPEED
            && view.texture_area > AUTOPLAY_PIXEL_AREA;
        if ready {
            self.played = true;
        }
        ready
    }
}

// --- Runtime state ----------------------------------------------------------

/// The live surface of the parcel's media.
#[derive(Debug, Clone)]
pub struct LiveParcelMedia {
    /// Which engine it runs on.
    pub kind: MediaEngineKind,
    /// The engine surface.
    pub surface: MediaSurfaceId,
    /// The media it was started for.
    pub params: ParcelMediaParams,
}

/// The parcel media player: the agent parcel's media, the live surface and the
/// autoplay clock.
#[derive(Resource, Debug, Default)]
pub struct ParcelMediaPlayer {
    /// The agent parcel's media as last stated (properties or update).
    params: Option<ParcelMediaParams>,
    /// The media the agent parcel's properties last stated, to tell a fresh
    /// properties change from an unchanged mirror.
    from_properties: Option<ParcelMediaParams>,
    /// Where the agent is.
    location: Option<ParcelLocation>,
    /// The live surface.
    live: Option<LiveParcelMedia>,
    /// The autoplay bookkeeping.
    autoplay: AutoplayClock,
    /// Set when something changed that the faces should follow this frame
    /// rather than at the next tick.
    urgent: bool,
}

impl ParcelMediaPlayer {
    /// The agent parcel's media, if it has any that could play.
    #[must_use]
    pub fn media(&self) -> Option<&ParcelMediaParams> {
        self.params.as_ref().filter(|params| params.has_media())
    }

    /// The live surface, if the media is loaded.
    #[must_use]
    pub const fn live(&self) -> Option<&LiveParcelMedia> {
        self.live.as_ref()
    }
}

/// On a face entity showing the parcel's media: the material it wore before,
/// and what the media material was built against.
#[derive(Component, Debug, Clone)]
pub struct ParcelMediaFace {
    /// The face's own material, put back when the media leaves it.
    restore: Handle<FaceMaterial>,
    /// The media material the face wears.
    material: Handle<FaceMaterial>,
    /// The surface size the material was built against (a resize rebuilds it,
    /// the one reliable way to rebind a re-allocated image).
    applied_size: UVec2,
    /// The texture placement the material was built with.
    applied_uv: bevy::math::Affine2,
}

/// The parcel media plugin.
#[derive(Debug)]
pub struct ParcelMediaPlugin;

impl Plugin for ParcelMediaPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, register_parcel_media_settings)
            .init_resource::<ParcelMediaPlayer>()
            .init_resource::<ParcelMediaShown>()
            .add_message::<ParcelMediaControl>()
            .add_systems(
                Update,
                (follow_parcel_media, paint_parcel_media)
                    .chain()
                    .after(MediaEngineSystems::Pump)
                    .before(crate::media_prim::MediaPrimSystems::Drive),
            );
    }
}

/// Startup: declare the persisted parcel media settings.
fn register_parcel_media_settings(settings: Option<ResMut<sl_viewer_settings::ViewerSettings>>) {
    let Some(mut settings) = settings else {
        return;
    };
    settings.register_in(
        MEDIA_SECTION,
        PARCEL_MEDIA_AUTO_PLAY_SETTING,
        sl_settings::SettingValue::Bool(false),
        "setting-desc-ParcelMediaAutoPlayEnable",
    );
    settings.register_in(
        MEDIA_SECTION,
        SCRIPTED_MEDIA_SETTING,
        sl_settings::SettingValue::Bool(false),
        "setting-desc-PermAllowScriptedMedia",
    );
}

/// The engine side of the player, bundled as one [`SystemParam`]: the player,
/// the engine and its surfaces, the image store a surface allocates in, the
/// painted faces (put back on a stop) and what is published about it.
#[derive(SystemParam)]
struct PlayerStores<'w, 's> {
    /// The player.
    player: ResMut<'w, ParcelMediaPlayer>,
    /// The engine surfaces are created on.
    engine: NonSendMut<'w, MediaEngine>,
    /// The live surfaces.
    surfaces: NonSendMut<'w, MediaSurfaces>,
    /// The image store a surface's texture is allocated in.
    images: ResMut<'w, Assets<Image>>,
    /// The faces showing the media, with what they wear now.
    painted: Query<
        'w,
        's,
        (
            Entity,
            &'static ParcelMediaFace,
            Option<&'static MeshMaterial3d<FaceMaterial>>,
        ),
    >,
    /// What the media is shown on, for the open-media click.
    shown: ResMut<'w, ParcelMediaShown>,
    /// What swaps materials on faces.
    commands: Commands<'w, 's>,
}

impl PlayerStores<'_, '_> {
    /// Whether the live surface is a paused video.
    fn paused(&self) -> bool {
        self.player
            .live
            .as_ref()
            .and_then(|live| self.surfaces.get(live.surface))
            .and_then(|slot| slot.status.playback.as_ref())
            .is_some_and(|playback| playback.state == sl_cef::PlaybackState::Paused)
    }

    /// Whether the live surface is playing (a page always is).
    fn playing(&self) -> bool {
        let Some(slot) = self
            .player
            .live
            .as_ref()
            .and_then(|live| self.surfaces.get(live.surface))
        else {
            return false;
        };
        match slot.kind {
            MediaEngineKind::Web => true,
            MediaEngineKind::Video => slot.status.playback.as_ref().is_some_and(|playback| {
                matches!(
                    playback.state,
                    sl_cef::PlaybackState::Playing
                        | sl_cef::PlaybackState::Buffering
                        | sl_cef::PlaybackState::Loading
                )
            }),
        }
    }

    /// Play the agent parcel's media — the reference's `play`: the same media
    /// already loaded is resumed, other media replaces it.
    fn play(&mut self) {
        let Some(params) = self.player.media().cloned() else {
            return;
        };
        if let Some(live) = &self.player.live {
            if live.params.same_media(&params) {
                if let Some(slot) = self.surfaces.get(live.surface) {
                    slot.surface.play();
                }
                self.player.autoplay.mark_played();
                return;
            }
            self.stop();
        }
        let Some((url, kind)) = params.engine() else {
            return;
        };
        let size = params.size.unwrap_or(DEFAULT_SIZE);
        let config = SurfaceConfig {
            width: size.x,
            height: size.y,
            initial_url: url.clone(),
            trust: SurfaceTrust::InWorld,
            max_fps: PARCEL_MEDIA_FPS,
            muted: false,
            loop_media: params.looping,
        };
        // 2-D: the reference's parcel media is not positional.
        let Some(surface) =
            self.surfaces
                .create_kind(&mut self.engine, &mut self.images, &config, kind, false)
        else {
            return;
        };
        info!("parcel media started at {url} ({kind:?})");
        self.shown.texture = params.texture;
        self.player.live = Some(LiveParcelMedia {
            kind,
            surface,
            params,
        });
        self.player.autoplay.mark_played();
        self.player.urgent = true;
    }

    /// Stop and unload the media, putting every face's own material back.
    fn stop(&mut self) {
        let Some(live) = self.player.live.take() else {
            return;
        };
        info!("parcel media stopped");
        self.surfaces.close(live.surface);
        self.shown.texture = None;
        for (entity, face, worn) in &self.painted {
            restore_face(&mut self.commands, entity, face, worn);
        }
    }

    /// Pause a playing surface.
    fn pause(&self) {
        if let Some(slot) = self
            .player
            .live
            .as_ref()
            .and_then(|live| self.surfaces.get(live.surface))
        {
            slot.surface.pause();
        }
    }

    /// Resume a paused surface.
    fn resume(&mut self) {
        if let Some(slot) = self
            .player
            .live
            .as_ref()
            .and_then(|live| self.surfaces.get(live.surface))
        {
            slot.surface.play();
        }
        self.player.autoplay.mark_played();
    }

    /// Seek the surface to `seconds`.
    fn seek(&self, seconds: f32) {
        if let Some(slot) = self
            .player
            .live
            .as_ref()
            .and_then(|live| self.surfaces.get(live.surface))
        {
            slot.surface.seek(f64::from(seconds));
        }
    }

    /// The reference's `handle_click_action_play`: pause while playing, resume
    /// while paused, else start.
    fn toggle(&mut self) {
        if self.paused() {
            self.resume();
        } else if self.player.live.is_some() && self.playing() {
            self.pause();
        } else {
            self.play();
        }
    }
}

/// Put `entity`'s own material back if it still wears the media's, and forget
/// that it showed the media.
fn restore_face(
    commands: &mut Commands,
    entity: Entity,
    face: &ParcelMediaFace,
    worn: Option<&MeshMaterial3d<FaceMaterial>>,
) {
    let Ok(mut entity_commands) = commands.get_entity(entity) else {
        return;
    };
    entity_commands.remove::<ParcelMediaFace>();
    if worn.is_some_and(|worn| worn.0 == face.material) && face.restore != Handle::default() {
        entity_commands.insert(MeshMaterial3d(face.restore.clone()));
    }
}

/// Where the agent's parcel media comes from, bundled as one [`SystemParam`].
#[derive(SystemParam)]
struct ParcelMediaInputs<'w, 's> {
    /// The protocol events (`ParcelMediaUpdate`, `ParcelMediaCommandMessage`).
    events: MessageReader<'w, 's, SlEvent>,
    /// What clicks and the Nearby Media window ask.
    controls: MessageReader<'w, 's, ParcelMediaControl>,
    /// The agent's parcel.
    parcel: Option<Res<'w, SlAgentParcel>>,
    /// The agent's region.
    regions: Query<'w, 's, &'static SlRegion, With<SlCurrentRegion>>,
    /// The persisted settings (autoplay, scripted media).
    settings: Option<Res<'w, sl_viewer_settings::ViewerSettings>>,
}

impl ParcelMediaInputs<'_, '_> {
    /// The boolean setting `name`, `false` when it cannot be read.
    fn flag(&self, name: &str) -> bool {
        self.settings
            .as_ref()
            .and_then(|settings| settings.store().get_bool(name).ok())
            .unwrap_or(false)
    }
}

/// Follow the agent's parcel and everything that drives its media: parcel
/// changes, `ParcelMediaUpdate`, script commands, and the clicks' and window's
/// [`ParcelMediaControl`]s.
fn follow_parcel_media(mut inputs: ParcelMediaInputs, mut stores: PlayerStores) {
    // The parcel the agent stands on, and the media it states.
    let current = inputs
        .parcel
        .as_ref()
        .and_then(|parcel| parcel.current.as_ref());
    let region = inputs.regions.iter().next().map(|region| region.handle);
    let location = current.zip(region).map(|(parcel, region)| ParcelLocation {
        region,
        parcel: parcel.local_id,
    });
    let from_properties = current.map(ParcelMediaParams::from_parcel);
    let location_changed = location != stores.player.location;
    if location_changed || from_properties != stores.player.from_properties {
        stores.player.location = location;
        stores.player.from_properties.clone_from(&from_properties);
        stores.player.params = from_properties;
        let change = on_parcel_change(
            stores.player.live.as_ref().map(|live| &live.params),
            stores.player.params.as_ref(),
            location_changed,
        );
        match change {
            ParcelChange::Keep => {}
            ParcelChange::Play => stores.play(),
            ParcelChange::Stop => stores.stop(),
        }
    }

    let scripted = inputs.flag(SCRIPTED_MEDIA_SETTING);
    for event in inputs.events.read() {
        match &event.0 {
            SlSessionEvent::ParcelMediaUpdate(update) => {
                let params = ParcelMediaParams::from_update(update);
                if stores.player.params.as_ref() == Some(&params) {
                    continue;
                }
                stores.player.params = Some(params);
                // The reference re-plays (restarting on a change of media) only
                // what is loaded; anything else waits for autoplay or a click.
                if stores.player.live.is_some() {
                    if stores.player.media().is_some() {
                        stores.play();
                    } else {
                        stores.stop();
                    }
                }
            }
            SlSessionEvent::ParcelMediaCommand {
                flags,
                command,
                time,
            } => {
                let steps = command_steps(
                    *flags,
                    *command,
                    *time,
                    stores.paused(),
                    stores.player.live.is_some(),
                );
                for step in steps {
                    match step {
                        CommandStep::Stop => stores.stop(),
                        CommandStep::Pause => stores.pause(),
                        CommandStep::Resume => stores.resume(),
                        CommandStep::Play if scripted => stores.play(),
                        CommandStep::Play => {
                            info!("parcel media: a script asked to play it; scripted media is off");
                        }
                        CommandStep::Seek(seconds) => stores.seek(seconds),
                    }
                }
            }
            _other => {}
        }
    }

    for control in inputs.controls.read() {
        match control {
            ParcelMediaControl::Toggle => stores.toggle(),
            ParcelMediaControl::Play => {
                if stores.paused() {
                    stores.resume();
                } else {
                    stores.play();
                }
            }
            ParcelMediaControl::Pause => stores.pause(),
            ParcelMediaControl::Stop => stores.stop(),
        }
    }
}

/// One prim face as the scan reads it: its texture and placement, where it is,
/// whether it is in view, whether it is on the HUD, whether it shows the parcel
/// media, and what it wears.
type ScannedFace = (
    Entity,
    &'static FaceTextureDebug,
    &'static GlobalTransform,
    &'static ViewVisibility,
    Option<&'static RenderLayers>,
    Option<&'static ParcelMediaFace>,
    Option<&'static MeshMaterial3d<FaceMaterial>>,
);

/// What the face scan reads, bundled as one [`SystemParam`].
#[derive(SystemParam)]
struct FaceScan<'w, 's> {
    /// Every prim face; faces showing their own prim media are left to that.
    faces: Query<'w, 's, ScannedFace, Without<MediaFace>>,
    /// The material store the media material is added to.
    materials: ResMut<'w, Assets<FaceMaterial>>,
    /// The world camera, for the on-screen area.
    camera: Query<'w, 's, (&'static GlobalTransform, &'static Projection), With<ViewerCamera>>,
    /// The window, for the on-screen area.
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
}

/// The own avatar's speed, bundled as one [`SystemParam`].
#[derive(SystemParam)]
struct OwnSpeed<'w, 's> {
    /// Who we are.
    identity: Option<Res<'w, SlIdentity>>,
    /// The avatar model, for the own avatar's body.
    avatars: Option<Res<'w, AvatarState>>,
    /// Each avatar's motion.
    motions: Query<'w, 's, &'static AvatarMotion>,
}

impl OwnSpeed<'_, '_> {
    /// The own avatar's speed in metres per second; zero while it is unknown.
    fn speed(&self) -> f32 {
        self.identity
            .as_ref()
            .and_then(|identity| identity.agent_id)
            .zip(self.avatars.as_ref())
            .and_then(|(own, avatars)| avatars.body_root_of(own))
            .and_then(|anchor| self.motions.get(anchor).ok())
            .map_or(0.0, |motion| motion.sl_velocity().length())
    }
}

/// Twice a second (or at once after a change): run the autoplay check, and put
/// the live surface on every face wearing the media texture — and take it off
/// the ones that stopped.
fn paint_parcel_media(
    (time, mut timer): (Res<Time>, Local<f32>),
    mut stores: PlayerStores,
    mut scan: FaceScan,
    speed: OwnSpeed,
    settings: Option<Res<sl_viewer_settings::ViewerSettings>>,
) {
    let delta = time.delta_secs();
    *timer += delta;
    let location = stores.player.location;
    stores.player.autoplay.tick(location, delta);
    if *timer < DRIVE_PERIOD && !stores.player.urgent {
        return;
    }
    *timer = 0.0;
    stores.player.urgent = false;

    let texture = stores
        .player
        .live
        .as_ref()
        .and_then(|live| live.params.texture)
        .or_else(|| stores.player.media().and_then(|params| params.texture));
    let surface = stores.player.live.as_ref().map(|live| live.surface);
    let metrics = scan
        .camera
        .single()
        .ok()
        .zip(scan.windows.single().ok())
        .and_then(|((transform, projection), window)| match projection {
            Projection::Perspective(perspective) => Some((
                ScreenMetrics::new(window.height(), perspective.fov),
                transform.translation(),
                window.width() * window.height(),
            )),
            _other => None,
        });

    let mut texture_area: f32 = 0.0;
    for (entity, FaceTextureDebug(tf), transform, visibility, layers, painted, worn) in &scan.faces
    {
        let wears = texture.is_some_and(|texture| tf.texture_id == texture);
        if wears && visibility.get() {
            let area = if on_hud_layer(layers) {
                metrics.map_or(0.0, |(_metrics, _camera, screen)| screen)
            } else {
                metrics.map_or(0.0, |(metrics, camera, _screen)| {
                    let (scale, _rotation, translation) = transform.to_scale_rotation_translation();
                    metrics.pixel_area(0.5 * scale.length(), camera.distance(translation))
                })
            };
            texture_area = texture_area.max(area);
        }
        let slot = surface
            .filter(|_live| wears)
            .and_then(|surface| stores.surfaces.get_mut(surface));
        let Some(slot) = slot else {
            if let Some(face) = painted {
                restore_face(&mut stores.commands, entity, face, worn);
            }
            continue;
        };
        let uv = texture_face_uv_transform(tf);
        let wearing_media =
            painted.is_some_and(|face| worn.is_some_and(|worn| worn.0 == face.material));
        let unchanged =
            painted.is_some_and(|face| face.applied_size == slot.size && face.applied_uv == uv);
        if wearing_media && unchanged {
            continue;
        }
        // What the face wears now is its own material, unless it is the media's
        // (a resize or a placement edit keeps the recorded one).
        let restore = match (painted, worn) {
            (Some(face), Some(worn)) if worn.0 == face.material => face.restore.clone(),
            (_painted, Some(worn)) => worn.0.clone(),
            (_painted, None) => Handle::default(),
        };
        let material = media_material(&mut scan.materials, slot, uv);
        if let Ok(mut entity_commands) = stores.commands.get_entity(entity) {
            entity_commands.insert((
                ParcelMediaFace {
                    restore,
                    material: material.clone(),
                    applied_size: slot.size,
                    applied_uv: uv,
                },
                MeshMaterial3d(material),
            ));
        }
    }

    let view = AutoplayView {
        live: stores.player.live.is_some(),
        speed: speed.speed(),
        texture_area,
    };
    let params = stores.player.media().cloned();
    if stores.player.autoplay.attempt(params.as_ref(), view) {
        let enabled = settings
            .as_ref()
            .and_then(|settings| {
                settings
                    .store()
                    .get_bool(PARCEL_MEDIA_AUTO_PLAY_SETTING)
                    .ok()
            })
            .unwrap_or(false);
        if enabled {
            stores.play();
        } else {
            debug!("parcel media would autoplay; the preference is off");
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{ParcelMediaCommand, RegionHandle, RegionLocalParcelId, TextureKey, Uuid};

    use super::{
        AutoplayClock, AutoplayView, CommandStep, ParcelChange, ParcelLocation, ParcelMediaParams,
        command_steps, on_parcel_change,
    };
    use sl_viewer_media::media_engine::MediaEngineKind;

    /// Media at `url` of type `mime` replacing texture `texture`.
    fn media(url: &str, mime: &str, texture: u128) -> Result<ParcelMediaParams, url::ParseError> {
        Ok(ParcelMediaParams {
            url: Some(url::Url::parse(url)?),
            mime: mime.to_owned(),
            texture: Some(TextureKey::from(Uuid::from_u128(texture))),
            size: None,
            looping: true,
        })
    }

    /// Parcel `id` of a fixed region.
    const fn parcel(id: i32) -> Option<ParcelLocation> {
        Some(ParcelLocation {
            region: RegionHandle::from_global(256_000, 256_000),
            parcel: RegionLocalParcelId(id),
        })
    }

    /// A standing agent looking at a big enough media texture.
    const WATCHING: AutoplayView = AutoplayView {
        live: false,
        speed: 0.0,
        texture_area: 10_000.0,
    };

    #[test]
    fn the_declared_type_picks_the_engine() -> Result<(), url::ParseError> {
        let film = media("https://example.com/", "video/vnd.secondlife.qt.legacy", 1)?;
        assert_eq!(
            film.engine().map(|(_url, kind)| kind),
            Some(MediaEngineKind::Video)
        );
        let page = media("https://example.com/clip.mp4", "text/html", 1)?;
        assert_eq!(
            page.engine().map(|(_url, kind)| kind),
            Some(MediaEngineKind::Web)
        );
        let off = media("https://example.com/clip.mp4", "none/none", 1)?;
        assert_eq!(off.engine().map(|(_url, kind)| kind), None);
        assert!(!off.has_media());
        // A scheme the allowlist refuses never plays.
        let local = media("file:///etc/passwd", "text/html", 1)?;
        assert_eq!(local.engine().map(|(_url, kind)| kind), None);
        Ok(())
    }

    #[test]
    fn a_parcel_change_keeps_restarts_or_stops() -> Result<(), url::ParseError> {
        let live = media("https://a.example/film.mp4", "video/mp4", 1)?;
        let same = live.clone();
        let other_film = media("https://a.example/other.mp4", "video/mp4", 1)?;
        let page = media("https://a.example/", "text/html", 1)?;
        // Nothing live: autoplay's business.
        assert_eq!(
            on_parcel_change(None, Some(&page), true),
            ParcelChange::Keep
        );
        // The same media on the next parcel keeps playing.
        assert_eq!(
            on_parcel_change(Some(&live), Some(&same), true),
            ParcelChange::Keep
        );
        // New media of the same type on the same parcel restarts.
        assert_eq!(
            on_parcel_change(Some(&live), Some(&other_film), false),
            ParcelChange::Play
        );
        // ... but not on another parcel, nor with another type.
        assert_eq!(
            on_parcel_change(Some(&live), Some(&other_film), true),
            ParcelChange::Stop
        );
        assert_eq!(
            on_parcel_change(Some(&live), Some(&page), false),
            ParcelChange::Stop
        );
        // A parcel with no media, or none at all, stops it.
        let mut none = live.clone();
        none.url = None;
        assert_eq!(
            on_parcel_change(Some(&live), Some(&none), false),
            ParcelChange::Stop
        );
        assert_eq!(
            on_parcel_change(Some(&live), None, true),
            ParcelChange::Stop
        );
        Ok(())
    }

    #[test]
    fn script_commands_map_to_steps() {
        let stop = 1 << 0;
        let pause = 1 << 1;
        let play = 1 << 2;
        let time = 1 << 6;
        let unload = 1 << 8;
        assert_eq!(
            command_steps(stop, ParcelMediaCommand::Stop, 0.0, false, true),
            vec![CommandStep::Stop]
        );
        assert_eq!(
            command_steps(unload, ParcelMediaCommand::Unload, 0.0, false, true),
            vec![CommandStep::Stop]
        );
        assert_eq!(
            command_steps(pause, ParcelMediaCommand::Pause, 0.0, false, true),
            vec![CommandStep::Pause]
        );
        // Play resumes a paused surface and starts anything else.
        assert_eq!(
            command_steps(play, ParcelMediaCommand::Play, 0.0, true, true),
            vec![CommandStep::Resume]
        );
        assert_eq!(
            command_steps(play, ParcelMediaCommand::Loop, 0.0, false, false),
            vec![CommandStep::Play]
        );
        // A seek loads the media first when nothing is.
        assert_eq!(
            command_steps(time, ParcelMediaCommand::Time, 12.5, false, false),
            vec![CommandStep::Play, CommandStep::Seek(12.5)]
        );
        assert_eq!(
            command_steps(time, ParcelMediaCommand::Time, 3.0, false, true),
            vec![CommandStep::Seek(3.0)]
        );
        // A command whose transport bit is not set carries nothing to do.
        assert_eq!(
            command_steps(0, ParcelMediaCommand::Play, 0.0, false, false),
            Vec::<CommandStep>::new()
        );
        // Play and seek together, as `llParcelMediaCommandList` sends them.
        assert_eq!(
            command_steps(play | time, ParcelMediaCommand::Play, 4.0, false, false),
            vec![CommandStep::Play, CommandStep::Play, CommandStep::Seek(4.0)]
        );
    }

    #[test]
    fn autoplay_waits_five_seconds_once_per_parcel() -> Result<(), url::ParseError> {
        let film = media("https://a.example/film.mp4", "video/mp4", 1)?;
        let mut clock = AutoplayClock::default();
        clock.tick(parcel(1), 4.0);
        assert!(!clock.attempt(Some(&film), WATCHING), "too soon");
        clock.tick(parcel(1), 1.5);
        assert!(clock.attempt(Some(&film), WATCHING), "five seconds in");
        clock.tick(parcel(1), 10.0);
        assert!(!clock.attempt(Some(&film), WATCHING), "only once");
        // A new parcel starts over.
        clock.tick(parcel(2), 6.0);
        assert!(clock.attempt(Some(&film), WATCHING), "the next parcel");
        Ok(())
    }

    #[test]
    fn autoplay_needs_a_still_agent_a_visible_texture_and_media() -> Result<(), url::ParseError> {
        let film = media("https://a.example/film.mp4", "video/mp4", 1)?;
        let mut clock = AutoplayClock::default();
        clock.tick(parcel(1), 6.0);
        let walking = AutoplayView {
            speed: 1.5,
            ..WATCHING
        };
        assert!(!clock.attempt(Some(&film), walking), "walking");
        let small = AutoplayView {
            texture_area: 100.0,
            ..WATCHING
        };
        assert!(!clock.attempt(Some(&film), small), "texture too small");
        let live = AutoplayView {
            live: true,
            ..WATCHING
        };
        assert!(!clock.attempt(Some(&film), live), "already playing");
        let mut untextured = film.clone();
        untextured.texture = None;
        assert!(!clock.attempt(Some(&untextured), WATCHING), "no texture");
        assert!(!clock.attempt(None, WATCHING), "no media");
        // None of those used up the attempt.
        assert!(clock.attempt(Some(&film), WATCHING));
        Ok(())
    }

    #[test]
    fn a_manual_play_counts_as_this_parcels_autoplay() -> Result<(), url::ParseError> {
        let film = media("https://a.example/film.mp4", "video/mp4", 1)?;
        let mut clock = AutoplayClock::default();
        clock.tick(parcel(1), 1.0);
        clock.mark_played();
        clock.tick(parcel(1), 10.0);
        assert!(!clock.attempt(Some(&film), WATCHING));
        Ok(())
    }

    #[test]
    fn an_update_states_its_size_only_when_both_sides_are_set() {
        let update = sl_client_bevy::ParcelMediaUpdateInfo {
            media_url: None,
            media_id: None,
            media_auto_scale: true,
            media_type: "text/html".to_owned(),
            media_desc: String::new(),
            media_width: Some(800),
            media_height: Some(600),
            media_loop: false,
        };
        let params = ParcelMediaParams::from_update(&update);
        assert_eq!(params.size, Some(bevy::math::UVec2::new(800, 600)));
        assert!(!params.has_media(), "no URL");
        let half = sl_client_bevy::ParcelMediaUpdateInfo {
            media_height: None,
            ..update
        };
        assert_eq!(ParcelMediaParams::from_update(&half).size, None);
    }

    /// What a face wears and whether it is marked as showing the parcel media.
    fn worn(
        app: &bevy::app::App,
        face: bevy::ecs::entity::Entity,
    ) -> Result<(bevy::asset::AssetId<super::FaceMaterial>, bool), String> {
        let entity = app.world().entity(face);
        let material = entity
            .get::<bevy::pbr::MeshMaterial3d<super::FaceMaterial>>()
            .ok_or("the face lost its material")?
            .0
            .id();
        Ok((material, entity.contains::<super::ParcelMediaFace>()))
    }

    /// **The parcel's media goes on every face wearing its texture, and comes off
    /// again**: played, a face with the media texture swaps to the media material
    /// (and the player says which texture it covers) while a face with another
    /// texture keeps its own; stopped, the first face wears its own material
    /// again. Driven through the real video engine, which creates a surface
    /// offline.
    #[test]
    fn the_media_covers_its_texture_and_comes_off_on_stop() -> Result<(), String> {
        use bevy::prelude::*;
        use sl_client_bevy::{ParcelMediaUpdateInfo, SlEvent, SlSessionEvent, TextureFace};
        use sl_viewer_world_api::{ParcelMediaControl, ParcelMediaShown};
        use sl_viewer_world_objects::objects::FaceTextureDebug;

        let screen = TextureKey::from(Uuid::from_u128(0x5C2E));
        let other = TextureKey::from(Uuid::from_u128(0x0BE2));
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Image>()
            .init_asset::<super::FaceMaterial>()
            .add_message::<SlEvent>()
            .add_plugins(sl_viewer_media::media_engine::MediaEnginePlugin {
                enabled: false,
                video_enabled: true,
                headless: true,
            })
            .add_plugins(super::ParcelMediaPlugin);
        app.update();

        let own = app
            .world_mut()
            .resource_mut::<Assets<super::FaceMaterial>>()
            .add(sl_viewer_kit::face_material::inert_face_material(
                StandardMaterial::default(),
            ));
        let mut spawn_face = |texture: TextureKey| {
            app.world_mut()
                .spawn((
                    FaceTextureDebug(TextureFace::new(texture)),
                    GlobalTransform::default(),
                    ViewVisibility::default(),
                    MeshMaterial3d(own.clone()),
                ))
                .id()
        };
        let on_screen = spawn_face(screen);
        let elsewhere = spawn_face(other);

        app.world_mut()
            .write_message(SlEvent(SlSessionEvent::ParcelMediaUpdate(
                ParcelMediaUpdateInfo {
                    media_url: url::Url::parse("http://127.0.0.1:9/film.mp4").ok(),
                    media_id: Some(screen),
                    media_auto_scale: true,
                    media_type: "video/mp4".to_owned(),
                    media_desc: String::new(),
                    media_width: None,
                    media_height: None,
                    media_loop: true,
                },
            )));
        app.update();
        assert_eq!(
            worn(&app, on_screen)?,
            (own.id(), false),
            "nothing plays yet"
        );

        app.world_mut().write_message(ParcelMediaControl::Play);
        for _frame in 0..3 {
            app.update();
        }
        if app
            .world()
            .resource::<super::ParcelMediaPlayer>()
            .live()
            .is_none()
        {
            return Err("the video engine started no surface".to_owned());
        }
        let (material, marked) = worn(&app, on_screen)?;
        assert!(marked && material != own.id(), "the screen shows the media");
        assert_eq!(
            worn(&app, elsewhere)?,
            (own.id(), false),
            "another texture keeps its own"
        );
        assert_eq!(
            app.world().resource::<ParcelMediaShown>().texture,
            Some(screen)
        );

        app.world_mut().write_message(ParcelMediaControl::Stop);
        for _frame in 0..3 {
            app.update();
        }
        assert_eq!(
            worn(&app, on_screen)?,
            (own.id(), false),
            "the screen has its own again"
        );
        assert_eq!(app.world().resource::<ParcelMediaShown>().texture, None);
        Ok(())
    }
}
