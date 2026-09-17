//! The in-world object graph.
//!
//! What the simulator streams about every prim in the scene, folded into one
//! entity per object: the identities, the entities, the shape fingerprints and
//! the wire-side blocks the edit, pick, physics, minimap and render-cost
//! surfaces all read. The *systems* that produce it — the tessellation, the
//! asset fetches, the deferred builds — stay in the world layer above.

use crate::object_flags::{
    FLAGS_ALLOW_INVENTORY_DROP, FLAGS_OBJECT_COPY, FLAGS_OBJECT_MODIFY, FLAGS_OBJECT_MOVE,
    FLAGS_OBJECT_YOU_OWNER, FLAGS_PHANTOM, is_hud_point,
};
use std::collections::{HashMap, HashSet};

use bevy::prelude::*;
use sl_client_bevy::{
    AgentKey, MeshKey, Object, ObjectExtraParams, ObjectKey, PrimLod, PrimShapeParams,
    RegionHandle, ScopedObjectId, SculptOrMeshKey, TextureAnimation, TextureKey, TreeLod, Uuid,
    pcode,
};

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
    /// The object-class byte this shape was fingerprinted from.
    ///
    /// The scene dump names an object's class the way the reference viewer
    /// spells it, and this is where the viewer already keeps the byte — the
    /// alternative being a second copy of it on every tracked object.
    #[must_use]
    pub const fn pcode(&self) -> u8 {
        self.pcode
    }

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
    /// The inventory item this attachment was worn from — its `AttachItemID`
    /// name-value ([`Object::attachment_item_id`]) — or `None` when it is not an
    /// attachment, or the simulator named none.
    ///
    /// It is not always an inventory item: a **temporary** attachment (one a
    /// script attached) has none, and the simulator repeats the object's own id
    /// instead. That equality is the only thing that tells a temp attachment
    /// apart, and the RLV admission test is the one caller that needs to
    /// ([`crate::rlv::is_temp_attachment`]), so the raw id is kept here rather than a
    /// derived flag.
    ///
    /// Retained across an update that carries no name-values at all, and
    /// dropped the moment the object stops being an attachment — the reference
    /// re-reads it on attach and nulls it on detach, and never in between.
    pub attachment_item: Option<Uuid>,
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
    /// and lives in the world's own `ObjectBuilds` component on the object entity.
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
    /// lockstep with [`crate::TerrainState`]'s origin (both follow
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
    /// purge by [`crate::AvatarState::purge`] (keyed
    /// by agent, so it does not flash), and the destination re-streams the object
    /// entity. Keeping it here would instead strand it as a ghost dot at the spot
    /// we left, because the same avatar is streamed by *every* connected region so
    /// no single copy is authoritative.
    ///
    /// Also drops the origin anchor so `recenter_objects` re-anchors on the
    /// destination without a spurious re-base shift.
    ///
    /// The purged objects' deferred builds go with the entities this despawns —
    /// they are the world's own `ObjectBuilds` component on each of them.
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
    /// again, so they are what a later un-derender re-fetches). Each removed
    /// object's deferred builds go with its entity — they are a component on it.
    /// Empty when `scoped` was not tracked.
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

    /// The **owner** and full [`ObjectKey`] of a tracked object, looked up by
    /// its region-scoped id — the two ids a mute entry can name. In-world
    /// collision sounds (`viewer-in-world-sounds`) resolve both in one lookup so
    /// a collision is tested against the same owner-or-object mute predicate the
    /// trigger and attached-sound paths use; they live on the same tracked
    /// object, so either both are known or neither is.
    #[must_use]
    pub fn owner_and_key_by_scoped(
        &self,
        scoped: &ScopedObjectId,
    ) -> Option<(AgentKey, ObjectKey)> {
        self.objects
            .get(scoped)
            .map(|tracked| (tracked.owner_id, tracked.full_key))
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
    #[must_use]
    pub fn agent_flags(&self, scoped: &ScopedObjectId) -> Option<u32> {
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
    /// *avatar* layer (drawn from [`crate::AvatarState`],
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

    /// The raw attachment-point id tracked object `scoped` is worn on — its own,
    /// or its attachment root's for a linked child prim — or `None` when it is
    /// not part of an attachment. HUD points included, unlike
    /// [`wearer_of`](Self::wearer_of): the caller decides what a point means.
    ///
    /// A worn rigged mesh draws from submeshes parented to its wearer's body
    /// rather than to the point's node, so what the point says about it (the
    /// first-person view hides a mesh head worn on the Skull) has to be looked
    /// up through the object, not the hierarchy.
    #[must_use]
    pub fn attachment_point_of(&self, scoped: ScopedObjectId) -> Option<u8> {
        let mut current = scoped;
        for _ in 0..MAX_PARENT_WALK {
            let tracked = self.objects.get(&current)?;
            if let Some(point) = tracked.attachment_point {
                return Some(point);
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

impl crate::world_scoped::WorldScoped for ObjectState {
    fn purge_world(&mut self, _purge: crate::world_scoped::WorldPurge, commands: &mut Commands) {
        self.purge(commands);
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
    /// [`crate::AvatarState::agent_of`]; `None`
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
