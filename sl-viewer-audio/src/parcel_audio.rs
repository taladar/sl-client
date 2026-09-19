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
//! **An unresolved parcel is not a parcel without music.** The agent's parcel
//! ([`SlAgentParcel::current`]) is `None` whenever the simulator has not
//! pushed one yet — before login, across a region crossing, through a
//! teleport — and that gap says nothing about the stream. Reading it as "a
//! different parcel, with no URL" is what let a crossing stop the radio and
//! then, on the way back in, forget that the user had pressed stop and start
//! it again. So the policy is expressed over a three-state `ParcelStream`: an
//! `Unknown` parcel changes nothing at all, only a `Resolved` one that really
//! names no URL stops, and the stop decision is remembered as *the URL that
//! was stopped* rather than as a flag a gap can clear.
//!
//! # The controls
//!
//! One right-aligned row in the bottom area's upper stack (the counterpart
//! of the nearby-chat bar on the leading side), always shown:
//! `♫ <now playing / stream host> ⏵/⏹ 🔊/🔇 [volume]`. While the current parcel
//! has no stream URL the row is greyed and its play / mute buttons are
//! disabled — the volume slider stays live, so a user can set it before
//! entering a loud parcel. The inline volume slider and mute drive the shared
//! mixer's **music bus** directly (the same `music_volume` / `music_mute`
//! settings the [volume panel](crate::volume_panel) edits), so the stream has
//! one volume, not a stream-level gain in series with the bus.
//!
//! Audio flows through the shared [`sl_audio`] mixer: the player is handed a
//! music-bus [`MixerStream`] input (2-D, stereo), so the parcel radio shares
//! the one audio device and the one set of buses with every other source
//! (`viewer-gst-audio-mixer-handoff`). The GStreamer engine owns the network
//! and decode; the mixer owns the device.
//!
//! Reference (Firestorm, read-only): `llviewermedia_streamingaudio`,
//! `llviewerparcelmedia`, `llpanelnearbymedia`.

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::{Activate, Button, SliderRange, SliderStep};
use sl_audio::{Bus, Mixer};
use sl_client_bevy::SlAgentParcel;
use sl_gst::{AudioStreamPlayer, AudioStreamState, ValidatedMediaUrl};

use crate::media_audio::MixerStream;
use crate::media_diagnostics::MediaDiagnostics;
use crate::settings::ViewerSettings;
use crate::settings_binding::{SettingBinding, bound_slider};
use crate::ui::BottomArea;
use crate::ui::row;
use crate::ui_element::{ElementCx, UiAction};
use crate::ui_font::UiFont;
use crate::ui_slider::{SliderStyle, SliderWidgetPlugin, spawn_slider};
use crate::volume_panel::{bus_mute_setting, bus_volume_setting};

/// The `element` the bar attributes its actions to.
pub(crate) const PARCEL_AUDIO_ELEMENT: &str = "parcel-audio";

/// The persisted-settings section the audio settings live under.
const AUDIO_SECTION: &[&str] = &["audio"];

/// Whether the parcel music stream starts automatically (the reference's
/// streaming-music preference). Public so the preferences audio tab can bind
/// its streaming row to the same setting.
pub const MUSIC_ENABLED_SETTING: &str = "MusicStreamEnabled";

/// The control cluster's font size, in logical pixels.
const BAR_FONT_SIZE: f32 = 12.0;

/// The widest the now-playing title may grow, in logical pixels (clipped
/// beyond).
const TITLE_MAX_WIDTH: f32 = 260.0;

/// How the stream volume slider is drawn — the volume panel's own style, since
/// the two sit side by side in the same bar and share a bus.
const SLIDER: SliderStyle = SliderStyle {
    track_width: 90.0,
    track_height: 12.0,
    border: 1.0,
    border_color: BUTTON_BORDER,
    track_fill: TRACK_FILL,
    thumb_width: 10.0,
    thumb_fill: THUMB_FILL,
};

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
/// Button border while the cluster is disabled (no parcel stream).
const BUTTON_BORDER_DISABLED: Color = Color::srgb(0.2, 0.2, 0.22);
/// Button fill while the cluster is disabled (no parcel stream).
const BUTTON_FILL_DISABLED: Color = Color::srgb(0.11, 0.12, 0.14);
/// The slider track's fill.
const TRACK_FILL: Color = Color::srgb(0.16, 0.19, 0.25);
/// The slider thumb's fill.
const THUMB_FILL: Color = Color::srgb(0.62, 0.72, 0.86);

/// What the grid currently says about the agent's parcel and its music stream.
///
/// The distinction the autoplay policy turns on: "I do not know which parcel
/// the agent is on" and "the agent is on a parcel that has no music" are
/// different facts, and only the second one should stop a stream or re-arm
/// autoplay.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ParcelStream {
    /// The agent's parcel has not resolved — before login, across a region
    /// crossing, through a teleport, or in any gap before the simulator's
    /// parcel push arrives. Carries no information about the stream.
    Unknown,
    /// The parcel resolved and named this music URL (`None`: it has none).
    Resolved(Option<url::Url>),
}

impl ParcelStream {
    /// Read the agent-parcel mirror. A missing resource (no session yet) and an
    /// unresolved parcel are the same fact: [`Unknown`](Self::Unknown).
    fn from_agent_parcel(parcel: Option<&SlAgentParcel>) -> Self {
        parcel
            .and_then(|parcel| parcel.current.as_ref())
            .map_or(Self::Unknown, |parcel| {
                Self::Resolved(parcel.music_url.clone())
            })
    }
}

/// What the decision below asks of the player.
#[derive(Debug, Clone, PartialEq, Eq)]
enum StreamAction {
    /// Start (or restart) the player on this stream.
    Play(ValidatedMediaUrl),
    /// Stop the player.
    Stop,
    /// Leave the player exactly as it is.
    Nothing,
}

/// The autoplay decision, with no player and no ECS in it: what the viewer
/// knows about the parcel's stream, which stream the user stopped, and the
/// enabled flag it last acted on. Every transition returns the
/// [`StreamAction`] the player should be given, so the policy is testable
/// directly rather than only through a running grid.
#[derive(Debug, Default, PartialEq, Eq)]
struct AutoplayState {
    /// The resolved parcel's music URL, once it passed the media scheme
    /// allowlist — the stream the player is (or would be) playing. A parcel
    /// whose URL the allowlist refuses reads as no stream at all.
    parcel_url: Option<ValidatedMediaUrl>,
    /// The resolved parcel's music URL as the grid sent it, the change detector
    /// for the above: validation (and its log line) then runs once per parcel
    /// switch rather than once per frame. Only ever written from a
    /// [`ParcelStream::Resolved`], so an unresolved parcel leaves the last
    /// known stream standing.
    parcel_url_raw: Option<url::Url>,
    /// The stream URL the user pressed **stop** on, if any: autoplay stays off
    /// for exactly that URL. Holding the URL rather than a bare flag is what
    /// makes the decision survive an unresolved-parcel gap — there is nothing
    /// for the gap to clear, and the comparison still re-arms autoplay for a
    /// genuinely different stream.
    stopped_url: Option<url::Url>,
    /// The enabled flag last seen (a change detector for the setting).
    applied_enabled: Option<bool>,
}

impl AutoplayState {
    /// Whether autoplay is armed for the currently known stream — i.e. the user
    /// has not pressed stop on *this* URL.
    fn autoplay_armed(&self) -> bool {
        self.stopped_url != self.parcel_url_raw
    }

    /// The agent's parcel, as of this frame. An [`Unknown`](ParcelStream::Unknown)
    /// parcel and a re-delivery of the URL already known both change nothing;
    /// a genuinely different stream re-arms autoplay and starts (or, when the
    /// new parcel has none or the setting is off, stops) the player.
    fn observe_parcel(&mut self, observed: &ParcelStream, enabled: bool) -> StreamAction {
        let ParcelStream::Resolved(parcel_url) = observed else {
            return StreamAction::Nothing;
        };
        if *parcel_url == self.parcel_url_raw {
            return StreamAction::Nothing;
        }
        debug!("parcel music stream now {parcel_url:?}");
        // The music URL is whatever the land owner typed: a `file://` one
        // would have `uridecodebin` open a local file on this machine.
        self.parcel_url = parcel_url.as_ref().and_then(|url| {
            ValidatedMediaUrl::from_url(url)
                .inspect_err(|error| warn!("parcel music URL not played: {error}"))
                .ok()
        });
        self.parcel_url_raw.clone_from(parcel_url);
        match self.parcel_url.clone() {
            Some(url) if enabled && self.autoplay_armed() => StreamAction::Play(url),
            _stopped_or_none_or_disabled => StreamAction::Stop,
        }
    }

    /// The `MusicStreamEnabled` setting, as of this frame: off stops the
    /// stream, on re-starts it (unless the user stopped this URL themselves).
    /// The first sight of the setting is not a flip — it is what the state was
    /// initialised to, and must not start anything on its own.
    fn observe_enabled(&mut self, enabled: bool, running: bool) -> StreamAction {
        if self.applied_enabled == Some(enabled) {
            return StreamAction::Nothing;
        }
        let first_sight = self.applied_enabled.is_none();
        self.applied_enabled = Some(enabled);
        if first_sight {
            return StreamAction::Nothing;
        }
        if enabled {
            match self.parcel_url.clone() {
                Some(url) if !running && self.autoplay_armed() => StreamAction::Play(url),
                _running_or_stopped_or_none => StreamAction::Nothing,
            }
        } else if running {
            StreamAction::Stop
        } else {
            StreamAction::Nothing
        }
    }

    /// The play / stop button. Stopping remembers *this* URL as the one the
    /// user silenced; starting forgets it, so an explicit play works even with
    /// the autoplay setting off.
    fn toggle_play(&mut self, running: bool) -> StreamAction {
        if running {
            self.stopped_url = self.parcel_url_raw.clone();
            return StreamAction::Stop;
        }
        match self.parcel_url.clone() {
            Some(url) => {
                self.stopped_url = None;
                StreamAction::Play(url)
            }
            None => StreamAction::Nothing,
        }
    }
}

/// The parcel stream player and its autoplay bookkeeping.
#[derive(Resource, Default)]
pub(crate) struct ParcelAudio {
    /// The GStreamer stream player.
    player: AudioStreamPlayer,
    /// The autoplay policy's state — the decisions, without the player.
    state: AutoplayState,
    /// The player's bridge into the mixer's **music** bus (2-D, stereo). Opened
    /// lazily once the mixer exists and the sink is attached to the player.
    audio: Option<MixerStream>,
}

impl ParcelAudio {
    /// Hand a decision to the player.
    fn apply(&mut self, action: StreamAction) {
        match action {
            StreamAction::Play(url) => self.player.play(&url),
            StreamAction::Stop => self.player.stop(),
            StreamAction::Nothing => {}
        }
    }

    /// Whether the player is running (or trying to run).
    const fn running(&self) -> bool {
        stream_running(self.player.status().state)
    }
}

/// The bar's entities.
#[derive(Resource)]
struct ParcelAudioUi {
    /// The leading `♫` glyph.
    marker: Entity,
    /// The play / stop glyph label.
    play_label: Entity,
    /// The play / stop button (disabled while the parcel has no stream).
    play_button: Entity,
    /// The mute glyph label.
    mute_label: Entity,
    /// The mute button (disabled while the parcel has no stream).
    mute_button: Entity,
    /// The now-playing / status text.
    title: Entity,
}

/// The parcel streaming-audio plugin.
#[derive(Debug)]
pub struct ParcelAudioPlugin;

impl Plugin for ParcelAudioPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<SliderWidgetPlugin>() {
            app.add_plugins(SliderWidgetPlugin);
        }
        app.init_resource::<ParcelAudio>()
            .add_systems(Startup, register_parcel_audio_settings)
            .add_systems(
                Update,
                (
                    spawn_parcel_audio_bar,
                    drive_parcel_audio,
                    handle_parcel_audio_actions,
                    request_parcel_audio_diagnosis,
                    sync_parcel_audio_ui,
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
    // The stream's volume / mute are the mixer's music bus, registered by the
    // volume panel; the inline slider and mute button below bind those same
    // settings, so there is one music volume, not two in series.
}

/// Spawn the control cluster into the bottom area's upper stack, once (the
/// [`Local`] latch waits for the bottom toolbar's host to exist).
pub(crate) fn spawn_parcel_audio_bar(
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
    // Sit at the trailing edge of the upper row's trailing half (which already
    // right-aligns its children), content-width so it packs beside any sibling
    // cluster the slot hosts (the quick-prefs button) rather than spanning the
    // whole half. The nearby-chat bar owns the leading half beside it; because
    // the two halves are fixed and side by side, showing / hiding this cluster
    // never moves the chat bar.
    let wrapper = commands
        .spawn((
            Node {
                align_items: AlignItems::FlexEnd,
                ..row(Val::ZERO)
            },
            Pickable {
                should_block_lower: false,
                is_hoverable: true,
            },
            Name::new("parcel-audio-bar"),
            ChildOf(area.upper_trailing),
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
    let marker = commands
        .spawn((
            Text::new("♫"),
            UiFont::Sans.at(BAR_FONT_SIZE),
            TextColor(BAR_LABEL),
            Pickable::IGNORE,
            ChildOf(cluster),
        ))
        .id();
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
    let (play_button, play_label) =
        spawn_glyph_button(&mut commands, cluster, "▶", "play-stop", 20);
    let (mute_button, mute_label) =
        spawn_glyph_button(&mut commands, cluster, "🔊", "mute-toggle", 21);
    spawn_slider(
        &mut commands,
        cluster,
        SLIDER,
        22,
        0.0,
        (
            bound_slider(
                // The inline stream volume *is* the music bus (the volume
                // panel's `music_volume`), so the two stay in lockstep.
                SettingBinding::global(bus_volume_setting(Bus::Music)),
                SliderRange::new(0.0, 1.0),
                SliderStep(0.05),
            ),
            Name::new("parcel-audio-volume"),
        ),
    );
    commands.insert_resource(ParcelAudioUi {
        marker,
        play_label,
        play_button,
        mute_label,
        mute_button,
        title,
    });
    *spawned = true;
}

/// One glyph button on the cluster; returns `(button, label)` entities.
fn spawn_glyph_button(
    commands: &mut Commands,
    parent: Entity,
    glyph: &str,
    action: &'static str,
    tab_index: i32,
) -> (Entity, Entity) {
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
    let label = commands
        .spawn((
            Text::new(glyph),
            UiFont::Sans.at(BAR_FONT_SIZE),
            TextColor(BAR_LABEL),
            Pickable::IGNORE,
            ChildOf(button),
        ))
        .id();
    (button, label)
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
    mut mixer: Option<NonSendMut<Mixer>>,
) {
    audio.player.poll();

    // Open the music-bus mixer input the first time the mixer is available and
    // hand its sink to the player, so playback routes through the shared mixer
    // (music bus, 2-D) rather than the sound card. Volume / mute are the music
    // bus itself, so nothing is pushed into the player here.
    if audio.audio.is_none() && mixer.is_some() {
        let (stream, sink) = MixerStream::new(Bus::Music, false);
        audio.player.set_audio_sink(sink);
        audio.audio = Some(stream);
    }
    if let (Some(mixer), Some(stream)) = (mixer.as_deref_mut(), audio.audio.as_mut()) {
        stream.service(mixer);
    }

    let enabled = settings
        .as_ref()
        .and_then(|settings| settings.store().get_bool(MUSIC_ENABLED_SETTING).ok())
        .unwrap_or(false);

    // Parcel switch: a new music URL re-arms autoplay; a parcel that resolves
    // with no URL stops. An *unresolved* parcel does neither.
    let observed = ParcelStream::from_agent_parcel(parcel.as_deref());
    let action = audio.state.observe_parcel(&observed, enabled);
    audio.apply(action);

    // Enabled flips: off stops the stream, on re-starts it (unless the user
    // stopped this URL themselves).
    let running = audio.running();
    let action = audio.state.observe_enabled(enabled, running);
    audio.apply(action);
}

/// Route the cluster's button actions.
fn handle_parcel_audio_actions(
    mut actions: MessageReader<UiAction>,
    mut audio: ResMut<ParcelAudio>,
    mut settings: Option<ResMut<ViewerSettings>>,
) {
    for action in actions.read() {
        if action.element != PARCEL_AUDIO_ELEMENT {
            continue;
        }
        // The play / mute buttons are disabled (greyed) while the parcel has no
        // stream; honour that here too, since `InteractionDisabled` is advisory
        // for these custom buttons.
        if audio.state.parcel_url.is_none() {
            continue;
        }
        match action.action {
            "play-stop" => {
                let running = audio.running();
                let decision = audio.state.toggle_play(running);
                audio.apply(decision);
            }
            "mute-toggle" => {
                // Mute is the music bus (the stream's single volume path); the
                // volume panel's music row reflects the same flip.
                if let Some(settings) = settings.as_mut() {
                    let key = bus_mute_setting(Bus::Music);
                    let now = settings.store().get_bool(&key).unwrap_or(false);
                    settings.set(
                        sl_settings::Scope::Global,
                        &key,
                        sl_settings::SettingValue::Bool(!now),
                    );
                }
            }
            _other => {}
        }
    }
}

/// When the stream player reports a generic HTTP-source failure (GStreamer hid
/// the real DNS / TCP / TLS / HTTP reason — see
/// [`sl_gst::AudioStreamStatus::network_diagnosable`]), ask the shared
/// [`MediaDiagnostics`] cache to probe the URL; the recovered reason is read
/// back in [`sync_parcel_audio_ui`].
fn request_parcel_audio_diagnosis(
    audio: Res<ParcelAudio>,
    mut diagnostics: ResMut<MediaDiagnostics>,
) {
    let status = audio.player.status();
    if status.state == AudioStreamState::Error
        && status.network_diagnosable
        && let Some(url) = status.url.as_deref()
    {
        diagnostics.request(url);
    }
}

/// Sync the cluster's chrome. The bar is always shown; while the parcel has no
/// The panel's chrome, bundled as one
/// [`SystemParam`](bevy::ecs::system::SystemParam): the disable marker the
/// greying is measured against, the labels, their colours, the fills and
/// borders it repaints, and the commands that add or drop the marker.
#[derive(bevy::ecs::system::SystemParam)]
struct ParcelAudioChrome<'w, 's> {
    /// Which widgets already carry the disable marker, so an unchanged one is
    /// left alone.
    disabled: Query<'w, 's, (), With<InteractionDisabled>>,
    /// The panel's labels.
    texts: Query<'w, 's, &'static mut Text>,
    /// Their colours, dimmed while there is no stream.
    text_colors: Query<'w, 's, &'static mut TextColor>,
    /// The button fills.
    fills: Query<'w, 's, &'static mut BackgroundColor>,
    /// Their borders.
    borders: Query<'w, 's, &'static mut BorderColor>,
    /// What adds or drops the disable marker.
    commands: Commands<'w, 's>,
}

/// stream URL it is greyed and its play / mute buttons are disabled (the volume
/// slider stays live, so a user can set it before entering a loud parcel).
fn sync_parcel_audio_ui(
    ui: Option<Res<ParcelAudioUi>>,
    audio: Res<ParcelAudio>,
    diagnostics: Res<MediaDiagnostics>,
    settings: Option<Res<ViewerSettings>>,
    chrome: ParcelAudioChrome,
) {
    let ParcelAudioChrome {
        disabled,
        mut texts,
        mut text_colors,
        mut fills,
        mut borders,
        mut commands,
    } = chrome;
    let Some(ui) = ui else { return };
    let active = audio.state.parcel_url.is_some();
    // The mute glyph reflects the music bus (the stream's mute lives there now).
    let music_muted = settings
        .as_ref()
        .and_then(|settings| {
            settings
                .store()
                .get_bool(&bus_mute_setting(Bus::Music))
                .ok()
        })
        .unwrap_or(false);

    // Grey the tintable glyphs (the ♫ marker and the ▶/■ play glyph; the mute
    // 🔊/🔇 is a colour emoji that ignores tint, so the button chrome carries
    // its greyed cue instead).
    let glyph_color = if active { BAR_LABEL } else { BAR_LABEL_DIM };
    for entity in [ui.marker, ui.play_label] {
        if let Ok(mut color) = text_colors.get_mut(entity)
            && color.0 != glyph_color
        {
            color.0 = glyph_color;
        }
    }

    // Grey and disable the two buttons.
    let (fill, border) = if active {
        (BUTTON_FILL, BUTTON_BORDER)
    } else {
        (BUTTON_FILL_DISABLED, BUTTON_BORDER_DISABLED)
    };
    for button in [ui.play_button, ui.mute_button] {
        if let Ok(mut background) = fills.get_mut(button)
            && background.0 != fill
        {
            background.0 = fill;
        }
        if let Ok(mut edge) = borders.get_mut(button) {
            let want = BorderColor::all(border);
            if *edge != want {
                *edge = want;
            }
        }
        let is_disabled = disabled.contains(button);
        if active && is_disabled {
            commands.entity(button).remove::<InteractionDisabled>();
        } else if !active && !is_disabled {
            commands.entity(button).insert(InteractionDisabled);
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
        let want = if music_muted { "🔇" } else { "🔊" };
        if mute.0 != want {
            want.clone_into(&mut mute.0);
        }
    }
    if let Ok(mut title) = texts.get_mut(ui.title) {
        // No stream: a plain placeholder. Otherwise the loud path first — only
        // while the stream is actually in error, preferring the precise probed
        // reason over GStreamer's generic one — then the ICY title, then the
        // stream's host as a placeholder.
        let want = if active {
            status.error.clone().map_or_else(
                || {
                    status.title.clone().unwrap_or_else(|| {
                        audio
                            .state
                            .parcel_url
                            .as_ref()
                            .and_then(ValidatedMediaUrl::url)
                            .and_then(|url| url.host_str().map(String::from))
                            .unwrap_or_default()
                    })
                },
                |generic| {
                    status
                        .url
                        .as_deref()
                        .and_then(|url| diagnostics.reason(url))
                        .map_or(generic, String::from)
                },
            )
        } else {
            String::from("No music stream")
        };
        if title.0 != want {
            title.0 = want;
        }
    }
}

/// The gallery specimen: the cluster's resting layout — a sample now-playing
/// title, the play and mute buttons and the volume slider at half — static,
/// so the bar is swept across scripts / sizes / directions like every
/// element ([`crate::ui_element`]).
pub fn spawn_parcel_audio_specimen(
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
    // Static: no `Slider`, so the thumb stays at the half the specimen draws.
    spawn_slider(commands, cluster, SLIDER, 0, 0.5, ());
    cluster
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_gst::ValidatedMediaUrl;

    use super::{AutoplayState, ParcelStream, StreamAction};

    /// Anything a helper below can fail with (a malformed test URL, a scheme
    /// the media allowlist refuses).
    type TestError = Box<dyn core::error::Error>;

    /// A parcel that resolved and names this stream.
    fn on(url: &str) -> Result<ParcelStream, TestError> {
        Ok(ParcelStream::Resolved(Some(url::Url::parse(url)?)))
    }

    /// A parcel that resolved and has no stream.
    const fn silent() -> ParcelStream {
        ParcelStream::Resolved(None)
    }

    /// The action that starts this stream.
    fn playing(url: &str) -> Result<StreamAction, TestError> {
        Ok(StreamAction::Play(ValidatedMediaUrl::parse(url)?))
    }

    /// A state that has already settled on `url` playing, with autoplay on —
    /// the position every parcel-change test starts from.
    fn listening(url: &str) -> Result<AutoplayState, TestError> {
        let mut state = AutoplayState::default();
        assert_eq!(state.observe_enabled(true, false), StreamAction::Nothing);
        assert_eq!(state.observe_parcel(&on(url)?, true), playing(url)?);
        Ok(state)
    }

    /// The same parcel arriving again — every frame, in fact — must not restart
    /// the stream it is already playing.
    #[test]
    fn a_same_url_redelivery_does_nothing() -> Result<(), TestError> {
        let mut state = listening("http://radio.example/stream")?;
        for _redelivery in 0..3 {
            assert_eq!(
                state.observe_parcel(&on("http://radio.example/stream")?, true),
                StreamAction::Nothing
            );
        }
        Ok(())
    }

    /// The bug: a region crossing (or any gap before the simulator's parcel
    /// push) leaves the agent's parcel unresolved for a few frames. That must
    /// neither stop the stream nor forget that the user pressed stop — the old
    /// code read the gap as "a different parcel with no URL", so the stream
    /// stopped and then autoplayed again on the way back in.
    #[test]
    fn an_unresolved_parcel_holds_the_stream_and_the_stop_decision() -> Result<(), TestError> {
        let mut state = listening("http://radio.example/stream")?;
        assert_eq!(state.toggle_play(true), StreamAction::Stop);

        for _frame_of_the_crossing in 0..5 {
            assert_eq!(
                state.observe_parcel(&ParcelStream::Unknown, true),
                StreamAction::Nothing
            );
        }
        // The same parcel resolves again on the far side.
        assert_eq!(
            state.observe_parcel(&on("http://radio.example/stream")?, true),
            StreamAction::Nothing,
            "the user's stop must survive the gap"
        );
        Ok(())
    }

    /// The flip side of the fix: a genuinely different stream still re-arms
    /// autoplay, even when the user stopped the previous one — including when
    /// an unresolved gap sits between the two parcels.
    #[test]
    fn a_different_stream_rearms_autoplay_after_a_stop() -> Result<(), TestError> {
        let mut state = listening("http://radio.example/stream")?;
        assert_eq!(state.toggle_play(true), StreamAction::Stop);
        assert_eq!(
            state.observe_parcel(&ParcelStream::Unknown, true),
            StreamAction::Nothing
        );
        assert_eq!(
            state.observe_parcel(&on("http://other.example/stream")?, true),
            playing("http://other.example/stream")?
        );
        Ok(())
    }

    /// A parcel that really has no music stops the stream — that is a resolved
    /// fact, not a gap — but the stop decision is about a URL, so walking
    /// through such a parcel and back does not re-autoplay what was stopped.
    #[test]
    fn a_parcel_without_music_stops_without_clearing_the_decision() -> Result<(), TestError> {
        let mut state = listening("http://radio.example/stream")?;
        assert_eq!(state.toggle_play(true), StreamAction::Stop);
        assert_eq!(state.observe_parcel(&silent(), true), StreamAction::Stop);
        assert_eq!(
            state.observe_parcel(&on("http://radio.example/stream")?, true),
            StreamAction::Stop,
            "back on the stopped stream: still stopped, not restarted"
        );
        Ok(())
    }

    /// A parcel whose URL the media scheme allowlist refuses reads as no stream
    /// at all — the land owner does not get to point `uridecodebin` at a local
    /// file.
    #[test]
    fn a_refused_scheme_reads_as_no_stream() -> Result<(), TestError> {
        let mut state = listening("http://radio.example/stream")?;
        assert_eq!(
            state.observe_parcel(&on("file:///etc/passwd")?, true),
            StreamAction::Stop
        );
        assert_eq!(state.parcel_url, None);
        Ok(())
    }

    /// With autoplay off nothing starts by itself, but the play button still
    /// does — and having pressed it, the *next* parcel does not autoplay either.
    #[test]
    fn autoplay_off_never_starts_but_the_button_does() -> Result<(), TestError> {
        let mut state = AutoplayState::default();
        assert_eq!(state.observe_enabled(false, false), StreamAction::Nothing);
        assert_eq!(
            state.observe_parcel(&on("http://radio.example/stream")?, false),
            StreamAction::Stop
        );
        assert_eq!(
            state.toggle_play(false),
            playing("http://radio.example/stream")?
        );
        assert_eq!(
            state.observe_parcel(&on("http://other.example/stream")?, false),
            StreamAction::Stop
        );
        Ok(())
    }

    /// The first sight of the setting is the state it was initialised to, not a
    /// flip: seeing `true` at startup must not start a stream on its own.
    #[test]
    fn the_first_sight_of_the_setting_starts_nothing() {
        let mut state = AutoplayState::default();
        assert_eq!(state.observe_enabled(true, false), StreamAction::Nothing);
        assert_eq!(state.observe_enabled(true, false), StreamAction::Nothing);
    }

    /// Turning the setting off stops a running stream; turning it back on
    /// restarts the parcel's stream, unless the user stopped that one.
    #[test]
    fn the_setting_stops_and_restarts_but_honours_a_stop() -> Result<(), TestError> {
        let mut state = listening("http://radio.example/stream")?;
        assert_eq!(state.observe_enabled(false, true), StreamAction::Stop);
        assert_eq!(
            state.observe_enabled(true, false),
            playing("http://radio.example/stream")?
        );

        assert_eq!(state.toggle_play(true), StreamAction::Stop);
        assert_eq!(state.observe_enabled(false, false), StreamAction::Nothing);
        assert_eq!(
            state.observe_enabled(true, false),
            StreamAction::Nothing,
            "the user's stop outlives a setting round-trip"
        );
        Ok(())
    }

    /// Nothing at all is known yet (no session): an unresolved parcel is the
    /// resting state, and it starts nothing.
    #[test]
    fn a_missing_agent_parcel_is_unknown() {
        assert_eq!(ParcelStream::from_agent_parcel(None), ParcelStream::Unknown);
        let mut state = AutoplayState::default();
        assert_eq!(state.observe_enabled(true, false), StreamAction::Nothing);
        assert_eq!(
            state.observe_parcel(&ParcelStream::Unknown, true),
            StreamAction::Nothing
        );
    }
}
