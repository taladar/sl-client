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

use std::collections::HashSet;

use bevy::prelude::*;
use sl_client_bevy::{ObjectKey, ObjectProperties, PrimFaceId, ScopedObjectId, Uuid};

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
