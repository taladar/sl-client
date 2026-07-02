//! LLMesh format parsing and decoding.
//!
//! A Second Life mesh asset is a binary-LLSD **header** map followed by
//! concatenated **zlib-compressed blocks**. The header names each block by a
//! `{ offset, size }` sub-map, where `offset` is measured *from the end of the
//! header* (see [`parse_header`]); a block's absolute byte range in the asset is
//! therefore `[header_size + offset, header_size + offset + size)`. Each block
//! zlib-inflates to a further binary-LLSD value:
//!
//! - a **geometry LOD block** (`lowest_lod` … `high_lod`) — an array of
//!   per-face submeshes with `u16`-quantized positions / normals / UV0 and `u16`
//!   triangle indices ([`decode_lod`]);
//! - the **`skin` block** — joint names, inverse-bind and bind-shape matrices,
//!   and (in each submesh's `Weights`) per-vertex joint influences
//!   ([`decode_skin`]);
//! - the **`physics_convex`** (convex-hull) and **`physics_mesh`** (a geometry
//!   block) collision blocks ([`decode_physics_convex`] / [`decode_physics_mesh`]).
//!
//! This mirrors Firestorm's `LLMeshRepoThread::headerReceived`,
//! `LLVolume::unpackVolumeFacesInternal`, and `LLModel`/`LLModel::Decomposition`
//! `fromLLSD`. All multi-byte quantized values in the *block* payloads are
//! little-endian `u16`; they are assembled with explicit shifts because the
//! crate lints forbid the `from_le_bytes` family.

use std::io::Read as _;

use flate2::read::ZlibDecoder;
use sl_proto::MeshLod;
use sl_wire::{Llsd, parse_llsd_binary, parse_llsd_binary_prefix};

/// The viewer's `MESH_HEADER_SIZE`: the initial byte-range probe read to obtain
/// a mesh's header (assumed large enough to contain it).
pub const MESH_HEADER_SIZE: usize = 4096;

/// The `1/65535` dequantization divisor for `u16`-packed values.
const U16_SCALE: f32 = 65535.0;

/// The deprecated legacy header prefix some older assets carry before the LLSD.
const LEGACY_PREFIX: &[u8] = b"<? LLSD/Binary ?>";

/// The viewer's `MAX_MESH_VERSION`: a header `version` above this marks the mesh
/// unavailable (treated like a `404`).
const MAX_MESH_VERSION: u32 = 999;

/// A failure to decode part of an LLMesh asset.
#[derive(Debug, thiserror::Error)]
pub enum MeshDecodeError {
    /// A block could not be zlib-inflated.
    #[error("mesh block zlib inflate failed: {0}")]
    Inflate(String),
    /// A block's inflated bytes were not valid binary LLSD.
    #[error("mesh block is not valid binary LLSD: {0}")]
    Llsd(String),
    /// A geometry / skin / physics block was not the expected LLSD shape.
    #[error("mesh block has an unexpected LLSD shape (expected {expected})")]
    Shape {
        /// The LLSD shape that was expected (e.g. `"array of submeshes"`).
        expected: &'static str,
    },
}

/// A reference to one compressed block within a mesh asset: its `offset` from
/// the end of the header and its compressed `size` in bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockRef {
    /// Byte offset of the block from the end of the header.
    pub offset: usize,
    /// Compressed size of the block in bytes.
    pub size: usize,
}

impl BlockRef {
    /// The absolute byte range `[start, end)` of this block in the asset, given
    /// the parsed `header_size`. Saturating, so a malformed huge offset cannot
    /// overflow.
    #[must_use]
    pub const fn range(&self, header_size: usize) -> (usize, usize) {
        let start = header_size.saturating_add(self.offset);
        let end = start.saturating_add(self.size);
        (start, end)
    }
}

/// The parsed mesh header: the format version, the four geometry LOD block
/// references (indexed by [`MeshLod::index`]), and the optional skin / physics
/// block references.
#[derive(Clone, Copy, Debug, Default)]
pub struct MeshHeader {
    /// The mesh format version.
    pub version: u32,
    /// The geometry LOD blocks, indexed by [`MeshLod::index`] (`None` if absent).
    pub lods: [Option<BlockRef>; sl_proto::MESH_LOD_COUNT],
    /// The rigging / skin block, if present.
    pub skin: Option<BlockRef>,
    /// The convex-hull physics block, if present.
    pub physics_convex: Option<BlockRef>,
    /// The triangle-mesh physics block, if present.
    pub physics_mesh: Option<BlockRef>,
    /// Whether the header marks the mesh unavailable (a `404` key, or a version
    /// past the viewer's `MAX_MESH_VERSION`).
    pub not_found: bool,
}

impl MeshHeader {
    /// The block reference for a geometry level of detail, if present.
    #[must_use]
    pub fn lod(&self, lod: MeshLod) -> Option<BlockRef> {
        let index = usize::from(lod.index());
        self.lods.get(index).copied().flatten()
    }

    /// The finest available geometry level no finer than `wanted`, or the finest
    /// available level if none is that coarse — i.e. the best block to serve a
    /// request for `wanted`. `None` when the mesh carries no geometry at all.
    #[must_use]
    pub fn best_lod(&self, wanted: MeshLod) -> Option<MeshLod> {
        // Prefer the wanted level, then coarser, then (if none) the finest we
        // have — a mesh always renders at *some* level.
        let mut candidate = Some(wanted);
        while let Some(lod) = candidate {
            if self.lod(lod).is_some() {
                return Some(lod);
            }
            candidate = if lod == MeshLod::COARSEST {
                None
            } else {
                Some(lod.coarser())
            };
        }
        MeshLod::ALL
            .into_iter()
            .rev()
            .find(|&lod| self.lod(lod).is_some())
    }
}

/// One face of a decoded mesh: dequantized geometry plus optional rig weights.
#[derive(Clone, Debug, Default)]
pub struct Submesh {
    /// Vertex positions in the mesh's local space.
    pub positions: Vec<[f32; 3]>,
    /// Per-vertex normals (empty if the block carried none).
    pub normals: Vec<[f32; 3]>,
    /// Per-vertex UV0 texture coordinates (empty if the block carried none).
    pub uvs: Vec<[f32; 2]>,
    /// Triangle-list indices into the vertex arrays (a multiple of 3).
    pub indices: Vec<u32>,
    /// Per-vertex rig influences, if this is a rigged submesh.
    pub weights: Option<Vec<VertexWeights>>,
    /// The per-axis normalized scale the uploader recorded (default `[1, 1, 1]`).
    /// Retained as metadata; positions are dequantized to the position domain
    /// and are *not* pre-multiplied by it, matching the viewer's core unpack.
    pub normalized_scale: [f32; 3],
    /// Whether the submesh is an explicit empty face (`NoGeometry`).
    pub no_geometry: bool,
}

/// One vertex's rig influences: up to four `(joint index, weight)` pairs.
#[derive(Clone, Debug, Default)]
pub struct VertexWeights {
    /// The `(joint index, normalized weight)` influences (at most four).
    pub influences: Vec<(u8, f32)>,
}

/// A decoded mesh at one geometry level of detail: its level and per-face
/// submeshes.
#[derive(Clone, Debug)]
pub struct DecodedMesh {
    /// The level of detail these submeshes were decoded from.
    pub lod: MeshLod,
    /// The per-face submeshes.
    pub submeshes: Vec<Submesh>,
}

impl DecodedMesh {
    /// The total vertex count across all submeshes.
    #[must_use]
    pub fn vertex_count(&self) -> usize {
        self.submeshes.iter().fold(0_usize, |total, submesh| {
            total.saturating_add(submesh.positions.len())
        })
    }

    /// The total triangle count across all submeshes.
    #[must_use]
    pub fn triangle_count(&self) -> usize {
        self.submeshes.iter().fold(0_usize, |total, submesh| {
            total.saturating_add(submesh.indices.len().checked_div(3).unwrap_or(0))
        })
    }
}

/// The joint / bind-matrix rigging data from a mesh's `skin` block.
#[derive(Clone, Debug, Default)]
pub struct MeshSkin {
    /// The rig's joint (bone) names, in binding order.
    pub joint_names: Vec<String>,
    /// One 4×4 inverse-bind matrix per joint (row-major, 16 floats).
    pub inverse_bind_matrix: Vec<[f32; 16]>,
    /// The 4×4 bind-shape matrix (row-major, 16 floats).
    pub bind_shape_matrix: [f32; 16],
    /// Optional per-joint alternate inverse-bind matrices.
    pub alt_inverse_bind_matrix: Vec<[f32; 16]>,
    /// The optional pelvis Z offset.
    pub pelvis_offset: Option<f32>,
    /// Whether joint scale is locked when a joint position is overridden.
    pub lock_scale_if_joint_position: bool,
}

/// The convex-hull collision decomposition from a `physics_convex` block.
#[derive(Clone, Debug, Default)]
pub struct PhysicsConvex {
    /// One point list per convex hull.
    pub hulls: Vec<Vec<[f32; 3]>>,
    /// The single low-detail bounding hull, if present.
    pub bounding_verts: Vec<[f32; 3]>,
    /// The dequantization domain minimum.
    pub min: [f32; 3],
    /// The dequantization domain maximum.
    pub max: [f32; 3],
}

/// A mesh's decoded physics blocks: the convex decomposition and/or the triangle
/// physics mesh (assembled by the store from the two separate header blocks).
#[derive(Clone, Debug, Default)]
pub struct MeshPhysics {
    /// The convex-hull decomposition, if the mesh carried a `physics_convex`.
    pub convex: Option<PhysicsConvex>,
    /// The triangle physics mesh, if the mesh carried a `physics_mesh`.
    pub mesh: Option<Vec<Submesh>>,
}

/// Parses a mesh header from the leading bytes of an asset, returning the header
/// and its byte length (`header_size`). Strips the optional legacy
/// `"<? LLSD/Binary ?>"` prefix, then parses the binary-LLSD map; block offsets
/// in the header are measured from the returned `header_size`.
///
/// Returns `None` if the bytes are not a recognisable mesh header (not a binary
/// LLSD map).
#[must_use]
pub fn parse_header(data: &[u8]) -> Option<(MeshHeader, usize)> {
    let (prefix_len, rest) = strip_legacy_prefix(data);
    let (llsd, consumed) = parse_llsd_binary_prefix(rest).ok()?;
    llsd.as_map()?;
    let header_size = prefix_len.checked_add(consumed)?;

    let version = llsd
        .get("version")
        .and_then(Llsd::as_i32)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(0);
    let not_found = llsd.get("404").is_some() || version > MAX_MESH_VERSION;

    let mut lods: [Option<BlockRef>; sl_proto::MESH_LOD_COUNT] = [None, None, None, None];
    for lod in MeshLod::ALL {
        if let Some(slot) = lods.get_mut(usize::from(lod.index())) {
            *slot = block_ref(llsd.get(lod.header_key()));
        }
    }

    Some((
        MeshHeader {
            version,
            lods,
            skin: block_ref(llsd.get("skin")),
            physics_convex: block_ref(llsd.get("physics_convex")),
            physics_mesh: block_ref(llsd.get("physics_mesh")),
            not_found,
        },
        header_size,
    ))
}

/// Strips the optional legacy `"<? LLSD/Binary ?>"` prefix (and a following
/// newline), returning the number of bytes stripped and the remaining slice.
fn strip_legacy_prefix(data: &[u8]) -> (usize, &[u8]) {
    if data.len() > LEGACY_PREFIX.len() && data.get(..LEGACY_PREFIX.len()) == Some(LEGACY_PREFIX) {
        let mut len = LEGACY_PREFIX.len();
        if data.get(len) == Some(&b'\n') {
            len = len.saturating_add(1);
        }
        (len, data.get(len..).unwrap_or(&[]))
    } else {
        (0, data)
    }
}

/// Reads a `{ offset, size }` block sub-map into a [`BlockRef`], or `None` if it
/// is absent, malformed, or empty (`size == 0`).
fn block_ref(value: Option<&Llsd>) -> Option<BlockRef> {
    let map = value?;
    let offset = usize::try_from(map.get("offset").and_then(Llsd::as_i32)?).ok()?;
    let size = usize::try_from(map.get("size").and_then(Llsd::as_i32)?).ok()?;
    if size == 0 {
        return None;
    }
    Some(BlockRef { offset, size })
}

/// Decodes a compressed geometry LOD block into a [`DecodedMesh`] at `lod`.
///
/// # Errors
///
/// Returns [`MeshDecodeError`] if the block cannot be inflated, is not binary
/// LLSD, or is not an array of submeshes.
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `decode_lod` reads clearly"
)]
pub fn decode_lod(compressed: &[u8], lod: MeshLod) -> Result<DecodedMesh, MeshDecodeError> {
    let submeshes = decode_submeshes(compressed)?;
    Ok(DecodedMesh { lod, submeshes })
}

/// Decodes a compressed `physics_mesh` block (a geometry block) into its
/// submeshes.
///
/// # Errors
///
/// Returns [`MeshDecodeError`] as [`decode_lod`] does.
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `decode_physics_mesh` reads clearly"
)]
pub fn decode_physics_mesh(compressed: &[u8]) -> Result<Vec<Submesh>, MeshDecodeError> {
    decode_submeshes(compressed)
}

/// Inflates and parses an array-of-submeshes block (a geometry or physics mesh).
fn decode_submeshes(compressed: &[u8]) -> Result<Vec<Submesh>, MeshDecodeError> {
    let inflated = inflate(compressed)?;
    let llsd =
        parse_llsd_binary(&inflated).map_err(|error| MeshDecodeError::Llsd(error.to_string()))?;
    let array = llsd.as_array().ok_or(MeshDecodeError::Shape {
        expected: "array of submeshes",
    })?;
    Ok(array.iter().map(decode_submesh).collect())
}

/// Decodes one submesh map: dequantizes its positions, normals, UVs, indices,
/// and optional rig weights.
fn decode_submesh(map: &Llsd) -> Submesh {
    let normalized_scale = vec3(map.get("NormalizedScale"), [1.0, 1.0, 1.0]);
    if map.get("NoGeometry").is_some() {
        return Submesh {
            normalized_scale,
            no_geometry: true,
            ..Submesh::default()
        };
    }

    let (pos_min, pos_max) = domain(map.get("PositionDomain"), [0.0, 0.0, 0.0], [1.0, 1.0, 1.0]);
    let positions = dequantize_positions(binary(map, "Position"), pos_min, pos_max);
    let normals = dequantize_normals(binary(map, "Normal"));

    let (tc_min, tc_max) = domain2(map.get("TexCoord0Domain"), [0.0, 0.0], [1.0, 1.0]);
    let uvs = dequantize_uvs(binary(map, "TexCoord0"), tc_min, tc_max);

    let indices = decode_indices(binary(map, "TriangleList"));
    let weights = map
        .get("Weights")
        .and_then(Llsd::as_binary)
        .map(|bytes| decode_weights(bytes, positions.len()));

    Submesh {
        positions,
        normals,
        uvs,
        indices,
        weights,
        normalized_scale,
        no_geometry: false,
    }
}

/// The binary payload of submesh field `key`, or an empty slice if absent.
fn binary<'a>(map: &'a Llsd, key: &str) -> &'a [u8] {
    map.get(key).and_then(Llsd::as_binary).unwrap_or(&[])
}

/// Dequantizes a `u16×3`-per-vertex position blob to the `[min, max]` domain.
fn dequantize_positions(bytes: &[u8], min: [f32; 3], max: [f32; 3]) -> Vec<[f32; 3]> {
    let [min_x, min_y, min_z] = min;
    let [max_x, max_y, max_z] = max;
    bytes
        .chunks_exact(6)
        .map(|chunk| {
            [
                dequantize(u16_at(chunk, 0), min_x, max_x),
                dequantize(u16_at(chunk, 2), min_y, max_y),
                dequantize(u16_at(chunk, 4), min_z, max_z),
            ]
        })
        .collect()
}

/// Dequantizes a `u16×3`-per-vertex normal blob to `[-1, 1]` per component.
fn dequantize_normals(bytes: &[u8]) -> Vec<[f32; 3]> {
    bytes
        .chunks_exact(6)
        .map(|chunk| {
            [
                dequantize(u16_at(chunk, 0), -1.0, 1.0),
                dequantize(u16_at(chunk, 2), -1.0, 1.0),
                dequantize(u16_at(chunk, 4), -1.0, 1.0),
            ]
        })
        .collect()
}

/// Dequantizes a `u16×2`-per-vertex UV blob to the `[min, max]` texture domain.
fn dequantize_uvs(bytes: &[u8], min: [f32; 2], max: [f32; 2]) -> Vec<[f32; 2]> {
    let [min_u, min_v] = min;
    let [max_u, max_v] = max;
    bytes
        .chunks_exact(4)
        .map(|chunk| {
            [
                dequantize(u16_at(chunk, 0), min_u, max_u),
                dequantize(u16_at(chunk, 2), min_v, max_v),
            ]
        })
        .collect()
}

/// Decodes a `u16` triangle-list blob into `u32` indices, dropping any trailing
/// indices that do not complete a triangle.
fn decode_indices(bytes: &[u8]) -> Vec<u32> {
    let mut indices: Vec<u32> = bytes
        .chunks_exact(2)
        .map(|chunk| u32::from(u16_at(chunk, 0)))
        .collect();
    let remainder = indices.len().checked_rem(3).unwrap_or(0);
    let keep = indices.len().saturating_sub(remainder);
    indices.truncate(keep);
    indices
}

/// Decodes the per-vertex rig-weight stream: for each vertex, `(u8 joint, u16
/// weight)` pairs until a `0xFF` sentinel or four influences.
fn decode_weights(bytes: &[u8], num_verts: usize) -> Vec<VertexWeights> {
    /// The end-of-influences sentinel joint byte.
    const END_INFLUENCES: u8 = 0xFF;
    /// The maximum number of influences per vertex.
    const MAX_INFLUENCES: usize = 4;

    let mut out = Vec::with_capacity(num_verts);
    let mut cursor = 0_usize;
    let mut vertex = 0_usize;
    while cursor < bytes.len() && vertex < num_verts {
        let mut influences = Vec::new();
        while let Some(&joint) = bytes.get(cursor) {
            cursor = cursor.saturating_add(1);
            if joint == END_INFLUENCES {
                break;
            }
            let Some(chunk) = bytes.get(cursor..cursor.saturating_add(2)) else {
                break;
            };
            cursor = cursor.saturating_add(2);
            let weight = clamp_weight(dequantize(u16_at(chunk, 0), 0.0, 1.0));
            influences.push((joint, weight));
            if influences.len() >= MAX_INFLUENCES {
                break;
            }
        }
        out.push(VertexWeights { influences });
        vertex = vertex.saturating_add(1);
    }
    out
}

/// Decodes a compressed `skin` block into a [`MeshSkin`].
///
/// # Errors
///
/// Returns [`MeshDecodeError`] if the block cannot be inflated, is not binary
/// LLSD, or is not a map.
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `decode_skin` reads clearly"
)]
pub fn decode_skin(compressed: &[u8]) -> Result<MeshSkin, MeshDecodeError> {
    let inflated = inflate(compressed)?;
    let llsd =
        parse_llsd_binary(&inflated).map_err(|error| MeshDecodeError::Llsd(error.to_string()))?;
    llsd.as_map().ok_or(MeshDecodeError::Shape {
        expected: "skin map",
    })?;

    let joint_names = llsd
        .get("joint_names")
        .and_then(Llsd::as_array)
        .map(|array| {
            array
                .iter()
                .filter_map(|value| value.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    let inverse_bind_matrix = matrices(llsd.get("inverse_bind_matrix"));
    let alt_inverse_bind_matrix = matrices(llsd.get("alt_inverse_bind_matrix"));
    let bind_shape_matrix = llsd
        .get("bind_shape_matrix")
        .and_then(Llsd::as_array)
        .map_or_else(identity_matrix, |array| matrix16(array, identity_matrix()));
    let pelvis_offset = llsd.get("pelvis_offset").and_then(Llsd::as_f32);
    let lock_scale_if_joint_position = llsd
        .get("lock_scale_if_joint_position")
        .and_then(Llsd::as_bool)
        .unwrap_or(false);

    Ok(MeshSkin {
        joint_names,
        inverse_bind_matrix,
        bind_shape_matrix,
        alt_inverse_bind_matrix,
        pelvis_offset,
        lock_scale_if_joint_position,
    })
}

/// Decodes a compressed `physics_convex` block into a [`PhysicsConvex`].
///
/// # Errors
///
/// Returns [`MeshDecodeError`] if the block cannot be inflated, is not binary
/// LLSD, or is not a map.
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `decode_physics_convex` reads clearly"
)]
pub fn decode_physics_convex(compressed: &[u8]) -> Result<PhysicsConvex, MeshDecodeError> {
    let inflated = inflate(compressed)?;
    let llsd =
        parse_llsd_binary(&inflated).map_err(|error| MeshDecodeError::Llsd(error.to_string()))?;
    llsd.as_map().ok_or(MeshDecodeError::Shape {
        expected: "physics_convex map",
    })?;

    let (min, max) = convex_domain(&llsd);
    let hulls = decode_hulls(&llsd, min, max);
    let bounding_verts = llsd
        .get("BoundingVerts")
        .and_then(Llsd::as_binary)
        .map(|bytes| dequantize_positions(bytes, min, max))
        .unwrap_or_default();

    Ok(PhysicsConvex {
        hulls,
        bounding_verts,
        min,
        max,
    })
}

/// Reads the convex decomposition domain (`Min`/`Max`), defaulting to `±0.5`.
fn convex_domain(map: &Llsd) -> ([f32; 3], [f32; 3]) {
    let min = vec3(map.get("Min"), [-0.5, -0.5, -0.5]);
    let max = vec3(map.get("Max"), [0.5, 0.5, 0.5]);
    (min, max)
}

/// Decodes the `HullList` / `Positions` convex hulls, dequantizing each hull's
/// points into the `[min, max]` domain.
fn decode_hulls(map: &Llsd, min: [f32; 3], max: [f32; 3]) -> Vec<Vec<[f32; 3]>> {
    /// A `HullList` count byte of `0` means the full 256 points.
    const FULL_HULL: usize = 256;

    let Some(hull_list) = map.get("HullList").and_then(Llsd::as_binary) else {
        return Vec::new();
    };
    let Some(positions) = map.get("Positions").and_then(Llsd::as_binary) else {
        return Vec::new();
    };
    let points = dequantize_positions(positions, min, max);
    let mut hulls = Vec::with_capacity(hull_list.len());
    let mut start = 0_usize;
    for &count in hull_list {
        let count = if count == 0 {
            FULL_HULL
        } else {
            usize::from(count)
        };
        let end = start.saturating_add(count);
        let hull = points.get(start..end).unwrap_or(&[]).to_vec();
        hulls.push(hull);
        start = end;
    }
    hulls
}

/// Inflates a zlib-compressed block, returning its bytes.
fn inflate(compressed: &[u8]) -> Result<Vec<u8>, MeshDecodeError> {
    let mut decoder = ZlibDecoder::new(compressed);
    let mut out = Vec::new();
    decoder
        .read_to_end(&mut out)
        .map_err(|error| MeshDecodeError::Inflate(error.to_string()))?;
    Ok(out)
}

/// Reads a little-endian `u16` at byte offset `at` in `chunk` (0 if out of
/// range), assembled with explicit shifts to satisfy the endian-byte lint.
fn u16_at(chunk: &[u8], at: usize) -> u16 {
    let low = chunk.get(at).copied().map_or(0_u16, u16::from);
    let high = chunk
        .get(at.saturating_add(1))
        .copied()
        .map_or(0_u16, u16::from);
    low | (high << 8_u16)
}

/// Dequantizes a `u16` sample to `[min, max]`: `min + (sample / 65535) *
/// (max - min)`.
fn dequantize(sample: u16, min: f32, max: f32) -> f32 {
    min + (f32::from(sample) / U16_SCALE) * (max - min)
}

/// Clamps a rig weight to the viewer's `[0.001, 0.999]` range.
const fn clamp_weight(weight: f32) -> f32 {
    weight.clamp(0.001, 0.999)
}

/// Reads a `{ Min, Max }` 3-vector domain, each defaulting componentwise.
fn domain(
    value: Option<&Llsd>,
    default_min: [f32; 3],
    default_max: [f32; 3],
) -> ([f32; 3], [f32; 3]) {
    let Some(map) = value else {
        return (default_min, default_max);
    };
    (
        vec3(map.get("Min"), default_min),
        vec3(map.get("Max"), default_max),
    )
}

/// Reads a `{ Min, Max }` 2-vector (texture) domain, each defaulting.
fn domain2(
    value: Option<&Llsd>,
    default_min: [f32; 2],
    default_max: [f32; 2],
) -> ([f32; 2], [f32; 2]) {
    let Some(map) = value else {
        return (default_min, default_max);
    };
    (
        vec2(map.get("Min"), default_min),
        vec2(map.get("Max"), default_max),
    )
}

/// Reads a 3-element real array into `[f32; 3]`, defaulting componentwise.
fn vec3(value: Option<&Llsd>, default: [f32; 3]) -> [f32; 3] {
    let [dx, dy, dz] = default;
    let Some(array) = value.and_then(Llsd::as_array) else {
        return default;
    };
    [
        array.first().and_then(Llsd::as_f32).unwrap_or(dx),
        array.get(1).and_then(Llsd::as_f32).unwrap_or(dy),
        array.get(2).and_then(Llsd::as_f32).unwrap_or(dz),
    ]
}

/// Reads a 2-element real array into `[f32; 2]`, defaulting componentwise.
fn vec2(value: Option<&Llsd>, default: [f32; 2]) -> [f32; 2] {
    let [du, dv] = default;
    let Some(array) = value.and_then(Llsd::as_array) else {
        return default;
    };
    [
        array.first().and_then(Llsd::as_f32).unwrap_or(du),
        array.get(1).and_then(Llsd::as_f32).unwrap_or(dv),
    ]
}

/// Reads an array of 16-float matrices (each an LLSD array of 16 reals).
fn matrices(value: Option<&Llsd>) -> Vec<[f32; 16]> {
    value
        .and_then(Llsd::as_array)
        .map(|array| {
            array
                .iter()
                .filter_map(Llsd::as_array)
                .map(|inner| matrix16(inner, identity_matrix()))
                .collect()
        })
        .unwrap_or_default()
}

/// Reads a 16-element real array into a `[f32; 16]`, defaulting from `fallback`.
fn matrix16(array: &[Llsd], fallback: [f32; 16]) -> [f32; 16] {
    let mut out = fallback;
    for (slot, value) in out.iter_mut().zip(array.iter()) {
        if let Some(number) = value.as_f32() {
            *slot = number;
        }
    }
    out
}

/// The 4×4 identity matrix (row-major, 16 floats).
fn identity_matrix() -> [f32; 16] {
    let mut matrix = [0.0_f32; 16];
    for (index, slot) in matrix.iter_mut().enumerate() {
        if index == 0 || index == 5 || index == 10 || index == 15 {
            *slot = 1.0;
        }
    }
    matrix
}

#[cfg(test)]
mod tests {
    use super::{
        MESH_HEADER_SIZE, MeshHeader, decode_lod, decode_physics_convex, decode_skin, parse_header,
    };
    use flate2::Compression;
    use flate2::write::ZlibEncoder;
    use pretty_assertions::assert_eq;
    use sl_proto::MeshLod;
    use sl_wire::Llsd;
    use std::collections::HashMap;
    use std::io::Write as _;

    /// A boxed error so tests can use `?` instead of disallowed `unwrap`/`expect`.
    type TestError = Box<dyn core::error::Error>;

    /// zlib-compresses `bytes` (raw zlib, as mesh blocks use).
    fn zlib(bytes: &[u8]) -> Result<Vec<u8>, TestError> {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(bytes)?;
        Ok(encoder.finish()?)
    }

    /// A little-endian `u16` blob from a list of values.
    fn u16_blob(values: &[u16]) -> Vec<u8> {
        let mut out = Vec::new();
        for value in values {
            out.push(u8::try_from(value & 0xFF).unwrap_or(0));
            out.push(u8::try_from((value >> 8_u16) & 0xFF).unwrap_or(0));
        }
        out
    }

    /// A `{ Min, Max }` domain map of 3-vectors.
    fn domain3(min: [f32; 3], max: [f32; 3]) -> Llsd {
        let vec = |value: [f32; 3]| {
            Llsd::Array(
                value
                    .into_iter()
                    .map(|component| Llsd::Real(f64::from(component)))
                    .collect(),
            )
        };
        Llsd::Map(HashMap::from([
            ("Min".to_owned(), vec(min)),
            ("Max".to_owned(), vec(max)),
        ]))
    }

    #[test]
    fn parses_header_offsets_from_header_size() -> Result<(), TestError> {
        // A minimal header naming a single high-LOD block at offset 0, size 10.
        let block = Llsd::Map(HashMap::from([
            ("offset".to_owned(), Llsd::Integer(0)),
            ("size".to_owned(), Llsd::Integer(10)),
        ]));
        let header = Llsd::Map(HashMap::from([
            ("version".to_owned(), Llsd::Integer(1)),
            ("high_lod".to_owned(), block),
        ]));
        let mut bytes = header.to_llsd_binary();
        let header_size = bytes.len();
        // Append the (dummy) block bytes past the header.
        bytes.extend_from_slice(&[0_u8; 10]);
        let (parsed, size) = parse_header(&bytes).ok_or("parse header")?;
        assert_eq!(size, header_size);
        assert_eq!(parsed.version, 1);
        let high = parsed.lod(MeshLod::High).ok_or("high lod block")?;
        assert_eq!(high.offset, 0);
        assert_eq!(high.size, 10);
        // The absolute range starts right after the header.
        assert_eq!(high.range(size), (header_size, header_size + 10));
        // No other blocks present.
        assert!(parsed.lod(MeshLod::Low).is_none());
        assert!(parsed.skin.is_none());
        assert!(MESH_HEADER_SIZE >= header_size);
        Ok(())
    }

    #[test]
    fn decodes_a_single_submesh_lod() -> Result<(), TestError> {
        // One face: 3 vertices forming a triangle, positions spanning the domain.
        let positions = u16_blob(&[
            0, 0, 0, // vertex 0 at domain min
            0xFFFF, 0, 0, // vertex 1 at max x
            0, 0xFFFF, 0, // vertex 2 at max y
        ]);
        let indices = u16_blob(&[0, 1, 2]);
        let submesh = Llsd::Map(HashMap::from([
            ("Position".to_owned(), Llsd::Binary(positions)),
            ("TriangleList".to_owned(), Llsd::Binary(indices)),
            (
                "PositionDomain".to_owned(),
                domain3([0.0, 0.0, 0.0], [2.0, 4.0, 1.0]),
            ),
        ]));
        let block = Llsd::Array(vec![submesh]).to_llsd_binary();
        let compressed = zlib(&block)?;
        let decoded = decode_lod(&compressed, MeshLod::High)?;
        assert_eq!(decoded.lod, MeshLod::High);
        assert_eq!(decoded.submeshes.len(), 1);
        let face = decoded.submeshes.first().ok_or("one face")?;
        assert_eq!(face.positions.len(), 3);
        assert_eq!(face.indices, vec![0, 1, 2]);
        assert_eq!(decoded.triangle_count(), 1);
        assert_eq!(decoded.vertex_count(), 3);
        // Vertex 0 dequantizes to the domain minimum, vertex 1 to max x.
        assert_eq!(face.positions.first().copied(), Some([0.0, 0.0, 0.0]));
        assert_eq!(face.positions.get(1).copied(), Some([2.0, 0.0, 0.0]));
        assert_eq!(face.positions.get(2).copied(), Some([0.0, 4.0, 0.0]));
        // Default normalized scale (wrapped in `Some` to sidestep the array
        // float-comparison lint; these dequantized values are exact).
        assert_eq!(Some(face.normalized_scale), Some([1.0_f32, 1.0, 1.0]));
        Ok(())
    }

    #[test]
    fn drops_incomplete_trailing_triangle_indices() -> Result<(), TestError> {
        // Four indices: only the first three form a complete triangle.
        let submesh = Llsd::Map(HashMap::from([
            ("Position".to_owned(), Llsd::Binary(u16_blob(&[0, 0, 0]))),
            (
                "TriangleList".to_owned(),
                Llsd::Binary(u16_blob(&[0, 0, 0, 0])),
            ),
            (
                "PositionDomain".to_owned(),
                domain3([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]),
            ),
        ]));
        let compressed = zlib(&Llsd::Array(vec![submesh]).to_llsd_binary())?;
        let decoded = decode_lod(&compressed, MeshLod::Low)?;
        let face = decoded.submeshes.first().ok_or("face")?;
        assert_eq!(face.indices.len(), 3);
        Ok(())
    }

    #[test]
    fn decodes_skin_joints_and_matrices() -> Result<(), TestError> {
        let identity: Vec<Llsd> = (0..16)
            .map(|index| Llsd::Real(if index % 5 == 0 { 1.0 } else { 0.0 }))
            .collect();
        let skin = Llsd::Map(HashMap::from([
            (
                "joint_names".to_owned(),
                Llsd::Array(vec![
                    Llsd::String("mPelvis".to_owned()),
                    Llsd::String("mTorso".to_owned()),
                ]),
            ),
            (
                "inverse_bind_matrix".to_owned(),
                Llsd::Array(vec![
                    Llsd::Array(identity.clone()),
                    Llsd::Array(identity.clone()),
                ]),
            ),
            ("bind_shape_matrix".to_owned(), Llsd::Array(identity)),
            ("pelvis_offset".to_owned(), Llsd::Real(0.5)),
        ]));
        let compressed = zlib(&skin.to_llsd_binary())?;
        let decoded = decode_skin(&compressed)?;
        assert_eq!(decoded.joint_names, vec!["mPelvis", "mTorso"]);
        assert_eq!(decoded.inverse_bind_matrix.len(), 2);
        assert_eq!(decoded.bind_shape_matrix.first().copied(), Some(1.0));
        assert_eq!(decoded.pelvis_offset, Some(0.5));
        Ok(())
    }

    #[test]
    fn decodes_physics_convex_hulls() -> Result<(), TestError> {
        // One hull of four points, default ±0.5 domain.
        let hull_list = vec![4_u8];
        let positions = u16_blob(&[
            0, 0, 0, // -0.5,-0.5,-0.5
            0xFFFF, 0xFFFF, 0xFFFF, // 0.5,0.5,0.5
            0, 0xFFFF, 0, //
            0xFFFF, 0, 0xFFFF, //
        ]);
        let convex = Llsd::Map(HashMap::from([
            ("HullList".to_owned(), Llsd::Binary(hull_list)),
            ("Positions".to_owned(), Llsd::Binary(positions)),
        ]));
        let compressed = zlib(&convex.to_llsd_binary())?;
        let decoded = decode_physics_convex(&compressed)?;
        assert_eq!(decoded.hulls.len(), 1);
        let hull = decoded.hulls.first().ok_or("one hull")?;
        assert_eq!(hull.len(), 4);
        assert_eq!(hull.first().copied(), Some([-0.5, -0.5, -0.5]));
        assert_eq!(hull.get(1).copied(), Some([0.5, 0.5, 0.5]));
        Ok(())
    }

    #[test]
    fn best_lod_falls_back_to_available_levels() {
        let mut header = MeshHeader::default();
        // Only the medium LOD is present.
        if let Some(slot) = header.lods.get_mut(usize::from(MeshLod::Medium.index())) {
            *slot = Some(super::BlockRef { offset: 0, size: 4 });
        }
        // A request for High falls back to the coarser Medium.
        assert_eq!(header.best_lod(MeshLod::High), Some(MeshLod::Medium));
        // A request for the (absent) Lowest falls up to the finest present.
        assert_eq!(header.best_lod(MeshLod::Lowest), Some(MeshLod::Medium));
        assert_eq!(MeshHeader::default().best_lod(MeshLod::High), None);
    }
}
