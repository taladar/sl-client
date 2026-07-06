//! Bevy integration for the decoding [`MeshStore`](sl_mesh::MeshStore): a bridge
//! from a decoded [`Submesh`] to Bevy's [`Mesh`], and a
//! blocking-HTTP [`MeshFetcher`](sl_mesh::MeshFetcher) so a Bevy app (which has
//! no async runtime of its own) can build and drive a mesh store.
//!
//! Because the store's `get`/`request` are `async`, a Bevy app drives them by
//! `block_on`-ing on a task/thread (the crate already fetches on `std` threads);
//! the store's decode still runs off-thread on its own `rayon` pool.
//!
//! Only geometry is bridged: a rigged mesh's skin/weights are exposed on the
//! decoded types for the app to feed into a Bevy `SkinnedMesh`; pairing each
//! face to its material is likewise the app's job (the object side carries the
//! `face → material_id` / `texture_id` pointers).

use arc_swap::ArcSwapOption;
use async_trait::async_trait;
use bevy::asset::RenderAssetUsages;
use bevy::math::Mat4;
use bevy::mesh::{Indices, Mesh, PrimitiveTopology, VertexAttributeValues};
use bytes::Bytes;
use reqwest::StatusCode as ReqwestStatusCode;
use reqwest::blocking::Client as ReqwestBlockingClient;
use sl_mesh::{
    AssetFetcher, DecodedMesh, FetchChunk, FetchError, MeshSkin, Submesh, VertexWeights,
};
use sl_proto::MeshKey;

/// The `Accept` MIME type for a mesh asset (the viewer's
/// `HTTP_CONTENT_VND_LL_MESH`).
const MESH_ACCEPT: &str = "application/vnd.ll.mesh";

/// Converts one decoded [`Submesh`] into a Bevy [`Mesh`] (a `TriangleList` with
/// position, and — when present — normal and UV0 attributes plus `u32` indices),
/// ready to insert into `Assets<Mesh>`.
#[must_use]
pub fn to_bevy_mesh(submesh: &Submesh) -> Mesh {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, submesh.positions.clone());
    if !submesh.normals.is_empty() {
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, submesh.normals.clone());
    }
    if !submesh.uvs.is_empty() {
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, submesh.uvs.clone());
    }
    mesh.insert_indices(Indices::U32(submesh.indices.clone()));
    mesh
}

/// Converts one decoded rigged [`Submesh`] into a Bevy [`Mesh`] for GPU skinning
/// (P17.2): like [`to_bevy_mesh`] but with the `JOINT_INDEX` (`Uint16x4`) and
/// `JOINT_WEIGHT` (`Float32x4`) attributes a Bevy `SkinnedMesh` consumes, taken
/// from the submesh's per-vertex [`VertexWeights`].
///
/// Each vertex's up-to-four `(joint, weight)` influences fill the four Bevy skin
/// slots in order; the joint indices are the rig-local indices into the skin's
/// `joint_names` table (so `SkinnedMesh.joints` and the
/// [`rigged_inverse_bindposes`] must be built in that same order). A vertex with
/// no influence (a face without weights) binds fully to slot `0`, so it is not
/// collapsed to the origin by an all-zero skinning matrix.
#[must_use]
pub fn to_bevy_rigged_mesh(submesh: &Submesh) -> Mesh {
    let mut mesh = to_bevy_mesh(submesh);
    let vertex_count = submesh.positions.len();
    let mut joint_indices: Vec<[u16; 4]> = Vec::with_capacity(vertex_count);
    let mut joint_weights: Vec<[f32; 4]> = Vec::with_capacity(vertex_count);
    for index in 0..vertex_count {
        let influences = submesh
            .weights
            .as_ref()
            .and_then(|weights| weights.get(index));
        let (indices, weights) = pack_influences(influences);
        joint_indices.push(indices);
        joint_weights.push(weights);
    }
    // `Vec<[u16; 4]>` has no `Into<VertexAttributeValues>` (its `TryFrom` is
    // ambiguous between `Uint16x4` and `Unorm16x4`), so name the variant.
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_JOINT_INDEX,
        VertexAttributeValues::Uint16x4(joint_indices),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, joint_weights);
    mesh
}

/// Pack one vertex's [`VertexWeights`] into the four `(joint index, weight)` slots
/// a Bevy `SkinnedMesh` reads. Influences beyond the fourth are dropped (Second
/// Life rigs carry at most four); a vertex with none binds fully to joint `0`.
fn pack_influences(weights: Option<&VertexWeights>) -> ([u16; 4], [f32; 4]) {
    let influences = weights.map_or(&[][..], |weights| weights.influences.as_slice());
    let indices = std::array::from_fn(|slot| {
        influences
            .get(slot)
            .map_or(0_u16, |&(joint, _weight)| u16::from(joint))
    });
    if influences.is_empty() {
        return (indices, [1.0, 0.0, 0.0, 0.0]);
    }
    let weights = std::array::from_fn(|slot| {
        influences
            .get(slot)
            .map_or(0.0_f32, |&(_joint, weight)| weight)
    });
    (indices, weights)
}

/// Build the inverse-bindpose matrices a rigged mesh's Bevy `SkinnedMesh` needs
/// from its [`MeshSkin`] (P17.2), one per rig joint in `joint_names` order.
///
/// Each entry folds the skin's single bind-shape matrix into the joint's
/// inverse-bind matrix, so a vertex is transformed as `joint_world *
/// inverse_bind * bind_shape * v` — the reference viewer's rigged-mesh skin
/// (bind-shape first, then the joint's palette matrix). The `sl_mesh` matrices
/// are Second Life's row-major, row-vector `[f32; 16]`; `Mat4::from_cols_array`
/// reads that same array as its transpose, which is exactly the column-vector
/// matrix Bevy's skinning multiplies, so no explicit transpose is needed. A joint
/// with no inverse-bind matrix (a malformed skin) falls back to the bind-shape
/// alone.
#[must_use]
pub fn rigged_inverse_bindposes(skin: &MeshSkin) -> Vec<Mat4> {
    let bind_shape = Mat4::from_cols_array(&skin.bind_shape_matrix);
    (0..skin.joint_names.len())
        .map(|index| {
            let inverse_bind = skin
                .inverse_bind_matrix
                .get(index)
                .map_or(Mat4::IDENTITY, Mat4::from_cols_array);
            inverse_bind.mul_mat4(&bind_shape)
        })
        .collect()
}

/// Converts a whole decoded mesh into one Bevy [`Mesh`] per face (skipping empty
/// `NoGeometry` faces), preserving face order so the app can pair each with its
/// per-face material.
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `to_bevy_meshes` reads clearly"
)]
#[must_use]
pub fn to_bevy_meshes(decoded: &DecodedMesh) -> Vec<Mesh> {
    decoded
        .submeshes
        .iter()
        .filter(|submesh| !submesh.no_geometry)
        .map(to_bevy_mesh)
        .collect()
}

/// A [`MeshFetcher`](sl_mesh::MeshFetcher) over blocking `reqwest`, for a Bevy
/// app with no async runtime. It fetches `GetMesh2` / `GetMesh` asset byte
/// ranges; the capability URL is held in an [`ArcSwapOption`] so it can be
/// refreshed on a region change.
#[derive(Debug)]
pub struct BevyMeshFetcher {
    /// The shared blocking HTTP client.
    http: ReqwestBlockingClient,
    /// The current mesh capability URL, or `None` before caps arrive.
    cap_url: ArcSwapOption<String>,
}

impl BevyMeshFetcher {
    /// A fetcher with a freshly built blocking client and no capability URL yet.
    #[must_use]
    pub fn new() -> Self {
        let http = ReqwestBlockingClient::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .unwrap_or_else(|_error| ReqwestBlockingClient::new());
        Self {
            http,
            cap_url: ArcSwapOption::empty(),
        }
    }

    /// Updates (or clears) the mesh capability URL (prefer `GetMesh2`, else
    /// `GetMesh`).
    pub fn set_cap_url(&self, url: Option<String>) {
        self.cap_url.store(url.map(std::sync::Arc::new));
    }

    /// Performs the blocking range request, returning the chunk.
    fn fetch_blocking(
        &self,
        id: MeshKey,
        start: usize,
        end: usize,
    ) -> Result<FetchChunk, FetchError> {
        let cap = self
            .cap_url
            .load_full()
            .ok_or_else(|| FetchError::Transport("mesh capability not available".to_owned()))?;
        let url = format!("{cap}/?mesh_id={id}");
        let response = self
            .http
            .get(&url)
            .header("Accept", MESH_ACCEPT)
            .header("Range", format!("bytes={start}-{}", end.saturating_sub(1)))
            .send()
            .map_err(|error| FetchError::Transport(error.to_string()))?;
        let status = response.status();
        if status == ReqwestStatusCode::NOT_FOUND {
            return Err(FetchError::NotFound);
        }
        if status == ReqwestStatusCode::RANGE_NOT_SATISFIABLE {
            return Ok(FetchChunk {
                bytes: Bytes::new(),
                whole: false,
            });
        }
        let whole = status == ReqwestStatusCode::OK;
        if !status.is_success() {
            return Err(FetchError::Transport(format!("unexpected status {status}")));
        }
        let bytes = response
            .bytes()
            .map_err(|error| FetchError::Transport(error.to_string()))?;
        Ok(FetchChunk { bytes, whole })
    }
}

impl Default for BevyMeshFetcher {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AssetFetcher<MeshKey> for BevyMeshFetcher {
    async fn fetch_range(
        &self,
        id: MeshKey,
        start: usize,
        end: usize,
    ) -> Result<FetchChunk, FetchError> {
        // The blocking request runs on whatever thread `block_on`s this future
        // (a Bevy task/thread dedicated to the fetch), which is the intended use.
        self.fetch_blocking(id, start, end)
    }
}

#[cfg(test)]
mod tests {
    use super::{rigged_inverse_bindposes, to_bevy_mesh, to_bevy_meshes, to_bevy_rigged_mesh};
    use bevy::math::Vec3;
    use bevy::mesh::{Mesh, VertexAttributeValues};
    use pretty_assertions::assert_eq;
    use sl_mesh::{DecodedMesh, MeshSkin, Submesh, VertexWeights};
    use sl_proto::MeshLod;

    /// A boxed error so tests can use `?` instead of disallowed `unwrap`/`expect`.
    type TestError = Box<dyn core::error::Error>;

    /// A single-triangle submesh with positions, normals, UVs, and indices.
    fn triangle() -> Submesh {
        Submesh {
            positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            normals: vec![[0.0, 0.0, 1.0], [0.0, 0.0, 1.0], [0.0, 0.0, 1.0]],
            uvs: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
            indices: vec![0, 1, 2],
            weights: None,
            normalized_scale: [1.0, 1.0, 1.0],
            no_geometry: false,
        }
    }

    /// The row-major, row-vector identity matrix (the `sl_mesh` skin layout).
    const IDENTITY: [f32; 16] = [
        1.0, 0.0, 0.0, 0.0, //
        0.0, 1.0, 0.0, 0.0, //
        0.0, 0.0, 1.0, 0.0, //
        0.0, 0.0, 0.0, 1.0, //
    ];

    #[test]
    fn builds_a_bevy_mesh_from_a_submesh() {
        let mesh = to_bevy_mesh(&triangle());
        // Positions attribute has the three vertices.
        let positions = mesh.attribute(Mesh::ATTRIBUTE_POSITION);
        assert!(matches!(
            positions,
            Some(VertexAttributeValues::Float32x3(values)) if values.len() == 3
        ));
        // Three indices form one triangle.
        assert_eq!(mesh.indices().map(bevy::mesh::Indices::len), Some(3));
    }

    #[test]
    fn skips_no_geometry_faces() {
        let empty = Submesh {
            no_geometry: true,
            ..Submesh::default()
        };
        let decoded = DecodedMesh {
            lod: MeshLod::High,
            submeshes: vec![triangle(), empty],
        };
        // Only the real face becomes a Bevy mesh.
        assert_eq!(to_bevy_meshes(&decoded).len(), 1);
    }

    #[test]
    fn rigged_mesh_carries_skin_attributes() -> Result<(), TestError> {
        // A rigged triangle: vertex 0 fully on joint 1, vertex 1 split 0/2, vertex
        // 2 with no influence (a defensive gap in the weights).
        let mut submesh = triangle();
        submesh.weights = Some(vec![
            VertexWeights {
                influences: vec![(1, 1.0)],
            },
            VertexWeights {
                influences: vec![(0, 0.25), (2, 0.75)],
            },
            VertexWeights {
                influences: Vec::new(),
            },
        ]);
        let mesh = to_bevy_rigged_mesh(&submesh);
        let Some(VertexAttributeValues::Uint16x4(indices)) =
            mesh.attribute(Mesh::ATTRIBUTE_JOINT_INDEX)
        else {
            return Err("JOINT_INDEX is not a Uint16x4 attribute".into());
        };
        let Some(VertexAttributeValues::Float32x4(weights)) =
            mesh.attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT)
        else {
            return Err("JOINT_WEIGHT is not a Float32x4 attribute".into());
        };
        assert_eq!(indices.len(), 3);
        assert_eq!(weights.len(), 3);
        // Vertex 0: single joint 1 at full weight. Vertex 1: two influences filling
        // the first two slots in order. Vertex 2: no influence binds fully to joint
        // 0 (not a zero matrix).
        assert_eq!(indices.first(), Some(&[1, 0, 0, 0]));
        assert_eq!(indices.get(1), Some(&[0, 2, 0, 0]));
        assert_eq!(indices.get(2), Some(&[0, 0, 0, 0]));
        assert!(weights_close(weights.first(), [1.0, 0.0, 0.0, 0.0]));
        assert!(weights_close(weights.get(1), [0.25, 0.75, 0.0, 0.0]));
        assert!(weights_close(weights.get(2), [1.0, 0.0, 0.0, 0.0]));
        Ok(())
    }

    #[test]
    fn rigged_inverse_bindposes_fold_the_bind_shape() -> Result<(), TestError> {
        // A skin whose inverse-bind is identity and whose bind shape translates
        // +2 X (row-vector: translation in the last row).
        let mut bind_shape = IDENTITY;
        bind_shape[12] = 2.0;
        let skin = MeshSkin {
            joint_names: vec!["a".to_owned(), "b".to_owned()],
            inverse_bind_matrix: vec![IDENTITY, IDENTITY],
            bind_shape_matrix: bind_shape,
            alt_inverse_bind_matrix: Vec::new(),
            pelvis_offset: None,
            lock_scale_if_joint_position: false,
        };
        let bindposes = rigged_inverse_bindposes(&skin);
        assert_eq!(bindposes.len(), 2);
        // Each folded bindpose maps a point through the bind shape alone (identity
        // inverse-bind), so the origin lands at +2 X. `Mat4::from_cols_array` of
        // the row-major bind shape is its column-vector transpose, which is exactly
        // what Bevy's skinning multiplies.
        for bindpose in &bindposes {
            let moved = bindpose.transform_point3(Vec3::ZERO);
            assert!((moved - Vec3::new(2.0, 0.0, 0.0)).length() < 1e-5);
        }
        Ok(())
    }

    #[test]
    fn rigged_inverse_bindpose_matches_a_known_transform() -> Result<(), TestError> {
        // inverse-bind translates -3 X, bind shape identity: the origin maps to -3 X.
        let mut inverse_bind = IDENTITY;
        inverse_bind[12] = -3.0;
        let skin = MeshSkin {
            joint_names: vec!["j".to_owned()],
            inverse_bind_matrix: vec![inverse_bind],
            bind_shape_matrix: IDENTITY,
            alt_inverse_bind_matrix: Vec::new(),
            pelvis_offset: None,
            lock_scale_if_joint_position: false,
        };
        let bindposes = rigged_inverse_bindposes(&skin);
        let bindpose = bindposes.first().ok_or("one bindpose")?;
        let moved = bindpose.transform_point3(Vec3::ZERO);
        assert!((moved - Vec3::new(-3.0, 0.0, 0.0)).length() < 1e-5);
        Ok(())
    }

    /// Compare an optional `[f32; 4]` weight slot to an expected value within a
    /// tolerance (keeps the assertion off `float_cmp`).
    fn weights_close(actual: Option<&[f32; 4]>, expected: [f32; 4]) -> bool {
        actual.is_some_and(|actual| {
            actual
                .iter()
                .zip(expected.iter())
                .all(|(a, b)| (a - b).abs() < 1e-6)
        })
    }
}
