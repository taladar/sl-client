//! A procedural Second Life **mesh asset**: the binary-LLSD header naming one
//! zlib-compressed geometry block per level of detail, which is what the
//! `GetMesh2` capability serves and what [`sl_mesh`](https://docs.rs/sl-mesh)
//! decodes.
//!
//! There is no mesh *encoder* in the workspace — nothing but a test needs to
//! produce an asset — so this module is the one place that writes the format.
//! It is deliberately the smallest asset the decoder accepts: a single
//! submesh, one block shared by all four LOD entries, no skin and no physics.

use std::io::Write as _;

use sl_llsd::Llsd;
use sl_proto::MeshLod;

use crate::{push_u16, quantize_u16};

/// The half-extent of the unit cube: its vertices span `[-0.5, 0.5]` on every
/// axis, the normalized box a prim's scale stretches.
const HALF: f32 = 0.5;

/// The mesh-header version the reference viewer writes and `sl-mesh` accepts.
const MESH_VERSION: i32 = 1;

/// The six faces of a cube as `(normal, u axis, v axis)`, each axis pair
/// ordered so `u × v` is the outward normal — which makes the two triangles
/// below wind counter-clockwise seen from outside.
const CUBE_FACES: [([f32; 3], [f32; 3], [f32; 3]); 6] = [
    ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
    ([-1.0, 0.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, 1.0]),
    ([0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
    ([0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
    ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
    ([0.0, 0.0, -1.0], [1.0, 0.0, 0.0], [0.0, -1.0, 0.0]),
];

/// The four corners of a face in `(u, v)` sign order, wound counter-clockwise.
const FACE_CORNERS: [(f32, f32); 4] = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)];

/// The two triangles of a face, as indices into [`FACE_CORNERS`].
const FACE_TRIANGLES: [u16; 6] = [0, 1, 2, 0, 2, 3];

/// One vertex of the generated cube: position, outward normal and face-local
/// texture coordinate.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Vertex {
    /// The position, in the normalized `[-0.5, 0.5]` mesh box.
    position: [f32; 3],
    /// The outward unit normal.
    normal: [f32; 3],
    /// The texture coordinate, `0..1` across the face.
    uv: [f32; 2],
}

/// A unit cube as a Second Life mesh asset: 24 vertices (four per face, so
/// each face keeps its own normal and a full `0..1` texture coordinate span),
/// 12 triangles, in **one** submesh — the prim therefore has a single texture
/// face, which is what a fixture wants when it asserts "the checker is on the
/// mesh".
///
/// All four LOD entries name the same block, so the asset renders identically
/// however aggressively a viewer picks its level of detail.
///
/// # Errors
///
/// Returns the zlib encoder's error, which writing to a `Vec` cannot produce.
pub fn unit_cube_mesh_asset() -> Result<Vec<u8>, std::io::Error> {
    let vertices = cube_vertices();
    let indices = cube_indices();
    let block = deflate(&Llsd::Array(vec![submesh(&vertices, &indices)]).to_llsd_binary())?;

    let mut header = vec![("version".to_owned(), Llsd::Integer(MESH_VERSION))];
    let reference = Llsd::Map(
        [
            ("offset".to_owned(), Llsd::Integer(0)),
            (
                "size".to_owned(),
                Llsd::Integer(i32::try_from(block.len()).unwrap_or(i32::MAX)),
            ),
        ]
        .into_iter()
        .collect(),
    );
    for lod in MeshLod::ALL {
        header.push((lod.header_key().to_owned(), reference.clone()));
    }

    let mut asset = Llsd::Map(header.into_iter().collect()).to_llsd_binary();
    asset.extend_from_slice(&block);
    Ok(asset)
}

/// The cube's 24 vertices, face by face.
fn cube_vertices() -> Vec<Vertex> {
    let mut vertices = Vec::new();
    for (normal, u_axis, v_axis) in CUBE_FACES {
        for (u_sign, v_sign) in FACE_CORNERS {
            vertices.push(Vertex {
                position: axes(normal, u_axis, v_axis, u_sign, v_sign),
                normal,
                uv: [f32::midpoint(u_sign, 1.0), f32::midpoint(v_sign, 1.0)],
            });
        }
    }
    vertices
}

/// The corner at `(u_sign, v_sign)` of the face whose outward normal is
/// `normal`: half a metre out along the normal, half a metre along each of the
/// face's own axes.
fn axes(
    normal: [f32; 3],
    u_axis: [f32; 3],
    v_axis: [f32; 3],
    u_sign: f32,
    v_sign: f32,
) -> [f32; 3] {
    let mut position = [0.0_f32; 3];
    for (component, (n, (u, v))) in position
        .iter_mut()
        .zip(normal.into_iter().zip(u_axis.into_iter().zip(v_axis)))
    {
        *component = (n + u * u_sign + v * v_sign) * HALF;
    }
    position
}

/// The cube's 36 triangle indices: each face's two triangles offset to its
/// four vertices.
fn cube_indices() -> Vec<u16> {
    let mut indices = Vec::new();
    for face in 0..CUBE_FACES.len() {
        let base = u16::try_from(face.saturating_mul(FACE_CORNERS.len())).unwrap_or(0);
        for corner in FACE_TRIANGLES {
            indices.push(base.saturating_add(corner));
        }
    }
    indices
}

/// One submesh map: the quantized position / normal / texture-coordinate
/// streams with the domains they are taken over, and the triangle list.
fn submesh(vertices: &[Vertex], indices: &[u16]) -> Llsd {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    for vertex in vertices {
        for component in vertex.position {
            push_u16(&mut positions, quantize_u16(component, -HALF, HALF));
        }
        for component in vertex.normal {
            push_u16(&mut normals, quantize_u16(component, -1.0, 1.0));
        }
        for component in vertex.uv {
            push_u16(&mut uvs, quantize_u16(component, 0.0, 1.0));
        }
    }
    let mut triangles = Vec::new();
    for index in indices {
        push_u16(&mut triangles, *index);
    }

    Llsd::Map(
        [
            (
                "PositionDomain".to_owned(),
                domain(&[-HALF, -HALF, -HALF], &[HALF, HALF, HALF]),
            ),
            ("Position".to_owned(), Llsd::Binary(positions)),
            ("Normal".to_owned(), Llsd::Binary(normals)),
            (
                "TexCoord0Domain".to_owned(),
                domain(&[0.0, 0.0], &[1.0, 1.0]),
            ),
            ("TexCoord0".to_owned(), Llsd::Binary(uvs)),
            ("TriangleList".to_owned(), Llsd::Binary(triangles)),
            ("NormalizedScale".to_owned(), reals(&[1.0, 1.0, 1.0])),
        ]
        .into_iter()
        .collect(),
    )
}

/// A `{ Min, Max }` domain map over vectors of any width.
fn domain(min: &[f32], max: &[f32]) -> Llsd {
    Llsd::Map(
        [
            ("Min".to_owned(), reals(min)),
            ("Max".to_owned(), reals(max)),
        ]
        .into_iter()
        .collect(),
    )
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

/// zlib-compresses `bytes` — the framing every mesh block is stored in.
fn deflate(bytes: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(bytes)?;
    encoder.finish()
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_proto::MeshLod;

    use super::unit_cube_mesh_asset;

    type TestError = Box<dyn core::error::Error>;

    /// The asset the fixtures serve decodes back through the viewer's own mesh
    /// decoder into the cube it was written as: 24 vertices, 12 triangles, all
    /// inside the normalized box, every level of detail present.
    #[test]
    fn the_unit_cube_decodes_back_into_a_cube() -> Result<(), TestError> {
        let asset = unit_cube_mesh_asset()?;
        let (header, header_size) = sl_mesh::parse_header(&asset).ok_or("no mesh header")?;
        assert!(!header.not_found);
        for lod in MeshLod::ALL {
            let block = header.lod(lod).ok_or("missing lod")?;
            let start = header_size.saturating_add(block.offset);
            let bytes = asset
                .get(start..start.saturating_add(block.size))
                .ok_or("lod block out of range")?;
            let decoded = sl_mesh::decode_lod(bytes, lod)?;
            assert_eq!(decoded.vertex_count(), 24);
            assert_eq!(decoded.triangle_count(), 12);
            let submesh = decoded.submeshes.first().ok_or("no submesh")?;
            for position in &submesh.positions {
                for component in position {
                    assert!(
                        component.abs() <= 0.5001,
                        "vertex {position:?} outside the normalized box"
                    );
                }
            }
            // Each of the six faces contributes four vertices sharing one
            // outward normal, so exactly six distinct normals appear.
            let mut normals: Vec<String> =
                submesh.normals.iter().map(|n| format!("{n:.2?}")).collect();
            normals.sort();
            normals.dedup();
            assert_eq!(normals.len(), 6);
        }
        Ok(())
    }
}
