//! The **Nearby Media** window (`viewer-streaming-audio`): every media source
//! around the agent in one list, each with its own controls — the reference
//! viewer's `LLPanelNearByMedia`, which it opens from the status bar's media
//! button. Here the parcel-audio bar's **▲** opens it.
//!
//! # What it lists
//!
//! - **Parcel Streaming Audio** — the agent's parcel's music stream, while the
//!   parcel has one. It is the parcel-audio bar's player, reached through
//!   [`sl_viewer_media::parcel_stream`] (the player lives in a crate this one
//!   does not depend on), and it heads the list as it does in the reference.
//! - **Every media-on-a-prim face the surface driver ranks** — playing or not,
//!   in the driver's own order (the focused face, then nearest first), each
//!   named by its page title or URL and marked *(playing)* while it holds a
//!   surface.
//!
//! The reference also lists legacy whole-parcel media (`ParcelMediaUpdate`),
//! which this viewer does not play yet (`viewer-video-playback`), so it has no
//! row.
//!
//! The **Show** filter is the reference's: all, the media in this parcel,
//! outside it, or worn by other avatars. "In this parcel" is the reference's
//! `isInAgentParcel` — a face on the agent's region inside the agent parcel's
//! membership bitmap — and the parcel stream counts as in it.
//!
//! # Controls
//!
//! A row's tick is whether it plays: untick to stop it, tick to start it. The
//! selected row's transport sits under the list — stop, play, pause (video),
//! a volume slider and mute, and zoom / unzoom (prim media) — showing exactly
//! the set the reference's `updateControls` shows for that kind of media.
//! **Stop All** / **Start All** do it to every row, the gear opens the audio
//! preferences where the autoplay switches live, and a right-click offers
//! **Copy URL** (and **Copy Data**, for a `data:` page).
//!
//! A stop is the reference's `setDisabled(true)`: the face keeps no surface,
//! whatever auto-play says, until the user starts it again — from here, or with
//! a click on the face ([`crate::media_prim::MediaStartRequests`]).
//!
//! Reference (Firestorm, read-only): `llpanelnearbymedia.cpp`,
//! `panel_nearby_media.xml`, `menu_nearby_media.xml`.

use base64::Engine as _;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::prelude::*;
use bevy::ui::{Checked, InteractionDisabled};
use bevy::ui_widgets::{Activate, Slider, SliderRange, SliderStep, SliderValue, ValueChange};
use bevy_flair::style::components::ClassList;
use sl_client_bevy::{SlAgentParcel, SlIdentity};
use sl_viewer_media::media_engine::{MediaEngineKind, MediaSurfaces};
use sl_viewer_media::parcel_stream::{
    NEARBY_MEDIA_FLOATER_ID, ParcelStreamRequest, ParcelStreamStatus,
    register_parcel_stream_vocabulary,
};
use sl_viewer_ui_core::glyph;
use sl_viewer_ui_core::i18n::{Translated, Translator};
use sl_viewer_ui_core::semantic::{Role, Semantic};
use sl_viewer_ui_core::skin::{SELECTED_CLASS, set_state_class, set_state_class_on, text_role};
use sl_viewer_ui_core::skin_palette::SkinPalette;
use sl_viewer_ui_core::ui::{UiRoot, UiScaffoldSystems, column, row};
use sl_viewer_ui_core::ui_element::{ElementCx, UiAction};
use sl_viewer_ui_core::ui_font::UiFont;
use sl_viewer_ui_core::ui_spawn::{self, ButtonKind, ButtonSpec, UiLabel};
use sl_viewer_ui_core::virtual_list::{VirtualList, VirtualRow, layout_virtual_lists};
use sl_viewer_ui_widgets::floater::{
    DeferredFloaterContent, FloaterCaps, FloaterHandle, FloaterSpec, floater_shown, spawn_floater,
};
use sl_viewer_ui_widgets::menu::{MenuCommand, MenuDef, MenuItemDef, OpenContextMenu};
use sl_viewer_ui_widgets::ui_checkbox::{CheckboxSpec, spawn_checkbox};
use sl_viewer_ui_widgets::ui_combo::{ComboChanged, ComboSelection, ComboSpec, spawn_combo};
use sl_viewer_ui_widgets::ui_slider::{SliderStyle, SliderWidgetPlugin, spawn_slider};
use sl_viewer_ui_widgets::ui_table::{
    TableAlign, TableColumn, TableColumnKind, TableColumnWidth, TableRowCells, TableSelectionMode,
    TableSpec, set_table_cell, spawn_specimen_table_rows, spawn_table, spawn_table_row,
};
use sl_viewer_world_api::{MediaFocus, MediaTarget, ObjectState};

use crate::media_controls::{MediaControlsState, MediaZoomRequest};
use crate::media_prim::{MediaData, MediaPrimState, MediaStartRequests};

/// The [`UiAction`] element the window's buttons and menu report under.
pub const NEARBY_MEDIA_ELEMENT: &str = "nearby-media";

/// The window's font size, in logical pixels.
const FONT_SIZE: f32 = 13.0;

/// A list row's height, in logical pixels.
const ROW_HEIGHT: f32 = 20.0;

/// How often the list is re-read while the window is open, in seconds — the
/// surface driver itself only moves twice a second.
const REFRESH_SECONDS: f32 = 0.25;

/// A row's name while its media plays.
const LABEL_COLOR: Color = SkinPalette::FALLBACK.text_primary;
/// A row's name while it does not (the reference draws those at quarter alpha).
const DIM_LABEL_COLOR: Color = SkinPalette::FALLBACK.text_muted;

/// The condition the context menu's Copy Data line shows under.
const COND_DATA_URL: &str = "data-url";

/// The volume slider — the volume panel's shape, stretched to the row.
const VOLUME_SLIDER: SliderStyle = SliderStyle {
    track_width: 120.0,
    track_height: 12.0,
    border: 1.0,
    border_color: Color::srgb(0.3, 0.3, 0.35),
    track_fill: Color::srgb(0.16, 0.19, 0.25),
    thumb_width: 10.0,
    thumb_fill: Color::srgb(0.62, 0.72, 0.86),
};

/// Column index of the play tick.
const COL_CHECK: usize = 0;
/// Column index of the name.
const COL_NAME: usize = 1;

/// The list: a tick and a name, the reference's two visible columns (its
/// proximity, visibility, class and debug columns show only under
/// `MediaPerformanceManagerDebug`).
static NEARBY_MEDIA_TABLE: TableSpec = TableSpec {
    element: "nearby-media",
    selection: TableSelectionMode::None,
    columns: &[
        TableColumn {
            header_key: "nearby-media-col-playing",
            token: "playing",
            kind: TableColumnKind::Custom,
            width: TableColumnWidth::Fixed { default: 24.0 },
            align: TableAlign::Center,
            sortable: false,
        },
        TableColumn {
            header_key: "nearby-media-col-name",
            token: "name",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Flex(1.0),
            align: TableAlign::Start,
            sortable: false,
        },
    ],
    // The driver's order — parcel stream first, then the focused face, then
    // nearest first — is the reference's proximity sort; clicking a header
    // would scatter it.
    default_sort: &[],
    builtin_sort: false,
    row_height: ROW_HEIGHT,
    font_size: FONT_SIZE,
    header_color: DIM_LABEL_COLOR,
    cell_color: LABEL_COLOR,
    column_gap: 4.0,
    row_padding: 4.0,
    sort_setting: None,
    widths_setting: None,
};

/// The per-row menu — the reference's `menu_nearby_media`.
static NEARBY_MEDIA_MENU: MenuDef = MenuDef {
    label_key: "nearby-media-title",
    items: &[
        MenuItemDef::Command(MenuCommand::new("nearby-media-copy-url", "copy-url")),
        MenuItemDef::Command(
            MenuCommand::new("nearby-media-copy-data", "copy-data").visible_when(COND_DATA_URL),
        ),
    ],
};

// --- Pure view model ------------------------------------------------------

/// Which media a row is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NearbyMediaId {
    /// The agent's parcel's music stream.
    ParcelAudio,
    /// A media-on-a-prim face.
    Prim(MediaTarget),
}

/// Where a prim's media is, relative to the agent — the reference's
/// `MediaClass`, less its *focused* class, which only its debug colouring uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaClass {
    /// On the agent's parcel.
    WithinParcel,
    /// Anywhere else — another parcel, a neighbour region.
    OutsideParcel,
    /// Worn by another avatar.
    OnOthers,
}

/// The **Show** filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MediaListFilter {
    /// Everything.
    #[default]
    All,
    /// The parcel stream and the media on the agent's parcel.
    WithinParcel,
    /// The media off the agent's parcel.
    OutsideParcel,
    /// The media other avatars wear.
    OnOthers,
}

impl MediaListFilter {
    /// The filters, in the combo's order.
    pub const ALL: [Self; 4] = [
        Self::All,
        Self::WithinParcel,
        Self::OutsideParcel,
        Self::OnOthers,
    ];

    /// The Fluent key of the filter's combo label.
    #[must_use]
    pub const fn label_key(self) -> &'static str {
        match self {
            Self::All => "nearby-media-show-all",
            Self::WithinParcel => "nearby-media-show-within-parcel",
            Self::OutsideParcel => "nearby-media-show-outside-parcel",
            Self::OnOthers => "nearby-media-show-on-others",
        }
    }

    /// The filter at combo index `index` (out of range reads as [`All`](Self::All)).
    #[must_use]
    pub fn from_index(index: usize) -> Self {
        Self::ALL.get(index).copied().unwrap_or_default()
    }

    /// Whether a row of `class` shows — `None` is the parcel stream, which the
    /// reference lists under *All* and *In this Parcel* only.
    #[must_use]
    pub const fn admits(self, class: Option<MediaClass>) -> bool {
        match (self, class) {
            (Self::All, _)
            | (Self::WithinParcel, None | Some(MediaClass::WithinParcel))
            | (Self::OutsideParcel, Some(MediaClass::OutsideParcel))
            | (Self::OnOthers, Some(MediaClass::OnOthers)) => true,
            (Self::WithinParcel, Some(MediaClass::OutsideParcel | MediaClass::OnOthers))
            | (Self::OutsideParcel, None | Some(MediaClass::WithinParcel | MediaClass::OnOthers))
            | (Self::OnOthers, None | Some(MediaClass::WithinParcel | MediaClass::OutsideParcel)) => {
                false
            }
        }
    }
}

/// How a row's media is playing, which decides the controls it gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaPlayback {
    /// A prim face with no surface — nothing loaded.
    Idle,
    /// A live web page (no transport of its own).
    Page,
    /// A live video / audio file (a media clock).
    Timed {
        /// Whether it is playing (else paused, ended or failed).
        playing: bool,
    },
    /// The parcel music stream, which starts and stops but cannot pause.
    Stream {
        /// Whether the player is running.
        playing: bool,
    },
}

impl MediaPlayback {
    /// Whether the media holds a surface / a running stream — the reference's
    /// `hasMedia`, drawn as the tick and the *(playing)* mark.
    #[must_use]
    pub const fn is_live(self) -> bool {
        match self {
            Self::Idle => false,
            Self::Page | Self::Timed { .. } => true,
            Self::Stream { playing } => playing,
        }
    }
}

/// One row of the list.
#[derive(Debug, Clone, PartialEq)]
pub struct NearbyMediaRow {
    /// Which media it is.
    pub id: NearbyMediaId,
    /// Its name: the page / stream title, else its URL; empty when it has
    /// neither (drawn as the reference's `<empty>`).
    pub name: String,
    /// The URL the media shows (what Copy URL copies), if any.
    pub url: Option<String>,
    /// Where it is (`None`: the parcel stream).
    pub class: Option<MediaClass>,
    /// How it is playing.
    pub playback: MediaPlayback,
    /// Whether its audio is muted.
    pub muted: bool,
    /// Its volume, linear `[0, 1]`.
    pub volume: f32,
}

/// How one transport control shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ControlShow {
    /// Not there at all.
    #[default]
    Hidden,
    /// There, and refused.
    Disabled,
    /// There, and live.
    Enabled,
}

/// The transport under the list, as the selected row wants it — the
/// reference's `showBasicControls` / `showTimeBasedControls` /
/// `showDisabledControls`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ControlSet {
    /// Stop.
    pub stop: ControlShow,
    /// Play.
    pub play: ControlShow,
    /// Pause.
    pub pause: ControlShow,
    /// The volume slider and mute.
    pub audio: ControlShow,
    /// Zoom onto the face (or, while zoomed onto it, back out).
    pub zoom: ControlShow,
}

/// The transport the selected `row` gets; `zoomed` says whether the camera is
/// zoomed onto it. No selection is the reference's disabled set: a refused
/// stop and zoom, nothing else.
#[must_use]
pub const fn controls_for(row: Option<&NearbyMediaRow>) -> ControlSet {
    let Some(row) = row else {
        return ControlSet {
            stop: ControlShow::Disabled,
            play: ControlShow::Hidden,
            pause: ControlShow::Hidden,
            audio: ControlShow::Hidden,
            zoom: ControlShow::Disabled,
        };
    };
    let zoom = match row.id {
        NearbyMediaId::Prim(_) => ControlShow::Enabled,
        NearbyMediaId::ParcelAudio => ControlShow::Hidden,
    };
    match row.playback {
        MediaPlayback::Idle => ControlSet {
            stop: ControlShow::Hidden,
            play: ControlShow::Enabled,
            pause: ControlShow::Hidden,
            audio: ControlShow::Hidden,
            zoom,
        },
        MediaPlayback::Page => ControlSet {
            stop: ControlShow::Enabled,
            play: ControlShow::Hidden,
            pause: ControlShow::Hidden,
            audio: ControlShow::Enabled,
            zoom,
        },
        MediaPlayback::Timed { playing } => ControlSet {
            stop: ControlShow::Enabled,
            play: shown_if(!playing),
            pause: shown_if(playing),
            audio: ControlShow::Enabled,
            zoom,
        },
        MediaPlayback::Stream { playing } => ControlSet {
            stop: shown_if(playing),
            play: shown_if(!playing),
            pause: ControlShow::Hidden,
            audio: ControlShow::Enabled,
            zoom,
        },
    }
}

/// [`ControlShow::Enabled`] when `on`, else [`ControlShow::Hidden`].
const fn shown_if(on: bool) -> ControlShow {
    if on {
        ControlShow::Enabled
    } else {
        ControlShow::Hidden
    }
}

/// Whether **Stop All** has anything to stop, and **Start All** anything to
/// start — the reference's enable rules for the two buttons.
#[must_use]
pub fn stop_start_all(rows: &[NearbyMediaRow]) -> (bool, bool) {
    let any_live = rows.iter().any(|row| row.playback.is_live());
    let any_idle = rows.iter().any(|row| !row.playback.is_live());
    (any_live, any_idle)
}

/// What **Copy Data** copies from a `data:` URL: the decoded payload of a
/// base64 one, else the URL unescaped — the reference's `copy_data`.
#[must_use]
pub fn copy_data_text(url: &str) -> String {
    const BASE64_MARK: &str = "base64,";
    if let Some(at) = url.find(BASE64_MARK) {
        let payload = url
            .get(at.saturating_add(BASE64_MARK.len())..)
            .unwrap_or("");
        if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(payload.trim()) {
            return String::from_utf8_lossy(&bytes).into_owned();
        }
    }
    percent_encoding::percent_decode_str(url)
        .decode_utf8_lossy()
        .into_owned()
}

/// Whether `url` is a `data:` URL, for which the menu also offers Copy Data.
#[must_use]
pub fn is_data_url(url: &str) -> bool {
    url.get(..5)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("data:"))
}

// --- Resources ------------------------------------------------------------

/// The window's live view: the rows as last read, and the filter.
#[derive(Resource, Debug, Default)]
struct NearbyMediaView {
    /// Every row, before the filter.
    all: Vec<NearbyMediaRow>,
    /// The rows the filter admits, in list order.
    rows: Vec<NearbyMediaRow>,
    /// The **Show** filter.
    filter: MediaListFilter,
}

/// The selected row — module-owned and keyed on the media, so it survives the
/// list reordering as the camera moves.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
struct SelectedNearbyMedia(Option<NearbyMediaId>);

/// The row a context menu was opened on.
#[derive(Resource, Debug, Default, Clone, Copy)]
struct NearbyMediaMenuTarget(Option<NearbyMediaId>);

/// The window's retained entities.
#[derive(Resource, Debug, Clone, Copy)]
struct NearbyMediaUi {
    /// The table root.
    table: Entity,
    /// The virtualized viewport.
    viewport: Entity,
    /// Stop All.
    stop_all: Entity,
    /// Start All.
    start_all: Entity,
    /// The selected row's controls.
    controls: TransportUi,
}

/// The transport row's entities.
#[derive(Debug, Clone, Copy)]
struct TransportUi {
    /// Stop.
    stop: Entity,
    /// Play.
    play: Entity,
    /// Pause.
    pause: Entity,
    /// The volume slider track.
    volume: Entity,
    /// Mute.
    mute: Entity,
    /// Mute's glyph.
    mute_label: Entity,
    /// Zoom / unzoom.
    zoom: Entity,
    /// Zoom's glyph.
    zoom_label: Entity,
}

/// Which row a pooled table row presents.
#[derive(Component, Debug, Clone, Copy, Default)]
struct BoundNearbyMedia(Option<NearbyMediaId>);

/// A row's play tick, naming the row it sits in.
#[derive(Component, Debug, Clone, Copy)]
struct NearbyMediaTick {
    /// The pooled row.
    row: Entity,
}

/// The volume slider, whose change the window routes to the selected row.
#[derive(Component, Debug, Clone, Copy)]
struct NearbyMediaVolume;

/// What the window's controls ask for, gathered into one message so one
/// system acts on it with everything it needs in reach.
#[derive(Message, Debug, Clone, Copy, PartialEq)]
enum NearbyMediaCommand {
    /// Start this row's media.
    Start(NearbyMediaId),
    /// Stop it.
    Stop(NearbyMediaId),
    /// Pause it (video).
    Pause(NearbyMediaId),
    /// Flip its mute.
    ToggleMute(NearbyMediaId),
    /// Set its volume.
    Volume(NearbyMediaId, f32),
}

/// Open the audio preferences, where the media autoplay switches live — the
/// window's gear, the reference's `MediaListCtrl.GoMediaPrefs`. The
/// preferences window answers it; this crate cannot name that window.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenMediaPreferences;

// --- Plugin ---------------------------------------------------------------

/// Registers the Nearby Media window and its systems.
#[derive(Debug, Clone, Copy, Default)]
pub struct NearbyMediaPlugin;

impl Plugin for NearbyMediaPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<SliderWidgetPlugin>() {
            app.add_plugins(SliderWidgetPlugin);
        }
        register_parcel_stream_vocabulary(app);
        app.init_resource::<NearbyMediaView>()
            .init_resource::<SelectedNearbyMedia>()
            .init_resource::<NearbyMediaMenuTarget>()
            .add_message::<NearbyMediaCommand>()
            .add_message::<OpenMediaPreferences>()
            .add_message::<MediaZoomRequest>()
            .add_systems(
                Startup,
                spawn_nearby_media_floater.after(UiScaffoldSystems::SpawnRoot),
            )
            .add_systems(
                Update,
                (
                    apply_nearby_media_filter,
                    follow_media_focus,
                    rebuild_nearby_media_view,
                )
                    .chain()
                    .before(layout_virtual_lists)
                    .after(crate::media_prim::MediaPrimSystems::Drive)
                    .run_if(floater_shown(NEARBY_MEDIA_FLOATER_ID)),
            )
            .add_systems(
                Update,
                (
                    populate_nearby_media_rows,
                    bind_nearby_media_rows,
                    sync_nearby_media_controls,
                )
                    .chain()
                    .after(layout_virtual_lists)
                    .run_if(floater_shown(NEARBY_MEDIA_FLOATER_ID)),
            )
            .add_systems(
                Update,
                (handle_nearby_media_actions, apply_nearby_media_commands)
                    .chain()
                    .before(crate::media_prim::MediaPrimSystems::Drive),
            );
    }
}

// --- Floater --------------------------------------------------------------

/// The window's [`FloaterSpec`] — the reference panel's 328 × 230 at rest.
#[must_use]
pub fn nearby_media_floater_spec() -> FloaterSpec {
    FloaterSpec {
        id: NEARBY_MEDIA_FLOATER_ID,
        title: "Nearby Media".to_owned(),
        position: Vec2::new(520.0, 260.0),
        default_size: Some(Vec2::new(380.0, 300.0)),
        min_size: Some(Vec2::new(328.0, 230.0)),
        dock_host: None,
        caps: FloaterCaps {
            resizable: true,
            minimizable: true,
            closable: true,
            dockable: false,
        },
    }
}

/// Startup: chrome only; the content builds on first open.
fn spawn_nearby_media_floater(mut commands: Commands, root: Res<UiRoot>) {
    let handle = spawn_floater(&mut commands, root.0, nearby_media_floater_spec());
    commands
        .entity(handle.title_text)
        .insert(Translated::new("nearby-media-title"));
    let builder = commands.register_system(build_nearby_media_content);
    commands
        .entity(handle.root)
        .insert(DeferredFloaterContent { builder, handle });
}

/// First-open content build.
fn build_nearby_media_content(In(handle): In<FloaterHandle>, mut commands: Commands) {
    let ui = spawn_nearby_media_content(&mut commands, handle.content, FONT_SIZE);
    commands.insert_resource(ui);
}

/// Build the window's content into `parent`: the Stop All / Start All / gear
/// row, the Show filter, the list and the transport. Shared by the live
/// window's first-open build and its specimen.
fn spawn_nearby_media_content(
    commands: &mut Commands,
    parent: Entity,
    font_size: f32,
) -> NearbyMediaUi {
    let content = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                min_height: Val::Px(0.0),
                padding: UiRect::all(Val::Px(4.0)),
                ..column(Val::Px(6.0))
            },
            Name::new("nearby-media-content"),
            ChildOf(parent),
        ))
        .id();

    // Stop All · Start All · gear — the reference's `minimized_controls`.
    let top = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                flex_shrink: 0.0,
                ..row(Val::Px(4.0))
            },
            ChildOf(content),
        ))
        .id();
    let stop_all = spawn_text_button(commands, top, "nearby-media-stop-all", "stop-all", 1);
    let start_all = spawn_text_button(commands, top, "nearby-media-start-all", "start-all", 2);
    let _prefs = spawn_glyph_button(commands, top, glyph::SETTINGS, "preferences", 3, font_size);

    // Show: [filter]
    let show = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                flex_shrink: 0.0,
                ..row(Val::Px(6.0))
            },
            ChildOf(content),
        ))
        .id();
    commands.spawn((
        Text::default(),
        Translated::new("nearby-media-show"),
        UiFont::Sans.at(font_size),
        text_role(LABEL_COLOR),
        Pickable::IGNORE,
        ChildOf(show),
    ));
    let labels: Vec<String> = MediaListFilter::ALL
        .iter()
        .map(|filter| filter.label_key().to_owned())
        .collect();
    let _combo = spawn_combo(
        commands,
        show,
        &ComboSpec {
            element: NEARBY_MEDIA_ELEMENT,
            labels: &labels,
            active: 0,
            tab_index: 4,
            font_size,
            translate_labels: true,
        },
    );

    let table = spawn_table(commands, content, &NEARBY_MEDIA_TABLE);
    commands.entity(table.viewport).insert(TabIndex(5));

    let controls = spawn_transport(commands, content, font_size);

    NearbyMediaUi {
        table: table.root,
        viewport: table.viewport,
        stop_all,
        start_all,
        controls,
    }
}

/// The transport row under the list.
fn spawn_transport(commands: &mut Commands, parent: Entity, font_size: f32) -> TransportUi {
    let bar = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                flex_shrink: 0.0,
                ..row(Val::Px(4.0))
            },
            Name::new("nearby-media-transport"),
            ChildOf(parent),
        ))
        .id();
    // Stop is the play-stop mark in its playing state (the stop box), play
    // the same mark at rest: one skin slot, the way the parcel bar draws them.
    let (stop, stop_label) =
        spawn_glyph_button(commands, bar, glyph::PLAY_STOP, "stop", 10, font_size);
    add_state_class(commands, stop_label, glyph::PLAYING);
    let (play, _) = spawn_glyph_button(commands, bar, glyph::PLAY_STOP, "play", 11, font_size);
    let (pause, pause_label) =
        spawn_glyph_button(commands, bar, glyph::PLAY_PAUSE, "pause", 12, font_size);
    add_state_class(commands, pause_label, glyph::PLAYING);
    let volume = spawn_slider(
        commands,
        bar,
        VOLUME_SLIDER,
        13,
        0.0,
        (
            Slider::default(),
            SliderValue(0.0),
            SliderRange::new(0.0, 1.0),
            SliderStep(0.05),
            NearbyMediaVolume,
            Name::new("nearby-media-volume"),
            // The slider draws no caption of its own.
            Semantic::new(Role::Slider).name_key("nearby-media-volume-name"),
        ),
    );
    commands.entity(volume).observe(on_volume_change);
    let (mute, mute_label) =
        spawn_glyph_button(commands, bar, glyph::SPEAKER, "mute", 14, font_size);
    let (zoom, zoom_label) = spawn_glyph_button(commands, bar, glyph::ZOOM, "zoom", 15, font_size);
    TransportUi {
        stop,
        play,
        pause,
        volume,
        mute,
        mute_label,
        zoom,
        zoom_label,
    }
}

/// Put the state class `class` on `entity`'s class list once it is spawned —
/// beside the glyph host's own classes, which a fresh `ClassList` would drop.
fn add_state_class(commands: &mut Commands, entity: Entity, class: &'static str) {
    commands
        .entity(entity)
        .queue(move |mut entity: EntityWorldMut| {
            if let Some(mut classes) = entity.get_mut::<ClassList>() {
                set_state_class(&mut classes, class, true);
            }
        });
}

/// A captioned button reporting `action`; returns the button.
fn spawn_text_button(
    commands: &mut Commands,
    parent: Entity,
    label_key: &'static str,
    action: &'static str,
    tab_index: i32,
) -> Entity {
    let spawned = ui_spawn::spawn_button(
        commands,
        parent,
        ButtonSpec::bordered(
            UiLabel::key(label_key),
            format!("nearby-media-button:{action}"),
        )
        .kind(ButtonKind::Headless)
        .tab_index(tab_index)
        .font_size(FONT_SIZE)
        .no_wrap(),
    );
    observe_action(commands, spawned.button, action);
    spawned.button
}

/// A glyph button reporting `action`; returns `(button, glyph label)`.
fn spawn_glyph_button(
    commands: &mut Commands,
    parent: Entity,
    slot: &'static str,
    action: &'static str,
    tab_index: i32,
    font_size: f32,
) -> (Entity, Entity) {
    let spawned = ui_spawn::spawn_button(
        commands,
        parent,
        ButtonSpec::bordered(
            UiLabel::glyph(slot, glyph_button_name_key(action)),
            format!("nearby-media-button:{action}"),
        )
        .kind(ButtonKind::Headless)
        .tab_index(tab_index)
        .compact()
        .label_color(LABEL_COLOR)
        .font_size(font_size),
    );
    observe_action(commands, spawned.button, action);
    (spawned.button, spawned.label)
}

/// The Fluent key naming the glyph button that reports `action`.
fn glyph_button_name_key(action: &'static str) -> &'static str {
    match action {
        "preferences" => "nearby-media-preferences-name",
        "stop" => "nearby-media-stop-name",
        "play" => "nearby-media-play-name",
        "pause" => "nearby-media-pause-name",
        "mute" => "nearby-media-mute-name",
        "zoom" => "nearby-media-zoom-name",
        _ => "",
    }
}

/// Make `button` report `action` under [`NEARBY_MEDIA_ELEMENT`] when pressed.
fn observe_action(commands: &mut Commands, button: Entity, action: &'static str) {
    commands.entity(button).observe(
        move |_activate: On<Activate>, mut actions: MessageWriter<UiAction>| {
            actions.write(UiAction {
                element: NEARBY_MEDIA_ELEMENT,
                action,
            });
        },
    );
}

// --- Gallery specimen -----------------------------------------------------

/// The window's gallery / `ui_test` specimen: the live content, built by the
/// same `spawn_nearby_media_content` the window is, with sample rows bound
/// through the live cell mapping and the transport set for the first row.
pub fn spawn_nearby_media_specimen(
    commands: &mut Commands,
    parent: Entity,
    cx: ElementCx,
) -> Entity {
    let ui = spawn_nearby_media_content(commands, parent, cx.font_size);
    let samples = specimen_rows();
    let values: Vec<Vec<(String, Color)>> = samples
        .iter()
        .map(|sample| {
            let (name, color) = row_name(sample, "Parcel Streaming Audio", "<empty>", "(playing)");
            vec![(String::new(), color), (cx.text(&name), color)]
        })
        .collect();
    let table = sl_viewer_ui_widgets::ui_table::SpecimenTable {
        root: ui.table,
        viewport: ui.viewport,
    };
    let bound = spawn_specimen_table_rows(commands, table, &NEARBY_MEDIA_TABLE, &values);
    for (sample, (row_entity, cells)) in samples.iter().zip(&bound) {
        if let Some(cell) = cells.cell(COL_CHECK) {
            let tick = spawn_row_tick(commands, cell, *row_entity);
            if sample.playback.is_live() {
                commands.entity(tick).insert(Checked);
            }
        }
    }
    parent
}

/// The specimen's rows: the parcel stream playing, a playing page and an idle
/// face.
fn specimen_rows() -> Vec<NearbyMediaRow> {
    let face = |face: u16| {
        NearbyMediaId::Prim(MediaTarget {
            object: sl_client_bevy::ObjectKey::from(sl_client_bevy::Uuid::from_u128(u128::from(
                face,
            ))),
            face: sl_client_bevy::PrimFaceId::new(face),
        })
    };
    vec![
        NearbyMediaRow {
            id: NearbyMediaId::ParcelAudio,
            name: String::new(),
            url: Some("http://radio.example/stream".to_owned()),
            class: None,
            playback: MediaPlayback::Stream { playing: true },
            muted: false,
            volume: 0.3,
        },
        NearbyMediaRow {
            id: face(1),
            name: "Welcome board".to_owned(),
            url: Some("https://example.com/welcome".to_owned()),
            class: Some(MediaClass::WithinParcel),
            playback: MediaPlayback::Page,
            muted: false,
            volume: 1.0,
        },
        NearbyMediaRow {
            id: face(2),
            name: "https://video.example/loop.mp4".to_owned(),
            url: Some("https://video.example/loop.mp4".to_owned()),
            class: Some(MediaClass::OutsideParcel),
            playback: MediaPlayback::Idle,
            muted: false,
            volume: 1.0,
        },
    ]
}

// --- View -----------------------------------------------------------------

/// The lookups a row is read through, bundled as one
/// [`SystemParam`](bevy::ecs::system::SystemParam).
#[derive(bevy::ecs::system::SystemParam)]
struct RowSources<'w, 's> {
    /// The ranked faces and their live surfaces.
    prim_state: Res<'w, MediaPrimState>,
    /// Their media entries.
    data: Res<'w, MediaData>,
    /// The engine's surfaces, for a live face's status and audio.
    surfaces: NonSend<'w, MediaSurfaces>,
    /// The parcel stream.
    parcel_stream: Res<'w, ParcelStreamStatus>,
    /// The object model, for where a face is and who wears it.
    objects: Res<'w, ObjectState>,
    /// World transforms, for a face's position.
    transforms: Query<'w, 's, &'static GlobalTransform>,
    /// The agent's parcel, for "in this parcel".
    parcel: Option<Res<'w, SlAgentParcel>>,
    /// The agent's own id, for "worn by someone else".
    identity: Option<Res<'w, SlIdentity>>,
}

impl RowSources<'_, '_> {
    /// Every row, parcel stream first, then the faces in the driver's order.
    fn rows(&self) -> Vec<NearbyMediaRow> {
        let mut rows = Vec::with_capacity(self.prim_state.nearby.len().saturating_add(1));
        if let Some(url) = &self.parcel_stream.url {
            rows.push(NearbyMediaRow {
                id: NearbyMediaId::ParcelAudio,
                name: String::new(),
                url: Some(url.to_string()),
                class: None,
                playback: MediaPlayback::Stream {
                    playing: self.parcel_stream.running,
                },
                muted: self.parcel_stream.muted,
                volume: self.parcel_stream.volume,
            });
        }
        for face in &self.prim_state.nearby {
            rows.push(self.prim_row(face.target));
        }
        rows
    }

    /// One face's row.
    fn prim_row(&self, target: MediaTarget) -> NearbyMediaRow {
        let entry_url = self.data.entry(target).and_then(|entry| {
            entry
                .current_url
                .as_ref()
                .or(entry.home_url.as_ref())
                .map(ToString::to_string)
        });
        let slot = self
            .prim_state
            .active
            .get(&target)
            .and_then(|active| self.surfaces.get(active.surface));
        let (name, url, playback, muted, volume) = match slot {
            Some(slot) => {
                let status_url = (!slot.status.url.is_empty()).then(|| slot.status.url.clone());
                let url = status_url.or_else(|| entry_url.clone());
                let name = if slot.status.title.is_empty() {
                    url.clone().unwrap_or_default()
                } else {
                    slot.status.title.clone()
                };
                let playback = match slot.kind {
                    MediaEngineKind::Web => MediaPlayback::Page,
                    MediaEngineKind::Video => MediaPlayback::Timed {
                        playing: slot.status.playback.as_ref().is_some_and(|playback| {
                            matches!(
                                playback.state,
                                sl_cef::PlaybackState::Playing
                                    | sl_cef::PlaybackState::Buffering
                                    | sl_cef::PlaybackState::Loading
                            )
                        }),
                    },
                };
                let volume = slot.audio.as_ref().map_or(1.0, |audio| audio.gain());
                (name, url, playback, slot.surface.muted(), volume)
            }
            None => (
                entry_url.clone().unwrap_or_default(),
                entry_url,
                MediaPlayback::Idle,
                false,
                1.0,
            ),
        };
        NearbyMediaRow {
            id: NearbyMediaId::Prim(target),
            name,
            url,
            class: Some(self.class_of(target)),
            playback,
            muted,
            volume,
        }
    }

    /// Where a face's media is — worn by another avatar, else on the agent's
    /// parcel or off it (the reference's `isAttachedToAnotherAvatar` and
    /// `isInAgentParcel`).
    fn class_of(&self, target: MediaTarget) -> MediaClass {
        let own = self
            .identity
            .as_ref()
            .and_then(|identity| identity.agent_id)
            .map(|agent| agent.uuid());
        let worn_by_another = self
            .objects
            .scoped_by_full_id(target.object.uuid())
            .into_iter()
            .filter_map(|scoped| self.objects.wearer_of(scoped))
            .filter_map(|wearer| self.objects.full_key(&wearer))
            .any(|wearer| Some(wearer.uuid()) != own);
        if worn_by_another {
            return MediaClass::OnOthers;
        }
        // The scene is anchored at the agent's region, so a position read back
        // out of the scene is region-local there — and off the parcel bitmap
        // (so "outside") anywhere else.
        let inside = self
            .objects
            .entity_of(target.object)
            .and_then(|entity| self.transforms.get(entity).ok())
            .map(|transform| sl_viewer_kit::coords::bevy_to_sl_vec(transform.translation()))
            .zip(
                self.parcel
                    .as_ref()
                    .and_then(|parcel| parcel.current.as_ref()),
            )
            .is_some_and(|(position, parcel)| parcel.contains_point(position.x, position.y));
        if inside {
            MediaClass::WithinParcel
        } else {
            MediaClass::OutsideParcel
        }
    }
}

/// Follow the **Show** combo.
fn apply_nearby_media_filter(
    mut changes: MessageReader<ComboChanged>,
    combos: Query<&ComboSelection>,
    mut view: ResMut<NearbyMediaView>,
) {
    for change in changes.read() {
        let Ok(selection) = combos.get(change.combo) else {
            continue;
        };
        if selection.element != NEARBY_MEDIA_ELEMENT {
            continue;
        }
        let filter = MediaListFilter::from_index(change.active);
        if view.filter != filter {
            view.filter = filter;
            let rows = filtered(&view.all, filter);
            view.rows = rows;
        }
    }
}

/// Select the face the agent focuses in the world — the reference selects the
/// media its controls are on, so the list and the face agree.
fn follow_media_focus(
    focus: Res<MediaFocus>,
    mut selected: ResMut<SelectedNearbyMedia>,
    mut last: Local<Option<MediaTarget>>,
) {
    if focus.focused == *last {
        return;
    }
    *last = focus.focused;
    if let Some(target) = focus.focused {
        let wanted = SelectedNearbyMedia(Some(NearbyMediaId::Prim(target)));
        if *selected != wanted {
            *selected = wanted;
        }
    }
}

/// Re-read the rows a few times a second while the window is open.
fn rebuild_nearby_media_view(
    time: Res<Time>,
    mut elapsed: Local<Option<f32>>,
    sources: RowSources,
    mut view: ResMut<NearbyMediaView>,
    ui: Option<Res<NearbyMediaUi>>,
    mut lists: Query<&mut VirtualList>,
) {
    let Some(ui) = ui else {
        return;
    };
    let waited = elapsed.map_or(REFRESH_SECONDS, |seconds| seconds + time.delta_secs());
    if waited < REFRESH_SECONDS {
        *elapsed = Some(waited);
    } else {
        *elapsed = Some(0.0);
        let all = sources.rows();
        if view.all != all {
            let rows = filtered(&all, view.filter);
            view.all = all;
            if view.rows != rows {
                view.rows = rows;
            }
        }
    }
    // Every frame, not only on a refresh: the filter moves the rows too.
    if let Ok(mut list) = lists.get_mut(ui.viewport)
        && list.item_count != view.rows.len()
    {
        list.item_count = view.rows.len();
    }
}

/// The rows `filter` admits, in order.
fn filtered(rows: &[NearbyMediaRow], filter: MediaListFilter) -> Vec<NearbyMediaRow> {
    rows.iter()
        .filter(|row| filter.admits(row.class))
        .cloned()
        .collect()
}

/// A row's drawn name and colour: the parcel stream's fixed name, a face's
/// title / URL (or `<empty>`), with *(playing)* after a live one, dimmed while
/// it is not.
fn row_name(
    row: &NearbyMediaRow,
    parcel_audio_name: &str,
    empty_name: &str,
    playing_suffix: &str,
) -> (String, Color) {
    let base = match row.id {
        NearbyMediaId::ParcelAudio => parcel_audio_name,
        NearbyMediaId::Prim(_) if row.name.is_empty() => empty_name,
        NearbyMediaId::Prim(_) => row.name.as_str(),
    };
    if row.playback.is_live() {
        (format!("{base} {playing_suffix}"), LABEL_COLOR)
    } else {
        (base.to_owned(), DIM_LABEL_COLOR)
    }
}

/// Build the cells of each freshly-pooled row: the table's own, a play tick in
/// the first, and the press observer.
fn populate_nearby_media_rows(
    mut commands: Commands,
    ui: Option<Res<NearbyMediaUi>>,
    new_rows: Query<(Entity, &ChildOf), Added<VirtualRow>>,
) {
    let Some(ui) = ui else {
        return;
    };
    for (row_entity, child_of) in &new_rows {
        if child_of.parent() != ui.viewport {
            continue;
        }
        let cells = spawn_table_row(&mut commands, row_entity, ui.table, &NEARBY_MEDIA_TABLE);
        if let Some(cell) = cells.cell(COL_CHECK) {
            spawn_row_tick(&mut commands, cell, row_entity);
        }
        commands
            .entity(row_entity)
            .insert(BoundNearbyMedia(None))
            .observe(on_nearby_media_row_press);
    }
}

/// A row's play tick, in its first cell.
fn spawn_row_tick(commands: &mut Commands, cell: Entity, row_entity: Entity) -> Entity {
    let tick = spawn_checkbox(
        commands,
        cell,
        &CheckboxSpec {
            element: "nearby-media-tick",
            label: String::new(),
            tab_index: -1,
            font_size: FONT_SIZE,
            translate_label: false,
        },
    );
    // The tick has no caption; its column header ("On") is too terse to name it.
    commands
        .entity(tick.checkbox)
        .insert((
            NearbyMediaTick { row: row_entity },
            Semantic::new(Role::Checkbox).name_key("nearby-media-tick-name"),
        ))
        .observe(on_tick_change);
    tick.checkbox
}

/// Bind each pooled row to the media it now presents.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its data access"
)]
fn bind_nearby_media_rows(
    view: Res<NearbyMediaView>,
    selected: Res<SelectedNearbyMedia>,
    ui: Option<Res<NearbyMediaUi>>,
    translator: Translator,
    mut rows: Query<(
        Entity,
        Ref<VirtualRow>,
        &ChildOf,
        &TableRowCells,
        &mut BoundNearbyMedia,
    )>,
    ticks: Query<(Entity, &NearbyMediaTick, Has<Checked>)>,
    mut classes: Query<&mut ClassList, Without<Text>>,
    mut texts: Query<(&mut Text, &mut TextColor, Option<&mut ClassList>)>,
    mut commands: Commands,
) {
    let Some(ui) = ui else {
        return;
    };
    let refresh_all = view.is_changed() || selected.is_changed();
    let parcel_audio_name = translator.get("nearby-media-parcel-audio");
    let empty_name = translator.get("nearby-media-empty");
    let playing_suffix = translator.get("nearby-media-playing");
    for (row_entity, row, child_of, cells, mut bound) in &mut rows {
        if child_of.parent() != ui.viewport {
            continue;
        }
        if !refresh_all && !row.is_changed() {
            continue;
        }
        let data = row.index.and_then(|index| view.rows.get(index));
        let id = data.map(|entry| entry.id);
        if bound.0 != id {
            bound.0 = id;
        }
        let (name, color) = data.map_or_else(
            || (String::new(), LABEL_COLOR),
            |entry| row_name(entry, &parcel_audio_name, &empty_name, &playing_suffix),
        );
        if let Some(cell) = cells.cell(COL_NAME) {
            set_table_cell(&mut texts, cell, &name, color);
        }
        let live = data.is_some_and(|entry| entry.playback.is_live());
        for (tick, owner, checked) in &ticks {
            if owner.row != row_entity || checked == live {
                continue;
            }
            if live {
                commands.entity(tick).insert(Checked);
            } else {
                commands.entity(tick).remove::<Checked>();
            }
        }
        set_state_class_on(
            &mut classes,
            row_entity,
            SELECTED_CLASS,
            id.is_some() && selected.0 == id,
        );
    }
}

/// The transport's chrome, bundled as one
/// [`SystemParam`](bevy::ecs::system::SystemParam).
#[derive(bevy::ecs::system::SystemParam)]
struct TransportChrome<'w, 's> {
    /// The buttons' layout (a hidden control takes no room).
    nodes: Query<'w, 's, &'static mut Node>,
    /// Which controls already carry the disable marker.
    disabled: Query<'w, 's, (), With<InteractionDisabled>>,
    /// The glyph labels' state classes.
    classes: Query<'w, 's, &'static mut ClassList>,
    /// The volume slider's value.
    values: Query<'w, 's, &'static SliderValue>,
    /// What adds or drops the markers.
    commands: Commands<'w, 's>,
}

impl TransportChrome<'_, '_> {
    /// Show, grey or hide `entity` as `show` says.
    fn apply(&mut self, entity: Entity, show: ControlShow) {
        let display = if show == ControlShow::Hidden {
            Display::None
        } else {
            Display::Flex
        };
        if let Ok(mut node) = self.nodes.get_mut(entity)
            && node.display != display
        {
            node.display = display;
        }
        let refused = show == ControlShow::Disabled;
        let is_refused = self.disabled.contains(entity);
        if refused && !is_refused {
            self.commands.entity(entity).insert(InteractionDisabled);
        } else if !refused && is_refused {
            self.commands.entity(entity).remove::<InteractionDisabled>();
        }
    }
}

/// Keep the Stop All / Start All buttons and the selected row's transport in
/// step with the list.
fn sync_nearby_media_controls(
    view: Res<NearbyMediaView>,
    selected: Res<SelectedNearbyMedia>,
    ui: Option<Res<NearbyMediaUi>>,
    zoom: Res<MediaControlsState>,
    mut chrome: TransportChrome,
) {
    let Some(ui) = ui else {
        return;
    };
    let (any_live, any_idle) = stop_start_all(&view.all);
    chrome.apply(ui.stop_all, enabled_if(any_live));
    chrome.apply(ui.start_all, enabled_if(any_idle));

    let row = selected
        .0
        .and_then(|id| view.rows.iter().find(|row| row.id == id));
    let set = controls_for(row);
    let controls = ui.controls;
    chrome.apply(controls.stop, set.stop);
    chrome.apply(controls.play, set.play);
    chrome.apply(controls.pause, set.pause);
    chrome.apply(controls.volume, set.audio);
    chrome.apply(controls.mute, set.audio);
    chrome.apply(controls.zoom, set.zoom);

    let muted = row.is_some_and(|row| row.muted);
    let zoomed = row.is_some_and(|row| match row.id {
        NearbyMediaId::Prim(target) => zoom.zoomed() == Some(target),
        NearbyMediaId::ParcelAudio => false,
    });
    if let Ok(mut classes) = chrome.classes.get_mut(controls.mute_label) {
        set_state_class(&mut classes, glyph::MUTED, muted);
    }
    if let Ok(mut classes) = chrome.classes.get_mut(controls.zoom_label) {
        set_state_class(&mut classes, glyph::ZOOMED, zoomed);
    }
    if let Some(row) = row
        && chrome
            .values
            .get(controls.volume)
            .is_ok_and(|value| value.0.to_bits() != row.volume.to_bits())
    {
        chrome
            .commands
            .entity(controls.volume)
            .insert(SliderValue(row.volume));
    }
}

/// [`ControlShow::Enabled`] when `on`, else [`ControlShow::Disabled`].
const fn enabled_if(on: bool) -> ControlShow {
    if on {
        ControlShow::Enabled
    } else {
        ControlShow::Disabled
    }
}

// --- Interaction ----------------------------------------------------------

/// A press on a row: select it; a right-click also opens the row menu.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its data access"
)]
fn on_nearby_media_row_press(
    mut press: On<Pointer<Press>>,
    rows: Query<&BoundNearbyMedia>,
    view: Res<NearbyMediaView>,
    ui: Res<NearbyMediaUi>,
    mut focus: ResMut<InputFocus>,
    mut selected: ResMut<SelectedNearbyMedia>,
    mut target: ResMut<NearbyMediaMenuTarget>,
    mut menus: MessageWriter<OpenContextMenu>,
) {
    let Ok(BoundNearbyMedia(Some(id))) = rows.get(press.entity).copied() else {
        return;
    };
    press.propagate(false);
    focus.set(ui.viewport, FocusCause::Navigated);
    if selected.0 != Some(id) {
        selected.0 = Some(id);
    }
    if press.button != PointerButton::Secondary {
        return;
    }
    let url = view
        .rows
        .iter()
        .find(|row| row.id == id)
        .and_then(|row| row.url.as_deref());
    let mut conditions = Vec::new();
    if url.is_some_and(is_data_url) {
        conditions.push(COND_DATA_URL);
    }
    target.0 = Some(id);
    menus.write(OpenContextMenu {
        menu: &NEARBY_MEDIA_MENU,
        at: press.pointer_location.position,
        element: NEARBY_MEDIA_ELEMENT,
        conditions,
    });
}

/// A row's tick was flipped: start or stop its media.
fn on_tick_change(
    change: On<ValueChange<bool>>,
    ticks: Query<&NearbyMediaTick>,
    rows: Query<&BoundNearbyMedia>,
    mut commands: MessageWriter<NearbyMediaCommand>,
) {
    let Some(id) = ticks
        .get(change.source)
        .ok()
        .and_then(|tick| rows.get(tick.row).ok())
        .and_then(|bound| bound.0)
    else {
        return;
    };
    commands.write(if change.value {
        NearbyMediaCommand::Start(id)
    } else {
        NearbyMediaCommand::Stop(id)
    });
}

/// The volume slider moved: set the selected row's volume.
fn on_volume_change(
    change: On<ValueChange<f32>>,
    selected: Res<SelectedNearbyMedia>,
    mut commands: Commands,
    mut media: MessageWriter<NearbyMediaCommand>,
) {
    // The headless slider does not move its own value.
    commands
        .entity(change.source)
        .insert(SliderValue(change.value));
    if let Some(id) = selected.0 {
        media.write(NearbyMediaCommand::Volume(id, change.value));
    }
}

/// What the window's button and menu dispatch reads and writes, bundled as one
/// [`SystemParam`](bevy::ecs::system::SystemParam).
#[derive(bevy::ecs::system::SystemParam)]
struct ActionIo<'w> {
    /// The rows, for Stop All / Start All and the menu's URL.
    view: Res<'w, NearbyMediaView>,
    /// The selected row, which the transport acts on.
    selected: Res<'w, SelectedNearbyMedia>,
    /// The row the menu was opened on.
    menu_target: Res<'w, NearbyMediaMenuTarget>,
    /// The zoom state, to tell zoom from unzoom.
    zoom: Res<'w, MediaControlsState>,
    /// The system clipboard, absent on a headless host.
    clipboard: Option<ResMut<'w, bevy::clipboard::Clipboard>>,
    /// Per-row commands.
    media: MessageWriter<'w, NearbyMediaCommand>,
    /// Zoom requests.
    zooms: MessageWriter<'w, MediaZoomRequest>,
    /// The gear.
    preferences: MessageWriter<'w, OpenMediaPreferences>,
}

/// Dispatch the window's buttons and its row menu.
fn handle_nearby_media_actions(mut actions: MessageReader<UiAction>, mut io: ActionIo) {
    for action in actions.read() {
        if action.element != NEARBY_MEDIA_ELEMENT {
            continue;
        }
        let selected = io.selected.0;
        match action.action {
            "stop-all" | "start-all" => {
                let start = action.action == "start-all";
                let ids: Vec<NearbyMediaId> = io
                    .view
                    .all
                    .iter()
                    .filter(|row| row.playback.is_live() != start)
                    .map(|row| row.id)
                    .collect();
                for id in ids {
                    io.media.write(if start {
                        NearbyMediaCommand::Start(id)
                    } else {
                        NearbyMediaCommand::Stop(id)
                    });
                }
            }
            "preferences" => {
                io.preferences.write(OpenMediaPreferences);
            }
            "stop" | "play" | "pause" | "mute" => {
                let Some(id) = selected else { continue };
                io.media.write(match action.action {
                    "stop" => NearbyMediaCommand::Stop(id),
                    "play" => NearbyMediaCommand::Start(id),
                    "pause" => NearbyMediaCommand::Pause(id),
                    _mute => NearbyMediaCommand::ToggleMute(id),
                });
            }
            "zoom" => {
                if let Some(NearbyMediaId::Prim(target)) = selected {
                    let zoomed = io.zoom.zoomed() == Some(target);
                    io.zooms.write(MediaZoomRequest {
                        target: (!zoomed).then_some(target),
                    });
                }
            }
            "copy-url" | "copy-data" => {
                let url = io
                    .menu_target
                    .0
                    .and_then(|id| io.view.all.iter().find(|row| row.id == id))
                    .and_then(|row| row.url.clone());
                if let (Some(url), Some(clipboard)) = (url, io.clipboard.as_deref_mut()) {
                    let text = if action.action == "copy-data" {
                        copy_data_text(&url)
                    } else {
                        url
                    };
                    // A failed clipboard write (a headless run) is dropped.
                    let _set = clipboard.set_text(text);
                }
            }
            _other => {}
        }
    }
}

/// What a per-row command reaches, bundled as one
/// [`SystemParam`](bevy::ecs::system::SystemParam).
#[derive(bevy::ecs::system::SystemParam)]
struct CommandTargets<'w> {
    /// Faces to start or keep stopped.
    requests: ResMut<'w, MediaStartRequests>,
    /// The live surfaces by face.
    prim_state: Res<'w, MediaPrimState>,
    /// The engine's surfaces.
    surfaces: NonSendMut<'w, MediaSurfaces>,
    /// The rows, for a mute flip's current state.
    view: Res<'w, NearbyMediaView>,
    /// The parcel stream's player.
    parcel_stream: MessageWriter<'w, ParcelStreamRequest>,
}

/// Carry out the per-row commands.
fn apply_nearby_media_commands(
    mut commands: MessageReader<NearbyMediaCommand>,
    mut targets: CommandTargets,
) {
    for command in commands.read() {
        match *command {
            NearbyMediaCommand::Start(NearbyMediaId::ParcelAudio) => {
                targets.parcel_stream.write(ParcelStreamRequest::Play);
            }
            NearbyMediaCommand::Stop(NearbyMediaId::ParcelAudio) => {
                targets.parcel_stream.write(ParcelStreamRequest::Stop);
            }
            NearbyMediaCommand::Pause(NearbyMediaId::ParcelAudio) => {}
            NearbyMediaCommand::ToggleMute(NearbyMediaId::ParcelAudio) => {
                let muted = targets
                    .view
                    .all
                    .iter()
                    .find(|row| row.id == NearbyMediaId::ParcelAudio)
                    .is_some_and(|row| row.muted);
                targets
                    .parcel_stream
                    .write(ParcelStreamRequest::SetMuted(!muted));
            }
            NearbyMediaCommand::Volume(NearbyMediaId::ParcelAudio, volume) => {
                targets
                    .parcel_stream
                    .write(ParcelStreamRequest::SetVolume(volume));
            }
            NearbyMediaCommand::Start(NearbyMediaId::Prim(target)) => {
                // A paused video resumes where it is; anything else starts.
                let resumed = targets
                    .prim_state
                    .active
                    .get(&target)
                    .filter(|active| active.kind == MediaEngineKind::Video)
                    .and_then(|active| targets.surfaces.get(active.surface))
                    .map(|slot| slot.surface.play())
                    .is_some();
                if !resumed {
                    targets.requests.request(target);
                }
            }
            NearbyMediaCommand::Stop(NearbyMediaId::Prim(target)) => {
                targets.requests.stop(target);
            }
            NearbyMediaCommand::Pause(NearbyMediaId::Prim(target)) => {
                if let Some(slot) = targets
                    .prim_state
                    .active
                    .get(&target)
                    .and_then(|active| targets.surfaces.get(active.surface))
                {
                    slot.surface.pause();
                }
            }
            NearbyMediaCommand::ToggleMute(NearbyMediaId::Prim(target)) => {
                if let Some(slot) = targets
                    .prim_state
                    .active
                    .get(&target)
                    .and_then(|active| targets.surfaces.get(active.surface))
                {
                    slot.surface.set_muted(!slot.surface.muted());
                }
            }
            NearbyMediaCommand::Volume(NearbyMediaId::Prim(target), volume) => {
                let Some(surface) = targets
                    .prim_state
                    .active
                    .get(&target)
                    .map(|active| active.surface)
                else {
                    continue;
                };
                if let Some(slot) = targets.surfaces.get_mut(surface) {
                    // Routed into the mixer, the level is the source's own gain
                    // there; an engine still on its own device takes it itself.
                    match slot.audio.as_mut() {
                        Some(audio) => audio.set_gain(volume),
                        None => slot.surface.set_volume(f64::from(volume)),
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{
        ControlSet, ControlShow, MediaClass, MediaListFilter, MediaPlayback, NearbyMediaId,
        NearbyMediaRow, controls_for, copy_data_text, is_data_url, stop_start_all,
    };

    /// A row of `playback`, parcel stream or face as `id` says.
    fn row(
        id: NearbyMediaId,
        class: Option<MediaClass>,
        playback: MediaPlayback,
    ) -> NearbyMediaRow {
        NearbyMediaRow {
            id,
            name: String::new(),
            url: None,
            class,
            playback,
            muted: false,
            volume: 1.0,
        }
    }

    /// A face row.
    fn face(playback: MediaPlayback) -> NearbyMediaRow {
        let target = sl_viewer_world_api::MediaTarget {
            object: sl_client_bevy::ObjectKey::from(sl_client_bevy::Uuid::from_u128(7)),
            face: sl_client_bevy::PrimFaceId::new(0),
        };
        row(
            NearbyMediaId::Prim(target),
            Some(MediaClass::WithinParcel),
            playback,
        )
    }

    /// The parcel stream's row.
    fn stream(playing: bool) -> NearbyMediaRow {
        row(
            NearbyMediaId::ParcelAudio,
            None,
            MediaPlayback::Stream { playing },
        )
    }

    /// No selection is the reference's disabled set: a refused stop and zoom.
    #[test]
    fn nothing_selected_refuses_stop_and_zoom() {
        assert_eq!(
            controls_for(None),
            ControlSet {
                stop: ControlShow::Disabled,
                play: ControlShow::Hidden,
                pause: ControlShow::Hidden,
                audio: ControlShow::Hidden,
                zoom: ControlShow::Disabled,
            }
        );
    }

    /// A playing video shows stop and pause; a paused one stop and play — the
    /// reference's time-based set — and both zoom.
    #[test]
    fn a_video_gets_the_time_based_set() {
        let playing = controls_for(Some(&face(MediaPlayback::Timed { playing: true })));
        assert_eq!(playing.stop, ControlShow::Enabled);
        assert_eq!(playing.pause, ControlShow::Enabled);
        assert_eq!(playing.play, ControlShow::Hidden);
        assert_eq!(playing.audio, ControlShow::Enabled);
        assert_eq!(playing.zoom, ControlShow::Enabled);

        let paused = controls_for(Some(&face(MediaPlayback::Timed { playing: false })));
        assert_eq!(paused.pause, ControlShow::Hidden);
        assert_eq!(paused.play, ControlShow::Enabled);
        assert_eq!(paused.stop, ControlShow::Enabled);
    }

    /// A page cannot pause; an idle face can only be started (it has no audio
    /// to set yet); the parcel stream never zooms.
    #[test]
    fn pages_idle_faces_and_the_stream() {
        let page = controls_for(Some(&face(MediaPlayback::Page)));
        assert_eq!(page.pause, ControlShow::Hidden);
        assert_eq!(page.play, ControlShow::Hidden);
        assert_eq!(page.stop, ControlShow::Enabled);

        let idle = controls_for(Some(&face(MediaPlayback::Idle)));
        assert_eq!(idle.play, ControlShow::Enabled);
        assert_eq!(idle.stop, ControlShow::Hidden);
        assert_eq!(idle.audio, ControlShow::Hidden);
        assert_eq!(idle.zoom, ControlShow::Enabled);

        let radio = controls_for(Some(&stream(true)));
        assert_eq!(radio.zoom, ControlShow::Hidden);
        assert_eq!(radio.stop, ControlShow::Enabled);
        assert_eq!(radio.play, ControlShow::Hidden);
        assert_eq!(
            radio.audio,
            ControlShow::Enabled,
            "the music bus is always settable"
        );
        let silent = controls_for(Some(&stream(false)));
        assert_eq!(silent.play, ControlShow::Enabled);
        assert_eq!(silent.stop, ControlShow::Hidden);
    }

    /// The filter is the reference's: the parcel stream shows under All and In
    /// this Parcel only.
    #[test]
    fn the_filter_places_the_stream_in_the_parcel() {
        assert!(MediaListFilter::All.admits(None));
        assert!(MediaListFilter::WithinParcel.admits(None));
        assert!(!MediaListFilter::OutsideParcel.admits(None));
        assert!(!MediaListFilter::OnOthers.admits(None));
        assert!(MediaListFilter::OnOthers.admits(Some(MediaClass::OnOthers)));
        assert!(!MediaListFilter::WithinParcel.admits(Some(MediaClass::OnOthers)));
        assert!(MediaListFilter::OutsideParcel.admits(Some(MediaClass::OutsideParcel)));
        assert_eq!(MediaListFilter::from_index(99), MediaListFilter::All);
        assert_eq!(MediaListFilter::from_index(3), MediaListFilter::OnOthers);
    }

    /// Stop All wants something playing, Start All something that is not.
    #[test]
    fn stop_all_and_start_all_enable_on_what_there_is() {
        assert_eq!(stop_start_all(&[]), (false, false));
        assert_eq!(stop_start_all(&[stream(true)]), (true, false));
        assert_eq!(
            stop_start_all(&[stream(true), face(MediaPlayback::Idle)]),
            (true, true)
        );
        assert_eq!(stop_start_all(&[stream(false)]), (false, true));
    }

    /// Copy Data decodes a base64 payload and unescapes anything else.
    #[test]
    fn copy_data_decodes_the_payload() {
        assert_eq!(
            copy_data_text("data:text/html;base64,PGI+aGk8L2I+"),
            "<b>hi</b>"
        );
        assert_eq!(
            copy_data_text("data:text/html,%3Cb%3Ehi%3C%2Fb%3E"),
            "data:text/html,<b>hi</b>"
        );
        assert!(is_data_url("DATA:text/plain,x"));
        assert!(!is_data_url("https://example.com/"));
        assert!(!is_data_url("dat"));
    }
}
