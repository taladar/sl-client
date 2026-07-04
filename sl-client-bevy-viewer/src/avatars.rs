//! Avatar placeholders: a ~2 m sphere and a floating name tag per nearby avatar.
//!
//! This is the Phase 10 slice — placeholder spheres, no rig / baked textures /
//! animation. Avatars are learned from two independent streams:
//!
//! - **full in-world objects** (`pcode` 47): the precise, per-frame position of
//!   every avatar the simulator streams as an [`Object`](sl_client_bevy::Object)
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

use bevy::prelude::*;
use sl_client_bevy::{
    AgentKey, AvatarName, CoarseLocation, Command, Object, ScopedObjectId, SlCommand, SlEvent,
    SlSessionEvent, pcode,
};

use crate::coords::sl_to_bevy_vec;

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

/// The gap, in metres, between the top of an avatar sphere and its name tag.
const NAME_TAG_GAP: f32 = 0.3;

/// The name-tag font size, in logical pixels.
const NAME_TAG_FONT_SIZE: f32 = 16.0;

/// How many leading hex characters of the agent id to show as a provisional tag
/// before the real name resolves.
const PROVISIONAL_ID_CHARS: usize = 8;

/// A marker component tagging an entity as an avatar placeholder sphere.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct AvatarSphere;

/// A `bevy_ui` name-tag text node, pointing back at the avatar sphere it floats
/// over so [`position_name_tags`] can project the sphere's world position to the
/// screen each frame.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct NameTag {
    /// The avatar sphere entity this tag labels.
    sphere: Entity,
}

/// The shared placeholder sphere mesh and material, built once and reused by
/// every avatar sphere.
struct AvatarAssets {
    /// The shared UV-sphere mesh handle.
    mesh: Handle<Mesh>,
    /// The shared soft-blue material handle.
    material: Handle<StandardMaterial>,
}

/// The pair of entities rendering one avatar: its world-space placeholder sphere
/// and its screen-space name-tag text node.
#[derive(Clone, Copy)]
struct AvatarEntities {
    /// The placeholder sphere entity.
    sphere: Entity,
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

    /// Spawn a placeholder sphere and its floating name tag for `agent` at
    /// `translation`, returning both entities.
    fn spawn_avatar(
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
            ))
            .id();
        let label = commands
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
                NameTag { sphere },
            ))
            .id();
        AvatarEntities { sphere, label }
    }

    /// Request the legacy name of `agent` once — skipped if it is already cached
    /// or already in flight.
    fn request_name(&mut self, agent: AgentKey, commands: &mut MessageWriter<SlCommand>) {
        if self.names.contains_key(&agent) || !self.requested.insert(agent) {
            return;
        }
        commands.write(SlCommand(Command::RequestAvatarNames(vec![agent])));
    }

    /// Spawn or move the placeholder sphere of a full-object avatar (`pcode` 47).
    ///
    /// A full object supersedes any coarse placeholder for the same agent (the
    /// object position is precise), so an existing coarse sphere is despawned.
    fn apply_object(
        &mut self,
        object: &Object,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
        writer: &mut MessageWriter<SlCommand>,
    ) {
        let agent = AgentKey::from(object.full_id.uuid());
        let scoped = object.scoped_id();
        let translation = sl_to_bevy_vec(&object.motion.position);
        // A precise full object takes over from any coarse dot for this agent.
        if let Some(entities) = self.coarse.remove(&agent) {
            despawn_avatar(entities, commands);
        }
        if let Some(existing) = self.objects.get(&agent) {
            commands
                .entity(existing.sphere)
                .insert(Transform::from_translation(translation));
            return;
        }
        self.request_name(agent, writer);
        let entities = self.spawn_avatar(agent, translation, commands, meshes, materials);
        self.by_scoped.insert(scoped, agent);
        self.objects.insert(agent, entities);
        debug!(
            "spawned avatar sphere for {agent} ({} tracked)",
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
                    .entity(existing.sphere)
                    .insert(Transform::from_translation(translation));
            } else {
                self.request_name(agent, writer);
                let entities = self.spawn_avatar(agent, translation, commands, meshes, materials);
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

/// Despawn both entities of an avatar placeholder (its sphere and its name tag).
fn despawn_avatar(entities: AvatarEntities, commands: &mut Commands) {
    commands.entity(entities.sphere).try_despawn();
    commands.entity(entities.label).try_despawn();
}

/// Spawn / move / despawn the placeholder of every avatar the simulator streams
/// as a full in-world object (`pcode` 47), requesting each avatar's legacy name
/// once.
pub(crate) fn update_avatar_objects(
    mut events: MessageReader<SlEvent>,
    mut state: ResMut<AvatarState>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut writer: MessageWriter<SlCommand>,
) {
    for event in events.read() {
        match &event.0 {
            SlSessionEvent::ObjectAdded(object) | SlSessionEvent::ObjectUpdated(object) => {
                if object.pcode == pcode::AVATAR {
                    state.apply_object(
                        object,
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

/// Position each avatar name tag over its sphere by projecting the sphere's world
/// position (offset up by the sphere radius plus a small gap) to the screen and
/// anchoring the tag's *bottom-centre* on that point, so the text is centred over
/// the sphere and floats just above it; tags whose sphere is off-screen or behind
/// the camera are hidden.
///
/// The projection ([`Camera::world_to_viewport`](sl_client_bevy::Camera)) and the
/// UI `Val::Px` layout are both in logical pixels, but [`ComputedNode::size`] is
/// physical, so the tag's own size is scaled by its
/// [`inverse_scale_factor`](ComputedNode::inverse_scale_factor) before centring.
pub(crate) fn position_name_tags(
    cameras: Query<(&Camera, &GlobalTransform)>,
    spheres: Query<&GlobalTransform, With<AvatarSphere>>,
    mut tags: Query<(&NameTag, &ComputedNode, &mut Node, &mut Visibility)>,
) {
    let Ok((camera, camera_transform)) = cameras.single() else {
        return;
    };
    for (tag, computed, mut node, mut visibility) in &mut tags {
        let Ok(sphere) = spheres.get(tag.sphere) else {
            *visibility = Visibility::Hidden;
            continue;
        };
        let base = sphere.translation();
        // Float the tag just above the sphere's top (per-component add to avoid the
        // `arithmetic_side_effects` lint on the glam `Vec3` operator).
        let head = Vec3::new(base.x, base.y + AVATAR_SPHERE_RADIUS + NAME_TAG_GAP, base.z);
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
    use super::{PROVISIONAL_ID_CHARS, coarse_translation, provisional_label};
    use bevy::math::Vec3;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{AgentKey, CoarseLocation, Uuid};

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
}
