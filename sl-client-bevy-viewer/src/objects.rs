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

use std::collections::HashMap;
use std::sync::Arc;

use bevy::camera::visibility::NoFrustumCulling;
use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
use bevy::prelude::*;
use sl_client_bevy::{
    AgentKey, DecodedMesh, DecodedTexture, GRASS_MAX_BLADES, JointOverrides, MeshKey, MeshSkin,
    Object, PrimFaceId, PrimLod, PrimMesh, PrimShapeFloat, PrimShapeParams, Priority,
    ScopedObjectId, SculptOrMeshKey, SlEvent, SlSessionEvent, TREE_RADIUS_SCALE_FACTOR,
    TREE_YAW_DEGREES, TextureFace, TextureKey, TreeLod, Uuid, Vector, avatar_texture,
    decode_texture_entry, grass_geometry, grass_species, pcode, planar_texgen_uv,
    rigged_inverse_bindposes, tessellate, tessellate_sculpt, to_bevy_grass_mesh, to_bevy_mesh,
    to_bevy_prim_mesh, to_bevy_rigged_mesh, to_bevy_tree_mesh, tree_billboard_geometry,
    tree_geometry, tree_species,
};

use crate::avatars::{AvatarBody, AvatarState, BomFace};
use crate::coords::{sl_rotation_to_quat, sl_to_bevy_object_rotation, sl_to_bevy_vec};
use crate::lights::{ObjectLight, light_from_object};
use crate::materials::ObjectRenderMaterials;
use crate::meshes::{MeshDecoded, MeshManager};
use crate::render_priority::AVATAR_BOOST_PRIORITY;
use crate::textures::{PrimTextures, TextureDecoded, TextureManager, face_material};

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
#[derive(Component, Debug, Clone, Copy)]
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

/// Per-object viewer-side bookkeeping, paired with the object's [`SceneObject`]
/// entity.
struct TrackedObject {
    /// The entity rendering this object: carries its position/rotation and is the
    /// parent linkset children and attachments hang off. It has **no scale** (see
    /// [`object_transform`]).
    entity: Entity,
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
    /// tracks whether it has been parented to its avatar's skeleton joint (P16.1).
    parented: bool,
    /// The raw attachment-point id if this object is an attachment worn on an
    /// avatar (its `parent` is the avatar), else `None`. An attachment is parented
    /// to its avatar's skeleton joint rather than a linkset root, by
    /// [`adopt_pending_attachments`] (P16.1).
    attachment_point: Option<u8>,
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
/// Keyed on the object carrying an attachment point (a worn attachment root); a
/// linkset child of a multi-prim attachment is not itself flagged, so this is the
/// common single-object attachment case.
const fn worn_base_priority(object: &Object) -> Priority {
    if object.attachment_point_id().is_some() {
        AVATAR_BOOST_PRIORITY
    } else {
        Priority::IDLE
    }
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
    /// The tree species byte (the object `state`), indexing the `LLVOTree`
    /// species table for the diffuse texture and geometry parameters.
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
}

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
fn object_transform(object: &Object, is_root: bool) -> Transform {
    if is_root {
        Transform {
            translation: sl_to_bevy_vec(&object.motion.position),
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

/// Fold the object event stream into the scene graph: spawn/update/despawn
/// entities, classify them, keep their transforms current, and maintain linkset
/// parenting.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system reading the object stream and the ECS resources the geometry build needs"
)]
pub(crate) fn update_objects(
    mut events: MessageReader<SlEvent>,
    mut state: ResMut<ObjectState>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut manager: ResMut<TextureManager>,
    mut prim_textures: ResMut<PrimTextures>,
    mut mesh_manager: ResMut<MeshManager>,
) {
    for event in events.read() {
        match &event.0 {
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
                );
            }
            SlSessionEvent::ObjectRemoved { local_id, .. } => {
                remove_object(&mut state, *local_id, &mut commands);
            }
            _other => {}
        }
    }
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
    camera: Query<&GlobalTransform, With<Camera3d>>,
    mut ray_cast: MeshRayCast,
    scene: Query<&SceneObject>,
    infos: Query<&ObjectDebugInfo>,
    lights: Query<&ObjectLight>,
    globals: Query<&GlobalTransform>,
    parents: Query<&ChildOf>,
    face_debug: Query<(&PrimFaceEntity, &FaceTextureDebug)>,
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
    // walking up to the object identity.
    if let Ok((face, FaceTextureDebug(tf))) = face_debug.get(*entity) {
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
fn apply_render_materials(geometry: Entity, object: &Object, commands: &mut Commands) {
    let faces: Vec<(u8, Uuid)> = object
        .extra
        .render_material
        .iter()
        .map(|reference| (reference.face, reference.material_id))
        .collect();
    if faces.is_empty() {
        commands.entity(geometry).remove::<ObjectRenderMaterials>();
    } else {
        commands
            .entity(geometry)
            .insert(ObjectRenderMaterials { faces });
    }
}

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
/// The last two returns are the plain-prim re-tessellation inputs
/// ([`PendingPrim`], P21.3) and the tree regeneration inputs ([`PendingTree`],
/// P26.2); at most one is ever `Some`.
#[expect(
    clippy::too_many_arguments,
    reason = "threads the several ECS resources the geometry build needs"
)]
fn build_object_geometry(
    object: &Object,
    category: ObjectCategory,
    entity: Entity,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    manager: &mut TextureManager,
    prim_textures: &mut PrimTextures,
    mesh_manager: &mut MeshManager,
) -> (
    Vec<Entity>,
    Option<PendingGeometry>,
    Option<PendingPrim>,
    Option<PendingTree>,
) {
    // A worn attachment's textures / mesh are boosted so they load with the
    // avatar rather than queued behind the surrounding scene (P20.2).
    let priority = worn_base_priority(object);
    match category {
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
            }),
            None,
        ),
        ObjectCategory::Mesh => {
            let Some(key) = mesh_key(object) else {
                return (Vec::new(), None, None, None);
            };
            mesh_manager.request(key, priority);
            // The store hands back an `Arc`; clone it out so the immutable borrow
            // of `mesh_manager` ends before the submesh build borrows the other
            // resources.
            match mesh_manager.decoded(key).map(Arc::clone) {
                Some(decoded) => (
                    build_mesh_submeshes(
                        &decoded,
                        &object.texture_entry,
                        [object.scale.x, object.scale.y, object.scale.z],
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
                    None,
                ),
                None => (
                    Vec::new(),
                    Some(PendingGeometry::Mesh(PendingMesh {
                        key,
                        texture_entry: object.texture_entry.clone(),
                        scale: [object.scale.x, object.scale.y, object.scale.z],
                        priority,
                    })),
                    None,
                    None,
                ),
            }
        }
        ObjectCategory::Sculpt => {
            let Some((map, sculpt_type)) = sculpt_key(object) else {
                return (Vec::new(), None, None, None);
            };
            manager.request_boosted(map, priority);
            // The store hands back an `Arc`; clone it out so the immutable borrow
            // of `manager` ends before the face build borrows it mutably.
            match manager.decoded(map).map(Arc::clone) {
                Some(map_image) => (
                    build_sculpt_faces(
                        &map_image,
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
                    ),
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
                    })),
                    None,
                    None,
                ),
            }
        }
        ObjectCategory::Tree => (
            build_tree_faces(
                object.state,
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
                species: object.state,
                priority,
            }),
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
        ),
        ObjectCategory::Avatar | ObjectCategory::Other => (Vec::new(), None, None, None),
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
#[expect(
    clippy::too_many_arguments,
    reason = "threads the several ECS resources the geometry build needs, plus the fetch priority and LOD"
)]
fn build_prim_faces(
    object: &Object,
    parent: Entity,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    manager: &mut TextureManager,
    prim_textures: &mut PrimTextures,
    priority: Priority,
    lod: PrimLod,
) -> Vec<Entity> {
    let shape = PrimShapeFloat::from_params(&object.shape);
    let prim = tessellate(&shape, lod);
    spawn_prim_faces(
        &prim,
        &object.texture_entry,
        [object.scale.x, object.scale.y, object.scale.z],
        parent,
        commands,
        meshes,
        materials,
        manager,
        prim_textures,
        priority,
    )
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
#[expect(
    clippy::too_many_arguments,
    reason = "threads the several ECS resources the geometry build needs"
)]
fn build_sculpt_faces(
    map: &DecodedTexture,
    sculpt_type: u8,
    texture_entry: &[u8],
    scale: [f32; 3],
    parent: Entity,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    manager: &mut TextureManager,
    prim_textures: &mut PrimTextures,
    priority: Priority,
) -> Vec<Entity> {
    let prim = tessellate_sculpt(map, sculpt_type);
    spawn_prim_faces(
        &prim,
        texture_entry,
        scale,
        parent,
        commands,
        meshes,
        materials,
        manager,
        prim_textures,
        priority,
    )
}

/// Generate a Linden tree's branch / leaf geometry for `species_byte` at
/// [`TreeTier`] `tier` and spawn its single face entity under `parent`, textured
/// with the species diffuse through the Phase 6 pipeline (P26.2).
///
/// The species byte is the object's `state`; an out-of-range value clamps to
/// species `0`, matching the reference viewer. The geometry (a branch/leaf mesh
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
    materials: &mut Assets<StandardMaterial>,
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
    let material = face_material(&texture_face, materials, manager, prim_textures, priority);
    // Foliage is alpha-**masked** (cutout), not opaque or blended: the reference
    // viewer renders trees in the alpha-mask pool so the leaf-card texture's alpha
    // clips each leaf to its shape (transparent around the edges) rather than
    // showing a solid quad. A fixed cutoff clips the trunk (opaque) cleanly too.
    // Set here so it is not overridden by the tint-based opaque/blend default.
    if let Some(mut tree_material) = materials.get_mut(&material) {
        tree_material.alpha_mode = AlphaMode::Mask(TREE_ALPHA_CUTOFF);
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
    materials: &mut Assets<StandardMaterial>,
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
    let material = face_material(&texture_face, materials, manager, prim_textures, priority);
    // Grass is alpha-**blended** (the reference's `PASS_GRASS` / `POOL_ALPHA`), so
    // the soft blade-card edges fade rather than clip. Set here so it is not
    // overridden by the tint-based opaque default.
    if let Some(mut grass_material) = materials.get_mut(&material) {
        grass_material.alpha_mode = AlphaMode::Blend;
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
    materials: &mut Assets<StandardMaterial>,
    manager: &mut TextureManager,
    prim_textures: &mut PrimTextures,
    priority: Priority,
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
        let material = face_material(texture_face, materials, manager, prim_textures, priority);
        let entity = commands
            .spawn((
                Mesh3d(mesh),
                MeshMaterial3d(material),
                PrimFaceEntity {
                    face_id: face.face_id,
                },
                FaceTextureDebug(*texture_face),
                ChildOf(parent),
            ))
            .id();
        face_entities.push(entity);
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
#[expect(
    clippy::too_many_arguments,
    reason = "threads the several ECS resources the geometry build needs"
)]
fn build_mesh_submeshes(
    decoded: &DecodedMesh,
    texture_entry: &[u8],
    scale: [f32; 3],
    parent: Entity,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    manager: &mut TextureManager,
    prim_textures: &mut PrimTextures,
    priority: Priority,
) -> Vec<Entity> {
    let entry = decode_texture_entry(texture_entry, decoded.submeshes.len());
    // The slot every face falls back to when the object carries no texture entry:
    // an untextured, opaque-white (untinted) face.
    let default_face = TextureFace::new(TextureKey::from(Uuid::nil()));
    let mut face_entities = Vec::new();
    for (index, submesh) in decoded.submeshes.iter().enumerate() {
        if submesh.no_geometry {
            continue;
        }
        let texture_face = entry.face(index).unwrap_or(&default_face);
        let mut bevy_mesh = to_bevy_mesh(submesh);
        apply_planar_texgen(
            &mut bevy_mesh,
            &submesh.positions,
            &submesh.normals,
            texture_face,
            scale,
        );
        let mesh = meshes.add(bevy_mesh);
        let material = face_material(texture_face, materials, manager, prim_textures, priority);
        // The submesh index is the Linden face index; a mesh has few faces, so the
        // widening never saturates in practice (a clamp keeps it lint-clean).
        let face_id = PrimFaceId::new(u16::try_from(index).unwrap_or(u16::MAX));
        let entity = commands
            .spawn((
                Mesh3d(mesh),
                MeshMaterial3d(material),
                PrimFaceEntity { face_id },
                FaceTextureDebug(*texture_face),
                ChildOf(parent),
            ))
            .id();
        face_entities.push(entity);
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

/// Spawn or update the entity for `object`, keeping its transform, classification,
/// and linkset parenting current.
#[expect(
    clippy::too_many_arguments,
    reason = "threads the several ECS resources the geometry build needs"
)]
fn apply_object(
    state: &mut ObjectState,
    object: &Object,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    manager: &mut TextureManager,
    prim_textures: &mut PrimTextures,
    mesh_manager: &mut MeshManager,
) {
    let scoped = object.scoped_id();
    let parent = object.scoped_parent_id();
    let is_root = object.parent_id.get() == 0;
    // A non-zero attachment-point id marks an attachment worn on an avatar: its
    // `parent` is the avatar, and it is parented to that avatar's skeleton joint
    // (P16.1) by `adopt_pending_attachments`, not to a linkset root here.
    let attachment_point = object.attachment_point_id();
    let category = classify(object);
    let shape = ShapeFingerprint::of(object);
    let transform = object_transform(object, is_root);
    // The parent's entity, if its root is already tracked (looked up before the
    // mutable borrow of the object's own entry below). A root has no parent, and
    // an attachment is left for the skeleton-joint parenting path — both `None`.
    let parent_entity = if is_root || attachment_point.is_some() {
        None
    } else {
        state.objects.get(&parent).map(|root| root.entity)
    };

    // The crosshair pick tool's identity for this object (full id, mesh/sculpt
    // asset, Second Life scale/position), refreshed with each update.
    // The object's light block (P25.1): present only if the prim is a light
    // source. Refreshed on every update so a light toggled off / retuned in-world
    // is reflected — [`apply_light`] inserts the component when present and
    // removes it when absent.
    let light = light_from_object(object);

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

    if let Some(existing) = state.objects.get_mut(&scoped) {
        // A known object: re-place it and refresh its classification (a
        // motion-only update stops here — the geometry is untouched). The scale
        // rides the geometry holder, refreshed here so a live resize is applied
        // without a re-tessellation.
        commands.entity(existing.entity).insert((
            transform,
            SceneObject {
                scoped_id: scoped,
                category,
            },
            debug_info,
        ));
        commands
            .entity(existing.geometry)
            .insert(holder_transform(object, category));
        apply_render_materials(existing.geometry, object, commands);
        apply_light(existing.entity, light, commands);
        if existing.shape != shape {
            // A genuine shape (or category) change: drop the old face meshes and
            // re-tessellate. A category change is subsumed here, since the
            // fingerprint covers pcode and the sculpt/mesh key.
            debug!("object {scoped} shape changed; re-tessellating");
            despawn_prim_faces(&existing.face_entities, commands);
            let (face_entities, pending, prim_rebuild, tree_rebuild) = build_object_geometry(
                object,
                category,
                existing.geometry,
                commands,
                meshes,
                materials,
                manager,
                prim_textures,
                mesh_manager,
            );
            existing.face_entities = face_entities;
            existing.pending = pending;
            // The geometry was re-requested from scratch; any prior LOD-rebuild
            // inputs are stale (the mesh key, scale, or category may have changed)
            // and are re-established from the new build: a mesh's on its next
            // decode (P21.2), a plain prim's immediately here (P21.3), a tree's here
            // (P26.2). An object that changed category drops the rebuild inputs it no
            // longer has (`prim_rebuild` / `tree_rebuild` is `None`).
            existing.mesh_rebuild = None;
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
            reconcile_parent(existing, is_root, parent_entity, commands);
        }
        existing.parent = parent;
        existing.is_root = is_root;
        existing.attachment_point = attachment_point;
        existing.animated = is_animated_object(object);
        return;
    }

    // A new object: spawn its entity, parent it if its root is already present,
    // and adopt any of its children that arrived first.
    let entity = commands
        .spawn((
            SceneObject {
                scoped_id: scoped,
                category,
            },
            debug_info,
            transform,
            // The per-face child meshes carry `Visibility` (required by
            // `Mesh3d`); the object entity needs it too so Bevy's visibility
            // propagation down the linkset hierarchy stays consistent.
            Visibility::default(),
        ))
        .id();
    let parented = match parent_entity {
        Some(root_entity) => {
            commands.entity(entity).insert(ChildOf(root_entity));
            true
        }
        None => false,
    };
    // A light-source prim carries its decoded light block (P25.1); a plain prim
    // gets nothing.
    apply_light(entity, light, commands);
    // The geometry holder: a child of the object entity carrying only the object's
    // scale, so the object's own faces are scaled while linkset children (which
    // parent to the object entity, not this) are not.
    let geometry = commands
        .spawn((
            holder_transform(object, category),
            Visibility::default(),
            ChildOf(entity),
        ))
        .id();
    apply_render_materials(geometry, object, commands);
    // A plain prim tessellates immediately; a mesh or sculpt requests its asset and
    // builds its geometry now if already decoded, else on decode; an avatar grows
    // its placeholder in a later phase.
    let (face_entities, pending, prim_rebuild, tree_rebuild) = build_object_geometry(
        object,
        category,
        geometry,
        commands,
        meshes,
        materials,
        manager,
        prim_textures,
        mesh_manager,
    );
    state.objects.insert(
        scoped,
        TrackedObject {
            entity,
            geometry,
            shape,
            parent,
            is_root,
            parented,
            attachment_point,
            face_entities,
            pending,
            mesh_rebuild: None,
            // A plain prim is first tessellated at the coarse placeholder level
            // (P21.3); a non-prim keeps `prim_rebuild` None and stays at FINEST.
            prim_rebuild,
            prim_lod: INITIAL_MANAGED_PRIM_LOD,
            // A tree is first generated at the placeholder tier (P26.2); a non-tree
            // keeps `tree_rebuild` None.
            tree_rebuild,
            tree_tier: INITIAL_TREE_TIER,
            animated: is_animated_object(object),
        },
    );
    debug!(
        "spawned object {scoped} ({category:?}); {} tracked",
        state.objects.len()
    );
    if is_root {
        adopt_pending_children(state, scoped, entity, commands);
    }
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
            commands
                .entity(existing.entity)
                .insert(ChildOf(root_entity));
            existing.parented = true;
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
/// stored local offset rather than sitting at a fixed world offset.
///
/// Attachments arrive in the same object stream as everything else but hang off a
/// **pcode-47 avatar** (not a prim linkset), so [`apply_object`] holds them
/// parentless and this system — running after the avatars (and their skeleton
/// instances) are spawned — resolves each one's target from the avatar's rigged
/// body: its raw attachment-point id maps to that avatar's attachment-point node
/// entity ([`AvatarState::attachment_point_entity`]), a child of the skeleton
/// joint carrying the fixed `avatar_lad.xml` offset (P16.2), onto which the
/// object's own local transform composes. An attachment whose avatar / point node
/// is not present yet (a HUD point, a sphere-only avatar, or the avatar simply not
/// spawned yet) stays pending and is retried on a later frame.
///
/// When no `--viewer-assets` avatar body is loaded the avatars are placeholder
/// spheres with no skeleton, so an attachment instead falls back to the avatar's
/// own object entity (its previous, position-only parent) so it at least tracks
/// the avatar's location.
pub(crate) fn adopt_pending_attachments(
    mut state: ResMut<ObjectState>,
    avatars: Res<AvatarState>,
    body: Option<Res<AvatarBody>>,
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

/// Despawn the entity for `scoped` and every tracked descendant, dropping them
/// all from the map. Bevy's hierarchy despawns the entity's parented children
/// with it; any tracked-but-not-yet-parented descendants are despawned
/// explicitly so a lingering child update can never touch a dead entity.
fn remove_object(state: &mut ObjectState, scoped: ScopedObjectId, commands: &mut Commands) {
    let Some(removed) = state.objects.remove(&scoped) else {
        return;
    };
    // Bevy despawns the parented sub-hierarchy together with the root entity.
    commands.entity(removed.entity).despawn();
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
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system reading decoded meshes and the ECS resources the geometry build needs"
)]
pub(crate) fn apply_object_meshes(
    mut decoded: MessageReader<MeshDecoded>,
    mut state: ResMut<ObjectState>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut manager: ResMut<TextureManager>,
    mut prim_textures: ResMut<PrimTextures>,
    mesh_manager: Res<MeshManager>,
) {
    for &MeshDecoded(key) in decoded.read() {
        let Some(mesh) = mesh_manager.decoded(key).map(Arc::clone) else {
            // The fetch failed: objects pending on this key stay geometry-less.
            continue;
        };
        // A worn rigged mesh (a mesh carrying a skin block, on an avatar) is not
        // built as a static child here — it defers to [`apply_rigged_attachments`],
        // which binds it to the wearer's skeleton once that avatar is spawned.
        let is_rigged = mesh_manager.skin(key).is_some();
        for tracked in state.objects.values_mut() {
            // First build: an object pending on this mesh key. A build pending on a
            // *different* asset (another mesh, or a sculpt) is left untouched.
            if matches!(&tracked.pending, Some(PendingGeometry::Mesh(pending)) if pending.key == key)
            {
                let Some(PendingGeometry::Mesh(pending)) = tracked.pending.take() else {
                    continue;
                };
                if is_rigged && tracked.attachment_point.is_some() {
                    // Defer the skinned build to `apply_rigged_attachments`.
                    tracked.pending = Some(PendingGeometry::RiggedMesh(PendingRiggedMesh {
                        key,
                        texture_entry: pending.texture_entry,
                    }));
                } else {
                    tracked.face_entities = build_mesh_submeshes(
                        &mesh,
                        &pending.texture_entry,
                        pending.scale,
                        tracked.geometry,
                        &mut commands,
                        &mut meshes,
                        &mut materials,
                        &mut manager,
                        &mut prim_textures,
                        pending.priority,
                    );
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
                let geometry = tracked.geometry;
                despawn_prim_faces(&tracked.face_entities, &mut commands);
                tracked.face_entities = build_mesh_submeshes(
                    &mesh,
                    &texture_entry,
                    scale,
                    geometry,
                    &mut commands,
                    &mut meshes,
                    &mut materials,
                    &mut manager,
                    &mut prim_textures,
                    priority,
                );
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
/// fetch: prim geometry is tessellated on the CPU here and now. A target for a
/// non-prim, an untracked (removed) object, or a prim already at the desired
/// level is a no-op — `prim_rebuild` is `Some` only for a plain prim.
pub(crate) fn apply_prim_lod(
    mut targets: ResMut<PrimLodTargets>,
    mut state: ResMut<ObjectState>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut manager: ResMut<TextureManager>,
    mut prim_textures: ResMut<PrimTextures>,
) {
    for (scoped, desired) in targets.0.drain() {
        let Some(tracked) = state.objects.get_mut(&scoped) else {
            continue;
        };
        // Only a plain prim carries re-tessellation inputs; a sculpt / mesh /
        // avatar has none and is left untouched.
        let Some(rebuild) = tracked.prim_rebuild.as_ref() else {
            continue;
        };
        if tracked.prim_lod == desired {
            continue;
        }
        // Clone the rebuild inputs out so the immutable borrow of `tracked` ends
        // before the mutable rebuild of its face entities below.
        let shape = PrimShapeFloat::from_params(&rebuild.shape);
        let texture_entry = rebuild.texture_entry.clone();
        let scale = rebuild.scale;
        let priority = rebuild.priority;
        let geometry = tracked.geometry;
        let prim = tessellate(&shape, desired);
        despawn_prim_faces(&tracked.face_entities, &mut commands);
        tracked.face_entities = spawn_prim_faces(
            &prim,
            &texture_entry,
            scale,
            geometry,
            &mut commands,
            &mut meshes,
            &mut materials,
            &mut manager,
            &mut prim_textures,
            priority,
        );
        tracked.prim_lod = desired;
        debug!(
            "re-tessellated prim {scoped} at {desired:?}: {} faces",
            tracked.face_entities.len()
        );
    }
}

/// Regenerate each tree the render-priority driver picked a new [`TreeTier`] for
/// (P26.2) — the tree counterpart of [`apply_prim_lod`]. Drains
/// [`TreeLodTargets`] and, for any tree whose desired tier differs from its
/// current one, despawns its face and regenerates the branch / leaf geometry (or
/// the billboard imposter) at the new tier.
pub(crate) fn apply_tree_lod(
    mut targets: ResMut<TreeLodTargets>,
    mut state: ResMut<ObjectState>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut manager: ResMut<TextureManager>,
    mut prim_textures: ResMut<PrimTextures>,
) {
    for (scoped, desired) in targets.0.drain() {
        let Some(tracked) = state.objects.get_mut(&scoped) else {
            continue;
        };
        // Only a tree carries regeneration inputs; anything else is left untouched.
        let Some(rebuild) = tracked.tree_rebuild.as_ref() else {
            continue;
        };
        if tracked.tree_tier == desired {
            continue;
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
    }
}

/// Whether worn rigged meshes' joint position overrides (R1) are applied to the
/// avatar skeleton. On by default; `SL_VIEWER_JOINT_OVERRIDES=0` disables it, so the
/// pre-override skeleton behaviour can be compared side by side in one session.
fn joint_overrides_enabled() -> bool {
    std::env::var("SL_VIEWER_JOINT_OVERRIDES").as_deref() != Ok("0")
}

/// Whether the object `scoped` belongs to an **animated object** (animesh) linkset:
/// walk its parent chain (the animated flag sits on the linkset root) up to the
/// avatar. An animesh drives its own control-avatar skeleton, so its rig joint
/// positions must not override the wearer's skeleton (R1) — the reference viewer's
/// `!vo->isAnimatedObject()` filter.
fn belongs_to_animesh(state: &ObjectState, scoped: ScopedObjectId) -> bool {
    let mut current = scoped;
    for _ in 0..MAX_LINKSET_DEPTH {
        let Some(tracked) = state.objects.get(&current) else {
            return false;
        };
        if tracked.animated {
            return true;
        }
        // A root's `parent` is its own scoped id; stop before looping forever.
        if tracked.parent == current {
            return false;
        }
        current = tracked.parent;
    }
    false
}

/// A guard on the linkset-chain walk in [`belongs_to_animesh`], against a malformed
/// parent cycle.
const MAX_LINKSET_DEPTH: usize = 32;

/// Bind every worn rigged mesh attachment whose skeleton instance is now
/// available (P17.2): for each object holding a [`PendingGeometry::RiggedMesh`],
/// resolve the wearer avatar's skeleton-instance joint entities and spawn the
/// mesh's skinned submeshes bound to them, so the mesh deforms with the avatar
/// rather than sitting rigidly at an attachment point.
///
/// A rigged mesh's build is deferred here (rather than in [`apply_object_meshes`])
/// because it needs the wearer's spawned skeleton — which can arrive before or
/// after the mesh decodes. The pending build is retried each frame until the
/// avatar's rigged body ([`AvatarState::joint_entities_of`]) is present; an avatar
/// with no rigged body (a sphere-only, no-`--viewer-assets` run) never resolves,
/// so the mesh simply stays unbuilt there. Each rig joint name is mapped to the
/// avatar's matching skeleton joint entity ([`AvatarBody::joint_index`]), falling
/// back to the pelvis for a name the skeleton lacks (the reference viewer's
/// unknown-joint fallback). The object is marked parented so
/// [`adopt_pending_attachments`] does not also pin it to a rigid
/// attachment-point node.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system joining the object, avatar, and mesh state with the ECS resources the skinned build needs"
)]
pub(crate) fn apply_rigged_attachments(
    mut state: ResMut<ObjectState>,
    mut avatars: ResMut<AvatarState>,
    body: Option<Res<AvatarBody>>,
    mesh_manager: Res<MeshManager>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut bindposes: ResMut<Assets<SkinnedMeshInverseBindposes>>,
    mut manager: ResMut<TextureManager>,
    mut prim_textures: ResMut<PrimTextures>,
) {
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
        let Some(tracked) = state.objects.get(&scoped) else {
            continue;
        };
        let Some(PendingGeometry::RiggedMesh(build)) = &tracked.pending else {
            continue;
        };
        let key = build.key;
        // The wearer avatar, found by chasing this mesh's parent links up to the
        // avatar root — a mesh body is worn as a multi-prim linkset whose parts
        // parent to the linkset root prim, not the avatar directly, so its direct
        // `parent` is not the avatar (P17.2 fix; verified live on a real mesh body).
        let Some(agent) = avatars.wearer_of(scoped) else {
            continue;
        };
        // The wearer's rigged body and its skeleton-instance joints; retry next
        // frame if the avatar (or its body) is not spawned yet. The joint entities
        // are cloned so the immutable `avatars` borrow ends before the override
        // record below borrows it mutably.
        let (Some(root), Some(joints)) = (
            avatars.body_root_of(agent),
            avatars.joint_entities_of(agent).cloned(),
        ) else {
            continue;
        };
        let Some(fallback) = joints.first().copied() else {
            continue;
        };
        // The decoded geometry + skin, cloned out so the immutable `mesh_manager`
        // borrow ends before the build borrows the other resources mutably.
        let (Some(decoded), Some(skin)) = (
            mesh_manager.decoded(key).map(Arc::clone),
            mesh_manager.skin(key).map(Arc::clone),
        ) else {
            continue;
        };
        // Resolve the rig's own joint-name table against the avatar's skeleton
        // instance (unknown names fall back to the pelvis joint).
        // Map each rig joint name to the avatar's skeleton-instance joint entity;
        // an unresolved name (a bone or collision volume the skeleton lacks) falls
        // back to the pelvis, which would misplace those vertices, so it is logged.
        let mut unresolved: Vec<&str> = Vec::new();
        let joint_entities: Vec<Entity> = skin
            .joint_names
            .iter()
            .map(|name| {
                body.joint_index(name)
                    .and_then(|index| joints.get(index).copied())
                    .unwrap_or_else(|| {
                        unresolved.push(name.as_str());
                        fallback
                    })
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
        // The wearer's agent id (resolved above) keys its baked textures (P17.3): a
        // bake-on-mesh face is textured from the wearer's own bake, not a fetch.
        let face_entities = build_rigged_submeshes(
            &decoded,
            &skin,
            &joint_entities,
            &texture_entry,
            root,
            Some(agent),
            body.skin_material(),
            &mut commands,
            &mut meshes,
            &mut materials,
            &mut bindposes,
            &mut manager,
            &mut prim_textures,
        );
        if let Some(tracked) = state.objects.get_mut(&scoped) {
            tracked.face_entities = face_entities;
            tracked.pending = None;
            // The skinned mesh follows the skeleton joints directly, so the object
            // must not also be pinned to a rigid attachment-point node.
            tracked.parented = true;
        }
        // Fold the rig's joint position overrides into the wearer's skeleton (R1):
        // a fitted mesh body/head repositions the joints its inverse-bind matrices
        // were baked against, so without these the mesh distorts at the extremities.
        // Recording flags the avatar for a skeleton re-deform (the reference
        // viewer's `addAttachmentOverridesForObject`). Skipped for an animesh (its
        // overrides drive its own control avatar, not the wearer) and when disabled
        // via `SL_VIEWER_JOINT_OVERRIDES=0` (A/B against the pre-override behaviour).
        let overrides = if joint_overrides_enabled() && !belongs_to_animesh(&state, scoped) {
            body.joint_overrides(&skin)
        } else {
            JointOverrides::default()
        };
        if !overrides.is_empty() {
            debug!(
                "rigged mesh {key} on avatar {agent}: {} joint position override(s), \
                 lock_scale={}",
                overrides.len(),
                overrides.lock_scale()
            );
        }
        avatars.record_joint_overrides(agent, key.uuid(), overrides);
        debug!("bound rigged mesh {key} on avatar {agent} to its skeleton");
    }
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
#[expect(
    clippy::too_many_arguments,
    reason = "threads the several ECS resources the skinned build needs"
)]
fn build_rigged_submeshes(
    decoded: &DecodedMesh,
    skin: &MeshSkin,
    joint_entities: &[Entity],
    texture_entry: &[u8],
    root: Entity,
    agent: Option<AgentKey>,
    skin_placeholder: &Handle<StandardMaterial>,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    bindposes: &mut Assets<SkinnedMeshInverseBindposes>,
    manager: &mut TextureManager,
    prim_textures: &mut PrimTextures,
) -> Vec<Entity> {
    let entry = decode_texture_entry(texture_entry, decoded.submeshes.len());
    // The slot every face falls back to when the object carries no texture entry.
    let default_face = TextureFace::new(TextureKey::from(Uuid::nil()));
    let inverse_bindposes = bindposes.add(SkinnedMeshInverseBindposes::from(
        rigged_inverse_bindposes(skin),
    ));
    let mut face_entities = Vec::new();
    for (index, submesh) in decoded.submeshes.iter().enumerate() {
        if submesh.no_geometry {
            continue;
        }
        let mesh = meshes.add(to_bevy_rigged_mesh(submesh));
        let texture_face = entry.face(index).unwrap_or(&default_face);
        // A bake-on-mesh face (P17.3): its texture id is an `IMG_USE_BAKED_*`
        // sentinel meaning "show the wearer's own baked skin here". Spawn it with an
        // opaque skin placeholder and tag it [`BomFace`] so `apply_bom_face_materials`
        // points it at the wearer's baked region material (never fetch the sentinel,
        // which is not a real texture — the P17.2 invisible-shell finding). Only
        // when the wearer's agent is known; otherwise fall through to a plain fetch.
        let bom = agent.and_then(|agent| {
            avatar_texture::use_baked_slot(texture_face.texture_id)
                .map(|slot| BomFace::new(agent, slot))
        });
        let material = match bom {
            Some(_) => skin_placeholder.clone(),
            // A rigged mesh is always a worn attachment, so its face textures are
            // boosted (P20.2) — its skinned entity transform does not reflect its
            // on-screen size, so the pixel-area pass cannot rank it.
            None => face_material(
                texture_face,
                materials,
                manager,
                prim_textures,
                AVATAR_BOOST_PRIORITY,
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
            // A skinned mesh's frustum bounds are its static bind-pose AABB placed
            // at the mesh *entity's* transform, while the vertices actually render
            // wherever the joint matrices put them — so the bounds need not match
            // the drawn mesh even at rest, and a close camera can wrongly cull the
            // whole worn body. Never frustum-cull it.
            NoFrustumCulling,
            Transform::default(),
            Visibility::default(),
            PrimFaceEntity { face_id },
            ChildOf(root),
        ));
        if let Some(bom) = bom {
            spawned.insert(bom);
        }
        face_entities.push(spawned.id());
    }
    face_entities
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
pub(crate) fn apply_object_sculpts(
    mut decoded: MessageReader<TextureDecoded>,
    mut state: ResMut<ObjectState>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut manager: ResMut<TextureManager>,
    mut prim_textures: ResMut<PrimTextures>,
) {
    for &TextureDecoded(id) in decoded.read() {
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
                    );
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
        ObjectCategory, ShapeFingerprint, classify, geometry_transform, holder_transform,
        object_transform,
    };
    use bevy::math::Vec3;
    use pretty_assertions::{assert_eq, assert_ne};
    use sl_client_bevy::{
        CircuitId, MeshKey, Object, ObjectMotion, RegionHandle, RegionLocalObjectId, Rotation,
        SculptData, SculptOrMeshKey, TextureKey, Uuid, Vector, pcode,
    };

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
        let transform = object_transform(&object, true);
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

    /// A child object's local transform stays in pure Second Life space (no axis
    /// swap), since the parent entity carries the basis change.
    #[test]
    fn child_transform_stays_in_sl_space() {
        let object = bare_object(pcode::PRIMITIVE);
        let transform = object_transform(&object, false);
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
}
