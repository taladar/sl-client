//! Terrain rendering: turn each decoded `LayerData` height patch into a mesh and
//! shade it with height-blended texture splatting, across the agent's region and
//! its neighbours.
//!
//! On every [`SlSessionEvent::TerrainPatch`] for a land (ground-height) layer
//! the viewer builds a heightfield mesh for that patch and places it in the
//! scene. A patch owns `size`×`size` height samples but spans `size` metres, so
//! its far (north / east / north-east) edge is the *shared* boundary with the
//! neighbouring patches; the mesh is built `(size + 1)`×`(size + 1)` vertices,
//! sampling that far edge from the neighbour patches (Firestorm's
//! `LLSurfacePatch` stitching) so adjacent meshes meet with no seam.
//!
//! Texture splatting (P2.2): a region's `RegionHandshake` carries four ground
//! ("detail") texture ids and per-corner elevation bands.
//! [`SlSessionEvent::RegionInfoHandshake`] delivers them; the viewer requests
//! the four textures through the shared [`textures`](crate::textures) store (the
//! same fetch/decode/disk-cache pipeline the prims use) and, from the elevation
//! bands, builds a [`sl_terrain::TerrainComposition`] that weights each vertex
//! between the four textures. Each store-decoded detail texture arrives as a
//! [`TextureDecoded`] message; the GPU blend + lighting is the custom
//! [`TerrainMaterial`]; one shared material per region has its four textures
//! swapped in (with a tiling sampler) as they decode, showing a flat olive
//! placeholder until then.
//!
//! Multi-region: terrain streams from the agent's own region *and* every
//! neighbour child circuit (the session opens those automatically). Patches are
//! keyed by `(region_handle, patch_x, patch_y)` and placed at a **global
//! offset** — each region's south-west corner relative to a moving scene origin
//! — so neighbour regions tile outward instead of overlapping the home region.
//! The origin follows the root region (see [`recenter_terrain`]): when a border
//! crossing promotes a neighbour to root, the whole scene is re-centred on the
//! new region and the fly-camera is shifted by the same delta, keeping
//! coordinates small (so `f32` precision holds far from the login region) while
//! the world stays visually continuous.
//!
//! The geometry is built in Second Life space (Z-up, relative to the patch
//! origin cell) and oriented by the entity `Transform` via
//! [`sl_to_bevy_rotation`], converting to Bevy's Y-up world only at the entity
//! boundary.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use sl_client_bevy::{
    ATTRIBUTE_TERRAIN_WEIGHTS, RegionHandle, RegionIdentity, SlEvent, SlIdentity, SlSessionEvent,
    TerrainMaterial, TerrainPatch, TextureKey, Vector, to_bevy_image,
};
use sl_terrain::TerrainComposition;

use crate::camera::FlyCamera;
use crate::coords::{metres_to_f32, sl_to_bevy_rotation, sl_to_bevy_vec};
use crate::textures::{TextureDecoded, TextureManager};

/// The region edge length in metres. A standard Second Life / OpenSim region is
/// 256 m (16×16 patches of 16×16 cells).
const REGION_SIZE_METRES: f32 = 256.0;

/// The world span, in metres, over which a detail texture repeats once. Terrain
/// detail textures tile far more finely than the whole region, so the mesh's UVs
/// wrap every few metres rather than stretching one texture across 256 m.
const DETAIL_TILE_METRES: f32 = 8.0;

/// The number of ground ("detail") textures a region blends between.
const DETAIL_COUNT: usize = 4;

/// The flat placeholder colour of the terrain material — a muted olive, shown
/// until a region's detail textures decode and are swapped in.
const TERRAIN_PLACEHOLDER_COLOR: [u8; 4] = [92, 112, 71, 255];

/// The default per-vertex blend weight before a region's elevation bands are
/// known: all weight on the lowest detail texture.
const DEFAULT_WEIGHTS: [f32; DETAIL_COUNT] = [1.0, 0.0, 0.0, 0.0];

/// A patch's key: its region plus grid position within that region.
type PatchKey = (RegionHandle, u32, u32);

/// Per-region terrain-compositing state: the elevation bands, the shared splat
/// material, and the detail-texture keys requested for it.
#[derive(Default)]
struct RegionTerrain {
    /// The region's terrain-compositing parameters, once its `RegionHandshake`
    /// has been seen; `None` until then (patches render with flat weights).
    composition: Option<TerrainComposition>,
    /// The region's shared splat material (one per region — neighbours may use
    /// different ground textures), created on its first patch.
    material: Option<Handle<TerrainMaterial>>,
    /// The texture key requested for each detail slot (`None` for a nil slot),
    /// used to route an arriving texture to the right material slot.
    detail_keys: [Option<TextureKey>; DETAIL_COUNT],
    /// Whether this region's detail textures have been requested yet.
    requested: bool,
}

/// Viewer-side terrain bookkeeping across the home region and its neighbours.
#[derive(Resource, Default)]
pub(crate) struct TerrainState {
    /// The scene origin: the region whose south-west corner is Bevy `(0, 0)`.
    /// Follows the root region so coordinates stay small near the camera.
    origin: Option<RegionHandle>,
    /// Per-region compositing state, keyed by region handle.
    regions: HashMap<RegionHandle, RegionTerrain>,
    /// The rendered entity for each patch; a repeat patch replaces its mesh.
    patches: HashMap<PatchKey, Entity>,
    /// The most recent raw patch for each key, kept so a patch's mesh can be
    /// rebuilt (with real weights, or when a neighbour arrives) after the fact.
    raw_patches: HashMap<PatchKey, TerrainPatch>,
    /// The flat olive placeholder texture, shared by every region's material
    /// until its real detail textures decode.
    placeholder: Option<Handle<Image>>,
    /// Decoded detail textures by key, so a texture shared by several regions is
    /// decoded once and a repeated delivery is not decoded again.
    decoded: HashMap<TextureKey, Handle<Image>>,
}

/// Keep the scene origin on the root region: when a border crossing promotes a
/// neighbour to root, re-centre every terrain patch on the new region and shift
/// the fly-camera by the same delta, so coordinates stay small while the world
/// stays visually continuous.
pub(crate) fn recenter_terrain(
    identity: Res<SlIdentity>,
    mut state: ResMut<TerrainState>,
    mut cameras: Query<&mut Transform, With<FlyCamera>>,
    mut commands: Commands,
) {
    let Some(root) = identity.region_handle else {
        return;
    };
    match state.origin {
        Some(current) if current == root => {}
        Some(previous) => {
            // A genuine recenter: shift the camera to compensate for the world
            // moving under it, then re-place every patch on the new origin.
            let (previous_x, previous_y) = previous.global_coordinates();
            let (root_x, root_y) = root.global_coordinates();
            let delta = Vector {
                x: metres_to_f32(root_x) - metres_to_f32(previous_x),
                y: metres_to_f32(root_y) - metres_to_f32(previous_y),
                z: 0.0,
            };
            let shift = sl_to_bevy_vec(&delta);
            for mut transform in &mut cameras {
                // Per-component (not the `glam` vector operator) to stay clear
                // of the workspace `arithmetic_side_effects` lint.
                transform.translation.x -= shift.x;
                transform.translation.y -= shift.y;
                transform.translation.z -= shift.z;
            }
            state.origin = Some(root);
            replace_all_transforms(&state, &mut commands);
        }
        None => state.origin = Some(root),
    }
}

/// Fold terrain events into the scene: build (or rebuild) each land patch's
/// heightfield mesh, learn each region's compositing parameters and request its
/// detail textures, and swap each decoded texture into the right material(s).
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected ECS resources and event \
              readers; folding terrain now also reads the texture store and its \
              decode messages"
)]
pub(crate) fn update_terrain(
    mut events: MessageReader<SlEvent>,
    mut decoded: MessageReader<TextureDecoded>,
    mut state: ResMut<TerrainState>,
    mut manager: ResMut<TextureManager>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<TerrainMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut commands: Commands,
) {
    for event in events.read() {
        match &event.0 {
            SlSessionEvent::TerrainPatch(patch) if patch.layer.is_land() => {
                let key = (patch.region_handle, patch.patch_x, patch.patch_y);
                ensure_region(&mut state, patch.region_handle, &mut images, &mut materials);
                state.raw_patches.insert(key, (**patch).clone());
                spawn_or_replace_patch(&mut state, key, &mut meshes, &mut commands);
                // This patch supplies the shared far edge for its west / south
                // neighbours in the same region, so rebuild them to close seams.
                rebuild_neighbours(&state, key, &mut meshes, &mut commands);
            }
            SlSessionEvent::RegionInfoHandshake(identity) => {
                learn_composition(&mut state, identity, &mut manager, &mut materials);
                rebuild_region_patches(&state, identity.region_handle, &mut meshes, &mut commands);
            }
            _other => {}
        }
    }
    // A detail texture the store finished decoding: build its (tiling) image and
    // swap it into every region material that requested it.
    for &TextureDecoded(id) in decoded.read() {
        apply_detail_texture(&mut state, id, &manager, &mut images, &mut materials);
    }
}

/// Ensure the shared placeholder texture, the region entry, and its splat
/// material exist, creating the material (with all four detail slots on the
/// placeholder) on the region's first patch and reconciling any already-decoded
/// textures into it.
fn ensure_region(
    state: &mut TerrainState,
    region: RegionHandle,
    images: &mut Assets<Image>,
    materials: &mut Assets<TerrainMaterial>,
) {
    let placeholder = state
        .placeholder
        .get_or_insert_with(|| images.add(placeholder_image()))
        .clone();
    let entry = state.regions.entry(region).or_default();
    if entry.material.is_none() {
        entry.material = Some(materials.add(TerrainMaterial {
            detail0: placeholder.clone(),
            detail1: placeholder.clone(),
            detail2: placeholder.clone(),
            detail3: placeholder,
        }));
    }
    reconcile_region(state, region, materials);
}

/// Spawn a fresh entity for the land patch at `key`, or replace the mesh and
/// transform of the entity already rendering that grid position.
fn spawn_or_replace_patch(
    state: &mut TerrainState,
    key: PatchKey,
    meshes: &mut Assets<Mesh>,
    commands: &mut Commands,
) {
    let (region, patch_x, patch_y) = key;
    let composition = state
        .regions
        .get(&region)
        .and_then(|entry| entry.composition);
    let Some(mesh_data) = build_patch_mesh(&state.raw_patches, composition.as_ref(), key) else {
        return;
    };
    let mesh = meshes.add(mesh_data);
    let size = state.raw_patches.get(&key).map_or(0, |patch| patch.size);
    let material = state
        .regions
        .get(&region)
        .and_then(|entry| entry.material.clone())
        .unwrap_or_default();
    let transform = patch_transform(state.origin, region, patch_x, patch_y, size);
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
                "spawned terrain patch {region:?} ({patch_x}, {patch_y}) ({} rendered)",
                state.patches.len()
            );
        }
    }
}

/// Rebuild the mesh of an already-spawned patch (its entity, material, and
/// transform stay); a no-op if that key has no entity yet.
fn rebuild_existing(
    state: &TerrainState,
    key: PatchKey,
    meshes: &mut Assets<Mesh>,
    commands: &mut Commands,
) {
    let (region, _, _) = key;
    let Some(entity) = state.patches.get(&key).copied() else {
        return;
    };
    let composition = state
        .regions
        .get(&region)
        .and_then(|entry| entry.composition);
    let Some(mesh_data) = build_patch_mesh(&state.raw_patches, composition.as_ref(), key) else {
        return;
    };
    let mesh = meshes.add(mesh_data);
    commands.entity(entity).insert(Mesh3d(mesh));
}

/// Rebuild the west / south / south-west neighbours of the patch at `key` (in
/// the same region): they share their far edge with this one, so a newly arrived
/// patch closes their seam.
fn rebuild_neighbours(
    state: &TerrainState,
    key: PatchKey,
    meshes: &mut Assets<Mesh>,
    commands: &mut Commands,
) {
    let (region, patch_x, patch_y) = key;
    let west = patch_x.checked_sub(1);
    let south = patch_y.checked_sub(1);
    if let Some(west_x) = west {
        rebuild_existing(state, (region, west_x, patch_y), meshes, commands);
    }
    if let Some(south_y) = south {
        rebuild_existing(state, (region, patch_x, south_y), meshes, commands);
    }
    if let (Some(west_x), Some(south_y)) = (west, south) {
        rebuild_existing(state, (region, west_x, south_y), meshes, commands);
    }
}

/// Rebuild every already-rendered patch of `region`, called once that region's
/// elevation bands arrive after its patches.
fn rebuild_region_patches(
    state: &TerrainState,
    region: RegionHandle,
    meshes: &mut Assets<Mesh>,
    commands: &mut Commands,
) {
    let keys: Vec<PatchKey> = state
        .patches
        .keys()
        .copied()
        .filter(|(patch_region, _, _)| *patch_region == region)
        .collect();
    for key in keys {
        rebuild_existing(state, key, meshes, commands);
    }
}

/// Re-place every patch's transform on the current scene origin, after a
/// recenter moved it.
fn replace_all_transforms(state: &TerrainState, commands: &mut Commands) {
    for (&(region, patch_x, patch_y), &entity) in &state.patches {
        let size = state
            .raw_patches
            .get(&(region, patch_x, patch_y))
            .map_or(0, |patch| patch.size);
        let transform = patch_transform(state.origin, region, patch_x, patch_y, size);
        commands.entity(entity).insert(transform);
    }
}

/// Learn a region's terrain-compositing parameters from its `RegionHandshake`
/// and request its four detail textures through the shared texture store (once).
fn learn_composition(
    state: &mut TerrainState,
    identity: &RegionIdentity,
    manager: &mut TextureManager,
    materials: &mut Assets<TerrainMaterial>,
) {
    let region = identity.region_handle;
    {
        let entry = state.regions.entry(region).or_default();
        let (global_x, global_y) = region.global_coordinates();
        entry.composition = Some(TerrainComposition::new(
            identity.terrain.start_heights,
            identity.terrain.height_ranges,
            REGION_SIZE_METRES,
            [metres_to_f32(global_x), metres_to_f32(global_y)],
        ));
        if !entry.requested {
            // A modern Second Life mainland region often leaves its `TerrainDetail`
            // ids nil; substitute the default Linden terrain textures for those
            // slots (as the reference viewer's `LLVLComposition` keeps its
            // defaults) so the ground is shaded rather than left flat (R15).
            let detail_textures = identity.terrain.detail_textures_or_default();
            for (slot, texture) in entry.detail_keys.iter_mut().zip(detail_textures.iter()) {
                if texture.is_nil() {
                    continue;
                }
                let key = TextureKey::from(*texture);
                *slot = Some(key);
                // Terrain textures are boosted (P20.2 / `BOOST_TERRAIN`): they are
                // few, always under the camera, and not ranked by the on-screen
                // face pass (terrain is a custom material, not a prim face), so a
                // fixed boost keeps the ground from being starved behind prims. The
                // composition is learned during the region handshake — before the
                // seed caps arrive — so the store holds this request until the
                // `GetTexture` cap is up rather than failing it (see
                // `TextureManager::request_from`).
                manager.request_boosted(key, crate::render_priority::TERRAIN_BOOST_PRIORITY);
            }
            entry.requested = true;
        }
        let requested = entry.detail_keys.iter().filter(|key| key.is_some()).count();
        debug!("learned terrain composition for {region:?} ({requested} detail textures)");
    }
    reconcile_region(state, region, materials);
}

/// Route a store-decoded texture into every region material that requested it as
/// a ground detail: build a Bevy image with a tiling (repeating) sampler once and
/// cache it. Ignores a texture no region wants, or one the store failed to decode
/// (the material keeps its placeholder).
fn apply_detail_texture(
    state: &mut TerrainState,
    id: TextureKey,
    manager: &TextureManager,
    images: &mut Assets<Image>,
    materials: &mut Assets<TerrainMaterial>,
) {
    let wanted = state
        .regions
        .values()
        .any(|entry| entry.detail_keys.contains(&Some(id)));
    if !wanted {
        return;
    }
    if let std::collections::hash_map::Entry::Vacant(slot) = state.decoded.entry(id) {
        let Some(decoded) = manager.decoded(id) else {
            // The fetch/decode failed; the region keeps the flat placeholder.
            return;
        };
        let mut image = to_bevy_image(decoded);
        image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::Repeat,
            address_mode_v: ImageAddressMode::Repeat,
            ..ImageSamplerDescriptor::linear()
        });
        slot.insert(images.add(image));
        debug!("built tiling image for terrain detail texture {id}");
    }
    let regions: Vec<RegionHandle> = state.regions.keys().copied().collect();
    for region in regions {
        reconcile_region(state, region, materials);
    }
}

/// Set every detail slot of a region's material to the decoded texture for its
/// requested key, or the placeholder while that texture is still in flight.
fn reconcile_region(
    state: &TerrainState,
    region: RegionHandle,
    materials: &mut Assets<TerrainMaterial>,
) {
    let Some(entry) = state.regions.get(&region) else {
        return;
    };
    let Some(material_handle) = &entry.material else {
        return;
    };
    let Some(mut material) = materials.get_mut(material_handle) else {
        return;
    };
    for (index, key) in entry.detail_keys.iter().enumerate() {
        let handle = key
            .and_then(|key| state.decoded.get(&key).cloned())
            .or_else(|| state.placeholder.clone())
            .unwrap_or_default();
        assign_detail(&mut material, index, handle);
    }
}

/// Set the material's detail-texture slot at `index` (0–3) to `handle`; an
/// out-of-range index is ignored (there are only four detail slots).
fn assign_detail(material: &mut TerrainMaterial, index: usize, handle: Handle<Image>) {
    match index {
        0 => material.detail0 = handle,
        1 => material.detail1 = handle,
        2 => material.detail2 = handle,
        3 => material.detail3 = handle,
        _other => {}
    }
}

/// The entity transform placing patch (`patch_x`, `patch_y`) of `region` in the
/// scene: the region's south-west corner relative to the scene `origin`, plus
/// the patch's local origin, converted to Bevy's Y-up world.
fn patch_transform(
    origin: Option<RegionHandle>,
    region: RegionHandle,
    patch_x: u32,
    patch_y: u32,
    size: u32,
) -> Transform {
    let (region_x, region_y) = region.global_coordinates();
    let (origin_x, origin_y) = origin.unwrap_or(region).global_coordinates();
    let position = Vector {
        x: metres_to_f32(region_x) - metres_to_f32(origin_x)
            + patch_coord_f32(patch_x.saturating_mul(size)),
        y: metres_to_f32(region_y) - metres_to_f32(origin_y)
            + patch_coord_f32(patch_y.saturating_mul(size)),
        z: 0.0,
    };
    Transform {
        translation: sl_to_bevy_vec(&position),
        rotation: sl_to_bevy_rotation(),
        ..default()
    }
}

/// A 1×1 olive placeholder [`Image`], used for every detail slot until the real
/// textures decode.
fn placeholder_image() -> Image {
    Image::new(
        Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        TERRAIN_PLACEHOLDER_COLOR.to_vec(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

/// Build a Bevy heightfield [`Mesh`] for the terrain patch at `key`, in Second
/// Life space relative to the patch origin cell, or `None` if that patch is not
/// stored.
///
/// The mesh is `(size + 1)`×`(size + 1)` vertices: a patch owns `size`×`size`
/// height samples but spans `size` metres, so its far (north / east /
/// north-east) edge is the *shared* boundary with the neighbouring patches. That
/// extra edge is sampled from the same region's neighbours in `raw` (see
/// [`sample_height`]) so adjacent patch meshes meet exactly and leave no seam.
/// Each vertex carries computed normals, tiled detail UVs, and a four-component
/// blend weight (from `composition`, or a flat default while it is unknown); the
/// grid is two triangles per cell quad.
fn build_patch_mesh(
    raw: &HashMap<PatchKey, TerrainPatch>,
    composition: Option<&TerrainComposition>,
    key: PatchKey,
) -> Option<Mesh> {
    let (region, patch_x, patch_y) = key;
    let size = raw.get(&key)?.size;
    // Vertices per edge: the `size` owned samples plus one shared boundary
    // sample from the north / east neighbour.
    let width = size.saturating_add(1);
    let region_origin_x = patch_x.saturating_mul(size);
    let region_origin_y = patch_y.saturating_mul(size);
    let capacity = usize::try_from(width.saturating_mul(width)).unwrap_or(0);

    // Sample the extended height grid once (own cells plus the neighbour-shared
    // far edge), so the normals and positions agree on the seam.
    let mut heights: Vec<f32> = Vec::with_capacity(capacity);
    for y in 0..width {
        for x in 0..width {
            heights.push(sample_height(raw, region, patch_x, patch_y, size, x, y));
        }
    }

    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(capacity);
    let mut normals: Vec<[f32; 3]> = Vec::with_capacity(capacity);
    let mut uvs: Vec<[f32; 2]> = Vec::with_capacity(capacity);
    let mut weights: Vec<[f32; DETAIL_COUNT]> = Vec::with_capacity(capacity);
    for y in 0..width {
        for x in 0..width {
            let height = grid_height(&heights, width, x, y);
            positions.push([patch_coord_f32(x), patch_coord_f32(y), height]);
            normals.push(grid_normal(&heights, width, x, y));
            // Region-local metres (0..=size within the region), the input the
            // composition expects; the composition's own global origin makes the
            // noise continuous across region borders.
            let local_x = patch_coord_f32(region_origin_x) + patch_coord_f32(x);
            let local_y = patch_coord_f32(region_origin_y) + patch_coord_f32(y);
            uvs.push([local_x / DETAIL_TILE_METRES, local_y / DETAIL_TILE_METRES]);
            weights.push(match composition {
                Some(composition) => composition.blend_weights(local_x, local_y, height),
                None => DEFAULT_WEIGHTS,
            });
        }
    }
    let mut indices: Vec<u32> = Vec::new();
    for y in 0..size {
        for x in 0..size {
            let x1 = x.saturating_add(1);
            let y1 = y.saturating_add(1);
            let i00 = vertex_index(width, x, y);
            let i10 = vertex_index(width, x1, y);
            let i01 = vertex_index(width, x, y1);
            let i11 = vertex_index(width, x1, y1);
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
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_attribute(ATTRIBUTE_TERRAIN_WEIGHTS, weights);
    mesh.insert_indices(Indices::U32(indices));
    Some(mesh)
}

/// The ground height for extended grid cell (`x`, `y`) of patch (`patch_x`,
/// `patch_y`) in `region`: an own sample for `x`/`y` in `0..size`, or the shared
/// boundary sample from the east / north / north-east neighbour patch in the
/// same region for the extra `size` index. Falls back to this patch's own
/// clamped edge sample when the neighbour is not loaded yet — a temporary 1 m
/// flat strip, rebuilt seamlessly once the neighbour arrives. (A patch at the
/// region's own far edge keeps that strip: its neighbour is in another region.)
fn sample_height(
    raw: &HashMap<PatchKey, TerrainPatch>,
    region: RegionHandle,
    patch_x: u32,
    patch_y: u32,
    size: u32,
    x: u32,
    y: u32,
) -> f32 {
    let (neighbour_x, local_x) = if x < size {
        (patch_x, x)
    } else {
        (patch_x.saturating_add(1), x.saturating_sub(size))
    };
    let (neighbour_y, local_y) = if y < size {
        (patch_y, y)
    } else {
        (patch_y.saturating_add(1), y.saturating_sub(size))
    };
    if let Some(height) = raw
        .get(&(region, neighbour_x, neighbour_y))
        .and_then(|patch| patch.value(local_x, local_y))
    {
        return height;
    }
    let clamped_x = x.min(size.saturating_sub(1));
    let clamped_y = y.min(size.saturating_sub(1));
    raw.get(&(region, patch_x, patch_y))
        .and_then(|patch| patch.value(clamped_x, clamped_y))
        .unwrap_or(0.0)
}

/// The height at extended grid cell (`x`, `y`) in a `width`-wide, row-major grid.
fn grid_height(heights: &[f32], width: u32, x: u32, y: u32) -> f32 {
    let index = usize::try_from(y.saturating_mul(width).saturating_add(x)).unwrap_or(0);
    heights.get(index).copied().unwrap_or(0.0)
}

/// The flat vertex-buffer index of grid cell (`x`, `y`) in a `width`-wide,
/// row-major grid.
const fn vertex_index(width: u32, x: u32, y: u32) -> u32 {
    y.saturating_mul(width).saturating_add(x)
}

/// The upward-facing unit normal at extended grid cell (`x`, `y`), from a
/// central-difference of the neighbouring ground heights (one-sided at the grid
/// edges). Because the grid already carries the neighbour-shared far edge, the
/// normals agree across patch seams.
fn grid_normal(heights: &[f32], width: u32, x: u32, y: u32) -> [f32; 3] {
    let left_x = if x > 0 { x.saturating_sub(1) } else { x };
    let right_x = if x.saturating_add(1) < width {
        x.saturating_add(1)
    } else {
        x
    };
    let down_y = if y > 0 { y.saturating_sub(1) } else { y };
    let up_y = if y.saturating_add(1) < width {
        y.saturating_add(1)
    } else {
        y
    };
    // Height differences over the cell span between the sampled neighbours (2
    // cells interior, 1 at an edge), guarded to at least one cell so a
    // degenerate grid cannot divide by zero.
    let span_x = patch_coord_f32(right_x.saturating_sub(left_x)).max(1.0);
    let span_y = patch_coord_f32(up_y.saturating_sub(down_y)).max(1.0);
    let dz_dx =
        (grid_height(heights, width, right_x, y) - grid_height(heights, width, left_x, y)) / span_x;
    let dz_dy =
        (grid_height(heights, width, x, up_y) - grid_height(heights, width, x, down_y)) / span_y;
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
    use std::collections::HashMap;

    use super::{DEFAULT_WEIGHTS, PatchKey, build_patch_mesh, metres_to_f32, patch_transform};
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::{Indices, Mesh, PrimitiveTopology, VertexAttributeValues};
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{ATTRIBUTE_TERRAIN_WEIGHTS, RegionHandle, TerrainLayerType, TerrainPatch};
    use sl_terrain::TerrainComposition;

    /// The region and grid position the test patches use.
    const KEY: PatchKey = (RegionHandle(0), 1, 2);

    /// A single-patch map for the land patch of the given edge size whose height
    /// is `f(x, y)`, at [`KEY`].
    fn one_patch_map(
        size: u32,
        mut height: impl FnMut(u32, u32) -> f32,
    ) -> HashMap<PatchKey, TerrainPatch> {
        let mut values = Vec::new();
        for y in 0..size {
            for x in 0..size {
                values.push(height(x, y));
            }
        }
        let (region, patch_x, patch_y) = KEY;
        let patch = TerrainPatch {
            region_handle: region,
            layer: TerrainLayerType::Land,
            patch_x,
            patch_y,
            size,
            values,
        };
        let mut map = HashMap::new();
        map.insert(KEY, patch);
        map
    }

    /// Build the mesh for [`KEY`], asserting it is present.
    fn mesh_for(map: &HashMap<PatchKey, TerrainPatch>, comp: Option<&TerrainComposition>) -> Mesh {
        let mesh = build_patch_mesh(map, comp, KEY);
        assert!(mesh.is_some(), "patch mesh should build for {KEY:?}");
        mesh.unwrap_or_else(|| {
            Mesh::new(
                PrimitiveTopology::TriangleList,
                RenderAssetUsages::default(),
            )
        })
    }

    /// The vertex normals of a built mesh, or empty if the attribute is absent.
    fn normals_of(mesh: &Mesh) -> Vec<[f32; 3]> {
        match mesh.attribute(Mesh::ATTRIBUTE_NORMAL) {
            Some(VertexAttributeValues::Float32x3(values)) => values.clone(),
            _other => Vec::new(),
        }
    }

    /// The four-component blend weights carried on a built mesh, or empty if the
    /// attribute is absent.
    fn weights_of(mesh: &Mesh) -> Vec<[f32; 4]> {
        match mesh.attribute(ATTRIBUTE_TERRAIN_WEIGHTS.id) {
            Some(VertexAttributeValues::Float32x4(values)) => values.clone(),
            _other => Vec::new(),
        }
    }

    /// A 16-sample patch spans 16 metres of quads: `(16 + 1)²` vertices (the
    /// extra edge is the shared boundary), `16×16` cell quads, straight-up
    /// normals on flat ground, and a four-component weight per vertex. The extra
    /// edge is what closes the seam between adjacent patches.
    #[test]
    fn flat_patch_spans_the_full_edge() {
        let map = one_patch_map(16, |_x, _y| 21.0);
        let mesh = mesh_for(&map, None);
        let positions = mesh.attribute(Mesh::ATTRIBUTE_POSITION);
        // 17×17 vertices: 16 owned samples plus the neighbour-shared far edge.
        assert!(matches!(
            positions,
            Some(VertexAttributeValues::Float32x3(values)) if values.len() == 289
        ));
        let weights = mesh.attribute(ATTRIBUTE_TERRAIN_WEIGHTS.id);
        assert!(matches!(
            weights,
            Some(VertexAttributeValues::Float32x4(values)) if values.len() == 289
        ));
        // 16×16 quads × 2 triangles × 3 indices — a full 16 m span, no gap.
        assert_eq!(mesh.indices().map(Indices::len), Some(16 * 16 * 2 * 3));
        // A flat field has every normal pointing straight up (Second Life +Z).
        let normals = normals_of(&mesh);
        let centre = normals.get(8 * 17 + 8).copied().unwrap_or([0.0; 3]);
        assert!((centre[0]).abs() < 1.0e-6);
        assert!((centre[1]).abs() < 1.0e-6);
        assert!((centre[2] - 1.0).abs() < 1.0e-6);
    }

    /// A slope in `x` tilts the interior normal away from straight up, toward
    /// `-x`.
    #[test]
    fn sloped_patch_tilts_the_normal() {
        // Height rises one metre per cell along `x`, so `dz/dx == 1` and the
        // normal is `(-1, 0, 1)` normalised.
        let map = one_patch_map(16, |x, _y| super::patch_coord_f32(x));
        let mesh = mesh_for(&map, None);
        let normals = normals_of(&mesh);
        let centre = normals.get(8 * 17 + 8).copied().unwrap_or([0.0; 3]);
        assert!(centre[0] < 0.0, "normal should tilt toward -x: {centre:?}");
        let expected = 1.0_f32 / 2.0_f32.sqrt();
        assert!((centre[0] + expected).abs() < 1.0e-5);
        assert!((centre[2] - expected).abs() < 1.0e-5);
    }

    /// Without a composition, every vertex carries the flat default weight.
    #[test]
    fn no_composition_yields_default_weights() {
        let map = one_patch_map(16, |_x, _y| 12.0);
        let mesh = mesh_for(&map, None);
        let weights = weights_of(&mesh);
        assert!(!weights.is_empty(), "weights attribute missing");
        for weight in &weights {
            let differs = weight
                .iter()
                .zip(DEFAULT_WEIGHTS.iter())
                .any(|(a, b)| (a - b).abs() > 1.0e-6);
            assert!(!differs, "weight {weight:?} is not the default");
        }
    }

    /// With a composition whose band rises across the region, a patch's vertex
    /// weights vary — higher ground shifts weight toward the higher detail
    /// textures — rather than staying flat.
    #[test]
    fn composition_varies_the_vertex_weights() {
        let composition =
            TerrainComposition::new([10.0; 4], [40.0; 4], 256.0, [256_000.0, 256_000.0]);
        let map = one_patch_map(16, |_x, y| super::patch_coord_f32(y) * 3.0);
        let mesh = mesh_for(&map, Some(&composition));
        let weights = weights_of(&mesh);
        assert!(!weights.is_empty(), "weights attribute missing");
        // Every weight vector is normalised (partitions unity).
        for weight in &weights {
            let sum: f32 = weight.iter().sum();
            assert!(
                (sum - 1.0).abs() < 1.0e-4,
                "weight {weight:?} sums to {sum}"
            );
        }
        // The low and high rows do not share the same weight (the band moved).
        let first = weights.first().copied().unwrap_or([0.0; 4]);
        let last = weights.last().copied().unwrap_or([0.0; 4]);
        let moved = first
            .iter()
            .zip(last.iter())
            .any(|(a, b)| (a - b).abs() > 1.0e-3);
        assert!(moved, "weights {first:?} and {last:?} did not vary");
    }

    /// A neighbour region east of the origin is offset one region (256 m) along
    /// Bevy `+X`, while the origin region sits at zero.
    #[test]
    fn neighbour_region_is_offset_by_one_region() {
        let origin = RegionHandle::from_global(256_000, 256_000);
        let east = RegionHandle::from_global(256_256, 256_000);
        // Patch (0, 0) of the origin region sits at Bevy X 0.
        let home = patch_transform(Some(origin), origin, 0, 0, 16);
        assert!(home.translation.x.abs() < 1.0e-3, "home at {home:?}");
        // Patch (0, 0) of the east neighbour sits 256 m along Bevy +X.
        let neighbour = patch_transform(Some(origin), east, 0, 0, 16);
        assert!(
            (neighbour.translation.x - 256.0).abs() < 1.0e-3,
            "east neighbour at {neighbour:?}"
        );
    }

    /// Global metres round-trip exactly through the 16-bit high/low split (the
    /// values are all within the `f32` mantissa's exact-integer range).
    #[test]
    fn metres_convert_exactly() {
        assert_eq!(metres_to_f32(0x0).to_bits(), 0.0_f32.to_bits());
        assert_eq!(metres_to_f32(0x0100).to_bits(), 256.0_f32.to_bits());
        assert_eq!(metres_to_f32(0x0001_0000).to_bits(), 65_536.0_f32.to_bits());
        assert_eq!(
            metres_to_f32(0x0004_0000).to_bits(),
            262_144.0_f32.to_bits()
        );
        assert_eq!(
            metres_to_f32(0x0010_0100).to_bits(),
            1_048_832.0_f32.to_bits()
        );
    }
}
