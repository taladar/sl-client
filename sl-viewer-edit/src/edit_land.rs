//! The **Land tool** (`viewer-terrain-edit-brushes`, `viewer-parcel-join-split`):
//! the Land panel of the Build Tools floater, the ground drag-select behind it,
//! and the terraform brushes — the reference's `LLToolSelectLand`,
//! `LLToolBrushLand` and `LLPanelLandInfo`, plus the `LLViewerParcelMgr` land
//! selection all three share.
//!
//! # Model
//!
//! - [`LandToolState`] is the picked land action: **Select Land**, or one of the
//!   six `ModifyLand` brushes (flatten / raise / lower / smooth / roughen /
//!   revert), together with the bulldozer's radius and strength. It mirrors the
//!   reference's `RadioLandBrushAction` / `LandBrushSize` / `LandBrushForce`
//!   settings and is persisted under those names.
//! - [`LandSelection`] is the land rectangle the panel's buttons act on, in
//!   region-local metres snapped to the 4 m parcel grid, plus the
//!   [`ParcelInfo`] the simulator answered for it. It is the reference's
//!   `mWestSouth` / `mEastNorth` plus `mCurrentParcelSelection`, and like the
//!   reference it is a *rectangle* first: a drag keeps what was drawn
//!   (`snap_selection: false`), while a plain click asks the simulator to snap
//!   the selection out to the whole parcel.
//! - The **pointer** (`handle_land_pointer`) drives both gestures off the same
//!   press on bare ground. In Select Land mode a press-drag-release builds the
//!   rectangle and requests its parcel; in a brush mode the press records the
//!   ground height under the cursor (the brush's reference `Height`) and every
//!   frame the button is held sends one `ModifyLand` at the rounded cursor
//!   point — the reference's per-frame `brush()` idle callback.
//! - The **Land panel** (`spawn_land_panel`) stands in for the per-aspect tabs
//!   while the Land tool is active, exactly as the Create panel does. It carries
//!   the action radio, the size / strength sliders, **Apply** (run the brush
//!   over the whole selection instead of under the cursor), the parcel
//!   read-out, **About Land** / **Subdivide** / **Join**, and the **Show
//!   owners** checkbox that drives the in-world ownership tint.
//! - **Undo** is not here: `Ctrl+Z` (and Build ▸ Undo) sends `UndoLand` instead
//!   of the object undo while a brush is picked, which
//!   [`crate::edit_undo`] decides — the reference swaps `gEditMenuHandler` to
//!   `LLToolBrushLand` for exactly this.
//! - **Subdivide / Join** (`handle_land_action_press`) reproduce the
//!   reference's `startDivideLand` / `startJoinLand` refusals and confirmations
//!   exactly — the `CannotDivideLand*` / `CannotJoinLand*` notifications, then
//!   `LandDivideWarning` / `JoinLandWarning`, and only on the OK answer the
//!   `ParcelDivide` / `ParcelJoin` itself.
//!
//! Reference (Firestorm, read-only): `lltoolselectland`, `lltoolbrush`
//! (`LLToolBrushLand`), `llpanelland` (`LLPanelLandInfo`), `llviewerparcelmgr`
//! (`selectLand`, `startDivideLand`, `startJoinLand`); the `ModifyLand`,
//! `UndoLand`, `ParcelPropertiesRequest`, `ParcelDivide` and `ParcelJoin`
//! messages.

use std::collections::HashSet;

use bevy::camera::visibility::RenderLayers;
use bevy::ecs::system::SystemParam;
use bevy::picking::hover::HoverMap;
use bevy::prelude::*;
use bevy::ui_widgets::{SliderRange, SliderStep};
use bevy_flair::style::components::ClassList;
use sl_client_bevy::{
    Command, ParcelInfo, ParcelRequestResult, RegionHandle, RegionLocalParcelId, SlCommand,
    SlEvent, SlIdentity, SlParcelOverlay, SlSessionEvent, Vector,
};
use sl_client_bevy::{LandBrushAction, LandBrushRadius, LandEdit, TerraformArea};
use sl_settings::{Scope, SettingValue};
use sl_viewer_notifications::{NotificationResponse, ShowNotification};
use sl_viewer_settings::ViewerSettings;
use sl_viewer_ui_widgets::settings_binding::{SettingBinding, bound_checkbox, bound_slider};
use sl_viewer_ui_widgets::ui_slider::{SliderStyle, spawn_slider};
use sl_viewer_world_scene::parcel_borders::SETTING_SHOW_PARCEL_OWNERS;

use crate::coords::{bevy_to_sl_vec, sl_to_bevy_vec};
use crate::edit_params::set_disabled_class;
use crate::edit_tool::{
    CHECKED_GLYPH, LABEL_CLASS, TOOL_FONT_SIZE, UNCHECKED_GLYPH, VALUE_CLASS, spawn_row_label,
};
use crate::gizmos::{GizmoInteraction, on_gizmo_layer};
use crate::i18n::{TransArgs, Translated, Translator};
use crate::intents::{AboutLandSubject, OpenAboutLand};
use crate::objects::SceneObject;
use crate::ui::{UiPanelShown, UiPointerClaim, column, row};
use crate::ui_font::UiFont;
use crate::ui_radio::{RadioLayout, RadioSelection, RadioSpec, spawn_radio_group};
use crate::ui_spawn::{ButtonKind, ButtonSpec, UiLabel, spawn_button};
use crate::world_api::{
    EditTool, EditToolState, TerrainState, ViewerCamera, on_hud_layer, pointer_over_blocking_ui,
};

/// The setting the picked land action is persisted under — the reference's
/// `RadioLandBrushAction`, an index into [`LAND_ACTIONS`].
pub const SETTING_LAND_BRUSH_ACTION: &str = "RadioLandBrushAction";

/// The setting the bulldozer radius is persisted under, in metres — the
/// reference's `LandBrushSize` (its `slider brush size`, 1 m…11 m).
pub const SETTING_LAND_BRUSH_SIZE: &str = "LandBrushSize";

/// The setting the bulldozer strength is persisted under — the reference's
/// `LandBrushForce`, the multiplier on the per-frame `Seconds` a brush sends.
pub const SETTING_LAND_BRUSH_FORCE: &str = "LandBrushForce";

/// The settings section the land-tool knobs live under.
const LAND_SECTION: &[&str] = &["build", "land"];

/// Side length, in metres, of one parcel-overlay grid square — the reference's
/// `PARCEL_GRID_STEP_METERS`, the granularity a land selection snaps to.
const PARCEL_GRID_STEP_METRES: f32 = 4.0;

/// The fallback region width, in metres, before the parcel overlay has said how
/// wide this region actually is (a var-region is wider).
const DEFAULT_REGION_WIDTH_METRES: f32 = 256.0;

/// The smallest brush force the reference's strength slider offers.
const FORCE_MIN: f32 = 0.1;

/// The largest brush force the reference's strength slider offers.
const FORCE_MAX: f32 = 100.0;

/// The default brush force — the reference's `LandBrushForce` default.
const FORCE_DEFAULT: f32 = 1.0;

/// The default bulldozer radius, in metres — the reference's `LandBrushSize`
/// default (its slider's `initial_value`).
const RADIUS_DEFAULT: f32 = 2.0;

/// How the `Seconds` a brush sends is scaled when it is applied to a whole
/// **selection** in one stroke rather than held under the cursor, indexed by the
/// action's wire code (`LLToolBrushLand::modifyLandInSelectionGlobal`). Revert
/// is the exception — see [`APPLY_REVERT_SECONDS`].
const APPLY_SCALE: [f32; 6] = [0.25, 0.25, 0.25, 5.0, 0.5, 1.0];

/// The fixed `Seconds` a whole-selection **revert** sends, ignoring the force
/// slider (the reference's `seconds = 0.5f` for `E_LAND_REVERT`).
const APPLY_REVERT_SECONDS: f32 = 0.5;

/// The frame rate the per-frame brush strength is computed against when the real
/// frame time is unusable (a zero delta on the first frame). The reference
/// likewise divides by its own clamped `gFPSClamped`.
const FALLBACK_FPS: f32 = 30.0;

/// The `ParcelPropertiesRequest` sequence id the *first* land selection asks
/// with; each later selection takes the next one below it.
///
/// The reference uses one constant here (`SELECTED_PARCEL_SEQ_ID`, -10000)
/// because it has exactly one selection, which leaves it unable to tell a late
/// reply to a superseded selection from the current one. Counting downward from
/// that constant keeps the ids in the same negative range — clear of About
/// Land's counter, which runs upward from 1 — while making each request
/// distinguishable.
const LAND_SELECTION_SEQ_ID: i32 = -10_000;

/// The smallest area, in m², that counts as a land selection worth joining —
/// one 4 m × 4 m parcel grid square (the reference's `PARCEL_UNIT_AREA`).
const PARCEL_UNIT_AREA: f32 = PARCEL_GRID_STEP_METRES * PARCEL_GRID_STEP_METRES;

/// How high above the ground the selection outline is drawn, in metres, so it
/// reads over the terrain rather than z-fighting it.
const OUTLINE_LIFT_METRES: f32 = 0.1;

/// How often, in metres, the selection outline samples the terrain height, so it
/// drapes over undulations instead of cutting through them.
const OUTLINE_SAMPLE_METRES: f32 = 2.0;

/// The most segments one outline edge is drawn with, so a region-wide selection
/// cannot turn into an unbounded line count.
const OUTLINE_MAX_STEPS: usize = 256;

/// The colour of the committed land selection's outline (the reference's white
/// selection rectangle).
const SELECTION_COLOR: Color = Color::srgb(1.0, 1.0, 1.0);

/// The colour of the rectangle being dragged out, before it is committed.
const DRAG_COLOR: Color = Color::srgb(1.0, 0.85, 0.2);

/// The colour of the bulldozer footprint drawn under the cursor.
const BRUSH_COLOR: Color = Color::srgb(0.4, 0.9, 1.0);

/// How the Land panel's two sliders are drawn.
const LAND_SLIDER: SliderStyle = SliderStyle {
    track_width: 110.0,
    track_height: 14.0,
    border: 2.0,
    border_color: Color::srgb(0.4, 0.4, 0.45),
    track_fill: Color::srgb(0.16, 0.17, 0.2),
    thumb_width: 12.0,
    thumb_fill: Color::srgb(0.62, 0.66, 0.74),
};

/// One land action the panel's radio offers: the rectangle drag-select, or one
/// of the six terraform brushes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LandAction {
    /// **Select Land** — drag a rectangle of ground (the reference's
    /// `LLToolSelectLand`). Sculpts nothing.
    #[default]
    Select,
    /// One of the six `ModifyLand` brushes (the reference's `LLToolBrushLand`).
    Brush(LandBrushAction),
}

impl LandAction {
    /// The brush this action sculpts with, or `None` for
    /// [`Select`](Self::Select).
    #[must_use]
    pub const fn brush(self) -> Option<LandBrushAction> {
        match self {
            Self::Select => None,
            Self::Brush(brush) => Some(brush),
        }
    }

    /// This action's index into [`LAND_ACTIONS`] — the radio option it selects,
    /// and the value persisted as [`SETTING_LAND_BRUSH_ACTION`].
    #[must_use]
    pub fn radio_index(self) -> usize {
        LAND_ACTIONS
            .iter()
            .position(|&action| action == self)
            .unwrap_or(0)
    }
}

/// The land-action radio options, in the reference's `land_radio_group` order.
/// The one place the index↔action mapping lives, so the panel, the persisted
/// setting and the brush all agree.
pub const LAND_ACTIONS: [LandAction; 7] = [
    LandAction::Select,
    LandAction::Brush(LandBrushAction::Level),
    LandAction::Brush(LandBrushAction::Raise),
    LandAction::Brush(LandBrushAction::Lower),
    LandAction::Brush(LandBrushAction::Smooth),
    LandAction::Brush(LandBrushAction::Noise),
    LandAction::Brush(LandBrushAction::Revert),
];

/// The Fluent label keys for [`LAND_ACTIONS`], in the same order.
const LAND_ACTION_KEYS: [&str; 7] = [
    "build-land-select",
    "build-land-flatten",
    "build-land-raise",
    "build-land-lower",
    "build-land-smooth",
    "build-land-roughen",
    "build-land-revert",
];

/// The land tool's picked action and bulldozer settings. See the
/// [module documentation](self).
#[derive(Resource, Debug)]
pub struct LandToolState {
    /// The picked action: the rectangle select, or a brush.
    pub action: LandAction,
    /// The bulldozer radius, in metres.
    pub radius: LandBrushRadius,
    /// The bulldozer strength (the reference's `LandBrushForce`).
    pub force: f32,
}

impl Default for LandToolState {
    /// The reference's defaults: the rectangle select, a 2 m brush at force 1.
    fn default() -> Self {
        Self {
            action: LandAction::Select,
            radius: LandBrushRadius::new(RADIUS_DEFAULT),
            force: FORCE_DEFAULT,
        }
    }
}

/// A land rectangle in region-local metres, west/south to east/north. Always
/// sanitised (`west <= east`, `south <= north`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LandRect {
    /// The western edge, in region-local metres.
    pub west: f32,
    /// The southern edge, in region-local metres.
    pub south: f32,
    /// The eastern edge, in region-local metres.
    pub east: f32,
    /// The northern edge, in region-local metres.
    pub north: f32,
}

impl LandRect {
    /// A rectangle from its four edges, as the simulator reports a parcel's
    /// bounding box. Sanitised but **not** snapped: a parcel's own bounds are
    /// already grid-aligned and are not the client's to move.
    #[must_use]
    pub const fn new(west: f32, south: f32, east: f32, north: f32) -> Self {
        Self {
            west: west.min(east),
            south: south.min(north),
            east: west.max(east),
            north: south.max(north),
        }
    }

    /// The rectangle two dragged ground corners select, snapped out to the 4 m
    /// parcel grid.
    ///
    /// This is the reference's `LLToolSelectLand::handleMouseUp` arithmetic:
    /// sanitise the corners, push them half a grid step apart, then round each
    /// to the nearest multiple of the step. A press and release on the same spot
    /// therefore selects the single 4 m square under the cursor rather than
    /// nothing.
    #[must_use]
    pub fn snapped_from_corners(start: Vec2, end: Vec2) -> Self {
        let half = PARCEL_GRID_STEP_METRES / 2.0;
        let snap = |value: f32| {
            let snapped = (value / PARCEL_GRID_STEP_METRES).round() * PARCEL_GRID_STEP_METRES;
            // A drag that starts within half a grid step of the region's west or
            // south edge snaps through zero: `(-1.0 / 4.0).round()` is `-0.0`,
            // which multiplies out to a **negative** zero. It compares equal to
            // zero but is a different float, so it would go on the wire as
            // `-0.0` and read as one in any log or test. Normalise it.
            if snapped == 0.0 { 0.0 } else { snapped }
        };
        Self {
            west: snap(start.x.min(end.x) - half),
            south: snap(start.y.min(end.y) - half),
            east: snap(start.x.max(end.x) + half),
            north: snap(start.y.max(end.y) + half),
        }
    }

    /// Clamp the rectangle into a region `width` metres across.
    ///
    /// A land selection may not cross a region boundary — the reference refuses
    /// one outright with `CantSelectLandFromMultipleRegions` — and the snap
    /// above can push an edge-hugging drag a grid step outside.
    #[must_use]
    pub const fn clamped_to_region(self, width: f32) -> Self {
        Self {
            west: self.west.clamp(0.0, width),
            south: self.south.clamp(0.0, width),
            east: self.east.clamp(0.0, width),
            north: self.north.clamp(0.0, width),
        }
    }

    /// The rectangle's area in m².
    #[must_use]
    pub fn area(self) -> f32 {
        (self.east - self.west) * (self.north - self.south)
    }

    /// Whether the rectangle covers no ground at all — a drag that snapped to
    /// nothing, which is not a selection.
    #[must_use]
    pub fn is_empty(self) -> bool {
        self.area() <= 0.0
    }

    /// This rectangle as a [`TerraformArea`], the form `ModifyLand` carries.
    #[must_use]
    pub const fn to_terraform_area(self) -> TerraformArea {
        TerraformArea::new(self.west, self.south, self.east, self.north)
    }
}

/// The land rectangle the Land panel acts on, and what the simulator said about
/// it — the reference's `LLViewerParcelMgr` selection. See the
/// [module documentation](self).
#[derive(Resource, Debug)]
pub struct LandSelection {
    /// The selected rectangle, or `None` when nothing is selected.
    pub rect: Option<LandRect>,
    /// The parcel the simulator answered for the rectangle, once its
    /// `ParcelProperties` arrives.
    pub parcel: Option<ParcelInfo>,
    /// Whether the selection covers a **whole** parcel rather than a piece of
    /// one — the reference's `mWholeParcelSelected`. Set when the selection was
    /// snapped out to the parcel's bounds, cleared for a drawn rectangle.
    pub whole_parcel: bool,
    /// Whether the rectangle spans parcels with **different owners** — the
    /// reference's `mSelectedMultipleOwners`, taken from a
    /// [`ParcelRequestResult::Multiple`] reply. Join requires it.
    pub multiple_owners: bool,
    /// The `ParcelPropertiesRequest` sequence id awaited for this selection, so
    /// a late reply to a superseded selection (or to another window's question)
    /// is ignored.
    pending: Option<i32>,
    /// The sequence id the next request will use, counting down from
    /// [`LAND_SELECTION_SEQ_ID`].
    next_sequence: i32,
}

impl Default for LandSelection {
    fn default() -> Self {
        Self {
            rect: None,
            parcel: None,
            whole_parcel: false,
            multiple_owners: false,
            pending: None,
            next_sequence: LAND_SELECTION_SEQ_ID,
        }
    }
}

impl LandSelection {
    /// Drop the selection entirely (the reference's `deselectLand`).
    fn clear(&mut self) {
        self.rect = None;
        self.parcel = None;
        self.whole_parcel = false;
        self.multiple_owners = false;
        self.pending = None;
    }

    /// The sequence id for the next `ParcelPropertiesRequest`, never repeated
    /// while the session lives.
    const fn next_sequence(&mut self) -> i32 {
        let id = self.next_sequence;
        self.next_sequence = self.next_sequence.wrapping_sub(1);
        id
    }

    /// The selection's area in m²: the **parcel's** own area when a whole parcel
    /// is selected, and the drawn rectangle's otherwise (the reference's
    /// `getSelectedArea`).
    #[must_use]
    fn area(&self) -> f32 {
        match (&self.parcel, self.whole_parcel) {
            (Some(parcel), true) => parcel_area_m2(parcel),
            _rectangle => self.rect.map_or(0.0, LandRect::area),
        }
    }
}

/// A parcel's reported area in m², as an `f32`.
///
/// `f32` represents every integer below 2^24 exactly, and the largest parcel any
/// grid can report is a whole var-region — 2048 m a side on Second Life, ~4.2M
/// m², well inside that — so this loses nothing a selection read-out could show.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "see above: every parcel area a grid can report is an f32-exact integer"
)]
const fn parcel_area_m2(parcel: &ParcelInfo) -> f32 {
    parcel.area.0 as f32
}

/// The live land drag: where the gesture started, whether it is a brush stroke,
/// and the reference height a brush levels toward.
#[derive(Resource, Debug, Default)]
struct LandDrag {
    /// The ground point the press landed on, in region-local metres.
    start: Option<Vec2>,
    /// The ground point the cursor is over now, in region-local metres.
    current: Option<Vec2>,
    /// Whether this drag is a brush stroke rather than a rectangle select.
    brushing: bool,
    /// The ground height under the press (the brush's wire `Height`) — the
    /// reference's `mStartingZ`, sampled once at mouse-down so a level/flatten
    /// stroke keeps levelling toward where it began.
    start_height: f32,
}

/// The ground point the cursor is over this frame, in region-local metres, so
/// the bulldozer footprint can be drawn under it with no button held.
#[derive(Resource, Debug, Default)]
struct LandHover(Option<Vec2>);

/// Which land action a panel button runs.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum LandButton {
    /// Run the picked brush over the whole selection in one stroke (the
    /// reference's `button apply to selection`).
    Apply,
    /// Open About Land on the selected parcel.
    AboutLand,
    /// Chop the selected rectangle out of its parcel (`ParcelDivide`).
    Subdivide,
    /// Merge the parcels under the selected rectangle (`ParcelJoin`).
    Join,
}

/// The action radio of the Land panel, so its selection can be mirrored to and
/// from [`LandToolState`].
#[derive(Component, Debug)]
struct LandActionRadio;

/// The Land panel's entities, kept so the sync pass can find each line without a
/// marker query apiece.
///
/// The four button entries are the buttons' **label** nodes, not their boxes:
/// the greyed-out state is a skin class on the caption
/// ([`set_disabled_class`]), which is where the build tools carry it.
#[derive(Resource, Debug)]
struct LandPanelUi {
    /// The panel root, shown only while the Land tool is active.
    panel: Entity,
    /// The parcel / area read-out line.
    summary: Entity,
    /// The **Apply** caption, greyed unless a brush and a selection are both up.
    apply: Entity,
    /// The **About Land** caption, greyed with no selected parcel.
    about_land: Entity,
    /// The **Subdivide** caption, greyed when the selection cannot be divided.
    subdivide: Entity,
    /// The **Join** caption, greyed when the selection cannot be joined.
    join: Entity,
    /// The **Show owners** checkbox's glyph node.
    owners_glyph: Entity,
}

/// A land rectangle awaiting the user's answer to a confirmation, so the
/// `ParcelDivide` / `ParcelJoin` goes out against the rectangle that was
/// *confirmed* rather than whatever is selected when the answer comes back.
#[derive(Resource, Debug, Default)]
struct PendingLandConfirm {
    /// The rectangle `LandDivideWarning` was raised for.
    divide: Option<LandRect>,
    /// The rectangle `JoinLandWarning` was raised for.
    join: Option<LandRect>,
}

/// Register the land tool's persisted settings (the reference's
/// `RadioLandBrushAction` / `LandBrushSize` / `LandBrushForce`).
pub fn register_settings(settings: &mut ViewerSettings) {
    settings.register_in(
        LAND_SECTION,
        SETTING_LAND_BRUSH_ACTION,
        SettingValue::I32(0),
        "The land tool's picked action: 0 = select land, 1..=6 = the flatten / \
         raise / lower / smooth / roughen / revert brushes",
    );
    settings.register_in(
        LAND_SECTION,
        SETTING_LAND_BRUSH_SIZE,
        SettingValue::F32(RADIUS_DEFAULT),
        "The terraform bulldozer's radius, in metres (1..=11)",
    );
    settings.register_in(
        LAND_SECTION,
        SETTING_LAND_BRUSH_FORCE,
        SettingValue::F32(FORCE_DEFAULT),
        "The terraform bulldozer's strength multiplier",
    );
}

/// The land tool: its state, the ground gestures, the panel and the parcel
/// actions.
#[derive(Debug, Default)]
pub struct EditLandPlugin;

impl Plugin for EditLandPlugin {
    /// Register the land state and its systems. The pointer runs after the gizmo
    /// interaction (as the create placer does) so a press on a manipulator handle
    /// is never also a land gesture, and the parcel reply is folded in before the
    /// panel syncs so a freshly-snapped selection shows its parcel the same frame.
    fn build(&self, app: &mut App) {
        app.init_resource::<LandToolState>()
            .init_resource::<LandSelection>()
            .init_resource::<LandDrag>()
            .init_resource::<LandHover>()
            .init_resource::<PendingLandConfirm>()
            .add_systems(
                Update,
                (
                    sync_land_state_with_settings,
                    sync_land_action_from_radio,
                    sync_radio_from_land_action,
                    apply_parcel_reply,
                    handle_land_pointer.after(crate::gizmos::drive_gizmo_interaction),
                    apply_land_confirmations,
                    sync_land_panel,
                )
                    .chain()
                    .run_if(crate::edit_tool::edit_tool_active_or_settling),
            );
        // The in-world outlines are immediate-mode gizmo lines — the right tool
        // for geometry that changes every frame a drag moves, and what the
        // reference draws in its tool's `render()`. They need Bevy's gizmo
        // pipeline, which `DefaultPlugins` brings but a headless fixture world
        // (no render app) does not, and `Gizmos` is a system param whose
        // `Res<GizmoConfigStore>` would fail parameter validation there and take
        // the whole schedule down with it.
        //
        // Gated on the store existing rather than on `GizmoPlugin` having been
        // added, because a run condition is re-checked every frame: a build-time
        // plugin check would silently and permanently drop the overlay if this
        // group were ever added before `DefaultPlugins`.
        app.add_systems(
            Update,
            draw_land_overlay
                .run_if(resource_exists::<GizmoConfigStore>)
                .run_if(crate::edit_tool::edit_tool_active_or_settling),
        );
    }
}

// ---------------------------------------------------------------------------
// State ↔ settings.
// ---------------------------------------------------------------------------

/// Keep [`LandToolState`] and the persisted settings in step, in both
/// directions.
///
/// The store is loaded (and its account layer re-loaded at login) long after
/// this resource is created, so the seed cannot happen in `FromWorld`; it runs
/// whenever the store changes. The two sliders are bound straight to the store
/// by the binding layer, so a slider drag arrives here as a store change — which
/// is also why the store wins when both moved in one frame.
fn sync_land_state_with_settings(
    settings: Option<ResMut<ViewerSettings>>,
    mut state: ResMut<LandToolState>,
) {
    let Some(mut settings) = settings else {
        return;
    };
    if settings.is_changed() {
        let store = settings.store();
        let index = store
            .get_i32(SETTING_LAND_BRUSH_ACTION)
            .ok()
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or(0);
        let action = LAND_ACTIONS.get(index).copied().unwrap_or_default();
        let radius = LandBrushRadius::new(
            store
                .get_f32(SETTING_LAND_BRUSH_SIZE)
                .unwrap_or(RADIUS_DEFAULT),
        );
        let force = store
            .get_f32(SETTING_LAND_BRUSH_FORCE)
            .unwrap_or(FORCE_DEFAULT)
            .clamp(FORCE_MIN, FORCE_MAX);
        if state.action != action
            || state.radius != radius
            || state.force.to_bits() != force.to_bits()
        {
            state.action = action;
            state.radius = radius;
            state.force = force;
        }
        return;
    }
    // The radio has no settings binding of its own (the binding layer covers
    // checkboxes, sliders and combos), so the action is written back by hand.
    if state.is_changed() {
        let index = i32::try_from(state.action.radio_index()).unwrap_or(0);
        settings.set(
            Scope::Global,
            SETTING_LAND_BRUSH_ACTION,
            SettingValue::I32(index),
        );
    }
}

/// Adopt the action the user picked in the panel's radio.
fn sync_land_action_from_radio(
    radios: Query<Ref<RadioSelection>, (With<LandActionRadio>, Changed<RadioSelection>)>,
    mut state: ResMut<LandToolState>,
) {
    for selection in &radios {
        if selection.is_added() {
            continue;
        }
        if let Some(&action) = LAND_ACTIONS.get(selection.active)
            && state.action != action
        {
            state.action = action;
        }
    }
}

/// Mirror the action back onto the radio when it changed from somewhere else
/// (the persisted seed, a test), including onto a newly-spawned group whose
/// content was built a frame after the state settled.
fn sync_radio_from_land_action(
    state: Res<LandToolState>,
    mut radios: Query<&mut RadioSelection, With<LandActionRadio>>,
) {
    let state_changed = state.is_changed();
    let index = state.action.radio_index();
    for mut selection in &mut radios {
        if !(state_changed || selection.is_added()) {
            continue;
        }
        if selection.active != index {
            selection.active = index;
        }
    }
}

// ---------------------------------------------------------------------------
// The ground gestures.
// ---------------------------------------------------------------------------

/// The pointer / camera inputs the land gestures read, bundled as one
/// [`SystemParam`] to stay inside Bevy's system-parameter limit (the
/// [`crate::edit_create`] pattern).
#[derive(SystemParam)]
struct LandPointer<'w, 's> {
    /// The mouse buttons.
    buttons: Res<'w, ButtonInput<MouseButton>>,
    /// The keyboard, for the `Alt` (camera) modifier.
    keyboard: Res<'w, ButtonInput<KeyCode>>,
    /// The `bevy_ui` hover map, for the UI-occlusion guard.
    hover_map: Res<'w, HoverMap>,
    /// Pickability, for the UI-occlusion guard.
    pickables: Query<'w, 's, &'static Pickable>,
    /// Node sizes, for the UI-occlusion guard.
    node_sizes: Query<'w, 's, &'static ComputedNode>,
    /// A widget's claim on this frame's pointer.
    ui_claim: Res<'w, UiPointerClaim>,
    /// The window the cursor is in.
    windows: Query<'w, 's, &'static Window>,
    /// The world camera the ground ray is cast from.
    camera: Query<'w, 's, (&'static Camera, &'static GlobalTransform), With<ViewerCamera>>,
    /// Render layers, to exclude HUD / gizmo geometry from the ground pick.
    layers: Query<'w, 's, (Entity, &'static RenderLayers)>,
}

/// The scene queries the ground pick walks to tell terrain from an object.
#[derive(SystemParam)]
struct LandScene<'w, 's> {
    /// The ray caster.
    ray_cast: MeshRayCast<'w, 's>,
    /// Scene objects, so a hit on a prim or an avatar is not ground.
    scene: Query<'w, 's, &'static SceneObject>,
    /// Parents, to walk a hit face up to its object.
    parents: Query<'w, 's, &'static ChildOf>,
}

/// The region the land tool works in, and how wide it is.
///
/// Both gestures and the overlay need the pair, and neither is worth a resource:
/// the handle is the agent's current region and the width comes from that
/// region's decoded parcel overlay — the same source the property lines size
/// themselves from.
#[derive(SystemParam)]
struct LandRegion<'w> {
    /// The session identity, for the agent's current region handle.
    identity: Res<'w, SlIdentity>,
    /// The decoded parcel overlay, for the region's width.
    overlay: Res<'w, SlParcelOverlay>,
}

impl LandRegion<'_> {
    /// The agent's current region handle, or `None` before the circuit is up.
    fn handle(&self) -> Option<RegionHandle> {
        self.identity.region_handle
    }

    /// The current region's width in metres, from its overlay grid — 256 m until
    /// the grid arrives, which is right for every region but a var-region and is
    /// only used to clamp a selection to the region's edge.
    fn width_metres(&self) -> f32 {
        self.handle()
            .and_then(|region| self.overlay.grid_of(region))
            .map_or(DEFAULT_REGION_WIDTH_METRES, |grid| {
                // Via `u16` rather than a cast: `grids_per_edge` is 64 for a
                // region and a small multiple of that for a var-region, so the
                // widening is exact.
                f32::from(u16::try_from(grid.grids_per_edge()).unwrap_or(u16::MAX))
                    * PARCEL_GRID_STEP_METRES
            })
    }
}

/// The land pointer gesture: the rectangle drag-select and the brush stroke.
///
/// Both start from the same press on bare ground; which one runs is the picked
/// [`LandAction`], exactly as the reference switches between `LLToolSelectLand`
/// and `LLToolBrushLand` off its `land_radio_group`.
#[expect(
    clippy::too_many_arguments,
    reason = "the inputs are already three SystemParam bundles plus the three resources \
              this gesture alone owns and the command outbox; bundling further would \
              only hide which of them it writes"
)]
fn handle_land_pointer(
    tool: Res<EditToolState>,
    land: Res<LandToolState>,
    gizmo: Res<GizmoInteraction>,
    pointer: LandPointer,
    mut scene: LandScene,
    region: LandRegion,
    terrain: Res<TerrainState>,
    time: Res<Time>,
    mut drag: ResMut<LandDrag>,
    mut hover: ResMut<LandHover>,
    mut selection: ResMut<LandSelection>,
    mut commands: MessageWriter<SlCommand>,
) {
    // Leaving the Land tool cancels any live gesture and drops the hover, so
    // neither the half-drawn rectangle nor the bulldozer footprint is still on
    // the ground when the tool comes back.
    if !tool.active || tool.tool != EditTool::SelectLand {
        if drag.start.take().is_some() {
            drag.current = None;
            drag.brushing = false;
        }
        if hover.0.is_some() {
            hover.0 = None;
        }
        return;
    }
    let Some(handle) = region.handle() else {
        return;
    };
    let ground = ground_under_cursor(&pointer, &mut scene);
    hover.0 = ground;

    // A press over UI, on a gizmo handle, or with `Alt` held (the camera drag) is
    // never a land gesture.
    let alt =
        pointer.keyboard.pressed(KeyCode::AltLeft) || pointer.keyboard.pressed(KeyCode::AltRight);
    let over_ui =
        pointer_over_blocking_ui(&pointer.hover_map, &pointer.pickables, &pointer.node_sizes);
    let blocked = alt || over_ui || gizmo.claims_pointer() || pointer.ui_claim.is_claimed();

    if pointer.buttons.just_pressed(MouseButton::Left)
        && !blocked
        && let Some(point) = ground
    {
        drag.start = Some(point);
        drag.current = Some(point);
        drag.brushing = land.action.brush().is_some();
        drag.start_height = terrain.land_height(handle, point.x, point.y).unwrap_or(0.0);
    }

    if drag.start.is_some() && pointer.buttons.pressed(MouseButton::Left) {
        if let Some(point) = ground {
            drag.current = Some(point);
        }
        // A brush strokes continuously while the button is held: the reference
        // registers an idle callback on mouse-down and sends one `ModifyLand` per
        // frame at the rounded cursor point.
        if drag.brushing
            && let Some(brush) = land.action.brush()
            && let Some(point) = drag.current
        {
            let delta = time.delta_secs();
            let fps = if delta > 0.0 {
                1.0 / delta
            } else {
                FALLBACK_FPS
            };
            commands.write(SlCommand(Command::ModifyLand(LandEdit {
                action: brush,
                brush_radius: land.radius,
                strength: land.force / fps,
                height: drag.start_height,
                // A free brush stroke, not a whole-parcel edit: the reference
                // sends `LocalID = -1` here even with a parcel selected.
                parcel: None,
                area: TerraformArea::point(point.x.round(), point.y.round()),
            })));
        }
    }

    if pointer.buttons.just_released(MouseButton::Left) {
        let start = drag.start.take();
        let end = drag.current.take();
        if std::mem::take(&mut drag.brushing) {
            return;
        }
        let (Some(start), Some(end)) = (start, end) else {
            return;
        };
        // A press and release on the same spot is a *click*, which the reference
        // answers by snapping the selection out to the whole parcel; a real drag
        // keeps the rectangle it drew.
        let click = start.abs_diff_eq(end, f32::EPSILON);
        let rect =
            LandRect::snapped_from_corners(start, end).clamped_to_region(region.width_metres());
        if rect.is_empty() {
            selection.clear();
            return;
        }
        let sequence_id = selection.next_sequence();
        selection.rect = Some(rect);
        selection.parcel = None;
        selection.whole_parcel = false;
        selection.multiple_owners = false;
        selection.pending = Some(sequence_id);
        commands.write(SlCommand(Command::RequestParcelProperties {
            west: rect.west,
            south: rect.south,
            east: rect.east,
            north: rect.north,
            sequence_id,
            snap_selection: click,
        }));
    }
}

/// The region-local ground point under the cursor, or `None` when the cursor is
/// not over bare terrain.
fn ground_under_cursor(pointer: &LandPointer, scene: &mut LandScene) -> Option<Vec2> {
    let window = pointer.windows.single().ok()?;
    let (camera, camera_transform) = pointer.camera.single().ok()?;
    let cursor = window.cursor_position()?;
    let ray = camera.viewport_to_world(camera_transform, cursor).ok()?;
    // Exclude HUD and gizmo geometry exactly as selection and the create placer
    // do, so an overlay never stands in for the ground.
    let exclude: HashSet<Entity> = pointer
        .layers
        .iter()
        .filter(|(_entity, layers)| on_hud_layer(Some(layers)) || on_gizmo_layer(Some(layers)))
        .map(|(entity, _layers)| entity)
        .collect();
    let world_filter = |entity: Entity| !exclude.contains(&entity);
    let settings = MeshRayCastSettings::default().with_filter(&world_filter);
    let (entity, hit) = scene.ray_cast.cast_ray(ray, &settings).first().cloned()?;
    if !is_bare_terrain(entity, scene) {
        return None;
    }
    // The current region's terrain sits at the scene origin, so the Bevy world
    // point converts straight to region-local metres (as the land pie's About
    // Land slice does).
    let local = bevy_to_sl_vec(hit.point);
    Some(Vec2::new(local.x, local.y))
}

/// Whether a ray hit is bare terrain rather than a prim or an avatar: no scene
/// object anywhere in its ancestry (the create placer's `classify_hit` rule).
fn is_bare_terrain(entity: Entity, scene: &LandScene) -> bool {
    let mut current = entity;
    loop {
        if scene.scene.get(current).is_ok() {
            return false;
        }
        match scene.parents.get(current) {
            Ok(parent) => current = parent.parent(),
            Err(_not_parented) => return true,
        }
    }
}

// ---------------------------------------------------------------------------
// The parcel reply.
// ---------------------------------------------------------------------------

/// Fold the `ParcelProperties` answering the selection's request into it.
///
/// A reply whose sequence id is not the one this selection asked for belongs to
/// another window's question (About Land's, the agent-parcel probe's) or to a
/// superseded selection, and is ignored. A `snap_selection` reply replaces the
/// drawn rectangle with the parcel's own bounds, which is what makes a click
/// select a whole parcel — the reference's `processParcelProperties` branch.
fn apply_parcel_reply(mut events: MessageReader<SlEvent>, mut selection: ResMut<LandSelection>) {
    for event in events.read() {
        let SlSessionEvent::ParcelProperties(parcel) = &event.0 else {
            continue;
        };
        if selection.pending != Some(parcel.sequence_id) {
            continue;
        }
        selection.pending = None;
        selection.multiple_owners = matches!(parcel.request_result, ParcelRequestResult::Multiple);
        // Public land (local id 0) has no parcel to snap to, so the drawn
        // rectangle stands even for a click — as it does in the reference.
        if parcel.snap_selection && parcel.local_id != RegionLocalParcelId(0) {
            selection.rect = Some(LandRect::new(
                parcel.aabb_min.x(),
                parcel.aabb_min.y(),
                parcel.aabb_max.x(),
                parcel.aabb_max.y(),
            ));
            selection.whole_parcel = true;
        } else {
            selection.whole_parcel = false;
        }
        selection.parcel = Some((**parcel).clone());
    }
}

// ---------------------------------------------------------------------------
// The in-world overlay.
// ---------------------------------------------------------------------------

/// The five stores the in-world overlay reads, bundled as one [`SystemParam`]:
/// which tool is up, what brush it holds, what is selected, what is being
/// dragged, and where the cursor is on the ground.
#[derive(SystemParam)]
struct LandOverlayState<'w> {
    /// Which build tool is active.
    tool: Res<'w, EditToolState>,
    /// The picked land action and bulldozer radius.
    land: Res<'w, LandToolState>,
    /// The committed land selection.
    selection: Res<'w, LandSelection>,
    /// The rectangle being dragged out, if any.
    drag: Res<'w, LandDrag>,
    /// The ground point under the cursor, for the bulldozer footprint.
    hover: Res<'w, LandHover>,
}

/// Draw the land selection, the rectangle being dragged, and the bulldozer
/// footprint under the cursor.
///
/// Immediate-mode gizmo lines rather than a mesh: all three follow the terrain
/// and change every frame a drag moves, so a retained mesh would be rebuilt each
/// frame anyway — the reference likewise draws them in its tool's `render`.
fn draw_land_overlay(
    what: LandOverlayState,
    region: LandRegion,
    terrain: Res<TerrainState>,
    mut gizmos: Gizmos,
) {
    let LandOverlayState {
        tool,
        land,
        selection,
        drag,
        hover,
    } = what;
    if !tool.active || tool.tool != EditTool::SelectLand {
        return;
    }
    let Some(handle) = region.handle() else {
        return;
    };
    if let Some(rect) = selection.rect {
        draw_rect_outline(&mut gizmos, &terrain, handle, rect, SELECTION_COLOR);
    }
    // The live drag, drawn snapped, so what is highlighted is exactly what a
    // release would select.
    if !drag.brushing
        && let (Some(start), Some(end)) = (drag.start, drag.current)
    {
        let rect =
            LandRect::snapped_from_corners(start, end).clamped_to_region(region.width_metres());
        draw_rect_outline(&mut gizmos, &terrain, handle, rect, DRAG_COLOR);
    }
    // The bulldozer footprint: what the next stroke would move.
    if land.action.brush().is_some()
        && let Some(point) = hover.0
    {
        let radius = land.radius.to_metres();
        let rect = LandRect::new(
            point.x - radius,
            point.y - radius,
            point.x + radius,
            point.y + radius,
        );
        draw_rect_outline(&mut gizmos, &terrain, handle, rect, BRUSH_COLOR);
    }
}

/// Draw one ground-hugging rectangle outline, sampling the terrain along each
/// edge so it drapes over undulations.
fn draw_rect_outline(
    gizmos: &mut Gizmos,
    terrain: &TerrainState,
    region: RegionHandle,
    rect: LandRect,
    color: Color,
) {
    let corners = [
        Vec2::new(rect.west, rect.south),
        Vec2::new(rect.east, rect.south),
        Vec2::new(rect.east, rect.north),
        Vec2::new(rect.west, rect.north),
    ];
    // Each corner paired with the one after it, wrapping — an iterator rather
    // than an index, so the walk cannot run off the end.
    for (&from, &to) in corners
        .iter()
        .zip(corners.iter().cycle().skip(1))
        .take(corners.len())
    {
        let steps = outline_steps(from.distance(to));
        let mut previous = draped_point(terrain, region, from);
        // Via `u16` rather than a cast: both are capped at `OUTLINE_MAX_STEPS`,
        // so the widening is exact.
        let total = f32::from(u16::try_from(steps).unwrap_or(u16::MAX));
        for step in 1..=steps {
            let fraction = f32::from(u16::try_from(step).unwrap_or(u16::MAX)) / total;
            let next = draped_point(terrain, region, from.lerp(to, fraction));
            gizmos.line(previous, next, color);
            previous = next;
        }
    }
}

/// How many segments an outline edge `length` metres long is drawn with: one per
/// sample step, at least one, and never more than [`OUTLINE_MAX_STEPS`].
fn outline_steps(length: f32) -> usize {
    let wanted = (length / OUTLINE_SAMPLE_METRES).ceil();
    if !wanted.is_finite() || wanted < 1.0 {
        return 1;
    }
    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "guarded finite and >= 1 above, and clamped to OUTLINE_MAX_STEPS below"
    )]
    let steps = wanted as usize;
    steps.min(OUTLINE_MAX_STEPS)
}

/// One outline vertex in Bevy world space: the region-local ground point lifted
/// clear of the terrain it sits on.
fn draped_point(terrain: &TerrainState, region: RegionHandle, point: Vec2) -> Vec3 {
    let height = terrain.land_height(region, point.x, point.y).unwrap_or(0.0);
    sl_to_bevy_vec(&Vector {
        x: point.x,
        y: point.y,
        z: height + OUTLINE_LIFT_METRES,
    })
}

// ---------------------------------------------------------------------------
// The panel.
// ---------------------------------------------------------------------------

/// Spawn the Land panel under the Build Tools floater's content column, hidden
/// until the Land tool is active — the sibling of
/// [`crate::edit_create::spawn_create_panel`].
pub(crate) fn spawn_land_panel(commands: &mut Commands, parent: Entity) {
    let panel = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                ..column(Val::Px(6.0))
            },
            UiPanelShown(false),
            Name::new("build-land:panel"),
            ChildOf(parent),
        ))
        .id();

    // The action radio: Select Land plus the six brushes, in the reference's
    // `land_radio_group` order. A column, not a row: seven captions do not fit
    // across the floater at any usable width.
    let labels: Vec<String> = LAND_ACTION_KEYS
        .iter()
        .map(|key| (*key).to_owned())
        .collect();
    let radio = spawn_radio_group(
        commands,
        panel,
        &RadioSpec {
            element: "build-land-action",
            labels: &labels,
            active: 0,
            tab_index: 40,
            font_size: TOOL_FONT_SIZE,
            layout: RadioLayout::Column,
            translate_labels: true,
        },
    );
    commands.entity(radio).insert(LandActionRadio);

    // The bulldozer's two sliders, bound straight to the persisted settings.
    spawn_land_slider(
        commands,
        panel,
        "build-land-size-label",
        SETTING_LAND_BRUSH_SIZE,
        LandBrushRadius::MIN_METRES,
        LandBrushRadius::MAX_METRES,
        41,
    );
    spawn_land_slider(
        commands,
        panel,
        "build-land-strength-label",
        SETTING_LAND_BRUSH_FORCE,
        FORCE_MIN,
        FORCE_MAX,
        42,
    );

    // Apply: run the picked brush over the whole selection in one stroke.
    let apply = spawn_land_button(commands, panel, LandButton::Apply, "build-land-apply", 43);

    // The parcel read-out and the parcel actions (the reference's
    // `land info panel`).
    let summary = commands
        .spawn((
            Text::default(),
            UiFont::Sans.at(TOOL_FONT_SIZE),
            // A skinless fallback; the skin recolours via the class token.
            TextColor(Color::srgba(0.85, 0.85, 0.85, 1.0)),
            ClassList::new_with_classes([LABEL_CLASS]),
            Name::new("build-land:summary"),
            ChildOf(panel),
        ))
        .id();
    let about_land = spawn_land_button(
        commands,
        panel,
        LandButton::AboutLand,
        "build-land-about",
        44,
    );
    let subdivide = spawn_land_button(
        commands,
        panel,
        LandButton::Subdivide,
        "build-land-subdivide",
        45,
    );
    let join = spawn_land_button(commands, panel, LandButton::Join, "build-land-join", 46);

    // Show owners: the in-world ownership tint, bound to the setting the terrain
    // overlay reads.
    let owners_glyph = spawn_owners_checkbox(commands, panel, 47);

    commands.insert_resource(LandPanelUi {
        panel,
        summary,
        apply,
        about_land,
        subdivide,
        join,
        owners_glyph,
    });
}

/// Spawn one labelled slider bound to a persisted land setting.
fn spawn_land_slider(
    commands: &mut Commands,
    parent: Entity,
    label_key: &'static str,
    setting: &'static str,
    min: f32,
    max: f32,
    tab_index: i32,
) {
    let row_entity = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                ..row(Val::Px(6.0))
            },
            Name::new(format!("build-land:{setting}-row")),
            ChildOf(parent),
        ))
        .id();
    spawn_row_label(commands, row_entity, label_key);
    // A hundred steps across the travel: fine enough that the thumb reads as
    // continuous, coarse enough that a drag does not write the setting (and so
    // re-seed the state) on every sub-pixel move.
    let step = (max - min) / 100.0;
    let track = spawn_slider(
        commands,
        row_entity,
        LAND_SLIDER,
        tab_index,
        0.0,
        bound_slider(
            SettingBinding::global(setting),
            SliderRange::new(min, max),
            SliderStep(step),
        ),
    );
    commands
        .entity(track)
        .insert(Name::new(format!("build-land:{setting}")));
}

/// Spawn one Land-panel action button, returning its **caption** — the node the
/// greyed-out skin class goes on.
fn spawn_land_button(
    commands: &mut Commands,
    parent: Entity,
    action: LandButton,
    label_key: &'static str,
    tab_index: i32,
) -> Entity {
    let spawned = spawn_button(
        commands,
        parent,
        ButtonSpec::bordered(
            UiLabel::key(label_key),
            format!("build-land-button:{label_key}"),
        )
        .kind(ButtonKind::Headless)
        .tab_index(tab_index)
        .padding(10.0, 2.0)
        .colors(
            Color::srgba(0.18, 0.18, 0.2, 1.0),
            Color::srgba(0.4, 0.4, 0.45, 1.0),
        )
        // A skinless fallback; the skin recolours via the class token.
        .label_color(Color::WHITE)
        .font_size(TOOL_FONT_SIZE)
        .label_class(VALUE_CLASS),
    );
    commands
        .entity(spawned.button)
        .insert(action)
        .observe(handle_land_action_press);
    spawned.label
}

/// Spawn the **Show owners** checkbox, bound to the setting the in-world
/// ownership tint reads. Returns its glyph node, which the sync pass rewrites.
fn spawn_owners_checkbox(commands: &mut Commands, parent: Entity, tab_index: i32) -> Entity {
    let row_entity = commands
        .spawn((
            bound_checkbox(SettingBinding::global(SETTING_SHOW_PARCEL_OWNERS)),
            bevy::input_focus::tab_navigation::TabIndex(tab_index),
            Node {
                align_items: AlignItems::Center,
                ..row(Val::Px(6.0))
            },
            Pickable::default(),
            Name::new("build-land:show-owners"),
            ChildOf(parent),
        ))
        .id();
    let glyph = commands
        .spawn((
            Text::new(UNCHECKED_GLYPH),
            UiFont::Sans.at(TOOL_FONT_SIZE),
            // A skinless fallback; the skin recolours via the class token.
            TextColor(Color::WHITE),
            ClassList::new_with_classes([VALUE_CLASS]),
            Pickable::IGNORE,
            ChildOf(row_entity),
        ))
        .id();
    commands.spawn((
        Text::default(),
        Translated::new("build-land-show-owners"),
        UiFont::Sans.at(TOOL_FONT_SIZE),
        // A skinless fallback; the skin recolours via the class token.
        TextColor(Color::srgba(0.85, 0.85, 0.85, 1.0)),
        ClassList::new_with_classes([LABEL_CLASS]),
        Pickable::IGNORE,
        ChildOf(row_entity),
    ));
    glyph
}

/// The panel's write targets, bundled so the sync pass stays inside Bevy's
/// system-parameter limit.
#[derive(SystemParam)]
struct LandPanelNodes<'w, 's> {
    /// The panel gates (the panel root's [`UiPanelShown`]).
    panels: Query<'w, 's, &'static mut UiPanelShown>,
    /// The read-out and glyph texts.
    texts: Query<'w, 's, &'static mut Text>,
    /// The buttons' skin classes, for the greyed-out state.
    classes: Query<'w, 's, &'static mut ClassList>,
}

/// Show the Land panel while the Land tool is active, and keep the parcel
/// read-out and the four buttons in step with the selection — the reference's
/// `LLPanelLandInfo::refresh`.
fn sync_land_panel(
    tool: Res<EditToolState>,
    land: Res<LandToolState>,
    selection: Res<LandSelection>,
    settings: Option<Res<ViewerSettings>>,
    translator: Translator,
    ui: Option<Res<LandPanelUi>>,
    mut nodes: LandPanelNodes,
) {
    let Some(ui) = ui else {
        return;
    };
    let settings_changed = settings
        .as_ref()
        .is_some_and(|settings| settings.is_changed());
    let editing_land = tool.active && tool.tool == EditTool::SelectLand;
    if let Ok(mut shown) = nodes.panels.get_mut(ui.panel)
        && shown.0 != editing_land
    {
        shown.0 = editing_land;
    }
    // The per-aspect tabs this panel stands in for are hidden by
    // [`crate::edit_tool::sync_tab_visibility`], the one place that decides.
    if !editing_land {
        return;
    }
    // Everything below writes text and skin classes, both of which mark their
    // node changed on the write. Only redraw when something it reads has
    // actually moved — or when the panel itself has only just been built, whose
    // first sync has nothing else to key off.
    let stale = ui.is_added()
        || tool.is_changed()
        || land.is_changed()
        || selection.is_changed()
        || settings_changed;
    if !stale {
        return;
    }
    // The read-out: the selected area, and the parcel's name once it is known.
    let want = if selection.rect.is_some() {
        let area = i64::from(area_m2(selection.area()));
        let mut line = translator.format(
            "build-land-selection-area",
            &TransArgs::new().int("area", area),
        );
        if let Some(parcel) = selection.parcel.as_ref() {
            line.push_str(" — ");
            line.push_str(&parcel.name);
        }
        line
    } else {
        translator.get("build-land-selection-none")
    };
    set_text(&mut nodes.texts, ui.summary, &want);

    // The four buttons, greyed exactly as the reference greys them.
    let can_apply = selection.rect.is_some() && land.action.brush().is_some();
    set_enabled(&mut nodes.classes, ui.apply, can_apply);
    set_enabled(
        &mut nodes.classes,
        ui.about_land,
        selection.parcel.is_some(),
    );
    set_enabled(&mut nodes.classes, ui.subdivide, can_divide(&selection));
    set_enabled(&mut nodes.classes, ui.join, can_join(&selection));

    // The checkbox glyph follows the bound setting, wherever it was changed
    // (this panel, the World menu, the debug-settings editor).
    let owners = settings.is_some_and(|settings| {
        settings
            .store()
            .get_bool(SETTING_SHOW_PARCEL_OWNERS)
            .unwrap_or(false)
    });
    set_text(
        &mut nodes.texts,
        ui.owners_glyph,
        if owners {
            CHECKED_GLYPH
        } else {
            UNCHECKED_GLYPH
        },
    );
}

/// A selected area as whole square metres, for the read-out.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "clamped into i32's range on both sides before the conversion"
)]
const fn area_m2(area: f32) -> i32 {
    area.round().clamp(0.0, 2_147_483_000.0) as i32
}

/// Write a node's text only on a real change.
fn set_text(texts: &mut Query<&mut Text>, entity: Entity, want: &str) {
    if let Ok(mut text) = texts.get_mut(entity)
        && text.0 != want
    {
        want.clone_into(&mut text.0);
    }
}

/// Grey a button out when its action is unavailable.
///
/// The button keeps answering presses: [`handle_land_action_press`] runs the same
/// gate and raises the reference's refusal notification, so a user who clicks
/// anyway is told *why* rather than nothing happening.
fn set_enabled(classes: &mut Query<&mut ClassList>, button: Entity, enabled: bool) {
    if let Ok(mut class_list) = classes.get_mut(button) {
        set_disabled_class(&mut class_list, !enabled);
    }
}

/// Whether the selection can be subdivided: something is selected, and it is a
/// *piece* of a parcel rather than a whole one (the reference's
/// `startDivideLand` refusals).
const fn can_divide(selection: &LandSelection) -> bool {
    selection.rect.is_some() && !selection.whole_parcel
}

/// Whether the selection can be joined: something bigger than a single grid
/// square is selected, it is not one whole parcel, and it spans more than one
/// owner (the reference's `startJoinLand` refusals plus `LLPanelLandInfo`'s area
/// gate).
fn can_join(selection: &LandSelection) -> bool {
    selection
        .rect
        .is_some_and(|rect| rect.area() > PARCEL_UNIT_AREA)
        && !selection.whole_parcel
        && selection.multiple_owners
}

// ---------------------------------------------------------------------------
// The parcel actions.
// ---------------------------------------------------------------------------

/// The sinks the four Land-panel buttons write between them.
#[derive(SystemParam)]
struct LandActionSinks<'w> {
    /// The rectangle a raised confirmation is waiting on.
    pending: ResMut<'w, PendingLandConfirm>,
    /// Refusals and confirmations.
    notify: MessageWriter<'w, ShowNotification>,
    /// The About Land open request.
    about_land: MessageWriter<'w, OpenAboutLand>,
    /// The protocol command outbox.
    commands: MessageWriter<'w, SlCommand>,
}

/// The observer every Land-panel button runs on press.
fn handle_land_action_press(
    press: On<Pointer<Press>>,
    buttons: Query<&LandButton>,
    land: Res<LandToolState>,
    selection: Res<LandSelection>,
    region: LandRegion,
    terrain: Res<TerrainState>,
    mut sinks: LandActionSinks,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let Ok(&button) = buttons.get(press.entity) else {
        return;
    };
    match button {
        LandButton::Apply => {
            if let Some(handle) = region.handle() {
                apply_brush_to_selection(&land, &selection, &terrain, handle, &mut sinks.commands);
            }
        }
        LandButton::AboutLand => {
            if let Some(parcel) = selection.parcel.as_ref() {
                sinks.about_land.write(OpenAboutLand {
                    subject: AboutLandSubject::CurrentParcel(parcel.local_id),
                    read_only: false,
                });
            }
        }
        LandButton::Subdivide => {
            // The reference's `startDivideLand`, refusal for refusal.
            let Some(rect) = selection.rect else {
                sinks
                    .notify
                    .write(ShowNotification::new("CannotDivideLandNothingSelected"));
                return;
            };
            if selection.whole_parcel {
                sinks
                    .notify
                    .write(ShowNotification::new("CannotDivideLandPartialSelection"));
                return;
            }
            sinks.pending.divide = Some(rect);
            sinks
                .notify
                .write(ShowNotification::new("LandDivideWarning"));
        }
        LandButton::Join => {
            // The reference's `startJoinLand`, refusal for refusal.
            let Some(rect) = selection.rect else {
                sinks
                    .notify
                    .write(ShowNotification::new("CannotJoinLandNothingSelected"));
                return;
            };
            if selection.whole_parcel {
                sinks
                    .notify
                    .write(ShowNotification::new("CannotJoinLandEntireParcelSelected"));
                return;
            }
            if !selection.multiple_owners {
                sinks
                    .notify
                    .write(ShowNotification::new("CannotJoinLandSelection"));
                return;
            }
            sinks.pending.join = Some(rect);
            sinks.notify.write(ShowNotification::new("JoinLandWarning"));
        }
    }
}

/// Run the picked brush over the whole land selection in one stroke — the
/// reference's `modifyLandInSelectionGlobal`, including its per-action strength
/// scaling and its fixed half-second revert.
fn apply_brush_to_selection(
    land: &LandToolState,
    selection: &LandSelection,
    terrain: &TerrainState,
    region: RegionHandle,
    commands: &mut MessageWriter<SlCommand>,
) {
    let (Some(rect), Some(brush)) = (selection.rect, land.action.brush()) else {
        return;
    };
    let strength = if brush == LandBrushAction::Revert {
        APPLY_REVERT_SECONDS
    } else {
        land.force
            * APPLY_SCALE
                .get(usize::from(brush.to_code()))
                .copied()
                .unwrap_or(1.0)
    };
    // The reference levels toward the ground height at the selection's centre.
    let centre_x = f32::midpoint(rect.west, rect.east);
    let centre_y = f32::midpoint(rect.south, rect.north);
    let height = terrain
        .land_height(region, centre_x, centre_y)
        .unwrap_or(0.0);
    commands.write(SlCommand(Command::ModifyLand(LandEdit {
        action: brush,
        brush_radius: land.radius,
        strength,
        height,
        // Only a *whole-parcel* selection names the parcel; a drawn rectangle
        // stays a free edit (`LocalID = -1`), as the reference sends it.
        parcel: selection
            .whole_parcel
            .then(|| selection.parcel.as_ref().map(|parcel| parcel.local_id))
            .flatten(),
        area: rect.to_terraform_area(),
    })));
}

/// Send the `ParcelDivide` / `ParcelJoin` once the user answers the warning,
/// against the rectangle the warning was raised for.
fn apply_land_confirmations(
    mut responses: MessageReader<NotificationResponse>,
    mut pending: ResMut<PendingLandConfirm>,
    mut commands: MessageWriter<SlCommand>,
) {
    for response in responses.read() {
        let divide = match response.template {
            "LandDivideWarning" => true,
            "JoinLandWarning" => false,
            _other => continue,
        };
        let rect = if divide {
            pending.divide.take()
        } else {
            pending.join.take()
        };
        // Any answer but OK — Cancel, or a dismissal — drops the pending
        // rectangle and sends nothing.
        let (Some(rect), Some("OK")) = (rect, response.button) else {
            continue;
        };
        let command = if divide {
            Command::DivideParcel {
                west: rect.west,
                south: rect.south,
                east: rect.east,
                north: rect.north,
            }
        } else {
            Command::JoinParcels {
                west: rect.west,
                south: rect.south,
                east: rect.east,
                north: rect.north,
            }
        };
        commands.write(SlCommand(command));
    }
}

#[cfg(test)]
mod tests {
    use super::{
        APPLY_SCALE, LAND_ACTIONS, LandAction, LandRect, LandSelection, LandToolState,
        PARCEL_UNIT_AREA, can_divide, can_join, outline_steps,
    };
    use bevy::prelude::Vec2;
    use pretty_assertions::{assert_eq, assert_ne};
    use sl_client_bevy::LandBrushAction;

    /// A click — a press and release on the same ground point — selects the
    /// single 4 m parcel square under the cursor, not an empty rectangle.
    #[test]
    fn a_click_selects_one_grid_square() {
        let point = Vec2::new(70.0, 34.0);
        let rect = LandRect::snapped_from_corners(point, point);
        assert_eq!(rect.area().to_bits(), PARCEL_UNIT_AREA.to_bits());
        assert_eq!(rect.west.to_bits(), 68.0_f32.to_bits());
        assert_eq!(rect.east.to_bits(), 72.0_f32.to_bits());
        assert_eq!(rect.south.to_bits(), 32.0_f32.to_bits());
        assert_eq!(rect.north.to_bits(), 36.0_f32.to_bits());
    }

    /// A drag snaps **out** to the grid in both directions, whichever corner it
    /// started from, so the selection always covers every square it touched.
    #[test]
    fn a_drag_snaps_out_to_whole_grid_squares() {
        let expected = LandRect {
            west: 12.0,
            south: 20.0,
            east: 40.0,
            north: 52.0,
        };
        let a = Vec2::new(13.0, 21.0);
        let b = Vec2::new(39.0, 51.0);
        // The same rectangle dragged from each of its four corners.
        for (start, end) in [
            (a, b),
            (b, a),
            (Vec2::new(a.x, b.y), Vec2::new(b.x, a.y)),
            (Vec2::new(b.x, a.y), Vec2::new(a.x, b.y)),
        ] {
            assert_eq!(LandRect::snapped_from_corners(start, end), expected);
        }
    }

    /// The snap never leaves a selection outside the region it was drawn in — a
    /// land selection may not cross a region boundary — and an edge-hugging drag
    /// snaps to a **positive** zero, not the negative one the division through
    /// zero would otherwise produce.
    #[test]
    fn the_rect_clamps_to_the_region() {
        let rect = LandRect::snapped_from_corners(Vec2::new(1.0, 1.0), Vec2::new(255.0, 255.0))
            .clamped_to_region(256.0);
        assert!(
            rect.west.is_sign_positive() && rect.south.is_sign_positive(),
            "a west/south edge snaps to +0.0, not -0.0: {rect:?}"
        );
        assert_eq!(rect.west.to_bits(), 0.0_f32.to_bits());
        assert_eq!(rect.south.to_bits(), 0.0_f32.to_bits());
        assert_eq!(rect.east.to_bits(), 256.0_f32.to_bits());
        assert_eq!(rect.north.to_bits(), 256.0_f32.to_bits());
    }

    /// Every [`LandAction`] round-trips through its radio index, index 0 is the
    /// rectangle select, and each brush's wire code indexes [`APPLY_SCALE`].
    #[test]
    fn land_actions_round_trip_through_their_radio_index() {
        for (index, action) in LAND_ACTIONS.iter().enumerate() {
            assert_eq!(action.radio_index(), index);
            if let Some(brush) = action.brush() {
                assert!(
                    APPLY_SCALE.get(usize::from(brush.to_code())).is_some(),
                    "every brush's wire code indexes APPLY_SCALE",
                );
            }
        }
        assert_eq!(LAND_ACTIONS.first().copied(), Some(LandAction::Select));
        assert_eq!(LandAction::Select.brush(), None);
        assert_eq!(
            LandAction::Brush(LandBrushAction::Revert).brush(),
            Some(LandBrushAction::Revert)
        );
        assert_eq!(LandToolState::default().action, LandAction::Select);
    }

    /// Subdivide wants a **piece** of a parcel; Join wants more than one grid
    /// square, spanning more than one owner, and not a whole parcel — the
    /// reference's two gates, which on a whole-parcel selection both refuse.
    #[test]
    fn the_divide_and_join_gates_match_the_reference() {
        let mut selection = LandSelection::default();
        assert!(!can_divide(&selection), "nothing selected divides nothing");
        assert!(!can_join(&selection), "nothing selected joins nothing");

        // One grid square of somebody's parcel: divisible, too small to join.
        selection.rect = Some(LandRect::new(0.0, 0.0, 4.0, 4.0));
        assert!(can_divide(&selection));
        assert!(!can_join(&selection));

        // A bigger piece, but all one owner: still not joinable.
        selection.rect = Some(LandRect::new(0.0, 0.0, 16.0, 16.0));
        assert!(!can_join(&selection));

        // Spanning two owners: joinable.
        selection.multiple_owners = true;
        assert!(can_join(&selection));

        // Snapped out to a whole parcel: neither.
        selection.whole_parcel = true;
        assert!(!can_divide(&selection));
        assert!(!can_join(&selection));
    }

    /// Each land selection asks with its own sequence id, in the negative range
    /// the reference reserves for the selection — so a late reply to a
    /// superseded drag cannot be mistaken for the current one.
    #[test]
    fn each_selection_takes_its_own_sequence_id() {
        let mut selection = LandSelection::default();
        let first = selection.next_sequence();
        let second = selection.next_sequence();
        assert_eq!(first, super::LAND_SELECTION_SEQ_ID);
        assert_ne!(first, second);
        assert!(second < first, "the ids count away from About Land's");
    }

    /// The outline samples often enough to drape, and cannot be asked for an
    /// unbounded number of segments.
    #[test]
    fn the_outline_step_count_is_bounded() {
        assert_eq!(outline_steps(0.0), 1);
        assert_eq!(outline_steps(4.0), 2);
        assert_eq!(outline_steps(f32::NAN), 1);
        assert_eq!(outline_steps(f32::INFINITY), 1);
        assert_eq!(outline_steps(1.0e9), super::OUTLINE_MAX_STEPS);
    }
}
