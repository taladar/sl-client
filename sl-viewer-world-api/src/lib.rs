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

use std::collections::{BTreeMap, HashSet};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use sl_client_bevy::{
    AgentKey, AssetUpdateLocation, ChatSessionKind, Command, Friend, FriendKey, FriendPresence,
    FriendRights, GroupKey, GroupMembership, ImSessionId, InventoryKey, MuteEntry, MuteFlags,
    MuteType, Object, ObjectKey, ObjectProperties, ParticleSystem, PrimFaceId, RegionCoordinates,
    RegionHandle, RestoreItem, Rotation, ScopedObjectId, ScriptLanguage, ScriptTarget,
    ScriptUploadLocation, SlCommand, TaskInventoryKey, TextureKey, Uuid, Vector,
};
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

#[cfg(test)]
mod tests {
    use super::target_for;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{ScriptLanguage, ScriptTarget};

    /// The compile backend follows the item's recorded language, defaulting to
    /// Mono (SL's LSL default) for LSL or an unrecognised subtype.
    #[test]
    fn target_follows_language() {
        assert_eq!(target_for(Some(ScriptLanguage::Luau)), ScriptTarget::Luau);
        assert_eq!(target_for(Some(ScriptLanguage::Lsl)), ScriptTarget::Mono);
        assert_eq!(target_for(None), ScriptTarget::Mono);
    }
}
