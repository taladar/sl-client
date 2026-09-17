//! What the build tool has selected and how it is editing it.
//!
//! The selection set, the active tool, the material / PBR channel the texture
//! tab is showing and the grid the handles snap to. `sl-viewer-edit` drives
//! all of it; the world layer reads it to draw the highlight, the beam and the
//! handles, and the menus read it to grey their entries.

use std::collections::HashSet;

use bevy::prelude::*;
use sl_client_bevy::{ObjectKey, ObjectProperties, PrimFaceId, ScopedObjectId, Uuid};

/// The Linden face a whole-object selection counts as last-touched — the
/// reference's `LLSelectNode::selectAllTEs` resetting `mLastTESelected` to `0`.
pub const FIRST_FACE: PrimFaceId = PrimFaceId::new(0);

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
    /// The **last face this node's selection touched** — the reference's
    /// `LLSelectNode::mLastTESelected`, set by every per-face select *and*
    /// deselect and reset to face `0` whenever the whole object is selected.
    ///
    /// It is the anchor the Texture tab's planar align measures from
    /// (`LLSelectedTE::getFace`, via `getLastSelectedTE`), which is why a
    /// deselect moves it too: the reference only asks whether the face it names
    /// is *still* selected, and falls back to the first face of the selection
    /// walk when it is not.
    pub last_face: PrimFaceId,
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
            last_face: FIRST_FACE,
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
            node.last_face = face;
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
                last_face: face,
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
                // The reference's `selectTE` moves `mLastTESelected` on a
                // deselect too; `getLastSelectedTE` then rejects it because the
                // face is no longer selected.
                node.last_face = face;
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
            last_face: face,
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
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MatModeState {
    /// Which material system the tab edits.
    pub matmedia: MatMedia,
    /// The Material-mode map channel.
    pub mat_type: MatChannel,
    /// The PBR-mode channel.
    pub pbr_type: PbrChannel,
}

/// Which material system the Texture tab edits — the reference's `mComboMatMedia`
/// selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MatMedia {
    /// The legacy **Material** (Blinn-Phong) mode — a diffuse texture plus
    /// optional normal / specular maps.
    #[default]
    Material,
    /// The **PBR** (GLTF) render-material mode.
    Pbr,
}

/// The Material-mode map channel a Blinn-Phong edit applies to — the
/// reference's `mRadioMaterialType` selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MatChannel {
    /// The diffuse **Texture** channel.
    #[default]
    Diffuse,
    /// The **Bumpiness** (normal-map) channel.
    Normal,
    /// The **Shininess** (specular-map) channel.
    Specular,
}

/// The active PBR texture channel a transform edits, or the whole material when
/// the render-material channel is selected — the reference's `mRadioPbrType`
/// selection, and what the PBR display path keys by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PbrChannel {
    /// The complete render material (its asset id), not a single texture.
    #[default]
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

/// The `matmedia` combo options, in the order they appear in the strip. The one
/// place the index↔mode mapping lives, like [`BUILD_TOOLS`].
pub(crate) const MAT_MEDIA_MODES: [MatMedia; 2] = [MatMedia::Material, MatMedia::Pbr];

/// The `radio_material_type` options, in the order they appear in the radio row.
pub(crate) const MATERIAL_CHANNELS: [MatChannel; 3] = [
    MatChannel::Diffuse,
    MatChannel::Normal,
    MatChannel::Specular,
];

/// The `radio_pbr_type` options, in the order they appear in the radio row.
pub(crate) const PBR_CHANNELS: [PbrChannel; 5] = [
    PbrChannel::Material,
    PbrChannel::BaseColor,
    PbrChannel::MetallicRoughness,
    PbrChannel::Emissive,
    PbrChannel::Normal,
];

impl MatMedia {
    /// This mode's index in the `matmedia` strip — the tab it selects.
    #[must_use]
    pub fn radio_index(self) -> usize {
        MAT_MEDIA_MODES
            .iter()
            .position(|&mode| mode == self)
            .unwrap_or(0)
    }

    /// The mode a strip tab index selects, defaulting to Material for an index
    /// the strip does not carry.
    #[must_use]
    pub fn from_radio_index(index: usize) -> Self {
        MAT_MEDIA_MODES.get(index).copied().unwrap_or_default()
    }
}

impl MatChannel {
    /// This channel's index in the material-type radio row.
    #[must_use]
    pub fn radio_index(self) -> usize {
        MATERIAL_CHANNELS
            .iter()
            .position(|&channel| channel == self)
            .unwrap_or(0)
    }

    /// The channel a radio index selects, defaulting to Diffuse for an index the
    /// row does not carry.
    #[must_use]
    pub fn from_radio_index(index: usize) -> Self {
        MATERIAL_CHANNELS.get(index).copied().unwrap_or_default()
    }
}

impl PbrChannel {
    /// This channel's index in the PBR-type radio row.
    #[must_use]
    pub fn radio_index(self) -> usize {
        PBR_CHANNELS
            .iter()
            .position(|&channel| channel == self)
            .unwrap_or(0)
    }

    /// The channel a radio index selects, defaulting to the whole render
    /// material for an index the row does not carry.
    #[must_use]
    pub fn from_radio_index(index: usize) -> Self {
        PBR_CHANNELS.get(index).copied().unwrap_or_default()
    }
}

impl MatModeState {
    /// Whether the Material (Blinn-Phong) mode is active.
    #[must_use]
    pub const fn is_material(self) -> bool {
        matches!(self.matmedia, MatMedia::Material)
    }

    /// Whether the PBR (GLTF) mode is active.
    #[must_use]
    pub const fn is_pbr(self) -> bool {
        matches!(self.matmedia, MatMedia::Pbr)
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

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{FIRST_FACE, SelectionSet};
    use bevy::prelude::Entity;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{
        CircuitId, ObjectKey, PrimFaceId, RegionLocalObjectId, ScopedObjectId, Uuid,
    };

    /// A node's `last_face` follows the reference's `mLastTESelected`: it starts
    /// at face `0` for a whole-object selection, moves to every picked face, and
    /// moves on an **un-pick** too — the planar align that reads it is what asks
    /// whether that face is still selected, not this.
    #[test]
    fn the_last_touched_face_follows_every_pick() {
        let entity = Entity::PLACEHOLDER;
        let scoped = ScopedObjectId {
            circuit: CircuitId::new(1),
            id: RegionLocalObjectId(9),
        };
        let full = ObjectKey::from(Uuid::from_u128(9));

        // A whole-object selection anchors on face 0.
        let mut set = SelectionSet::default();
        set.insert(scoped, full, entity);
        assert_eq!(set.primary().map(|node| node.last_face), Some(FIRST_FACE));

        // A plain face click moves it to that face.
        set.select_only_face(scoped, full, entity, PrimFaceId::new(3));
        assert_eq!(
            set.primary().map(|node| node.last_face),
            Some(PrimFaceId::new(3))
        );

        // So does a shift-click that adds a face...
        set.toggle_face(scoped, full, entity, PrimFaceId::new(5));
        assert_eq!(
            set.primary().map(|node| node.last_face),
            Some(PrimFaceId::new(5))
        );
        // ... and one that takes it away again.
        set.toggle_face(scoped, full, entity, PrimFaceId::new(5));
        assert_eq!(
            set.primary().map(|node| node.last_face),
            Some(PrimFaceId::new(5))
        );
        assert_eq!(
            set.primary_faces().map(HashSet::len),
            Some(1),
            "un-picking face 5 leaves only face 3 selected"
        );
    }
}
