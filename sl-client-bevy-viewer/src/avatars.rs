//! Avatar placeholders: a ~2 m sphere and a floating name tag per nearby avatar.
//!
//! This is the Phase 10 slice — placeholder spheres, no rig / baked textures /
//! animation. Avatars are learned from two independent streams:
//!
//! - **full in-world objects** (`pcode` 47): the precise, per-frame position of
//!   every avatar the simulator streams as an [`Object`]
//!   (the reliable source for avatars within draw distance, including the agent's
//!   own). [`update_avatar_objects`] spawns / moves / despawns one sphere per such
//!   avatar keyed by its agent id;
//! - **coarse (minimap) locations** (`CoarseLocationUpdate`): the low-resolution
//!   (1 m) positions the simulator pushes for nearby avatars, some of which are
//!   beyond the object interest radius and so never arrive as a full object.
//!   [`update_coarse_avatars`] renders a sphere for every coarse-only avatar (one
//!   already tracked as a full object is skipped, and the agent's own `you` entry
//!   is left to the object path), and despawns a sphere the moment its avatar
//!   drops out of the coarse list.
//!
//! Each avatar also carries a floating **name tag** — a `bevy_ui` text node
//! positioned each frame over the sphere by projecting its world position to the
//! screen ([`position_name_tags`]). The legacy name is resolved once per agent via
//! a `UUIDNameRequest` ([`Command::RequestAvatarNames`](sl_client_bevy::Command))
//! and cached in [`AvatarState`], so a repeatedly-updated avatar is never
//! re-requested; until the reply arrives the tag shows a short id fragment so the
//! avatars are still distinguishable.
//!
//! Both sources share one placeholder sphere mesh and material, built lazily on
//! first use. The spheres are plain world-space entities positioned via the
//! Second Life → Bevy [coordinate map](crate::coords) — they are markers, not the
//! avatar's object root, so (unlike a linkset root in [`objects`](crate::objects))
//! they carry no attachment children and are not scaled by the avatar object's
//! bounding box.

use std::collections::{HashMap, HashSet};

use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
use bevy::prelude::*;
use sl_client_bevy::{
    AgentKey, AvatarName, BakeRegion, BaseMesh, CoarseLocation, Command, MAX_FACES, MaskTexture,
    MorphWeights, Object, PartMorphMask, ResolvedParams, ScopedObjectId, SkeletalDeformations,
    SlCommand, SlEvent, SlIdentity, SlSessionEvent, TextureEntry, TextureKey, avatar_texture,
    composite_region, decode_texture_entry, pcode, to_bevy_base_mesh, to_bevy_image,
    to_bevy_morphed_mesh,
};

use crate::avatar_assets::{AvatarAssetLibrary, BodyRegion, LoadedBinding};
use crate::bake_inputs::OwnBakeInputs;
use crate::coords::{sl_to_bevy_object_rotation, sl_to_bevy_vec};
use crate::textures::{TextureDecoded, TextureManager};

/// The radius, in metres, of an avatar placeholder sphere (a ~2 m-diameter
/// UV-sphere, roughly avatar-sized).
const AVATAR_SPHERE_RADIUS: f32 = 1.0;

/// The number of longitudinal segments (sectors) of the placeholder UV-sphere.
const SPHERE_SECTORS: u32 = 32;

/// The number of latitudinal segments (stacks) of the placeholder UV-sphere.
const SPHERE_STACKS: u32 = 18;

/// The soft-blue base colour of the placeholder material, so avatars stand out
/// from prims and terrain.
const AVATAR_COLOR: Color = Color::srgb(0.40, 0.60, 0.90);

/// The gap, in metres, between the top of an avatar (sphere top or body head)
/// and its name tag.
const NAME_TAG_GAP: f32 = 0.3;

/// The height, in metres above the avatar object position, at which a rigged
/// body's name tag floats — roughly the head of an average-height avatar (the
/// object position sits near the pelvis).
const BODY_TAG_HEIGHT: f32 = 1.9;

/// A skin-toned base colour for the un-textured Phase-13.2 body, before the
/// baked-texture phases (P14) drape real textures over it.
const BODY_COLOR: Color = Color::srgb(0.85, 0.70, 0.62);

/// The channel count of a decoded RGBA8 texture — the pixel stride used when
/// sampling a bake's alpha for the clothing-morph masks (P14.5).
const RGBA_CHANNELS: usize = 4;

/// The name-tag font size, in logical pixels.
const NAME_TAG_FONT_SIZE: f32 = 16.0;

/// How many leading hex characters of the agent id to show as a provisional tag
/// before the real name resolves.
const PROVISIONAL_ID_CHARS: usize = 8;

/// A marker component tagging an entity as an avatar placeholder sphere.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct AvatarSphere;

/// A marker component on the transform-bearing *anchor* entity of an avatar —
/// its placeholder sphere or the root of its rigged body — whose world position
/// [`position_name_tags`] projects to place the floating name tag.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct AvatarAnchor;

/// A marker on one rigged base-part render entity, tying it back to its avatar
/// and its index in [`AvatarBody::parts`] / [`AvatarAssetLibrary::parts`] so the
/// appearance system ([`apply_avatar_appearance`]) can rebuild just that part's mesh from
/// the avatar's resolved visual-param weights.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct AvatarBodyPart {
    /// The avatar this part belongs to.
    agent: AgentKey,
    /// The part's index into the shared part list (base-mesh and render data
    /// share the same order).
    part: usize,
    /// Which baked region this part belongs to, so the visibility system
    /// ([`apply_avatar_part_visibility`]) can hide it when a worn attachment
    /// replaces its region, or (for the skirt) show it only when a skirt is worn.
    region: BodyRegion,
}

/// A marker on one skeleton-instance joint entity, tying it back to its avatar
/// and its index in the shared [`BevySkeleton`](sl_client_bevy::BevySkeleton) so
/// the appearance system ([`apply_avatar_appearance`]) can re-set that joint's
/// local transform from the avatar's resolved skeletal deformations (P13.4).
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct AvatarJoint {
    /// The avatar this joint belongs to.
    agent: AgentKey,
    /// The joint's index into the shared skeleton (joint order).
    index: usize,
}

/// A `bevy_ui` name-tag text node, pointing back at the avatar anchor it floats
/// over so [`position_name_tags`] can project the anchor's world position to the
/// screen each frame.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct NameTag {
    /// The avatar anchor entity (sphere or body root) this tag labels.
    anchor: Entity,
    /// The height, in metres above the anchor's world position, at which to
    /// float the tag (a sphere's top or a body's head).
    tag_height: f32,
}

/// The shared placeholder sphere mesh and material, built once and reused by
/// every avatar sphere.
struct AvatarAssets {
    /// The shared UV-sphere mesh handle.
    mesh: Handle<Mesh>,
    /// The shared soft-blue material handle.
    material: Handle<StandardMaterial>,
}

/// The pair of entities rendering one avatar: its world-space anchor (a
/// placeholder sphere or the root of a rigged body) and its screen-space
/// name-tag text node.
#[derive(Clone, Copy)]
struct AvatarEntities {
    /// The anchor entity — a placeholder sphere or a rigged-body root. Despawned
    /// recursively, so a body's whole joint / mesh sub-hierarchy goes with it.
    anchor: Entity,
    /// The floating name-tag UI text entity.
    label: Entity,
}

/// Viewer-side avatar bookkeeping: the placeholder entities for every nearby
/// avatar, split by which stream it came from, plus a legacy-name cache.
///
/// A full-object avatar's `ObjectRemoved` carries only its scoped local id (not
/// its agent id), so [`by_scoped`](Self::by_scoped) maps back to the agent id the
/// avatar is keyed by.
#[derive(Resource, Default)]
pub(crate) struct AvatarState {
    /// Avatars known as a full in-world object (`pcode` 47), keyed by agent id;
    /// their sphere follows the object's precise position.
    objects: HashMap<AgentKey, AvatarEntities>,
    /// Avatars known only from coarse (minimap) locations — not (currently) a full
    /// object — keyed by agent id; their sphere sits at the 1 m coarse position.
    coarse: HashMap<AgentKey, AvatarEntities>,
    /// A reverse map from an object's scoped id to its agent id, so an
    /// `ObjectRemoved` can find the avatar to despawn.
    by_scoped: HashMap<ScopedObjectId, AgentKey>,
    /// Resolved legacy names, keyed by agent id — the "simple name cache" that
    /// keeps a repeatedly-seen avatar from being re-requested.
    names: HashMap<AgentKey, String>,
    /// Agents whose legacy name has already been requested (but has not
    /// necessarily arrived), so the same `UUIDNameRequest` is not sent twice.
    requested: HashSet<AgentKey>,
    /// The latest `AvatarAppearance.visual_params` byte vector per avatar, kept so
    /// a body spawned after (or re-spawned) can be morphed from the last known
    /// appearance (P13.3).
    appearances: HashMap<AgentKey, Vec<u8>>,
    /// Avatars whose rigged body needs its appearance (re)applied — its morphs
    /// re-blended and its skeleton re-deformed — set on a fresh appearance and on
    /// a newly spawned body, drained by [`apply_avatar_appearance`].
    appearance_dirty: HashSet<AgentKey>,
    /// Whether each avatar's `TEX_SKIRT_BAKED` slot holds a visible bake, from its
    /// latest appearance — the reference viewer's skirt-worn test. Absent means
    /// not yet known, treated as no skirt (the base skirt mesh stays hidden).
    skirt_visible: HashMap<AgentKey, bool>,
    /// The visible baked-texture id in each base-body region slot per avatar,
    /// from its latest appearance (P14.1): the published baked UUIDs the viewer
    /// fetches through the shared [`TextureManager`] and (from P14.2) drapes over
    /// the system body. Keyed by baked slot ([`BODY_BAKE_SLOTS`]); a slot with no
    /// real bake is simply absent.
    baked_textures: HashMap<AgentKey, HashMap<usize, TextureKey>>,
    /// The Current Outfit Folder version whose bakes were last fetched per avatar
    /// (P14.4), so a later `AvatarAppearance` with a strictly-older `cof_version`
    /// (an out-of-order / duplicate resend) is skipped and cannot clobber a newer
    /// bake. Absent means none seen yet; an appearance with no `cof_version`
    /// (OpenSim / the older path) is always ingested.
    baked_cof_version: HashMap<AgentKey, i32>,
    /// Avatars whose body-region bake materials need (re)assigning — set on a
    /// fresh appearance and on a newly spawned body, drained by
    /// [`assign_avatar_bake_materials`] (P14.2).
    bake_dirty: HashSet<AgentKey>,
    /// The parent scoped id of every tracked non-root object (linkset children and
    /// attachments), so an attachment's chain can be chased up to its avatar root
    /// (P13.5 `IMG_USE_BAKED_*` region hide).
    object_parents: HashMap<ScopedObjectId, ScopedObjectId>,
    /// For every tracked non-root object whose texture entry carries
    /// `IMG_USE_BAKED_*` sentinels, the baked slots it replaces — aggregated up the
    /// attachment chain to hide the matching base-avatar mesh regions.
    baked_hides: HashMap<ScopedObjectId, Vec<usize>>,
    /// Non-root objects whose texture entry has already been scanned for
    /// `IMG_USE_BAKED_*` sentinels, so a motion-only update never re-decodes it.
    scanned_objects: HashSet<ScopedObjectId>,
    /// The shared placeholder sphere mesh + material, built lazily on first use.
    assets: Option<AvatarAssets>,
}

/// The maximum attachment/linkset depth chased when attributing an object's
/// `IMG_USE_BAKED_*` hide to its avatar, a guard against a malformed parent cycle.
const MAX_ATTACHMENT_DEPTH: usize = 32;

/// The shared, per-avatar-invariant render assets for the rigged base body,
/// built once from [`AvatarAssetLibrary`] and reused by every avatar body: one
/// mesh / material / inverse-bindposes set, plus the joint rest data a fresh
/// skeleton instance is spawned from.
///
/// Present as a resource only when the `--viewer-assets` directory loaded; its
/// absence is the signal to fall back to the placeholder sphere.
#[derive(Resource, Debug)]
pub(crate) struct AvatarBody {
    /// The shared skin material for the un-textured body.
    material: Handle<StandardMaterial>,
    /// One render entry per resolved base part.
    parts: Vec<BodyPart>,
    /// Each joint's local rest transform (Second Life Z-up), parallel to
    /// [`joint_parents`](Self::joint_parents); a fresh joint entity is spawned
    /// per avatar from these.
    joint_locals: Vec<Transform>,
    /// Each joint's parent index (`None` for a root), parallel to
    /// [`joint_locals`](Self::joint_locals).
    joint_parents: Vec<Option<usize>>,
    /// The pelvis rest height (Second Life Z, metres) used to plant the body
    /// vertically so its pelvis sits at the reported object position.
    pelvis_height: f32,
}

/// One base part's shared render data.
#[derive(Debug)]
struct BodyPart {
    /// The Bevy mesh, shared across avatars (identical un-morphed geometry).
    mesh: Handle<Mesh>,
    /// How the part binds to a skeleton instance's joint entities.
    binding: BodyPartBinding,
    /// Which baked region this part belongs to (for P13.5 visibility).
    region: BodyRegion,
}

/// A base part's skeleton binding, resolved to Bevy render data.
#[derive(Debug)]
enum BodyPartBinding {
    /// A skinned part: shared inverse bindposes plus the skeleton joint indices
    /// its `JOINT_INDEX` attribute maps to (in the part's own table order).
    Skinned {
        /// The shared inverse-bindposes asset, parallel to
        /// [`joint_map`](Self::Skinned::joint_map).
        inverse_bindposes: Handle<SkinnedMeshInverseBindposes>,
        /// The skeleton joint index each `JOINT_INDEX` slot refers to; mapped to
        /// this avatar's joint entities to fill `SkinnedMesh.joints`.
        joint_map: Vec<usize>,
    },
    /// A rigid (un-skinned) part parented to the skeleton joint at this index.
    Rigid(usize),
}

/// Startup system: if the avatar asset library loaded, build the shared body
/// render assets and insert them as [`AvatarBody`], so every rigged avatar reuses
/// one mesh / material / inverse-bindposes set. A no-op (leaving avatars as
/// spheres) when no `--viewer-assets` directory was given or it failed to load.
pub(crate) fn setup_avatar_body(
    library: Option<Res<AvatarAssetLibrary>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut bindposes: ResMut<Assets<SkinnedMeshInverseBindposes>>,
) {
    let Some(library) = library else {
        return;
    };
    let material = materials.add(StandardMaterial {
        base_color: BODY_COLOR,
        ..default()
    });
    let mut parts = Vec::with_capacity(library.parts().len());
    for part in library.parts() {
        let mesh = meshes.add(to_bevy_base_mesh(&part.mesh));
        let binding = match &part.binding {
            LoadedBinding::Skinned(skin) => BodyPartBinding::Skinned {
                inverse_bindposes: bindposes.add(SkinnedMeshInverseBindposes::from(
                    skin.inverse_bindposes.clone(),
                )),
                joint_map: skin.joints.clone(),
            },
            LoadedBinding::Rigid(index) => BodyPartBinding::Rigid(*index),
        };
        parts.push(BodyPart {
            mesh,
            binding,
            region: part.region,
        });
    }
    let skeleton = library.skeleton();
    let part_count = parts.len();
    commands.insert_resource(AvatarBody {
        material,
        parts,
        joint_locals: skeleton.local_transforms().to_vec(),
        joint_parents: skeleton.parents().to_vec(),
        pelvis_height: library.pelvis_height(),
    });
    info!("built rigged avatar body ({part_count} parts)");
}

/// The world [`Transform`] of a rigged avatar body root: the object's position
/// and orientation carried into Bevy's Y-up world by the Second Life → Bevy
/// basis change, lowered by the pelvis rest height so the pelvis sits at the
/// reported object position (Second Life reports an avatar near its pelvis).
fn body_root_transform(object: &Object, pelvis_height: f32) -> Transform {
    let translation = sl_to_bevy_vec(&object.motion.position);
    Transform {
        // Per-component subtract to avoid the `arithmetic_side_effects` lint on
        // the glam `Vec3` operator.
        translation: Vec3::new(translation.x, translation.y - pelvis_height, translation.z),
        rotation: sl_to_bevy_object_rotation(&object.motion.rotation),
        scale: Vec3::ONE,
    }
}

/// Spawn one base part's render entity into a skeleton instance: a `SkinnedMesh`
/// under the body root for a skinned part, or a plain mesh parented to a single
/// joint for a rigid part. A part whose joints cannot be resolved is skipped.
///
/// Each spawned entity carries an [`AvatarBodyPart`] marker (its `agent` and part
/// `index`) so [`apply_avatar_appearance`] can later swap in a morphed mesh.
fn spawn_body_part(
    part: &BodyPart,
    index: usize,
    agent: AgentKey,
    joints: &[Entity],
    root: Entity,
    material: &Handle<StandardMaterial>,
    commands: &mut Commands,
) {
    let marker = AvatarBodyPart {
        agent,
        part: index,
        region: part.region,
    };
    // The skirt is hidden until an appearance says a skirt is worn; every other
    // region shows by default, hidden only if a worn attachment replaces it. The
    // per-frame [`apply_avatar_part_visibility`] keeps this current; the initial
    // value only avoids a one-frame flash of an un-worn skirt.
    let initial = match part.region {
        BodyRegion::Skirt => Visibility::Hidden,
        _other => Visibility::Inherited,
    };
    match &part.binding {
        BodyPartBinding::Skinned {
            inverse_bindposes,
            joint_map,
        } => {
            let Some(part_joints) = joint_map
                .iter()
                .map(|&index| joints.get(index).copied())
                .collect::<Option<Vec<Entity>>>()
            else {
                return;
            };
            commands.spawn((
                Mesh3d(part.mesh.clone()),
                MeshMaterial3d(material.clone()),
                Transform::default(),
                initial,
                SkinnedMesh {
                    inverse_bindposes: inverse_bindposes.clone(),
                    joints: part_joints,
                },
                ChildOf(root),
                marker,
            ));
        }
        BodyPartBinding::Rigid(joint_index) => {
            let Some(joint) = joints.get(*joint_index).copied() else {
                return;
            };
            commands.spawn((
                Mesh3d(part.mesh.clone()),
                MeshMaterial3d(material.clone()),
                Transform::default(),
                initial,
                ChildOf(joint),
                marker,
            ));
        }
    }
}

/// The placeholder UV-sphere mesh (radius [`AVATAR_SPHERE_RADIUS`]).
fn placeholder_sphere_mesh() -> Mesh {
    Sphere::new(AVATAR_SPHERE_RADIUS)
        .mesh()
        .uv(SPHERE_SECTORS, SPHERE_STACKS)
}

/// The placeholder material (opaque soft blue).
fn placeholder_material() -> StandardMaterial {
    StandardMaterial {
        base_color: AVATAR_COLOR,
        ..default()
    }
}

/// The coarse (minimap) position of an avatar as a Bevy translation.
///
/// A [`CoarseLocation`] is a whole-metre position relative to the region's
/// south-west corner (`x`/`y` in `0`–`255`, `z` already in metres), carried into
/// Bevy's Y-up world by the Second Life → Bevy [axis map](crate::coords). It sits
/// in the root region's frame like the objects in [`objects`](crate::objects) —
/// no multi-region origin offset yet.
fn coarse_translation(location: &CoarseLocation) -> Vec3 {
    let position = sl_client_bevy::Vector {
        x: f32::from(location.x),
        y: f32::from(location.y),
        z: f32::from(location.z),
    };
    sl_to_bevy_vec(&position)
}

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
    /// The shared placeholder mesh and material handles, building them on first
    /// use. Borrows only [`assets`](Self::assets), so a caller can hold a
    /// disjoint borrow of the other maps.
    fn asset_handles(
        assets: &mut Option<AvatarAssets>,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
    ) -> (Handle<Mesh>, Handle<StandardMaterial>) {
        let built = assets.get_or_insert_with(|| AvatarAssets {
            mesh: meshes.add(placeholder_sphere_mesh()),
            material: materials.add(placeholder_material()),
        });
        (built.mesh.clone(), built.material.clone())
    }

    /// The tag text for an agent: its resolved legacy name, or a provisional id
    /// fragment until the name arrives.
    fn label_text(&self, agent: AgentKey) -> String {
        self.names
            .get(&agent)
            .cloned()
            .unwrap_or_else(|| provisional_label(agent))
    }

    /// Spawn the floating name-tag text node for `agent`, anchored to `anchor`
    /// and floating `tag_height` metres above it.
    fn spawn_label(
        &self,
        agent: AgentKey,
        anchor: Entity,
        tag_height: f32,
        commands: &mut Commands,
    ) -> Entity {
        commands
            .spawn((
                Text::new(self.label_text(agent)),
                TextFont {
                    font_size: FontSize::Px(NAME_TAG_FONT_SIZE),
                    ..default()
                },
                TextColor(Color::WHITE),
                // Positioned each frame by `position_name_tags`; hidden until the
                // first projection so it never flashes at the origin.
                Node {
                    position_type: PositionType::Absolute,
                    ..default()
                },
                Visibility::Hidden,
                NameTag { anchor, tag_height },
            ))
            .id()
    }

    /// Spawn a placeholder sphere and its floating name tag for `agent` at
    /// `translation`, returning both entities.
    fn spawn_sphere(
        &mut self,
        agent: AgentKey,
        translation: Vec3,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
    ) -> AvatarEntities {
        let (mesh, material) = Self::asset_handles(&mut self.assets, meshes, materials);
        let sphere = commands
            .spawn((
                Mesh3d(mesh),
                MeshMaterial3d(material),
                Transform::from_translation(translation),
                AvatarSphere,
                AvatarAnchor,
            ))
            .id();
        let label = self.spawn_label(agent, sphere, AVATAR_SPHERE_RADIUS + NAME_TAG_GAP, commands);
        AvatarEntities {
            anchor: sphere,
            label,
        }
    }

    /// Spawn a rigged base body for `agent` from the shared [`AvatarBody`]
    /// assets: a fresh joint-entity skeleton instance under a body-root anchor,
    /// with each base part skinned or pinned to it, plus the floating name tag.
    fn spawn_body(
        &self,
        agent: AgentKey,
        object: &Object,
        body: &AvatarBody,
        commands: &mut Commands,
    ) -> AvatarEntities {
        let root = commands
            .spawn((
                body_root_transform(object, body.pelvis_height),
                Visibility::default(),
                AvatarAnchor,
            ))
            .id();
        // A fresh joint entity per skeleton joint, parented in a second pass once
        // all entities exist (a parent always precedes its children, but building
        // first keeps the parenting simple). Each carries an [`AvatarJoint`]
        // marker so the appearance system can re-deform it (P13.4).
        let joints: Vec<Entity> = body
            .joint_locals
            .iter()
            .enumerate()
            .map(|(index, local)| {
                commands
                    .spawn((*local, Visibility::default(), AvatarJoint { agent, index }))
                    .id()
            })
            .collect();
        for (entity, parent) in joints.iter().zip(body.joint_parents.iter().copied()) {
            let target = parent
                .and_then(|index| joints.get(index).copied())
                .unwrap_or(root);
            commands.entity(*entity).insert(ChildOf(target));
        }
        for (index, part) in body.parts.iter().enumerate() {
            spawn_body_part(part, index, agent, &joints, root, &body.material, commands);
        }
        let label = self.spawn_label(agent, root, BODY_TAG_HEIGHT, commands);
        AvatarEntities {
            anchor: root,
            label,
        }
    }

    /// Request the legacy name of `agent` once — skipped if it is already cached
    /// or already in flight.
    fn request_name(&mut self, agent: AgentKey, commands: &mut MessageWriter<SlCommand>) {
        if self.names.contains_key(&agent) || !self.requested.insert(agent) {
            return;
        }
        commands.write(SlCommand(Command::RequestAvatarNames(vec![agent])));
    }

    /// Spawn or move a full-object avatar (`pcode` 47): its rigged base body when
    /// the [`AvatarBody`] assets are loaded, else the placeholder sphere.
    ///
    /// A full object supersedes any coarse placeholder for the same agent (the
    /// object position is precise), so an existing coarse sphere is despawned.
    fn apply_object(
        &mut self,
        object: &Object,
        body: Option<&AvatarBody>,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
        writer: &mut MessageWriter<SlCommand>,
    ) {
        let agent = AgentKey::from(object.full_id.uuid());
        let scoped = object.scoped_id();
        // A precise full object takes over from any coarse dot for this agent.
        if let Some(entities) = self.coarse.remove(&agent) {
            despawn_avatar(entities, commands);
        }
        if let Some(existing) = self.objects.get(&agent) {
            // Move the existing anchor: a body root gets the full position +
            // orientation transform, a sphere just its translation.
            let transform = match body {
                Some(body) => body_root_transform(object, body.pelvis_height),
                None => Transform::from_translation(sl_to_bevy_vec(&object.motion.position)),
            };
            commands.entity(existing.anchor).insert(transform);
            return;
        }
        self.request_name(agent, writer);
        let entities = match body {
            Some(body) => self.spawn_body(agent, object, body, commands),
            None => self.spawn_sphere(
                agent,
                sl_to_bevy_vec(&object.motion.position),
                commands,
                meshes,
                materials,
            ),
        };
        self.by_scoped.insert(scoped, agent);
        self.objects.insert(agent, entities);
        debug!(
            "spawned avatar for {agent} ({} tracked)",
            self.objects.len()
        );
    }

    /// Despawn the placeholder of the full-object avatar that left the scene under
    /// `scoped`, if one is tracked.
    fn remove_object(&mut self, scoped: ScopedObjectId, commands: &mut Commands) {
        let Some(agent) = self.by_scoped.remove(&scoped) else {
            return;
        };
        if let Some(entities) = self.objects.remove(&agent) {
            despawn_avatar(entities, commands);
        }
    }

    /// Reconcile the coarse-only avatar placeholders with one
    /// `CoarseLocationUpdate`: spawn/move a sphere for every coarse avatar that is
    /// not already a full object (and is not the agent's own `you` entry), and
    /// despawn any coarse placeholder whose avatar has dropped out of the list.
    fn apply_coarse(
        &mut self,
        locations: &[CoarseLocation],
        you: Option<usize>,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
        writer: &mut MessageWriter<SlCommand>,
    ) {
        let mut present: HashSet<AgentKey> = HashSet::new();
        for (index, location) in locations.iter().enumerate() {
            // The agent's own coarse dot is left to the (precise) object path.
            if Some(index) == you {
                continue;
            }
            let agent = location.agent_id;
            // A full-object avatar renders from its precise object position.
            if self.objects.contains_key(&agent) {
                continue;
            }
            present.insert(agent);
            let translation = coarse_translation(location);
            if let Some(existing) = self.coarse.get(&agent) {
                commands
                    .entity(existing.anchor)
                    .insert(Transform::from_translation(translation));
            } else {
                self.request_name(agent, writer);
                let entities = self.spawn_sphere(agent, translation, commands, meshes, materials);
                self.coarse.insert(agent, entities);
            }
        }
        // Despawn coarse placeholders for avatars no longer in the coarse list.
        self.coarse.retain(|agent, entities| {
            let keep = present.contains(agent);
            if !keep {
                despawn_avatar(*entities, commands);
            }
            keep
        });
    }

    /// Record a resolved legacy name and refresh the tag text of any avatar
    /// currently rendered for that agent.
    fn set_name(&mut self, name: &AvatarName, texts: &mut Query<&mut Text, With<NameTag>>) {
        let agent = name.id;
        let resolved = name.legacy_name();
        for map in [&self.objects, &self.coarse] {
            if let Some(entities) = map.get(&agent)
                && let Ok(mut text) = texts.get_mut(entities.label)
            {
                *text = Text::new(resolved.clone());
            }
        }
        debug!("resolved avatar name {agent} = {resolved:?}");
        self.names.insert(agent, resolved);
    }

    /// Record the parenting of an in-world object and, once, scan its texture
    /// entry for the `IMG_USE_BAKED_*` sentinels a worn attachment uses to hide a
    /// base-avatar region. Called for every object; a *root* object (no parent)
    /// can never be an attachment, so it is ignored.
    fn track_object(&mut self, object: &Object) {
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
    fn forget_object(&mut self, scoped: ScopedObjectId) {
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

    /// The set of baked slots to hide for each avatar: every tracked attachment
    /// whose texture entry carries `IMG_USE_BAKED_*` sentinels is attributed to
    /// its avatar (by chasing its chain), and its replaced slots unioned in.
    fn hidden_slots_per_agent(&self) -> HashMap<AgentKey, HashSet<usize>> {
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

/// The base-body baked-texture slots the viewer fetches and drapes over the
/// system body (P14): the six region bakes — head, upper body, lower body, eyes,
/// hair, and skirt. The "universal" baked slots (`*_ARM` / `*_LEG` / `AUX*`) are
/// not used by the base mesh, so they are not fetched.
const BODY_BAKE_SLOTS: [usize; 6] = [
    avatar_texture::HEAD_BAKED,
    avatar_texture::UPPER_BAKED,
    avatar_texture::LOWER_BAKED,
    avatar_texture::EYES_BAKED,
    avatar_texture::HAIR_BAKED,
    avatar_texture::SKIRT_BAKED,
];

/// The visible baked texture id in each base-body region slot of an avatar's
/// texture entry — every [`BODY_BAKE_SLOTS`] slot whose id names a real,
/// renderable bake ([`is_bake_visible`](avatar_texture::is_bake_visible)), keyed
/// by baked slot. A slot that is empty, defaulted, or invisible is omitted, so a
/// region with no published bake has nothing to fetch.
fn visible_body_bakes(texture_entry: &TextureEntry) -> HashMap<usize, TextureKey> {
    let mut bakes = HashMap::new();
    for slot in BODY_BAKE_SLOTS {
        if let Some(id) = texture_entry.texture_id(slot)
            && avatar_texture::is_bake_visible(id)
        {
            let _replaced = bakes.insert(slot, id);
        }
    }
    bakes
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

/// Despawn both entities of an avatar (its anchor — sphere or body root, whose
/// sub-hierarchy goes with it — and its name tag).
fn despawn_avatar(entities: AvatarEntities, commands: &mut Commands) {
    commands.entity(entities.anchor).try_despawn();
    commands.entity(entities.label).try_despawn();
}

/// Spawn / move / despawn the placeholder of every avatar the simulator streams
/// as a full in-world object (`pcode` 47), requesting each avatar's legacy name
/// once.
pub(crate) fn update_avatar_objects(
    mut events: MessageReader<SlEvent>,
    mut state: ResMut<AvatarState>,
    body: Option<Res<AvatarBody>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut writer: MessageWriter<SlCommand>,
) {
    let body = body.as_deref();
    for event in events.read() {
        match &event.0 {
            SlSessionEvent::ObjectAdded(object) | SlSessionEvent::ObjectUpdated(object) => {
                // Track every object's attachment linkage (an avatar's worn mesh
                // hides base-body regions via `IMG_USE_BAKED_*` faces), then render
                // the avatars themselves.
                state.track_object(object);
                if object.pcode == pcode::AVATAR {
                    state.apply_object(
                        object,
                        body,
                        &mut commands,
                        &mut meshes,
                        &mut materials,
                        &mut writer,
                    );
                }
            }
            SlSessionEvent::ObjectRemoved { local_id, .. } => {
                state.forget_object(*local_id);
                state.remove_object(*local_id, &mut commands);
            }
            _other => {}
        }
    }
}

/// Render a placeholder for every coarse-only avatar, keeping the set current with
/// each `CoarseLocationUpdate`.
///
/// Runs after [`update_avatar_objects`] so the full-object set it dedupes against
/// is current within the frame.
pub(crate) fn update_coarse_avatars(
    mut events: MessageReader<SlEvent>,
    mut state: ResMut<AvatarState>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut writer: MessageWriter<SlCommand>,
) {
    for event in events.read() {
        if let SlSessionEvent::CoarseLocationUpdate { locations, you, .. } = &event.0 {
            state.apply_coarse(
                locations,
                *you,
                &mut commands,
                &mut meshes,
                &mut materials,
                &mut writer,
            );
        }
    }
}

/// Fold resolved legacy names (`UUIDNameReply`) into the name cache and refresh
/// the tag text of any avatar already on screen.
pub(crate) fn apply_avatar_names(
    mut events: MessageReader<SlEvent>,
    mut state: ResMut<AvatarState>,
    mut texts: Query<&mut Text, With<NameTag>>,
) {
    for event in events.read() {
        if let SlSessionEvent::AvatarNames(names) = &event.0 {
            for name in names {
                state.set_name(name, &mut texts);
            }
        }
    }
}

/// Ingest each avatar's server-published baked textures (P14.1): on an
/// `AvatarAppearance`, read the baked-slot UUIDs from its texture entry
/// ([`visible_body_bakes`]), fetch each visible bake through the shared
/// [`TextureManager`] (the Phase-6 fetch / off-thread-decode / disk-cache
/// pipeline — deduped, so a bake shared by many avatars is fetched once), and
/// record them per avatar for the region materials (P14.2) to drape over the
/// system body.
///
/// These baked UUIDs are the composited avatar textures other clients render: on
/// Second Life they come from the server "Sunshine" bake, on OpenSim from other
/// avatars' viewers' client-side bakes — either way they are published ids the
/// viewer simply fetches. A slot with no real bake (empty / default / invisible)
/// is skipped, so a region with no published texture keeps its flat skin tint.
pub(crate) fn ingest_avatar_bakes(
    mut events: MessageReader<SlEvent>,
    mut state: ResMut<AvatarState>,
    mut manager: ResMut<TextureManager>,
) {
    for event in events.read() {
        if let SlSessionEvent::AvatarAppearance(appearance) = &event.0 {
            // Skip an out-of-order / duplicate resend so a stale appearance cannot
            // clobber a newer bake (P14.4); a newer or equal COF version, or one
            // with no COF version at all, is (re)fetched.
            let seen = state.baked_cof_version.get(&appearance.avatar_id).copied();
            if !should_refetch_bakes(seen, appearance.cof_version) {
                continue;
            }
            let bakes = visible_body_bakes(&appearance.texture_entry);
            for &id in bakes.values() {
                manager.request(id);
            }
            debug!(
                "requested {} baked texture(s) for {}",
                bakes.len(),
                appearance.avatar_id
            );
            if let Some(cof_version) = appearance.cof_version {
                state
                    .baked_cof_version
                    .insert(appearance.avatar_id, cof_version);
            }
            state.baked_textures.insert(appearance.avatar_id, bakes);
            // Flag the avatar so its body-region materials are (re)assigned to the
            // new bakes (P14.2); the actual draping is deferred until the textures
            // decode.
            state.bake_dirty.insert(appearance.avatar_id);
        }
    }
}

/// Whether a newly arrived `AvatarAppearance` should have its baked textures
/// (re)fetched (P14.4), given the COF version whose bakes were last fetched for
/// that avatar (`seen`) and the new appearance's COF version (`cof`).
///
/// A later appearance whose COF version is *strictly older* than the one already
/// fetched is an out-of-order / duplicate resend and is skipped, so a stale
/// appearance cannot clobber a newer bake. An *equal* version is still ingested —
/// a same-outfit rebake (e.g. after a `RebakeAvatarTextures`) can republish new
/// baked ids at the same version — and an appearance with *no* COF version
/// (OpenSim / the older path, where there is nothing to compare) always ingests.
const fn should_refetch_bakes(seen: Option<i32>, cof: Option<i32>) -> bool {
    match (seen, cof) {
        (Some(seen), Some(cof)) => cof >= seen,
        _ => true,
    }
}

/// The per-region baked-texture materials draped over the system body (P14.2):
/// one [`StandardMaterial`] per `(avatar, baked slot)`, plus the uploaded baked
/// images (deduped across avatars) and the materials parked on a bake that has
/// not decoded yet.
#[derive(Resource, Default)]
pub(crate) struct AvatarBakeMaterials {
    /// Uploaded baked Bevy images by texture id, so a bake shared by several
    /// avatars (or regions) is turned into a Bevy [`Image`] once.
    images: HashMap<TextureKey, Handle<Image>>,
    /// The material draped on each avatar body region, keyed by
    /// `(avatar, baked slot)`; its `base_color_texture` is filled once the bake
    /// decodes.
    materials: HashMap<(AgentKey, usize), Handle<StandardMaterial>>,
    /// Region materials parked on a not-yet-decoded baked texture id, filled by
    /// [`apply_avatar_bake_textures`] once it decodes.
    pending: HashMap<TextureKey, Vec<Handle<StandardMaterial>>>,
    /// The composited-alpha classification of each decoded baked texture (P14.3),
    /// computed once per id: whether it is opaque, alpha-masked, or wholly carved
    /// away (a worn mesh body's alpha layer). Drives each region material's
    /// [`AlphaMode`] and, when [`Transparent`](BakeAlpha::Transparent), hides the
    /// base region outright ([`apply_avatar_part_visibility`]).
    alpha: HashMap<TextureKey, BakeAlpha>,
}

impl AvatarBakeMaterials {
    /// The uploaded Bevy [`Image`] for a baked texture `id` together with its
    /// composited-alpha classification (P14.3), uploading and classifying it from
    /// the manager's decoded pixels on first use (both cached), or `None` if the
    /// bake is not decoded yet (still in flight or the fetch failed).
    fn ensure_bake(
        &mut self,
        id: TextureKey,
        manager: &TextureManager,
        images: &mut Assets<Image>,
    ) -> Option<(Handle<Image>, BakeAlpha)> {
        if let Some(handle) = self.images.get(&id) {
            let alpha = self.alpha.get(&id).copied().unwrap_or(BakeAlpha::Opaque);
            return Some((handle.clone(), alpha));
        }
        let decoded = manager.decoded(id)?;
        let alpha = classify_bake_alpha(decoded.components, &decoded.pixels);
        let handle = images.add(to_bevy_image(decoded));
        let _inserted = self.images.insert(id, handle.clone());
        let _classified = self.alpha.insert(id, alpha);
        Some((handle, alpha))
    }

    /// Whether the decoded bake `id` is wholly transparent — an alpha wearable
    /// carved the entire region away (typically a worn mesh body) — so the base
    /// region mesh it drapes should be hidden (P14.3). `false` for a bake that is
    /// opaque, partly masked, or not yet decoded.
    fn region_transparent(&self, id: TextureKey) -> bool {
        self.alpha
            .get(&id)
            .is_some_and(|alpha| alpha.hides_region())
    }

    /// The material for one avatar body region, keyed by `(agent, slot)`: reused
    /// across the region's parts (and re-pointed on a fresh appearance), with its
    /// baked texture filled immediately when already decoded, else parked on the
    /// bake id so [`apply_avatar_bake_textures`] fills it when it decodes.
    fn region_material(
        &mut self,
        agent: AgentKey,
        slot: usize,
        id: TextureKey,
        manager: &TextureManager,
        images: &mut Assets<Image>,
        materials: &mut Assets<StandardMaterial>,
    ) -> Handle<StandardMaterial> {
        let handle = self
            .materials
            .entry((agent, slot))
            .or_insert_with(|| materials.add(baked_region_material()))
            .clone();
        match self.ensure_bake(id, manager, images) {
            Some((image, alpha)) => {
                if let Some(mut material) = materials.get_mut(&handle) {
                    apply_bake_image(&mut material, image, alpha.alpha_mode());
                }
            }
            None => self.pending.entry(id).or_default().push(handle.clone()),
        }
        handle
    }

    /// The material for one avatar body region draped with a **locally composited**
    /// client-side bake (P15.3) rather than a fetched server bake: reuse (or
    /// create) the `(agent, slot)` region material and set the already-uploaded
    /// composited `image` + its composited-alpha `alpha` mode directly, bypassing
    /// the fetched-UUID [`ensure_bake`](Self::ensure_bake) path. Shares the same
    /// per-region material slot as [`region_material`](Self::region_material), so a
    /// server bake arriving later cleanly replaces the local one.
    fn local_region_material(
        &mut self,
        agent: AgentKey,
        slot: usize,
        image: Handle<Image>,
        alpha: BakeAlpha,
        materials: &mut Assets<StandardMaterial>,
    ) -> Handle<StandardMaterial> {
        let handle = self
            .materials
            .entry((agent, slot))
            .or_insert_with(|| materials.add(baked_region_material()))
            .clone();
        if let Some(mut material) = materials.get_mut(&handle) {
            apply_bake_image(&mut material, image, alpha.alpha_mode());
        }
        handle
    }
}

/// The un-textured base material for a body region: the skin tint as a fallback
/// until the baked texture decodes and is draped over it (P14.2). Opaque until a
/// bake with alpha overrides it; once the bake fills `base_color_texture`,
/// [`apply_bake_image`] resets the tint to white and sets the region's
/// [`AlphaMode`] from the bake's composited alpha (P14.3).
fn baked_region_material() -> StandardMaterial {
    StandardMaterial {
        base_color: BODY_COLOR,
        perceptual_roughness: 0.9,
        // Single-sided, matching the prim / base-body surfaces: Second Life
        // renders a face only from its front.
        ..default()
    }
}

/// Drape a decoded baked texture over a region material: set its diffuse image,
/// reset `base_color` to white so the composited bake (which already carries the
/// skin / clothing colour) is shown unmodified rather than tinted by the fallback
/// skin colour, and set its [`AlphaMode`] from the bake's composited alpha (P14.3)
/// so an alpha wearable carved into the bake turns that part of the region
/// invisible.
fn apply_bake_image(material: &mut StandardMaterial, image: Handle<Image>, alpha_mode: AlphaMode) {
    material.base_color = Color::WHITE;
    material.base_color_texture = Some(image);
    material.alpha_mode = alpha_mode;
}

/// The alpha threshold, as a fraction, below which a baked-texture fragment is
/// discarded — an alpha wearable carved it away. Matches the classification
/// cutoff [`BAKE_ALPHA_CUTOFF`] (`0.5 * 255`), and is the [`AlphaMode::Mask`]
/// cutoff used for a masked region (P14.3).
const BAKE_ALPHA_MASK_THRESHOLD: f32 = 0.5;

/// The 8-bit alpha value below which a baked-texture pixel counts as carved away
/// when classifying a bake ([`classify_bake_alpha`]) — `0.5 * 255`, rounded, to
/// match [`BAKE_ALPHA_MASK_THRESHOLD`].
const BAKE_ALPHA_CUTOFF: u8 = 128;

/// How a decoded baked texture's composited alpha channel renders its body
/// region (P14.3): the alpha wearables the grid composited into the bake carve
/// pixels away, and the region is drawn accordingly.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum BakeAlpha {
    /// Fully opaque — no alpha channel, or every pixel at or above the cutoff.
    /// Rendered as [`AlphaMode::Opaque`] (cheapest, and correct for plain skin).
    Opaque,
    /// A mix of kept and carved pixels — an alpha wearable cut part of the region
    /// away. Rendered as [`AlphaMode::Mask`] so the carved pixels vanish.
    Masked,
    /// Every pixel carved away — the whole region is invisible (typically a worn
    /// mesh body's alpha layer). The base region mesh is hidden outright.
    Transparent,
}

impl BakeAlpha {
    /// The Bevy [`AlphaMode`] to render a region bake with: opaque skin stays in
    /// the cheap opaque pass, anything carved uses masking (a wholly transparent
    /// region also masks, though it is normally hidden by
    /// [`hides_region`](Self::hides_region) before it draws).
    const fn alpha_mode(self) -> AlphaMode {
        match self {
            Self::Opaque => AlphaMode::Opaque,
            Self::Masked | Self::Transparent => AlphaMode::Mask(BAKE_ALPHA_MASK_THRESHOLD),
        }
    }

    /// Whether the region this bake drapes should be hidden entirely — true only
    /// when the whole bake is carved away ([`Transparent`](Self::Transparent)).
    const fn hides_region(self) -> bool {
        matches!(self, Self::Transparent)
    }
}

/// Classify a decoded baked texture's composited alpha (P14.3) from its source
/// component count and RGBA8 pixels: a source with no alpha channel
/// (`components < 4`) is always [`Opaque`](BakeAlpha::Opaque); otherwise the
/// alpha bytes are scanned once — all at or above the cutoff is `Opaque`, all
/// below is [`Transparent`](BakeAlpha::Transparent), and any mix is
/// [`Masked`](BakeAlpha::Masked).
fn classify_bake_alpha(components: u16, pixels: &[u8]) -> BakeAlpha {
    // No alpha channel: the decoder filled alpha to fully opaque.
    if components < 4 {
        return BakeAlpha::Opaque;
    }
    let mut any_kept = false;
    let mut any_carved = false;
    for &alpha in pixels.iter().skip(3).step_by(4) {
        if alpha < BAKE_ALPHA_CUTOFF {
            any_carved = true;
        } else {
            any_kept = true;
        }
        // Once both kinds are seen the region is masked; stop scanning.
        if any_kept && any_carved {
            return BakeAlpha::Masked;
        }
    }
    match (any_kept, any_carved) {
        // Nothing carved (or no pixels at all) → opaque.
        (_, false) => BakeAlpha::Opaque,
        // Every pixel carved → wholly transparent.
        (false, true) => BakeAlpha::Transparent,
        // A mix is returned inside the loop; kept here for totality.
        (true, true) => BakeAlpha::Masked,
    }
}

/// Drape each avatar's server-published baked textures over its system body
/// (P14.2): give every base part a per-`(avatar, region)` material carrying that
/// region's baked texture (head → head bake, upper → upper-body bake, …), so the
/// avatar renders skin- and clothing-textured instead of flat skin tone. A region
/// with no published bake keeps the shared un-textured skin material.
///
/// Deferred and idempotent, mirroring [`apply_avatar_appearance`]: a fresh
/// appearance (or a body part that just spawned, matched by [`Added`]) flags the
/// avatar, and its region materials are (re)assigned from the tracked bakes — so a
/// bake ingested before the body still lands once the body exists. The baked
/// image itself is filled in when it decodes ([`apply_avatar_bake_textures`]). A
/// no-op when no avatar asset library / body loaded (avatars stay spheres).
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system reading the tracked bakes and the ECS resources the region materials need"
)]
pub(crate) fn assign_avatar_bake_materials(
    mut state: ResMut<AvatarState>,
    body: Option<Res<AvatarBody>>,
    mut bake_mats: ResMut<AvatarBakeMaterials>,
    manager: Res<TextureManager>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    added: Query<&AvatarBodyPart, Added<AvatarBodyPart>>,
    mut parts: Query<(&AvatarBodyPart, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    // A newly spawned part needs its region material assigned (the bakes can
    // arrive before the body object does).
    for part in &added {
        if state.baked_textures.contains_key(&part.agent) {
            state.bake_dirty.insert(part.agent);
        }
    }
    if state.bake_dirty.is_empty() {
        return;
    }
    let Some(body) = body else {
        state.bake_dirty.clear();
        return;
    };
    let mut draped = 0_usize;
    for (part, mut material) in &mut parts {
        if !state.bake_dirty.contains(&part.agent) {
            continue;
        }
        let slot = part.region.baked_slot();
        let desired = match state
            .baked_textures
            .get(&part.agent)
            .and_then(|bakes| bakes.get(&slot))
        {
            // A published bake for this region: its per-avatar region material.
            Some(&id) => bake_mats.region_material(
                part.agent,
                slot,
                id,
                &manager,
                &mut images,
                &mut materials,
            ),
            // No bake for this region: the shared un-textured skin material.
            None => body.material.clone(),
        };
        if material.0 != desired {
            *material = MeshMaterial3d(desired);
            draped = draped.saturating_add(1);
        }
    }
    if draped > 0 {
        debug!("assigned bake material to {draped} avatar body part(s)");
    }
    state.bake_dirty.clear();
}

/// Fill each newly decoded avatar bake into the region materials parked on it
/// (P14.2): upload (and cache) the baked [`Image`], then drop it into every parked
/// material's `base_color_texture`. Mirrors [`apply_prim_textures`](crate::textures::apply_prim_textures);
/// a decode that failed leaves the parked materials on their fallback skin tint.
pub(crate) fn apply_avatar_bake_textures(
    mut decoded: MessageReader<TextureDecoded>,
    manager: Res<TextureManager>,
    mut bake_mats: ResMut<AvatarBakeMaterials>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mut filled = 0_usize;
    for &TextureDecoded(id) in decoded.read() {
        let Some(parked) = bake_mats.pending.remove(&id) else {
            // Not a bake any avatar region is waiting on (e.g. a prim texture).
            continue;
        };
        let Some((image, alpha)) = bake_mats.ensure_bake(id, &manager, &mut images) else {
            // The fetch failed: the parked regions keep their flat skin tint.
            continue;
        };
        for material_handle in parked {
            if let Some(mut material) = materials.get_mut(&material_handle) {
                apply_bake_image(&mut material, image.clone(), alpha.alpha_mode());
                filled = filled.saturating_add(1);
            }
        }
    }
    if filled > 0 {
        debug!("draped {filled} decoded bake(s) onto avatar body region material(s)");
    }
}

/// The side length, in pixels, of a locally composited client-side bake region
/// (P15.3). The reference viewer bakes body regions at 512×512; each source
/// wearable layer is bilinearly resampled to this by [`composite_region`].
const LOCAL_BAKE_SIZE: u32 = 512;

/// Our own avatar's **client-side** composited bake (P15.3): one uploaded
/// [`Image`] plus its composited-alpha classification per baked slot, built once
/// the client-side bake inputs ([`OwnBakeInputs`]) are assembled.
///
/// On a grid that publishes no server "Sunshine" bake for our own avatar
/// (OpenSim, and any grid without central baking) our own avatar would otherwise
/// stay an untextured cloud: the P14 [`ingest_avatar_bakes`] path finds no baked
/// UUIDs in our own appearance, so [`assign_avatar_bake_materials`] leaves our
/// body on the flat skin material. This resource instead holds the bake the
/// *viewer* composited from the worn wearable layers (P15.1/P15.2), which
/// [`apply_own_local_bake`] drapes over our own body regions — the client-bake
/// counterpart of the server bake other avatars (and our own on Second Life)
/// carry.
#[derive(Resource, Default)]
pub(crate) struct OwnLocalBake {
    /// The composited region image + its alpha classification, keyed by baked
    /// slot ([`BakeRegion::slot`]); a region with no worn layers is absent (so its
    /// body part keeps the flat skin material rather than a transparent bake).
    regions: HashMap<usize, (Handle<Image>, BakeAlpha)>,
    /// Whether the composite has been built from the ready bake inputs (a one-shot
    /// per session — the worn outfit does not change in this passive viewer).
    built: bool,
}

/// Flip an RGBA8 image's rows in place — mirror it about its horizontal axis —
/// mapping between the top-down decoded-image row order and the OpenGL bottom-up
/// convention Second Life avatar UVs are authored in (P15.3). A zero dimension,
/// or a pixel buffer too short for `width`×`height` RGBA, is left untouched.
const fn flip_rows_vertically(pixels: &mut [u8], width: usize, height: usize) {
    let stride = width.saturating_mul(RGBA_CHANNELS);
    // Guard the swaps: every index touched must be within the buffer.
    if stride == 0 || height == 0 || pixels.len() < stride.saturating_mul(height) {
        return;
    }
    let mut row = 0_usize;
    while row < height / 2 {
        let opposite = height.saturating_sub(1).saturating_sub(row);
        let top = row.saturating_mul(stride);
        let bottom = opposite.saturating_mul(stride);
        let mut offset = 0_usize;
        while offset < stride {
            pixels.swap(top.saturating_add(offset), bottom.saturating_add(offset));
            offset = offset.saturating_add(1);
        }
        row = row.saturating_add(1);
    }
}

/// Force every pixel of an RGBA8 image fully opaque (alpha byte → 255), so a
/// bake draped on a solid surface (the eyeball) is not carved by stray
/// source-texture transparency (P15.3).
fn force_alpha_opaque(pixels: &mut [u8]) {
    let mut index = RGBA_CHANNELS.saturating_sub(1);
    while index < pixels.len() {
        // The alpha byte of each RGBA texel.
        if let Some(alpha) = pixels.get_mut(index) {
            *alpha = u8::MAX;
        }
        index = index.saturating_add(RGBA_CHANNELS);
    }
}

/// Composite our own avatar's ready client-side bake inputs (P15.2) into one
/// uploaded [`Image`] + alpha classification per baked slot: walk each bake
/// region's ordered layer list ([`OwnBakeInputs::region_layers`]) through
/// [`composite_region`], classify the composited alpha (so an alpha wearable
/// carved into the bake renders masked, P14.3), and upload the RGBA to a Bevy
/// [`Image`]. A region with no worn layers is skipped — an empty composite is
/// wholly transparent and would wrongly carve the region away.
fn build_local_bake(
    inputs: &OwnBakeInputs,
    images: &mut Assets<Image>,
) -> HashMap<usize, (Handle<Image>, BakeAlpha)> {
    let mut regions = HashMap::new();
    let mut summary: Vec<String> = Vec::new();
    for region in BakeRegion::ALL {
        let layers = inputs.region_layers(region);
        if layers.is_empty() {
            continue;
        }
        let mut baked = composite_region(region, LOCAL_BAKE_SIZE, layers);
        // Second Life avatar `.llm` UVs are authored in the OpenGL bottom-up
        // convention (V = 0 at the bottom), so the body samples a baked texture
        // upside down relative to a top-down decoded image — the composite (like
        // a fetched J2C) is top-down, which would land the head bake's chin/teeth
        // (low in the image) on the forehead. Flip the composited rows so the
        // body-region UVs sample it the right way up.
        let side = usize::try_from(LOCAL_BAKE_SIZE).unwrap_or(0);
        flip_rows_vertically(&mut baked.pixels, side, side);
        // The eyeball is an opaque surface. Our simplified eye composite carries
        // only the iris layer, not the opaque sclera base the reference viewer's
        // eye layer set builds, so the iris texture's own transparent surround
        // would classify the bake as masked and carve the eyeballs into empty
        // sockets. Force the eye bake opaque so the eyeballs render solid.
        if region == BakeRegion::Eyes {
            force_alpha_opaque(&mut baked.pixels);
        }
        let decoded = baked.to_decoded_image();
        let alpha = classify_bake_alpha(decoded.components, &decoded.pixels);
        let handle = images.add(to_bevy_image(&decoded));
        let _prev = regions.insert(region.slot(), (handle, alpha));
        summary.push(format!(
            "{}={} layer(s)/{alpha:?}",
            region.name(),
            layers.len()
        ));
    }
    info!(
        "composited client-side bake for own avatar: {}",
        summary.join(" ")
    );
    regions
}

/// Drape our own avatar's locally composited client-side bake (P15.3) over its
/// body regions when the grid publishes no server bake for us (OpenSim).
///
/// Once the bake inputs are assembled ([`OwnBakeInputs::is_ready`]) the composite
/// is built once ([`build_local_bake`]) and, for each of our own body parts whose
/// region the grid did **not** bake for us, the composited region image is set as
/// that region's material — reusing the same per-`(agent, slot)` material slot the
/// P14 server-bake path uses, so a server bake (Second Life) cleanly wins over the
/// local one. Runs every frame but idempotent: it only re-assigns a body part
/// whose material actually differs, so it self-heals after
/// [`assign_avatar_bake_materials`] resets a part on a fresh appearance, and lands
/// on parts that spawn after the composite is ready.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system compositing our own bake and draping it over the body-region materials"
)]
pub(crate) fn apply_own_local_bake(
    identity: Res<SlIdentity>,
    inputs: Res<OwnBakeInputs>,
    state: Res<AvatarState>,
    body: Option<Res<AvatarBody>>,
    mut local: ResMut<OwnLocalBake>,
    mut bake_mats: ResMut<AvatarBakeMaterials>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut parts: Query<(&AvatarBodyPart, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    // Nothing to drape until the body assets loaded, the bake inputs are ready,
    // and we know which agent is our own avatar.
    if body.is_none() || !inputs.is_ready() {
        return;
    }
    let Some(agent) = identity.agent_id else {
        return;
    };
    if !local.built {
        local.regions = build_local_bake(&inputs, &mut images);
        local.built = true;
    }
    if local.regions.is_empty() {
        return;
    }
    let mut draped = 0_usize;
    for (part, mut material) in &mut parts {
        if part.agent != agent {
            continue;
        }
        let slot = part.region.baked_slot();
        // A server-published bake for this region wins (P14 / Second Life); the
        // local composite only fills regions the grid did not bake for us.
        if state
            .baked_textures
            .get(&agent)
            .is_some_and(|bakes| bakes.contains_key(&slot))
        {
            continue;
        }
        let Some((image, alpha)) = local.regions.get(&slot) else {
            continue;
        };
        let desired =
            bake_mats.local_region_material(agent, slot, image.clone(), *alpha, &mut materials);
        if material.0 != desired {
            *material = MeshMaterial3d(desired);
            draped = draped.saturating_add(1);
        }
    }
    if draped > 0 {
        debug!("draped client-side bake onto {draped} own avatar body part(s)");
    }
}

/// Apply each rigged avatar's appearance (P13.3 morphs + P13.4 skeletal shape):
/// resolve an `AvatarAppearance.visual_params` vector once into its
/// driver-propagated, sex-gated weights, then (a) rebuild every affected base
/// part's mesh from the morph-target deltas so the body takes its real shape and
/// (b) re-deform the skeleton instance's joint transforms so the avatar's
/// proportions (height, limb / head scale, hips) match. Re-applied whenever a
/// newer appearance arrives.
///
/// The work is deferred and idempotent: a fresh appearance (or a body part that
/// just spawned, matched by [`Added`]) marks the avatar dirty, and the
/// appearance is (re)built from the cached vector — so an appearance that arrives
/// before the body still lands once the body exists. A no-op when no avatar asset
/// library loaded (avatars stay as un-shaped bodies or spheres).
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system folding appearances and bakes into the morphed body meshes"
)]
pub(crate) fn apply_avatar_appearance(
    mut events: MessageReader<SlEvent>,
    mut decoded: MessageReader<TextureDecoded>,
    library: Option<Res<AvatarAssetLibrary>>,
    manager: Res<TextureManager>,
    mut state: ResMut<AvatarState>,
    mut meshes: ResMut<Assets<Mesh>>,
    added: Query<&AvatarBodyPart, Added<AvatarBodyPart>>,
    mut parts: Query<(&AvatarBodyPart, &mut Mesh3d)>,
    mut joints: Query<(&AvatarJoint, &mut Transform)>,
) {
    // A decoded baked texture of a masked body region (head / upper / lower)
    // supplies the clothing-morph mask, so re-shape any avatar wearing it: its
    // flared morphs were applied unmasked until the bake decoded (P14.5).
    for &TextureDecoded(id) in decoded.read() {
        let wearers: Vec<AgentKey> = state
            .baked_textures
            .iter()
            .filter(|(_, bakes)| {
                bakes
                    .iter()
                    .any(|(&slot, &bake)| bake == id && is_masked_body_slot(slot))
            })
            .map(|(&agent, _)| agent)
            .collect();
        for agent in wearers {
            state.appearance_dirty.insert(agent);
        }
    }
    // Fold any fresh appearance vectors into the cache and flag those avatars.
    for event in events.read() {
        if let SlSessionEvent::AvatarAppearance(appearance) = &event.0 {
            state
                .appearances
                .insert(appearance.avatar_id, appearance.visual_params.clone());
            // The base skirt mesh renders only when the skirt bake is visible (the
            // reference viewer's `isWearingWearableType(WT_SKIRT) &&
            // isTextureVisible(TEX_SKIRT_BAKED)`, which for another avatar reduces
            // to the baked slot holding a real, non-invisible texture).
            let skirt_visible = appearance
                .texture_entry
                .texture_id(avatar_texture::SKIRT_BAKED)
                .is_some_and(avatar_texture::is_bake_visible);
            state
                .skirt_visible
                .insert(appearance.avatar_id, skirt_visible);
            debug!(
                "appearance for {}: skirt {}",
                appearance.avatar_id,
                if skirt_visible { "worn" } else { "not worn" }
            );
            state.appearance_dirty.insert(appearance.avatar_id);
        }
    }
    // A body part that just spawned needs its cached appearance applied (the
    // appearance can arrive before the body object does). The joints spawn with
    // the same body, so this one signal covers both morphs and skeleton.
    for part in &added {
        if state.appearances.contains_key(&part.agent) {
            state.appearance_dirty.insert(part.agent);
        }
    }
    if state.appearance_dirty.is_empty() {
        return;
    }
    let Some(library) = library else {
        state.appearance_dirty.clear();
        return;
    };
    // Resolve each dirty avatar's appearance once into its morph weights and the
    // deformed joint transforms (both share one `ResolvedParams`).
    let mut morph_weights: HashMap<AgentKey, MorphWeights> = HashMap::new();
    let mut joint_transforms: HashMap<AgentKey, Vec<Transform>> = HashMap::new();
    for &agent in &state.appearance_dirty {
        if let Some(bytes) = state.appearances.get(&agent) {
            let resolved = ResolvedParams::from_appearance(library.params(), bytes);
            morph_weights.insert(
                agent,
                MorphWeights::from_resolved(library.params(), &resolved),
            );
            let deform = SkeletalDeformations::from_resolved(library.params(), &resolved);
            joint_transforms.insert(agent, library.skeleton().deformed_local_transforms(&deform));
        }
    }
    // Rebuild the mesh of every part belonging to a resolved avatar, masking its
    // clothing morphs by the region's decoded bake where one is available (P14.5).
    let mut morphed_parts = 0_usize;
    for (part, mut mesh) in &mut parts {
        if let Some(weights) = morph_weights.get(&part.agent)
            && let Some(loaded) = library.parts().get(part.part)
        {
            let morphed = match part_clothing_mask(
                &library,
                &manager,
                state.baked_textures.get(&part.agent),
                part.region,
                &loaded.mesh,
            ) {
                Some(mask) => weights.apply_masked(&loaded.mesh, &mask),
                None => weights.apply(&loaded.mesh),
            };
            *mesh = Mesh3d(meshes.add(to_bevy_morphed_mesh(&loaded.mesh, &morphed)));
            morphed_parts = morphed_parts.saturating_add(1);
        }
    }
    // Re-set every joint transform of a resolved avatar's skeleton instance.
    let mut deformed_joints = 0_usize;
    for (joint, mut transform) in &mut joints {
        if let Some(transforms) = joint_transforms.get(&joint.agent)
            && let Some(deformed) = transforms.get(joint.index)
        {
            *transform = *deformed;
            deformed_joints = deformed_joints.saturating_add(1);
        }
    }
    if morphed_parts > 0 || deformed_joints > 0 {
        debug!(
            "shaped {morphed_parts} body part(s) + {deformed_joints} joint(s) across {} avatar(s)",
            morph_weights.len()
        );
    }
    state.appearance_dirty.clear();
}

/// Whether a baked-texture `slot` supplies a clothing-morph mask (P14.5) — the
/// head, upper-body and lower-body region bakes, whose alpha channel masks the
/// flared clothing morphs. A decode of one of these re-shapes the wearing avatar.
const fn is_masked_body_slot(slot: usize) -> bool {
    slot == avatar_texture::HEAD_BAKED
        || slot == avatar_texture::UPPER_BAKED
        || slot == avatar_texture::LOWER_BAKED
}

/// The per-vertex clothing-morph mask (P14.5) for one base part, sampled from its
/// region's decoded baked texture, or `None` when the part has no masked morphs,
/// no published bake for its region, or the bake has not decoded yet (its morphs
/// then apply unmasked — the full flare — until the bake arrives and re-shapes it).
fn part_clothing_mask(
    library: &AvatarAssetLibrary,
    manager: &TextureManager,
    baked: Option<&HashMap<usize, TextureKey>>,
    region: BodyRegion,
    mesh: &BaseMesh,
) -> Option<PartMorphMask> {
    let region_name = region.morph_mask_region()?;
    if !library.masks().has_region(region_name) {
        return None;
    }
    let id = *baked?.get(&region.baked_slot())?;
    let decoded = manager.decoded(id)?;
    // The decoded pixels are always expanded to RGBA8 (stride 4, alpha at offset
    // 3) regardless of the source component count; a source with no alpha channel
    // decodes to opaque alpha (255), which masks nothing — the correct fallback
    // when a bake carries no clothing-coverage mask (Firestorm's null-aux path).
    let texture = MaskTexture {
        pixels: &decoded.pixels,
        width: usize::try_from(decoded.width).unwrap_or(0),
        height: usize::try_from(decoded.height).unwrap_or(0),
        components: RGBA_CHANNELS,
    };
    let mask = library.masks().sample_part(mesh, region_name, &texture);
    if mask.is_empty() { None } else { Some(mask) }
}

/// Show or hide each rigged base-part mesh from the avatar's worn items (P13.5
/// whole-mesh show/hide): hide a whole base region (head / hair / eyes / upper /
/// lower / skirt) when a worn attachment face carries the matching
/// `IMG_USE_BAKED_*` sentinel (a mesh body replacing it), and render the skirt
/// part only when the avatar's `TEX_SKIRT_BAKED` slot holds a visible bake.
///
/// A region is also hidden when its whole baked texture is transparent (P14.3):
/// an alpha wearable carved the entire region away (typically a worn mesh body),
/// which the `IMG_USE_BAKED_*` sentinel path may not signal on its own.
///
/// Runs every frame — cheap (a handful of parts per avatar, and only the rare
/// `IMG_USE_BAKED_*`-bearing attachment is chased) and idempotent: it only writes
/// a [`Visibility`] that actually changed, so it never churns change-detection.
/// The clothing-morph alpha masks (P14.5) — the per-vertex flared-cuff carving —
/// are a *geometry* mask applied in [`apply_avatar_appearance`], not a visibility
/// toggle, so they are not handled here.
pub(crate) fn apply_avatar_part_visibility(
    state: Res<AvatarState>,
    bake_mats: Res<AvatarBakeMaterials>,
    mut parts: Query<(&AvatarBodyPart, &mut Visibility)>,
) {
    let hidden = state.hidden_slots_per_agent();
    let mut changed = 0_usize;
    for (part, mut visibility) in &mut parts {
        let slot = part.region.baked_slot();
        // Hidden either by a worn mesh's `IMG_USE_BAKED_*` sentinel (P13.5) or by
        // the region's own bake being wholly carved away by alpha (P14.3).
        let alpha_hidden = state
            .baked_textures
            .get(&part.agent)
            .and_then(|bakes| bakes.get(&slot))
            .is_some_and(|&id| bake_mats.region_transparent(id));
        let region_hidden = alpha_hidden
            || hidden
                .get(&part.agent)
                .is_some_and(|slots| slots.contains(&slot));
        let visible = match part.region {
            // A skirt shows only when worn (and not itself replaced by a mesh).
            BodyRegion::Skirt => {
                !region_hidden
                    && state
                        .skirt_visible
                        .get(&part.agent)
                        .copied()
                        .unwrap_or(false)
            }
            _other => !region_hidden,
        };
        let desired = if visible {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != desired {
            *visibility = desired;
            changed = changed.saturating_add(1);
        }
    }
    if changed > 0 {
        debug!("updated visibility of {changed} avatar body part(s)");
    }
}

/// Position each avatar name tag over its anchor by projecting the anchor's world
/// position (offset up by the tag's own height) to the screen and anchoring the
/// tag's *bottom-centre* on that point, so the text is centred over the avatar
/// and floats just above it; tags whose anchor is off-screen or behind the camera
/// are hidden.
///
/// The projection ([`Camera::world_to_viewport`](sl_client_bevy::Camera)) and the
/// UI `Val::Px` layout are both in logical pixels, but [`ComputedNode::size`] is
/// physical, so the tag's own size is scaled by its
/// [`inverse_scale_factor`](ComputedNode::inverse_scale_factor) before centring.
pub(crate) fn position_name_tags(
    cameras: Query<(&Camera, &GlobalTransform)>,
    anchors: Query<&GlobalTransform, With<AvatarAnchor>>,
    mut tags: Query<(&NameTag, &ComputedNode, &mut Node, &mut Visibility)>,
) {
    let Ok((camera, camera_transform)) = cameras.single() else {
        return;
    };
    for (tag, computed, mut node, mut visibility) in &mut tags {
        let Ok(anchor) = anchors.get(tag.anchor) else {
            *visibility = Visibility::Hidden;
            continue;
        };
        let base = anchor.translation();
        // Float the tag just above the avatar's head (per-component add to avoid
        // the `arithmetic_side_effects` lint on the glam `Vec3` operator).
        let head = Vec3::new(base.x, base.y + tag.tag_height, base.z);
        match camera.world_to_viewport(camera_transform, head) {
            Ok(screen) => {
                // The tag's own logical size, to anchor its bottom-centre on the
                // projected head point (previous frame's layout — one-frame lag is
                // imperceptible; a just-spawned tag has zero size for one frame).
                let size = computed.size();
                let inverse_scale = computed.inverse_scale_factor();
                let half_width = size.x * inverse_scale / 2.0;
                let height = size.y * inverse_scale;
                node.left = Val::Px(screen.x - half_width);
                node.top = Val::Px(screen.y - height);
                *visibility = Visibility::Inherited;
            }
            Err(_off_screen) => {
                *visibility = Visibility::Hidden;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AvatarState, BakeAlpha, PROVISIONAL_ID_CHARS, body_root_transform, classify_bake_alpha,
        coarse_translation, provisional_label, should_refetch_bakes, used_baked_slots,
        visible_body_bakes,
    };
    use crate::avatar_assets::BodyRegion;
    use crate::coords::sl_to_bevy_rotation;
    use bevy::math::Vec3;
    use bevy::prelude::AlphaMode;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{
        AgentKey, BakeRegion, CircuitId, CoarseLocation, Object, ObjectMotion, RegionHandle,
        RegionLocalObjectId, Rotation, ScopedObjectId, TextureEntry, TextureFace, TextureKey, Uuid,
        Vector, avatar_texture, encode_texture_entry,
    };

    /// The zero vector (`Vector` does not derive `Default`).
    const fn zero() -> Vector {
        Vector {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }
    }

    /// A minimal avatar object at `position` with an identity (facing) rotation.
    fn avatar_object_at(position: Vector) -> Object {
        Object {
            region_handle: RegionHandle(0),
            local_id: RegionLocalObjectId(1),
            circuit: CircuitId::new(1),
            full_id: Uuid::from_u128(1).into(),
            parent_id: RegionLocalObjectId(0),
            pcode: sl_client_bevy::pcode::AVATAR,
            state: 0,
            crc: 0,
            material: 0,
            click_action: 0,
            update_flags: 0,
            scale: Vector {
                x: 1.0,
                y: 1.0,
                z: 1.0,
            },
            motion: ObjectMotion {
                position,
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

    /// A coarse location maps its whole-metre region-relative position through the
    /// Second Life → Bevy axis map (Second Life `(x, y, z)` → Bevy `(x, z, -y)`).
    #[test]
    fn coarse_translation_maps_through_axis_swap() {
        let location = CoarseLocation {
            agent_id: AgentKey::from(Uuid::from_u128(1)),
            x: 10,
            y: 20,
            z: 24,
        };
        assert_eq!(coarse_translation(&location), Vec3::new(10.0, 24.0, -20.0));
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

    /// A body root maps the object position through the Second Life → Bevy axis
    /// swap and lowers it by the pelvis rest height, so the skeleton's pelvis
    /// (at that height) lands back at the reported position; with an identity
    /// facing rotation, the root carries just the basis change.
    #[test]
    fn body_root_plants_pelvis_at_the_object_position() {
        let pelvis_height = 1.067;
        let object = avatar_object_at(Vector {
            x: 10.0,
            y: 20.0,
            z: 30.0,
        });
        let transform = body_root_transform(&object, pelvis_height);
        // Second Life (10, 20, 30) → Bevy (10, 30, -20), then lowered in Y by the
        // pelvis height.
        assert_eq!(
            transform.translation,
            Vec3::new(10.0, 30.0 - pelvis_height, -20.0)
        );
        // An identity object rotation leaves only the basis change at the root.
        assert!(
            transform
                .rotation
                .abs_diff_eq(sl_to_bevy_rotation(), 1.0e-6)
        );
    }

    /// Each body region keys its visibility off its own baked slot — the head
    /// (and eyelashes) off the head bake, the eyes off the eyes bake, and so on.
    #[test]
    fn body_region_maps_to_its_baked_slot() {
        assert_eq!(BodyRegion::Head.baked_slot(), avatar_texture::HEAD_BAKED);
        assert_eq!(BodyRegion::Hair.baked_slot(), avatar_texture::HAIR_BAKED);
        assert_eq!(BodyRegion::Eyes.baked_slot(), avatar_texture::EYES_BAKED);
        assert_eq!(BodyRegion::Upper.baked_slot(), avatar_texture::UPPER_BAKED);
        assert_eq!(BodyRegion::Lower.baked_slot(), avatar_texture::LOWER_BAKED);
        assert_eq!(BodyRegion::Skirt.baked_slot(), avatar_texture::SKIRT_BAKED);
    }

    /// The client-side bake (P15.3) keys its composited regions by
    /// [`BakeRegion::slot`], and looks them up per body part by
    /// [`BodyRegion::baked_slot`]; the two slot mappings must agree for every
    /// region, or a composited bake would never be found for its body part.
    #[test]
    fn body_region_baked_slots_round_trip_through_bake_region() {
        for region in [
            BodyRegion::Head,
            BodyRegion::Hair,
            BodyRegion::Eyes,
            BodyRegion::Upper,
            BodyRegion::Lower,
            BodyRegion::Skirt,
        ] {
            let slot = region.baked_slot();
            // Every body region's baked slot names a bake region, and that bake
            // region reports the same slot the local composite is keyed by.
            assert_eq!(
                BakeRegion::from_slot(slot).map(BakeRegion::slot),
                Some(slot)
            );
        }
    }

    /// The client-side bake flip (P15.3) mirrors the image about its horizontal
    /// axis: the top row and bottom row swap, and a degenerate buffer is left
    /// untouched. A 1×2 RGBA image (one red row over one blue row) inverts.
    #[test]
    fn flip_rows_vertically_mirrors_top_and_bottom() {
        // Row 0 red, row 1 blue (1 px wide, RGBA).
        let mut pixels = vec![255, 0, 0, 255, 0, 0, 255, 255];
        super::flip_rows_vertically(&mut pixels, 1, 2);
        assert_eq!(pixels, vec![0, 0, 255, 255, 255, 0, 0, 255]);
        // A buffer too short for its declared geometry is left as-is.
        let mut short = vec![1, 2, 3];
        super::flip_rows_vertically(&mut short, 1, 2);
        assert_eq!(short, vec![1, 2, 3]);
    }

    /// The eye-bake opacity forcing (P15.3) sets every texel's alpha byte to 255
    /// while leaving the colour channels untouched, so a transparent-surround iris
    /// no longer carves the opaque eyeball away.
    #[test]
    fn force_alpha_opaque_fills_only_the_alpha_channel() {
        // Two RGBA texels with varied colour and alpha 0 / 128.
        let mut pixels = vec![10, 20, 30, 0, 40, 50, 60, 128];
        super::force_alpha_opaque(&mut pixels);
        assert_eq!(pixels, vec![10, 20, 30, 255, 40, 50, 60, 255]);
    }

    /// Only the head, upper-body and lower-body regions carry masked clothing
    /// morphs (P14.5); their bakes are the ones whose decode re-shapes the body,
    /// and they map to the `<morph_masks>` `body_region` names.
    #[test]
    fn masked_body_regions_map_to_morph_mask_names() {
        assert_eq!(BodyRegion::Head.morph_mask_region(), Some("head"));
        assert_eq!(BodyRegion::Upper.morph_mask_region(), Some("upper_body"));
        assert_eq!(BodyRegion::Lower.morph_mask_region(), Some("lower_body"));
        assert_eq!(BodyRegion::Hair.morph_mask_region(), None);
        assert_eq!(BodyRegion::Eyes.morph_mask_region(), None);
        assert_eq!(BodyRegion::Skirt.morph_mask_region(), None);

        // The masked slots are exactly the head / upper / lower bakes.
        assert!(super::is_masked_body_slot(avatar_texture::HEAD_BAKED));
        assert!(super::is_masked_body_slot(avatar_texture::UPPER_BAKED));
        assert!(super::is_masked_body_slot(avatar_texture::LOWER_BAKED));
        assert!(!super::is_masked_body_slot(avatar_texture::HAIR_BAKED));
        assert!(!super::is_masked_body_slot(avatar_texture::EYES_BAKED));
        assert!(!super::is_masked_body_slot(avatar_texture::SKIRT_BAKED));
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

    /// `visible_body_bakes` picks out the visible baked texture in each base-body
    /// region slot (keyed by slot) and skips a slot left empty or set to the
    /// invisible / default sentinel.
    #[test]
    fn visible_body_bakes_reads_the_region_slots() {
        let head = TextureKey::from(Uuid::from_u128(0xabc));
        let upper = TextureKey::from(Uuid::from_u128(0xdef));
        // Build a full-length face table so every baked slot index exists, with a
        // real bake in head/upper, the invisible sentinel in lower, and the null
        // id everywhere else (built by index to avoid slice indexing).
        let faces = (0..avatar_texture::COUNT)
            .map(|slot| {
                let id = if slot == avatar_texture::HEAD_BAKED {
                    head
                } else if slot == avatar_texture::UPPER_BAKED {
                    upper
                } else if slot == avatar_texture::LOWER_BAKED {
                    TextureKey::from(avatar_texture::IMG_INVISIBLE)
                } else {
                    TextureKey::from(Uuid::nil())
                };
                TextureFace::new(id)
            })
            .collect();
        let bakes = visible_body_bakes(&TextureEntry { faces });
        assert_eq!(bakes.get(&avatar_texture::HEAD_BAKED), Some(&head));
        assert_eq!(bakes.get(&avatar_texture::UPPER_BAKED), Some(&upper));
        // The invisible-sentinel lower slot and the empty eyes/hair/skirt slots
        // are not visible bakes.
        assert!(!bakes.contains_key(&avatar_texture::LOWER_BAKED));
        assert!(!bakes.contains_key(&avatar_texture::EYES_BAKED));
        assert_eq!(bakes.len(), 2, "only the two real bakes are picked up");
    }

    /// A baked texture's composited alpha (P14.3) is classified from its source
    /// component count and RGBA8 pixels: no alpha channel is opaque, an all-carved
    /// alpha is wholly transparent, an all-kept alpha is opaque, and any mix is
    /// masked.
    #[test]
    fn classify_bake_alpha_reads_the_alpha_channel() {
        // No alpha channel (RGB source): opaque regardless of the filled byte.
        assert_eq!(classify_bake_alpha(3, &[10, 20, 30, 0]), BakeAlpha::Opaque);
        // Every alpha at/above the cutoff → opaque.
        assert_eq!(
            classify_bake_alpha(4, &[0, 0, 0, 255, 1, 1, 1, 200]),
            BakeAlpha::Opaque
        );
        // Every alpha below the cutoff → wholly transparent (hide the region).
        assert_eq!(
            classify_bake_alpha(4, &[9, 9, 9, 0, 9, 9, 9, 10]),
            BakeAlpha::Transparent
        );
        // A mix of kept and carved pixels → masked.
        assert_eq!(
            classify_bake_alpha(4, &[9, 9, 9, 255, 9, 9, 9, 0]),
            BakeAlpha::Masked
        );
        // No pixels at all → opaque (nothing is carved away).
        assert_eq!(classify_bake_alpha(4, &[]), BakeAlpha::Opaque);
    }

    /// Each classification maps to the right render behaviour: opaque skin stays
    /// opaque, a carved bake masks, and only a wholly transparent bake hides its
    /// region.
    #[test]
    fn bake_alpha_drives_render_mode_and_hiding() {
        assert_eq!(BakeAlpha::Opaque.alpha_mode(), AlphaMode::Opaque);
        assert!(matches!(BakeAlpha::Masked.alpha_mode(), AlphaMode::Mask(_)));
        assert!(matches!(
            BakeAlpha::Transparent.alpha_mode(),
            AlphaMode::Mask(_)
        ));
        assert!(!BakeAlpha::Opaque.hides_region());
        assert!(!BakeAlpha::Masked.hides_region());
        assert!(BakeAlpha::Transparent.hides_region());
    }

    /// Bake re-fetch is gated on the COF version (P14.4): a first appearance and
    /// any appearance without a COF version always ingest, a newer or equal COF
    /// version re-fetches, and only a strictly-older (out-of-order / duplicate)
    /// appearance is skipped.
    #[test]
    fn should_refetch_bakes_gates_on_cof_version() {
        // No COF version seen yet, or none on the appearance: always ingest.
        assert!(should_refetch_bakes(None, Some(15)));
        assert!(should_refetch_bakes(None, None));
        assert!(should_refetch_bakes(Some(15), None));
        // Newer and equal COF versions re-fetch (equal covers a same-outfit
        // rebake republishing new baked ids).
        assert!(should_refetch_bakes(Some(15), Some(16)));
        assert!(should_refetch_bakes(Some(15), Some(15)));
        // A strictly-older appearance is a stale resend and is skipped.
        assert!(!should_refetch_bakes(Some(15), Some(14)));
    }

    /// An attachment's `IMG_USE_BAKED_*` hide is attributed to the avatar it hangs
    /// off, by chasing the parent chain up (through nested linkset prims) to the
    /// avatar root; an object whose chain does not reach an avatar is ignored.
    #[test]
    fn hidden_slots_chase_the_attachment_chain_to_the_avatar() {
        let mut state = AvatarState::default();
        let agent = AgentKey::from(Uuid::from_u128(0xa5));
        let circuit = CircuitId::new(1);
        let avatar = ScopedObjectId::new(circuit, RegionLocalObjectId(100));
        let attachment = ScopedObjectId::new(circuit, RegionLocalObjectId(200));
        let child_prim = ScopedObjectId::new(circuit, RegionLocalObjectId(300));
        let orphan = ScopedObjectId::new(circuit, RegionLocalObjectId(400));

        state.by_scoped.insert(avatar, agent);
        // child prim -> attachment root -> avatar root.
        state.object_parents.insert(attachment, avatar);
        state.object_parents.insert(child_prim, attachment);
        // A deep child prim of the worn mesh replaces the upper region.
        state
            .baked_hides
            .insert(child_prim, vec![avatar_texture::UPPER_BAKED]);
        // An object whose chain does not reach any avatar is not attributed.
        state
            .baked_hides
            .insert(orphan, vec![avatar_texture::HEAD_BAKED]);

        let hidden = state.hidden_slots_per_agent();
        assert_eq!(hidden.len(), 1, "only the one avatar gets a hide set");
        let slots = hidden.get(&agent).cloned().unwrap_or_default();
        assert!(slots.contains(&avatar_texture::UPPER_BAKED));
        assert!(
            !slots.contains(&avatar_texture::HEAD_BAKED),
            "the orphan's hide must not leak onto the avatar"
        );
    }
}
