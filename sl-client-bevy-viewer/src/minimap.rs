//! The minimap ("net map") floater: a top-down view of the region around the
//! camera.
//!
//! The reference viewer's net map (Firestorm `llnetmap.cpp` /
//! `llfloatermap.cpp`, read-only reference) composites a terrain backdrop, a
//! cached "untextured objects" raster, a cached parcel-overlay raster, avatar
//! dots, the camera frustum wedge and optional chat-range rings, under eight
//! compass labels — with SHIFT-drag panning, wheel zoom, double-click teleport
//! and a right-click context menu.
//!
//! This implementation keeps the same layer architecture (cached object /
//! parcel rasters on their own refresh triggers — the geometry lives in
//! [`crate::minimap_math`]) but composites everything into **one CPU image**
//! shown by a single [`ImageNode`], recomposited only when an input changed
//! (camera pose, pan / zoom, a regenerated layer, a moved dot, the hover
//! cursor). Rotation is baked into the world↔surface transform exactly as the
//! reference bakes it into `globalPosToView`, so dots, hit-tests and the
//! backdrop always agree.
//!
//! Deliberate deviations from the reference, pending other roadmap tasks: the
//! terrain backdrop is shaded from our own terrain mirror (heights + splat
//! weights) on every grid — the OpenSim world-map-tile backdrop belongs to the
//! world-map floater's shared tile fetcher (`viewer-world-map-floater`); the
//! collision ("banned parcel") fill has no data source yet; "open world map"
//! on double-click falls back to only placing the tracking beacon until the
//! world map floater exists; and a neighbour region whose circuit has not
//! delivered a parcel overlay yet (OpenSim only pushes them on parcel
//! changes) draws its full border outline instead of property lines.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::ui::RelativeCursorPosition;
use bevy::window::PrimaryWindow;
use sl_client_bevy::{
    AgentKey, Command, MuteFlags, MuteType, OwnerKey, ParcelOwnership, RegionCoordinates,
    RegionHandle, SlCommand, SlCurrentRegion, SlEvent, SlIdentity, SlParcel, SlParcelOverlay,
    SlRegion, SlRegionIdentity, SlSessionEvent, Vector,
};
use sl_settings::{Scope, SettingValue};

use crate::avatar_profile::OpenAvatarProfile;
use crate::avatars::AvatarState;
use crate::camera::{CameraMode, ViewerCamera};
use crate::conversations::{ConversationKey, OpenConversation};
use crate::coords::bevy_to_sl_vec;
use crate::floater::{FloaterCaps, FloaterSpec, spawn_floater};
use crate::i18n::{TransArgs, Translated, Translator};
use crate::menu::{MenuCommand, MenuDef, MenuItemDef, OpenContextMenu};
use crate::minimap_math::{
    self, COLOR_AVATAR, COLOR_AVATAR_FRIEND, COLOR_AVATAR_LINDEN, COLOR_AVATAR_SELF,
    COLOR_CHAT_RING, COLOR_FRUSTUM, COLOR_PARCEL_LINE, COLOR_SHOUT_RING, COLOR_TRACK,
    COLOR_WHISPER_RING, DoubleClickAction, LayerRaster, MapView, ObjectAccents, ParcelCell, Rgba,
    Surface,
};
use crate::objects::{ObjectDebugInfo, ObjectState};
use crate::people::FriendsModel;
use crate::settings::ViewerSettings;
use crate::terrain::TerrainState;
use crate::ui::{UiPanelShown, UiRoot, UiScaffoldSystems, column};
use crate::ui_element::{ElementCx, UiAction};
use crate::ui_font::UiFont;
use crate::water::WaterState;

/// The `element` tag the minimap attributes its [`UiAction`]s to.
pub(crate) const MINIMAP_ELEMENT: &str = "minimap";

/// The settings section every minimap setting registers under.
const MINIMAP_SECTION: &[&str] = &["minimap"];

/// The map scale setting (pixels per 256 m region), shared by all instances.
const SETTING_SCALE: &str = "MiniMapScale";

/// Whether the map rotates so the camera heading points up.
const SETTING_ROTATE: &str = "MiniMapRotate";

/// Whether the pan offset eases back to centre each frame.
const SETTING_AUTO_CENTER: &str = "MiniMapAutoCenter";

/// The minimap surface opacity.
const SETTING_OPACITY: &str = "MiniMapOpacity";

/// Whether the object layer draws at all.
const SETTING_OBJECTS: &str = "MiniMapObjects";

/// Accent toggle: highlight physical objects.
const SETTING_PHYSICAL: &str = "NetMapPhysical";

/// Accent toggle: highlight scripted objects.
const SETTING_SCRIPTED: &str = "NetMapScripted";

/// Accent toggle: highlight temp-on-rez objects.
const SETTING_TEMP_ON_REZ: &str = "NetMapTempOnRez";

/// Phantom-object dot opacity, in percent.
const SETTING_PHANTOM_OPACITY: &str = "NetMapPhantomOpacity";

/// Whether the parcel layer (property lines) draws at all.
const SETTING_PROPERTY_LINES: &str = "MiniMapShowPropertyLines";

/// Whether for-sale / auction parcels are filled.
const SETTING_FOR_SALE: &str = "MiniMapForSaleParcels";

/// Master toggle for the chat-range rings.
const SETTING_CHAT_RING: &str = "MiniMapChatRing";

/// Per-ring toggle: the whisper-range ring.
const SETTING_WHISPER_RING: &str = "MiniMapWhisperRing";

/// Per-ring toggle: the say-range ring.
const SETTING_SAY_RING: &str = "MiniMapSayRing";

/// Per-ring toggle: the shout-range ring.
const SETTING_SHOUT_RING: &str = "MiniMapShoutRing";

/// What a double-click does (0 none, 1 world map, 2 teleport).
const SETTING_DOUBLE_CLICK: &str = "NetMapDoubleClickAction";

/// The hover pick radius, as a multiple of the dot radius.
const SETTING_PICK_SCALE: &str = "MinimapPickScale";

/// The largest radius (metres) an object rasterises at.
const SETTING_PRIM_MAX_RADIUS: &str = "MiniMapPrimMaxRadius";

/// The vertical cull distance (metres) for the object layer.
const SETTING_PRIM_MAX_VERT: &str = "MiniMapPrimMaxVertDistance";

/// Register every minimap setting (called from
/// [`crate::settings::ViewerSettings`]'s `FromWorld`).
pub(crate) fn register_settings(settings: &mut ViewerSettings) {
    settings.register_in(
        MINIMAP_SECTION,
        SETTING_SCALE,
        SettingValue::F32(minimap_math::MAP_SCALE_MEDIUM),
        "Minimap zoom, in pixels per 256 m region (32-4096)",
    );
    settings.register_in(
        MINIMAP_SECTION,
        SETTING_ROTATE,
        SettingValue::Bool(true),
        "Rotate the minimap so the camera heading points up (off = north up)",
    );
    settings.register_in(
        MINIMAP_SECTION,
        SETTING_AUTO_CENTER,
        SettingValue::Bool(true),
        "Ease a panned minimap back to centre",
    );
    settings.register_in(
        MINIMAP_SECTION,
        SETTING_OPACITY,
        SettingValue::F32(0.66),
        "Minimap surface opacity",
    );
    settings.register_in(
        MINIMAP_SECTION,
        SETTING_OBJECTS,
        SettingValue::Bool(true),
        "Show objects on the minimap",
    );
    settings.register_in(
        MINIMAP_SECTION,
        SETTING_PHYSICAL,
        SettingValue::Bool(false),
        "Highlight physical objects on the minimap",
    );
    settings.register_in(
        MINIMAP_SECTION,
        SETTING_SCRIPTED,
        SettingValue::Bool(false),
        "Highlight scripted objects on the minimap",
    );
    settings.register_in(
        MINIMAP_SECTION,
        SETTING_TEMP_ON_REZ,
        SettingValue::Bool(false),
        "Highlight temp-on-rez objects on the minimap",
    );
    settings.register_in(
        MINIMAP_SECTION,
        SETTING_PHANTOM_OPACITY,
        SettingValue::U32(100),
        "Phantom-object opacity on the minimap, in percent",
    );
    settings.register_in(
        MINIMAP_SECTION,
        SETTING_PROPERTY_LINES,
        SettingValue::Bool(true),
        "Show parcel property lines on the minimap",
    );
    settings.register_in(
        MINIMAP_SECTION,
        SETTING_FOR_SALE,
        SettingValue::Bool(true),
        "Fill for-sale and auction parcels on the minimap",
    );
    settings.register_in(
        MINIMAP_SECTION,
        SETTING_CHAT_RING,
        SettingValue::Bool(false),
        "Show chat-distance rings on the minimap",
    );
    settings.register_in(
        MINIMAP_SECTION,
        SETTING_WHISPER_RING,
        SettingValue::Bool(true),
        "Show the whisper-range ring (when chat rings are on)",
    );
    settings.register_in(
        MINIMAP_SECTION,
        SETTING_SAY_RING,
        SettingValue::Bool(true),
        "Show the say-range ring (when chat rings are on)",
    );
    settings.register_in(
        MINIMAP_SECTION,
        SETTING_SHOUT_RING,
        SettingValue::Bool(true),
        "Show the shout-range ring (when chat rings are on)",
    );
    settings.register_in(
        MINIMAP_SECTION,
        SETTING_DOUBLE_CLICK,
        SettingValue::I32(2),
        "Minimap double-click action: 0 nothing, 1 world map, 2 teleport",
    );
    settings.register_in(
        MINIMAP_SECTION,
        SETTING_PICK_SCALE,
        SettingValue::F32(3.0),
        "Minimap hover pick radius, as a multiple of the dot radius",
    );
    settings.register_in(
        MINIMAP_SECTION,
        SETTING_PRIM_MAX_RADIUS,
        SettingValue::F32(16.0),
        "Largest radius (metres) an object draws with on the minimap",
    );
    settings.register_in(
        MINIMAP_SECTION,
        SETTING_PRIM_MAX_VERT,
        SettingValue::F32(256.0),
        "Hide minimap objects more than this many metres above/below the agent (0 = no limit)",
    );
}

// ---------------------------------------------------------------------------
// Resources.
// ---------------------------------------------------------------------------

/// The minimap floater's entity handles.
#[derive(Resource)]
pub(crate) struct MinimapUi {
    /// The floater root (carries [`UiPanelShown`]).
    root: Entity,
    /// The map surface node (the [`ImageNode`], the input target).
    surface: Entity,
    /// The composited surface image.
    image: Handle<Image>,
    /// The eight compass label wrapper nodes, in [`COMPASS_POINTS`] order.
    compass: [Entity; 8],
    /// The hover tooltip panel.
    tooltip: Entity,
    /// The hover tooltip's text node.
    tooltip_text: Entity,
}

impl MinimapUi {
    /// The floater root, for open-state checks and toggling.
    pub(crate) const fn panel(&self) -> Entity {
        self.root
    }
}

/// One avatar dot as composited this frame — the hover / context-menu
/// hit-testing data.
#[derive(Debug, Clone, Copy)]
struct DotInfo {
    /// The avatar.
    agent: AgentKey,
    /// Its surface position, in image pixels.
    view: Vec2,
    /// Its distance from the own avatar, in metres (3-D).
    distance: f32,
    /// Whether its altitude is the coarse "unknown" sentinel.
    altitude_unknown: bool,
}

/// The right-click context captured when the menu opened.
#[derive(Debug, Clone, Default)]
struct MenuContext {
    /// The closest avatar dot under the click, if any.
    agent: Option<AgentKey>,
    /// Every avatar dot within pick radius of the click.
    agents: Vec<AgentKey>,
}

/// The minimap's live state: the view transform inputs, the cached content
/// layers, the composited-frame stamp, and the interaction state.
#[expect(
    clippy::struct_excessive_bools,
    reason = "the flags are genuinely independent one-shot latches (scale seeded, re-centring, \
              dragging, per-layer dirty), not a state machine in disguise"
)]
#[derive(Resource)]
struct MinimapState {
    /// The runtime scale (pixels per region); mirrored to the persisted
    /// setting, debounced by [`scale_save_timer`](Self::scale_save_timer).
    scale: f32,
    /// Whether [`scale`](Self::scale) was seeded from the setting yet.
    scale_loaded: bool,
    /// Seconds until the changed scale is written back to the settings store,
    /// or `None` when nothing is pending.
    scale_save_timer: Option<f32>,
    /// The pan offset (surface pixels, the reference's `mCurPan`).
    pan: Vec2,
    /// A "re-center" was requested: ease back even with auto-centre off.
    centering: bool,
    /// A SHIFT-drag pan is in progress (suspends auto-centring).
    dragging: bool,
    /// This frame's world↔surface transform.
    view: MapView,
    /// The camera heading (`atan2(at_east, at_north)`), kept for the north-up
    /// frustum wedge.
    heading: f32,
    /// The camera's global position (metres east / north) and altitude.
    camera: (f64, f64, f32),
    /// The scene origin region the Bevy world is anchored to.
    origin: Option<RegionHandle>,
    /// The own avatar's global position, once known.
    agent: Option<(f64, f64, f32)>,
    /// The composited image size, in pixels.
    surface_px: UVec2,
    /// The cursor position over the surface, in image pixels.
    cursor: Option<Vec2>,
    /// The cursor position over the surface, in logical node pixels (for
    /// placing the tooltip).
    cursor_node: Option<Vec2>,
    /// The object layer raster.
    object_layer: LayerRaster,
    /// The object layer's capture centre (global metres east / north).
    object_center: (f64, f64),
    /// The object layer's texels per metre at capture.
    object_tpm: f32,
    /// Seconds since the object layer was last regenerated.
    object_elapsed: f32,
    /// The object layer must regenerate now (scale / resize / toggle).
    object_dirty: bool,
    /// The parcel layer raster.
    parcel_layer: LayerRaster,
    /// The parcel layer's capture centre (global metres east / north).
    parcel_center: (f64, f64),
    /// The parcel layer's texels per metre at capture.
    parcel_tpm: f32,
    /// The parcel layer must regenerate now (overlay change / toggle).
    parcel_dirty: bool,
    /// Per-region shaded terrain backdrops (256×256, one texel per metre,
    /// bottom-up rows like every layer raster).
    terrain_maps: HashMap<RegionHandle, LayerRaster>,
    /// The [`TerrainState::map_revision`] the backdrops were built at.
    terrain_revision: Option<u64>,
    /// Seconds since the terrain backdrops were last rebuilt (throttles the
    /// rebuild while patches stream in).
    terrain_elapsed: f32,
    /// The stamp of the last composited frame; recomposite when it changes.
    last_stamp: Option<CompositeStamp>,
    /// This frame's avatar dots (hover / context-menu hit-testing).
    dots: Vec<DotInfo>,
    /// The double-click tracker: the time and position of the last click.
    last_click: Option<(f64, Vec2)>,
    /// The context captured by the last right-click.
    menu: MenuContext,
}

impl Default for MinimapState {
    fn default() -> Self {
        Self {
            scale: minimap_math::MAP_SCALE_MEDIUM,
            scale_loaded: false,
            scale_save_timer: None,
            pan: Vec2::ZERO,
            centering: false,
            dragging: false,
            view: MapView {
                scale: minimap_math::MAP_SCALE_MEDIUM,
                rotation: 0.0,
                pan: Vec2::ZERO,
                size: Vec2::new(64.0, 64.0),
            },
            heading: 0.0,
            camera: (0.0, 0.0, 0.0),
            origin: None,
            agent: None,
            surface_px: UVec2::new(64, 64),
            cursor: None,
            cursor_node: None,
            object_layer: LayerRaster::default(),
            object_center: (0.0, 0.0),
            object_tpm: 1.0,
            object_elapsed: 0.0,
            object_dirty: true,
            parcel_layer: LayerRaster::default(),
            parcel_center: (0.0, 0.0),
            parcel_tpm: 1.0,
            parcel_dirty: true,
            terrain_maps: HashMap::new(),
            terrain_revision: None,
            terrain_elapsed: 1.0,
            last_stamp: None,
            dots: Vec::new(),
            last_click: None,
            menu: MenuContext::default(),
        }
    }
}

/// Everything the composited image depends on, quantised — when this stamp
/// equals the previous frame's, the image is left untouched.
#[derive(Debug, Clone, PartialEq)]
struct CompositeStamp {
    /// Camera east / north, in 1/16 m steps.
    camera: (i64, i64),
    /// Rotation in milliradian steps.
    rotation: i32,
    /// Pan in 1/4 px steps.
    pan: (i32, i32),
    /// The scale in 1/16 px steps.
    scale: i32,
    /// The image size.
    size: UVec2,
    /// The cursor cell (whole pixels), for the pick-radius circle.
    cursor: Option<(i32, i32)>,
    /// Which layer generations went in (terrain revision, raster sizes and
    /// capture centres).
    layers: (u64, u32, u32, (i64, i64), (i64, i64)),
    /// The dots (agent and rounded position) that went in.
    dots: Vec<(AgentKey, i32, i32)>,
    /// The tracking target that went in (quantised location, or the tracked
    /// avatar's id).
    tracking: Option<(i64, i64, u128)>,
    /// The visibility toggles that pick layers (objects, lines, rings).
    toggles: (bool, bool, bool),
}

/// A per-avatar map mark set from the context menu (colours the dot).
#[derive(Resource, Default)]
pub(crate) struct MinimapMarks(HashMap<AgentKey, Rgba>);

/// The map tracking target — a shared shape for the minimap today and the
/// world map later (`viewer-world-map-tracking-teleport`), so both surfaces
/// drive one beacon.
#[derive(Resource, Default)]
pub(crate) struct MapTracking {
    /// The current target, or `None` when not tracking.
    pub(crate) target: Option<TrackTarget>,
}

/// What the map is tracking.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum TrackTarget {
    /// A fixed world location (global metres).
    Location {
        /// Global metres west→east.
        east: f64,
        /// Global metres south→north.
        north: f64,
        /// Altitude in metres.
        up: f32,
    },
    /// An avatar, followed while it is known.
    Avatar(AgentKey),
}

/// The chat ranges the rings draw at — the viewer defaults unless the grid's
/// `SimulatorFeatures` extras override them.
#[derive(Resource, Debug, Clone, Copy)]
pub(crate) struct ChatRanges {
    /// Whisper range in metres (default 10).
    pub(crate) whisper: f32,
    /// Say range in metres (default 20).
    pub(crate) say: f32,
    /// Shout range in metres (default 100).
    pub(crate) shout: f32,
}

impl Default for ChatRanges {
    fn default() -> Self {
        Self {
            whisper: 10.0,
            say: 20.0,
            shout: 100.0,
        }
    }
}

// ---------------------------------------------------------------------------
// Plugin and spawn.
// ---------------------------------------------------------------------------

/// The compass points, as (Fluent key, world angle in radians where 0 = east,
/// counter-clockwise) — the label placement adds the map rotation.
const COMPASS_POINTS: [(&str, f32); 8] = [
    ("minimap-compass-east", 0.0),
    ("minimap-compass-north-east", core::f32::consts::FRAC_PI_4),
    ("minimap-compass-north", core::f32::consts::FRAC_PI_2),
    (
        "minimap-compass-north-west",
        3.0 * core::f32::consts::FRAC_PI_4,
    ),
    ("minimap-compass-west", core::f32::consts::PI),
    (
        "minimap-compass-south-west",
        5.0 * core::f32::consts::FRAC_PI_4,
    ),
    ("minimap-compass-south", 3.0 * core::f32::consts::FRAC_PI_2),
    (
        "minimap-compass-south-east",
        7.0 * core::f32::consts::FRAC_PI_4,
    ),
];

/// Which compass points are the four diagonals (hidden on a small map).
const COMPASS_MINOR: [bool; 8] = [false, true, false, true, false, true, false, true];

/// The minimap plugin: the floater, the per-frame surface pipeline, and the
/// context-menu / interaction wiring.
pub(crate) struct MinimapPlugin;

impl Plugin for MinimapPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MinimapState>()
            .init_resource::<MinimapMarks>()
            .init_resource::<MapTracking>()
            .init_resource::<ChatRanges>()
            .add_systems(Startup, spawn_minimap.after(UiScaffoldSystems::SpawnRoot))
            .add_systems(
                Update,
                (
                    read_chat_ranges,
                    drive_minimap_view,
                    regen_minimap_layers,
                    composite_minimap,
                    layout_minimap_compass,
                    update_minimap_hover,
                    handle_minimap_actions,
                    apply_minimap_mouselook,
                )
                    .chain(),
            );
    }
}

/// The floater's default content size, in logical pixels.
const DEFAULT_SIZE: f32 = 200.0;

/// The smallest content size the resize grip allows.
const MIN_SIZE: f32 = 64.0;

/// The compass label font size.
const COMPASS_FONT_SIZE: f32 = 12.0;

/// The tooltip font size.
const TOOLTIP_FONT_SIZE: f32 = 12.0;

/// The largest composited image side, in pixels (the reference caps its layer
/// rasters at 512 too; a larger widget upscales).
const MAX_SURFACE_PX: u32 = 512;

/// Startup: build the minimap floater — surface image node, compass labels,
/// tooltip — and wire the input observers.
fn spawn_minimap(
    mut commands: Commands,
    root: Res<UiRoot>,
    mut images: ResMut<Assets<Image>>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    // Vintage-skin placement: free-floating, spawning toward the top-right.
    let position = windows.single().map_or(Vec2::new(760.0, 60.0), |window| {
        Vec2::new((window.width() - DEFAULT_SIZE - 80.0).max(40.0), 60.0)
    });
    let handle = spawn_floater(
        &mut commands,
        root.0,
        FloaterSpec {
            id: "minimap",
            title: String::from("Mini-map"),
            position,
            default_size: Some(Vec2::splat(DEFAULT_SIZE)),
            min_size: Some(Vec2::splat(MIN_SIZE)),
            dock_host: None,
            caps: FloaterCaps {
                resizable: true,
                // The Vintage-style free-floating map: minimize disabled.
                minimizable: false,
                closable: true,
                dockable: false,
            },
        },
    );
    commands
        .entity(handle.title_text)
        .insert(Translated::new("minimap-floater-title"));

    let image = images.add(blank_surface(64, 64));

    let surface = commands
        .spawn((
            Node {
                flex_grow: 1.0,
                width: Val::Percent(100.0),
                min_width: Val::Px(MIN_SIZE),
                min_height: Val::Px(MIN_SIZE),
                ..default()
            },
            ImageNode::new(image.clone()),
            RelativeCursorPosition::default(),
            Pickable {
                should_block_lower: true,
                is_hoverable: true,
            },
            Name::new("minimap-surface"),
            ChildOf(handle.content),
        ))
        .observe(on_minimap_click)
        .observe(on_minimap_drag)
        .observe(on_minimap_drag_end)
        .observe(on_minimap_scroll)
        .observe(on_minimap_context)
        .id();

    let compass = COMPASS_POINTS.map(|(key, _angle)| {
        let wrapper = commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    top: Val::Px(0.0),
                    ..default()
                },
                Pickable::IGNORE,
                Name::new("minimap-compass"),
                ChildOf(surface),
            ))
            .id();
        commands.spawn((
            Text::default(),
            Translated::new(key),
            UiFont::Sans.at(COMPASS_FONT_SIZE),
            TextColor(Color::srgba(1.0, 1.0, 1.0, 0.7)),
            Pickable::IGNORE,
            ChildOf(wrapper),
        ));
        wrapper
    });

    let tooltip = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                padding: UiRect::all(Val::Px(4.0)),
                ..column(Val::ZERO)
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.8)),
            Visibility::Hidden,
            Pickable::IGNORE,
            Name::new("minimap-tooltip"),
            ChildOf(surface),
        ))
        .id();
    let tooltip_text = commands
        .spawn((
            Text::default(),
            UiFont::Sans.at(TOOLTIP_FONT_SIZE),
            TextColor(Color::WHITE),
            Pickable::IGNORE,
            ChildOf(tooltip),
        ))
        .id();

    commands.insert_resource(MinimapUi {
        root: handle.root,
        surface,
        image,
        compass,
        tooltip,
        tooltip_text,
    });
}

/// A transparent RGBA surface image of the given size.
fn blank_surface(width: u32, height: u32) -> Image {
    let texels = usize::try_from(width)
        .unwrap_or(0)
        .saturating_mul(usize::try_from(height).unwrap_or(0))
        .saturating_mul(4);
    Image::new(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        vec![0; texels],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

// ---------------------------------------------------------------------------
// Event intake.
// ---------------------------------------------------------------------------

/// Learn the grid's chat ranges from `SimulatorFeatures` (OpenSim extras);
/// grids without the extras keep the viewer defaults.
fn read_chat_ranges(mut events: MessageReader<SlEvent>, mut ranges: ResMut<ChatRanges>) {
    for event in events.read() {
        if let SlSessionEvent::SimulatorFeatures(features) = &event.0
            && let Some(extras) = &features.open_sim_extras
        {
            let mut next = ChatRanges::default();
            if let Some(value) = extras.whisper_range {
                next.whisper = range_metres(value);
            }
            if let Some(value) = extras.say_range {
                next.say = range_metres(value);
            }
            if let Some(value) = extras.shout_range {
                next.shout = range_metres(value);
            }
            *ranges = next;
        }
    }
}

/// A grid-advertised chat range as metres, clamped to something sane.
fn range_metres(value: i32) -> f32 {
    let clamped = value.clamp(0, 4096);
    f32::from(u16::try_from(clamped).unwrap_or(0))
}

// ---------------------------------------------------------------------------
// Per-frame view state.
// ---------------------------------------------------------------------------

/// Scale a [`Vec2`] by a scalar without the glam `*` operator (the workspace
/// `arithmetic_side_effects` lint trips on operator arithmetic of non-primitive
/// types).
const fn vec2_scale(v: Vec2, s: f32) -> Vec2 {
    Vec2::new(v.x * s, v.y * s)
}

/// Narrow an `f64` metre offset to `f32` (offsets near the camera are small,
/// so the narrowing is exact enough for pixel placement).
const fn narrow(value: f64) -> f32 {
    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        reason = "camera-relative metre offsets are far inside f32 range; sub-millimetre \
                  precision loss is irrelevant at map scales"
    )]
    let out = value as f32;
    out
}

/// The scene origin's global coordinates in metres, as `f64`.
fn origin_global(origin: Option<RegionHandle>) -> (f64, f64) {
    let Some(origin) = origin else {
        return (0.0, 0.0);
    };
    let (east, north) = origin.global_coordinates();
    (f64::from(east), f64::from(north))
}

/// A Bevy world translation as global metres (east, north, up).
fn global_from_bevy(origin: (f64, f64), translation: Vec3) -> (f64, f64, f32) {
    let sl = bevy_to_sl_vec(translation);
    (origin.0 + f64::from(sl.x), origin.1 + f64::from(sl.y), sl.z)
}

/// Update the per-frame view state: seed the scale from its setting, size the
/// surface image to the node, read the camera pose and cursor, ease the pan.
#[expect(
    clippy::too_many_arguments,
    reason = "the view state genuinely reads the settings, clock, camera, terrain origin, \
              avatar anchors and the surface node, and resizes the image — one per-frame pass"
)]
fn drive_minimap_view(
    ui: Option<Res<MinimapUi>>,
    mut state: ResMut<MinimapState>,
    mut settings: ResMut<ViewerSettings>,
    time: Res<Time>,
    cameras: Query<&GlobalTransform, With<ViewerCamera>>,
    terrain: Res<TerrainState>,
    identity: Res<SlIdentity>,
    avatars: Res<AvatarState>,
    transforms: Query<&GlobalTransform>,
    computed: Query<&ComputedNode>,
    cursors: Query<&RelativeCursorPosition>,
    panels: Query<&UiPanelShown>,
    mut images: ResMut<Assets<Image>>,
    mut image_nodes: Query<&mut ImageNode>,
) {
    let Some(ui) = ui else {
        return;
    };
    if !state.scale_loaded {
        if let Ok(scale) = settings.store().get_f32(SETTING_SCALE) {
            state.scale = minimap_math::clamp_scale(scale);
        }
        state.scale_loaded = true;
    }
    // Debounced write-back of a wheel-changed scale.
    if let Some(timer) = state.scale_save_timer {
        let next = timer - time.delta_secs();
        if next <= 0.0 {
            let value = state.scale;
            settings.set(Scope::Global, SETTING_SCALE, SettingValue::F32(value));
            state.scale_save_timer = None;
        } else {
            state.scale_save_timer = Some(next);
        }
    }

    if !panels.get(ui.root).is_ok_and(|shown| shown.0) {
        return;
    }

    // Surface sizing: match the composited image to the laid-out node, capped.
    if let Ok(node) = computed.get(ui.surface) {
        let size = node.size();
        let width = surface_dimension(size.x);
        let height = surface_dimension(size.y);
        if width >= 16 && height >= 16 && UVec2::new(width, height) != state.surface_px {
            state.surface_px = UVec2::new(width, height);
            if images
                .insert(ui.image.id(), blank_surface(width, height))
                .is_err()
            {
                warn!("minimap: could not resize the surface image");
            }
            state.object_dirty = true;
            state.parcel_dirty = true;
            state.last_stamp = None;
        }
        // The cursor, in image pixels and in logical node pixels.
        let logical = vec2_scale(size, node.inverse_scale_factor());
        state.cursor = None;
        state.cursor_node = None;
        if let Ok(relative) = cursors.get(ui.surface)
            && let Some(normalized) = relative.normalized
            && normalized.x.abs() <= 0.5
            && normalized.y.abs() <= 0.5
        {
            state.cursor = Some(Vec2::new(
                (normalized.x + 0.5) * minimap_math::u32_to_f32(state.surface_px.x),
                (normalized.y + 0.5) * minimap_math::u32_to_f32(state.surface_px.y),
            ));
            state.cursor_node = Some(Vec2::new(
                (normalized.x + 0.5) * logical.x,
                (normalized.y + 0.5) * logical.y,
            ));
        }
    }

    // Surface opacity.
    let opacity = settings.store().get_f32(SETTING_OPACITY).unwrap_or(0.66);
    if let Ok(mut node) = image_nodes.get_mut(ui.surface) {
        let tint = Color::srgba(1.0, 1.0, 1.0, opacity.clamp(0.05, 1.0));
        if node.color != tint {
            node.color = tint;
        }
    }

    // Camera pose → global position and map rotation.
    state.origin = terrain.origin().or(identity.region_handle);
    let origin = origin_global(state.origin);
    if let Ok(camera) = cameras.single() {
        state.camera = global_from_bevy(origin, camera.translation());
        let rotate = settings.store().get_bool(SETTING_ROTATE).unwrap_or(true);
        let at = bevy_to_sl_vec(camera.forward().as_vec3());
        state.heading = minimap_math::rotation_for_camera(at.x, at.y);
        state.view.rotation = if rotate { state.heading } else { 0.0 };
    }

    // The own avatar's position (the marker, rings, distance readouts).
    state.agent = identity
        .agent_id
        .and_then(|agent| avatars.root_entity_of(agent))
        .and_then(|entity| transforms.get(entity).ok())
        .map(|transform| global_from_bevy(origin, transform.translation()));

    // Pan easing: auto-centre (or an explicit re-centre) eases back to zero.
    let auto = settings
        .store()
        .get_bool(SETTING_AUTO_CENTER)
        .unwrap_or(true);
    if (auto || state.centering) && !state.dragging {
        state.pan = minimap_math::auto_center_step(state.pan, time.delta_secs());
        if state.pan == Vec2::ZERO {
            state.centering = false;
        }
    }

    state.view.scale = state.scale;
    state.view.pan = state.pan;
    state.view.size = Vec2::new(
        minimap_math::u32_to_f32(state.surface_px.x),
        minimap_math::u32_to_f32(state.surface_px.y),
    );

    state.object_elapsed += time.delta_secs();
    state.terrain_elapsed += time.delta_secs();
}

/// A laid-out node dimension as a surface pixel count, capped.
fn surface_dimension(value: f32) -> u32 {
    let clamped = value.clamp(0.0, 4096.0);
    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to [0, 4096] just above"
    )]
    let out = (clamped.round() as u32).min(MAX_SURFACE_PX);
    out
}

// ---------------------------------------------------------------------------
// Layer regeneration.
// ---------------------------------------------------------------------------

/// The reference's object-layer refresh period, in seconds.
const OBJECT_LAYER_PERIOD: f32 = 0.5;

/// The parcel layer regenerates when the map centre moves more than this many
/// metres (squared test, the reference's 3 m).
const PARCEL_MOVE_METRES: f64 = 3.0;

/// Regenerate the cached content layers on their own triggers: the object
/// raster on a 0.5 s timer (or dirty), the parcel raster on overlay change /
/// centre movement, the terrain backdrops on terrain revision change.
#[expect(
    clippy::too_many_arguments,
    reason = "the three cached layers read disjoint world state (objects + transforms, parcel \
              overlay + regions, terrain + water); splitting into three systems would triple \
              the shared view-state plumbing without removing any parameter"
)]
fn regen_minimap_layers(
    ui: Option<Res<MinimapUi>>,
    mut state: ResMut<MinimapState>,
    settings: Res<ViewerSettings>,
    objects: Res<ObjectState>,
    transforms: Query<&GlobalTransform>,
    infos: Query<&ObjectDebugInfo>,
    overlay: Res<SlParcelOverlay>,
    regions: Query<(&SlRegion, Option<&SlCurrentRegion>)>,
    terrain: Res<TerrainState>,
    water: Res<WaterState>,
    panels: Query<&UiPanelShown>,
) {
    let Some(ui) = ui else {
        return;
    };
    if overlay.is_changed() {
        state.parcel_dirty = true;
    }
    if !panels.get(ui.root).is_ok_and(|shown| shown.0) {
        return;
    }
    let store = settings.store();

    // Terrain backdrops: rebuild (throttled) when the terrain data moved on.
    if state.terrain_revision != Some(terrain.map_revision()) && state.terrain_elapsed >= 0.5 {
        state.terrain_elapsed = 0.0;
        state.terrain_revision = Some(terrain.map_revision());
        let handles: Vec<RegionHandle> = regions.iter().map(|(region, _)| region.handle).collect();
        state.terrain_maps.clear();
        for handle in handles {
            let map = build_terrain_map(&terrain, &water, handle);
            state.terrain_maps.insert(handle, map);
        }
        state.last_stamp = None;
    }

    // The shared layer raster geometry.
    let raster_size = minimap_math::layer_raster_size(state.view.size);
    let tpm = minimap_math::layer_texels_per_metre(raster_size, state.view.size, state.view.scale);

    // Object layer.
    let show_objects = store.get_bool(SETTING_OBJECTS).unwrap_or(true);
    if show_objects && (state.object_dirty || state.object_elapsed >= OBJECT_LAYER_PERIOD) {
        state.object_dirty = false;
        state.object_elapsed = 0.0;
        let accents = ObjectAccents {
            physical: store.get_bool(SETTING_PHYSICAL).unwrap_or(false),
            scripted: store.get_bool(SETTING_SCRIPTED).unwrap_or(false),
            temp_on_rez: store.get_bool(SETTING_TEMP_ON_REZ).unwrap_or(false),
            phantom_alpha: phantom_alpha(store.get_u32(SETTING_PHANTOM_OPACITY).unwrap_or(100)),
        };
        let max_radius = store.get_f32(SETTING_PRIM_MAX_RADIUS).unwrap_or(16.0);
        let max_vert = store.get_f32(SETTING_PRIM_MAX_VERT).unwrap_or(256.0);
        if state.object_layer.size == raster_size {
            state.object_layer.clear();
        } else {
            state.object_layer = LayerRaster::new(raster_size);
        }
        state.object_center = (state.camera.0, state.camera.1);
        state.object_tpm = tpm;
        let origin = origin_global(state.origin);
        let agent_z = state.agent.map_or(state.camera.2, |agent| agent.2);
        for (entity, flags) in objects.minimap_objects() {
            let Ok(transform) = transforms.get(entity) else {
                continue;
            };
            let scale = infos
                .get(entity)
                .map_or([1.0, 1.0, 1.0], ObjectDebugInfo::scale);
            if !minimap_math::object_on_map(flags, scale, accents) {
                continue;
            }
            let (east, north, up) = global_from_bevy(origin, transform.translation());
            if max_vert > 0.0 && (up - agent_z).abs() > max_vert {
                continue;
            }
            let water_height = region_water_height(&water, east, north);
            let color = minimap_math::object_map_color(flags, up >= water_height, accents);
            let radius = minimap_math::object_map_radius(scale, flags, accents, max_radius);
            let rel_east = narrow(east - state.object_center.0);
            let rel_north = narrow(north - state.object_center.1);
            minimap_math::render_object_point(
                &mut state.object_layer,
                tpm,
                rel_east,
                rel_north,
                color,
                radius,
            );
        }
        state.last_stamp = None;
    }

    // Parcel layer: dirty flag, or the centre moved > 3 m.
    let show_lines = store.get_bool(SETTING_PROPERTY_LINES).unwrap_or(true);
    let centre_moved = {
        let de = state.camera.0 - state.parcel_center.0;
        let dn = state.camera.1 - state.parcel_center.1;
        de * de + dn * dn > PARCEL_MOVE_METRES * PARCEL_MOVE_METRES
    };
    if show_lines && (state.parcel_dirty || centre_moved) {
        state.parcel_dirty = false;
        if state.parcel_layer.size == raster_size {
            state.parcel_layer.clear();
        } else {
            state.parcel_layer = LayerRaster::new(raster_size);
        }
        state.parcel_center = (state.camera.0, state.camera.1);
        state.parcel_tpm = tpm;
        let show_sale = store.get_bool(SETTING_FOR_SALE).unwrap_or(true);
        let parcel_center = state.parcel_center;
        // Split-borrow the raster out of the state so the overlay closure and
        // the &mut raster do not alias.
        let mut raster = core::mem::take(&mut state.parcel_layer);
        for (region, _current) in &regions {
            let (region_east, region_north) = region.handle.global_coordinates();
            let origin_east = narrow(f64::from(region_east) - parcel_center.0);
            let origin_north = narrow(f64::from(region_north) - parcel_center.1);
            // Each region's own decoded overlay (current region always;
            // neighbours once their child circuit delivered one — Second Life
            // pushes them on child establishment, OpenSim on parcel changes).
            let grid = overlay.grid_of(region.handle);
            let cell = |row: usize, col: usize| -> Option<ParcelCell> {
                let cell = grid?.cell(row, col)?;
                let fill = match cell.ownership {
                    ParcelOwnership::ForSale => Some(minimap_math::ParcelFill::ForSale),
                    ParcelOwnership::Auction => Some(minimap_math::ParcelFill::Auction),
                    _other => None,
                };
                Some(ParcelCell {
                    fill,
                    west_line: cell.west_line,
                    south_line: cell.south_line,
                })
            };
            let grids_per_edge = grid.map_or(0, sl_client_bevy::ParcelOverlayGrid::grids_per_edge);
            minimap_math::render_parcel_region(
                &mut raster,
                tpm,
                origin_east,
                origin_north,
                minimap_math::REGION_WIDTH_METRES,
                COLOR_PARCEL_LINE,
                show_sale,
                // A region without a decoded overlay (a neighbour) draws its
                // full outline, since no edge cells supply its south / west
                // property lines.
                grid.is_none(),
                grids_per_edge,
                &cell,
            );
        }
        state.parcel_layer = raster;
        state.last_stamp = None;
    }
}

/// The phantom accent alpha from its percent setting.
fn phantom_alpha(percent: u32) -> u8 {
    let clamped = percent.min(100);
    u8::try_from(clamped.saturating_mul(255).wrapping_div(100)).unwrap_or(255)
}

/// The water height at a global position, from the containing region's
/// handshake (default 20 m, the grid default, when unknown).
fn region_water_height(water: &WaterState, east: f64, north: f64) -> f32 {
    region_handle_at(east, north)
        .and_then(|handle| water.height_of(handle))
        .unwrap_or(20.0)
}

/// The grid index containing a global metre coordinate, if representable.
fn grid_index_at(value: f64) -> Option<u32> {
    if !value.is_finite() || !(0.0..1.0e12).contains(&value) {
        return None;
    }
    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        reason = "range-checked to [0, 1e12) just above, far inside i64"
    )]
    let index = (value / f64::from(minimap_math::REGION_WIDTH_METRES)).floor() as i64;
    u32::try_from(index).ok()
}

/// The region handle containing a global position, if it is on the grid.
fn region_handle_at(east: f64, north: f64) -> Option<RegionHandle> {
    Some(RegionHandle::from_grid(
        grid_index_at(east)?,
        grid_index_at(north)?,
    ))
}

/// Build one region's shaded terrain backdrop: 256×256 texels (one per metre,
/// bottom-up rows), coloured from the decoded height patches and splat
/// weights, hill-shaded, and tinted toward water below the water height.
/// Cells with no decoded patch stay transparent.
fn build_terrain_map(
    terrain: &TerrainState,
    water: &WaterState,
    region: RegionHandle,
) -> LayerRaster {
    /// The backdrop side, in texels (one per metre of a classic region).
    const SIDE: usize = 256;
    let mut heights = vec![f32::NAN; SIDE * SIDE];
    for patch in terrain.land_patches_of(region) {
        let size = usize::try_from(patch.size).unwrap_or(16);
        let base_x = usize::try_from(patch.patch_x)
            .unwrap_or(0)
            .saturating_mul(size);
        let base_y = usize::try_from(patch.patch_y)
            .unwrap_or(0)
            .saturating_mul(size);
        for sy in 0..size {
            for sx in 0..size {
                let x = base_x.saturating_add(sx);
                let y = base_y.saturating_add(sy);
                if x >= SIDE || y >= SIDE {
                    continue;
                }
                let value = patch
                    .value(
                        u32::try_from(sx).unwrap_or(0),
                        u32::try_from(sy).unwrap_or(0),
                    )
                    .unwrap_or(f32::NAN);
                if let Some(slot) = heights.get_mut(y.saturating_mul(SIDE).saturating_add(x)) {
                    *slot = value;
                }
            }
        }
    }
    let water_height = water.height_of(region).unwrap_or(20.0);
    let composition = terrain.composition_of(region);
    let mut raster = LayerRaster::new(256);
    let height_at = |x: usize, y: usize| -> f32 {
        heights
            .get(
                y.min(SIDE - 1)
                    .saturating_mul(SIDE)
                    .saturating_add(x.min(SIDE - 1)),
            )
            .copied()
            .unwrap_or(f32::NAN)
    };
    for y in 0..SIDE {
        for x in 0..SIDE {
            let height = height_at(x, y);
            if !height.is_finite() {
                continue;
            }
            let east = height_at(x.saturating_add(1), y);
            let north = height_at(x, y.saturating_add(1));
            let gradient_east = if east.is_finite() { east - height } else { 0.0 };
            let gradient_north = if north.is_finite() {
                north - height
            } else {
                0.0
            };
            let x_f = minimap_math::u32_to_f32(u32::try_from(x).unwrap_or(0));
            let y_f = minimap_math::u32_to_f32(u32::try_from(y).unwrap_or(0));
            let weights = composition.map_or([0.25, 0.25, 0.25, 0.25], |composition| {
                composition.blend_weights(x_f, y_f, height)
            });
            let color = minimap_math::terrain_texel_color(
                height,
                weights,
                gradient_east,
                gradient_north,
                water_height,
            );
            raster.put(
                i32::try_from(x).unwrap_or(0),
                i32::try_from(y).unwrap_or(0),
                color,
            );
        }
    }
    raster
}

// ---------------------------------------------------------------------------
// Compositing.
// ---------------------------------------------------------------------------

/// The background colour where no region terrain is known (the void).
const VOID_COLOR: Rgba = [24, 28, 34, 255];

/// The neighbouring-region tint (the current region draws untinted).
const NEIGHBOUR_TINT: f32 = 0.8;

/// Composite the surface image when any of its inputs changed: terrain
/// backdrop, object and parcel layers, frustum wedge, chat rings, pick-radius
/// circle, avatar dots, self marker, and the tracking beacon.
#[expect(
    clippy::too_many_arguments,
    reason = "the composite genuinely folds every map data source into one image; a staging \
              resource would only rename the parameters"
)]
fn composite_minimap(
    ui: Option<Res<MinimapUi>>,
    mut state: ResMut<MinimapState>,
    settings: Res<ViewerSettings>,
    identity: Res<SlIdentity>,
    avatars: Res<AvatarState>,
    friends: Option<Res<FriendsModel>>,
    marks: Res<MinimapMarks>,
    tracking: Res<MapTracking>,
    ranges: Res<ChatRanges>,
    transforms: Query<&GlobalTransform>,
    cameras: Query<&Projection, With<ViewerCamera>>,
    regions: Query<(&SlRegion, Option<&SlCurrentRegion>)>,
    panels: Query<&UiPanelShown>,
    mut images: ResMut<Assets<Image>>,
) {
    let Some(ui) = ui else {
        return;
    };
    if !panels.get(ui.root).is_ok_and(|shown| shown.0) {
        return;
    }
    let store = settings.store();
    let show_objects = store.get_bool(SETTING_OBJECTS).unwrap_or(true);
    let show_lines = store.get_bool(SETTING_PROPERTY_LINES).unwrap_or(true);
    let rings_on = store.get_bool(SETTING_CHAT_RING).unwrap_or(false);
    let origin = origin_global(state.origin);

    // Gather this frame's dots first — they are part of the change stamp.
    let mut dots: Vec<DotInfo> = Vec::new();
    let mut altitudes: Vec<f32> = Vec::new();
    let own = identity.agent_id;
    for avatar in avatars.map_avatars() {
        if Some(avatar.agent) == own {
            continue;
        }
        let Ok(transform) = transforms.get(avatar.anchor) else {
            continue;
        };
        let (east, north, up) = global_from_bevy(origin, transform.translation());
        let view = state.view.view_from_rel(
            narrow(east - state.camera.0),
            narrow(north - state.camera.1),
        );
        let altitude_unknown = avatar
            .coarse_z
            .is_some_and(minimap_math::coarse_altitude_unknown);
        let distance = state
            .agent
            .map_or(0.0, |(agent_east, agent_north, agent_up)| {
                let de = narrow(east - agent_east);
                let dn = narrow(north - agent_north);
                let du = up - agent_up;
                (de * de + dn * dn + du * du).sqrt()
            });
        dots.push(DotInfo {
            agent: avatar.agent,
            view,
            distance,
            altitude_unknown,
        });
        altitudes.push(up - state.camera.2);
    }

    let stamp = CompositeStamp {
        camera: (
            quantise(state.camera.0, 16.0),
            quantise(state.camera.1, 16.0),
        ),
        rotation: minimap_math::round_i32(state.view.rotation * 1000.0),
        pan: (
            minimap_math::round_i32(state.pan.x * 4.0),
            minimap_math::round_i32(state.pan.y * 4.0),
        ),
        scale: minimap_math::round_i32(state.scale * 16.0),
        size: state.surface_px,
        cursor: state.cursor.map(|cursor| {
            (
                minimap_math::round_i32(cursor.x),
                minimap_math::round_i32(cursor.y),
            )
        }),
        layers: (
            state.terrain_revision.unwrap_or(0),
            state.object_layer.size,
            state.parcel_layer.size,
            (
                quantise(state.object_center.0, 16.0),
                quantise(state.object_center.1, 16.0),
            ),
            (
                quantise(state.parcel_center.0, 16.0),
                quantise(state.parcel_center.1, 16.0),
            ),
        ),
        dots: dots
            .iter()
            .map(|dot| {
                (
                    dot.agent,
                    minimap_math::round_i32(dot.view.x),
                    minimap_math::round_i32(dot.view.y),
                )
            })
            .collect(),
        tracking: tracking.target.map(|target| match target {
            TrackTarget::Location { east, north, .. } => {
                (quantise(east, 4.0), quantise(north, 4.0), 0)
            }
            TrackTarget::Avatar(agent) => (0, 0, agent.uuid().as_u128()),
        }),
        toggles: (show_objects, show_lines, rings_on),
    };
    if state.last_stamp.as_ref() == Some(&stamp) {
        state.dots = dots;
        return;
    }

    let width = state.surface_px.x;
    let height = state.surface_px.y;
    let mut data = vec![
        0u8;
        usize::try_from(width)
            .unwrap_or(0)
            .saturating_mul(usize::try_from(height).unwrap_or(0))
            .saturating_mul(4)
    ];

    // The inverse transform, incrementally: rel(x, y) = rel00 + x·dx + y·dy.
    let (rel00_e, rel00_n) = state.view.rel_from_view(Vec2::new(0.5, 0.5));
    let (rel10_e, rel10_n) = state.view.rel_from_view(Vec2::new(1.5, 0.5));
    let (rel01_e, rel01_n) = state.view.rel_from_view(Vec2::new(0.5, 1.5));
    let (dx_e, dx_n) = (rel10_e - rel00_e, rel10_n - rel00_n);
    let (dy_e, dy_n) = (rel01_e - rel00_e, rel01_n - rel00_n);

    let current_region = regions
        .iter()
        .find_map(|(region, current)| current.map(|_| region.handle));
    let region_width = f64::from(minimap_math::REGION_WIDTH_METRES);

    let mut offset = 0usize;
    let mut row_e = rel00_e;
    let mut row_n = rel00_n;
    for _y in 0..height {
        let mut rel_e = row_e;
        let mut rel_n = row_n;
        for _x in 0..width {
            let global_e = state.camera.0 + f64::from(rel_e);
            let global_n = state.camera.1 + f64::from(rel_n);
            let mut pixel = VOID_COLOR;
            if global_e >= 0.0 && global_n >= 0.0 {
                let region_e = (global_e / region_width).floor() * region_width;
                let region_n = (global_n / region_width).floor() * region_width;
                if let Some(handle) = region_handle_at(global_e, global_n)
                    && let Some(map) = state.terrain_maps.get(&handle)
                {
                    let local_x = minimap_math::round_i32(narrow(global_e - region_e) - 0.5);
                    let local_y = minimap_math::round_i32(narrow(global_n - region_n) - 0.5);
                    let texel = map.get(local_x, local_y);
                    if texel[3] > 0 {
                        pixel = if Some(handle) == current_region {
                            texel
                        } else {
                            region_tint(texel, NEIGHBOUR_TINT)
                        };
                    }
                }
            }
            if show_objects && state.object_layer.size > 0 {
                let sample = sample_layer(
                    &state.object_layer,
                    state.object_tpm,
                    state.object_center,
                    global_e,
                    global_n,
                );
                pixel = minimap_math::blend_over(pixel, sample);
            }
            if show_lines && state.parcel_layer.size > 0 {
                let sample = sample_layer(
                    &state.parcel_layer,
                    state.parcel_tpm,
                    state.parcel_center,
                    global_e,
                    global_n,
                );
                pixel = minimap_math::blend_over(pixel, sample);
            }
            if let Some(slot) = data.get_mut(offset..offset.saturating_add(4)) {
                slot.copy_from_slice(&pixel);
            }
            offset = offset.saturating_add(4);
            rel_e += dx_e;
            rel_n += dx_n;
        }
        row_e += dy_e;
        row_n += dy_n;
    }

    let mut surface = Surface {
        width,
        height,
        data: &mut data,
    };
    let ppm = state.view.pixels_per_metre();

    // Camera frustum wedge, from the (pan-adjusted) camera point. With
    // rotate-on the map turns under a fixed upward wedge; north-up rotates
    // the wedge itself by the camera heading (clockwise, hence the sign).
    let centre = state.view.view_from_rel(0.0, 0.0);
    let rotate_on = state.view.rotation.abs() > f32::EPSILON;
    let wedge_direction = if rotate_on { 0.0 } else { -state.heading };
    let (fov_width, far_clip) =
        cameras
            .single()
            .map_or((1.3, 4096.0), |projection| match projection {
                Projection::Perspective(perspective) => {
                    (perspective.fov * perspective.aspect_ratio, perspective.far)
                }
                Projection::Orthographic(_) | Projection::Custom(_) => (1.3, 4096.0),
            });
    let wedge_radius = (far_clip * ppm).min(minimap_math::u32_to_f32(width.max(height)) * 1.5);
    minimap_math::draw_wedge(
        &mut surface,
        centre.x,
        centre.y,
        wedge_radius,
        wedge_direction,
        fov_width,
        COLOR_FRUSTUM,
    );

    // Chat rings around the self marker.
    let self_view = state.agent.map(|(east, north, _up)| {
        state.view.view_from_rel(
            narrow(east - state.camera.0),
            narrow(north - state.camera.1),
        )
    });
    if rings_on && let Some(self_view) = self_view {
        for (enabled_setting, range, color) in [
            (SETTING_WHISPER_RING, ranges.whisper, COLOR_WHISPER_RING),
            (SETTING_SAY_RING, ranges.say, COLOR_CHAT_RING),
            (SETTING_SHOUT_RING, ranges.shout, COLOR_SHOUT_RING),
        ] {
            if store.get_bool(enabled_setting).unwrap_or(true) {
                minimap_math::draw_ring(
                    &mut surface,
                    self_view.x,
                    self_view.y,
                    range * ppm,
                    2.0,
                    color,
                );
            }
        }
    }

    // Pick-radius circle at the cursor.
    let dot_radius = minimap_math::dot_radius(ppm);
    if let Some(cursor) = state.cursor {
        let pick_scale = store.get_f32(SETTING_PICK_SCALE).unwrap_or(3.0);
        minimap_math::draw_ring(
            &mut surface,
            cursor.x,
            cursor.y,
            dot_radius * pick_scale,
            1.5,
            [255, 255, 255, 40],
        );
    }

    // Avatar dots.
    for (dot, altitude) in dots.iter().zip(altitudes.iter()) {
        let color = avatar_color(dot.agent, friends.as_deref(), &avatars, &marks);
        let glyph = minimap_math::height_glyph(*altitude, dot.altitude_unknown, state.camera.2);
        minimap_math::draw_avatar_glyph(
            &mut surface,
            dot.view.x,
            dot.view.y,
            dot_radius,
            glyph,
            color,
        );
    }

    // The tracking beacon.
    if let Some(target) = tracking.target {
        let position = match target {
            TrackTarget::Location { east, north, .. } => Some(state.view.view_from_rel(
                narrow(east - state.camera.0),
                narrow(north - state.camera.1),
            )),
            TrackTarget::Avatar(agent) => avatars
                .root_entity_of(agent)
                .and_then(|entity| transforms.get(entity).ok())
                .map(|transform| {
                    let (east, north, _up) = global_from_bevy(origin, transform.translation());
                    state.view.view_from_rel(
                        narrow(east - state.camera.0),
                        narrow(north - state.camera.1),
                    )
                }),
        };
        if let Some(position) = position {
            minimap_math::draw_tracking(&mut surface, position, COLOR_TRACK);
        }
    }

    // The self marker: a yellow dot with a white outline, at the avatar.
    if let Some(self_view) = self_view {
        minimap_math::draw_ring(
            &mut surface,
            self_view.x,
            self_view.y,
            dot_radius + 1.0,
            1.5,
            [255, 255, 255, 230],
        );
        minimap_math::draw_disc(
            &mut surface,
            self_view.x,
            self_view.y,
            dot_radius,
            COLOR_AVATAR_SELF,
        );
    }

    if let Some(mut image) = images.get_mut(ui.image.id()) {
        image.data = Some(data);
    }
    state.dots = dots;
    state.last_stamp = Some(stamp);
}

/// Quantise a global metre coordinate to `1/step` steps for the stamp.
fn quantise(value: f64, step: f64) -> i64 {
    let scaled = (value * step).clamp(-9.0e15, 9.0e15);
    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        reason = "clamped to well inside i64 just above"
    )]
    let out = scaled as i64;
    out
}

/// Sample a world-anchored layer raster at a global position.
fn sample_layer(
    layer: &LayerRaster,
    tpm: f32,
    center: (f64, f64),
    global_e: f64,
    global_n: f64,
) -> Rgba {
    let half = minimap_math::u32_to_f32(layer.size) / 2.0;
    let x = minimap_math::round_i32(narrow(global_e - center.0) * tpm + half - 0.5);
    let y = minimap_math::round_i32(narrow(global_n - center.1) * tpm + half - 0.5);
    layer.get(x, y)
}

/// Multiply a colour's RGB by a tint factor (the neighbour-region dim).
fn region_tint(color: Rgba, factor: f32) -> Rgba {
    let scale = |channel: u8| -> u8 {
        #[expect(
            clippy::as_conversions,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a 0..=255 channel scaled by a 0..=1 factor stays in 0..=255"
        )]
        let out = (f32::from(channel) * factor).round().clamp(0.0, 255.0) as u8;
        out
    };
    [scale(color[0]), scale(color[1]), scale(color[2]), color[3]]
}

/// The dot colour for an avatar: a context-menu mark wins, then Linden /
/// friend classification, then the base colour. (Muted-avatar grey is a
/// follow-up: the viewer holds no mute-list mirror yet.)
fn avatar_color(
    agent: AgentKey,
    friends: Option<&FriendsModel>,
    avatars: &AvatarState,
    marks: &MinimapMarks,
) -> Rgba {
    if let Some(mark) = marks.0.get(&agent) {
        return *mark;
    }
    if let Some(name) = avatars.name_of(agent)
        && name.ends_with(" Linden")
    {
        return COLOR_AVATAR_LINDEN;
    }
    if friends.is_some_and(|friends| friends.is_friend(agent)) {
        return COLOR_AVATAR_FRIEND;
    }
    COLOR_AVATAR
}

// ---------------------------------------------------------------------------
// Compass labels.
// ---------------------------------------------------------------------------

/// Reposition the eight compass labels on the surface edge every frame, and
/// hide the diagonals when the map is small.
fn layout_minimap_compass(
    ui: Option<Res<MinimapUi>>,
    state: Res<MinimapState>,
    computed: Query<&ComputedNode>,
    mut nodes: Query<&mut Node>,
    mut visibilities: Query<&mut Visibility>,
    panels: Query<&UiPanelShown>,
) {
    let Some(ui) = ui else {
        return;
    };
    if !panels.get(ui.root).is_ok_and(|shown| shown.0) {
        return;
    }
    let Ok(surface) = computed.get(ui.surface) else {
        return;
    };
    let surface_logical = vec2_scale(surface.size(), surface.inverse_scale_factor());
    if surface_logical.x < 1.0 || surface_logical.y < 1.0 {
        return;
    }
    let half = vec2_scale(surface_logical, 0.5);
    for (index, wrapper) in ui.compass.iter().enumerate() {
        let Some((_key, base_angle)) = COMPASS_POINTS.get(index) else {
            continue;
        };
        let label_logical = computed
            .get(*wrapper)
            .map_or(Vec2::new(12.0, 12.0), |node| {
                vec2_scale(node.size(), node.inverse_scale_factor())
            });
        let minor = COMPASS_MINOR.get(index).copied().unwrap_or(false);
        if minor && let Ok(mut visibility) = visibilities.get_mut(*wrapper) {
            *visibility =
                if minimap_math::minor_directions_visible(label_logical.y, surface_logical) {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
        }
        // The label direction is the world direction turned by the map
        // rotation (labels ride the rotating map).
        let angle = base_angle + state.view.rotation;
        let padding = label_logical.x / 2.0;
        let offset = minimap_math::compass_label_offset(
            angle,
            (half.x - label_logical.x / 2.0 - padding).max(4.0),
            (half.y - label_logical.y / 2.0 - padding).max(4.0),
        );
        if let Ok(mut node) = nodes.get_mut(*wrapper) {
            node.left = Val::Px(half.x + offset.x - label_logical.x / 2.0);
            node.top = Val::Px(half.y + offset.y - label_logical.y / 2.0);
        }
    }
}

// ---------------------------------------------------------------------------
// Hover and tooltip.
// ---------------------------------------------------------------------------

/// The closest dot within the pick radius of a cursor, plus every dot inside
/// the radius.
fn dots_near(
    dots: &[DotInfo],
    cursor: Vec2,
    pick_radius: f32,
) -> (Option<AgentKey>, Vec<AgentKey>) {
    let mut closest: Option<(AgentKey, f32)> = None;
    let mut all = Vec::new();
    for dot in dots {
        let distance = dot.view.distance(cursor);
        if distance <= pick_radius {
            all.push(dot.agent);
            if closest.is_none_or(|(_, best)| distance < best) {
                closest = Some((dot.agent, distance));
            }
        }
    }
    (closest.map(|(agent, _)| agent), all)
}

/// Update the hover tooltip: an avatar's name and distance when a dot is
/// under the cursor, otherwise the region (and, with property lines on, the
/// parcel) under the cursor plus the double-click hint.
#[expect(
    clippy::too_many_arguments,
    reason = "the tooltip reads the hover state, the settings, avatar names, region and parcel \
              mirrors, and writes the tooltip nodes — one cohesive pass"
)]
fn update_minimap_hover(
    ui: Option<Res<MinimapUi>>,
    state: Res<MinimapState>,
    settings: Res<ViewerSettings>,
    avatars: Res<AvatarState>,
    translator: Translator,
    regions: Query<(&SlRegion, Option<&SlRegionIdentity>)>,
    parcels: Query<(&SlParcel, &ChildOf)>,
    region_handles: Query<&SlRegion>,
    panels: Query<&UiPanelShown>,
    mut nodes: Query<&mut Node>,
    mut visibilities: Query<&mut Visibility>,
    mut texts: Query<&mut Text>,
) {
    let Some(ui) = ui else {
        return;
    };
    let shown = panels.get(ui.root).is_ok_and(|shown| shown.0);
    let store = settings.store();
    let mut lines: Vec<String> = Vec::new();
    if shown && let (Some(cursor), Some(cursor_node)) = (state.cursor, state.cursor_node) {
        let ppm = state.view.pixels_per_metre();
        let pick_scale = store.get_f32(SETTING_PICK_SCALE).unwrap_or(3.0);
        let pick_radius = minimap_math::dot_radius(ppm) * pick_scale;
        let (closest, _all) = dots_near(&state.dots, cursor, pick_radius);
        if let Some(agent) = closest {
            let name = avatars
                .name_of(agent)
                .map_or_else(|| agent.uuid().to_string(), ToOwned::to_owned);
            let dot = state.dots.iter().find(|dot| dot.agent == agent);
            let unknown = dot.is_some_and(|dot| dot.altitude_unknown);
            let distance = dot.map_or(0.0, |dot| dot.distance);
            let key = if unknown {
                "minimap-tooltip-avatar-far"
            } else {
                "minimap-tooltip-avatar"
            };
            let shown_distance = if unknown { 4096.0 } else { distance };
            lines.push(
                translator.format(
                    key,
                    &TransArgs::new()
                        .text("name", &name)
                        .int("distance", metres_int(shown_distance)),
                ),
            );
        } else {
            // The region (and parcel) under the cursor.
            let (rel_east, rel_north) = state.view.rel_from_view(cursor);
            let global_e = state.camera.0 + f64::from(rel_east);
            let global_n = state.camera.1 + f64::from(rel_north);
            if let Some(handle) = region_handle_at(global_e, global_n) {
                if let Some(name) = regions.iter().find_map(|(region, identity)| {
                    (region.handle == handle)
                        .then(|| {
                            identity.and_then(|identity| {
                                identity.0.sim_name.as_ref().map(ToString::to_string)
                            })
                        })
                        .flatten()
                }) {
                    lines.push(translator.format(
                        "minimap-tooltip-region",
                        &TransArgs::new().text("name", &name),
                    ));
                }
                if store.get_bool(SETTING_PROPERTY_LINES).unwrap_or(true) {
                    let (region_east, region_north) = handle.global_coordinates();
                    let local_x = narrow(global_e - f64::from(region_east));
                    let local_y = narrow(global_n - f64::from(region_north));
                    if let Some(info) =
                        parcel_at(&parcels, &region_handles, handle, local_x, local_y)
                    {
                        parcel_tooltip_lines(&translator, &avatars, info, &mut lines);
                    }
                }
            }
            match double_click_action(store) {
                DoubleClickAction::Teleport => {
                    lines.push(translator.get("minimap-tooltip-hint-teleport"));
                }
                DoubleClickAction::WorldMap => {
                    lines.push(translator.get("minimap-tooltip-hint-map"));
                }
                DoubleClickAction::Nothing => {}
            }
        }
        if let Ok(mut node) = nodes.get_mut(ui.tooltip) {
            node.left = Val::Px(cursor_node.x + 12.0);
            node.top = Val::Px(cursor_node.y + 12.0);
        }
    }
    let visible = !lines.is_empty();
    if let Ok(mut visibility) = visibilities.get_mut(ui.tooltip) {
        *visibility = if visible {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
    if visible && let Ok(mut text) = texts.get_mut(ui.tooltip_text) {
        let joined = lines.join("\n");
        if text.0 != joined {
            text.0 = joined;
        }
    }
}

/// A distance in metres as a whole number for the tooltip.
const fn metres_int(value: f32) -> i64 {
    let clamped = value.clamp(0.0, 1.0e9);
    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        reason = "clamped to [0, 1e9] just above"
    )]
    let out = clamped.round() as i64;
    out
}

/// The double-click action from its setting.
fn double_click_action(store: &sl_settings::SettingsStore) -> DoubleClickAction {
    DoubleClickAction::from_setting(store.get_i32(SETTING_DOUBLE_CLICK).unwrap_or(2))
}

/// The parcel containing a region-local point, from the parcel mirror (today
/// only the current region's visited parcels are known).
fn parcel_at<'world>(
    parcels: &'world Query<(&SlParcel, &ChildOf)>,
    region_handles: &Query<&SlRegion>,
    handle: RegionHandle,
    local_x: f32,
    local_y: f32,
) -> Option<&'world ParcelInfoRef> {
    parcels.iter().find_map(|(parcel, child_of)| {
        let region = region_handles.get(child_of.parent()).ok()?;
        (region.handle == handle && parcel.0.contains_point(local_x, local_y)).then_some(&parcel.0)
    })
}

/// The parcel-info alias [`parcel_at`] returns (the mirror's stored type).
type ParcelInfoRef = sl_client_bevy::ParcelInfo;

/// Append the parcel lines (name, owner, for-sale price and area) to the
/// tooltip.
fn parcel_tooltip_lines(
    translator: &Translator,
    avatars: &AvatarState,
    info: &ParcelInfoRef,
    lines: &mut Vec<String>,
) {
    if !info.name.is_empty() {
        lines.push(translator.format(
            "minimap-tooltip-parcel",
            &TransArgs::new().text("name", &info.name),
        ));
    }
    if let OwnerKey::Agent(agent) = info.owner
        && let Some(name) = avatars.name_of(agent)
    {
        lines.push(translator.format(
            "minimap-tooltip-owner",
            &TransArgs::new().text("name", name),
        ));
    }
    if let Some(price) = &info.sale_price {
        lines.push(
            translator.format(
                "minimap-tooltip-sale",
                &TransArgs::new()
                    .int("price", i64::try_from(price.0).unwrap_or(0))
                    .int("area", i64::from(info.area.0)),
            ),
        );
    }
}

// ---------------------------------------------------------------------------
// Input observers.
// ---------------------------------------------------------------------------

/// Seconds within which two clicks are a double-click.
const DOUBLE_CLICK_SECONDS: f64 = 0.4;

/// Pixels within which two clicks are a double-click.
const DOUBLE_CLICK_SLOP: f32 = 6.0;

/// A primary click on the surface: track double-clicks and run the configured
/// double-click action (teleport / beacon).
#[expect(
    clippy::too_many_arguments,
    reason = "the double-click action needs the clock, view state, settings, terrain (for the \
              arrival height), identity, tracking and the command channel at click time"
)]
fn on_minimap_click(
    click: On<Pointer<Click>>,
    time: Res<Time>,
    mut state: ResMut<MinimapState>,
    settings: Res<ViewerSettings>,
    terrain: Res<TerrainState>,
    identity: Res<SlIdentity>,
    mut tracking: ResMut<MapTracking>,
    mut commands: MessageWriter<SlCommand>,
) {
    if click.button != PointerButton::Primary {
        return;
    }
    let Some(cursor) = state.cursor else {
        return;
    };
    let now = time.elapsed_secs_f64();
    let double = state.last_click.is_some_and(|(at, position)| {
        now - at <= DOUBLE_CLICK_SECONDS && position.distance(cursor) <= DOUBLE_CLICK_SLOP
    });
    if !double {
        state.last_click = Some((now, cursor));
        return;
    }
    state.last_click = None;
    let action = double_click_action(settings.store());
    if action == DoubleClickAction::Nothing {
        return;
    }
    // Both remaining actions set a tracking beacon first (unless already
    // tracking), as the reference does.
    let (rel_east, rel_north) = state.view.rel_from_view(cursor);
    let global_e = state.camera.0 + f64::from(rel_east);
    let global_n = state.camera.1 + f64::from(rel_north);
    let Some(handle) = region_handle_at(global_e, global_n) else {
        return;
    };
    let (region_east, region_north) = handle.global_coordinates();
    let local_x = narrow(global_e - f64::from(region_east));
    let local_y = narrow(global_n - f64::from(region_north));
    let up = terrain
        .land_height(handle, local_x, local_y)
        .unwrap_or_else(|| state.agent.map_or(state.camera.2, |agent| agent.2));
    if tracking.target.is_none() {
        tracking.target = Some(TrackTarget::Location {
            east: global_e,
            north: global_n,
            up,
        });
    }
    if action == DoubleClickAction::Teleport {
        // Aim the arrival look-at from the agent toward the target.
        let look = state.agent.map_or(
            Vector {
                x: 1.0,
                y: 0.0,
                z: 0.0,
            },
            |(agent_east, agent_north, _up)| {
                let de = narrow(global_e - agent_east);
                let dn = narrow(global_n - agent_north);
                let length = (de * de + dn * dn).sqrt().max(0.001);
                Vector {
                    x: de / length,
                    y: dn / length,
                    z: 0.0,
                }
            },
        );
        let _current = identity.region_handle;
        commands.write(SlCommand(Command::Teleport {
            region_handle: handle,
            position: RegionCoordinates::new(local_x, local_y, up),
            look_at: look,
        }));
    }
    // The world-map action has no world-map floater to open yet
    // (`viewer-world-map-floater`); the beacon above still places.
}

/// A SHIFT-drag on the surface pans the map (2 px slop via the drag
/// threshold), suspending auto-centring while the button is held.
fn on_minimap_drag(
    drag: On<Pointer<Drag>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<MinimapState>,
) {
    if drag.button != PointerButton::Primary {
        return;
    }
    if !(keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight)) {
        return;
    }
    state.dragging = true;
    state.centering = false;
    // Screen y grows downward; the pan's frame has +y toward the top edge.
    state.pan.x += drag.delta.x;
    state.pan.y -= drag.delta.y;
}

/// The end of a drag re-enables auto-centring.
fn on_minimap_drag_end(drag: On<Pointer<DragEnd>>, mut state: ResMut<MinimapState>) {
    if drag.button == PointerButton::Primary {
        state.dragging = false;
    }
}

/// A scroll wheel over the surface zooms (4 % per notch), toward the cursor
/// when auto-centring is off.
fn on_minimap_scroll(
    mut event: On<Pointer<Scroll>>,
    mut state: ResMut<MinimapState>,
    settings: Res<ViewerSettings>,
) {
    let old_scale = state.scale;
    // Wheel up (positive y) zooms in: the reference's reversed "clicks".
    let new_scale = minimap_math::wheel_scale(old_scale, -event.y);
    if (new_scale - old_scale).abs() < f32::EPSILON {
        event.propagate(false);
        return;
    }
    state.scale = new_scale;
    state.pan = minimap_math::rescale_pan(state.pan, old_scale, new_scale);
    let auto = settings
        .store()
        .get_bool(SETTING_AUTO_CENTER)
        .unwrap_or(true);
    if !auto && let Some(cursor) = state.cursor {
        state.pan = minimap_math::zoom_to_cursor_pan(
            state.pan,
            cursor,
            state.view.size,
            old_scale,
            new_scale,
        );
    }
    state.object_dirty = true;
    state.parcel_dirty = true;
    state.scale_save_timer = Some(1.0);
    event.propagate(false);
}

/// A right-click (secondary press) on the surface opens the context menu,
/// snapshotting the dot under the cursor and the checked/enabled conditions.
fn on_minimap_context(
    mut press: On<Pointer<Press>>,
    mut state: ResMut<MinimapState>,
    settings: Res<ViewerSettings>,
    tracking: Res<MapTracking>,
    ui: Option<Res<MinimapUi>>,
    mut menus: MessageWriter<OpenContextMenu>,
) {
    if press.button != PointerButton::Secondary {
        return;
    }
    let Some(_ui) = ui else {
        return;
    };
    let store = settings.store();
    let cursor = state.cursor.unwrap_or(Vec2::ZERO);
    let ppm = state.view.pixels_per_metre();
    let pick_radius =
        minimap_math::dot_radius(ppm) * store.get_f32(SETTING_PICK_SCALE).unwrap_or(3.0);
    let (closest, all) = dots_near(&state.dots, cursor, pick_radius);
    state.menu = MenuContext {
        agent: closest,
        agents: all,
    };

    let mut conditions: Vec<&'static str> = Vec::new();
    if state.menu.agent.is_some() {
        conditions.push(COND_AVATAR);
    }
    if tracking.target.is_some() {
        conditions.push(COND_TRACKING);
    }
    for (condition, preset) in [
        (COND_ZOOM_VERY_CLOSE, minimap_math::MAP_SCALE_VERY_CLOSE),
        (COND_ZOOM_CLOSE, minimap_math::MAP_SCALE_CLOSE),
        (COND_ZOOM_MEDIUM, minimap_math::MAP_SCALE_MEDIUM),
        (COND_ZOOM_FAR, minimap_math::MAP_SCALE_FAR),
    ] {
        if (state.scale - preset).abs() < 0.5 {
            conditions.push(condition);
        }
    }
    if store.get_bool(SETTING_ROTATE).unwrap_or(true) {
        conditions.push(COND_CAMERA_UP);
    } else {
        conditions.push(COND_NORTH_UP);
    }
    if store.get_bool(SETTING_AUTO_CENTER).unwrap_or(true) {
        conditions.push(COND_AUTO_CENTER);
    }
    if state.pan != Vec2::ZERO && !store.get_bool(SETTING_AUTO_CENTER).unwrap_or(true) {
        conditions.push(COND_CAN_RECENTER);
    }
    for (condition, setting, default) in [
        (COND_SHOW_OBJECTS, SETTING_OBJECTS, true),
        (COND_SHOW_PHYSICAL, SETTING_PHYSICAL, false),
        (COND_SHOW_SCRIPTED, SETTING_SCRIPTED, false),
        (COND_SHOW_TEMP, SETTING_TEMP_ON_REZ, false),
        (COND_SHOW_LINES, SETTING_PROPERTY_LINES, true),
        (COND_SHOW_SALE, SETTING_FOR_SALE, true),
        (COND_RING, SETTING_CHAT_RING, false),
        (COND_RING_WHISPER, SETTING_WHISPER_RING, true),
        (COND_RING_SAY, SETTING_SAY_RING, true),
        (COND_RING_SHOUT, SETTING_SHOUT_RING, true),
    ] {
        if store.get_bool(setting).unwrap_or(default) {
            conditions.push(condition);
        }
    }

    menus.write(OpenContextMenu {
        menu: &MINIMAP_MENU,
        at: press.pointer_location.position,
        element: MINIMAP_ELEMENT,
        conditions,
    });
    press.propagate(false);
}

// ---------------------------------------------------------------------------
// The context menu.
// ---------------------------------------------------------------------------

/// Condition: an avatar dot is under the right-click.
const COND_AVATAR: &str = "minimap-avatar";

/// Condition: a tracking beacon is active.
const COND_TRACKING: &str = "minimap-tracking";

/// Condition: the scale matches the Very Close preset.
const COND_ZOOM_VERY_CLOSE: &str = "minimap-zoom-very-close";

/// Condition: the scale matches the Close preset.
const COND_ZOOM_CLOSE: &str = "minimap-zoom-close";

/// Condition: the scale matches the Medium preset.
const COND_ZOOM_MEDIUM: &str = "minimap-zoom-medium";

/// Condition: the scale matches the Far preset.
const COND_ZOOM_FAR: &str = "minimap-zoom-far";

/// Condition: the map is north-up.
const COND_NORTH_UP: &str = "minimap-north-up";

/// Condition: the map rotates with the camera.
const COND_CAMERA_UP: &str = "minimap-camera-up";

/// Condition: auto-centring is on.
const COND_AUTO_CENTER: &str = "minimap-auto-center";

/// Condition: the map is panned off-centre and can be re-centred.
const COND_CAN_RECENTER: &str = "minimap-can-recenter";

/// Condition: the object layer is shown.
const COND_SHOW_OBJECTS: &str = "minimap-show-objects";

/// Condition: the physical-object accent is on.
const COND_SHOW_PHYSICAL: &str = "minimap-show-physical";

/// Condition: the scripted-object accent is on.
const COND_SHOW_SCRIPTED: &str = "minimap-show-scripted";

/// Condition: the temp-on-rez accent is on.
const COND_SHOW_TEMP: &str = "minimap-show-temp";

/// Condition: property lines are shown.
const COND_SHOW_LINES: &str = "minimap-show-lines";

/// Condition: for-sale parcel fills are shown.
const COND_SHOW_SALE: &str = "minimap-show-sale";

/// Condition: the chat-ring master toggle is on.
const COND_RING: &str = "minimap-ring";

/// Condition: the whisper ring is on.
const COND_RING_WHISPER: &str = "minimap-ring-whisper";

/// Condition: the say ring is on.
const COND_RING_SAY: &str = "minimap-ring-say";

/// Condition: the shout ring is on.
const COND_RING_SHOUT: &str = "minimap-ring-shout";

/// The avatar Mark submenu (dot colours fed to [`MinimapMarks`]).
static MINIMAP_MARK_MENU: MenuDef = MenuDef {
    label: "Mark",
    items: &[
        MenuItemDef::Command(MenuCommand::new("Mark Red", "mark-red")),
        MenuItemDef::Command(MenuCommand::new("Mark Green", "mark-green")),
        MenuItemDef::Command(MenuCommand::new("Mark Blue", "mark-blue")),
        MenuItemDef::Command(MenuCommand::new("Mark Purple", "mark-purple")),
        MenuItemDef::Command(MenuCommand::new("Mark Light Yellow", "mark-yellow")),
        MenuItemDef::Separator,
        MenuItemDef::Command(MenuCommand::new("Clear Mark", "mark-clear")),
        MenuItemDef::Command(MenuCommand::new("Clear All Marks", "mark-clear-all")),
    ],
};

/// The avatar More Options submenu — routed to the same shared actions the
/// avatar pie / people panel use ([`OpenConversation`], [`OpenAvatarProfile`],
/// friendship / teleport-offer / mute commands).
static MINIMAP_MORE_MENU: MenuDef = MenuDef {
    label: "More Options",
    items: &[
        MenuItemDef::Command(MenuCommand::new("IM", "im")),
        MenuItemDef::Command(MenuCommand::new("Add Friend", "add-friend")),
        MenuItemDef::Command(MenuCommand::new("Offer Teleport", "offer-teleport")),
        MenuItemDef::Command(MenuCommand::new("Block", "block")),
    ],
};

/// The Zoom submenu (radio checks on the preset in effect).
static MINIMAP_ZOOM_MENU: MenuDef = MenuDef {
    label: "Zoom",
    items: &[
        MenuItemDef::Command(
            MenuCommand::new("Very Close", "zoom-very-close").checked_when(COND_ZOOM_VERY_CLOSE),
        ),
        MenuItemDef::Command(MenuCommand::new("Close", "zoom-close").checked_when(COND_ZOOM_CLOSE)),
        MenuItemDef::Command(
            MenuCommand::new("Medium", "zoom-medium").checked_when(COND_ZOOM_MEDIUM),
        ),
        MenuItemDef::Command(MenuCommand::new("Far", "zoom-far").checked_when(COND_ZOOM_FAR)),
    ],
};

/// The Show submenu (layer and accent toggles).
static MINIMAP_SHOW_MENU: MenuDef = MenuDef {
    label: "Show",
    items: &[
        MenuItemDef::Command(
            MenuCommand::new("Objects", "toggle-objects").checked_when(COND_SHOW_OBJECTS),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Physical Objects", "toggle-physical")
                .checked_when(COND_SHOW_PHYSICAL)
                .enabled_when(COND_SHOW_OBJECTS),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Scripted Objects", "toggle-scripted")
                .checked_when(COND_SHOW_SCRIPTED)
                .enabled_when(COND_SHOW_OBJECTS),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Temp-on-rez Objects", "toggle-temp")
                .checked_when(COND_SHOW_TEMP)
                .enabled_when(COND_SHOW_OBJECTS),
        ),
        MenuItemDef::Separator,
        MenuItemDef::Command(
            MenuCommand::new("Property Lines", "toggle-lines").checked_when(COND_SHOW_LINES),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Parcels for Sale", "toggle-sale")
                .checked_when(COND_SHOW_SALE)
                .enabled_when(COND_SHOW_LINES),
        ),
    ],
};

/// The Chat Distance Rings submenu.
static MINIMAP_RINGS_MENU: MenuDef = MenuDef {
    label: "Chat Distance Rings",
    items: &[
        MenuItemDef::Command(MenuCommand::new("Show Rings", "toggle-ring").checked_when(COND_RING)),
        MenuItemDef::Separator,
        MenuItemDef::Command(
            MenuCommand::new("Whisper Range", "toggle-ring-whisper")
                .checked_when(COND_RING_WHISPER)
                .enabled_when(COND_RING),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Chat Range", "toggle-ring-say")
                .checked_when(COND_RING_SAY)
                .enabled_when(COND_RING),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Shout Range", "toggle-ring-shout")
                .checked_when(COND_RING_SHOUT)
                .enabled_when(COND_RING),
        ),
    ],
};

/// The minimap context menu: the avatar group (when a dot is under the
/// cursor), tracking, then the map controls.
static MINIMAP_MENU: MenuDef = MenuDef {
    label: "Minimap",
    items: &[
        MenuItemDef::Command(MenuCommand::new("View Profile", "profile").visible_when(COND_AVATAR)),
        MenuItemDef::SubmenuWhen(&MINIMAP_MARK_MENU, COND_AVATAR),
        MenuItemDef::SubmenuWhen(&MINIMAP_MORE_MENU, COND_AVATAR),
        MenuItemDef::Command(
            MenuCommand::new("Start Tracking", "start-tracking").visible_when(COND_AVATAR),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Stop Tracking", "stop-tracking").visible_when(COND_TRACKING),
        ),
        MenuItemDef::Separator,
        MenuItemDef::Submenu(&MINIMAP_ZOOM_MENU),
        MenuItemDef::Submenu(&MINIMAP_SHOW_MENU),
        MenuItemDef::Submenu(&MINIMAP_RINGS_MENU),
        MenuItemDef::Separator,
        MenuItemDef::Command(
            MenuCommand::new("North at Top", "north-at-top").checked_when(COND_NORTH_UP),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Camera at Top", "camera-at-top").checked_when(COND_CAMERA_UP),
        ),
        MenuItemDef::Separator,
        MenuItemDef::Command(
            MenuCommand::new("Auto-center Map", "toggle-auto-center")
                .checked_when(COND_AUTO_CENTER),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Re-center Map", "recenter").enabled_when(COND_CAN_RECENTER),
        ),
    ],
};

/// The mark colours by action suffix.
const MARK_COLORS: [(&str, Rgba); 5] = [
    ("mark-red", [255, 64, 64, 255]),
    ("mark-green", [64, 255, 64, 255]),
    ("mark-blue", [64, 96, 255, 255]),
    ("mark-purple", [200, 64, 255, 255]),
    ("mark-yellow", [255, 255, 160, 255]),
];

/// Dispatch the minimap's context-menu picks: settings toggles, zoom presets,
/// marks, tracking, and the avatar actions (routed to the shared avatar
/// action messages / commands).
#[expect(
    clippy::too_many_arguments,
    reason = "the action dispatch fans out to the settings store, the mark / tracking \
              resources, and the three shared avatar-action channels"
)]
fn handle_minimap_actions(
    mut actions: MessageReader<UiAction>,
    mut state: ResMut<MinimapState>,
    mut settings: ResMut<ViewerSettings>,
    mut marks: ResMut<MinimapMarks>,
    mut tracking: ResMut<MapTracking>,
    avatars: Res<AvatarState>,
    mut commands: MessageWriter<SlCommand>,
    mut conversations: MessageWriter<OpenConversation>,
    mut profiles: MessageWriter<OpenAvatarProfile>,
) {
    for action in actions.read() {
        if action.element != MINIMAP_ELEMENT {
            continue;
        }
        let agent = state.menu.agent;
        match action.action {
            "profile" => {
                if let Some(agent) = agent {
                    profiles.write(OpenAvatarProfile { agent });
                }
            }
            "im" => {
                if let Some(agent) = agent {
                    conversations.write(OpenConversation {
                        key: ConversationKey::Direct(agent),
                    });
                }
            }
            "add-friend" => {
                if let Some(agent) = agent {
                    commands.write(SlCommand(Command::OfferFriendship {
                        to_agent_id: agent,
                        message: String::new(),
                    }));
                }
            }
            "offer-teleport" => {
                if let Some(agent) = agent {
                    commands.write(SlCommand(Command::OfferTeleport {
                        targets: vec![agent],
                        message: String::new(),
                    }));
                }
            }
            "block" => {
                if let Some(agent) = agent {
                    let name = avatars
                        .name_of(agent)
                        .map(ToOwned::to_owned)
                        .unwrap_or_default();
                    commands.write(SlCommand(Command::Mute {
                        id: agent.uuid(),
                        name,
                        mute_type: MuteType::Agent,
                        flags: MuteFlags::default(),
                    }));
                }
            }
            "start-tracking" => {
                if let Some(agent) = agent {
                    tracking.target = Some(TrackTarget::Avatar(agent));
                }
            }
            "stop-tracking" => {
                tracking.target = None;
            }
            "mark-clear" => {
                for agent in &state.menu.agents {
                    marks.0.remove(agent);
                }
            }
            "mark-clear-all" => {
                marks.0.clear();
            }
            "zoom-very-close" => set_scale(&mut state, minimap_math::MAP_SCALE_VERY_CLOSE),
            "zoom-close" => set_scale(&mut state, minimap_math::MAP_SCALE_CLOSE),
            "zoom-medium" => set_scale(&mut state, minimap_math::MAP_SCALE_MEDIUM),
            "zoom-far" => set_scale(&mut state, minimap_math::MAP_SCALE_FAR),
            "toggle-objects" => toggle_setting(&mut settings, SETTING_OBJECTS, true),
            "toggle-physical" => {
                toggle_setting(&mut settings, SETTING_PHYSICAL, false);
                state.object_dirty = true;
            }
            "toggle-scripted" => {
                toggle_setting(&mut settings, SETTING_SCRIPTED, false);
                state.object_dirty = true;
            }
            "toggle-temp" => {
                toggle_setting(&mut settings, SETTING_TEMP_ON_REZ, false);
                state.object_dirty = true;
            }
            "toggle-lines" => {
                toggle_setting(&mut settings, SETTING_PROPERTY_LINES, true);
                state.parcel_dirty = true;
            }
            "toggle-sale" => {
                toggle_setting(&mut settings, SETTING_FOR_SALE, true);
                state.parcel_dirty = true;
            }
            "toggle-ring" => toggle_setting(&mut settings, SETTING_CHAT_RING, false),
            "toggle-ring-whisper" => toggle_setting(&mut settings, SETTING_WHISPER_RING, true),
            "toggle-ring-say" => toggle_setting(&mut settings, SETTING_SAY_RING, true),
            "toggle-ring-shout" => toggle_setting(&mut settings, SETTING_SHOUT_RING, true),
            "north-at-top" => {
                settings.set(Scope::Global, SETTING_ROTATE, SettingValue::Bool(false));
            }
            "camera-at-top" => {
                settings.set(Scope::Global, SETTING_ROTATE, SettingValue::Bool(true));
            }
            "toggle-auto-center" => toggle_setting(&mut settings, SETTING_AUTO_CENTER, true),
            "recenter" => {
                state.centering = true;
            }
            other => {
                if let Some((_action, color)) =
                    MARK_COLORS.iter().find(|(name, _color)| *name == other)
                {
                    for agent in state.menu.agents.clone() {
                        marks.0.insert(agent, *color);
                    }
                    // A mark recolours dots immediately.
                    state.last_stamp = None;
                }
            }
        }
        // Any toggle can change what the composite draws.
        if action.action.starts_with("toggle-") {
            state.last_stamp = None;
        }
    }
}

/// Set the map scale from a zoom preset (persisted immediately — a menu pick
/// is deliberate, unlike wheel spam).
fn set_scale(state: &mut MinimapState, scale: f32) {
    let old = state.scale;
    let clamped = minimap_math::clamp_scale(scale);
    state.scale = clamped;
    state.pan = minimap_math::rescale_pan(state.pan, old, clamped);
    state.object_dirty = true;
    state.parcel_dirty = true;
    state.scale_save_timer = Some(0.0);
}

/// Flip a boolean setting in the global scope.
fn toggle_setting(settings: &mut ViewerSettings, name: &str, default: bool) {
    let current = settings.store().get_bool(name).unwrap_or(default);
    settings.set(Scope::Global, name, SettingValue::Bool(!current));
}

// ---------------------------------------------------------------------------
// Mouselook.
// ---------------------------------------------------------------------------

/// Make the minimap mouse-transparent while in mouselook (the reference's
/// behaviour: the map stays visible but never captures the mouse).
fn apply_minimap_mouselook(
    ui: Option<Res<MinimapUi>>,
    mode: Res<CameraMode>,
    mut pickables: Query<&mut Pickable>,
) {
    let Some(ui) = ui else {
        return;
    };
    if !mode.is_changed() {
        return;
    }
    let transparent = *mode == CameraMode::Mouselook;
    for entity in [ui.root, ui.surface] {
        if let Ok(mut pickable) = pickables.get_mut(entity) {
            *pickable = if transparent {
                Pickable {
                    should_block_lower: false,
                    is_hoverable: false,
                }
            } else {
                Pickable {
                    should_block_lower: true,
                    is_hoverable: true,
                }
            };
        }
    }
}

// ---------------------------------------------------------------------------
// Gallery specimen.
// ---------------------------------------------------------------------------

/// A static minimap look for the gallery / harness: the surface panel with a
/// terrain-ish background, a few avatar dots, the self marker and the compass
/// labels — no live session, no image compositing.
pub(crate) fn spawn_minimap_specimen(
    commands: &mut Commands,
    parent: Entity,
    _cx: ElementCx,
) -> Entity {
    let root = commands
        .spawn((
            Node {
                width: Val::Px(180.0),
                height: Val::Px(180.0),
                ..default()
            },
            BackgroundColor(Color::srgb(0.24, 0.33, 0.19)),
            Name::new("minimap-specimen"),
            ChildOf(parent),
        ))
        .id();
    // A "water" corner, a parcel line, some dots.
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            top: Val::Px(110.0),
            width: Val::Px(70.0),
            height: Val::Px(70.0),
            ..default()
        },
        BackgroundColor(Color::srgb(0.10, 0.22, 0.35)),
        ChildOf(root),
    ));
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(90.0),
            top: Val::Px(0.0),
            width: Val::Px(1.0),
            height: Val::Px(180.0),
            ..default()
        },
        BackgroundColor(Color::WHITE),
        ChildOf(root),
    ));
    for (x, y, color) in [
        (40.0, 60.0, Color::srgb(1.0, 0.0, 0.0)),
        (120.0, 40.0, Color::srgb(0.0, 1.0, 0.0)),
        (88.0, 88.0, Color::srgb(1.0, 1.0, 0.0)),
    ] {
        commands.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(x),
                top: Val::Px(y),
                width: Val::Px(7.0),
                height: Val::Px(7.0),
                border_radius: BorderRadius::all(Val::Px(4.0)),
                ..default()
            },
            BackgroundColor(color),
            ChildOf(root),
        ));
    }
    for (label, left, top) in [
        ("N", 84.0, 2.0),
        ("E", 166.0, 84.0),
        ("S", 84.0, 160.0),
        ("W", 4.0, 84.0),
    ] {
        let wrapper = commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(left),
                    top: Val::Px(top),
                    ..default()
                },
                ChildOf(root),
            ))
            .id();
        commands.spawn((
            Text::new(label),
            UiFont::Sans.at(COMPASS_FONT_SIZE),
            TextColor(Color::srgba(1.0, 1.0, 1.0, 0.7)),
            ChildOf(wrapper),
        ));
    }
    root
}

#[cfg(test)]
mod tests {
    use super::{
        COMPASS_MINOR, COMPASS_POINTS, MARK_COLORS, MINIMAP_MENU, MenuDef, MenuItemDef,
        grid_index_at, phantom_alpha, range_metres, region_handle_at,
    };
    use pretty_assertions::assert_eq;

    /// Collect every action string reachable from a menu.
    fn collect_actions(menu: &MenuDef, out: &mut Vec<&'static str>) {
        for item in menu.items {
            match item {
                MenuItemDef::Command(command) => out.push(command.action),
                MenuItemDef::Submenu(submenu) | MenuItemDef::SubmenuWhen(submenu, _) => {
                    collect_actions(submenu, out);
                }
                MenuItemDef::Separator => {}
            }
        }
    }

    /// The actions [`super::handle_minimap_actions`] dispatches explicitly.
    const HANDLED: &[&str] = &[
        "profile",
        "im",
        "add-friend",
        "offer-teleport",
        "block",
        "start-tracking",
        "stop-tracking",
        "mark-clear",
        "mark-clear-all",
        "zoom-very-close",
        "zoom-close",
        "zoom-medium",
        "zoom-far",
        "toggle-objects",
        "toggle-physical",
        "toggle-scripted",
        "toggle-temp",
        "toggle-lines",
        "toggle-sale",
        "toggle-ring",
        "toggle-ring-whisper",
        "toggle-ring-say",
        "toggle-ring-shout",
        "north-at-top",
        "camera-at-top",
        "toggle-auto-center",
        "recenter",
    ];

    #[test]
    fn every_menu_action_has_a_handler() {
        let mut actions = Vec::new();
        collect_actions(&MINIMAP_MENU, &mut actions);
        for action in actions {
            let handled = HANDLED.contains(&action)
                || MARK_COLORS.iter().any(|(name, _color)| *name == action);
            assert!(handled, "menu action {action:?} has no handler arm");
        }
    }

    #[test]
    fn compass_tables_stay_aligned() {
        assert_eq!(COMPASS_POINTS.len(), COMPASS_MINOR.len());
        // The four cardinal points are never minor.
        for (index, (key, _angle)) in COMPASS_POINTS.iter().enumerate() {
            let is_diagonal = key.contains("-north-") || key.contains("-south-");
            assert_eq!(
                COMPASS_MINOR.get(index).copied(),
                Some(is_diagonal),
                "compass minor flag mismatched for {key}"
            );
        }
    }

    #[test]
    fn grid_indexing_matches_regions() {
        assert_eq!(grid_index_at(0.0), Some(0));
        assert_eq!(grid_index_at(255.9), Some(0));
        assert_eq!(grid_index_at(256.0), Some(1));
        assert_eq!(grid_index_at(-1.0), None);
        let handle = region_handle_at(256_000.0, 256_256.0);
        assert_eq!(
            handle.map(|handle| handle.grid_coordinates()),
            Some((1000, 1001))
        );
    }

    #[test]
    fn phantom_alpha_scales_percent() {
        assert_eq!(phantom_alpha(100), 255);
        assert_eq!(phantom_alpha(0), 0);
        assert_eq!(phantom_alpha(50), 127);
        assert_eq!(phantom_alpha(999), 255);
    }

    #[test]
    fn chat_ranges_clamp_to_sane_metres() {
        assert!((range_metres(20) - 20.0).abs() < f32::EPSILON);
        assert!((range_metres(-5) - 0.0).abs() < f32::EPSILON);
        assert!((range_metres(1_000_000) - 4096.0).abs() < f32::EPSILON);
    }
}
