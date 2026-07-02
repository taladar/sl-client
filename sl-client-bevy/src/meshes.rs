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
use bevy::mesh::{Indices, Mesh, PrimitiveTopology};
use bytes::Bytes;
use reqwest::StatusCode as ReqwestStatusCode;
use reqwest::blocking::Client as ReqwestBlockingClient;
use sl_mesh::{AssetFetcher, DecodedMesh, FetchChunk, FetchError, Submesh};
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
    use super::{to_bevy_mesh, to_bevy_meshes};
    use bevy::mesh::{Mesh, VertexAttributeValues};
    use pretty_assertions::assert_eq;
    use sl_mesh::{DecodedMesh, Submesh};
    use sl_proto::MeshLod;

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
}
