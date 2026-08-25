//! State the world layer reads and a feature owns.
//!
//! The world — rendering, avatars, objects, terrain — has to consult things the
//! features above it maintain: a material preview depends on what the build
//! tool selected, a name tag's colour on whether the resident is a friend or
//! muted, a beacon on what the map is tracking. Left in the feature that owns
//! them, those types make the world depend on what sits on top of it.
//!
//! So the *types* live here and the *systems* stay with their feature:
//! `edit_selection` still drives [`SelectionSet`], `people` still fills the
//! friends model, and the world reads either without knowing who wrote it.
//!
//! Nothing here reaches back into a feature: the crate depends on `bevy` and
//! `sl-client-bevy` and on nothing else in the viewer. That is what lets the
//! world layer be lifted out after it, and it is a property worth keeping —
//! a single upward reference added here would put the whole feature tier back
//! underneath the world.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;

use bevy::camera::visibility::RenderLayers;
use bevy::picking::hover::HoverMap;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use sl_client_bevy::{
    AgentKey, AssetUpdateLocation, AttachmentPoint, AvatarName, BodyPhysics, ChatSessionKind,
    Command, ControlFlags, DisplayName, Friend, FriendKey, FriendPresence, FriendRights, GroupKey,
    GroupMembership, ImSessionId, InventoryKey, JointOverrides, LightData, MAX_FACES, MeshKey,
    MuteEntry, MuteFlags, MuteType, Object, ObjectExtraParams, ObjectKey, ObjectProperties,
    ParticleSystem, PrimFaceId, PrimLod, PrimShapeParams, Priority, ReflectionProbe,
    ReflectionProbeFlags, RegionCoordinates, RegionHandle, RestoreItem, Rotation, ScopedObjectId,
    ScriptLanguage, ScriptTarget, ScriptUploadLocation, SculptOrMeshKey, SkeletalDeformations,
    SlCommand, SurfaceInfo, TaskInventoryKey, TerrainPatch, TextureAnimation, TextureFace,
    TextureKey, TreeLod, Uuid, Vector, VolumeDeformations, avatar_texture, decode_texture_entry,
    pcode, texture_face_uv_transform,
};
use sl_terrain::TerrainComposition;
use sl_viewer_kit::coords::{sl_rotation_to_quat, sl_to_bevy_rotation};
use sl_viewer_settings::ViewerSettings;

/// Whether the autorespond mode is on (the reference `FSAutorespondMode`).
/// Account-scoped and persisted.
pub const SETTING_AUTORESPOND_MODE: &str = "AutorespondMode";

/// Whether the autorespond-to-non-friends mode is on (the reference
/// `FSAutorespondNonFriendsMode`). Account-scoped and persisted.
pub const SETTING_AUTORESPOND_NON_FRIENDS_MODE: &str = "AutorespondNonFriendsMode";

/// Whether either autorespond mode is on, which is the one question three
/// different tiers ask: the IM auto-reply decides whether to answer, the
/// Comm menu ticks its toggles, and a name tag badges the own avatar. Kept
/// here so the "either mode counts" rule has exactly one statement.
#[must_use]
pub fn shows_autoresponse(settings: Option<&ViewerSettings>) -> bool {
    [
        SETTING_AUTORESPOND_MODE,
        SETTING_AUTORESPOND_NON_FRIENDS_MODE,
    ]
    .into_iter()
    .any(|name| settings.is_some_and(|settings| settings.store().get_bool(name).unwrap_or(false)))
}

/// One selected object in the [`SelectionSet`].
#[derive(Debug, Clone)]
pub struct SelectedNode {
    /// The object's region-scoped id — what the select / deselect / update
    /// commands address.
    pub scoped: ScopedObjectId,
    /// The object's grid-wide key — what the `ObjectProperties` reply is
    /// matched back by.
    pub full: ObjectKey,
    /// The object's scene entity (the linkset root when whole-linkset
    /// selection put it here).
    pub entity: Entity,
    /// The extended properties the simulator returned for the selection —
    /// permission masks, owner, creator, names — or `None` until the
    /// `ObjectProperties` reply lands.
    pub properties: Option<Box<ObjectProperties>>,
    /// The **selected faces** of this object, for the Select Face tool
    /// ([`EditTool::SelectFace`]) and the Texture tab that edits them: `None`
    /// means the whole object (every face) — the default for an ordinary
    /// object selection — and `Some(set)` means exactly those Linden face
    /// indices (the reference's per-`LLSelectNode` texture-entry flags).
    pub faces: Option<HashSet<PrimFaceId>>,
}

impl SelectedNode {
    /// This node's region-scoped id — what the link / unlink commands address.
    #[must_use]
    pub const fn scoped(&self) -> ScopedObjectId {
        self.scoped
    }

    /// The extended properties the simulator returned for this node, or `None`
    /// until its `ObjectProperties` reply lands.
    #[must_use]
    pub fn properties(&self) -> Option<&ObjectProperties> {
        self.properties.as_deref()
    }
}

/// The maintained selection set — the shared state the edit floater, the
/// numeric fields, the transform gizmos, and the future linking / per-aspect
/// editors all read. See the [module documentation](self).
#[derive(Resource, Debug, Default)]
pub struct SelectionSet {
    /// The selected objects, in selection order; the **primary** is the last.
    selected: Vec<SelectedNode>,
    /// The objects a live rubber-band drag currently sweeps (tentative,
    /// highlight-only until the drag commits).
    rect_pending: Vec<(ScopedObjectId, Entity)>,
}

impl SelectionSet {
    /// Whether `scoped` is in the selection.
    #[must_use]
    pub fn is_selected(&self, scoped: ScopedObjectId) -> bool {
        self.selected.iter().any(|node| node.scoped == scoped)
    }

    /// Add an object to the selection (a no-op if already present), making it
    /// the primary.
    pub fn insert(&mut self, scoped: ScopedObjectId, full: ObjectKey, entity: Entity) {
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
    pub fn select_only_face(
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
    pub fn select_only(&mut self, scoped: ScopedObjectId, full: ObjectKey, entity: Entity) {
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
    pub fn toggle_face(
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
    #[must_use]
    pub fn primary_faces(&self) -> Option<&HashSet<PrimFaceId>> {
        self.selected.last().and_then(|node| node.faces.as_ref())
    }

    /// Remove an object from the selection (a no-op if absent).
    pub fn remove(&mut self, scoped: ScopedObjectId) {
        self.selected.retain(|node| node.scoped != scoped);
    }

    /// Remove every selected object with the persistent id `id` (a no-op if
    /// absent) — the derender path (`viewer-derender-blacklist`), which knows a
    /// full id rather than a region-scoped one, dropping an object it is about
    /// to despawn out of the selection first (the reference's `stopEditing` on
    /// a derendered edit target).
    pub fn remove_by_full_id(&mut self, id: Uuid) {
        self.selected.retain(|node| node.full.uuid() != id);
    }

    /// The selected nodes, in selection order.
    ///
    /// Paired with [`Self::replace_nodes`] for logic that has to rebuild the
    /// selection from world knowledge this layer deliberately lacks — see
    /// `edit_selection::promote_selection_to_roots`.
    #[must_use]
    pub fn nodes(&self) -> &[SelectedNode] {
        &self.selected
    }

    /// Replace the selection wholesale, keeping the last entry primary.
    pub fn replace_nodes(&mut self, nodes: Vec<SelectedNode>) {
        self.selected = nodes;
    }

    /// The tentative rubber-band sweep, for the drag that owns it.
    pub const fn rect_pending_mut(&mut self) -> &mut Vec<(ScopedObjectId, Entity)> {
        &mut self.rect_pending
    }

    /// Empty the selection (both committed and tentative).
    pub fn clear(&mut self) {
        self.selected.clear();
        self.rect_pending.clear();
    }

    /// The selected objects, in selection order.
    pub fn iter(&self) -> impl Iterator<Item = &SelectedNode> {
        self.selected.iter()
    }

    /// The **primary** selection — the most recently selected object; the one
    /// the numeric fields display and the local grid frame follows.
    #[must_use]
    pub fn primary(&self) -> Option<&SelectedNode> {
        self.selected.last()
    }

    /// How many objects are selected.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.selected.len()
    }

    /// Whether nothing is selected.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.selected.is_empty()
    }

    /// The tentative rubber-band sweep, for the highlight pass.
    #[must_use]
    pub fn rect_pending(&self) -> &[(ScopedObjectId, Entity)] {
        &self.rect_pending
    }

    /// Locally echo an edited name / description onto the **primary** node's
    /// properties (the build floater's Object tab commit): an `ObjectName` /
    /// `ObjectDescription` send is not echoed back by the simulator, so the
    /// floater's own copy is the one the summary and fields re-read.
    pub fn set_primary_name_description(&mut self, name: Option<&str>, description: Option<&str>) {
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
    pub fn primary_properties_mut(&mut self) -> Option<&mut ObjectProperties> {
        self.selected
            .last_mut()
            .and_then(|node| node.properties.as_deref_mut())
    }

    /// Fold an `ObjectProperties` reply onto the node it belongs to (matched
    /// by grid-wide key). Returns whether a node took it.
    pub fn apply_properties(&mut self, properties: Box<ObjectProperties>) -> bool {
        for node in &mut self.selected {
            if node.full == properties.object_id {
                node.properties = Some(properties);
                return true;
            }
        }
        false
    }
}

/// Which manipulator the build tool drives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EditTool {
    /// The translate gizmo (axis arrows + planar handles).
    #[default]
    Move,
    /// The rotate gizmo (axis rings).
    Rotate,
    /// The scale gizmo (face + corner handles).
    Stretch,
    /// The **Select Face** tool (the reference's `LLToolFace`, its
    /// `radio select face`): no transform gizmo — a click picks a prim face into
    /// the per-face texture-entry selection the Texture tab
    /// (`edit_texture`) edits, `Shift`-click builds a multi-face set.
    SelectFace,
    /// The **Create** tool (the reference's `LLToolPlacer` / `LLToolCompCreate`):
    /// no transform gizmo — a click on a surface rezzes the base type picked in
    /// the create panel (`edit_create`) at the ray-cast build point and
    /// drops into edit on the new object.
    Create,
}

impl EditTool {
    /// This tool's index into [`BUILD_TOOLS`] — the radio option it selects.
    #[must_use]
    pub fn radio_index(self) -> usize {
        BUILD_TOOLS
            .iter()
            .position(|&tool| tool == self)
            .unwrap_or(0)
    }
}

/// The build tool's shared state. See the [module documentation](self).
#[expect(
    clippy::struct_excessive_bools,
    reason = "the flags mirror the reference viewer's independent build-tool toggles \
              (EditLinkedParts, ScaleUniform, SnapEnabled) plus the tool's own active bit; \
              none is a state machine in disguise"
)]
#[derive(Resource, Debug)]
pub struct EditToolState {
    /// Whether the build tool is active (the floater is open): selection
    /// clicks, gizmos, and the touch-suppression all key off this.
    pub active: bool,
    /// The manipulator picked in the floater (the resting tool).
    pub tool: EditTool,
    /// A manipulator temporarily forced by a held modifier — the reference's
    /// `Ctrl` = rotate / `Ctrl+Shift` = stretch while held
    /// (`LLToolCompTranslate::handleHover`'s mask dispatch). Cleared on
    /// release; [`effective_tool`](Self::effective_tool) folds it in.
    pub held_override: Option<EditTool>,
    /// Edit linked parts: select and edit individual linkset prims instead of
    /// whole linksets (the reference's `EditLinkedParts`).
    pub edit_linked: bool,
    /// Stretch both sides: scale about the selection centre instead of
    /// holding the opposite face in place (the reference's `ScaleUniform`).
    pub stretch_both: bool,
    /// Whether grid snapping is on (the reference's `SnapEnabled`).
    pub snap: bool,
    /// The grid unit, in metres (the reference's `GridResolution`).
    pub grid_unit: f32,
    /// The grid frame the gizmos align to.
    pub frame: GridFrame,
}

impl Default for EditToolState {
    /// Reference-faithful defaults: move tool, whole-linkset selection, snap
    /// on at a half-metre grid, world frame.
    fn default() -> Self {
        Self {
            active: false,
            tool: EditTool::Move,
            held_override: None,
            edit_linked: false,
            stretch_both: false,
            snap: true,
            grid_unit: DEFAULT_GRID_UNIT,
            frame: GridFrame::World,
        }
    }
}

impl EditToolState {
    /// The manipulator actually in effect: a held modifier override
    /// (`Ctrl` = rotate, `Ctrl+Shift` = stretch), or the floater's resting
    /// tool.
    #[must_use]
    pub fn effective_tool(&self) -> EditTool {
        self.held_override.unwrap_or(self.tool)
    }
}

/// The current material mode / channel the Texture tab edits — the resolved
/// `(matmedia, material-type, pbr-type)` selection, mirrored from the three
/// selector widgets each frame so the visibility system and the channel editors
/// read one place. Mirrors the reference's `mComboMatMedia` /
/// `mRadioMaterialType` / `mRadioPbrType` current indices.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatModeState {
    /// The `matmedia` selection ([`MATMEDIA_MATERIAL`] / [`MATMEDIA_PBR`]).
    pub matmedia: usize,
    /// The Material-mode map channel ([`MATTYPE_DIFFUSE`] / [`MATTYPE_NORMAL`] /
    /// [`MATTYPE_SPECULAR`]).
    pub mat_type: usize,
    /// The PBR-mode channel ([`PBRTYPE_RENDER_MATERIAL`] …).
    pub pbr_type: usize,
}

impl Default for MatModeState {
    /// The tab opens in Material / Diffuse mode with the render-material PBR
    /// channel pre-selected, exactly as the reference initialises its selectors.
    fn default() -> Self {
        Self {
            matmedia: MATMEDIA_MATERIAL,
            mat_type: MATTYPE_DIFFUSE,
            pbr_type: PBRTYPE_RENDER_MATERIAL,
        }
    }
}

/// The active PBR texture channel a transform edits, or the whole material when
/// the render-material channel is selected — the resolved form of
/// [`MatModeState::pbr_type`] the PBR display path keys by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PbrChannel {
    /// The complete render material (its asset id), not a single texture.
    Material,
    /// The base-colour texture.
    BaseColor,
    /// The metallic-roughness texture.
    MetallicRoughness,
    /// The emissive texture.
    Emissive,
    /// The normal texture.
    Normal,
}

impl MatModeState {
    /// Whether the Material (Blinn-Phong) mode is active.
    #[must_use]
    pub const fn is_material(self) -> bool {
        self.matmedia == MATMEDIA_MATERIAL
    }

    /// Whether the PBR (GLTF) mode is active.
    #[must_use]
    pub const fn is_pbr(self) -> bool {
        self.matmedia == MATMEDIA_PBR
    }

    /// The active PBR channel for the current `pbr_type` selection.
    #[must_use]
    pub const fn pbr_channel(self) -> PbrChannel {
        match self.pbr_type {
            PBRTYPE_BASE_COLOR => PbrChannel::BaseColor,
            PBRTYPE_METALLIC => PbrChannel::MetallicRoughness,
            PBRTYPE_EMISSIVE => PbrChannel::Emissive,
            PBRTYPE_NORMAL => PbrChannel::Normal,
            _material => PbrChannel::Material,
        }
    }
}

/// The default grid unit, in metres — the reference's `GridResolution`.
pub const DEFAULT_GRID_UNIT: f32 = 0.5;

/// The tool-mode radio options, in the order they appear in the floater (the
/// reference's `move` / `rotate` / `stretch`). The one place the index↔tool
/// mapping lives, so `spawn_build_floater` and the two sync systems agree.
pub const BUILD_TOOLS: [EditTool; 5] = [
    EditTool::Create,
    EditTool::Move,
    EditTool::Rotate,
    EditTool::Stretch,
    EditTool::SelectFace,
];

/// The grid frame the gizmos align to and snap in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GridFrame {
    /// The world axes (the reference's `GRID_MODE_WORLD`).
    #[default]
    World,
    /// The primary selection's own axes (`GRID_MODE_LOCAL`).
    Local,
    /// A reference object's axes (`GRID_MODE_REF_OBJECT`). Modelled now so the
    /// snapping code handles it, but only settable once the grid-options task
    /// (`viewer-build-grid-options`) ships its *Use Selection for Grid*
    /// command.
    Reference,
}

/// The `matmedia` combo index for the legacy **Material** (Blinn-Phong) mode —
/// diffuse texture plus optional normal / specular maps.
pub const MATMEDIA_MATERIAL: usize = 0;

/// The `matmedia` combo index for the **PBR** (GLTF) render-material mode.
pub const MATMEDIA_PBR: usize = 1;

/// The `radio_material_type` index for the diffuse **Texture** channel.
pub const MATTYPE_DIFFUSE: usize = 0;

/// The `radio_material_type` index for the **Bumpiness** (normal-map) channel.
pub const MATTYPE_NORMAL: usize = 1;

/// The `radio_material_type` index for the **Shininess** (specular-map) channel.
pub const MATTYPE_SPECULAR: usize = 2;

/// The `radio_pbr_type` index for the whole render **material** (the material-id
/// swatch — assign or clear a stored GLTF material asset).
pub const PBRTYPE_RENDER_MATERIAL: usize = 0;

/// The `radio_pbr_type` index for the PBR **base-colour** channel transform.
pub const PBRTYPE_BASE_COLOR: usize = 1;

/// The `radio_pbr_type` index for the PBR **metallic-roughness** channel
/// transform.
pub const PBRTYPE_METALLIC: usize = 2;

/// The `radio_pbr_type` index for the PBR **emissive** channel transform.
pub const PBRTYPE_EMISSIVE: usize = 3;

/// The `radio_pbr_type` index for the PBR **normal** channel transform.
pub const PBRTYPE_NORMAL: usize = 4;

/// The most entries the mute list holds — the reference's `MuteListLimit`
/// debug setting, whose default this matches. A mute past the limit is
/// refused client-side (the server silently drops it) and reported as
/// `MuteLimitReached`.
pub const MUTE_LIST_LIMIT: usize = 1000;

/// The agent's mute list: every muted entry (agents and objects alike — the
/// tag colouring only ever looks up agent ids).
#[derive(Resource, Debug, Default)]
pub struct MuteModel {
    /// The entries, in the order the list was received / mutes were added.
    entries: Vec<MuteEntry>,
    /// The non-nil muted ids, derived from [`Self::entries`] — the hot-path
    /// `is_muted` index.
    muted: HashSet<Uuid>,
    /// Whether the one-per-session `RequestMuteList` has been sent.
    requested: bool,
    /// Bumped on every change to [`Self::entries`], so the block-list view
    /// rebuilds exactly when the list actually moved.
    revision: u64,
}

impl MuteModel {
    /// Claim the one-per-session `RequestMuteList` slot: true the first time
    /// it is called, false every time after. The latch lives with the model so
    /// a second requester cannot race a duplicate request onto the wire.
    pub const fn claim_request(&mut self) -> bool {
        if self.requested {
            return false;
        }
        self.requested = true;
        true
    }

    /// Whether `id` is on the mute list at all (any aspect).
    #[must_use]
    pub fn is_muted(&self, id: Uuid) -> bool {
        self.muted.contains(&id)
    }

    /// Whether the aspect whose *exception* bit is `allow_mask` (one of the
    /// `MuteFlags::ALLOW_*` constants) is actually muted for `id`: the id is on
    /// the list **and** the entry does not carry that exception.
    #[must_use]
    pub fn is_muted_aspect(&self, id: Uuid, allow_mask: u32) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.id == id && !entry.flags.contains(allow_mask))
    }

    /// The whole list, in display order.
    #[must_use]
    pub fn entries(&self) -> &[MuteEntry] {
        &self.entries
    }

    /// The list revision — a view stores the value it last built at and
    /// rebuilds when it advances.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Whether the list is at [`MUTE_LIST_LIMIT`] and refuses further mutes.
    #[must_use]
    pub const fn is_full(&self) -> bool {
        self.entries.len() >= MUTE_LIST_LIMIT
    }

    /// Whether a **by-name** entry already carries `name` (case-insensitively)
    /// — the duplicate check a by-name block needs, since such entries share a
    /// nil id and nothing else tells them apart. Entries with an id are not
    /// consulted: the reference keeps its by-name mutes in a separate set, so
    /// blocking an object *by name* is allowed even when a same-named avatar is
    /// blocked by id.
    #[must_use]
    pub fn has_by_name(&self, name: &str) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.id.is_nil() && entry.name.eq_ignore_ascii_case(name))
    }

    /// The entry matching `id` / `name`, if any (see the module docs for how a
    /// nil id falls back to the name).
    #[must_use]
    pub fn entry(&self, id: Uuid, name: &str) -> Option<&MuteEntry> {
        self.entries
            .iter()
            .find(|entry| same_target(entry, id, name))
    }

    /// Record a locally-issued mute so consumers update without waiting for a
    /// list re-request. An existing entry for the same target is **replaced**
    /// (that is how a flag edit lands, since it re-sends the whole entry).
    pub fn note_mute(&mut self, entry: MuteEntry) {
        if let Some(existing) = self
            .entries
            .iter_mut()
            .find(|candidate| same_target(candidate, entry.id, &entry.name))
        {
            *existing = entry;
        } else {
            self.entries.push(entry);
        }
        self.reindex();
    }

    /// Record a locally-issued unmute (see [`Self::note_mute`]).
    pub fn note_unmute(&mut self, id: Uuid, name: &str) {
        self.entries.retain(|entry| !same_target(entry, id, name));
        self.reindex();
    }

    /// Replace the whole list (a received `MuteList`).
    pub fn replace(&mut self, entries: Vec<MuteEntry>) {
        self.entries = entries;
        self.reindex();
    }

    /// Rebuild the derived id index and bump the revision.
    fn reindex(&mut self) {
        self.muted = self
            .entries
            .iter()
            .map(|entry| entry.id)
            .filter(|id| !id.is_nil())
            .collect();
        self.revision = self.revision.wrapping_add(1);
    }
}

/// Whether `entry` is the mute list's record of `id` / `name`: by id when the
/// id is non-nil, else by case-folded name (a [`MuteType::ByName`] entry).
fn same_target(entry: &MuteEntry, id: Uuid, name: &str) -> bool {
    if id.is_nil() {
        entry.id.is_nil() && entry.name.eq_ignore_ascii_case(name)
    } else {
        entry.id == id
    }
}

/// A short, readable stand-in for an unresolved agent id — its first eight hex
/// digits (mirrors `conversations`'s placeholder).
#[must_use]
pub fn short_id(id: Uuid) -> String {
    id.simple().to_string().chars().take(8).collect()
}

/// One friend's cached state: the friendship rights in both directions and the
/// last-known presence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FriendEntry {
    /// The rights this agent grants the friend.
    rights_granted: FriendRights,
    /// The rights the friend grants this agent.
    rights_received: FriendRights,
    /// Whether the friend is currently known-online (`false` is "offline or not
    /// visible", never provably offline).
    online: bool,
}

impl FriendEntry {
    /// A fresh entry from a login / snapshot [`Friend`] record, offline until a
    /// presence notification says otherwise.
    const fn new(friend: Friend, online: bool) -> Self {
        Self {
            rights_granted: friend.rights_granted,
            rights_received: friend.rights_received,
            online,
        }
    }
}

/// The pure friends model: the buddy cache keyed by friend id, the resolved name
/// cache, and a revision stamp bumped on every change so the view rebuilds only
/// when something actually moved. Fed solely from the event stream.
#[derive(Resource, Debug, Default)]
pub struct FriendsModel {
    /// The buddy list, by friend id.
    friends: BTreeMap<FriendKey, FriendEntry>,
    /// Last-seen legacy display name per agent, for the row labels.
    names: BTreeMap<AgentKey, String>,
    /// The name the user gave a friend instead, if any (already quoted, as the
    /// name cache shows it) — mirrored from the contact-set store by
    /// `contact_sets::apply_name_aliases`. Kept beside the resolved
    /// names rather than over them: a wire action still needs the real one.
    aliases: BTreeMap<AgentKey, String>,
    /// Bumped on each mutation; the view compares its last-built value to skip an
    /// unchanged rebuild.
    revision: u64,
}

impl FriendsModel {
    /// Bump the revision after a mutation.
    pub const fn touch(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    /// Merge a buddy-list record set (login `FriendList`), keeping any presence
    /// already learned for a friend that is being refreshed.
    pub fn note_friends(&mut self, friends: &[Friend]) {
        for friend in friends {
            let online = self
                .friends
                .get(&friend.id)
                .is_some_and(|entry| entry.online);
            self.friends
                .insert(friend.id, FriendEntry::new(*friend, online));
        }
        self.touch();
    }

    /// Replace the model from a presence snapshot (the [`Command::QueryFriends`]
    /// reply): authoritative for both rights and the online flag.
    pub fn apply_snapshot(&mut self, presence: &[FriendPresence]) {
        self.friends.clear();
        for entry in presence {
            self.friends.insert(
                entry.friend.id,
                FriendEntry::new(entry.friend, entry.online),
            );
        }
        self.touch();
    }

    /// Set the online flag on a set of friends (an online / offline notification).
    pub fn set_online(&mut self, friends: &[FriendKey], online: bool) {
        let mut changed = false;
        for id in friends {
            if let Some(entry) = self.friends.get_mut(id)
                && entry.online != online
            {
                entry.online = online;
                changed = true;
            }
        }
        if changed {
            self.touch();
        }
    }

    /// Update one friend's rights from a [`SlSessionEvent::FriendRightsChanged`](sl_client_bevy::SlSessionEvent::FriendRightsChanged):
    /// `granted_to_us` distinguishes the rights the friend now grants us from a
    /// server echo of the rights we grant them.
    pub fn update_rights(&mut self, friend: FriendKey, rights: FriendRights, granted_to_us: bool) {
        if let Some(entry) = self.friends.get_mut(&friend) {
            if granted_to_us {
                entry.rights_received = rights;
            } else {
                entry.rights_granted = rights;
            }
            self.touch();
        }
    }

    /// Drop a friend (friendship terminated by either side).
    pub fn remove(&mut self, friend: FriendKey) {
        if self.friends.remove(&friend).is_some() {
            self.touch();
        }
    }

    /// Record a resolved legacy name for an agent (ignoring empties).
    pub fn note_name(&mut self, id: AgentKey, name: &str) {
        if !name.is_empty() && self.names.get(&id).map(String::as_str) != Some(name) {
            self.names.insert(id, name.to_owned());
            self.touch();
        }
    }

    /// The resolved name for an agent, if known — the **grid's** answer, which
    /// is what a wire action (a mute entry) has to carry.
    pub fn name_of(&self, id: AgentKey) -> Option<&str> {
        self.names.get(&id).map(String::as_str)
    }

    /// The name to **show** for an agent: the alias the user gave them, else the
    /// resolved name.
    pub(crate) fn shown_name_of(&self, id: AgentKey) -> Option<&str> {
        self.aliases
            .get(&id)
            .or_else(|| self.names.get(&id))
            .map(String::as_str)
    }

    /// Replace the mirrored aliases, rebuilding the list when they moved (an
    /// alias given now renames that friend in the list at once). The one way in;
    /// `contact_sets::apply_name_aliases` is the caller.
    pub fn set_name_aliases(&mut self, aliases: BTreeMap<AgentKey, String>) {
        if self.aliases == aliases {
            return;
        }
        self.aliases = aliases;
        self.touch();
    }

    /// The model revision — a consumer that mirrors the roster (the friends-only
    /// render filter, `derender`) compares its last-mirrored value to
    /// skip an unchanged rebuild.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Every friend's agent id. The friends-only render filter mirrors this by
    /// revision so its per-avatar gate — which runs for every streamed object at
    /// a crowded event — stays a single hash lookup.
    #[must_use]
    pub fn friend_ids(&self) -> std::collections::HashSet<Uuid> {
        self.friends
            .keys()
            .map(|id| AgentKey::from(*id).uuid())
            .collect()
    }

    /// Whether `agent` is already in the buddy cache — a friend.
    ///
    /// The avatar context menu reads this to disable "Add as Friend" for someone
    /// who already is one, matching the reference viewer's `on_enable`.
    #[must_use]
    pub fn is_friend(&self, agent: AgentKey) -> bool {
        self.friends.contains_key(&FriendKey::from(agent.uuid()))
    }

    /// Whether `agent` is a friend the grid last reported **online**. Someone
    /// who is not a friend at all is not online as far as this model knows — the
    /// buddy cache is the only presence the protocol gives us.
    #[must_use]
    pub fn is_online(&self, agent: AgentKey) -> bool {
        self.friends
            .get(&FriendKey::from(agent.uuid()))
            .is_some_and(|entry| entry.online)
    }

    /// The whole roster as `(agent, display label)` pairs, name order — the
    /// avatar picker's Friends tab reads this. A friend whose name has not
    /// resolved yet labels as a provisional id fragment.
    #[must_use]
    pub fn roster(&self) -> Vec<(AgentKey, String)> {
        let mut entries: Vec<(AgentKey, String)> = self
            .friends
            .keys()
            .map(|id| {
                let agent = AgentKey::from(*id);
                let label = self
                    .names
                    .get(&agent)
                    .cloned()
                    .unwrap_or_else(|| format!("({id})"));
                (agent, label)
            })
            .collect();
        entries.sort_by_key(|entry| entry.1.to_lowercase());
        entries
    }

    /// The friends whose name is not yet resolved — the set to request names for.
    #[must_use]
    pub fn unnamed(&self) -> Vec<AgentKey> {
        self.friends
            .keys()
            .map(|id| AgentKey::from(*id))
            .filter(|agent| !self.names.contains_key(agent))
            .collect()
    }

    /// The render-ready row list, in map order. The table sorts it through
    /// its own `SortState`; the model has no opinion on
    /// display order.
    #[must_use]
    pub fn rows(&self) -> Vec<FriendRow> {
        self.friends
            .iter()
            .map(|(id, entry)| {
                let agent = AgentKey::from(*id);
                let name = self
                    .shown_name_of(agent)
                    .map_or_else(|| short_id(agent.uuid()), ToOwned::to_owned);
                FriendRow {
                    friend: *id,
                    agent,
                    name,
                    online: entry.online,
                    rights_granted: entry.rights_granted,
                    rights_received: entry.rights_received,
                }
            })
            .collect()
    }

    /// The rights this agent currently grants `friend`, if known.
    #[must_use]
    pub fn granted_rights(&self, friend: FriendKey) -> Option<FriendRights> {
        self.friends.get(&friend).map(|entry| entry.rights_granted)
    }

    /// Optimistically set the rights this agent grants `friend` (so a toggled
    /// checkbox flips immediately; the server echo re-confirms the same value).
    pub fn set_granted(&mut self, friend: FriendKey, rights: FriendRights) {
        if let Some(entry) = self.friends.get_mut(&friend)
            && entry.rights_granted != rights
        {
            entry.rights_granted = rights;
            self.touch();
        }
    }
}

/// One render-ready friend row: the ids the actions need, the display name, the
/// presence flag, and the friendship rights in both directions (the table's
/// permission columns).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FriendRow {
    /// The friend id (for remove / grant-rights, which take a [`FriendKey`]).
    pub friend: FriendKey,
    /// The agent id (for IM / teleport / mute, which take an [`AgentKey`]).
    pub agent: AgentKey,
    /// The display name (or a short-id placeholder until the name resolves).
    pub name: String,
    /// Whether the friend is currently known-online.
    pub online: bool,
    /// The rights this agent grants the friend (the "They can …" columns).
    pub rights_granted: FriendRights,
    /// The rights the friend grants this agent (the "You can …" columns).
    pub rights_received: FriendRights,
}

/// The pure groups model: the agent's group memberships keyed by group id (to its
/// display name), the active (worn) group, and a revision stamp bumped on every
/// change so the view rebuilds only when something actually moved. Fed solely from
/// the event stream. The list and its actions need only the name; the membership
/// record's powers / contribution belong to the (out-of-scope) profile.
#[derive(Resource, Debug, Default)]
pub struct GroupsModel {
    /// The agent's groups, by group id, mapped to the group's display name.
    groups: BTreeMap<GroupKey, String>,
    /// Names of **other** groups the agent is not a member of, resolved on
    /// demand (`UUIDGroupNameRequest` → [`SlSessionEvent::GroupNames`], or a
    /// group profile). Kept separate from [`groups`](Self::groups), which is the
    /// authoritative membership set; [`group_name`](Self::group_name) falls back
    /// to this so a group-owned parcel / object shows a name, not a UUID.
    resolved: BTreeMap<GroupKey, String>,
    /// Whether the agent accepts notices from each group — retained (unlike the
    /// display name, which the list needs) for the group profile floater's
    /// membership toggle, which has no other source for the login-time value.
    accept_notices: BTreeMap<GroupKey, bool>,
    /// Each member group's insignia (texture id), from the login-time
    /// `AgentGroupDataUpdate` — the source the group-notice toast
    /// (`group_notice`) reads the notice's group image from.
    insignia: BTreeMap<GroupKey, TextureKey>,
    /// The currently-active (worn) group, if any.
    active: Option<GroupKey>,
    /// The own agent's active group **title** (e.g. `"Officer"`), from the
    /// same `ActiveGroupChanged` push; `None` when no group is active or the
    /// title is empty.
    own_title: Option<String>,
    /// Bumped on each mutation; the view compares its last-built value to skip an
    /// unchanged rebuild.
    revision: u64,
}

impl GroupsModel {
    /// Bump the revision after a mutation.
    pub(crate) const fn touch(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    /// Replace the membership set from an `AgentGroupDataUpdate`
    /// ([`SlSessionEvent::GroupMemberships`](sl_client_bevy::SlSessionEvent::GroupMemberships)) — the wire message carries the
    /// agent's **full** group list, so it is authoritative and replaces the cache
    /// wholesale. The active group is left untouched (it is tracked separately from
    /// [`SlSessionEvent::ActiveGroupChanged`](sl_client_bevy::SlSessionEvent::ActiveGroupChanged)).
    pub fn apply_memberships(&mut self, memberships: &[GroupMembership]) {
        self.groups.clear();
        self.accept_notices.clear();
        self.insignia.clear();
        for membership in memberships {
            self.groups
                .insert(membership.group_id, membership.group_name.clone());
            self.accept_notices
                .insert(membership.group_id, membership.accept_notices);
            self.insignia
                .insert(membership.group_id, membership.group_insignia_id);
        }
        self.touch();
    }

    /// The insignia texture of a member `group`, if known — the group-notice toast
    /// (`group_notice`) reads it to show the notice's group image. A nil
    /// texture (a group with no insignia) is reported as `None`.
    #[must_use]
    pub fn group_insignia(&self, group: GroupKey) -> Option<TextureKey> {
        self.insignia
            .get(&group)
            .copied()
            .filter(|key| *key != TextureKey::from(Uuid::nil()))
    }

    /// Whether the agent accepts notices from `group`, if the agent is a member —
    /// the group profile floater's membership toggle seeds from this (the
    /// login-time value is not otherwise available to a floater opened later).
    #[must_use]
    pub fn accepts_notices(&self, group: GroupKey) -> Option<bool> {
        self.accept_notices.get(&group).copied()
    }

    /// The display name of `group` — the agent's own membership name, else a
    /// name resolved on demand ([`note_resolved_name`](Self::note_resolved_name)),
    /// else `None` (the caller falls back to the id and can request a resolve).
    pub fn group_name(&self, group: GroupKey) -> Option<&str> {
        self.groups
            .get(&group)
            .or_else(|| self.resolved.get(&group))
            .map(String::as_str)
    }

    /// Whether the agent is a member of `group` — a membership test that, unlike
    /// [`group_name`](Self::group_name), does **not** consider the on-demand
    /// resolved-name cache (a resolved non-member group must not read as a member).
    #[must_use]
    pub fn is_member(&self, group: GroupKey) -> bool {
        self.groups.contains_key(&group)
    }

    /// Request `group`'s name (`UUIDGroupNameRequest`) if it is not already known
    /// — the shared resolve path every group-name display site uses so a
    /// non-member group's name fills the cache instead of showing a UUID forever.
    /// Call at a discrete event (a floater open, a selection change), not per
    /// frame; the reply folds into the `resolved` cache.
    pub fn request_name(&self, group: GroupKey, commands: &mut MessageWriter<SlCommand>) {
        if self.group_name(group).is_none() {
            commands.write(SlCommand(Command::RequestGroupNames(vec![group])));
        }
    }

    /// Fold a resolved name for a non-member `group` into the on-demand cache.
    /// Public so any group-name display site can seed the shared cache from a
    /// name it learned (an IM session, a profile) rather than keeping its own.
    pub fn note_resolved_name(&mut self, group: GroupKey, name: &str) {
        if name.is_empty() || self.groups.contains_key(&group) {
            return;
        }
        if self.resolved.get(&group).map(String::as_str) != Some(name) {
            self.resolved.insert(group, name.to_owned());
            self.touch();
        }
    }

    /// The agent's group ids, in the map's stable id order — the build
    /// floater's set-group cycle walks these (with "none" between the wrap).
    #[must_use]
    pub fn group_ids(&self) -> Vec<GroupKey> {
        self.groups.keys().copied().collect()
    }

    /// Set the active (worn) group, bumping the revision only on a real change.
    pub fn set_active(&mut self, active: Option<GroupKey>, title: &str) {
        let title = if title.is_empty() {
            None
        } else {
            Some(title.to_owned())
        };
        if self.active != active || self.own_title != title {
            self.active = active;
            self.own_title = title;
            self.touch();
        }
    }

    /// The own agent's active group title (from `ActiveGroupChanged`) — the
    /// freshest source for the own tag's title line (the NameValue `Title`
    /// The list revision — a view stores the value it last built at and
    /// rebuilds when it advances.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// only refreshes when the simulator re-streams the avatar object).
    #[must_use]
    pub fn own_title(&self) -> Option<&str> {
        self.own_title.as_deref()
    }

    /// Drop a group the agent is no longer in (left, ejected, or dissolved),
    /// clearing the active marker if it was the active group.
    pub fn remove(&mut self, group: GroupKey) {
        if self.groups.remove(&group).is_some() {
            self.accept_notices.remove(&group);
            if self.active == Some(group) {
                self.active = None;
            }
            self.touch();
        }
    }

    /// The ordered, render-ready row list: case-folded by group name, with a stable
    /// id tie-break so equal names keep a fixed order.
    #[must_use]
    pub fn ordered(&self) -> Vec<GroupRow> {
        let mut rows: Vec<GroupRow> = self
            .groups
            .iter()
            .map(|(id, group_name)| {
                let name = if group_name.is_empty() {
                    short_id(id.uuid())
                } else {
                    group_name.clone()
                };
                GroupRow {
                    group: *id,
                    name,
                    active: self.active == Some(*id),
                }
            })
            .collect();
        rows.sort_by(|left, right| {
            left.name
                .to_lowercase()
                .cmp(&right.name.to_lowercase())
                .then_with(|| left.group.uuid().cmp(&right.group.uuid()))
        });
        rows
    }

    /// The number of groups the agent is in — the count line under the list.
    #[must_use]
    pub fn len(&self) -> usize {
        self.groups.len()
    }

    /// Whether the agent is in no groups at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }

    /// The display name for a group, if known (for the leave-confirm prompt).
    pub fn name_of(&self, group: GroupKey) -> Option<&str> {
        self.groups.get(&group).map(String::as_str)
    }
}

/// One render-ready group row: the id its actions need, the display name, and
/// whether it is the active (worn) group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupRow {
    /// The group id (for every action).
    pub group: GroupKey,
    /// The display name (or a short-id placeholder for an unnamed group).
    pub name: String,
    /// Whether this is the agent's active (worn) group.
    pub active: bool,
}

/// How long the away state must have held before input clears it (the
/// reference's `LLAgent::MIN_AFK_TIME`) — without it, the mouse move that
/// happens to arrive one frame after the auto-AFK fires would cancel it.
pub const MIN_AFK_SECS: f32 = 10.0;

/// The live presence state: the two session modes and the timers behind
/// auto-AFK. The two autorespond modes are **not** here — they are their own
/// persisted settings, read straight from the store wherever they are needed.
#[expect(
    clippy::struct_excessive_bools,
    reason = "the two modes are independent (either, both or neither can be on) and the three \
              remaining flags are per-mode bookkeeping — an enum would have to enumerate every \
              combination to say the same thing"
)]
#[derive(Resource, Debug, Default)]
pub struct PresenceState {
    /// Whether the avatar is away.
    away: bool,
    /// Whether Do Not Disturb is on.
    do_not_disturb: bool,
    /// Seconds since the last user input (the reference `gAwayTriggerTimer`).
    idle_secs: f32,
    /// Seconds the away state has held (the reference `gAwayTimer`), used for
    /// the clear debounce and the quit-after-AFK timeout.
    away_secs: f32,
    /// The away state last advertised to the simulator, so the animation
    /// request is sent on the edge only.
    advertised_away: bool,
    /// The Do Not Disturb state last advertised, likewise.
    advertised_dnd: bool,
    /// Whether *we* sat the avatar down on going away, so returning only stands
    /// it back up when it was our doing.
    sat_on_away: bool,
}

impl PresenceState {
    /// Advance the idle clock, and the away clock while away.
    pub const fn tick(&mut self, dt: f32) {
        self.idle_secs += dt;
        if self.away {
            self.away_secs += dt;
        }
    }

    /// Seconds since the last user input.
    #[must_use]
    pub const fn idle_secs(&self) -> f32 {
        self.idle_secs
    }

    /// Seconds the away state has held.
    #[must_use]
    pub const fn away_secs(&self) -> f32 {
        self.away_secs
    }

    /// Restart the idle clock alone — there is no session to be away in yet,
    /// so the away clock is not the caller's business.
    pub const fn reset_idle(&mut self) {
        self.idle_secs = 0.0;
    }

    /// The away state if it differs from what was last advertised, marking it
    /// advertised in the same step; `None` when the wire already agrees. Read
    /// and mark cannot be separated, or a failed send would leave the two
    /// permanently out of step.
    pub const fn take_away_edge(&mut self) -> Option<bool> {
        if self.away == self.advertised_away {
            return None;
        }
        self.advertised_away = self.away;
        Some(self.away)
    }

    /// The Do Not Disturb state on the same terms as [`Self::take_away_edge`].
    pub const fn take_dnd_edge(&mut self) -> Option<bool> {
        if self.do_not_disturb == self.advertised_dnd {
            return None;
        }
        self.advertised_dnd = self.do_not_disturb;
        Some(self.do_not_disturb)
    }

    /// Whether *we* sat the avatar down on going away.
    #[must_use]
    pub const fn sat_on_away(&self) -> bool {
        self.sat_on_away
    }

    /// Record whether we sat the avatar down on going away.
    pub const fn set_sat_on_away(&mut self, sat: bool) {
        self.sat_on_away = sat;
    }

    /// Whether the avatar is away.
    #[must_use]
    pub const fn is_away(&self) -> bool {
        self.away
    }

    /// Whether Do Not Disturb is on.
    #[must_use]
    pub const fn is_do_not_disturb(&self) -> bool {
        self.do_not_disturb
    }

    /// Set the away state, restarting the away clock on a rising edge. The wire
    /// writes are reconciled by `advertise_presence`.
    pub const fn set_away(&mut self, away: bool) {
        if self.away != away {
            self.away = away;
            self.away_secs = 0.0;
        }
    }

    /// Set the Do Not Disturb state. The wire writes and the toast queue's
    /// drain are reconciled by `advertise_presence` and the hosts that read
    /// [`is_do_not_disturb`](Self::is_do_not_disturb).
    pub const fn set_do_not_disturb(&mut self, busy: bool) {
        self.do_not_disturb = busy;
    }

    /// Note user input: reset the idle clock and, once away has held long
    /// enough to be real, clear it (the reference's `MIN_AFK_TIME` debounce).
    pub fn note_activity(&mut self) {
        if self.away && self.away_secs > MIN_AFK_SECS {
            self.set_away(false);
        }
        self.idle_secs = 0.0;
    }
}

/// The map tracking target — a shared shape for the minimap today and the
/// world map later (`viewer-world-map-tracking-teleport`), so both surfaces
/// drive one beacon.
#[derive(Resource, Debug, Default)]
pub struct MapTracking {
    /// The current target, or `None` when not tracking.
    pub target: Option<TrackTarget>,
}

/// What the map is tracking.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TrackTarget {
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

// ---------------------------------------------------------------------------
// Cross-tier intents
// ---------------------------------------------------------------------------
//
// Messages a surface writes to ask for something it does not own: block this
// resident, open that profile, pick a texture. Each is read by a feature that
// sits far from the ones asking -- `RequestBlock` alone is written from avatar
// menus, the radar, the minimap, the profile, the inspector, the friends list,
// three kinds of toast and a `secondlife:///` link.
//
// They live here rather than with the floater that answers them, so asking
// does not mean depending on the answer. Every payload is an id or a string
// this crate or `sl-client-bevy` already owns.

/// A request to block a target: the single **guarded** entry point every Block
/// surface writes instead of putting a `Command::Mute` on the wire itself.
///
/// `apply_block_requests` runs the reference's `LLMuteList::add` checks and
/// only then sends, so every Block in the viewer — the avatar / object pie
/// menus, the radar, the minimap, the profile floater, the inspector, the
/// friends list, the offer / dialog / URL toasts, a `secondlife:///…/mute`
/// link, and the block list's own add paths — refuses a Linden, the agent
/// itself, a malformed or duplicate by-name entry and an over-full list
/// identically, and reports the refusal with the same notification.
#[derive(Message, Debug, Clone)]
pub struct RequestBlock {
    /// The blocked entity's id (nil for a [`MuteType::ByName`] block).
    pub id: Uuid,
    /// The blocked entity's name, as the asking surface knows it.
    pub name: String,
    /// What kind of entity is blocked.
    pub mute_type: MuteType,
    /// The per-aspect *exception* flags ([`MuteFlags::default`] mutes all).
    pub flags: MuteFlags,
}

impl RequestBlock {
    /// Block `id` as `mute_type` under `name`, with every aspect muted — what a
    /// menu's plain "Block" does.
    pub fn new(id: Uuid, name: impl Into<String>, mute_type: MuteType) -> Self {
        Self {
            id,
            name: name.into(),
            mute_type,
            flags: MuteFlags::default(),
        }
    }

    /// The same request with explicit exception flags — the block list's
    /// per-aspect toggles re-sending an edited entry.
    #[must_use]
    pub const fn with_flags(mut self, flags: MuteFlags) -> Self {
        self.flags = flags;
        self
    }
}

/// Open the profile floater on an avatar (from the pie menu's Profile slice,
/// the People list, or a repaint after an edit).
#[derive(Message, Debug, Clone, Copy)]
pub struct OpenAvatarProfile {
    /// The avatar whose profile to show.
    pub agent: AgentKey,
}

/// Open the group profile floater on a group (from the Groups list's Info button).
#[derive(Message, Debug, Clone, Copy)]
pub struct OpenGroupProfile {
    /// The group whose profile to show.
    pub group: GroupKey,
}

/// A client-generated local-chat notice — a line the viewer itself posts to the
/// overlay (not a `ChatReceived` from the grid), for feedback like a build-tool
/// no-permission alert. Written by whichever system produced the notice
/// (e.g. `crate::gizmos::dispatch_shift_drag_copy`) and rendered by
/// `update_chat_overlay` alongside received chat.
#[derive(Message, Debug, Clone)]
pub struct LocalChatNotice {
    /// The already-formatted line to show.
    pub text: String,
}

impl LocalChatNotice {
    /// A notice carrying `text`.
    #[must_use]
    pub const fn new(text: String) -> Self {
        Self { text }
    }
}

/// Ask the picker to open for a feature. `requester` tags the eventual
/// [`AvatarPicked`] so only the asking feature consumes it.
#[derive(Message, Debug, Clone, Copy)]
pub struct OpenAvatarPicker {
    /// The feature tag echoed back in [`AvatarPicked`].
    pub requester: &'static str,
    /// Whether the user may choose several residents at once — the reference's
    /// `allow_multiple`. Build one with [`OpenAvatarPicker::one`] or
    /// [`OpenAvatarPicker::many`] rather than by hand, so the choice reads at
    /// the call site.
    pub allow_multiple: bool,
}

impl OpenAvatarPicker {
    /// Ask for exactly one resident.
    #[must_use]
    pub const fn one(requester: &'static str) -> Self {
        Self {
            requester,
            allow_multiple: false,
        }
    }

    /// Ask for any number of residents at once.
    #[must_use]
    pub const fn many(requester: &'static str) -> Self {
        Self {
            requester,
            allow_multiple: true,
        }
    }
}

/// The confirmed pick: every chosen resident, in list order. A picker opened
/// One resident the picker returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickedAvatar {
    /// The chosen avatar.
    pub agent: AgentKey,
    /// The label the picked row carried — the avatar's name as the source that
    /// produced the row knew it (a search reply's legacy name, the friend's
    /// name, or the nearby avatar's name). Consumers that must *record* a name
    /// against the id (the block list writes it into the mute entry) take it
    /// from here rather than re-resolving.
    pub name: String,
}

/// with [`OpenAvatarPicker::one`] answers with exactly one element.
#[derive(Message, Debug, Clone)]
pub struct AvatarPicked {
    /// The tag of the feature that opened the picker.
    pub requester: &'static str,
    /// The chosen residents — never empty (the picker does not confirm an empty
    /// selection).
    pub picks: Vec<PickedAvatar>,
}

impl AvatarPicked {
    /// The first chosen resident — for a single-resident requester, *the* pick.
    #[must_use]
    pub fn first(&self) -> Option<&PickedAvatar> {
        self.picks.first()
    }
}

/// What a picker open browses: **textures** (the default — the reference's
/// `LLTextureCtrl` `PICK_TEXTURE`) or **materials** (GLTF render materials, the
/// reference's `PICK_MATERIAL`). It drives the inventory filter, the floater
/// title, and which quick choices show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PickerKind {
    /// Browse texture / snapshot items.
    #[default]
    Texture,
    /// Browse GLTF render-material items (`InventoryType::Material`).
    Material,
}

/// Open the texture picker for `requester`, seeded with `current`.
#[derive(Message, Debug, Clone, Copy)]
pub struct OpenTexturePicker {
    /// The swatch (or other widget) the reply is tagged back to.
    pub requester: Entity,
    /// The texture (or, in material mode, material id) to open on.
    pub current: TextureKey,
    /// Whether to browse textures or materials.
    pub kind: PickerKind,
}

/// The chosen texture, tagged back to the [`requester`](Self::requester). Emitted
/// **non-final** on each selection so a consumer can live-preview it, once on
/// **OK** with [`final_pick`](Self::final_pick) true, and on **Cancel** as the
/// original (a revert), mirroring the colour picker.
#[derive(Message, Debug, Clone, Copy)]
pub struct TexturePicked {
    /// The widget that opened the picker.
    pub requester: Entity,
    /// The chosen texture.
    pub texture: TextureKey,
    /// Whether this is the committed choice (**OK**) rather than a live-preview
    /// or revert update.
    pub final_pick: bool,
}

/// A conversation's stable identity — the per-tab key. `Nearby` is the singleton
/// local-chat tab; the rest key on the peer, group or conference.
///
/// Derives [`Ord`] so it can key the `ConversationsUi` view map (sl-types gives
/// the newtypes their ordering).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum ConversationKey {
    /// The local (nearby) chat tab — always present, always first.
    Nearby,
    /// A one-to-one instant-message conversation with a peer.
    Direct(AgentKey),
    /// A group IM session.
    Group(GroupKey),
    /// An ad-hoc conference IM session.
    Conference(ImSessionId),
}

impl ConversationKey {
    /// Whether this is the un-closable Nearby tab.
    #[must_use]
    pub const fn is_nearby(self) -> bool {
        matches!(self, Self::Nearby)
    }

    /// The runtime chat-session kind behind a keyed tab, or `None` for Nearby
    /// (local chat is not a [`ChatSessionKind`] session).
    #[must_use]
    pub const fn session_kind(self) -> Option<ChatSessionKind> {
        match self {
            Self::Nearby => None,
            Self::Direct(peer) => Some(ChatSessionKind::Direct { peer }),
            Self::Group(group_id) => Some(ChatSessionKind::Group { group_id }),
            Self::Conference(id) => Some(ChatSessionKind::Conference { id }),
        }
    }

    /// The tab key for a runtime chat-session kind (the inverse of
    /// [`Self::session_kind`]).
    #[must_use]
    pub const fn from_session_kind(kind: ChatSessionKind) -> Self {
        match kind {
            ChatSessionKind::Direct { peer } => Self::Direct(peer),
            ChatSessionKind::Group { group_id } => Self::Group(group_id),
            ChatSessionKind::Conference { id } => Self::Conference(id),
        }
    }
}

/// A request to open (create if needed) and activate `key`'s conversation — the
/// hook another module uses to start an IM from outside the floater. The
/// `crate::people` Friends list writes this to open a one-to-one IM tab for a
/// selected friend in this same floater.
#[derive(Message, Debug, Clone, Copy)]
pub struct OpenConversation {
    /// The conversation to open and select.
    pub key: ConversationKey,
}

/// Open (and optionally navigate) the web browser floater.
#[derive(Message, Debug, Clone)]
pub struct OpenWebBrowser {
    /// The URL to show; `None` keeps the current page (or the home page on
    /// first open).
    pub url: Option<String>,
}

/// A concrete teleport target, kept so the overlay's **Retry** button can
/// re-issue the exact same teleport after a failure.
#[derive(Debug, Clone)]
pub struct TeleportTarget {
    /// The destination region handle.
    pub region_handle: RegionHandle,
    /// The destination region-local arrival position.
    pub position: RegionCoordinates,
    /// The arrival look-at direction.
    pub look_at: Vector,
}

/// A request to open the teleport overlay for a teleport this frame's surface is
/// initiating. Emitting it is optional — the overlay also opens from the incoming
/// teleport events — but it lets a surface pre-fill the destination label and
/// enable Retry. Prefer the [`issue_teleport`] helper, which writes this and the
/// [`Command::Teleport`] together.
#[derive(Message, Debug, Clone)]
pub struct BeginTeleportFlow {
    /// A human-readable destination label (e.g. a region name or `Region (128, 128)`),
    /// shown on the overlay. `None` leaves the destination line blank.
    pub destination: Option<String>,
    /// The target to re-issue if the user hits Retry. `None` (landmark / lure
    /// teleports, whose destination is not known until arrival) disables Retry.
    pub retry: Option<TeleportTarget>,
}

/// Fire a location teleport **and** open the progress overlay in one call: writes
/// [`Command::Teleport`] and a [`BeginTeleportFlow`] carrying the destination
/// label and a Retry payload. The shared entry point every location-teleport
/// surface (double-click, minimap, world map) routes through.
pub fn issue_teleport(
    commands: &mut MessageWriter<SlCommand>,
    begin: &mut MessageWriter<BeginTeleportFlow>,
    target: TeleportTarget,
    destination: Option<String>,
) {
    begin.write(BeginTeleportFlow {
        destination,
        retry: Some(target.clone()),
    });
    commands.write(SlCommand(Command::Teleport {
        region_handle: target.region_handle,
        position: target.position,
        look_at: target.look_at,
    }));
}

/// Ask for the add-to-set floater. The avatar pie's **Add ▸ Add to Set**, the
/// panel's **Move to Set…** and the minimap's multi-avatar **Add to Set** all
/// write this.
#[derive(Message, Debug, Clone)]
pub struct OpenAddToContactSet {
    /// The residents to file, each with the best name the opening surface knows
    /// for them (empty when it knows none). Usually one; the reference's
    /// multi-avatar entries hand over several, and the floater then asks for one
    /// set to file the lot under.
    pub agents: Vec<(AgentKey, String)>,
    /// The set to take them out of once they are filed — the reference's move
    /// mode. `None` for a plain add.
    pub move_from: Option<String>,
}

impl OpenAddToContactSet {
    /// File one resident.
    #[must_use]
    pub fn one(agent: AgentKey, name: String) -> Self {
        Self {
            agents: vec![(agent, name)],
            move_from: None,
        }
    }

    /// File several residents at once.
    #[must_use]
    pub const fn many(agents: Vec<(AgentKey, String)>) -> Self {
        Self {
            agents,
            move_from: None,
        }
    }

    /// The same request in the reference's *move* mode: take them out of `set`
    /// once they are filed.
    #[must_use]
    pub fn moving_from(mut self, set: String) -> Self {
        self.move_from = Some(set);
        self
    }
}

// ---------------------------------------------------------------------------
// Drag, drop and open-editor vocabulary
// ---------------------------------------------------------------------------
//
// The inventory drags items onto things other features own -- an object's
// contents, a notecard's body, whatever the cursor is over in the world -- and
// opens editors it does not implement. The markers, messages and the one
// command builder that describe those hand-offs live here so neither side has
// to name the other.

/// A description of an item added to a prim's contents, for the pending-add
/// phantom row shown until the server confirms it.
#[derive(Debug, Clone)]
pub struct PendingAdd {
    /// The source item's id (the phantom row's key until reconcile).
    pub item_id: InventoryKey,
    /// The added item's display name.
    pub name: String,
    /// The added item's type-icon glyph.
    pub icon: &'static str,
}

/// A signal that an object's task inventory was mutated from outside this module
/// (a drag-in add resolved by `crate::inventory_drag`), so its cached listing
/// must be reconciled against the server — the same round trip the in-module
/// mutations do inline. `added` carries the dropped items so a "…adding" phantom
/// row can stand in until the server's listing includes them.
#[derive(Message, Debug, Clone)]
pub struct ContentsMutated {
    /// The region-scoped id of the mutated object.
    pub scoped: ScopedObjectId,
    /// The grid-wide key of the mutated object.
    pub full: ObjectKey,
    /// The items added by this mutation (empty for a pure reconcile).
    pub added: Vec<PendingAdd>,
}

/// Add the dropped inventory item to `object`'s task inventory — the drag-in
/// path, called from `crate::inventory_drag` when a drag ends over a contents
/// list. Returns the command to send, or `None` when the source is a folder
/// (task inventory takes single items).
#[must_use]
pub fn contents_drop_command(
    item: &sl_client_bevy::ItemInfo,
    scoped: ScopedObjectId,
    object: ObjectKey,
) -> Option<Command> {
    let inventory_item = item.to_item();
    let restore = RestoreItem::for_task_drop(&inventory_item, object, Uuid::new_v4()).ok()?;
    Some(Command::UpdateTaskInventory {
        target: scoped,
        key: TaskInventoryKey::Item,
        item: Box::new(restore),
    })
}

/// Where the notecard being edited lives — the agent's own inventory, or an
/// in-world object's task inventory. Carried through the editor so Save writes
/// back to the right place (the reference's "opened-from-task" provenance).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotecardSource {
    /// A notecard in the agent's own inventory.
    Agent {
        /// The agent-inventory item.
        item_id: InventoryKey,
    },
    /// A notecard inside an in-world object's task inventory.
    Task {
        /// The object (task) holding the notecard.
        task_id: ObjectKey,
        /// The notecard item within that object's inventory.
        item_id: InventoryKey,
    },
}

impl NotecardSource {
    /// The notecard item's own id, whichever inventory it lives in — the
    /// `notecard-id` a `CopyInventoryFromNotecard` copy names.
    #[must_use]
    pub const fn item_id(self) -> InventoryKey {
        match self {
            Self::Agent { item_id } | Self::Task { item_id, .. } => item_id,
        }
    }

    /// The asset-update location this source saves back to.
    #[must_use]
    pub const fn location(self) -> AssetUpdateLocation {
        match self {
            Self::Agent { item_id } => AssetUpdateLocation::AgentInventory { item_id },
            Self::Task { task_id, item_id } => {
                AssetUpdateLocation::TaskInventory { task_id, item_id }
            }
        }
    }

    /// The prim holding the notecard when it lives in a task inventory, or
    /// `None` for an agent-inventory notecard — the `object-id` a
    /// `CopyInventoryFromNotecard` copy of an embedded item names.
    #[must_use]
    pub const fn object_id(self) -> Option<ObjectKey> {
        match self {
            Self::Agent { .. } => None,
            Self::Task { task_id, .. } => Some(task_id),
        }
    }
}

/// Open the notecard editor on a notecard. Written by the inventory **Open**
/// action (routed here from `crate::inventory_properties`) and by the Object
/// Contents floater's Open (`crate::edit_contents`) for a task-inventory
/// notecard.
#[derive(Message, Debug, Clone)]
pub struct OpenNotecard {
    /// The notecard's name, shown as the floater title.
    pub name: String,
    /// The notecard asset to fetch and show.
    pub asset_id: Uuid,
    /// Whether the notecard is editable (the caller applies the right
    /// permission rule: an agent item's own modify bit, or an object's modify
    /// **and** the item's modify bit for a task notecard).
    pub editable: bool,
    /// Where the notecard lives, so Save writes back to the right place.
    pub source: NotecardSource,
}

/// Marks the notecard editor floater as an **inventory drop target**: dropping
/// an inventory item on it while [`editable`](Self::editable) adds the item as
/// an embedded item. `crate::inventory_drag` walks up from the hovered node to
/// find it; `open_notecard` keeps [`editable`](Self::editable) in step with
/// the notecard currently shown (a no-modify notecard rejects drops).
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct NotecardDropTarget {
    /// Whether the notecard currently shown accepts an added embedded item.
    pub editable: bool,
}

/// Where the script being edited lives — the agent's own inventory, or an
/// in-world object's task inventory. Carried through the editor so Save writes
/// back to the right capability (the reference's "opened-from-task" provenance).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptSource {
    /// A script in the agent's own inventory (`UpdateScriptAgent`).
    Agent {
        /// The agent-inventory item.
        item_id: InventoryKey,
    },
    /// A script inside an in-world object's task inventory (`UpdateScriptTask`).
    Task {
        /// The object (task) holding the script.
        task_id: ObjectKey,
        /// The script item within that object's inventory.
        item_id: InventoryKey,
    },
}

impl ScriptSource {
    /// Whether this source is an in-world object's task inventory (which carries
    /// a run state the save must preserve).
    #[must_use]
    pub const fn is_task(self) -> bool {
        matches!(self, Self::Task { .. })
    }

    /// The upload location this source saves back to, carrying `running` for a
    /// task script (`is_script_running`). No experience is set — a script is not
    /// associated with an experience through this editor in v1.
    #[must_use]
    pub const fn location(self, running: bool) -> ScriptUploadLocation {
        match self {
            Self::Agent { item_id } => ScriptUploadLocation::AgentInventory { item_id },
            Self::Task { task_id, item_id } => ScriptUploadLocation::TaskInventory {
                task_id,
                item_id,
                running,
                experience: None,
            },
        }
    }
}

/// Open the script editor on a script. Written by the inventory **Open** action
/// (routed here from `crate::inventory_properties`) and by the Object Contents
/// The compile backend to request for a script's [`ScriptLanguage`]. Second Life
/// honours the token (Mono is its LSL default); OpenSim ignores it and reads the
/// language from a source-header comment, so an unknown backend does no harm.
#[must_use]
pub const fn target_for(language: Option<ScriptLanguage>) -> ScriptTarget {
    match language {
        Some(ScriptLanguage::Luau) => ScriptTarget::Luau,
        // LSL, or an item whose subtype byte we do not recognise.
        _ => ScriptTarget::Mono,
    }
}

/// floater's Open (`crate::edit_contents`) for a task-inventory script.
#[derive(Message, Debug, Clone)]
pub struct OpenScript {
    /// The script's name, shown as the floater title.
    pub name: String,
    /// The script source asset to fetch and show.
    pub asset_id: Uuid,
    /// Whether the script is editable (the caller applies the right permission
    /// rule: an agent item's own modify bit, or an object's modify **and** the
    /// item's modify bit for a task script).
    pub editable: bool,
    /// Where the script lives, so Save writes back to the right place.
    pub source: ScriptSource,
    /// The compile backend to request, derived from the item's language.
    pub target: ScriptTarget,
}

/// The in-world object an inventory drag is currently hovering, if it accepts the
/// drop — set by `crate::inventory_drag` each frame while a drag is active and
/// consumed by `apply_drag_hover_highlight` to draw the accept / foreign
/// outline (the reference's `highlightObjectAndFamily` during a drag).
#[derive(Resource, Debug, Default)]
pub struct DragHoverHighlight {
    /// The hovered object's root render entity and whether it is foreign (not
    /// owned, so the outline is red), or `None` when nothing droppable is hovered.
    pub hover: Option<DragHover>,
}

/// One drag-hover target: the object's root render entity and its ownership tint.
#[derive(Debug, Clone, Copy)]
pub struct DragHover {
    /// The hovered object's root render entity (a `SceneObject`).
    pub root: Entity,
    /// Whether the object is **foreign** (not owned / not modifiable but accepts
    /// the drop) — drawn red rather than the green accept colour.
    pub foreign: bool,
}

/// A request to start an **ad-hoc conference** with several residents, or to
/// invite more people into one that is already open — the reference's
/// `LLAvatarActions::startConference` (`llavataractions.cpp:423`), and the one
/// verb every multi-selection of avatars in this viewer routes to: the radar's
/// multi-row *IM*, the People panel's Friends list, and the inventory's
/// *Start Conference Chat* on calling cards.
///
/// The list is taken as the user picked it — the handler drops our own agent
/// and any repeats, and a list that leaves **one** resident opens a plain
/// one-to-one IM instead (what the reference's `Avatar.IM` does by count), so a
/// caller never has to branch on how many rows are selected.
#[derive(Message, Debug, Clone)]
pub struct StartConference {
    /// The residents to invite.
    pub agents: Vec<AgentKey>,
    /// The conference to invite them **into**, or `None` to start a new one.
    /// Inviting into an open conference is the same wire request with the same
    /// session id (the reference's "Add participants" on an IM floater).
    pub into: Option<ImSessionId>,
}

impl StartConference {
    /// Start a fresh conference with `agents`.
    #[must_use]
    pub const fn with(agents: Vec<AgentKey>) -> Self {
        Self { agents, into: None }
    }

    /// Invite `agents` into the already-open conference `session`.
    #[must_use]
    pub const fn adding(session: ImSessionId, agents: Vec<AgentKey>) -> Self {
        Self {
            agents,
            into: Some(session),
        }
    }
}

/// The inventory item sent along with every autoresponse, as its item id in
/// text form; empty = none (the reference `FSAutoresponseItemUUID`). Consumed
/// by the presence auto-reply (`crate::presence`), which is why it is
/// account-scoped like the replies themselves.
pub const SETTING_AUTORESPONSE_ITEM: &str = "AutoresponseItemUUID";

/// The agent's own region-local position, folded from its own-avatar object
/// updates (`SlSessionEvent::ObjectAdded` / `SlSessionEvent::ObjectUpdated`
/// whose `full_id` is the agent id). `None` before the own avatar arrives. This
/// is the region-local `⟨x, y, z⟩` the location read-out shows, the same source
/// the reference viewer's `LLAgentUI::buildLocationString` reads
/// (`gAgent.getPositionAgent`).
#[derive(Resource, Debug, Clone, Default)]
pub struct AgentRegionPosition {
    /// The region-local position in metres, or `None` before the own avatar
    /// object arrives.
    pub position: Option<Vector>,
}

impl AgentRegionPosition {
    /// The agent's region-local position in metres, or `None` before the own
    /// avatar object arrives. Read by the About Land Options tab to set a
    /// parcel's landing point to where the agent stands.
    #[must_use]
    pub const fn position(&self) -> Option<&Vector> {
        self.position.as_ref()
    }
}

// ---------------------------------------------------------------------------
// Settings the behaviour reads, not the tab that shows them
// ---------------------------------------------------------------------------
//
// A preferences tab draws a control; the setting it writes is read somewhere
// else entirely -- the chat overlay sizes itself from the font setting, the
// auto-reply picks its text by mode, the idle timer reads the AFK seconds.
// The keys live with the behaviour, so a tab is never something the behaviour
// has to depend on.

/// The chat font-size step: `0` small, `1` medium, `2` large. Consumed by the
/// overlay (`crate::chat`) and the conversations transcript
/// (`crate::conversations`); the reference `ChatFontSize` radio group.
pub const SETTING_CHAT_FONT_SIZE: &str = "ChatFontSize";

/// Seconds a nearby-chat overlay line lives before it has fully faded (the
/// reference `NearbyToastLifeTime`); the fade itself takes the last
/// `crate::chat` fade-duration seconds of it.
pub const SETTING_NEARBY_TOAST_LIFETIME: &str = "NearbyChatToastLifetime";

/// The most lines the nearby-chat overlay shows at once (the burst safety
/// valve; the reference console's `ConsoleMaxLines`).
pub const SETTING_CHAT_MAX_LINES: &str = "ChatOverlayMaxLines";

/// The auto-reply sent to an IM sender while in Do Not Disturb (busy) mode.
/// Account-scoped; consumed by `viewer-do-not-disturb-away`.
pub const SETTING_BUSY_RESPONSE: &str = "BusyResponse";

/// The auto-reply sent while in autorespond mode (the Firestorm extension).
/// Account-scoped; consumed by `viewer-do-not-disturb-away`.
pub const SETTING_AUTORESPOND_RESPONSE: &str = "AutorespondResponse";

/// The auto-reply sent to non-friends while in autorespond-to-non-friends
/// mode. Account-scoped; consumed by `viewer-do-not-disturb-away`.
pub const SETTING_AUTORESPOND_NON_FRIENDS_RESPONSE: &str = "AutorespondNonFriendsResponse";

/// Seconds of inactivity before the viewer marks the avatar away; `0` = never.
/// Registered here, consumed by the away-mode task
/// (`viewer-do-not-disturb-away`).
pub const SETTING_AFK_TIMEOUT: &str = "AfkTimeoutSeconds";

/// Tracks whether the local avatar is **ground-sitting**.
///
/// The session records object-sits (`SlAgentParcel::seated_on`) but keeps *no*
/// ground-sit state — `sit_on_ground` sends only a transient control bit, so
/// there is nothing on the wire to read back. The viewer therefore tracks it
/// here: set when this menu sends Sit Down, cleared when it sends Stand Up or the
/// avatar walks (which stands it up). Best-effort — a ground sit begun or ended
/// by something other than this menu or ordinary locomotion is not observed, and
/// the worst case is a momentarily wrong Stand Up / Sit Down enable that the next
/// sit / stand / step corrects.
#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct SelfGroundSit {
    /// Whether the local avatar is currently sitting on the ground.
    pub sitting: bool,
}

/// The settings key (under the `[statusbar]` section) gating the agent-position
/// coordinates in the location read-out, mirroring the reference viewer's
/// `NavBarShowCoordinates`. Bare, like the floater-geometry keys — the section
/// only shapes the persisted file, not the lookup. `pub(crate)` for the
/// preferences floater's bound checkbox.
pub const SHOW_COORDINATES_KEY: &str = "statusbar_show_coordinates";

/// The in-world double-click action setting name (mirrors the reference's
/// `DoubleClickAction`). Bound by the preferences camera & movement tab
/// (`preferences_camera_move`).
pub const SETTING_DOUBLE_CLICK_ACTION: &str = "DoubleClickAction";

/// The agent-frame rear-view camera offset (forward, left, up metres), the
/// reference's `CameraOffsetRearView`: three metres behind and 0.75 m above the
/// focus. Its length is the default zoom distance and its elevation the default
/// tilt.
pub const CAMERA_OFFSET: Vec3 = Vec3::new(-3.0, 0.0, 0.75);

/// The closest the third-person camera zooms before it crosses into mouselook —
/// the reference's `LAND_MIN_ZOOM`, near enough to the head that the transition
/// reads as "stepping inside".
pub const MOUSELOOK_CROSS_DISTANCE: f32 = 0.5;

/// The farthest the third-person camera zooms from the avatar
/// (`MAX_CAMERA_DISTANCE_FROM_AGENT`).
pub const MAX_DISTANCE: f32 = 50.0;

/// Pitch clamp (just under a quarter turn) so the view never flips over the pole.
pub const MAX_PITCH: f32 = 1.54;

// ---------------------------------------------------------------------------
// World state the world's own layers share
// ---------------------------------------------------------------------------
//
// Where the camera is and what it is doing, how the avatar is moving, which
// region's terrain is loaded, what has been derendered: state produced by one
// part of the world layer and read by the others. It sits here so those parts
// can be separate crates that describe the same world without depending on
// each other to name it.

/// The camera mode: one of the three the [`ViewerCamera`] cycles between. See the
/// [module documentation](self).
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CameraMode {
    /// First-person: at the eyes, mouse aims, cursor captured.
    Mouselook,
    /// Orbiting third-person around a `FocusTarget` (the default).
    #[default]
    ThirdPerson,
    /// Free 6-DOF spectator camera (the promoted debug fly-camera).
    Flycam,
}

/// The marker on the one main viewer camera entity — the camera every world
/// system means by "the camera", as opposed to the reflection-probe, mirror and
/// minimap cameras that also carry `Camera3d`. Mode-agnostic: the same entity is
/// the camera in mouselook, third person and flycam.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct ViewerCamera;

/// The drivable state of the [`ViewerCamera`], shared by every mode.
///
/// Third person reads the orbit fields (`azimuth` /
/// `elevation` / `distance`); mouselook and
/// flycam read the aim fields (`yaw` / `pitch` /
/// `roll`). The flycam's *position* is the entity `Transform`'s
/// translation, not stored here — so a debug focus system that writes the
/// transform moves the flycam directly. The smoothed pose eases toward the mode's
/// desired pose so mode changes glide.
#[derive(Component, Debug, Clone)]
pub struct CameraRig {
    /// Third-person horizontal orbit offset from dead-behind the avatar, radians
    /// (`0` = rear view). Only a mouse-drag moves it — never the arrow keys.
    pub azimuth: f32,
    /// Third-person vertical orbit angle, radians (positive looks down onto the
    /// avatar). Seeded from `CAMERA_OFFSET`'s elevation.
    pub elevation: f32,
    /// Third-person camera distance from the focus, metres, clamped between
    /// [`MOUSELOOK_CROSS_DISTANCE`] and the tunable maximum
    /// (`CameraTuning::max_distance`, default `MAX_DISTANCE`).
    pub distance: f32,
    /// Mouselook / flycam yaw about Bevy up (`+Y`), radians.
    pub yaw: f32,
    /// Mouselook / flycam pitch about the camera's local right, radians, clamped
    /// to `±MAX_PITCH`.
    pub pitch: f32,
    /// Flycam roll about the camera's local forward, radians (only `CameraSpin`
    /// roll moves it).
    pub roll: f32,
    /// The world-space offset from a `FocusTarget::Point` focus to the camera
    /// eye, used only in focus-on-object. Captured at alt-click so the camera does
    /// not jump, and orbited / zoomed since. Unlike the avatar rear-view orbit
    /// (which follows the heading) this is fixed in the world, as the reference's
    /// object focus is.
    pub point_offset: Vec3,
    /// The last rendered eye position, eased toward the mode's desired eye.
    pub smoothed_eye: Vec3,
    /// The last rendered look-at point, eased toward the mode's desired focus.
    pub smoothed_focus: Vec3,
    /// Whether the smoothed pose has been seeded yet (so the first valid frame
    /// snaps rather than gliding in from an arbitrary origin).
    pub seeded: bool,
}

impl Default for CameraRig {
    /// The reference rear-view orbit: dead behind, tilted and distanced by
    /// `CAMERA_OFFSET`.
    fn default() -> Self {
        let horizontal =
            (CAMERA_OFFSET.x * CAMERA_OFFSET.x + CAMERA_OFFSET.y * CAMERA_OFFSET.y).sqrt();
        Self {
            azimuth: 0.0,
            elevation: CAMERA_OFFSET.z.atan2(horizontal),
            distance: CAMERA_OFFSET.length(),
            yaw: 0.0,
            pitch: 0.0,
            roll: 0.0,
            point_offset: Vec3::ZERO,
            smoothed_eye: Vec3::ZERO,
            smoothed_focus: Vec3::ZERO,
            seeded: false,
        }
    }
}

impl CameraRig {
    /// Reset the third-person orbit to the default rear view — the reference's
    /// `Escape` "reset camera". Leaves the aim / smoothing alone (the caller snaps
    /// via the mode change).
    pub fn reset_orbit(&mut self) {
        let default = Self::default();
        self.azimuth = default.azimuth;
        self.elevation = default.elevation;
        self.distance = default.distance;
    }

    /// Seed the third-person orbit from the debug framing environment variables,
    /// so the offline screenshot harness can frame the avatar from a chosen angle
    /// (the same `SL_VIEWER_CAMERA_*` knobs the old login-snap read). A no-op when
    /// none are set — the default rear view stands.
    ///
    /// `SL_VIEWER_CAMERA_ORBIT_DEG` swings the azimuth (90 = a side view),
    /// `_ELEV_DEG` the elevation (positive looks down), `_DISTANCE` the zoom.
    pub fn seed_orbit_from_env(&mut self) {
        let env_f32 = |key: &str| -> Option<f32> {
            std::env::var(key).ok().and_then(|value| value.parse().ok())
        };
        if let Some(orbit) = env_f32("SL_VIEWER_CAMERA_ORBIT_DEG") {
            self.azimuth = orbit.to_radians();
        }
        if let Some(elevation) = env_f32("SL_VIEWER_CAMERA_ELEV_DEG") {
            self.elevation = elevation.to_radians().clamp(-MAX_PITCH, MAX_PITCH);
        }
        if let Some(distance) = env_f32("SL_VIEWER_CAMERA_DISTANCE") {
            self.distance = distance.clamp(MOUSELOOK_CROSS_DISTANCE, MAX_DISTANCE);
        }
    }

    /// Reset the smoothing so the next frame snaps to the mode's desired pose
    /// rather than gliding — called after a region-origin shift
    /// (`crate::terrain::recenter_terrain`) so the eased pose does not drift
    /// across the 256 m rebase (the reference's sideways-after-crossing bug).
    pub const fn resnap(&mut self) {
        self.seeded = false;
    }

    /// Aim the flycam / mouselook along `direction` (Bevy Y-up space) by setting
    /// the yaw/pitch, so the aim survives the next frame's re-derivation. A zero
    /// direction is ignored. Yaw is measured so `-Z` gives yaw `0`; pitch is the
    /// elevation, clamped to `±MAX_PITCH`.
    pub fn aim_along(&mut self, direction: Vec3) {
        let dir = direction.normalize_or_zero();
        if dir == Vec3::ZERO {
            return;
        }
        self.yaw = (-dir.x).atan2(-dir.z);
        self.pitch = dir.y.asin().clamp(-MAX_PITCH, MAX_PITCH);
    }

    /// The rotation the rig's current yaw/pitch aims along (roll excluded) — the
    /// same reconstruction mouselook uses, so `rotation * NEG_Z` is the aim
    /// direction. A fixed flycam bakes this into its entity transform at spawn:
    /// `drive_flycam` integrates input deltas onto the transform and never reads
    /// the rig, so without this the transform keeps its identity (SL-north)
    /// orientation and `--camera-look-at` has no effect.
    #[must_use]
    pub fn aim_quat(&self) -> Quat {
        Quat::from_euler(EulerRot::YXZ, self.yaw, self.pitch, 0.0)
    }

    /// Place the focus-on-point eye offset directly (world space, eye = point +
    /// offset). The media-controls **Zoom** (`crate::media_controls`) uses this
    /// to park the camera squarely in front of a media face — the counterpart of
    /// `focus_on_object` capturing the offset at an alt-click.
    pub const fn set_point_offset(&mut self, offset: Vec3) {
        self.point_offset = offset;
    }
}

/// The authoritative kinematic motion of a full-object avatar (`pcode` 47) as of
/// its last `ObjectUpdate`, attached to the avatar's anchor entity by
/// `apply_object`(crate::avatars) and change-detected: a fresh insert on every
/// update reseeds the interpolation. Its presence marks the avatar anchors
/// `drive_avatar_motion` dead-reckons between updates. Coarse (minimap-only)
/// avatars carry no velocity and so get no [`AvatarMotion`].
#[derive(Debug, Component, Clone)]
pub struct AvatarMotion {
    /// Region-local position (metres, Second Life Z-up frame).
    pub position: Vector,
    /// Linear velocity (metres/second).
    pub velocity: Vector,
    /// Linear acceleration (metres/second²).
    pub acceleration: Vector,
    /// Orientation (a Second Life unit quaternion).
    pub rotation: Rotation,
    /// Angular velocity (rotation axis scaled by radians/second).
    pub angular_velocity: Vector,
    /// The region this avatar lives in, for the region-edge / neighbour lookups.
    pub region_handle: RegionHandle,
    /// The avatar's bounding-box height (object scale Z), for the ground floor.
    pub height: f32,
    /// Whether the anchor applies the object's orientation (a rigged body root) or
    /// stays upright (a placeholder sphere, which does not visibly rotate).
    pub apply_rotation: bool,
    /// The **collision (foot) plane** the simulator reports for this avatar: the
    /// surface its physics capsule is resting on, as the plane equation
    /// `[nx, ny, nz, w]` (a unit normal and a distance) in the region-local
    /// Second Life frame — `n · p = w`. `None` when the update carried no plane
    /// (a placeholder sphere, or a compressed update). This is the simulator's
    /// authoritative ground under the avatar — it already accounts for prims the
    /// avatar stands on, unlike a terrain-only lookup — and is what
    /// `crate::ground` resolves the foot-IK ground from, exactly as the
    /// reference viewer's `getGround` / `mFootPlane` do.
    collision_plane: Option<[f32; 4]>,
}

impl AvatarMotion {
    /// The avatar's current heading (yaw about the Second Life up axis, radians),
    /// extracted from its reported orientation. The viewer's movement controls
    /// (`crate::movement`) seed the walk heading from this so the first step does
    /// not snap the avatar to an arbitrary facing.
    #[must_use]
    pub fn yaw(&self) -> f32 {
        let Rotation { x, y, z, s } = &self.rotation;
        // Yaw about Z from a unit quaternion (`atan2(2(sz + xy), 1 - 2(y² + z²))`).
        let siny_cosp = 2.0 * (s * z + x * y);
        let cosy_cosp = 1.0 - 2.0 * (y * y + z * z);
        siny_cosp.atan2(cosy_cosp)
    }

    /// The avatar's vertical (Second Life Z-up) velocity component (metres/second):
    /// positive climbing, negative descending / falling. The client-side locomotion
    /// fallback (`crate::locomotion`) reads this to pick the ascend / descend /
    /// fall states — the only states with no advertised control-flag intent.
    #[must_use]
    pub const fn vertical_speed(&self) -> f32 {
        self.velocity.z
    }

    /// The region this avatar is in — the frame the terrain queries and its reported
    /// position are expressed in.
    #[must_use]
    pub const fn region(&self) -> RegionHandle {
        self.region_handle
    }

    /// The avatar's reported linear velocity (Second Life Z-up metres/second, region
    /// frame). The walk-adjust foot-slip servo (P31.14) matches the walk animation's
    /// playback speed to this.
    #[must_use]
    pub const fn sl_velocity(&self) -> Vec3 {
        Vec3::new(self.velocity.x, self.velocity.y, self.velocity.z)
    }

    /// The avatar's reported angular velocity (rotation axis scaled by radians/second,
    /// region frame). The fly-adjust bank (P31.14) rolls the pelvis into a turn by its
    /// Z component, exactly as the reference's `LLFlyAdjustMotion` does.
    #[must_use]
    pub const fn sl_angular_velocity(&self) -> Vec3 {
        Vec3::new(
            self.angular_velocity.x,
            self.angular_velocity.y,
            self.angular_velocity.z,
        )
    }

    /// Build the authoritative motion from an avatar's object update. `apply_rotation`
    /// is `true` for a rigged body root (whose anchor carries the object rotation)
    /// and `false` for a placeholder sphere.
    #[must_use]
    pub fn from_object(object: &Object, apply_rotation: bool) -> Self {
        Self {
            position: object.motion.position.clone(),
            velocity: object.motion.velocity.clone(),
            acceleration: object.motion.acceleration.clone(),
            rotation: object.motion.rotation.clone(),
            angular_velocity: object.motion.angular_velocity.clone(),
            region_handle: object.region_handle,
            height: object.scale.z,
            apply_rotation,
            collision_plane: object.motion.collision_plane,
        }
    }

    /// The simulator's collision (foot) plane for this avatar (region-local
    /// `[nx, ny, nz, w]`), or `None` when the last update carried none. The ground
    /// probe (`crate::ground`) resolves the foot-IK ground from it.
    #[must_use]
    pub const fn collision_plane(&self) -> Option<[f32; 4]> {
        self.collision_plane
    }
}

/// What kind of thing a blacklist entry names — the reference's `LLAssetType`,
/// narrowed to the kinds a viewer can actually refuse.
///
/// The two **in-world** kinds are what the derender menus produce and what the
/// scene mirror gates on. The three **asset** kinds are refused at their own
/// point of use instead — a blacklisted sound is never played, a blacklisted
/// animation never runs, a blacklisted texture is never fetched — which is
/// exactly where the reference refuses them. Their producers are the explorer
/// floaters (the sound explorer feeds `Sound`, the animation explorer
/// `Animation`); until those land, an asset entry comes from the per-account
/// file itself, which is also how the reference's distributed blacklist data
/// (`fsdata`) feeds textures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DerenderKind {
    /// An in-world object (the reference's `AT_OBJECT`).
    Object,
    /// An avatar (the reference's `AT_PERSON`).
    Resident,
    /// A sound asset, never played (`AT_SOUND`).
    Sound,
    /// An animation asset, never run (`AT_ANIMATION`).
    Animation,
    /// A texture asset, never fetched (`AT_TEXTURE`).
    Texture,
}

impl DerenderKind {
    /// The Fluent key naming this kind in the blacklist's Type column.
    #[must_use]
    pub const fn label_key(self) -> &'static str {
        match self {
            Self::Object => "derender-type-object",
            Self::Resident => "derender-type-resident",
            Self::Sound => "derender-type-sound",
            Self::Animation => "derender-type-animation",
            Self::Texture => "derender-type-texture",
        }
    }

    /// A stable sort rank, so the Type column orders deterministically.
    #[must_use]
    pub const fn rank(self) -> u8 {
        match self {
            Self::Object => 0,
            Self::Resident => 1,
            Self::Sound => 2,
            Self::Animation => 3,
            Self::Texture => 4,
        }
    }

    /// Whether this kind names something the **scene mirror** suppresses (as
    /// opposed to an asset refused at its point of use).
    #[must_use]
    pub const fn is_in_world(self) -> bool {
        matches!(self, Self::Object | Self::Resident)
    }
}

/// A component marking an object entity as a **particle source**, carrying the
/// decoded `LLPartSysData` particle-system parameters in Second Life semantics —
/// ready for P30.2 to drive a CPU particle simulation and render its particles as
/// camera-facing billboards.
///
/// Attached to (and refreshed / cleared on) each object entity by
/// `apply_object`(crate::objects) as its updates arrive. Only a *live* system
/// (non-zero CRC) is carried; see `particles_from_object`.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct ObjectParticleSystem {
    /// The decoded particle system: the source parameters (pattern, burst / age
    /// timing, emission angles / radius / speed, angular velocity, acceleration,
    /// texture, target) plus the template particle parameters it emits
    /// (per-particle age, start / end colour and scale, glow, blend).
    pub system: ParticleSystem,
}

/// The top of the pixel-area priority range: [`Priority::from_pixel_area`]
/// saturates here (`FULL_RESOLUTION_PIXEL_AREA` = `2048 * 2048`). Boost
/// priorities sit *strictly above* this, so a boosted asset always outranks even
/// the closest, largest prim face rather than merely tying with it on a region
/// dense with max-pixel-area content — mirroring how the reference viewer's
/// `BOOST_*` levels force a texture ahead of ordinary pixel-area-ranked content.
pub const PIXEL_AREA_CAP: u32 = 2048 * 2048;

/// The fixed boost priority for a region's four terrain detail textures
/// (`LLGLTexture::BOOST_TERRAIN`): one step into the boost band, so the ground is
/// not starved behind nearer prims (the terrain textures are few and always
/// under the camera, and the on-screen face pass does not rank them — terrain is
/// a custom material, not a tessellated prim face).
pub const TERRAIN_BOOST_PRIORITY: Priority = Priority::new(PIXEL_AREA_CAP + 1);

/// The fixed boost priority for the sky's referenced textures — the rainbow /
/// halo (and, later, sun / moon / cloud / bloom) maps the atmospheric sky dome
/// samples (`LLGLTexture::BOOST_HIGH`). In the boost band so a sky texture
/// resolves ahead of ordinary pixel-area-ranked scene faces (the sky is drawn
/// behind everything and, like terrain, is a custom material the on-screen face
/// pass cannot rank), one step above the avatar boost.
pub const SKY_BOOST_PRIORITY: Priority = Priority::new(PIXEL_AREA_CAP + 3);

/// The fixed boost priority for an avatar's textures and server-side bakes
/// (`LLGLTexture::BOOST_AVATAR` / `BOOST_AVATAR_BAKED`): above terrain, so the
/// avatars the camera is looking at resolve first even on a region dense with
/// max-pixel-area prims. The avatar is a skinned mesh, not a tessellated prim
/// face, so the on-screen face pass does not rank it — this boost is what keeps
/// its bakes ahead of the surrounding scene.
pub const AVATAR_BOOST_PRIORITY: Priority = Priority::new(PIXEL_AREA_CAP + 2);

/// A GPU-posed pose slot's identity (§5): either a rigged **avatar** keyed by
/// its wearer agent, or an **animesh** control avatar keyed by its animated-
/// object root. The registry, the feed and pass D's staging all key their
/// per-slot state on this, so avatars and animesh share the one passes-A–D
/// pipeline and one dense slot space.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum PoseSlotKey {
    /// A rigged avatar, keyed by its wearer agent.
    Avatar(AgentKey),
    /// An animesh control avatar, keyed by its animated-object root
    /// ([`ObjectKey`]) — it has no wearer agent.
    Animesh(ObjectKey),
    /// A synthetic **debug-crowd copy** of the local avatar
    /// (`SL_VIEWER_CROWD`, `gpu_avatars::crowd`), keyed by its crowd
    /// index. It carries no real agent: it reuses the local avatar's shape,
    /// clips and body submesh handles but stages its own slot, so passes A–D
    /// run at crowd scale for perf measurement. Never allocated when the env is
    /// unset (the crowd resource is empty), so a normal run never sees it.
    Crowd(u32),
}

/// One blacklist entry: what was derendered, where and when.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DerenderEntry {
    /// The derendered thing's persistent id (an object's full id, an avatar's
    /// agent id).
    pub id: Uuid,
    /// Its name, as the surface that derendered it knew it (may be empty when
    /// the object-properties reply had not landed yet).
    pub name: String,
    /// The region it was derendered in (empty when unknown).
    pub region: String,
    /// What kind of thing it is.
    pub kind: DerenderKind,
    /// Whether it survives a teleport and a relog (the "Blacklist" slice) or is
    /// a session-only "Temporary" derender.
    pub permanent: bool,
    /// When it was added, as Unix epoch seconds (stored as a plain integer so
    /// the file needs no date parser).
    pub added_epoch_secs: i64,
}

/// Why a region-scoped id is suppressed — which release frees it again.
///
/// Two sources share one suppression index (and therefore one ingest gate, one
/// transitive parent walk, one purge and one re-fetch): the **blacklist**, keyed
/// by the entry's id, and the **friends-only filter**, keyed by the non-friend
/// agent it hides. Keeping the source on each entry is what lets a release be
/// exact — un-blacklisting one object, or befriending one avatar, frees that
/// subtree and nothing else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HiddenBy {
    /// The blacklist entry with this id ([`DerenderList::remove`] frees it).
    Blacklist(Uuid),
    /// The friends-only filter, hiding this non-friend agent (turning the filter
    /// off — or befriending them — frees it).
    FriendsOnly(Uuid),
}

impl HiddenBy {
    /// The persistent id at the root of this suppression — a blacklist entry's
    /// id, or the hidden agent's.
    #[must_use]
    pub const fn id(self) -> Uuid {
        match self {
            Self::Blacklist(id) | Self::FriendsOnly(id) => id,
        }
    }
}

/// The viewer's derender / blacklist state, and the friends-only filter that
/// shares its suppression machinery.
#[derive(Resource, Debug, Default)]
pub struct DerenderList {
    /// The entries, newest last.
    pub entries: Vec<DerenderEntry>,
    /// The blacklisted ids and what each is blacklisted **as**, derived from
    /// [`Self::entries`] — the hot-path index every check goes through. Keyed by
    /// id alone: an id is one thing, so a second entry for it would be a
    /// contradiction, and the kind rides along so a sound check never matches an
    /// object entry.
    ids: HashMap<Uuid, DerenderKind>,
    /// The region-scoped ids currently suppressed, each mapped to **what hides
    /// it**: an entry's own object maps to its own source, a linkset child or
    /// attachment to its root's. Keeping the source is what lets a single
    /// release free exactly its own subtree (see `Self::release`).
    /// Session-derived, never persisted.
    pub hidden_scoped: HashMap<ScopedObjectId, HiddenBy>,
    /// Suppressions whose scene entities still need despawning, by source: a
    /// fresh blacklist entry, or an avatar the friends-only filter just started
    /// hiding.
    pub pending_ids: Vec<HiddenBy>,
    /// Scoped ids whose scene entities still need despawning (an object that
    /// was already tracked when its parent became hidden).
    pub pending_scoped: Vec<ScopedObjectId>,
    /// Scoped ids just **released** from suppression, to be re-fetched from the
    /// simulator so an un-derendered object comes back at once
    /// (`refetch_released_objects`).
    pub pending_refetch: Vec<ScopedObjectId>,
    /// Bumped on every change to [`Self::entries`], so the floater rebuilds
    /// exactly when the list moved.
    revision: u64,
    /// The per-account store path, resolved at login; `None` until then (and
    /// when the platform has no per-avatar directory, disabling persistence).
    pub path: Option<PathBuf>,
    /// Whether the on-disk list has been read — a once-per-session load.
    pub loaded: bool,
    /// Whether the **permanent** entries changed since the last flush.
    pub dirty: bool,
    /// Whether the **friends-only** filter is on (`viewer-render-friends-only`,
    /// the reference's `FSRenderFriendsOnly`): while it is, every avatar that is
    /// not a friend and not the agent itself is suppressed exactly as a
    /// derendered one is.
    pub friends_only: bool,
    /// The agent's own id, which the filter never hides.
    pub own_agent: Option<Uuid>,
    /// The friends the filter spares, mirrored from
    /// [`FriendsModel`] so the per-object gate stays
    /// one hash lookup.
    pub friends: HashSet<Uuid>,
}

impl DerenderList {
    /// Whether `id` is blacklisted **as** `kind` — the query each point of use
    /// runs (a sound before playing it, an animation before running it, a
    /// texture before fetching it).
    #[must_use]
    pub fn blacklists(&self, id: Uuid, kind: DerenderKind) -> bool {
        self.ids.get(&id) == Some(&kind)
    }

    /// Whether `id` names an in-world thing this viewer must not draw — a
    /// blacklisted object / avatar, or an avatar the friends-only filter hides.
    /// The hot-path query the scene mirror runs per streamed object.
    #[must_use]
    pub fn hides_in_world(&self, id: Uuid) -> bool {
        self.blacklists_in_world(id) || self.friends_only_hides(id)
    }

    /// Whether `id` is on the **blacklist** as an in-world kind (as opposed to
    /// being hidden by the friends-only filter).
    #[must_use]
    pub fn blacklists_in_world(&self, id: Uuid) -> bool {
        self.ids.get(&id).is_some_and(|kind| kind.is_in_world())
    }

    /// Whether the friends-only filter hides the avatar `agent`: the filter is
    /// on, and they are neither the agent itself nor a friend. Animesh
    /// ("control") avatars are exempt for free — they are ordinary mesh objects
    /// on the wire, never `pcode` 47, so this gate never sees them, which is the
    /// reference's `!avatar->isControlAvatar()` by construction.
    #[must_use]
    pub fn friends_only_hides(&self, agent: Uuid) -> bool {
        self.friends_only && self.own_agent != Some(agent) && !self.friends.contains(&agent)
    }

    /// Every blacklisted id of `kind` — how a consumer that cannot consult the
    /// list per item (the texture store, whose fetch gate is not a Bevy system)
    /// mirrors the set it needs.
    #[must_use]
    pub fn ids_of_kind(&self, kind: DerenderKind) -> HashSet<Uuid> {
        self.ids
            .iter()
            .filter(|(_id, held)| **held == kind)
            .map(|(id, _held)| *id)
            .collect()
    }

    /// Whether the object with region-scoped id `scoped` must not be mirrored
    /// into the scene: it is blacklisted itself, or it hangs off something that
    /// is (a linkset child, an attachment). Maintained by
    /// `index_derendered_objects`.
    #[must_use]
    pub fn is_suppressed(&self, scoped: ScopedObjectId) -> bool {
        self.hidden_scoped.contains_key(&scoped)
    }

    /// What suppresses `scoped`, if anything — the source an inherited
    /// suppression is inherited from.
    #[must_use]
    pub fn suppressing_root(&self, scoped: ScopedObjectId) -> Option<HiddenBy> {
        self.hidden_scoped.get(&scoped).copied()
    }

    /// Record every id in `removed` as suppressed by the blacklisted `root`.
    ///
    /// The scene purge calls this with what it despawned, because those ids are
    /// often the *only* record of them: the simulator streams a static object
    /// once, so an object derendered long after it was streamed never produces
    /// another update for `index_derendered_objects` to learn from — and
    /// without the record, un-derendering it would have nothing to re-fetch.
    pub fn note_hidden(
        &mut self,
        removed: impl IntoIterator<Item = ScopedObjectId>,
        root: HiddenBy,
    ) {
        for scoped in removed {
            let _prior = self.hidden_scoped.insert(scoped, root);
        }
    }

    /// The whole list, in insertion order.
    #[must_use]
    pub fn entries(&self) -> &[DerenderEntry] {
        &self.entries
    }

    /// The list revision — a view stores the value it last built at and rebuilds
    /// when it advances.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Add (or replace) an entry, marking the scene for a purge of its id.
    /// Re-derendering an id already listed **upgrades** it: a temporary entry
    /// that is blacklisted becomes permanent, never the other way round, which
    /// is what the reference's `addNewItemToBlacklist` overwrite amounts to for
    /// the only two paths that reach it.
    pub fn add(&mut self, entry: DerenderEntry) {
        if let Some(existing) = self.entries.iter_mut().find(|held| held.id == entry.id) {
            let upgraded = entry.permanent && !existing.permanent;
            existing.permanent |= entry.permanent;
            if existing.name.is_empty() {
                existing.name.clone_from(&entry.name);
            }
            if upgraded {
                self.dirty = true;
                self.revision = self.revision.wrapping_add(1);
            }
            return;
        }
        self.dirty |= entry.permanent;
        self.pending_ids.push(HiddenBy::Blacklist(entry.id));
        self.entries.push(entry);
        self.reindex();
    }

    /// Drop the entry for `id`, if held, releasing everything it suppressed and
    /// queueing those objects for a re-fetch (see `Self::release`).
    pub fn remove(&mut self, id: Uuid) {
        let before = self.entries.len();
        self.entries.retain(|entry| entry.id != id);
        if self.entries.len() == before {
            return;
        }
        self.dirty = true;
        self.reindex();
        // Release exactly what this entry was suppressing — its own object and
        // everything that inherited the suppression from it — and nothing else:
        // another blacklisted root's children (and anything the friends-only
        // filter hides) must stay hidden.
        self.release(|root| root == HiddenBy::Blacklist(id));
    }

    /// Drop every temporary entry (a teleport, or the floater's Clear temporary).
    pub fn clear_temporary(&mut self) {
        let before = self.entries.len();
        self.entries.retain(|entry| entry.permanent);
        if self.entries.len() == before {
            return;
        }
        self.reindex();
        // Every suppression whose blacklist entry just left the list is
        // released; the permanent entries — and the friends-only filter — keep
        // theirs.
        let live: HashSet<Uuid> = self.ids.keys().copied().collect();
        self.release(|root| match root {
            HiddenBy::Blacklist(id) => !live.contains(&id),
            HiddenBy::FriendsOnly(_agent) => false,
        });
    }

    /// Drop every suppression whose root `released` accepts, and queue the
    /// freed region-scoped ids for a re-fetch.
    ///
    /// The re-fetch is what makes "Re-render" mean it: the simulator streams an
    /// object once, and the viewer dropped every update for it while it was
    /// suppressed, so simply forgetting the entry would leave the object absent
    /// until the region streamed it again (a teleport away and back — which is
    /// all the reference does). Because the index kept the object's *region-local*
    /// id the whole time, we can instead ask for it back right now
    /// (`RequestMultipleObjects`, a full cache miss).
    fn release(&mut self, released: impl Fn(HiddenBy) -> bool) {
        let freed: Vec<ScopedObjectId> = self
            .hidden_scoped
            .iter()
            .filter(|(_scoped, root)| released(**root))
            .map(|(scoped, _root)| *scoped)
            .collect();
        for scoped in &freed {
            let _dropped = self.hidden_scoped.remove(scoped);
        }
        self.pending_refetch.extend(freed);
    }

    /// Re-apply the friends-only filter after its inputs moved (the toggle
    /// flipped, the friends list changed, or the own agent became known): free
    /// everyone it no longer hides — queuing their re-fetch, so they come back
    /// without a relog — and queue a purge for every avatar it now does.
    ///
    /// `known` is the agents this viewer currently tracks; only they can have
    /// anything in the scene to purge, and anyone streaming in later is caught
    /// by the ingest gate instead.
    pub fn resync_friends_only(&mut self, known: &[Uuid]) {
        let spared: HashSet<Uuid> = self
            .hidden_scoped
            .values()
            .filter_map(|hidden| match hidden {
                HiddenBy::FriendsOnly(agent) => Some(*agent),
                HiddenBy::Blacklist(_id) => None,
            })
            .filter(|agent| !self.friends_only_hides(*agent))
            .collect();
        if !spared.is_empty() {
            self.release(
                |root| matches!(root, HiddenBy::FriendsOnly(agent) if spared.contains(&agent)),
            );
        }
        for agent in known {
            if self.friends_only_hides(*agent) {
                self.pending_ids.push(HiddenBy::FriendsOnly(*agent));
            }
        }
    }

    /// Rebuild the derived id index and bump the revision.
    fn reindex(&mut self) {
        self.ids = self
            .entries
            .iter()
            .map(|entry| (entry.id, entry.kind))
            .collect();
        self.revision = self.revision.wrapping_add(1);
    }
}

// ---------------------------------------------------------------------------
// Object flag bits and attachment points
// ---------------------------------------------------------------------------
//
// The `ObjectUpdate` flag word says what may be done with an object and how it
// behaves: whether this agent owns it, may copy, modify or move it, whether it
// takes physics or is phantom, whether it accepts a dropped inventory item.
// Every tier tests against these -- the build tool to grey a control, a menu
// to enable an entry, the world to decide whether to simulate -- so the bits
// live here rather than in the module that happens to parse the word.

/// The agent-relative `FLAGS_OBJECT_MODIFY` bit of `PrimFlags` (`object_flags.h`):
/// this agent may modify the object. The simulator sets it per-agent, folding in
/// the object's owner / group / everyone modify permission.
pub const FLAGS_OBJECT_MODIFY: u32 = 1 << 2;

/// The agent-relative `FLAGS_OBJECT_COPY` bit: this agent may copy the object.
pub const FLAGS_OBJECT_COPY: u32 = 1 << 3;

/// The agent-relative `FLAGS_OBJECT_YOU_OWNER` bit: this agent owns the object.
pub const FLAGS_OBJECT_YOU_OWNER: u32 = 1 << 5;

/// The agent-relative `FLAGS_OBJECT_MOVE` bit: this agent may move (position /
/// rotate) the object — set for the owner and for an "anyone can move" object.
pub const FLAGS_OBJECT_MOVE: u32 = 1 << 8;

/// The `FLAGS_ALLOW_INVENTORY_DROP` bit of `PrimFlags` (`object_flags.h`): the
/// object is set to let **anyone** add inventory to its contents, the reference
/// viewer's `flagAllowInventoryAdd`. Unlike the modify / copy bits this is a
/// property of the object itself (not agent-relative), and it is the one
/// exception to needing modify on the object to drop an item into it.
pub const FLAGS_ALLOW_INVENTORY_DROP: u32 = 1 << 16;

/// The `FLAGS_PHANTOM` bit of `PrimFlags` (`object_flags.h`): the object is
/// non-solid — nothing collides with it. The static collider index
/// (`physics::build_static_colliders`) still gives a phantom prim a
/// collider (so it is in the shared spatial index for proximity queries) but
/// files it in the non-collidable layer.
pub const FLAGS_PHANTOM: u32 = 1 << 10;

/// Whether a raw attachment-point id names a HUD (screen-space) slot rather than
/// a body joint — the reference viewer's `LLVOVolume::isHUDAttachment`, which
/// tests the same `31..=38` id range.
#[must_use]
pub const fn is_hud_point(point_id: u8) -> bool {
    AttachmentPoint::from_code(point_id).is_hud()
}

// ---------------------------------------------------------------------------
// Small world vocabulary the world's own layers share
// ---------------------------------------------------------------------------
//
// Which render layer the HUD draws on and whether an entity sits on it, the
// markers on an avatar's anchor and its pick target, a region's terrain
// surface, the name-tag render layers, and what a click on a media prim
// carries. Each is named by parts of the world that do not otherwise know
// about each other, and none of them names anything back.

/// The render layer the whole HUD subtree lives on, and which the world (fly)
/// camera — on the default layer `0` — therefore does not render. P35.2's HUD
/// camera renders this layer and nothing else, so the HUD is drawn exactly once,
/// in screen space, and never leaks into the world pass (or into a reflection
/// probe's capture, which is likewise a default-layer camera).
pub const HUD_RENDER_LAYER: usize = 1;

/// Whether an entity's render layers put it on the HUD layer — i.e. whether it is
/// part of the HUD subtree rather than the world scene.
///
/// The HUD screen propagates `HUD_RENDER_LAYER` down its hierarchy, so every
/// entity of a routed HUD attachment (its object entity, its geometry holder, and
/// each face) carries it. The world's pixel-area render-priority / level-of-detail
/// pass uses this to recognise geometry it must not rank by on-screen size: a HUD
/// sits in its own space, where the world camera's distance to it is meaningless
/// (the reference viewer special-cases it the same way, treating every HUD face as
/// full-screen and pinning it to the finest level of detail).
///
/// `layers` is the entity's [`RenderLayers`] component, absent on a world entity
/// (which is then implicitly on the default layer `0`).
#[must_use]
pub fn on_hud_layer(layers: Option<&RenderLayers>) -> bool {
    layers.is_some_and(|layers| layers.intersects(&RenderLayers::layer(HUD_RENDER_LAYER)))
}

/// A marker component on the transform-bearing *anchor* entity of an avatar —
/// its placeholder sphere or the root of its rigged body — whose world position
/// the name-tag placement (`name_tag_billboard::follow_tag_anchors`)
/// follows to float the tag.
#[derive(Component, Debug, Clone, Copy)]
pub struct AvatarAnchor;

/// A component tagging an entity as **part of** a specific avatar, carrying that
/// avatar's [`AgentKey`] — the reusable "what avatar is this?" identity that
/// picking reads.
///
/// It sits on every pickable piece of an avatar: the placeholder sphere, each
/// rigged base-body part, each **worn rigged-mesh submesh** (on a modern
/// mesh-body avatar the base body is hidden, so the worn mesh *is* the
/// silhouette), and the floating name tag. That breadth is the point — a ray
/// that hits any body part, or a pointer over the name tag (resolved by the
/// `name_tag_billboard::NameTagHitTest` rect test — tags are custom
/// billboard meshes no picking backend covers), resolves to the
/// same agent through one component, so a caller never has to know *which* piece
/// it hit. Kept separate from `AvatarBodyPart` (which also holds an agent) so
/// non-mesh pieces (the sphere, the name tag) can carry the identity too, and
/// so consumers — the GPU pick-tag assignment
/// (`crate::gpu_pick::assign_avatar_pick_tags`) is the main one — read a
/// single, purpose-named component rather than three different markers.
#[derive(Component, Debug, Clone, Copy)]
pub struct AvatarPickTarget {
    /// The avatar this entity is part of.
    pub agent: AgentKey,
}

impl AvatarPickTarget {
    /// Tag a pickable piece of `agent` (used by the rigged-attachment spawn in
    /// `objects`, where the wearer is known only sometimes).
    #[must_use]
    pub const fn new(agent: AgentKey) -> Self {
        Self { agent }
    }

    /// The avatar this entity belongs to.
    #[must_use]
    pub const fn agent(&self) -> AgentKey {
        self.agent
    }
}

/// Marks a rendered land-patch entity as a **walkable ground surface**, so the
/// avatar ground probe (`ground`, P31.14) can accept it as something the
/// feet may plant on — the same role the reference viewer's
/// `LLWorld::resolveStepHeightGlobal` gives the land when its object raycast misses.
///
/// The probe only ever accepts geometry that is explicitly ground-like (this, and
/// object faces), so it never plants an avatar's feet on the water plane, a particle
/// billboard, the sky dome, or another avatar.
#[derive(Debug, Component)]
pub struct TerrainSurface;

/// One media face: the object (grid-wide key) and the Linden face index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MediaTarget {
    /// The object carrying the face.
    pub object: ObjectKey,
    /// The face index.
    pub face: PrimFaceId,
}

/// A left click on a media-capable prim face, claimed from the world touch
/// pick (`hud_pick::pick_and_touch`) before it becomes a touch.
#[derive(Message, Debug, Clone)]
pub struct MediaWorldClick {
    /// The face entity the ray struck.
    pub entity: Entity,
    /// The picked object's scoped id.
    pub scoped: ScopedObjectId,
    /// The struck face.
    pub face: PrimFaceId,
    /// The **sampled** texture coordinate of the hit (the `SurfaceInfo` UV:
    /// texture placement applied, Second Life bottom-up `v`).
    pub uv: Vec2,
}

/// The in-world media focus / hover state. Read by
/// `crate::input_context::compute_input_context` (a focused media face
/// takes the keyboard away from the world) and by the floating controls bar
/// (`crate::media_controls`).
#[derive(Resource, Debug, Default)]
pub struct MediaFocus {
    /// The face holding media keyboard focus, if any.
    pub focused: Option<MediaTarget>,
    /// Whether the focused face is a browser page that takes the keyboard
    /// away from the world (`input_context`); a focused *video*
    /// face keeps the bar visible but leaves the keyboard with the world —
    /// there is nothing to type at a video.
    pub focused_takes_keyboard: bool,
    /// The media face under the cursor this frame, if any.
    pub hover: Option<MediaTarget>,
    /// The surface pixel under the cursor on the hover face.
    pub hover_pixel: Option<(i32, i32)>,
    /// The world-space face normal at the **last** media hover hit (not
    /// cleared when the hover leaves), for the controls bar's camera zoom.
    pub hover_normal: Option<Vec3>,
    /// Whether a forwarded button press is outstanding (its release is
    /// forwarded to the same surface).
    pub pressed: Option<MediaTarget>,
}

/// How many leading hex characters of the agent id to show as a provisional tag
/// before the real name resolves.
const PROVISIONAL_ID_CHARS: usize = 8;

/// The user's **own** naming of a resident, overriding what the grid answers:
/// a contact-set pseudonym or display-name removal
/// (`crate::contact_sets`, `viewer-contact-set-pseudonyms`).
///
/// It is mirrored into the name cache rather than consulted beside it, so every
/// surface that resolves a name through a [`NameRecord`] — name tags, the radar,
/// tooltips, the inspectors, linkified names — shows the alias without knowing
/// that contact sets exist. The grid's own answer is never overwritten; it stays
/// in the record's fields and [`NameRecord::grid_name`] returns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NameAlias {
    /// Show this text in place of the resident's name. It carries the
    /// reference's **quoted** form (`'Nickname'`), which is what keeps an alias
    /// from being read as the grid's own answer.
    Pseudonym(String),
    /// Show this resident's legacy name only — the reference's display-name
    /// removal (`hasDisplayNameRemoved`), for someone whose chosen display name
    /// the user would rather not see.
    LegacyOnly,
}

/// One agent's resolved names, merged from every source: the instant
/// `ObjectUpdate` NameValue seed, the legacy `UUIDNameReply`, and the
/// `GetDisplayNames` cap (SL only — OpenSim generally lacks the cap, so the
/// legacy fields must always work on their own) — plus the user's own
/// [alias](NameAlias) for them, if they gave one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NameRecord {
    /// The legacy `"First Last"` name (`"First"` alone for a single-name
    /// account), from whichever source arrived first.
    pub legacy: Option<String>,
    /// The immutable dotted SLID (`"first.last"`), display-name cap only.
    pub username: Option<String>,
    /// The chosen display name, display-name cap only (`None` on OpenSim).
    pub display_name: Option<String>,
    /// Whether the display name is just the legacy-derived default (a custom
    /// display name shows with the username line under it, the reference's
    /// `is_display_name_default` behaviour).
    pub is_display_name_default: bool,
    /// The user's own name for this resident, mirrored from the contact-set
    /// store by `crate::contact_sets::apply_name_aliases`. Not a grid answer,
    /// and never written by an ingest path.
    pub alias: Option<NameAlias>,
}

impl NameRecord {
    /// The name to show for this resident: the user's own alias when they gave
    /// one, else the display name when one resolved, else the legacy name.
    #[must_use]
    pub fn preferred_name(&self) -> Option<&str> {
        match self.alias {
            Some(NameAlias::Pseudonym(ref shown)) => Some(shown),
            Some(NameAlias::LegacyOnly) => self.legacy.as_deref(),
            None => self.grid_name(),
        }
    }

    /// [`Self::preferred_name`] with display names switched off (the name tags'
    /// `ShowDisplayNames`): the legacy name, but a **pseudonym still wins** —
    /// the toggle says which of the grid's two names to believe, and an alias is
    /// not one of the grid's answers.
    #[must_use]
    pub fn legacy_display_name(&self) -> Option<&str> {
        match self.alias {
            Some(NameAlias::Pseudonym(ref shown)) => Some(shown),
            Some(NameAlias::LegacyOnly) | None => self.legacy.as_deref(),
        }
    }

    /// The **grid's** own answer, with no alias applied — what a surface shows
    /// when the person's real identity is the point (a profile), and what a
    /// name filed in a store must remember.
    #[must_use]
    pub fn grid_name(&self) -> Option<&str> {
        self.display_name.as_deref().or(self.legacy.as_deref())
    }

    /// Whether the shown name is something other than this resident's legacy
    /// name — a custom display name, or a pseudonym. It is what puts the
    /// username line under a name tag: the shown name does not say who this is,
    /// so the username has to.
    #[must_use]
    pub const fn has_custom_display_name(&self) -> bool {
        match self.alias {
            Some(NameAlias::Pseudonym(_)) => true,
            // The point of display-name removal is to be shown the legacy name,
            // which is the one name that needs no username under it.
            Some(NameAlias::LegacyOnly) => false,
            None => self.display_name.is_some() && !self.is_display_name_default,
        }
    }
}

/// One nearby avatar as the map surfaces (minimap, radar) consume it — see
/// [`AvatarState::map_avatars`].
#[derive(Debug, Clone, Copy)]
pub struct MapAvatar {
    /// The avatar's agent id.
    pub agent: AgentKey,
    /// The world entity whose transform places the avatar.
    pub anchor: Entity,
    /// For a coarse-only avatar, its last coarse altitude in metres (`0` /
    /// `1020` are the "unknown" sentinels); `None` for a precisely-known
    /// full-object avatar.
    pub coarse_z: Option<f32>,
}

/// The pair of entities rendering one avatar: its world-space anchor (a
/// placeholder sphere or the root of a rigged body) and its screen-space
/// name-tag text node.
#[derive(Debug, Clone, Copy)]
pub struct AvatarEntities {
    /// The anchor entity — a placeholder sphere or a rigged-body root. Despawned
    /// recursively, so a body's whole joint / mesh sub-hierarchy goes with it.
    pub anchor: Entity,
    /// The floating name-tag UI text entity.
    pub label: Entity,
}

/// Viewer-side avatar bookkeeping: the placeholder entities for every nearby
/// avatar, split by which stream it came from, plus a legacy-name cache.
///
/// A full-object avatar's `ObjectRemoved` carries only its scoped local id (not
/// its agent id), so `by_scoped` maps back to the agent id the
/// avatar is keyed by.
///
/// Everything here is **data about avatars**: entity ids, names, positions,
/// flags. The render machinery that produces it — the placeholder sphere's mesh
/// and material handles, the rigged base body, the texture fetches — stays in
/// the world layer (`avatars::AvatarPlaceholderAssets` and the free functions
/// beside it), which is what lets the bookkeeping sit below every surface that
/// reads it: the radar, the minimap, the name tags, the pickers, the profiles.
#[derive(Debug, Resource, Default)]
pub struct AvatarState {
    /// The region the Bevy scene is anchored at (origin `<0,0,0>`), so a **full
    /// object** avatar in a neighbour region is offset onto the right terrain
    /// (mirroring the coarse-dot and object offsets) and every avatar is re-based
    /// when this moves (`recenter_avatars`). `None` until the first region is
    /// known; kept in lockstep with the object/terrain origins (all follow
    /// `SlIdentity`'s root handle).
    pub origin: Option<RegionHandle>,
    /// Avatars known as a full in-world object (`pcode` 47), keyed by agent id;
    /// their sphere follows the object's precise position.
    pub objects: HashMap<AgentKey, AvatarEntities>,
    /// Avatars known only from coarse (minimap) locations — not (currently) a full
    /// object — keyed by agent id; their sphere sits at the 1 m coarse position.
    pub coarse: HashMap<AgentKey, AvatarEntities>,
    /// The source region of each coarse-only avatar (R24). `CoarseLocationUpdate`
    /// arrives per-region (root *and* each neighbour child circuit), so a coarse
    /// dot is reconciled only against its own region's update — a neighbour's
    /// update must not despawn the root region's dots. Also lets a region's dots be
    /// dropped when that region is disabled (an empty update for the region).
    pub coarse_region: HashMap<AgentKey, RegionHandle>,
    /// A reverse map from an object's scoped id to its agent id, so an
    /// `ObjectRemoved` can find the avatar to despawn.
    pub by_scoped: HashMap<ScopedObjectId, AgentKey>,
    /// The per-avatar attachment-point node entities, keyed by agent id then by
    /// raw attachment-point id (P16.2). Each node is a child of its skeleton joint
    /// carrying the fixed `avatar_lad.xml` offset; a worn attachment parents to the
    /// node for its point so it seats at the stored local offset from the joint.
    /// Absent for a sphere-only (no `--viewer-assets`) avatar.
    pub attachment_nodes: HashMap<AgentKey, HashMap<u8, Entity>>,
    /// The camera's head-focus socket entity per rigged avatar (Phase 4 §5.4):
    /// a root child the pose driver's socket writer places at the posed `mHead`
    /// joint each frame, so the camera holds the animated head without a head
    /// joint entity. Absent for a sphere-only avatar; despawned with the anchor.
    pub head_sockets: HashMap<AgentKey, Entity>,
    /// Resolved names, keyed by agent id — the "simple name cache" that keeps
    /// a repeatedly-seen avatar from being re-requested; merged from the
    /// NameValue seed, the legacy `UUIDNameReply` and the display-name cap.
    pub names: HashMap<AgentKey, NameRecord>,
    /// The user's own [aliases](NameAlias), mirrored from the contact-set store
    /// by `crate::contact_sets::apply_name_aliases`. Held beside the records
    /// (as well as folded into them) so an avatar seen *after* the alias was
    /// given still shows it: [`Self::name_entry`] folds it in as the record is
    /// created. Session state — the store is what persists.
    name_aliases: HashMap<AgentKey, NameAlias>,
    /// Group titles from each avatar object's NameValue `Title` — the classic
    /// mechanism the reference reads for other avatars' tags. (The own
    /// avatar's fresher title comes from `ActiveGroupChanged` via
    /// [`crate::world_api::GroupsModel`].)
    titles: HashMap<AgentKey, String>,
    /// Agents whose name has already been requested (but has not necessarily
    /// arrived), so the same request is never sent twice.
    requested: HashSet<AgentKey>,
    /// Agents queued for this frame's batched name request
    /// (`flush_name_requests`): one `UUIDNameRequest` **and** one
    /// `GetDisplayNames` cap call per frame, however many avatars appeared
    /// (each cap call costs an HTTP request; cap absence — OpenSim — is a
    /// silent no-op, which is why the legacy request always goes out too).
    pub pending_name_requests: HashSet<AgentKey>,
    /// The latest `AvatarAppearance.visual_params` byte vector per avatar, kept so
    /// a body spawned after (or re-spawned) can be morphed from the last known
    /// appearance (P13.3).
    pub appearances: HashMap<AgentKey, Vec<u8>>,
    /// Avatars whose rigged body needs its appearance (re)applied — its morphs
    /// re-blended and its skeleton re-deformed — set on a fresh appearance and on
    /// a newly spawned body, drained by `apply_avatar_appearance`.
    pub appearance_dirty: HashSet<AgentKey>,
    /// The debounce ledger behind [`appearance_dirty`](Self::appearance_dirty):
    /// per still-unserviced avatar, when (app elapsed seconds) it was first and
    /// last marked dirty. `apply_avatar_appearance` folds fresh marks in each
    /// frame and picks avatars from here under its per-frame budget — a
    /// never-shaped avatar immediately, a re-marked one only after a quiet
    /// window, so the appearance → body-spawn → bake-decode trigger cascade
    /// resolves once instead of once per trigger.
    pub appearance_pending: HashMap<AgentKey, AppearanceDirtyStamps>,
    /// A generation counter over every input the skeleton pose fold consumes from
    /// this state (deformations, volume deformations, joint overrides, body
    /// physics): bumped by [`bump_pose_inputs`](Self::bump_pose_inputs) whenever
    /// one is (re)applied. The pose gate re-evaluates **all** avatars for one
    /// frame on any bump — coarse but simple, and these are rare events.
    pose_inputs_generation: u64,
    /// The joint position overrides each avatar's worn rigged meshes impose (R1),
    /// keyed by agent id then by the contributing **mesh asset id**. Kept per-mesh
    /// (rather than pre-merged) so the set can be rebuilt as meshes come and go — the
    /// reference viewer's `clearAttachmentOverrides` + rebuild — and so a per-joint
    /// conflict resolves to the highest-mesh-id override (`findActiveOverride`), via
    /// [`effective_joint_overrides`](Self::effective_joint_overrides). Absent for an
    /// avatar wearing no position-carrying rig — its skeleton stays on the plain
    /// appearance shape. `apply_avatar_appearance` folds the effective set in.
    joint_overrides: HashMap<AgentKey, HashMap<Uuid, JointOverrides>>,
    /// Every worn **rigged mesh asset id** bound to each avatar's skeleton, kept so
    /// the avatar-state dump (viewer-avatar-state-dump-replay) can record which
    /// meshes make up an avatar — the heavy geometry itself already persists in the
    /// mesh cache, so only the id set is needed to reconstruct it offline.
    worn_rigged_meshes: HashMap<AgentKey, HashSet<Uuid>>,
    /// Whether each avatar's `TEX_SKIRT_BAKED` slot holds a visible bake, from its
    /// latest appearance — the reference viewer's skirt-worn test. Absent means
    /// not yet known, treated as no skirt (the base skirt mesh stays hidden).
    pub skirt_visible: HashMap<AgentKey, bool>,
    /// Each avatar's ingested body-physics (`WT_PHYSICS`) configuration (P34.1),
    /// resolved from its latest appearance: the six breast / belly / butt
    /// spring-damper motions, their settings, and the runtime morph params each
    /// one drives. The per-frame simulation (P34.2) reads it; an avatar whose
    /// appearance switches physics off keeps an entry whose motions are all
    /// inactive.
    pub body_physics: HashMap<AgentKey, BodyPhysics>,
    /// The visible baked-texture id in each base-body region slot per avatar,
    /// from its latest appearance (P14.1): the published baked UUIDs the viewer
    /// fetches through the shared `TextureManager` and (from P14.2) drapes over
    /// the system body. Keyed by baked slot (`BODY_BAKE_SLOTS`); a slot with no
    /// real bake is simply absent.
    pub baked_textures: HashMap<AgentKey, HashMap<usize, TextureKey>>,
    /// The base-body region slots each avatar has baked **invisible**
    /// (`IMG_INVISIBLE`) via a worn system alpha layer, from its latest appearance
    /// (R22). These regions are hidden outright (`apply_avatar_part_visibility`),
    /// matching the reference viewer's `isTextureVisible`, so the system body does
    /// not render and z-fight a non-BOM mesh body worn over it.
    pub invisible_regions: HashMap<AgentKey, HashSet<usize>>,
    /// The Current Outfit Folder version whose bakes were last fetched per avatar
    /// (P14.4), so a later `AvatarAppearance` with a strictly-older `cof_version`
    /// (an out-of-order / duplicate resend) is skipped and cannot clobber a newer
    /// bake. Absent means none seen yet; an appearance with no `cof_version`
    /// (OpenSim / the older path) is always ingested.
    pub baked_cof_version: HashMap<AgentKey, i32>,
    /// Avatars whose body-region bake materials need (re)assigning — set on a
    /// fresh appearance and on a newly spawned body, drained by
    /// `assign_avatar_bake_materials` (P14.2).
    pub bake_dirty: HashSet<AgentKey>,
    /// The parent scoped id of every tracked non-root object (linkset children and
    /// attachments), so an attachment's chain can be chased up to its avatar root
    /// (P13.5 `IMG_USE_BAKED_*` region hide).
    pub object_parents: HashMap<ScopedObjectId, ScopedObjectId>,
    /// For every tracked non-root object whose texture entry carries
    /// `IMG_USE_BAKED_*` sentinels, the baked slots it replaces — aggregated up the
    /// attachment chain to hide the matching base-avatar mesh regions.
    pub baked_hides: HashMap<ScopedObjectId, Vec<usize>>,
    /// Non-root objects whose texture entry has already been scanned for
    /// `IMG_USE_BAKED_*` sentinels, so a motion-only update never re-decodes it.
    scanned_objects: HashSet<ScopedObjectId>,
    /// Each rigged avatar's resolved skeletal deformations, the shape
    /// `apply_avatar_appearance` last applied — kept so the animation driver
    /// (P18.3) can re-run the Second Life skeletal recurrence with the playing
    /// motion folded in and write each joint's world matrix straight to its
    /// `GlobalTransform` (avoiding the limb-shear a rotation overlaid onto the
    /// baked-scale rest transform would cause). Absent for a sphere-only
    /// (no `--viewer-assets`) avatar, or before its first appearance.
    pub deformations: HashMap<AgentKey, SkeletalDeformations>,
    /// Each rigged avatar's resolved **collision-volume** displacements (P34.3):
    /// the shape morphs' `<volume_morph>` children, which move the volumes a worn
    /// rigged-mesh body is rigged to. Resolved and folded into the skeletal
    /// recurrence alongside [`deformations`](Self::deformations).
    pub volume_deformations: HashMap<AgentKey, VolumeDeformations>,
    /// Each avatar's resolved **root drop** (R23): how far below the reported
    /// wire Z its body-root entity is planted, in Second Life Z-up metres —
    /// `root_drop_from_metrics` of the shape's `computeBodySize` quantities
    /// (the wire Z is the physics-capsule *centre*, so the drop is half the
    /// shape-scaled body height, corrected for the pelvis sitting above the
    /// root and any hover). Shoe heel / platform offsets (R17) fold in through
    /// the foot term of those metrics, as in the reference. Absent (the rest
    /// shape's `AvatarBody::rest_root_drop` applies) until an appearance
    /// resolves, or for a sphere-only avatar.
    pub root_drops: HashMap<AgentKey, f32>,
    /// Each avatar's resolved **seat drop** (R23 counterpart): the pelvis's
    /// shape-scaled local height above the body root (`pelvis_local_z`), keyed by
    /// agent. A sit offset targets the avatar **root** (hips), so a seated avatar's
    /// anchor is dropped by this so the hips land on the sit target
    /// (`place_seated_avatars`) — unlike the standing [`root_drops`](Self::root_drops),
    /// which also folds in the capsule-centre correction that does not apply while
    /// seated. Absent (the rest `AvatarBody::rest_seat_drop` applies, seeded on
    /// body spawn) until an appearance resolves, or for a sphere-only avatar.
    pub seat_drops: HashMap<AgentKey, f32>,
    /// R22b diagnostic: every agent the session has *ever* surfaced a full avatar
    /// object (`pcode` 47) for, so the `log_avatar_interest`-gated census can
    /// tell a "the simulator never streamed this avatar" case (agent absent here)
    /// from a "we received it but failed to render it" case (agent present here yet
    /// still a coarse sphere). Never pruned — it is a cumulative diagnostic marker.
    pub ever_full_object: HashSet<AgentKey>,
    /// The last coarse (minimap) position `(x, y, z)` seen per coarse-only
    /// agent — `x`/`y` region-local metres (0..255), `z` already in metres
    /// (0..1020, the `u8 × 4` coarse scale). A `z` at the 1020 ceiling is the
    /// simulator's "height unknown / off this region" sentinel; a `0` from some
    /// simulators means the same. Read by the R22b census diagnostic and by the
    /// minimap's dot layer (the unknown-altitude glyph).
    pub coarse_pos: HashMap<AgentKey, (u8, u8, u16)>,
    /// Avatars currently **seated on an object** (their full-object `ObjectUpdate`
    /// carries a non-zero `ParentID`), keyed by agent id — self and others alike
    /// (several avatars share one boat). The value is the seat and the avatar's
    /// pose **in the seat's frame** (the parent-relative wire transform, the
    /// `llSitTarget` offset): `place_seated_avatars` composes it onto the seat's
    /// live world transform each frame so the avatar rides the moving seat, and
    /// `drive_avatar_motion` leaves a
    /// [`Seated`] anchor alone (its motion is the seat's, not region dead-reckoned).
    /// Entries clear the instant an update arrives with `ParentID` zero (a stand).
    pub seated: HashMap<AgentKey, SeatedTarget>,
}

/// Where a seated avatar sits: the seat object and the avatar's pose **relative to
/// the seat**, both taken from the seated avatar's `ObjectUpdate` (whose
/// `ParentID` is the seat and whose `motion` is parent-relative — the reference's
/// `sitOnObject` `rel_pos` / `rel_rot`). Kept in pure Second Life space (no axis
/// swap): the seat entity carries the single SL→Bevy basis change, so composing
/// this onto the seat's world transform places the avatar exactly as a linkset
/// child prim at the same offset would sit. **No root drop** is applied — the
/// reference skips the pelvis/capsule correction entirely while sitting on an
/// object (`LLVOAvatar::updateRootPositionAndRotation` takes the parent transform
/// directly).
#[derive(Debug, Clone, Copy)]
pub struct SeatedTarget {
    /// The seat object's scoped id — resolved to its scene entity through
    /// `ObjectState::entity_by_scoped`
    /// each frame (the seat may stream in after, or independently of, the avatar).
    pub seat: ScopedObjectId,
    /// The avatar's pose in the seat's local frame, as a pure-SL [`Transform`].
    pub offset: Transform,
}

/// Marker on a seated avatar's anchor: its world pose is driven by
/// `place_seated_avatars` from its seat, so the region-space dead-reckoner
/// (`drive_avatar_motion`) must leave it be.
#[derive(Component, Debug, Clone, Copy)]
pub struct Seated;

/// The maximum attachment/linkset depth chased when attributing an object's
/// `IMG_USE_BAKED_*` hide to its avatar, a guard against a malformed parent cycle.
const MAX_ATTACHMENT_DEPTH: usize = 32;

/// The provisional tag text for an agent before its real name resolves: a short
/// leading fragment of its id, so the avatars are distinguishable immediately.
fn provisional_label(agent: AgentKey) -> String {
    agent
        .uuid()
        .simple()
        .to_string()
        .chars()
        .take(PROVISIONAL_ID_CHARS)
        .collect()
}

impl AvatarState {
    /// The tag text for an agent: its display name when resolved, else its
    /// legacy name, else a provisional id fragment until either arrives.
    pub fn label_text(&self, agent: AgentKey) -> String {
        self.names
            .get(&agent)
            .and_then(NameRecord::preferred_name)
            .map_or_else(|| provisional_label(agent), str::to_owned)
    }

    /// Every labelled avatar: `(agent, anchor entity, label entity)` — full
    /// objects first, then the coarse-only spheres (the object path despawns
    /// a coarse twin, but the filter keeps a mid-frame overlap harmless).
    /// The tag-content composer iterates this.
    pub fn labelled_avatars(&self) -> impl Iterator<Item = (AgentKey, Entity, Entity)> + '_ {
        self.objects
            .iter()
            .map(|(agent, entities)| (*agent, entities.anchor, entities.label))
            .chain(
                self.coarse
                    .iter()
                    .filter(|(agent, _)| !self.objects.contains_key(agent))
                    .map(|(agent, entities)| (*agent, entities.anchor, entities.label)),
            )
    }

    /// This agent's resolved legacy name, if one has arrived yet.
    ///
    /// The avatar context menu reads it for actions that carry a name on the wire
    /// (a mute entry names the muted avatar); a `None` means the name has not
    /// resolved, and the caller falls back to a provisional label.
    #[must_use]
    pub fn name_of(&self, agent: AgentKey) -> Option<&str> {
        self.names
            .get(&agent)
            .and_then(|record| record.legacy.as_deref())
    }

    /// Record a name learned from **traffic** rather than a name lookup — an
    /// instant message's sender, a chat-session invitation's inviter, a
    /// server-history line's speaker. The wire carries these names alongside
    /// the message, so the person is nameable without asking.
    ///
    /// Only fills a name that is **not** already known: a lookup reply (and the
    /// display-name cap behind it) is the better-defined answer, and this must
    /// not overwrite it with whatever a message happened to be stamped with.
    pub fn note_legacy_name(&mut self, agent: AgentKey, name: &str) {
        if name.is_empty() {
            return;
        }
        let record = self.name_entry(agent);
        if record.legacy.is_none() {
            record.legacy = Some(name.to_owned());
        }
    }

    /// This agent's full name record, if any of its sources answered yet —
    /// the tag-content composer reads the display name / username / default
    /// flag from it.
    #[must_use]
    pub fn name_record(&self, agent: AgentKey) -> Option<&NameRecord> {
        self.names.get(&agent)
    }

    /// The name to **show** for this agent — the user's alias, else the display
    /// name, else the legacy name — or `None` while nothing has resolved.
    ///
    /// This is the accessor a drawn name wants; [`Self::name_of`] is the grid's
    /// legacy answer, which is what a wire action (a mute entry naming the muted
    /// avatar) has to carry.
    pub fn shown_name_of(&self, agent: AgentKey) -> Option<&str> {
        self.names.get(&agent).and_then(NameRecord::preferred_name)
    }

    /// Replace the user's name aliases, re-folding every cached record so a
    /// pseudonym given (or cleared) now shows (or stops showing) everywhere at
    /// once. The one way an alias reaches the name cache.
    pub fn set_name_aliases(&mut self, aliases: HashMap<AgentKey, NameAlias>) {
        for (agent, record) in &mut self.names {
            record.alias = aliases.get(agent).cloned();
        }
        self.name_aliases = aliases;
    }

    /// The record for `agent`, created if this is the first thing known about
    /// them, with the user's alias folded in — the one way an ingest path takes
    /// a record, so a name that arrives after the alias was given is aliased
    /// too.
    fn name_entry(&mut self, agent: AgentKey) -> &mut NameRecord {
        let alias = self.name_aliases.get(&agent).cloned();
        let record = self.names.entry(agent).or_default();
        if record.alias != alias {
            record.alias = alias;
        }
        record
    }

    /// This agent's group title (from its avatar object's NameValue `Title`),
    /// if it has one.
    pub fn title_of(&self, agent: AgentKey) -> Option<&str> {
        self.titles.get(&agent).map(String::as_str)
    }

    /// The agent whose avatar object carries the region-scoped id `scoped`, if
    /// this viewer tracks one — the reverse of the object stream's view of an
    /// avatar. An attachment names its wearer only by that scoped id, so this is
    /// how a worn linkset is attributed to the avatar it is worn on
    /// (`crate::avatar_complexity`).
    #[must_use]
    pub fn agent_of_scoped(&self, scoped: ScopedObjectId) -> Option<AgentKey> {
        self.by_scoped.get(&scoped).copied()
    }

    /// Mark this avatar's body-region materials for (re)assignment by
    /// `assign_avatar_bake_materials` — how a pass that borrowed those
    /// materials hands them back. The jellydoll render
    /// (`crate::avatar_complexity`) paints a limited avatar's body flat and
    /// calls this when it stops, so the real bakes are draped again.
    pub fn mark_bake_dirty(&mut self, agent: AgentKey) {
        let _fresh = self.bake_dirty.insert(agent);
    }

    /// How many base-body regions this avatar has a **visible** bake in, from its
    /// latest appearance — the count the render-cost model charges its per-region
    /// body cost for. Zero before an appearance arrives (or for a sphere-only
    /// avatar); a region baked invisible by a worn system alpha layer was already
    /// filtered out when the appearance was ingested, exactly as the reference
    /// skips an `IMG_INVISIBLE` slot.
    pub fn visible_bake_count(&self, agent: AgentKey) -> usize {
        self.baked_textures.get(&agent).map_or(0, HashMap::len)
    }

    /// Every avatar this viewer currently knows in-world, with the anchor
    /// entity whose transform places it — full objects first, then the
    /// coarse-only dots. The avatar picker's Near Me tab reads this.
    #[must_use]
    pub fn known_agents(&self) -> Vec<(AgentKey, Entity)> {
        let mut agents: Vec<(AgentKey, Entity)> = self
            .objects
            .iter()
            .map(|(agent, entities)| (*agent, entities.anchor))
            .collect();
        for (agent, entities) in &self.coarse {
            if !self.objects.contains_key(agent) {
                agents.push((*agent, entities.anchor));
            }
        }
        agents
    }

    /// Every nearby avatar as the map surfaces (minimap, radar) consume it:
    /// full-object avatars first (precise positions from their anchor
    /// transforms), then the coarse-only dots, deduplicated by agent — the
    /// reference's `LLWorld::getAvatars` merge. A coarse-only entry carries its
    /// last coarse altitude so the consumer can detect the "altitude unknown"
    /// sentinel (`crate::minimap_math::coarse_altitude_unknown`).
    #[must_use]
    pub fn map_avatars(&self) -> Vec<MapAvatar> {
        let mut avatars: Vec<MapAvatar> = self
            .objects
            .iter()
            .map(|(agent, entities)| MapAvatar {
                agent: *agent,
                anchor: entities.anchor,
                coarse_z: None,
            })
            .collect();
        for (agent, entities) in &self.coarse {
            if !self.objects.contains_key(agent) {
                avatars.push(MapAvatar {
                    agent: *agent,
                    anchor: entities.anchor,
                    coarse_z: Some(f32::from(
                        self.coarse_pos.get(agent).map_or(0, |&(_, _, z)| z),
                    )),
                });
            }
        }
        avatars
    }

    /// The anchor entity of an agent's in-world presence (a full object
    /// preferred over a coarse dot), if any.
    #[must_use]
    pub fn root_entity_of(&self, agent: AgentKey) -> Option<Entity> {
        self.objects
            .get(&agent)
            .or_else(|| self.coarse.get(&agent))
            .map(|entities| entities.anchor)
    }

    /// Seed the name cache and title map from an avatar object's NameValue
    /// pairs (`FirstName` / `LastName` / `Title` — the classic mechanism; the
    /// simulator sends them with every avatar `ObjectUpdate`, so the legacy
    /// name and group title arrive *with the object*, zero round trips).
    /// Never clobbers a legacy name another source already resolved, and only
    /// touches the title when a `Title` pair is actually present (a present
    /// but empty title means "title taken off").
    pub fn seed_from_name_values(&mut self, agent: AgentKey, object: &Object) {
        self.seed_name_fields(
            agent,
            object.name_value_data("FirstName"),
            object.name_value_data("LastName"),
            object.name_value_data("Title"),
        );
    }

    /// The merge rules of [`Self::seed_from_name_values`], on the extracted
    /// NameValue fields (split out so they are unit-testable without
    /// constructing a full [`Object`]).
    pub fn seed_name_fields(
        &mut self,
        agent: AgentKey,
        first: Option<String>,
        last: Option<String>,
        title: Option<String>,
    ) {
        if let Some(first) = first {
            let legacy = match last {
                Some(last) if !last.is_empty() && !last.eq_ignore_ascii_case("Resident") => {
                    format!("{first} {last}")
                }
                _ => first,
            };
            if !legacy.is_empty() {
                let record = self.name_entry(agent);
                if record.legacy.is_none() {
                    record.legacy = Some(legacy);
                }
            }
        }
        if let Some(title) = title {
            // The reference strips control characters from titles.
            let cleaned: String = title.chars().filter(|c| !c.is_control()).collect();
            if cleaned.is_empty() {
                self.titles.remove(&agent);
            } else {
                self.titles.insert(agent, cleaned);
            }
        }
    }

    /// Fold one display-name record from the `GetDisplayNames` cap (or a
    /// pushed `DisplayNameUpdate`) into the cache. A `missing` placeholder
    /// (the grid could not resolve the id) changes nothing — the legacy
    /// fallback stays. (The tag refreshes via the content composer.)
    pub fn set_display_name(&mut self, resolved: &DisplayName) {
        if !self.merge_display_name_record(resolved) {
            return;
        }
        debug!(
            "resolved display name {} = {:?} (@{})",
            resolved.id, resolved.display_name, resolved.username
        );
    }

    /// Fold one non-`missing` display-name record into the name cache;
    /// returns whether anything was (potentially) updated. Split from
    /// [`Self::set_display_name`] so the merge rules are unit-testable
    /// without an ECS world.
    pub fn merge_display_name_record(&mut self, resolved: &DisplayName) -> bool {
        if resolved.missing {
            return false;
        }
        let record = self.name_entry(resolved.id);
        record.legacy = Some(resolved.legacy_name());
        record.username = Some(resolved.username.clone());
        record.display_name = Some(resolved.display_name.clone());
        record.is_display_name_default = resolved.is_display_name_default;
        true
    }

    /// Queue a name request for `agent` once — a no-op if it is already in
    /// flight or answered. The actual wire traffic goes out batched, once per
    /// frame, in `flush_name_requests`. `pub(crate)` for the build
    /// floater's General tab, which resolves a selected object's creator /
    /// owner through the same cache.
    pub fn request_name(&mut self, agent: AgentKey) {
        if !self.requested.insert(agent) {
            return;
        }
        self.pending_name_requests.insert(agent);
    }

    /// Despawn the full-object avatar tracked under `scoped` because it was
    /// derendered — the scoped-id counterpart of `derender_agent`,
    /// for the suppression index (which works in region-scoped ids). A no-op
    /// when `scoped` is not an avatar.
    pub fn derender_scoped(&mut self, scoped: ScopedObjectId, commands: &mut Commands) {
        self.remove_object(scoped, commands);
    }

    /// Despawn the placeholder of the full-object avatar that left the scene under
    /// `scoped`, if one is tracked.
    pub fn remove_object(&mut self, scoped: ScopedObjectId, commands: &mut Commands) {
        let Some(agent) = self.by_scoped.remove(&scoped) else {
            return;
        };
        if let Some(entities) = self.objects.remove(&agent) {
            despawn_avatar(entities, commands);
        }
        // The body's attachment-point nodes and head socket are despawned with
        // its anchor; drop the stores so a later attachment can no longer resolve
        // them (P16.2). The recorded joint overrides go too, so a re-spawn
        // rebuilds them from the meshes that re-bind (R1).
        let _dropped_nodes = self.attachment_nodes.remove(&agent);
        let _dropped_head = self.head_sockets.remove(&agent);
        let _dropped_deform = self.deformations.remove(&agent);
        let _dropped_volumes = self.volume_deformations.remove(&agent);
        let _dropped_physics = self.body_physics.remove(&agent);
        let _dropped_seat = self.seated.remove(&agent);
        let _dropped_seat_drop = self.seat_drops.remove(&agent);
        self.clear_joint_overrides(agent);
    }

    /// Whether `agent`'s avatar is currently seated on an object — its latest
    /// full-object update carried a non-zero `ParentID`. The camera reads this to
    /// take a seated own avatar's world pose from its (seat-driven) global
    /// transform rather than its region-space motion.
    #[must_use]
    pub fn is_seated(&self, agent: AgentKey) -> bool {
        self.seated.contains_key(&agent)
    }

    /// Unseat any avatars seated on the object `seat` that was just removed
    /// (`ObjectRemoved`) — drop their seated state and the [`Seated`] tag so the
    /// dead-reckoner resumes owning their anchor. Their anchor stays at its last
    /// seat-driven world pose until the simulator's own stand / motion update lands.
    ///
    /// The simulator normally unseats a rider before (or as) it kills the seat, so
    /// the avatar's own `ObjectUpdate` (with `ParentID` zero) already cleared the
    /// seat; this covers the seat vanishing — deleted, or culled from the interest
    /// list — *without* or *before* that update, so an avatar is never left frozen,
    /// invisibly parented, to a seat that no longer exists. Returns the agents it
    /// unseated (empty when the removed object was not anyone's seat).
    pub fn unseat_from_seat(&mut self, seat: ScopedObjectId, commands: &mut Commands) {
        let riders: Vec<AgentKey> = self
            .seated
            .iter()
            .filter(|(_agent, target)| target.seat == seat)
            .map(|(agent, _target)| *agent)
            .collect();
        for agent in riders {
            let _unseated = self.seated.remove(&agent);
            if let Some(entities) = self.objects.get(&agent) {
                commands.entity(entities.anchor).remove::<Seated>();
            }
        }
    }

    /// Each seated avatar's `(anchor entity, seat scoped id, seat-relative pose,
    /// seat drop)`, for `place_seated_avatars` to drive the anchor from its seat's
    /// live world transform. The seat drop is the pelvis's height above the body
    /// root (zero for a sphere), applied so the hips land on the sit target. Skips
    /// any avatar whose anchor is not (yet) a tracked full object.
    pub fn seated_placements(
        &self,
    ) -> impl Iterator<Item = (Entity, ScopedObjectId, Transform, f32)> + '_ {
        self.seated.iter().filter_map(|(agent, target)| {
            let anchor = self.objects.get(agent)?.anchor;
            let seat_drop = self.seat_drops.get(agent).copied().unwrap_or(0.0);
            Some((anchor, target.seat, target.offset, seat_drop))
        })
    }

    /// The agent whose avatar is tracked under the scoped object id `avatar_scoped`
    /// — the wearer of an attachment whose parent is that object. `None` if no
    /// avatar object with that scoped id is tracked (yet).
    ///
    /// The HUD routing (P35.1) needs it to tell the agent's **own** HUD attachments
    /// (which go to the screen-space HUD layer) from another avatar's (which are
    /// hidden: the reference viewer gives a non-self avatar no HUD joints at all).
    #[must_use]
    pub fn agent_of(&self, avatar_scoped: ScopedObjectId) -> Option<AgentKey> {
        self.by_scoped.get(&avatar_scoped).copied()
    }

    /// The attachment-point node entity a worn attachment parents to (P16.2): the
    /// node for raw attachment-point `point_id` on the rigged body of the avatar
    /// tracked under `avatar_scoped`, carrying the fixed `avatar_lad.xml` offset
    /// from its skeleton joint. `None` if that avatar is not a tracked full-object
    /// rigged body yet, or the point has no body joint (a HUD point) — in which
    /// case the caller holds the attachment pending and retries.
    #[must_use]
    pub fn attachment_point_entity(
        &self,
        avatar_scoped: ScopedObjectId,
        point_id: u8,
    ) -> Option<Entity> {
        let agent = self.by_scoped.get(&avatar_scoped)?;
        self.attachment_nodes.get(agent)?.get(&point_id).copied()
    }

    /// The rigged-body root (anchor) entity of `agent`'s avatar (P17.2): the entity
    /// a worn rigged mesh's skinned submeshes are parented to so they despawn with
    /// the avatar and inherit its visibility. `None` if that avatar is not a tracked
    /// full-object avatar yet.
    /// The name-tag (label) entity of `agent`, if it is currently rendered.
    #[must_use]
    pub fn label_of(&self, agent: AgentKey) -> Option<Entity> {
        self.objects
            .get(&agent)
            .or_else(|| self.coarse.get(&agent))
            .map(|entities| entities.label)
    }

    /// The rigged-body root (anchor) entity of `agent`'s full-object avatar,
    /// if one is rendered.
    #[must_use]
    pub fn body_root_of(&self, agent: AgentKey) -> Option<Entity> {
        self.objects.get(&agent).map(|entities| entities.anchor)
    }

    /// Whether `agent` has a spawned rigged body (Phase 4: keyed on the presence
    /// of the head socket, spawned with the body — the joint entities the old
    /// `joint_entities_of` returned are gone). `false` for a sphere-only
    /// (no-`--viewer-assets`) avatar or one not spawned yet.
    #[must_use]
    pub fn is_rigged(&self, agent: AgentKey) -> bool {
        self.head_sockets.contains_key(&agent)
    }

    /// The per-point attachment-point node entities of `agent`'s rigged avatar
    /// as `(raw attachment-point id, node entity)` pairs — the socket writer
    /// (§5.4) places each worn node at its joint's posed world composed with the
    /// point's fixed `avatar_lad.xml` offset each frame. Empty for an avatar
    /// with no rigged body.
    pub fn attachment_nodes_of(&self, agent: AgentKey) -> impl Iterator<Item = (u8, Entity)> + '_ {
        self.attachment_nodes
            .get(&agent)
            .into_iter()
            .flat_map(|nodes| nodes.iter().map(|(&point_id, &entity)| (point_id, entity)))
    }

    /// The camera's head-focus socket entity of `agent`'s rigged avatar
    /// (Phase 4 §5.4): a root child the socket writer places at the posed
    /// `mHead` joint each frame, so the camera reads the animated head without a
    /// head joint entity. `None` for a sphere-only avatar or before the body
    /// spawns.
    #[must_use]
    pub fn head_socket_of(&self, agent: AgentKey) -> Option<Entity> {
        self.head_sockets.get(&agent).copied()
    }

    /// The resolved skeletal deformations the animation driver (P18.3) folds a
    /// playing motion into when recomputing each joint's world matrix, as last
    /// shaped by `apply_avatar_appearance`. `None` for an avatar with no rigged
    /// body, or before its first appearance.
    #[must_use]
    pub fn deformations(&self, agent: AgentKey) -> Option<&SkeletalDeformations> {
        self.deformations.get(&agent)
    }

    /// The resolved collision-volume displacements (P34.3) the animation driver
    /// folds into the same recurrence, as last shaped by
    /// `apply_avatar_appearance`. An avatar whose shape displaces no volume has
    /// no entry, which is the same as the (empty) default.
    #[must_use]
    pub fn volume_deformations(&self, agent: AgentKey) -> Option<&VolumeDeformations> {
        self.volume_deformations.get(&agent)
    }

    /// Every avatar with a spawned rigged body (Phase 4: keyed on the head
    /// socket, since the joint entities are gone). The pose driver publishes each
    /// one's root + adjuster corrections and places its sockets every frame; the
    /// GPU samples, blends and FK-poses the skinning in place.
    #[must_use]
    pub fn rigged_agents(&self) -> Vec<AgentKey> {
        self.head_sockets.keys().copied().collect()
    }

    /// Note that `agent` wears the rigged mesh asset `mesh` (for the avatar-state
    /// dump). Idempotent; forgotten with the avatar on despawn.
    pub fn record_worn_rigged_mesh(&mut self, agent: AgentKey, mesh: Uuid) {
        let _new = self
            .worn_rigged_meshes
            .entry(agent)
            .or_default()
            .insert(mesh);
    }

    /// The anchor entity (rigged-body root, or placeholder sphere) of `agent`'s
    /// full-object avatar, if one is tracked — the world pose the replay test rig
    /// (an orbiting light, a reflection probe) centres itself on.
    #[must_use]
    pub fn anchor_of(&self, agent: AgentKey) -> Option<Entity> {
        self.objects.get(&agent).map(|entities| entities.anchor)
    }

    /// Record the joint position overrides that worn rigged `mesh` imposes on
    /// `agent`'s skeleton (R1), replacing any previous contribution from that mesh
    /// (a rebind is idempotent). Flags the avatar for a skeleton re-deform **only
    /// when the contribution actually changed**, so re-binding identical rig parts
    /// (a mesh body's many same-rigged pieces) does not thrash the appearance pass.
    pub fn record_joint_overrides(
        &mut self,
        agent: AgentKey,
        mesh: Uuid,
        overrides: JointOverrides,
    ) {
        let per_mesh = self.joint_overrides.entry(agent).or_default();
        if per_mesh.get(&mesh) == Some(&overrides) {
            return;
        }
        if overrides.is_empty() {
            // A mesh that used to override but no longer does: drop its entry so the
            // rebuilt effective set no longer carries it.
            if per_mesh.remove(&mesh).is_none() {
                return;
            }
        } else {
            let _prev = per_mesh.insert(mesh, overrides);
        }
        self.appearance_dirty.insert(agent);
    }

    /// The body-physics configuration ingested from `agent`'s latest appearance
    /// (P34.1), or `None` before one arrived. Every motion it holds is ready to
    /// simulate: a motion whose `Max_Effect` is zero is present but
    /// [inactive](sl_client_bevy::PhysicsSettings::is_active).
    #[must_use]
    pub fn body_physics(&self, agent: AgentKey) -> Option<&BodyPhysics> {
        self.body_physics.get(&agent)
    }

    /// The current pose-inputs generation (see the field doc): the pose gate
    /// stores it per avatar and re-evaluates when it moved.
    #[must_use]
    pub const fn pose_inputs_generation(&self) -> u64 {
        self.pose_inputs_generation
    }

    /// Record that an input the skeleton pose fold consumes (deformations, volume
    /// deformations, joint overrides, body physics) was (re)applied. Over-bumping
    /// is harmless — an extra bump costs one frame of full re-evaluation.
    pub const fn bump_pose_inputs(&mut self) {
        self.pose_inputs_generation = self.pose_inputs_generation.wrapping_add(1);
    }

    /// The effective joint position overrides for `agent` (R1): the per-joint winner
    /// across every worn rigged mesh, resolved to the **highest mesh id** on a
    /// conflict (the reference viewer's `findActiveOverride`) with the scale lock
    /// sticky. `None` when the avatar wears no position-carrying rig.
    #[must_use]
    pub fn effective_joint_overrides(&self, agent: AgentKey) -> Option<JointOverrides> {
        let per_mesh = self.joint_overrides.get(&agent)?;
        if per_mesh.is_empty() {
            return None;
        }
        // Merge in ascending mesh-id order so the highest mesh id wins each joint.
        let mut meshes: Vec<(&Uuid, &JointOverrides)> = per_mesh.iter().collect();
        meshes.sort_by_key(|(mesh, _)| **mesh);
        let mut effective = JointOverrides::default();
        for (_mesh, overrides) in meshes {
            effective.merge(overrides);
        }
        Some(effective)
    }

    /// Forget every joint position override recorded for `agent` (R1) — e.g. when
    /// the avatar despawns, so a re-spawn rebuilds them from scratch.
    pub(crate) fn clear_joint_overrides(&mut self, agent: AgentKey) {
        let _prev = self.joint_overrides.remove(&agent);
        let _worn = self.worn_rigged_meshes.remove(&agent);
        self.bump_pose_inputs();
    }

    /// The agent whose avatar a worn object `scoped` hangs off — chasing parent
    /// links up to the tracked avatar root, so a rigged mesh that is a *child link*
    /// of a multi-prim attachment linkset (a mesh body, whose parts parent to the
    /// linkset root prim, not the avatar) still resolves to its wearer (P17.2).
    /// `None` if the chain does not reach an avatar.
    #[must_use]
    pub fn wearer_of(&self, scoped: ScopedObjectId) -> Option<AgentKey> {
        self.avatar_root_of(scoped)
    }

    /// Despawn every **other** avatar (full objects and coarse dots) and forget
    /// their per-agent state — the scene-mirror purge a **distant** teleport
    /// needs, since the session cleared its object cache with no per-object
    /// `KillObject` to drive the incremental removal path
    /// ([`Event::RegionChanged`](sl_client_bevy::SlSessionEvent)'s `world_reset`).
    ///
    /// The agent's **own** avatar (`own`) is kept — its body, skeleton, appearance
    /// and worn state all cross with the agent on a teleport, so despawning it
    /// would flash the self view and force an appearance / bake refetch. Its
    /// visible body simply re-anchors when the destination re-streams its
    /// (agent-keyed) full object. The scoped-id-keyed bookkeeping is dropped
    /// wholesale (the source region's local-id space is gone) and rebuilt as the
    /// destination streams. Also drops the origin anchor so `recenter_avatars`
    /// re-anchors on the destination without a spurious re-base shift.
    pub fn purge(&mut self, own: Option<AgentKey>, commands: &mut Commands) {
        let keep = |agent: &AgentKey| own == Some(*agent);
        // Despawn every non-own avatar's entities (full objects + coarse dots).
        let others: Vec<AgentKey> = self
            .objects
            .keys()
            .chain(self.coarse.keys())
            .copied()
            .filter(|agent| !keep(agent))
            .collect();
        for agent in others {
            if let Some(entities) = self.objects.remove(&agent) {
                despawn_avatar(entities, commands);
            }
            if let Some(entities) = self.coarse.remove(&agent) {
                despawn_avatar(entities, commands);
            }
        }
        // Retain only the own agent on the per-agent bookkeeping.
        //
        // **Names are not scene state and are deliberately kept.** A name is
        // knowledge about a person, not about a presence: most of the names
        // this viewer shows are for avatars nowhere near it — group members and
        // group chat, an object's or parcel's owner and creator, an inventory
        // item's creator, an open conversation's peer. Dropping the cache
        // because the *region* changed would re-ask the grid for names it
        // already knew, and blank every one of those surfaces until the replies
        // land. It cannot grow enough to matter over a session; if it ever
        // did, the bound would be least-recently-used, not "is standing near
        // me". The request bookkeeping stays with it, so a name already
        // resolved is never re-requested.
        self.coarse_region.retain(|agent, _| keep(agent));
        self.coarse_pos.retain(|agent, _| keep(agent));
        self.attachment_nodes.retain(|agent, _| keep(agent));
        self.head_sockets.retain(|agent, _| keep(agent));
        self.titles.retain(|agent, _| keep(agent));
        self.appearances.retain(|agent, _| keep(agent));
        self.appearance_dirty.retain(keep);
        self.appearance_pending.retain(|agent, _| keep(agent));
        self.joint_overrides.retain(|agent, _| keep(agent));
        self.worn_rigged_meshes.retain(|agent, _| keep(agent));
        self.skirt_visible.retain(|agent, _| keep(agent));
        self.body_physics.retain(|agent, _| keep(agent));
        self.baked_textures.retain(|agent, _| keep(agent));
        self.invisible_regions.retain(|agent, _| keep(agent));
        self.baked_cof_version.retain(|agent, _| keep(agent));
        self.bake_dirty.retain(keep);
        self.deformations.retain(|agent, _| keep(agent));
        self.volume_deformations.retain(|agent, _| keep(agent));
        self.root_drops.retain(|agent, _| keep(agent));
        self.seat_drops.retain(|agent, _| keep(agent));
        self.ever_full_object.retain(keep);
        self.seated.retain(|agent, _| keep(agent));
        // The source region's local-id space is gone; drop every scoped-id-keyed
        // entry (own included — its ids are reassigned when the destination
        // re-streams it). `by_scoped` is repopulated by `apply_object`, the parent
        // / hide maps by `track_object`.
        self.by_scoped.clear();
        self.object_parents.clear();
        self.baked_hides.clear();
        self.scanned_objects.clear();
        self.origin = None;
    }

    /// Record a resolved legacy name. (The tag itself refreshes via the
    /// content composer, which recomposes whenever this state changes.)
    pub fn set_name(&mut self, name: &AvatarName) {
        let agent = name.id;
        let resolved = name.legacy_name();
        self.name_entry(agent).legacy = Some(resolved.clone());
        debug!("resolved avatar name {agent} = {resolved:?}");
    }

    /// Record the parenting of an in-world object and, once, scan its texture
    /// entry for the `IMG_USE_BAKED_*` sentinels a worn attachment uses to hide a
    /// base-avatar region. Called for every object; a *root* object (no parent)
    /// can never be an attachment, so it is ignored.
    pub fn track_object(&mut self, object: &Object) {
        if object.parent_id.get() == 0 {
            return;
        }
        let scoped = object.scoped_id();
        self.object_parents
            .insert(scoped, object.scoped_parent_id());
        // Decode + scan a given object's texture entry only once (attachments do
        // not change their baked-body sentinels under normal wear).
        if self.scanned_objects.insert(scoped) {
            let slots = used_baked_slots(&object.texture_entry);
            if !slots.is_empty() {
                self.baked_hides.insert(scoped, slots);
            }
        }
    }

    /// Forget a departed object's attachment bookkeeping.
    pub fn forget_object(&mut self, scoped: ScopedObjectId) {
        self.object_parents.remove(&scoped);
        self.baked_hides.remove(&scoped);
        self.scanned_objects.remove(&scoped);
    }

    /// The agent whose avatar `scoped` hangs off, by chasing parent links up to a
    /// tracked avatar root; `None` if the chain does not reach an avatar (an
    /// ordinary in-world linkset) or is malformed.
    fn avatar_root_of(&self, scoped: ScopedObjectId) -> Option<AgentKey> {
        let mut current = scoped;
        for _ in 0..MAX_ATTACHMENT_DEPTH {
            if let Some(&agent) = self.by_scoped.get(&current) {
                return Some(agent);
            }
            match self.object_parents.get(&current) {
                Some(&parent) => current = parent,
                None => return None,
            }
        }
        None
    }

    /// Diagnostic form of `avatar_root_of`: `Ok(agent)` when the parent chain
    /// reaches a recognised avatar, else `Err((terminus, hops))` — the object the
    /// walk stopped at (a root with no recorded parent, or the last hop when the
    /// depth cap is hit) and how many hops it took. Lets a stuck rigged
    /// attachment's `wearer not resolved` failure be classified against the object
    /// state: a *tracked in-world* terminus means it is genuinely not worn (an
    /// in-world rigged mesh), while an *untracked* terminus means the wearer /
    /// linkset-root object never arrived (a parenting / ordering gap).
    ///
    /// # Errors
    ///
    /// `Err((terminus, hops))` when the walk reaches no avatar: `terminus` is the
    /// object it stopped at — a root with no recorded parent, or the last hop
    /// when the depth cap is reached — and `hops` how many parent links it
    /// followed to get there.
    pub fn avatar_root_walk(
        &self,
        scoped: ScopedObjectId,
    ) -> Result<AgentKey, (ScopedObjectId, usize)> {
        let mut current = scoped;
        for hops in 0..MAX_ATTACHMENT_DEPTH {
            if let Some(&agent) = self.by_scoped.get(&current) {
                return Ok(agent);
            }
            match self.object_parents.get(&current) {
                Some(&parent) => current = parent,
                None => return Err((current, hops)),
            }
        }
        Err((current, MAX_ATTACHMENT_DEPTH))
    }

    /// The set of baked slots to hide for each avatar: every tracked attachment
    /// whose texture entry carries `IMG_USE_BAKED_*` sentinels is attributed to
    /// its avatar (by chasing its chain), and its replaced slots unioned in.
    #[must_use]
    pub fn hidden_slots_per_agent(&self) -> HashMap<AgentKey, HashSet<usize>> {
        let mut hidden: HashMap<AgentKey, HashSet<usize>> = HashMap::new();
        for (&scoped, slots) in &self.baked_hides {
            if let Some(agent) = self.avatar_root_of(scoped) {
                hidden
                    .entry(agent)
                    .or_default()
                    .extend(slots.iter().copied());
            }
        }
        hidden
    }
}

/// Despawn both entities of an avatar (its anchor — sphere or body root, whose
/// sub-hierarchy goes with it — and its name tag).
pub fn despawn_avatar(entities: AvatarEntities, commands: &mut Commands) {
    commands.entity(entities.anchor).try_despawn();
    commands.entity(entities.label).try_despawn();
}

/// When an avatar was first and last marked appearance-dirty, in
/// [`Time::elapsed_secs_f64`] seconds (see
/// [`AvatarState::appearance_pending`]).
#[derive(Debug, Clone, Copy)]
pub struct AppearanceDirtyStamps {
    /// When the avatar entered the pending set (unchanged by re-marks).
    pub first: f64,
    /// When the avatar was most recently marked (each re-mark refreshes it).
    pub last: f64,
}

/// Scan a raw texture-entry blob for the `IMG_USE_BAKED_*` sentinels and return
/// the (sorted, de-duplicated) baked slots it signals should be replaced — empty
/// for an ordinary object.
fn used_baked_slots(texture_entry: &[u8]) -> Vec<usize> {
    let entry = decode_texture_entry(texture_entry, MAX_FACES);
    let mut slots: Vec<usize> = entry
        .faces
        .iter()
        .filter_map(|face| avatar_texture::use_baked_slot(face.texture_id))
        .collect();
    slots.sort_unstable();
    slots.dedup();
    slots
}

// ---------------------------------------------------------------------------
// Ordering phases
// ---------------------------------------------------------------------------

/// The points in a frame that parts of the world order themselves against.
///
/// A system that must run once the objects have been folded in, or once the
/// avatars have, would otherwise have to name the system that does it -- and
/// naming a system across a boundary is a dependency on the code that produces
/// the world, not on the world it produced. These sets are the vocabulary for
/// that ordering, so the constraint can be stated without the reference.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WorldPhase {
    /// This frame's `ObjectUpdate` batch has been folded into the object store
    /// and its entities spawned, moved or despawned.
    ObjectsUpdated,
    /// This frame's avatar updates -- full objects and coarse locations alike --
    /// have been folded into the avatar store.
    AvatarsUpdated,
    /// The third-person camera has consumed this frame's orbit input.
    CameraOrbited,
}

/// The number of ground ("detail") textures a region blends between.
pub const DETAIL_COUNT: usize = 4;

/// A patch's key: its region plus grid position within that region.
pub type PatchKey = (RegionHandle, u32, u32);

/// Resolve the land height at region-local `(x, y)` from a patch map (the live
/// [`TerrainState::raw_patches`] or the retained [`TerrainState::land_cache`]).
/// `None` when no land patch in that map covers the point.
fn land_height_in(
    patches: &HashMap<PatchKey, TerrainPatch>,
    region: RegionHandle,
    x: f32,
    y: f32,
) -> Option<f32> {
    for (&(patch_region, _, _), patch) in patches {
        if patch_region != region || !patch.layer.is_land() {
            continue;
        }
        let span = f32::from(u16::try_from(patch.size).unwrap_or(u16::MAX));
        let x0 = f32::from(u16::try_from(patch.patch_x).unwrap_or(u16::MAX)) * span;
        let y0 = f32::from(u16::try_from(patch.patch_y).unwrap_or(u16::MAX)) * span;
        if x < x0 || y < y0 || x >= x0 + span || y >= y0 + span {
            continue;
        }
        // The floored offset into the patch, in `0..size`. The subtraction is
        // non-negative (guarded above) and below `size`, so the truncating cast
        // is exact.
        #[expect(
            clippy::as_conversions,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "the offset is in 0.0..size, so it fits u32 exactly"
        )]
        let (cell_x, cell_y) = ((x - x0).floor() as u32, (y - y0).floor() as u32);
        let last = patch.size.saturating_sub(1);
        return patch.value(cell_x.min(last), cell_y.min(last));
    }
    None
}

/// Per-region terrain-compositing state: the elevation bands and the
/// detail-texture keys requested for the region's shared splat material. The
/// material handle itself is machinery and lives beside the systems that build
/// it, keyed by the same region handle.
#[derive(Debug, Default)]
pub struct RegionTerrain {
    /// The region's terrain-compositing parameters, once its `RegionHandshake`
    /// has been seen; `None` until then (patches render with flat weights).
    pub composition: Option<TerrainComposition>,
    /// The texture key requested for each detail slot (`None` for a nil slot),
    /// used to route an arriving texture to the right material slot.
    pub detail_keys: [Option<TextureKey>; DETAIL_COUNT],
    /// Whether this region's detail textures have been requested yet.
    pub requested: bool,
}

/// Viewer-side terrain bookkeeping across the home region and its neighbours.
#[derive(Debug, Resource, Default)]
pub struct TerrainState {
    /// The scene origin: the region whose south-west corner is Bevy `(0, 0)`.
    /// Follows the root region so coordinates stay small near the camera.
    pub origin: Option<RegionHandle>,
    /// Per-region compositing state, keyed by region handle.
    pub regions: HashMap<RegionHandle, RegionTerrain>,
    /// The rendered entity for each patch; a repeat patch replaces its mesh.
    pub patches: HashMap<PatchKey, Entity>,
    /// The most recent raw patch for each key, kept so a patch's mesh can be
    /// rebuilt (with real weights, or when a neighbour arrives) after the fact.
    pub raw_patches: HashMap<PatchKey, TerrainPatch>,
    /// The **last known land patch** for each key, retained across a region
    /// teardown / rebuild (and, with the disk layer, across sessions) so
    /// [`Self::land_height`] can still answer while the live [`Self::raw_patches`]
    /// are gone — a region's ground height is stable, so a stale cached value is a
    /// far better ground-floor answer than `None`. This is what keeps the avatar
    /// ground floor (`physics.rs`) working through the login window and the
    /// region-disappears-then-rebuilds flicker, so the avatar does not fall through
    /// the terrain while its patches are absent. Land layers only.
    pub land_cache: HashMap<PatchKey, TerrainPatch>,
    /// A monotonically increasing counter bumped whenever height or compositing
    /// data changes (a patch arrives, a handshake's composition lands), so a
    /// derived consumer (the minimap's terrain backdrop) can cheaply notice
    /// staleness without hashing the patch maps.
    map_revision: u64,
    /// Per-region version of [`map_revision`](Self::map_revision), bumped only
    /// for the region whose data actually changed, so a per-region consumer (the
    /// parcel-border bands, which ground-follow) can rebuild only the terraformed
    /// / newly-streamed region instead of all of them.
    region_revisions: HashMap<RegionHandle, u64>,
}

impl TerrainState {
    /// The scene origin region (whose south-west corner is Bevy `(0, 0)`), or
    /// `None` before the first terrain patch arrives.
    #[must_use]
    pub const fn origin(&self) -> Option<RegionHandle> {
        self.origin
    }

    /// Despawn every rendered land patch and forget all per-region terrain state —
    /// the terrain half of the distant-teleport scene purge
    /// ([`Event::RegionChanged`](sl_client_bevy::SlSessionEvent)'s `world_reset`).
    /// The destination streams its terrain fresh. Drops the origin so the
    /// recentring pass re-anchors on the destination without a spurious camera
    /// shift. The region materials and the decoded-detail-texture cache are the
    /// world layer's to purge (a texture shared with the destination need not be
    /// refetched).
    pub fn purge(&mut self, commands: &mut Commands) {
        for (_key, entity) in self.patches.drain() {
            commands.entity(entity).try_despawn();
        }
        self.regions.clear();
        self.raw_patches.clear();
        self.map_revision = self.map_revision.wrapping_add(1);
        // The purged regions are gone; a re-streamed region restarts at revision
        // 1, which a stale per-region stamp (its pre-purge revision) will not
        // match, so its bands rebuild.
        self.region_revisions.clear();
        self.origin = None;
    }

    /// The terrain-data revision — bumped on every ingested patch and learned
    /// composition, so a derived map texture knows when to rebuild.
    #[must_use]
    pub const fn map_revision(&self) -> u64 {
        self.map_revision
    }

    /// This region's own terrain revision, bumped only when *its* height or
    /// compositing data changes; `0` before any of it has arrived.
    #[must_use]
    pub fn region_revision(&self, region: RegionHandle) -> u64 {
        self.region_revisions.get(&region).copied().unwrap_or(0)
    }

    /// Bump both the global [`map_revision`](Self::map_revision) and `region`'s
    /// per-region revision — call wherever `region`'s height / compositing data
    /// changes.
    pub fn bump_revision(&mut self, region: RegionHandle) {
        self.map_revision = self.map_revision.wrapping_add(1);
        let revision = self.region_revisions.entry(region).or_insert(0);
        *revision = revision.wrapping_add(1);
    }

    /// Every decoded land patch of `region`, for compositing a top-down map.
    pub fn land_patches_of(&self, region: RegionHandle) -> impl Iterator<Item = &TerrainPatch> {
        self.raw_patches.iter().filter_map(move |(key, patch)| {
            (key.0 == region && patch.layer.is_land()).then_some(patch)
        })
    }

    /// The terrain-compositing parameters of `region`, once its handshake has
    /// been seen.
    #[must_use]
    pub fn composition_of(&self, region: RegionHandle) -> Option<&TerrainComposition> {
        self.regions
            .get(&region)
            .and_then(|entry| entry.composition.as_ref())
    }

    /// The ground height at region-local metre position (`x`, `y`) in `region`,
    /// read from the nearest decoded land-patch cell, or `None` when that region's
    /// terrain has not been ingested (or the point is off-region / non-finite).
    ///
    /// A land patch holds `size`×`size` height samples spanning `size` metres (one
    /// sample per metre for a standard 16-cell patch), so cell `(⌊x⌋, ⌊y⌋)` within
    /// the patch is the nearest sample. Used by the physics dead-reckoning (P31.2)
    /// as the ground floor an extrapolating object is clamped to
    /// (`getMinAllowedZ`).
    #[must_use]
    pub fn land_height(&self, region: RegionHandle, x: f32, y: f32) -> Option<f32> {
        if !x.is_finite() || !y.is_finite() || x < 0.0 || y < 0.0 {
            return None;
        }
        // The live patches first; fall back to the retained cache when they are
        // absent (a region mid-rebuild, or not yet streamed after login), so the
        // avatar ground floor keeps a stable answer instead of dropping to `None`.
        land_height_in(&self.raw_patches, region, x, y)
            .or_else(|| land_height_in(&self.land_cache, region, x, y))
    }
}

// ---------------------------------------------------------------------------
// The in-world object graph.
//
// What the simulator streams about every prim in the scene, folded into one
// entity per object: the identities, the entities, the shape fingerprints and
// the wire-side blocks the edit, pick, physics, minimap and render-cost
// surfaces all read. The *systems* that produce it — the tessellation, the
// asset fetches, the deferred builds — stay in the world layer above.
// ---------------------------------------------------------------------------

/// The shape-defining parameters of an object, compared between updates so a
/// motion-only update never triggers a re-tessellation. Deliberately excludes
/// the object's position/rotation/scale (which live in the `Transform`, not the
/// mesh) — only a change here means the geometry must be rebuilt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShapeFingerprint {
    /// The object class byte.
    pcode: u8,
    /// The quantized path/profile shape parameters of a volume prim.
    shape: PrimShapeParams,
    /// The sculpt/mesh key and type byte, when the object is a sculpt or mesh.
    sculpt: Option<(SculptOrMeshKey, u8)>,
    /// For a **grass** clump only: the object's X/Y scale (in millimetres) that
    /// sets the blade-centre spread. `None` for every other category, so a resize
    /// rebuilds only a grass patch — whose blade geometry is generated with the
    /// scale baked in (P26.3) — and never a prim / mesh / sculpt / tree (whose
    /// scale rides the geometry holder, so a resize needs no rebuild).
    pub grass_spread: Option<(i32, i32)>,
    /// For a **flexi** prim (P32.2): the flexible block's softness (`Some(0..3)`),
    /// else `None`. A flexi prim's geometry is built at a section count of
    /// `1 << softness`, so toggling flexi on / off or changing the softness must
    /// rebuild the faces (and re-seed the chain state); the other flexi params
    /// (tension / gravity / …) drive the sim live and need no rebuild, so they are
    /// deliberately excluded.
    flexi_softness: Option<u8>,
}

impl ShapeFingerprint {
    /// The shape fingerprint of `object`.
    #[must_use]
    pub fn of(object: &Object) -> Self {
        Self {
            pcode: object.pcode,
            shape: object.shape,
            sculpt: object
                .extra
                .sculpt
                .map(|sculpt| (sculpt.texture, sculpt.sculpt_type)),
            grass_spread: (object.pcode == pcode::GRASS).then(|| {
                // Quantise to millimetres so the fingerprint stays `Eq`; grass is
                // rebuilt when its clump-defining scale changes by ≥ 1 mm.
                #[expect(
                    clippy::as_conversions,
                    clippy::cast_possible_truncation,
                    reason = "object scale in mm is far inside i32 range"
                )]
                (
                    (object.scale.x * 1000.0).round() as i32,
                    (object.scale.y * 1000.0).round() as i32,
                )
            }),
            flexi_softness: object.extra.flexible.as_ref().map(|flexi| flexi.softness),
        }
    }
}

/// Per-object viewer-side bookkeeping, paired with the object's `SceneObject`
/// entity.
#[derive(Debug)]
pub struct TrackedObject {
    /// The entity rendering this object: carries its position/rotation and is the
    /// parent linkset children and attachments hang off. It has **no scale** (see
    /// `object_transform`).
    pub entity: Entity,
    /// The object's full asset [`ObjectKey`] (the region-independent UUID), kept so
    /// an animesh root can be matched to the `ObjectAnimation` (`object_id`) that
    /// drives its control avatar (P29) and its control avatar pruned when the object
    /// is gone. Distinct from the region-scoped local id this object is keyed by.
    pub full_key: ObjectKey,
    /// The per-object geometry holder — a child of [`entity`](Self::entity)
    /// carrying the object's Second Life scale, onto which this object's own faces
    /// are parented so the scale never reaches the child prims below it.
    pub geometry: Entity,
    /// The object's last-seen shape fingerprint, to detect a shape change.
    pub shape: ShapeFingerprint,
    /// The scoped id of this object's parent (a linkset root or the avatar it is
    /// attached to); its own scoped id when it is a root (parent-local id 0).
    pub parent: ScopedObjectId,
    /// Whether this object is a root (has no parent object).
    pub is_root: bool,
    /// Whether this object's entity has been parented to its root entity yet (a
    /// child whose root has not arrived stays `false` until it does). For an
    /// attachment (see [`attachment_point`](Self::attachment_point)) this instead
    /// tracks whether it has been parented to its avatar's skeleton joint (P16.1)
    /// — or, for one worn on a HUD point, whether it has been *routed*: parented
    /// to the HUD screen or hidden as another avatar's (P35.1, terminal either
    /// way).
    pub parented: bool,
    /// The raw attachment-point id if this object is an attachment worn on an
    /// avatar (its `parent` is the avatar), else `None`. An attachment is parented
    /// to its avatar's skeleton joint rather than a linkset root, by
    /// `adopt_pending_attachments` (P16.1).
    pub attachment_point: Option<u8>,
    /// The object's owner (`owner_id` from the object update). For a worn
    /// attachment this is its wearer, so a stuck attachment can be attributed to
    /// the avatar it belongs to (the `SL_VIEWER_LOG_ATTACHMENT_BIND` diagnostic).
    pub owner_id: AgentKey,
    /// The object's last-seen `PrimFlags` bitfield (the update's `UpdateFlags`),
    /// kept for the object context menu's enable gates
    /// ([`ObjectState::pick_summary`]): the agent-relative permission bits
    /// (you-owner, copy) and the touch-handler flag decide which pie slices are
    /// live for this object.
    pub update_flags: u32,
    /// The object's physical-material byte (`LL_MCODE_*`), kept for the build
    /// floater's material editor ([`ObjectState::edit_data`]).
    pub material: u8,
    /// The object's complete last-received extra parameters, kept so an
    /// `ObjectExtraParams` edit (the build floater's Features tab) can resend
    /// the **full** set — the message states the object's complete
    /// extra-parameter state, so a partial send would clear whatever it
    /// omitted (sculpt, animesh, render materials, …). Also a
    /// [`non_motion_blocks_changed`](Self::non_motion_blocks_changed) input.
    pub extra: ObjectExtraParams,
    /// The last-applied texture-animation block — a
    /// [`non_motion_blocks_changed`](Self::non_motion_blocks_changed) input,
    /// so a motion-only update skips the texture-animation refresh.
    pub texture_animation: Option<TextureAnimation>,
    /// The last-applied floating text (`llSetText`) — a
    /// [`non_motion_blocks_changed`](Self::non_motion_blocks_changed) input.
    pub text: String,
    /// The last-applied floating-text colour (alongside
    /// [`text`](Self::text)).
    pub text_color: [u8; 4],
    /// The per-face child entities carrying this object's geometry: one per
    /// non-empty [`PrimFace`](sl_client_bevy::PrimFace) for a plain prim or a
    /// sculpt, or one per non-empty submesh for a mesh object. Rebuilt on a shape
    /// change. Empty for an object not yet tessellated (a mesh or sculpt still
    /// waiting on its asset, or a non-rendered category).
    pub face_entities: Vec<Entity>,
    /// A plain prim's currently tessellated [`PrimLod`] (P21.3), compared against
    /// the driver's desired level to decide whether to re-tessellate. Meaningless
    /// (and left at [`PrimLod::FINEST`]) for a non-prim.
    ///
    /// The re-tessellation *inputs* are not here: a deferred build is machinery,
    /// and lives in the world's own `PendingBuilds` side table.
    pub prim_lod: PrimLod,
    /// A tree's currently generated `TreeTier` (P26.2), compared against the
    /// driver's desired tier to decide whether to regenerate. Meaningless (and left
    /// at [`INITIAL_TREE_TIER`]) for a non-tree.
    pub tree_tier: TreeTier,
    /// Whether this object is an **animated object** (animesh) — its
    /// `ExtendedMesh` param carries the `ANIMATED_MESH_ENABLED` flag. Set on the
    /// linkset root; a worn animesh drives its own control-avatar skeleton, so its
    /// rig joint positions must NOT override the wearer's skeleton (R1), matching
    /// the reference viewer's `!vo->isAnimatedObject()` filter.
    pub animated: bool,
    /// The object's last-received raw `TextureEntry` bytes, retained so the build
    /// floater's Texture tab (`crate::edit_texture`) can read the current
    /// per-face placement and re-send a modified entry (`ObjectImage`). A
    /// non-empty full update overwrites it; a terse (motion-only) update, which
    /// carries no texture entry, leaves it untouched.
    pub texture_entry: Vec<u8>,
    /// The object's last-received legacy media URL, round-tripped on an
    /// `ObjectImage` send so a texture edit does not clear it (the wire message
    /// carries the whole media-URL field, so omitting it would blank it).
    pub media_url: Option<String>,
    /// The object's size along each axis, in Second Life metres, exactly as sent
    /// ([`Object::scale`]). The rendered scale rides the geometry holder's
    /// transform (in the Bevy frame), so the wire value is kept here for the
    /// consumers that reason in Second Life units — the avatar render-cost model
    /// (`crate::avatar_complexity`), whose triangle estimate is weighted by the
    /// prim's radius and whose surface-area trigger is in square metres.
    pub scale: Vec3,
}

impl TrackedObject {
    /// Whether any **non-motion** input of the known-object component refresh
    /// differs from the last applied update — the gate that lets a terse
    /// motion update (whose merged snapshot changes only the motion fields)
    /// skip the per-block component helpers and their no-op removes entirely.
    /// Compares exactly what those helpers read: the extra params (light /
    /// particles / flexi / reflection probe / render materials), the texture
    /// animation, the floating text, the update flags (the physics toggle
    /// among them), the material byte, and the linkset / attachment identity
    /// (which decides the HUD routing and root marker).
    #[must_use]
    pub fn non_motion_blocks_changed(
        &self,
        object: &Object,
        is_root: bool,
        parent: ScopedObjectId,
        attachment_point: Option<u8>,
    ) -> bool {
        self.update_flags != object.update_flags
            || self.material != object.material
            || self.is_root != is_root
            || self.parent != parent
            || self.attachment_point != attachment_point
            || self.text != object.text
            || self.text_color != object.text_color
            || self.texture_animation != object.texture_animation
            || self.extra != object.extra
    }
}

/// How far up a parent chain `in_hud_attachment` walks before giving up. An
/// attachment's chain is short — object → (linkset root) → avatar — so this only
/// guards against a malformed (cyclic) parent link in the object stream.
pub const MAX_PARENT_WALK: usize = 8;

/// The rendered level of detail of a Linden tree (P26.2): one of the four
/// [`TreeLod`] branching-geometry tiers, or the far [`TreeTier::Billboard`]
/// imposter that stands in for the whole tree once it is small on screen. Selected
/// by the render-priority driver from the tree's on-screen size, mirroring the
/// reference viewer's `LLVOTree::mTrunkLOD` selection plus its billboard fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeTier {
    /// Procedural branch / leaf geometry at the given trunk level of detail.
    Lod(TreeLod),
    /// The distant crossed-quad billboard imposter (`tree_billboard_geometry`).
    Billboard,
}

/// The tree tier a new tree is first built at (P26.2), before the render-priority
/// driver has a camera to size it against — a mid branching level the driver
/// refines toward the tier the tree's on-screen size warrants, like a plain prim's
/// `INITIAL_MANAGED_PRIM_LOD`.
pub const INITIAL_TREE_TIER: TreeTier = TreeTier::Lod(TreeLod::High);

/// Viewer-side object bookkeeping: the entity and metadata for every in-world
/// object currently in the scene, keyed by scoped id.
#[derive(Debug, Resource, Default)]
pub struct ObjectState {
    /// Every tracked object, keyed by its scoped id.
    pub objects: HashMap<ScopedObjectId, TrackedObject>,
    /// The region the Bevy scene is currently anchored at (origin `<0,0,0>`), so
    /// a **root** object in a neighbour region is offset onto the right terrain
    /// (`object_transform`) and every root is re-based when this moves
    /// (`recenter_objects`). `None` until the first region is known; kept in
    /// lockstep with [`TerrainState`]'s origin (both follow
    /// `SlIdentity`'s root handle).
    pub origin: Option<RegionHandle>,
}

impl ObjectState {
    /// Despawn **every** tracked object entity (and its faces) and forget them —
    /// the object half of the scene-mirror purge a **fresh-circuit** teleport
    /// needs. The session cleared its object cache with no per-object
    /// `KillObject`, so the incremental [`remove_object`](Self::remove_object) path
    /// never fires;
    /// without this the old region's objects linger forever, at offsets that no
    /// longer correspond to any connected region
    /// ([`Event::RegionChanged`](sl_client_bevy::SlSessionEvent)'s `world_reset`).
    ///
    /// The own avatar's **object** entity is purged along with the rest — it is
    /// only a position-only mirror; the agent's *visible body* is kept across the
    /// purge by [`AvatarState::purge`] (keyed
    /// by agent, so it does not flash), and the destination re-streams the object
    /// entity. Keeping it here would instead strand it as a ghost dot at the spot
    /// we left, because the same avatar is streamed by *every* connected region so
    /// no single copy is authoritative.
    ///
    /// Also drops the origin anchor so `recenter_objects` re-anchors on the
    /// destination without a spurious re-base shift.
    ///
    /// The purged objects' deferred builds are **not** dropped here — they live
    /// in the world's own `PendingBuilds`, whose `clear`
    /// the reset system calls alongside this.
    pub fn purge(&mut self, commands: &mut Commands) {
        for tracked in self.objects.values() {
            // Bevy's hierarchy despawn takes the geometry holder + parented
            // linkset children; `try_despawn` tolerates an entity a parent already
            // reaped. A rigged mesh's faces hang off the avatar body root, so
            // despawn them explicitly (a no-op for a static mesh).
            commands.entity(tracked.entity).try_despawn();
            despawn_prim_faces(&tracked.face_entities, commands);
        }
        self.objects.clear();
        self.origin = None;
    }

    /// Despawn the entity for `scoped` and every tracked descendant, dropping them
    /// all from the map. Bevy's hierarchy despawns the entity's parented children
    /// with it; any tracked-but-not-yet-parented descendants are despawned
    /// explicitly so a lingering child update can never touch a dead entity.
    ///
    /// Returns every region-scoped id it dropped (the root first, then its
    /// descendants) — the removal's full extent, which the caller both records as
    /// suppressed on the derender path (`crate::derender`: those ids are the only
    /// handle left on objects the simulator has already streamed and will not send
    /// again, so they are what a later un-derender re-fetches) and hands to
    /// `PendingBuilds::forget_all`, which no longer loses their deferred builds
    /// implicitly. Empty when `scoped` was not tracked.
    pub fn remove_object(
        &mut self,
        scoped: ScopedObjectId,
        commands: &mut Commands,
    ) -> Vec<ScopedObjectId> {
        let Some(removed) = self.objects.remove(&scoped) else {
            return Vec::new();
        };
        // Bevy despawns the parented sub-hierarchy together with the root entity.
        // `try_despawn` because this entity may already be dead — a linkset child or
        // attachment can be taken by its parent's hierarchy despawn before its own
        // `KillObject` arrives here (the same race
        // [`drop_stale_tracked_entity`](Self::drop_stale_tracked_entity) guards on the
        // update path), and a plain `despawn` on it would itself warn.
        commands.entity(removed.entity).try_despawn();
        // A rigged mesh's skinned faces hang off the *avatar body root*, not this
        // object entity (P17.2), so Bevy's hierarchy despawn above does not take them —
        // despawn them explicitly (a no-op for a static mesh's faces, already gone with
        // their object entity).
        despawn_prim_faces(&removed.face_entities, commands);
        // Drop tracked descendants; despawn any that were still waiting to be
        // parented (Bevy did not despawn those with the root), and their faces.
        // Collected before they are dropped, since the walk follows the parent links.
        let mut dropped = vec![scoped];
        for descendant in self.tracked_descendants(scoped) {
            if let Some(entry) = self.objects.remove(&descendant) {
                despawn_prim_faces(&entry.face_entities, commands);
                if !entry.parented {
                    commands.entity(entry.entity).try_despawn();
                }
                dropped.push(descendant);
            }
        }
        dropped
    }

    /// The scoped ids of every tracked transitive descendant of `root` (children,
    /// grandchildren, …), following the stored parent links.
    fn tracked_descendants(&self, root: ScopedObjectId) -> Vec<ScopedObjectId> {
        let mut descendants = Vec::new();
        let mut frontier = vec![root];
        while let Some(parent) = frontier.pop() {
            for (&scoped, tracked) in &self.objects {
                if !tracked.is_root && tracked.parent == parent {
                    descendants.push(scoped);
                    frontier.push(scoped);
                }
            }
        }
        descendants
    }

    /// Drop the tracked object under `scoped` when its entity has been despawned out
    /// from under the map — a linkset child or worn attachment that Bevy's recursive
    /// despawn took with its parent (a removed linkset root, or a departed avatar whose
    /// skeleton-joint node it hangs off), with no
    /// [`remove_object`](Self::remove_object) to clean the entry.
    ///
    /// `is_alive` reports whether an entity is still spawned (in the viewer,
    /// `Commands::get_entity(..).is_ok()`). A live entity is left untouched — this never
    /// drops an object still on screen, so no live transform / material write is lost.
    /// Returns the dropped entity when a stale entry was removed, else `None`; the
    /// caller then also forgets the object's deferred builds.
    pub fn drop_stale_tracked_entity(
        &mut self,
        scoped: ScopedObjectId,
        mut is_alive: impl FnMut(Entity) -> bool,
    ) -> Option<Entity> {
        let entity = self.objects.get(&scoped)?.entity;
        if is_alive(entity) {
            return None;
        }
        let _stale = self.objects.remove(&scoped);
        Some(entity)
    }

    /// The region the Bevy scene is currently anchored at (scene origin), or
    /// [`None`] before the first root region streams. In-world sounds
    /// (`viewer-in-world-sounds`) need it to place a `SoundTrigger`'s
    /// region-local position into absolute scene space
    /// (`region_offset_bevy`).
    #[must_use]
    pub const fn origin(&self) -> Option<RegionHandle> {
        self.origin
    }

    /// The full (grid-wide) [`ObjectKey`] of a tracked object, looked up by its
    /// region-scoped id. Used by the physics module (P31.3) to translate a pushed
    /// `ObjectPhysicsProperties` event — which keys by [`ScopedObjectId`] — onto the
    /// same [`ObjectKey`] the `GetObjectPhysicsData` capability reply uses.
    #[must_use]
    pub fn full_key(&self, scoped: &ScopedObjectId) -> Option<ObjectKey> {
        self.objects.get(scoped).map(|tracked| tracked.full_key)
    }

    /// The entity of the object with region-scoped id `scoped`, or [`None`] if
    /// this viewer does not track it. Used by the object-selection core
    /// (`viewer-object-selection-core`) to resolve a simulator-forced selection
    /// (`ForceObjectSelect`) onto scene entities.
    #[must_use]
    pub fn entity_by_scoped(&self, scoped: &ScopedObjectId) -> Option<Entity> {
        self.objects.get(scoped).map(|tracked| tracked.entity)
    }

    /// The object's physical-material byte (`LL_MCODE_*`), looked up by its
    /// region-scoped id. In-world collision sounds (`viewer-in-world-sounds`)
    /// read it to pick the reference default material collision sound.
    #[must_use]
    pub fn material_by_scoped(&self, scoped: &ScopedObjectId) -> Option<u8> {
        self.objects.get(scoped).map(|tracked| tracked.material)
    }

    /// The geometry-holder child entity of the object with region-scoped id
    /// `scoped` — the entity carrying the object's Second Life scale — or
    /// [`None`] if untracked. The transform gizmos (`viewer-transform-gizmos`)
    /// write a live scale edit there so the resize shows before the simulator
    /// echoes it.
    #[must_use]
    pub fn geometry_of(&self, scoped: &ScopedObjectId) -> Option<Entity> {
        self.objects.get(scoped).map(|tracked| tracked.geometry)
    }

    /// The **parent object's** entity of the linked part with region-scoped id
    /// `scoped`, or [`None`] for a root / attachment / untracked parent. The
    /// transform gizmos fold a linked part's world-space edit back into its
    /// parent's frame through this entity's global transform.
    #[must_use]
    pub fn parent_entity_of(&self, scoped: &ScopedObjectId) -> Option<Entity> {
        let tracked = self.objects.get(scoped)?;
        if tracked.is_root || tracked.attachment_point.is_some() {
            return None;
        }
        self.objects
            .get(&tracked.parent)
            .map(|parent| parent.entity)
    }

    /// Every prim of the in-world linkset rooted at `root`: the root itself
    /// first, then every tracked child prim whose parent is it in a **stable**
    /// order (by region-local id — attachments excluded, as they hang off an
    /// avatar, not a linkset root). An untracked or non-root `root` yields the
    /// object alone if present, else nothing.
    ///
    /// Second Life linksets are one level deep — a child's parent is always the
    /// linkset root — so a single pass over the object table finds the whole
    /// set. The child order is the local-id sort, not the simulator's true link
    /// order (the wire carries no per-child link position; even the reference
    /// notes its child order "is not always the same as sim's idea of link
    /// order"), but it is stable frame to frame, which the prim-navigation
    /// buttons (`crate::edit_tool`) and link-number read-out rely on.
    ///
    /// Used by prim unlinking (`viewer-prim-linking`): a whole-linkset unlink
    /// sends an `ObjectDelink` naming **every** prim of the set
    /// (`SEND_INDIVIDUALS`) to break it fully apart; naming only the root would
    /// leave the simulator re-linking the orphaned children into a new set
    /// (OpenSim's `SceneGraph::DelinkObjects`).
    #[must_use]
    pub fn linkset_members(&self, root: &ScopedObjectId) -> Vec<ScopedObjectId> {
        let mut members = Vec::new();
        if !self.objects.contains_key(root) {
            return members;
        }
        members.push(*root);
        let mut children: Vec<ScopedObjectId> = self
            .objects
            .iter()
            .filter(|(scoped, tracked)| {
                *scoped != root
                    && !tracked.is_root
                    && tracked.attachment_point.is_none()
                    && tracked.parent == *root
            })
            .map(|(scoped, _tracked)| *scoped)
            .collect();
        children.sort_by_key(|scoped| scoped.id);
        members.extend(children);
        members
    }

    /// The scoped id of the linkset **root** that the object `scoped` belongs to
    /// — the object itself when it is a root, its parent when it is a linked
    /// child, or `None` when untracked or a worn attachment. The edit surfaces
    /// resolve a picked linked part back to its linkset this way.
    #[must_use]
    pub fn linkset_root_of(&self, scoped: &ScopedObjectId) -> Option<ScopedObjectId> {
        let tracked = self.objects.get(scoped)?;
        if tracked.attachment_point.is_some() {
            return None;
        }
        if tracked.is_root {
            Some(*scoped)
        } else {
            Some(tracked.parent)
        }
    }

    /// The number of prims in the linkset rooted at `root` — the reference's
    /// per-linkset prim count. Drives the link-limit guard
    /// (`viewer-prim-linking`): a Second Life linkset may hold at most 255
    /// children, so the summed prim count of a link operation is capped.
    #[must_use]
    pub fn linkset_prim_count(&self, root: &ScopedObjectId) -> usize {
        self.linkset_members(root).len()
    }

    /// The region-scoped ids of every tracked object whose persistent id is `id`
    /// — normally one, but an object streamed by two connected regions has one
    /// per circuit. The derender path (`viewer-derender-blacklist`) uses it to
    /// despawn what a fresh blacklist entry names: it knows the target's full
    /// id, not its (transient) region-scoped one.
    ///
    /// A scan, like [`entity_of`](Self::entity_of), and run only per derender —
    /// never per frame.
    #[must_use]
    pub fn scoped_by_full_id(&self, id: Uuid) -> Vec<ScopedObjectId> {
        self.objects
            .iter()
            .filter(|(_scoped, tracked)| tracked.full_key.uuid() == id)
            .map(|(scoped, _tracked)| *scoped)
            .collect()
    }

    /// The entity of the object with grid-wide key `key`, or [`None`] if this viewer does
    /// not have it. The reverse of [`full_key`](Self::full_key), used by the point-at
    /// receive path (P31.15) to resolve another avatar's point-at effect — whose target is
    /// named by its full key — against the target object's current transform.
    ///
    /// Objects are keyed by their region-scoped id, so this is a scan; it runs only per
    /// received effect (a handful a second at most), not per frame.
    #[must_use]
    pub fn entity_of(&self, key: ObjectKey) -> Option<Entity> {
        self.objects
            .values()
            .find(|tracked| tracked.full_key == key)
            .map(|tracked| tracked.entity)
    }

    /// The region-scoped ids of the tracked objects whose full [`ObjectKey`] is
    /// in `keys`, in one pass over the object table. The bulk counterpart of
    /// [`entity_of`](Self::entity_of), for the animesh drivers (P29.2): an
    /// `ObjectAnimation` names the linkset **part** holding the animations by
    /// full key, and every signalled part must resolve each frame — a per-key
    /// scan would be quadratic.
    #[must_use]
    pub fn scoped_by_full_keys(
        &self,
        keys: &HashSet<ObjectKey>,
    ) -> HashMap<ObjectKey, ScopedObjectId> {
        if keys.is_empty() {
            return HashMap::new();
        }
        self.objects
            .iter()
            .filter(|(_scoped, tracked)| keys.contains(&tracked.full_key))
            .map(|(&scoped, tracked)| (tracked.full_key, scoped))
            .collect()
    }

    /// Everything the object context menu needs to know about a picked object
    /// (`crate::object_menu`), resolved by walking the linkset parent chain up
    /// to its root: the picked prim itself (the touch / sit target), the linkset
    /// root (the derez target — take / delete / return act on roots), the
    /// combined permission flags, and whether the chain is a worn attachment
    /// (which gets an attachment pie — `crate::attachment_menu` — rather than
    /// the object one).
    ///
    /// The flags are the **union** of the picked prim's and the root's, because
    /// the agent-relative bits (you-owner, copy) ride the root while the
    /// touch-handler flag can sit on either. For an attachment the walk stops at
    /// the **attachment root** (the object carrying the attachment point), whose
    /// parent is the avatar wearing it — surfaced as
    /// [`wearer`](ObjectPickSummary::wearer) so the attachment pies can decide
    /// self vs other. The walk is bounded like `in_hud_attachment`'s, against
    /// a malformed (cyclic) parent link.
    #[must_use]
    pub fn pick_summary(&self, scoped: ScopedObjectId) -> Option<ObjectPickSummary> {
        let picked = self.objects.get(&scoped)?;
        let mut root_scoped = scoped;
        let mut root = picked;
        let mut attachment = picked.attachment_point.is_some();
        for _step in 0..MAX_PARENT_WALK {
            if root.is_root || attachment {
                break;
            }
            let next = root.parent;
            let Some(parent) = self.objects.get(&next) else {
                break;
            };
            root_scoped = next;
            root = parent;
            attachment = root.attachment_point.is_some();
        }
        Some(ObjectPickSummary {
            picked_scoped: scoped,
            picked_full: picked.full_key,
            root_scoped,
            root_full: root.full_key,
            flags: picked.update_flags | root.update_flags,
            attachment,
            wearer: attachment.then_some(root.parent),
        })
    }

    /// The per-face child entities of the object with grid-wide key `key`, or
    /// `None` if the object is unknown (or not yet tessellated). Used by the
    /// media-on-a-prim driver (`crate::media_prim`) to find the face entity a
    /// The per-face child entities carrying `scoped`'s geometry, or an empty
    /// slice if it is untracked or not yet tessellated.
    ///
    /// A **rigged** attachment's faces are parented to the wearer's body root
    /// rather than to the object entity, so hiding the object alone does not hide
    /// them — the jelly render (`crate::avatar_complexity`) needs the faces
    /// themselves.
    #[must_use]
    pub fn face_entities_of(&self, scoped: &ScopedObjectId) -> &[Entity] {
        self.objects
            .get(scoped)
            .map_or(&[], |tracked| &tracked.face_entities)
    }

    /// The per-face child entities of the object with grid-wide key `key`, or
    /// `None` if the object is unknown (or not yet tessellated). Used by the
    /// media-on-a-prim driver (`crate::media_prim`) to find the face entity a
    /// media surface's texture goes onto. A scan like [`entity_of`](Self::entity_of),
    /// run only when media data changes — not per frame.
    #[must_use]
    pub fn face_entities_by_key(&self, key: ObjectKey) -> Option<&[Entity]> {
        self.objects
            .values()
            .find(|tracked| tracked.full_key == key)
            .map(|tracked| tracked.face_entities.as_slice())
    }

    /// The `UpdateFlags` bits of the object with grid-wide key `key` (its own,
    /// not OR-ed with its root's), or `None` if unknown. The media permission
    /// check reads the you-owner bit from these.
    #[must_use]
    pub fn update_flags_by_key(&self, key: ObjectKey) -> Option<u32> {
        self.objects
            .values()
            .find(|tracked| tracked.full_key == key)
            .map(|tracked| tracked.update_flags)
    }

    /// The agent-relative `UpdateFlags` of `scoped` — its own bits OR-ed with its
    /// linkset **root's** (the agent-relative modify / move / copy / you-owner
    /// bits ride the root, exactly as [`pick_summary`](Self::pick_summary) reads
    /// them), or `None` if untracked. The simulator computes these for *this*
    /// agent (OpenSim's `GenerateClientFlags`), so they already fold in owner /
    /// group / everyone permissions and the object's "anyone can move" flag —
    /// the same signal the reference viewer's `permModify` / `permMove` read.
    pub(crate) fn agent_flags(&self, scoped: &ScopedObjectId) -> Option<u32> {
        let picked = self.objects.get(scoped)?;
        let mut flags = picked.update_flags;
        let mut attachment = picked.attachment_point.is_some();
        let mut current = picked;
        for _step in 0..MAX_PARENT_WALK {
            if current.is_root || attachment {
                break;
            }
            let Some(parent) = self.objects.get(&current.parent) else {
                break;
            };
            current = parent;
            flags |= current.update_flags;
            attachment = current.attachment_point.is_some();
        }
        Some(flags)
    }

    /// Whether this agent may **modify** `scoped` (shape / scale / texture /
    /// material / name / flags) — the `FLAGS_OBJECT_MODIFY` bit. An untracked
    /// object reads modifiable (optimistic: the simulator arbitrates), so a
    /// transient tracking gap never wrongly greys a control.
    #[must_use]
    pub fn agent_can_modify(&self, scoped: &ScopedObjectId) -> bool {
        self.agent_flags(scoped)
            .is_none_or(|flags| flags & FLAGS_OBJECT_MODIFY != 0)
    }

    /// Whether this agent may **move** `scoped` (position / rotation) — modify
    /// permission, or the `FLAGS_OBJECT_MOVE` bit the simulator sets for the
    /// owner and for an "anyone can move" object. Untracked reads movable.
    #[must_use]
    pub fn agent_can_move(&self, scoped: &ScopedObjectId) -> bool {
        self.agent_flags(scoped)
            .is_none_or(|flags| flags & (FLAGS_OBJECT_MODIFY | FLAGS_OBJECT_MOVE) != 0)
    }

    /// Whether this agent may **copy** `scoped` — the `FLAGS_OBJECT_COPY` bit.
    /// Untracked reads copyable.
    #[must_use]
    pub fn agent_can_copy(&self, scoped: &ScopedObjectId) -> bool {
        self.agent_flags(scoped)
            .is_none_or(|flags| flags & FLAGS_OBJECT_COPY != 0)
    }

    /// Whether this agent **owns** `scoped` — the `FLAGS_OBJECT_YOU_OWNER` bit
    /// (the reference viewer's `permYouOwner`). Unlike the modify / move / copy
    /// helpers this is **not** optimistic: an untracked object reads *not owned*,
    /// because ownership is a positive grant that gates owner-only affordances
    /// (the contents rename / remove menu items), where a wrong "yes" would offer
    /// an action the simulator then refuses.
    #[must_use]
    pub fn agent_owns(&self, scoped: &ScopedObjectId) -> bool {
        self.agent_flags(scoped)
            .is_some_and(|flags| flags & FLAGS_OBJECT_YOU_OWNER != 0)
    }

    /// Whether `scoped` lets **anyone** add inventory to its contents — the
    /// `FLAGS_ALLOW_INVENTORY_DROP` bit (the reference's `flagAllowInventoryAdd`),
    /// the one exception to needing modify on the object to drop an item in.
    /// Untracked reads *false* (the drop still needs modify then).
    #[must_use]
    pub fn agent_allows_inventory_drop(&self, scoped: &ScopedObjectId) -> bool {
        self.agent_flags(scoped)
            .is_some_and(|flags| flags & FLAGS_ALLOW_INVENTORY_DROP != 0)
    }

    /// Locally echo an edited `PrimFlags` bit (the build floater's
    /// physical / temporary / phantom toggles) so the checkbox flips
    /// immediately; the simulator's own `ObjectUpdate` echo confirms (or
    /// reverts) it. Display-only: the physics / render systems re-sync from
    /// the echoed update, not from this.
    pub fn apply_local_flag_edit(&mut self, scoped: &ScopedObjectId, bit: u32, on: bool) {
        if let Some(tracked) = self.objects.get_mut(scoped) {
            if on {
                tracked.update_flags |= bit;
            } else {
                tracked.update_flags &= !bit;
            }
        }
    }

    /// Locally echo an edited material byte (the build floater's material
    /// cycle); display-only, confirmed by the simulator's echo.
    pub fn apply_local_material_edit(&mut self, scoped: &ScopedObjectId, material: u8) {
        if let Some(tracked) = self.objects.get_mut(scoped) {
            tracked.material = material;
        }
    }

    /// Locally echo an edited extra-parameter set (the build floater's flexi /
    /// light editors) so the Features tab reflects the send immediately;
    /// display-only — the renderers' components re-sync from the simulator's
    /// echoed update, and the shape fingerprint is deliberately untouched so
    /// that echo still triggers the re-tessellation it needs.
    pub fn apply_local_extra_edit(&mut self, scoped: &ScopedObjectId, extra: ObjectExtraParams) {
        if let Some(tracked) = self.objects.get_mut(scoped) {
            tracked.extra = extra;
        }
    }

    /// Everything the build floater's parameter tabs
    /// (`viewer-prim-parameter-editing`) read for one selected object: its
    /// object class, quantized shape, material byte, `PrimFlags` bits, and its
    /// complete extra parameters (borrowed — clone only what an edit resends).
    #[must_use]
    pub fn edit_data(&self, scoped: &ScopedObjectId) -> Option<ObjectEditData<'_>> {
        self.objects.get(scoped).map(|tracked| ObjectEditData {
            pcode: tracked.shape.pcode,
            shape: tracked.shape.shape,
            material: tracked.material,
            update_flags: tracked.update_flags,
            extra: &tracked.extra,
        })
    }

    /// The object's last-received raw `TextureEntry` bytes, for the Texture-tab
    /// editor (`crate::edit_texture`) to decode the current per-face placement
    /// and re-send a modified entry. `None` if untracked, an empty slice if the
    /// object has not carried a texture entry yet.
    #[must_use]
    pub fn texture_entry_of(&self, scoped: &ScopedObjectId) -> Option<&[u8]> {
        self.objects
            .get(scoped)
            .map(|tracked| tracked.texture_entry.as_slice())
    }

    /// The object's last-received legacy media URL, round-tripped on an
    /// `ObjectImage` send so a Texture-tab edit does not clear it.
    #[must_use]
    pub fn media_url_of(&self, scoped: &ScopedObjectId) -> Option<String> {
        self.objects
            .get(scoped)
            .and_then(|tracked| tracked.media_url.clone())
    }

    /// Every tracked in-world (non-attachment) prim for the minimap's object
    /// layer: its entity (for the transform), its own `PrimFlags` bits, and its
    /// root's flags OR-ed in (the agent-relative you-owner / group-owned bits
    /// ride the root, exactly as [`pick_summary`](Self::pick_summary) reads
    /// them). Worn objects — anything whose parent walk reaches an attachment
    /// point — are excluded, as the reference's map membership excludes them.
    ///
    /// **Avatars** (`pcode` 47) are excluded too: an avatar belongs on the minimap
    /// *avatar* layer (drawn from [`AvatarState`],
    /// deduplicated by agent), not the object layer. The same avatar is streamed
    /// as a separate object by *every* connected region (root and each neighbour
    /// child circuit), so admitting them here would plot one object dot per region
    /// — and leave a ghost dot at a region left behind whose copy has not been
    /// reaped (viewer-crossing-stale-minimap-self-dot).
    #[must_use]
    pub fn minimap_objects(&self) -> Vec<(Entity, u32)> {
        let mut out = Vec::with_capacity(self.objects.len());
        for tracked in self.objects.values() {
            if tracked.shape.pcode == pcode::AVATAR {
                continue;
            }
            let mut flags = tracked.update_flags;
            let mut attachment = tracked.attachment_point.is_some();
            let mut current = tracked;
            for _step in 0..MAX_PARENT_WALK {
                if current.is_root || attachment {
                    break;
                }
                let Some(parent) = self.objects.get(&current.parent) else {
                    break;
                };
                current = parent;
                flags |= current.update_flags;
                attachment = current.attachment_point.is_some();
            }
            if attachment {
                continue;
            }
            out.push((tracked.entity, flags));
        }
        out
    }

    /// The facts the static collider index (`crate::physics::build_static_colliders`)
    /// needs about one tracked prim, keyed by its scoped id, or `None` if the prim
    /// is not tracked. Reads the wire-side state the resource already holds so the
    /// collider builder does not need its own per-entity component mirror.
    #[must_use]
    pub fn static_collider_facts(&self, scoped: &ScopedObjectId) -> Option<StaticColliderFacts> {
        let tracked = self.objects.get(scoped)?;
        Some(StaticColliderFacts {
            full_key: tracked.full_key,
            phantom: tracked.update_flags & FLAGS_PHANTOM != 0,
            // A mesh prim's collider comes from its uploaded physics shape; a plain
            // prim / sculpt from its tessellated geometry (mesh key `None`).
            mesh: match tracked.extra.sculpt.map(|sculpt| sculpt.texture) {
                Some(SculptOrMeshKey::Mesh(key)) => Some(key),
                _other => None,
            },
            // A flexi prim's geometry is baked in absolute metres (its holder
            // applies no scale — see [`holder_transform`]), so scaling it by the
            // object scale would be wrong; the collider builder skips it (it is also
            // phantom, so nothing collides with it anyway).
            flexi: tracked.extra.flexible.is_some(),
        })
    }

    /// Every tracked **worn attachment root**, grouped by the scoped id of the
    /// avatar object wearing it — the index the avatar render-cost model walks
    /// (`viewer-avatar-complexity-limit`).
    ///
    /// Only an attachment *root* carries an attachment point, so this is exactly
    /// the set of worn linksets; each one's prims come from
    /// [`linkset_members`](Self::linkset_members). **HUD attachments are
    /// excluded**: they hang off your own screen, are drawn for nobody else, and
    /// the reference likewise leaves them out of the wearer's complexity
    /// (`!attached_object->isHUDAttachment()`).
    #[must_use]
    pub fn attachment_roots_by_wearer(&self) -> HashMap<ScopedObjectId, Vec<ScopedObjectId>> {
        let mut worn: HashMap<ScopedObjectId, Vec<ScopedObjectId>> = HashMap::new();
        for (scoped, tracked) in &self.objects {
            let Some(point) = tracked.attachment_point else {
                continue;
            };
            if is_hud_point(point) {
                continue;
            }
            worn.entry(tracked.parent).or_default().push(*scoped);
        }
        worn
    }

    /// The scoped id of the **avatar object** that tracked object `scoped` is
    /// worn on — walking up the linkset to the attachment root and taking its
    /// parent — or `None` when it is not (part of) a worn, non-HUD attachment.
    ///
    /// The render-cost model marks a wearer's score stale from any object event
    /// in their attachments, and only the attachment *root* names the avatar, so
    /// a linked child prim has to be chased up to it. The walk is bounded exactly
    /// like `in_hud_attachment`'s, against a malformed parent cycle.
    #[must_use]
    pub fn wearer_of(&self, scoped: ScopedObjectId) -> Option<ScopedObjectId> {
        let mut current = scoped;
        for _ in 0..MAX_PARENT_WALK {
            let tracked = self.objects.get(&current)?;
            if let Some(point) = tracked.attachment_point {
                return (!is_hud_point(point)).then_some(tracked.parent);
            }
            if tracked.is_root {
                return None;
            }
            current = tracked.parent;
        }
        None
    }

    /// The wire-side facts the avatar render-cost model needs about one tracked
    /// prim (`crate::avatar_complexity`), or `None` if it is not tracked.
    ///
    /// Like [`static_collider_facts`](Self::static_collider_facts) this reads
    /// state the resource already holds rather than adding a per-entity mirror —
    /// the cost is evaluated for a handful of avatars at a time, never per frame
    /// for the whole scene.
    #[must_use]
    pub fn complexity_facts(&self, scoped: &ScopedObjectId) -> Option<PrimComplexityFacts<'_>> {
        let tracked = self.objects.get(scoped)?;
        let sculpt = tracked.extra.sculpt.map(|sculpt| sculpt.texture);
        Some(PrimComplexityFacts {
            entity: tracked.entity,
            scale: tracked.scale,
            shape: tracked.shape.shape,
            mesh: match sculpt {
                Some(SculptOrMeshKey::Mesh(key)) => Some(key),
                _other => None,
            },
            sculpt_map: match sculpt {
                Some(SculptOrMeshKey::Sculpt(key)) => Some(key),
                _other => None,
            },
            texture_entry: &tracked.texture_entry,
            flexi: tracked.extra.flexible.is_some(),
            light: tracked.extra.light.is_some(),
            animated: tracked.animated,
            is_root: tracked.is_root,
            texture_animated: tracked.texture_animation.is_some(),
        })
    }
}

/// The wire-side facts [`ObjectState::complexity_facts`] surfaces for the avatar
/// render-cost model: everything the reference's `LLVOVolume::getRenderCost`
/// reads off one prim, without exposing the tracked object itself.
#[derive(Debug, Clone, Copy)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "one flag per input the reference's render-cost formula reads off a prim"
)]
pub struct PrimComplexityFacts<'state> {
    /// The prim's scene entity — the handle the jelly render uses to hide it (and
    /// the key its particle-system component is queried by).
    pub entity: Entity,
    /// The prim's size along each axis, in Second Life metres.
    pub scale: Vec3,
    /// The prim's path / profile shape parameters, from which a non-mesh prim's
    /// per-level triangle counts are estimated.
    pub shape: PrimShapeParams,
    /// The mesh asset key when the prim is a mesh, else `None`.
    pub mesh: Option<MeshKey>,
    /// The sculpt-map texture when the prim is a legacy sculpt, else `None` (it
    /// counts as one of the prim's textures, as in the reference).
    pub sculpt_map: Option<TextureKey>,
    /// The prim's raw `TextureEntry` bytes (per-face texture, tint, glow, bump,
    /// shiny, tex-gen and media flags).
    pub texture_entry: &'state [u8],
    /// Whether the prim is flexible (the reference's heaviest multiplier).
    pub flexi: bool,
    /// Whether the prim emits light.
    pub light: bool,
    /// Whether the prim is an animated object (animesh).
    pub animated: bool,
    /// Whether the prim is a linkset root.
    pub is_root: bool,
    /// Whether the prim carries a texture animation (`llSetTextureAnim`).
    pub texture_animated: bool,
}

/// The wire-side facts [`ObjectState::static_collider_facts`] surfaces for the
/// static collider index: enough to pick a prim's collision layer and shape source
/// without a dedicated per-entity component.
#[derive(Debug, Clone, Copy)]
pub struct StaticColliderFacts {
    /// The object's full (grid-wide) key — how its physics-shape data is keyed in
    /// `ObjectPhysicsShapes`(crate::physics::ObjectPhysicsShapes).
    pub full_key: ObjectKey,
    /// Whether the prim is phantom (`FLAGS_PHANTOM`): indexed but not collidable.
    pub phantom: bool,
    /// The mesh asset key when the prim is a mesh, else `None` (a plain prim or
    /// sculpt whose collider comes from its tessellated geometry).
    pub mesh: Option<MeshKey>,
    /// Whether the prim is a flexi prim (skip — its geometry is not holder-scaled).
    pub flexi: bool,
}

/// What [`ObjectState::edit_data`] reports for one tracked object — the
/// last-received wire-side state the build floater's parameter tabs edit.
#[derive(Debug, Clone, Copy)]
pub struct ObjectEditData<'state> {
    /// The object class byte (`PCode`); only a [`pcode::PRIMITIVE`] is
    /// shape-editable.
    pub pcode: u8,
    /// The quantized path/profile shape parameters.
    pub shape: PrimShapeParams,
    /// The physical-material byte (`LL_MCODE_*`).
    pub material: u8,
    /// The object's `PrimFlags` bits (physical / temporary / phantom live
    /// here).
    pub update_flags: u32,
    /// The object's complete extra parameters (flexi, light, sculpt, …).
    pub extra: &'state ObjectExtraParams,
}

/// What [`ObjectState::pick_summary`] resolves a picked prim to: the identities
/// the object context menu's actions need, and the flag bits its enable gates
/// read. See `crate::object_menu`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObjectPickSummary {
    /// The picked prim itself — the touch and sit target.
    pub picked_scoped: ScopedObjectId,
    /// The picked prim's full (grid-wide) key — what `AgentRequestSit` targets.
    pub picked_full: ObjectKey,
    /// The linkset root — what take / delete / return derez.
    pub root_scoped: ScopedObjectId,
    /// The root's full key — what a properties(-family) request queries.
    pub root_full: ObjectKey,
    /// The union of the picked prim's and the root's `PrimFlags` bits.
    pub flags: u32,
    /// Whether the picked chain is worn on an avatar (including HUDs) — such a
    /// pick belongs to the attachment pies (`crate::attachment_menu`), not the
    /// object one.
    pub attachment: bool,
    /// For a worn chain, the scoped id of the **avatar object** the attachment
    /// root hangs on (its wearer), resolvable to an agent via
    /// [`AvatarState::agent_of`]; `None`
    /// for an ordinary in-world object.
    pub wearer: Option<ScopedObjectId>,
}

/// Despawn every face child entity of a prim (used before rebuilding on a shape
/// change), leaving the caller to clear the tracked list.
pub fn despawn_prim_faces(face_entities: &[Entity], commands: &mut Commands) {
    for &face in face_entities {
        commands.entity(face).try_despawn();
    }
}

// ---------------------------------------------------------------------------
// Object-entity components the world's ingest path attaches, and the state the
// input / motion drivers keep. Each is *described* here and *produced* above in
// the world layer: the object update path lifts a light / particle / probe /
// physics block onto its component, and the movement, picking and HUD drivers
// own the resources.
// ---------------------------------------------------------------------------

/// A component marking an object entity as a **reflection probe**, carrying the
/// decoded `LLReflectionProbeParams` parameters (in Second Life semantics) plus
/// the prim's metre scale — the inputs the capture / volume side needs.
///
/// Attached to (and refreshed / cleared on) each object entity by
/// `apply_object` (the object ingest path) as its updates arrive. See
/// `reflection_probe_from_object` for the present-vs-absent lift.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct ObjectReflectionProbe {
    /// The decoded reflection-probe parameters: the ambiance (irradiance) scale,
    /// the reflection-capture near-clip distance in metres, and the flag set
    /// (box-vs-sphere volume, dynamic capture, mirror).
    pub data: ReflectionProbe,
    /// The prim's Second Life metre scale, refreshed every update so a **resized**
    /// probe's influence volume (a box of these half-extents, or a sphere of the
    /// bounding radius) stays correct. The reference viewer likewise derives the
    /// probe volume from the prim's dimensions, not from the probe params.
    pub scale: [f32; 3],
}

impl ObjectReflectionProbe {
    /// Whether this probe's influence volume is a **box** (the prim's oriented
    /// bounding box) rather than a **sphere** — the `BOX_VOLUME` flag, which the
    /// reference reads as `LLVOVolume::getReflectionProbeIsBox`.
    #[must_use]
    pub const fn is_box_volume(&self) -> bool {
        self.data.flags.contains(ReflectionProbeFlags::BOX_VOLUME)
    }

    /// Whether this probe drives a **realtime mirror** — the `MIRROR` flag, which the
    /// reference reads as `LLVOVolume::isMirror` to hand the prim to the hero-probe
    /// manager. A mirror is captured sharp and live (all six faces every frame,
    /// dynamic content included) by the `hero` path rather than the amortized
    /// P33 local pool; see the reflection-probe plugin.
    #[must_use]
    pub const fn is_mirror(&self) -> bool {
        self.data.flags.contains(ReflectionProbeFlags::MIRROR)
    }

    /// The influence volume as a scale for Bevy's unit-cube `LightProbe` volume,
    /// in the prim's **local** frame (the frame below the object entity, i.e. still
    /// Second Life axes — the object entity carries the basis change, exactly as the
    /// geometry holder's scale does).
    ///
    /// A **box** probe scales the unit cube by the prim's metre scale, so the volume
    /// is the prim's own oriented box (`LLReflectionMap::getBox`: half-extents
    /// `scale * 0.5`). A **sphere** probe has no cuboid counterpart in Bevy, so it
    /// becomes the smallest cube containing the reference's sphere — whose radius is
    /// `scale.x * 0.5`, the *X* extent alone (`LLReflectionMap::update`) — and the
    /// corners the cube adds beyond that sphere are taken back out by
    /// `SPHERE_FALLOFF`.
    #[must_use]
    pub const fn volume_scale(&self) -> Vec3 {
        let [x, y, z] = self.scale;
        if self.is_box_volume() {
            Vec3::new(x, y, z)
        } else {
            Vec3::splat(x)
        }
    }

    /// The `LightProbe` falloff (per axis, as a fraction of the volume) this
    /// probe's influence tapers over: a hard-edged [`BOX_FALLOFF`] for a box volume,
    /// the far softer `SPHERE_FALLOFF` for a sphere approximated by a cube.
    #[must_use]
    pub const fn falloff(&self) -> Vec3 {
        if self.is_box_volume() {
            Vec3::splat(BOX_FALLOFF)
        } else {
            Vec3::splat(SPHERE_FALLOFF)
        }
    }

    /// The probe's influence radius in metres, as `LLReflectionMap::update` computes
    /// it: the half-diagonal of the prim's box for a box volume, half the prim's *X*
    /// extent for a sphere. Used to rank probes by distance (the reference's
    /// `mDistance = |eye - origin| - radius`), so a large probe the camera is just
    /// outside of outranks a tiny one the same distance away.
    #[must_use]
    pub fn radius(&self) -> f32 {
        let [x, y, z] = self.scale;
        if self.is_box_volume() {
            Vec3::new(x * 0.5, y * 0.5, z * 0.5).length()
        } else {
            x * 0.5
        }
    }

    /// The near-clip distance the probe's capture cameras render with — the probe's
    /// own clip distance, floored at [`MIN_NEAR_CLIP`] the way
    /// `LLReflectionMap::getNearClip` floors it at `MINIMUM_NEAR_CLIP`. It is how a
    /// probe inside a room excludes the walls of the prim (or the furniture) it sits
    /// in from its own reflection.
    #[must_use]
    pub const fn near_clip(&self) -> f32 {
        self.data.clip_distance.max(MIN_NEAR_CLIP)
    }
}

/// Lift an object's reflection-probe block onto an `ObjectReflectionProbe`, or
/// `None` when the object is not (or is no longer) a probe.
///
/// Mirrors the reference viewer's `LLViewerObject::getReflectionProbeParams`: a
/// prim is a probe exactly when it carries a reflection-probe extra-param block, so
/// this is a straight `Option` lift with no sentinel to reject.
#[must_use]
pub fn reflection_probe_from_object(object: &Object) -> Option<ObjectReflectionProbe> {
    object
        .extra
        .reflection_probe
        .map(|data| ObjectReflectionProbe {
            data,
            scale: [object.scale.x, object.scale.y, object.scale.z],
        })
}

/// The smallest near-clip distance a probe's capture cameras may use, in metres —
/// `LLReflectionMap::getNearClip`'s `MINIMUM_NEAR_CLIP`.
pub const MIN_NEAR_CLIP: f32 = 0.1;

/// The `LightProbe` falloff of a **box**-volume local probe: the fraction of the
/// volume over which its influence tapers out toward the faces of the box. Small, so
/// a box probe's reflection fills the room it bounds (as the reference's box probes
/// do) and only blends out right at the boundary rather than fading across it.
pub const BOX_FALLOFF: f32 = 0.1;

/// The `LightProbe` falloff of a **sphere**-volume local probe. Bevy's influence
/// volume is always a cuboid, so a sphere probe is bound as the cube circumscribing
/// its sphere; a broad taper pulls the influence back in toward the sphere, so the
/// corners the cube adds contribute little.
pub const SPHERE_FALLOFF: f32 = 0.5;

/// The projector parameters of a **spotlight** — a light that carries a
/// light-image ([`LightImage`](sl_client_bevy::LightImage)) extra-param and so
/// projects a texture within a cone (`LLVOVolume::isLightSpotlight`). A plain
/// point light has none of this.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LightProjection {
    /// The projected texture id (`LLLightImageParams::getLightTexture`).
    pub texture: TextureKey,
    /// The projector cone field-of-view, in radians (`params.mV[0]`).
    pub fov: f32,
    /// The projector focus / blur (`params.mV[1]`).
    pub focus: f32,
    /// The projector ambiance — the diffuse spill outside the cone
    /// (`params.mV[2]`).
    pub ambiance: f32,
}

/// A component marking an object entity as a **light source**, carrying the
/// decoded `LLLightParams` (and, for a spotlight, `LLLightImageParams`)
/// parameters in Second Life semantics — ready for P25.2 to convert into a Bevy
/// `PointLight` / `SpotLight`.
///
/// Attached to (and refreshed / cleared on) each object entity by
/// `apply_object` (the object ingest path) as its updates arrive.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct ObjectLight {
    /// The light's **linear** RGB colour, each channel in `0.0..=1.0`. The wire
    /// bytes are the linear (not gamma-corrected) colour — Firestorm's
    /// `LLLightParams::unpack` feeds them straight into `setLinearColor` — so no
    /// sRGB decode is applied here.
    pub linear_color: [f32; 3],
    /// The light intensity in `0.0..=1.0` — the alpha channel of the wire colour
    /// (`LLVOVolume::getLightIntensity` reads `getLinearColor().mV[3]`). The
    /// effective emitted colour is `linear_color * intensity`.
    pub intensity: f32,
    /// The light radius, in metres (`LIGHT_MIN_RADIUS`..=`LIGHT_MAX_RADIUS`,
    /// i.e. `0.0..=20.0`).
    pub radius: f32,
    /// The falloff exponent (`LIGHT_MIN_FALLOFF`..=`LIGHT_MAX_FALLOFF`, i.e.
    /// `0.0..=2.0`): how sharply the light dims toward its radius.
    pub falloff: f32,
    /// The spotlight cutoff cone half-angle, in degrees
    /// (`LIGHT_MIN_CUTOFF`..=`LIGHT_MAX_CUTOFF`, i.e. `0.0..=180.0`). Sent for
    /// every light but only meaningful for a projector.
    pub cutoff: f32,
    /// The projector parameters when this is a **spotlight** (it carries a
    /// light-image block); `None` for a plain point light.
    pub projection: Option<LightProjection>,
}

impl ObjectLight {
    /// Whether this light is a **spotlight** (projector) rather than a plain
    /// point light — true exactly when it carries projector parameters, mirroring
    /// `LLVOVolume::isLightSpotlight` (a light-image block is present).
    #[must_use]
    pub const fn is_spotlight(&self) -> bool {
        self.projection.is_some()
    }

    /// The light's effective emitted linear colour: its base colour scaled by its
    /// intensity, mirroring `LLVOVolume::getLightLinearColor`
    /// (`color * color.mV[3]`).
    #[must_use]
    pub const fn effective_linear_color(&self) -> [f32; 3] {
        [
            self.linear_color[0] * self.intensity,
            self.linear_color[1] * self.intensity,
            self.linear_color[2] * self.intensity,
        ]
    }
}

/// Convert one wire colour byte to a normalized `0.0..=1.0` float. The workspace
/// denies `as` casts, so the widening goes through [`f32::from`].
fn channel(byte: u8) -> f32 {
    f32::from(byte) / 255.0
}

/// Decode an object's light extra-params into an [`ObjectLight`], or `None` if the
/// object is not a light source (it carries no `LLLightParams` block).
///
/// A spotlight additionally carries a light-image block; when present it becomes
/// the [`projection`](ObjectLight::projection).
#[must_use]
pub fn light_from_object(object: &Object) -> Option<ObjectLight> {
    let light: LightData = object.extra.light?;
    let projection = object
        .extra
        .light_image
        .as_ref()
        .map(|image| LightProjection {
            texture: image.texture,
            fov: image.params.x,
            focus: image.params.y,
            ambiance: image.params.z,
        });
    Some(ObjectLight {
        linear_color: [
            channel(light.color[0]),
            channel(light.color[1]),
            channel(light.color[2]),
        ],
        intensity: channel(light.color[3]),
        radius: light.radius,
        falloff: light.falloff,
        cutoff: light.cutoff,
        projection,
    })
}

/// Lift a live particle system off an object into an [`ObjectParticleSystem`], or
/// `None` when the object is not (or is no longer) a particle source.
///
/// Returns `None` in the two cases the reference viewer treats as "no source":
/// the object carries no particle-system block at all (`Object::particles` is
/// `None` — sl-proto already yields `None` for an empty `PSBlock`, matching
/// `isNullPS`'s zero-size check), or it carries a **null** system whose CRC is
/// zero (`LLPartSysData::isNullPS` — the `llParticleSystem([])` stop sentinel).
#[must_use]
pub fn particles_from_object(object: &Object) -> Option<ObjectParticleSystem> {
    let system = object.particles.clone()?;
    // A zero-CRC system is the reference viewer's "null" particle system: the
    // sentinel a script sends to stop emitting. `isNullPS` rejects it, so it is
    // not a live source.
    if system.crc == 0 {
        return None;
    }
    Some(ObjectParticleSystem { system })
}

/// The authoritative kinematic state of a server-flagged physical root prim as of
/// its last `ObjectUpdate`, attached to the object entity by `apply_physics` and
/// change-detected: a fresh insert on every update reseeds the interpolation. The
/// component is absent on any object that is not a physical root, so its presence
/// alone marks the entities `drive_physical_objects` gives a kinematic body.
#[derive(Component, Clone, Debug)]
pub struct PhysicalObject {
    /// The object's full (grid-wide) key — the id the `GetObjectPhysicsData`
    /// capability request and its reply use, and the key
    /// `ObjectPhysicsShapes` stores this object's physics data under.
    pub full_key: ObjectKey,
    /// Region-local position (metres, Second Life Z-up frame).
    pub position: Vector,
    /// Linear velocity (metres/second).
    pub velocity: Vector,
    /// Linear acceleration (metres/second²) — usually gravity for a falling prim.
    pub acceleration: Vector,
    /// Orientation (a Second Life unit quaternion).
    pub rotation: Rotation,
    /// Angular velocity (rotation axis scaled by radians/second).
    pub angular_velocity: Vector,
    /// The region this object lives in, for the region-edge / neighbour lookups.
    pub region_handle: RegionHandle,
    /// The object's size (metres per axis), the source for its cuboid collider.
    pub scale: Vector,
}

/// The evolving dead-reckoning prediction shared by the object
/// (`PhysicsInterp`) and avatar ([`AvatarInterp`]) motion drivers: the
/// extrapolated (predicted) region-local pose plus the motion state that
/// `advance_motion` steps forward each frame between authoritative server
/// updates. All of it is in Second Life space (Z-up, pre basis-change), so the
/// same math serves both paths — they differ only in the ground floor they apply
/// (permissive for objects, stricter for avatars).
#[derive(Debug)]
pub struct MotionState {
    /// The predicted region-local position (Second Life Z-up metres).
    pub position: [f32; 3],
    /// The predicted orientation, in Second Life space (pre basis-change).
    pub rotation: Quat,
    /// The current linear velocity (metres/second), decaying under the phase-out.
    pub velocity: [f32; 3],
    /// The current linear acceleration (metres/second²); zeroed on a region cross
    /// or an empty-edge clip, matching the reference viewer.
    pub acceleration: [f32; 3],
    /// The angular velocity (axis·radians/second).
    pub angular_velocity: [f32; 3],
    /// The object's / avatar's region, for the region-edge / neighbour lookups.
    pub region_handle: RegionHandle,
    /// While predicted to be crossing a border, the elapsed-seconds deadline after
    /// which motion is stopped (`mRegionCrossExpire`); `None` when not crossing.
    pub region_cross_expire: Option<f64>,
}

impl MotionState {
    /// Seed the prediction from an authoritative update's motion fields.
    #[must_use]
    pub fn new(
        position: &Vector,
        velocity: &Vector,
        acceleration: &Vector,
        rotation: &Rotation,
        angular_velocity: &Vector,
        region_handle: RegionHandle,
    ) -> Self {
        Self {
            position: vector_to_array(position),
            rotation: sl_rotation_to_quat(rotation),
            velocity: vector_to_array(velocity),
            acceleration: vector_to_array(acceleration),
            angular_velocity: vector_to_array(angular_velocity),
            region_handle,
            region_cross_expire: None,
        }
    }
}

/// A [`Vector`]'s components as a plain `[f32; 3]` for the per-component
/// dead-reckoning math (Bevy's `Vec3` arithmetic operators are forbidden by the
/// workspace `arithmetic_side_effects` lint).
const fn vector_to_array(vector: &Vector) -> [f32; 3] {
    [vector.x, vector.y, vector.z]
}

/// The Bevy-world orientation of a predicted motion: its Second Life-space rotation
/// composed with the Second Life → Bevy basis change, matching the root transform
/// `body_root_transform` (the avatar path) writes on an authoritative update.
#[must_use]
pub fn bevy_rotation_of(motion: &MotionState) -> Quat {
    sl_to_bevy_rotation().mul_quat(motion.rotation)
}

/// The viewer-side interpolation state for one avatar, owned entirely by
/// `drive_avatar_motion`: the shared dead-reckoning prediction plus the avatar's
/// ground-floor height and whether its anchor carries the object rotation. Unlike
/// the object path, this driver moves the anchor by the *delta* between successive
/// predictions, so the root-drop vertical render offset (R23, owned by
/// `apply_object` (the avatar path) and refreshed by the appearance path) is left
/// untouched.
#[derive(Debug, Component)]
pub struct AvatarInterp {
    /// The shared dead-reckoning prediction (pose + motion) advanced each frame.
    pub motion: MotionState,
    /// Elapsed seconds when the last server update was ingested.
    pub last_message_secs: f64,
    /// Elapsed seconds at the last interpolation step.
    pub last_interp_secs: f64,
    /// The avatar's bounding-box height, for the stricter ground floor.
    pub height: f32,
    /// Whether to write the predicted orientation onto the anchor (a rigged body).
    pub apply_rotation: bool,
    /// The orientation actually written to the anchor this frame (Bevy space), eased
    /// toward the authoritative / dead-reckoned facing each frame rather than snapped
    /// to it (P31.7). This decouples the rendered turn from the sparse authoritative
    /// rotation updates — the own avatar's facing arrives only as terse
    /// `ObjectUpdate`s echoing the client-driven `SetRotation` (P31.5), so without
    /// this easing a turn snaps between those updates while translation stays smooth.
    pub rendered_rotation: Quat,
    /// The anchor **translation** actually written this frame (Bevy space,
    /// including the R23 root-drop offset baked in by the avatar path),
    /// eased toward the authoritative / dead-reckoned position each update rather
    /// than snapped to it. This is the translation counterpart of
    /// [`rendered_rotation`](Self::rendered_rotation): on each terse `ObjectUpdate`
    /// the authoritative position jumps a little (fast motion, sparse updates), and
    /// snapping the anchor to it made the world visibly shake against a rigid
    /// follow camera — easing spreads the correction across a few frames. A
    /// region crossing / teleport still snaps (see `TRANSLATION_SNAP_DISTANCE_M`).
    pub rendered_translation: Vec3,
    /// The **authoritative / dead-reckoned** anchor translation (Bevy space, with
    /// the root-drop offset) that [`rendered_translation`](Self::rendered_translation)
    /// eases toward every frame: captured from the anchor on each server update and
    /// advanced by the prediction delta between updates. Tracking it separately (vs.
    /// easing only on update frames) is what lets a short teleport that leaves the
    /// avatar standing still converge fully to the destination instead of freezing
    /// part-way once updates stop arriving.
    pub target_translation: Vec3,
}

impl AvatarInterp {
    /// Seed the interpolation state from an authoritative update at time `now`,
    /// starting the eased translation at the anchor's current position `anchor`
    /// (already placed by the avatar path).
    #[must_use]
    pub fn seeded(motion: &AvatarMotion, now: f64, anchor: Vec3) -> Self {
        let motion_state = MotionState::new(
            &motion.position,
            &motion.velocity,
            &motion.acceleration,
            &motion.rotation,
            &motion.angular_velocity,
            motion.region_handle,
        );
        // Start the eased orientation at the authoritative facing so the avatar does
        // not visibly rotate into place from identity on its first frame.
        let rendered_rotation = bevy_rotation_of(&motion_state);
        Self {
            motion: motion_state,
            last_message_secs: now,
            last_interp_secs: now,
            height: motion.height,
            apply_rotation: motion.apply_rotation,
            rendered_rotation,
            rendered_translation: anchor,
            target_translation: anchor,
        }
    }

    /// Re-base the eased translation onto a moved scene origin: a region crossing
    /// (or a teleport to an already-connected region) shifts every origin-anchored
    /// entity by the same `delta`, so shift both the rendered and target
    /// translations to keep the avatar in the same world spot across the re-base
    /// (`recenter_avatars`). The region-local
    /// [`motion`](Self::motion) is unaffected — its dead-reckoned deltas are
    /// origin-invariant.
    pub fn rebase(&mut self, delta: Vec3) {
        // Per-component to avoid the `arithmetic_side_effects` lint on the glam
        // `Vec3` operator.
        self.rendered_translation.x += delta.x;
        self.rendered_translation.y += delta.y;
        self.rendered_translation.z += delta.z;
        self.target_translation.x += delta.x;
        self.target_translation.y += delta.y;
        self.target_translation.z += delta.z;
    }

    /// Re-seed the predicted pose to a fresh authoritative update at time `now`,
    /// snapping the prediction back to the server truth and restarting the timers.
    pub fn reseed(&mut self, motion: &AvatarMotion, now: f64) {
        self.motion = MotionState::new(
            &motion.position,
            &motion.velocity,
            &motion.acceleration,
            &motion.rotation,
            &motion.angular_velocity,
            motion.region_handle,
        );
        self.last_message_secs = now;
        self.last_interp_secs = now;
        self.height = motion.height;
        self.apply_rotation = motion.apply_rotation;
    }
}

/// The minimum interval, in seconds, between the body-rotation `AgentUpdate`s sent
/// while turning (~20 Hz), so a held turn key does not flood the circuit — the
/// heading still advances every frame client-side, it is just broadcast at this
/// rate.
pub const ROTATION_SEND_INTERVAL_SECS: f32 = 0.05;

/// The per-key state of the tap-tap-hold-to-run detector: how recently the key
/// was last tapped and whether a double-tap's run is currently latched (held).
#[derive(Debug, Clone)]
pub struct DoubleTapRun {
    /// Seconds since the key was last freshly pressed; starts beyond the window
    /// so the first tap of a session can never pair with "before the session".
    pub since_last_tap: f32,
    /// Whether the second tap of a double-tap is still held, running the avatar.
    pub latched: bool,
}

impl Default for DoubleTapRun {
    fn default() -> Self {
        Self {
            since_last_tap: f32::INFINITY,
            latched: false,
        }
    }
}

/// The persistent state of the avatar movement controls: the client-tracked walk
/// heading, whether flying is toggled on, and the bookkeeping that keeps the viewer
/// from re-sending an unchanged intent every frame.
#[derive(Debug, Resource)]
pub struct AvatarControls {
    /// The walk heading (yaw about the Second Life up axis, radians) the body faces;
    /// seeded once from the own avatar's reported facing so the first step does not
    /// snap it.
    pub yaw: f32,
    /// Whether flying is toggled on ([`ControlFlags::FLY`] is advertised).
    pub flying: bool,
    /// Whether `yaw` has been seeded from the own avatar yet.
    pub seeded: bool,
    /// Whether the seeded heading has been advertised to the simulator at least
    /// once, so a walk before the first turn moves in the right direction.
    pub sent_initial_rotation: bool,
    /// The control-flag set last advertised, so a [`Command::SetControls`] is emitted
    /// only when the flags actually change.
    pub last_controls: ControlFlags,
    /// Seconds accumulated since the last rotation send, for the turning throttle.
    pub rotation_send_accum: f32,
    /// Seconds the ascend key has been held while standing and not flying, for the
    /// P31.16 hold-to-take-off; reset whenever that precondition lapses.
    pub ascend_hold_secs: f32,
    /// The tap-tap-hold-to-run detector for the walk-forward key.
    pub tap_run_forward: DoubleTapRun,
    /// The tap-tap-hold-to-run detector for the walk-backward key.
    pub tap_run_backward: DoubleTapRun,
}

impl AvatarControls {
    /// The [`ControlFlags`] set last advertised to the simulator (walk / run /
    /// fly / ascend / descend). The client-side locomotion fallback
    /// (the `locomotion` module) reads the same advertised intent that moves the
    /// avatar to pick which built-in animation to play for immediate feedback.
    ///
    /// The set includes [`ControlFlags::FLY`] while flying is toggled on, so the
    /// locomotion fallback reads the fly / hover states straight off it.
    #[must_use]
    pub const fn advertised(&self) -> ControlFlags {
        self.last_controls
    }
}

impl Default for AvatarControls {
    fn default() -> Self {
        Self {
            yaw: 0.0,
            flying: false,
            seeded: false,
            sent_initial_rotation: false,
            last_controls: ControlFlags::empty(),
            rotation_send_accum: ROTATION_SEND_INTERVAL_SECS,
            ascend_hold_secs: 0.0,
            tap_run_forward: DoubleTapRun::default(),
            tap_run_backward: DoubleTapRun::default(),
        }
    }
}

/// Who input belongs to this frame.
///
/// Derived from `InputFocus` by `compute_input_context`; never assigned by
/// hand.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum InputContext {
    /// Nothing in the UI holds focus: the world has the keyboard and the mouse.
    ///
    /// The seam the camera / movement modes (mouselook, third-person, sitting —
    /// Firestorm's `keys.xml` modes) subdivide when they arrive.
    #[default]
    World,
    /// A focusable UI node that does not take text holds focus — a button, a
    /// checkbox. `Enter` / `Space` activate it, and the world gets no keys.
    UiWidget,
    /// A text-accepting node holds focus. Characters, the arrows and `Backspace`
    /// are all its; the world gets nothing.
    TextEntry,
    /// An in-world **media face** holds keyboard focus
    /// ([`MediaFocus`]): keys go to the embedded page, so
    /// the world gets nothing — the reference's `LLViewerMediaFocus` taking
    /// `gFocusMgr`'s keyboard focus.
    Media,
}

impl InputContext {
    /// Whether the world owns input right now.
    #[must_use]
    pub const fn is_world(self) -> bool {
        matches!(self, Self::World)
    }
}

/// The spawned HUD point nodes, keyed by raw attachment-point id, so an
/// attachment can be routed to the node for its point.
///
/// Empty when the run has no avatar assets (no `--viewer-assets`): the HUD point
/// offsets come from `avatar_lad.xml`, so without it there is no HUD screen and a
/// HUD attachment is hidden rather than routed (the same degradation that leaves
/// avatars as placeholder spheres).
#[derive(Resource, Debug, Default)]
pub struct HudState {
    /// The HUD point node entities, keyed by raw attachment-point id.
    pub points: HashMap<u8, Entity>,
}

impl HudState {
    /// The node entity a HUD attachment worn on `point_id` parents to, or `None`
    /// if there is no HUD screen (no avatar assets) or the id is not a HUD point.
    #[must_use]
    pub fn point_entity(&self, point_id: u8) -> Option<Entity> {
        self.points.get(&point_id).copied()
    }
}

/// Component-wise vector subtraction (`a - b`), avoiding the glam `-` operator the
/// workspace `arithmetic_side_effects` lint trips on.
#[must_use]
pub fn vsub(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.x - b.x, a.y - b.y, a.z - b.z)
}

/// Component-wise vector scaling (`v * s`).
#[must_use]
pub fn vscale(v: Vec3, s: f32) -> Vec3 {
    Vec3::new(v.x * s, v.y * s, v.z * s)
}

/// Build the [`SurfaceInfo`] a touch carries from a ray hit, the picked face, and
/// the touched object's world transform — the viewer's `LLPickInfo::getSurfaceInfo`.
///
/// - **Face** is the Linden face index the ray struck (`-1` when the hit is not on
///   a textured face, the reference's "no intersection" value).
/// - **ST** is the face's own `[0, 1]` surface coordinate: the mesh's stored
///   texture coordinate, un-flipped from the bottom-up→top-down convention this
///   viewer bakes into `ATTRIBUTE_UV_0` back into Second Life's bottom-up space.
/// - **UV** is `ST` with the face's texture placement (repeats / offset /
///   rotation, [`texture_face_uv_transform`]) applied — the coordinate as the
///   texture is actually sampled, matching the reference's `surfaceToTexture`.
/// - **Position / normal / binormal** are given in the object's own Second Life
///   frame (its global's inverse carries the world hit back into it). A HUD lives
///   in screen space with no meaningful region position, so the object-local
///   frame is the sensible finite choice; the reference instead reports region /
///   HUD-matrix coordinates, a deliberate simplification here. The binormal is
///   derived geometrically (perpendicular to the normal, along the hit triangle)
///   rather than from a texture tangent the ray hit does not carry.
#[must_use]
pub fn surface_info_from_hit(
    hit: &bevy::picking::mesh_picking::ray_cast::RayMeshHit,
    face_id: Option<PrimFaceId>,
    texture_face: Option<&TextureFace>,
    object_global: &GlobalTransform,
) -> SurfaceInfo {
    let inverse = object_global.affine().inverse();
    // The hit point / normal in the object's own Second Life frame (the object
    // subtree lives in Second Life space under the root's basis change, so the
    // inverse of its global lands here directly).
    let position = inverse.transform_point3(hit.point);
    let normal = inverse.transform_vector3(hit.normal).normalize_or_zero();

    // The binormal: perpendicular to the normal and along the surface, derived
    // from the hit triangle's first edge projected off the normal.
    let binormal = hit
        .triangle
        .map(|tri| {
            let edge = inverse.transform_vector3(vsub(tri[1], tri[0]));
            let along = vsub(edge, vscale(normal, edge.dot(normal)));
            normal.cross(along).normalize_or_zero()
        })
        .filter(|binormal| *binormal != Vec3::ZERO)
        .unwrap_or_else(|| normal.any_orthonormal_vector());

    // ST: the mesh's stored surface coordinate, back in Second Life bottom-up
    // space (this viewer flips `v` when building the Bevy mesh).
    let bevy_uv = hit.uv.unwrap_or(Vec2::ZERO);
    let st = Vec2::new(bevy_uv.x, 1.0 - bevy_uv.y);
    // UV: ST with the face's texture placement applied, as sampled — the
    // `uv_transform` acts in the Bevy (flipped) UV space, so flip back after.
    let placed = texture_face.map_or(bevy_uv, |tf| {
        texture_face_uv_transform(tf).transform_point2(bevy_uv)
    });
    let uv = Vec2::new(placed.x, 1.0 - placed.y);

    SurfaceInfo {
        uv: [uv.x, uv.y],
        st: [st.x, st.y],
        face_index: face_id.map_or(-1, |face| i32::from(face.get())),
        position: Vector {
            x: position.x,
            y: position.y,
            z: position.z,
        },
        normal: Vector {
            x: normal.x,
            y: normal.y,
            z: normal.z,
        },
        binormal: Vector {
            x: binormal.x,
            y: binormal.y,
            z: binormal.z,
        },
    }
}

/// Whether the pointer is over a **blocking** UI element — a hovered `bevy_ui`
/// node that occludes what is behind it.
///
/// A node **without** a [`Pickable`] component blocks by default in `bevy_ui`
/// (`should_block_lower` defaults to `true`) — and most pane content (the pane
/// column, the group-list body, the transcript text) has no explicit `Pickable`,
/// so it must count as blocking. Only nodes that opt **out** with an explicit
/// `Pickable { should_block_lower: false, .. }` — the full-window
/// UI root and the (empty) dock host — are transparent to the pick,
/// so an empty-UI click still touches the world / HUD through them.
///
/// A hovered entry only occludes if it is an **actual UI node with positive
/// area** — it has a [`ComputedNode`] whose laid-out size is non-zero. Two kinds
/// of hover-map entry are *not* a UI surface and must never suppress a world pick:
/// a hover entry that is not a `bevy_ui` node at all (it has no `ComputedNode`),
/// and a degenerate zero-area node (e.g. an empty, collapsed text node). Without
/// this guard such an entry — hovered everywhere, covering nothing — reported the
/// whole world as "blocked", silently killing every world pick (touch, and the
/// avatar context menu's body pick).
#[must_use]
pub fn pointer_over_blocking_ui(
    hover_map: &HoverMap,
    pickables: &Query<&Pickable>,
    sizes: &Query<&ComputedNode>,
) -> bool {
    hover_map
        .values()
        .flat_map(|hits| hits.keys())
        .any(|entity| {
            let blocks = pickables
                .get(*entity)
                .map_or(true, |pickable| pickable.should_block_lower);
            let has_area = sizes
                .get(*entity)
                .is_ok_and(|computed| computed.size().x > 0.0 && computed.size().y > 0.0);
            blocks && has_area
        })
}

// ---- moved down from the object layer (step 19) ----
/// The broad render classification of an in-world object, decided from its
/// `pcode` and sculpt/mesh extra parameters. It routes the object to the right
/// (later-phase) rendering path; P5.1 only records it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectCategory {
    /// An avatar (`pcode` 47) — a placeholder sphere in Phase 10.
    Avatar,
    /// A plain volume prim — tessellated with `sl_prim` in Phase 5.2.
    Prim,
    /// A sculpted prim (its shape comes from a sculpt texture) — Phase 9.
    Sculpt,
    /// A mesh object (its shape comes from a mesh asset) — Phase 7.
    Mesh,
    /// A Linden tree (`PCODE_TREE` / `PCODE_NEW_TREE`) — its branch / leaf
    /// geometry is generated procedurally from its species (P26.2).
    Tree,
    /// A Linden grass clump (`PCODE_GRASS`) — its crossed-quad blade geometry is
    /// generated procedurally from its species and scale (P26.3).
    Grass,
    /// Anything else (particle-system object, …); not rendered by the current
    /// phases.
    Other,
}

/// A marker component tagging an entity as an in-world object, carrying its
/// scoped id and render classification for the rendering phases to query — the
/// `pick_object` crosshair tool (both fields) and the `drive_render_priority`
/// prim LOD pass (P21.3, keyed off the classification and scoped id).
///
/// Both readers live in the object layer (`sl_viewer_world_objects`, modules
/// `objects` and `render_priority`), which depends on this crate rather than
/// the other way round, so they cannot be linked from here.
#[derive(Component, Debug, Clone, Copy)]
pub struct SceneObject {
    /// The object's scoped (circuit + region-local) id.
    pub scoped_id: ScopedObjectId,
    /// The object's render classification.
    pub category: ObjectCategory,
}

/// Debug identity carried on each object's root entity so the `pick_object`
/// crosshair tool (in the object layer) can report exactly what the camera is
/// looking at — the object's
/// full id, its mesh/sculpt asset id (the thing to fetch and decode offline when
/// its geometry looks wrong), and its Second Life scale/position.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct ObjectDebugInfo {
    /// The object's full (asset) id.
    full_id: Uuid,
    /// The mesh or sculpt-map asset id, when the object has one.
    asset: Option<Uuid>,
    /// The object's Second Life scale (metres per axis).
    scale: [f32; 3],
    /// The object's Second Life region-local position.
    position: [f32; 3],
    /// The object's quantized prim shape parameters, so a wrongly tessellated plain
    /// prim can be reproduced offline exactly as the simulator described it.
    shape: PrimShapeParams,
}

impl ObjectDebugInfo {
    /// The object's mesh or sculpt-map asset id, or `None` for a plain prim. Used
    /// by the P20.2 render-priority driver to rank a mesh object's still-fetching
    /// geometry (or a sculpt's map) from the object's on-screen size before its
    /// face entities exist.
    #[must_use]
    pub const fn render_asset(&self) -> Option<Uuid> {
        self.asset
    }

    /// Build the debug identity for an object's root entity from what the
    /// simulator described: its full id, its mesh / sculpt asset id if it has
    /// one, and its Second Life scale, position and prim shape.
    ///
    /// The fields stay private so the object layer records this identity
    /// through one call rather than reaching into five fields from another
    /// crate.
    #[must_use]
    pub const fn new(
        full_id: Uuid,
        asset: Option<Uuid>,
        scale: [f32; 3],
        position: [f32; 3],
        shape: PrimShapeParams,
    ) -> Self {
        Self {
            full_id,
            asset,
            scale,
            position,
            shape,
        }
    }

    /// The object's Second Life scale (metres per axis), whose half-diagonal is
    /// its bounding radius for the P20.2 pixel-area computation.
    #[must_use]
    pub const fn scale(&self) -> [f32; 3] {
        self.scale
    }

    /// The object's full (asset) id, as the crosshair pick tool reports it.
    #[must_use]
    pub const fn full_id(&self) -> Uuid {
        self.full_id
    }

    /// The object's Second Life region-local position.
    #[must_use]
    pub const fn position(&self) -> [f32; 3] {
        self.position
    }

    /// The object's quantized prim shape parameters, so a wrongly tessellated
    /// plain prim can be reproduced offline exactly as the simulator described
    /// it.
    #[must_use]
    pub const fn shape(&self) -> PrimShapeParams {
        self.shape
    }
}

/// Whether the own avatar is currently typing into local chat — driven by the
/// nearby-chat bar (`crate::nearby_chat_bar`) through [`set`](Self::set): active
/// while the bar is focused and holds a draft, inactive on send / blur.
#[derive(Debug, Resource, Default)]
pub struct TypingState {
    /// Whether typing is active this frame.
    active: bool,
    /// The `active` value last advertised to the simulator, so a `StartTyping` /
    /// `StopTyping` `ChatFromViewer` is emitted only on the *edge* rather than every
    /// frame — the simulator holds the state until the opposite signal arrives.
    advertised: bool,
}

impl TypingState {
    /// Whether the own avatar is typing this frame.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.active
    }

    /// Set the typing state (the nearby-chat bar calls this while a draft is being
    /// typed, and clears it on send / blur). The wire edge is reconciled by the
    /// object layer's typing driver, so this only records intent.
    pub const fn set(&mut self, active: bool) {
        self.active = active;
    }

    /// Take the un-advertised typing edge, if there is one: `Some(active)` the
    /// first time the state differs from what the simulator was last told, and
    /// `None` on every frame after. The simulator holds each state between
    /// signals, so re-sending every frame would flood the circuit.
    ///
    /// Taking the edge records it as advertised, so the caller must actually
    /// send the wire signals when this returns `Some`.
    pub const fn take_edge(&mut self) -> Option<bool> {
        if self.active == self.advertised {
            None
        } else {
            self.advertised = self.active;
            Some(self.active)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{
        PROVISIONAL_ID_CHARS, PatchKey, TerrainState, provisional_label, target_for,
        used_baked_slots,
    };
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{
        AgentKey, RegionHandle, ScriptLanguage, ScriptTarget, TerrainLayerType, TerrainPatch,
        TextureEntry, TextureFace, TextureKey, Uuid, avatar_texture, encode_texture_entry,
    };

    /// The region and grid position the terrain test patches use.
    const KEY: PatchKey = (RegionHandle(0), 1, 2);

    /// A single-patch map for the land patch of the given edge size whose height
    /// is `f(x, y)`, at [`KEY`].
    fn one_patch_map(
        size: u32,
        mut height: impl FnMut(u32, u32) -> f32,
    ) -> HashMap<PatchKey, TerrainPatch> {
        let mut values = Vec::new();
        for y in 0..size {
            for x in 0..size {
                values.push(height(x, y));
            }
        }
        let (region, patch_x, patch_y) = KEY;
        let patch = TerrainPatch {
            region_handle: region,
            layer: TerrainLayerType::Land,
            patch_x,
            patch_y,
            size,
            values,
        };
        let mut map = HashMap::new();
        map.insert(KEY, patch);
        map
    }

    #[test]
    fn per_region_revision_bumps_only_its_region() {
        let mut state = TerrainState::default();
        let a = RegionHandle(256_000);
        let b = RegionHandle(256_256);
        assert_eq!(state.region_revision(a), 0);
        assert_eq!(state.region_revision(b), 0);

        state.bump_revision(a);
        state.bump_revision(a);
        assert_eq!(state.region_revision(a), 2, "region a bumped twice");
        assert_eq!(state.region_revision(b), 0, "region b left untouched");

        let global_before = state.map_revision();
        state.bump_revision(b);
        assert_eq!(state.region_revision(b), 1);
        assert_eq!(state.region_revision(a), 2, "bumping b leaves a alone");
        assert!(
            state.map_revision() > global_before,
            "the global revision advances on any per-region bump"
        );
    }

    #[test]
    fn land_height_falls_back_to_the_retained_cache() {
        let mut state = TerrainState::default();
        // A patch whose height is a recognisable function of the cell.
        let map = one_patch_map(16, |x, y| {
            100.0 + f32::from(u16::try_from(x + y).unwrap_or(0))
        });
        // Only in the retained cache — the live patches are gone (a region mid-
        // rebuild, or not yet streamed after login), so `raw_patches` misses.
        state.land_cache = map;
        // Point (20.5, 35.5) is cell (4, 3) of the patch at grid (1, 2): height
        // 100 + (4 + 3) = 107. The cache answers even with no live patch.
        let height = state.land_height(RegionHandle(0), 20.5, 35.5);
        assert!(
            height.is_some_and(|height| (height - 107.0).abs() <= 1.0e-4),
            "land_height should fall back to the retained cache, got {height:?}"
        );
        // A region with no cached patch still returns `None` (nothing to stand on).
        assert!(
            state
                .land_height(RegionHandle(999_000), 20.5, 35.5)
                .is_none()
        );
    }

    /// The compile backend follows the item's recorded language, defaulting to
    /// Mono (SL's LSL default) for LSL or an unrecognised subtype.
    #[test]
    fn target_follows_language() {
        assert_eq!(target_for(Some(ScriptLanguage::Luau)), ScriptTarget::Luau);
        assert_eq!(target_for(Some(ScriptLanguage::Lsl)), ScriptTarget::Mono);
        assert_eq!(target_for(None), ScriptTarget::Mono);
    }

    /// The provisional tag is the agent id's leading hex fragment, so two distinct
    /// avatars read differently before their names resolve.
    #[test]
    fn provisional_label_is_a_short_id_fragment() {
        let agent = AgentKey::from(Uuid::from_u128(0x1234_5678_9abc));
        let label = provisional_label(agent);
        assert_eq!(label.chars().count(), PROVISIONAL_ID_CHARS);
        assert!(agent.uuid().simple().to_string().starts_with(&label));
    }

    /// A texture entry carrying an `IMG_USE_BAKED_*` sentinel yields that region's
    /// baked slot; an ordinary entry yields none.
    #[test]
    fn used_baked_slots_reads_the_sentinels() {
        let with_sentinel = TextureEntry {
            faces: vec![
                TextureFace::new(TextureKey::from(Uuid::from_u128(0x1234))),
                TextureFace::new(TextureKey::from(avatar_texture::IMG_USE_BAKED_UPPER)),
            ],
        };
        assert_eq!(
            used_baked_slots(&encode_texture_entry(&with_sentinel)),
            vec![avatar_texture::UPPER_BAKED]
        );

        let ordinary = TextureEntry {
            faces: vec![TextureFace::new(TextureKey::from(Uuid::from_u128(0x99)))],
        };
        assert!(used_baked_slots(&encode_texture_entry(&ordinary)).is_empty());
        // An empty blob decodes to no faces, so no slots.
        assert!(used_baked_slots(&[]).is_empty());
    }
}
