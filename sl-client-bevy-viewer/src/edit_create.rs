//! The **Create tool** (`viewer-prim-creation`): the create panel of the Build
//! Tools floater and the click-to-rez placer behind it — the reference's
//! `LLToolPlacer` / `LLToolCompCreate`.
//!
//! # Model
//!
//! - [`CreateToolState`] is the picked base type: one of the seven prim volume
//!   types, a Linden **tree** (with a species), or Linden **grass** (with a
//!   species). It is the single source of truth the placer reads.
//! - The **create panel** ([`spawn_create_panel`]) is shown only while the
//!   Create tool ([`crate::edit_tool::EditTool::Create`]) is active, standing in
//!   for the per-aspect tabs (which [`sync_create_panel`] hides). It holds the
//!   base-type radio and, for a tree / grass base, a species combo.
//! - The **placer** ([`handle_create_pointer`]): while the Create tool is
//!   active, a left click on a surface ray-casts the build point, converts it to
//!   the region's frame, and rezzes the picked base type there through the same
//!   `ObjectAdd` ([`Command::RezObject`]) the `rez_sample_*` examples use — the
//!   three families differ only in `pcode` / `state`. A held `Shift` keeps the
//!   placer active for repeat-rez; otherwise [`select_new_object`] drops into
//!   edit on the new object (selects it and switches to the Move tool) once its
//!   `ObjectAdded` arrives.
//!
//! Reference (Firestorm, read-only): `lltoolplacer` (incl. its tree / grass
//! placer variants), `lltoolcomp` (create); the `ObjectAdd` message. The prim
//! path / profile bytes, the tree / grass `pcode` / species, and the rez scales
//! mirror the `rez_sample_prims` / `rez_sample_trees` / `rez_sample_grass`
//! examples in `sl-client-tokio`.

use std::collections::HashSet;

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::ecs::system::SystemParam;
use bevy::picking::hover::HoverMap;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::window::{CursorIcon, CustomCursor, CustomCursorImage, PrimaryWindow, SystemCursorIcon};
use bevy_flair::style::components::ClassList;
use sl_client_bevy::{Command, PrimShape, SlCommand, Vector, pcode};

use crate::camera::ViewerCamera;
use crate::coords::bevy_to_sl_vec;
use crate::edit_selection::SelectionSet;
use crate::edit_tool::{
    EditTool, EditToolState, LABEL_CLASS, TOOL_FONT_SIZE, VALUE_CLASS, spawn_row_label,
};
use crate::gizmos::{GizmoInteraction, on_gizmo_layer};
use crate::hud::on_hud_layer;
use crate::hud_pick::{UiPointerClaim, pointer_over_blocking_ui};
use crate::i18n::Translated;
use crate::objects::{ObjectCategory, ObjectSlMotion, ObjectState, SceneObject};
use crate::ui::{UiPanelShown, column, row};
use crate::ui_combo::{ComboChanged, ComboSpec, spawn_combo};
use crate::ui_font::UiFont;
use crate::ui_radio::{RadioLayout, RadioSelection, RadioSpec, spawn_radio_group};

/// The uniform scale (metres) a Linden tree is rezzed at — its vector length
/// drives the tree's rendered size (the reference's `radius = scale.length() *
/// 0.05`). OpenSim's vegetation module multiplies this by ~8
/// (`VegetationModule.AdaptTree`), so a small rez scale still yields a
/// several-metre tree. Matches `rez_sample_trees`.
const TREE_SCALE: f32 = 0.5;

/// The uniform scale (metres) a Linden grass clump is rezzed at — its X/Y spread
/// the blades over an area rather than a single tuft. Matches `rez_sample_grass`.
const GRASS_SCALE: f32 = 4.0;

// The prim path / profile curve bytes (`LL_PCODE_PATH_*` / `LL_PCODE_PROFILE_*`).
/// Circle profile (`LL_PCODE_PROFILE_CIRCLE`).
const PROFILE_CIRCLE: u8 = 0x00;
/// Square profile (`LL_PCODE_PROFILE_SQUARE`).
const PROFILE_SQUARE: u8 = 0x01;
/// Equilateral-triangle profile (`LL_PCODE_PROFILE_EQUALTRI`).
const PROFILE_EQUALTRI: u8 = 0x03;
/// Half-circle profile (`LL_PCODE_PROFILE_CIRCLE_HALF`) — the sphere.
const PROFILE_CIRCLE_HALF: u8 = 0x05;
/// Straight extrusion path (`LL_PCODE_PATH_LINE`).
const PATH_LINE: u8 = 0x10;
/// Circular sweep path (`LL_PCODE_PATH_CIRCLE`) — sphere / torus family.
const PATH_CIRCLE: u8 = 0x20;

// The path top-size bytes (`200 - ratio / 0.01`), the reference's `setRatio`.
/// A full (`ratio 1.0`) top size — the default extrusion.
const TOP_FULL: u8 = 100;
/// The torus-family tube thickness (`ratio 0.25`), the reference's `setRatio(1,
/// 0.25)` — a thin ring with an open hole rather than a fat blob.
const TOP_TORUS: u8 = 175;
/// A collapsed (`ratio 0.0`) top edge — the prism's ridge.
const TOP_RIDGE: u8 = 200;
/// The prism's `-0.5` path shear (`shear / 0.01 = -50`, two's-complement in the
/// wire's unsigned byte), the reference's `setShear(-0.5, 0)` that leans the
/// collapsed top to one side into a triangular wedge.
const SHEAR_NEG_HALF: u8 = 206;

/// How long (seconds) a pending rez waits for its `ObjectAdded` before it is
/// given up on — a generous window covering a slow round-trip, after which an
/// un-matched rez (e.g. one the simulator refused) stops trying to auto-select.
const PENDING_REZ_TTL: f32 = 10.0;

/// How near (metres) a streamed object's **horizontal** position must be to a
/// pending rez point to count as that rez's object — the placement is exact in
/// X/Y (a bypass-raycast `ObjectAdd`), so a small tolerance absorbs only
/// rounding.
const REZ_MATCH_SLOP: f32 = 1.0;

/// How near (metres) a streamed object's **vertical** position must be to a
/// pending rez point — looser than the horizontal tolerance to absorb whatever
/// surface-seating Z offset the simulator applies to a freshly rezzed object.
const REZ_MATCH_SLOP_Z: f32 = 5.0;

/// One prim volume type the Create tool can rez, with the exact path / profile /
/// top-size / shear bytes and orientation the reference's `LLToolPlacer` sends
/// (`addObject`'s per-`pcode` `LLVolumeParams`).
struct PrimType {
    /// The Fluent key for the option's radio label.
    label_key: &'static str,
    /// The literal name the gallery specimen shows (no Fluent bundle there).
    gallery: &'static str,
    /// The `LL_PCODE_PATH_*` path curve byte.
    path_curve: u8,
    /// The `LL_PCODE_PROFILE_*` profile curve byte.
    profile_curve: u8,
    /// The path top-size X byte (`setRatio` X, quantized).
    path_scale_x: u8,
    /// The path top-size Y byte (`setRatio` Y, quantized).
    path_scale_y: u8,
    /// The path shear X byte (`setShear` X, quantized two's-complement).
    path_shear_x: u8,
    /// Whether the reference rezzes this type turned 90° about the Second Life
    /// Y axis (`rotation.setQuat(90°, y_axis)`) — the sphere and torus family, so
    /// the swept-circle shapes stand up rather than lying flat.
    upright: bool,
}

/// The seven prim volume types, in the order they appear in the base radio (the
/// reference's per-type create buttons). The one place the prim-type table
/// lives, so the radio, the gallery specimen, and the placer agree.
const PRIM_TYPES: [PrimType; 7] = [
    PrimType {
        label_key: "build-create-box",
        gallery: "Box",
        path_curve: PATH_LINE,
        profile_curve: PROFILE_SQUARE,
        path_scale_x: TOP_FULL,
        path_scale_y: TOP_FULL,
        path_shear_x: 0,
        upright: false,
    },
    PrimType {
        label_key: "build-create-cylinder",
        gallery: "Cylinder",
        path_curve: PATH_LINE,
        profile_curve: PROFILE_CIRCLE,
        path_scale_x: TOP_FULL,
        path_scale_y: TOP_FULL,
        path_shear_x: 0,
        upright: false,
    },
    PrimType {
        label_key: "build-create-prism",
        gallery: "Prism",
        path_curve: PATH_LINE,
        profile_curve: PROFILE_SQUARE,
        // ratio(0, 1) + shear(-0.5, 0): the top collapses in X and leans over
        // into a triangular wedge.
        path_scale_x: TOP_RIDGE,
        path_scale_y: TOP_FULL,
        path_shear_x: SHEAR_NEG_HALF,
        upright: false,
    },
    PrimType {
        label_key: "build-create-sphere",
        gallery: "Sphere",
        path_curve: PATH_CIRCLE,
        profile_curve: PROFILE_CIRCLE_HALF,
        path_scale_x: TOP_FULL,
        path_scale_y: TOP_FULL,
        path_shear_x: 0,
        upright: true,
    },
    PrimType {
        label_key: "build-create-torus",
        gallery: "Torus",
        path_curve: PATH_CIRCLE,
        profile_curve: PROFILE_CIRCLE,
        path_scale_x: TOP_FULL,
        path_scale_y: TOP_TORUS,
        path_shear_x: 0,
        upright: true,
    },
    PrimType {
        label_key: "build-create-tube",
        gallery: "Tube",
        path_curve: PATH_CIRCLE,
        profile_curve: PROFILE_SQUARE,
        path_scale_x: TOP_FULL,
        path_scale_y: TOP_TORUS,
        path_shear_x: 0,
        upright: true,
    },
    PrimType {
        label_key: "build-create-ring",
        gallery: "Ring",
        path_curve: PATH_CIRCLE,
        profile_curve: PROFILE_EQUALTRI,
        path_scale_x: TOP_FULL,
        path_scale_y: TOP_TORUS,
        path_shear_x: 0,
        upright: true,
    },
];

/// The Second Life rotation of an **upright** create type: a 90° turn about the
/// Y axis (`sin/cos 45°`), the reference's `rotation.setQuat(F_PI_BY_TWO,
/// y_axis)` for the sphere and torus family.
const fn upright_rotation() -> sl_client_bevy::Rotation {
    let half = core::f32::consts::FRAC_1_SQRT_2;
    sl_client_bevy::Rotation {
        x: 0.0,
        y: half,
        z: 0.0,
        s: half,
    }
}

/// The Linden **tree** species table (`app_settings/trees.xml`): the species
/// byte carried in the object `state` and its display name. The build tool's
/// species picker; matches the reference viewer's `LLVOTree::sSpeciesTable`.
const TREE_SPECIES: [(u8, &str); 21] = [
    (0, "Pine 1"),
    (1, "Oak"),
    (2, "Tropical Bush 1"),
    (3, "Palm 1"),
    (4, "Dogwood"),
    (5, "Tropical Bush 2"),
    (6, "Palm 2"),
    (7, "Cypress 1"),
    (8, "Cypress 2"),
    (9, "Pine 2"),
    (10, "Plumeria"),
    (11, "Winter Pine 1"),
    (12, "Winter Aspen"),
    (13, "Winter Pine 2"),
    (14, "Eucalyptus"),
    (15, "Fern"),
    (16, "Eelgrass"),
    (17, "Sea Sword"),
    (18, "Kelp 1"),
    (19, "Beach Grass 1"),
    (20, "Kelp 2"),
];

/// The Linden **grass** species table (`app_settings/grass.xml`): the species
/// byte carried in the object `state` and its display name. Matches the
/// reference viewer's `LLVOGrass::sSpeciesTable`.
const GRASS_SPECIES: [(u8, &str); 6] = [
    (0, "Grass 0"),
    (1, "Grass 1"),
    (2, "Grass 2"),
    (3, "Grass 3"),
    (4, "Grass 4"),
    (5, "Undergrowth 1"),
];

/// The radio index of the **Tree** base (after the seven prim types).
const TREE_BASE: usize = PRIM_TYPES.len();

/// The radio index of the **Grass** base (after Tree).
const GRASS_BASE: usize = PRIM_TYPES.len() + 1;

/// The number of base-type options in the create radio (prims + tree + grass).
const BASE_COUNT: usize = PRIM_TYPES.len() + 2;

/// The element id of the tree-species combo (also its [`ComboChanged`] tag).
const TREE_COMBO_ELEMENT: &str = "build-create-tree";

/// The element id of the grass-species combo.
const GRASS_COMBO_ELEMENT: &str = "build-create-grass";

/// The Create tool's picked base type and species. See the
/// [module documentation](self).
#[derive(Resource, Debug, Clone, Copy)]
pub(crate) struct CreateToolState {
    /// The selected base-type radio index: `0..PRIM_TYPES.len()` is a prim
    /// volume type, [`TREE_BASE`] a tree, [`GRASS_BASE`] grass.
    base: usize,
    /// The selected tree species byte (the object `state` for a tree rez).
    tree_species: u8,
    /// The selected grass species byte.
    grass_species: u8,
}

impl Default for CreateToolState {
    /// Default to the box prim and the first species of each plant — the
    /// reference's default create type.
    fn default() -> Self {
        Self {
            base: 0,
            tree_species: 0,
            grass_species: 0,
        }
    }
}

/// One in-flight rez awaiting its `ObjectAdded`, so the placer can drop into
/// edit on the object once it streams back.
#[derive(Debug, Clone)]
struct PendingRez {
    /// The region-local position the object was rezzed at (the match key).
    position: Vector,
    /// The object class rezzed (`pcode`), so a plant rez is not matched to a
    /// prim that happened to land nearby.
    pcode: u8,
    /// Whether to select the new object and switch to the Move tool when it
    /// arrives (false for a `Shift`-held repeat-rez, which stays in Create).
    drop_into_edit: bool,
    /// When (seconds, [`Time::elapsed_secs`]) this pending rez is given up on.
    expires_at: f32,
}

/// The pending rezzes awaiting their `ObjectAdded`.
#[derive(Resource, Debug, Default)]
struct PendingRezzes {
    /// The in-flight rezzes, oldest first.
    rezzes: Vec<PendingRez>,
}

/// Marks the Build Tools tab container, so [`sync_create_panel`] can hide the
/// per-aspect tabs while the Create tool's panel stands in for them. Inserted by
/// [`crate::edit_tool::spawn_build_floater`].
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct BuildTabContainer;

/// Marks the create panel's base-type radio group, so [`sync_create_base`] finds
/// it to mirror its selection into [`CreateToolState::base`].
#[derive(Component, Debug, Clone, Copy)]
struct CreateBaseRadio;

/// The create panel's entities, for the visibility / selection systems.
#[derive(Resource, Debug, Clone, Copy)]
struct CreatePanelUi {
    /// The panel root holding the base radio and hint (a [`UiPanelShown`] gate).
    panel: Entity,
    /// The tree-species row (a [`UiPanelShown`] gate, shown for a tree base).
    tree_row: Entity,
    /// The grass-species row (shown for a grass base).
    grass_row: Entity,
    /// The tree-species combo anchor (carries its [`ComboSelection`]).
    tree_combo: Entity,
    /// The grass-species combo anchor.
    grass_combo: Entity,
}

/// The size (pixels, square) of the magic-wand cursor image.
const WAND_CURSOR_SIZE: u32 = 24;

/// The magic-wand cursor hotspot (the sparkle at the wand tip), in pixels — the
/// point the click registers at.
const WAND_HOTSPOT: (u16, u16) = (18, 5);

/// The half-length (pixels) of each ray of the wand's tip sparkle.
const WAND_SPARKLE_RAY: f32 = 4.5;

/// The Create tool's custom mouse cursor — a magic wand, built once. Set while
/// the Create tool is active ([`update_create_cursor`]) so the pointer signals
/// "click to conjure an object".
#[derive(Resource, Debug, Clone)]
struct CreateCursor {
    /// The wand cursor icon (a custom image cursor).
    icon: CursorIcon,
}

impl FromWorld for CreateCursor {
    /// Draw the wand image once and wrap it as a custom cursor.
    fn from_world(world: &mut World) -> Self {
        let image = world.resource_mut::<Assets<Image>>().add(wand_image());
        Self {
            icon: CursorIcon::Custom(CustomCursor::Image(CustomCursorImage {
                handle: image,
                texture_atlas: None,
                flip_x: false,
                flip_y: false,
                rect: None,
                hotspot: WAND_HOTSPOT,
            })),
        }
    }
}

/// The plugin wiring the Create tool into the viewer.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct EditCreatePlugin;

impl Plugin for EditCreatePlugin {
    /// Register the create state and its systems. The placer runs after the
    /// gizmo interaction so a press on a manipulator handle never doubles as a
    /// rez, and the drop-into-edit runs after the object stream is applied so the
    /// new object is tracked by the time it is matched.
    fn build(&self, app: &mut App) {
        app.init_resource::<CreateToolState>()
            .init_resource::<PendingRezzes>()
            .init_resource::<CreateCursor>()
            .add_systems(
                Update,
                (
                    sync_create_base,
                    sync_create_species,
                    sync_create_panel,
                    handle_create_pointer.after(crate::gizmos::drive_gizmo_interaction),
                    select_new_object.after(crate::objects::update_objects),
                )
                    .chain(),
            )
            // The wand cursor runs after the camera's cursor system so, in Create
            // mode, the wand wins over the camera's default arrow.
            .add_systems(
                Update,
                update_create_cursor.after(crate::camera::update_camera_cursor),
            );
    }
}

/// Show the magic-wand cursor while the Create tool is active and the pointer is
/// over the world (not the floater), and hand the cursor back to the default
/// otherwise. Writes only on a transition (the camera cursor system's pattern),
/// so it is idle most frames and never fights the camera's own zoom / orbit
/// cursors when a modifier is held (those gestures suppress the wand).
///
/// Deliberately *not* gated on the keyboard [`InputContext`]: the wand must
/// appear the moment the Create tool is picked (that pick focuses the tool
/// radio, which would leave the context non-world until the first world click),
/// so it is gated on the pointer being over the world instead.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the tool state, \
              the wand cursor, the keyboard, the UI-occlusion inputs, the window, and the \
              transition guard"
)]
fn update_create_cursor(
    tool: Res<EditToolState>,
    cursor: Res<CreateCursor>,
    keyboard: Res<ButtonInput<KeyCode>>,
    hover_map: Res<HoverMap>,
    pickables: Query<&Pickable>,
    node_sizes: Query<&ComputedNode>,
    windows: Query<Entity, With<PrimaryWindow>>,
    mut showing_wand: Local<bool>,
    mut commands: Commands,
) {
    let alt = keyboard.pressed(KeyCode::AltLeft) || keyboard.pressed(KeyCode::AltRight);
    let over_ui = pointer_over_blocking_ui(&hover_map, &pickables, &node_sizes);
    let want_wand = tool.active && tool.tool == EditTool::Create && !alt && !over_ui;
    if want_wand == *showing_wand {
        return;
    }
    *showing_wand = want_wand;
    let Ok(entity) = windows.single() else {
        return;
    };
    if want_wand {
        commands.entity(entity).insert(cursor.icon.clone());
    } else {
        // Hand the cursor back to the default; the camera cursor system takes it
        // from here (its own last-state guard then keeps it in step).
        commands
            .entity(entity)
            .insert(CursorIcon::System(SystemCursorIcon::Default));
    }
}

/// Build the magic-wand cursor image: a diagonal wand with a four-point sparkle
/// at the tip, white with a dark outline for contrast on any background, on a
/// transparent field.
fn wand_image() -> Image {
    let size = WAND_CURSOR_SIZE;
    let texels = usize::try_from(size).unwrap_or(0);
    let mut data = Vec::with_capacity(texels.saturating_mul(texels).saturating_mul(4));
    // The wand runs from the lower-left handle to the upper-right tip; the
    // sparkle sits at the tip.
    let handle = Vec2::new(5.0, 19.0);
    let tip = Vec2::new(18.0, 6.0);
    for y in 0..size {
        for x in 0..size {
            #[expect(
                clippy::as_conversions,
                clippy::cast_precision_loss,
                reason = "cursor texel coordinates are small non-negative integers, exact as f32"
            )]
            let point = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
            let (r, g, b, a) = wand_texel(point, handle, tip);
            data.extend_from_slice(&[r, g, b, a]);
        }
    }
    Image::new(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        // The cursor pixels are read on the CPU to hand to the windowing system,
        // so the image must be retained in the main world.
        RenderAssetUsages::default(),
    )
}

/// The RGBA of one wand-cursor texel: white on the wand stick and sparkle, a
/// dark outline just outside them, transparent elsewhere.
fn wand_texel(point: Vec2, handle: Vec2, tip: Vec2) -> (u8, u8, u8, u8) {
    // Distance from the wand stick (a thick line handle→tip).
    let stick = distance_to_segment(point, handle, tip);
    // The four-point sparkle at the tip: a plus of a horizontal and a vertical
    // bar centred on the tip, each `WAND_SPARKLE_RAY` long. `h` / `v` are the
    // distance from each bar (0 while on it, growing past its rounded end), and
    // the sparkle is the nearer of the two.
    let dx = (point.x - tip.x).abs();
    let dy = (point.y - tip.y).abs();
    let h = dy.max((dx - WAND_SPARKLE_RAY).max(0.0));
    let v = dx.max((dy - WAND_SPARKLE_RAY).max(0.0));
    let ray = h.min(v);
    // Core (white) if on the stick or a sparkle ray; outline (dark) just outside.
    let core = stick < 1.3 || ray < 1.3;
    let outline = stick < 2.3 || ray < 2.3;
    if core {
        (255, 255, 255, 255)
    } else if outline {
        (30, 30, 40, 255)
    } else {
        (0, 0, 0, 0)
    }
}

/// The distance from `point` to the line segment `a`–`b`, for the wand stick.
/// Component-wise `f32` math throughout: glam's `Vec2` operators trip the
/// workspace's `arithmetic_side_effects` lint, so only its field reads are used.
fn distance_to_segment(point: Vec2, a: Vec2, b: Vec2) -> f32 {
    let abx = b.x - a.x;
    let aby = b.y - a.y;
    let apx = point.x - a.x;
    let apy = point.y - a.y;
    let len_sq = abx * abx + aby * aby;
    if len_sq <= f32::EPSILON {
        return apx.hypot(apy);
    }
    let t = ((apx * abx + apy * aby) / len_sq).clamp(0.0, 1.0);
    let cx = a.x + abx * t;
    let cy = a.y + aby * t;
    (point.x - cx).hypot(point.y - cy)
}

/// Spawn the create panel under `parent` (the floater content): the base-type
/// radio, a build hint, and the tree / grass species rows. Publishes
/// [`CreatePanelUi`]. The panel and species rows start hidden ([`UiPanelShown`]);
/// [`sync_create_panel`] reveals them with the Create tool.
pub(crate) fn spawn_create_panel(commands: &mut Commands, parent: Entity) {
    // The panel root: hidden until the Create tool is active.
    let panel = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                ..column(Val::Px(6.0))
            },
            UiPanelShown(false),
            Name::new("build-create:panel"),
            ChildOf(parent),
        ))
        .id();

    // The base-type radio: the seven prim volume types, then Tree and Grass.
    let mut base_labels: Vec<String> = PRIM_TYPES
        .iter()
        .map(|prim| prim.label_key.to_owned())
        .collect();
    base_labels.push("build-create-tree".to_owned());
    base_labels.push("build-create-grass".to_owned());
    let base_radio = spawn_radio_group(
        commands,
        panel,
        &RadioSpec {
            element: "build-create-base",
            labels: &base_labels,
            active: 0,
            tab_index: 30,
            font_size: TOOL_FONT_SIZE,
            layout: RadioLayout::Row,
            translate_labels: true,
        },
    );
    commands.entity(base_radio).insert(CreateBaseRadio);

    // A build hint: click a surface to rez.
    commands.spawn((
        Text::default(),
        Translated::new("build-create-hint"),
        UiFont::Sans.at(TOOL_FONT_SIZE),
        // A skinless fallback; the skin recolours via the class token.
        TextColor(Color::srgba(0.6, 0.6, 0.6, 1.0)),
        ClassList::new_with_classes([LABEL_CLASS]),
        ChildOf(panel),
    ));

    // The species rows, siblings of the panel (not nested inside its
    // `UiPanelShown`, so their own gates never race the panel's un-park). Each is
    // shown by [`sync_create_panel`] only when its plant base is picked.
    let (tree_row, tree_combo) = spawn_species_row(
        commands,
        parent,
        "build-create-tree-species-label",
        TREE_COMBO_ELEMENT,
        &TREE_SPECIES,
        31,
    );
    let (grass_row, grass_combo) = spawn_species_row(
        commands,
        parent,
        "build-create-grass-species-label",
        GRASS_COMBO_ELEMENT,
        &GRASS_SPECIES,
        32,
    );

    commands.insert_resource(CreatePanelUi {
        panel,
        tree_row,
        grass_row,
        tree_combo,
        grass_combo,
    });
}

/// Spawn one species row: a label and a combo of the species names. Returns the
/// row (a [`UiPanelShown`] gate) and the combo anchor.
fn spawn_species_row(
    commands: &mut Commands,
    parent: Entity,
    label_key: &'static str,
    element: &'static str,
    species: &[(u8, &str)],
    tab_index: i32,
) -> (Entity, Entity) {
    let row_entity = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                ..row(Val::Px(6.0))
            },
            UiPanelShown(false),
            Name::new(format!("build-create:{element}-row")),
            ChildOf(parent),
        ))
        .id();
    spawn_row_label(commands, row_entity, label_key);
    let labels: Vec<String> = species
        .iter()
        .map(|(_byte, name)| (*name).to_owned())
        .collect();
    let combo = spawn_combo(
        commands,
        row_entity,
        &ComboSpec {
            element,
            labels: &labels,
            active: 0,
            tab_index,
            font_size: TOOL_FONT_SIZE,
            // Literal species names, not Fluent keys.
            translate_labels: false,
        },
    );
    (row_entity, combo)
}

/// Mirror the base-type radio's selection into [`CreateToolState::base`] when the
/// user picks an option.
fn sync_create_base(
    radios: Query<&RadioSelection, (With<CreateBaseRadio>, Changed<RadioSelection>)>,
    mut state: ResMut<CreateToolState>,
) {
    for selection in &radios {
        let base = selection.active.min(BASE_COUNT.saturating_sub(1));
        if state.base != base {
            state.base = base;
        }
    }
}

/// Fold a species-combo pick into [`CreateToolState`]: the chosen index maps to
/// the species byte for whichever plant the combo drives.
fn sync_create_species(
    ui: Option<Res<CreatePanelUi>>,
    mut changed: MessageReader<ComboChanged>,
    mut state: ResMut<CreateToolState>,
) {
    let Some(ui) = ui else {
        return;
    };
    for event in changed.read() {
        if event.combo == ui.tree_combo
            && let Some((byte, _name)) = TREE_SPECIES.get(event.active)
        {
            state.tree_species = *byte;
        } else if event.combo == ui.grass_combo
            && let Some((byte, _name)) = GRASS_SPECIES.get(event.active)
        {
            state.grass_species = *byte;
        }
    }
}

/// Reveal the create panel (and the matching species row) while the Create tool
/// is active, hiding the per-aspect tabs so the panel stands in for them; hide it
/// all otherwise.
fn sync_create_panel(
    tool: Res<EditToolState>,
    create: Res<CreateToolState>,
    ui: Option<Res<CreatePanelUi>>,
    mut panels: Query<&mut UiPanelShown>,
    mut tabs: Query<&mut Node, With<BuildTabContainer>>,
) {
    if !(tool.is_changed() || create.is_changed()) {
        return;
    }
    let Some(ui) = ui else {
        return;
    };
    let creating = tool.active && tool.tool == EditTool::Create;
    set_panel_shown(&mut panels, ui.panel, creating);
    set_panel_shown(
        &mut panels,
        ui.tree_row,
        creating && create.base == TREE_BASE,
    );
    set_panel_shown(
        &mut panels,
        ui.grass_row,
        creating && create.base == GRASS_BASE,
    );
    // Hide the tabs while creating; restore them otherwise.
    let display = if creating {
        Display::None
    } else {
        Display::Flex
    };
    for mut node in &mut tabs {
        if node.display != display {
            node.display = display;
        }
    }
}

/// Set a panel's [`UiPanelShown`] only on a real change, so a stable state does
/// not re-run the (Display + tab-parking) reconcile every frame.
fn set_panel_shown(panels: &mut Query<&mut UiPanelShown>, panel: Entity, shown: bool) {
    if let Ok(mut flag) = panels.get_mut(panel)
        && flag.0 != shown
    {
        flag.0 = shown;
    }
}

/// The pointer / camera inputs the placer reads, bundled as one [`SystemParam`]
/// to stay inside Bevy's system-parameter limit (the [`crate::edit_selection`]
/// pattern).
#[derive(SystemParam)]
struct CreatePointer<'w, 's> {
    /// The mouse buttons.
    buttons: Res<'w, ButtonInput<MouseButton>>,
    /// The keyboard, for the `Shift` repeat-rez and `Alt` (camera) modifiers.
    keyboard: Res<'w, ButtonInput<KeyCode>>,
    /// The `bevy_ui` hover map, for the UI-occlusion guard.
    hover_map: Res<'w, HoverMap>,
    /// Pickability, for the UI-occlusion guard.
    pickables: Query<'w, 's, &'static Pickable>,
    /// Node sizes, for the UI-occlusion guard.
    node_sizes: Query<'w, 's, &'static ComputedNode>,
    /// The per-frame UI-claim flag (a widget that consumed the press).
    ui_claim: Res<'w, UiPointerClaim>,
    /// The window, for the cursor position.
    windows: Query<'w, 's, &'static Window>,
    /// The world camera, to build the pick ray.
    camera: Query<'w, 's, (&'static Camera, &'static GlobalTransform), With<ViewerCamera>>,
    /// Render layers, to exclude HUD / gizmo geometry from the build pick.
    layers: Query<'w, 's, (Entity, &'static RenderLayers)>,
}

/// The scene queries the placer walks to classify the ray hit (land vs an object
/// vs an avatar / attachment it may not build on).
#[derive(SystemParam)]
struct CreateScene<'w, 's> {
    /// Object identities, to resolve a hit entity to its object.
    scene: Query<'w, 's, &'static SceneObject>,
    /// Parent links, to walk from a face entity up to its object.
    parents: Query<'w, 's, &'static ChildOf>,
    /// Motions, for the attachment test on the hit object.
    motions: Query<'w, 's, &'static ObjectSlMotion>,
}

/// What the build ray struck, deciding whether a rez is allowed and where.
enum HitClass {
    /// Bare terrain (or any non-object mesh): rez on the land.
    Land,
    /// An in-world object (a prim / sculpt / mesh): rez on its surface.
    Object,
    /// An avatar or a worn attachment: no rez (the reference forbids it).
    Blocked,
}

/// The Create tool's placer: on a left click over a surface, rez the picked base
/// type at the ray-cast build point.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the tool / create \
              state, the bundled pointer + scene queries, the pick machinery, the clock, and the \
              pending-rez + command writers"
)]
fn handle_create_pointer(
    tool: Res<EditToolState>,
    create: Res<CreateToolState>,
    gizmo: Res<GizmoInteraction>,
    pointer: CreatePointer,
    scene: CreateScene,
    mut ray_cast: MeshRayCast,
    time: Res<Time>,
    mut pending: ResMut<PendingRezzes>,
    mut commands: MessageWriter<SlCommand>,
) {
    if !tool.active || tool.tool != EditTool::Create {
        return;
    }
    if !pointer.buttons.just_pressed(MouseButton::Left) {
        return;
    }
    let alt =
        pointer.keyboard.pressed(KeyCode::AltLeft) || pointer.keyboard.pressed(KeyCode::AltRight);
    if alt {
        return;
    }
    // A press over UI or a gizmo handle (or a widget that already claimed it) is
    // never a build click.
    let over_ui =
        pointer_over_blocking_ui(&pointer.hover_map, &pointer.pickables, &pointer.node_sizes);
    if gizmo.claims_pointer() || over_ui || pointer.ui_claim.is_claimed() {
        return;
    }
    let Ok(window) = pointer.windows.single() else {
        return;
    };
    let Ok((camera, camera_transform)) = pointer.camera.single() else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor) else {
        return;
    };
    // The world pick, excluding HUD / gizmo geometry exactly as selection does.
    let exclude: HashSet<Entity> = pointer
        .layers
        .iter()
        .filter(|(_entity, layers)| on_hud_layer(Some(layers)) || on_gizmo_layer(Some(layers)))
        .map(|(entity, _layers)| entity)
        .collect();
    let world_filter = |entity: Entity| !exclude.contains(&entity);
    let settings = MeshRayCastSettings::default().with_filter(&world_filter);
    let Some((entity, hit)) = ray_cast.cast_ray(ray, &settings).first().cloned() else {
        return;
    };
    // Refuse to build on an avatar or a worn attachment (the reference's guard).
    if matches!(classify_hit(entity, &scene), HitClass::Blocked) {
        return;
    }

    // Rez at the exact build point, in the region's Second Life frame — the
    // reference sends the surface point as the ray end and lets the simulator
    // seat the object on the surface, so the client adds no offset of its own.
    let mut shape = base_shape(&create);
    shape.position = bevy_to_sl_vec(hit.point);

    let shift = pointer.keyboard.pressed(KeyCode::ShiftLeft)
        || pointer.keyboard.pressed(KeyCode::ShiftRight);
    debug!(
        "build-create: rez pcode {} at ({:.2},{:.2},{:.2}){}",
        shape.pcode,
        shape.position.x,
        shape.position.y,
        shape.position.z,
        if shift { " (repeat)" } else { "" },
    );
    pending.rezzes.push(PendingRez {
        position: shape.position.clone(),
        pcode: shape.pcode,
        drop_into_edit: !shift,
        expires_at: time.elapsed_secs() + PENDING_REZ_TTL,
    });
    commands.write(SlCommand(Command::RezObject {
        shape,
        group_id: None,
    }));
}

/// Classify what the build ray struck: walk up from the hit entity to the object
/// it belongs to (if any) and decide whether a rez is allowed there.
fn classify_hit(entity: Entity, scene: &CreateScene) -> HitClass {
    let mut current = entity;
    loop {
        if let Ok(object) = scene.scene.get(current) {
            if object.category == ObjectCategory::Avatar {
                return HitClass::Blocked;
            }
            if scene
                .motions
                .get(current)
                .is_ok_and(|motion| motion.attachment)
            {
                return HitClass::Blocked;
            }
            return HitClass::Object;
        }
        match scene.parents.get(current) {
            Ok(parent) => current = parent.parent(),
            // No scene object in the ancestry — bare terrain / land.
            Err(_not_parented) => return HitClass::Land,
        }
    }
}

/// Build the [`PrimShape`] for the picked base type, at the region origin for
/// the caller to place.
fn base_shape(create: &CreateToolState) -> PrimShape {
    let origin = Vector {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };
    if let Some(prim) = PRIM_TYPES.get(create.base) {
        let mut shape = PrimShape::cube(origin);
        shape.path_curve = prim.path_curve;
        shape.profile_curve = prim.profile_curve;
        shape.path_scale_x = prim.path_scale_x;
        shape.path_scale_y = prim.path_scale_y;
        shape.path_shear_x = prim.path_shear_x;
        if prim.upright {
            shape.rotation = upright_rotation();
        }
        return shape;
    }
    // A plant base: a tree or grass, differing only in pcode / species / scale.
    let (pcode_value, scale, species) = if create.base == GRASS_BASE {
        (pcode::GRASS, GRASS_SCALE, create.grass_species)
    } else {
        (pcode::NEW_TREE, TREE_SCALE, create.tree_species)
    };
    let mut shape = PrimShape::cube(origin);
    shape.pcode = pcode_value;
    shape.state = species;
    shape.scale = Vector {
        x: scale,
        y: scale,
        z: scale,
    };
    shape
}

/// Drop into edit on a freshly rezzed object: poll the tracked scene for a root
/// object matching each pending rez (its kind and build point) and, unless it
/// was a repeat-rez, select it and switch to the Move tool. Polling the scene
/// rather than consuming the `ObjectAdded` event stream is robust to the
/// object's entity not being spawned the frame its event lands. Expired pending
/// rezzes are dropped.
fn select_new_object(
    time: Res<Time>,
    objects: Res<ObjectState>,
    scene_objects: Query<(Entity, &SceneObject, &ObjectSlMotion)>,
    mut pending: ResMut<PendingRezzes>,
    mut selection: ResMut<SelectionSet>,
    mut tool: ResMut<EditToolState>,
) {
    // Resolve every pending rez whose object is now in the scene.
    let mut resolved: Vec<usize> = Vec::new();
    for (index, rez) in pending.rezzes.iter().enumerate() {
        let Some((entity, scene)) = scene_objects.iter().find_map(|(entity, scene, motion)| {
            (motion.is_root
                && !motion.attachment
                && category_matches(rez.pcode, scene.category)
                && near_build(&motion.position, &rez.position))
            .then_some((entity, scene))
        }) else {
            continue;
        };
        resolved.push(index);
        if rez.drop_into_edit
            && let Some(full) = objects.full_key(&scene.scoped_id)
        {
            selection.select_only(scene.scoped_id, full, entity);
            // Drop into edit: the transform gizmos need a manipulator tool.
            if tool.tool != EditTool::Move {
                tool.tool = EditTool::Move;
            }
            debug!(
                "build-create: dropped into edit on new object {:?}",
                scene.scoped_id
            );
        }
    }
    // Drop resolved (high indices first, so earlier removals do not shift them)
    // and any pending rez whose object never arrived.
    let now = time.elapsed_secs();
    let mut index = pending.rezzes.len();
    while index > 0 {
        index = index.saturating_sub(1);
        let expired = pending
            .rezzes
            .get(index)
            .is_some_and(|rez| rez.expires_at <= now);
        if expired || resolved.contains(&index) {
            pending.rezzes.remove(index);
        }
    }
}

/// Whether the rezzed object's class ([`PendingRez::pcode`]) matches the
/// scene-object category the placer should adopt: a prim rez adopts a
/// prim / sculpt / mesh, a tree rez a tree, a grass rez grass.
fn category_matches(pcode: u8, category: ObjectCategory) -> bool {
    match pcode {
        pcode::NEW_TREE | pcode::TREE => category == ObjectCategory::Tree,
        pcode::GRASS => category == ObjectCategory::Grass,
        // A new prim streams back as a plain prim (never a sculpt / mesh, which
        // carry extra params a fresh rez has none of).
        _prim => matches!(
            category,
            ObjectCategory::Prim | ObjectCategory::Sculpt | ObjectCategory::Mesh
        ),
    }
}

/// Whether a streamed object at `position` is the object rezzed at `target`:
/// the horizontal (X/Y) placement is exact (a bypass-raycast rez), so it is
/// matched tightly, while the vertical (Z) is matched loosely to absorb any
/// surface-seating offset the simulator applies.
fn near_build(position: &Vector, target: &Vector) -> bool {
    (position.x - target.x).abs() < REZ_MATCH_SLOP
        && (position.y - target.y).abs() < REZ_MATCH_SLOP
        && (position.z - target.z).abs() < REZ_MATCH_SLOP_Z
}

/// Spawn the gallery specimen of the create panel: the base-type radio and a
/// species combo, a static shape for the no-login gallery and the `ui_test`
/// matrix.
pub(crate) fn spawn_create_panel_specimen(
    commands: &mut Commands,
    parent: Entity,
    cx: crate::ui_element::ElementCx,
) -> Entity {
    let root = commands
        .spawn((
            Node {
                padding: UiRect::all(Val::Px(8.0)),
                ..column(Val::Px(6.0))
            },
            Name::new("build-create-specimen"),
            ChildOf(parent),
        ))
        .id();
    // The base-type radio, with literal (sampled) labels rather than Fluent keys.
    let mut base_labels: Vec<String> = PRIM_TYPES
        .iter()
        .map(|prim| cx.text(prim.gallery))
        .collect();
    base_labels.push(cx.text("Tree"));
    base_labels.push(cx.text("Grass"));
    spawn_radio_group(
        commands,
        root,
        &RadioSpec {
            element: "build-create-base",
            labels: &base_labels,
            active: 0,
            tab_index: 1,
            font_size: cx.font_size,
            layout: RadioLayout::Row,
            translate_labels: false,
        },
    );
    // A species row: a label and a combo of the tree species.
    let species_row = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                ..row(Val::Px(6.0))
            },
            ChildOf(root),
        ))
        .id();
    commands.spawn((
        Text::new(cx.text("Species")),
        cx.font(UiFont::Sans),
        TextColor(Color::srgba(0.85, 0.85, 0.85, 1.0)),
        ClassList::new_with_classes([LABEL_CLASS]),
        ChildOf(species_row),
    ));
    let labels: Vec<String> = TREE_SPECIES
        .iter()
        .map(|(_byte, name)| cx.text(name))
        .collect();
    spawn_combo(
        commands,
        species_row,
        &ComboSpec {
            element: "build-create-tree",
            labels: &labels,
            active: 0,
            tab_index: 2,
            font_size: cx.font_size,
            translate_labels: false,
        },
    );
    // A build hint line.
    commands.spawn((
        Text::new(cx.text("Click a surface to create.")),
        cx.font(UiFont::Sans),
        TextColor(Color::srgba(0.6, 0.6, 0.6, 1.0)),
        ClassList::new_with_classes([VALUE_CLASS]),
        ChildOf(root),
    ));
    root
}

#[cfg(test)]
mod tests {
    use super::{
        BASE_COUNT, CreateToolState, GRASS_BASE, GRASS_SPECIES, PRIM_TYPES, TOP_TORUS, TREE_BASE,
        TREE_SPECIES, base_shape, category_matches, near_build,
    };
    use crate::objects::ObjectCategory;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{Vector, pcode};

    /// The default create state is the box prim (square profile, straight path).
    #[test]
    fn default_is_box_prim() {
        let state = CreateToolState::default();
        assert_eq!(state.base, 0);
        let shape = base_shape(&state);
        assert_eq!(shape.pcode, pcode::PRIMITIVE);
        // The box's path / profile bytes (LINE / SQUARE).
        assert_eq!(shape.path_curve, 0x10);
        assert_eq!(shape.profile_curve, 0x01);
        // A box is not turned upright (identity rotation).
        assert!((shape.rotation.s - 1.0).abs() < 1.0e-6);
    }

    /// A tube (a torus-family type) carries the reference's thin-tube ratio and
    /// stands upright — the fix for the "cylinder on its side" look.
    #[test]
    fn tube_is_a_thin_upright_ring() {
        // Tube is index 5 in PRIM_TYPES.
        assert!(
            PRIM_TYPES.get(5).is_some_and(|tube| tube.upright),
            "the tube stands upright"
        );
        let state = CreateToolState {
            base: 5,
            tree_species: 0,
            grass_species: 0,
        };
        let shape = base_shape(&state);
        // Circle path, square profile — a square-section ring.
        assert_eq!(shape.path_curve, 0x20);
        assert_eq!(shape.profile_curve, 0x01);
        assert_eq!(shape.path_scale_y, TOP_TORUS);
        // Upright: a 90° turn about Y (s = cos 45°).
        assert!((shape.rotation.s - core::f32::consts::FRAC_1_SQRT_2).abs() < 1.0e-4);
        assert!((shape.rotation.y - core::f32::consts::FRAC_1_SQRT_2).abs() < 1.0e-4);
    }

    /// A tree base rezzes a `NEW_TREE` carrying the species byte, at the tree
    /// scale.
    #[test]
    fn tree_base_carries_species() {
        let state = CreateToolState {
            base: TREE_BASE,
            tree_species: 14,
            grass_species: 0,
        };
        let shape = base_shape(&state);
        assert_eq!(shape.pcode, pcode::NEW_TREE);
        assert_eq!(shape.state, 14);
        assert!((shape.scale.z - 0.5).abs() < 1.0e-6);
    }

    /// A grass base rezzes a `GRASS` carrying the species byte, at the grass
    /// scale.
    #[test]
    fn grass_base_carries_species() {
        let state = CreateToolState {
            base: GRASS_BASE,
            tree_species: 0,
            grass_species: 3,
        };
        let shape = base_shape(&state);
        assert_eq!(shape.pcode, pcode::GRASS);
        assert_eq!(shape.state, 3);
        assert!((shape.scale.z - 4.0).abs() < 1.0e-6);
    }

    /// A rez's `pcode` matches only the scene category it should adopt: a prim
    /// rez a prim, a tree rez a tree, a grass rez grass.
    #[test]
    fn category_matches_the_rezzed_class() {
        assert!(category_matches(pcode::PRIMITIVE, ObjectCategory::Prim));
        assert!(category_matches(pcode::PRIMITIVE, ObjectCategory::Mesh));
        assert!(!category_matches(pcode::PRIMITIVE, ObjectCategory::Tree));
        assert!(category_matches(pcode::NEW_TREE, ObjectCategory::Tree));
        assert!(!category_matches(pcode::NEW_TREE, ObjectCategory::Grass));
        assert!(category_matches(pcode::GRASS, ObjectCategory::Grass));
    }

    /// The base radio count covers every prim type plus tree and grass, and the
    /// two plant indices are the last two.
    #[test]
    fn base_indices_are_consistent() {
        assert_eq!(BASE_COUNT, PRIM_TYPES.len() + 2);
        assert_eq!(TREE_BASE, PRIM_TYPES.len());
        assert_eq!(GRASS_BASE, PRIM_TYPES.len() + 1);
    }

    /// The species tables are the reference's full sets.
    #[test]
    fn species_tables_are_complete() {
        assert_eq!(TREE_SPECIES.len(), 21);
        assert_eq!(GRASS_SPECIES.len(), 6);
        // Contiguous species bytes 0..N, so a combo index maps to its byte.
        for (index, (byte, _name)) in TREE_SPECIES.iter().enumerate() {
            assert_eq!(usize::from(*byte), index);
        }
    }

    /// The rez-match tolerance is tight horizontally (rejects a metre-scale X/Y
    /// miss) but loose vertically (accepts a several-metre Z seating offset).
    #[test]
    fn near_build_is_tight_in_xy_loose_in_z() {
        let target = Vector {
            x: 128.0,
            y: 64.0,
            z: 25.0,
        };
        // A small horizontal offset and a metre-scale vertical seating offset:
        // still the same object.
        assert!(near_build(
            &Vector {
                x: 128.3,
                y: 64.0,
                z: 27.0
            },
            &target
        ));
        // A metre-scale horizontal miss: a different object.
        assert!(!near_build(
            &Vector {
                x: 130.0,
                y: 64.0,
                z: 25.0
            },
            &target
        ));
    }
}
