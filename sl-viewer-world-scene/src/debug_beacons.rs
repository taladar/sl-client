//! The in-world **debug beacons**: the coloured cross markers a floater asks for
//! while it is open, drawn at a place it is talking about — the reference viewer's
//! `LLViewerObjectList::addDebugBeacon` / `renderObjectBeacons`.
//!
//! The telehub floater (`sl-viewer-places`, a crate above this one, so it is
//! named rather than linked) is the first caller: it marks the region's telehub
//! in yellow and the spawn point selected in its list in orange, for exactly as
//! long as its window is open. The reference's other callers (area search, the
//! pathfinding lists) are the same shape — an open window, a chosen object, a
//! colour — which is why the *asking* side is a shared resource
//! ([`DebugBeacons`]) rather than anything telehub-specific.
//!
//! # What one looks like
//!
//! The reference draws each beacon twice, and this does the same:
//!
//! - a **see-through** pass with the depth test off and the colour's alpha
//!   quartered — a tall cross (±2 m across, ±50 m up and down) that stays visible
//!   through the very object it marks, which is the whole point of a marker on a
//!   prim you are standing next to;
//! - a **solid** pass, depth-tested at full alpha — a small cross (±0.5 m) and a
//!   little cube at the point itself, so the exact spot reads sharply where
//!   nothing is in the way.
//!
//! One deliberate divergence: the reference draws both as `GL_LINES` at a 3–4
//! pixel width, and `wgpu` has no line width at all (the feature is not in the
//! WebGPU spec, and a 1-pixel cross fifty metres up is invisible). Each arm is
//! therefore a thin **box** instead — world-space rather than screen-space
//! thickness, which reads the same close up and thins out with distance the way
//! the rest of the scene does.
//!
//! # Following the object
//!
//! A beacon usually marks an *object*, and an object moves. As in the reference's
//! `LLFloaterTelehub::addBeacons`, a beacon naming an [`anchor`] is placed from
//! the **live** object each frame and falls back to the position the asking
//! floater cached only while that object is not in the scene — out of draw
//! distance, not streamed yet, or gone. The beacon's offset is applied in the
//! anchor's own frame (the reference's `hub_pos_region + spawn_pos * hub_rot`),
//! which is how a telehub spawn point — stored relative to the hub, and rotating
//! with it — lands where the simulator will actually put an arriving avatar.
//!
//! [`anchor`]: DebugBeacon::anchor
//!
//! Reference (Firestorm, read-only): `llviewerobjectlist.cpp`
//! (`addDebugBeacon`), `llglsandbox.cpp` (`renderObjectBeacons`,
//! `draw_cross_lines`, `draw_line_cube`), `llfloatertelehub.cpp` (`addBeacons`).

use std::collections::HashSet;

use bevy::asset::{Asset, RenderAssetUsages, load_internal_asset, uuid_handle};
use bevy::mesh::{Indices, MeshVertexBufferLayoutRef, PrimitiveTopology};
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::reflect::TypePath;
use bevy::render::render_resource::{
    AsBindGroup, CompareFunction, RenderPipelineDescriptor, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;

use sl_client_bevy::{ObjectKey, RegionHandle, SlIdentity};

use crate::coords::{region_offset_bevy, sl_to_bevy_object_rotation, sl_to_bevy_vec};
use crate::world_api::{DebugBeacon, DebugBeacons, ObjectState};

/// The internal handle the marker shader (`debug_beacon.wgsl`) is loaded under, so
/// the material can reference it without an on-disk asset path.
const DEBUG_BEACON_SHADER_HANDLE: Handle<Shader> =
    uuid_handle!("6b1d9f42-0c58-4a7e-9f23-1d8e57c0a4b6");

/// The see-through cross's half-extent across the ground plane, in metres — the
/// reference's `draw_cross_lines(pos, 2.0f, 2.0f, 50.f)` horizontal arms.
const THROUGH_HALF_ACROSS_M: f32 = 2.0;

/// The see-through cross's half-height, in metres (the same call's vertical arm):
/// a hundred-metre bar through the marked point, so the marker is findable from
/// the ground when the thing it marks is up a tower, and from the air when it is
/// under a roof.
const THROUGH_HALF_UP_M: f32 = 50.0;

/// The solid cross's half-extent on every axis, in metres — the reference's
/// `draw_cross_lines(pos, 0.5f, 0.5f, 0.5f)`.
const SOLID_HALF_M: f32 = 0.5;

/// The solid pass's little cube half-size, in metres — the reference's
/// `draw_line_cube(0.10f, pos)`, which pins the exact point.
const SOLID_CUBE_HALF_M: f32 = 0.10;

/// Half the thickness of a see-through arm, in metres. The reference's 4-pixel
/// line has no `wgpu` equivalent (see the [module documentation](self)); this is
/// the world-space stand-in — wide enough that the tall bar is still a couple of
/// pixels at the far side of a region.
const THROUGH_ARM_HALF_M: f32 = 0.09;

/// Half the thickness of a solid arm, in metres — thinner than the see-through
/// one, because it is only ever read from close by.
const SOLID_ARM_HALF_M: f32 = 0.03;

/// The factor the see-through pass scales the beacon's alpha by — the reference's
/// `color.mV[3] *= 0.25f` on its no-depth pass.
const THROUGH_ALPHA_SCALE: f32 = 0.25;

// ---------------------------------------------------------------------------
// The marker material.
// ---------------------------------------------------------------------------

/// Which of a beacon's two passes a material draws — the specialisation key, since
/// the passes differ only in their depth compare.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct DebugBeaconKey {
    /// Whether this is the see-through pass (depth test off).
    through: bool,
}

/// A tiny unlit, alpha-blended material for one debug-beacon pass: the marker's
/// tint and alpha ride the `color` uniform, and `through` picks the depth compare
/// in [`specialize`](Material::specialize).
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
#[bind_group_data(DebugBeaconKey)]
pub(crate) struct DebugBeaconMaterial {
    /// The marker's RGB tint (`rgb`) and alpha (`a`).
    #[uniform(0)]
    color: Vec4,
    /// Whether this material draws the see-through pass.
    through: bool,
}

impl From<&DebugBeaconMaterial> for DebugBeaconKey {
    /// The pass is the whole specialisation.
    fn from(material: &DebugBeaconMaterial) -> Self {
        Self {
            through: material.through,
        }
    }
}

impl Material for DebugBeaconMaterial {
    /// The bundled marker shader carries the mesh through the standard transform.
    fn vertex_shader() -> ShaderRef {
        ShaderRef::Handle(DEBUG_BEACON_SHADER_HANDLE)
    }

    /// The bundled marker shader emits the uniform tint.
    fn fragment_shader() -> ShaderRef {
        ShaderRef::Handle(DEBUG_BEACON_SHADER_HANDLE)
    }

    /// Alpha-blended: a marker is a translucent coloured overlay, so it sorts in
    /// the transparent phase and writes no depth.
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }

    /// No depth / normal prepass: the mesh carries only positions, and a
    /// translucent overlay belongs in neither prepass.
    fn enable_prepass() -> bool {
        false
    }

    /// A marker casts no shadows (an overlay, not solid geometry).
    fn enable_shadows() -> bool {
        false
    }

    /// Pin the vertex layout to positions alone, draw both faces (a marker arm is
    /// read from every side), keep the marker out of the glow mask so it does not
    /// bloom, and — for the see-through pass — disable the depth test the way the
    /// reference's first beacon pass does.
    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let vertex_layout = layout
            .0
            .get_layout(&[Mesh::ATTRIBUTE_POSITION.at_shader_location(0)])?;
        descriptor.vertex.buffers = vec![vertex_layout];
        descriptor.primitive.cull_mode = None;
        if key.bind_group_data.through
            && let Some(depth_stencil) = descriptor.depth_stencil.as_mut()
        {
            depth_stencil.depth_compare = Some(CompareFunction::Always);
        }
        sl_client_bevy::preserve_glow_mask_alpha(descriptor);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// The marker meshes.
// ---------------------------------------------------------------------------

/// The shared marker meshes and the per-colour material cache.
#[derive(Resource)]
pub(crate) struct DebugBeaconAssets {
    /// The see-through pass's mesh: the tall, wide cross.
    through: Handle<Mesh>,
    /// The solid pass's mesh: the small cross and the point cube.
    solid: Handle<Mesh>,
    /// One material per (quantised colour, pass), created on first use.
    materials: HashMap<([u8; 4], bool), Handle<DebugBeaconMaterial>>,
}

impl FromWorld for DebugBeaconAssets {
    /// Build the two shared marker meshes once.
    fn from_world(world: &mut World) -> Self {
        let mut meshes = world.resource_mut::<Assets<Mesh>>();
        let through = meshes.add(cross_mesh(
            [
                THROUGH_HALF_ACROSS_M,
                THROUGH_HALF_ACROSS_M,
                THROUGH_HALF_UP_M,
            ],
            THROUGH_ARM_HALF_M,
            None,
        ));
        let solid = meshes.add(cross_mesh(
            [SOLID_HALF_M, SOLID_HALF_M, SOLID_HALF_M],
            SOLID_ARM_HALF_M,
            Some(SOLID_CUBE_HALF_M),
        ));
        Self {
            through,
            solid,
            materials: HashMap::default(),
        }
    }
}

impl DebugBeaconAssets {
    /// The shared material for a colour and pass, created on first use. The colour
    /// is quantised to bytes for the cache key (a handful of distinct marker
    /// colours across the whole viewer).
    fn material_for(
        &mut self,
        color: LinearRgba,
        through: bool,
        materials: &mut Assets<DebugBeaconMaterial>,
    ) -> Handle<DebugBeaconMaterial> {
        let alpha = if through {
            color.alpha * THROUGH_ALPHA_SCALE
        } else {
            color.alpha
        };
        let key = (
            [
                quantise(color.red),
                quantise(color.green),
                quantise(color.blue),
                quantise(alpha),
            ],
            through,
        );
        if let Some(handle) = self.materials.get(&key) {
            return handle.clone();
        }
        let handle = materials.add(DebugBeaconMaterial {
            color: Vec4::new(color.red, color.green, color.blue, alpha),
            through,
        });
        self.materials.insert(key, handle.clone());
        handle
    }
}

/// Quantise a `0..=1` colour component to a byte for the material cache key.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a clamped 0..=255 value is whole after rounding; only used as a HashMap key"
)]
fn quantise(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// Build a marker mesh: three thin boxes along the Bevy axes reaching `half`
/// metres each way (`[across, across, up]` in **Second Life** axis order — x and y
/// across the ground, z up — so the constants read as the reference writes them),
/// each `arm_half` metres thick, plus an optional solid cube of half-size `cube` at
/// the centre.
fn cross_mesh(half: [f32; 3], arm_half: f32, cube: Option<f32>) -> Mesh {
    let [across_x, across_y, up] = half;
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    // Bevy axes: x is Second Life x (east), y is up, z is south (−y). Each arm is a
    // box whose half-extents are the arm length on its own axis and the arm
    // thickness on the other two.
    push_box(&mut positions, &mut indices, [across_x, arm_half, arm_half]);
    push_box(&mut positions, &mut indices, [arm_half, up, arm_half]);
    push_box(&mut positions, &mut indices, [arm_half, arm_half, across_y]);
    if let Some(cube) = cube {
        push_box(&mut positions, &mut indices, [cube, cube, cube]);
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// Append an origin-centred box of the given half-extents to a mesh under
/// construction.
fn push_box(positions: &mut Vec<[f32; 3]>, indices: &mut Vec<u32>, half: [f32; 3]) {
    let [hx, hy, hz] = half;
    let base = u32::try_from(positions.len()).unwrap_or(0);
    positions.extend_from_slice(&[
        [-hx, -hy, -hz],
        [hx, -hy, -hz],
        [hx, hy, -hz],
        [-hx, hy, -hz],
        [-hx, -hy, hz],
        [hx, -hy, hz],
        [hx, hy, hz],
        [-hx, hy, hz],
    ]);
    // The six faces, wound consistently; the material draws both sides anyway.
    for face in [
        [0_u32, 1, 2, 0, 2, 3],
        [4, 6, 5, 4, 7, 6],
        [0, 4, 5, 0, 5, 1],
        [3, 2, 6, 3, 6, 7],
        [0, 3, 7, 0, 7, 4],
        [1, 5, 6, 1, 6, 2],
    ] {
        indices.extend(face.into_iter().map(|index| base.saturating_add(index)));
    }
}

// ---------------------------------------------------------------------------
// Placing the markers.
// ---------------------------------------------------------------------------

/// Where one beacon sits this frame, and how it is turned.
#[derive(Debug, Clone, Copy, PartialEq)]
struct PlacedBeacon {
    /// The marker's Bevy world position.
    position: Vec3,
    /// The marker's colour.
    color: LinearRgba,
}

/// Place `beacon` in Bevy world space: from the live anchor object when the scene
/// has it, else from the position the asking floater cached, with the offset
/// applied in whichever frame was used.
///
/// # The offset stays in Second Life space
///
/// The rotation on both paths is a **root object's** world rotation, which
/// composes the Second Life → Bevy basis change with the object's own
/// orientation ([`sl_to_bevy_object_rotation`]) — so the space it rotates *from*
/// is Second Life's, not Bevy's. The offset is therefore handed to it raw, and
/// the single basis change comes out of the rotation itself. Converting the
/// offset first ([`sl_to_bevy_vec`]) and then rotating applies the basis change
/// twice, which puts a marker on the wrong side of the object it is offset from
/// — how this was found, with an unrotated telehub and a spawn point that landed
/// nowhere near the prim it was recorded at.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "finite world-space scene geometry; the glam operators are the readable form"
)]
fn place_beacon(
    beacon: &DebugBeacon,
    anchors: &HashMap<ObjectKey, (Vec3, Quat)>,
    origin: Option<RegionHandle>,
) -> PlacedBeacon {
    let (base, rotation) = beacon
        .anchor
        .and_then(|anchor| anchors.get(&anchor).copied())
        .unwrap_or_else(|| {
            (
                region_offset_bevy(beacon.region, origin) + sl_to_bevy_vec(&beacon.position),
                sl_to_bevy_object_rotation(&beacon.rotation),
            )
        });
    let offset = Vec3::new(beacon.offset.x, beacon.offset.y, beacon.offset.z);
    PlacedBeacon {
        position: base + rotation * offset,
        color: beacon.color.to_linear(),
    }
}

/// One spawned marker: the two pass entities, pooled across frames so a selection
/// change re-points a marker rather than churning entities.
#[derive(Debug, Clone, Copy)]
struct MarkerEntities {
    /// The see-through pass entity.
    through: Entity,
    /// The solid pass entity.
    solid: Entity,
}

/// The renderer's pool of spawned markers.
#[derive(Resource, Default)]
pub(crate) struct DebugBeaconState {
    /// The spawned markers, reused in order; never shrunk, only hidden.
    markers: Vec<MarkerEntities>,
}

/// Drive the in-world markers from [`DebugBeacons`]: resolve each beacon's live
/// anchor, place it, and point the pooled marker entities at the results, hiding
/// the surplus.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the asked-for \
              beacons, the object table and identity that resolve an anchor, the shared assets \
              and material store, this feature's pool, the transform query and the command \
              buffer to spawn a marker"
)]
fn update_debug_beacons(
    beacons: Res<DebugBeacons>,
    objects: Res<ObjectState>,
    identity: Res<SlIdentity>,
    globals: Query<&GlobalTransform>,
    mut assets: ResMut<DebugBeaconAssets>,
    mut materials: ResMut<Assets<DebugBeaconMaterial>>,
    mut state: ResMut<DebugBeaconState>,
    mut placed_query: Query<(&mut Transform, &mut Visibility)>,
    mut commands: Commands,
) {
    // Resolve every distinct anchor in one pass over the object table, rather than
    // a scan per beacon (`ObjectState::scoped_by_full_keys` exists for exactly
    // this).
    let wanted: HashSet<ObjectKey> = beacons.iter().filter_map(|beacon| beacon.anchor).collect();
    let mut anchors: HashMap<ObjectKey, (Vec3, Quat)> = HashMap::default();
    for (key, scoped) in objects.scoped_by_full_keys(&wanted) {
        if let Some(tracked) = objects.objects.get(&scoped)
            && let Ok(global) = globals.get(tracked.entity)
        {
            let (_scale, rotation, translation) = global.to_scale_rotation_translation();
            anchors.insert(key, (translation, rotation));
        }
    }
    let origin = objects.origin.or(identity.region_handle);
    let placed: Vec<PlacedBeacon> = beacons
        .iter()
        .map(|beacon| place_beacon(beacon, &anchors, origin))
        .collect();

    // Re-point the pooled markers, spawning the ones this frame is short of
    // already placed — a marker spawned blank and moved next frame would flash at
    // the scene origin on the frame the window opens.
    for (index, beacon) in placed.iter().enumerate() {
        let through_material = assets.material_for(beacon.color, true, &mut materials);
        let solid_material = assets.material_for(beacon.color, false, &mut materials);
        let transform = Transform::from_translation(beacon.position);
        if let Some(marker) = state.markers.get(index).copied() {
            for (entity, material) in [
                (marker.through, through_material),
                (marker.solid, solid_material),
            ] {
                commands.entity(entity).insert(MeshMaterial3d(material));
                if let Ok((mut placed_transform, mut visibility)) = placed_query.get_mut(entity) {
                    placed_transform.translation = beacon.position;
                    visibility.set_if_neq(Visibility::Visible);
                }
            }
            continue;
        }
        let through = commands
            .spawn((
                Mesh3d(assets.through.clone()),
                MeshMaterial3d(through_material),
                transform,
                Visibility::Visible,
                Name::new("debug-beacon:through"),
            ))
            .id();
        let solid = commands
            .spawn((
                Mesh3d(assets.solid.clone()),
                MeshMaterial3d(solid_material),
                transform,
                Visibility::Visible,
                Name::new("debug-beacon:solid"),
            ))
            .id();
        state.markers.push(MarkerEntities { through, solid });
    }

    // Hide the surplus: a window that dropped a marker keeps its entity pooled.
    for marker in state.markers.iter().skip(placed.len()) {
        for entity in [marker.through, marker.solid] {
            if let Ok((_transform, mut visibility)) = placed_query.get_mut(entity) {
                visibility.set_if_neq(Visibility::Hidden);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Plugin.
// ---------------------------------------------------------------------------

/// The debug-beacon plugin: loads the marker shader, registers the marker
/// material, and drives the pooled markers from [`DebugBeacons`].
#[derive(Debug, Default)]
pub struct DebugBeaconPlugin;

impl Plugin for DebugBeaconPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(
            app,
            DEBUG_BEACON_SHADER_HANDLE,
            "debug_beacon.wgsl",
            Shader::from_wgsl
        );
        app.add_plugins(MaterialPlugin::<DebugBeaconMaterial>::default())
            .init_resource::<DebugBeacons>()
            .init_resource::<DebugBeaconAssets>()
            .init_resource::<DebugBeaconState>()
            .add_systems(Update, update_debug_beacons.run_if(beacons_wanted_or_shown));
    }
}

/// Run condition: something is asking for a beacon, or a marker is still shown and
/// has to be taken down. Keeps a viewer with no open marker-using window out of
/// the object-table pass entirely.
fn beacons_wanted_or_shown(beacons: Res<DebugBeacons>, state: Res<DebugBeaconState>) -> bool {
    !beacons.is_empty() || !state.markers.is_empty()
}

#[cfg(test)]
mod tests {
    use bevy::platform::collections::HashMap;
    use bevy::prelude::{Color, Vec3};
    use sl_client_bevy::{ObjectKey, RegionHandle, Rotation, Uuid, Vector};

    use super::{DebugBeacon, cross_mesh, place_beacon, push_box, sl_to_bevy_object_rotation};

    /// Absolute-difference float check (the workspace forbids bare `==` on floats).
    fn near(a: f32, b: f32, eps: f32) {
        assert!((a - b).abs() <= eps, "{a} not within {eps} of {b}");
    }

    /// A Second Life vector.
    const fn vector(x: f32, y: f32, z: f32) -> Vector {
        Vector { x, y, z }
    }

    /// The identity rotation.
    const fn identity() -> Rotation {
        Rotation {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            s: 1.0,
        }
    }

    /// A beacon at a region-local position, with no anchor.
    fn beacon_at(position: Vector, offset: Vector, rotation: Rotation) -> DebugBeacon {
        DebugBeacon {
            region: RegionHandle::new((1_000_000_u64 << 32) | 2_000_000_u64),
            anchor: None,
            position,
            rotation,
            offset,
            color: Color::WHITE,
        }
    }

    /// With no anchor known, the beacon sits at its cached region-local position,
    /// converted into Bevy space against the scene origin.
    #[test]
    fn an_unanchored_beacon_uses_its_cached_position() {
        let origin = RegionHandle::new((1_000_000_u64 << 32) | 2_000_000_u64);
        let beacon = beacon_at(vector(128.0, 64.0, 25.0), vector(0.0, 0.0, 0.0), identity());
        let placed = place_beacon(&beacon, &HashMap::default(), Some(origin));
        // Second Life (x east, y north, z up) → Bevy (x east, y up, z south).
        near(placed.position.x, 128.0, 1e-3);
        near(placed.position.y, 25.0, 1e-3);
        near(placed.position.z, -64.0, 1e-3);
    }

    /// An **unrotated** anchor's offset is the plain Second Life → Bevy
    /// conversion of the offset, added to where the object is. This is the case
    /// that catches a basis change applied twice: convert the offset *and* then
    /// rotate by the object's world rotation — which already carries the basis
    /// change — and a spawn point 4 m north of the hub lands 4 m below it.
    #[test]
    fn an_unrotated_anchor_offsets_in_second_life_axes() {
        let origin = RegionHandle::new((1_000_000_u64 << 32) | 2_000_000_u64);
        let anchor = ObjectKey::from(Uuid::from_u128(0x1234));
        let mut beacon = beacon_at(vector(10.0, 10.0, 10.0), vector(0.0, 4.0, 1.0), identity());
        beacon.anchor = Some(anchor);
        let mut anchors = HashMap::default();
        // An object with no rotation of its own: its world rotation is the bare
        // Second Life → Bevy basis change, as `sl_to_bevy_object_rotation` builds
        // it for any root object.
        anchors.insert(
            anchor,
            (
                Vec3::new(50.0, 30.0, -20.0),
                sl_to_bevy_object_rotation(&identity()),
            ),
        );
        let placed = place_beacon(&beacon, &anchors, Some(origin));
        // 4 m north and 1 m up of the object → Bevy (+0, +1, −4).
        near(placed.position.x, 50.0, 1e-3);
        near(placed.position.y, 31.0, 1e-3);
        near(placed.position.z, -24.0, 1e-3);
    }

    /// A known anchor wins over the cached position, and the offset turns with
    /// the object — the reference's `hub_pos + spawn_pos * hub_rot`.
    #[test]
    fn an_anchored_beacon_follows_the_object_and_rotates_its_offset() {
        let origin = RegionHandle::new((1_000_000_u64 << 32) | 2_000_000_u64);
        let anchor = ObjectKey::from(Uuid::from_u128(0x1234));
        let mut beacon = beacon_at(vector(10.0, 10.0, 10.0), vector(2.0, 0.0, 0.0), identity());
        beacon.anchor = Some(anchor);
        let mut anchors = HashMap::default();
        // The object is yawed a quarter turn about Second Life up, which takes
        // the offset's +x (east) round to +y (north) — Bevy −z.
        let yawed = Rotation {
            x: 0.0,
            y: 0.0,
            z: core::f32::consts::FRAC_1_SQRT_2,
            s: core::f32::consts::FRAC_1_SQRT_2,
        };
        anchors.insert(
            anchor,
            (
                Vec3::new(50.0, 30.0, -20.0),
                sl_to_bevy_object_rotation(&yawed),
            ),
        );
        let placed = place_beacon(&beacon, &anchors, Some(origin));
        near(placed.position.x, 50.0, 1e-3);
        near(placed.position.y, 30.0, 1e-3);
        near(placed.position.z, -22.0, 1e-3);
    }

    /// An anchor the scene does not have falls back to the cached position, so a
    /// telehub out of draw distance is still marked where the simulator said.
    #[test]
    fn an_unknown_anchor_falls_back() {
        let origin = RegionHandle::new((1_000_000_u64 << 32) | 2_000_000_u64);
        let mut beacon = beacon_at(vector(8.0, 4.0, 2.0), vector(0.0, 0.0, 0.0), identity());
        beacon.anchor = Some(ObjectKey::from(Uuid::from_u128(0x4321)));
        let placed = place_beacon(&beacon, &HashMap::default(), Some(origin));
        near(placed.position.x, 8.0, 1e-3);
        near(placed.position.y, 2.0, 1e-3);
        near(placed.position.z, -4.0, 1e-3);
    }

    /// Each arm (and the optional cube) contributes one box: eight vertices and
    /// twelve triangles, indexed from its own base.
    #[test]
    fn a_marker_mesh_is_one_box_per_arm() {
        let mut positions = Vec::new();
        let mut indices = Vec::new();
        push_box(&mut positions, &mut indices, [1.0, 1.0, 1.0]);
        pretty_assertions::assert_eq!(positions.len(), 8);
        pretty_assertions::assert_eq!(indices.len(), 36);
        let three_arms = cross_mesh([2.0, 2.0, 50.0], 0.1, None);
        pretty_assertions::assert_eq!(three_arms.count_vertices(), 24);
        let with_cube = cross_mesh([0.5, 0.5, 0.5], 0.03, Some(0.1));
        pretty_assertions::assert_eq!(with_cube.count_vertices(), 32);
    }
}
