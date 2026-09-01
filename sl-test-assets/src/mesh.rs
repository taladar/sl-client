//! A procedural Second Life **mesh asset**: the binary-LLSD header naming one
//! zlib-compressed geometry block per level of detail, which is what the
//! `GetMesh2` capability serves and what [`sl_mesh`](https://docs.rs/sl-mesh)
//! decodes.
//!
//! The format itself lives in `sl_mesh::encode`, beside the decoder it is the
//! inverse of — one encoder, one set of upload limits. What is here is the
//! *geometry*: the fixture cube this crate's callers rez.

use sl_mesh::{MeshEncodeError, MeshModel, Submesh, encode_mesh};

/// The half-extent of the unit cube: its vertices span `[-0.5, 0.5]` on every
/// axis, the normalized box a prim's scale stretches.
const HALF: f32 = 0.5;

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
const FACE_TRIANGLES: [u32; 6] = [0, 1, 2, 0, 2, 3];

/// A unit cube as a Second Life mesh asset: 24 vertices (four per face, so
/// each face keeps its own normal and a full `0..1` texture coordinate span),
/// 12 triangles, in **one** submesh — the prim therefore has a single texture
/// face, which is what a fixture wants when it asserts "the checker is on the
/// mesh".
///
/// All four levels of detail carry the same geometry, so the asset renders
/// identically however aggressively a viewer picks its level.
///
/// # Errors
///
/// Returns [`MeshEncodeError`] if the encoder refuses the model, which a cube
/// of 24 vertices in one face cannot provoke.
pub fn unit_cube_mesh_asset() -> Result<Vec<u8>, MeshEncodeError> {
    encode_mesh(&MeshModel::default().with_every_lod(vec![unit_cube_submesh()]))
}

/// The cube as a single submesh: its 24 vertices face by face, and the 36
/// triangle indices that pair them up.
///
/// Public so a fixture can put the same geometry in a model of its own — a
/// rigged one, say, whose `Weights` stream this crate does not choose.
#[must_use]
pub fn unit_cube_submesh() -> Submesh {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::new();
    for (face, (normal, u_axis, v_axis)) in CUBE_FACES.into_iter().enumerate() {
        let base = u32::try_from(face.saturating_mul(FACE_CORNERS.len())).unwrap_or(0);
        for (u_sign, v_sign) in FACE_CORNERS {
            positions.push(axes(normal, u_axis, v_axis, u_sign, v_sign));
            normals.push(normal);
            uvs.push([f32::midpoint(u_sign, 1.0), f32::midpoint(v_sign, 1.0)]);
        }
        for corner in FACE_TRIANGLES {
            indices.push(base.saturating_add(corner));
        }
    }
    Submesh {
        positions,
        normals,
        uvs,
        indices,
        weights: None,
        normalized_scale: [1.0, 1.0, 1.0],
        no_geometry: false,
    }
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
            let (start, end) = block.range(header_size);
            let bytes = asset.get(start..end).ok_or("lod block out of range")?;
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
