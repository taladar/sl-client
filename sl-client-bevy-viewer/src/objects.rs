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

use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
use bevy::prelude::*;
use sl_client_bevy::{
    AgentKey, DecodedMesh, DecodedTexture, MeshKey, MeshSkin, Object, PrimFaceId, PrimLod,
    PrimMesh, PrimShapeFloat, PrimShapeParams, ScopedObjectId, SculptOrMeshKey, SlEvent,
    SlSessionEvent, TextureFace, TextureKey, Uuid, Vector, avatar_texture, decode_texture_entry,
    pcode, rigged_inverse_bindposes, tessellate, tessellate_sculpt, to_bevy_mesh,
    to_bevy_prim_mesh, to_bevy_rigged_mesh,
};

use crate::avatars::{AvatarBody, AvatarState, BomFace};
use crate::coords::{sl_rotation_to_quat, sl_to_bevy_object_rotation, sl_to_bevy_vec};
use crate::meshes::{MeshDecoded, MeshManager};
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
    /// Anything else (tree, grass, particle-system object, …); not rendered by
    /// the current phases.
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
        }
    }
}

/// A marker component tagging an entity as an in-world object, carrying its
/// scoped id and render classification for the later rendering phases to query.
///
/// P5.1 only spawns the marker; the fields are read by the rendering phases
/// (P5.2 prims / P7 mesh / P9 sculpt / P10 avatars) that attach geometry keyed
/// off the classification.
#[derive(Component, Debug, Clone, Copy)]
#[expect(
    dead_code,
    reason = "read by the later rendering phases that attach geometry to these entities"
)]
pub(crate) struct SceneObject {
    /// The object's scoped (circuit + region-local) id.
    pub(crate) scoped_id: ScopedObjectId,
    /// The object's render classification.
    pub(crate) category: ObjectCategory,
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
#[expect(
    dead_code,
    reason = "face_id retained for later per-face addressing (material overrides, picking)"
)]
pub(crate) struct PrimFaceEntity {
    /// The Linden semantic face index this face is textured from.
    pub(crate) face_id: PrimFaceId,
}

/// Per-object viewer-side bookkeeping, paired with the object's [`SceneObject`]
/// entity.
struct TrackedObject {
    /// The entity rendering this object.
    entity: Entity,
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
}

/// Viewer-side object bookkeeping: the entity and metadata for every in-world
/// object currently in the scene, keyed by scoped id.
#[derive(Resource, Default)]
pub(crate) struct ObjectState {
    /// Every tracked object, keyed by its scoped id.
    objects: HashMap<ScopedObjectId, TrackedObject>,
}

/// Classify an object from its `pcode` and sculpt/mesh extra parameters.
fn classify(object: &Object) -> ObjectCategory {
    match object.pcode {
        pcode::AVATAR => ObjectCategory::Avatar,
        pcode::PRIMITIVE => match object.extra.sculpt.map(|sculpt| sculpt.texture) {
            Some(SculptOrMeshKey::Mesh(_)) => ObjectCategory::Mesh,
            Some(SculptOrMeshKey::Sculpt(_)) => ObjectCategory::Sculpt,
            None => ObjectCategory::Prim,
        },
        _other => ObjectCategory::Other,
    }
}

/// The Bevy `Transform` for an object.
///
/// A **root** object (no parent) gets a world transform: its region-local
/// position and orientation carried into Bevy's Y-up world by the Second Life →
/// Bevy [basis change](crate::coords). A **child** (linkset member / attachment)
/// gets a *local* transform in pure Second Life space — its position and
/// rotation are already relative to its parent, whose entity carries the single
/// basis change for the whole subtree. In both cases the scale is applied in the
/// object's own local frame (before the rotation), so no axis swap is needed.
fn object_transform(object: &Object, is_root: bool) -> Transform {
    let scale = Vec3::new(object.scale.x, object.scale.y, object.scale.z);
    if is_root {
        Transform {
            translation: sl_to_bevy_vec(&object.motion.position),
            rotation: sl_to_bevy_object_rotation(&object.motion.rotation),
            scale,
        }
    } else {
        Transform {
            translation: local_translation(&object.motion.position),
            rotation: sl_rotation_to_quat(&object.motion.rotation),
            scale,
        }
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
/// Every other category renders nothing here.
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
) -> (Vec<Entity>, Option<PendingGeometry>) {
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
            ),
            None,
        ),
        ObjectCategory::Mesh => {
            let Some(key) = mesh_key(object) else {
                return (Vec::new(), None);
            };
            mesh_manager.request(key);
            // The store hands back an `Arc`; clone it out so the immutable borrow
            // of `mesh_manager` ends before the submesh build borrows the other
            // resources.
            match mesh_manager.decoded(key).map(Arc::clone) {
                Some(decoded) => (
                    build_mesh_submeshes(
                        &decoded,
                        &object.texture_entry,
                        entity,
                        commands,
                        meshes,
                        materials,
                        manager,
                        prim_textures,
                    ),
                    None,
                ),
                None => (
                    Vec::new(),
                    Some(PendingGeometry::Mesh(PendingMesh {
                        key,
                        texture_entry: object.texture_entry.clone(),
                    })),
                ),
            }
        }
        ObjectCategory::Sculpt => {
            let Some((map, sculpt_type)) = sculpt_key(object) else {
                return (Vec::new(), None);
            };
            manager.request(map);
            // The store hands back an `Arc`; clone it out so the immutable borrow
            // of `manager` ends before the face build borrows it mutably.
            match manager.decoded(map).map(Arc::clone) {
                Some(map_image) => (
                    build_sculpt_faces(
                        &map_image,
                        sculpt_type,
                        &object.texture_entry,
                        entity,
                        commands,
                        meshes,
                        materials,
                        manager,
                        prim_textures,
                    ),
                    None,
                ),
                None => (
                    Vec::new(),
                    Some(PendingGeometry::Sculpt(PendingSculpt {
                        map,
                        sculpt_type,
                        texture_entry: object.texture_entry.clone(),
                    })),
                ),
            }
        }
        ObjectCategory::Avatar | ObjectCategory::Other => (Vec::new(), None),
    }
}

/// Tessellate a plain prim at a fixed high level of detail and spawn one child
/// entity per non-empty [`PrimFace`](sl_client_bevy::PrimFace) under `parent`,
/// each carrying its geometry mesh, its per-face diffuse material (from the
/// object's decoded [`TextureEntry`](sl_client_bevy::TextureEntry)), and a
/// [`PrimFaceEntity`] tag naming its Linden face index. Returns the spawned face
/// entities so a later shape change can despawn and rebuild them.
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
fn build_prim_faces(
    object: &Object,
    parent: Entity,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    manager: &mut TextureManager,
    prim_textures: &mut PrimTextures,
) -> Vec<Entity> {
    let shape = PrimShapeFloat::from_params(&object.shape);
    let prim = tessellate(&shape, PrimLod::High);
    spawn_prim_faces(
        &prim,
        &object.texture_entry,
        parent,
        commands,
        meshes,
        materials,
        manager,
        prim_textures,
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
    parent: Entity,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    manager: &mut TextureManager,
    prim_textures: &mut PrimTextures,
) -> Vec<Entity> {
    let prim = tessellate_sculpt(map, sculpt_type);
    spawn_prim_faces(
        &prim,
        texture_entry,
        parent,
        commands,
        meshes,
        materials,
        manager,
        prim_textures,
    )
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
    parent: Entity,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    manager: &mut TextureManager,
    prim_textures: &mut PrimTextures,
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
        let mesh = meshes.add(to_bevy_prim_mesh(face));
        let texture_face = entry.face(face.face_id.as_usize()).unwrap_or(&default_face);
        let material = face_material(texture_face, materials, manager, prim_textures);
        let entity = commands
            .spawn((
                Mesh3d(mesh),
                MeshMaterial3d(material),
                PrimFaceEntity {
                    face_id: face.face_id,
                },
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
    parent: Entity,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    manager: &mut TextureManager,
    prim_textures: &mut PrimTextures,
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
        let mesh = meshes.add(to_bevy_mesh(submesh));
        let texture_face = entry.face(index).unwrap_or(&default_face);
        let material = face_material(texture_face, materials, manager, prim_textures);
        // The submesh index is the Linden face index; a mesh has few faces, so the
        // widening never saturates in practice (a clamp keeps it lint-clean).
        let face_id = PrimFaceId::new(u16::try_from(index).unwrap_or(u16::MAX));
        let entity = commands
            .spawn((
                Mesh3d(mesh),
                MeshMaterial3d(material),
                PrimFaceEntity { face_id },
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

    if let Some(existing) = state.objects.get_mut(&scoped) {
        // A known object: re-place it and refresh its classification (a
        // motion-only update stops here — the geometry is untouched).
        commands.entity(existing.entity).insert((
            transform,
            SceneObject {
                scoped_id: scoped,
                category,
            },
        ));
        if existing.shape != shape {
            // A genuine shape (or category) change: drop the old face meshes and
            // re-tessellate. A category change is subsumed here, since the
            // fingerprint covers pcode and the sculpt/mesh key.
            debug!("object {scoped} shape changed; re-tessellating");
            despawn_prim_faces(&existing.face_entities, commands);
            let (face_entities, pending) = build_object_geometry(
                object,
                category,
                existing.entity,
                commands,
                meshes,
                materials,
                manager,
                prim_textures,
                mesh_manager,
            );
            existing.face_entities = face_entities;
            existing.pending = pending;
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
    // A plain prim tessellates immediately; a mesh or sculpt requests its asset and
    // builds its geometry now if already decoded, else on decode; an avatar grows
    // its placeholder in a later phase.
    let (face_entities, pending) = build_object_geometry(
        object,
        category,
        entity,
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
            shape,
            parent,
            is_root,
            parented,
            attachment_point,
            face_entities,
            pending,
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
            // Take the pending build so a built object is not rebuilt; a build
            // pending on a *different* asset (another mesh, or a sculpt) is put
            // back untouched.
            match tracked.pending.take() {
                Some(PendingGeometry::Mesh(pending)) if pending.key == key => {
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
                            tracked.entity,
                            &mut commands,
                            &mut meshes,
                            &mut materials,
                            &mut manager,
                            &mut prim_textures,
                        );
                        debug!(
                            "built mesh {key}: {} submesh entities",
                            tracked.face_entities.len()
                        );
                    }
                }
                other => tracked.pending = other,
            }
        }
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
    avatars: Res<AvatarState>,
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
        // frame if the avatar (or its body) is not spawned yet.
        let (Some(root), Some(joints)) = (
            avatars.body_root_of(agent),
            avatars.joint_entities_of(agent),
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
            None => face_material(texture_face, materials, manager, prim_textures),
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
                        tracked.entity,
                        &mut commands,
                        &mut meshes,
                        &mut materials,
                        &mut manager,
                        &mut prim_textures,
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
    use super::{ObjectCategory, ShapeFingerprint, classify, object_transform};
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

    /// A tree object is neither prim nor avatar — it classifies as
    /// [`ObjectCategory::Other`].
    #[test]
    fn tree_classifies_as_other() {
        assert_eq!(classify(&bare_object(pcode::TREE)), ObjectCategory::Other);
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
        assert!(
            transform
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
