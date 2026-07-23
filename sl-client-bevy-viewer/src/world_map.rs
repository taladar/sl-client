//! The world-map floater: pan and zoom across the whole grid with region tile
//! imagery, per-region info and item markers, and a region-name search.
//!
//! The reference viewer's world map (Firestorm `llfloaterworldmap.cpp` /
//! `llworldmapview.cpp` / `llworldmapmessage.cpp`, read-only reference) has two
//! zoom regimes: a grid-wide regime showing composited map tiles alone, and a
//! region-detail regime (tile level ≤ 3) that additionally requests per-region
//! map blocks (names, access, agent counts) and per-region item markers
//! (avatars, telehubs, land for sale, events). The same thresholds drive this
//! implementation (the pure math lives in [`crate::world_map_math`]).
//!
//! Tile imagery comes from the grid's map-tile service over HTTP through the
//! shared `sl-map-apis` fetch / disk cache on a worker thread
//! ([`crate::world_map_tiles`]); the base URL pairs per grid (login
//! `map-server-url`, then a region's `SimulatorFeatures`, then the Second
//! Life CDN for the main grid). The protocol side — `MapBlockRequest`,
//! `MapNameRequest`, `MapItemRequest` and their replies — is the long-done
//! `protocol-12` surface, reached through [`SlCommand`] / [`SlEvent`].
//!
//! Like the minimap, the surface is **one CPU-composited image** in a single
//! [`ImageNode`], recomposited only when an input changed; region-name labels
//! are a pooled set of `Text` nodes overlaid on the surface (text does not
//! rasterise into the image). Search results are rows in a side panel; picking
//! one recentres the map on that region.
//!
//! Deliberate scope edges, owned by follow-up tasks: double-click teleport and
//! the tracking hand-off (`viewer-world-map-tracking-teleport`) — the shared
//! [`MapTracking`] beacon is *drawn* here when another surface set one, but
//! nothing sets or clears it from this floater yet.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::text::{EditableText, FontCx, LayoutCx};
use bevy::ui::RelativeCursorPosition;
use bevy::ui_widgets::{Activate, Button};
use bevy::window::PrimaryWindow;
use sl_client_bevy::{
    Command, MapItem, MapItemType, MapRegionInfo, Maturity, RegionCoordinates, RegionHandle,
    SlCommand, SlEvent, SlIdentity, SlSessionEvent, Vector,
};
use sl_settings::{Scope, SettingValue};

use crate::avatars::AvatarState;
use crate::floater::{FloaterCaps, FloaterSpec, spawn_floater};
use crate::i18n::{TransArgs, Translated, Translator};
use crate::menu::{MenuCommand, MenuDef, MenuItemDef, OpenContextMenu};
use crate::minimap::{MapTracking, TrackTarget};
use crate::minimap_math::{self, REGION_WIDTH_METRES, Rgba, Surface};
use crate::settings::{AccountContext, ViewerSettings};
use crate::ui::{UiPanelShown, UiRoot, UiScaffoldSystems, column};
use crate::ui_element::{ElementCx, UiAction};
use crate::ui_font::UiFont;
use crate::ui_search::{SearchFieldSpec, spawn_search_field};
use crate::ui_text_input::{TextInputKind, TextInputSpec, spawn_text_input};
use crate::web_floater::set_editor_text;
use crate::world_map_math::{
    self, TileRaster, WorldMapView, tile_corner, tile_level, tile_span_regions,
};
use crate::world_map_tiles::{TileKey, TileState, WorldMapTiles};

/// The `element` tag the world map attributes its [`UiAction`]s to.
pub(crate) const WORLD_MAP_ELEMENT: &str = "worldmap";

/// The settings section every world-map setting registers under.
const WORLD_MAP_SECTION: &[&str] = &["worldmap"];

/// The map scale setting (pixels per 256 m region).
const SETTING_SCALE: &str = "WorldMapScale";

/// Layer toggle: avatar ("people") markers.
const SETTING_PEOPLE: &str = "WorldMapShowPeople";

/// Layer toggle: telehub / infohub markers.
const SETTING_INFOHUBS: &str = "WorldMapShowInfohubs";

/// Layer toggle: land-for-sale markers.
const SETTING_LAND_SALE: &str = "WorldMapShowLandForSale";

/// Layer toggle: PG event markers.
const SETTING_EVENTS: &str = "WorldMapShowEvents";

/// Layer toggle: Moderate event markers.
const SETTING_MATURE_EVENTS: &str = "WorldMapShowMatureEvents";

/// Layer toggle: Adult event markers.
const SETTING_ADULT_EVENTS: &str = "WorldMapShowAdultEvents";

/// Whether region-name labels draw in the detail regime.
const SETTING_REGION_NAMES: &str = "WorldMapShowRegionNames";

/// Register every world-map setting (called from
/// [`crate::settings::ViewerSettings`]'s `FromWorld`).
pub(crate) fn register_settings(settings: &mut ViewerSettings) {
    settings.register_in(
        WORLD_MAP_SECTION,
        SETTING_SCALE,
        SettingValue::F32(world_map_math::WORLD_MAP_SCALE_DEFAULT),
        "World-map zoom, in pixels per 256 m region (1-256)",
    );
    settings.register_in(
        WORLD_MAP_SECTION,
        SETTING_PEOPLE,
        SettingValue::Bool(true),
        "Show avatar markers on the world map",
    );
    settings.register_in(
        WORLD_MAP_SECTION,
        SETTING_INFOHUBS,
        SettingValue::Bool(true),
        "Show telehub / infohub markers on the world map",
    );
    settings.register_in(
        WORLD_MAP_SECTION,
        SETTING_LAND_SALE,
        SettingValue::Bool(true),
        "Show land-for-sale markers on the world map",
    );
    settings.register_in(
        WORLD_MAP_SECTION,
        SETTING_EVENTS,
        SettingValue::Bool(true),
        "Show event markers on the world map",
    );
    settings.register_in(
        WORLD_MAP_SECTION,
        SETTING_MATURE_EVENTS,
        SettingValue::Bool(false),
        "Show Moderate event markers on the world map",
    );
    settings.register_in(
        WORLD_MAP_SECTION,
        SETTING_ADULT_EVENTS,
        SettingValue::Bool(false),
        "Show Adult event markers on the world map",
    );
    settings.register_in(
        WORLD_MAP_SECTION,
        SETTING_REGION_NAMES,
        SettingValue::Bool(true),
        "Show region names on the world map (region-detail zoom)",
    );
}

// ---------------------------------------------------------------------------
// Resources.
// ---------------------------------------------------------------------------

/// The world-map floater's entity handles.
#[derive(Resource)]
pub(crate) struct WorldMapUi {
    /// The floater root (carries [`UiPanelShown`]).
    root: Entity,
    /// The map surface node (the [`ImageNode`], the input target).
    surface: Entity,
    /// The composited surface image.
    image: Handle<Image>,
    /// The hover tooltip panel.
    tooltip: Entity,
    /// The hover tooltip's text node.
    tooltip_text: Entity,
    /// The search field (carries [`EditableText`]).
    search_field: Entity,
    /// The search-result rows' parent column.
    results: Entity,
    /// The selected-location readout line.
    location_text: Entity,
    /// The selected location's region-local X input field.
    field_x: Entity,
    /// The selected location's region-local Y input field.
    field_y: Entity,
    /// The selected location's altitude (Z) input field.
    field_z: Entity,
    /// The pooled region-name label nodes (wrapper, text), grown on demand.
    labels: Vec<(Entity, Entity)>,
}

impl WorldMapUi {
    /// The floater root, for open-state checks and toggling.
    pub(crate) const fn panel(&self) -> Entity {
        self.root
    }
}

/// Open the world-map floater centred on a global position — the hand-off
/// another surface (the minimap's double-click "world map" action) writes.
#[derive(Message, Debug, Clone, Copy)]
pub(crate) struct OpenWorldMap {
    /// Global metres east of the point to centre on.
    pub(crate) east: f64,
    /// Global metres north of the point to centre on.
    pub(crate) north: f64,
}

/// The OS clipboard handle for Copy SLURL, kept alive so the copied selection
/// survives on Linux (dropping the handle can drop the offered selection).
#[derive(Resource, Default)]
struct WorldMapClipboard(std::sync::Mutex<Option<arboard::Clipboard>>);

/// A layer-filter checkbox's fill node: which setting it mirrors.
#[derive(Component)]
struct WorldMapCheckbox {
    /// The `[worldmap]` setting the box shows.
    setting: &'static str,
    /// The setting's declared default.
    default: bool,
}

/// A search-result row: the region it recentres on when clicked.
#[derive(Component)]
struct WorldMapResultRow {
    /// The region's grid x.
    grid_x: u32,
    /// The region's grid y.
    grid_y: u32,
}

/// One drawn marker, kept for hover hit-testing.
#[derive(Debug, Clone)]
struct MarkerInfo {
    /// The marker's surface position, in image pixels.
    view: Vec2,
    /// The item's kind.
    kind: MapItemType,
    /// The item's name.
    name: String,
    /// Type-specific context (agent count / parcel area / event id).
    extra: i32,
    /// Type-specific context (hub kind / sale price / event flags).
    extra2: i32,
}

/// The world-map data mirror: known regions, item layers, and the request
/// bookkeeping that keeps the map from re-asking every frame.
#[derive(Resource, Default)]
pub(crate) struct WorldMapModel {
    /// Known regions, keyed by grid coordinates.
    regions: HashMap<(u32, u32), MapRegionInfo>,
    /// Sent map-block request chunks → seconds timestamp.
    block_requests: HashMap<(u32, u32, u32, u32), f64>,
    /// Sent item requests (region handle, item-type code) → seconds timestamp.
    item_requests: HashMap<(u64, u32), f64>,
    /// Received items, keyed by (region handle, item-type code).
    items: HashMap<(u64, u32), Vec<MapItem>>,
    /// Bumped whenever regions or items change (a composite-stamp input).
    revision: u64,
}

/// The world-map floater's live view / interaction state.
#[derive(Resource)]
struct WorldMapState {
    /// The runtime scale (pixels per region); mirrored to the persisted
    /// setting, debounced by [`scale_save_timer`](Self::scale_save_timer).
    scale: f32,
    /// Whether [`scale`](Self::scale) was seeded from the setting yet.
    scale_loaded: bool,
    /// Seconds until the changed scale is written back, or `None`.
    scale_save_timer: Option<f32>,
    /// The view centre, in global metres (east, north).
    center: (f64, f64),
    /// Whether the map centred itself on the avatar once after login.
    centered: bool,
    /// This frame's world↔surface transform.
    view: WorldMapView,
    /// The composited image size, in pixels.
    surface_px: UVec2,
    /// The cursor position over the surface, in image pixels.
    cursor: Option<Vec2>,
    /// The cursor position over the surface, in logical node pixels.
    cursor_node: Option<Vec2>,
    /// The own avatar's global position, once known.
    agent: Option<(f64, f64)>,
    /// The map-server base URL from `SimulatorFeatures`, when announced.
    features_map_url: Option<String>,
    /// Bumped when tile answers arrive (a composite-stamp input).
    tiles_revision: u64,
    /// The stamp of the last composited frame; recomposite when it changes.
    last_stamp: Option<WorldMapStamp>,
    /// This frame's markers (hover hit-testing).
    markers: Vec<MarkerInfo>,
    /// The live search-field text (trimmed).
    search_query: String,
    /// Seconds until the query is sent, or `None` when nothing is pending.
    search_debounce: Option<f32>,
    /// The last query actually sent to the grid.
    search_sent: Option<String>,
    /// The current result rows (name, grid x, grid y).
    results: Vec<(String, u32, u32)>,
    /// The result rows must be rebuilt.
    results_dirty: bool,
    /// The selected region (grid coordinates), if any.
    selected: Option<(u32, u32)>,
    /// The selection's region-local coordinates, parsed from the X/Y/Z fields.
    selected_local: (u8, u8, u16),
    /// Click-selected local coordinates waiting to be pushed into the fields.
    pending_local: Option<(u8, u8)>,
    /// The cursor position at the last primary press (click-vs-drag test).
    press_cursor: Option<Vec2>,
}

impl Default for WorldMapState {
    fn default() -> Self {
        Self {
            scale: world_map_math::WORLD_MAP_SCALE_DEFAULT,
            scale_loaded: false,
            scale_save_timer: None,
            center: (0.0, 0.0),
            centered: false,
            view: WorldMapView {
                center_east: 0.0,
                center_north: 0.0,
                scale: world_map_math::WORLD_MAP_SCALE_DEFAULT,
                size: Vec2::new(64.0, 64.0),
            },
            surface_px: UVec2::new(64, 64),
            cursor: None,
            cursor_node: None,
            agent: None,
            features_map_url: None,
            tiles_revision: 0,
            last_stamp: None,
            markers: Vec::new(),
            search_query: String::new(),
            search_debounce: None,
            search_sent: None,
            results: Vec::new(),
            results_dirty: false,
            selected: None,
            selected_local: (128, 128, 0),
            pending_local: None,
            press_cursor: None,
        }
    }
}

/// Everything the composited image depends on, quantised.
#[derive(Debug, Clone, PartialEq)]
struct WorldMapStamp {
    /// The centre, in quarter-metre steps.
    center: (i64, i64),
    /// The scale, in 1/16 px steps.
    scale: i32,
    /// The image size.
    size: UVec2,
    /// The data revision (regions / items).
    data_revision: u64,
    /// The tile revision.
    tiles_revision: u64,
    /// The own-avatar marker position, in quarter-metre steps.
    agent: Option<(i64, i64)>,
    /// The tracked location, in quarter-metre steps.
    tracking: Option<(i64, i64)>,
    /// The layer toggles that pick markers.
    toggles: u32,
    /// The selected location marker, in quarter-metre steps.
    selected: Option<(i64, i64)>,
}

// ---------------------------------------------------------------------------
// Plugin and spawn.
// ---------------------------------------------------------------------------

/// The world-map plugin: the floater, the data mirror, the tile service, and
/// the per-frame pipeline.
pub(crate) struct WorldMapPlugin;

impl Plugin for WorldMapPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WorldMapState>()
            .init_resource::<WorldMapModel>()
            .init_resource::<WorldMapTiles>()
            .init_resource::<WorldMapClipboard>()
            .add_message::<OpenWorldMap>()
            .add_systems(Startup, spawn_world_map.after(UiScaffoldSystems::SpawnRoot))
            .add_systems(Update, toggle_world_map_shortcut)
            .add_systems(
                Update,
                (
                    handle_open_world_map,
                    ingest_world_map_events,
                    drive_world_map_view,
                    drive_world_map_location,
                    request_world_map_data,
                    composite_world_map,
                    layout_world_map_labels,
                    update_world_map_hover,
                    drive_world_map_search,
                    refresh_world_map_checkboxes,
                    refresh_world_map_result_selection,
                    handle_world_map_actions,
                )
                    .chain(),
            );
    }
}

/// The floater's default content size, in logical pixels.
const DEFAULT_SIZE: Vec2 = Vec2::new(620.0, 440.0);

/// The smallest content size the resize grip allows.
const MIN_SIZE: Vec2 = Vec2::new(360.0, 260.0);

/// The side panel (search) width, in logical pixels.
const SIDE_WIDTH: f32 = 190.0;

/// The tooltip / results font size.
const PANEL_FONT_SIZE: f32 = 12.0;

/// The region-name label font size.
const LABEL_FONT_SIZE: f32 = 11.0;

/// The largest composited image side, in pixels (matches the minimap's cap; a
/// larger widget upscales).
const MAX_SURFACE_PX: u32 = 512;

/// The most search-result rows shown.
const MAX_RESULTS: usize = 30;

/// The most region-name labels the pool grows to.
const MAX_LABELS: usize = 48;

/// Startup: build the world-map floater — the surface image node with its
/// input observers and tooltip, and the search side panel.
fn spawn_world_map(
    mut commands: Commands,
    root: Res<UiRoot>,
    mut images: ResMut<Assets<Image>>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    let position = windows.single().map_or(Vec2::new(120.0, 80.0), |window| {
        Vec2::new(
            ((window.width() - DEFAULT_SIZE.x) / 2.0).max(40.0),
            ((window.height() - DEFAULT_SIZE.y) / 2.0 - 40.0).max(40.0),
        )
    });
    let handle = spawn_floater(
        &mut commands,
        root.0,
        FloaterSpec {
            id: "worldmap",
            title: String::from("World Map"),
            position,
            default_size: Some(DEFAULT_SIZE),
            min_size: Some(MIN_SIZE),
            dock_host: None,
            caps: FloaterCaps {
                resizable: true,
                minimizable: true,
                closable: true,
                dockable: false,
            },
        },
    );
    commands
        .entity(handle.title_text)
        .insert(Translated::new("worldmap-floater-title"));

    // The content row: the map surface (grows) and the search side panel.
    let content_row = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                column_gap: Val::Px(6.0),
                ..default()
            },
            Name::new("worldmap-content"),
            ChildOf(handle.content),
        ))
        .id();

    let image = images.add(blank_surface(64, 64));
    let surface = commands
        .spawn((
            Node {
                flex_grow: 1.0,
                min_width: Val::Px(64.0),
                min_height: Val::Px(64.0),
                ..default()
            },
            ImageNode::new(image.clone()),
            RelativeCursorPosition::default(),
            Pickable {
                should_block_lower: true,
                is_hoverable: true,
            },
            Name::new("worldmap-surface"),
            ChildOf(content_row),
        ))
        .observe(on_world_map_press)
        .observe(on_world_map_click)
        .observe(on_world_map_drag)
        .observe(on_world_map_scroll)
        .observe(on_world_map_context)
        .id();

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
            Name::new("worldmap-tooltip"),
            ChildOf(surface),
        ))
        .id();
    let tooltip_text = commands
        .spawn((
            Text::default(),
            UiFont::Sans.at(PANEL_FONT_SIZE),
            TextColor(Color::WHITE),
            Pickable::IGNORE,
            ChildOf(tooltip),
        ))
        .id();

    // The side panel: the search field and the result rows.
    let side = commands
        .spawn((
            Node {
                width: Val::Px(SIDE_WIDTH),
                flex_shrink: 0.0,
                ..column(Val::Px(6.0))
            },
            Name::new("worldmap-side"),
            ChildOf(content_row),
        ))
        .id();
    let search = spawn_search_field(
        &mut commands,
        side,
        &SearchFieldSpec {
            font_size: PANEL_FONT_SIZE,
            min_width: SIDE_WIDTH - 12.0,
            placeholder: String::from("Search regions"),
            ..SearchFieldSpec::new(WORLD_MAP_ELEMENT)
        },
    );
    let results = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                overflow: Overflow::scroll_y(),
                ..column(Val::Px(2.0))
            },
            ScrollPosition::default(),
            Name::new("worldmap-results"),
            ChildOf(side),
        ))
        .observe(on_results_scroll)
        .id();

    // The selected-location block: readout, X/Y/Z fields, action buttons.
    let location_text = commands
        .spawn((
            Text::default(),
            UiFont::Sans.at(PANEL_FONT_SIZE),
            TextColor(Color::srgba(0.85, 0.85, 0.85, 1.0)),
            Name::new("worldmap-location"),
            ChildOf(side),
        ))
        .id();
    let coords_row = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                column_gap: Val::Px(4.0),
                align_items: AlignItems::Center,
                ..default()
            },
            Name::new("worldmap-coords"),
            ChildOf(side),
        ))
        .id();
    let mut coord_fields: Vec<Entity> = Vec::new();
    for (label, initial) in [("X", "128"), ("Y", "128"), ("Z", "0")] {
        commands.spawn((
            Text::new(label),
            UiFont::Sans.at(PANEL_FONT_SIZE),
            TextColor(Color::srgba(0.7, 0.7, 0.7, 1.0)),
            Pickable::IGNORE,
            ChildOf(coords_row),
        ));
        let field = spawn_text_input(
            &mut commands,
            coords_row,
            &TextInputSpec {
                font_size: PANEL_FONT_SIZE,
                width_glyphs: 4.0,
                initial: initial.to_owned(),
                max_characters: Some(4),
                ..TextInputSpec::new(WORLD_MAP_ELEMENT, TextInputKind::NonNegativeInteger)
            },
        );
        coord_fields.push(field);
    }
    let (field_x, field_y, field_z) = match coord_fields.as_slice() {
        [x, y, z] => (*x, *y, *z),
        _other => (
            Entity::PLACEHOLDER,
            Entity::PLACEHOLDER,
            Entity::PLACEHOLDER,
        ),
    };
    let buttons_row = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                column_gap: Val::Px(4.0),
                ..default()
            },
            Name::new("worldmap-buttons"),
            ChildOf(side),
        ))
        .id();
    spawn_panel_button(
        &mut commands,
        buttons_row,
        "worldmap-button-teleport",
        "teleport-selected",
    );
    spawn_panel_button(
        &mut commands,
        buttons_row,
        "worldmap-button-copy-slurl",
        "copy-slurl",
    );

    // The layer-filter checkboxes (the reference's info-display toggles).
    let filters = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                ..column(Val::Px(2.0))
            },
            Name::new("worldmap-filters"),
            ChildOf(side),
        ))
        .id();
    for (label_key, action, setting, default) in [
        (
            "worldmap-layer-people",
            "toggle-people",
            SETTING_PEOPLE,
            true,
        ),
        (
            "worldmap-layer-infohubs",
            "toggle-infohubs",
            SETTING_INFOHUBS,
            true,
        ),
        (
            "worldmap-layer-land-sale",
            "toggle-land-sale",
            SETTING_LAND_SALE,
            true,
        ),
        (
            "worldmap-layer-events",
            "toggle-events",
            SETTING_EVENTS,
            true,
        ),
        (
            "worldmap-layer-mature-events",
            "toggle-mature-events",
            SETTING_MATURE_EVENTS,
            false,
        ),
        (
            "worldmap-layer-adult-events",
            "toggle-adult-events",
            SETTING_ADULT_EVENTS,
            false,
        ),
        (
            "worldmap-layer-region-names",
            "toggle-region-names",
            SETTING_REGION_NAMES,
            true,
        ),
    ] {
        spawn_layer_toggle(&mut commands, filters, label_key, action, setting, default);
    }

    commands.insert_resource(WorldMapUi {
        root: handle.root,
        surface,
        image,
        tooltip,
        tooltip_text,
        search_field: search.field,
        results,
        location_text,
        field_x,
        field_y,
        field_z,
        labels: Vec::new(),
    });
}

/// One labelled side-panel button emitting a [`UiAction`].
fn spawn_panel_button(
    commands: &mut Commands,
    parent: Entity,
    label_key: &'static str,
    action: &'static str,
) {
    let button = commands
        .spawn((
            Button,
            TabIndex(0),
            Node {
                padding: UiRect::axes(Val::Px(7.0), Val::Px(3.0)),
                border: UiRect::all(Val::Px(1.0)),
                justify_content: JustifyContent::Center,
                flex_grow: 1.0,
                ..default()
            },
            BorderColor::all(Color::srgb(0.35, 0.35, 0.4)),
            BackgroundColor(Color::srgb(0.16, 0.17, 0.2)),
            Pickable::default(),
            Name::new(format!("worldmap-button:{action}")),
            ChildOf(parent),
        ))
        .observe(
            move |_activate: On<Activate>, mut actions: MessageWriter<UiAction>| {
                actions.write(UiAction {
                    element: WORLD_MAP_ELEMENT,
                    action,
                });
            },
        )
        .id();
    commands.spawn((
        Text::default(),
        Translated::new(label_key),
        UiFont::Sans.at(PANEL_FONT_SIZE),
        TextColor(Color::srgba(0.9, 0.92, 0.96, 1.0)),
        Pickable::IGNORE,
        ChildOf(button),
    ));
}

/// One layer-filter checkbox row: a mirrored check square plus a label,
/// toggling its setting through the shared [`UiAction`] dispatch.
fn spawn_layer_toggle(
    commands: &mut Commands,
    parent: Entity,
    label_key: &'static str,
    action: &'static str,
    setting: &'static str,
    default_on: bool,
) {
    let row = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                column_gap: Val::Px(6.0),
                align_items: AlignItems::Center,
                padding: UiRect::axes(Val::Px(2.0), Val::Px(1.0)),
                ..default()
            },
            Pickable {
                should_block_lower: true,
                is_hoverable: true,
            },
            Name::new(format!("worldmap-filter:{action}")),
            ChildOf(parent),
        ))
        .observe(
            move |click: On<Pointer<Click>>, mut actions: MessageWriter<UiAction>| {
                if click.button == PointerButton::Primary {
                    actions.write(UiAction {
                        element: WORLD_MAP_ELEMENT,
                        action,
                    });
                }
            },
        )
        .id();
    let box_outer = commands
        .spawn((
            Node {
                width: Val::Px(12.0),
                height: Val::Px(12.0),
                border: UiRect::all(Val::Px(1.0)),
                padding: UiRect::all(Val::Px(2.0)),
                flex_shrink: 0.0,
                ..default()
            },
            BorderColor::all(Color::srgb(0.5, 0.5, 0.55)),
            Pickable::IGNORE,
            ChildOf(row),
        ))
        .id();
    commands.spawn((
        Node {
            flex_grow: 1.0,
            ..default()
        },
        BackgroundColor(Color::NONE),
        WorldMapCheckbox {
            setting,
            default: default_on,
        },
        Pickable::IGNORE,
        ChildOf(box_outer),
    ));
    commands.spawn((
        Text::default(),
        Translated::new(label_key),
        UiFont::Sans.at(PANEL_FONT_SIZE),
        TextColor(Color::srgba(0.85, 0.85, 0.85, 1.0)),
        Pickable::IGNORE,
        ChildOf(row),
    ));
}

/// Paint each layer checkbox's fill from its setting.
fn refresh_world_map_checkboxes(
    settings: Res<ViewerSettings>,
    mut boxes: Query<(&WorldMapCheckbox, &mut BackgroundColor)>,
) {
    let store = settings.store();
    for (checkbox, mut background) in &mut boxes {
        let checked = store.get_bool(checkbox.setting).unwrap_or(checkbox.default);
        let wanted = if checked {
            Color::srgb(0.55, 0.75, 1.0)
        } else {
            Color::NONE
        };
        if background.0 != wanted {
            background.0 = wanted;
        }
    }
}

/// Open the floater centred on a requested global position (the minimap's
/// double-click "world map" hand-off).
fn handle_open_world_map(
    mut requests: MessageReader<OpenWorldMap>,
    ui: Option<Res<WorldMapUi>>,
    mut state: ResMut<WorldMapState>,
    mut panels: Query<&mut UiPanelShown>,
) {
    let Some(ui) = ui else {
        return;
    };
    for request in requests.read() {
        state.center = (request.east, request.north);
        state.centered = true;
        if let Ok(mut shown) = panels.get_mut(ui.root)
            && !shown.0
        {
            shown.0 = true;
        }
    }
}

/// `Ctrl+M` opens / closes the world map, matching the reference viewer's
/// shortcut. The `Ctrl` modifier keeps it from firing while a bare `m` is
/// typed (bare `M` is the mouselook toggle).
fn toggle_world_map_shortcut(
    keyboard: Res<ButtonInput<KeyCode>>,
    ui: Option<Res<WorldMapUi>>,
    mut panels: Query<&mut UiPanelShown>,
) {
    let ctrl = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    if !(ctrl && keyboard.just_pressed(KeyCode::KeyM)) {
        return;
    }
    let Some(ui) = ui else {
        return;
    };
    if let Ok(mut shown) = panels.get_mut(ui.root) {
        shown.0 = !shown.0;
    }
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

/// Fold map replies into the model (always running, so replies landing while
/// the floater is closed are not lost) and note the grid's tile base URL from
/// `SimulatorFeatures`.
fn ingest_world_map_events(
    mut events: MessageReader<SlEvent>,
    mut model: ResMut<WorldMapModel>,
    mut state: ResMut<WorldMapState>,
) {
    for event in events.read() {
        match &event.0 {
            SlSessionEvent::MapBlock(info) => {
                let key = (info.grid_coordinates.x(), info.grid_coordinates.y());
                let changed = model.regions.get(&key) != Some(info.as_ref());
                if changed {
                    model.regions.insert(key, info.as_ref().clone());
                    model.revision = model.revision.saturating_add(1);
                    state.results_dirty = true;
                }
            }
            SlSessionEvent::MapItems { item_type, items } => {
                // Group the reply by the region each item actually sits in and
                // replace those regions' layers wholesale.
                let mut grouped: HashMap<(u64, u32), Vec<MapItem>> = HashMap::new();
                for item in items {
                    let Some(handle) = item.region_handle() else {
                        continue;
                    };
                    grouped
                        .entry((handle.0, item_type.to_u32()))
                        .or_default()
                        .push(item.clone());
                }
                for (key, group) in grouped {
                    model.items.insert(key, group);
                }
                model.revision = model.revision.saturating_add(1);
            }
            SlSessionEvent::SimulatorFeatures(features) => {
                if let Some(url) = features
                    .open_sim_extras
                    .as_ref()
                    .and_then(|extras| extras.map_server_url.as_ref())
                {
                    state.features_map_url = Some(url.as_str().to_owned());
                }
            }
            _other => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Per-frame view state.
// ---------------------------------------------------------------------------

/// Scale a [`Vec2`] by a scalar without the glam `*` operator (the workspace
/// `arithmetic_side_effects` lint trips on operator arithmetic of
/// non-primitive types).
const fn vec2_scale(v: Vec2, s: f32) -> Vec2 {
    Vec2::new(v.x * s, v.y * s)
}

/// Update the per-frame view state: seed the scale from its setting, size the
/// surface image to the node, read the cursor and the own avatar's position,
/// and centre the map once after login.
#[expect(
    clippy::too_many_arguments,
    reason = "the view state genuinely reads the settings, clock, identity, avatar anchors and \
              the surface node, and resizes the image — one per-frame pass"
)]
fn drive_world_map_view(
    ui: Option<Res<WorldMapUi>>,
    mut state: ResMut<WorldMapState>,
    mut settings: ResMut<ViewerSettings>,
    time: Res<Time>,
    identity: Res<SlIdentity>,
    avatars: Res<AvatarState>,
    terrain: Res<crate::terrain::TerrainState>,
    transforms: Query<&GlobalTransform>,
    computed: Query<&ComputedNode>,
    cursors: Query<&RelativeCursorPosition>,
    panels: Query<&UiPanelShown>,
    mut images: ResMut<Assets<Image>>,
) {
    let Some(ui) = ui else {
        return;
    };
    if !state.scale_loaded {
        if let Ok(scale) = settings.store().get_f32(SETTING_SCALE) {
            state.scale = world_map_math::clamp_world_scale(scale);
        }
        state.scale_loaded = true;
    }
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

    // The own avatar's global position (marker + initial centring).
    let origin = origin_global(terrain.origin().or(identity.region_handle));
    state.agent = identity
        .agent_id
        .and_then(|agent| avatars.root_entity_of(agent))
        .and_then(|entity| transforms.get(entity).ok())
        .map(|transform| {
            let sl = crate::coords::bevy_to_sl_vec(transform.translation());
            (origin.0 + f64::from(sl.x), origin.1 + f64::from(sl.y))
        });

    if !panels.get(ui.root).is_ok_and(|shown| shown.0) {
        return;
    }

    // Centre on the avatar (or the login region) once.
    if !state.centered {
        if let Some((east, north)) = state.agent {
            state.center = (east, north);
            state.centered = true;
        } else if let Some(handle) = identity.region_handle {
            let (east, north) = handle.global_coordinates();
            state.center = (
                f64::from(east) + f64::from(REGION_WIDTH_METRES) / 2.0,
                f64::from(north) + f64::from(REGION_WIDTH_METRES) / 2.0,
            );
            state.centered = true;
        }
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
                warn!("world map: could not resize the surface image");
            }
            state.last_stamp = None;
        }
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

    state.view = WorldMapView {
        center_east: state.center.0,
        center_north: state.center.1,
        scale: state.scale,
        size: Vec2::new(
            minimap_math::u32_to_f32(state.surface_px.x),
            minimap_math::u32_to_f32(state.surface_px.y),
        ),
    };
}

/// The scene origin's global coordinates in metres, as `f64`.
fn origin_global(origin: Option<RegionHandle>) -> (f64, f64) {
    let Some(origin) = origin else {
        return (0.0, 0.0);
    };
    let (east, north) = origin.global_coordinates();
    (f64::from(east), f64::from(north))
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

/// Keep the selected-location block in step: push a click-selected position
/// into the X/Y fields, parse the fields back into the selection state, and
/// refresh the readout line (region name once known, else grid coordinates).
#[expect(
    clippy::too_many_arguments,
    reason = "the location block reads the fields and model and writes the fields, the readout \
              text and the selection state — one cohesive pass"
)]
fn drive_world_map_location(
    ui: Option<Res<WorldMapUi>>,
    mut state: ResMut<WorldMapState>,
    model: Res<WorldMapModel>,
    translator: Translator,
    mut fields: Query<&mut EditableText>,
    mut texts: Query<&mut Text>,
    panels: Query<&UiPanelShown>,
    mut font_cx: ResMut<FontCx>,
    mut layout_cx: ResMut<LayoutCx>,
) {
    let Some(ui) = ui else {
        return;
    };
    if !panels.get(ui.root).is_ok_and(|shown| shown.0) {
        return;
    }
    if let Some((x, y)) = state.pending_local.take() {
        for (entity, value) in [(ui.field_x, x), (ui.field_y, y)] {
            if let Ok(mut editable) = fields.get_mut(entity) {
                set_editor_text(
                    &mut editable,
                    &value.to_string(),
                    &mut font_cx,
                    &mut layout_cx,
                );
            }
        }
    }
    let parse = |entity: Entity, fields: &Query<&mut EditableText>, max: u32| -> Option<u32> {
        let field = fields.get(entity).ok()?;
        field
            .value()
            .to_string()
            .trim()
            .parse::<u32>()
            .ok()
            .map(|value| value.min(max))
    };
    let (old_x, old_y, old_z) = state.selected_local;
    let x = parse(ui.field_x, &fields, 255)
        .map_or(old_x, |value| u8::try_from(value).unwrap_or(u8::MAX));
    let y = parse(ui.field_y, &fields, 255)
        .map_or(old_y, |value| u8::try_from(value).unwrap_or(u8::MAX));
    let z = parse(ui.field_z, &fields, 4095)
        .map_or(old_z, |value| u16::try_from(value).unwrap_or(u16::MAX));
    state.selected_local = (x, y, z);
    // The readout: region name once its map block arrived, else coordinates.
    let line = state.selected.map_or_else(
        || translator.get("worldmap-location-none"),
        |(grid_x, grid_y)| {
            let name = model
                .regions
                .get(&(grid_x, grid_y))
                .and_then(|info| info.name.as_ref())
                .map_or_else(|| format!("({grid_x}, {grid_y})"), ToString::to_string);
            format!("{name} ({x}, {y}, {z})")
        },
    );
    if let Ok(mut text) = texts.get_mut(ui.location_text)
        && text.0 != line
    {
        text.0 = line;
    }
}

// ---------------------------------------------------------------------------
// Data requests (tiles, blocks, items).
// ---------------------------------------------------------------------------

/// How long a sent map-block chunk stays fresh before a visible chunk is
/// re-requested (agent counts drift), in seconds.
const BLOCK_REFRESH_SECONDS: f64 = 120.0;

/// How long a region's item layer stays fresh, in seconds.
const ITEM_REFRESH_SECONDS: f64 = 30.0;

/// The map-block request chunk edge, in regions.
const BLOCK_CHUNK: u32 = 16;

/// The most map-block chunks sent per frame.
const MAX_BLOCK_SENDS: usize = 4;

/// The most item requests sent per frame.
const MAX_ITEM_SENDS: usize = 12;

/// The most tiles requested per frame.
const MAX_TILE_REQUESTS: usize = 64;

/// The Second Life main-grid credential keys whose tile base URL falls back
/// to the public CDN when the grid announced none.
const AGNI_GRID_KEYS: [&str; 2] = ["agni", "secondlife"];

/// The effective tile base URL for this grid: a region's `SimulatorFeatures`
/// `map-server-url` wins, then the login response's, then the Second Life CDN
/// on the main grid; `None` disables tile fetching.
fn effective_base_url(
    features_url: Option<&str>,
    login_url: Option<&str>,
    grid: &str,
) -> Option<String> {
    if let Some(url) = features_url {
        return Some(url.to_owned());
    }
    if let Some(url) = login_url {
        return Some(url.to_owned());
    }
    let grid_lower = grid.to_lowercase();
    AGNI_GRID_KEYS
        .iter()
        .any(|key| grid_lower.contains(key))
        .then(|| sl_map_apis::map_tiles::DEFAULT_MAP_TILE_BASE_URL.to_owned())
}

/// Keep the map fed while it is open: run the tile service and request the
/// visible tiles, and — in the detail regime — the visible map blocks and the
/// enabled item layers.
#[expect(
    clippy::too_many_arguments,
    reason = "the request pass reads the view state, the model bookkeeping, the settings, the \
              account context and the clock, and writes commands and tile requests — one pass"
)]
fn request_world_map_data(
    ui: Option<Res<WorldMapUi>>,
    mut state: ResMut<WorldMapState>,
    mut model: ResMut<WorldMapModel>,
    mut tiles: ResMut<WorldMapTiles>,
    settings: Res<ViewerSettings>,
    identity: Res<SlIdentity>,
    context: Option<Res<AccountContext>>,
    time: Res<Time>,
    panels: Query<&UiPanelShown>,
    mut commands: MessageWriter<SlCommand>,
) {
    let Some(ui) = ui else {
        return;
    };
    // Tile answers keep arriving even while hidden (cheap; keeps reopen warm).
    if tiles.drain() {
        state.tiles_revision = state.tiles_revision.saturating_add(1);
    }
    if !panels.get(ui.root).is_ok_and(|shown| shown.0) {
        return;
    }

    // The tile service pairs with the grid's announced base URL.
    if let Some(context) = &context {
        let login_url = identity
            .map_server_url
            .as_ref()
            .map(|url| url.as_str().to_owned());
        if let Some(base_url) = effective_base_url(
            state.features_map_url.as_deref(),
            login_url.as_deref(),
            &context.grid,
        ) {
            let cache_dir = crate::paths::asset_cache_dir("maptiles").map_or_else(
                || std::env::temp_dir().join("sl-client-maptiles"),
                |dir| dir.join(&context.grid),
            );
            tiles.ensure_service(&base_url, cache_dir);
        }
    }

    let (min_x, max_x, min_y, max_y) = state.view.visible_grid_rect();
    let level = tile_level(state.scale);

    // Visible tiles at the current level.
    if tiles.running() {
        for (x, y) in
            world_map_math::tiles_in_rect(level, min_x, max_x, min_y, max_y, MAX_TILE_REQUESTS)
        {
            tiles.request(TileKey { level, x, y });
        }
    }

    if !world_map_math::detail_regime(state.scale) {
        return;
    }
    let now = time.elapsed_secs_f64();

    // Visible map blocks, chunked.
    let mut block_sends = 0_usize;
    for chunk in world_map_math::request_chunks(min_x, max_x, min_y, max_y, BLOCK_CHUNK, 64) {
        if block_sends >= MAX_BLOCK_SENDS {
            break;
        }
        let fresh = model
            .block_requests
            .get(&chunk)
            .is_some_and(|sent| now - sent < BLOCK_REFRESH_SECONDS);
        if fresh {
            continue;
        }
        model.block_requests.insert(chunk, now);
        commands.write(SlCommand(Command::RequestMapBlocks {
            min_x: chunk.0,
            max_x: chunk.1,
            min_y: chunk.2,
            max_y: chunk.3,
        }));
        block_sends = block_sends.saturating_add(1);
    }

    // Item layers for the visible, known regions.
    let store = settings.store();
    let mut wanted: Vec<MapItemType> = Vec::new();
    if store.get_bool(SETTING_PEOPLE).unwrap_or(true) {
        wanted.push(MapItemType::AgentLocations);
    }
    if store.get_bool(SETTING_INFOHUBS).unwrap_or(true) {
        wanted.push(MapItemType::Telehub);
    }
    if store.get_bool(SETTING_LAND_SALE).unwrap_or(true) {
        wanted.push(MapItemType::LandForSale);
        wanted.push(MapItemType::AdultLandForSale);
    }
    if store.get_bool(SETTING_EVENTS).unwrap_or(true) {
        wanted.push(MapItemType::PgEvent);
    }
    if store.get_bool(SETTING_MATURE_EVENTS).unwrap_or(false) {
        wanted.push(MapItemType::MatureEvent);
    }
    if store.get_bool(SETTING_ADULT_EVENTS).unwrap_or(false) {
        wanted.push(MapItemType::AdultEvent);
    }
    let mut item_sends = 0_usize;
    let visible_regions: Vec<RegionHandle> = model
        .regions
        .values()
        .filter(|info| {
            let (x, y) = (info.grid_coordinates.x(), info.grid_coordinates.y());
            x >= min_x && x <= max_x && y >= min_y && y <= max_y
        })
        .map(|info| info.region_handle)
        .collect();
    'regions: for handle in visible_regions {
        for item_type in &wanted {
            if item_sends >= MAX_ITEM_SENDS {
                break 'regions;
            }
            let key = (handle.0, item_type.to_u32());
            let fresh = model
                .item_requests
                .get(&key)
                .is_some_and(|sent| now - sent < ITEM_REFRESH_SECONDS);
            if fresh {
                continue;
            }
            model.item_requests.insert(key, now);
            commands.write(SlCommand(Command::RequestMapItems {
                item_type: *item_type,
                region_handle: handle,
            }));
            item_sends = item_sends.saturating_add(1);
        }
    }
}

// ---------------------------------------------------------------------------
// Compositing.
// ---------------------------------------------------------------------------

/// Composite the surface image when any input changed: the tile backdrop (with
/// coarser-level fallback while the right tiles load), region-border grid
/// lines in the detail regime, the enabled marker layers, the shared tracking
/// beacon, and the own-avatar marker.
#[expect(
    clippy::too_many_arguments,
    reason = "the composite folds every map data source into one image; a staging resource \
              would only rename the parameters"
)]
fn composite_world_map(
    ui: Option<Res<WorldMapUi>>,
    mut state: ResMut<WorldMapState>,
    mut tiles: ResMut<WorldMapTiles>,
    model: Res<WorldMapModel>,
    settings: Res<ViewerSettings>,
    tracking: Res<MapTracking>,
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
    let toggles = layer_toggle_bits(store);

    // Gather this frame's markers (they are hit-test data and stamp input).
    let markers = gather_markers(&state.view, &model, store);

    let tracked = tracking.target.and_then(|target| match target {
        TrackTarget::Location { east, north, .. } => Some((east, north)),
        TrackTarget::Avatar(_) => None,
    });
    let stamp = WorldMapStamp {
        center: (quantise(state.center.0), quantise(state.center.1)),
        scale: minimap_math::round_i32(state.scale * 16.0),
        size: state.surface_px,
        data_revision: model.revision,
        tiles_revision: state.tiles_revision,
        agent: state
            .agent
            .map(|(east, north)| (quantise(east), quantise(north))),
        tracking: tracked.map(|(east, north)| (quantise(east), quantise(north))),
        toggles,
        selected: selection_global(&state).map(|(east, north)| (quantise(east), quantise(north))),
    };
    if state.last_stamp.as_ref() == Some(&stamp) {
        state.markers = markers;
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
    fill(&mut data, world_map_math::COLOR_MAP_VOID);

    draw_tiles(&mut data, &state.view, state.surface_px, tiles.as_mut());

    let mut surface = Surface {
        width,
        height,
        data: &mut data,
    };

    // Region-border grid lines in the detail regime.
    if world_map_math::detail_regime(state.scale) {
        draw_region_grid(&mut surface, &state.view);
    }

    // Markers.
    for marker in &markers {
        draw_marker(&mut surface, marker);
    }

    // The shared tracking beacon (set by the minimap today).
    if let Some((east, north)) = tracked {
        let position = state.view.view_from_global(east, north);
        minimap_math::draw_tracking(&mut surface, position, minimap_math::COLOR_TRACK);
    }

    // The selected-location marker (red ring + dot, the reference's track
    // circle look).
    if let Some((east, north)) = selection_global(&state) {
        let position = state.view.view_from_global(east, north);
        minimap_math::draw_ring(
            &mut surface,
            position.x,
            position.y,
            6.0,
            1.8,
            minimap_math::COLOR_TRACK,
        );
        minimap_math::draw_disc(
            &mut surface,
            position.x,
            position.y,
            2.0,
            minimap_math::COLOR_TRACK,
        );
    }

    // The own-avatar marker.
    if let Some((east, north)) = state.agent {
        let position = state.view.view_from_global(east, north);
        minimap_math::draw_ring(
            &mut surface,
            position.x,
            position.y,
            5.0,
            1.5,
            [255, 255, 255, 230],
        );
        minimap_math::draw_disc(
            &mut surface,
            position.x,
            position.y,
            3.5,
            world_map_math::COLOR_MAP_SELF,
        );
    }

    if let Some(mut image) = images.get_mut(ui.image.id()) {
        image.data = Some(data);
    }
    state.markers = markers;
    state.last_stamp = Some(stamp);
}

/// Quantise a global metre coordinate to quarter-metre steps for the stamp.
fn quantise(value: f64) -> i64 {
    let scaled = (value * 4.0).clamp(-9.0e15, 9.0e15);
    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        reason = "clamped to well inside i64 just above"
    )]
    let out = scaled as i64;
    out
}

/// The selection's global position (metres east, north), if a location is
/// selected.
fn selection_global(state: &WorldMapState) -> Option<(f64, f64)> {
    let (grid_x, grid_y) = state.selected?;
    let (x, y, _z) = state.selected_local;
    let region_width = f64::from(REGION_WIDTH_METRES);
    Some((
        f64::from(grid_x) * region_width + f64::from(x),
        f64::from(grid_y) * region_width + f64::from(y),
    ))
}

/// The enabled layer toggles as a stamp bitmask.
fn layer_toggle_bits(store: &sl_settings::SettingsStore) -> u32 {
    let mut bits = 0_u32;
    for (index, (setting, default)) in [
        (SETTING_PEOPLE, true),
        (SETTING_INFOHUBS, true),
        (SETTING_LAND_SALE, true),
        (SETTING_EVENTS, true),
        (SETTING_MATURE_EVENTS, false),
        (SETTING_ADULT_EVENTS, false),
        (SETTING_REGION_NAMES, true),
    ]
    .iter()
    .enumerate()
    {
        if store.get_bool(setting).unwrap_or(*default) {
            bits |= 1_u32 << u32::try_from(index).unwrap_or(0);
        }
    }
    bits
}

/// Fill an RGBA byte buffer with one colour.
fn fill(data: &mut [u8], color: Rgba) {
    for texel in data.chunks_exact_mut(4) {
        texel.copy_from_slice(&color);
    }
}

/// Draw the tile backdrop: for every visible tile at the current level, blit
/// the best available raster — the tile itself, or the nearest coarser-level
/// tile already resident while it loads.
fn draw_tiles(data: &mut [u8], view: &WorldMapView, surface_px: UVec2, tiles: &mut WorldMapTiles) {
    let level = tile_level(view.scale);
    let (min_x, max_x, min_y, max_y) = view.visible_grid_rect();
    let span = tile_span_regions(level);
    let region_width = f64::from(REGION_WIDTH_METRES);
    for (corner_x, corner_y) in
        world_map_math::tiles_in_rect(level, min_x, max_x, min_y, max_y, 256)
    {
        // The best resident source for this tile: itself, else coarser levels.
        let mut source: Option<(u8, u32, u32, std::sync::Arc<TileRaster>)> = None;
        let mut probe_level = level;
        while probe_level <= world_map_math::MAX_TILE_LEVEL {
            let (px, py) = tile_corner(probe_level, corner_x, corner_y);
            if let Some(TileState::Ready(raster)) = tiles.state(TileKey {
                level: probe_level,
                x: px,
                y: py,
            }) {
                source = Some((probe_level, px, py, raster));
                break;
            }
            probe_level = probe_level.saturating_add(1);
        }
        let Some((src_level, src_x, src_y, raster)) = source else {
            continue;
        };
        // The tile's view rectangle (top edge = its north edge).
        let tile_min_east = f64::from(corner_x) * region_width;
        let tile_max_north = f64::from(corner_y.saturating_add(span)) * region_width;
        let top_left = view.view_from_global(tile_min_east, tile_max_north);
        let extent = f64::from(span) * region_width * f64::from(view.pixels_per_metre());
        let x0 = minimap_math::round_i32(top_left.x).max(0);
        let y0 = minimap_math::round_i32(top_left.y).max(0);
        let x1 = minimap_math::round_i32(top_left.x + world_map_math::narrow_f64(extent))
            .min(i32::try_from(surface_px.x).unwrap_or(i32::MAX));
        let y1 = minimap_math::round_i32(top_left.y + world_map_math::narrow_f64(extent))
            .min(i32::try_from(surface_px.y).unwrap_or(i32::MAX));
        let mut y = y0;
        while y < y1 {
            let mut x = x0;
            while x < x1 {
                let pixel = Vec2::new(
                    minimap_math::i32_to_f32(x) + 0.5,
                    minimap_math::i32_to_f32(y) + 0.5,
                );
                let (east, north) = view.global_from_view(pixel);
                let texel = raster.sample(src_level, src_x, src_y, east, north);
                if texel[3] > 0 {
                    let offset = usize::try_from(y)
                        .unwrap_or(0)
                        .saturating_mul(usize::try_from(surface_px.x).unwrap_or(0))
                        .saturating_add(usize::try_from(x).unwrap_or(0))
                        .saturating_mul(4);
                    if let Some(slot) = data.get_mut(offset..offset.saturating_add(4)) {
                        slot.copy_from_slice(&[texel[0], texel[1], texel[2], 255]);
                    }
                }
                x = x.saturating_add(1);
            }
            y = y.saturating_add(1);
        }
    }
}

/// Draw the region-border grid lines across the surface.
fn draw_region_grid(surface: &mut Surface<'_>, view: &WorldMapView) {
    let (min_x, max_x, min_y, max_y) = view.visible_grid_rect();
    let region_width = f64::from(REGION_WIDTH_METRES);
    let mut grid_x = min_x;
    while grid_x <= max_x.saturating_add(1) {
        let x = view
            .view_from_global(f64::from(grid_x) * region_width, view.center_north)
            .x;
        world_map_math::draw_vline(surface, x, world_map_math::COLOR_MAP_GRID_LINE);
        let Some(next) = grid_x.checked_add(1) else {
            break;
        };
        grid_x = next;
    }
    let mut grid_y = min_y;
    while grid_y <= max_y.saturating_add(1) {
        let y = view
            .view_from_global(view.center_east, f64::from(grid_y) * region_width)
            .y;
        world_map_math::draw_hline(surface, y, world_map_math::COLOR_MAP_GRID_LINE);
        let Some(next) = grid_y.checked_add(1) else {
            break;
        };
        grid_y = next;
    }
}

/// Gather the enabled, on-surface markers from the item layers.
fn gather_markers(
    view: &WorldMapView,
    model: &WorldMapModel,
    store: &sl_settings::SettingsStore,
) -> Vec<MarkerInfo> {
    if !world_map_math::detail_regime(view.scale) {
        return Vec::new();
    }
    let mut markers = Vec::new();
    for ((_region, type_code), items) in &model.items {
        let kind = MapItemType::from_u32(*type_code);
        let enabled = match kind {
            MapItemType::AgentLocations => store.get_bool(SETTING_PEOPLE).unwrap_or(true),
            MapItemType::Telehub => store.get_bool(SETTING_INFOHUBS).unwrap_or(true),
            MapItemType::LandForSale | MapItemType::AdultLandForSale => {
                store.get_bool(SETTING_LAND_SALE).unwrap_or(true)
            }
            MapItemType::PgEvent => store.get_bool(SETTING_EVENTS).unwrap_or(true),
            MapItemType::MatureEvent => store.get_bool(SETTING_MATURE_EVENTS).unwrap_or(false),
            MapItemType::AdultEvent => store.get_bool(SETTING_ADULT_EVENTS).unwrap_or(false),
            _other => false,
        };
        if !enabled {
            continue;
        }
        for item in items {
            // OpenSim answers an agent-locations request for a region without
            // *other* avatars with a single count-0 sentinel item at the
            // region corner — not a dot to draw.
            if kind == MapItemType::AgentLocations && item.extra <= 0 {
                continue;
            }
            let position = view.view_from_global(item.position.x(), item.position.y());
            if position.x < -8.0
                || position.y < -8.0
                || position.x > view.size.x + 8.0
                || position.y > view.size.y + 8.0
            {
                continue;
            }
            markers.push(MarkerInfo {
                view: position,
                kind,
                name: item.name.clone(),
                extra: item.extra,
                extra2: item.extra2,
            });
        }
    }
    markers
}

/// Draw one marker glyph by kind.
fn draw_marker(surface: &mut Surface<'_>, marker: &MarkerInfo) {
    let (x, y) = (marker.view.x, marker.view.y);
    match marker.kind {
        MapItemType::AgentLocations => {
            // Slightly larger with more avatars at the same spot.
            let count = minimap_math::i32_to_f32(marker.extra.clamp(1, 10));
            let radius = 2.5 + count * 0.4;
            minimap_math::draw_disc(surface, x, y, radius, world_map_math::COLOR_MAP_AGENT);
        }
        MapItemType::Telehub => {
            minimap_math::draw_ring(surface, x, y, 4.0, 1.8, world_map_math::COLOR_MAP_TELEHUB);
        }
        MapItemType::LandForSale => {
            world_map_math::draw_square(surface, x, y, 3.0, world_map_math::COLOR_MAP_LAND_SALE);
        }
        MapItemType::AdultLandForSale => {
            world_map_math::draw_square(
                surface,
                x,
                y,
                3.0,
                world_map_math::COLOR_MAP_LAND_SALE_ADULT,
            );
        }
        MapItemType::PgEvent => {
            minimap_math::draw_disc(surface, x, y, 3.0, world_map_math::COLOR_MAP_EVENT);
        }
        MapItemType::MatureEvent => {
            minimap_math::draw_disc(surface, x, y, 3.0, world_map_math::COLOR_MAP_EVENT_MATURE);
        }
        MapItemType::AdultEvent => {
            minimap_math::draw_disc(surface, x, y, 3.0, world_map_math::COLOR_MAP_EVENT_ADULT);
        }
        _other => {}
    }
}

// ---------------------------------------------------------------------------
// Region-name labels.
// ---------------------------------------------------------------------------

/// Overlay pooled region-name labels on the surface in the detail regime.
#[expect(
    clippy::too_many_arguments,
    reason = "the label pass reads the view, model and settings and writes the pooled label \
              nodes' position, text and visibility — one cohesive pass"
)]
fn layout_world_map_labels(
    mut commands: Commands,
    ui: Option<ResMut<WorldMapUi>>,
    state: Res<WorldMapState>,
    model: Res<WorldMapModel>,
    settings: Res<ViewerSettings>,
    computed: Query<&ComputedNode>,
    panels: Query<&UiPanelShown>,
    mut nodes: Query<&mut Node>,
    mut visibilities: Query<&mut Visibility>,
    mut texts: Query<&mut Text>,
) {
    let Some(mut ui) = ui else {
        return;
    };
    let shown = panels.get(ui.root).is_ok_and(|shown| shown.0);
    let names_on = settings
        .store()
        .get_bool(SETTING_REGION_NAMES)
        .unwrap_or(true)
        && state.scale >= world_map_math::REGION_NAME_MIN_SCALE;
    let Ok(surface_node) = computed.get(ui.surface) else {
        return;
    };
    let logical = vec2_scale(surface_node.size(), surface_node.inverse_scale_factor());
    let to_logical = Vec2::new(
        logical.x / minimap_math::u32_to_f32(state.surface_px.x.max(1)),
        logical.y / minimap_math::u32_to_f32(state.surface_px.y.max(1)),
    );

    let mut used = 0_usize;
    if shown && names_on {
        let (min_x, max_x, min_y, max_y) = state.view.visible_grid_rect();
        let region_width = f64::from(REGION_WIDTH_METRES);
        for ((grid_x, grid_y), info) in &model.regions {
            if used >= MAX_LABELS {
                break;
            }
            if *grid_x < min_x || *grid_x > max_x || *grid_y < min_y || *grid_y > max_y {
                continue;
            }
            let Some(name) = &info.name else {
                continue;
            };
            // Anchor at the region's north-west corner, inset a little.
            let anchor = state.view.view_from_global(
                f64::from(*grid_x) * region_width,
                f64::from(grid_y.saturating_add(1)) * region_width,
            );
            let position = Vec2::new(anchor.x * to_logical.x + 3.0, anchor.y * to_logical.y + 2.0);
            if position.x < -80.0
                || position.y < -20.0
                || position.x > logical.x
                || position.y > logical.y
            {
                continue;
            }
            // Grow the pool on demand.
            if ui.labels.len() <= used {
                let wrapper = commands
                    .spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(0.0),
                            top: Val::Px(0.0),
                            ..default()
                        },
                        Pickable::IGNORE,
                        Name::new("worldmap-region-label"),
                        ChildOf(ui.surface),
                    ))
                    .id();
                let text = commands
                    .spawn((
                        Text::default(),
                        UiFont::Sans.at(LABEL_FONT_SIZE),
                        TextColor(Color::srgba(1.0, 1.0, 1.0, 0.85)),
                        Pickable::IGNORE,
                        ChildOf(wrapper),
                    ))
                    .id();
                ui.labels.push((wrapper, text));
            }
            let Some((wrapper, text)) = ui.labels.get(used).copied() else {
                break;
            };
            if let Ok(mut node) = nodes.get_mut(wrapper) {
                node.left = Val::Px(position.x);
                node.top = Val::Px(position.y);
            }
            if let Ok(mut label) = texts.get_mut(text) {
                let name = name.to_string();
                if label.0 != name {
                    label.0 = name;
                }
            }
            if let Ok(mut visibility) = visibilities.get_mut(wrapper) {
                *visibility = Visibility::Inherited;
            }
            used = used.saturating_add(1);
        }
    }
    for (wrapper, _text) in ui.labels.iter().skip(used) {
        if let Ok(mut visibility) = visibilities.get_mut(*wrapper) {
            *visibility = Visibility::Hidden;
        }
    }
}

// ---------------------------------------------------------------------------
// Hover and tooltip.
// ---------------------------------------------------------------------------

/// The marker hover pick radius, in surface pixels.
const MARKER_PICK_RADIUS: f32 = 8.0;

/// Update the hover tooltip: the marker under the cursor, otherwise the region
/// under the cursor (name, rating, agent count).
#[expect(
    clippy::too_many_arguments,
    reason = "the tooltip reads the hover state, the model and the translator, and writes the \
              tooltip nodes — one cohesive pass"
)]
fn update_world_map_hover(
    ui: Option<Res<WorldMapUi>>,
    state: Res<WorldMapState>,
    model: Res<WorldMapModel>,
    translator: Translator,
    panels: Query<&UiPanelShown>,
    mut nodes: Query<&mut Node>,
    mut visibilities: Query<&mut Visibility>,
    mut texts: Query<&mut Text>,
) {
    let Some(ui) = ui else {
        return;
    };
    let shown = panels.get(ui.root).is_ok_and(|shown| shown.0);
    let mut lines: Vec<String> = Vec::new();
    if shown && let (Some(cursor), Some(cursor_node)) = (state.cursor, state.cursor_node) {
        if let Some(marker) = state
            .markers
            .iter()
            .filter(|marker| marker.view.distance(cursor) <= MARKER_PICK_RADIUS)
            .min_by(|a, b| a.view.distance(cursor).total_cmp(&b.view.distance(cursor)))
        {
            marker_tooltip_lines(&translator, marker, &mut lines);
        } else {
            let (east, north) = state.view.global_from_view(cursor);
            let key = (
                world_map_math::grid_index(east),
                world_map_math::grid_index(north),
            );
            if let Some(info) = model.regions.get(&key) {
                region_tooltip_lines(&translator, info, &mut lines);
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

/// Append a marker's tooltip lines.
fn marker_tooltip_lines(translator: &Translator, marker: &MarkerInfo, lines: &mut Vec<String>) {
    match marker.kind {
        MapItemType::AgentLocations => {
            lines.push(translator.format(
                "worldmap-tooltip-agents",
                &TransArgs::new().int("count", i64::from(marker.extra)),
            ));
        }
        MapItemType::Telehub => {
            let key = if marker.extra2 == 1 {
                "worldmap-tooltip-infohub"
            } else {
                "worldmap-tooltip-telehub"
            };
            lines.push(translator.format(key, &TransArgs::new().text("name", &marker.name)));
        }
        MapItemType::LandForSale | MapItemType::AdultLandForSale => {
            lines.push(
                translator.format(
                    "worldmap-tooltip-land-sale",
                    &TransArgs::new()
                        .text("name", &marker.name)
                        .int("price", i64::from(marker.extra2))
                        .int("area", i64::from(marker.extra)),
                ),
            );
        }
        MapItemType::PgEvent | MapItemType::MatureEvent | MapItemType::AdultEvent => {
            lines.push(translator.format(
                "worldmap-tooltip-event",
                &TransArgs::new().text("name", &marker.name),
            ));
        }
        _other => {}
    }
}

/// Append a region's tooltip lines (name, rating, agent count).
fn region_tooltip_lines(translator: &Translator, info: &MapRegionInfo, lines: &mut Vec<String>) {
    if let Some(name) = &info.name {
        lines.push(translator.format(
            "worldmap-tooltip-region",
            &TransArgs::new().text("name", name.as_ref()),
        ));
    }
    let maturity_key = match info.maturity {
        Maturity::Pg => Some("worldmap-maturity-general"),
        Maturity::Mature => Some("worldmap-maturity-moderate"),
        Maturity::Adult => Some("worldmap-maturity-adult"),
        _other => None,
    };
    if let Some(key) = maturity_key {
        lines.push(translator.get(key));
    }
    if info.agents > 0 {
        lines.push(translator.format(
            "worldmap-tooltip-region-agents",
            &TransArgs::new().int("count", i64::from(info.agents)),
        ));
    }
}

// ---------------------------------------------------------------------------
// Input observers.
// ---------------------------------------------------------------------------

/// A primary press records the cursor, so a later click can tell a genuine
/// click from the release of a pan drag.
fn on_world_map_press(press: On<Pointer<Press>>, mut state: ResMut<WorldMapState>) {
    if press.button == PointerButton::Primary {
        state.press_cursor = state.cursor;
    }
}

/// Pixels of cursor travel between press and release below which a release
/// still reads as a click (above it, it was a pan drag).
const CLICK_SLOP: f32 = 4.0;

/// A primary click on the map selects the location under the cursor: the
/// region plus its region-local coordinates, which the X/Y fields take over
/// (the reference's map click driving the location spinners).
fn on_world_map_click(click: On<Pointer<Click>>, mut state: ResMut<WorldMapState>) {
    if click.button != PointerButton::Primary {
        return;
    }
    let (Some(cursor), Some(pressed)) = (state.cursor, state.press_cursor) else {
        return;
    };
    if cursor.distance(pressed) > CLICK_SLOP {
        return;
    }
    let (east, north) = state.view.global_from_view(cursor);
    let grid_x = world_map_math::grid_index(east);
    let grid_y = world_map_math::grid_index(north);
    let local = |value: f64, corner: u32| -> u8 {
        let offset = value - f64::from(corner) * f64::from(REGION_WIDTH_METRES);
        let clamped = offset.clamp(0.0, 255.0);
        #[expect(
            clippy::as_conversions,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "clamped to [0, 255] just above"
        )]
        let out = clamped as u8;
        out
    };
    state.selected = Some((grid_x, grid_y));
    state.pending_local = Some((local(east, grid_x), local(north, grid_y)));
}

/// A primary drag pans the map (the world map has no auto-centre; plain drag,
/// no modifier, as in the reference).
fn on_world_map_drag(drag: On<Pointer<Drag>>, mut state: ResMut<WorldMapState>) {
    if drag.button != PointerButton::Primary {
        return;
    }
    let ppm = f64::from(state.view.pixels_per_metre());
    if ppm <= 0.0 {
        return;
    }
    state.center.0 -= f64::from(drag.delta.x) / ppm;
    state.center.1 += f64::from(drag.delta.y) / ppm;
}

/// A scroll wheel over the surface zooms toward the cursor.
fn on_world_map_scroll(mut event: On<Pointer<Scroll>>, mut state: ResMut<WorldMapState>) {
    let old_scale = state.scale;
    let new_scale = world_map_math::wheel_world_scale(old_scale, event.y);
    if (new_scale - old_scale).abs() < f32::EPSILON {
        event.propagate(false);
        return;
    }
    if let Some(cursor) = state.cursor {
        let (center_east, center_north) =
            world_map_math::zoom_center(&state.view, cursor, new_scale);
        state.center = (center_east, center_north);
    }
    state.scale = new_scale;
    state.scale_save_timer = Some(1.0);
    event.propagate(false);
}

/// A right-click (secondary press) opens the context menu with the layer
/// toggles and zoom presets.
fn on_world_map_context(
    mut press: On<Pointer<Press>>,
    settings: Res<ViewerSettings>,
    state: Res<WorldMapState>,
    ui: Option<Res<WorldMapUi>>,
    mut menus: MessageWriter<OpenContextMenu>,
) {
    if press.button != PointerButton::Secondary {
        return;
    }
    let Some(_ui) = ui else {
        return;
    };
    let store = settings.store();
    let mut conditions: Vec<&'static str> = Vec::new();
    for (condition, setting, default) in [
        (COND_SHOW_PEOPLE, SETTING_PEOPLE, true),
        (COND_SHOW_INFOHUBS, SETTING_INFOHUBS, true),
        (COND_SHOW_LAND_SALE, SETTING_LAND_SALE, true),
        (COND_SHOW_EVENTS, SETTING_EVENTS, true),
        (COND_SHOW_MATURE_EVENTS, SETTING_MATURE_EVENTS, false),
        (COND_SHOW_ADULT_EVENTS, SETTING_ADULT_EVENTS, false),
        (COND_SHOW_REGION_NAMES, SETTING_REGION_NAMES, true),
    ] {
        if store.get_bool(setting).unwrap_or(default) {
            conditions.push(condition);
        }
    }
    for (condition, preset) in [
        (COND_ZOOM_CLOSE, world_map_math::WORLD_MAP_SCALE_CLOSE),
        (COND_ZOOM_MEDIUM, world_map_math::WORLD_MAP_SCALE_MEDIUM),
        (COND_ZOOM_FAR, world_map_math::WORLD_MAP_SCALE_FAR),
        (COND_ZOOM_GRID, world_map_math::WORLD_MAP_SCALE_GRID),
    ] {
        if (state.scale - preset).abs() < 0.5 {
            conditions.push(condition);
        }
    }
    menus.write(OpenContextMenu {
        menu: &WORLD_MAP_MENU,
        at: press.pointer_location.position,
        element: WORLD_MAP_ELEMENT,
        conditions,
    });
    press.propagate(false);
}

// ---------------------------------------------------------------------------
// Search.
// ---------------------------------------------------------------------------

/// Seconds the search field must be quiet before the query is sent.
const SEARCH_DEBOUNCE_SECONDS: f32 = 0.6;

/// The shortest query worth sending to the grid.
const SEARCH_MIN_CHARS: usize = 2;

/// Drive the region-name search: debounce the field's text into a
/// `MapNameRequest`, and rebuild the result rows when the matches change.
#[expect(
    clippy::too_many_arguments,
    reason = "the search pass reads the field, clock and model, sends the command, and rebuilds \
              the result rows — one cohesive pass"
)]
fn drive_world_map_search(
    mut commands: Commands,
    ui: Option<Res<WorldMapUi>>,
    mut state: ResMut<WorldMapState>,
    model: Res<WorldMapModel>,
    time: Res<Time>,
    fields: Query<&EditableText>,
    panels: Query<&UiPanelShown>,
    rows: Query<Entity, With<WorldMapResultRow>>,
    mut sl_commands: MessageWriter<SlCommand>,
) {
    let Some(ui) = ui else {
        return;
    };
    if !panels.get(ui.root).is_ok_and(|shown| shown.0) {
        return;
    }
    // Mirror the field text; a change re-arms the debounce.
    if let Ok(field) = fields.get(ui.search_field) {
        let text = field.value().to_string().trim().to_owned();
        if text != state.search_query {
            state.search_query = text;
            state.search_debounce = Some(SEARCH_DEBOUNCE_SECONDS);
            state.results_dirty = true;
        }
    }
    // Debounced send.
    if let Some(timer) = state.search_debounce {
        let next = timer - time.delta_secs();
        if next <= 0.0 {
            state.search_debounce = None;
            let query = state.search_query.clone();
            if query.chars().count() >= SEARCH_MIN_CHARS
                && state.search_sent.as_deref() != Some(query.as_str())
            {
                sl_commands.write(SlCommand(Command::RequestMapByName {
                    name: query.clone(),
                }));
                state.search_sent = Some(query);
            }
        } else {
            state.search_debounce = Some(next);
        }
    }
    // Rebuild the result rows when the matches changed.
    if !state.results_dirty {
        return;
    }
    state.results_dirty = false;
    let results = search_results(&state.search_query, model.regions.values());
    if results == state.results {
        return;
    }
    state.results = results;
    for row in &rows {
        commands.entity(row).despawn();
    }
    for (name, grid_x, grid_y) in &state.results {
        let row = commands
            .spawn((
                Node {
                    width: Val::Percent(100.0),
                    padding: UiRect::axes(Val::Px(4.0), Val::Px(2.0)),
                    ..default()
                },
                BackgroundColor(Color::NONE),
                Pickable {
                    should_block_lower: true,
                    is_hoverable: true,
                },
                WorldMapResultRow {
                    grid_x: *grid_x,
                    grid_y: *grid_y,
                },
                Name::new("worldmap-result"),
                ChildOf(ui.results),
            ))
            .observe(on_result_click)
            .id();
        commands.spawn((
            Text::new(name.clone()),
            UiFont::Sans.at(PANEL_FONT_SIZE),
            TextColor(Color::srgba(0.9, 0.9, 0.9, 1.0)),
            Pickable::IGNORE,
            ChildOf(row),
        ));
    }
}

/// The regions matching a query, prefix matches first then alphabetical,
/// capped at [`MAX_RESULTS`]. An empty / too-short query yields no rows.
fn search_results<'model>(
    query: &str,
    regions: impl Iterator<Item = &'model MapRegionInfo>,
) -> Vec<(String, u32, u32)> {
    let query = query.trim().to_lowercase();
    if query.chars().count() < SEARCH_MIN_CHARS {
        return Vec::new();
    }
    let mut matches: Vec<(bool, String, u32, u32)> = regions
        .filter_map(|info| {
            let name = info.name.as_ref()?.to_string();
            let lower = name.to_lowercase();
            lower.contains(&query).then(|| {
                (
                    !lower.starts_with(&query),
                    name,
                    info.grid_coordinates.x(),
                    info.grid_coordinates.y(),
                )
            })
        })
        .collect();
    matches.sort_unstable();
    matches.dedup();
    matches
        .into_iter()
        .take(MAX_RESULTS)
        .map(|(_suffix, name, x, y)| (name, x, y))
        .collect()
}

/// A wheel over the result list scrolls it (`bevy_ui` clips an
/// `Overflow::scroll` node but does not itself move it — the app owns the
/// wheel, as in [`crate::gallery`]).
fn on_results_scroll(mut event: On<Pointer<Scroll>>, mut positions: Query<&mut ScrollPosition>) {
    /// Logical pixels one wheel notch scrolls.
    const LINE_SCROLL_PIXELS: f32 = 24.0;
    if let Ok(mut position) = positions.get_mut(event.entity) {
        position.0.y = (position.0.y - event.y * LINE_SCROLL_PIXELS).max(0.0);
    }
    event.propagate(false);
}

/// A click on a search result recentres the map on that region (zooming in to
/// the detail regime when currently zoomed far out) and **selects** it — the
/// region becomes the Teleport / Copy SLURL target at its centre, as picking
/// a reference search result tracks the region.
fn on_result_click(
    click: On<Pointer<Click>>,
    rows: Query<&WorldMapResultRow>,
    mut state: ResMut<WorldMapState>,
) {
    if click.button != PointerButton::Primary {
        return;
    }
    let Ok(row) = rows.get(click.entity) else {
        return;
    };
    let region_width = f64::from(REGION_WIDTH_METRES);
    state.center = (
        (f64::from(row.grid_x) + 0.5) * region_width,
        (f64::from(row.grid_y) + 0.5) * region_width,
    );
    if state.scale < world_map_math::WORLD_MAP_SCALE_FAR {
        state.scale = world_map_math::WORLD_MAP_SCALE_MEDIUM;
        state.scale_save_timer = Some(0.0);
    }
    state.selected = Some((row.grid_x, row.grid_y));
    state.pending_local = Some((128, 128));
}

/// Highlight the selected search-result row (the region the Teleport / Copy
/// SLURL target sits in).
fn refresh_world_map_result_selection(
    state: Res<WorldMapState>,
    mut rows: Query<(&WorldMapResultRow, &mut BackgroundColor)>,
) {
    for (row, mut background) in &mut rows {
        let selected = state.selected == Some((row.grid_x, row.grid_y));
        let wanted = if selected {
            Color::srgba(0.25, 0.42, 0.62, 0.9)
        } else {
            Color::NONE
        };
        if background.0 != wanted {
            background.0 = wanted;
        }
    }
}

// ---------------------------------------------------------------------------
// The context menu.
// ---------------------------------------------------------------------------

/// Condition: avatar markers are shown.
const COND_SHOW_PEOPLE: &str = "worldmap-show-people";

/// Condition: telehub markers are shown.
const COND_SHOW_INFOHUBS: &str = "worldmap-show-infohubs";

/// Condition: land-for-sale markers are shown.
const COND_SHOW_LAND_SALE: &str = "worldmap-show-land-sale";

/// Condition: PG event markers are shown.
const COND_SHOW_EVENTS: &str = "worldmap-show-events";

/// Condition: Moderate event markers are shown.
const COND_SHOW_MATURE_EVENTS: &str = "worldmap-show-mature-events";

/// Condition: Adult event markers are shown.
const COND_SHOW_ADULT_EVENTS: &str = "worldmap-show-adult-events";

/// Condition: region-name labels are shown.
const COND_SHOW_REGION_NAMES: &str = "worldmap-show-region-names";

/// Condition: the scale matches the Close preset.
const COND_ZOOM_CLOSE: &str = "worldmap-zoom-close";

/// Condition: the scale matches the Medium preset.
const COND_ZOOM_MEDIUM: &str = "worldmap-zoom-medium";

/// Condition: the scale matches the Far preset.
const COND_ZOOM_FAR: &str = "worldmap-zoom-far";

/// Condition: the scale matches the Grid preset.
const COND_ZOOM_GRID: &str = "worldmap-zoom-grid";

/// The Show submenu (marker layer toggles).
static WORLD_MAP_SHOW_MENU: MenuDef = MenuDef {
    label: "Show",
    items: &[
        MenuItemDef::Command(
            MenuCommand::new("People", "toggle-people").checked_when(COND_SHOW_PEOPLE),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Telehubs", "toggle-infohubs").checked_when(COND_SHOW_INFOHUBS),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Land for Sale", "toggle-land-sale").checked_when(COND_SHOW_LAND_SALE),
        ),
        MenuItemDef::Separator,
        MenuItemDef::Command(
            MenuCommand::new("Events", "toggle-events").checked_when(COND_SHOW_EVENTS),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Moderate Events", "toggle-mature-events")
                .checked_when(COND_SHOW_MATURE_EVENTS),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Adult Events", "toggle-adult-events")
                .checked_when(COND_SHOW_ADULT_EVENTS),
        ),
        MenuItemDef::Separator,
        MenuItemDef::Command(
            MenuCommand::new("Region Names", "toggle-region-names")
                .checked_when(COND_SHOW_REGION_NAMES),
        ),
    ],
};

/// The Zoom submenu (radio checks on the preset in effect).
static WORLD_MAP_ZOOM_MENU: MenuDef = MenuDef {
    label: "Zoom",
    items: &[
        MenuItemDef::Command(MenuCommand::new("Close", "zoom-close").checked_when(COND_ZOOM_CLOSE)),
        MenuItemDef::Command(
            MenuCommand::new("Medium", "zoom-medium").checked_when(COND_ZOOM_MEDIUM),
        ),
        MenuItemDef::Command(MenuCommand::new("Far", "zoom-far").checked_when(COND_ZOOM_FAR)),
        MenuItemDef::Command(
            MenuCommand::new("Whole Grid", "zoom-grid").checked_when(COND_ZOOM_GRID),
        ),
    ],
};

/// The world-map context menu.
static WORLD_MAP_MENU: MenuDef = MenuDef {
    label: "World Map",
    items: &[
        MenuItemDef::Command(MenuCommand::new("Center on Me", "center-self")),
        MenuItemDef::Separator,
        MenuItemDef::Submenu(&WORLD_MAP_ZOOM_MENU),
        MenuItemDef::Submenu(&WORLD_MAP_SHOW_MENU),
    ],
};

/// Dispatch the world map's context-menu and side-panel picks.
fn handle_world_map_actions(
    mut actions: MessageReader<UiAction>,
    mut state: ResMut<WorldMapState>,
    mut settings: ResMut<ViewerSettings>,
    model: Res<WorldMapModel>,
    clipboard: Res<WorldMapClipboard>,
    mut commands: MessageWriter<SlCommand>,
) {
    for action in actions.read() {
        if action.element != WORLD_MAP_ELEMENT {
            continue;
        }
        match action.action {
            "center-self" => {
                if let Some((east, north)) = state.agent {
                    state.center = (east, north);
                }
            }
            "teleport-selected" => {
                if let Some((grid_x, grid_y)) = state.selected {
                    let (x, y, z) = state.selected_local;
                    commands.write(SlCommand(Command::Teleport {
                        region_handle: RegionHandle::from_grid(grid_x, grid_y),
                        position: RegionCoordinates::new(f32::from(x), f32::from(y), f32::from(z)),
                        look_at: Vector {
                            x: 1.0,
                            y: 0.0,
                            z: 0.0,
                        },
                    }));
                }
            }
            "copy-slurl" => {
                if let Some(slurl) = selection_slurl(&state, &model) {
                    copy_to_clipboard(&clipboard, &slurl);
                }
            }
            "zoom-close" => set_scale(&mut state, world_map_math::WORLD_MAP_SCALE_CLOSE),
            "zoom-medium" => set_scale(&mut state, world_map_math::WORLD_MAP_SCALE_MEDIUM),
            "zoom-far" => set_scale(&mut state, world_map_math::WORLD_MAP_SCALE_FAR),
            "zoom-grid" => set_scale(&mut state, world_map_math::WORLD_MAP_SCALE_GRID),
            "toggle-people" => toggle_setting(&mut settings, SETTING_PEOPLE, true),
            "toggle-infohubs" => toggle_setting(&mut settings, SETTING_INFOHUBS, true),
            "toggle-land-sale" => toggle_setting(&mut settings, SETTING_LAND_SALE, true),
            "toggle-events" => toggle_setting(&mut settings, SETTING_EVENTS, true),
            "toggle-mature-events" => {
                toggle_setting(&mut settings, SETTING_MATURE_EVENTS, false);
            }
            "toggle-adult-events" => toggle_setting(&mut settings, SETTING_ADULT_EVENTS, false),
            "toggle-region-names" => toggle_setting(&mut settings, SETTING_REGION_NAMES, true),
            _other => {}
        }
        if action.action.starts_with("toggle-") {
            state.last_stamp = None;
        }
    }
}

/// Set the map scale from a zoom preset (persisted immediately).
const fn set_scale(state: &mut WorldMapState, scale: f32) {
    state.scale = world_map_math::clamp_world_scale(scale);
    state.scale_save_timer = Some(0.0);
}

/// Flip a boolean setting in the global scope.
fn toggle_setting(settings: &mut ViewerSettings, name: &str, default: bool) {
    let current = settings.store().get_bool(name).unwrap_or(default);
    settings.set(Scope::Global, name, SettingValue::Bool(!current));
}

/// The SLURL for the selected location, once its region's name is known (the
/// shared `sl-types` `Location` maps-URL form).
fn selection_slurl(state: &WorldMapState, model: &WorldMapModel) -> Option<String> {
    let (grid_x, grid_y) = state.selected?;
    let name = model
        .regions
        .get(&(grid_x, grid_y))
        .and_then(|info| info.name.clone())?;
    let (x, y, z) = state.selected_local;
    Some(sl_types::map::Location::new(name, x, y, z.min(4095)).as_maps_url())
}

/// Offer text on the OS clipboard, initialising the kept-alive handle on
/// first use; a clipboard failure only logs (headless / portal-less setups).
fn copy_to_clipboard(clipboard: &WorldMapClipboard, text: &str) {
    let Ok(mut holder) = clipboard.0.lock() else {
        return;
    };
    if holder.is_none() {
        match arboard::Clipboard::new() {
            Ok(handle) => *holder = Some(handle),
            Err(error) => {
                warn!("world map: no clipboard available: {error}");
                return;
            }
        }
    }
    if let Some(handle) = holder.as_mut()
        && let Err(error) = handle.set_text(text.to_owned())
    {
        warn!("world map: could not copy to the clipboard: {error}");
    }
}

// ---------------------------------------------------------------------------
// Gallery specimen.
// ---------------------------------------------------------------------------

/// A static world-map look for the gallery / harness: a tile-ish backdrop with
/// grid lines, a few markers, and a search side panel — no live session.
pub(crate) fn spawn_world_map_specimen(
    commands: &mut Commands,
    parent: Entity,
    _cx: ElementCx,
) -> Entity {
    let root = commands
        .spawn((
            Node {
                width: Val::Px(300.0),
                height: Val::Px(180.0),
                column_gap: Val::Px(6.0),
                ..default()
            },
            Name::new("worldmap-specimen"),
            ChildOf(parent),
        ))
        .id();
    let map = commands
        .spawn((
            Node {
                width: Val::Px(190.0),
                height: Val::Percent(100.0),
                ..default()
            },
            BackgroundColor(Color::srgb(0.06, 0.11, 0.17)),
            ChildOf(root),
        ))
        .id();
    for (left, top, size, color) in [
        (0.0, 0.0, 90.0, Color::srgb(0.22, 0.34, 0.24)),
        (95.0, 0.0, 90.0, Color::srgb(0.28, 0.30, 0.20)),
        (0.0, 95.0, 90.0, Color::srgb(0.16, 0.26, 0.33)),
    ] {
        commands.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(left),
                top: Val::Px(top),
                width: Val::Px(size),
                height: Val::Px(size.min(180.0 - top)),
                ..default()
            },
            BackgroundColor(color),
            ChildOf(map),
        ));
    }
    for (x, y, color) in [
        (40.0, 50.0, Color::srgb(0.0, 0.9, 0.0)),
        (120.0, 30.0, Color::srgb(1.0, 0.9, 0.4)),
        (70.0, 120.0, Color::srgb(1.0, 1.0, 0.0)),
    ] {
        commands.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(x),
                top: Val::Px(y),
                width: Val::Px(6.0),
                height: Val::Px(6.0),
                border_radius: BorderRadius::all(Val::Px(3.0)),
                ..default()
            },
            BackgroundColor(color),
            ChildOf(map),
        ));
    }
    let side = commands
        .spawn((
            Node {
                width: Val::Px(100.0),
                ..column(Val::Px(4.0))
            },
            ChildOf(root),
        ))
        .id();
    commands.spawn((
        Text::new("Search regions"),
        UiFont::Sans.at(PANEL_FONT_SIZE),
        TextColor(Color::srgba(0.7, 0.7, 0.7, 1.0)),
        Node {
            padding: UiRect::all(Val::Px(3.0)),
            border: UiRect::all(Val::Px(1.0)),
            ..default()
        },
        BorderColor::all(Color::srgba(0.5, 0.5, 0.5, 0.6)),
        ChildOf(side),
    ));
    for name in ["Da Boom", "Dore", "Dublin"] {
        commands.spawn((
            Text::new(name),
            UiFont::Sans.at(PANEL_FONT_SIZE),
            TextColor(Color::srgba(0.9, 0.9, 0.9, 1.0)),
            ChildOf(side),
        ));
    }
    root
}

#[cfg(test)]
mod tests {
    use super::{MenuDef, MenuItemDef, WORLD_MAP_MENU, effective_base_url, search_results};
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{MapRegionInfo, Maturity, RegionHandle, Uuid};
    use sl_types::map::GridCoordinates;

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

    /// The actions [`super::handle_world_map_actions`] dispatches explicitly.
    const HANDLED: &[&str] = &[
        "center-self",
        "teleport-selected",
        "copy-slurl",
        "zoom-close",
        "zoom-medium",
        "zoom-far",
        "zoom-grid",
        "toggle-people",
        "toggle-infohubs",
        "toggle-land-sale",
        "toggle-events",
        "toggle-mature-events",
        "toggle-adult-events",
        "toggle-region-names",
    ];

    #[test]
    fn every_menu_action_has_a_handler() {
        let mut actions = Vec::new();
        collect_actions(&WORLD_MAP_MENU, &mut actions);
        for action in actions {
            assert!(
                HANDLED.contains(&action),
                "menu action {action:?} has no handler arm"
            );
        }
    }

    #[test]
    fn base_url_prefers_features_then_login_then_agni_cdn() {
        assert_eq!(
            effective_base_url(Some("http://f/"), Some("http://l/"), "localhost"),
            Some("http://f/".to_owned())
        );
        assert_eq!(
            effective_base_url(None, Some("http://l/"), "localhost"),
            Some("http://l/".to_owned())
        );
        assert_eq!(effective_base_url(None, None, "localhost"), None);
        assert_eq!(effective_base_url(None, None, "aditi"), None);
        assert_eq!(
            effective_base_url(None, None, "agni"),
            Some(sl_map_apis::map_tiles::DEFAULT_MAP_TILE_BASE_URL.to_owned())
        );
    }

    /// A region info fixture with just a name and grid coordinates.
    fn region(name: &str, x: u32, y: u32) -> MapRegionInfo {
        let grid_coordinates = GridCoordinates::new(x, y);
        MapRegionInfo {
            name: sl_types::map::RegionName::try_new(name).ok(),
            grid_coordinates,
            region_handle: RegionHandle::from(grid_coordinates),
            maturity: Maturity::Pg,
            region_flags: 0,
            size_x: 256,
            size_y: 256,
            agents: 0,
            water_height: 20,
            map_image_id: sl_types::key::TextureKey::from(Uuid::nil()),
        }
    }

    #[test]
    fn search_matches_prefix_first_then_alphabetical() {
        let regions = [
            region("Dublin", 1000, 1000),
            region("Da Boom", 1001, 1000),
            region("Sandy Dune", 1002, 1000),
            region("Elsewhere", 1003, 1000),
        ];
        let results = search_results("du", regions.iter());
        let names: Vec<&str> = results.iter().map(|(name, _x, _y)| name.as_str()).collect();
        // Prefix match ("Dublin") first, then the substring match.
        assert_eq!(names, vec!["Dublin", "Sandy Dune"]);
        // Too-short queries yield nothing.
        assert_eq!(search_results("d", regions.iter()).len(), 0);
    }
}
