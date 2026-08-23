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
//! This module is staging for a crate. It is being filled one connected cluster
//! of types at a time, each move small enough to compile and test on its own;
//! when nothing here reaches back into a feature, it becomes
//! `sl-viewer-world-api` and the world layer can follow it out.

use std::collections::{BTreeMap, HashSet};

use bevy::prelude::*;
use sl_client_bevy::{
    AgentKey, Command, Friend, FriendKey, FriendPresence, FriendRights, GroupKey, GroupMembership,
    MuteEntry, ObjectKey, ObjectProperties, PrimFaceId, ScopedObjectId, SlCommand, TextureKey,
    Uuid,
};

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

    /// Remove every selected object with the persistent id `id` (a no-op if
    /// absent) — the derender path (`viewer-derender-blacklist`), which knows a
    /// full id rather than a region-scoped one, dropping an object it is about
    /// to despawn out of the selection first (the reference's `stopEditing` on
    /// a derendered edit target).
    pub(crate) fn remove_by_full_id(&mut self, id: Uuid) {
        self.selected.retain(|node| node.full.uuid() != id);
    }

    /// The selected nodes, in selection order.
    ///
    /// Paired with [`Self::replace_nodes`] for logic that has to rebuild the
    /// selection from world knowledge this layer deliberately lacks — see
    /// `edit_selection::promote_selection_to_roots`.
    pub(crate) fn nodes(&self) -> &[SelectedNode] {
        &self.selected
    }

    /// Replace the selection wholesale, keeping the last entry primary.
    pub(crate) fn replace_nodes(&mut self, nodes: Vec<SelectedNode>) {
        self.selected = nodes;
    }

    /// The tentative rubber-band sweep, for the drag that owns it.
    pub(crate) const fn rect_pending_mut(&mut self) -> &mut Vec<(ScopedObjectId, Entity)> {
        &mut self.rect_pending
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
    pub(crate) fn apply_properties(&mut self, properties: Box<ObjectProperties>) -> bool {
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
pub(crate) enum EditTool {
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
    /// ([`crate::edit_texture`]) edits, `Shift`-click builds a multi-face set.
    SelectFace,
    /// The **Create** tool (the reference's `LLToolPlacer` / `LLToolCompCreate`):
    /// no transform gizmo — a click on a surface rezzes the base type picked in
    /// the create panel ([`crate::edit_create`]) at the ray-cast build point and
    /// drops into edit on the new object.
    Create,
}

impl EditTool {
    /// This tool's index into [`BUILD_TOOLS`] — the radio option it selects.
    pub(crate) fn radio_index(self) -> usize {
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
pub(crate) struct EditToolState {
    /// Whether the build tool is active (the floater is open): selection
    /// clicks, gizmos, and the touch-suppression all key off this.
    pub(crate) active: bool,
    /// The manipulator picked in the floater (the resting tool).
    pub(crate) tool: EditTool,
    /// A manipulator temporarily forced by a held modifier — the reference's
    /// `Ctrl` = rotate / `Ctrl+Shift` = stretch while held
    /// (`LLToolCompTranslate::handleHover`'s mask dispatch). Cleared on
    /// release; [`effective_tool`](Self::effective_tool) folds it in.
    pub(crate) held_override: Option<EditTool>,
    /// Edit linked parts: select and edit individual linkset prims instead of
    /// whole linksets (the reference's `EditLinkedParts`).
    pub(crate) edit_linked: bool,
    /// Stretch both sides: scale about the selection centre instead of
    /// holding the opposite face in place (the reference's `ScaleUniform`).
    pub(crate) stretch_both: bool,
    /// Whether grid snapping is on (the reference's `SnapEnabled`).
    pub(crate) snap: bool,
    /// The grid unit, in metres (the reference's `GridResolution`).
    pub(crate) grid_unit: f32,
    /// The grid frame the gizmos align to.
    pub(crate) frame: GridFrame,
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
    pub(crate) fn effective_tool(&self) -> EditTool {
        self.held_override.unwrap_or(self.tool)
    }
}

/// The current material mode / channel the Texture tab edits — the resolved
/// `(matmedia, material-type, pbr-type)` selection, mirrored from the three
/// selector widgets each frame so the visibility system and the channel editors
/// read one place. Mirrors the reference's `mComboMatMedia` /
/// `mRadioMaterialType` / `mRadioPbrType` current indices.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MatModeState {
    /// The `matmedia` selection ([`MATMEDIA_MATERIAL`] / [`MATMEDIA_PBR`]).
    pub(crate) matmedia: usize,
    /// The Material-mode map channel ([`MATTYPE_DIFFUSE`] / [`MATTYPE_NORMAL`] /
    /// [`MATTYPE_SPECULAR`]).
    pub(crate) mat_type: usize,
    /// The PBR-mode channel ([`PBRTYPE_RENDER_MATERIAL`] …).
    pub(crate) pbr_type: usize,
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
pub(crate) enum PbrChannel {
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
    pub(crate) const fn is_material(self) -> bool {
        self.matmedia == MATMEDIA_MATERIAL
    }

    /// Whether the PBR (GLTF) mode is active.
    pub(crate) const fn is_pbr(self) -> bool {
        self.matmedia == MATMEDIA_PBR
    }

    /// The active PBR channel for the current `pbr_type` selection.
    pub(crate) const fn pbr_channel(self) -> PbrChannel {
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
pub(crate) const DEFAULT_GRID_UNIT: f32 = 0.5;

/// The tool-mode radio options, in the order they appear in the floater (the
/// reference's `move` / `rotate` / `stretch`). The one place the index↔tool
/// mapping lives, so [`spawn_build_floater`] and the two sync systems agree.
pub(crate) const BUILD_TOOLS: [EditTool; 5] = [
    EditTool::Create,
    EditTool::Move,
    EditTool::Rotate,
    EditTool::Stretch,
    EditTool::SelectFace,
];

/// The grid frame the gizmos align to and snap in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum GridFrame {
    /// The world axes (the reference's `GRID_MODE_WORLD`).
    #[default]
    World,
    /// The primary selection's own axes (`GRID_MODE_LOCAL`).
    Local,
    /// A reference object's axes (`GRID_MODE_REF_OBJECT`). Modelled now so the
    /// snapping code handles it, but only settable once the grid-options task
    /// (`viewer-build-grid-options`) ships its *Use Selection for Grid*
    /// command.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "the reference-object grid frame is set by the grid-options task \
                      (viewer-build-grid-options); the frame model carries it from the start"
        )
    )]
    Reference,
}

/// The `matmedia` combo index for the legacy **Material** (Blinn-Phong) mode —
/// diffuse texture plus optional normal / specular maps.
pub(crate) const MATMEDIA_MATERIAL: usize = 0;

/// The `matmedia` combo index for the **PBR** (GLTF) render-material mode.
pub(crate) const MATMEDIA_PBR: usize = 1;

/// The `radio_material_type` index for the diffuse **Texture** channel.
pub(crate) const MATTYPE_DIFFUSE: usize = 0;

/// The `radio_material_type` index for the **Bumpiness** (normal-map) channel.
pub(crate) const MATTYPE_NORMAL: usize = 1;

/// The `radio_material_type` index for the **Shininess** (specular-map) channel.
pub(crate) const MATTYPE_SPECULAR: usize = 2;

/// The `radio_pbr_type` index for the whole render **material** (the material-id
/// swatch — assign or clear a stored GLTF material asset).
pub(crate) const PBRTYPE_RENDER_MATERIAL: usize = 0;

/// The `radio_pbr_type` index for the PBR **base-colour** channel transform.
pub(crate) const PBRTYPE_BASE_COLOR: usize = 1;

/// The `radio_pbr_type` index for the PBR **metallic-roughness** channel
/// transform.
pub(crate) const PBRTYPE_METALLIC: usize = 2;

/// The `radio_pbr_type` index for the PBR **emissive** channel transform.
pub(crate) const PBRTYPE_EMISSIVE: usize = 3;

/// The `radio_pbr_type` index for the PBR **normal** channel transform.
pub(crate) const PBRTYPE_NORMAL: usize = 4;

/// The most entries the mute list holds — the reference's `MuteListLimit`
/// debug setting, whose default this matches. A mute past the limit is
/// refused client-side (the server silently drops it) and reported as
/// `MuteLimitReached`.
pub(crate) const MUTE_LIST_LIMIT: usize = 1000;

/// The agent's mute list: every muted entry (agents and objects alike — the
/// tag colouring only ever looks up agent ids).
#[derive(Resource, Debug, Default)]
pub(crate) struct MuteModel {
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
    pub(crate) const fn claim_request(&mut self) -> bool {
        if self.requested {
            return false;
        }
        self.requested = true;
        true
    }

    /// Whether `id` is on the mute list at all (any aspect).
    pub(crate) fn is_muted(&self, id: Uuid) -> bool {
        self.muted.contains(&id)
    }

    /// Whether the aspect whose *exception* bit is `allow_mask` (one of the
    /// `MuteFlags::ALLOW_*` constants) is actually muted for `id`: the id is on
    /// the list **and** the entry does not carry that exception.
    pub(crate) fn is_muted_aspect(&self, id: Uuid, allow_mask: u32) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.id == id && !entry.flags.contains(allow_mask))
    }

    /// The whole list, in display order.
    pub(crate) fn entries(&self) -> &[MuteEntry] {
        &self.entries
    }

    /// The list revision — a view stores the value it last built at and
    /// rebuilds when it advances.
    pub(crate) const fn revision(&self) -> u64 {
        self.revision
    }

    /// Whether the list is at [`MUTE_LIST_LIMIT`] and refuses further mutes.
    pub(crate) const fn is_full(&self) -> bool {
        self.entries.len() >= MUTE_LIST_LIMIT
    }

    /// Whether a **by-name** entry already carries `name` (case-insensitively)
    /// — the duplicate check a by-name block needs, since such entries share a
    /// nil id and nothing else tells them apart. Entries with an id are not
    /// consulted: the reference keeps its by-name mutes in a separate set, so
    /// blocking an object *by name* is allowed even when a same-named avatar is
    /// blocked by id.
    pub(crate) fn has_by_name(&self, name: &str) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.id.is_nil() && entry.name.eq_ignore_ascii_case(name))
    }

    /// The entry matching `id` / `name`, if any (see the module docs for how a
    /// nil id falls back to the name).
    pub(crate) fn entry(&self, id: Uuid, name: &str) -> Option<&MuteEntry> {
        self.entries
            .iter()
            .find(|entry| same_target(entry, id, name))
    }

    /// Record a locally-issued mute so consumers update without waiting for a
    /// list re-request. An existing entry for the same target is **replaced**
    /// (that is how a flag edit lands, since it re-sends the whole entry).
    pub(crate) fn note_mute(&mut self, entry: MuteEntry) {
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
    pub(crate) fn note_unmute(&mut self, id: Uuid, name: &str) {
        self.entries.retain(|entry| !same_target(entry, id, name));
        self.reindex();
    }

    /// Replace the whole list (a received `MuteList`).
    pub(crate) fn replace(&mut self, entries: Vec<MuteEntry>) {
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
/// digits (mirrors [`crate::conversations`]'s placeholder).
pub(crate) fn short_id(id: Uuid) -> String {
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
pub(crate) struct FriendsModel {
    /// The buddy list, by friend id.
    friends: BTreeMap<FriendKey, FriendEntry>,
    /// Last-seen legacy display name per agent, for the row labels.
    names: BTreeMap<AgentKey, String>,
    /// The name the user gave a friend instead, if any (already quoted, as the
    /// name cache shows it) — mirrored from the contact-set store by
    /// [`crate::contact_sets::apply_name_aliases`]. Kept beside the resolved
    /// names rather than over them: a wire action still needs the real one.
    aliases: BTreeMap<AgentKey, String>,
    /// Bumped on each mutation; the view compares its last-built value to skip an
    /// unchanged rebuild.
    revision: u64,
}

impl FriendsModel {
    /// Bump the revision after a mutation.
    pub(crate) const fn touch(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    /// Merge a buddy-list record set (login `FriendList`), keeping any presence
    /// already learned for a friend that is being refreshed.
    pub(crate) fn note_friends(&mut self, friends: &[Friend]) {
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
    pub(crate) fn apply_snapshot(&mut self, presence: &[FriendPresence]) {
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
    pub(crate) fn set_online(&mut self, friends: &[FriendKey], online: bool) {
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

    /// Update one friend's rights from a [`SlSessionEvent::FriendRightsChanged`]:
    /// `granted_to_us` distinguishes the rights the friend now grants us from a
    /// server echo of the rights we grant them.
    pub(crate) fn update_rights(
        &mut self,
        friend: FriendKey,
        rights: FriendRights,
        granted_to_us: bool,
    ) {
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
    pub(crate) fn remove(&mut self, friend: FriendKey) {
        if self.friends.remove(&friend).is_some() {
            self.touch();
        }
    }

    /// Record a resolved legacy name for an agent (ignoring empties).
    pub(crate) fn note_name(&mut self, id: AgentKey, name: &str) {
        if !name.is_empty() && self.names.get(&id).map(String::as_str) != Some(name) {
            self.names.insert(id, name.to_owned());
            self.touch();
        }
    }

    /// The resolved name for an agent, if known — the **grid's** answer, which
    /// is what a wire action (a mute entry) has to carry.
    pub(crate) fn name_of(&self, id: AgentKey) -> Option<&str> {
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
    /// [`crate::contact_sets::apply_name_aliases`] is the caller.
    pub(crate) fn set_name_aliases(&mut self, aliases: BTreeMap<AgentKey, String>) {
        if self.aliases == aliases {
            return;
        }
        self.aliases = aliases;
        self.touch();
    }

    /// The model revision — a consumer that mirrors the roster (the friends-only
    /// render filter, [`crate::derender`]) compares its last-mirrored value to
    /// skip an unchanged rebuild.
    pub(crate) const fn revision(&self) -> u64 {
        self.revision
    }

    /// Every friend's agent id. The friends-only render filter mirrors this by
    /// revision so its per-avatar gate — which runs for every streamed object at
    /// a crowded event — stays a single hash lookup.
    pub(crate) fn friend_ids(&self) -> std::collections::HashSet<Uuid> {
        self.friends
            .keys()
            .map(|id| AgentKey::from(*id).uuid())
            .collect()
    }

    /// Whether `agent` is already in the buddy cache — a friend.
    ///
    /// The avatar context menu reads this to disable "Add as Friend" for someone
    /// who already is one, matching the reference viewer's `on_enable`.
    pub(crate) fn is_friend(&self, agent: AgentKey) -> bool {
        self.friends.contains_key(&FriendKey::from(agent.uuid()))
    }

    /// Whether `agent` is a friend the grid last reported **online**. Someone
    /// who is not a friend at all is not online as far as this model knows — the
    /// buddy cache is the only presence the protocol gives us.
    pub(crate) fn is_online(&self, agent: AgentKey) -> bool {
        self.friends
            .get(&FriendKey::from(agent.uuid()))
            .is_some_and(|entry| entry.online)
    }

    /// The whole roster as `(agent, display label)` pairs, name order — the
    /// avatar picker's Friends tab reads this. A friend whose name has not
    /// resolved yet labels as a provisional id fragment.
    pub(crate) fn roster(&self) -> Vec<(AgentKey, String)> {
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
    pub(crate) fn unnamed(&self) -> Vec<AgentKey> {
        self.friends
            .keys()
            .map(|id| AgentKey::from(*id))
            .filter(|agent| !self.names.contains_key(agent))
            .collect()
    }

    /// The render-ready row list, in map order. The table sorts it through
    /// its own [`SortState`](crate::people); the model has no opinion on
    /// display order.
    pub(crate) fn rows(&self) -> Vec<FriendRow> {
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
    pub(crate) fn granted_rights(&self, friend: FriendKey) -> Option<FriendRights> {
        self.friends.get(&friend).map(|entry| entry.rights_granted)
    }

    /// Optimistically set the rights this agent grants `friend` (so a toggled
    /// checkbox flips immediately; the server echo re-confirms the same value).
    pub(crate) fn set_granted(&mut self, friend: FriendKey, rights: FriendRights) {
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
pub(crate) struct FriendRow {
    /// The friend id (for remove / grant-rights, which take a [`FriendKey`]).
    pub(crate) friend: FriendKey,
    /// The agent id (for IM / teleport / mute, which take an [`AgentKey`]).
    pub(crate) agent: AgentKey,
    /// The display name (or a short-id placeholder until the name resolves).
    pub(crate) name: String,
    /// Whether the friend is currently known-online.
    pub(crate) online: bool,
    /// The rights this agent grants the friend (the "They can …" columns).
    pub(crate) rights_granted: FriendRights,
    /// The rights the friend grants this agent (the "You can …" columns).
    pub(crate) rights_received: FriendRights,
}

/// The pure groups model: the agent's group memberships keyed by group id (to its
/// display name), the active (worn) group, and a revision stamp bumped on every
/// change so the view rebuilds only when something actually moved. Fed solely from
/// the event stream. The list and its actions need only the name; the membership
/// record's powers / contribution belong to the (out-of-scope) profile.
#[derive(Resource, Debug, Default)]
pub(crate) struct GroupsModel {
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
    /// ([`crate::group_notice`]) reads the notice's group image from.
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
    /// ([`SlSessionEvent::GroupMemberships`]) — the wire message carries the
    /// agent's **full** group list, so it is authoritative and replaces the cache
    /// wholesale. The active group is left untouched (it is tracked separately from
    /// [`SlSessionEvent::ActiveGroupChanged`]).
    pub(crate) fn apply_memberships(&mut self, memberships: &[GroupMembership]) {
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
    /// ([`crate::group_notice`]) reads it to show the notice's group image. A nil
    /// texture (a group with no insignia) is reported as `None`.
    pub(crate) fn group_insignia(&self, group: GroupKey) -> Option<TextureKey> {
        self.insignia
            .get(&group)
            .copied()
            .filter(|key| *key != TextureKey::from(Uuid::nil()))
    }

    /// Whether the agent accepts notices from `group`, if the agent is a member —
    /// the group profile floater's membership toggle seeds from this (the
    /// login-time value is not otherwise available to a floater opened later).
    pub(crate) fn accepts_notices(&self, group: GroupKey) -> Option<bool> {
        self.accept_notices.get(&group).copied()
    }

    /// The display name of `group` — the agent's own membership name, else a
    /// name resolved on demand ([`note_resolved_name`](Self::note_resolved_name)),
    /// else `None` (the caller falls back to the id and can request a resolve).
    pub(crate) fn group_name(&self, group: GroupKey) -> Option<&str> {
        self.groups
            .get(&group)
            .or_else(|| self.resolved.get(&group))
            .map(String::as_str)
    }

    /// Whether the agent is a member of `group` — a membership test that, unlike
    /// [`group_name`](Self::group_name), does **not** consider the on-demand
    /// resolved-name cache (a resolved non-member group must not read as a member).
    pub(crate) fn is_member(&self, group: GroupKey) -> bool {
        self.groups.contains_key(&group)
    }

    /// Request `group`'s name (`UUIDGroupNameRequest`) if it is not already known
    /// — the shared resolve path every group-name display site uses so a
    /// non-member group's name fills the cache instead of showing a UUID forever.
    /// Call at a discrete event (a floater open, a selection change), not per
    /// frame; the reply folds into the [`resolved`](Self::resolved) cache.
    pub(crate) fn request_name(&self, group: GroupKey, commands: &mut MessageWriter<SlCommand>) {
        if self.group_name(group).is_none() {
            commands.write(SlCommand(Command::RequestGroupNames(vec![group])));
        }
    }

    /// Fold a resolved name for a non-member `group` into the on-demand cache.
    /// Public so any group-name display site can seed the shared cache from a
    /// name it learned (an IM session, a profile) rather than keeping its own.
    pub(crate) fn note_resolved_name(&mut self, group: GroupKey, name: &str) {
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
    pub(crate) fn group_ids(&self) -> Vec<GroupKey> {
        self.groups.keys().copied().collect()
    }

    /// Set the active (worn) group, bumping the revision only on a real change.
    pub(crate) fn set_active(&mut self, active: Option<GroupKey>, title: &str) {
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
    pub(crate) const fn revision(&self) -> u64 {
        self.revision
    }

    /// only refreshes when the simulator re-streams the avatar object).
    pub(crate) fn own_title(&self) -> Option<&str> {
        self.own_title.as_deref()
    }

    /// Drop a group the agent is no longer in (left, ejected, or dissolved),
    /// clearing the active marker if it was the active group.
    pub(crate) fn remove(&mut self, group: GroupKey) {
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
    pub(crate) fn ordered(&self) -> Vec<GroupRow> {
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
    pub(crate) fn len(&self) -> usize {
        self.groups.len()
    }

    /// The display name for a group, if known (for the leave-confirm prompt).
    pub(crate) fn name_of(&self, group: GroupKey) -> Option<&str> {
        self.groups.get(&group).map(String::as_str)
    }
}

/// One render-ready group row: the id its actions need, the display name, and
/// whether it is the active (worn) group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GroupRow {
    /// The group id (for every action).
    pub(crate) group: GroupKey,
    /// The display name (or a short-id placeholder for an unnamed group).
    pub(crate) name: String,
    /// Whether this is the agent's active (worn) group.
    pub(crate) active: bool,
}

/// How long the away state must have held before input clears it (the
/// reference's `LLAgent::MIN_AFK_TIME`) — without it, the mouse move that
/// happens to arrive one frame after the auto-AFK fires would cancel it.
pub(crate) const MIN_AFK_SECS: f32 = 10.0;

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
pub(crate) struct PresenceState {
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
    pub(crate) const fn tick(&mut self, dt: f32) {
        self.idle_secs += dt;
        if self.away {
            self.away_secs += dt;
        }
    }

    /// Seconds since the last user input.
    pub(crate) const fn idle_secs(&self) -> f32 {
        self.idle_secs
    }

    /// Seconds the away state has held.
    pub(crate) const fn away_secs(&self) -> f32 {
        self.away_secs
    }

    /// Restart the idle clock alone — there is no session to be away in yet,
    /// so the away clock is not the caller's business.
    pub(crate) const fn reset_idle(&mut self) {
        self.idle_secs = 0.0;
    }

    /// The away state if it differs from what was last advertised, marking it
    /// advertised in the same step; `None` when the wire already agrees. Read
    /// and mark cannot be separated, or a failed send would leave the two
    /// permanently out of step.
    pub(crate) const fn take_away_edge(&mut self) -> Option<bool> {
        if self.away == self.advertised_away {
            return None;
        }
        self.advertised_away = self.away;
        Some(self.away)
    }

    /// The Do Not Disturb state on the same terms as [`Self::take_away_edge`].
    pub(crate) const fn take_dnd_edge(&mut self) -> Option<bool> {
        if self.do_not_disturb == self.advertised_dnd {
            return None;
        }
        self.advertised_dnd = self.do_not_disturb;
        Some(self.do_not_disturb)
    }

    /// Whether *we* sat the avatar down on going away.
    pub(crate) const fn sat_on_away(&self) -> bool {
        self.sat_on_away
    }

    /// Record whether we sat the avatar down on going away.
    pub(crate) const fn set_sat_on_away(&mut self, sat: bool) {
        self.sat_on_away = sat;
    }

    /// Whether the avatar is away.
    #[must_use]
    pub(crate) const fn is_away(&self) -> bool {
        self.away
    }

    /// Whether Do Not Disturb is on.
    #[must_use]
    pub(crate) const fn is_do_not_disturb(&self) -> bool {
        self.do_not_disturb
    }

    /// Set the away state, restarting the away clock on a rising edge. The wire
    /// writes are reconciled by [`advertise_presence`].
    pub(crate) const fn set_away(&mut self, away: bool) {
        if self.away != away {
            self.away = away;
            self.away_secs = 0.0;
        }
    }

    /// Set the Do Not Disturb state. The wire writes and the toast queue's
    /// drain are reconciled by [`advertise_presence`] and the hosts that read
    /// [`is_do_not_disturb`](Self::is_do_not_disturb).
    pub(crate) const fn set_do_not_disturb(&mut self, busy: bool) {
        self.do_not_disturb = busy;
    }

    /// Note user input: reset the idle clock and, once away has held long
    /// enough to be real, clear it (the reference's `MIN_AFK_TIME` debounce).
    pub(crate) fn note_activity(&mut self) {
        if self.away && self.away_secs > MIN_AFK_SECS {
            self.set_away(false);
        }
        self.idle_secs = 0.0;
    }
}

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
