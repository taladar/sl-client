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
    AgentKey, AvatarName, CoarseLocation, Command, Object, ScopedObjectId, SlCommand, SlEvent,
    SlSessionEvent, pcode, to_bevy_base_mesh,
};

use crate::avatar_assets::{AvatarAssetLibrary, LoadedBinding};
use crate::coords::{sl_to_bevy_object_rotation, sl_to_bevy_vec};

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
    /// The shared placeholder sphere mesh + material, built lazily on first use.
    assets: Option<AvatarAssets>,
}

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
        parts.push(BodyPart { mesh, binding });
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
fn spawn_body_part(
    part: &BodyPart,
    joints: &[Entity],
    root: Entity,
    material: &Handle<StandardMaterial>,
    commands: &mut Commands,
) {
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
                SkinnedMesh {
                    inverse_bindposes: inverse_bindposes.clone(),
                    joints: part_joints,
                },
                ChildOf(root),
            ));
        }
        BodyPartBinding::Rigid(index) => {
            let Some(joint) = joints.get(*index).copied() else {
                return;
            };
            commands.spawn((
                Mesh3d(part.mesh.clone()),
                MeshMaterial3d(material.clone()),
                Transform::default(),
                ChildOf(joint),
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
        // first keeps the parenting simple).
        let joints: Vec<Entity> = body
            .joint_locals
            .iter()
            .map(|local| commands.spawn((*local, Visibility::default())).id())
            .collect();
        for (entity, parent) in joints.iter().zip(body.joint_parents.iter().copied()) {
            let target = parent
                .and_then(|index| joints.get(index).copied())
                .unwrap_or(root);
            commands.entity(*entity).insert(ChildOf(target));
        }
        for part in &body.parts {
            spawn_body_part(part, &joints, root, &body.material, commands);
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
    use super::{PROVISIONAL_ID_CHARS, body_root_transform, coarse_translation, provisional_label};
    use crate::coords::sl_to_bevy_rotation;
    use bevy::math::Vec3;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{
        AgentKey, CircuitId, CoarseLocation, Object, ObjectMotion, RegionHandle,
        RegionLocalObjectId, Rotation, Uuid, Vector,
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
}
