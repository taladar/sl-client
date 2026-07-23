//! **Parcel streaming audio** (`viewer-streaming-audio`): play the parcel's
//! music-stream URL (Shoutcast / Icecast / HLS) through the GStreamer stream
//! player ([`sl_gst::AudioStreamPlayer`]), switching per parcel, with a
//! compact control cluster in the bottom bar — the reference viewer's
//! `LLViewerParcelMedia` + nearby-media music row, reduced to the controls
//! that exist today: play / stop, mute, a volume slider and the stream's
//! "now playing" ICY title.
//!
//! # Autoplay policy
//!
//! The `MusicStreamEnabled` setting (default **off**, persisted globally —
//! world audio is distracting unprompted) is the reference's "play parcel
//! streaming music automatically": when enabled and the agent's parcel
//! resolves to a (new) music URL the stream starts by itself. Pressing
//! **stop** remembers the choice *for that URL* — crossing into a parcel with
//! a different stream re-arms autoplay, exactly so a user fleeing one
//! parcel's radio is not condemned to silence everywhere else. Disabling the
//! setting stops the stream and suppresses all autoplay; the play button
//! still works (an explicit user start).
//!
//! # The controls
//!
//! One right-aligned row in the bottom area's upper stack (the counterpart
//! of the nearby-chat bar on the leading side), shown only while the current
//! parcel has a stream URL: `♫ <now playing / stream host> ⏵/⏹ 🔊/🔇 [volume]`.
//! The volume slider is bound to the persisted `MusicStreamVolume` setting
//! through [`crate::settings_binding`], so the preference survives restarts
//! and any future volume panel ([[viewer-volume-panel]]) edits the same
//! value.
//!
//! Audio goes straight to the system device for now (the `sl-gst` interim —
//! see that crate's docs); when the shared mixer (`viewer-audio-backend`)
//! lands this stream moves onto its music bus unchanged.
//!
//! Reference (Firestorm, read-only): `llviewermedia_streamingaudio`,
//! `llviewerparcelmedia`, `llpanelnearbymedia`.

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::ui_widgets::{
    Activate, Button, Slider, SliderRange, SliderStep, SliderThumb, SliderValue,
};
use sl_client_bevy::SlAgentParcel;
use sl_gst::{AudioStreamPlayer, AudioStreamState};

use crate::bottom_toolbar::BottomArea;
use crate::settings::ViewerSettings;
use crate::settings_binding::{SettingBinding, bound_slider};
use crate::ui::{LogicalInset, LogicalRect, row};
use crate::ui_element::{ElementCx, UiAction};
use crate::ui_font::UiFont;

/// The `element` the bar attributes its actions to.
pub(crate) const PARCEL_AUDIO_ELEMENT: &str = "parcel-audio";

/// The persisted-settings section the audio settings live under.
const AUDIO_SECTION: &[&str] = &["audio"];

/// Whether the parcel music stream starts automatically (the reference's
/// streaming-music preference).
const MUSIC_ENABLED_SETTING: &str = "MusicStreamEnabled";

/// The music-stream volume in `[0, 1]`.
const MUSIC_VOLUME_SETTING: &str = "MusicStreamVolume";

/// The default music volume.
const DEFAULT_MUSIC_VOLUME: f32 = 0.5;

/// The control cluster's font size, in logical pixels.
const BAR_FONT_SIZE: f32 = 12.0;

/// The widest the now-playing title may grow, in logical pixels (clipped
/// beyond).
const TITLE_MAX_WIDTH: f32 = 260.0;

/// The volume slider track's width, in logical pixels.
const VOLUME_TRACK_WIDTH: f32 = 90.0;
/// The volume slider thumb's width, in logical pixels.
const VOLUME_THUMB_WIDTH: f32 = 10.0;
/// The volume slider track / thumb height, in logical pixels.
const VOLUME_TRACK_HEIGHT: f32 = 12.0;

/// The cluster's backdrop (matches the toolbar's dark surface).
const BAR_BACKGROUND: Color = Color::srgba(0.08, 0.09, 0.12, 0.92);
/// Label / glyph colour.
const BAR_LABEL: Color = Color::srgb(0.9, 0.9, 0.92);
/// The dimmed colour for the title / an idle state.
const BAR_LABEL_DIM: Color = Color::srgb(0.62, 0.65, 0.72);
/// Button borders.
const BUTTON_BORDER: Color = Color::srgb(0.3, 0.3, 0.35);
/// Button fill.
const BUTTON_FILL: Color = Color::srgb(0.16, 0.17, 0.2);
/// The slider track's fill.
const TRACK_FILL: Color = Color::srgb(0.16, 0.19, 0.25);
/// The slider thumb's fill.
const THUMB_FILL: Color = Color::srgb(0.62, 0.72, 0.86);

/// The parcel stream player and its autoplay bookkeeping.
#[derive(Resource, Default)]
pub(crate) struct ParcelAudio {
    /// The GStreamer stream player.
    player: AudioStreamPlayer,
    /// The current parcel's music URL, if any.
    parcel_url: Option<String>,
    /// The user stopped this URL's stream; autoplay stays off until the
    /// parcel URL changes.
    user_stopped: bool,
    /// The volume last pushed into the player (a change detector for the
    /// setting).
    applied_volume: Option<f32>,
    /// The enabled flag last seen (a change detector for the setting).
    applied_enabled: Option<bool>,
}

/// The bar's entities.
#[derive(Resource)]
struct ParcelAudioUi {
    /// The whole cluster (hidden while the parcel has no stream).
    wrapper: Entity,
    /// The play / stop glyph label.
    play_label: Entity,
    /// The mute glyph label.
    mute_label: Entity,
    /// The now-playing / status text.
    title: Entity,
}

/// A marker on the volume slider's thumb node, so it slides to the bound
/// value.
#[derive(Component, Debug, Clone, Copy)]
struct VolumeThumb;

/// The parcel streaming-audio plugin.
pub(crate) struct ParcelAudioPlugin;

impl Plugin for ParcelAudioPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ParcelAudio>()
            .add_systems(Startup, register_parcel_audio_settings)
            .add_systems(
                Update,
                (
                    spawn_parcel_audio_bar,
                    drive_parcel_audio,
                    handle_parcel_audio_actions,
                    sync_parcel_audio_ui,
                    drive_volume_thumb,
                )
                    .chain(),
            );
    }
}

/// Startup: declare the persisted audio settings.
fn register_parcel_audio_settings(settings: Option<ResMut<ViewerSettings>>) {
    let Some(mut settings) = settings else {
        return;
    };
    settings.register_in(
        AUDIO_SECTION,
        MUSIC_ENABLED_SETTING,
        sl_settings::SettingValue::Bool(false),
        "Play the parcel's music stream automatically (off by default; the \
         play button on the audio bar starts a stream on demand)",
    );
    settings.register_in(
        AUDIO_SECTION,
        MUSIC_VOLUME_SETTING,
        sl_settings::SettingValue::F32(DEFAULT_MUSIC_VOLUME),
        "Parcel music stream volume (0-1)",
    );
}

/// Spawn the control cluster into the bottom area's upper stack, once (the
/// [`Local`] latch waits for the bottom toolbar's host to exist).
fn spawn_parcel_audio_bar(
    mut commands: Commands,
    area: Option<Res<BottomArea>>,
    mut spawned: Local<bool>,
) {
    if *spawned {
        return;
    }
    let Some(area) = area else {
        return;
    };
    // A full-width, trailing-aligned wrapper: the nearby-chat bar owns the
    // leading half of this stack, the audio cluster sits on the trailing side
    // (mirrored under RTL for free).
    let wrapper = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                justify_content: JustifyContent::FlexEnd,
                // Hidden until the parcel has a stream.
                display: Display::None,
                ..row(Val::ZERO)
            },
            Pickable {
                should_block_lower: false,
                is_hoverable: true,
            },
            Name::new("parcel-audio-bar"),
            ChildOf(area.upper),
        ))
        .id();
    let cluster = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
                ..row(Val::Px(6.0))
            },
            BackgroundColor(BAR_BACKGROUND),
            Pickable {
                should_block_lower: true,
                is_hoverable: true,
            },
            ChildOf(wrapper),
        ))
        .id();
    commands.spawn((
        Text::new("♫"),
        UiFont::Sans.at(BAR_FONT_SIZE),
        TextColor(BAR_LABEL),
        Pickable::IGNORE,
        ChildOf(cluster),
    ));
    let title_clip = commands
        .spawn((
            Node {
                max_width: Val::Px(TITLE_MAX_WIDTH),
                overflow: Overflow::clip(),
                ..row(Val::ZERO)
            },
            ChildOf(cluster),
        ))
        .id();
    let title = commands
        .spawn((
            Text::default(),
            UiFont::Sans.at(BAR_FONT_SIZE),
            TextColor(BAR_LABEL_DIM),
            Pickable::IGNORE,
            ChildOf(title_clip),
        ))
        .id();
    let play_label = spawn_glyph_button(&mut commands, cluster, "▶", "play-stop", 20);
    let mute_label = spawn_glyph_button(&mut commands, cluster, "🔊", "mute-toggle", 21);
    let slider = commands
        .spawn((
            bound_slider(
                SettingBinding::global(MUSIC_VOLUME_SETTING),
                SliderRange::new(0.0, 1.0),
                SliderStep(0.05),
            ),
            Node {
                width: Val::Px(VOLUME_TRACK_WIDTH),
                height: Val::Px(VOLUME_TRACK_HEIGHT),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(BUTTON_BORDER),
            BackgroundColor(TRACK_FILL),
            TabIndex(22),
            Pickable::default(),
            Name::new("parcel-audio-volume"),
            ChildOf(cluster),
        ))
        .id();
    commands.spawn((
        SliderThumb,
        Node {
            position_type: PositionType::Absolute,
            width: Val::Px(VOLUME_THUMB_WIDTH),
            height: Val::Px(VOLUME_TRACK_HEIGHT),
            ..default()
        },
        LogicalInset(LogicalRect {
            inline_start: Val::Px(0.0),
            ..LogicalRect::ZERO
        }),
        BackgroundColor(THUMB_FILL),
        VolumeThumb,
        Pickable::IGNORE,
        ChildOf(slider),
    ));
    commands.insert_resource(ParcelAudioUi {
        wrapper,
        play_label,
        mute_label,
        title,
    });
    *spawned = true;
}

/// One glyph button on the cluster; returns the label entity.
fn spawn_glyph_button(
    commands: &mut Commands,
    parent: Entity,
    glyph: &str,
    action: &'static str,
    tab_index: i32,
) -> Entity {
    let button = commands
        .spawn((
            Button,
            TabIndex(tab_index),
            Node {
                padding: UiRect::axes(Val::Px(6.0), Val::Px(1.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(BUTTON_BORDER),
            BackgroundColor(BUTTON_FILL),
            Pickable::default(),
            Name::new(format!("parcel-audio-button:{action}")),
            ChildOf(parent),
        ))
        .observe(
            move |_activate: On<Activate>, mut actions: MessageWriter<UiAction>| {
                actions.write(UiAction {
                    element: PARCEL_AUDIO_ELEMENT,
                    action,
                });
            },
        )
        .id();
    commands
        .spawn((
            Text::new(glyph),
            UiFont::Sans.at(BAR_FONT_SIZE),
            TextColor(BAR_LABEL),
            Pickable::IGNORE,
            ChildOf(button),
        ))
        .id()
}

/// Whether the player is running (or trying to run) rather than stopped /
/// failed.
const fn stream_running(state: AudioStreamState) -> bool {
    matches!(
        state,
        AudioStreamState::Connecting | AudioStreamState::Buffering | AudioStreamState::Playing
    )
}

/// Per frame: poll the player's bus, follow parcel changes (the autoplay
/// policy in the module docs), and apply the persisted enabled / volume
/// settings.
fn drive_parcel_audio(
    mut audio: ResMut<ParcelAudio>,
    parcel: Option<Res<SlAgentParcel>>,
    settings: Option<Res<ViewerSettings>>,
) {
    audio.player.poll();

    let enabled = settings
        .as_ref()
        .and_then(|settings| settings.store().get_bool(MUSIC_ENABLED_SETTING).ok())
        .unwrap_or(false);
    let volume = settings
        .as_ref()
        .and_then(|settings| settings.store().get_f32(MUSIC_VOLUME_SETTING).ok())
        .unwrap_or(DEFAULT_MUSIC_VOLUME);

    // Volume: push into the player only when the setting changed.
    if audio.applied_volume != Some(volume) {
        audio.applied_volume = Some(volume);
        audio.player.set_volume(f64::from(volume));
    }

    // Parcel switch: a new music URL re-arms autoplay; losing the URL stops.
    let parcel_url = parcel
        .as_ref()
        .and_then(|parcel| parcel.current.as_ref())
        .and_then(|parcel| parcel.music_url.as_ref())
        .map(url::Url::to_string);
    if parcel_url != audio.parcel_url {
        debug!("parcel music stream now {parcel_url:?}");
        audio.parcel_url = parcel_url;
        audio.user_stopped = false;
        match audio.parcel_url.clone() {
            Some(url) if enabled => audio.player.play(&url),
            _none_or_disabled => audio.player.stop(),
        }
    }

    // Enabled flips: off stops the stream, on re-starts it (unless the user
    // stopped this URL themselves).
    if audio.applied_enabled != Some(enabled) {
        let first_sight = audio.applied_enabled.is_none();
        audio.applied_enabled = Some(enabled);
        if !first_sight {
            if enabled {
                if !audio.user_stopped
                    && !stream_running(audio.player.status().state)
                    && let Some(url) = audio.parcel_url.clone()
                {
                    audio.player.play(&url);
                }
            } else if stream_running(audio.player.status().state) {
                audio.player.stop();
            }
        }
    }
}

/// Route the cluster's button actions.
fn handle_parcel_audio_actions(
    mut actions: MessageReader<UiAction>,
    mut audio: ResMut<ParcelAudio>,
) {
    for action in actions.read() {
        if action.element != PARCEL_AUDIO_ELEMENT {
            continue;
        }
        match action.action {
            "play-stop" => {
                if stream_running(audio.player.status().state) {
                    audio.player.stop();
                    audio.user_stopped = true;
                } else if let Some(url) = audio.parcel_url.clone() {
                    audio.user_stopped = false;
                    audio.player.play(&url);
                }
            }
            "mute-toggle" => {
                let muted = audio.player.muted();
                audio.player.set_muted(!muted);
            }
            _other => {}
        }
    }
}

/// Sync the cluster's chrome: visibility, the play / mute glyphs and the
/// now-playing line.
fn sync_parcel_audio_ui(
    ui: Option<Res<ParcelAudioUi>>,
    audio: Res<ParcelAudio>,
    mut nodes: Query<&mut Node>,
    mut texts: Query<&mut Text>,
) {
    let Some(ui) = ui else { return };
    if let Ok(mut node) = nodes.get_mut(ui.wrapper) {
        let want = if audio.parcel_url.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != want {
            node.display = want;
        }
    }
    let status = audio.player.status();
    if let Ok(mut play) = texts.get_mut(ui.play_label) {
        // U+25A0/U+25B6, not U+23F9/U+23F5: the latter are in no bundled
        // font face and render as tofu.
        let want = if stream_running(status.state) {
            "■"
        } else {
            "▶"
        };
        if play.0 != want {
            want.clone_into(&mut play.0);
        }
    }
    if let Ok(mut mute) = texts.get_mut(ui.mute_label) {
        let want = if audio.player.muted() { "🔇" } else { "🔊" };
        if mute.0 != want {
            want.clone_into(&mut mute.0);
        }
    }
    if let Ok(mut title) = texts.get_mut(ui.title) {
        // The loud path first (a missing decoder / dead stream), then the ICY
        // title, then the stream's host as a placeholder.
        let want = status.error.clone().unwrap_or_else(|| {
            status.title.clone().unwrap_or_else(|| {
                audio
                    .parcel_url
                    .as_deref()
                    .and_then(|url| url::Url::parse(url).ok())
                    .and_then(|url| url.host_str().map(String::from))
                    .unwrap_or_default()
            })
        });
        if title.0 != want {
            title.0 = want;
        }
    }
}

/// Keep the volume slider's thumb at the bound value (the value itself is
/// synced from the store by [`crate::settings_binding`]).
fn drive_volume_thumb(
    sliders: Query<(&SliderValue, &SliderRange, &Children), With<Slider>>,
    mut thumbs: Query<&mut LogicalInset, With<VolumeThumb>>,
) {
    for (value, range, children) in &sliders {
        let span = range.span();
        let fraction = if span > f32::EPSILON {
            ((value.0 - range.start()) / span).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let offset = fraction * (VOLUME_TRACK_WIDTH - VOLUME_THUMB_WIDTH);
        for child in children {
            if let Ok(mut inset) = thumbs.get_mut(*child)
                && inset.0.inline_start != Val::Px(offset)
            {
                inset.0.inline_start = Val::Px(offset);
            }
        }
    }
}

/// The gallery specimen: the cluster's resting layout — a sample now-playing
/// title, the play and mute buttons and the volume slider at half — static,
/// so the bar is swept across scripts / sizes / directions like every
/// element ([`crate::ui_element`]).
pub(crate) fn spawn_parcel_audio_specimen(
    commands: &mut Commands,
    parent: Entity,
    cx: ElementCx,
) -> Entity {
    let cluster = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
                ..row(Val::Px(6.0))
            },
            BackgroundColor(BAR_BACKGROUND),
            Name::new("parcel-audio-bar"),
            ChildOf(parent),
        ))
        .id();
    commands.spawn((
        Text::new("♫"),
        cx.font(UiFont::Sans),
        TextColor(BAR_LABEL),
        ChildOf(cluster),
    ));
    let title_clip = commands
        .spawn((
            Node {
                max_width: Val::Px(TITLE_MAX_WIDTH),
                overflow: Overflow::clip(),
                ..row(Val::ZERO)
            },
            crate::ui_element::TextMayClip {
                reason: "the now-playing title is unbounded stream metadata; the cluster caps \
                         its width and clips the tail",
            },
            ChildOf(cluster),
        ))
        .id();
    commands.spawn((
        Text::new(cx.text("Now playing: Synthwave FM")),
        cx.font(UiFont::Sans),
        TextColor(BAR_LABEL_DIM),
        ChildOf(title_clip),
    ));
    for glyph in ["▶", "🔊"] {
        let button = commands
            .spawn((
                Node {
                    padding: UiRect::axes(Val::Px(6.0), Val::Px(1.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                BorderColor::all(BUTTON_BORDER),
                BackgroundColor(BUTTON_FILL),
                ChildOf(cluster),
            ))
            .id();
        commands.spawn((
            Text::new(glyph),
            cx.font(UiFont::Sans),
            TextColor(BAR_LABEL),
            ChildOf(button),
        ));
    }
    let track = commands
        .spawn((
            Node {
                width: Val::Px(VOLUME_TRACK_WIDTH),
                height: Val::Px(VOLUME_TRACK_HEIGHT),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(BUTTON_BORDER),
            BackgroundColor(TRACK_FILL),
            ChildOf(cluster),
        ))
        .id();
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            width: Val::Px(VOLUME_THUMB_WIDTH),
            height: Val::Px(VOLUME_TRACK_HEIGHT),
            ..default()
        },
        LogicalInset(LogicalRect {
            inline_start: Val::Px((VOLUME_TRACK_WIDTH - VOLUME_THUMB_WIDTH) * 0.5),
            ..LogicalRect::ZERO
        }),
        BackgroundColor(THUMB_FILL),
        ChildOf(track),
    ));
    cluster
}
