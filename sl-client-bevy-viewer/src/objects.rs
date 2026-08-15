//! Object lifecycle: fold the session's object stream into a Bevy scene graph,
//! one entity per in-world object, kept current across adds, updates, and
//! removes.
//!
//! This is the Phase 5.1 slice — the lifecycle skeleton the later rendering
//! phases hang geometry on:
//!
//! - each [`SlSessionEvent::ObjectAdded`] spawns an entity, tagged with a
//!   [`SceneObject`] marker classifying it (avatar / mesh / sculpt / plain prim /
//!   other), and its `Transform` set from the object's kinematic
//!   [`motion`](sl_client_bevy::ObjectMotion) and scale via the Second Life →
//!   Bevy [coordinate map](crate::coords);
//! - each [`SlSessionEvent::ObjectUpdated`] moves the existing entity (a
//!   motion-only update just re-places it) and, only when the object's *shape*
//!   parameters actually change, re-tessellates its geometry (a motion update
//!   never re-tessellates);
//! - linkset children are parented to their root entity so the root's transform
//!   carries the whole set; a child that arrives before its root is held
//!   parentless and re-parented once the root appears;
//! - each [`SlSessionEvent::ObjectRemoved`] despawns the entity (and, via Bevy's
//!   hierarchy, its parented children — including the face meshes) and drops it —
//!   and any tracked descendants — from the map.
//!
//! Since Phase 5.2 a plain prim ([`ObjectCategory::Prim`]) is tessellated with
//! [`sl_prim`](sl_client_bevy) at a fixed high level of detail and rendered as
//! one child entity per [`PrimFace`](sl_client_bevy::PrimFace) parented to the
//! object entity — so each face can carry its own material — kept in Second Life
//! space with the object entity's `Transform` carrying the single basis change
//! (and the object's scale / rotation / position). Since Phase 6 each face
//! carries its own diffuse material built from the object's decoded
//! [`TextureEntry`](sl_client_bevy::TextureEntry) slot (tint + texture) by the
//! [`textures`](crate::textures) pipeline. Since Phase 7 a mesh object fetches
//! and decodes its `LLMesh` asset through the shared [`MeshManager`] and spawns
//! one child entity per submesh; since Phase 9 a sculpted prim fetches its sculpt
//! map through the shared [`TextureManager`], stitches it into geometry with
//! [`tessellate_sculpt`], and spawns its face the same way. Avatar placeholders
//! (P10) attach their geometry to these entities in the same way.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
use bevy::prelude::*;
use sl_client_bevy::{
    AgentKey, DecodedMesh, DecodedTexture, FlexiAttributes, FlexiChain, GRASS_MAX_BLADES,
    JointOverrides, MeshKey, MeshSkin, Object, ObjectExtraParams, ObjectKey, PrimFaceId, PrimLod,
    PrimMesh, PrimShapeFloat, PrimShapeParams, Priority, RegionHandle, Rotation, ScopedObjectId,
    SculptOrMeshKey, SlEvent, SlIdentity, SlSessionEvent, TREE_RADIUS_SCALE_FACTOR,
    TREE_YAW_DEGREES, TextureAnimation, TextureFace, TextureKey, TreeLod, Uuid, Vector,
    avatar_texture, decode_texture_entry, grass_geometry, grass_species, pcode, planar_texgen_uv,
    rigged_inverse_bindposes, tessellate, tessellate_sculpt, tessellate_with_path,
    texture_face_uv_transform, to_bevy_grass_mesh, to_bevy_mesh, to_bevy_prim_mesh,
    to_bevy_rigged_mesh, to_bevy_tree_mesh, tree_billboard_geometry, tree_geometry, tree_species,
};

use crate::animesh::ControlAvatarState;
use crate::asset_budget::MeshUploadBudget;
use crate::avatars::{
    AvatarBody, AvatarPickTarget, AvatarState, BomFace, bom_face_material, log_avatar_faces_enabled,
};
use crate::camera::ViewerCamera;
use crate::coords::{
    origin_shift_bevy, region_offset_bevy, sl_rotation_to_quat, sl_to_bevy_object_rotation,
    sl_to_bevy_vec,
};
use crate::face_material::FaceMaterial;
use crate::flexi::{FLEXI_LOD, FlexiSimState, apply_flexi, flexi_attributes, flexi_from_object};
use crate::geometry_cache::{GeometryCache, GeometryKey, ScaleMm, scale_mm};
use bevy::app::Propagate;
use bevy::camera::visibility::RenderLayers;

use crate::hud::{HUD_RENDER_LAYER, HudState, is_hud_point};
use crate::legacy_materials::LegacyMaterialManager;
use crate::lights::{ObjectLight, light_from_object};
use crate::material_cache::{MaterialCache, MaterialInternContext, SharedFaceMaterial};
use crate::materials::ObjectRenderMaterials;
use crate::meshes::{MeshDecoded, MeshManager};
use crate::particles::{apply_particles, particles_from_object};
use crate::physics::apply_physics;
use crate::probe_layers::{dynamic_render_layers, world_geom_render_layers};
use crate::probes::{apply_reflection_probe, reflection_probe_from_object};
use crate::render_priority::{AVATAR_BOOST_PRIORITY, HUD_BOOST_PRIORITY};
use crate::texture_anim::{ObjectTextureAnimation, running_texture_animation};
use crate::textures::{
    PrimTextures, TextureAlpha, TextureDecoded, TextureManager, face_material, intern_face_material,
};

/// The broad render classification of an in-world object, decided from its
/// `pcode` and sculpt/mesh extra parameters. It routes the object to the right
/// (later-phase) rendering path; P5.1 only records it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ObjectCategory {
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

/// The shape-defining parameters of an object, compared between updates so a
/// motion-only update never triggers a re-tessellation. Deliberately excludes
/// the object's position/rotation/scale (which live in the `Transform`, not the
/// mesh) — only a change here means the geometry must be rebuilt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ShapeFingerprint {
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
    grass_spread: Option<(i32, i32)>,
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
    fn of(object: &Object) -> Self {
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

/// A marker component tagging an entity as an in-world object, carrying its
/// scoped id and render classification for the rendering phases to query — the
/// [`pick_object`] crosshair tool (both fields) and the [`drive_render_priority`]
/// prim LOD pass (P21.3, keyed off the classification and scoped id).
///
/// [`drive_render_priority`]: crate::render_priority::drive_render_priority
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct SceneObject {
    /// The object's scoped (circuit + region-local) id.
    pub(crate) scoped_id: ScopedObjectId,
    /// The object's render classification.
    pub(crate) category: ObjectCategory,
}

/// Debug identity carried on each object's root entity so the [`pick_object`]
/// crosshair tool can report exactly what the camera is looking at — the object's
/// full id, its mesh/sculpt asset id (the thing to fetch and decode offline when
/// its geometry looks wrong), and its Second Life scale/position.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub(crate) struct ObjectDebugInfo {
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
    pub(crate) const fn render_asset(&self) -> Option<Uuid> {
        self.asset
    }

    /// The object's Second Life scale (metres per axis), whose half-diagonal is
    /// its bounding radius for the P20.2 pixel-area computation.
    pub(crate) const fn scale(&self) -> [f32; 3] {
        self.scale
    }
}

/// A marker component tagging one child entity as a single tessellated
/// [`PrimFace`](sl_client_bevy::PrimFace) of its parent prim, carrying the
/// Linden face index its material is looked up by (`TextureEntry.faces[face_id]`).
///
/// Phase 6 builds each face's diffuse material at tessellation time (indexing the
/// `TextureEntry` by this face index); the marker's `face_id` is retained for the
/// later phases that re-address an individual face (per-face material overrides,
/// object picking).
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct PrimFaceEntity {
    /// The Linden semantic face index this face is textured from.
    pub(crate) face_id: PrimFaceId,
}

/// The decoded [`TextureFace`] a face entity was built from, carried so the
/// [`pick_object`] crosshair tool can report the exact per-face texture
/// placement (repeats / offset / rotation / texgen / texture id) of whatever is
/// under the crosshair — the ground truth for debugging a texture-mapping bug.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct FaceTextureDebug(pub(crate) TextureFace);

/// The object's last-received Second Life transform, mirrored onto its entity
/// for the object-editing surfaces (the selection set, the build floater's
/// numeric fields, and the transform gizmos — `viewer-object-selection-core` /
/// `viewer-transform-gizmos`).
///
/// These are the wire-space values an edit round-trips: a **root**'s position /
/// rotation are region-local, a linkset **child**'s are parent-relative —
/// exactly the frame `MultipleObjectUpdate` expects them back in. Refreshed on
/// every object update (including a local echo applied by the edit tools), so
/// readers never re-derive Second Life values from the Bevy transform.
#[derive(Component, Debug, Clone, PartialEq)]
pub(crate) struct ObjectSlMotion {
    /// The position, in region-local metres (root) or parent-relative metres
    /// (linkset child).
    pub(crate) position: Vector,
    /// The orientation, in the same frame as [`position`](Self::position).
    pub(crate) rotation: Rotation,
    /// The object's size in metres per axis.
    pub(crate) scale: Vector,
    /// Whether this object is a linkset root (no parent object).
    pub(crate) is_root: bool,
    /// Whether this object is worn on an avatar (attachments are edited through
    /// the attachment path, not the world gizmos).
    pub(crate) attachment: bool,
}

/// Marks a **linkset-root** object entity — one placed in absolute scene space
/// (offset from the scene origin by its region's global metres,
/// [`object_transform`]), as opposed to a linkset child (parent-relative) or an
/// attachment (skeleton-joint-relative).
///
/// When the scene origin moves — a region crossing, or a teleport to an
/// already-connected region — [`recenter_objects`] shifts every entity carrying
/// this marker by the same uniform delta the camera and terrain shift by, so a
/// static object (one receiving no fresh update across the handover) stays put in
/// the world rather than piling into the new region. A child / attachment carries
/// no marker: its parent already carries it across.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct WorldRootObject;

/// Tags a worn **rigged** submesh with the tracked worn object its geometry
/// belongs to.
///
/// A rigged submesh hangs off the wearer's body root (not its own object
/// entity, see [`apply_rigged_attachments`]), so a hit on it cannot be walked
/// up the entity hierarchy to a [`SceneObject`]; this component carries the
/// identity instead. The GPU pick-tag assignment
/// ([`crate::gpu_pick::assign_avatar_pick_tags`]) reads it so a right-click
/// on a worn mesh resolves to the **attachment** pies
/// ([`crate::attachment_menu`], submesh → worn object → wearer) rather than
/// the wearer's plain avatar pie — the wearer itself rides the sibling
/// [`AvatarPickTarget`]. An animesh submesh (no wearer) is never tagged.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct WornPickTarget {
    /// The scoped id of the worn object this submesh renders.
    pub(crate) scoped: ScopedObjectId,
}

/// Per-object viewer-side bookkeeping, paired with the object's [`SceneObject`]
/// entity.
struct TrackedObject {
    /// The entity rendering this object: carries its position/rotation and is the
    /// parent linkset children and attachments hang off. It has **no scale** (see
    /// [`object_transform`]).
    entity: Entity,
    /// The object's full asset [`ObjectKey`] (the region-independent UUID), kept so
    /// an animesh root can be matched to the `ObjectAnimation` (`object_id`) that
    /// drives its control avatar (P29) and its control avatar pruned when the object
    /// is gone. Distinct from the region-scoped local id this object is keyed by.
    full_key: ObjectKey,
    /// The per-object geometry holder — a child of [`entity`](Self::entity)
    /// carrying the object's Second Life scale, onto which this object's own faces
    /// are parented so the scale never reaches the child prims below it.
    geometry: Entity,
    /// The object's last-seen shape fingerprint, to detect a shape change.
    shape: ShapeFingerprint,
    /// The scoped id of this object's parent (a linkset root or the avatar it is
    /// attached to); its own scoped id when it is a root (parent-local id 0).
    parent: ScopedObjectId,
    /// Whether this object is a root (has no parent object).
    is_root: bool,
    /// Whether this object's entity has been parented to its root entity yet (a
    /// child whose root has not arrived stays `false` until it does). For an
    /// attachment (see [`attachment_point`](Self::attachment_point)) this instead
    /// tracks whether it has been parented to its avatar's skeleton joint (P16.1)
    /// — or, for one worn on a HUD point, whether it has been *routed*: parented
    /// to the HUD screen or hidden as another avatar's (P35.1, terminal either
    /// way).
    parented: bool,
    /// The raw attachment-point id if this object is an attachment worn on an
    /// avatar (its `parent` is the avatar), else `None`. An attachment is parented
    /// to its avatar's skeleton joint rather than a linkset root, by
    /// [`adopt_pending_attachments`] (P16.1).
    attachment_point: Option<u8>,
    /// The object's owner (`owner_id` from the object update). For a worn
    /// attachment this is its wearer, so a stuck attachment can be attributed to
    /// the avatar it belongs to (the `SL_VIEWER_LOG_ATTACHMENT_BIND` diagnostic).
    owner_id: AgentKey,
    /// The object's last-seen `PrimFlags` bitfield (the update's `UpdateFlags`),
    /// kept for the object context menu's enable gates
    /// ([`ObjectState::pick_summary`]): the agent-relative permission bits
    /// (you-owner, copy) and the touch-handler flag decide which pie slices are
    /// live for this object.
    update_flags: u32,
    /// The object's physical-material byte (`LL_MCODE_*`), kept for the build
    /// floater's material editor ([`ObjectState::edit_data`]).
    material: u8,
    /// The object's complete last-received extra parameters, kept so an
    /// `ObjectExtraParams` edit (the build floater's Features tab) can resend
    /// the **full** set — the message states the object's complete
    /// extra-parameter state, so a partial send would clear whatever it
    /// omitted (sculpt, animesh, render materials, …). Also a
    /// [`non_motion_blocks_changed`](Self::non_motion_blocks_changed) input.
    extra: ObjectExtraParams,
    /// The last-applied texture-animation block — a
    /// [`non_motion_blocks_changed`](Self::non_motion_blocks_changed) input,
    /// so a motion-only update skips the texture-animation refresh.
    texture_animation: Option<TextureAnimation>,
    /// The last-applied floating text (`llSetText`) — a
    /// [`non_motion_blocks_changed`](Self::non_motion_blocks_changed) input.
    text: String,
    /// The last-applied floating-text colour (alongside
    /// [`text`](Self::text)).
    text_color: [u8; 4],
    /// The per-face child entities carrying this object's geometry: one per
    /// non-empty [`PrimFace`](sl_client_bevy::PrimFace) for a plain prim or a
    /// sculpt, or one per non-empty submesh for a mesh object. Rebuilt on a shape
    /// change. Empty for an object not yet tessellated (a mesh or sculpt still
    /// waiting on its asset, or a non-rendered category).
    face_entities: Vec<Entity>,
    /// For an object still waiting on an asset fetch to decode (a mesh's `LLMesh`
    /// asset or a sculpt's map texture), the pending build request; `None` once
    /// the geometry is built or for an object whose geometry needs no fetch.
    pending: Option<PendingGeometry>,
    /// For a **built static** (non-rigged, pixel-area LOD managed) mesh object, the
    /// inputs needed to rebuild its submesh entities when the mesh store swaps its
    /// geometry to a different level of detail (P21.2): the mesh key, texture
    /// entry, scale, and fetch priority. `None` for a prim, sculpt, worn rigged
    /// mesh, or a mesh still pending its first decode. Retained so a LOD swap can
    /// despawn the old submeshes and rebuild from the new block.
    mesh_rebuild: Option<PendingMesh>,
    /// For a **plain prim**, the inputs needed to re-tessellate its face entities
    /// when the pixel-area LOD driver picks a different [`PrimLod`] for its
    /// on-screen size (P21.3). `None` for a sculpt, mesh, or non-rendered
    /// category (none of which is client-tessellation LOD managed).
    prim_rebuild: Option<PendingPrim>,
    /// A plain prim's currently tessellated [`PrimLod`] (P21.3), compared against
    /// the driver's desired level to decide whether to re-tessellate. Meaningless
    /// (and left at [`PrimLod::FINEST`]) for a non-prim.
    prim_lod: PrimLod,
    /// For a **tree**, the inputs needed to regenerate its geometry when the
    /// pixel-area LOD driver picks a different [`TreeTier`] for its on-screen size
    /// (P26.2). `None` for a non-tree.
    tree_rebuild: Option<PendingTree>,
    /// A tree's currently generated [`TreeTier`] (P26.2), compared against the
    /// driver's desired tier to decide whether to regenerate. Meaningless (and left
    /// at [`INITIAL_TREE_TIER`]) for a non-tree.
    tree_tier: TreeTier,
    /// Whether this object is an **animated object** (animesh) — its
    /// `ExtendedMesh` param carries the `ANIMATED_MESH_ENABLED` flag. Set on the
    /// linkset root; a worn animesh drives its own control-avatar skeleton, so its
    /// rig joint positions must NOT override the wearer's skeleton (R1), matching
    /// the reference viewer's `!vo->isAnimatedObject()` filter.
    animated: bool,
    /// The object's last-received raw `TextureEntry` bytes, retained so the build
    /// floater's Texture tab ([`crate::edit_texture`]) can read the current
    /// per-face placement and re-send a modified entry (`ObjectImage`). A
    /// non-empty full update overwrites it; a terse (motion-only) update, which
    /// carries no texture entry, leaves it untouched.
    texture_entry: Vec<u8>,
    /// The object's last-received legacy media URL, round-tripped on an
    /// `ObjectImage` send so a texture edit does not clear it (the wire message
    /// carries the whole media-URL field, so omitting it would blank it).
    media_url: Option<String>,
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
    fn non_motion_blocks_changed(
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

/// The `ExtendedMesh` `ANIMATED_MESH_ENABLED` flag (`llprimitive.h`): the object
/// is an animated object (animesh).
const ANIMATED_MESH_ENABLED_FLAG: u32 = 0x1;

/// Whether `object` carries the animated-object (animesh) flag in its
/// `ExtendedMesh` extra params.
fn is_animated_object(object: &Object) -> bool {
    object
        .extra
        .extended_mesh
        .as_ref()
        .is_some_and(|mesh| mesh.flags & ANIMATED_MESH_ENABLED_FLAG != 0)
}

/// The request-time (base) fetch priority for an object's textures and mesh
/// geometry (P20.2): a worn avatar attachment is boosted so it loads with the
/// avatar rather than queued behind the surrounding scene — its skinned / joint-
/// parented entity transform does not reflect its on-screen size, so the
/// pixel-area render-priority pass cannot rank it, and the base priority (which
/// the driver never demotes below) is what keeps it ahead. Ordinary scene objects
/// start [idle](Priority::IDLE) and are ranked purely by on-screen pixel area.
///
/// A HUD attachment (P35.1) is boosted a step higher still, mirroring the
/// reference viewer's `BOOST_HUD`: it hangs off the screen, always in front of the
/// eye and at full size, so it is never worth deferring behind world content.
///
/// Keyed on the object carrying an attachment point (a worn attachment root); a
/// linkset child of a multi-prim attachment is not itself flagged, so this is the
/// common single-object attachment case. (A HUD linkset's children are boosted by
/// the render-priority pass instead, which sees the whole routed subtree.)
const fn worn_base_priority(object: &Object) -> Priority {
    match object.attachment_point_id() {
        Some(point_id) if is_hud_point(point_id) => HUD_BOOST_PRIORITY,
        Some(_) => AVATAR_BOOST_PRIORITY,
        None => Priority::IDLE,
    }
}

/// How far up a parent chain [`in_hud_attachment`] walks before giving up. An
/// attachment's chain is short — object → (linkset root) → avatar — so this only
/// guards against a malformed (cyclic) parent link in the object stream.
const MAX_PARENT_WALK: usize = 8;

/// The agent-relative `FLAGS_OBJECT_MODIFY` bit of `PrimFlags` (`object_flags.h`):
/// this agent may modify the object. The simulator sets it per-agent, folding in
/// the object's owner / group / everyone modify permission.
pub(crate) const FLAGS_OBJECT_MODIFY: u32 = 1 << 2;

/// The agent-relative `FLAGS_OBJECT_COPY` bit: this agent may copy the object.
pub(crate) const FLAGS_OBJECT_COPY: u32 = 1 << 3;

/// The agent-relative `FLAGS_OBJECT_YOU_OWNER` bit: this agent owns the object.
pub(crate) const FLAGS_OBJECT_YOU_OWNER: u32 = 1 << 5;

/// The agent-relative `FLAGS_OBJECT_MOVE` bit: this agent may move (position /
/// rotate) the object — set for the owner and for an "anyone can move" object.
pub(crate) const FLAGS_OBJECT_MOVE: u32 = 1 << 8;

/// The `FLAGS_ALLOW_INVENTORY_DROP` bit of `PrimFlags` (`object_flags.h`): the
/// object is set to let **anyone** add inventory to its contents, the reference
/// viewer's `flagAllowInventoryAdd`. Unlike the modify / copy bits this is a
/// property of the object itself (not agent-relative), and it is the one
/// exception to needing modify on the object to drop an item into it.
pub(crate) const FLAGS_ALLOW_INVENTORY_DROP: u32 = 1 << 16;

/// The `FLAGS_PHANTOM` bit of `PrimFlags` (`object_flags.h`): the object is
/// non-solid — nothing collides with it. The static collider index
/// ([`crate::physics::build_static_colliders`]) still gives a phantom prim a
/// collider (so it is in the shared spatial index for proximity queries) but
/// files it in the non-collidable layer.
pub(crate) const FLAGS_PHANTOM: u32 = 1 << 10;

/// Whether the tracked object `scoped` belongs to a **HUD attachment**: it is
/// itself worn on a HUD point, or it is a linkset child of an object that is (the
/// reference viewer's `getRootEdit()`-based `LLVOVolume::isHUDAttachment`).
///
/// Only an attachment *root* carries the attachment point, so a multi-prim HUD's
/// child prims have to be recognised by walking up to it. Used to keep every part
/// of a HUD out of the world-scene paths — the rigged-mesh bind, which would
/// otherwise skin a HUD mesh onto the wearer's in-world skeleton.
fn in_hud_attachment(state: &ObjectState, scoped: ScopedObjectId) -> bool {
    let mut current = scoped;
    for _ in 0..MAX_PARENT_WALK {
        let Some(tracked) = state.objects.get(&current) else {
            return false;
        };
        if tracked.attachment_point.is_some_and(is_hud_point) {
            return true;
        }
        if tracked.is_root {
            // A linkset root that is not an attachment (or the avatar an attachment
            // hangs off, which is a root object): the chain ends here.
            return false;
        }
        current = tracked.parent;
    }
    false
}

/// Whether `object` — being spawned or rebuilt, so its own tracked entry may not
/// exist yet — is (part of) a HUD attachment: it is worn on a HUD point itself, or
/// it is a linkset child of a root that is. A rigged mesh on a HUD is built as
/// static HUD geometry rather than skinned to a body skeleton it does not have, so
/// the warm-cache mesh build ([`build_object_geometry`]) needs this classification
/// up front (the cold-cache path derives it in [`apply_object_meshes`] via
/// [`in_hud_attachment`], once the object is tracked). Its own point is checked
/// directly; a child defers to its linkset root's tracked entry.
fn object_in_hud_attachment(
    state: &ObjectState,
    attachment_point: Option<u8>,
    is_root: bool,
    parent: ScopedObjectId,
) -> bool {
    if attachment_point.is_some_and(is_hud_point) {
        return true;
    }
    if is_root {
        return false;
    }
    in_hud_attachment(state, parent)
}

/// A deferred geometry build waiting on an asset fetch — a mesh object on its
/// `LLMesh` asset, or a sculpted prim on its sculpt map texture — retained so the
/// object's face entities can be spawned (and textured) once the asset decodes.
enum PendingGeometry {
    /// A mesh object waiting on its mesh asset (built by [`apply_object_meshes`]).
    Mesh(PendingMesh),
    /// A sculpted prim waiting on its sculpt map texture (built by
    /// [`apply_object_sculpts`]).
    Sculpt(PendingSculpt),
    /// A worn **rigged** mesh attachment whose geometry and skin have decoded but
    /// whose avatar skeleton instance is not yet available to bind to (P17.2).
    /// Held until [`apply_rigged_attachments`] can resolve the avatar's joint
    /// entities, then built as a `SkinnedMesh`.
    RiggedMesh(PendingRiggedMesh),
}

/// A mesh object's deferred geometry build: the mesh asset key it is waiting on
/// and the object's texture-entry bytes, retained so its submesh entities can be
/// spawned (and textured) once [`MeshManager`] decodes the mesh.
struct PendingMesh {
    /// The mesh asset key to look the decoded geometry up by.
    key: MeshKey,
    /// The object's raw texture-entry bytes, decoded per-submesh at build time to
    /// texture each face.
    texture_entry: Vec<u8>,
    /// The object's Second Life scale, needed to project planar-texgen faces.
    scale: [f32; 3],
    /// The request-time (base) fetch priority for this object's face textures — a
    /// boost for a worn attachment, else idle (P20.2).
    priority: Priority,
    /// The object-level material-intern inputs, retained because this rebuild
    /// runs without the live [`Object`] at hand.
    intern: MaterialInternContext,
}

/// A worn rigged mesh attachment's deferred skinned build (P17.2): the decoded
/// mesh asset key and the object's texture-entry bytes, retained so its skinned
/// submesh entities can be spawned (and textured) once the wearer avatar's
/// skeleton instance is available to bind against.
struct PendingRiggedMesh {
    /// The mesh asset key to look the decoded geometry and skin up by.
    key: MeshKey,
    /// The object's raw texture-entry bytes, decoded per-submesh at build time to
    /// texture each face.
    texture_entry: Vec<u8>,
}

/// A sculpted prim's deferred geometry build: the sculpt map texture key it is
/// waiting on, the sculpt topology byte, and the object's texture-entry bytes,
/// retained so its face entity can be spawned (and textured) once
/// [`TextureManager`] decodes the map.
struct PendingSculpt {
    /// The sculpt map texture key whose decoded pixels are the geometry input.
    map: TextureKey,
    /// The sculpt type byte (plane / cylinder / sphere / torus topology + the
    /// invert / mirror flags), passed to [`tessellate_sculpt`].
    sculpt_type: u8,
    /// The object's raw texture-entry bytes, decoded at build time to texture the
    /// sculpt's single face.
    texture_entry: Vec<u8>,
    /// The object's Second Life scale, needed to project planar-texgen faces.
    scale: [f32; 3],
    /// The request-time (base) fetch priority for this object's face textures — a
    /// boost for a worn attachment, else idle (P20.2).
    priority: Priority,
    /// The object-level material-intern inputs, retained because this rebuild
    /// runs without the live [`Object`] at hand.
    intern: MaterialInternContext,
}

/// A plain prim's deferred re-tessellation inputs (P21.3): the shape, texture
/// entry, scale, and fetch priority retained so the pixel-area LOD driver can
/// re-tessellate the prim at a different [`PrimLod`] as its on-screen size
/// changes, without needing the live [`Object`] (which the driver does not hold).
///
/// Only a **plain prim** carries this — a sculpt tessellates from its decoded
/// map (no [`PrimLod`] input) and a mesh from fetched geometry blocks, so neither
/// is client-tessellation LOD managed.
struct PendingPrim {
    /// The object's quantized prim shape, re-hydrated to a float
    /// [`PrimShapeFloat`] and re-tessellated at the new level on a LOD swap.
    shape: PrimShapeParams,
    /// The object's raw texture-entry bytes, decoded per-face at build time to
    /// texture each re-tessellated face.
    texture_entry: Vec<u8>,
    /// The object's Second Life scale, needed to project planar-texgen faces.
    scale: [f32; 3],
    /// The request-time (base) fetch priority for this object's face textures — a
    /// boost for a worn attachment, else idle (P20.2).
    priority: Priority,
    /// The object-level material-intern inputs, retained because this rebuild
    /// runs without the live [`Object`] at hand.
    intern: MaterialInternContext,
}

/// The [`PrimLod`] a pixel-area-managed plain prim is first tessellated at
/// (P21.3), before the render-priority driver has a camera to size it against —
/// a coarse placeholder the driver upgrades toward the level the prim's on-screen
/// size warrants (mirroring the mesh path's [placeholder block][crate::meshes]).
/// Client tessellation is cheap, but starting coarse keeps a dense region's
/// initial geometry small and only refines the prims the camera looks at.
const INITIAL_MANAGED_PRIM_LOD: PrimLod = PrimLod::Low;

/// The rendered level of detail of a Linden tree (P26.2): one of the four
/// [`TreeLod`] branching-geometry tiers, or the far [`TreeTier::Billboard`]
/// imposter that stands in for the whole tree once it is small on screen. Selected
/// by the render-priority driver from the tree's on-screen size, mirroring the
/// reference viewer's `LLVOTree::mTrunkLOD` selection plus its billboard fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TreeTier {
    /// Procedural branch / leaf geometry at the given trunk level of detail.
    Lod(TreeLod),
    /// The distant crossed-quad billboard imposter (`tree_billboard_geometry`).
    Billboard,
}

/// The tree tier a new tree is first built at (P26.2), before the render-priority
/// driver has a camera to size it against — a mid branching level the driver
/// refines toward the tier the tree's on-screen size warrants, like a plain prim's
/// [`INITIAL_MANAGED_PRIM_LOD`].
const INITIAL_TREE_TIER: TreeTier = TreeTier::Lod(TreeLod::High);

/// The alpha-test cutoff for tree foliage (P26.2): a leaf-card / trunk texel with
/// alpha below this is discarded, clipping each leaf to its shape. Matches the
/// reference viewer's alpha-mask tree rendering (a mid cutoff for crisp cutout
/// edges without eroding the leaf).
const TREE_ALPHA_CUTOFF: f32 = 0.5;

/// A tree's deferred rebuild inputs (P26.2): its species byte and fetch priority,
/// retained so the pixel-area LOD driver can regenerate its geometry at a
/// different [`TreeTier`] as its on-screen size changes, without the live
/// [`Object`] (which the driver does not hold). The species diffuse texture and
/// geometry parameters are looked up from the species table at rebuild time.
struct PendingTree {
    /// The tree species byte (the object's `Data` genome — see
    /// [`tree_species_byte`], *not* `state`), indexing the `LLVOTree` species
    /// table for the diffuse texture and geometry parameters.
    species: u8,
    /// The request-time (base) fetch priority for the species diffuse texture.
    priority: Priority,
}

/// Viewer-side object bookkeeping: the entity and metadata for every in-world
/// object currently in the scene, keyed by scoped id.
#[derive(Resource, Default)]
pub(crate) struct ObjectState {
    /// Every tracked object, keyed by its scoped id.
    objects: HashMap<ScopedObjectId, TrackedObject>,
    /// The region the Bevy scene is currently anchored at (origin `<0,0,0>`), so
    /// a **root** object in a neighbour region is offset onto the right terrain
    /// ([`object_transform`]) and every root is re-based when this moves
    /// ([`recenter_objects`]). `None` until the first region is known; kept in
    /// lockstep with [`crate::terrain::TerrainState`]'s origin (both follow
    /// [`SlIdentity`]'s root handle).
    origin: Option<RegionHandle>,
}

impl ObjectState {
    /// Despawn **every** tracked object entity (and its faces) and forget them —
    /// the object half of the scene-mirror purge a **fresh-circuit** teleport
    /// needs. The session cleared its object cache with no per-object
    /// `KillObject`, so the incremental [`remove_object`] path never fires;
    /// without this the old region's objects linger forever, at offsets that no
    /// longer correspond to any connected region
    /// ([`Event::RegionChanged`](sl_client_bevy::SlSessionEvent)'s `world_reset`).
    ///
    /// The own avatar's **object** entity is purged along with the rest — it is
    /// only a position-only mirror; the agent's *visible body* is kept across the
    /// purge by [`AvatarState::purge`](crate::avatars::AvatarState::purge) (keyed
    /// by agent, so it does not flash), and the destination re-streams the object
    /// entity. Keeping it here would instead strand it as a ghost dot at the spot
    /// we left, because the same avatar is streamed by *every* connected region so
    /// no single copy is authoritative.
    ///
    /// Also drops the origin anchor so [`recenter_objects`] re-anchors on the
    /// destination without a spurious re-base shift.
    pub(crate) fn purge(&mut self, commands: &mut Commands) {
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

    /// The region the Bevy scene is currently anchored at (scene origin), or
    /// [`None`] before the first root region streams. In-world sounds
    /// (`viewer-in-world-sounds`) need it to place a `SoundTrigger`'s
    /// region-local position into absolute scene space
    /// ([`region_offset_bevy`]).
    pub(crate) const fn origin(&self) -> Option<RegionHandle> {
        self.origin
    }

    /// The full (grid-wide) [`ObjectKey`] of a tracked object, looked up by its
    /// region-scoped id. Used by the physics module (P31.3) to translate a pushed
    /// `ObjectPhysicsProperties` event — which keys by [`ScopedObjectId`] — onto the
    /// same [`ObjectKey`] the `GetObjectPhysicsData` capability reply uses.
    pub(crate) fn full_key(&self, scoped: &ScopedObjectId) -> Option<ObjectKey> {
        self.objects.get(scoped).map(|tracked| tracked.full_key)
    }

    /// The entity of the object with region-scoped id `scoped`, or [`None`] if
    /// this viewer does not track it. Used by the object-selection core
    /// (`viewer-object-selection-core`) to resolve a simulator-forced selection
    /// (`ForceObjectSelect`) onto scene entities.
    pub(crate) fn entity_by_scoped(&self, scoped: &ScopedObjectId) -> Option<Entity> {
        self.objects.get(scoped).map(|tracked| tracked.entity)
    }

    /// The object's physical-material byte (`LL_MCODE_*`), looked up by its
    /// region-scoped id. In-world collision sounds (`viewer-in-world-sounds`)
    /// read it to pick the reference default material collision sound.
    pub(crate) fn material_by_scoped(&self, scoped: &ScopedObjectId) -> Option<u8> {
        self.objects.get(scoped).map(|tracked| tracked.material)
    }

    /// The geometry-holder child entity of the object with region-scoped id
    /// `scoped` — the entity carrying the object's Second Life scale — or
    /// [`None`] if untracked. The transform gizmos (`viewer-transform-gizmos`)
    /// write a live scale edit there so the resize shows before the simulator
    /// echoes it.
    pub(crate) fn geometry_of(&self, scoped: &ScopedObjectId) -> Option<Entity> {
        self.objects.get(scoped).map(|tracked| tracked.geometry)
    }

    /// The **parent object's** entity of the linked part with region-scoped id
    /// `scoped`, or [`None`] for a root / attachment / untracked parent. The
    /// transform gizmos fold a linked part's world-space edit back into its
    /// parent's frame through this entity's global transform.
    pub(crate) fn parent_entity_of(&self, scoped: &ScopedObjectId) -> Option<Entity> {
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
    /// buttons ([`crate::edit_tool`]) and link-number read-out rely on.
    ///
    /// Used by prim unlinking (`viewer-prim-linking`): a whole-linkset unlink
    /// sends an `ObjectDelink` naming **every** prim of the set
    /// (`SEND_INDIVIDUALS`) to break it fully apart; naming only the root would
    /// leave the simulator re-linking the orphaned children into a new set
    /// (OpenSim's `SceneGraph::DelinkObjects`).
    pub(crate) fn linkset_members(&self, root: &ScopedObjectId) -> Vec<ScopedObjectId> {
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
    pub(crate) fn linkset_root_of(&self, scoped: &ScopedObjectId) -> Option<ScopedObjectId> {
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
    pub(crate) fn linkset_prim_count(&self, root: &ScopedObjectId) -> usize {
        self.linkset_members(root).len()
    }

    /// The entity of the object with grid-wide key `key`, or [`None`] if this viewer does
    /// not have it. The reverse of [`full_key`](Self::full_key), used by the point-at
    /// receive path (P31.15) to resolve another avatar's point-at effect — whose target is
    /// named by its full key — against the target object's current transform.
    ///
    /// Objects are keyed by their region-scoped id, so this is a scan; it runs only per
    /// received effect (a handful a second at most), not per frame.
    pub(crate) fn entity_of(&self, key: ObjectKey) -> Option<Entity> {
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
    pub(crate) fn scoped_by_full_keys(
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
    /// ([`crate::object_menu`]), resolved by walking the linkset parent chain up
    /// to its root: the picked prim itself (the touch / sit target), the linkset
    /// root (the derez target — take / delete / return act on roots), the
    /// combined permission flags, and whether the chain is a worn attachment
    /// (which gets an attachment pie — [`crate::attachment_menu`] — rather than
    /// the object one).
    ///
    /// The flags are the **union** of the picked prim's and the root's, because
    /// the agent-relative bits (you-owner, copy) ride the root while the
    /// touch-handler flag can sit on either. For an attachment the walk stops at
    /// the **attachment root** (the object carrying the attachment point), whose
    /// parent is the avatar wearing it — surfaced as
    /// [`wearer`](ObjectPickSummary::wearer) so the attachment pies can decide
    /// self vs other. The walk is bounded like [`in_hud_attachment`]'s, against
    /// a malformed (cyclic) parent link.
    pub(crate) fn pick_summary(&self, scoped: ScopedObjectId) -> Option<ObjectPickSummary> {
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
    /// media-on-a-prim driver ([`crate::media_prim`]) to find the face entity a
    /// media surface's texture goes onto. A scan like [`entity_of`](Self::entity_of),
    /// run only when media data changes — not per frame.
    pub(crate) fn face_entities_by_key(&self, key: ObjectKey) -> Option<&[Entity]> {
        self.objects
            .values()
            .find(|tracked| tracked.full_key == key)
            .map(|tracked| tracked.face_entities.as_slice())
    }

    /// The `UpdateFlags` bits of the object with grid-wide key `key` (its own,
    /// not OR-ed with its root's), or `None` if unknown. The media permission
    /// check reads the you-owner bit from these.
    pub(crate) fn update_flags_by_key(&self, key: ObjectKey) -> Option<u32> {
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
    pub(crate) fn agent_can_modify(&self, scoped: &ScopedObjectId) -> bool {
        self.agent_flags(scoped)
            .is_none_or(|flags| flags & FLAGS_OBJECT_MODIFY != 0)
    }

    /// Whether this agent may **move** `scoped` (position / rotation) — modify
    /// permission, or the `FLAGS_OBJECT_MOVE` bit the simulator sets for the
    /// owner and for an "anyone can move" object. Untracked reads movable.
    pub(crate) fn agent_can_move(&self, scoped: &ScopedObjectId) -> bool {
        self.agent_flags(scoped)
            .is_none_or(|flags| flags & (FLAGS_OBJECT_MODIFY | FLAGS_OBJECT_MOVE) != 0)
    }

    /// Whether this agent may **copy** `scoped` — the `FLAGS_OBJECT_COPY` bit.
    /// Untracked reads copyable.
    pub(crate) fn agent_can_copy(&self, scoped: &ScopedObjectId) -> bool {
        self.agent_flags(scoped)
            .is_none_or(|flags| flags & FLAGS_OBJECT_COPY != 0)
    }

    /// Whether this agent **owns** `scoped` — the `FLAGS_OBJECT_YOU_OWNER` bit
    /// (the reference viewer's `permYouOwner`). Unlike the modify / move / copy
    /// helpers this is **not** optimistic: an untracked object reads *not owned*,
    /// because ownership is a positive grant that gates owner-only affordances
    /// (the contents rename / remove menu items), where a wrong "yes" would offer
    /// an action the simulator then refuses.
    pub(crate) fn agent_owns(&self, scoped: &ScopedObjectId) -> bool {
        self.agent_flags(scoped)
            .is_some_and(|flags| flags & FLAGS_OBJECT_YOU_OWNER != 0)
    }

    /// Whether `scoped` lets **anyone** add inventory to its contents — the
    /// `FLAGS_ALLOW_INVENTORY_DROP` bit (the reference's `flagAllowInventoryAdd`),
    /// the one exception to needing modify on the object to drop an item in.
    /// Untracked reads *false* (the drop still needs modify then).
    pub(crate) fn agent_allows_inventory_drop(&self, scoped: &ScopedObjectId) -> bool {
        self.agent_flags(scoped)
            .is_some_and(|flags| flags & FLAGS_ALLOW_INVENTORY_DROP != 0)
    }

    /// Locally echo an edited `PrimFlags` bit (the build floater's
    /// physical / temporary / phantom toggles) so the checkbox flips
    /// immediately; the simulator's own `ObjectUpdate` echo confirms (or
    /// reverts) it. Display-only: the physics / render systems re-sync from
    /// the echoed update, not from this.
    pub(crate) fn apply_local_flag_edit(&mut self, scoped: &ScopedObjectId, bit: u32, on: bool) {
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
    pub(crate) fn apply_local_material_edit(&mut self, scoped: &ScopedObjectId, material: u8) {
        if let Some(tracked) = self.objects.get_mut(scoped) {
            tracked.material = material;
        }
    }

    /// Locally echo an edited extra-parameter set (the build floater's flexi /
    /// light editors) so the Features tab reflects the send immediately;
    /// display-only — the renderers' components re-sync from the simulator's
    /// echoed update, and the shape fingerprint is deliberately untouched so
    /// that echo still triggers the re-tessellation it needs.
    pub(crate) fn apply_local_extra_edit(
        &mut self,
        scoped: &ScopedObjectId,
        extra: ObjectExtraParams,
    ) {
        if let Some(tracked) = self.objects.get_mut(scoped) {
            tracked.extra = extra;
        }
    }

    /// Everything the build floater's parameter tabs
    /// (`viewer-prim-parameter-editing`) read for one selected object: its
    /// object class, quantized shape, material byte, `PrimFlags` bits, and its
    /// complete extra parameters (borrowed — clone only what an edit resends).
    pub(crate) fn edit_data(&self, scoped: &ScopedObjectId) -> Option<ObjectEditData<'_>> {
        self.objects.get(scoped).map(|tracked| ObjectEditData {
            pcode: tracked.shape.pcode,
            shape: tracked.shape.shape,
            material: tracked.material,
            update_flags: tracked.update_flags,
            extra: &tracked.extra,
        })
    }

    /// The object's last-received raw `TextureEntry` bytes, for the Texture-tab
    /// editor ([`crate::edit_texture`]) to decode the current per-face placement
    /// and re-send a modified entry. `None` if untracked, an empty slice if the
    /// object has not carried a texture entry yet.
    pub(crate) fn texture_entry_of(&self, scoped: &ScopedObjectId) -> Option<&[u8]> {
        self.objects
            .get(scoped)
            .map(|tracked| tracked.texture_entry.as_slice())
    }

    /// The object's last-received legacy media URL, round-tripped on an
    /// `ObjectImage` send so a Texture-tab edit does not clear it.
    pub(crate) fn media_url_of(&self, scoped: &ScopedObjectId) -> Option<String> {
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
    /// *avatar* layer (drawn from [`AvatarState`](crate::avatars::AvatarState),
    /// deduplicated by agent), not the object layer. The same avatar is streamed
    /// as a separate object by *every* connected region (root and each neighbour
    /// child circuit), so admitting them here would plot one object dot per region
    /// — and leave a ghost dot at a region left behind whose copy has not been
    /// reaped (viewer-crossing-stale-minimap-self-dot).
    pub(crate) fn minimap_objects(&self) -> Vec<(Entity, u32)> {
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

    /// The facts the static collider index ([`crate::physics::build_static_colliders`])
    /// needs about one tracked prim, keyed by its scoped id, or `None` if the prim
    /// is not tracked. Reads the wire-side state the resource already holds so the
    /// collider builder does not need its own per-entity component mirror.
    pub(crate) fn static_collider_facts(
        &self,
        scoped: &ScopedObjectId,
    ) -> Option<StaticColliderFacts> {
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
}

/// The wire-side facts [`ObjectState::static_collider_facts`] surfaces for the
/// static collider index: enough to pick a prim's collision layer and shape source
/// without a dedicated per-entity component.
#[derive(Debug, Clone, Copy)]
pub(crate) struct StaticColliderFacts {
    /// The object's full (grid-wide) key — how its physics-shape data is keyed in
    /// [`ObjectPhysicsShapes`](crate::physics::ObjectPhysicsShapes).
    pub(crate) full_key: ObjectKey,
    /// Whether the prim is phantom (`FLAGS_PHANTOM`): indexed but not collidable.
    pub(crate) phantom: bool,
    /// The mesh asset key when the prim is a mesh, else `None` (a plain prim or
    /// sculpt whose collider comes from its tessellated geometry).
    pub(crate) mesh: Option<MeshKey>,
    /// Whether the prim is a flexi prim (skip — its geometry is not holder-scaled).
    pub(crate) flexi: bool,
}

/// What [`ObjectState::edit_data`] reports for one tracked object — the
/// last-received wire-side state the build floater's parameter tabs edit.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ObjectEditData<'state> {
    /// The object class byte (`PCode`); only a [`pcode::PRIMITIVE`] is
    /// shape-editable.
    pub(crate) pcode: u8,
    /// The quantized path/profile shape parameters.
    pub(crate) shape: PrimShapeParams,
    /// The physical-material byte (`LL_MCODE_*`).
    pub(crate) material: u8,
    /// The object's `PrimFlags` bits (physical / temporary / phantom live
    /// here).
    pub(crate) update_flags: u32,
    /// The object's complete extra parameters (flexi, light, sculpt, …).
    pub(crate) extra: &'state ObjectExtraParams,
}

/// What [`ObjectState::pick_summary`] resolves a picked prim to: the identities
/// the object context menu's actions need, and the flag bits its enable gates
/// read. See [`crate::object_menu`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ObjectPickSummary {
    /// The picked prim itself — the touch and sit target.
    pub(crate) picked_scoped: ScopedObjectId,
    /// The picked prim's full (grid-wide) key — what `AgentRequestSit` targets.
    pub(crate) picked_full: ObjectKey,
    /// The linkset root — what take / delete / return derez.
    pub(crate) root_scoped: ScopedObjectId,
    /// The root's full key — what a properties(-family) request queries.
    pub(crate) root_full: ObjectKey,
    /// The union of the picked prim's and the root's `PrimFlags` bits.
    pub(crate) flags: u32,
    /// Whether the picked chain is worn on an avatar (including HUDs) — such a
    /// pick belongs to the attachment pies ([`crate::attachment_menu`]), not the
    /// object one.
    pub(crate) attachment: bool,
    /// For a worn chain, the scoped id of the **avatar object** the attachment
    /// root hangs on (its wearer), resolvable to an agent via
    /// [`AvatarState::agent_of`](crate::avatars::AvatarState::agent_of); `None`
    /// for an ordinary in-world object.
    pub(crate) wearer: Option<ScopedObjectId>,
}

/// Marker for the per-object **geometry holder** entity — the child of an object
/// entity that carries the object's own faces (and its Second Life scale), spawned
/// in [`apply_object`]. It lets the physics module (P31.3) find an object's own
/// tessellated geometry to build a shape-aware collider from, without walking into
/// the linkset **child** prims that also parent to the object entity (they have
/// their own holders and scales).
#[derive(Component)]
pub(crate) struct GeometryHolder;

/// The [`PrimLod`] the render-priority driver (P21.3) wants each plain prim
/// re-tessellated at, keyed by scoped id. The driver ([`drive_render_priority`])
/// computes a prim's level from its on-screen size each throttled pass and writes
/// it here; [`apply_prim_lod`] drains the map and re-tessellates any prim whose
/// desired level differs from its current one. Kept separate from [`ObjectState`]
/// because the driver holds no `Commands` / asset resources to rebuild geometry.
///
/// [`drive_render_priority`]: crate::render_priority::drive_render_priority
#[derive(Resource, Default)]
pub(crate) struct PrimLodTargets(pub(crate) HashMap<ScopedObjectId, PrimLod>);

/// The [`TreeTier`] the render-priority driver (P26.2) wants each tree rendered
/// at, keyed by scoped id — the tree counterpart of [`PrimLodTargets`]. The driver
/// ([`drive_render_priority`]) computes a tree's tier from its on-screen size each
/// throttled pass and writes it here; [`apply_tree_lod`] drains the map and
/// regenerates any tree whose desired tier differs from its current one.
///
/// [`drive_render_priority`]: crate::render_priority::drive_render_priority
#[derive(Resource, Default)]
pub(crate) struct TreeLodTargets(pub(crate) HashMap<ScopedObjectId, TreeTier>);

/// Classify an object from its `pcode` and sculpt/mesh extra parameters.
fn classify(object: &Object) -> ObjectCategory {
    match object.pcode {
        pcode::AVATAR => ObjectCategory::Avatar,
        pcode::PRIMITIVE => match object.extra.sculpt.map(|sculpt| sculpt.texture) {
            Some(SculptOrMeshKey::Mesh(_)) => ObjectCategory::Mesh,
            Some(SculptOrMeshKey::Sculpt(_)) => ObjectCategory::Sculpt,
            None => ObjectCategory::Prim,
        },
        pcode::TREE | pcode::NEW_TREE => ObjectCategory::Tree,
        pcode::GRASS => ObjectCategory::Grass,
        _other => ObjectCategory::Other,
    }
}

/// The Bevy `Transform` for an object entity — position and orientation only,
/// **never scale**.
///
/// A **root** object (no parent) gets a world transform: its region-local
/// position and orientation carried into Bevy's Y-up world by the Second Life →
/// Bevy [basis change](crate::coords). A **child** (linkset member / attachment)
/// gets a *local* transform in pure Second Life space — its position and
/// rotation are already relative to its parent, whose entity carries the single
/// basis change for the whole subtree.
///
/// The object's scale is deliberately **not** on this entity: linkset children
/// parent to it, and Second Life prims each have an absolute size, whereas Bevy's
/// transform hierarchy multiplies a parent's scale into its children (and shears
/// them when it is non-uniform and they are rotated). The scale lives on a
/// per-object geometry holder ([`geometry_transform`]) that only this object's
/// own faces hang off, so it reaches the geometry but never the child prims.
fn object_transform(object: &Object, is_root: bool, origin: Option<RegionHandle>) -> Transform {
    if is_root {
        // A root is placed in absolute scene space, offset from the origin region
        // by its own region's global metres so a neighbour region's objects land
        // on the right terrain (0 for an object in the root region itself). A
        // linkset child stays parent-relative and gets no offset — its parent
        // root already carries it.
        let offset = region_offset_bevy(object.region_handle, origin);
        let local = sl_to_bevy_vec(&object.motion.position);
        Transform {
            translation: Vec3::new(local.x + offset.x, local.y + offset.y, local.z + offset.z),
            rotation: sl_to_bevy_object_rotation(&object.motion.rotation),
            scale: Vec3::ONE,
        }
    } else {
        Transform {
            translation: local_translation(&object.motion.position),
            rotation: sl_rotation_to_quat(&object.motion.rotation),
            scale: Vec3::ONE,
        }
    }
}

/// Insert or remove the [`WorldRootObject`] re-base marker on `entity` so it
/// carries the marker exactly when the object is a linkset root — see
/// [`recenter_objects`].
fn sync_world_root_marker(entity: Entity, is_root: bool, commands: &mut Commands) {
    if is_root {
        commands.entity(entity).insert(WorldRootObject);
    } else {
        commands.entity(entity).remove::<WorldRootObject>();
    }
}

/// Keep the scene origin on the root region for **world objects**: when the root
/// region changes — a border crossing, or a teleport to an already-connected
/// region — shift every [`WorldRootObject`] by the same `-shift`
/// [`recenter_terrain`](crate::terrain::recenter_terrain) applies to the camera
/// and terrain, and record the new origin so freshly-streamed objects are placed
/// against it ([`object_transform`]).
///
/// The origin moved once, so a single uniform delta re-bases every root object
/// regardless of which region it sits in — a root in the region left behind, one
/// in the region entered, and one in a diagonal neighbour all shift by the same
/// vector ([`origin_shift_bevy`]). Only **root** objects carry the marker; a
/// linkset child / attachment is parent-relative, so shifting it too would
/// double-move it.
///
/// This is belt-and-braces with [`object_transform`], which already places each
/// arriving update against the current origin: a root receiving a fresh update
/// across the handover re-places itself correctly regardless, so the shift here
/// is what keeps a *static* root (no update across the crossing) in place. The
/// two never fight — the shift adjusts the old transform, and any same-frame
/// update overwrites it wholesale with the absolute-correct pose.
///
/// Runs after [`recenter_terrain`](crate::terrain::recenter_terrain) (so it reads
/// the same authoritative root the camera/terrain re-based to) and before
/// [`update_objects`] (so a new object this frame is placed against the updated
/// origin).
pub(crate) fn recenter_objects(
    identity: Res<SlIdentity>,
    mut state: ResMut<ObjectState>,
    mut roots: Query<&mut Transform, With<WorldRootObject>>,
) {
    let Some(root) = identity.region_handle else {
        return;
    };
    match state.origin {
        // Unchanged origin: nothing to re-base.
        Some(current) if current == root => {}
        Some(previous) => {
            let shift = origin_shift_bevy(previous, root);
            for mut transform in &mut roots {
                // Per-component (not the `glam` vector operator) to stay clear of
                // the workspace `arithmetic_side_effects` lint, matching
                // `recenter_terrain`.
                transform.translation.x -= shift.x;
                transform.translation.y -= shift.y;
                transform.translation.z -= shift.z;
            }
            state.origin = Some(root);
        }
        // First region learned (login): anchor the origin without shifting.
        None => state.origin = Some(root),
    }
}

/// The object's Second Life scale as the local [`Transform`] of its geometry
/// holder — a child of the object entity that carries the object's faces, so the
/// scale is applied to the geometry in the object's own local frame (after the
/// object's rotation, before nothing else) without propagating down the linkset
/// to child prims. See [`object_transform`] for why the scale is kept off the
/// object entity itself.
const fn geometry_transform(object: &Object) -> Transform {
    Transform::from_scale(Vec3::new(object.scale.x, object.scale.y, object.scale.z))
}

/// The geometry-holder transform for an object of `category` (P26.2). Ordinary
/// objects use the anisotropic per-axis [`geometry_transform`] scale; a **tree**
/// instead reproduces the reference viewer's tree placement, which its generated
/// geometry (in unit-outer-scale Second Life space) needs applied here:
///
/// - a **uniform** scale of `scale.length() * 0.05` (`LLVOTree`'s
///   `radius = getScale().magVec() * 0.05`) — a tree's size tracks the *magnitude*
///   of its scale vector, not its per-axis components;
/// - a fixed 90° yaw about Second Life Z (`LLQuaternion(90°, (0,0,1))`), applied
///   here (in the object's local frame) before the object's own rotation on the
///   object entity;
/// - a small `-0.1 m` Z nudge that plants the trunk base slightly underground
///   (the reference's `pos.z - 0.1` translation).
fn holder_transform(object: &Object, category: ObjectCategory) -> Transform {
    // A flexi prim's geometry is baked in absolute metres by the chain simulation
    // (P32.2, like grass), so — unlike a rigid prim — its holder applies no scale;
    // a non-uniform scale here would shear the bent cross-section.
    if object.extra.flexible.is_some() {
        return Transform::IDENTITY;
    }
    match category {
        ObjectCategory::Tree => {
            let scale_length = Vec3::new(object.scale.x, object.scale.y, object.scale.z).length()
                * TREE_RADIUS_SCALE_FACTOR;
            Transform {
                translation: Vec3::new(0.0, 0.0, -0.1),
                rotation: Quat::from_rotation_z(TREE_YAW_DEGREES.to_radians()),
                scale: Vec3::splat(scale_length),
            }
        }
        // A grass clump's blade geometry is generated in absolute metres with the
        // object scale already folded into the blade-centre spread (P26.3), so —
        // unlike every other category — the holder applies **no** scale (an
        // identity transform), lest the clump be scaled twice.
        ObjectCategory::Grass => Transform::IDENTITY,
        ObjectCategory::Prim
        | ObjectCategory::Sculpt
        | ObjectCategory::Mesh
        | ObjectCategory::Avatar
        | ObjectCategory::Other => geometry_transform(object),
    }
}

/// A child's parent-relative position as a Bevy `Vec3`, kept in pure Second Life
/// space (no axis swap): the parent entity carries the single basis change for
/// the whole linkset subtree.
const fn local_translation(position: &Vector) -> Vec3 {
    Vec3::new(position.x, position.y, position.z)
}

/// One buffered object-stream event awaiting processing under the per-frame spawn
/// budget (see [`PendingObjectEvents`]). Upsert snapshots live out-of-line in
/// [`PendingObjectEvents::payloads`] so a newer snapshot for a still-queued
/// object can replace the queued one in place.
enum PendingObjectEvent {
    /// An `ObjectAdded` / `ObjectUpdated`: (re)apply the object — spawn, move, or
    /// reshape — from its oldest queued snapshot in
    /// [`PendingObjectEvents::payloads`].
    Upsert(ScopedObjectId),
    /// An `ObjectRemoved`: despawn the object and its tracked subtree.
    Remove(ScopedObjectId),
}

/// The FIFO backlog of object-stream events, drained front-to-back by
/// [`update_objects`] at up to [`MeshUploadBudget`] geometry-builds per frame so a
/// region-rez burst does not spawn every object (and build every face material) in
/// one frame. Kept in strict arrival order, so a linkset's root still spawns before
/// its children and an update / remove still lands after the add it targets — exactly
/// as inline processing did, just spread across frames.
///
/// Repeated updates for one object **coalesce**: every `ObjectAdded` /
/// `ObjectUpdated` carries a full merged snapshot (`sl-proto`'s
/// `upsert_object` re-emits the whole cached object), so an update arriving
/// for an object whose newest queued event is a still-undrained upsert just
/// replaces that queued snapshot — one build from the newest data instead of
/// one per update, at the original queue position (ordering intact). An
/// upsert queued behind a remove for the same id never merges across it.
#[derive(Resource, Default)]
pub(crate) struct PendingObjectEvents {
    /// Events not yet processed, oldest at the front.
    queue: VecDeque<PendingObjectEvent>,
    /// The queued upsert snapshots, per object in queue order (front =
    /// oldest). Almost always one element; only an upsert → remove → upsert
    /// interleave for one id holds two.
    payloads: HashMap<ScopedObjectId, VecDeque<Box<Object>>>,
    /// How many removes are queued per object id — an upsert only coalesces
    /// into the previous one when no remove sits between them.
    queued_removes: HashMap<ScopedObjectId, usize>,
}

impl PendingObjectEvents {
    /// Buffer an upsert, coalescing it into the object's newest still-queued
    /// snapshot where ordering allows (see the type docs).
    fn push_upsert(&mut self, object: &Object) {
        let scoped = object.scoped_id();
        if !self.queued_removes.contains_key(&scoped)
            && let Some(slots) = self.payloads.get_mut(&scoped)
            && let Some(back) = slots.back_mut()
        {
            **back = object.clone();
            return;
        }
        self.payloads
            .entry(scoped)
            .or_default()
            .push_back(Box::new(object.clone()));
        self.queue.push_back(PendingObjectEvent::Upsert(scoped));
    }

    /// Buffer a remove.
    fn push_remove(&mut self, scoped: ScopedObjectId) {
        let count = self.queued_removes.entry(scoped).or_insert(0);
        *count = count.saturating_add(1);
        self.queue.push_back(PendingObjectEvent::Remove(scoped));
    }

    /// Drop the whole backlog — a distant teleport purged the scene, so any
    /// buffered upsert / remove targets a now-gone (old-region local-id) object.
    pub(crate) fn clear(&mut self) {
        self.queue.clear();
        self.payloads.clear();
        self.queued_removes.clear();
    }
}

/// Take the oldest queued upsert snapshot for `scoped` (see
/// [`PendingObjectEvents::payloads`]), dropping the id's slot once emptied.
fn pop_upsert_payload(
    payloads: &mut HashMap<ScopedObjectId, VecDeque<Box<Object>>>,
    scoped: ScopedObjectId,
) -> Option<Box<Object>> {
    let slots = payloads.get_mut(&scoped)?;
    let object = slots.pop_front();
    if slots.is_empty() {
        let _empty = payloads.remove(&scoped);
    }
    object
}

/// The most decoded keys [`apply_object_meshes`] / [`apply_object_sculpts`]
/// pop per frame: each popped key costs one scan of the tracked-object map
/// even when nothing is pending on it (most `TextureDecoded` ids are ordinary
/// face textures, not sculpt maps), so the scans are capped separately from
/// the build budget.
const GEOMETRY_APPLY_SCAN_CAP: usize = 64;

/// The outcome of examining one LOD target in [`retain_lod_budgeted`].
enum LodOutcome {
    /// Resolved or irrelevant (object gone, wrong kind, or already at the
    /// desired level) — drop the target, free (no budget charged).
    Resolved,
    /// A genuine re-tessellation was performed — drop the target, one unit spent.
    Rebuilt,
    /// Out of budget this frame — keep the target for a later frame.
    Deferred,
}

/// Budgeted `retain` over a LOD target map. For each `(key, value)`, `apply` is
/// given the budget still remaining this frame and returns whether it was a free
/// skip ([`LodOutcome::Resolved`]), did a budgeted rebuild
/// ([`LodOutcome::Rebuilt`], allowed only while `remaining > 0`), or must wait
/// ([`LodOutcome::Deferred`]). Resolved and Rebuilt targets are removed; Deferred
/// targets are kept for the next frame. Returns the number of rebuilds performed.
///
/// Unlike [`drain_budgeted`], the target set is a `HashMap` keyed by
/// [`ScopedObjectId`], so it is **already deduplicated per object** — a re-insert
/// for the same object overwrites its desired level, and `drive_render_priority`
/// visits each object at most once per tick — so there is no duplicate LOD apply
/// to guard against and no FIFO to preserve. Iteration order does not matter:
/// any target left un-applied is re-derived on the next tick.
fn retain_lod_budgeted<K: Copy + Eq + std::hash::Hash, V: Copy>(
    map: &mut HashMap<K, V>,
    budget: usize,
    mut apply: impl FnMut(K, V, usize) -> LodOutcome,
) -> usize {
    let mut builds = 0usize;
    map.retain(|&key, &mut value| {
        let remaining = budget.saturating_sub(builds);
        match apply(key, value, remaining) {
            LodOutcome::Resolved => false,
            LodOutcome::Rebuilt => {
                builds = builds.saturating_add(1);
                false
            }
            LodOutcome::Deferred => true,
        }
    });
    builds
}

/// Decoded mesh keys awaiting application by [`apply_object_meshes`], deduped
/// — a key already queued absorbs later decode events (the apply always reads
/// the store's current block).
#[derive(Resource, Default)]
pub(crate) struct PendingDecodedMeshes {
    /// Keys not yet applied, oldest at the front.
    queue: VecDeque<MeshKey>,
    /// The queued keys, for O(1) dedup.
    queued: HashSet<MeshKey>,
}

/// Decoded texture keys awaiting the sculpt-build check by
/// [`apply_object_sculpts`], deduped (see [`PendingDecodedMeshes`]). Most
/// entries are ordinary face textures that no sculpt waits on; they drain as
/// free scans under [`GEOMETRY_APPLY_SCAN_CAP`].
#[derive(Resource, Default)]
pub(crate) struct PendingDecodedSculpts {
    /// Keys not yet checked, oldest at the front.
    queue: VecDeque<TextureKey>,
    /// The queued keys, for O(1) dedup.
    queued: HashSet<TextureKey>,
}

/// Process queued items front-to-back, calling `process` on each; `process` returns
/// `true` when the item did a geometry build (the budgeted work). The drain stops
/// once `budget` builds have happened, leaving the rest queued for a later frame.
/// Cheap items (`process` returns `false` — a move, a remove) are free and processed
/// until the next build, so the backlog never stalls behind them. Strict FIFO, so
/// arrival order is preserved across frames. Returns the number of builds performed.
/// Testable core of [`update_objects`]'s spawn budgeting.
fn drain_budgeted<T>(
    queue: &mut VecDeque<T>,
    budget: usize,
    mut process: impl FnMut(T) -> bool,
) -> usize {
    let mut builds = 0;
    while builds < budget {
        let Some(item) = queue.pop_front() else {
            break;
        };
        if process(item) {
            builds = builds.saturating_add(1);
        }
    }
    builds
}

/// Fold the object event stream into the scene graph: spawn / update / despawn
/// entities, classify them, keep their transforms current, and maintain linkset
/// parenting — draining any earlier-frame backlog first, then applying new events
/// inline (no clone) while the queue is empty and the [`MeshUploadBudget`] holds, else
/// buffering the overflow into [`PendingObjectEvents`] for a later frame.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system reading the object stream and the ECS resources the geometry build needs"
)]
pub(crate) fn update_objects(
    mut events: MessageReader<SlEvent>,
    mut state: ResMut<ObjectState>,
    mut pending: ResMut<PendingObjectEvents>,
    mut mesh_budget: ResMut<MeshUploadBudget>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<FaceMaterial>>,
    mut manager: ResMut<TextureManager>,
    mut prim_textures: ResMut<PrimTextures>,
    mut mesh_manager: ResMut<MeshManager>,
    mut cache: ResMut<GeometryCache>,
    mut material_cache: ResMut<MaterialCache>,
) {
    // Object spawns draw from the shared per-frame mesh-upload lane, in schedule
    // order with the other mesh-inserting systems — seed a local counter from what
    // the lane has left and write the remainder back so later systems see it spent.
    let mut budget = mesh_budget.remaining;
    // 1. Drain any backlog carried over from earlier frames first (FIFO), so new
    //    events never jump ahead of it. Only a spawn / re-tessellation (`apply_object`
    //    returns `true`) costs budget; a move or remove is free.
    let drained = {
        let PendingObjectEvents {
            queue,
            payloads,
            queued_removes,
        } = &mut *pending;
        drain_budgeted(queue, budget, |event| match event {
            PendingObjectEvent::Upsert(scoped) => {
                let Some(object) = pop_upsert_payload(payloads, scoped) else {
                    warn!("queued upsert for {scoped:?} had no snapshot (coalescing bug)");
                    return false;
                };
                apply_object(
                    &mut state,
                    &object,
                    &mut commands,
                    &mut meshes,
                    &mut materials,
                    &mut manager,
                    &mut prim_textures,
                    &mut mesh_manager,
                    &mut cache,
                    &mut material_cache,
                )
            }
            PendingObjectEvent::Remove(scoped) => {
                if let Some(count) = queued_removes.get_mut(&scoped) {
                    *count = count.saturating_sub(1);
                    if *count == 0 {
                        let _empty = queued_removes.remove(&scoped);
                    }
                }
                remove_object(&mut state, scoped, &mut commands);
                false
            }
        })
    };
    budget = budget.saturating_sub(drained);
    // 2. Process new events in arrival order: while the backlog is fully drained and
    //    budget remains, apply them **inline without cloning**; once the budget is
    //    spent (or any backlog remains) buffer the rest — only the overflow is cloned.
    //    Strict FIFO holds because the moment one event is buffered, `queue.is_empty()`
    //    is false and every later event is buffered too.
    for event in events.read() {
        if pending.queue.is_empty() && budget > 0 {
            let built = match &event.0 {
                SlSessionEvent::ObjectAdded(object) | SlSessionEvent::ObjectUpdated(object) => {
                    apply_object(
                        &mut state,
                        object,
                        &mut commands,
                        &mut meshes,
                        &mut materials,
                        &mut manager,
                        &mut prim_textures,
                        &mut mesh_manager,
                        &mut cache,
                        &mut material_cache,
                    )
                }
                SlSessionEvent::ObjectRemoved { local_id, .. } => {
                    remove_object(&mut state, *local_id, &mut commands);
                    false
                }
                _other => false,
            };
            if built {
                budget = budget.saturating_sub(1);
            }
        } else {
            match &event.0 {
                SlSessionEvent::ObjectAdded(object) | SlSessionEvent::ObjectUpdated(object) => {
                    // Cloned only now, on the overflow; a snapshot already
                    // queued for this object is replaced in place instead.
                    pending.push_upsert(object);
                }
                SlSessionEvent::ObjectRemoved { local_id, .. } => {
                    pending.push_remove(*local_id);
                }
                _other => {}
            }
        }
    }
    // Hand the unspent remainder back to the shared lane for the later mesh systems.
    mesh_budget.remaining = budget;
}

/// A scale component (metres) above this is unusual for ordinary content and
/// flagged by [`log_suspicious_objects`] — a megaprim, a region-surround shell,
/// or a wrongly sized render.
const SUSPICIOUS_SCALE_M: f32 = 16.0;

/// A region-local Z (metres) above this is "up in the sky" — a skybox or sky
/// platform, flagged by [`log_suspicious_objects`].
const SUSPICIOUS_HEIGHT_M: f32 = 500.0;

/// Diagnostic (opt-in via the `SL_VIEWER_LOG_OBJECTS` env var): logs each object
/// whose scale or height is out of the ordinary — big enough to read as
/// "region-sized" or high enough to be a skybox — so a live session can tell a
/// genuinely large/high object (which a reference viewer would draw-distance
/// cull, not misplace) from a wrongly parsed or wrongly scaled one. Each object is
/// logged once per full id.
///
/// The distinction it draws: if the flagged objects sit at plausible sky
/// positions with sane (if large) scales, the viewer is simply not culling by
/// distance the way a reference viewer does (empty OpenSim has none, so it never
/// showed); if they carry impossible scales/positions, a decode is wrong.
pub(crate) fn log_suspicious_objects(
    mut events: MessageReader<SlEvent>,
    mut seen: Local<std::collections::HashSet<Uuid>>,
    mut enabled: Local<Option<bool>>,
) {
    // Resolve the env gate once and cache it (a `Local` persists across runs).
    let on = *enabled.get_or_insert_with(|| std::env::var_os("SL_VIEWER_LOG_OBJECTS").is_some());
    if !on {
        return;
    }
    for event in events.read() {
        let (SlSessionEvent::ObjectAdded(object) | SlSessionEvent::ObjectUpdated(object)) =
            &event.0
        else {
            continue;
        };
        let scale = &object.scale;
        let position = &object.motion.position;
        let big = scale.x.abs() > SUSPICIOUS_SCALE_M
            || scale.y.abs() > SUSPICIOUS_SCALE_M
            || scale.z.abs() > SUSPICIOUS_SCALE_M;
        let high = position.z > SUSPICIOUS_HEIGHT_M || position.z < -100.0;
        let off_region =
            !(-64.0..=320.0).contains(&position.x) || !(-64.0..=320.0).contains(&position.y);
        if !(big || high || off_region) {
            continue;
        }
        if !seen.insert(object.full_id.uuid()) {
            continue;
        }
        let kind = match classify(object) {
            ObjectCategory::Prim => "prim",
            ObjectCategory::Mesh => "mesh",
            ObjectCategory::Sculpt => "sculpt",
            ObjectCategory::Avatar => "avatar",
            ObjectCategory::Tree => "tree",
            ObjectCategory::Grass => "grass",
            ObjectCategory::Other => "other",
        };
        warn!(
            "suspicious object {} pcode={} kind={kind} parent={} scale=({:.2},{:.2},{:.2}) \
             pos=({:.1},{:.1},{:.1}) big={big} high={high} off_region={off_region}",
            object.full_id,
            object.pcode,
            object.parent_id.get(),
            scale.x,
            scale.y,
            scale.z,
            position.x,
            position.y,
            position.z,
        );
    }
}

/// Crosshair pick tool (press **`P`**): casts a ray straight out of the camera
/// and logs the object under the centre of the screen — its full id, mesh/sculpt
/// asset id, kind, scale, and Second Life position — so a wrongly rendered object can
/// be identified by looking at it rather than by trawling the object stream. Aim
/// the middle of the window at the object and press the key; the `asset` id is the
/// mesh/sculpt to fetch and decode offline when its geometry looks wrong.
///
/// It also logs the live level of detail under the crosshair: the diffuse
/// texture's current discard level (P21.1) and, for a mesh, its decoded geometry
/// LOD (P21.2). Aim at a face and press the key while walking toward it to confirm
/// the discard level falls (finer) and the mesh LOD rises as it should.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system querying the several components the pick report reads"
)]
pub(crate) fn pick_object(
    keyboard: Res<ButtonInput<KeyCode>>,
    // `ViewerCamera`, not `Camera3d`: the probe-capture cameras (P33.2) also carry
    // `Camera3d`, and `single()` fails once more than one matches.
    camera: Query<&GlobalTransform, With<ViewerCamera>>,
    mut ray_cast: MeshRayCast,
    scene: Query<&SceneObject>,
    infos: Query<&ObjectDebugInfo>,
    lights: Query<&ObjectLight>,
    tex_anims: Query<&ObjectTextureAnimation>,
    globals: Query<&GlobalTransform>,
    parents: Query<&ChildOf>,
    face_debug: Query<(&PrimFaceEntity, &FaceTextureDebug)>,
    face_materials: Query<&MeshMaterial3d<FaceMaterial>>,
    materials: Res<Assets<FaceMaterial>>,
    legacy: Res<LegacyMaterialManager>,
    textures: Res<TextureManager>,
    mesh_manager: Res<MeshManager>,
    state: Res<ObjectState>,
) {
    if !keyboard.just_pressed(KeyCode::KeyP) {
        return;
    }
    let camera = match camera.single() {
        Ok(camera) => camera,
        Err(error) => {
            warn!("pick: expected exactly one 3D camera ({error})");
            return;
        }
    };
    let ray = Ray3d::new(camera.translation(), camera.forward());
    let hits = ray_cast.cast_ray(ray, &MeshRayCastSettings::default());
    let Some((entity, hit)) = hits.first() else {
        warn!("pick: nothing under the crosshair (aim at a surface and press P)");
        return;
    };
    // The ray strikes a face/submesh child entity: report that exact face's
    // texture placement (the ground truth for a texture-mapping bug) before
    // walking up to the object identity. The face index is retained so the
    // object's texture animation (P28.1), reported below, can say whether it
    // targets this particular face.
    let mut picked_face: Option<PrimFaceId> = None;
    if let Ok((face, FaceTextureDebug(tf))) = face_debug.get(*entity) {
        picked_face = Some(face.face_id);
        warn!(
            "pick face {}: texture={} repeats=({:.3},{:.3}) offset=({:.3},{:.3}) \
             rot={:.3}rad media_flags=0x{:02x} texgen=0x{:02x} planar={} \
             color=[{},{},{},{}] glow={:.3} material_id={:?}",
            face.face_id.get(),
            tf.texture_id,
            tf.scale_s,
            tf.scale_t,
            tf.offset_s,
            tf.offset_t,
            tf.rotation,
            tf.media_flags,
            tf.tex_gen(),
            tf.is_planar_texgen(),
            tf.color[0],
            tf.color[1],
            tf.color[2],
            tf.color[3],
            tf.glow,
            tf.material_id,
        );
        // The face's *resolved* render alpha state plus the fetched legacy
        // material's alpha fields (R25): together they pin an opaque-vs-blend
        // divergence to the TE tint, the legacy override, or a missing
        // `RenderMaterials` fetch.
        if let Ok(material) = face_materials.get(*entity)
            && let Some(standard) = materials.get(&material.0)
        {
            warn!(
                "pick face render: alpha_mode={:?} base_color_alpha={:.3} unlit={}",
                standard.base.alpha_mode,
                standard.base.base_color.alpha(),
                standard.base.unlit,
            );
        }
        if let Some(material_id) = tf.material_id {
            match legacy.decoded_material(&material_id) {
                Some(fetched) => warn!(
                    "pick face legacy material {material_id}: diffuse_alpha_mode={} \
                     alpha_mask_cutoff={} normal_map={} specular_map={}",
                    fetched.diffuse_alpha_mode,
                    fetched.alpha_mask_cutoff,
                    fetched.normal_map.uuid(),
                    fetched.specular_map.uuid(),
                ),
                None => warn!(
                    "pick face legacy material {material_id}: not fetched/decoded \
                     (RenderMaterials fetch missing or still in flight)"
                ),
            }
        }
        // The live level-of-detail of the face's diffuse texture (P21.1): its
        // current discard level should *fall* (toward 0 = full resolution) as the
        // camera moves toward the face. Aim and press the pick key while walking in
        // to confirm the texture actually refines.
        match textures.lod_debug(tf.texture_id) {
            Some(lod) => warn!(
                "pick texture {}: discard={} current={}x{} native={:?} header_native={:?} managed={}",
                tf.texture_id,
                lod.discard.get(),
                lod.width,
                lod.height,
                lod.native,
                lod.header_native,
                lod.managed,
            ),
            None => warn!(
                "pick texture {}: not decoded yet (still fetching or no texture)",
                tf.texture_id,
            ),
        }
    }
    // The ray strikes a face/submesh child entity; walk up the linkset to the
    // object root that carries the identity component.
    let mut current = *entity;
    loop {
        if let Ok(info) = infos.get(current) {
            let kind = scene
                .get(current)
                .map_or("?", |scene| match scene.category {
                    ObjectCategory::Prim => "prim",
                    ObjectCategory::Mesh => "mesh",
                    ObjectCategory::Sculpt => "sculpt",
                    ObjectCategory::Avatar => "avatar",
                    ObjectCategory::Tree => "tree",
                    ObjectCategory::Grass => "grass",
                    ObjectCategory::Other => "other",
                });
            // The object entity's actual world scale — if it is much larger than
            // `scale` below, the linkset root's scale is wrongly propagating to
            // this child (Bevy composes parent scale; Second Life does not).
            let world_scale = globals
                .get(current)
                .map(|global| global.to_scale_rotation_translation().0);
            warn!(
                "pick: {kind} full_id={} asset={:?} scale=({:.2},{:.2},{:.2}) \
                 world_scale={:?} pos=({:.1},{:.1},{:.1}) hit_dist={:.2}m shape={:?}",
                info.full_id,
                info.asset,
                info.scale[0],
                info.scale[1],
                info.scale[2],
                world_scale,
                info.position[0],
                info.position[1],
                info.position[2],
                hit.distance,
                info.shape,
            );
            // The live mesh level of detail (P21.2): for a mesh object, its decoded
            // geometry block should move toward `High` as the camera approaches. A
            // boosted (worn attachment) mesh stays at the finest level and is not
            // LOD managed.
            if matches!(scene.get(current), Ok(obj) if obj.category == ObjectCategory::Mesh)
                && let Some(asset) = info.asset
                && let Some((lod, managed)) = mesh_manager.lod_debug(MeshKey::from(asset))
            {
                warn!("pick mesh {asset}: lod={lod:?} managed={managed}");
            }
            // The live prim level of detail (P21.3): for a plain prim, its current
            // tessellation level should move toward `High` as the camera
            // approaches. Aim at a prim face and press the pick key while walking
            // in to confirm it refines.
            if let Ok(obj) = scene.get(current)
                && obj.category == ObjectCategory::Prim
                && let Some(tracked) = state.objects.get(&obj.scoped_id)
            {
                warn!("pick prim {}: lod={:?}", info.full_id, tracked.prim_lod);
            }
            // The ingested light block (P25.1): a light-source prim reports its
            // decoded colour / intensity / radius / falloff and, for a spotlight,
            // its projector texture + cone params — the ground truth for the
            // P25.2 render pass.
            if let Ok(light) = lights.get(current) {
                let emitted = light.effective_linear_color();
                warn!(
                    "pick light {}: spotlight={} linear_color=[{:.3},{:.3},{:.3}] \
                     intensity={:.3} emitted=[{:.3},{:.3},{:.3}] radius={:.2}m \
                     falloff={:.2} cutoff={:.1}deg projection={:?}",
                    info.full_id,
                    light.is_spotlight(),
                    light.linear_color[0],
                    light.linear_color[1],
                    light.linear_color[2],
                    light.intensity,
                    emitted[0],
                    emitted[1],
                    emitted[2],
                    light.radius,
                    light.falloff,
                    light.cutoff,
                    light.projection,
                );
            }
            // The ingested texture animation (P28.1): a prim running
            // `llSetTextureAnim` reports its decoded mode / frame-grid / timing —
            // the ground truth for the P28.2 UV / flipbook driver — plus whether
            // it targets the face under the crosshair (`face == -1` = all faces).
            if let Ok(obj) = scene.get(current)
                && let Some(tracked) = state.objects.get(&obj.scoped_id)
                && let Ok(tex_anim) = tex_anims.get(tracked.geometry)
            {
                let anim = tex_anim.anim;
                let targets_face = picked_face.map(|face| tex_anim.applies_to_face(face.get()));
                warn!(
                    "pick texture-anim {}: mode=0x{:02x} face={} grid={}x{} \
                     start={:.3} length={:.3} rate={:.3} targets_picked_face={:?}",
                    info.full_id,
                    anim.mode,
                    anim.face,
                    anim.size_x,
                    anim.size_y,
                    anim.start,
                    anim.length,
                    anim.rate,
                    targets_face,
                );
            }
            return;
        }
        let Ok(child_of) = parents.get(current) else {
            warn!("pick: hit an entity with no object identity");
            return;
        };
        current = child_of.parent();
    }
}

/// The mesh asset key of a mesh object, or `None` if the object is not a mesh.
fn mesh_key(object: &Object) -> Option<MeshKey> {
    match object.extra.sculpt.map(|sculpt| sculpt.texture) {
        Some(SculptOrMeshKey::Mesh(key)) => Some(key),
        _other => None,
    }
}

/// The sculpt map texture key and topology byte of a sculpted prim, or `None` if
/// the object is not a sculpt (a plain prim, a mesh, or a non-prim).
fn sculpt_key(object: &Object) -> Option<(TextureKey, u8)> {
    let sculpt = object.extra.sculpt?;
    match sculpt.texture {
        SculptOrMeshKey::Sculpt(key) => Some((key, sculpt.sculpt_type)),
        SculptOrMeshKey::Mesh(_) => None,
    }
}

/// Attach (or clear) the object's per-face GLTF render-material references on its
/// geometry-holder entity — the parent of its face entities — so
/// [`register_pbr_materials`](crate::materials::register_pbr_materials) can look a
/// face's PBR material up by index (P27.1). Refreshed on every update, and the
/// component removed when the object carries no PBR material, so a material
/// cleared in-world stops being applied.
fn apply_render_materials(
    geometry: Entity,
    scoped: ScopedObjectId,
    object: &Object,
    commands: &mut Commands,
) {
    let faces: Vec<(u8, Uuid)> = object
        .extra
        .render_material
        .iter()
        .map(|reference| (reference.face, reference.material_id))
        .collect();
    if faces.is_empty() {
        commands.entity(geometry).remove::<ObjectRenderMaterials>();
    } else {
        commands.entity(geometry).insert(ObjectRenderMaterials {
            scoped_id: scoped,
            faces,
        });
    }
}

/// Carry (or clear) the object's decoded texture animation on its geometry-holder
/// entity — the parent of its face entities — so the P28.2 driver can advance it
/// each frame (P28.1). Refreshed on every update: the [`ObjectTextureAnimation`]
/// holder is inserted while the object reports a **running** animation (the
/// [`ON`](sl_client_bevy::texture_anim_mode::ON) bit set) and removed otherwise,
/// so an animation stopped in-world reverts the faces to their static placement.
fn apply_texture_animation(geometry: Entity, object: &Object, commands: &mut Commands) {
    match running_texture_animation(object.texture_animation) {
        Some(anim) => {
            debug!(
                "object {} texture animation: mode=0x{:02x} face={} grid={}x{}",
                object.scoped_id(),
                anim.mode,
                anim.face,
                anim.size_x,
                anim.size_y,
            );
            commands
                .entity(geometry)
                .insert(ObjectTextureAnimation { anim });
        }
        None => {
            commands.entity(geometry).remove::<ObjectTextureAnimation>();
        }
    }
}

/// The result of [`build_object_geometry`]: the spawned face entities plus the
/// category-specific follow-up state — a deferred asset build ([`PendingGeometry`],
/// a mesh / sculpt), the plain-prim LOD re-tessellation inputs ([`PendingPrim`]),
/// the tree regeneration inputs ([`PendingTree`]), and a flexi prim's seeded
/// [`FlexiChain`]; at most one of the last three is ever `Some`.
type ObjectGeometryBuild = (
    Vec<Entity>,
    Option<PendingGeometry>,
    Option<PendingPrim>,
    Option<PendingTree>,
    Option<FlexiChain>,
    // The mesh LOD-rebuild inputs, set when a mesh is built immediately from an
    // already-decoded (warm-cache) asset — the cold-cache path instead sets them
    // in `apply_object_meshes` when its `PendingGeometry::Mesh` resolves. Without
    // this a warm-built shared mesh has no `mesh_rebuild` and so never rebuilds its
    // submeshes when the pixel-area driver swaps the shared geometry's LOD, leaving
    // it frozen at the level the first instance decoded at (the coarse
    // `INITIAL_MANAGED_LOD`).
    Option<PendingMesh>,
);

/// Build an object's renderable geometry for its category, returning the spawned
/// child entities and — for a mesh or sculpt whose asset has not decoded yet — the
/// pending build to finish once the asset arrives.
///
/// A plain prim is tessellated and spawned immediately. A mesh requests its asset
/// through `mesh_manager` and, if the geometry is already decoded, spawns its
/// submeshes now; otherwise it returns a [`PendingGeometry::Mesh`] so
/// [`apply_object_meshes`] can build it on decode. A sculpt requests its map
/// texture through `manager` (the shared texture store) and, if the map is already
/// decoded, stitches and spawns its face now; otherwise it returns a
/// [`PendingGeometry::Sculpt`] so [`apply_object_sculpts`] can build it on decode.
/// A **tree** generates its branch / leaf geometry immediately from its species
/// (P26.2) and returns a [`PendingTree`] so [`apply_tree_lod`] can regenerate it
/// at a different [`TreeTier`]. Every other category renders nothing here.
///
/// The last three returns are the plain-prim re-tessellation inputs
/// ([`PendingPrim`], P21.3), the tree regeneration inputs ([`PendingTree`],
/// P26.2), and a **flexi** prim's seeded [`FlexiChain`] (P32.2, in place of
/// `PendingPrim` — a flexi prim is chain-driven, not pixel-area LOD managed); at
/// most one of the three is ever `Some`.
#[expect(
    clippy::too_many_arguments,
    reason = "threads the several ECS resources the geometry build needs"
)]
fn build_object_geometry(
    object: &Object,
    category: ObjectCategory,
    entity: Entity,
    is_hud: bool,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<FaceMaterial>,
    manager: &mut TextureManager,
    prim_textures: &mut PrimTextures,
    mesh_manager: &mut MeshManager,
    cache: &mut GeometryCache,
    intern: &MaterialInternContext,
    material_cache: &mut MaterialCache,
) -> ObjectGeometryBuild {
    // A worn attachment's textures / mesh are boosted so they load with the
    // avatar rather than queued behind the surrounding scene (P20.2).
    let priority = worn_base_priority(object);
    match category {
        // A flexi prim (P32.2) builds its geometry from the chain's rest path (at
        // the softness's section count) and is driven by [`simulate_flexi`], so it
        // is NOT on the pixel-area LOD re-tessellation path (no `PendingPrim`); the
        // returned chain seeds its [`FlexiSimState`].
        ObjectCategory::Prim if object.extra.flexible.is_some() => {
            let (faces, chain) = build_flexi_faces(
                object,
                entity,
                commands,
                meshes,
                materials,
                manager,
                prim_textures,
                priority,
                intern,
                material_cache,
            );
            (faces, None, None, None, Some(chain), None)
        }
        ObjectCategory::Prim => (
            build_prim_faces(
                object,
                entity,
                commands,
                meshes,
                materials,
                manager,
                prim_textures,
                priority,
                INITIAL_MANAGED_PRIM_LOD,
                cache,
                intern,
                material_cache,
            ),
            None,
            // Retain the re-tessellation inputs so the pixel-area LOD driver can
            // rebuild this prim at a different level as its on-screen size
            // changes (P21.3).
            Some(PendingPrim {
                shape: object.shape,
                texture_entry: object.texture_entry.clone(),
                scale: [object.scale.x, object.scale.y, object.scale.z],
                priority,
                intern: intern.clone(),
            }),
            None,
            None,
            None,
        ),
        ObjectCategory::Mesh => {
            let Some(key) = mesh_key(object) else {
                return (Vec::new(), None, None, None, None, None);
            };
            mesh_manager.request(key, priority);
            // The store hands back an `Arc`; clone it out so the immutable borrow
            // of `mesh_manager` ends before the submesh build borrows the other
            // resources.
            match mesh_manager.decoded(key).map(Arc::clone) {
                // A rigged mesh (one carrying a skin block) is worn by an avatar and
                // must be skinned to its skeleton, never built as a static child —
                // even when its asset is already warm in the cache, which is the
                // case on a runtime re-attach (detach then "add to current outfit").
                // The cold-cache decode path routes rigged meshes to
                // `apply_rigged_attachments` via a `RiggedMesh` pending; the warm
                // cache used to skip that and build static submeshes here, so a
                // re-attached rigged garment rendered unrigged and mislocated. Mirror
                // the decode path: defer to the rigged build (and upgrade to the
                // finest block, since a skinned mesh cannot be pixel-area LOD ranked)
                // unless it is worn on a HUD, which has no skeleton to skin to.
                Some(_decoded) if !is_hud && mesh_manager.skin(key).is_some() => {
                    mesh_manager.upgrade_to_finest(key);
                    (
                        Vec::new(),
                        Some(PendingGeometry::RiggedMesh(PendingRiggedMesh {
                            key,
                            texture_entry: object.texture_entry.clone(),
                        })),
                        None,
                        None,
                        None,
                        None,
                    )
                }
                Some(decoded) => (
                    build_mesh_submeshes(
                        &decoded,
                        key,
                        &object.texture_entry,
                        [object.scale.x, object.scale.y, object.scale.z],
                        entity,
                        commands,
                        meshes,
                        materials,
                        manager,
                        prim_textures,
                        priority,
                        cache,
                        intern,
                        material_cache,
                    ),
                    None,
                    None,
                    None,
                    None,
                    // Warm cache: the submeshes were built immediately above, so
                    // (unlike the cold path) no `PendingGeometry::Mesh` will later
                    // set the rebuild inputs. Carry them out here so the pixel-area
                    // LOD driver can rebuild this instance when the shared geometry's
                    // level of detail changes on approach (the stuck-low-LOD fix).
                    Some(PendingMesh {
                        key,
                        texture_entry: object.texture_entry.clone(),
                        scale: [object.scale.x, object.scale.y, object.scale.z],
                        priority,
                        intern: intern.clone(),
                    }),
                ),
                None => (
                    Vec::new(),
                    Some(PendingGeometry::Mesh(PendingMesh {
                        key,
                        texture_entry: object.texture_entry.clone(),
                        scale: [object.scale.x, object.scale.y, object.scale.z],
                        priority,
                        intern: intern.clone(),
                    })),
                    None,
                    None,
                    None,
                    None,
                ),
            }
        }
        ObjectCategory::Sculpt => {
            let Some((map, sculpt_type)) = sculpt_key(object) else {
                return (Vec::new(), None, None, None, None, None);
            };
            manager.request_boosted(map, priority);
            // The store hands back an `Arc`; clone it out so the immutable borrow
            // of `manager` ends before the face build borrows it mutably.
            match manager.decoded(map).map(Arc::clone) {
                Some(map_image) => (
                    build_sculpt_faces(
                        &map_image,
                        map,
                        sculpt_type,
                        &object.texture_entry,
                        [object.scale.x, object.scale.y, object.scale.z],
                        entity,
                        commands,
                        meshes,
                        materials,
                        manager,
                        prim_textures,
                        priority,
                        cache,
                        intern,
                        material_cache,
                    ),
                    None,
                    None,
                    None,
                    None,
                    None,
                ),
                None => (
                    Vec::new(),
                    Some(PendingGeometry::Sculpt(PendingSculpt {
                        map,
                        sculpt_type,
                        texture_entry: object.texture_entry.clone(),
                        scale: [object.scale.x, object.scale.y, object.scale.z],
                        priority,
                        intern: intern.clone(),
                    })),
                    None,
                    None,
                    None,
                    None,
                ),
            }
        }
        ObjectCategory::Tree => (
            build_tree_faces(
                tree_species_byte(object),
                INITIAL_TREE_TIER,
                entity,
                commands,
                meshes,
                materials,
                manager,
                prim_textures,
                priority,
            ),
            None,
            None,
            // Retain the regeneration inputs so the pixel-area LOD driver can
            // rebuild this tree at a different tier as its size changes (P26.2).
            Some(PendingTree {
                species: tree_species_byte(object),
                priority,
            }),
            None,
            None,
        ),
        ObjectCategory::Grass => (
            build_grass_faces(
                object.state,
                [object.scale.x, object.scale.y],
                entity,
                commands,
                meshes,
                materials,
                manager,
                prim_textures,
                priority,
            ),
            // A grass clump is generated immediately from its species and scale
            // (like a tree) and never needs a deferred asset build or an LOD
            // rebuild; a scale change rebuilds it through the shape fingerprint
            // ([`ShapeFingerprint::grass_spread`]).
            None,
            None,
            None,
            None,
            None,
        ),
        ObjectCategory::Avatar | ObjectCategory::Other => {
            (Vec::new(), None, None, None, None, None)
        }
    }
}

/// Tessellate a plain prim at level of detail `lod` and spawn one child
/// entity per non-empty [`PrimFace`](sl_client_bevy::PrimFace) under `parent`,
/// each carrying its geometry mesh, its per-face diffuse material (from the
/// object's decoded [`TextureEntry`](sl_client_bevy::TextureEntry)), and a
/// [`PrimFaceEntity`] tag naming its Linden face index. Returns the spawned face
/// entities so a later shape change or LOD swap can despawn and rebuild them.
///
/// `lod` is the pixel-area-selected tessellation level (P21.3): a new prim starts
/// at [`INITIAL_MANAGED_PRIM_LOD`] and [`apply_prim_lod`] re-tessellates it toward
/// the level its on-screen size warrants.
///
/// Each face's material is built from its `TextureEntry` slot (tint + texture
/// id) by [`face_material`], which requests the texture through `manager` and
/// parks the material in `prim_textures` until it decodes (Phase 6). A face whose
/// slot is missing (an object with no texture entry) falls back to an untextured
/// white material.
///
/// The face geometry stays in the prim's local Second Life space; the object
/// entity's `Transform` carries the object's scale / rotation / position and the
/// single Second Life → Bevy basis change for the whole prim.
///
/// The geometry is shared across identical instances through the
/// [`GeometryCache`] keyed by `(shape, lod)` — tessellation only runs when no
/// live instance already holds the same geometry.
#[expect(
    clippy::too_many_arguments,
    reason = "threads the several ECS resources the geometry build needs, plus the fetch priority and LOD"
)]
fn build_prim_faces(
    object: &Object,
    parent: Entity,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<FaceMaterial>,
    manager: &mut TextureManager,
    prim_textures: &mut PrimTextures,
    priority: Priority,
    lod: PrimLod,
    cache: &mut GeometryCache,
    intern: &MaterialInternContext,
    material_cache: &mut MaterialCache,
) -> Vec<Entity> {
    let shape = object.shape;
    spawn_cached_prim_faces(
        GeometryKey::Prim { shape, lod },
        || tessellate(&PrimShapeFloat::from_params(&shape), lod),
        &object.texture_entry,
        [object.scale.x, object.scale.y, object.scale.z],
        parent,
        commands,
        meshes,
        materials,
        manager,
        prim_textures,
        priority,
        cache,
        intern,
        material_cache,
    )
}

/// The Second Life default flexible-object parameters (Firestorm's
/// `FLEXIBLE_OBJECT_DEFAULT_*`), used only as an unreachable fallback when
/// [`build_flexi_faces`] is somehow called on a prim without a flexible block (the
/// caller gates on its presence).
const DEFAULT_FLEXI: FlexiAttributes = FlexiAttributes {
    softness: 2,
    tension: 1.0,
    air_friction: 0.0,
    gravity: 0.3,
    wind_sensitivity: 0.0,
    user_force: [0.0, 0.0, 0.0],
};

/// Tessellate a **flexi** prim at its chain's rest pose and spawn its face
/// entities under `parent` (the geometry holder), returning them alongside the
/// seeded [`FlexiChain`] (P32.2).
///
/// A flexi prim's geometry is the prim's profile swept along the deformed chain
/// path, at a section count of `1 << softness` (fixed, not pixel-area managed). The
/// chain is initialised from the prim's current pose and the rest path built from
/// it, so the spawn geometry is a straight rest chain;
/// [`simulate_flexi`](crate::flexi::simulate_flexi) then
/// deforms and rewrites these same meshes each frame.
///
/// The faces stay **ordinary `Aabb`-managed entities** — deliberately *not* the
/// skinned-mesh `NoFrustumCulling` opt-out (`viewer-flexi-prim-picking`). The
/// per-frame rewrite goes through `Assets::get_mut`, which marks the mesh asset
/// changed, and Bevy's `calculate_bounds` refreshes a changed mesh's `Aabb` in
/// the same frame (its `AssetChanged<Mesh3d>` branch) — so frustum culling
/// tracks the *bent* geometry rather than the spawn pose. Unlike a skinned mesh
/// (whose deformation happens on the GPU and never exists in the mesh data), a
/// flexi's deformed vertices genuinely live in the asset, so the refreshed
/// bounds are exact. Keeping the `Aabb` is also what makes a flexi **pickable**:
/// `MeshRayCast` reads it non-optionally, so the old opt-out silently excluded
/// flexi prims from left-click touch and the object context menu entirely.
#[expect(
    clippy::too_many_arguments,
    reason = "threads the several ECS resources the geometry build needs, like the sibling build fns"
)]
fn build_flexi_faces(
    object: &Object,
    parent: Entity,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<FaceMaterial>,
    manager: &mut TextureManager,
    prim_textures: &mut PrimTextures,
    priority: Priority,
    intern: &MaterialInternContext,
    material_cache: &mut MaterialCache,
) -> (Vec<Entity>, FlexiChain) {
    let shape = PrimShapeFloat::from_params(&object.shape);
    let attributes = object
        .extra
        .flexible
        .as_ref()
        .map_or(DEFAULT_FLEXI, flexi_attributes);
    let scale = [object.scale.x, object.scale.y, object.scale.z];
    // The prim's pose seeds the chain: for a root this is its world pose, for a
    // child its parent-local pose (the first simulate step re-anchors it from the
    // live world transform, so a child briefly catches up — see `simulate_flexi`).
    let base_position = [
        object.motion.position.x,
        object.motion.position.y,
        object.motion.position.z,
    ];
    let base_rotation = [
        object.motion.rotation.x,
        object.motion.rotation.y,
        object.motion.rotation.z,
        object.motion.rotation.s,
    ];
    let chain = FlexiChain::new(&shape, &attributes, scale, base_position, base_rotation);
    let path = chain.path(base_position, base_rotation, scale);
    let prim = tessellate_with_path(&shape, FLEXI_LOD, &path);
    // A flexi prim's per-frame deformation rewrites its *meshes*, not its
    // materials, so its faces intern like any other prim's.
    let face_entities = spawn_prim_faces(
        &prim,
        &object.texture_entry,
        scale,
        parent,
        commands,
        meshes,
        materials,
        manager,
        prim_textures,
        priority,
        intern,
        material_cache,
    );
    (face_entities, chain)
}

/// Seed or clear a flexi prim's [`FlexiSimState`] (P32.2) on the object entity: a
/// prim that built a chain (`Some`) gets the state so
/// [`simulate_flexi`](crate::flexi::simulate_flexi) drives it;
/// one that did not (a rigid prim, or a prim toggled rigid) has any stale state
/// removed. Mirrors the [`apply_flexi`] block-component reconcile, but for the
/// solver state that rides the built geometry.
fn apply_flexi_sim(
    entity: Entity,
    chain: Option<FlexiChain>,
    object: &Object,
    face_entities: &[Entity],
    commands: &mut Commands,
) {
    match chain {
        Some(chain) => {
            let softness = object
                .extra
                .flexible
                .as_ref()
                .map_or(0, |data| data.softness);
            commands.entity(entity).insert(FlexiSimState {
                chain,
                shape: PrimShapeFloat::from_params(&object.shape),
                softness,
                face_entities: face_entities.to_vec(),
                // Unlatched: the first frames drive the fresh chain onto its rest pose
                // before it latches settled and stops re-uploading.
                rest: None,
            });
        }
        None => {
            commands.entity(entity).remove::<FlexiSimState>();
        }
    }
}

/// Stitch a sculpted prim's decoded sculpt map into geometry and spawn its face
/// entity under `parent`, textured via the Phase 6 pipeline exactly as a plain
/// prim's faces are.
///
/// The map pixels come from the shared [`TextureManager`] (the same fetch /
/// off-thread-decode / disk-cache the Phase 6 texturing drives — the sculpt is
/// not decoded on the render thread), and are stitched by [`tessellate_sculpt`]
/// into a single-face [`PrimMesh`] honouring the object's `sculpt_type`
/// (plane / cylinder / sphere / torus + invert / mirror flags). The resulting face
/// is textured from the object's `TextureEntry` slot 0 and spawned as one child
/// entity, kept in the prim's local Second Life space — the object entity's
/// `Transform` carries its scale / rotation / position and the single basis
/// change, like a plain prim.
///
/// The geometry is shared across identical instances through the
/// [`GeometryCache`] keyed by the map asset (`map_key`), sculpt type, and the
/// decoded map's pixel size — copies of one sculpt stitch the map once, and a
/// re-decode at another discard level is a clean different key.
#[expect(
    clippy::too_many_arguments,
    reason = "threads the several ECS resources the geometry build needs"
)]
fn build_sculpt_faces(
    map: &DecodedTexture,
    map_key: TextureKey,
    sculpt_type: u8,
    texture_entry: &[u8],
    scale: [f32; 3],
    parent: Entity,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<FaceMaterial>,
    manager: &mut TextureManager,
    prim_textures: &mut PrimTextures,
    priority: Priority,
    cache: &mut GeometryCache,
    intern: &MaterialInternContext,
    material_cache: &mut MaterialCache,
) -> Vec<Entity> {
    spawn_cached_prim_faces(
        GeometryKey::Sculpt {
            map: map_key,
            sculpt_type,
            width: map.width,
            height: map.height,
        },
        || tessellate_sculpt(map, sculpt_type),
        texture_entry,
        scale,
        parent,
        commands,
        meshes,
        materials,
        manager,
        prim_textures,
        priority,
        cache,
        intern,
        material_cache,
    )
}

/// The `LLVOTree` species byte of a tree object: the first byte of its `Data`
/// (genome) field, matching the reference viewer's `mSpecies = ((U8 *)mData)[0]`
/// (`LLVOTree::processUpdateMessage`).
///
/// The `state` byte is **not** the tree species — for a prim it is the
/// attachment-point slot, and Second Life leaves it zero for a tree, carrying the
/// species only in `Data` (a full update's one-byte `Data` field; a compressed
/// update's inline genome byte under the tree flag). Reading `state` rendered
/// every SL tree as species `0` ("Pine 1", a large evergreen) regardless of its
/// real species — big evergreens where the region had autumn trees and ferns.
///
/// The reference never consults `State` for a tree: `mSpecies` is set only from
/// `mData[0]`, and a missing `Data` leaves the prior species (`0` initially).
/// Both grids always send `Data` for a tree (SL only there; OpenSim redundantly
/// in `State` too), so an absent `Data` is degenerate — we default to species `0`
/// like the reference rather than fall back to `state`, which would just
/// reintroduce this bug. Grass differs — the reference reads *its* species from
/// `State` (`getAttachmentState`), so [`build_grass_faces`] keeps using
/// `object.state`.
fn tree_species_byte(object: &Object) -> u8 {
    object.data.first().copied().unwrap_or(0)
}

/// Generate a Linden tree's branch / leaf geometry for `species_byte` at
/// [`TreeTier`] `tier` and spawn its single face entity under `parent`, textured
/// with the species diffuse through the Phase 6 pipeline (P26.2).
///
/// The species byte comes from [`tree_species_byte`] (the object's `Data` genome,
/// not `state`); an out-of-range value clamps to species `0`, matching the
/// reference viewer. The geometry (a branch/leaf mesh
/// at the tier's trunk level of detail, or the crossed-quad billboard imposter)
/// is generated by `sl_tree` in unit-outer-scale Second Life space and sized by
/// the tree's [`holder_transform`]. Its diffuse texture is the species'
/// `texture_id` (its trunk region textures the cylinders, its leaf-card region the
/// leaves), fetched and applied exactly as a prim face's — a synthetic white
/// [`TextureFace`] carrying the species texture drives [`face_material`], so a
/// tree's leaf alpha upgrades it to blending on decode like any other face.
#[expect(
    clippy::too_many_arguments,
    reason = "threads the several ECS resources the geometry build needs"
)]
fn build_tree_faces(
    species_byte: u8,
    tier: TreeTier,
    parent: Entity,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<FaceMaterial>,
    manager: &mut TextureManager,
    prim_textures: &mut PrimTextures,
    priority: Priority,
) -> Vec<Entity> {
    // Clamp an unknown species to 0, as the reference viewer does (species 0 is
    // always defined, so the fallback resolves).
    let Some(species) = tree_species(species_byte).or_else(|| tree_species(0)) else {
        return Vec::new();
    };
    let tree = match tier {
        TreeTier::Lod(lod) => tree_geometry(species, lod),
        TreeTier::Billboard => tree_billboard_geometry(species),
    };
    let mesh = meshes.add(to_bevy_tree_mesh(&tree));
    // The tree's single diffuse comes from the species table, not a `TextureEntry`.
    let texture_face = TextureFace::new(species.texture_id);
    let material = face_material(
        &texture_face,
        materials,
        manager,
        prim_textures,
        priority,
        TextureAlpha::Mask,
    );
    // Foliage is alpha-**masked** (cutout), not opaque or blended: the reference
    // viewer renders trees in the alpha-mask pool so the leaf-card texture's alpha
    // clips each leaf to its shape (transparent around the edges) rather than
    // showing a solid quad. A fixed cutoff clips the trunk (opaque) cleanly too.
    // Set here so it is not overridden by the tint-based opaque/blend default.
    if let Some(mut tree_material) = materials.get_mut(&material) {
        tree_material.base.alpha_mode = AlphaMode::Mask(TREE_ALPHA_CUTOFF);
    }
    let entity = commands
        .spawn((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            PrimFaceEntity {
                face_id: PrimFaceId::new(0),
            },
            FaceTextureDebug(texture_face),
            ChildOf(parent),
        ))
        .id();
    vec![entity]
}

/// Generate a grass clump's crossed-quad blade geometry for the species in
/// `species_byte`, spread over the object's X/Y `scale`, and spawn its single face
/// entity under `parent`, textured with the species diffuse through the Phase 6
/// pipeline (P26.3) — the grass counterpart of [`build_tree_faces`].
///
/// The species byte is the object's `state`; an out-of-range value clamps to
/// species `0`, matching the reference viewer's substitution. The geometry (a fan
/// of up to [`GRASS_MAX_BLADES`] leaning blade cards) is generated by `sl_tree` in
/// absolute-metre Second Life space with the object scale folded into the blade
/// spread, so it is placed by an identity [`holder_transform`] (no further scale).
/// Its diffuse texture is the species' `texture_id`, fetched and applied exactly as
/// a prim face's — a synthetic white [`TextureFace`] drives [`face_material`].
///
/// Grass renders in the reference viewer's **alpha-blend** pool (`PASS_GRASS` /
/// `POOL_ALPHA`), so the material is forced to [`AlphaMode::Blend`] here (rather
/// than the cutout mask used for trees) to reproduce the soft-edged blades.
#[expect(
    clippy::too_many_arguments,
    reason = "threads the several ECS resources the geometry build needs"
)]
fn build_grass_faces(
    species_byte: u8,
    scale: [f32; 2],
    parent: Entity,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<FaceMaterial>,
    manager: &mut TextureManager,
    prim_textures: &mut PrimTextures,
    priority: Priority,
) -> Vec<Entity> {
    // Clamp an unknown species to 0, as the reference viewer does (species 0 is
    // always defined, so the fallback resolves).
    let Some(species) = grass_species(species_byte).or_else(|| grass_species(0)) else {
        return Vec::new();
    };
    let clump = grass_geometry(species, scale[0], scale[1], GRASS_MAX_BLADES);
    let mesh = meshes.add(to_bevy_grass_mesh(&clump));
    // The clump's single diffuse comes from the species table, not a `TextureEntry`.
    let texture_face = TextureFace::new(species.texture_id);
    let material = face_material(
        &texture_face,
        materials,
        manager,
        prim_textures,
        priority,
        TextureAlpha::Mask,
    );
    // Grass is alpha-**blended** (the reference's `PASS_GRASS` / `POOL_ALPHA`), so
    // the soft blade-card edges fade rather than clip. Set here so it is not
    // overridden by the tint-based opaque default.
    if let Some(mut grass_material) = materials.get_mut(&material) {
        grass_material.base.alpha_mode = AlphaMode::Blend;
    }
    let entity = commands
        .spawn((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            PrimFaceEntity {
                face_id: PrimFaceId::new(0),
            },
            FaceTextureDebug(texture_face),
            ChildOf(parent),
        ))
        .id();
    vec![entity]
}

/// Overwrite a face `mesh`'s UV0 with planar-texgen coordinates when its
/// `texture_face` requests planar mapping (`TEX_GEN_PLANAR`).
///
/// A planar face ignores the volume's stored UVs; the reference viewer projects
/// each vertex's texture coordinate from its position (in the object's local
/// Second Life space, scaled by the object size) and normal via
/// [`planar_texgen_uv`]. The projected coordinate gets the same `1 − v` flip
/// [`to_bevy_prim_mesh`] / [`to_bevy_mesh`] apply to stored UVs, so a planar face
/// samples the texture the same way up; the per-face repeats / offset / rotation
/// still apply afterwards through the material's `uv_transform`, matching the
/// reference viewer's order (`planarProjection` then `xform`). A no-op for a
/// non-planar face, or when the face carries no per-vertex normals to project
/// from.
fn apply_planar_texgen(
    mesh: &mut Mesh,
    positions: &[[f32; 3]],
    normals: &[[f32; 3]],
    texture_face: &TextureFace,
    scale: [f32; 3],
) {
    if !texture_face.is_planar_texgen() || normals.len() != positions.len() {
        return;
    }
    let uvs: Vec<[f32; 2]> = positions
        .iter()
        .zip(normals.iter())
        .map(|(&position, &normal)| {
            let [u, v] = planar_texgen_uv(position, normal, scale);
            [u, 1.0 - v]
        })
        .collect();
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
}

/// Spawn one child entity per non-empty [`PrimFace`](sl_client_bevy::PrimFace) of
/// a tessellated [`PrimMesh`] under `parent`, each carrying its geometry mesh, its
/// per-face diffuse material (from `texture_entry`), and a [`PrimFaceEntity`] tag.
/// Returns the spawned face entities so a later shape change can despawn and
/// rebuild them. Shared by the plain-prim ([`build_prim_faces`]) and sculpt
/// ([`build_sculpt_faces`]) paths, which differ only in how the `PrimMesh` was
/// produced.
///
/// Each face's material is built from its `TextureEntry` slot (tint + texture id)
/// by [`face_material`], which requests the texture through `manager` and parks
/// the material in `prim_textures` until it decodes (Phase 6). A face whose slot is
/// missing (an object with no texture entry) falls back to an untextured white
/// material. The face geometry stays in the object's local Second Life space.
#[expect(
    clippy::too_many_arguments,
    reason = "threads the several ECS resources the geometry build needs"
)]
fn spawn_prim_faces(
    prim: &PrimMesh,
    texture_entry: &[u8],
    scale: [f32; 3],
    parent: Entity,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<FaceMaterial>,
    manager: &mut TextureManager,
    prim_textures: &mut PrimTextures,
    priority: Priority,
    intern: &MaterialInternContext,
    material_cache: &mut MaterialCache,
) -> Vec<Entity> {
    let entry = decode_texture_entry(texture_entry, prim.faces.len());
    // The slot every face falls back to when the object carries no texture entry:
    // an untextured, opaque-white (untinted) face.
    let default_face = TextureFace::new(TextureKey::from(Uuid::nil()));
    let mut face_entities = Vec::new();
    for face in &prim.faces {
        if face.is_empty() {
            continue;
        }
        let texture_face = entry.face(face.face_id.as_usize()).unwrap_or(&default_face);
        let mut bevy_mesh = to_bevy_prim_mesh(face);
        apply_planar_texgen(
            &mut bevy_mesh,
            &face.positions,
            &face.normals,
            texture_face,
            scale,
        );
        let mesh = meshes.add(bevy_mesh);
        let entity = spawn_face_entity(
            mesh,
            texture_face,
            face.face_id,
            parent,
            commands,
            materials,
            manager,
            prim_textures,
            priority,
            intern,
            material_cache,
        );
        face_entities.push(entity);
    }
    face_entities
}

/// Spawn one face child entity under `parent`, carrying `mesh` and the per-face
/// diffuse material built from `texture_face` (via [`intern_face_material`],
/// which requests the texture through `manager` and parks the material in
/// `prim_textures` until it decodes — the Phase 6 pipeline). The shared tail of
/// every face-geometry build path, cached or not — and the interception point
/// of the cross-instance [`MaterialCache`]: a face `intern` judges internable
/// shares one material handle with every identical face (so matched-geometry
/// copies batch into instanced draws) and is marked [`SharedFaceMaterial`] for
/// the copy-on-write detach net; an excluded face keeps a private material.
#[expect(
    clippy::too_many_arguments,
    reason = "threads the several ECS resources the material build needs"
)]
fn spawn_face_entity(
    mesh: Handle<Mesh>,
    texture_face: &TextureFace,
    face_id: PrimFaceId,
    parent: Entity,
    commands: &mut Commands,
    materials: &mut Assets<FaceMaterial>,
    manager: &mut TextureManager,
    prim_textures: &mut PrimTextures,
    priority: Priority,
    intern: &MaterialInternContext,
    material_cache: &mut MaterialCache,
) -> Entity {
    let (material, shared) = intern_face_material(
        texture_face,
        intern.internable(face_id, texture_face),
        material_cache,
        materials,
        manager,
        prim_textures,
        priority,
    );
    let mut face = commands.spawn((
        Mesh3d(mesh),
        MeshMaterial3d(material),
        PrimFaceEntity { face_id },
        FaceTextureDebug(*texture_face),
        ChildOf(parent),
    ));
    if shared {
        face.insert(SharedFaceMaterial);
    }
    face.id()
}

/// Try to spawn `key`'s faces purely from the cross-instance [`GeometryCache`]:
/// when **every** non-empty face revives a live shared mesh handle (the planar
/// variant at this instance's quantized scale for a planar-texgen face, the
/// scale-independent one otherwise), spawn the face entities with zero
/// tessellation / conversion work and return them (`Ok`).
///
/// Otherwise return the partial revival (`Err`): the handles that *did* revive,
/// keyed by face id, so the caller's geometry build reuses them and only builds
/// the rest. An unrecorded key yields an empty map.
#[expect(
    clippy::too_many_arguments,
    reason = "threads the several ECS resources the face spawn needs"
)]
fn spawn_revived_faces(
    key: &GeometryKey,
    texture_entry: &[u8],
    quantized: ScaleMm,
    parent: Entity,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<FaceMaterial>,
    manager: &mut TextureManager,
    prim_textures: &mut PrimTextures,
    priority: Priority,
    cache: &mut GeometryCache,
    intern: &MaterialInternContext,
    material_cache: &mut MaterialCache,
) -> Result<Vec<Entity>, HashMap<PrimFaceId, Handle<Mesh>>> {
    let Some(face_count) = cache.cached_face_count(key) else {
        return Err(HashMap::new());
    };
    let entry = decode_texture_entry(texture_entry, face_count);
    // The slot every face falls back to when the object carries no texture entry:
    // an untextured, opaque-white (untinted, non-planar) face.
    let default_face = TextureFace::new(TextureKey::from(Uuid::nil()));
    let Some(revived) = cache.revive(
        key,
        quantized,
        |face_id| {
            entry
                .face(face_id.as_usize())
                .unwrap_or(&default_face)
                .is_planar_texgen()
        },
        meshes,
    ) else {
        return Err(HashMap::new());
    };
    if !revived.complete() {
        return Err(revived
            .faces
            .into_iter()
            .filter_map(|face| face.mesh.map(|mesh| (face.face_id, mesh)))
            .collect());
    }
    cache.note_hit();
    let face_entities = revived
        .faces
        .into_iter()
        .filter_map(|face| {
            let mesh = face.mesh?;
            let texture_face = entry.face(face.face_id.as_usize()).unwrap_or(&default_face);
            Some(spawn_face_entity(
                mesh,
                texture_face,
                face.face_id,
                parent,
                commands,
                materials,
                manager,
                prim_textures,
                priority,
                intern,
                material_cache,
            ))
        })
        .collect();
    Ok(face_entities)
}

/// Spawn the face entities of a cacheable tessellated geometry (a plain prim or
/// a sculpt), sharing mesh handles across identical instances through the
/// [`GeometryCache`]: a full revival spawns without running
/// `tessellate_geometry` at all; otherwise the geometry is produced once and
/// only the faces without a live shared asset are converted and uploaded (and
/// recorded for the next instance). A planar-texgen face bakes the object
/// scale into its UVs ([`apply_planar_texgen`]), so it shares per quantized
/// scale rather than unconditionally.
#[expect(
    clippy::too_many_arguments,
    reason = "threads the several ECS resources the geometry build needs"
)]
fn spawn_cached_prim_faces(
    key: GeometryKey,
    tessellate_geometry: impl FnOnce() -> PrimMesh,
    texture_entry: &[u8],
    scale: [f32; 3],
    parent: Entity,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<FaceMaterial>,
    manager: &mut TextureManager,
    prim_textures: &mut PrimTextures,
    priority: Priority,
    cache: &mut GeometryCache,
    intern: &MaterialInternContext,
    material_cache: &mut MaterialCache,
) -> Vec<Entity> {
    let quantized = scale_mm(scale);
    let mut revived = match spawn_revived_faces(
        &key,
        texture_entry,
        quantized,
        parent,
        commands,
        meshes,
        materials,
        manager,
        prim_textures,
        priority,
        cache,
        intern,
        material_cache,
    ) {
        Ok(face_entities) => return face_entities,
        Err(partial) => partial,
    };
    let any_revived = !revived.is_empty();
    let prim = tessellate_geometry();
    let entry = decode_texture_entry(texture_entry, prim.faces.len());
    // The slot every face falls back to when the object carries no texture entry:
    // an untextured, opaque-white (untinted) face.
    let default_face = TextureFace::new(TextureKey::from(Uuid::nil()));
    cache.ensure_entry(key, prim.faces.len());
    let mut face_entities = Vec::new();
    for face in &prim.faces {
        if face.is_empty() {
            continue;
        }
        let texture_face = entry.face(face.face_id.as_usize()).unwrap_or(&default_face);
        let mesh = match revived.remove(&face.face_id) {
            Some(mesh) => mesh,
            None => {
                let mut bevy_mesh = to_bevy_prim_mesh(face);
                apply_planar_texgen(
                    &mut bevy_mesh,
                    &face.positions,
                    &face.normals,
                    texture_face,
                    scale,
                );
                // Whether planar UVs were actually baked (the same condition
                // `apply_planar_texgen` no-ops on): only then is the mesh a
                // scale-dependent variant.
                let planar =
                    texture_face.is_planar_texgen() && face.normals.len() == face.positions.len();
                let mesh = meshes.add(bevy_mesh);
                cache.record_face(key, face.face_id, planar.then_some(quantized), mesh.id());
                mesh
            }
        };
        let entity = spawn_face_entity(
            mesh,
            texture_face,
            face.face_id,
            parent,
            commands,
            materials,
            manager,
            prim_textures,
            priority,
            intern,
            material_cache,
        );
        face_entities.push(entity);
    }
    if any_revived {
        cache.note_partial_hit();
    } else {
        cache.note_miss();
    }
    face_entities
}

/// Spawn one child entity per non-empty submesh of a decoded mesh under `parent`,
/// each carrying its geometry mesh, its per-face diffuse material (from the
/// object's decoded [`TextureEntry`](sl_client_bevy::TextureEntry) slot), and a
/// [`PrimFaceEntity`] tag naming the submesh (Linden face) index. Returns the
/// spawned entities so a later shape change can despawn and rebuild them.
///
/// Each submesh maps to one Linden face: the material comes from the object's
/// `TextureEntry` slot at the submesh's index (via [`face_material`], sharing the
/// Phase 6 texture pipeline), and empty `NoGeometry` submeshes are skipped while
/// still counting as a face slot (so later submeshes keep their correct index).
/// The mesh geometry stays in the object's local Second Life space; the object
/// entity's `Transform` carries the object's scale / rotation / position and the
/// single Second Life → Bevy basis change for the whole object.
///
/// The converted Bevy meshes are shared across identical instances through the
/// [`GeometryCache`] keyed by `(mesh_key, decoded.lod)`: copies of one mesh
/// asset (which already share the *decoded* geometry via `MeshManager`)
/// additionally share one converted, GPU-uploaded mesh per submesh instead of
/// each re-converting and re-uploading their own.
#[expect(
    clippy::too_many_arguments,
    reason = "threads the several ECS resources the geometry build needs"
)]
fn build_mesh_submeshes(
    decoded: &DecodedMesh,
    mesh_key: MeshKey,
    texture_entry: &[u8],
    scale: [f32; 3],
    parent: Entity,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<FaceMaterial>,
    manager: &mut TextureManager,
    prim_textures: &mut PrimTextures,
    priority: Priority,
    cache: &mut GeometryCache,
    intern: &MaterialInternContext,
    material_cache: &mut MaterialCache,
) -> Vec<Entity> {
    let key = GeometryKey::Mesh {
        mesh: mesh_key,
        lod: decoded.lod,
    };
    let quantized = scale_mm(scale);
    let mut revived = match spawn_revived_faces(
        &key,
        texture_entry,
        quantized,
        parent,
        commands,
        meshes,
        materials,
        manager,
        prim_textures,
        priority,
        cache,
        intern,
        material_cache,
    ) {
        Ok(face_entities) => return face_entities,
        Err(partial) => partial,
    };
    let any_revived = !revived.is_empty();
    let entry = decode_texture_entry(texture_entry, decoded.submeshes.len());
    // The slot every face falls back to when the object carries no texture entry:
    // an untextured, opaque-white (untinted) face.
    let default_face = TextureFace::new(TextureKey::from(Uuid::nil()));
    cache.ensure_entry(key, decoded.submeshes.len());
    let mut face_entities = Vec::new();
    for (index, submesh) in decoded.submeshes.iter().enumerate() {
        // Skip a submesh with no renderable geometry — the explicit `NoGeometry`
        // marker *or* a `no_geometry == false` submesh that still decoded to zero
        // vertices. Converting the latter yields a zero-vertex mesh the GPU mesh
        // allocator floods the log over (viewer-r26) and which renders nothing.
        if !submesh.has_geometry() {
            continue;
        }
        let texture_face = entry.face(index).unwrap_or(&default_face);
        // The submesh index is the Linden face index; a mesh has few faces, so the
        // widening never saturates in practice (a clamp keeps it lint-clean).
        let face_id = PrimFaceId::new(u16::try_from(index).unwrap_or(u16::MAX));
        let mesh = match revived.remove(&face_id) {
            Some(mesh) => mesh,
            None => {
                let mut bevy_mesh = to_bevy_mesh(submesh);
                apply_planar_texgen(
                    &mut bevy_mesh,
                    &submesh.positions,
                    &submesh.normals,
                    texture_face,
                    scale,
                );
                // Whether planar UVs were actually baked (the same condition
                // `apply_planar_texgen` no-ops on): only then is the mesh a
                // scale-dependent variant.
                let planar = texture_face.is_planar_texgen()
                    && submesh.normals.len() == submesh.positions.len();
                let mesh = meshes.add(bevy_mesh);
                cache.record_face(key, face_id, planar.then_some(quantized), mesh.id());
                mesh
            }
        };
        let entity = spawn_face_entity(
            mesh,
            texture_face,
            face_id,
            parent,
            commands,
            materials,
            manager,
            prim_textures,
            priority,
            intern,
            material_cache,
        );
        face_entities.push(entity);
    }
    if any_revived {
        cache.note_partial_hit();
    } else {
        cache.note_miss();
    }
    face_entities
}

/// Despawn every face child entity of a prim (used before rebuilding on a shape
/// change), leaving the caller to clear the tracked list.
fn despawn_prim_faces(face_entities: &[Entity], commands: &mut Commands) {
    for &face in face_entities {
        commands.entity(face).try_despawn();
    }
}

/// Mirror the object's `llSetText` floating text onto its entity as an
/// [`ObjectFloatingText`], which [`crate::hover_text`] renders as a world-space
/// billboard. Removed when the text is cleared (`llSetText("")` — an empty
/// string) or when the object is a HUD attachment (whose floating text renders
/// in HUD screen space, not the world, and is out of scope here). Refreshed on
/// every update; a terse motion update carries the cached text unchanged, so
/// the mirrored value stays put and the change-guarded renderer stays quiet.
fn apply_floating_text(entity: Entity, object: &Object, is_hud: bool, commands: &mut Commands) {
    use crate::hover_text::ObjectFloatingText;
    if is_hud || object.text.is_empty() {
        commands.entity(entity).remove::<ObjectFloatingText>();
    } else {
        commands.entity(entity).insert(ObjectFloatingText {
            text: object.text.clone(),
            raw_color: object.text_color,
        });
    }
}

/// Reconcile an object entity's [`ObjectLight`] component (P25.1) with its current
/// light block: insert / refresh it when the object is a light source, remove it
/// when the light was cleared in-world. Called on both the spawn and update paths
/// so a light toggled on or off between updates is tracked.
fn apply_light(entity: Entity, light: Option<ObjectLight>, commands: &mut Commands) {
    match light {
        Some(light) => {
            debug!(
                "object light: spotlight={} emitted={:?} radius={:.2}m falloff={:.2} \
                 cutoff={:.1}deg",
                light.is_spotlight(),
                light.effective_linear_color(),
                light.radius,
                light.falloff,
                light.cutoff,
            );
            commands.entity(entity).insert(light);
        }
        None => {
            commands.entity(entity).remove::<ObjectLight>();
        }
    }
}

/// Drop the tracked object under `scoped` when its entity has been despawned out
/// from under the map — a linkset child or worn attachment that Bevy's recursive
/// despawn took with its parent (a removed linkset root, or a departed avatar whose
/// skeleton-joint node it hangs off), with no `remove_object` to clean the entry.
///
/// `is_alive` reports whether an entity is still spawned (in the viewer,
/// `Commands::get_entity(..).is_ok()`). A live entity is left untouched — this never
/// drops an object still on screen, so no live transform / material write is lost.
/// Returns the dropped entity when a stale entry was removed, else `None`.
fn drop_stale_tracked_entity(
    state: &mut ObjectState,
    scoped: ScopedObjectId,
    mut is_alive: impl FnMut(Entity) -> bool,
) -> Option<Entity> {
    let entity = state.objects.get(&scoped)?.entity;
    if is_alive(entity) {
        return None;
    }
    let _stale = state.objects.remove(&scoped);
    Some(entity)
}

/// Spawn or update the entity for `object`, keeping its transform, classification,
/// and linkset parenting current.
#[expect(
    clippy::too_many_arguments,
    reason = "threads the several ECS resources the geometry build needs"
)]
/// Apply one object update — spawn a new object, or move / reshape / retexture a
/// known one. Returns `true` when it **built geometry** (a new spawn, or a known
/// object's shape/texture re-tessellation), which creates the object's face materials
/// and is what [`update_objects`] budgets per frame; `false` for a motion-only move,
/// a component-only refresh, or anything that touched no geometry.
fn apply_object(
    state: &mut ObjectState,
    object: &Object,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<FaceMaterial>,
    manager: &mut TextureManager,
    prim_textures: &mut PrimTextures,
    mesh_manager: &mut MeshManager,
    cache: &mut GeometryCache,
    material_cache: &mut MaterialCache,
) -> bool {
    let scoped = object.scoped_id();
    let parent = object.scoped_parent_id();
    let is_root = object.parent_id.get() == 0;
    // A non-zero attachment-point id marks an attachment worn on an avatar: its
    // `parent` is the avatar, and it is parented to that avatar's skeleton joint
    // (P16.1) by `adopt_pending_attachments`, not to a linkset root here.
    let attachment_point = object.attachment_point_id();
    let category = classify(object);
    let shape = ShapeFingerprint::of(object);
    let transform = object_transform(object, is_root, state.origin);
    // The parent's entity, if its root is already tracked (looked up before the
    // mutable borrow of the object's own entry below). A root has no parent, and
    // an attachment is left for the skeleton-joint parenting path — both `None`.
    let parent_entity = if is_root || attachment_point.is_some() {
        None
    } else {
        state.objects.get(&parent).map(|root| root.entity)
    };
    // Whether this object is worn on a HUD (itself or via its linkset root). Two
    // things need it: a rigged mesh on a HUD is built as static HUD geometry
    // rather than skinned to a body skeleton (the warm-cache mesh build needs it
    // up front; the cold-cache decode path derives it once the object is
    // tracked), and a HUD face's material is mutated in place (forced fullbright)
    // so HUD faces never intern.
    let is_hud = object_in_hud_attachment(state, attachment_point, is_root, parent);
    // The object-level inputs of the per-face material-intern decision, threaded
    // into every face build this update runs (and retained by the deferred mesh /
    // sculpt / LOD rebuilds).
    let intern = MaterialInternContext::for_object(object, is_hud);

    // The per-block components (light P25.1, particles P30.1, flexi P32.1,
    // reflection probe P33) are derived from the object where they are applied
    // — the spawn path below, and the known-object path's block refresh (which
    // a motion-only update skips entirely, derivations included).

    // The crosshair pick tool's identity for this object (full id, mesh/sculpt
    // asset, Second Life scale/position), refreshed with each update.
    let debug_info = ObjectDebugInfo {
        full_id: object.full_id.uuid(),
        asset: mesh_key(object)
            .map(|key| key.uuid())
            .or_else(|| sculpt_key(object).map(|(key, _type)| key.uuid())),
        scale: [object.scale.x, object.scale.y, object.scale.z],
        position: [
            object.motion.position.x,
            object.motion.position.y,
            object.motion.position.z,
        ],
        shape: object.shape,
    };
    // The Second Life transform mirror the object-editing surfaces read (and
    // the frame an edit sends back over `MultipleObjectUpdate`).
    let sl_motion = ObjectSlMotion {
        position: object.motion.position.clone(),
        rotation: object.motion.rotation.clone(),
        scale: object.scale.clone(),
        is_root,
        attachment: attachment_point.is_some(),
    };

    // A tracked object whose entity was despawned out from under us leaves a stale
    // entry: Bevy's recursive despawn takes a linkset child — or a worn attachment
    // hanging off an avatar's skeleton-joint node — the instant its parent despawns
    // (a linkset root removed, or an avatar that left), and that hierarchy despawn
    // does not run our own `remove_object`, so nothing drops the entry from the map.
    // Re-inserting on its dead (and, once the slot is reused, generation-mismatched)
    // entity is the `bevy_ecs::error::handler` "Entity despawned" warning this fixes.
    // An objects.rs entity only dies via `remove_object` (which cleans the map) or a
    // parent's hierarchy despawn, so a dead tracked entity means the object's parent
    // is gone: drop the stale entry and fall through to the spawn path, re-creating
    // it if the simulator still streams it (else its imminent `KillObject` reaps it).
    if let Some(stale) =
        drop_stale_tracked_entity(state, scoped, |entity| commands.get_entity(entity).is_ok())
    {
        debug!(
            "object {scoped}: tracked entity {stale:?} was despawned externally (parent hierarchy gone); respawning"
        );
    }

    if let Some(existing) = state.objects.get_mut(&scoped) {
        // A known object: re-place it and refresh its classification (a
        // motion-only update stops here — the geometry is untouched). The scale
        // rides the geometry holder, refreshed here so a live resize is applied
        // without a re-tessellation.
        // The pose transforms go through `set_if_neq` via `entry`, NOT a plain
        // `insert`: re-inserting an identical `Transform` marks it changed,
        // which dirties the whole transform tree above the object — for a worn
        // (HUD) object whose script draws a steady update stream, that reverted
        // the wearer's driver-written joint globals to rest every update and
        // pinned the pose gate awake.
        commands
            .entity(existing.entity)
            .entry::<Transform>()
            .and_modify(move |mut current| {
                current.set_if_neq(transform);
            })
            .or_insert(transform);
        // The pick/edit mirrors go through `set_if_neq` too: they genuinely
        // change on every motion packet for a mover, but a repeated identical
        // update (a select echo on a static object) must not mark them changed.
        commands
            .entity(existing.entity)
            .entry::<ObjectDebugInfo>()
            .and_modify(move |mut current| {
                current.set_if_neq(debug_info);
            })
            .or_insert(debug_info);
        let sl_motion_modify = sl_motion.clone();
        commands
            .entity(existing.entity)
            .entry::<ObjectSlMotion>()
            .and_modify(move |mut current| {
                current.set_if_neq(sl_motion_modify);
            })
            .or_insert(sl_motion);
        let holder = holder_transform(object, category);
        commands
            .entity(existing.geometry)
            .entry::<Transform>()
            .and_modify(move |mut current| {
                current.set_if_neq(holder);
            })
            .or_insert(holder);
        // A texture-only change (same shape, a new `TextureEntry` from a retexture
        // in-world or the sim's echo of the build floater's `ObjectImage` send)
        // re-tessellates too: the per-face materials (tint / repeats / offset /
        // rotation / glow / shiny / fullbright) and the planar-texgen UVs are baked
        // at build time, so rebuilding the faces is what makes a texture edit show.
        // Guarded on a non-empty incoming entry, since a terse motion update carries
        // none and must not be read as "cleared to empty".
        let texture_changed =
            !object.texture_entry.is_empty() && object.texture_entry != existing.texture_entry;
        // Whether this update re-tessellates (rebuilds the faces / face materials) —
        // captured here, before `existing.shape` is overwritten below, so it can be
        // returned as this call's "built geometry" verdict for the spawn budget.
        let rebuilt = existing.shape != shape || texture_changed;
        // The terse-update fast path: a motion-only update (the overwhelmingly
        // most frequent object event — every mover at up to sim frame rate)
        // changes none of the per-block component inputs, so the whole helper
        // cascade below — and each absent block's no-op remove command — is
        // skipped. The merged snapshot semantics make the comparison exact:
        // sl-proto re-emits the full cached object, so an unchanged block is
        // byte-identical to the one last applied.
        let refresh_blocks = rebuilt
            || existing.non_motion_blocks_changed(object, is_root, parent, attachment_point);
        if refresh_blocks {
            commands.entity(existing.entity).insert(SceneObject {
                scoped_id: scoped,
                category,
            });
            // Keep the world-root marker in step with a live relink/unlink so
            // [`recenter_objects`] re-bases exactly the roots (a child that just
            // became a root gains it; a root demoted to a child loses it) — an
            // is_root change always lands here via the fingerprint.
            sync_world_root_marker(existing.entity, is_root, commands);
            apply_render_materials(existing.geometry, scoped, object, commands);
            apply_texture_animation(existing.geometry, object, commands);
            // The light (P25.1) / particle (P30.1) / flexi (P32.1) / probe
            // (P33) blocks: each helper inserts its component when the block is
            // present and removes it when absent, so one toggled off / retuned
            // in-world is reflected.
            apply_light(existing.entity, light_from_object(object), commands);
            apply_particles(existing.entity, particles_from_object(object), commands);
            apply_flexi(existing.entity, flexi_from_object(object), commands);
            apply_reflection_probe(
                existing.entity,
                reflection_probe_from_object(object),
                commands,
            );
            // Attach / refresh / drop the physics body marker (P31.2) so a prim
            // toggled physical is driven kinematically.
            apply_physics(existing.entity, object, commands);
            // Mirror the object's floating text (`llSetText`, viewer-hover-text)
            // so a script setting / changing / clearing it is reflected live.
            apply_floating_text(existing.entity, object, is_hud, commands);
        } else {
            // Motion-only: a physical mover still needs its authoritative
            // motion snapshot re-seeded (a fresh `PhysicalObject` insert
            // restarts the dead-reckoning); the physics flag itself is known
            // unchanged, so the non-physical case pays nothing.
            crate::physics::refresh_physical_motion(existing.entity, object, commands);
        }
        if rebuilt {
            // A genuine shape (or category) change, or a texture change: drop the
            // old face meshes and re-tessellate. A category change is subsumed
            // here, since the fingerprint covers pcode and the sculpt/mesh key.
            debug!("object {scoped} shape/texture changed; re-tessellating");
            despawn_prim_faces(&existing.face_entities, commands);
            let (face_entities, pending, prim_rebuild, tree_rebuild, flexi_chain, mesh_rebuild) =
                build_object_geometry(
                    object,
                    category,
                    existing.geometry,
                    is_hud,
                    commands,
                    meshes,
                    materials,
                    manager,
                    prim_textures,
                    mesh_manager,
                    cache,
                    &intern,
                    material_cache,
                );
            // Seed or clear the flexi chain state (P32.2): a prim that is (still) flexi
            // gets a fresh chain at the new softness / geometry; one toggled rigid drops
            // it so [`simulate_flexi`] stops driving stale faces.
            apply_flexi_sim(
                existing.entity,
                flexi_chain,
                object,
                &face_entities,
                commands,
            );
            existing.face_entities = face_entities;
            existing.pending = pending;
            // The geometry was re-requested from scratch; any prior LOD-rebuild
            // inputs are stale (the mesh key, scale, or category may have changed)
            // and are re-established from the new build: a cold-cache mesh's on its
            // next decode (P21.2), a warm-cache mesh's immediately here (built now,
            // so `mesh_rebuild` is set from the build — the stuck-low-LOD fix), a
            // plain prim's immediately here (P21.3), a tree's here (P26.2). An object
            // that changed category drops the rebuild inputs it no longer has (each
            // is `None`).
            existing.mesh_rebuild = mesh_rebuild;
            existing.prim_rebuild = prim_rebuild;
            existing.prim_lod = INITIAL_MANAGED_PRIM_LOD;
            existing.tree_rebuild = tree_rebuild;
            existing.tree_tier = INITIAL_TREE_TIER;
            existing.shape = shape;
        }
        // Reconcile parenting: an object relinked to a root becomes a child of
        // it; an unlinked one (now a root) drops its parent. A child whose new
        // root is not tracked yet is left parentless until it arrives. An
        // attachment keeps its skeleton-joint parent (managed by
        // [`adopt_pending_attachments`]) rather than reconciling a linkset root.
        if attachment_point.is_none() {
            let parent_changed = existing.parent != parent;
            reconcile_parent(existing, is_root, parent_entity, parent_changed, commands);
        }
        existing.parent = parent;
        existing.is_root = is_root;
        existing.attachment_point = attachment_point;
        existing.animated = is_animated_object(object);
        existing.full_key = object.full_id;
        existing.update_flags = object.update_flags;
        existing.material = object.material;
        existing.extra = object.extra.clone();
        existing.texture_animation = object.texture_animation;
        existing.text.clone_from(&object.text);
        existing.text_color = object.text_color;
        // Retain the current texture entry / media URL for the Texture-tab editor.
        // A terse (motion-only) update carries neither, so both are refreshed only
        // when a full update brings a texture entry — keeping the last known media
        // URL rather than letting a terse update blank it.
        if !object.texture_entry.is_empty() {
            existing.texture_entry.clone_from(&object.texture_entry);
            existing.media_url = object.media_url.as_ref().map(url::Url::to_string);
        }
        return rebuilt;
    }

    // A new object: spawn its entity, parent it if its root is already present,
    // and adopt any of its children that arrived first.
    // Tag the object's whole subtree with the reflection-probe render layers so
    // probe capture cameras (which are off the main layer, to keep the sun from
    // building shadow cascades for them) can see it: an avatar is dynamic content,
    // everything else static world geometry. A HUD attachment overrides this back
    // to the HUD layer when it is routed (see [`route_hud_attachment`]).
    let probe_layers = match category {
        ObjectCategory::Avatar => dynamic_render_layers(),
        _ => world_geom_render_layers(),
    };
    let entity = commands
        .spawn((
            SceneObject {
                scoped_id: scoped,
                category,
            },
            debug_info,
            sl_motion,
            transform,
            // The per-face child meshes carry `Visibility` (required by
            // `Mesh3d`); the object entity needs it too so Bevy's visibility
            // propagation down the linkset hierarchy stays consistent.
            Visibility::default(),
            Propagate(probe_layers),
        ))
        .id();
    // A fresh root carries the re-base marker (see [`recenter_objects`]); a child
    // / attachment does not (its parent re-bases it).
    sync_world_root_marker(entity, is_root, commands);
    let parented = match parent_entity {
        Some(root_entity) => {
            commands.entity(entity).insert(ChildOf(root_entity));
            true
        }
        None => false,
    };
    // A light-source prim carries its decoded light block (P25.1); a plain prim
    // gets nothing.
    apply_light(entity, light_from_object(object), commands);
    // A particle-source prim carries its decoded particle system (P30.1); a plain
    // prim gets nothing.
    apply_particles(entity, particles_from_object(object), commands);
    // A flexi prim carries its decoded flexible-object block (P32.1); a rigid prim
    // gets nothing.
    apply_flexi(entity, flexi_from_object(object), commands);
    // A reflection-probe prim carries its decoded probe block (P33); any other
    // object gets nothing.
    apply_reflection_probe(entity, reflection_probe_from_object(object), commands);
    // A server-flagged physical root prim gets the kinematic-body marker (P31.2);
    // any other object gets nothing (the marker's absence is the signal).
    apply_physics(entity, object, commands);
    // Mirror the object's floating text (`llSetText`, viewer-hover-text) onto the
    // fresh entity so [`crate::hover_text`] spawns its billboard.
    apply_floating_text(entity, object, is_hud, commands);
    // The geometry holder: a child of the object entity carrying only the object's
    // scale, so the object's own faces are scaled while linkset children (which
    // parent to the object entity, not this) are not.
    let geometry = commands
        .spawn((
            GeometryHolder,
            holder_transform(object, category),
            Visibility::default(),
            ChildOf(entity),
        ))
        .id();
    apply_render_materials(geometry, scoped, object, commands);
    apply_texture_animation(geometry, object, commands);
    // A plain prim tessellates immediately; a mesh or sculpt requests its asset and
    // builds its geometry now if already decoded, else on decode; an avatar grows
    // its placeholder in a later phase.
    let (face_entities, pending, prim_rebuild, tree_rebuild, flexi_chain, mesh_rebuild) =
        build_object_geometry(
            object,
            category,
            geometry,
            is_hud,
            commands,
            meshes,
            materials,
            manager,
            prim_textures,
            mesh_manager,
            cache,
            &intern,
            material_cache,
        );
    // A flexi prim carries its seeded chain state so [`simulate_flexi`] can drive it
    // (P32.2); a rigid prim gets nothing.
    apply_flexi_sim(entity, flexi_chain, object, &face_entities, commands);
    state.objects.insert(
        scoped,
        TrackedObject {
            entity,
            full_key: object.full_id,
            geometry,
            shape,
            parent,
            is_root,
            parented,
            attachment_point,
            owner_id: AgentKey::from(object.owner_id),
            update_flags: object.update_flags,
            material: object.material,
            extra: object.extra.clone(),
            face_entities,
            pending,
            // Set when this object built a warm-cache mesh immediately (so no
            // later decode sets it): lets the pixel-area driver rebuild it on an
            // LOD swap. A cold-cache mesh keeps `None` here and has it set on decode
            // in `apply_object_meshes`; a non-mesh keeps `None`.
            mesh_rebuild,
            // A plain prim is first tessellated at the coarse placeholder level
            // (P21.3); a non-prim keeps `prim_rebuild` None and stays at FINEST.
            prim_rebuild,
            prim_lod: INITIAL_MANAGED_PRIM_LOD,
            // A tree is first generated at the placeholder tier (P26.2); a non-tree
            // keeps `tree_rebuild` None.
            tree_rebuild,
            tree_tier: INITIAL_TREE_TIER,
            animated: is_animated_object(object),
            texture_entry: object.texture_entry.clone(),
            media_url: object.media_url.as_ref().map(url::Url::to_string),
            texture_animation: object.texture_animation,
            text: object.text.clone(),
            text_color: object.text_color,
        },
    );
    debug!(
        "spawned object {scoped} ({category:?}); {} tracked",
        state.objects.len()
    );
    if is_root {
        adopt_pending_children(state, scoped, entity, commands);
    }
    // A new object always built its geometry (spawned its faces).
    true
}

/// Reconcile a known object's Bevy parenting with its current linkset role,
/// updating both the `ChildOf` relationship and the entry's `parented` flag.
///
/// A now-root object drops any `ChildOf`; a child whose (possibly new) root is
/// tracked is parented to it; a child whose root is not tracked yet is left
/// parentless (to be adopted once the root arrives).
fn reconcile_parent(
    existing: &mut TrackedObject,
    is_root: bool,
    parent_entity: Option<Entity>,
    parent_changed: bool,
    commands: &mut Commands,
) {
    if is_root {
        if existing.parented {
            commands.entity(existing.entity).remove::<ChildOf>();
            existing.parented = false;
        }
        return;
    }
    match parent_entity {
        Some(root_entity) => {
            // Re-inserting `ChildOf` on an already-parented child marks the
            // hierarchy changed — for a moving vehicle's children that used to
            // be one re-insert per motion packet — so only (re)parent when not
            // yet parented or when the update actually moved it to a new root
            // (a relink arrives with `parented` still true).
            if !existing.parented || parent_changed {
                commands
                    .entity(existing.entity)
                    .insert(ChildOf(root_entity));
                existing.parented = true;
            }
        }
        None => {
            if existing.parented {
                commands.entity(existing.entity).remove::<ChildOf>();
                existing.parented = false;
            }
        }
    }
}

/// Parent every already-spawned child of the just-arrived root `scoped` (entity
/// `root_entity`) that was waiting for it.
fn adopt_pending_children(
    state: &mut ObjectState,
    scoped: ScopedObjectId,
    root_entity: Entity,
    commands: &mut Commands,
) {
    for child in state.objects.values_mut() {
        // An attachment parents to its avatar's skeleton joint, not the linkset
        // root entity — [`adopt_pending_attachments`] handles it (P16.1).
        if !child.parented
            && !child.is_root
            && child.attachment_point.is_none()
            && child.parent == scoped
        {
            commands.entity(child.entity).insert(ChildOf(root_entity));
            child.parented = true;
        }
    }
}

/// Parent every tracked attachment that is not yet parented to its avatar's
/// attachment-point node (P16.1/P16.2), so it follows the posed skeleton at the
/// stored local offset rather than sitting at a fixed world offset — or, for one
/// worn on a HUD point, route it out of the world scene onto the screen-space HUD
/// layer (P35.1).
///
/// Attachments arrive in the same object stream as everything else but hang off a
/// **pcode-47 avatar** (not a prim linkset), so [`apply_object`] holds them
/// parentless and this system — running after the avatars (and their skeleton
/// instances) are spawned — resolves each one's target from the avatar's rigged
/// body: its raw attachment-point id maps to that avatar's attachment-point node
/// entity ([`AvatarState::attachment_point_entity`]), a child of the skeleton
/// joint carrying the fixed `avatar_lad.xml` offset (P16.2), onto which the
/// object's own local transform composes. An attachment whose avatar / point node
/// is not present yet (a sphere-only avatar, or the avatar simply not spawned yet)
/// stays pending and is retried on a later frame.
///
/// When no `--viewer-assets` avatar body is loaded the avatars are placeholder
/// spheres with no skeleton, so an attachment instead falls back to the avatar's
/// own object entity (its previous, position-only parent) so it at least tracks
/// the avatar's location.
///
/// A **HUD** attachment ([`is_hud_point`]) takes neither path: its point hangs off
/// the reference viewer's `mScreen` pseudo-joint, not the skeleton, so it is
/// parented to the [`HudState`] node for its point — the screen-space subtree
/// (P35.1) — and only when the wearer is the agent itself. Another avatar's HUD
/// attachment is hidden instead: `LLVOAvatar::initAttachmentPoints` creates the
/// HUD joints for `isSelf()` alone, so there such an object never attaches and
/// never renders, and it must not become world geometry here either. Both are
/// terminal: the object is marked parented (routed) and not retried.
pub(crate) fn adopt_pending_attachments(
    mut state: ResMut<ObjectState>,
    avatars: Res<AvatarState>,
    body: Option<Res<AvatarBody>>,
    hud: Res<HudState>,
    identity: Res<SlIdentity>,
    mut commands: Commands,
) {
    // Snapshot the pending attachments first so the target lookup can read
    // `state.objects` immutably (for the sphere-mode fallback) before the
    // `parented` flag is set.
    let pending: Vec<(ScopedObjectId, Entity, u8, ScopedObjectId)> = state
        .objects
        .iter()
        .filter_map(|(&scoped, tracked)| {
            let point_id = tracked.attachment_point?;
            (!tracked.parented).then_some((scoped, tracked.entity, point_id, tracked.parent))
        })
        .collect();
    for (scoped, entity, point_id, avatar) in pending {
        if is_hud_point(point_id) {
            // The wearer's agent id, needed to tell our own HUD from someone else's.
            // An attachment can arrive before the avatar object it hangs off, so an
            // unresolved wearer is retried next frame rather than taken for a
            // stranger's (which would hide our own HUD for the whole session).
            let Some(agent) = avatars.agent_of(avatar) else {
                continue;
            };
            let own = identity.agent_id == Some(agent);
            route_hud_attachment(
                &mut state,
                scoped,
                entity,
                point_id,
                own,
                &hud,
                &mut commands,
            );
            continue;
        }
        let target = match body.as_deref() {
            // Rigged body: parent to the avatar's attachment-point node, which sits
            // at the stored `avatar_lad.xml` offset from its skeleton joint, so the
            // attachment's own local transform seats it correctly (P16.1/P16.2).
            Some(_body) => avatars.attachment_point_entity(avatar, point_id),
            // Sphere-only avatars (no assets): fall back to the avatar's object
            // entity so the attachment at least follows its position.
            None => state.objects.get(&avatar).map(|tracked| tracked.entity),
        };
        if let Some(target) = target {
            commands.entity(entity).insert(ChildOf(target));
            if let Some(tracked) = state.objects.get_mut(&scoped) {
                tracked.parented = true;
            }
            debug!("parented attachment {scoped} (point {point_id}) to avatar {avatar} joint");
        }
    }
}

/// Route one attachment worn on a HUD point (P35.1): parent the agent's **own**
/// HUD to the screen node for its point, and hide anyone else's (or any HUD at
/// all when the run has no avatar assets, so no HUD screen was spawned).
///
/// Either way the object leaves the world scene — a HUD's local transform is
/// relative to the screen, so left in the world it would sit as loose geometry at
/// the region origin, which is exactly what it did before this phase. Both
/// outcomes are terminal, so the caller marks it routed and stops retrying.
fn route_hud_attachment(
    state: &mut ObjectState,
    scoped: ScopedObjectId,
    entity: Entity,
    point_id: u8,
    own: bool,
    hud: &HudState,
    commands: &mut Commands,
) {
    match hud.point_entity(point_id).filter(|_node| own) {
        Some(node) => {
            // Override the world-geometry probe layers this object got at spawn
            // with the HUD layer, so the HUD subtree renders on the HUD camera
            // (not the world / probe cameras) — a child's own `Propagate` wins
            // over the HUD screen's propagation, so it must be set explicitly.
            commands.entity(entity).insert((
                ChildOf(node),
                Propagate(RenderLayers::layer(HUD_RENDER_LAYER)),
            ));
            debug!("routed own HUD attachment {scoped} to HUD point {point_id}");
        }
        None => {
            commands.entity(entity).insert(Visibility::Hidden);
            debug!(
                "hid HUD attachment {scoped} (point {point_id}): {}",
                if own {
                    "no HUD screen (no avatar assets)"
                } else {
                    "not the agent's own"
                }
            );
        }
    }
    if let Some(tracked) = state.objects.get_mut(&scoped) {
        tracked.parented = true;
    }
}

/// Despawn the entity for `scoped` and every tracked descendant, dropping them
/// all from the map. Bevy's hierarchy despawns the entity's parented children
/// with it; any tracked-but-not-yet-parented descendants are despawned
/// explicitly so a lingering child update can never touch a dead entity.
fn remove_object(state: &mut ObjectState, scoped: ScopedObjectId, commands: &mut Commands) {
    let Some(removed) = state.objects.remove(&scoped) else {
        return;
    };
    // Bevy despawns the parented sub-hierarchy together with the root entity.
    // `try_despawn` because this entity may already be dead — a linkset child or
    // attachment can be taken by its parent's hierarchy despawn before its own
    // `KillObject` arrives here (the same race [`drop_stale_tracked_entity`] guards
    // on the update path), and a plain `despawn` on it would itself warn.
    commands.entity(removed.entity).try_despawn();
    // A rigged mesh's skinned faces hang off the *avatar body root*, not this
    // object entity (P17.2), so Bevy's hierarchy despawn above does not take them —
    // despawn them explicitly (a no-op for a static mesh's faces, already gone with
    // their object entity).
    despawn_prim_faces(&removed.face_entities, commands);
    // Drop tracked descendants; despawn any that were still waiting to be
    // parented (Bevy did not despawn those with the root), and their faces.
    for descendant in tracked_descendants(state, scoped) {
        if let Some(entry) = state.objects.remove(&descendant) {
            despawn_prim_faces(&entry.face_entities, commands);
            if !entry.parented {
                commands.entity(entry.entity).try_despawn();
            }
        }
    }
}

/// The scoped ids of every tracked transitive descendant of `root` (children,
/// grandchildren, …), following the stored parent links.
fn tracked_descendants(state: &ObjectState, root: ScopedObjectId) -> Vec<ScopedObjectId> {
    let mut descendants = Vec::new();
    let mut frontier = vec![root];
    while let Some(parent) = frontier.pop() {
        for (&scoped, tracked) in &state.objects {
            if !tracked.is_root && tracked.parent == parent {
                descendants.push(scoped);
                frontier.push(scoped);
            }
        }
    }
    descendants
}

/// Build the deferred geometry of every mesh object waiting on a mesh that just
/// decoded: for each [`MeshDecoded`], spawn the submesh entities of every tracked
/// object pending on that key (texturing them via the Phase 6 pipeline). A decode
/// that failed leaves the objects geometry-less (they keep waiting until a later
/// update re-requests the mesh).
///
/// Budgeted: freshly decoded keys park in [`PendingDecodedMeshes`] and drain
/// under the shared [`MeshUploadBudget`], so a decode burst (a cache-warm
/// login resolves everything at once) builds a few keys per frame instead of
/// the whole backlog in one. Deferral is safe — the apply reads the store's
/// current (newest) block when its key's turn comes.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system reading decoded meshes and the ECS resources the geometry build needs"
)]
pub(crate) fn apply_object_meshes(
    mut decoded: MessageReader<MeshDecoded>,
    mut pending_keys: ResMut<PendingDecodedMeshes>,
    mut budget: ResMut<MeshUploadBudget>,
    mut state: ResMut<ObjectState>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<FaceMaterial>>,
    mut manager: ResMut<TextureManager>,
    mut prim_textures: ResMut<PrimTextures>,
    mut mesh_manager: ResMut<MeshManager>,
    mut cache: ResMut<GeometryCache>,
    mut material_cache: ResMut<MaterialCache>,
) {
    for &MeshDecoded(key) in decoded.read() {
        if pending_keys.queued.insert(key) {
            pending_keys.queue.push_back(key);
        }
    }
    let mut scans = 0_usize;
    while budget.remaining > 0 && scans < GEOMETRY_APPLY_SCAN_CAP {
        let Some(key) = pending_keys.queue.pop_front() else {
            break;
        };
        let _was_queued = pending_keys.queued.remove(&key);
        scans = scans.saturating_add(1);
        let Some(mesh) = mesh_manager.decoded(key).map(Arc::clone) else {
            // The fetch failed: objects pending on this key stay geometry-less.
            continue;
        };
        // A rigged mesh (a mesh carrying a skin block) is worn by an avatar — or is
        // an animesh (Phase 29) — and is never built as a static child: it must be
        // skinned to a skeleton. It defers to [`apply_rigged_attachments`], which
        // finds its wearer by walking the parent chain to an avatar root.
        let is_rigged = mesh_manager.skin(key).is_some();
        // …except on a HUD (P35.1), which has no skeleton: the reference viewer
        // warns the user outright that a rigged mesh does not belong on a HUD
        // (the `RiggedMeshAttachedToHUD` notification). Binding it would skin its
        // submeshes onto the wearer's *in-world* body root — dragging the HUD back
        // into the world scene this phase routes it out of — so a HUD-worn rigged
        // mesh is built as static geometry in the HUD's own space instead.
        let hud_rigged: HashSet<ScopedObjectId> = if is_rigged {
            state
                .objects
                .iter()
                .filter(|(_scoped, tracked)| {
                    matches!(&tracked.pending, Some(PendingGeometry::Mesh(pending)) if pending.key == key)
                })
                .map(|(&scoped, _tracked)| scoped)
                .filter(|&scoped| in_hud_attachment(&state, scoped))
                .collect()
        } else {
            HashSet::new()
        };
        if !hud_rigged.is_empty() {
            warn!(
                "rigged mesh {key} is worn on a HUD: building it as static HUD geometry \
                 (the reference viewer warns the user this is unsupported)"
            );
        }
        for (&scoped, tracked) in &mut state.objects {
            // First build: an object pending on this mesh key. A build pending on a
            // *different* asset (another mesh, or a sculpt) is left untouched.
            if matches!(&tracked.pending, Some(PendingGeometry::Mesh(pending)) if pending.key == key)
            {
                let Some(PendingGeometry::Mesh(pending)) = tracked.pending.take() else {
                    continue;
                };
                if is_rigged && !hud_rigged.contains(&scoped) {
                    // Defer the skinned build to `apply_rigged_attachments`. This is
                    // gated on the mesh being rigged, NOT on `attachment_point`: an
                    // attachment's point can arrive in a later update than the mesh
                    // decode, and that race used to strand a far / late-rezzing
                    // avatar's body in a static, un-skinned T-pose with coarse,
                    // pixel-area-managed textures that never recovered on approach
                    // (R22). The rigged bind resolves the wearer by parent chain, so
                    // it does not need the point. A rigged mesh's skinned transform
                    // also cannot be ranked by the pixel-area pass, so its geometry
                    // must render at the finest block and never be LOD reduced —
                    // upgrade it now in case its worn status was unknown when the
                    // fetch began and it started on the managed, coarse-block path.
                    mesh_manager.upgrade_to_finest(key);
                    tracked.pending = Some(PendingGeometry::RiggedMesh(PendingRiggedMesh {
                        key,
                        texture_entry: pending.texture_entry,
                    }));
                } else {
                    tracked.face_entities = build_mesh_submeshes(
                        &mesh,
                        key,
                        &pending.texture_entry,
                        pending.scale,
                        tracked.geometry,
                        &mut commands,
                        &mut meshes,
                        &mut materials,
                        &mut manager,
                        &mut prim_textures,
                        pending.priority,
                        &mut cache,
                        &pending.intern,
                        &mut material_cache,
                    );
                    budget.remaining = budget.remaining.saturating_sub(1);
                    debug!(
                        "built mesh {key}: {} submesh entities",
                        tracked.face_entities.len()
                    );
                    // Remember how to rebuild on a later LOD swap (P21.2); a rigged
                    // mesh (handled above) is boosted and never LOD managed.
                    tracked.mesh_rebuild = Some(pending);
                }
                continue;
            }
            // LOD swap (P21.2): this object already built this static mesh, and the
            // store just swapped its geometry to a different level of detail.
            // Despawn the old submesh entities and rebuild from the new block.
            if !is_rigged
                && tracked.pending.is_none()
                && matches!(&tracked.mesh_rebuild, Some(rebuild) if rebuild.key == key)
            {
                let Some(rebuild) = tracked.mesh_rebuild.as_ref() else {
                    continue;
                };
                let texture_entry = rebuild.texture_entry.clone();
                let scale = rebuild.scale;
                let priority = rebuild.priority;
                let intern = rebuild.intern.clone();
                let geometry = tracked.geometry;
                despawn_prim_faces(&tracked.face_entities, &mut commands);
                tracked.face_entities = build_mesh_submeshes(
                    &mesh,
                    key,
                    &texture_entry,
                    scale,
                    geometry,
                    &mut commands,
                    &mut meshes,
                    &mut materials,
                    &mut manager,
                    &mut prim_textures,
                    priority,
                    &mut cache,
                    &intern,
                    &mut material_cache,
                );
                budget.remaining = budget.remaining.saturating_sub(1);
                debug!(
                    "rebuilt mesh {key} at new LOD: {} submesh entities",
                    tracked.face_entities.len()
                );
            }
        }
    }
}

/// Re-tessellate every plain prim whose pixel-area-selected [`PrimLod`] just
/// changed (P21.3): drain the [`PrimLodTargets`] the render-priority driver
/// filled this pass and, for each prim whose desired level differs from its
/// current one, despawn its old face entities and rebuild them from a fresh
/// tessellation at the new level.
///
/// The mirror of the mesh LOD swap in [`apply_object_meshes`], but with no async
/// fetch: prim geometry is tessellated on the CPU here and now — through the
/// cross-instance [`GeometryCache`], so a level another instance of the same
/// shape already sits at revives its shared meshes instead of re-tessellating
/// (the camera-move LOD-thrash win). A target for a non-prim, an untracked
/// (removed) object, or a prim already at the desired level is a no-op —
/// `prim_rebuild` is `Some` only for a plain prim.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system reading the LOD targets and the ECS resources the geometry build needs"
)]
pub(crate) fn apply_prim_lod(
    mut targets: ResMut<PrimLodTargets>,
    mut budget: ResMut<MeshUploadBudget>,
    mut state: ResMut<ObjectState>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<FaceMaterial>>,
    mut manager: ResMut<TextureManager>,
    mut prim_textures: ResMut<PrimTextures>,
    mut cache: ResMut<GeometryCache>,
    mut material_cache: ResMut<MaterialCache>,
) {
    // Budgeted so a tick's worth of re-tessellations spreads across frames
    // instead of a single command-flush spike (see `MeshUploadBudget`). Shared
    // with `apply_tree_lod`, which runs after and sees the remaining budget.
    let builds = retain_lod_budgeted(
        &mut targets.0,
        budget.remaining,
        |scoped, desired, remaining| {
            let Some(tracked) = state.objects.get_mut(&scoped) else {
                return LodOutcome::Resolved;
            };
            // Only a plain prim carries re-tessellation inputs; a sculpt / mesh /
            // avatar has none and is left untouched.
            let Some(rebuild) = tracked.prim_rebuild.as_ref() else {
                return LodOutcome::Resolved;
            };
            if tracked.prim_lod == desired {
                return LodOutcome::Resolved;
            }
            if remaining == 0 {
                return LodOutcome::Deferred;
            }
            // Clone the rebuild inputs out so the immutable borrow of `tracked` ends
            // before the mutable rebuild of its face entities below.
            let shape = rebuild.shape;
            let texture_entry = rebuild.texture_entry.clone();
            let scale = rebuild.scale;
            let priority = rebuild.priority;
            let intern = rebuild.intern.clone();
            let geometry = tracked.geometry;
            despawn_prim_faces(&tracked.face_entities, &mut commands);
            tracked.face_entities = spawn_cached_prim_faces(
                GeometryKey::Prim {
                    shape,
                    lod: desired,
                },
                || tessellate(&PrimShapeFloat::from_params(&shape), desired),
                &texture_entry,
                scale,
                geometry,
                &mut commands,
                &mut meshes,
                &mut materials,
                &mut manager,
                &mut prim_textures,
                priority,
                &mut cache,
                &intern,
                &mut material_cache,
            );
            tracked.prim_lod = desired;
            debug!(
                "re-tessellated prim {scoped} at {desired:?}: {} faces",
                tracked.face_entities.len()
            );
            LodOutcome::Rebuilt
        },
    );
    budget.remaining = budget.remaining.saturating_sub(builds);
}

/// Regenerate each tree the render-priority driver picked a new [`TreeTier`] for
/// (P26.2) — the tree counterpart of [`apply_prim_lod`], sharing its
/// [`MeshUploadBudget`]. For any tree whose desired tier differs from its current
/// one, despawns its face and regenerates the branch / leaf geometry (or the
/// billboard imposter) at the new tier, up to the remaining per-frame budget.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system reading the LOD targets and the ECS resources the geometry build needs"
)]
pub(crate) fn apply_tree_lod(
    mut targets: ResMut<TreeLodTargets>,
    mut budget: ResMut<MeshUploadBudget>,
    mut state: ResMut<ObjectState>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<FaceMaterial>>,
    mut manager: ResMut<TextureManager>,
    mut prim_textures: ResMut<PrimTextures>,
) {
    // Budgeted from the shared `MeshUploadBudget`, spent after `apply_prim_lod`.
    let builds = retain_lod_budgeted(
        &mut targets.0,
        budget.remaining,
        |scoped, desired, remaining| {
            let Some(tracked) = state.objects.get_mut(&scoped) else {
                return LodOutcome::Resolved;
            };
            // Only a tree carries regeneration inputs; anything else is left untouched.
            let Some(rebuild) = tracked.tree_rebuild.as_ref() else {
                return LodOutcome::Resolved;
            };
            if tracked.tree_tier == desired {
                return LodOutcome::Resolved;
            }
            if remaining == 0 {
                return LodOutcome::Deferred;
            }
            let species = rebuild.species;
            let priority = rebuild.priority;
            let geometry = tracked.geometry;
            despawn_prim_faces(&tracked.face_entities, &mut commands);
            tracked.face_entities = build_tree_faces(
                species,
                desired,
                geometry,
                &mut commands,
                &mut meshes,
                &mut materials,
                &mut manager,
                &mut prim_textures,
                priority,
            );
            tracked.tree_tier = desired;
            debug!("regenerated tree {scoped} at {desired:?}");
            LodOutcome::Rebuilt
        },
    );
    budget.remaining = budget.remaining.saturating_sub(builds);
}

/// Whether worn rigged meshes' joint position overrides (R1) are applied to the
/// avatar skeleton. On by default; `SL_VIEWER_JOINT_OVERRIDES=0` disables it, so the
/// pre-override skeleton behaviour can be compared side by side in one session.
fn joint_overrides_enabled() -> bool {
    std::env::var("SL_VIEWER_JOINT_OVERRIDES").as_deref() != Ok("0")
}

/// A guard on the linkset-chain walk in [`animesh_root`], against a malformed
/// parent cycle.
const MAX_LINKSET_DEPTH: usize = 32;

/// The animesh linkset root that `scoped` belongs to (P29): walk its parent chain
/// up to the object carrying the animated-object flag and return that root's full
/// [`ObjectKey`] (the key its control avatar is filed under) and scene entity
/// (the control-avatar skeleton parents to it so it follows the object). `None`
/// if the chain reaches no animated-object root (not an animesh).
///
/// This walk is also how a signalled animation finds its control avatar
/// (P29.2): the sim keys `ObjectAnimation` by the linkset **part** holding the
/// animations (the prim the script runs in) — often a *child*, not the flagged
/// root — and the reference merges every part's signalled set into the root's
/// control avatar (`LLControlAvatar::updateAnimations` over the whole linkset).
pub(crate) fn animesh_root(
    state: &ObjectState,
    scoped: ScopedObjectId,
) -> Option<(ObjectKey, Entity)> {
    let mut current = scoped;
    for _ in 0..MAX_LINKSET_DEPTH {
        let tracked = state.objects.get(&current)?;
        if tracked.animated {
            return Some((tracked.full_key, tracked.entity));
        }
        // A root's `parent` is its own scoped id; stop before looping forever.
        if tracked.parent == current {
            return None;
        }
        current = tracked.parent;
    }
    None
}

/// Whether the worn-attachment bind trace is enabled
/// (`SL_VIEWER_LOG_ATTACHMENT_BIND=1`): logs, once per reason-change, why each
/// worn rigged attachment is not yet bound, so an attachment that never binds
/// (boots / hair missing while the rest of the avatar draws — roadmap
/// viewer-mesh-hair-not-rendering) reveals *which* stall it is stuck on — the
/// mesh not decoding, an in-flight LOD upgrade, an unresolved wearer, or the
/// wearer's body not spawned — instead of retrying silently every frame.
pub(crate) fn log_attachment_bind_enabled() -> bool {
    std::env::var("SL_VIEWER_LOG_ATTACHMENT_BIND").as_deref() == Ok("1")
}

/// Diagnostic state for [`log_attachment_bind_enabled`]: the last-logged
/// not-yet-bound reason per worn rigged attachment, so [`apply_rigged_attachments`]
/// logs a stuck attachment's reason once per change rather than every frame, and
/// re-logs when the reason advances (progress) or clears when it finally binds.
#[derive(Resource, Default)]
pub(crate) struct RiggedBindSkipLog(HashMap<ScopedObjectId, &'static str>);

impl RiggedBindSkipLog {
    /// Record `reason` as `scoped`'s current not-yet-bound reason, returning
    /// `true` when it changed since last time — so a caller that wants to log
    /// something richer than [`note`](Self::note) still fires exactly once per
    /// reason-change.
    fn changed(&mut self, scoped: ScopedObjectId, reason: &'static str) -> bool {
        self.0.insert(scoped, reason) != Some(reason)
    }

    /// Note that `scoped` is still unbound for `reason`, logging it only when the
    /// reason changed since last time (the caller has already checked the trace is
    /// enabled).
    fn note(&mut self, scoped: ScopedObjectId, reason: &'static str) {
        if self.changed(scoped, reason) {
            info!("rigged attachment {scoped} not yet bound: {reason}");
        }
    }

    /// Forget `scoped`'s stall reason — it bound (or is gone), so a later
    /// re-attach traces afresh.
    fn bound(&mut self, scoped: ScopedObjectId) {
        let _prev = self.0.remove(&scoped);
    }
}

/// One-word description of a tracked object's geometry-pending state, for the
/// wearer-walk terminus diagnostic ([`log_attachment_bind_enabled`]).
const fn pending_kind(pending: Option<&PendingGeometry>) -> &'static str {
    match pending {
        None => "built",
        Some(PendingGeometry::Mesh(_)) => "Mesh-pending",
        Some(PendingGeometry::Sculpt(_)) => "Sculpt-pending",
        Some(PendingGeometry::RiggedMesh(_)) => "RiggedMesh-pending",
    }
}

/// Bind every worn rigged mesh attachment whose skeleton instance is now
/// available (P17.2): for each object holding a [`PendingGeometry::RiggedMesh`],
/// resolve the wearer avatar's skeleton-instance joint entities and spawn the
/// mesh's skinned submeshes bound to them, so the mesh deforms with the avatar
/// rather than sitting rigidly at an attachment point.
///
/// A rigged mesh's build is deferred here (rather than in [`apply_object_meshes`])
/// because it needs the wearer's spawned skeleton — which can arrive before or
/// after the mesh decodes. The pending build is retried each frame until the
/// avatar's rigged body ([`AvatarState::is_rigged`]) is present; an avatar
/// with no rigged body (a sphere-only, no-`--viewer-assets` run) never resolves,
/// so the mesh simply stays unbuilt there. Each rig joint name is mapped to the
/// avatar's matching skeleton joint entity ([`AvatarBody::joint_index`]), falling
/// back to the pelvis for a name the skeleton lacks (the reference viewer's
/// unknown-joint fallback). The object is marked parented so
/// [`adopt_pending_attachments`] does not also pin it to a rigid
/// attachment-point node.
///
/// Budgeted: skinned builds spend from the shared [`MeshUploadBudget`], so
/// a crowd's rigged bodies bind over several frames; the not-yet-built rest
/// stays pending and is re-collected next frame (the cheap not-ready retries
/// — skeleton or finest LOD still loading — are free, as before).
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system joining the object, avatar, and mesh state with the ECS resources the skinned build needs"
)]
pub(crate) fn apply_rigged_attachments(
    mut state: ResMut<ObjectState>,
    mut avatars: ResMut<AvatarState>,
    mut control: ResMut<ControlAvatarState>,
    body: Option<Res<AvatarBody>>,
    mesh_manager: Res<MeshManager>,
    mut budget: ResMut<MeshUploadBudget>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<FaceMaterial>>,
    mut bindposes: ResMut<Assets<SkinnedMeshInverseBindposes>>,
    mut manager: ResMut<TextureManager>,
    mut prim_textures: ResMut<PrimTextures>,
    mut cache: ResMut<GeometryCache>,
    mut skip_log: ResMut<RiggedBindSkipLog>,
) {
    // The worn-attachment bind trace (off unless SL_VIEWER_LOG_ATTACHMENT_BIND=1),
    // read once so the per-object skips below cost nothing in the normal case.
    let trace = log_attachment_bind_enabled();
    // Without loaded avatar assets there are no rigged bodies to bind to.
    let Some(body) = body else {
        return;
    };
    // Snapshot the objects whose rigged build is pending, so the per-object reads
    // below can borrow `state.objects` immutably before the final update.
    let pending: Vec<ScopedObjectId> = state
        .objects
        .iter()
        .filter_map(|(&scoped, tracked)| {
            matches!(tracked.pending, Some(PendingGeometry::RiggedMesh(_))).then_some(scoped)
        })
        .collect();
    for scoped in pending {
        // A skinned build is among the heaviest per-object costs (submesh
        // meshes + inverse bindposes + skeleton binding); spend from the
        // shared decode-apply budget so a crowd's worth of rigged bodies
        // binds over several frames. The unbuilt rest stays pending and is
        // re-collected next frame.
        if budget.remaining == 0 {
            break;
        }
        let Some(tracked) = state.objects.get(&scoped) else {
            continue;
        };
        let Some(PendingGeometry::RiggedMesh(build)) = &tracked.pending else {
            continue;
        };
        let key = build.key;
        // A rigged mesh that started on the managed coarse-LOD path (an animesh, or
        // an attachment whose worn status resolved late) is being upgraded to the
        // finest block asynchronously (`upgrade_to_finest`). Building now would bind
        // the coarse geometry, and a rigged mesh is not on the LOD-swap rebuild path
        // (`apply_object_meshes`), so it would stay frozen at that block — an animesh
        // rendered from its few-vertex coarsest LOD (P29). Wait for the finest decode.
        if mesh_manager.lod_change_inflight(key) {
            if trace {
                skip_log.note(scoped, "finest-LOD upgrade in flight");
            }
            continue;
        }
        // Resolve the skeleton this rigged mesh binds to: an animated object
        // (animesh) drives its OWN control-avatar skeleton (P29), spawned on demand
        // as a child of the linkset root; every other rigged mesh binds to the
        // wearer avatar it hangs off. The `bind_agent` (the wearer, keying its baked
        // textures for a bake-on-mesh face) is `None` for an animesh — an animated
        // object has no wearer bake, so its faces texture from ordinary fetches.
        let animesh = animesh_root(&state, scoped);
        let (root, joints, bind_agent, slot) = if let Some((object, object_entity)) = animesh {
            // The animesh control avatar's root (a child of the linkset root, so
            // the skeleton tracks the object). Phase 4/§5: no per-object joint
            // entities — the submeshes bind every palette slot to the shared
            // dummy and are GPU-posed in place on the `Animesh` pose slot.
            let root = control.ensure_spawned(object, object_entity, &mut commands);
            let joints = vec![body.dummy_joint(); body.skeleton_joint_count()];
            (
                root,
                joints,
                None,
                crate::gpu_avatars::PoseSlotKey::Animesh(object),
            )
        } else {
            // The wearer avatar, found by chasing this mesh's parent links up to the
            // avatar root — a mesh body is worn as a multi-prim linkset whose parts
            // parent to the linkset root prim, not the avatar directly, so its direct
            // `parent` is not the avatar (P17.2 fix; verified live on a real mesh body).
            let Some(agent) = avatars.wearer_of(scoped) else {
                if trace && skip_log.changed(scoped, "wearer avatar not resolved (parent chain)") {
                    // Classify the failure by walking the parent chain to where it
                    // stopped: a *tracked in-world* terminus means the object is
                    // genuinely not worn (an in-world rigged mesh that should never
                    // be in the attachment bind); an *untracked* terminus means the
                    // wearer / linkset-root object never arrived (a parenting gap).
                    match avatars.avatar_root_walk(scoped) {
                        Ok(_resolved) => {}
                        Err((terminus, hops)) => {
                            let kind = match state.objects.get(&terminus) {
                                Some(tracked) => format!(
                                    "tracked in-world object (is_root={}, attach_point={:?}, {})",
                                    tracked.is_root,
                                    tracked.attachment_point,
                                    pending_kind(tracked.pending.as_ref()),
                                ),
                                None => {
                                    "UNTRACKED — its parent/root object never arrived".to_owned()
                                }
                            };
                            // Attribute the stuck attachment to the avatar that wears
                            // it: the update's `owner_id` is the wearer, so resolving
                            // it to a spawned avatar's name (when present) names which
                            // avatar is rendering wrong — e.g. a mesh head whose root
                            // is one of the UNTRACKED termini.
                            let (attach, owner) = state.objects.get(&scoped).map_or_else(
                                || ("?".to_owned(), "?".to_owned()),
                                |worn| {
                                    let owner = avatars.name_of(worn.owner_id).map_or_else(
                                        || worn.owner_id.to_string(),
                                        |name| format!("{name} [{}]", worn.owner_id),
                                    );
                                    (format!("{:?}", worn.attachment_point), owner)
                                },
                            );
                            info!(
                                "rigged attachment {scoped} (attach_point={attach}, \
                                 owner={owner}): wearer unresolved after {hops} hop(s); \
                                 chain terminus {terminus} is {kind}"
                            );
                        }
                    }
                }
                continue;
            };
            // The wearer's rigged body; retry next frame if the avatar (or its
            // body) is not spawned yet. Phase 4 removed the per-avatar joint
            // entities: a worn rig's `SkinnedMesh` binds every palette slot to
            // the single shared dummy joint (its wearer's canonical joint indices
            // ride the `GpuSkinBinding` below), so a full-skeleton-length vec of
            // dummies is all the skin mapping needs — `joints.get(index)` still
            // resolves any valid skeleton index to a (dummy) entity.
            let Some(root) = avatars
                .body_root_of(agent)
                .filter(|_| avatars.is_rigged(agent))
            else {
                if trace {
                    skip_log.note(scoped, "wearer body / skeleton not spawned yet");
                }
                continue;
            };
            let joints = vec![body.dummy_joint(); body.skeleton_joint_count()];
            (
                root,
                joints,
                Some(agent),
                crate::gpu_avatars::PoseSlotKey::Avatar(agent),
            )
        };
        let Some(fallback) = joints.first().copied() else {
            if trace {
                skip_log.note(scoped, "wearer skeleton has no joints");
            }
            continue;
        };
        // The decoded geometry + skin, cloned out so the immutable `mesh_manager`
        // borrow ends before the build borrows the other resources mutably.
        let (Some(decoded), Some(skin)) = (
            mesh_manager.decoded(key).map(Arc::clone),
            mesh_manager.skin(key).map(Arc::clone),
        ) else {
            if trace {
                skip_log.note(scoped, "attachment mesh / skin not decoded yet");
            }
            continue;
        };
        // Resolve the rig's own joint-name table against the avatar's skeleton
        // instance (unknown names fall back to the pelvis joint).
        // Map each rig joint name to the avatar's skeleton-instance joint entity;
        // an unresolved name (a bone or collision volume the skeleton lacks) falls
        // back to the pelvis, which would misplace those vertices, so it is logged.
        let mut unresolved: Vec<&str> = Vec::new();
        // The canonical skeleton joint index of each palette slot, built in
        // lockstep with the joint entities for the GPU-avatar real-skin
        // resolver (Phase 4 groundwork, `crate::gpu_avatars::GpuSkinBinding`).
        // An unresolved name binds to the pelvis `fallback` entity (joint 0),
        // so its canonical entry is 0 too — the canonical index always matches
        // the entity that palette slot actually skins to.
        let mut canonical: Vec<u32> = Vec::with_capacity(skin.joint_names.len());
        let joint_entities: Vec<Entity> = skin
            .joint_names
            .iter()
            .map(|name| {
                match body
                    .joint_index(name)
                    .and_then(|index| joints.get(index).copied().map(|entity| (index, entity)))
                {
                    Some((index, entity)) => {
                        canonical.push(u32::try_from(index).unwrap_or(0));
                        entity
                    }
                    None => {
                        unresolved.push(name.as_str());
                        canonical.push(0);
                        fallback
                    }
                }
            })
            .collect();
        if !unresolved.is_empty() {
            warn!(
                "rigged mesh {key}: {}/{} joint(s) unresolved, bound to pelvis: {:?}",
                unresolved.len(),
                skin.joint_names.len(),
                unresolved
            );
        }
        let texture_entry = build.texture_entry.clone();
        // The wearer's agent id (when known) keys its baked textures (P17.3): a
        // bake-on-mesh face is textured from the wearer's own bake, not a fetch. An
        // animesh has no wearer bake (`bind_agent` is `None`), so its faces texture
        // from ordinary fetches.
        let face_entities = build_rigged_submeshes(
            &decoded,
            &skin,
            &joint_entities,
            &canonical,
            &texture_entry,
            root,
            slot,
            bind_agent,
            // A worn mesh's submeshes carry their worn-object identity for the
            // attachment pies; an animesh (`bind_agent` `None`) is not worn.
            bind_agent.is_some().then_some(scoped),
            key,
            &mut commands,
            &mut meshes,
            &mut materials,
            &mut bindposes,
            &mut manager,
            &mut prim_textures,
            &mut cache,
        );
        budget.remaining = budget.remaining.saturating_sub(1);
        if trace {
            // Bound (or built as empty — the `rendered no geometry` warn covers
            // that): stop tracing this attachment so a later re-attach starts fresh.
            skip_log.bound(scoped);
        }
        if let Some(tracked) = state.objects.get_mut(&scoped) {
            tracked.face_entities = face_entities;
            tracked.pending = None;
            // The skinned mesh follows the skeleton joints directly, so the object
            // must not also be pinned to a rigid attachment-point node.
            tracked.parented = true;
        }
        // Fold the rig's joint position overrides into the skeleton it binds to (R1):
        // a fitted mesh body/head repositions the joints its inverse-bind matrices
        // were baked against, so without these the mesh distorts at the extremities.
        // For an animesh they go onto its own control avatar; for a worn mesh onto
        // the wearer (flagging it for a skeleton re-deform, the reference viewer's
        // `addAttachmentOverridesForObject`). Suppressible via
        // `SL_VIEWER_JOINT_OVERRIDES=0` (A/B against the pre-override behaviour).
        let overrides = if joint_overrides_enabled() {
            body.joint_overrides(&skin)
        } else {
            JointOverrides::default()
        };
        if !overrides.is_empty() {
            debug!(
                "rigged mesh {key}: {} joint position override(s), lock_scale={}",
                overrides.len(),
                overrides.lock_scale()
            );
        }
        // Whether this rig is *fitted* (P34.3): how many of the joints it binds are
        // collision volumes, and how many of those its own joint positions override
        // — an overridden volume is pinned by the rig and ignores the shape's volume
        // morphs (the reference does the same: `LLJoint::setPosition` yields to an
        // active attachment override). A rig that binds no volume at all cannot
        // follow the chest / belly / butt sliders however faithfully they resolve.
        let volumes_bound = skin
            .joint_names
            .iter()
            .filter(|name| body.is_collision_volume(name))
            .count();
        if volumes_bound > 0 {
            let volumes_pinned = skin
                .joint_names
                .iter()
                .filter(|name| {
                    body.is_collision_volume(name)
                        && body
                            .joint_index(name)
                            .is_some_and(|joint| overrides.position(joint).is_some())
                })
                .count();
            // Binding a volume means nothing if the rig puts no *weight* on it, so
            // report the share of the skin's total weight mass that rides the
            // collision volumes: that — not the joint list — is what decides whether
            // the shape's volume morphs move this mesh at all.
            let is_volume_slot: Vec<bool> = skin
                .joint_names
                .iter()
                .map(|name| body.is_collision_volume(name))
                .collect();
            let mut volume_mass = 0.0_f32;
            let mut total_mass = 0.0_f32;
            for submesh in &decoded.submeshes {
                let Some(weights) = submesh.weights.as_ref() else {
                    continue;
                };
                for vertex in weights {
                    for &(slot, weight) in &vertex.influences {
                        total_mass += weight;
                        if is_volume_slot.get(usize::from(slot)).copied() == Some(true) {
                            volume_mass += weight;
                        }
                    }
                }
            }
            let share = if total_mass > 0.0 {
                100.0 * volume_mass / total_mass
            } else {
                0.0
            };
            debug!(
                "rigged mesh {key}: binds {volumes_bound} collision volume(s), \
                 {volumes_pinned} pinned by its own joint overrides, \
                 {share:.1}% of its skin weight rides them"
            );
        } else {
            debug!("rigged mesh {key}: binds no collision volume (not a fitted rig)");
        }
        match (animesh, bind_agent) {
            (Some((object, _entity)), _) => control.record_overrides(object, key.uuid(), overrides),
            (None, Some(agent)) => {
                avatars.record_joint_overrides(agent, key.uuid(), overrides);
                // Record the worn rigged mesh for the avatar-state dump
                // (viewer-avatar-state-dump-replay).
                avatars.record_worn_rigged_mesh(agent, key.uuid());
            }
            (None, None) => {}
        }
        debug!("bound rigged mesh {key} to its skeleton");
    }
}

/// Spawn a control avatar (P29) for every tracked animesh root any part of whose
/// linkset has an animation playing, as soon as the animation arrives — rather
/// than waiting for its rigged mesh to bind (`apply_rigged_attachments`), which
/// can be many seconds later once the mesh's finest LOD decodes. The reference
/// viewer creates an `LLControlAvatar` when the object is detected as animated,
/// not when its geometry loads; spawning early means an `ObjectAnimation` that
/// arrives before the mesh decode is captured and posed the moment the mesh
/// binds, instead of being lost in the gap. The skeleton is invisible until a
/// mesh binds to it, so this only materialises for animesh we are actually
/// animating.
///
/// Each signalled **part** resolves to its flagged root through
/// [`animesh_root`] (P29.2): the sim keys `ObjectAnimation` by the prim holding
/// the animations, which is often a plain (un-flagged) linkset child — keying
/// the spawn on the root itself being signalled left every such animesh
/// permanently un-posed.
pub(crate) fn spawn_animesh_control_avatars(
    state: Res<ObjectState>,
    mut control: ResMut<ControlAvatarState>,
    body: Option<Res<AvatarBody>>,
    mut commands: Commands,
) {
    // Only spawn control avatars when the avatar asset library (the shared
    // skeleton the GPU rest solve needs) is present.
    if body.is_none() {
        return;
    }
    let parts = control.signalled_parts();
    let scoped_by_full = state.scoped_by_full_keys(&parts);
    let roots: HashSet<(ObjectKey, Entity)> = scoped_by_full
        .values()
        .filter_map(|&scoped| animesh_root(&state, scoped))
        .collect();
    for (key, entity) in roots {
        let _spawned = control.ensure_spawned(key, entity, &mut commands);
    }
}

/// Drop the control avatar of every animesh (P29) whose root object is no longer
/// tracked — removed, or its region left. The skeleton entities parent under the
/// object entity, so Bevy's recursive despawn already took them with the object;
/// this only clears the stale [`ControlAvatarState`] bookkeeping, so a re-rez
/// rebuilds a fresh control avatar.
///
/// The **signalled-animation sets are deliberately not pruned here** (P29.2):
/// an `ObjectAnimation` routinely arrives before its part's first
/// `ObjectUpdate` (and for a part we may never track at all), and pruning the
/// set by tracked-object liveness destroyed that early-arrival buffer the same
/// frame it was folded — the event cursor had advanced, so the animation was
/// gone for good. The reference keeps its signalled map for the session and
/// re-reads it whenever a control avatar is (re)built; only a safety cap
/// bounds ours ([`ControlAvatarState::bound_signalled`]).
pub(crate) fn prune_control_avatars(
    state: Res<ObjectState>,
    mut control: ResMut<ControlAvatarState>,
) {
    let live: HashSet<ObjectKey> = state
        .objects
        .values()
        .map(|tracked| tracked.full_key)
        .collect();
    control.retain(|object| live.contains(&object));
    control.bound_signalled(|part| live.contains(&part));
}

/// Spawn one skinned child entity per non-empty submesh of a decoded rigged mesh
/// under the wearer avatar's body `root` (P17.2), each a Bevy `SkinnedMesh` bound
/// to the shared `joint_entities` (the avatar's skeleton-instance joints, in the
/// skin's `joint_names` order) and the mesh's own inverse bindposes, textured per
/// submesh via the Phase-6 pipeline exactly as the static mesh path is. Returns
/// the spawned entities so a detach (or the avatar leaving) can despawn them.
///
/// All submeshes share the mesh's single skin, so the inverse bindposes are built
/// once. The skinned vertices are computed in world space from the joint entities'
/// global transforms, so the entities are parented under the avatar body root only
/// for lifecycle and visibility — their own `Transform` does not place them.
///
/// The converted submesh [`Mesh`]es and the [`SkinnedMeshInverseBindposes`] are
/// shared across wearers through the [`GeometryCache`] rigged slots — both are
/// pure functions of the decoded asset, and sharing the *asset handles* is what
/// lets Bevy batch N wearers of the same body into one instanced draw per
/// submesh (batching keys on the mesh asset). The per-wearer state stays
/// per-entity: `SkinnedMesh::joints` is this wearer's own skeleton-instance
/// joint entities, and the per-face materials / bake textures are built per
/// spawn as before.
#[expect(
    clippy::too_many_arguments,
    reason = "threads the several ECS resources the skinned build needs"
)]
fn build_rigged_submeshes(
    decoded: &DecodedMesh,
    skin: &MeshSkin,
    joint_entities: &[Entity],
    canonical: &[u32],
    texture_entry: &[u8],
    root: Entity,
    slot: crate::gpu_avatars::PoseSlotKey,
    agent: Option<AgentKey>,
    worn: Option<ScopedObjectId>,
    mesh_key: MeshKey,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<FaceMaterial>,
    bindposes: &mut Assets<SkinnedMeshInverseBindposes>,
    manager: &mut TextureManager,
    prim_textures: &mut PrimTextures,
    cache: &mut GeometryCache,
) -> Vec<Entity> {
    let entry = decode_texture_entry(texture_entry, decoded.submeshes.len());
    // The slot every face falls back to when the object carries no texture entry.
    let default_face = TextureFace::new(TextureKey::from(Uuid::nil()));
    // Shared across wearers of the same mesh asset at this decoded level: the
    // second wearer revives the first wearer's asset instead of minting its own.
    let rigged_key = (mesh_key, decoded.lod);
    let inverse_bindposes = cache
        .revive_rigged_bindposes(rigged_key, bindposes)
        .unwrap_or_else(|| {
            let built = bindposes.add(SkinnedMeshInverseBindposes::from(rigged_inverse_bindposes(
                skin,
            )));
            cache.record_rigged_bindposes(rigged_key, built.id());
            built
        });
    let log_faces = agent.is_some() && log_avatar_faces_enabled();
    // The GPU-avatar real-skin binding's canonical joint map (Phase 4
    // groundwork), shared by every submesh of this rig (they all skin to the
    // one `joint_names` list). Interned into an `Arc` once so each spawned
    // submesh clones a handle rather than the slice.
    let canonical_binding: Arc<[u32]> = Arc::from(canonical);
    let mut face_entities = Vec::new();
    for (index, submesh) in decoded.submeshes.iter().enumerate() {
        // Skip a submesh with no renderable geometry — the explicit `NoGeometry`
        // marker *or* a `no_geometry == false` submesh that still decoded to zero
        // vertices. Converting the latter yields a zero-vertex mesh the GPU mesh
        // allocator floods the log over (viewer-r26) and which renders nothing.
        if !submesh.has_geometry() {
            continue;
        }
        // Revive the shared converted submesh, or convert once and record it
        // for the next wearer. The handle — not a per-wearer copy — is what
        // Bevy's draw batching keys on.
        let mesh = cache
            .revive_rigged_submesh(rigged_key, index, meshes)
            .unwrap_or_else(|| {
                let converted = meshes.add(to_bevy_rigged_mesh(submesh));
                cache.record_rigged_submesh(rigged_key, index, converted.id());
                converted
            });
        let texture_face = entry.face(index).unwrap_or(&default_face);
        // A bake-on-mesh face (P17.3): its texture id is an `IMG_USE_BAKED_*`
        // sentinel meaning "show the wearer's own baked skin here". Tag it [`BomFace`]
        // (carrying the face's TE tint + UV) so `apply_bom_face_materials` textures it
        // from the wearer's own bake with the reference viewer's per-face tint / hide /
        // blend (R22) — never fetch the sentinel, which is not a real texture (the
        // P17.2 invisible-shell finding). Only when the wearer's agent is known;
        // otherwise fall through to a plain fetch.
        let bom = agent.and_then(|agent| {
            avatar_texture::use_baked_slot(texture_face.texture_id).map(|slot| {
                BomFace::new(
                    agent,
                    slot,
                    texture_face.color,
                    texture_face_uv_transform(texture_face),
                )
            })
        });
        if log_faces {
            log_rigged_face(mesh_key, index, texture_face, bom.as_ref());
        }
        let material = match &bom {
            // A BoM face owns its material so `apply_bom_face_materials` can give it
            // the reference per-face tint / blend / hide on the sampled bake; until
            // then it shows the neutral fallback (not the reddish skin placeholder).
            Some(bom) => materials.add(bom_face_material(bom.tint(), bom.uv())),
            // A rigged mesh is always a worn attachment, so its face textures are
            // boosted (P20.2) — its skinned entity transform does not reflect its
            // on-screen size, so the pixel-area pass cannot rank it.
            None => face_material(
                texture_face,
                materials,
                manager,
                prim_textures,
                AVATAR_BOOST_PRIORITY,
                // A rigged face cannot alpha-mask (reference: `canRenderAsMask` is
                // false for rigged), so one with a genuinely transparent texture
                // alpha-*blends* — hair / eyelashes render soft, not as a solid card.
                // A texture with no real transparency stays opaque. (A rigged face
                // sampling a 5-channel bake is the separate BoM path in avatars.rs.)
                TextureAlpha::Blend,
            ),
        };
        // The submesh index is the Linden face index; a mesh has few faces, so the
        // widening never saturates in practice (a clamp keeps it lint-clean).
        let face_id = PrimFaceId::new(u16::try_from(index).unwrap_or(u16::MAX));
        let mut spawned = commands.spawn((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            SkinnedMesh {
                inverse_bindposes: inverse_bindposes.clone(),
                joints: joint_entities.to_vec(),
            },
            // Frustum culling is driven by the GPU-computed posed AABB
            // (`crate::gpu_avatars::stage::apply_gpu_avatar_bounds`) via this
            // submesh's `GpuSkinBinding` slot: its drawn vertices live wherever
            // the GPU palettes put them, not at this entity's transform, so the
            // real posed bound is read back and set on its `Aabb` rather than
            // opting culling out (Phase 5, retiring `NoFrustumCulling`).
            Transform::default(),
            Visibility::default(),
            PrimFaceEntity { face_id },
            ChildOf(root),
        ));
        if let Some(bom) = bom {
            spawned.insert(bom);
        }
        // A worn rigged submesh is part of its wearer's drawn silhouette — on a
        // modern mesh-body avatar the *base* body is hidden, so without this tag
        // the wearer would have no pickable geometry in the GPU pick view
        // (`crate::gpu_pick`). An animesh (no wearer, `agent` `None`) is not an
        // avatar and stays untagged, matching the reference viewer's
        // control-avatar exclusion from avatar picking.
        if let Some(agent) = agent {
            spawned.insert(AvatarPickTarget::new(agent));
        }
        // The GPU-avatar real-skin binding (§1.1): the pose slot + per-slot
        // canonical joint indices, so pass D resolves this submesh's palette in
        // place. Both a worn avatar rig (`PoseSlotKey::Avatar`) and an animesh
        // control-avatar submesh (`PoseSlotKey::Animesh`) bind it — only the
        // avatar pick target above distinguishes them.
        spawned.insert(crate::gpu_avatars::GpuSkinBinding {
            slot,
            canonical: Arc::clone(&canonical_binding),
        });
        // …and the worn-object identity beside it, so that same pick can route a
        // hit on this submesh to the attachment pies (`crate::attachment_menu`)
        // instead of the wearer's plain avatar pie.
        if let Some(scoped) = worn {
            spawned.insert(WornPickTarget { scoped });
        }
        face_entities.push(spawned.id());
    }
    // Every submesh was dropped as empty above, so this worn rigged attachment
    // renders NOTHING — the "boots / hair missing while the rest of the avatar
    // draws" symptom (roadmap viewer-mesh-hair-not-rendering). Surface it, with the
    // decoded LOD and per-submesh vertex counts, so an intermittent and otherwise
    // silent drop is observable: it distinguishes a genuinely-empty chosen LOD
    // block (the author put no geometry there — the fix is a coarser-LOD fallback)
    // from an `sl-mesh` decode gap yielding zero vertices for a block that has data
    // (a decoder bug to fix, not mask). Rigged meshes are forced to the finest
    // block (`upgrade_to_finest`), so this is the finest available LOD.
    if face_entities.is_empty() && !decoded.submeshes.is_empty() {
        let vertex_counts: Vec<usize> = decoded
            .submeshes
            .iter()
            .map(|submesh| submesh.positions.len())
            .collect();
        warn!(
            "rigged attachment {mesh_key} rendered no geometry: all {} submesh(es) \
             empty at LOD {:?} (vertex counts {vertex_counts:?}); agent={agent:?} worn={worn:?}",
            decoded.submeshes.len(),
            decoded.lod,
        );
    }
    face_entities
}

/// Log one rigged-mesh face's `TextureEntry` for BoM diagnostics (R22, gated by
/// `SL_VIEWER_LOG_AVATAR_FACES=1`): its sampled bake slot (an `IMG_USE_BAKED_*`
/// sentinel) or real texture id, plus the tint and UV placement the reference
/// viewer applies. The tint alpha reveals a hidden alpha-cut / "onion shell" layer
/// (`tint a=0`) and the slot reveals whether a mesh body's arm samples the classic
/// `upper` bake or a `universal` (`leftarm`/`aux*`) one.
fn log_rigged_face(mesh_key: MeshKey, index: usize, face: &TextureFace, bom: Option<&BomFace>) {
    let [r, g, b, a] = face.color;
    let source = match bom {
        Some(bom) => format!("BoM slot {}", bom.slot_name()),
        None => format!("tex {}", face.texture_id),
    };
    info!(
        "rigged face {mesh_key} #{index}: {source} tint=({r},{g},{b},a={a}) \
         repeats=({:.3},{:.3}) offset=({:.3},{:.3}) rot={:.3}",
        face.scale_s, face.scale_t, face.offset_s, face.offset_t, face.rotation
    );
}

/// Build the deferred geometry of every sculpted prim waiting on a sculpt map
/// texture that just decoded: for each [`TextureDecoded`], stitch and spawn the
/// face of every tracked object pending on that key (texturing it via the Phase 6
/// pipeline). A decode that failed leaves the objects geometry-less (they keep
/// waiting until a later update re-requests the map).
///
/// This reads the same [`TextureDecoded`] stream as
/// [`apply_prim_textures`](crate::textures::apply_prim_textures) — the sculpt map
/// flows through the shared [`TextureManager`] like any face texture — but keys off
/// a *pending sculpt build* rather than a parked face material, so the two
/// consumers never contend for the same decoded texture.
///
/// Budgeted: decoded keys park in [`PendingDecodedSculpts`] and drain under
/// the shared [`MeshUploadBudget`] (and the per-frame scan cap, since most
/// decoded textures are not sculpt maps), spreading a decode burst's sculpt
/// tessellation across frames.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system reading decoded sculpt maps and the ECS resources the geometry build needs"
)]
pub(crate) fn apply_object_sculpts(
    mut decoded: MessageReader<TextureDecoded>,
    mut pending_keys: ResMut<PendingDecodedSculpts>,
    mut budget: ResMut<MeshUploadBudget>,
    mut state: ResMut<ObjectState>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<FaceMaterial>>,
    mut manager: ResMut<TextureManager>,
    mut prim_textures: ResMut<PrimTextures>,
    mut cache: ResMut<GeometryCache>,
    mut material_cache: ResMut<MaterialCache>,
) {
    for &TextureDecoded(id) in decoded.read() {
        if pending_keys.queued.insert(id) {
            pending_keys.queue.push_back(id);
        }
    }
    if pending_keys.queue.is_empty() {
        return;
    }
    // Most decoded textures are ordinary face textures; when no sculpt build is
    // pending at all, the whole backlog is irrelevant — drop it with one scan
    // instead of burning the per-frame scan cap on it.
    if !state
        .objects
        .values()
        .any(|tracked| matches!(tracked.pending, Some(PendingGeometry::Sculpt(_))))
    {
        pending_keys.queue.clear();
        pending_keys.queued.clear();
        return;
    }
    let mut scans = 0_usize;
    while budget.remaining > 0 && scans < GEOMETRY_APPLY_SCAN_CAP {
        let Some(id) = pending_keys.queue.pop_front() else {
            break;
        };
        let _was_queued = pending_keys.queued.remove(&id);
        scans = scans.saturating_add(1);
        // The decoded sculpt-map pixels; clone the `Arc` out so the immutable
        // borrow of `manager` ends before the face build borrows it mutably.
        let Some(map) = manager.decoded(id).map(Arc::clone) else {
            // The fetch failed: sculpts pending on this map stay geometry-less.
            continue;
        };
        for tracked in state.objects.values_mut() {
            // Take the pending build so a built object is not rebuilt; a build
            // pending on a *different* asset (a mesh, or another sculpt map) is put
            // back untouched.
            match tracked.pending.take() {
                Some(PendingGeometry::Sculpt(pending)) if pending.map == id => {
                    tracked.face_entities = build_sculpt_faces(
                        &map,
                        pending.map,
                        pending.sculpt_type,
                        &pending.texture_entry,
                        pending.scale,
                        tracked.geometry,
                        &mut commands,
                        &mut meshes,
                        &mut materials,
                        &mut manager,
                        &mut prim_textures,
                        pending.priority,
                        &mut cache,
                        &pending.intern,
                        &mut material_cache,
                    );
                    budget.remaining = budget.remaining.saturating_sub(1);
                    debug!(
                        "built sculpt {id}: {} face entities",
                        tracked.face_entities.len()
                    );
                }
                other => tracked.pending = other,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        LodOutcome, ObjectCategory, ShapeFingerprint, classify, drain_budgeted, geometry_transform,
        holder_transform, object_transform, retain_lod_budgeted, tree_species_byte,
    };
    use bevy::math::Vec3;
    use pretty_assertions::{assert_eq, assert_ne};
    use sl_client_bevy::{
        AgentKey, CircuitId, MeshKey, Object, ObjectMotion, RegionHandle, RegionLocalObjectId,
        Rotation, SculptData, SculptOrMeshKey, TextureKey, Uuid, Vector, pcode,
    };
    use std::collections::{HashMap, VecDeque};

    /// The zero vector (`Vector` does not derive `Default`).
    const fn zero() -> Vector {
        Vector {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }
    }

    /// A minimal object with the given `pcode`, at a root position, no sculpt.
    fn bare_object(pcode: u8) -> Object {
        Object {
            region_handle: RegionHandle(0),
            local_id: RegionLocalObjectId(1),
            circuit: CircuitId::new(1),
            full_id: Uuid::from_u128(1).into(),
            parent_id: RegionLocalObjectId(0),
            pcode,
            state: 0,
            crc: 0,
            material: 0,
            click_action: 0,
            update_flags: 0,
            scale: Vector {
                x: 2.0,
                y: 3.0,
                z: 4.0,
            },
            motion: ObjectMotion {
                position: Vector {
                    x: 10.0,
                    y: 20.0,
                    z: 30.0,
                },
                velocity: zero(),
                acceleration: zero(),
                rotation: Rotation {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    s: 1.0,
                },
                angular_velocity: zero(),
                collision_plane: None,
            },
            owner_id: Uuid::from_u128(0),
            sound: Uuid::from_u128(0),
            gain: 0.0,
            sound_flags: 0,
            sound_radius: 0.0,
            text: String::new(),
            text_color: [0; 4],
            name_value: String::new(),
            media_url: None,
            texture_entry: Vec::new(),
            texture_anim: Vec::new(),
            texture_animation: None,
            shape: sl_client_bevy::PrimShapeParams::default(),
            particle_system: Vec::new(),
            particles: None,
            data: Vec::new(),
            extra_params: Vec::new(),
            extra: sl_client_bevy::ObjectExtraParams::default(),
            properties: None,
            joint_type: 0,
            joint_pivot: zero(),
            joint_axis_or_anchor: zero(),
        }
    }

    /// An avatar object classifies as [`ObjectCategory::Avatar`].
    #[test]
    fn avatar_classifies_as_avatar() {
        assert_eq!(
            classify(&bare_object(pcode::AVATAR)),
            ObjectCategory::Avatar
        );
    }

    /// A plain prim (no sculpt/mesh params) classifies as [`ObjectCategory::Prim`].
    #[test]
    fn plain_prim_classifies_as_prim() {
        assert_eq!(
            classify(&bare_object(pcode::PRIMITIVE)),
            ObjectCategory::Prim
        );
    }

    /// A prim carrying a sculpt texture classifies as [`ObjectCategory::Sculpt`],
    /// and one carrying a mesh key as [`ObjectCategory::Mesh`].
    #[test]
    fn sculpt_and_mesh_classify_by_key() {
        let mut sculpt = bare_object(pcode::PRIMITIVE);
        sculpt.extra.sculpt = Some(SculptData {
            texture: SculptOrMeshKey::Sculpt(TextureKey::from(Uuid::from_u128(7))),
            sculpt_type: 1,
        });
        assert_eq!(classify(&sculpt), ObjectCategory::Sculpt);

        let mut mesh = bare_object(pcode::PRIMITIVE);
        mesh.extra.sculpt = Some(SculptData {
            texture: SculptOrMeshKey::Mesh(MeshKey::from(Uuid::from_u128(9))),
            sculpt_type: 5,
        });
        assert_eq!(classify(&mesh), ObjectCategory::Mesh);
    }

    /// A tree object (`PCODE_TREE` / `PCODE_NEW_TREE`) classifies as
    /// [`ObjectCategory::Tree`] and is rendered as procedural branch geometry
    /// (P26.2).
    #[test]
    fn tree_classifies_as_tree() {
        assert_eq!(classify(&bare_object(pcode::TREE)), ObjectCategory::Tree);
        assert_eq!(
            classify(&bare_object(pcode::NEW_TREE)),
            ObjectCategory::Tree
        );
    }

    /// A tree's species comes from its `Data` genome (the reference viewer's
    /// `mSpecies = mData[0]`), **not** its `state` byte: Second Life carries the
    /// species only in `Data` and leaves `state` zero, so reading `state`
    /// rendered every SL tree as species 0 ("Pine 1", a large evergreen).
    #[test]
    fn tree_species_reads_the_data_genome_not_state() {
        // SL: state is 0, the real species (15 = Fern) is in the Data genome.
        let mut sl_tree = bare_object(pcode::TREE);
        sl_tree.state = 0;
        sl_tree.data = vec![15];
        assert_eq!(tree_species_byte(&sl_tree), 15);

        // Data wins even when both are present (OpenSim packs the species into
        // both State and Data, so preferring Data is correct on both grids).
        let mut both = bare_object(pcode::TREE);
        both.state = 7;
        both.data = vec![7];
        assert_eq!(tree_species_byte(&both), 7);

        // A (degenerate) update with no Data defaults to species 0, as the
        // reference does — it never falls back to `state`, which for a tree is 0
        // on SL anyway and would only reintroduce the bug.
        let mut no_data = bare_object(pcode::TREE);
        no_data.state = 3;
        no_data.data = Vec::new();
        assert_eq!(tree_species_byte(&no_data), 0);
    }

    /// A grass object (`PCODE_GRASS`) classifies as [`ObjectCategory::Grass`] and
    /// is rendered as a procedural crossed-quad blade clump (P26.3).
    #[test]
    fn grass_classifies_as_grass() {
        assert_eq!(classify(&bare_object(pcode::GRASS)), ObjectCategory::Grass);
    }

    /// A grass clump's geometry is generated in absolute metres with the object
    /// scale folded into the blade spread, so — unlike a tree — its geometry holder
    /// applies no scale (an identity transform). Its shape fingerprint carries the
    /// clump-defining X/Y scale so a resize rebuilds the clump.
    #[test]
    fn grass_holder_is_identity_and_fingerprint_tracks_scale() {
        let object = bare_object(pcode::GRASS);
        let holder = holder_transform(&object, ObjectCategory::Grass);
        assert!(holder.scale.abs_diff_eq(Vec3::ONE, 1.0e-5));
        assert!(holder.translation.abs_diff_eq(Vec3::ZERO, 1.0e-5));
        // The fingerprint records the X/Y scale (bare_object is scale 2,3,4 → mm).
        let fingerprint = ShapeFingerprint::of(&object);
        assert_eq!(fingerprint.grass_spread, Some((2000, 3000)));
        // A resize changes the fingerprint, so the known-object path rebuilds it.
        let mut resized = object;
        resized.scale.x = 5.0;
        assert_ne!(ShapeFingerprint::of(&resized), fingerprint);
        // A non-grass object carries no grass spread (a resize never rebuilds it).
        assert_eq!(
            ShapeFingerprint::of(&bare_object(pcode::PRIMITIVE)).grass_spread,
            None
        );
    }

    /// A root object's world transform carries its region-local position into
    /// Bevy's Y-up world (Second Life `+Y`/north → Bevy `-Z`) and keeps its
    /// per-axis scale.
    #[test]
    fn root_transform_maps_to_world() {
        let object = bare_object(pcode::PRIMITIVE);
        // No origin known → no region offset (placed as if in the root region).
        let transform = object_transform(&object, true, None);
        // Second Life (10, 20, 30) → Bevy (x, z, -y) = (10, 30, -20).
        assert!(
            transform
                .translation
                .abs_diff_eq(Vec3::new(10.0, 30.0, -20.0), 1.0e-5)
        );
        // The object entity carries no scale (it would propagate to linkset
        // children); the scale rides the geometry holder instead.
        assert!(transform.scale.abs_diff_eq(Vec3::ONE, 1.0e-5));
        assert!(
            geometry_transform(&object)
                .scale
                .abs_diff_eq(Vec3::new(2.0, 3.0, 4.0), 1.0e-5)
        );
    }

    /// A root object in a **neighbour** region is offset onto that region's
    /// terrain: its region-local placement plus the region's global-metre offset
    /// from the scene origin. A child stays parent-relative and gets no offset.
    #[test]
    fn root_transform_offsets_a_neighbour_region() {
        // Origin at the SW corner (1024, 1024); the object's region is 256 m east.
        let origin = RegionHandle::new((1024_u64 << 32) | 1024);
        let mut object = bare_object(pcode::PRIMITIVE);
        object.region_handle = RegionHandle::new((1280_u64 << 32) | 1024);
        // Root: (10, 20, 30) → Bevy (10, 30, -20), plus +256 east (Bevy +X).
        let root = object_transform(&object, true, Some(origin));
        assert!(
            root.translation
                .abs_diff_eq(Vec3::new(266.0, 30.0, -20.0), 1.0e-4)
        );
        // A child is parent-relative — the neighbour offset must NOT apply (its
        // root already carries it).
        let child = object_transform(&object, false, Some(origin));
        assert!(
            child
                .translation
                .abs_diff_eq(Vec3::new(10.0, 20.0, 30.0), 1.0e-4)
        );
    }

    /// A child object's local transform stays in pure Second Life space (no axis
    /// swap), since the parent entity carries the basis change.
    #[test]
    fn child_transform_stays_in_sl_space() {
        let object = bare_object(pcode::PRIMITIVE);
        let transform = object_transform(&object, false, None);
        // The parent-relative offset is carried across verbatim.
        assert!(
            transform
                .translation
                .abs_diff_eq(Vec3::new(10.0, 20.0, 30.0), 1.0e-5)
        );
    }

    /// A motion-only change leaves the shape fingerprint equal, so no
    /// re-tessellation is triggered; changing a shape parameter changes it.
    #[test]
    fn fingerprint_ignores_motion_but_tracks_shape() {
        let object = bare_object(pcode::PRIMITIVE);
        let base = ShapeFingerprint::of(&object);

        let mut moved = object.clone();
        moved.motion.position.x = 999.0;
        moved.scale.x = 8.0;
        assert_eq!(
            base,
            ShapeFingerprint::of(&moved),
            "motion/scale must not count"
        );

        let mut reshaped = object.clone();
        reshaped.shape.profile_hollow = 12_345;
        assert_ne!(
            base,
            ShapeFingerprint::of(&reshaped),
            "a shape change must count"
        );
    }

    /// Flexi faces must stay ordinary `Aabb`-managed entities
    /// (`viewer-flexi-prim-picking`): a `NoFrustumCulling` opt-out means
    /// `calculate_bounds` never gives them an `Aabb` — and `MeshRayCast` reads
    /// the `Aabb` non-optionally, so an opted-out flexi is silently invisible
    /// to every world pick (left-click touch, the object pie menu) on top of
    /// never being culled. The per-frame mesh rewrite keeps the `Aabb` fresh
    /// instead (see `simulated_flexi_mesh_keeps_its_aabb_fresh` in
    /// `crate::flexi`).
    #[test]
    fn flexi_faces_stay_aabb_managed() -> Result<(), Box<dyn core::error::Error>> {
        use crate::textures::{PrimTextures, TextureManager};
        use bevy::camera::visibility::NoFrustumCulling;
        use bevy::ecs::system::SystemState;
        use bevy::prelude::{Assets, Commands, Mesh, Mesh3d, ResMut, World};
        use sl_client_bevy::{FlexibleData, Priority};

        use crate::face_material::FaceMaterial;

        /// The resources [`build_flexi_faces`](super::build_flexi_faces) takes,
        /// as one `SystemState` tuple (named to satisfy `type_complexity`).
        type BuildParams<'w, 's> = (
            Commands<'w, 's>,
            ResMut<'w, Assets<Mesh>>,
            ResMut<'w, Assets<FaceMaterial>>,
            ResMut<'w, TextureManager>,
            ResMut<'w, PrimTextures>,
        );

        let mut world = World::new();
        world.init_resource::<Assets<Mesh>>();
        world.init_resource::<Assets<FaceMaterial>>();
        world.init_resource::<TextureManager>();
        world.init_resource::<PrimTextures>();
        let parent = world.spawn_empty().id();

        let mut object = bare_object(pcode::PRIMITIVE);
        object.extra.flexible = Some(FlexibleData {
            softness: 2,
            tension: 1.0,
            air_friction: 2.0,
            gravity: 0.3,
            wind_sensitivity: 0.0,
            user_force: zero(),
        });

        let mut state: SystemState<BuildParams> = SystemState::new(&mut world);
        let (mut commands, mut meshes, mut materials, mut manager, mut prim_textures) = state
            .get_mut(&mut world)
            .map_err(|error| format!("system params: {error}"))?;
        let intern = crate::material_cache::MaterialInternContext::for_object(&object, false);
        let mut material_cache = crate::material_cache::MaterialCache::default();
        let (faces, _chain) = super::build_flexi_faces(
            &object,
            parent,
            &mut commands,
            &mut meshes,
            &mut materials,
            &mut manager,
            &mut prim_textures,
            Priority::IDLE,
            &intern,
            &mut material_cache,
        );
        state.apply(&mut world);

        assert!(
            !faces.is_empty(),
            "a default prim must tessellate at least one flexi face"
        );
        for face in faces {
            assert!(world.get::<Mesh3d>(face).is_some(), "face without a mesh");
            assert!(
                world.get::<NoFrustumCulling>(face).is_none(),
                "a flexi face opted out of Aabb management — that makes it \
                 invisible to MeshRayCast and unpickable (viewer-flexi-prim-picking)"
            );
        }
        Ok(())
    }

    /// The spawn budget spends only on geometry-builds, processes strictly FIFO, and
    /// leaves the overflow queued: a burst of five events (three builds) with a budget
    /// of two processes the first four (both builds plus the free cheap items between)
    /// and holds the third build back for the next frame.
    #[test]
    fn spawn_budget_charges_builds_only_and_preserves_fifo() {
        // (id, is_build): a build costs budget; a cheap item (move / remove) is free.
        let mut queue: VecDeque<(u32, bool)> = VecDeque::from(vec![
            (0, false),
            (1, true),
            (2, false),
            (3, true),
            (4, true),
        ]);
        let mut processed = Vec::new();
        let builds = drain_budgeted(&mut queue, 2, |item| {
            processed.push(item.0);
            item.1
        });
        assert_eq!(builds, 2, "stops after the second build");
        assert_eq!(
            processed,
            vec![0, 1, 2, 3],
            "FIFO: cheap items ahead of / between the budgeted builds are freed too"
        );
        assert_eq!(
            queue.into_iter().map(|item| item.0).collect::<Vec<_>>(),
            vec![4],
            "the third build waits for the next frame",
        );
    }

    /// Under budget, the whole backlog drains and nothing is left queued.
    #[test]
    fn spawn_budget_drains_the_whole_backlog_when_under_budget() {
        let mut queue: VecDeque<bool> = VecDeque::from(vec![true, false, true, false]);
        let builds = drain_budgeted(&mut queue, 10, |item| item);
        assert_eq!(builds, 2);
        assert!(queue.is_empty(), "the backlog fully drains");
    }

    /// Value convention for the LOD-budget tests: an **even** key is a free skip
    /// (resolved / already at the desired level), an **odd** key needs a rebuild.
    fn classify_lod(key: u32, remaining: usize, built: &mut Vec<u32>) -> LodOutcome {
        if key.is_multiple_of(2) {
            LodOutcome::Resolved
        } else if remaining == 0 {
            LodOutcome::Deferred
        } else {
            built.push(key);
            LodOutcome::Rebuilt
        }
    }

    /// The LOD budget spends only on rebuilds (free skips are removed for free),
    /// caps rebuilds at the budget, and keeps the over-budget rebuild targets in
    /// the map for a later frame while dropping every resolved one.
    #[test]
    fn lod_budget_charges_rebuilds_only_and_keeps_overflow() {
        // Keys 0..6: evens {0,2,4} are free skips, odds {1,3,5} need a rebuild.
        let mut map: HashMap<u32, u32> = (0..6).map(|key| (key, 0)).collect();
        let mut built = Vec::new();
        let builds = retain_lod_budgeted(&mut map, 2, |key, _value, remaining| {
            classify_lod(key, remaining, &mut built)
        });

        assert_eq!(builds, 2, "stops after the second rebuild");
        assert_eq!(built.len(), 2, "exactly two rebuilds ran");
        assert!(
            built.iter().all(|key| key % 2 == 1),
            "only rebuild (odd) keys were built, never a free skip",
        );
        assert_eq!(
            map.len(),
            1,
            "the third rebuild waits; all three free skips are dropped",
        );
        assert!(
            map.keys().all(|key| !key.is_multiple_of(2)),
            "the deferred target is an un-built (odd) rebuild, not a free skip",
        );
        assert!(
            map.keys().all(|key| !built.contains(key)),
            "the deferred target was not also built",
        );
    }

    /// Under budget, every rebuild is applied and the whole target map empties
    /// (both the rebuilds and the free skips are removed).
    #[test]
    fn lod_budget_applies_all_when_under_budget() {
        let mut map: HashMap<u32, u32> = (0..6).map(|key| (key, 0)).collect();
        let mut built = Vec::new();
        let builds = retain_lod_budgeted(&mut map, 10, |key, _value, remaining| {
            classify_lod(key, remaining, &mut built)
        });

        assert_eq!(builds, 3, "all three odd keys rebuilt");
        assert!(map.is_empty(), "nothing deferred, all skips dropped");
    }

    /// Repeated upserts for one still-queued object coalesce into a single
    /// queue slot holding the newest snapshot (every event carries a full
    /// merged snapshot, so only the newest matters), while a remove queued
    /// between two upserts blocks the merge so replay order stays
    /// upsert → remove → upsert.
    #[test]
    fn pending_object_events_coalesce_repeated_upserts() {
        let mut first = bare_object(pcode::PRIMITIVE);
        first.crc = 1;
        let mut second = bare_object(pcode::PRIMITIVE);
        second.crc = 2;
        let scoped = first.scoped_id();

        let mut pending = super::PendingObjectEvents::default();
        pending.push_upsert(&first);
        pending.push_upsert(&second);
        assert_eq!(
            pending.queue.len(),
            1,
            "the second upsert merged into the queued slot"
        );
        assert_eq!(
            super::pop_upsert_payload(&mut pending.payloads, scoped).map(|object| object.crc),
            Some(2),
            "the newest snapshot won"
        );
        assert!(
            pending.payloads.is_empty(),
            "the drained id's payload slot is dropped"
        );

        // An upsert queued behind a remove for the same id must not merge
        // across it — the replay must still remove before re-adding.
        pending.clear();
        pending.push_upsert(&first);
        pending.push_remove(scoped);
        pending.push_upsert(&second);
        assert_eq!(
            pending.queue.len(),
            3,
            "upsert → remove → upsert stays three ordered events"
        );
        assert_eq!(
            super::pop_upsert_payload(&mut pending.payloads, scoped).map(|object| object.crc),
            Some(1),
            "the pre-remove snapshot survives unmerged"
        );
        assert_eq!(
            super::pop_upsert_payload(&mut pending.payloads, scoped).map(|object| object.crc),
            Some(2),
            "the post-remove snapshot queues separately"
        );
    }

    /// A [`TrackedObject`](super::TrackedObject) stub for the stale-guard tests: a
    /// plain-prim root at `entity` / `geometry`, every other field at its spawn
    /// default. Only the `entity` field is under test.
    fn tracked_stub(
        object: &Object,
        entity: bevy::prelude::Entity,
        geometry: bevy::prelude::Entity,
    ) -> super::TrackedObject {
        super::TrackedObject {
            entity,
            full_key: object.full_id,
            geometry,
            shape: ShapeFingerprint::of(object),
            parent: object.scoped_id(),
            is_root: true,
            parented: false,
            attachment_point: None,
            owner_id: AgentKey::from(object.owner_id),
            update_flags: object.update_flags,
            material: object.material,
            extra: object.extra.clone(),
            texture_animation: object.texture_animation,
            text: object.text.clone(),
            text_color: object.text_color,
            face_entities: Vec::new(),
            pending: None,
            mesh_rebuild: None,
            prim_rebuild: None,
            prim_lod: super::INITIAL_MANAGED_PRIM_LOD,
            tree_rebuild: None,
            tree_tier: super::INITIAL_TREE_TIER,
            animated: false,
            texture_entry: Vec::new(),
            media_url: None,
        }
    }

    /// The terse-update fast path's gate: a motion-only update (the merged
    /// snapshot moves, but every per-block component input is identical)
    /// reports unchanged — while a flipped block input (floating text, update
    /// flags / physics toggle, material byte, linkset identity) trips it.
    #[test]
    fn non_motion_gate_ignores_motion_and_tracks_block_inputs() {
        use bevy::prelude::World;

        let object = bare_object(pcode::PRIMITIVE);
        let scoped = object.scoped_id();
        let mut world = World::new();
        let entity = world.spawn_empty().id();
        let geometry = world.spawn_empty().id();
        let tracked = tracked_stub(&object, entity, geometry);

        // A pure motion change: position and velocity move, nothing else.
        let mut moved = object.clone();
        moved.motion.position.x += 5.0;
        moved.motion.velocity.z = 1.5;
        assert!(
            !tracked.non_motion_blocks_changed(&moved, true, scoped, None),
            "a motion-only update must take the fast path"
        );

        // Floating text set by a script.
        let mut texted = object.clone();
        texted.text = "hello".to_owned();
        assert!(tracked.non_motion_blocks_changed(&texted, true, scoped, None));

        // An update-flags change (e.g. the physics toggle).
        let mut flagged = object.clone();
        flagged.update_flags |= 1;
        assert!(tracked.non_motion_blocks_changed(&flagged, true, scoped, None));

        // A material-byte change.
        let mut rematerialed = object.clone();
        rematerialed.material = rematerialed.material.wrapping_add(1);
        assert!(tracked.non_motion_blocks_changed(&rematerialed, true, scoped, None));

        // A linkset-identity change (unlink/relink).
        assert!(tracked.non_motion_blocks_changed(&object, false, scoped, None));
    }

    /// The stale-entity guard drops a tracked object whose entity Bevy's recursive
    /// despawn has already taken with its parent — the linkset-child / worn-attachment
    /// race behind the `bevy_ecs::error::handler` "Entity despawned" warning. The
    /// entry must be gone so a later update respawns the object cleanly instead of
    /// queuing an insert on a dead (later generation-mismatched) entity.
    #[test]
    fn stale_guard_drops_a_hierarchy_despawned_tracked_object() {
        use bevy::prelude::{ChildOf, World};

        let mut world = World::new();
        // A linkset: a root, its child, and the child's geometry holder. Bevy's
        // recursive despawn takes the child (and holder) with the root, exactly how an
        // objects.rs entity dies without our own `remove_object` cleaning the map.
        let root = world.spawn_empty().id();
        let child = world.spawn(ChildOf(root)).id();
        let geometry = world.spawn(ChildOf(child)).id();

        let object = bare_object(pcode::PRIMITIVE);
        let scoped = object.scoped_id();
        let mut state = super::ObjectState::default();
        let _absent = state
            .objects
            .insert(scoped, tracked_stub(&object, child, geometry));

        world.entity_mut(root).despawn();
        assert!(
            world.get_entity(child).is_err(),
            "the child dies with its root — the premise of the race"
        );

        let dropped =
            super::drop_stale_tracked_entity(&mut state, scoped, |e| world.get_entity(e).is_ok());
        assert_eq!(dropped, Some(child), "the stale entry is reported dropped");
        assert!(
            state.objects.is_empty(),
            "the stale entry is gone so a later update respawns the object"
        );
    }

    /// The guard leaves a live tracked object untouched: it must never drop an object
    /// still on screen, or a real live transform / material write would be lost (the
    /// warning's fix must not become a rendering bug of its own).
    #[test]
    fn stale_guard_keeps_a_live_tracked_object() {
        use bevy::prelude::World;

        let mut world = World::new();
        let entity = world.spawn_empty().id();
        let geometry = world.spawn_empty().id();

        let object = bare_object(pcode::PRIMITIVE);
        let scoped = object.scoped_id();
        let mut state = super::ObjectState::default();
        let _absent = state
            .objects
            .insert(scoped, tracked_stub(&object, entity, geometry));

        let dropped =
            super::drop_stale_tracked_entity(&mut state, scoped, |e| world.get_entity(e).is_ok());
        assert_eq!(dropped, None, "a live entity is not dropped");
        assert!(
            state.objects.contains_key(&scoped),
            "the live object is retained"
        );
    }

    /// End-to-end guard on the real [`apply_object`](super::apply_object) path: a
    /// linkset child whose entity was despawned out from under the map (Bevy's
    /// recursive despawn taking it with a parent, without our `remove_object`) is
    /// **respawned** on its next update — a fresh, live entity re-parented to its
    /// still-live root — rather than queuing an insert on the dead entity (the
    /// `bevy_ecs::error::handler` "Entity despawned" warning this task fixes).
    #[test]
    fn apply_object_respawns_a_child_despawned_out_from_under_the_map()
    -> Result<(), Box<dyn core::error::Error>> {
        use crate::face_material::FaceMaterial;
        use crate::geometry_cache::GeometryCache;
        use crate::material_cache::MaterialCache;
        use crate::meshes::MeshManager;
        use crate::textures::{PrimTextures, TextureManager};
        use bevy::ecs::system::SystemState;
        use bevy::prelude::{Assets, ChildOf, Commands, Mesh, ResMut, World};

        /// The resources [`apply_object`](super::apply_object) takes, as one
        /// `SystemState` tuple (named to satisfy `type_complexity`).
        type ApplyParams<'w, 's> = (
            Commands<'w, 's>,
            ResMut<'w, Assets<Mesh>>,
            ResMut<'w, Assets<FaceMaterial>>,
            ResMut<'w, TextureManager>,
            ResMut<'w, PrimTextures>,
            ResMut<'w, MeshManager>,
            ResMut<'w, GeometryCache>,
            ResMut<'w, MaterialCache>,
        );

        let mut world = World::new();
        world.init_resource::<Assets<Mesh>>();
        world.init_resource::<Assets<FaceMaterial>>();
        world.init_resource::<TextureManager>();
        world.init_resource::<PrimTextures>();
        world.init_resource::<MeshManager>();
        world.init_resource::<GeometryCache>();
        world.init_resource::<MaterialCache>();

        let mut state = super::ObjectState::default();
        let root_obj = bare_object(pcode::PRIMITIVE);
        let mut child_obj = bare_object(pcode::PRIMITIVE);
        child_obj.local_id = RegionLocalObjectId(2);
        // A linkset child: its parent is the root prim (local id 1).
        child_obj.parent_id = RegionLocalObjectId(1);
        let root_scoped = root_obj.scoped_id();
        let child_scoped = child_obj.scoped_id();

        // Runs one `apply_object` and flushes its commands into `world`. Takes
        // `world` as a parameter (rather than capturing it) so the world stays free
        // to inspect / despawn between invocations.
        let apply = |world: &mut World,
                     state: &mut super::ObjectState,
                     object: &Object|
         -> Result<(), Box<dyn core::error::Error>> {
            let mut params: SystemState<ApplyParams> = SystemState::new(world);
            let (
                mut commands,
                mut meshes,
                mut materials,
                mut manager,
                mut prim_textures,
                mut mesh_manager,
                mut cache,
                mut material_cache,
            ) = params
                .get_mut(world)
                .map_err(|error| format!("system params: {error}"))?;
            super::apply_object(
                state,
                object,
                &mut commands,
                &mut meshes,
                &mut materials,
                &mut manager,
                &mut prim_textures,
                &mut mesh_manager,
                &mut cache,
                &mut material_cache,
            );
            params.apply(world);
            Ok(())
        };

        // Spawn the root then the child; the child parents to the root's entity.
        apply(&mut world, &mut state, &root_obj)?;
        apply(&mut world, &mut state, &child_obj)?;
        let root_entity = state
            .objects
            .get(&root_scoped)
            .ok_or("root tracked")?
            .entity;
        let child_entity = state
            .objects
            .get(&child_scoped)
            .ok_or("child tracked")?
            .entity;
        assert!(world.get_entity(child_entity).is_ok(), "child spawned live");

        // Kill just the child entity — standing in for the hierarchy despawn that
        // takes a child / attachment with its parent (a linkset root or an avatar's
        // joint node) with no `remove_object` to clean the map. The stale entry stays.
        world.entity_mut(child_entity).despawn();
        assert!(
            world.get_entity(child_entity).is_err(),
            "the child entity is now dead"
        );
        assert!(
            state.objects.contains_key(&child_scoped),
            "but the map still tracks it (no remove_object ran) — the stale entry"
        );

        // A later ObjectUpdated for the child: the guard drops the stale entry and the
        // spawn path re-creates the object, re-parented to the still-live root.
        apply(&mut world, &mut state, &child_obj)?;
        let new_child = state
            .objects
            .get(&child_scoped)
            .ok_or("child re-tracked")?
            .entity;
        assert_ne!(new_child, child_entity, "respawned as a fresh entity");
        assert!(
            world.get_entity(new_child).is_ok(),
            "the respawned child entity is live"
        );
        assert_eq!(
            world.get::<ChildOf>(new_child).map(ChildOf::parent),
            Some(root_entity),
            "the respawned child re-parents to its still-live root"
        );
        Ok(())
    }
}
