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

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::ecs::system::SystemParam;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::input_focus::InputFocus;
use bevy::light::NotShadowCaster;
use bevy::picking::hover::HoverMap;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use sl_client_bevy::{
    Command, DeRezDestination, FolderType, ObjectKey, ObjectProperties, PrimFaceId, ScopedObjectId,
    SlCommand, SlEvent, SlSessionEvent, TransactionId, Uuid, texture_face_uv_transform,
};

use crate::camera::ViewerCamera;
use crate::edit_math::rect_selects;
use crate::edit_tool::{EditTool, EditToolState};
use crate::face_material::{FaceMaterial, inert_face_material};
use crate::gizmos::GizmoInteraction;
use crate::hud::on_hud_layer;
use crate::hud_pick::pointer_over_blocking_ui;
use crate::input_context::InputContext;
use crate::inventory::InventoryModel;
use crate::object_menu::ObjectPicker;
use crate::objects::{
    FaceTextureDebug, ObjectCategory, ObjectSlMotion, ObjectState, PrimFaceEntity, SceneObject,
};
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
    /// The **selected faces** of this object, for the Select Face tool
    /// ([`EditTool::SelectFace`]) and the Texture tab that edits them: `None`
    /// means the whole object (every face) — the default for an ordinary
    /// object selection — and `Some(set)` means exactly those Linden face
    /// indices (the reference's per-`LLSelectNode` texture-entry flags).
    pub(crate) faces: Option<HashSet<PrimFaceId>>,
}

impl SelectedNode {
    /// This node's region-scoped id — what the link / unlink commands address.
    pub(crate) const fn scoped(&self) -> ScopedObjectId {
        self.scoped
    }

    /// The extended properties the simulator returned for this node, or `None`
    /// until its `ObjectProperties` reply lands.
    pub(crate) fn properties(&self) -> Option<&ObjectProperties> {
        self.properties.as_deref()
    }
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
            faces: None,
        });
    }

    /// The Select Face tool's **plain click**: replace the whole selection with
    /// exactly this one object and its one face (the reference's
    /// `deselectAll()` + `selectObjectOnly(obj, face)`).
    pub(crate) fn select_only_face(
        &mut self,
        scoped: ScopedObjectId,
        full: ObjectKey,
        entity: Entity,
        face: PrimFaceId,
    ) {
        let mut faces = HashSet::new();
        faces.insert(face);
        // Keep the object's existing node (its `ObjectProperties` intact) when it
        // was already selected — only its face set changes — so re-picking a face
        // on the same object does not blank the floater (see [`select_only`]).
        if let Some(index) = self.selected.iter().position(|node| node.scoped == scoped) {
            let mut node = self.selected.remove(index);
            node.faces = Some(faces);
            self.selected.clear();
            self.selected.push(node);
        } else {
            self.selected.clear();
            self.selected.push(SelectedNode {
                scoped,
                full,
                entity,
                properties: None,
                faces: Some(faces),
            });
        }
    }

    /// Select exactly `scoped`, dropping every other object — the plain-click
    /// replace of the object-selection tool. Crucially, if the object was
    /// **already** selected it keeps its existing node (its `ObjectProperties`
    /// name / owner / permissions intact), so re-clicking the same object does
    /// not blank the build floater; a re-select of an already-synced object is
    /// not re-requested on the wire, so a fresh `properties: None` node would
    /// stay blank forever.
    pub(crate) fn select_only(&mut self, scoped: ScopedObjectId, full: ObjectKey, entity: Entity) {
        if let Some(index) = self.selected.iter().position(|node| node.scoped == scoped) {
            let node = self.selected.remove(index);
            self.selected.clear();
            self.selected.push(node);
        } else {
            self.selected.clear();
            self.insert(scoped, full, entity);
        }
    }

    /// The Select Face tool's **Shift-click**: extend / toggle a face in the set
    /// (the reference's `addAsIndividual` / `remove`). If the object is not
    /// selected it is added with just this face; if the object is selected but
    /// this face is not in its set the face is added; if the face is already in
    /// the set it is removed — and if that empties the set the object drops out
    /// of the selection (cleaner than the reference's known no-op-on-last bug).
    pub(crate) fn toggle_face(
        &mut self,
        scoped: ScopedObjectId,
        full: ObjectKey,
        entity: Entity,
        face: PrimFaceId,
    ) {
        if let Some(index) = self.selected.iter().position(|node| node.scoped == scoped) {
            let emptied = {
                let Some(node) = self.selected.get_mut(index) else {
                    return;
                };
                let set = node.faces.get_or_insert_with(HashSet::new);
                if !set.remove(&face) {
                    set.insert(face);
                }
                set.is_empty()
            };
            if emptied {
                self.selected.remove(index);
            } else {
                // Promote the touched object to primary (the last-clicked object
                // is the alignment reference the Texture tab reads).
                let node = self.selected.remove(index);
                self.selected.push(node);
            }
            return;
        }
        let mut faces = HashSet::new();
        faces.insert(face);
        self.selected.push(SelectedNode {
            scoped,
            full,
            entity,
            properties: None,
            faces: Some(faces),
        });
    }

    /// The **primary** selection's selected faces: `None` for the whole object
    /// (every face), else the chosen Linden face indices. The Texture tab reads
    /// this to decide which faces an `ObjectImage` edit hits.
    pub(crate) fn primary_faces(&self) -> Option<&HashSet<PrimFaceId>> {
        self.selected.last().and_then(|node| node.faces.as_ref())
    }

    /// Remove an object from the selection (a no-op if absent).
    pub(crate) fn remove(&mut self, scoped: ScopedObjectId) {
        self.selected.retain(|node| node.scoped != scoped);
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
    /// A promoted node drops its part's [`ObjectProperties`]; the wire diff
    /// ([`sync_selection_wire`]) then selects the root and re-requests them.
    pub(crate) fn promote_to_roots(&mut self, objects: &ObjectState) -> bool {
        let mut promoted: Vec<SelectedNode> = Vec::new();
        for node in &self.selected {
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
        let changed = promoted.len() != self.selected.len()
            || promoted
                .iter()
                .zip(&self.selected)
                .any(|(new, old)| new.scoped != old.scoped);
        if changed {
            self.selected = promoted;
        }
        changed
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

/// The in-world object an inventory drag is currently hovering, if it accepts the
/// drop — set by [`crate::inventory_drag`] each frame while a drag is active and
/// consumed by [`apply_drag_hover_highlight`] to draw the accept / foreign
/// outline (the reference's `highlightObjectAndFamily` during a drag).
#[derive(Resource, Debug, Default)]
pub(crate) struct DragHoverHighlight {
    /// The hovered object's root render entity and whether it is foreign (not
    /// owned, so the outline is red), or `None` when nothing droppable is hovered.
    pub(crate) hover: Option<DragHover>,
}

/// One drag-hover target: the object's root render entity and its ownership tint.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DragHover {
    /// The hovered object's root render entity (a [`SceneObject`]).
    pub(crate) root: Entity,
    /// Whether the object is **foreign** (not owned / not modifiable but accepts
    /// the drop) — drawn red rather than the green accept colour.
    pub(crate) foreign: bool,
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
    ui_claim: Res<'w, crate::hud_pick::UiPointerClaim>,
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
            let exclude: HashSet<Entity> = pointer
                .layers
                .iter()
                .filter(|(_entity, layers)| {
                    on_hud_layer(Some(layers)) || crate::gizmos::on_gizmo_layer(Some(layers))
                })
                .map(|(entity, _layers)| entity)
                .collect();
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
    // outline (primary, then root, then child) wins over a tentative one when
    // both apply.
    let mut desired: HashMap<Entity, HighlightKind> = HashMap::new();
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
                &faces,
                root_kind,
                HighlightKind::Child,
                &mut desired,
            );
        }
        for (_scoped, entity) in selection.rect_pending() {
            collect_faces(
                *entity,
                &children,
                &scene,
                &faces,
                HighlightKind::Pending,
                HighlightKind::Pending,
                &mut desired,
            );
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

/// Draw the drag-drop hover outline: while an inventory drag hovers an object
/// that accepts the drop ([`DragHoverHighlight`]), every face of that object (and
/// its linkset family) gets an outline overlay — green when you may edit it, red
/// when it is foreign (the reference's `highlightObjectAndFamily` during a drag).
/// A separate overlay from the selection's, so the two reconcilers never fight.
fn apply_drag_hover_highlight(
    hover: Res<DragHoverHighlight>,
    assets: Res<HighlightAssets>,
    children: Query<&Children>,
    scene: Query<(), With<SceneObject>>,
    faces: Query<&Mesh3d, With<PrimFaceEntity>>,
    overlays: Query<(Entity, &ChildOf, &DragHoverOverlay)>,
    mut commands: Commands,
) {
    let mut desired: HashMap<Entity, HighlightKind> = HashMap::new();
    if let Some(target) = hover.hover {
        let kind = if target.foreign {
            HighlightKind::DropForeign
        } else {
            HighlightKind::DropAccept
        };
        // One colour for the whole family (the drop targets this object) — pass
        // the same kind for the root and its children.
        collect_faces(
            target.root,
            &children,
            &scene,
            &faces,
            kind,
            kind,
            &mut desired,
        );
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
    // Spawn the missing ones (the same inflated inverted-hull shell the selection
    // outline uses).
    for (face, kind) in desired {
        let Ok(mesh) = faces.get(face) else {
            continue;
        };
        commands.spawn((
            Mesh3d(mesh.0.clone()),
            MeshMaterial3d(assets.material(kind)),
            Transform::from_scale(Vec3::splat(OUTLINE_INFLATE)),
            NotShadowCaster,
            DragHoverOverlay { kind },
            ChildOf(face),
        ));
    }
}

/// Collect every face-mesh entity under `root` (the object's own faces and its
/// linkset children's) into `desired`, colouring the selected object's own root
/// faces as `root_kind` and any linkset child's (a descendant carrying its own
/// [`SceneObject`]) as `child_kind` — the reference's parent / child silhouette
/// split, with the primary root distinguished. A stronger outline (primary,
/// then root, then child) wins over a tentative ([`HighlightKind::Pending`])
/// one when both apply.
fn collect_faces(
    root: Entity,
    children: &Query<&Children>,
    scene: &Query<(), With<SceneObject>>,
    faces: &Query<&Mesh3d, With<PrimFaceEntity>>,
    root_kind: HighlightKind,
    child_kind: HighlightKind,
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
            let kind = if is_child { child_kind } else { root_kind };
            desired
                .entry(entity)
                .and_modify(|existing| {
                    // Primary beats root beats child beats pending.
                    let rank = |kind: HighlightKind| match kind {
                        HighlightKind::Primary => 0_u8,
                        HighlightKind::Root => 1_u8,
                        HighlightKind::Child => 2_u8,
                        HighlightKind::Pending => 3_u8,
                        // Drag-hover kinds never merge with the selection kinds
                        // (they are reconciled by a separate system over their own
                        // overlay), so their relative rank is immaterial.
                        HighlightKind::DropAccept => 4_u8,
                        HighlightKind::DropForeign => 5_u8,
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
        let Ok((mesh, _marker, _debug)) = cursor_faces.get(face) else {
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
        commands.spawn((
            Mesh3d(mesh.0.clone()),
            MeshMaterial3d(material),
            NotShadowCaster,
            FaceCursorOverlay,
            ChildOf(face),
        ));
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
        if let Ok((_mesh, marker, debug)) = cursor_faces.get(entity)
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

#[cfg(test)]
mod tests {
    use super::{HighlightAssets, SelectionSet, WireSelection};
    use crate::face_material::FaceMaterial;
    use bevy::app::{App, TaskPoolPlugin};
    use bevy::asset::{AssetApp as _, AssetPlugin};
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

    /// Promoting a selection of already-root (or untracked) objects is a no-op:
    /// nothing to jump, so the set and its primary are unchanged. (The
    /// child→root jump needs a populated `ObjectState` and is exercised live.)
    #[test]
    fn promote_to_roots_is_a_noop_when_all_roots() {
        let objects = crate::objects::ObjectState::default();
        let mut set = SelectionSet::default();
        set.insert(scoped(1), full(1), Entity::PLACEHOLDER);
        set.insert(scoped(2), full(2), Entity::PLACEHOLDER);
        assert!(
            !set.promote_to_roots(&objects),
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
}
