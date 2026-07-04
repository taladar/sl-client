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
//! (and the object's scale / rotation / position). Until Phase 6 textures each
//! face, every face uses a shared neutral placeholder material. Mesh objects
//! (P7), sculpts (P9), and avatar placeholders (P10) attach their geometry to
//! these entities in the same way.

use std::collections::HashMap;

use bevy::prelude::*;
use sl_client_bevy::{
    Object, PrimFaceId, PrimLod, PrimShapeFloat, PrimShapeParams, ScopedObjectId, SculptOrMeshKey,
    SlEvent, SlSessionEvent, Vector, pcode, tessellate, to_bevy_prim_mesh,
};

use crate::coords::{sl_rotation_to_quat, sl_to_bevy_object_rotation, sl_to_bevy_vec};

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
/// P5.2 spawns the marker and renders the face with a shared placeholder
/// material; the per-face diffuse texturing pass (Phase 6) reads the `face_id` to
/// pair each face with its own material.
#[derive(Component, Debug, Clone, Copy)]
#[expect(
    dead_code,
    reason = "face_id read by the Phase 6 per-face diffuse texturing pass"
)]
pub(crate) struct PrimFaceEntity {
    /// The Linden semantic face index this face is textured from.
    pub(crate) face_id: PrimFaceId,
}

/// Shared prim-rendering materials. Until per-face diffuse texturing lands
/// (Phase 6), every tessellated prim face uses one neutral placeholder material.
#[derive(Resource)]
pub(crate) struct PrimMaterials {
    /// The neutral placeholder material every prim face renders with until Phase
    /// 6 textures it. It is double-sided (culling off) so a face renders
    /// regardless of its winding while the geometry is being brought up.
    placeholder: Handle<StandardMaterial>,
}

impl FromWorld for PrimMaterials {
    /// Create the shared placeholder material in the world's
    /// `Assets<StandardMaterial>` (registered by Bevy's PBR plugin before this
    /// resource is initialised).
    fn from_world(world: &mut World) -> Self {
        let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
        let placeholder = materials.add(StandardMaterial {
            base_color: Color::srgb(0.7, 0.7, 0.72),
            perceptual_roughness: 0.9,
            double_sided: true,
            cull_mode: None,
            ..default()
        });
        Self { placeholder }
    }
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
    /// child whose root has not arrived stays `false` until it does).
    parented: bool,
    /// The per-face child entities carrying this prim's tessellated geometry (one
    /// per non-empty [`PrimFace`](sl_client_bevy::PrimFace)), rebuilt on a shape
    /// change. Empty for a non-prim object or one not yet tessellated.
    face_entities: Vec<Entity>,
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
pub(crate) fn update_objects(
    mut events: MessageReader<SlEvent>,
    mut state: ResMut<ObjectState>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    prim_materials: Res<PrimMaterials>,
) {
    for event in events.read() {
        match &event.0 {
            SlSessionEvent::ObjectAdded(object) | SlSessionEvent::ObjectUpdated(object) => {
                apply_object(
                    &mut state,
                    object,
                    &mut commands,
                    &mut meshes,
                    &prim_materials,
                );
            }
            SlSessionEvent::ObjectRemoved { local_id, .. } => {
                remove_object(&mut state, *local_id, &mut commands);
            }
            _other => {}
        }
    }
}

/// Tessellate a plain prim at a fixed high level of detail and spawn one child
/// entity per non-empty [`PrimFace`](sl_client_bevy::PrimFace) under `parent`,
/// each carrying its geometry mesh, the shared placeholder material, and a
/// [`PrimFaceEntity`] tag naming its Linden face index. Returns the spawned face
/// entities so a later shape change can despawn and rebuild them.
///
/// The face geometry stays in the prim's local Second Life space; the object
/// entity's `Transform` carries the object's scale / rotation / position and the
/// single Second Life → Bevy basis change for the whole prim.
fn build_prim_faces(
    object: &Object,
    parent: Entity,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &PrimMaterials,
) -> Vec<Entity> {
    let shape = PrimShapeFloat::from_params(&object.shape);
    let prim = tessellate(&shape, PrimLod::High);
    let mut face_entities = Vec::new();
    for face in &prim.faces {
        if face.is_empty() {
            continue;
        }
        let mesh = meshes.add(to_bevy_prim_mesh(face));
        let entity = commands
            .spawn((
                Mesh3d(mesh),
                MeshMaterial3d(materials.placeholder.clone()),
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

/// Despawn every face child entity of a prim (used before rebuilding on a shape
/// change), leaving the caller to clear the tracked list.
fn despawn_prim_faces(face_entities: &[Entity], commands: &mut Commands) {
    for &face in face_entities {
        commands.entity(face).try_despawn();
    }
}

/// Spawn or update the entity for `object`, keeping its transform, classification,
/// and linkset parenting current.
fn apply_object(
    state: &mut ObjectState,
    object: &Object,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &PrimMaterials,
) {
    let scoped = object.scoped_id();
    let parent = object.scoped_parent_id();
    let is_root = object.parent_id.get() == 0;
    let category = classify(object);
    let shape = ShapeFingerprint::of(object);
    let transform = object_transform(object, is_root);
    // The parent's entity, if its root is already tracked (looked up before the
    // mutable borrow of the object's own entry below).
    let parent_entity = if is_root {
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
            existing.face_entities = if category == ObjectCategory::Prim {
                build_prim_faces(object, existing.entity, commands, meshes, materials)
            } else {
                Vec::new()
            };
            existing.shape = shape;
        }
        // Reconcile parenting: an object relinked to a root becomes a child of
        // it; an unlinked one (now a root) drops its parent. A child whose new
        // root is not tracked yet is left parentless until it arrives.
        reconcile_parent(existing, is_root, parent_entity, commands);
        existing.parent = parent;
        existing.is_root = is_root;
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
    // A plain prim tessellates immediately; the other categories (mesh / sculpt /
    // avatar) grow their geometry in later phases.
    let face_entities = if category == ObjectCategory::Prim {
        build_prim_faces(object, entity, commands, meshes, materials)
    } else {
        Vec::new()
    };
    state.objects.insert(
        scoped,
        TrackedObject {
            entity,
            shape,
            parent,
            is_root,
            parented,
            face_entities,
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
        if !child.parented && !child.is_root && child.parent == scoped {
            commands.entity(child.entity).insert(ChildOf(root_entity));
            child.parented = true;
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
    // Drop tracked descendants; despawn any that were still waiting to be
    // parented (Bevy did not despawn those with the root).
    for descendant in tracked_descendants(state, scoped) {
        if let Some(entry) = state.objects.remove(&descendant)
            && !entry.parented
        {
            commands.entity(entry.entity).try_despawn();
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
