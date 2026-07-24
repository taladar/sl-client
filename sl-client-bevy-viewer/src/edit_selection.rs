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
//!   [`ObjectProperties`] the simulator returned for it (permission masks,
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
//! - The **wire side** ([`sync_selection_wire`]): every object added to the
//!   set is sent in an `ObjectSelect` ([`Command::RequestObjectProperties`]),
//!   whose `ObjectProperties` reply is folded back onto the node; every
//!   object removed is sent in an `ObjectDeselect`. A simulator-forced
//!   selection (`ForceObjectSelect`) replaces or extends the set, and an
//!   object killed out of the scene is pruned.
//! - The **highlight** ([`apply_selection_highlight`]): every face mesh of a
//!   selected object (and its linkset children) gets a translucent unlit
//!   overlay child sharing its mesh — a simpler stand-in for the reference's
//!   silhouette edge rendering (`generateSilhouette`), deliberately not a
//!   port of it.
//!
//! Reference (Firestorm, read-only): `llselectmgr`, `lltoolselect`,
//! `lltoolselectrect`.

use std::collections::{HashMap, HashSet};

use bevy::camera::visibility::RenderLayers;
use bevy::ecs::system::SystemParam;
use bevy::input_focus::InputFocus;
use bevy::light::NotShadowCaster;
use bevy::picking::hover::HoverMap;
use bevy::prelude::*;
use sl_client_bevy::{
    Command, ObjectKey, ObjectProperties, ScopedObjectId, SlCommand, SlEvent, SlSessionEvent,
};

use crate::camera::ViewerCamera;
use crate::edit_math::rect_selects;
use crate::edit_tool::EditToolState;
use crate::gizmos::GizmoInteraction;
use crate::hud::on_hud_layer;
use crate::hud_pick::pointer_over_blocking_ui;
use crate::object_menu::ObjectPicker;
use crate::objects::{ObjectCategory, ObjectSlMotion, ObjectState, PrimFaceEntity, SceneObject};
use crate::ui::UiRoot;

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

/// A selected linkset **child**'s outline colour — the reference's
/// `SilhouetteChildColor` (`SL-MidBlue`, `0.3 0.6 0.9`).
const CHILD_OUTLINE: Color = Color::srgba(0.3, 0.6, 0.9, 0.85);

/// The tentative (mid-rubber-band) outline tint — the reference's hover
/// highlight colour family.
const PENDING_OUTLINE: Color = Color::srgba(0.35, 0.7, 1.0, 0.6);

/// How far the outline shell is inflated past the face geometry: an
/// inverted-hull outline (front faces culled, mesh slightly enlarged) reads as
/// the reference's silhouette edge glow without porting its edge-walk.
const OUTLINE_INFLATE: f32 = 1.035;

/// One selected object in the [`SelectionSet`].
#[derive(Debug, Clone)]
pub(crate) struct SelectedNode {
    /// The object's region-scoped id — what the select / deselect / update
    /// commands address.
    pub(crate) scoped: ScopedObjectId,
    /// The object's grid-wide key — what the `ObjectProperties` reply is
    /// matched back by.
    pub(crate) full: ObjectKey,
    /// The object's scene entity (the linkset root when whole-linkset
    /// selection put it here).
    pub(crate) entity: Entity,
    /// The extended properties the simulator returned for the selection —
    /// permission masks, owner, creator, names — or `None` until the
    /// `ObjectProperties` reply lands.
    pub(crate) properties: Option<Box<ObjectProperties>>,
}

/// The maintained selection set — the shared state the edit floater, the
/// numeric fields, the transform gizmos, and the future linking / per-aspect
/// editors all read. See the [module documentation](self).
#[derive(Resource, Debug, Default)]
pub(crate) struct SelectionSet {
    /// The selected objects, in selection order; the **primary** is the last.
    selected: Vec<SelectedNode>,
    /// The objects a live rubber-band drag currently sweeps (tentative,
    /// highlight-only until the drag commits).
    rect_pending: Vec<(ScopedObjectId, Entity)>,
}

impl SelectionSet {
    /// Whether `scoped` is in the selection.
    pub(crate) fn is_selected(&self, scoped: ScopedObjectId) -> bool {
        self.selected.iter().any(|node| node.scoped == scoped)
    }

    /// Add an object to the selection (a no-op if already present), making it
    /// the primary.
    pub(crate) fn insert(&mut self, scoped: ScopedObjectId, full: ObjectKey, entity: Entity) {
        if let Some(index) = self.selected.iter().position(|node| node.scoped == scoped) {
            // Re-selecting an already-selected object promotes it to primary.
            let node = self.selected.remove(index);
            self.selected.push(node);
            return;
        }
        self.selected.push(SelectedNode {
            scoped,
            full,
            entity,
            properties: None,
        });
    }

    /// Remove an object from the selection (a no-op if absent).
    pub(crate) fn remove(&mut self, scoped: ScopedObjectId) {
        self.selected.retain(|node| node.scoped != scoped);
    }

    /// Empty the selection (both committed and tentative).
    pub(crate) fn clear(&mut self) {
        self.selected.clear();
        self.rect_pending.clear();
    }

    /// The selected objects, in selection order.
    pub(crate) fn iter(&self) -> impl Iterator<Item = &SelectedNode> {
        self.selected.iter()
    }

    /// The **primary** selection — the most recently selected object; the one
    /// the numeric fields display and the local grid frame follows.
    pub(crate) fn primary(&self) -> Option<&SelectedNode> {
        self.selected.last()
    }

    /// How many objects are selected.
    pub(crate) const fn len(&self) -> usize {
        self.selected.len()
    }

    /// Whether nothing is selected.
    pub(crate) const fn is_empty(&self) -> bool {
        self.selected.is_empty()
    }

    /// The tentative rubber-band sweep, for the highlight pass.
    pub(crate) fn rect_pending(&self) -> &[(ScopedObjectId, Entity)] {
        &self.rect_pending
    }

    /// Locally echo an edited name / description onto the **primary** node's
    /// properties (the build floater's Object tab commit): an `ObjectName` /
    /// `ObjectDescription` send is not echoed back by the simulator, so the
    /// floater's own copy is the one the summary and fields re-read.
    pub(crate) fn set_primary_name_description(
        &mut self,
        name: Option<&str>,
        description: Option<&str>,
    ) {
        if let Some(node) = self.selected.last_mut()
            && let Some(properties) = node.properties.as_mut()
        {
            if let Some(name) = name {
                name.clone_into(&mut properties.name);
            }
            if let Some(description) = description {
                description.clone_into(&mut properties.description);
            }
        }
    }

    /// The **primary** node's mutable properties, for the build floater's
    /// local echo of a permission / group edit (the simulator does not echo
    /// an `ObjectPermissions` / `ObjectGroup` back; the floater re-requests
    /// the properties to confirm).
    pub(crate) fn primary_properties_mut(&mut self) -> Option<&mut ObjectProperties> {
        self.selected
            .last_mut()
            .and_then(|node| node.properties.as_deref_mut())
    }

    /// Fold an `ObjectProperties` reply onto the node it belongs to (matched
    /// by grid-wide key). Returns whether a node took it.
    fn apply_properties(&mut self, properties: Box<ObjectProperties>) -> bool {
        for node in &mut self.selected {
            if node.full == properties.object_id {
                node.properties = Some(properties);
                return true;
            }
        }
        false
    }
}

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
    /// The selected object itself (a linkset root, or the picked part in
    /// edit-linked-parts mode).
    Root,
    /// A linkset child riding along with its selected root.
    Child,
    /// Tentatively swept by the live rubber band.
    Pending,
}

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
    /// The selected root's outline material.
    root: Handle<StandardMaterial>,
    /// A linkset child's outline material.
    child: Handle<StandardMaterial>,
    /// The tentative rubber-band outline material.
    pending: Handle<StandardMaterial>,
}

impl HighlightAssets {
    /// The material for `kind`.
    fn material(&self, kind: HighlightKind) -> Handle<StandardMaterial> {
        match kind {
            HighlightKind::Root => self.root.clone(),
            HighlightKind::Child => self.child.clone(),
            HighlightKind::Pending => self.pending.clone(),
        }
    }
}

impl FromWorld for HighlightAssets {
    /// Build the three inverted-hull outline materials once: unlit, front
    /// faces culled, so only the inflated shell's back-facing rim shows — an
    /// edge glow, not a fill.
    fn from_world(world: &mut World) -> Self {
        let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
        let mut outline = |color: Color| {
            materials.add(StandardMaterial {
                base_color: color,
                unlit: true,
                alpha_mode: AlphaMode::Blend,
                cull_mode: Some(bevy::render::render_resource::Face::Front),
                ..Default::default()
            })
        };
        let root = outline(ROOT_OUTLINE);
        let child = outline(CHILD_OUTLINE);
        let pending = outline(PENDING_OUTLINE);
        Self {
            root,
            child,
            pending,
        }
    }
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
pub(crate) struct EditSelectionPlugin;

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
            .add_systems(
                Update,
                (
                    handle_select_pointer.after(crate::gizmos::drive_gizmo_interaction),
                    clear_selection_on_escape,
                    ingest_selection_events,
                    sync_selection_wire,
                    apply_selection_highlight,
                )
                    .chain(),
            );
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
    /// The window, for the cursor position.
    windows: Query<'w, 's, &'static Window>,
    /// The world camera, to build pick rays and project candidate bounds.
    camera: Query<'w, 's, (&'static Camera, &'static GlobalTransform), With<ViewerCamera>>,
    /// Render layers, to exclude HUD / gizmo geometry from world picks.
    layers: Query<'w, 's, (Entity, &'static RenderLayers)>,
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

    // -- Press: classify what the gesture starts on. --------------------------
    if buttons.just_pressed(MouseButton::Left) && !alt {
        // A press over UI, over a gizmo handle, or with no cursor is not a
        // selection gesture.
        if gizmo.claims_pointer()
            || pointer_over_blocking_ui(&pointer.hover_map, &pointer.pickables, &pointer.node_sizes)
        {
            return;
        }
        let Some(cursor) = window.cursor_position() else {
            return;
        };
        let Ok(ray) = camera.viewport_to_world(camera_transform, cursor) else {
            return;
        };
        // The world pick, excluding HUD geometry exactly as the touch pick does.
        let exclude: HashSet<Entity> = pointer
            .layers
            .iter()
            .filter(|(_entity, layers)| {
                on_hud_layer(Some(layers)) || crate::gizmos::on_gizmo_layer(Some(layers))
            })
            .map(|(entity, _layers)| entity)
            .collect();
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
            selection.rect_pending =
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
        if !finished.extend {
            selection.selected.clear();
        }
        let pending = core::mem::take(&mut selection.rect_pending);
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
                selection.selected.clear();
                selection.insert(scoped, full, entity);
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
    context: Res<crate::input_context::InputContext>,
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
                    .rect_pending
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
    scene: Query<(), With<SceneObject>>,
    faces: Query<&Mesh3d, With<PrimFaceEntity>>,
    overlays: Query<(Entity, &ChildOf, &SelectionHighlightOverlay)>,
    mut commands: Commands,
) {
    // The desired overlay set: face entity → outline kind. A committed
    // outline (root, then child) wins over a tentative one when both apply.
    let mut desired: HashMap<Entity, HighlightKind> = HashMap::new();
    if tool.active {
        for node in selection.iter() {
            collect_faces(node.entity, &children, &scene, &faces, false, &mut desired);
        }
        for (_scoped, entity) in selection.rect_pending() {
            collect_faces(*entity, &children, &scene, &faces, true, &mut desired);
        }
    }
    // Despawn stale overlays, keep matching ones.
    for (overlay, child_of, marker) in overlays.iter() {
        match desired.get(&child_of.parent()) {
            Some(kind) if *kind == marker.kind => {
                desired.remove(&child_of.parent());
            }
            _stale => commands.entity(overlay).despawn(),
        }
    }
    // Spawn the missing ones: an inflated shell sharing the face's mesh,
    // front faces culled, so only the rim shows (the silhouette-glow
    // approximation).
    for (face, kind) in desired {
        let Ok(mesh) = faces.get(face) else {
            continue;
        };
        commands.spawn((
            Mesh3d(mesh.0.clone()),
            MeshMaterial3d(assets.material(kind)),
            Transform::from_scale(Vec3::splat(OUTLINE_INFLATE)),
            NotShadowCaster,
            SelectionHighlightOverlay { kind },
            ChildOf(face),
        ));
    }
}

/// Collect every face-mesh entity under `root` (the object's own faces and its
/// linkset children's) into `desired`, colouring the selected object's own
/// faces as [`HighlightKind::Root`] and any linkset child's (a descendant
/// carrying its own [`SceneObject`]) as [`HighlightKind::Child`] — the
/// reference's parent / child silhouette split. A committed outline wins over
/// a tentative ([`HighlightKind::Pending`]) one when both apply.
fn collect_faces(
    root: Entity,
    children: &Query<&Children>,
    scene: &Query<(), With<SceneObject>>,
    faces: &Query<&Mesh3d, With<PrimFaceEntity>>,
    pending: bool,
    desired: &mut HashMap<Entity, HighlightKind>,
) {
    let mut stack = vec![(root, false)];
    while let Some((entity, mut is_child)) = stack.pop() {
        // Crossing into a descendant that is its own scene object means the
        // subtree below belongs to a linkset child.
        if entity != root && scene.contains(entity) {
            is_child = true;
        }
        if faces.contains(entity) {
            let kind = if pending {
                HighlightKind::Pending
            } else if is_child {
                HighlightKind::Child
            } else {
                HighlightKind::Root
            };
            desired
                .entry(entity)
                .and_modify(|existing| {
                    // Committed beats pending; root beats child.
                    let rank = |kind: HighlightKind| match kind {
                        HighlightKind::Root => 0_u8,
                        HighlightKind::Child => 1_u8,
                        HighlightKind::Pending => 2_u8,
                    };
                    if rank(kind) < rank(*existing) {
                        *existing = kind;
                    }
                })
                .or_insert(kind);
        }
        if let Ok(list) = children.get(entity) {
            for child in list.iter() {
                stack.push((child, is_child));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{SelectionSet, WireSelection};
    use bevy::prelude::Entity;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{CircuitId, ObjectKey, RegionLocalObjectId, ScopedObjectId, Uuid};

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
}
