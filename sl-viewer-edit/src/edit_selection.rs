//! Object selection core (`viewer-object-selection-core`): the maintained
//! **selection set** every object-editing operation plugs into, with click and
//! drag-rectangle selection, a highlight on the selected objects, and the
//! object-select / deselect / object-properties wire protocol behind it.
//!
//! # Model
//!
//! - [`SelectionSet`] is the shared state: the ordered list of selected
//!   objects (the **primary** — the one the numeric fields and local-frame
//!   gizmos follow — is the most recently added), each carrying the
//!   `ObjectProperties` the simulator returned for it (permission masks,
//!   names, owner), plus the tentative set a rubber-band drag is sweeping.
//! - While the build tool ([`crate::edit_tool`]) is active, a **left click**
//!   in the world selects the object under the cursor — the whole linkset by
//!   default, the picked prim alone in edit-linked-parts mode — with
//!   Shift / Ctrl toggling membership (the reference's `LLToolSelect` extend
//!   semantics, applied on mouse-up with a drag slop). A click on nothing
//!   deselects all; `Escape` does too.
//! - A **left drag** that starts on empty world sweeps a rubber-band
//!   rectangle ([`crate::edit_math::rect_selects`]): objects whose projected
//!   bounds overlap it are tentatively highlighted and committed on release
//!   (the reference's `LLToolSelectRect` with its default inclusive test).
//!   Only in-world volume objects (prims / sculpts / meshes) are swept —
//!   avatars, trees, grass, and worn attachments are not rubber-band
//!   selectable, matching the reference.
//! - The **wire side** (`sync_selection_wire`): every object added to the
//!   set is sent in an `ObjectSelect` ([`Command::RequestObjectProperties`]),
//!   whose `ObjectProperties` reply is folded back onto the node; every
//!   object removed is sent in an `ObjectDeselect`. A simulator-forced
//!   selection (`ForceObjectSelect`) replaces or extends the set, and an
//!   object killed out of the scene is pruned.
//! - The **highlight** (`apply_selection_highlight`): every face mesh of a
//!   selected object (and its linkset children) gets a translucent unlit
//!   overlay child. Which overlay follows the reference's own split by object
//!   kind: a face of an uploaded **mesh** object (`isMesh()`, rigged or not)
//!   wears a wireframe of its geometry — posed with it when it is rigged —
//!   as `renderMeshSelection_f` draws, see the `selection_wireframe` module;
//!   every other face (prim, sculpt, tree, grass) wears an inflated shell
//!   sharing its mesh, a simpler stand-in for the reference's silhouette edge
//!   rendering (`generateSilhouette`), deliberately not a port of it.
//!
//! Reference (Firestorm, read-only): `llselectmgr`, `lltoolselect`,
//! `lltoolselectrect`.

use std::collections::{HashMap, HashSet};

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::ecs::system::SystemParam;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::input_focus::InputFocus;
use bevy::light::NotShadowCaster;
use bevy::mesh::skinning::SkinnedMesh;
use bevy::picking::hover::HoverMap;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use sl_client_bevy::{
    Command, DeRezDestination, FolderType, ObjectKey, PrimFaceId, ScopedObjectId, SlCommand,
    SlEvent, SlSessionEvent, TransactionId, Uuid, texture_face_uv_transform,
};

use crate::edit_math::rect_selects;
use crate::face_material::{FaceMaterial, inert_face_material};
use crate::gizmos::GizmoInteraction;
use crate::inventory::InventoryModel;
use crate::objects::ObjectPicker;
use crate::objects::{
    FaceTextureDebug, ObjectCategory, ObjectSlMotion, PrimFaceEntity, SceneObject, WornPickTarget,
};
use crate::ui::UiRoot;
use crate::world_api::InputContext;
use crate::world_api::ObjectState;
use crate::world_api::SkinPoseTwin;
use crate::world_api::ViewerCamera;
use crate::world_api::on_hud_layer;
use crate::world_api::pointer_over_blocking_ui;
use crate::world_api::{DragHoverHighlight, EditTool, EditToolState};
use crate::world_api::{SelectedNode, SelectionSet};

/// How far (logical pixels) the cursor may wander between press and release
/// and still count as a **click**; any further and the gesture is a
/// rubber-band drag — the reference's `SLOP_RADIUS`.
const CLICK_SLOP: f32 = 5.0;

/// The rubber-band rectangle's border colour (the reference draws the sweep in
/// the focus colour).
const RUBBER_BAND_BORDER: Color = Color::srgba(0.4, 0.75, 1.0, 0.9);

/// The rubber-band rectangle's fill.
const RUBBER_BAND_FILL: Color = Color::srgba(0.4, 0.75, 1.0, 0.10);

/// The selected **root**'s outline colour — the reference's
/// `SilhouetteParentColor` (`Yellow`, `1 1 0`).
const ROOT_OUTLINE: Color = Color::srgba(1.0, 1.0, 0.0, 0.85);

/// The **primary** selection's root outline — the last-selected object, the one
/// the numeric fields / gizmo follow and the one that becomes the linkset root
/// on a link. A bright near-white, deliberately distinct from the parent-yellow
/// of the other selected roots so it reads as "the active one" when several
/// objects are selected. (The reference draws every root the same yellow and
/// distinguishes the primary only in the floater; a distinct 3D colour is a
/// small addition on top, so a builder can see which prim will win a link.)
const PRIMARY_OUTLINE: Color = Color::srgba(1.0, 1.0, 1.0, 0.95);

/// A selected linkset **child**'s outline colour — the reference's
/// `SilhouetteChildColor` (`SL-MidBlue`, `0.3 0.6 0.9`).
const CHILD_OUTLINE: Color = Color::srgba(0.3, 0.6, 0.9, 0.85);

/// The tentative (mid-rubber-band) outline tint — the reference's hover
/// highlight colour family.
const PENDING_OUTLINE: Color = Color::srgba(0.35, 0.7, 1.0, 0.6);

/// The drag-drop hover outline for an object you may edit (own / modify) — a
/// green "accept" glow while an inventory item is dragged over it.
const DROP_ACCEPT_OUTLINE: Color = Color::srgba(0.3, 1.0, 0.45, 0.85);

/// The drag-drop hover outline for an object you do **not** own but which still
/// accepts the drop (its "allow anyone to add inventory" flag) — **red**, the
/// reference's no-modify silhouette colour, so a drop into someone else's object
/// is unmistakable.
const DROP_FOREIGN_OUTLINE: Color = Color::srgba(1.0, 0.25, 0.2, 0.9);

/// How far the outline shell is inflated past the face geometry: an
/// inverted-hull outline (front faces culled, mesh slightly enlarged) reads as
/// the reference's silhouette edge glow without porting its edge-walk.
const OUTLINE_INFLATE: f32 = 1.035;

/// The visibility every face overlay this module parents onto a face entity is
/// spawned with — [`Visibility::Visible`], which in Bevy shows the entity
/// **regardless of its ancestors**, rather than the inherited default.
///
/// A fully transparent face is built hidden (the reference's alpha-pool gate,
/// `sl_viewer_world_objects::objects`), and an invisible root box is exactly the
/// prim a builder selects. The reference draws its selection silhouette from the
/// object's volume in `LLSelectMgr::renderSilhouettes`, entirely outside the draw
/// pools the gate keeps it out of, so the outline shows there whether or not the
/// face itself draws — inheriting the face's visibility here would lose it.
const OUTLINE_VISIBILITY: Visibility = Visibility::Visible;

/// The in-flight left-button gesture of the selection tool: where it pressed,
/// what it pressed on, and whether it has grown past the click slop into a
/// rubber-band sweep.
#[derive(Resource, Debug, Default)]
pub(crate) struct SelectGesture {
    /// The live gesture, or `None` outside a press.
    state: Option<GestureState>,
}

/// See [`SelectGesture`].
#[derive(Debug)]
struct GestureState {
    /// The cursor position at press, in logical pixels.
    anchor: Vec2,
    /// Whether Shift / Ctrl was held at press (extend / toggle semantics).
    extend: bool,
    /// Whether the press landed on an object (a click selects it) rather than
    /// empty world (a drag sweeps a rectangle, a click deselects all).
    pressed_object: Option<(ScopedObjectId, ObjectKey, Entity)>,
    /// Whether the gesture has crossed [`CLICK_SLOP`] and become a
    /// rubber-band sweep (only ever set for an empty-world press).
    banding: bool,
}

/// The rubber-band rectangle's UI node, spawned lazily on the first sweep and
/// hidden between sweeps.
#[derive(Resource, Debug, Default)]
struct RubberBandNode {
    /// The `bevy_ui` node drawing the rectangle, once spawned.
    node: Option<Entity>,
}

/// Which outline a highlight overlay carries — the reference's silhouette
/// colour split (parent yellow, child mid-blue) plus the tentative
/// rubber-band tint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HighlightKind {
    /// The **primary** selection's root — the last-selected object, which the
    /// numeric fields / gizmo follow and which becomes the linkset root on a
    /// link. Drawn distinct from the other selected roots.
    Primary,
    /// A (non-primary) selected object's root prim (or the picked part in
    /// edit-linked-parts mode).
    Root,
    /// A linkset child riding along with its selected root.
    Child,
    /// Tentatively swept by the live rubber band.
    Pending,
    /// An inventory drag is hovering an object you may add to (own / modify) — a
    /// green "accept" outline ([`DragHoverHighlight`]).
    DropAccept,
    /// An inventory drag is hovering an object you do not own but which accepts
    /// the drop — a **red** outline.
    DropForeign,
}

/// Every editor overlay the selection core hangs off a face mesh — the
/// silhouette shell, the drag-hover shell, the Select Face grid cursor.
///
/// It is a **pick exclusion**, and that is why the three share a marker. Each
/// overlay reuses its face's own mesh: the shells are inflated (so they sit
/// strictly in front of the face) and the grid cursor is exactly coplanar with
/// it. A world ray therefore strikes an overlay before — or indistinguishably
/// from — the surface it decorates, and an overlay carries no
/// [`PrimFaceEntity`], so the resolved hit loses its **face index**. That is
/// invisible to whole-object selection (the walk up to the [`SceneObject`]
/// finds the same object either way) and fatal to the Select Face tool, whose
/// second click on a face resolved to "no face" and did nothing at all.
#[derive(Component, Debug, Clone, Copy)]
struct EditorOverlay;

/// An outline-shell overlay child on one selected (or tentatively swept) face
/// mesh — the selection highlight.
#[derive(Component, Debug)]
struct SelectionHighlightOverlay {
    /// Which outline this overlay carries, so a change swaps the material.
    kind: HighlightKind,
}

/// The shared outline materials, one per [`HighlightKind`].
#[derive(Resource, Debug)]
struct HighlightAssets {
    /// The primary selection's root outline material.
    primary: Handle<FaceMaterial>,
    /// A (non-primary) selected root's outline material.
    root: Handle<FaceMaterial>,
    /// A linkset child's outline material.
    child: Handle<FaceMaterial>,
    /// The tentative rubber-band outline material.
    pending: Handle<FaceMaterial>,
    /// The drag-drop accept (own / modify) outline material.
    drop_accept: Handle<FaceMaterial>,
    /// The drag-drop foreign (not-owned, allow-drop) outline material.
    drop_foreign: Handle<FaceMaterial>,
}

impl HighlightAssets {
    /// The material for `kind`.
    fn material(&self, kind: HighlightKind) -> Handle<FaceMaterial> {
        match kind {
            HighlightKind::Primary => self.primary.clone(),
            HighlightKind::Root => self.root.clone(),
            HighlightKind::Child => self.child.clone(),
            HighlightKind::Pending => self.pending.clone(),
            HighlightKind::DropAccept => self.drop_accept.clone(),
            HighlightKind::DropForeign => self.drop_foreign.clone(),
        }
    }
}

impl FromWorld for HighlightAssets {
    /// Build the inverted-hull outline materials once: unlit, front faces
    /// culled, so only the inflated shell's back-facing rim shows — an edge
    /// glow, not a fill.
    fn from_world(world: &mut World) -> Self {
        let mut materials = world.resource_mut::<Assets<FaceMaterial>>();
        let mut outline = |color: Color| {
            // An inert `FaceMaterial` (bit-identical to the bare `StandardMaterial`)
            // so `SlFaceExt`'s `specialize` keeps this translucent outline's coverage
            // out of the glow mask — an editor overlay must not bloom under the glow
            // pass.
            materials.add(inert_face_material(StandardMaterial {
                base_color: color,
                unlit: true,
                alpha_mode: AlphaMode::Blend,
                cull_mode: Some(bevy::render::render_resource::Face::Front),
                ..Default::default()
            }))
        };
        let primary = outline(PRIMARY_OUTLINE);
        let root = outline(ROOT_OUTLINE);
        let child = outline(CHILD_OUTLINE);
        let pending = outline(PENDING_OUTLINE);
        let drop_accept = outline(DROP_ACCEPT_OUTLINE);
        let drop_foreign = outline(DROP_FOREIGN_OUTLINE);
        Self {
            primary,
            root,
            child,
            pending,
            drop_accept,
            drop_foreign,
        }
    }
}

/// A drag-drop hover outline overlay, kept apart from the selection's
/// [`SelectionHighlightOverlay`] so the two reconcilers never fight.
#[derive(Component, Debug)]
struct DragHoverOverlay {
    /// Which outline this overlay carries, so a change swaps the material.
    kind: HighlightKind,
}

/// The wire-side bookkeeping: which objects have been sent as selected
/// (`ObjectSelect`) and not yet deselected, so set changes are diffed into
/// select / deselect messages exactly once.
#[derive(Resource, Debug, Default)]
struct WireSelection {
    /// The scoped ids currently selected on the wire.
    synced: HashSet<ScopedObjectId>,
}

/// The plugin wiring the selection core into the viewer.
#[derive(Debug, Clone, Copy, Default)]
pub struct EditSelectionPlugin;

impl Plugin for EditSelectionPlugin {
    /// Register the selection state and its systems. The pointer gesture runs
    /// after the gizmo interaction ([`crate::gizmos`]) so a press on a
    /// manipulator handle never doubles as a selection click.
    fn build(&self, app: &mut App) {
        app.init_resource::<SelectionSet>()
            .init_resource::<SelectGesture>()
            .init_resource::<RubberBandNode>()
            .init_resource::<WireSelection>()
            .init_resource::<HighlightAssets>()
            .init_resource::<FaceCursorAssets>()
            .init_resource::<DragHoverHighlight>()
            // The selection pipeline is gated on build mode. The input systems
            // already bailed on `!active`; the wire-diff / highlight / face-cursor
            // systems are the teardown reconcilers that must run on the
            // active→inactive edge (send the deselects, despawn the outline /
            // face-cursor overlays) — the settling window covers that edge.
            .add_systems(
                Update,
                (
                    handle_select_pointer.after(crate::gizmos::drive_gizmo_interaction),
                    clear_selection_on_escape,
                    delete_selected_objects,
                    ingest_selection_events,
                    sync_selection_wire,
                    apply_selection_highlight,
                    apply_face_cursor_highlight,
                )
                    .chain()
                    .run_if(crate::edit_tool::edit_tool_active_or_settling),
            )
            // The inventory drag-drop hover outline is NOT build-mode work — you
            // can drop an item onto an in-world object without opening the Build
            // floater — so it stays ungated. It owns its own `DragHoverOverlay`
            // component, distinct from the selection outline, so dropping it out
            // of the chain above changes no behaviour.
            .add_systems(Update, apply_drag_hover_highlight);
    }
}

/// The pointer / camera / occlusion inputs the selection gesture reads,
/// bundled as one [`SystemParam`] to stay inside Bevy's system-parameter
/// limit.
#[derive(SystemParam)]
struct SelectPointer<'w, 's> {
    /// The mouse buttons.
    buttons: Res<'w, ButtonInput<MouseButton>>,
    /// The keyboard, for the Shift / Ctrl extend modifiers and Alt (camera).
    keyboard: Res<'w, ButtonInput<KeyCode>>,
    /// The `bevy_ui` hover map, for the UI-occlusion guard.
    hover_map: Res<'w, HoverMap>,
    /// Pickability, for the UI-occlusion guard.
    pickables: Query<'w, 's, &'static Pickable>,
    /// Node sizes, for the UI-occlusion guard.
    node_sizes: Query<'w, 's, &'static ComputedNode>,
    /// The per-frame UI-claim flag: a widget that consumes a press (a combo
    /// dropdown closing on a pick) sets this, and the world pick then skips it —
    /// the reliable path where the despawning widget leaves a stale hover-map
    /// entry the occlusion guard alone would miss.
    ui_claim: Res<'w, crate::ui::UiPointerClaim>,
    /// The window, for the cursor position.
    windows: Query<'w, 's, &'static Window>,
    /// The world camera, to build pick rays and project candidate bounds.
    camera: Query<'w, 's, (&'static Camera, &'static GlobalTransform), With<ViewerCamera>>,
    /// Render layers, to exclude HUD / gizmo geometry from world picks.
    layers: Query<'w, 's, (Entity, &'static RenderLayers)>,
    /// The editor's own overlay shells, to exclude them too — see
    /// [`EditorOverlay`].
    overlays: Query<'w, 's, Entity, With<EditorOverlay>>,
}

impl SelectPointer<'_, '_> {
    /// The entities a world pick must not strike: HUD and gizmo geometry (by
    /// render layer, exactly as the touch pick excludes them) and the editor's
    /// own overlay shells ([`EditorOverlay`]).
    fn pick_exclusions(&self) -> HashSet<Entity> {
        self.layers
            .iter()
            .filter(|(_entity, layers)| {
                on_hud_layer(Some(layers)) || crate::gizmos::on_gizmo_layer(Some(layers))
            })
            .map(|(entity, _layers)| entity)
            .chain(self.overlays.iter())
            .collect()
    }
}

/// The click / rubber-band pointer gesture of the selection tool. See the
/// [module documentation](self) for the semantics.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the tool state, \
              the gesture and selection state, the bundled pointer inputs, the pick machinery, \
              and the candidate queries the rubber band sweeps"
)]
fn handle_select_pointer(
    tool: Res<EditToolState>,
    gizmo: Res<GizmoInteraction>,
    pointer: SelectPointer,
    mut ray_cast: MeshRayCast,
    picker: ObjectPicker,
    state: Res<ObjectState>,
    candidates: Query<(Entity, &SceneObject, &ObjectSlMotion, &GlobalTransform)>,
    mut gesture: ResMut<SelectGesture>,
    mut selection: ResMut<SelectionSet>,
    mut band: ResMut<RubberBandNode>,
    ui_root: Option<Res<UiRoot>>,
    mut band_nodes: Query<(&mut Node, &mut Visibility)>,
    mut commands: Commands,
) {
    if !tool.active {
        // Leaving edit mode cancels any live gesture and hides the band.
        if gesture.state.take().is_some() {
            hide_rubber_band(&band, &mut band_nodes);
        }
        return;
    }
    let Ok(window) = pointer.windows.single() else {
        return;
    };
    let Ok((camera, camera_transform)) = pointer.camera.single() else {
        return;
    };
    let keyboard = &pointer.keyboard;
    let buttons = &pointer.buttons;
    let alt = keyboard.pressed(KeyCode::AltLeft) || keyboard.pressed(KeyCode::AltRight);

    // -- Select Face tool: pick a per-face texture-entry selection. -----------
    // A distinct mode (the reference's `LLToolFace`): a click resolves to one
    // prim face rather than sweeping a rubber band or driving a gizmo, so it
    // bypasses the object-selection gesture machinery entirely.
    if tool.tool == EditTool::SelectFace {
        if buttons.just_pressed(MouseButton::Left) && !alt {
            let over_ui = pointer_over_blocking_ui(
                &pointer.hover_map,
                &pointer.pickables,
                &pointer.node_sizes,
            );
            if gizmo.claims_pointer() || over_ui || pointer.ui_claim.is_claimed() {
                return;
            }
            let Some(cursor) = window.cursor_position() else {
                return;
            };
            let Ok(ray) = camera.viewport_to_world(camera_transform, cursor) else {
                return;
            };
            let exclude = pointer.pick_exclusions();
            let shift =
                keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);
            handle_face_pick(
                ray,
                shift,
                &mut ray_cast,
                &picker,
                &state,
                &exclude,
                &mut selection,
            );
        }
        return;
    }

    // -- Press: classify what the gesture starts on. --------------------------
    if buttons.just_pressed(MouseButton::Left) && !alt {
        // A press over UI, over a gizmo handle, or with no cursor is not a
        // selection gesture.
        let over_ui =
            pointer_over_blocking_ui(&pointer.hover_map, &pointer.pickables, &pointer.node_sizes);
        if gizmo.claims_pointer() || over_ui || pointer.ui_claim.is_claimed() {
            return;
        }
        let Some(cursor) = window.cursor_position() else {
            return;
        };
        let Ok(ray) = camera.viewport_to_world(camera_transform, cursor) else {
            return;
        };
        // The world pick, excluding HUD geometry exactly as the touch pick does.
        let exclude = pointer.pick_exclusions();
        let pressed_object = picker.pick(ray, &mut ray_cast, &exclude).and_then(|hit| {
            // A worn attachment is not world-editable here (the attachment
            // alignment tools are their own task); treat it as empty world.
            if hit.summary.attachment {
                return None;
            }
            if tool.edit_linked {
                Some((
                    hit.summary.picked_scoped,
                    hit.summary.picked_full,
                    state.entity_by_scoped(&hit.summary.picked_scoped)?,
                ))
            } else {
                Some((
                    hit.summary.root_scoped,
                    hit.summary.root_full,
                    state.entity_by_scoped(&hit.summary.root_scoped)?,
                ))
            }
        });
        gesture.state = Some(GestureState {
            anchor: cursor,
            extend: keyboard.pressed(KeyCode::ShiftLeft)
                || keyboard.pressed(KeyCode::ShiftRight)
                || keyboard.pressed(KeyCode::ControlLeft)
                || keyboard.pressed(KeyCode::ControlRight),
            pressed_object,
            banding: false,
        });
        return;
    }

    let Some(active) = gesture.state.as_mut() else {
        return;
    };

    // -- Drag: grow an empty-world press into a rubber-band sweep. ------------
    if buttons.pressed(MouseButton::Left) {
        let cursor = window.cursor_position().unwrap_or(active.anchor);
        let moved = cursor.distance(active.anchor);
        if active.pressed_object.is_none() && (active.banding || moved > CLICK_SLOP) {
            active.banding = true;
            let (min, max) = crate::edit_math::rect_from_corners(active.anchor, cursor);
            show_rubber_band(
                min,
                max,
                &mut band,
                ui_root.as_deref(),
                &mut band_nodes,
                &mut commands,
            );
            *selection.rect_pending_mut() =
                sweep_candidates(min, max, camera, camera_transform, &candidates);
        }
        return;
    }

    // -- Release: commit the gesture. -----------------------------------------
    let Some(finished) = gesture.state.take() else {
        return;
    };
    hide_rubber_band(&band, &mut band_nodes);
    if finished.banding {
        // Commit the sweep: extend keeps the existing selection, plain replaces.
        // The sweep is taken **before** the replace, because `clear` empties the
        // tentative set along with the committed one — clearing first left every
        // plain (non-extend) rubber band committing an empty vector, so the
        // gesture selected nothing at all and only a Shift-sweep ever worked.
        let pending = core::mem::take(selection.rect_pending_mut());
        if !finished.extend {
            selection.clear();
        }
        for (scoped, entity) in pending {
            if let Some(full) = state.full_key(&scoped) {
                selection.insert(scoped, full, entity);
            }
        }
        return;
    }
    // A click (within slop).
    match finished.pressed_object {
        Some((scoped, full, entity)) => {
            if finished.extend {
                if selection.is_selected(scoped) {
                    selection.remove(scoped);
                } else {
                    selection.insert(scoped, full, entity);
                }
            } else {
                selection.select_only(scoped, full, entity);
            }
        }
        None => {
            // A click on empty world deselects (plain click only; an extend
            // click on nothing leaves the selection alone, as the reference
            // does).
            if !finished.extend {
                selection.clear();
            }
        }
    }
}

/// The Select Face tool's click resolution (the reference's `LLToolFace`
/// `pickCallback`): pick the prim face under `ray` and fold it into the per-face
/// selection — plain click replaces the whole selection with that one face,
/// `shift` extends / toggles it. A click on empty world deselects (plain click
/// only). A worn attachment or a hit with no face index is ignored. The picked
/// **prim** (not its linkset root) is what carries the face, matching the
/// reference, whose face selection is always per-object.
fn handle_face_pick(
    ray: Ray3d,
    shift: bool,
    ray_cast: &mut MeshRayCast,
    picker: &ObjectPicker,
    state: &ObjectState,
    exclude: &HashSet<Entity>,
    selection: &mut SelectionSet,
) {
    let Some(hit) = picker.pick(ray, ray_cast, exclude) else {
        // Empty world: a plain click clears the selection; shift leaves it.
        if !shift {
            selection.clear();
        }
        return;
    };
    if hit.summary.attachment {
        return;
    }
    // A negative face index is the reference's "no face" sentinel.
    let Ok(face_index) = u16::try_from(hit.surface.face_index) else {
        return;
    };
    let face = PrimFaceId::new(face_index);
    let scoped = hit.summary.picked_scoped;
    let full = hit.summary.picked_full;
    let Some(entity) = state.entity_by_scoped(&scoped) else {
        return;
    };
    if shift {
        selection.toggle_face(scoped, full, entity, face);
    } else {
        selection.select_only_face(scoped, full, entity, face);
    }
}

/// Sweep every selectable in-world volume object against the rubber-band
/// rectangle: project the corners of each object's scale box and apply the
/// inclusive overlap test.
fn sweep_candidates(
    min: Vec2,
    max: Vec2,
    camera: &Camera,
    camera_transform: &GlobalTransform,
    candidates: &Query<(Entity, &SceneObject, &ObjectSlMotion, &GlobalTransform)>,
) -> Vec<(ScopedObjectId, Entity)> {
    let mut swept = Vec::new();
    for (entity, scene, motion, global) in candidates.iter() {
        if !motion.is_root || motion.attachment {
            continue;
        }
        if !matches!(
            scene.category,
            ObjectCategory::Prim | ObjectCategory::Sculpt | ObjectCategory::Mesh
        ) {
            continue;
        }
        // The eight corners of the object's own scale box, projected to the
        // viewport (corners behind the camera project to nothing).
        let half = Vec3::new(
            motion.scale.x * 0.5,
            motion.scale.y * 0.5,
            motion.scale.z * 0.5,
        );
        let corners = (0_u8..8_u8).filter_map(|index| {
            let corner = Vec3::new(
                if index & 1 == 0 { -half.x } else { half.x },
                if index & 2 == 0 { -half.y } else { half.y },
                if index & 4 == 0 { -half.z } else { half.z },
            );
            let world = global.transform_point(corner);
            camera.world_to_viewport(camera_transform, world).ok()
        });
        if rect_selects(min, max, corners, true) {
            swept.push((scene.scoped_id, entity));
        }
    }
    swept
}

/// Show (spawning on first use) and place the rubber-band rectangle node.
fn show_rubber_band(
    min: Vec2,
    max: Vec2,
    band: &mut RubberBandNode,
    ui_root: Option<&UiRoot>,
    band_nodes: &mut Query<(&mut Node, &mut Visibility)>,
    commands: &mut Commands,
) {
    let rect_node = Node {
        position_type: PositionType::Absolute,
        left: Val::Px(min.x),
        top: Val::Px(min.y),
        width: Val::Px(max.x - min.x),
        height: Val::Px(max.y - min.y),
        border: UiRect::all(Val::Px(1.0)),
        ..Default::default()
    };
    if let Some(node) = band.node
        && let Ok((mut layout, mut visibility)) = band_nodes.get_mut(node)
    {
        *layout = rect_node;
        *visibility = Visibility::Visible;
        return;
    }
    let Some(root) = ui_root.map(|root| root.0) else {
        return;
    };
    let node = commands
        .spawn((
            rect_node,
            BorderColor::all(RUBBER_BAND_BORDER),
            BackgroundColor(RUBBER_BAND_FILL),
            // Draw over floaters' base layer but never intercept the pointer.
            Pickable::IGNORE,
            Visibility::Visible,
            Name::new("edit-selection:rubber-band"),
            ChildOf(root),
        ))
        .id();
    band.node = Some(node);
}

/// Hide the rubber-band rectangle between sweeps.
fn hide_rubber_band(band: &RubberBandNode, band_nodes: &mut Query<(&mut Node, &mut Visibility)>) {
    if let Some(node) = band.node
        && let Ok((_layout, mut visibility)) = band_nodes.get_mut(node)
    {
        *visibility = Visibility::Hidden;
    }
}

/// `Escape` (in the world, with the build tool active) deselects everything —
/// the reference's escape-out of an edit selection.
fn clear_selection_on_escape(
    tool: Res<EditToolState>,
    context: Res<crate::world_api::InputContext>,
    focus: Res<InputFocus>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut selection: ResMut<SelectionSet>,
) {
    if !tool.active || !context.is_world() || focus.get().is_some() {
        return;
    }
    if keyboard.just_pressed(KeyCode::Escape) && !selection.is_empty() {
        selection.clear();
    }
}

/// **Delete** derezzes the selected in-world objects to the Trash while the build
/// tool is active and the **world** owns input (the reference's build-mode Delete
/// accelerator). Gated on [`InputContext::is_world`], so a focused inventory /
/// contents list (which makes the context `UiWidget`) keeps `Delete` for *its*
/// selection instead — the three delete handlers never fight over the key. Each
/// selected part is resolved to its linkset **root** and deduplicated, matching
/// the object pie's Delete; the simulator arbitrates the permission.
fn delete_selected_objects(
    tool: Res<EditToolState>,
    context: Res<InputContext>,
    keyboard: Res<ButtonInput<KeyCode>>,
    selection: Res<SelectionSet>,
    objects: Res<ObjectState>,
    inventory: Res<InventoryModel>,
    mut commands: MessageWriter<SlCommand>,
) {
    if !tool.active || !context.is_world() || selection.is_empty() {
        return;
    }
    if !keyboard.just_pressed(KeyCode::Delete) {
        return;
    }
    let Some(trash) = inventory.folder_by_type(FolderType::Trash) else {
        return;
    };
    // The selected parts resolved to their linkset roots, deduplicated (derez
    // acts on whole objects, as the object pie's Delete does).
    let mut roots: Vec<ScopedObjectId> = Vec::new();
    for node in selection.iter() {
        let root = objects.linkset_root_of(&node.scoped).unwrap_or(node.scoped);
        if !roots.contains(&root) {
            roots.push(root);
        }
    }
    if roots.is_empty() {
        return;
    }
    commands.write(SlCommand(Command::DerezObjects {
        local_ids: roots,
        destination: DeRezDestination::Trash(trash),
        transaction_id: TransactionId::from(Uuid::new_v4()),
        group_id: None,
    }));
}

/// Fold the session's selection-related events into the set: `ObjectProperties`
/// replies onto their nodes, a simulator-forced selection into the set, and a
/// killed object out of it.
fn ingest_selection_events(
    mut events: MessageReader<SlEvent>,
    state: Res<ObjectState>,
    mut selection: ResMut<SelectionSet>,
    mut wire: ResMut<WireSelection>,
) {
    for event in events.read() {
        match &event.0 {
            SlSessionEvent::ObjectProperties(properties) => {
                // `bypass_change_detection` is deliberately NOT used: a
                // properties arrival is a real change the floater re-reads.
                if !selection.apply_properties(properties.clone()) {
                    debug!(
                        "edit-selection: ObjectProperties for unselected object {:?}",
                        properties.object_id
                    );
                }
            }
            SlSessionEvent::ForceObjectSelect {
                reset_list,
                objects,
            } => {
                if *reset_list {
                    selection.clear();
                    wire.synced.clear();
                }
                for scoped in objects {
                    if let (Some(full), Some(entity)) =
                        (state.full_key(scoped), state.entity_by_scoped(scoped))
                    {
                        selection.insert(*scoped, full, entity);
                        // Simulator-initiated: already selected on the sim's
                        // side, so do not echo an ObjectSelect back.
                        wire.synced.insert(*scoped);
                    }
                }
            }
            SlSessionEvent::ObjectRemoved { local_id, .. } => {
                if selection.is_selected(*local_id) {
                    selection.remove(*local_id);
                }
                selection
                    .rect_pending_mut()
                    .retain(|(scoped, _entity)| scoped != local_id);
                // Gone from the region — nothing to deselect on the wire.
                wire.synced.remove(local_id);
            }
            _other => {}
        }
    }
}

/// Diff the selection set against what has been sent on the wire, sending
/// `ObjectSelect` for additions (which also subscribes the `ObjectProperties`
/// reply) and `ObjectDeselect` for removals.
fn sync_selection_wire(
    selection: Res<SelectionSet>,
    mut wire: ResMut<WireSelection>,
    mut commands: MessageWriter<SlCommand>,
) {
    if !selection.is_changed() {
        return;
    }
    let current: HashSet<ScopedObjectId> = selection.iter().map(|node| node.scoped).collect();
    let added: Vec<ScopedObjectId> = current
        .iter()
        .filter(|scoped| !wire.synced.contains(scoped))
        .copied()
        .collect();
    let removed: Vec<ScopedObjectId> = wire
        .synced
        .iter()
        .filter(|scoped| !current.contains(scoped))
        .copied()
        .collect();
    if !added.is_empty() {
        commands.write(SlCommand(Command::RequestObjectProperties {
            local_ids: added.clone(),
        }));
    }
    if !removed.is_empty() {
        commands.write(SlCommand(Command::DeselectObjects {
            local_ids: removed.clone(),
        }));
    }
    wire.synced = current;
}

/// Keep the selection highlight overlays in step with the set: every face mesh
/// under a selected object (or one tentatively swept by the rubber band) gets a
/// translucent overlay child sharing its mesh; stale overlays are despawned.
///
/// Runs its reconciliation every frame — the face sets are small and the walk
/// is cheap — so a face rebuilt by an LOD swap (which despawns the old face
/// entities, taking their overlays with them) regains its overlay without any
/// extra bookkeeping.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the tool / \
              selection state, the shared outline materials, and the hierarchy / face / overlay \
              queries the reconcile walks"
)]
fn apply_selection_highlight(
    tool: Res<EditToolState>,
    selection: Res<SelectionSet>,
    assets: Res<HighlightAssets>,
    children: Query<&Children>,
    scene: Query<&SceneObject>,
    transforms: Query<&Transform>,
    faces: HighlightFaceQuery,
    worn: WornFaceQuery,
    mut overlays: Query<(
        Entity,
        &ChildOf,
        &mut SelectionHighlightOverlay,
        &mut MeshMaterial3d<FaceMaterial>,
    )>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut commands: Commands,
) {
    // The desired overlay set: face entity → the outline it should wear. A
    // committed outline (primary, then root, then child) wins over a tentative one
    // when both apply.
    let mut desired: HashMap<Entity, DesiredOutline> = HashMap::new();
    // The same set by scoped id, for the worn rigged faces the hierarchy walk
    // cannot reach ([`collect_worn_faces`]).
    let mut objects: HashMap<ScopedObjectId, HighlightKind> = HashMap::new();
    // In Select Face mode the per-face grid cursor ([`apply_face_cursor_highlight`])
    // is the highlight; the whole-object silhouette outline is suppressed so the
    // two do not stack.
    if tool.active && tool.tool != EditTool::SelectFace {
        let primary_entity = selection.primary().map(|node| node.entity);
        for node in selection.iter() {
            // The last-selected object's own root prim reads as the primary
            // (the one the fields / gizmo follow and the future link root); the
            // other selected roots stay parent-yellow.
            let root_kind = if Some(node.entity) == primary_entity {
                HighlightKind::Primary
            } else {
                HighlightKind::Root
            };
            collect_faces(
                node.entity,
                &children,
                &scene,
                &transforms,
                &faces,
                root_kind,
                HighlightKind::Child,
                &mut desired,
                &mut objects,
            );
        }
        for (_scoped, entity) in selection.rect_pending() {
            collect_faces(
                *entity,
                &children,
                &scene,
                &transforms,
                &faces,
                HighlightKind::Pending,
                HighlightKind::Pending,
                &mut desired,
                &mut objects,
            );
        }
        collect_worn_faces(&objects, &worn, &mut desired);
    }
    // Despawn stale overlays; an overlay whose face is still highlighted stays,
    // taking the new colour in place rather than being rebuilt. That matters for
    // a mesh face, whose overlay is a derived wireframe mesh: promoting one
    // selection to primary re-colours a whole linkset, and rebuilding a body's
    // worth of edges to change a tint would be a visible hitch.
    for (overlay, child_of, mut marker, mut material) in &mut overlays {
        match desired.remove(&child_of.parent()) {
            Some(outline) => {
                if outline.kind != marker.kind {
                    marker.kind = outline.kind;
                    material.0 = assets.material(outline.kind);
                }
            }
            None => commands.entity(overlay).despawn(),
        }
    }
    // Spawn the missing ones: a wireframe of the face's mesh for a mesh object (or
    // a rigged face), an inflated shell sharing it otherwise — see
    // [`spawn_outline_overlay`].
    for (face, outline) in desired {
        let Ok((mesh, skin)) = faces.get(face) else {
            continue;
        };
        spawn_outline_overlay(
            face,
            mesh,
            skin,
            outline,
            assets.material(outline.kind),
            SelectionHighlightOverlay { kind: outline.kind },
            &mut meshes,
            &mut commands,
        );
    }
}

/// The face-mesh query both highlight reconcilers walk: a face's mesh and, when
/// it is rigged, the skin its overlay has to be drawn with.
type HighlightFaceQuery<'w, 's> =
    Query<'w, 's, (&'static Mesh3d, Option<&'static SkinnedMesh>), With<PrimFaceEntity>>;

/// The worn-rigged-face query both reconcilers walk once the hierarchy walk is
/// done: a face and the worn object it renders. These faces hang off their
/// **wearer's** body root, so nothing under the selected object's entity leads to
/// them — see [`collect_worn_faces`].
type WornFaceQuery<'w, 's> = Query<'w, 's, (Entity, &'static WornPickTarget), With<PrimFaceEntity>>;

/// What one face's outline overlay should be, as [`collect_faces`] works it out
/// from the object the face hangs under: its colour, whether that object is an
/// uploaded **mesh** (which the reference wireframes rather than silhouettes),
/// and the scale between the face mesh's own space and metres.
#[derive(Clone, Copy, Debug)]
struct DesiredOutline {
    /// The outline colour — parent, child, primary, tentative, or a drop target's.
    kind: HighlightKind,
    /// Whether the face's object is an uploaded mesh asset
    /// ([`ObjectCategory::Mesh`], the reference's `LLVOVolume::isMesh`).
    mesh_object: bool,
    /// Metres per unit of the face mesh's own space, from the transforms between
    /// the selection root and the face. Only the wireframe path uses it, to keep
    /// its lift a world distance.
    scale: Vec3,
}

/// Spawn one outline overlay on `face` — the shared body of the selection
/// highlight and the drag-drop hover highlight, which draw the same two
/// highlights in different colours under different markers.
///
/// The reference splits by **object kind**: `LLSelectMgr::renderSilhouettes`
/// sends every object whose volume `isMesh()` — an uploaded mesh asset, rigged or
/// not — to `renderMeshSelection_f`, which wireframes its selected faces, and
/// only prims, sculpts, trees and grass reach `renderOneSilhouette`. So a face of
/// an [`ObjectCategory::Mesh`] object (`outline.mesh_object`) gets
/// [`crate::selection_wireframe`]'s line-list derivation of its mesh, and every
/// other face wears an **inverted-hull shell**: its own mesh again, front faces
/// culled and pushed out by an entity-`Transform` scale, so only the rim shows.
///
/// A **rigged** face takes the wireframe whatever its object says, because the
/// shell is impossible for it: the shared mesh specializes into the skinned
/// pipeline (a shell without the skin is the wgpu validation error that quits the
/// viewer) and a skinned draw ignores the entity scale that would inflate it. It
/// additionally carries the skin that poses it and the [`SkinPoseTwin`] that
/// earns that skin the same GPU palette as the face. See that module for the
/// details.
///
/// A face whose mesh is not loaded (or is not an indexed triangle list) gets no
/// wireframe this frame; the reconciler runs every frame, so it gains one as soon
/// as the mesh is there.
#[expect(
    clippy::too_many_arguments,
    reason = "the face and its mesh, its skin, what its object asks for, the colour, the caller's \
              own marker, and the two stores the spawn writes through"
)]
fn spawn_outline_overlay(
    face: Entity,
    mesh: &Mesh3d,
    skin: Option<&SkinnedMesh>,
    outline: DesiredOutline,
    material: Handle<FaceMaterial>,
    marker: impl Bundle,
    meshes: &mut Assets<Mesh>,
    commands: &mut Commands,
) {
    if skin.is_none() && !outline.mesh_object {
        commands.spawn((
            Mesh3d(mesh.0.clone()),
            MeshMaterial3d(material),
            Transform::from_scale(Vec3::splat(OUTLINE_INFLATE)),
            NotShadowCaster,
            marker,
            EditorOverlay,
            ChildOf(face),
            OUTLINE_VISIBILITY,
        ));
        return;
    }
    // A rigged face's geometry is already in metres and its entity transform is
    // ignored by the skinned draw; an unrigged mesh object's is in the asset's
    // normalized space, with the object's Second Life size on the geometry holder
    // above it. Only the latter has a scale for the lift to compensate for.
    let scale = if skin.is_some() {
        Vec3::ONE
    } else {
        outline.scale
    };
    let Some(wireframe) = meshes
        .get(&mesh.0)
        .and_then(|source| crate::selection_wireframe::wireframe_mesh(source, scale))
    else {
        return;
    };
    let mut overlay = commands.spawn((
        Mesh3d(meshes.add(wireframe)),
        MeshMaterial3d(material),
        // No inflate: the wireframe hugs the face it outlines, carrying its lift
        // off the surface in the geometry — which is also the only lever left on a
        // skinned draw, whose vertices come from the joint palette.
        Transform::IDENTITY,
        NotShadowCaster,
        marker,
        EditorOverlay,
        ChildOf(face),
        OUTLINE_VISIBILITY,
    ));
    if let Some(skin) = skin {
        overlay.insert((skin.clone(), SkinPoseTwin { source: face }));
    }
}

/// Draw the drag-drop hover outline: while an inventory drag hovers an object
/// that accepts the drop ([`DragHoverHighlight`]), every face of that object (and
/// its linkset family) gets an outline overlay — green when you may edit it, red
/// when it is foreign (the reference's `highlightObjectAndFamily` during a drag).
/// A separate overlay from the selection's, so the two reconcilers never fight.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the hover state, \
              the shared outline materials, the hierarchy / face / overlay queries, the mesh \
              store the rigged overlay derives into, and the stage the diagnostic dedups on"
)]
fn apply_drag_hover_highlight(
    hover: Res<DragHoverHighlight>,
    assets: Res<HighlightAssets>,
    children: Query<&Children>,
    scene: Query<&SceneObject>,
    transforms: Query<&Transform>,
    faces: HighlightFaceQuery,
    worn: WornFaceQuery,
    mut overlays: Query<(
        Entity,
        &ChildOf,
        &mut DragHoverOverlay,
        &mut MeshMaterial3d<FaceMaterial>,
    )>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut commands: Commands,
    mut last_target: Local<Option<Entity>>,
) {
    let mut desired: HashMap<Entity, DesiredOutline> = HashMap::new();
    if let Some(target) = hover.hover {
        let kind = if target.foreign {
            HighlightKind::DropForeign
        } else {
            HighlightKind::DropAccept
        };
        // One colour for the whole family (the drop targets this object) — pass
        // the same kind for the root and its children.
        let mut objects: HashMap<ScopedObjectId, HighlightKind> = HashMap::new();
        collect_faces(
            target.root,
            &children,
            &scene,
            &transforms,
            &faces,
            kind,
            kind,
            &mut desired,
            &mut objects,
        );
        collect_worn_faces(&objects, &worn, &mut desired);
    }
    // The draw half of the same diagnostic the hover driver writes
    // (`sl_viewer::drag_hover`): with a target published, this says how many faces
    // of it the outline actually found. A target with **no** faces is the one
    // failure the driver cannot see — an object whose render entity is not the one
    // its faces hang under.
    let target = hover.hover.map(|target| target.root);
    if *last_target != target {
        *last_target = target;
        debug!(
            target: crate::inventory_drag::DRAG_HOVER_LOG_TARGET,
            "drag hover outline: target {target:?}, {} face(s) to outline",
            desired.len()
        );
    }
    // Despawn stale overlays; one still hovered stays and takes the new colour in
    // place (a drop target that flips own → foreign), as the selection outline does.
    for (overlay, child_of, mut marker, mut material) in &mut overlays {
        match desired.remove(&child_of.parent()) {
            Some(outline) => {
                if outline.kind != marker.kind {
                    marker.kind = outline.kind;
                    material.0 = assets.material(outline.kind);
                }
            }
            None => commands.entity(overlay).despawn(),
        }
    }
    // Spawn the missing ones (the same overlay the selection outline draws —
    // a shell on a prim face, a wireframe on a mesh object's or a rigged one).
    for (face, outline) in desired {
        let Ok((mesh, skin)) = faces.get(face) else {
            continue;
        };
        spawn_outline_overlay(
            face,
            mesh,
            skin,
            outline,
            assets.material(outline.kind),
            DragHoverOverlay { kind: outline.kind },
            &mut meshes,
            &mut commands,
        );
    }
}

/// Collect every face-mesh entity under `root` (the object's own faces and its
/// linkset children's) into `desired`, colouring the selected object's own root
/// faces as `root_kind` and any linkset child's (a descendant carrying its own
/// [`SceneObject`]) as `child_kind` — the reference's parent / child silhouette
/// split, with the primary root distinguished. A stronger outline (primary,
/// then root, then child) wins over a tentative ([`HighlightKind::Pending`])
/// one when both apply.
///
/// The walk also carries down the two things [`spawn_outline_overlay`] cannot
/// read off a face entity: which of the two highlights that face's **object**
/// takes ([`ObjectCategory::Mesh`] — the reference's `isMesh()`), and the scale
/// accumulated from the selection root down to the face, which is the object's
/// Second Life size (it sits on the object's geometry holder, not on the object
/// entity). Accumulating the local `Transform`s rather than reading the face's
/// `GlobalTransform` keeps this correct on the frame a face is rebuilt, before
/// transform propagation has run for it.
#[expect(
    clippy::too_many_arguments,
    reason = "the walk reads the hierarchy, the objects, their transforms and the faces, and is \
              parameterised by the two colours it assigns and the map it fills"
)]
fn collect_faces(
    root: Entity,
    children: &Query<&Children>,
    scene: &Query<&SceneObject>,
    transforms: &Query<&Transform>,
    faces: &HighlightFaceQuery,
    root_kind: HighlightKind,
    child_kind: HighlightKind,
    desired: &mut HashMap<Entity, DesiredOutline>,
    worn: &mut HashMap<ScopedObjectId, HighlightKind>,
) {
    let scale_of = |entity: Entity| {
        transforms
            .get(entity)
            .map_or(Vec3::ONE, |transform| transform.scale)
    };
    let mut stack = vec![(
        root,
        false,
        scene.get(root).ok().map(|object| object.category),
        scale_of(root),
    )];
    while let Some((entity, mut is_child, mut category, mut scale)) = stack.pop() {
        // Crossing into a descendant that is its own scene object means the
        // subtree below belongs to a linkset child, whose own kind decides its
        // own highlight.
        if entity != root
            && let Ok(object) = scene.get(entity)
        {
            is_child = true;
            category = Some(object.category);
        }
        // Every object the walk crosses is noted by scoped id, because its
        // **rigged** faces are not in this subtree at all — a worn rigged submesh
        // hangs off its wearer's body root, not its own object entity, and is
        // reached through [`WornPickTarget`] after the walk.
        if let Ok(object) = scene.get(entity) {
            let kind = if is_child { child_kind } else { root_kind };
            merge_kind(worn, object.scoped_id, kind);
        }
        if entity != root {
            // Component-wise: the glam `Vec3` operators trip the workspace
            // `arithmetic_side_effects` lint.
            let step = scale_of(entity);
            scale = Vec3::new(scale.x * step.x, scale.y * step.y, scale.z * step.z);
        }
        if faces.contains(entity) {
            let kind = if is_child { child_kind } else { root_kind };
            merge_outline(
                desired,
                entity,
                DesiredOutline {
                    kind,
                    mesh_object: category == Some(ObjectCategory::Mesh),
                    scale,
                },
            );
        }
        if let Ok(list) = children.get(entity) {
            for child in list.iter() {
                stack.push((child, is_child, category, scale));
            }
        }
    }
}

/// How strongly one outline claims a face: a committed selection beats a
/// tentative one, and within a selection the primary beats another root, which
/// beats a linkset child. Used to settle a face (or an object) reached twice —
/// selected outright *and* swept by the rubber band, or a linkset child that is
/// also a selected root.
const fn outline_rank(kind: HighlightKind) -> u8 {
    match kind {
        HighlightKind::Primary => 0_u8,
        HighlightKind::Root => 1_u8,
        HighlightKind::Child => 2_u8,
        HighlightKind::Pending => 3_u8,
        // Drag-hover kinds never merge with the selection kinds (they are
        // reconciled by a separate system over their own overlay), so their
        // relative rank is immaterial.
        HighlightKind::DropAccept => 4_u8,
        HighlightKind::DropForeign => 5_u8,
    }
}

/// Claim `face` for `outline`, keeping the stronger colour if something already
/// claimed it. Only the colour merges: the other fields describe the face's own
/// object, so every claim on one face carries the same ones.
fn merge_outline(
    desired: &mut HashMap<Entity, DesiredOutline>,
    face: Entity,
    outline: DesiredOutline,
) {
    desired
        .entry(face)
        .and_modify(|existing| {
            if outline_rank(outline.kind) < outline_rank(existing.kind) {
                existing.kind = outline.kind;
            }
        })
        .or_insert(outline);
}

/// The same merge for the by-scoped-id ledger of objects whose rigged faces are
/// collected after the walk.
fn merge_kind(
    objects: &mut HashMap<ScopedObjectId, HighlightKind>,
    scoped: ScopedObjectId,
    kind: HighlightKind,
) {
    objects
        .entry(scoped)
        .and_modify(|existing| {
            if outline_rank(kind) < outline_rank(*existing) {
                *existing = kind;
            }
        })
        .or_insert(kind);
}

/// Add the outline for every **worn rigged** face of the objects the walk
/// crossed. Such a face is parented under its wearer's body root rather than its
/// own object entity (the skinned vertices are placed by the joint palette, so
/// the entity only carries lifecycle and visibility), which is why
/// [`collect_faces`] cannot reach it and why it carries its
/// [`WornPickTarget`] identity instead — the same handle the GPU pick uses to
/// route a click on a worn mesh to the attachment pies.
///
/// A rigged face always wears the wireframe, so the object-kind and scale fields
/// the shell path reads are not consulted for it.
fn collect_worn_faces(
    objects: &HashMap<ScopedObjectId, HighlightKind>,
    worn: &WornFaceQuery,
    desired: &mut HashMap<Entity, DesiredOutline>,
) {
    if objects.is_empty() {
        return;
    }
    for (face, target) in worn {
        let Some(kind) = objects.get(&target.scoped) else {
            continue;
        };
        merge_outline(
            desired,
            face,
            DesiredOutline {
                kind: *kind,
                mesh_object: true,
                scale: Vec3::ONE,
            },
        );
    }
}

// ---------------------------------------------------------------------------
// The Select Face tool's per-face grid cursor.
// ---------------------------------------------------------------------------

/// The face-cursor grid texture's size, in texels (one crisp cell — it is drawn
/// at the face's own texture repeats, so one tile is enough).
const FACE_CURSOR_TEXELS: u32 = 128;

/// Half the width, in texels, of the cursor's white lines (border frame, circle
/// ring, and crosshair). The border sits at the tile edges, so adjacent repeats'
/// borders meet at each integer UV as one hairline.
const FACE_CURSOR_LINE: f32 = 1.5;

/// The face cursor's depth bias, pulling the coplanar grid overlay in front of
/// the face it sits on so it never z-fights the surface it marks.
const FACE_CURSOR_DEPTH_BIAS: f32 = 8.0;

/// The shared face-cursor grid texture — a white cell marker (a border frame, an
/// inscribed circle, and a centred crosshair) on a transparent tile, wrapped
/// `Repeat` so, drawn with a face's own UV transform, it marks every texture
/// repeat: the reference's white "select face" overlay, whose circle and cross
/// make it obvious when a face shows only part of a texture (the cell centre and
/// bounds shift with the placement).
#[derive(Resource, Debug)]
struct FaceCursorAssets {
    /// The repeat-wrapped marker image.
    grid: Handle<Image>,
}

impl FromWorld for FaceCursorAssets {
    /// Build the marker tile once: an opaque-white border frame, inscribed circle
    /// ring, and centred crosshair on a transparent field, with a `Repeat`
    /// sampler.
    fn from_world(world: &mut World) -> Self {
        let size = FACE_CURSOR_TEXELS;
        #[expect(
            clippy::as_conversions,
            clippy::cast_precision_loss,
            reason = "the tile size is a small power of two, exact as f32"
        )]
        let dim = size as f32;
        let half = dim * 0.5;
        // The circle sits just inside the border, its ring the same width as the
        // other lines.
        let radius = half - FACE_CURSOR_LINE * 3.0;
        let texels = usize::try_from(size).unwrap_or(0);
        let mut data = Vec::with_capacity(texels.saturating_mul(texels).saturating_mul(4));
        for y in 0..size {
            for x in 0..size {
                #[expect(
                    clippy::as_conversions,
                    clippy::cast_precision_loss,
                    reason = "texel coordinates are small non-negative integers, exact as f32"
                )]
                let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                // The border frame: within a line-width of any tile edge.
                let edge = fx < FACE_CURSOR_LINE
                    || fy < FACE_CURSOR_LINE
                    || fx > dim - FACE_CURSOR_LINE
                    || fy > dim - FACE_CURSOR_LINE;
                // The centred crosshair: a horizontal and a vertical line.
                let cross =
                    (fx - half).abs() < FACE_CURSOR_LINE || (fy - half).abs() < FACE_CURSOR_LINE;
                // The inscribed circle ring.
                let dist = ((fx - half).powi(2) + (fy - half).powi(2)).sqrt();
                let circle = (dist - radius).abs() < FACE_CURSOR_LINE;
                // White line texels are opaque; elsewhere transparent so the face's
                // own texture shows through inside each repeat cell.
                let alpha = if edge || cross || circle { 255 } else { 0 };
                data.extend_from_slice(&[255, 255, 255, alpha]);
            }
        }
        let mut image = Image::new(
            Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            data,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::RENDER_WORLD,
        );
        // Wrap the tile so a face's repeat count draws that many grid cells.
        image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::Repeat,
            address_mode_v: ImageAddressMode::Repeat,
            ..ImageSamplerDescriptor::linear()
        });
        let grid = world.resource_mut::<Assets<Image>>().add(image);
        Self { grid }
    }
}

/// A per-face grid-cursor overlay child on a selected face (Select Face tool):
/// the face's own mesh drawn with the white repeat grid.
#[derive(Component, Debug)]
struct FaceCursorOverlay;

/// The face-mesh data the cursor overlay reads: the shared mesh handle, the
/// face's Linden index (to test membership in the selected-face set), and its
/// decoded texture placement (whose UV transform the grid follows so the grid
/// lines land on the texture's repeat boundaries).
type CursorFaceQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static Mesh3d,
        &'static PrimFaceEntity,
        &'static FaceTextureDebug,
        Option<&'static SkinnedMesh>,
    ),
>;

/// Draw the white repeat-grid cursor on the selected faces while the Select Face
/// tool is active: each chosen face gets an overlay child sharing its mesh, drawn
/// with the grid texture under the face's own UV transform so the grid outlines
/// every texture repeat. A node with no explicit face set (`faces == None`)
/// cursors all of its own faces. Reconciled every frame like the silhouette
/// overlays, so a face rebuilt by a texture edit / LOD swap regains its cursor.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the tool / \
              selection state, the grid asset, the hierarchy / scene / face / overlay queries, and \
              the material store the reconcile spawns into"
)]
fn apply_face_cursor_highlight(
    tool: Res<EditToolState>,
    selection: Res<SelectionSet>,
    assets: Res<FaceCursorAssets>,
    children: Query<&Children>,
    scene: Query<(), With<SceneObject>>,
    cursor_faces: CursorFaceQuery,
    overlays: Query<(Entity, &ChildOf), With<FaceCursorOverlay>>,
    mut materials: ResMut<Assets<FaceMaterial>>,
    mut commands: Commands,
) {
    // The desired cursor set: face entity → its texture placement.
    let mut desired: HashMap<Entity, FaceTextureDebug> = HashMap::new();
    if tool.active && tool.tool == EditTool::SelectFace {
        for node in selection.iter() {
            collect_own_face_ids(
                node.entity,
                node.faces.as_ref(),
                &children,
                &scene,
                &cursor_faces,
                &mut desired,
            );
        }
    }
    // Despawn cursors whose face left the set, keep the rest.
    for (overlay, child_of) in overlays.iter() {
        if desired.remove(&child_of.parent()).is_none() {
            commands.entity(overlay).despawn();
        }
    }
    // Spawn the missing cursors: the face's mesh, the grid material carrying the
    // face's own UV transform, pulled in front of the face by a depth bias.
    for (face, FaceTextureDebug(texture_face)) in desired {
        let Ok((mesh, _marker, _debug, skin)) = cursor_faces.get(face) else {
            continue;
        };
        // An inert `FaceMaterial` (bit-identical to the bare `StandardMaterial`) so
        // `SlFaceExt`'s `specialize` keeps this translucent grid cursor's coverage out
        // of the glow mask — an editor overlay must not bloom under the glow pass.
        let material = materials.add(inert_face_material(StandardMaterial {
            base_color: Color::WHITE,
            base_color_texture: Some(assets.grid.clone()),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            // The grid follows the face's texture placement (repeats / offset /
            // rotation), so its lines fall on each repeat's boundary.
            uv_transform: texture_face_uv_transform(&texture_face),
            // The face is single-sided; the cursor shows on either side so it is
            // visible however the face is wound.
            cull_mode: None,
            double_sided: true,
            depth_bias: FACE_CURSOR_DEPTH_BIAS,
            ..Default::default()
        }));
        let mut cursor = commands.spawn((
            Mesh3d(mesh.0.clone()),
            MeshMaterial3d(material),
            NotShadowCaster,
            FaceCursorOverlay,
            EditorOverlay,
            ChildOf(face),
            OUTLINE_VISIBILITY,
        ));
        // A rigged face's cursor shares its **mesh**, which carries the skin
        // attributes — so Bevy specializes it into the skinned pipeline and would
        // then hand this entity a model-only bind group, a wgpu validation error
        // the viewer quits on. It needs the skin, and the pose that goes with it
        // ([`SkinPoseTwin`]); the cursor is coplanar with the face, so skinning it
        // identically is also exactly where it belongs.
        if let Some(skin) = skin {
            cursor.insert((skin.clone(), SkinPoseTwin { source: face }));
        }
    }
}

/// Collect the object's **own** face entities (not its linkset children's) whose
/// Linden index is in `wanted` — or all of them when `wanted` is `None` (the
/// whole object) — into `desired`, each with its decoded texture placement. The
/// walk stops at any descendant carrying its own [`SceneObject`], so a
/// face-selected prim never cursors a sibling prim's faces.
fn collect_own_face_ids(
    root: Entity,
    wanted: Option<&HashSet<PrimFaceId>>,
    children: &Query<&Children>,
    scene: &Query<(), With<SceneObject>>,
    cursor_faces: &CursorFaceQuery,
    desired: &mut HashMap<Entity, FaceTextureDebug>,
) {
    let mut stack = vec![root];
    while let Some(entity) = stack.pop() {
        // Do not descend into a linkset child object.
        if entity != root && scene.contains(entity) {
            continue;
        }
        if let Ok((_mesh, marker, debug, _skin)) = cursor_faces.get(entity)
            && wanted.is_none_or(|set| set.contains(&marker.face_id))
        {
            desired.insert(entity, *debug);
        }
        if let Ok(list) = children.get(entity) {
            for child in list.iter() {
                stack.push(child);
            }
        }
    }
}

/// Promote every selected linked part to its linkset **root** — whole-linkset
/// mode's invariant, the reference's `promoteSelectionToRoot`, run when
/// *Edit Linked Parts* is switched off. Each node is resolved to its root
/// (via [`ObjectState::linkset_root_of`]); duplicates collapse (two parts of
/// one linkset become the single root); and selection order — hence the
/// primary = last — is preserved, the last-selected part's root becoming the
/// primary root. A root the viewer cannot resolve is kept as-is. Returns
/// whether anything changed.
///
/// A promoted node drops its part's `ObjectProperties`; the wire diff
/// (`sync_selection_wire`) then selects the root and re-requests them.
///
/// Public because the child→root jump needs a *populated* [`ObjectState`] — a
/// real linkset in a real world — which only the viewer's fixture world can
/// stand up ([[viewer-edit-selection-interaction-tests]]); the unit test below
/// can reach no further than the all-roots no-op.
pub fn promote_selection_to_roots(selection: &mut SelectionSet, objects: &ObjectState) -> bool {
    let mut promoted: Vec<SelectedNode> = Vec::new();
    for node in selection.nodes() {
        let root_scoped = objects.linkset_root_of(&node.scoped).unwrap_or(node.scoped);
        let promoted_node = if root_scoped == node.scoped {
            // Already a root (or unresolvable): keep it, properties intact.
            node.clone()
        } else if let (Some(full), Some(entity)) = (
            objects.full_key(&root_scoped),
            objects.entity_by_scoped(&root_scoped),
        ) {
            SelectedNode {
                scoped: root_scoped,
                full,
                entity,
                properties: None,
                // Promoting to the whole linkset drops any per-face selection.
                faces: None,
            }
        } else {
            // Root known but not resolvable to a scene entity: leave as-is.
            node.clone()
        };
        // Dedupe with move-to-end, so the last-selected part's root wins the
        // primary slot (mirrors `insert`'s promote-on-reselect).
        if let Some(pos) = promoted
            .iter()
            .position(|existing| existing.scoped == promoted_node.scoped)
        {
            let existing = promoted.remove(pos);
            promoted.push(existing);
        } else {
            promoted.push(promoted_node);
        }
    }
    let changed = promoted.len() != selection.nodes().len()
        || promoted
            .iter()
            .zip(selection.nodes())
            .any(|(new, old)| new.scoped != old.scoped);
    if changed {
        selection.replace_nodes(promoted);
    }
    changed
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::expect_used,
        clippy::panic,
        reason = "a failed expectation is the intended failure signal in a unit test"
    )]

    use super::{HighlightAssets, SelectionSet, WireSelection, promote_selection_to_roots};
    use crate::face_material::FaceMaterial;
    use crate::objects::ObjectCategory;
    use bevy::app::{App, TaskPoolPlugin};
    use bevy::asset::{AssetApp as _, AssetPlugin};
    use bevy::math::Vec3;
    use bevy::prelude::Entity;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{CircuitId, ObjectKey, RegionLocalObjectId, ScopedObjectId, Uuid};

    /// The Second Life scale on the fixture object's geometry holder — big enough
    /// that a lift taken in the mesh's own units would be visibly wrong, and not a
    /// round power of two so a dropped factor cannot pass by luck.
    const HOLDER_SCALE: Vec3 = Vec3::new(6.0, 6.0, 6.0);

    /// A scoped id for tests.
    fn scoped(id: u32) -> ScopedObjectId {
        ScopedObjectId {
            circuit: CircuitId::new(1),
            id: RegionLocalObjectId(id),
        }
    }

    /// A full key for tests.
    fn full(id: u128) -> ObjectKey {
        ObjectKey::from(Uuid::from_u128(id))
    }

    /// Insert / remove / primary semantics: the most recent selection is
    /// primary, re-selecting promotes, removing forgets.
    #[test]
    fn selection_set_semantics() {
        let mut set = SelectionSet::default();
        assert!(set.is_empty());
        set.insert(scoped(1), full(1), Entity::PLACEHOLDER);
        set.insert(scoped(2), full(2), Entity::PLACEHOLDER);
        assert_eq!(set.len(), 2);
        assert!(set.is_selected(scoped(1)));
        assert_eq!(set.primary().map(|node| node.scoped), Some(scoped(2)));
        // Re-selecting an existing object promotes it to primary without
        // growing the set.
        set.insert(scoped(1), full(1), Entity::PLACEHOLDER);
        assert_eq!(set.len(), 2);
        assert_eq!(set.primary().map(|node| node.scoped), Some(scoped(1)));
        set.remove(scoped(1));
        assert!(!set.is_selected(scoped(1)));
        assert_eq!(set.len(), 1);
        set.clear();
        assert!(set.is_empty());
    }

    /// The wire diff bookkeeping starts empty.
    #[test]
    fn wire_selection_starts_empty() {
        let wire = WireSelection::default();
        assert!(wire.synced.is_empty());
    }

    /// Promoting a selection of already-root (or untracked) objects is a no-op:
    /// nothing to jump, so the set and its primary are unchanged. (The
    /// child→root jump needs a populated `ObjectState` and is exercised live.)
    #[test]
    fn promote_to_roots_is_a_noop_when_all_roots() {
        let objects = crate::world_api::ObjectState::default();
        let mut set = SelectionSet::default();
        set.insert(scoped(1), full(1), Entity::PLACEHOLDER);
        set.insert(scoped(2), full(2), Entity::PLACEHOLDER);
        assert!(
            !promote_selection_to_roots(&mut set, &objects),
            "no linked parts to promote"
        );
        assert_eq!(set.len(), 2);
        assert_eq!(set.primary().map(|node| node.scoped), Some(scoped(2)));
    }

    /// `HighlightAssets::from_world` builds the selection-outline materials from
    /// `Assets<FaceMaterial>`, so that asset MUST be registered before this
    /// resource is initialised. This guards the plugin-ordering regression where
    /// `EditSelectionPlugin` (which `init_resource`s `HighlightAssets` at build
    /// time) ran *before* `SlFaceMaterialPlugin`, panicking at every startup — the
    /// editor overlays were switched from `StandardMaterial` to `FaceMaterial` for
    /// the glow pass, so they no longer piggy-back on Bevy's always-present
    /// `Assets<StandardMaterial>`. See the plugin ordering in `lib.rs`.
    #[test]
    fn highlight_assets_build_from_face_material_asset() {
        let mut app = App::new();
        app.add_plugins((TaskPoolPlugin::default(), AssetPlugin::default()));
        // Register `Assets<FaceMaterial>` first (as `SlFaceMaterialPlugin` does),
        // then build the resource — it must not panic.
        app.init_asset::<FaceMaterial>();
        app.init_resource::<HighlightAssets>();
        assert!(
            app.world().get_resource::<HighlightAssets>().is_some(),
            "HighlightAssets should build once Assets<FaceMaterial> is registered"
        );
    }

    /// The fixture face geometry: a single triangle, with the skin attributes when
    /// the face is rigged — the wireframe derivation walks the real geometry.
    fn triangle_mesh(skinned: bool) -> bevy::mesh::Mesh {
        use bevy::asset::RenderAssetUsages;
        use bevy::mesh::{Indices, Mesh, PrimitiveTopology, VertexAttributeValues};

        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_POSITION,
            vec![[0.0_f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0_f32, 0.0, 1.0]; 3]);
        if skinned {
            mesh.insert_attribute(
                Mesh::ATTRIBUTE_JOINT_INDEX,
                VertexAttributeValues::Uint16x4(vec![[0_u16; 4]; 3]),
            );
            mesh.insert_attribute(
                Mesh::ATTRIBUTE_JOINT_WEIGHT,
                vec![[1.0_f32, 0.0, 0.0, 0.0]; 3],
            );
        }
        mesh.insert_indices(Indices::U32(vec![0, 1, 2]));
        mesh
    }

    /// A world holding one object with a single face, and the assets both
    /// highlight reconcilers need: the app, the object's root, and its face.
    /// `skinned` gives the face the skin (and skin vertex attributes) a rigged one
    /// carries; `category` is the object's render kind, which decides between the
    /// shell and the wireframe for an unrigged face.
    ///
    /// The face hangs under a geometry holder carrying the object's Second Life
    /// scale, as the object builder spawns it — the shape the outline's lift reads
    /// to keep itself a world distance.
    fn face_app(skinned: bool, category: ObjectCategory) -> (App, Entity, Entity) {
        use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
        use bevy::prelude::*;
        use sl_client_bevy::PrimFaceId;

        use crate::objects::{PrimFaceEntity, SceneObject};
        use crate::world_api::EditToolState;

        let mut app = App::new();
        app.add_plugins((TaskPoolPlugin::default(), AssetPlugin::default()));
        app.init_asset::<FaceMaterial>();
        app.init_asset::<Mesh>();
        app.init_asset::<SkinnedMeshInverseBindposes>();
        app.init_resource::<HighlightAssets>();
        app.insert_resource(EditToolState {
            active: true,
            ..EditToolState::default()
        });

        let mesh = app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .add(triangle_mesh(skinned));

        let root = app
            .world_mut()
            .spawn((
                SceneObject {
                    scoped_id: scoped(1),
                    category,
                },
                Transform::IDENTITY,
            ))
            .id();
        let holder = app
            .world_mut()
            .spawn((Transform::from_scale(HOLDER_SCALE), ChildOf(root)))
            .id();
        let mut face = app.world_mut().spawn((
            Mesh3d(mesh),
            PrimFaceEntity {
                face_id: PrimFaceId::new(0),
            },
            ChildOf(holder),
        ));
        if skinned {
            let bindposes = face.world_scope(|world| {
                world
                    .resource_mut::<Assets<SkinnedMeshInverseBindposes>>()
                    .add(SkinnedMeshInverseBindposes::from(vec![Mat4::IDENTITY]))
            });
            let joint = face.world_scope(|world| world.spawn(Transform::default()).id());
            face.insert(SkinnedMesh {
                inverse_bindposes: bindposes,
                joints: vec![joint],
            });
        }
        let face = face.id();
        (app, root, face)
    }

    /// [`face_app`] with the object **selected** and the selection reconciler
    /// running: the app and the face.
    fn selected_face_app(skinned: bool, category: ObjectCategory) -> (App, Entity) {
        let (mut app, root, face) = face_app(skinned, category);
        app.add_systems(bevy::app::Update, super::apply_selection_highlight);
        let mut selection = SelectionSet::default();
        selection.insert(scoped(1), full(1), root);
        app.insert_resource(selection);
        app.update();
        (app, face)
    }

    /// The **drag-drop hover** outline draws the same overlay as the selection's,
    /// off its own state: publishing a hover target outlines that object's faces.
    /// It shares one spawn path with the selection outline and had no coverage of
    /// its own, which is how it kept the crash-on-a-rigged-face the selection
    /// outline was given a mitigation for in 2026-08-05.
    #[test]
    fn a_drag_hover_target_outlines_the_object() {
        use bevy::prelude::*;

        use crate::world_api::{DragHover, DragHoverHighlight};

        let (mut app, root, face) = face_app(false, ObjectCategory::Prim);
        app.add_systems(bevy::app::Update, super::apply_drag_hover_highlight);
        app.insert_resource(DragHoverHighlight {
            hover: Some(DragHover {
                root,
                foreign: false,
            }),
        });
        app.update();
        let overlay = app
            .world_mut()
            .query_filtered::<&ChildOf, With<super::DragHoverOverlay>>()
            .iter(app.world())
            .any(|child_of| child_of.parent() == face);
        assert!(
            overlay,
            "a published drop target outlines the face of the object it names"
        );

        // Dropping the target takes the outline with it.
        app.world_mut().resource_mut::<DragHoverHighlight>().hover = None;
        app.update();
        assert_eq!(
            app.world_mut()
                .query_filtered::<Entity, With<super::DragHoverOverlay>>()
                .iter(app.world())
                .count(),
            0,
            "the outline goes when the drag leaves the object"
        );
    }

    /// The split is by **object kind**, not by rigging: the reference sends every
    /// object whose volume `isMesh()` to `renderMeshSelection_f`, so an ordinary
    /// (unrigged) uploaded mesh is wireframed exactly like an animesh — no skin
    /// and no pose twin, since there is no pose to follow.
    ///
    /// Its lift is a world distance, so the geometry holder's Second Life scale
    /// divides the offset the wireframe carries in its own normalized units: a
    /// six-metre object lifted by the local extent would stand a hand's width off
    /// its own surface.
    #[test]
    fn an_unrigged_mesh_object_wears_the_wireframe() {
        use bevy::mesh::skinning::SkinnedMesh;
        use bevy::mesh::{Mesh, PrimitiveTopology, VertexAttributeValues};
        use bevy::prelude::*;

        use crate::world_api::SkinPoseTwin;

        let (mut app, face) = selected_face_app(false, ObjectCategory::Mesh);
        let overlay = app
            .world_mut()
            .query::<(Entity, &ChildOf)>()
            .iter(app.world())
            .find(|(_entity, child_of)| child_of.parent() == face)
            .map(|(entity, _child_of)| entity)
            .expect("the selected mesh face gets a highlight overlay");
        let world = app.world();
        let handle = world
            .get::<Mesh3d>(overlay)
            .expect("the overlay draws a mesh");
        let asset = world
            .resource::<Assets<Mesh>>()
            .get(&handle.0)
            .expect("its mesh is loaded");
        assert_eq!(
            asset.primitive_topology(),
            PrimitiveTopology::LineList,
            "the reference wireframes every mesh object, rigged or not"
        );
        assert!(
            world.get::<SkinnedMesh>(overlay).is_none()
                && world.get::<SkinPoseTwin>(overlay).is_none(),
            "an unrigged wireframe has no pose to follow"
        );
        assert_eq!(
            world.get::<Transform>(overlay).map(Transform::to_matrix),
            Some(Transform::IDENTITY.to_matrix()),
            "the wireframe hugs the face; its lift lives in the geometry"
        );

        // The holder's scale reaches the lift: with the object six metres to a
        // side, the same offset in world metres is six times smaller in the mesh's
        // own units than it would be at unit scale.
        let Some(VertexAttributeValues::Float32x3(lifted)) =
            asset.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("the wireframe carries positions");
        };
        let source = world
            .resource::<Assets<Mesh>>()
            .get(&world.get::<Mesh3d>(face).expect("the face draws a mesh").0)
            .expect("the face's mesh is loaded");
        let unscaled = crate::selection_wireframe::wireframe_mesh(source, Vec3::ONE)
            .expect("the fixture mesh is an indexed triangle list");
        let Some(VertexAttributeValues::Float32x3(unscaled)) =
            unscaled.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("the unscaled wireframe carries positions");
        };
        let lift_of = |positions: &[[f32; 3]]| {
            positions
                .first()
                .and_then(|position| position.get(2).copied())
                .expect("a three-component position")
        };
        assert!(
            lift_of(lifted) < lift_of(unscaled),
            "the holder's {HOLDER_SCALE:?} must shrink the local lift, got {} against {}",
            lift_of(lifted),
            lift_of(unscaled)
        );
    }

    /// A worn **rigged attachment** is outlined too, even though none of its
    /// faces are in the subtree the walk covers: a skinned submesh hangs off its
    /// wearer's body root, not its own object entity, so the hierarchy walk
    /// reaches nothing. It is found by the [`WornPickTarget`] identity it carries
    /// for exactly this reason (the GPU pick routes a click on a worn mesh the
    /// same way), and wears the posed wireframe like any other rigged face.
    ///
    /// This is what "selecting my shoes highlights nothing" was.
    #[test]
    fn a_worn_rigged_attachment_is_outlined_through_its_wearer() {
        use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
        use bevy::mesh::{Mesh, PrimitiveTopology};
        use bevy::prelude::*;
        use sl_client_bevy::PrimFaceId;

        use crate::objects::{PrimFaceEntity, SceneObject, WornPickTarget};
        use crate::world_api::{EditToolState, SkinPoseTwin};

        let mut app = App::new();
        app.add_plugins((TaskPoolPlugin::default(), AssetPlugin::default()));
        app.init_asset::<FaceMaterial>();
        app.init_asset::<Mesh>();
        app.init_asset::<SkinnedMeshInverseBindposes>();
        app.init_resource::<HighlightAssets>();
        app.insert_resource(EditToolState {
            active: true,
            ..EditToolState::default()
        });
        app.add_systems(bevy::app::Update, super::apply_selection_highlight);

        // The attachment's own object entity: a mesh object with a geometry holder
        // and **no** faces under it, which is what a rigged attachment looks like.
        let root = app
            .world_mut()
            .spawn((
                SceneObject {
                    scoped_id: scoped(1),
                    category: ObjectCategory::Mesh,
                },
                Transform::IDENTITY,
            ))
            .id();
        app.world_mut()
            .spawn((Transform::from_scale(HOLDER_SCALE), ChildOf(root)));

        // Its drawn geometry, parented under the wearer instead.
        let mesh = app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .add(triangle_mesh(true));
        let bindposes = app
            .world_mut()
            .resource_mut::<Assets<SkinnedMeshInverseBindposes>>()
            .add(SkinnedMeshInverseBindposes::from(vec![Mat4::IDENTITY]));
        let wearer = app.world_mut().spawn(Transform::default()).id();
        let joint = app.world_mut().spawn(Transform::default()).id();
        let worn = app
            .world_mut()
            .spawn((
                Mesh3d(mesh),
                PrimFaceEntity {
                    face_id: PrimFaceId::new(0),
                },
                SkinnedMesh {
                    inverse_bindposes: bindposes,
                    joints: vec![joint],
                },
                WornPickTarget { scoped: scoped(1) },
                ChildOf(wearer),
            ))
            .id();

        let mut selection = SelectionSet::default();
        selection.insert(scoped(1), full(1), root);
        app.insert_resource(selection);
        app.update();

        let overlay = app
            .world_mut()
            .query::<(Entity, &ChildOf)>()
            .iter(app.world())
            .find(|(_entity, child_of)| child_of.parent() == worn)
            .map(|(entity, _child_of)| entity)
            .expect("selecting the attachment outlines the submesh its wearer carries");
        let world = app.world();
        let handle = world
            .get::<Mesh3d>(overlay)
            .expect("the overlay draws a mesh");
        assert_eq!(
            world
                .resource::<Assets<Mesh>>()
                .get(&handle.0)
                .expect("its mesh is loaded")
                .primitive_topology(),
            PrimitiveTopology::LineList,
            "a worn rigged submesh wears the wireframe like any other rigged face"
        );
        assert_eq!(
            world.get::<SkinPoseTwin>(overlay),
            Some(&SkinPoseTwin { source: worn }),
            "and follows the submesh's own GPU palette binding"
        );
    }

    /// A prim face wears the inverted-hull shell instead: its own mesh again,
    /// inflated by the entity transform. Prims, sculpts, trees and grass are what
    /// the reference leaves on the silhouette path.
    #[test]
    fn an_unrigged_face_wears_the_inflated_shell() {
        use bevy::prelude::*;

        let (mut app, face) = selected_face_app(false, ObjectCategory::Prim);
        let overlay = app
            .world_mut()
            .query::<(Entity, &ChildOf)>()
            .iter(app.world())
            .find(|(_entity, child_of)| child_of.parent() == face)
            .map(|(entity, _child_of)| entity)
            .expect("the selected face gets a highlight overlay");
        let world = app.world();
        assert_eq!(
            world.get::<Transform>(overlay).map(Transform::to_matrix),
            Some(Transform::from_scale(Vec3::splat(super::OUTLINE_INFLATE)).to_matrix()),
            "the shell is the face's mesh pushed out by an entity scale"
        );
        assert!(
            world
                .get::<bevy::mesh::skinning::SkinnedMesh>(overlay)
                .is_none(),
            "an unrigged shell carries no skin"
        );
    }

    /// A rigged face wears a **wireframe** of its posed geometry instead: the
    /// shell is impossible on a skinned draw (the entity scale is ignored, and a
    /// shell without the skin is the wgpu validation error that quits the
    /// viewer), and a wireframe is what the reference draws for a mesh object.
    /// The overlay carries the skin *and* the pose marker, which is what makes it
    /// land on the same posed vertices as the face.
    #[test]
    fn a_rigged_face_wears_a_posed_wireframe() {
        use bevy::mesh::skinning::SkinnedMesh;
        use bevy::mesh::{Mesh, PrimitiveTopology};
        use bevy::prelude::*;

        use crate::world_api::SkinPoseTwin;

        let (mut app, face) = selected_face_app(true, ObjectCategory::Mesh);
        let overlay = app
            .world_mut()
            .query::<(Entity, &ChildOf)>()
            .iter(app.world())
            .find(|(_entity, child_of)| child_of.parent() == face)
            .map(|(entity, _child_of)| entity)
            .expect("the selected rigged face gets a highlight overlay");
        let world = app.world();
        let mesh = world
            .get::<Mesh3d>(overlay)
            .expect("the overlay draws a mesh");
        let asset = world
            .resource::<Assets<Mesh>>()
            .get(&mesh.0)
            .expect("its mesh is loaded");
        assert_eq!(
            asset.primitive_topology(),
            PrimitiveTopology::LineList,
            "a rigged face is outlined by a wireframe, not a hull"
        );
        assert!(
            asset.contains_attribute(Mesh::ATTRIBUTE_JOINT_INDEX),
            "the wireframe skins with the face it outlines"
        );
        assert!(
            world.get::<SkinnedMesh>(overlay).is_some(),
            "without the skin the shared skinned pipeline gets a model-only bind group"
        );
        assert_eq!(
            world.get::<SkinPoseTwin>(overlay),
            Some(&SkinPoseTwin { source: face }),
            "the overlay is posed from the face's own GPU palette binding"
        );
    }
}
