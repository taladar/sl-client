//! LLMesh asset **encoding** — the inverse of [`crate::decode`].
//!
//! An asset is an *uncompressed* binary-LLSD header naming each section by a
//! `{ offset, size }` sub-map, followed by the zlib-deflated section bodies.
//! Offsets are measured from the end of the header and are assigned in **write
//! order**: `skin`, `physics_convex`, then `lowest_lod`, `low_lod`,
//! `medium_lod`, `high_lod`, `physics_mesh` — the order Firestorm's
//! `LLModel::writeModelToStream` uses, and the one a reader must not depend on
//! but a writer has to pick.
//!
//! Every block is binary LLSD deflated at zlib level 9 (`Z_BEST_COMPRESSION`,
//! what the reference's `zip_llsd` asks for).
//!
//! Quantization is the exact inverse of [`crate::decode`]'s dequantization:
//!
//! - `Position` — three `u16` per vertex across a **per-model**
//!   `PositionDomain` derived from every face of that level of detail, and
//!   written into every face;
//! - `Normal` — three `u16` per vertex across the fixed `[-1, 1]`;
//! - `TexCoord0` — two `u16` per vertex across a **per-face**
//!   `TexCoord0Domain`;
//! - `TriangleList` — `u16` indices;
//! - `Weights` — per vertex, up to four `(u8 joint, u16 weight)` pairs, with a
//!   `0xFF` terminator only when the list is **shorter** than four (a
//!   four-influence vertex carries none, which is what [`crate::decode`]'s
//!   `decode_weights` expects).
//!
//! Tangents are not transported: the reference's tangent branch is `#if 0`.
//!
//! Unlike the decoder, which is deliberately lenient because it reads what a
//! grid serves, the encoder **rejects** a model it cannot represent rather than
//! emitting an asset the format does not allow — see [`MeshEncodeError`]. The
//! limits it enforces are the wire's and the uploader's, never the *content's*:
//! an unnormalised weight set, a vertex with no influences at all, and an
//! influence naming a joint the `skin` block does not list are all writable,
//! because those are exactly the shapes a skinning test needs a fixture for.
//!
//! Reference (read-only): `indra/llprimitive/llmodel.cpp` — `writeModel`,
//! `writeModelToStream`, `LLMeshSkinInfo::asLLSD`,
//! `LLModel::Decomposition::asLLSD`.

use std::collections::HashMap;
use std::io::Write as _;

use flate2::Compression;
use flate2::write::ZlibEncoder;
use sl_proto::{MESH_LOD_COUNT, MeshLod};
use sl_wire::Llsd;

use crate::decode::{MeshSkin, PhysicsConvex, Submesh, VertexWeights};

/// The `65535` quantization scale, matching [`crate::decode`]'s divisor.
const U16_SCALE: f32 = 65535.0;

/// The header `version` a served mesh asset carries.
///
/// The reference *viewer* writes no `version` at all — the asset service adds
/// one when it stores the upload — so this is what a fetch sees rather than
/// what `writeModelToStream` emits.
pub const DEFAULT_MESH_VERSION: u32 = 1;

/// The reference's `MAX_MODEL_FACES`: a prim has eight texture faces, so a
/// model may not carry more submeshes than that.
pub const MAX_MODEL_FACES: usize = 8;

/// The largest vertex count one face may carry, bounded by the `u16` triangle
/// index space.
pub const MAX_VERTICES_PER_FACE: usize = 0x1_0000;

/// The reference's `LL_MAX_JOINTS_PER_MESH_OBJECT`: the skinning shader's
/// matrix palette holds 110 joints.
pub const MAX_JOINTS: usize = 110;

/// The largest joint index an influence may name. `0xFF` terminates an
/// influence list, and `writeModel` writes only indices `< 255`.
pub const MAX_JOINT_INDEX: u8 = 254;

/// The largest number of influences one vertex may carry.
pub const MAX_INFLUENCES: usize = 4;

/// The largest number of convex hulls a `physics_convex` block may carry.
pub const MAX_HULLS: usize = 256;

/// The largest number of points one convex hull may carry. A `HullList` count
/// byte of `0` means exactly this many.
pub const MAX_HULL_POINTS: usize = 256;

/// A model that cannot be written as a valid LLMesh asset.
///
/// Every variant is a *format* or *upload* limit, not a content judgement: an
/// asset that violates one of these would either fail to decode or be refused
/// by an upload, so emitting it would be worse than refusing to.
#[derive(Debug, thiserror::Error)]
pub enum MeshEncodeError {
    /// The model carries no `high_lod`, which every mesh asset must have.
    #[error("a mesh asset must carry a high_lod block")]
    MissingHighLod,
    /// A level of detail is present while the finer one above it is not.
    /// The reference's LOD chain is contiguous from `high_lod` downwards.
    #[error("level of detail `{present}` is present but `{missing}` above it is not")]
    LodGap {
        /// The header key of the level that is present.
        present: &'static str,
        /// The header key of the missing level above it.
        missing: &'static str,
    },
    /// A geometry block carried no faces at all.
    #[error("level of detail `{lod}` carries no faces")]
    EmptyLod {
        /// The header key of the empty level.
        lod: &'static str,
    },
    /// A coarser level of detail carries more vertices than the finer one
    /// above it — a level is a *decimation* of the one above.
    #[error(
        "level of detail `{coarser}` has {coarser_vertices} vertices, more than \
         the {finer_vertices} of `{finer}` above it"
    )]
    LodNotCoarser {
        /// The header key of the offending coarser level.
        coarser: &'static str,
        /// Its vertex count.
        coarser_vertices: usize,
        /// The header key of the finer level above it.
        finer: &'static str,
        /// That level's vertex count.
        finer_vertices: usize,
    },
    /// A model carried more submeshes than a prim has texture faces.
    #[error("a model may carry at most {MAX_MODEL_FACES} faces, not {faces}")]
    TooManyFaces {
        /// The offending face count.
        faces: usize,
    },
    /// A face carried more vertices than the `u16` index space addresses.
    #[error("a face may carry at most {MAX_VERTICES_PER_FACE} vertices, not {vertices}")]
    TooManyVertices {
        /// The offending vertex count.
        vertices: usize,
    },
    /// A per-vertex stream did not have one entry per vertex.
    #[error("the `{stream}` stream has {length} entries for {vertices} vertices")]
    StreamLengthMismatch {
        /// The name of the stream (`Normal`, `TexCoord0`, `Weights`).
        stream: &'static str,
        /// The length that was supplied.
        length: usize,
        /// The vertex count it should have matched.
        vertices: usize,
    },
    /// A triangle list was not a whole number of triangles.
    #[error("a triangle list must be a multiple of 3, not {indices} indices")]
    RaggedTriangleList {
        /// The offending index count.
        indices: usize,
    },
    /// A triangle named a vertex the face does not have.
    #[error("a triangle names vertex {index} of a {vertices}-vertex face")]
    IndexOutOfRange {
        /// The offending index.
        index: u32,
        /// The face's vertex count.
        vertices: usize,
    },
    /// A vertex carried more influences than the format transports.
    #[error("a vertex may carry at most {MAX_INFLUENCES} influences, not {influences}")]
    TooManyInfluences {
        /// The offending influence count.
        influences: usize,
    },
    /// An influence named a joint index the format reserves.
    #[error("a joint index may be at most {MAX_JOINT_INDEX}, not {joint}")]
    JointIndexOutOfRange {
        /// The offending joint index.
        joint: u8,
    },
    /// The skin block named more joints than the skinning shader binds.
    #[error("a skin may name at most {MAX_JOINTS} joints, not {joints}")]
    TooManyJoints {
        /// The offending joint count.
        joints: usize,
    },
    /// The convex decomposition carried neither hulls nor a bounding hull.
    #[error("a physics_convex block must carry a hull or a bounding hull")]
    EmptyPhysicsConvex,
    /// The convex decomposition carried more hulls than the format allows.
    #[error("a decomposition may carry at most {MAX_HULLS} hulls, not {hulls}")]
    TooManyHulls {
        /// The offending hull count.
        hulls: usize,
    },
    /// A convex hull carried more points than its `HullList` byte can count.
    #[error("a hull may carry at most {MAX_HULL_POINTS} points, not {points}")]
    HullTooLarge {
        /// The offending point count.
        points: usize,
    },
    /// A convex hull carried no points, which the reference asserts against.
    #[error("a convex hull must carry at least one point")]
    EmptyHull,
    /// A block or the asset as a whole outgrew the header's `i32` offsets.
    #[error("the encoded asset outgrew the header's 32-bit offsets")]
    AssetTooLarge,
    /// The zlib deflate of a block failed — writing to a `Vec` cannot produce
    /// this, so it is here for completeness rather than as a live path.
    #[error("mesh block zlib deflate failed: {0}")]
    Deflate(String),
}

/// A mesh ready to be written as an LLMesh asset: the geometry levels of
/// detail, and the optional `skin` and physics blocks.
///
/// The field types are [`crate::decode`]'s, so a decoded asset can be edited
/// and written back — the two halves share one model rather than one each.
#[derive(Clone, Debug)]
pub struct MeshModel {
    /// The header `version` to write (see [`DEFAULT_MESH_VERSION`]).
    pub version: u32,
    /// The geometry levels of detail, indexed by [`MeshLod::index`]. `high_lod`
    /// is required and each coarser level requires the one above it.
    pub lods: [Option<Vec<Submesh>>; MESH_LOD_COUNT],
    /// The rigging block, if the mesh is rigged.
    pub skin: Option<MeshSkin>,
    /// The convex-hull collision decomposition, if any.
    pub physics_convex: Option<PhysicsConvex>,
    /// The triangle collision mesh, if any.
    pub physics_mesh: Option<Vec<Submesh>>,
}

impl Default for MeshModel {
    fn default() -> Self {
        Self {
            version: DEFAULT_MESH_VERSION,
            lods: [None, None, None, None],
            skin: None,
            physics_convex: None,
            physics_mesh: None,
        }
    }
}

impl MeshModel {
    /// A model carrying `high_lod` geometry and nothing else.
    #[must_use]
    pub fn new(high_lod: Vec<Submesh>) -> Self {
        Self::default().with_lod(MeshLod::High, high_lod)
    }

    /// Sets one level of detail's geometry.
    #[must_use]
    pub fn with_lod(mut self, lod: MeshLod, submeshes: Vec<Submesh>) -> Self {
        if let Some(slot) = self.lods.get_mut(usize::from(lod.index())) {
            *slot = Some(submeshes);
        }
        self
    }

    /// Sets the same geometry for every level of detail, so the mesh renders
    /// identically however aggressively a viewer picks its level.
    #[must_use]
    pub fn with_every_lod(mut self, submeshes: Vec<Submesh>) -> Self {
        for lod in MeshLod::ALL {
            self = self.with_lod(lod, submeshes.clone());
        }
        self
    }

    /// Sets the rigging block.
    #[must_use]
    pub fn with_skin(mut self, skin: MeshSkin) -> Self {
        self.skin = Some(skin);
        self
    }

    /// Sets the convex-hull collision decomposition.
    #[must_use]
    pub fn with_physics_convex(mut self, convex: PhysicsConvex) -> Self {
        self.physics_convex = Some(convex);
        self
    }

    /// Sets the triangle collision mesh.
    #[must_use]
    pub fn with_physics_mesh(mut self, submeshes: Vec<Submesh>) -> Self {
        self.physics_mesh = Some(submeshes);
        self
    }

    /// Sets the header `version`.
    #[must_use]
    pub const fn with_version(mut self, version: u32) -> Self {
        self.version = version;
        self
    }

    /// The geometry at one level of detail, if present.
    #[must_use]
    pub fn lod(&self, lod: MeshLod) -> Option<&[Submesh]> {
        self.lods
            .get(usize::from(lod.index()))
            .and_then(Option::as_deref)
    }
}

/// Serialises `model` into a complete LLMesh asset.
///
/// # Errors
///
/// Returns [`MeshEncodeError`] if the model breaks a format or upload limit —
/// see that type for the whole list.
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `encode_mesh` reads clearly"
)]
pub fn encode_mesh(model: &MeshModel) -> Result<Vec<u8>, MeshEncodeError> {
    check_lod_chain(model)?;

    // Write order fixes the offsets, so the sections are assembled in it.
    let mut sections: Vec<(&'static str, Vec<u8>)> = Vec::new();
    if let Some(skin) = model.skin.as_ref() {
        sections.push(("skin", encode_skin_block(skin)?));
    }
    if let Some(convex) = model.physics_convex.as_ref() {
        sections.push(("physics_convex", encode_physics_convex_block(convex)?));
    }
    for lod in MeshLod::ALL {
        if let Some(submeshes) = model.lod(lod) {
            sections.push((lod.header_key(), encode_lod_block(submeshes)?));
        }
    }
    if let Some(submeshes) = model.physics_mesh.as_ref() {
        sections.push(("physics_mesh", encode_lod_block(submeshes)?));
    }

    let mut header: HashMap<String, Llsd> = HashMap::new();
    let version = i32::try_from(model.version).map_err(|_error| MeshEncodeError::AssetTooLarge)?;
    let _previous = header.insert("version".to_owned(), Llsd::Integer(version));
    let mut offset: usize = 0;
    for &(key, ref bytes) in &sections {
        let _previous = header.insert(key.to_owned(), block_ref(offset, bytes.len())?);
        offset = offset
            .checked_add(bytes.len())
            .ok_or(MeshEncodeError::AssetTooLarge)?;
    }

    let mut asset = Llsd::Map(header).to_llsd_binary();
    for (_, bytes) in &sections {
        asset.extend_from_slice(bytes);
    }
    Ok(asset)
}

/// A `{ offset, size }` header sub-map, refusing a range the header's `i32`
/// fields cannot express.
fn block_ref(offset: usize, size: usize) -> Result<Llsd, MeshEncodeError> {
    let offset = i32::try_from(offset).map_err(|_error| MeshEncodeError::AssetTooLarge)?;
    let size = i32::try_from(size).map_err(|_error| MeshEncodeError::AssetTooLarge)?;
    Ok(Llsd::Map(HashMap::from([
        ("offset".to_owned(), Llsd::Integer(offset)),
        ("size".to_owned(), Llsd::Integer(size)),
    ])))
}

/// Checks that `high_lod` is present, that the levels below it are contiguous,
/// and that each coarser level decimates rather than refines the one above.
fn check_lod_chain(model: &MeshModel) -> Result<(), MeshEncodeError> {
    if model.lod(MeshLod::High).is_none() {
        return Err(MeshEncodeError::MissingHighLod);
    }
    // Walk finest to coarsest: once a level is absent, no coarser one may be
    // present, and each present level must be no finer than the one above.
    let mut finer: Option<(&'static str, usize)> = None;
    for lod in MeshLod::ALL.into_iter().rev() {
        let Some(submeshes) = model.lod(lod) else {
            finer = None;
            continue;
        };
        if submeshes.is_empty() {
            return Err(MeshEncodeError::EmptyLod {
                lod: lod.header_key(),
            });
        }
        let vertices = vertex_count(submeshes);
        if lod != MeshLod::High {
            let Some((finer_key, finer_vertices)) = finer else {
                return Err(MeshEncodeError::LodGap {
                    present: lod.header_key(),
                    missing: lod.finer().header_key(),
                });
            };
            if vertices > finer_vertices {
                return Err(MeshEncodeError::LodNotCoarser {
                    coarser: lod.header_key(),
                    coarser_vertices: vertices,
                    finer: finer_key,
                    finer_vertices,
                });
            }
        }
        finer = Some((lod.header_key(), vertices));
    }
    Ok(())
}

/// The total vertex count across a level's faces.
fn vertex_count(submeshes: &[Submesh]) -> usize {
    submeshes.iter().fold(0_usize, |total, submesh| {
        total.saturating_add(submesh.positions.len())
    })
}

/// Encodes a geometry block — one level of detail, or the `physics_mesh` — as
/// the deflated binary-LLSD array of faces the header names.
///
/// The `PositionDomain` is derived from every face of the block and written
/// into each of them, matching `writeModel`; the `TexCoord0Domain` is derived
/// per face.
///
/// # Errors
///
/// Returns [`MeshEncodeError`] if the block breaks a face, vertex, index or
/// influence limit.
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `encode_lod_block` reads clearly"
)]
pub fn encode_lod_block(submeshes: &[Submesh]) -> Result<Vec<u8>, MeshEncodeError> {
    if submeshes.len() > MAX_MODEL_FACES {
        return Err(MeshEncodeError::TooManyFaces {
            faces: submeshes.len(),
        });
    }
    let (min, max) = position_domain(submeshes);
    let faces = submeshes
        .iter()
        .map(|submesh| encode_submesh(submesh, min, max))
        .collect::<Result<Vec<Llsd>, MeshEncodeError>>()?;
    deflate(&Llsd::Array(faces).to_llsd_binary())
}

/// The bounding box of every position in a block, the domain its quantized
/// positions are taken over. Faces without geometry still contribute, as they
/// do in `writeModel`, whose domain pass runs before the face loop skips them.
fn position_domain(submeshes: &[Submesh]) -> ([f32; 3], [f32; 3]) {
    bounds(submeshes.iter().flat_map(|submesh| &submesh.positions))
}

/// The componentwise bounding box of a stream of points, or the origin box if
/// the stream is empty.
fn bounds<'a, const N: usize>(points: impl Iterator<Item = &'a [f32; N]>) -> ([f32; N], [f32; N]) {
    let mut min = [0.0_f32; N];
    let mut max = [0.0_f32; N];
    let mut seen = false;
    for point in points {
        if seen {
            for ((low, high), &value) in min.iter_mut().zip(max.iter_mut()).zip(point) {
                *low = low.min(value);
                *high = high.max(value);
            }
        } else {
            min = *point;
            max = *point;
            seen = true;
        }
    }
    (min, max)
}

/// Encodes one face: its quantized streams, the domains they were taken over,
/// and the triangle list.
fn encode_submesh(
    submesh: &Submesh,
    pos_min: [f32; 3],
    pos_max: [f32; 3],
) -> Result<Llsd, MeshEncodeError> {
    let vertices = submesh.positions.len();
    // The reference writes a bare `NoGeometry` face for anything that cannot
    // make a triangle, and so does the decoder's `no_geometry` marker.
    if submesh.no_geometry || vertices < 3 {
        return Ok(Llsd::Map(HashMap::from([(
            "NoGeometry".to_owned(),
            Llsd::Boolean(true),
        )])));
    }
    if vertices > MAX_VERTICES_PER_FACE {
        return Err(MeshEncodeError::TooManyVertices { vertices });
    }

    let mut face: HashMap<String, Llsd> = HashMap::new();
    let _previous = face.insert("PositionDomain".to_owned(), domain(&pos_min, &pos_max));
    let _previous = face.insert(
        "NormalizedScale".to_owned(),
        reals(&submesh.normalized_scale),
    );

    let mut positions = Vec::with_capacity(vertices.saturating_mul(6));
    for position in &submesh.positions {
        for (component, (low, high)) in position.iter().zip(pos_min.into_iter().zip(pos_max)) {
            push_u16(&mut positions, quantize(*component, low, high));
        }
    }
    let _previous = face.insert("Position".to_owned(), Llsd::Binary(positions));

    if !submesh.normals.is_empty() {
        if submesh.normals.len() != vertices {
            return Err(MeshEncodeError::StreamLengthMismatch {
                stream: "Normal",
                length: submesh.normals.len(),
                vertices,
            });
        }
        let mut normals = Vec::with_capacity(vertices.saturating_mul(6));
        for normal in &submesh.normals {
            for component in normal {
                push_u16(&mut normals, quantize(*component, -1.0, 1.0));
            }
        }
        let _previous = face.insert("Normal".to_owned(), Llsd::Binary(normals));
    }

    if !submesh.uvs.is_empty() {
        if submesh.uvs.len() != vertices {
            return Err(MeshEncodeError::StreamLengthMismatch {
                stream: "TexCoord0",
                length: submesh.uvs.len(),
                vertices,
            });
        }
        let (uv_min, uv_max) = uv_domain(&submesh.uvs);
        let mut uvs = Vec::with_capacity(vertices.saturating_mul(4));
        for uv in &submesh.uvs {
            for (component, (low, high)) in uv.iter().zip(uv_min.into_iter().zip(uv_max)) {
                push_u16(&mut uvs, quantize(*component, low, high));
            }
        }
        let _previous = face.insert("TexCoord0Domain".to_owned(), domain(&uv_min, &uv_max));
        let _previous = face.insert("TexCoord0".to_owned(), Llsd::Binary(uvs));
    }

    let _previous = face.insert(
        "TriangleList".to_owned(),
        Llsd::Binary(encode_indices(&submesh.indices, vertices)?),
    );

    if let Some(weights) = submesh.weights.as_ref() {
        if weights.len() != vertices {
            return Err(MeshEncodeError::StreamLengthMismatch {
                stream: "Weights",
                length: weights.len(),
                vertices,
            });
        }
        let _previous = face.insert("Weights".to_owned(), Llsd::Binary(encode_weights(weights)?));
    }

    Ok(Llsd::Map(face))
}

/// The bounding rectangle of a face's texture coordinates.
fn uv_domain(uvs: &[[f32; 2]]) -> ([f32; 2], [f32; 2]) {
    bounds(uvs.iter())
}

/// Encodes a triangle list as little-endian `u16` indices, refusing a ragged
/// list or one naming a vertex the face does not have — either would decode
/// into a triangle the renderer cannot draw.
fn encode_indices(indices: &[u32], vertices: usize) -> Result<Vec<u8>, MeshEncodeError> {
    if indices.len().checked_rem(3) != Some(0) {
        return Err(MeshEncodeError::RaggedTriangleList {
            indices: indices.len(),
        });
    }
    let limit = u32::try_from(vertices).unwrap_or(u32::MAX);
    let mut out = Vec::with_capacity(indices.len().saturating_mul(2));
    for &index in indices {
        if index >= limit {
            return Err(MeshEncodeError::IndexOutOfRange { index, vertices });
        }
        let packed = u16::try_from(index)
            .map_err(|_error| MeshEncodeError::IndexOutOfRange { index, vertices })?;
        push_u16(&mut out, packed);
    }
    Ok(out)
}

/// Encodes the per-vertex influence stream: `(u8 joint, u16 weight)` pairs,
/// terminated by `0xFF` only when the vertex carries fewer than four.
fn encode_weights(weights: &[VertexWeights]) -> Result<Vec<u8>, MeshEncodeError> {
    /// The end-of-influences sentinel joint byte.
    const END_INFLUENCES: u8 = 0xFF;

    let mut out = Vec::new();
    for vertex in weights {
        if vertex.influences.len() > MAX_INFLUENCES {
            return Err(MeshEncodeError::TooManyInfluences {
                influences: vertex.influences.len(),
            });
        }
        for &(joint, weight) in &vertex.influences {
            if joint > MAX_JOINT_INDEX {
                return Err(MeshEncodeError::JointIndexOutOfRange { joint });
            }
            out.push(joint);
            push_u16(&mut out, quantize(weight, 0.0, 1.0));
        }
        if vertex.influences.len() < MAX_INFLUENCES {
            out.push(END_INFLUENCES);
        }
    }
    Ok(out)
}

/// Encodes a `skin` block as deflated binary LLSD.
///
/// The inverse-bind list is padded with identity matrices to the joint count:
/// the reference's reader **drops the whole rig** when the two disagree, so a
/// short list would silently unrig the mesh rather than partially rig it.
///
/// # Errors
///
/// Returns [`MeshEncodeError::TooManyJoints`] if the skin names more joints
/// than the skinning shader binds.
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `encode_skin_block` reads clearly"
)]
pub fn encode_skin_block(skin: &MeshSkin) -> Result<Vec<u8>, MeshEncodeError> {
    if skin.joint_names.len() > MAX_JOINTS {
        return Err(MeshEncodeError::TooManyJoints {
            joints: skin.joint_names.len(),
        });
    }
    let mut map: HashMap<String, Llsd> = HashMap::new();
    let _previous = map.insert(
        "joint_names".to_owned(),
        Llsd::Array(
            skin.joint_names
                .iter()
                .map(|name| Llsd::String(name.clone()))
                .collect(),
        ),
    );
    let _previous = map.insert(
        "inverse_bind_matrix".to_owned(),
        matrices(&skin.inverse_bind_matrix, skin.joint_names.len()),
    );
    let _previous = map.insert(
        "bind_shape_matrix".to_owned(),
        reals(&skin.bind_shape_matrix),
    );
    if !skin.alt_inverse_bind_matrix.is_empty() {
        let _previous = map.insert(
            "alt_inverse_bind_matrix".to_owned(),
            matrices(&skin.alt_inverse_bind_matrix, skin.joint_names.len()),
        );
    }
    if skin.lock_scale_if_joint_position {
        let _previous = map.insert(
            "lock_scale_if_joint_position".to_owned(),
            Llsd::Boolean(true),
        );
    }
    if let Some(offset) = skin.pelvis_offset {
        let _previous = map.insert("pelvis_offset".to_owned(), Llsd::Real(f64::from(offset)));
    }
    deflate(&Llsd::Map(map).to_llsd_binary())
}

/// An LLSD array of 16-float matrices, padded to `count` with identities.
fn matrices(source: &[[f32; 16]], count: usize) -> Llsd {
    let identity = identity_matrix();
    Llsd::Array(
        (0..count.max(source.len()))
            .map(|index| reals(source.get(index).unwrap_or(&identity)))
            .collect(),
    )
}

/// The 4×4 identity matrix (row-major, 16 floats).
fn identity_matrix() -> [f32; 16] {
    let mut matrix = [0.0_f32; 16];
    for (index, slot) in matrix.iter_mut().enumerate() {
        if index.checked_rem(5) == Some(0) {
            *slot = 1.0;
        }
    }
    matrix
}

/// Encodes a `physics_convex` block as deflated binary LLSD.
///
/// The quantization domain is **derived** from the hull and bounding-hull
/// points rather than taken from [`PhysicsConvex::min`] / [`PhysicsConvex::max`],
/// exactly as `Decomposition::asLLSD` does — a caller building a decomposition
/// by hand would otherwise have to keep a redundant bounding box in step with
/// its own points, and a default one would collapse every point to the origin.
///
/// # Errors
///
/// Returns [`MeshEncodeError`] if the decomposition is empty or breaks a hull
/// count or size limit.
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `encode_physics_convex_block` reads clearly"
)]
pub fn encode_physics_convex_block(convex: &PhysicsConvex) -> Result<Vec<u8>, MeshEncodeError> {
    if convex.hulls.is_empty() && convex.bounding_verts.is_empty() {
        return Err(MeshEncodeError::EmptyPhysicsConvex);
    }
    if convex.hulls.len() > MAX_HULLS {
        return Err(MeshEncodeError::TooManyHulls {
            hulls: convex.hulls.len(),
        });
    }

    let (min, max) = convex_domain(convex);
    let mut map: HashMap<String, Llsd> = HashMap::new();
    let _previous = map.insert("Min".to_owned(), reals(&min));
    let _previous = map.insert("Max".to_owned(), reals(&max));

    if !convex.hulls.is_empty() {
        let mut hull_list = Vec::with_capacity(convex.hulls.len());
        let mut positions = Vec::new();
        for hull in &convex.hulls {
            if hull.is_empty() {
                return Err(MeshEncodeError::EmptyHull);
            }
            if hull.len() > MAX_HULL_POINTS {
                return Err(MeshEncodeError::HullTooLarge { points: hull.len() });
            }
            // A count byte of `0` is the full 256 points, which is how the
            // decoder reads it back.
            hull_list.push(u8::try_from(hull.len()).unwrap_or(0));
            push_points(&mut positions, hull, min, max);
        }
        let _previous = map.insert("HullList".to_owned(), Llsd::Binary(hull_list));
        let _previous = map.insert("Positions".to_owned(), Llsd::Binary(positions));
    }

    if !convex.bounding_verts.is_empty() {
        let mut bounding = Vec::new();
        push_points(&mut bounding, &convex.bounding_verts, min, max);
        let _previous = map.insert("BoundingVerts".to_owned(), Llsd::Binary(bounding));
    }

    deflate(&Llsd::Map(map).to_llsd_binary())
}

/// The bounding box of every hull point and bounding-hull point.
fn convex_domain(convex: &PhysicsConvex) -> ([f32; 3], [f32; 3]) {
    bounds(
        convex
            .hulls
            .iter()
            .flatten()
            .chain(convex.bounding_verts.iter()),
    )
}

/// Appends `points` as quantized `u16` triples over the `[min, max]` domain.
fn push_points(out: &mut Vec<u8>, points: &[[f32; 3]], min: [f32; 3], max: [f32; 3]) {
    for point in points {
        for (component, (low, high)) in point.iter().zip(min.into_iter().zip(max)) {
            push_u16(out, quantize(*component, low, high));
        }
    }
}

/// A `{ Min, Max }` domain map over vectors of any width.
fn domain(min: &[f32], max: &[f32]) -> Llsd {
    Llsd::Map(HashMap::from([
        ("Min".to_owned(), reals(min)),
        ("Max".to_owned(), reals(max)),
    ]))
}

/// An LLSD array of reals.
fn reals(values: &[f32]) -> Llsd {
    Llsd::Array(
        values
            .iter()
            .map(|value| Llsd::Real(f64::from(*value)))
            .collect(),
    )
}

/// zlib-deflates `bytes` at level 9, the `Z_BEST_COMPRESSION` the reference's
/// `zip_llsd` asks for.
fn deflate(bytes: &[u8]) -> Result<Vec<u8>, MeshEncodeError> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
    encoder
        .write_all(bytes)
        .map_err(|error| MeshEncodeError::Deflate(error.to_string()))?;
    encoder
        .finish()
        .map_err(|error| MeshEncodeError::Deflate(error.to_string()))
}

/// Appends `value` little-endian, assembled with explicit shifts because the
/// crate lints forbid the `to_le_bytes` family.
fn push_u16(out: &mut Vec<u8>, value: u16) {
    out.push(u8::try_from(value & 0xFF).unwrap_or(0));
    out.push(u8::try_from(value >> 8_u16).unwrap_or(0));
}

/// Quantizes `value` to a `u16` across `[min, max]` — the inverse of
/// [`crate::decode`]'s `dequantize`, rounding to nearest rather than
/// truncating so a round trip lands on the sample it started from.
///
/// A degenerate (zero-width or non-finite) domain quantizes to `0`, which
/// dequantizes back to `min` — the one value such a domain holds. The
/// reference divides by the zero range instead, and casts the resulting
/// infinity.
fn quantize(value: f32, min: f32, max: f32) -> u16 {
    let span = max - min;
    if span <= 0.0 {
        return 0;
    }
    round_to_u16(((value - min) / span).clamp(0.0, 1.0) * U16_SCALE)
}

/// Rounds a value already clamped into `0..=65535` to its `u16`.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the value is clamped into 0..=65535 before the cast; no From impl exists"
)]
const fn round_to_u16(value: f32) -> u16 {
    value.round().clamp(0.0, U16_SCALE) as u16
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_proto::MeshLod;

    use super::{
        MAX_HULL_POINTS, MAX_HULLS, MAX_INFLUENCES, MAX_JOINT_INDEX, MAX_JOINTS, MAX_MODEL_FACES,
        MeshEncodeError, MeshModel, encode_mesh,
    };
    use crate::decode::{
        DecodedMesh, MeshPhysics, MeshSkin, PhysicsConvex, Submesh, VertexWeights, decode_lod,
        decode_physics_convex, decode_skin, parse_header,
    };

    /// A boxed error so tests can use `?` instead of disallowed `unwrap`/`expect`.
    type TestError = Box<dyn core::error::Error>;

    /// The largest error a `u16` quantization over a domain of width `span` can
    /// leave behind: half a step, plus room for the `f32` round trip.
    fn tolerance(span: f32) -> f32 {
        span / super::U16_SCALE
    }

    /// A right triangle in the unit box, with normals, UVs and a triangle list.
    fn triangle() -> Submesh {
        Submesh {
            positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 2.0, 0.5]],
            normals: vec![[0.0, 0.0, 1.0], [0.0, 0.0, 1.0], [0.0, 0.0, 1.0]],
            uvs: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
            indices: vec![0, 1, 2],
            weights: None,
            normalized_scale: [1.0, 1.0, 1.0],
            no_geometry: false,
        }
    }

    /// A degenerate one-vertex face, which the format writes as `NoGeometry`.
    fn stub_face() -> Submesh {
        Submesh {
            positions: vec![[0.0, 0.0, 0.0]],
            ..Submesh::default()
        }
    }

    /// Everything an encoded asset decodes back into.
    struct Decoded {
        /// One entry per level of detail, indexed by [`MeshLod::index`].
        lods: Vec<Option<DecodedMesh>>,
        /// The `skin` block, if the asset carried one.
        skin: Option<MeshSkin>,
        /// The two physics blocks, if the asset carried them.
        physics: MeshPhysics,
    }

    /// Decodes an encoded asset back into its header and every block it names.
    fn round_trip(asset: &[u8]) -> Result<Decoded, TestError> {
        let (header, header_size) = parse_header(asset).ok_or("no mesh header")?;
        let block = |reference: Option<crate::decode::BlockRef>| -> Option<&[u8]> {
            let reference = reference?;
            let (start, end) = reference.range(header_size);
            asset.get(start..end)
        };
        let mut lods = Vec::new();
        for lod in MeshLod::ALL {
            lods.push(match block(header.lod(lod)) {
                Some(bytes) => Some(decode_lod(bytes, lod)?),
                None => None,
            });
        }
        let skin = match block(header.skin) {
            Some(bytes) => Some(decode_skin(bytes)?),
            None => None,
        };
        let physics = MeshPhysics {
            convex: match block(header.physics_convex) {
                Some(bytes) => Some(decode_physics_convex(bytes)?),
                None => None,
            },
            mesh: match block(header.physics_mesh) {
                Some(bytes) => Some(crate::decode::decode_physics_mesh(bytes)?),
                None => None,
            },
        };
        Ok(Decoded {
            lods,
            skin,
            physics,
        })
    }

    /// The whole point of the encoder: what it writes, the decoder reads back
    /// as what it was handed — positions, normals and UVs to within the `u16`
    /// quantization step, indices exactly.
    #[test]
    fn a_geometry_block_round_trips_through_the_decoder() -> Result<(), TestError> {
        let face = triangle();
        let asset = encode_mesh(&MeshModel::new(vec![face.clone()]))?;
        let Decoded {
            lods,
            skin,
            physics,
        } = round_trip(&asset)?;
        assert!(skin.is_none(), "no skin was supplied");
        assert!(physics.convex.is_none() && physics.mesh.is_none());

        let high = lods
            .get(usize::from(MeshLod::High.index()))
            .and_then(Option::as_ref)
            .ok_or("no high lod")?;
        let decoded = high.submeshes.first().ok_or("no face")?;
        assert_eq!(decoded.indices, face.indices);
        assert_eq!(decoded.positions.len(), face.positions.len());

        // The position domain spans [0, 1] x [0, 2] x [0, 0.5], so each axis
        // gets its own quantization step.
        for (got, want) in decoded.positions.iter().zip(&face.positions) {
            for ((component, expected), span) in got.iter().zip(want).zip([1.0_f32, 2.0, 0.5]) {
                assert!(
                    (component - expected).abs() <= tolerance(span),
                    "position {got:?} != {want:?}"
                );
            }
        }
        for (got, want) in decoded.normals.iter().zip(&face.normals) {
            for (component, expected) in got.iter().zip(want) {
                assert!(
                    (component - expected).abs() <= tolerance(2.0),
                    "normal {got:?} != {want:?}"
                );
            }
        }
        for (got, want) in decoded.uvs.iter().zip(&face.uvs) {
            for (component, expected) in got.iter().zip(want) {
                assert!(
                    (component - expected).abs() <= tolerance(1.0),
                    "uv {got:?} != {want:?}"
                );
            }
        }
        // Every LOD entry that was not supplied is absent from the header.
        for lod in [MeshLod::Lowest, MeshLod::Low, MeshLod::Medium] {
            assert!(
                lods.get(usize::from(lod.index()))
                    .and_then(Option::as_ref)
                    .is_none(),
                "{} should be absent",
                lod.header_key()
            );
        }
        Ok(())
    }

    /// A face that cannot make a triangle is written as the bare `NoGeometry`
    /// marker, exactly as `writeModel` does, and decodes back as one.
    #[test]
    fn a_degenerate_face_becomes_a_no_geometry_marker() -> Result<(), TestError> {
        let asset = encode_mesh(&MeshModel::new(vec![triangle(), stub_face()]))?;
        let Decoded { lods, .. } = round_trip(&asset)?;
        let high = lods
            .get(usize::from(MeshLod::High.index()))
            .and_then(Option::as_ref)
            .ok_or("no high lod")?;
        assert_eq!(high.submeshes.len(), 2);
        let stub = high.submeshes.get(1).ok_or("no second face")?;
        assert!(stub.no_geometry);
        assert!(!stub.has_geometry());
        Ok(())
    }

    /// The influence stream is the contract the decoder already implements: a
    /// four-influence vertex carries **no** terminator, a shorter list does,
    /// and a vertex with none at all is just a terminator. An influence naming
    /// a joint the skin does not list survives too — the pathological fixtures
    /// depend on it.
    #[test]
    fn rig_weights_round_trip_including_their_edge_cases() -> Result<(), TestError> {
        let weights = vec![
            // Four influences: the no-terminator case.
            VertexWeights {
                influences: vec![(0, 0.4), (1, 0.3), (2, 0.2), (3, 0.1)],
            },
            // One influence, and an unnormalised one at that.
            VertexWeights {
                influences: vec![(1, 0.5)],
            },
            // No influences at all, naming a joint index past the skin's list.
            VertexWeights {
                influences: Vec::new(),
            },
        ];
        let mut face = triangle();
        face.weights = Some(weights.clone());
        let skin = MeshSkin {
            joint_names: vec!["mPelvis".to_owned(), "mTorso".to_owned()],
            ..MeshSkin::default()
        };
        let asset = encode_mesh(&MeshModel::new(vec![face]).with_skin(skin))?;
        let Decoded {
            lods,
            skin: decoded_skin,
            ..
        } = round_trip(&asset)?;
        let decoded_skin = decoded_skin.ok_or("no skin block")?;
        assert_eq!(decoded_skin.joint_names, vec!["mPelvis", "mTorso"]);
        // The inverse-bind list is padded to the joint count, or the reference
        // reader drops the whole rig.
        assert_eq!(decoded_skin.inverse_bind_matrix.len(), 2);

        let high = lods
            .get(usize::from(MeshLod::High.index()))
            .and_then(Option::as_ref)
            .ok_or("no high lod")?;
        let decoded = high
            .submeshes
            .first()
            .ok_or("no face")?
            .weights
            .clone()
            .ok_or("no weights")?;
        assert_eq!(decoded.len(), weights.len());
        for (got, want) in decoded.iter().zip(&weights) {
            assert_eq!(got.influences.len(), want.influences.len());
            for (&(joint, weight), &(expected_joint, expected_weight)) in
                got.influences.iter().zip(&want.influences)
            {
                assert_eq!(joint, expected_joint);
                // The decoder clamps a weight into [0.001, 0.999], so the
                // round trip is exact only inside that band.
                assert!(
                    (weight - expected_weight).abs() <= tolerance(1.0),
                    "weight {weight} != {expected_weight}"
                );
            }
        }
        Ok(())
    }

    /// An influence naming a joint index past 254 cannot be written: `0xFF` is
    /// the terminator, so such a stream would decode as a shorter list.
    #[test]
    fn a_reserved_joint_index_is_refused() {
        let mut face = triangle();
        face.weights = Some(vec![
            VertexWeights {
                influences: vec![(255, 1.0)],
            },
            VertexWeights::default(),
            VertexWeights::default(),
        ]);
        assert!(matches!(
            encode_mesh(&MeshModel::new(vec![face])),
            Err(MeshEncodeError::JointIndexOutOfRange { joint: 255 })
        ));
    }

    /// More influences than the format transports, and more joints than the
    /// skinning shader binds, are both refused.
    #[test]
    fn the_rig_limits_are_enforced() {
        let mut face = triangle();
        face.weights = Some(vec![
            VertexWeights {
                influences: vec![(0, 0.2), (1, 0.2), (2, 0.2), (3, 0.2), (4, 0.2)],
            },
            VertexWeights::default(),
            VertexWeights::default(),
        ]);
        assert!(matches!(
            encode_mesh(&MeshModel::new(vec![face])),
            Err(MeshEncodeError::TooManyInfluences { influences }) if influences > MAX_INFLUENCES
        ));

        let skin = MeshSkin {
            joint_names: (0..=MAX_JOINTS).map(|index| format!("j{index}")).collect(),
            ..MeshSkin::default()
        };
        assert!(matches!(
            encode_mesh(&MeshModel::new(vec![triangle()]).with_skin(skin)),
            Err(MeshEncodeError::TooManyJoints { joints }) if joints > MAX_JOINTS
        ));
        assert_eq!(MAX_JOINT_INDEX, 254);
    }

    /// A prim has eight texture faces, so a ninth submesh has nowhere to go.
    #[test]
    fn a_ninth_face_is_refused() -> Result<(), TestError> {
        let faces = vec![triangle(); MAX_MODEL_FACES.saturating_add(1)];
        let error = encode_mesh(&MeshModel::new(faces))
            .err()
            .ok_or("expected a refusal")?;
        assert!(matches!(
            error,
            MeshEncodeError::TooManyFaces { faces } if faces == 9
        ));
        // The limit constants are interpolated into the messages, so a reader
        // is told the bound and not only that it was passed.
        assert_eq!(
            error.to_string(),
            "a model may carry at most 8 faces, not 9"
        );
        Ok(())
    }

    /// A triangle list must be whole triangles naming vertices the face has.
    #[test]
    fn a_malformed_triangle_list_is_refused() {
        let mut ragged = triangle();
        ragged.indices = vec![0, 1];
        assert!(matches!(
            encode_mesh(&MeshModel::new(vec![ragged])),
            Err(MeshEncodeError::RaggedTriangleList { indices: 2 })
        ));

        let mut dangling = triangle();
        dangling.indices = vec![0, 1, 7];
        assert!(matches!(
            encode_mesh(&MeshModel::new(vec![dangling])),
            Err(MeshEncodeError::IndexOutOfRange {
                index: 7,
                vertices: 3,
            })
        ));
    }

    /// A per-vertex stream that is not per-vertex would silently shorten on
    /// decode, so it is refused instead.
    #[test]
    fn a_short_per_vertex_stream_is_refused() {
        let mut face = triangle();
        face.normals.pop();
        assert!(matches!(
            encode_mesh(&MeshModel::new(vec![face])),
            Err(MeshEncodeError::StreamLengthMismatch {
                stream: "Normal",
                length: 2,
                vertices: 3,
            })
        ));
    }

    /// `high_lod` is required, the chain below it must be contiguous, and each
    /// coarser level must decimate the one above rather than refine it.
    #[test]
    fn the_lod_chain_is_validated() -> Result<(), TestError> {
        assert!(matches!(
            encode_mesh(&MeshModel::default()),
            Err(MeshEncodeError::MissingHighLod)
        ));

        // `low_lod` present while `medium_lod` is not.
        let gapped = MeshModel::new(vec![triangle()]).with_lod(MeshLod::Low, vec![triangle()]);
        assert!(matches!(
            encode_mesh(&gapped),
            Err(MeshEncodeError::LodGap {
                present: "low_lod",
                missing: "medium_lod",
            })
        ));

        // A medium level with *more* vertices than the high level above it.
        let mut fatter = triangle();
        fatter.positions.push([1.0, 1.0, 1.0]);
        fatter.normals.push([0.0, 0.0, 1.0]);
        fatter.uvs.push([1.0, 1.0]);
        let refined = MeshModel::new(vec![triangle()]).with_lod(MeshLod::Medium, vec![fatter]);
        assert!(matches!(
            encode_mesh(&refined),
            Err(MeshEncodeError::LodNotCoarser {
                coarser: "medium_lod",
                coarser_vertices: 4,
                finer: "high_lod",
                finer_vertices: 3,
            })
        ));

        // An empty level is not a level.
        assert!(matches!(
            encode_mesh(&MeshModel::new(Vec::new())),
            Err(MeshEncodeError::EmptyLod { lod: "high_lod" })
        ));

        // Four identical levels are legal: equal counts are a decimation of
        // zero, and that is what a fixture mesh wants.
        let flat = MeshModel::default().with_every_lod(vec![triangle()]);
        let asset = encode_mesh(&flat)?;
        let Decoded { lods, .. } = round_trip(&asset)?;
        assert_eq!(lods.iter().flatten().count(), 4);
        Ok(())
    }

    /// The convex decomposition round trips, and its hull limits are enforced.
    #[test]
    fn a_convex_decomposition_round_trips() -> Result<(), TestError> {
        let hull = vec![
            [-0.5, -0.5, -0.5],
            [0.5, -0.5, -0.5],
            [0.0, 0.5, -0.5],
            [0.0, 0.0, 0.5],
        ];
        let convex = PhysicsConvex {
            hulls: vec![hull.clone()],
            bounding_verts: hull.clone(),
            ..PhysicsConvex::default()
        };
        let model = MeshModel::new(vec![triangle()]).with_physics_convex(convex);
        let asset = encode_mesh(&model)?;
        let Decoded { physics, .. } = round_trip(&asset)?;
        let decoded = physics.convex.ok_or("no convex block")?;
        assert_eq!(decoded.hulls.len(), 1);
        let decoded_hull = decoded.hulls.first().ok_or("no hull")?;
        assert_eq!(decoded_hull.len(), hull.len());
        for (got, want) in decoded_hull.iter().zip(&hull) {
            for (component, expected) in got.iter().zip(want) {
                assert!(
                    (component - expected).abs() <= tolerance(1.0),
                    "hull point {got:?} != {want:?}"
                );
            }
        }
        assert_eq!(decoded.bounding_verts.len(), hull.len());

        // Both hull limits, and the empty decomposition.
        let too_many = PhysicsConvex {
            hulls: vec![hull.clone(); MAX_HULLS.saturating_add(1)],
            ..PhysicsConvex::default()
        };
        assert!(matches!(
            encode_mesh(&MeshModel::new(vec![triangle()]).with_physics_convex(too_many)),
            Err(MeshEncodeError::TooManyHulls { hulls }) if hulls > MAX_HULLS
        ));
        let too_big = PhysicsConvex {
            hulls: vec![vec![[0.0, 0.0, 0.0]; MAX_HULL_POINTS.saturating_add(1)]],
            ..PhysicsConvex::default()
        };
        assert!(matches!(
            encode_mesh(&MeshModel::new(vec![triangle()]).with_physics_convex(too_big)),
            Err(MeshEncodeError::HullTooLarge { points }) if points > MAX_HULL_POINTS
        ));
        assert!(matches!(
            encode_mesh(
                &MeshModel::new(vec![triangle()]).with_physics_convex(PhysicsConvex::default())
            ),
            Err(MeshEncodeError::EmptyPhysicsConvex)
        ));
        Ok(())
    }

    /// The header's offsets are contiguous from zero in the order the blocks
    /// are appended — `skin`, `physics_convex`, the levels coarsest to finest,
    /// then `physics_mesh` — and each block's range lands on real bytes.
    #[test]
    fn header_offsets_follow_the_write_order() -> Result<(), TestError> {
        let convex = PhysicsConvex {
            hulls: vec![vec![[-0.5, -0.5, -0.5], [0.5, -0.5, -0.5], [0.0, 0.5, 0.0]]],
            ..PhysicsConvex::default()
        };
        let model = MeshModel::default()
            .with_every_lod(vec![triangle()])
            .with_skin(MeshSkin {
                joint_names: vec!["mPelvis".to_owned()],
                ..MeshSkin::default()
            })
            .with_physics_convex(convex)
            .with_physics_mesh(vec![triangle()]);
        let asset = encode_mesh(&model)?;
        let (header, header_size) = parse_header(&asset).ok_or("no mesh header")?;
        assert_eq!(header.version, super::DEFAULT_MESH_VERSION);

        let mut expected_offset = 0_usize;
        let mut references = vec![header.skin, header.physics_convex];
        references.extend(MeshLod::ALL.map(|lod| header.lod(lod)));
        references.push(header.physics_mesh);
        for reference in references {
            let reference = reference.ok_or("a block was not written")?;
            assert_eq!(reference.offset, expected_offset);
            let (start, end) = reference.range(header_size);
            assert!(asset.get(start..end).is_some(), "block range out of asset");
            expected_offset = expected_offset.saturating_add(reference.size);
        }
        assert_eq!(
            header_size.saturating_add(expected_offset),
            asset.len(),
            "the asset is exactly its header plus its blocks"
        );
        // Everything decodes back.
        let Decoded {
            lods,
            skin,
            physics,
        } = round_trip(&asset)?;
        assert_eq!(lods.iter().flatten().count(), 4);
        assert!(skin.is_some());
        assert!(physics.convex.is_some());
        assert!(physics.mesh.is_some());
        Ok(())
    }
}
