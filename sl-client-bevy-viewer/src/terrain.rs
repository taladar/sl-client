//! Terrain rendering: turn each decoded `LayerData` height patch into a mesh.
//!
//! This is the Phase 2 slice. On every [`SlSessionEvent::TerrainPatch`] for a
//! land (ground-height) layer the viewer builds a `size`×`size` heightfield
//! mesh — one vertex per patch cell, at that cell's decoded ground height, with
//! per-vertex normals from a central-difference of the neighbouring heights and
//! whole-region UVs — and places it at the patch origin. A
//! [`HashMap`] keyed by the patch grid position holds the spawned entity so a
//! later refresh of the same patch replaces its mesh in place rather than
//! stacking a second copy.
//!
//! The geometry is built in Second Life space (Z-up, relative to the patch
//! origin cell) and oriented by the entity `Transform` via
//! [`sl_to_bevy_rotation`], mirroring how the geometry crates stay in Second
//! Life space and convert only at the entity boundary. One flat
//! [`StandardMaterial`] covers the whole region — no per-layer texture
//! splatting (a non-goal of the minimum-viable viewer).

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use sl_client_bevy::{SlEvent, SlSessionEvent, TerrainPatch, Vector};

use crate::coords::{sl_to_bevy_rotation, sl_to_bevy_vec};

/// The region edge length in metres, used to spread the whole-region terrain
/// UVs. A standard Second Life / OpenSim region is 256 m (16×16 patches of
/// 16×16 cells).
const REGION_SIZE_METRES: f32 = 256.0;

/// The flat base colour of the terrain material — a muted olive so the ground
/// reads clearly without any splat textures.
const TERRAIN_BASE_COLOR: Color = Color::srgb(0.36, 0.44, 0.28);

/// Viewer-side terrain bookkeeping: the entity rendering each patch, keyed by
/// its `(patch_x, patch_y)` grid position, plus the shared ground material
/// (built on first use).
#[derive(Resource, Default)]
pub(crate) struct TerrainState {
    /// The rendered entity for each patch grid position; a repeat patch replaces
    /// the entity's mesh rather than spawning a second one.
    patches: HashMap<(u32, u32), Entity>,
    /// The shared flat ground material, created lazily on the first patch.
    material: Option<Handle<StandardMaterial>>,
}

/// Fold terrain-patch events into the scene: build (or rebuild) the heightfield
/// mesh for each land patch and spawn or replace its entity.
pub(crate) fn update_terrain(
    mut events: MessageReader<SlEvent>,
    mut state: ResMut<TerrainState>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
) {
    for event in events.read() {
        let SlSessionEvent::TerrainPatch(patch) = &event.0 else {
            continue;
        };
        // Only ground-height layers are rendered; wind / cloud / water patches
        // ride the same event but carry no terrain.
        if !patch.layer.is_land() {
            continue;
        }
        let material = state
            .material
            .get_or_insert_with(|| {
                materials.add(StandardMaterial {
                    base_color: TERRAIN_BASE_COLOR,
                    perceptual_roughness: 1.0,
                    // A debug viewer looks at terrain from any side; keep both
                    // faces lit rather than culling the underside.
                    double_sided: true,
                    cull_mode: None,
                    ..default()
                })
            })
            .clone();
        let mesh = meshes.add(build_patch_mesh(patch));
        let origin = Vector {
            x: patch_coord_f32(patch.patch_x.saturating_mul(patch.size)),
            y: patch_coord_f32(patch.patch_y.saturating_mul(patch.size)),
            z: 0.0,
        };
        let transform = Transform {
            translation: sl_to_bevy_vec(&origin),
            rotation: sl_to_bevy_rotation(),
            ..default()
        };
        let key = (patch.patch_x, patch.patch_y);
        match state.patches.get(&key).copied() {
            Some(entity) => {
                commands
                    .entity(entity)
                    .insert((Mesh3d(mesh), MeshMaterial3d(material), transform));
            }
            None => {
                let entity = commands
                    .spawn((Mesh3d(mesh), MeshMaterial3d(material), transform))
                    .id();
                state.patches.insert(key, entity);
                debug!(
                    "spawned terrain patch ({}, {}) ({} rendered)",
                    key.0,
                    key.1,
                    state.patches.len()
                );
            }
        }
    }
}

/// Build a Bevy heightfield [`Mesh`] for one terrain patch, in Second Life
/// space relative to the patch origin cell: `size`×`size` vertices (cell `x`,
/// `y` at local position `(x, y, height)`), computed normals, whole-region UVs,
/// and two triangles per cell quad.
fn build_patch_mesh(patch: &TerrainPatch) -> Mesh {
    let size = patch.size;
    let region_origin_x = patch.patch_x.saturating_mul(size);
    let region_origin_y = patch.patch_y.saturating_mul(size);
    let capacity = usize::try_from(size.saturating_mul(size)).unwrap_or(0);
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(capacity);
    let mut normals: Vec<[f32; 3]> = Vec::with_capacity(capacity);
    let mut uvs: Vec<[f32; 2]> = Vec::with_capacity(capacity);
    for y in 0..size {
        for x in 0..size {
            let height = patch.value(x, y).unwrap_or(0.0);
            positions.push([patch_coord_f32(x), patch_coord_f32(y), height]);
            normals.push(patch_normal(patch, x, y));
            let u = (patch_coord_f32(region_origin_x) + patch_coord_f32(x)) / REGION_SIZE_METRES;
            let v = (patch_coord_f32(region_origin_y) + patch_coord_f32(y)) / REGION_SIZE_METRES;
            uvs.push([u, v]);
        }
    }
    let mut indices: Vec<u32> = Vec::new();
    for y in 0..size.saturating_sub(1) {
        for x in 0..size.saturating_sub(1) {
            let x1 = x.saturating_add(1);
            let y1 = y.saturating_add(1);
            let i00 = vertex_index(size, x, y);
            let i10 = vertex_index(size, x1, y);
            let i01 = vertex_index(size, x, y1);
            let i11 = vertex_index(size, x1, y1);
            // Wound counter-clockwise viewed from above (+Z), so the upward
            // normals match the front face.
            indices.extend_from_slice(&[i00, i10, i11, i00, i11, i01]);
        }
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// The flat vertex-buffer index of patch cell (`x`, `y`) in a `size`-wide,
/// row-major grid.
const fn vertex_index(size: u32, x: u32, y: u32) -> u32 {
    y.saturating_mul(size).saturating_add(x)
}

/// The upward-facing unit normal at patch cell (`x`, `y`), from a
/// central-difference of the neighbouring ground heights (one-sided at the
/// patch edges, where the adjacent patch's cells are not part of this mesh).
fn patch_normal(patch: &TerrainPatch, x: u32, y: u32) -> [f32; 3] {
    let size = patch.size;
    let left_x = if x > 0 { x.saturating_sub(1) } else { x };
    let right_x = if x.saturating_add(1) < size {
        x.saturating_add(1)
    } else {
        x
    };
    let down_y = if y > 0 { y.saturating_sub(1) } else { y };
    let up_y = if y.saturating_add(1) < size {
        y.saturating_add(1)
    } else {
        y
    };
    // Height differences over the cell span between the sampled neighbours (2
    // cells interior, 1 at an edge), guarded to at least one cell so a 1×1 or
    // degenerate patch cannot divide by zero.
    let span_x = patch_coord_f32(right_x.saturating_sub(left_x)).max(1.0);
    let span_y = patch_coord_f32(up_y.saturating_sub(down_y)).max(1.0);
    let dz_dx =
        (patch.value(right_x, y).unwrap_or(0.0) - patch.value(left_x, y).unwrap_or(0.0)) / span_x;
    let dz_dy =
        (patch.value(x, up_y).unwrap_or(0.0) - patch.value(x, down_y).unwrap_or(0.0)) / span_y;
    // The surface `z = h(x, y)` has upward normal `(-dz/dx, -dz/dy, 1)`.
    let nx = -dz_dx;
    let ny = -dz_dy;
    let nz = 1.0_f32;
    let length = (nx * nx + ny * ny + nz * nz).sqrt();
    [nx / length, ny / length, nz / length]
}

/// Convert a small patch/region cell coordinate (well under `u16::MAX` for any
/// region) to `f32`; there is no `From<u32>` for `f32`, and the value is exact.
fn patch_coord_f32(value: u32) -> f32 {
    f32::from(u16::try_from(value).unwrap_or(u16::MAX))
}

#[cfg(test)]
mod tests {
    use super::{build_patch_mesh, patch_normal};
    use bevy::mesh::{Indices, Mesh, VertexAttributeValues};
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{TerrainLayerType, TerrainPatch};

    /// A land patch of the given edge size whose height is `f(x, y)`.
    fn land_patch(size: u32, mut height: impl FnMut(u32, u32) -> f32) -> TerrainPatch {
        let mut values = Vec::new();
        for y in 0..size {
            for x in 0..size {
                values.push(height(x, y));
            }
        }
        TerrainPatch {
            region_handle: sl_client_bevy::RegionHandle::default(),
            layer: TerrainLayerType::Land,
            patch_x: 1,
            patch_y: 2,
            size,
            values,
        }
    }

    /// A flat patch yields `size²` vertices, two triangles per cell quad, and
    /// straight-up normals.
    #[test]
    fn flat_patch_is_a_flat_grid() {
        let patch = land_patch(16, |_x, _y| 21.0);
        let mesh = build_patch_mesh(&patch);
        let positions = mesh.attribute(Mesh::ATTRIBUTE_POSITION);
        assert!(matches!(
            positions,
            Some(VertexAttributeValues::Float32x3(values)) if values.len() == 256
        ));
        // 15×15 quads × 2 triangles × 3 indices.
        assert_eq!(mesh.indices().map(Indices::len), Some(15 * 15 * 2 * 3));
        // A flat field has every normal pointing straight up.
        let normal = patch_normal(&patch, 8, 8);
        assert!((normal[0]).abs() < 1.0e-6);
        assert!((normal[1]).abs() < 1.0e-6);
        assert!((normal[2] - 1.0).abs() < 1.0e-6);
    }

    /// A slope in `x` tilts the normal away from straight up, toward `-x`.
    #[test]
    fn sloped_patch_tilts_the_normal() {
        // Height rises one metre per cell along `x`, so `dz/dx == 1` and the
        // normal is `(-1, 0, 1)` normalised.
        let patch = land_patch(16, |x, _y| super::patch_coord_f32(x));
        let normal = patch_normal(&patch, 8, 8);
        assert!(normal[0] < 0.0, "normal should tilt toward -x: {normal:?}");
        let expected = 1.0_f32 / 2.0_f32.sqrt();
        assert!((normal[0] + expected).abs() < 1.0e-5);
        assert!((normal[2] - expected).abs() < 1.0e-5);
    }
}
