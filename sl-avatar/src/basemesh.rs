//! Decoding of the legacy Linden avatar base-body mesh format, the `.llm`
//! (`Linden Binary Mesh 1.0`) files that ship in a viewer's `character/`
//! directory (P12.3).
//!
//! These are the *system avatar* body parts — head, upper body, lower body,
//! eyes, hair, skirt, eyelashes — one full-resolution mesh per part plus a
//! chain of lower-detail LOD reductions. They are wholly distinct from the
//! `LLMesh` format `sl-mesh` decodes (which is per-object rigged/static mesh
//! streamed from the grid): the base body is client-side content, keyed to the
//! skeleton by a per-vertex blend weight and shaped at runtime by morph
//! targets driven from the visual params.
//!
//! Like the rest of the crate this module is I/O-free — it decodes a borrowed
//! `&[u8]` into an owned model in Second Life's right-handed **Z-up** metre
//! space (the Bevy axis/skin conversion lives in `sl-client-bevy` at P13).
//!
//! Two entry points mirror the two roles a `.llm` file plays (per the `lod`
//! attribute in `avatar_lad.xml`):
//!
//! - [`BaseMesh::from_bytes`] decodes a full base mesh (`lod="0"`): vertices
//!   (positions, normals, binormals, UVs), the per-vertex skin
//!   [`weights`](BaseMesh::weights), triangle faces, the skin-joint name table,
//!   and the [`MorphTarget`] deltas plus shared-vertex remaps.
//! - [`LodMesh::from_bytes`] decodes a reduced LOD (`lod="1"`..`"5"`): the same
//!   binary shape, but only the header transform and the reduced triangle face
//!   list are meaningful (the reduced faces index into the base part's vertex
//!   array), so that is all it exposes.
//!
//! The binary layout follows Firestorm `LLPolyMeshSharedData::loadMesh` /
//! `LLPolyMorphData::loadBinary` (read-only reference; reimplemented here).

/// The magic string every `.llm` file begins with; the file header occupies the
/// first [`HEADER_LEN`] bytes and the payload starts immediately after it.
const HEADER_MAGIC: &[u8] = b"Linden Binary Mesh 1.0";

/// The length of the fixed file header the payload starts after. The reference
/// loader seeks to absolute offset 24 before reading `HasWeights`, so the magic
/// (22 bytes) is followed by two bytes of padding.
const HEADER_LEN: usize = 24;

/// The fixed on-disk length of a skin-joint or morph name field, in bytes. Each
/// is a NUL-padded ASCII string of exactly this many bytes.
const NAME_LEN: usize = 64;

/// The sentinel morph name that terminates the morph section.
const END_MORPHS: &str = "End Morphs";

/// A guard on the vertex count so a corrupt or hostile header cannot request a
/// multi-gigabyte allocation; the real base parts top out near 6k vertices.
const MAX_VERTICES: usize = 1 << 20;

/// An error returned while decoding a `.llm` base-mesh or LOD file.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum BaseMeshError {
    /// The file did not begin with the `Linden Binary Mesh 1.0` magic.
    #[error("not a Linden binary mesh (bad or missing magic header)")]
    BadMagic,
    /// The stream ended before a required field could be read in full.
    #[error("unexpected end of mesh data while reading {field}")]
    UnexpectedEof {
        /// The field the decoder was reading when the data ran out.
        field: &'static str,
    },
    /// The header declared more vertices than the decoder's sane maximum
    /// allows.
    #[error("vertex count {count} exceeds the sane maximum {max}")]
    TooManyVertices {
        /// The declared vertex count.
        count: usize,
        /// The maximum the decoder accepts.
        max: usize,
    },
}

/// A per-vertex skin weight in the legacy base body: the vertex is rigidly
/// bound between two adjacent joints in the mesh's **joint-render-data** list and
/// linearly blended between them.
///
/// The on-disk form is a single `f32` whose integer part selects the first joint
/// and whose fractional part is the blend toward the *next* joint (the Firestorm
/// avatar skinning shader `avatarSkinV.glsl`: `i = floor(weight); mix(palette[i],
/// palette[i+1], fract(weight))`). Crucially the integer part indexes the
/// reference viewer's `mJointRenderData` — a depth-first walk of the skeleton with
/// each disjoint group's base ancestor prepended — **not** the mesh's own
/// [`joint_names`](BaseMesh::joint_names) table (the two orders differ; e.g. the
/// head names `[mHead, mNeck]` but its render list is `[…, mNeck, mHead]`, so a
/// face vertex's weight `2.0` means `mHead`, not the second name). The runtime
/// rebuilds that render list (`sl-client-bevy`'s `base_mesh_skin`) and this index
/// lands into it directly.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VertexSkinWeight {
    /// The index of the first bound joint within the mesh's joint-render-data
    /// list. The second joint is `joint + 1` (clamped to the last render-list
    /// entry when `blend` is `0`).
    pub joint: usize,
    /// The blend factor toward `joint + 1`, in `0.0..=1.0`.
    pub blend: f32,
}

/// One morph target: a sparse set of per-vertex deltas that, scaled by a visual
/// param's driven weight, deform the base mesh into a shaped body.
#[derive(Clone, Debug, PartialEq)]
pub struct MorphTarget {
    /// The morph's name, matched against the visual-param `param_morph` refs
    /// (P12.4) to know which param drives it.
    pub name: String,
    /// The sparse per-vertex deltas this morph applies.
    pub deltas: Vec<MorphDelta>,
}

/// A single vertex's contribution to a [`MorphTarget`]: the delta added to that
/// base vertex's position / normal / binormal / UV at full morph weight.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MorphDelta {
    /// The index of the affected base vertex within [`BaseMesh::positions`].
    pub vertex_index: usize,
    /// The position delta, in metres (Z-up).
    pub position: [f32; 3],
    /// The normal delta.
    pub normal: [f32; 3],
    /// The binormal delta.
    pub binormal: [f32; 3],
    /// The texture-coordinate delta.
    pub tex_coord: [f32; 2],
}

/// A remap of one base vertex onto another: seam vertices that share a position
/// but differ in UV/normal are welded for morphing by this table (the reference
/// loader's `mSharedVerts`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SharedVertex {
    /// The source (duplicate) vertex index.
    pub source: usize,
    /// The destination (canonical) vertex index its data is shared from.
    pub destination: usize,
}

/// The header transform a `.llm` file carries: a rigid placement of the part in
/// avatar space. The reference viewer applies it to the whole part; the fields
/// are kept raw (rotation as Euler XYZ **degrees**) for the P13 conversion.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeshTransform {
    /// The part's translation in avatar space, in metres (Z-up).
    pub position: [f32; 3],
    /// The part's rotation as Euler XYZ angles, in degrees.
    pub rotation: [f32; 3],
    /// The part's scale (unitless multipliers).
    pub scale: [f32; 3],
}

/// A decoded full-resolution avatar base-body part (`lod="0"`).
///
/// Positions/normals/binormals/UVs are parallel arrays of length
/// [`vertex_count`](Self::vertex_count); [`faces`](Self::faces) are triangle
/// index triples into them.
#[derive(Clone, Debug)]
pub struct BaseMesh {
    /// The part's header placement transform.
    transform: MeshTransform,
    /// Per-vertex rest positions, in metres (Z-up).
    positions: Vec<[f32; 3]>,
    /// Per-vertex rest normals.
    normals: Vec<[f32; 3]>,
    /// Per-vertex rest binormals.
    binormals: Vec<[f32; 3]>,
    /// Per-vertex primary texture coordinates.
    tex_coords: Vec<[f32; 2]>,
    /// Per-vertex secondary ("detail") texture coordinates; empty when the file
    /// had no detail UVs.
    detail_tex_coords: Vec<[f32; 2]>,
    /// Per-vertex skin weights; empty when the part is unweighted.
    weights: Vec<VertexSkinWeight>,
    /// Triangle faces (index triples into the per-vertex arrays).
    faces: Vec<[u16; 3]>,
    /// The skin-joint name table the [`weights`](Self::weights) index into.
    joint_names: Vec<String>,
    /// The morph targets that shape this part.
    morphs: Vec<MorphTarget>,
    /// The shared-vertex remap table.
    shared_verts: Vec<SharedVertex>,
}

impl BaseMesh {
    /// Decode a full base-body `.llm` mesh (the `lod="0"` file) from its bytes.
    ///
    /// # Errors
    ///
    /// Returns [`BaseMeshError`] if the magic is wrong, the stream is truncated,
    /// or the header declares an implausible vertex count.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, BaseMeshError> {
        let mut cursor = Cursor::new(bytes)?;
        let (transform, has_weights, has_detail_tex_coords) = cursor.read_header()?;

        let num_vertices = usize::from(cursor.read_u16("num_vertices")?);
        if num_vertices > MAX_VERTICES {
            return Err(BaseMeshError::TooManyVertices {
                count: num_vertices,
                max: MAX_VERTICES,
            });
        }

        let positions = cursor.read_vec3_array(num_vertices, "positions")?;
        let normals = cursor.read_vec3_array(num_vertices, "normals")?;
        let binormals = cursor.read_vec3_array(num_vertices, "binormals")?;
        let tex_coords = cursor.read_vec2_array(num_vertices, "tex_coords")?;
        let detail_tex_coords = if has_detail_tex_coords {
            cursor.read_vec2_array(num_vertices, "detail_tex_coords")?
        } else {
            Vec::new()
        };
        let raw_weights = if has_weights {
            cursor.read_f32_array(num_vertices, "weights")?
        } else {
            Vec::new()
        };

        let faces = cursor.read_faces()?;

        let joint_names = if has_weights {
            let num_joints = usize::from(cursor.read_u16("num_skin_joints")?);
            cursor.read_names(num_joints, "skin_joint_name")?
        } else {
            Vec::new()
        };

        let weights = decode_weights(&raw_weights);

        let morphs = cursor.read_morphs()?;
        let shared_verts = cursor.read_shared_verts()?;

        Ok(Self {
            transform,
            positions,
            normals,
            binormals,
            tex_coords,
            detail_tex_coords,
            weights,
            faces,
            joint_names,
            morphs,
            shared_verts,
        })
    }

    /// The part's header placement transform.
    #[must_use]
    pub const fn transform(&self) -> &MeshTransform {
        &self.transform
    }

    /// The number of vertices (the length of the parallel per-vertex arrays).
    #[must_use]
    pub const fn vertex_count(&self) -> usize {
        self.positions.len()
    }

    /// Per-vertex rest positions, in metres (Z-up).
    #[must_use]
    pub fn positions(&self) -> &[[f32; 3]] {
        &self.positions
    }

    /// Per-vertex rest normals.
    #[must_use]
    pub fn normals(&self) -> &[[f32; 3]] {
        &self.normals
    }

    /// Per-vertex rest binormals.
    #[must_use]
    pub fn binormals(&self) -> &[[f32; 3]] {
        &self.binormals
    }

    /// Per-vertex primary texture coordinates.
    #[must_use]
    pub fn tex_coords(&self) -> &[[f32; 2]] {
        &self.tex_coords
    }

    /// Per-vertex detail texture coordinates (empty if the file had none).
    #[must_use]
    pub fn detail_tex_coords(&self) -> &[[f32; 2]] {
        &self.detail_tex_coords
    }

    /// Whether the part carries per-vertex skin weights.
    #[must_use]
    pub const fn has_weights(&self) -> bool {
        !self.weights.is_empty()
    }

    /// Per-vertex skin weights (empty for an unweighted part).
    #[must_use]
    pub fn weights(&self) -> &[VertexSkinWeight] {
        &self.weights
    }

    /// Triangle faces (index triples into the per-vertex arrays).
    #[must_use]
    pub fn faces(&self) -> &[[u16; 3]] {
        &self.faces
    }

    /// The skin-joint name table the weights index into.
    #[must_use]
    pub fn joint_names(&self) -> &[String] {
        &self.joint_names
    }

    /// The morph targets that shape this part.
    #[must_use]
    pub fn morphs(&self) -> &[MorphTarget] {
        &self.morphs
    }

    /// The morph target with the given name, if the part defines it.
    #[must_use]
    pub fn morph(&self, name: &str) -> Option<&MorphTarget> {
        self.morphs.iter().find(|morph| morph.name == name)
    }

    /// The shared-vertex remap table.
    #[must_use]
    pub fn shared_verts(&self) -> &[SharedVertex] {
        &self.shared_verts
    }
}

/// A decoded lower-detail LOD reduction (`lod="1"`..`"5"`) of a base part.
///
/// A LOD file repeats the base part's header transform and carries a reduced
/// triangle [`faces`](Self::faces) list; the reduced faces index into the
/// *base* part's vertex array (they share its geometry), so no vertex data is
/// decoded here — [`vertex_count`](Self::vertex_count) is the highest index the
/// faces reference (one past it), matching the reference loader's LOD bookkeeping.
#[derive(Clone, Debug)]
pub struct LodMesh {
    /// The part's header placement transform.
    transform: MeshTransform,
    /// The reduced triangle faces (index triples into the base part's vertices).
    faces: Vec<[u16; 3]>,
    /// One past the largest vertex index the faces reference.
    vertex_count: usize,
}

impl LodMesh {
    /// Decode a reduced-LOD `.llm` file (`lod="1"`..`"5"`) from its bytes.
    ///
    /// # Errors
    ///
    /// Returns [`BaseMeshError`] if the magic is wrong or the stream is
    /// truncated.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, BaseMeshError> {
        let mut cursor = Cursor::new(bytes)?;
        let (transform, _has_weights, _has_detail) = cursor.read_header()?;
        let faces = cursor.read_faces()?;
        let vertex_count = faces
            .iter()
            .flat_map(|face| face.iter())
            .map(|&index| usize::from(index).saturating_add(1))
            .max()
            .unwrap_or(0);
        Ok(Self {
            transform,
            faces,
            vertex_count,
        })
    }

    /// The part's header placement transform.
    #[must_use]
    pub const fn transform(&self) -> &MeshTransform {
        &self.transform
    }

    /// The reduced triangle faces (index triples into the base part's vertices).
    #[must_use]
    pub fn faces(&self) -> &[[u16; 3]] {
        &self.faces
    }

    /// One past the largest vertex index the reduced faces reference.
    #[must_use]
    pub const fn vertex_count(&self) -> usize {
        self.vertex_count
    }
}

/// Split each raw skin-weight float into its `(joint, blend)` form: `joint =
/// floor(w)` (clamped to the joint table) and `blend = w - joint`.
fn decode_weights(raw: &[f32]) -> Vec<VertexSkinWeight> {
    raw.iter().copied().map(split_weight).collect()
}

/// An upper bound on a decoded skin-weight's integer part, guarding
/// [`split_weight`]'s loop against a garbage weight (the reference viewer's
/// matrix palette holds 45 joints; base-body render lists are far smaller).
const MAX_SKIN_JOINT_INDEX: usize = 63;

/// Split one raw skin-weight float into `(joint, blend)` without a float→int cast
/// (the workspace lints forbid `as`): step the joint index up while the next
/// integer boundary still fits under the (non-negative) weight.
///
/// The integer part is kept **raw** — it indexes the mesh's *joint-render-data*
/// list (the reference viewer's `mJointRenderData`: a depth-first walk of the
/// skeleton with each disjoint group's base ancestor prepended), **not** the
/// mesh's own `joint_names` table, and the two orders differ. The runtime
/// (`sl-client-bevy`'s `base_mesh_skin`) rebuilds that render list so this index
/// lands on the right joint; clamping it to `joint_names.len()` here would bind,
/// for example, every head-mesh face vertex (weight `2.0`) to `mNeck` instead of
/// `mHead`, which only shows up as a skinning distortion once the skeleton is
/// deformed.
fn split_weight(weight: f32) -> VertexSkinWeight {
    let clamped = weight.max(0.0);
    let mut joint = 0_usize;
    loop {
        let next = joint.saturating_add(1);
        let next_boundary = u16::try_from(next).map_or(f32::MAX, f32::from);
        if next <= MAX_SKIN_JOINT_INDEX && next_boundary <= clamped {
            joint = next;
        } else {
            break;
        }
    }
    let joint_start = u16::try_from(joint).map_or(0.0, f32::from);
    let blend = (clamped - joint_start).clamp(0.0, 1.0);
    VertexSkinWeight { joint, blend }
}

/// A forward-only cursor over the `.llm` payload (the bytes after the fixed file
/// header), reading little-endian primitives without indexing or the
/// lint-forbidden `from_le_bytes` family.
struct Cursor<'a> {
    /// The full mesh byte slice.
    data: &'a [u8],
    /// The current read offset into [`data`](Self::data).
    pos: usize,
}

impl<'a> Cursor<'a> {
    /// Create a cursor positioned just past the file header, validating the
    /// magic first.
    fn new(data: &'a [u8]) -> Result<Self, BaseMeshError> {
        let magic = data.get(..HEADER_MAGIC.len());
        if magic != Some(HEADER_MAGIC) {
            return Err(BaseMeshError::BadMagic);
        }
        Ok(Self {
            data,
            pos: HEADER_LEN,
        })
    }

    /// Take the next `len` bytes, advancing the cursor, or an EOF error tagged
    /// with `field`.
    fn take(&mut self, len: usize, field: &'static str) -> Result<&'a [u8], BaseMeshError> {
        let end = self
            .pos
            .checked_add(len)
            .ok_or(BaseMeshError::UnexpectedEof { field })?;
        let slice = self
            .data
            .get(self.pos..end)
            .ok_or(BaseMeshError::UnexpectedEof { field })?;
        self.pos = end;
        Ok(slice)
    }

    /// Read a single byte.
    fn read_u8(&mut self, field: &'static str) -> Result<u8, BaseMeshError> {
        self.take(1, field)?
            .first()
            .copied()
            .ok_or(BaseMeshError::UnexpectedEof { field })
    }

    /// Read a little-endian `u16`.
    fn read_u16(&mut self, field: &'static str) -> Result<u16, BaseMeshError> {
        let bytes = self.take(2, field)?;
        Ok(bytes.iter().enumerate().fold(0_u16, |acc, (index, &byte)| {
            let shift = u32::try_from(index).unwrap_or(0).saturating_mul(8);
            acc | (u16::from(byte).checked_shl(shift).unwrap_or(0))
        }))
    }

    /// Read a little-endian `u32`.
    fn read_u32(&mut self, field: &'static str) -> Result<u32, BaseMeshError> {
        let bytes = self.take(4, field)?;
        Ok(bytes.iter().enumerate().fold(0_u32, |acc, (index, &byte)| {
            let shift = u32::try_from(index).unwrap_or(0).saturating_mul(8);
            acc | (u32::from(byte).checked_shl(shift).unwrap_or(0))
        }))
    }

    /// Read a little-endian IEEE-754 `f32` (via its bit pattern, avoiding the
    /// lint-forbidden `from_le_bytes`).
    fn read_f32(&mut self, field: &'static str) -> Result<f32, BaseMeshError> {
        Ok(f32::from_bits(self.read_u32(field)?))
    }

    /// Read a 3-vector of little-endian floats.
    fn read_vec3(&mut self, field: &'static str) -> Result<[f32; 3], BaseMeshError> {
        Ok([
            self.read_f32(field)?,
            self.read_f32(field)?,
            self.read_f32(field)?,
        ])
    }

    /// Read a 2-vector of little-endian floats.
    fn read_vec2(&mut self, field: &'static str) -> Result<[f32; 2], BaseMeshError> {
        Ok([self.read_f32(field)?, self.read_f32(field)?])
    }

    /// Read `count` 3-vectors into a fresh `Vec`.
    fn read_vec3_array(
        &mut self,
        count: usize,
        field: &'static str,
    ) -> Result<Vec<[f32; 3]>, BaseMeshError> {
        let mut out = Vec::with_capacity(count);
        for _ in 0..count {
            out.push(self.read_vec3(field)?);
        }
        Ok(out)
    }

    /// Read `count` 2-vectors into a fresh `Vec`.
    fn read_vec2_array(
        &mut self,
        count: usize,
        field: &'static str,
    ) -> Result<Vec<[f32; 2]>, BaseMeshError> {
        let mut out = Vec::with_capacity(count);
        for _ in 0..count {
            out.push(self.read_vec2(field)?);
        }
        Ok(out)
    }

    /// Read `count` little-endian floats into a fresh `Vec`.
    fn read_f32_array(
        &mut self,
        count: usize,
        field: &'static str,
    ) -> Result<Vec<f32>, BaseMeshError> {
        let mut out = Vec::with_capacity(count);
        for _ in 0..count {
            out.push(self.read_f32(field)?);
        }
        Ok(out)
    }

    /// Read the fixed transform/flags header block that both a full mesh and a
    /// LOD carry, returning `(transform, has_weights, has_detail_tex_coords)`.
    fn read_header(&mut self) -> Result<(MeshTransform, bool, bool), BaseMeshError> {
        let has_weights = self.read_u8("has_weights")? > 0;
        let has_detail_tex_coords = self.read_u8("has_detail_tex_coords")? > 0;
        let position = self.read_vec3("position")?;
        let rotation = self.read_vec3("rotation_angles")?;
        // Rotation order byte: the reference loader reads then forces it to 0,
        // so the angles are always interpreted as Euler XYZ. We read past it.
        let _rotation_order = self.read_u8("rotation_order")?;
        let scale = self.read_vec3("scale")?;
        Ok((
            MeshTransform {
                position,
                rotation,
                scale,
            },
            has_weights,
            has_detail_tex_coords,
        ))
    }

    /// Read the `NumFaces` count and that many triangle index triples.
    fn read_faces(&mut self) -> Result<Vec<[u16; 3]>, BaseMeshError> {
        let num_faces = usize::from(self.read_u16("num_faces")?);
        let mut faces = Vec::with_capacity(num_faces);
        for _ in 0..num_faces {
            faces.push([
                self.read_u16("face_index")?,
                self.read_u16("face_index")?,
                self.read_u16("face_index")?,
            ]);
        }
        Ok(faces)
    }

    /// Read `count` fixed-width [`NAME_LEN`]-byte NUL-padded names.
    fn read_names(
        &mut self,
        count: usize,
        field: &'static str,
    ) -> Result<Vec<String>, BaseMeshError> {
        let mut names = Vec::with_capacity(count);
        for _ in 0..count {
            names.push(self.read_name(field)?);
        }
        Ok(names)
    }

    /// Read one fixed-width [`NAME_LEN`]-byte NUL-padded name, trimming at the
    /// first NUL.
    fn read_name(&mut self, field: &'static str) -> Result<String, BaseMeshError> {
        let bytes = self.take(NAME_LEN, field)?;
        let end = bytes.iter().position(|&byte| byte == 0).unwrap_or(NAME_LEN);
        let text = bytes.get(..end).unwrap_or_default();
        Ok(String::from_utf8_lossy(text).into_owned())
    }

    /// Read the morph section: successive named [`MorphTarget`]s until the
    /// [`END_MORPHS`] sentinel (or the data runs out).
    fn read_morphs(&mut self) -> Result<Vec<MorphTarget>, BaseMeshError> {
        let mut morphs = Vec::new();
        // A missing morph-name field means the file ended without the sentinel;
        // tolerate it rather than erroring (matches the reference loader's
        // `fread(..) == 64` loop condition), so the loop ends on a read error.
        while let Ok(name) = self.read_name("morph_name") {
            if name == END_MORPHS {
                break;
            }
            let deltas = self.read_morph_deltas()?;
            morphs.push(MorphTarget { name, deltas });
        }
        Ok(morphs)
    }

    /// Read one morph target's sparse per-vertex delta list.
    fn read_morph_deltas(&mut self) -> Result<Vec<MorphDelta>, BaseMeshError> {
        let count = usize::try_from(self.read_u32("morph_vertex_count")?).unwrap_or(0);
        let mut deltas = Vec::with_capacity(count);
        for _ in 0..count {
            let vertex_index = usize::try_from(self.read_u32("morph_vertex_index")?).unwrap_or(0);
            deltas.push(MorphDelta {
                vertex_index,
                position: self.read_vec3("morph_position")?,
                normal: self.read_vec3("morph_normal")?,
                binormal: self.read_vec3("morph_binormal")?,
                tex_coord: self.read_vec2("morph_tex_coord")?,
            });
        }
        Ok(deltas)
    }

    /// Read the trailing shared-vertex remap table (absent → empty).
    fn read_shared_verts(&mut self) -> Result<Vec<SharedVertex>, BaseMeshError> {
        // The remap-count field is optional: an older file may simply end here.
        let Ok(count) = self.read_u32("num_remaps") else {
            return Ok(Vec::new());
        };
        let count = usize::try_from(count).unwrap_or(0);
        let mut remaps = Vec::with_capacity(count);
        for _ in 0..count {
            let source = usize::try_from(self.read_u32("remap_source")?).unwrap_or(0);
            let destination = usize::try_from(self.read_u32("remap_destination")?).unwrap_or(0);
            remaps.push(SharedVertex {
                source,
                destination,
            });
        }
        Ok(remaps)
    }
}

#[cfg(test)]
mod tests {
    use super::{BaseMesh, BaseMeshError, LodMesh};
    use pretty_assertions::assert_eq;

    /// A boxed error so tests can use `?` instead of disallowed `unwrap`/`expect`.
    type TestError = Box<dyn core::error::Error>;

    /// A minimal committed full base-mesh fixture: 4 vertices, 2 faces, weighted
    /// (2 skin joints), 1 morph (2 deltas), 1 shared-vertex remap, no detail UVs.
    const MINI_BASEMESH: &[u8] = include_bytes!("../tests/fixtures/mini_basemesh.llm");
    /// A minimal committed LOD fixture: header + 1 reduced face, no vertex data.
    const MINI_LOD: &[u8] = include_bytes!("../tests/fixtures/mini_basemesh_lod.llm");

    /// Compare two float vectors within a tolerance (keeps the assertion off
    /// `float_cmp`).
    fn close<const N: usize>(a: [f32; N], b: [f32; N]) -> bool {
        a.iter().zip(b.iter()).all(|(x, y)| (x - y).abs() < 1.0e-4)
    }

    #[test]
    fn decodes_full_base_mesh() -> Result<(), TestError> {
        let mesh = BaseMesh::from_bytes(MINI_BASEMESH)?;
        assert_eq!(mesh.vertex_count(), 4);
        assert_eq!(mesh.positions().len(), 4);
        assert_eq!(mesh.normals().len(), 4);
        assert_eq!(mesh.binormals().len(), 4);
        assert_eq!(mesh.tex_coords().len(), 4);
        assert!(mesh.detail_tex_coords().is_empty());
        assert_eq!(mesh.faces().len(), 2);

        // Header transform round-trips.
        assert!(close(mesh.transform().position, [0.0, 0.0, 0.0]));
        assert!(close(mesh.transform().scale, [1.0, 1.0, 1.0]));

        // First vertex is the fixture's canonical origin corner.
        let first = mesh.positions().first().ok_or("first vertex")?;
        assert!(close(*first, [0.0, 0.0, 0.0]));
        let last = mesh.positions().get(3).ok_or("fourth vertex")?;
        assert!(close(*last, [1.0, 1.0, 0.0]));

        // Faces reference the four vertices.
        assert_eq!(mesh.faces().first().copied(), Some([0, 1, 2]));
        assert_eq!(mesh.faces().get(1).copied(), Some([2, 1, 3]));
        Ok(())
    }

    #[test]
    fn decodes_skin_weights_and_joints() -> Result<(), TestError> {
        let mesh = BaseMesh::from_bytes(MINI_BASEMESH)?;
        assert!(mesh.has_weights());
        assert_eq!(mesh.joint_names(), &["mPelvis", "mTorso"]);
        assert_eq!(mesh.weights().len(), 4);

        // Weight 0.0 -> joint 0, no blend; weight 0.25 -> joint 0, blend 0.25;
        // weight 1.0 -> joint 1 (the raw integer part), blend 0.
        let w0 = mesh.weights().first().ok_or("weight 0")?;
        assert_eq!(w0.joint, 0);
        assert!((w0.blend - 0.0).abs() < 1.0e-4);
        let w1 = mesh.weights().get(1).ok_or("weight 1")?;
        assert_eq!(w1.joint, 0);
        assert!((w1.blend - 0.25).abs() < 1.0e-4);
        let w3 = mesh.weights().get(3).ok_or("weight 3")?;
        assert_eq!(w3.joint, 1);
        assert!((w3.blend - 0.0).abs() < 1.0e-4);
        Ok(())
    }

    #[test]
    fn decodes_morphs_and_remaps() -> Result<(), TestError> {
        let mesh = BaseMesh::from_bytes(MINI_BASEMESH)?;
        assert_eq!(mesh.morphs().len(), 1);
        let morph = mesh.morph("Fatten").ok_or("Fatten morph")?;
        assert_eq!(morph.deltas.len(), 2);
        let d0 = morph.deltas.first().ok_or("delta 0")?;
        assert_eq!(d0.vertex_index, 0);
        assert!(close(d0.position, [0.1, 0.0, 0.0]));
        let d1 = morph.deltas.get(1).ok_or("delta 1")?;
        assert_eq!(d1.vertex_index, 3);

        assert_eq!(mesh.shared_verts().len(), 1);
        let remap = mesh.shared_verts().first().ok_or("remap 0")?;
        assert_eq!(remap.source, 2);
        assert_eq!(remap.destination, 0);
        Ok(())
    }

    #[test]
    fn decodes_lod_mesh_faces_only() -> Result<(), TestError> {
        let lod = LodMesh::from_bytes(MINI_LOD)?;
        assert_eq!(lod.faces().len(), 1);
        assert_eq!(lod.faces().first().copied(), Some([0, 1, 2]));
        // vertex_count is one past the largest referenced index.
        assert_eq!(lod.vertex_count(), 3);
        Ok(())
    }

    #[test]
    fn rejects_bad_magic() {
        let result = BaseMesh::from_bytes(b"not a linden mesh at all really");
        assert_eq!(result.err(), Some(BaseMeshError::BadMagic));
    }

    #[test]
    fn rejects_truncated_stream() {
        // Valid magic + header start, but truncated before the vertex data.
        let mut bytes = b"Linden Binary Mesh 1.0\0\0".to_vec();
        bytes.push(1); // has_weights
        bytes.push(0); // has_detail_tex_coords
        let result = BaseMesh::from_bytes(&bytes);
        assert!(matches!(result, Err(BaseMeshError::UnexpectedEof { .. })));
    }
}
