//! Fetch, decode, and cache a mesh through the higher-level
//! [`MeshStore`]: pull an SL mesh asset over the live
//! `GetMesh2` / `GetMesh` capability, decode its LLMesh geometry, confirm a
//! second request is served from the in-memory cache (the same shared entry, no
//! re-fetch), and exercise a level-of-detail switch (a separate block fetch +
//! decode) and re-upgrade.
//!
//! Unlike the plywood texture, there is no universal mesh asset id. The case
//! takes the mesh id from the `mesh_asset` fixture when one is configured,
//! otherwise it scans the region's object stream for a **mesh-shaped prim** (a
//! `SculptData` whose key is a [`SculptOrMeshKey::Mesh`]) and pulls that. A grid
//! that offers neither — e.g. an OpenSim region with no mesh in view — is
//! recorded `partial` rather than failed.

use std::sync::Arc;
use std::time::{Duration, Instant};

use sl_client_tokio::{
    Command, Event, MeshCacheLimits, MeshKey, MeshLod, MeshStore, ReqwestMeshFetcher,
    SculptOrMeshKey, Throttle,
};

use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, check};

/// How long to scan the object-update stream for a mesh-shaped prim when no
/// `mesh_asset` fixture is configured.
const MESH_SCAN_WINDOW: Duration = Duration::from_secs(20);

/// Drives an SL mesh asset through the decoding, LOD-aware `MeshStore`.
#[derive(Debug)]
pub struct MeshFetchHttp;

impl GridTest for MeshFetchHttp {
    fn name(&self) -> &'static str {
        "mesh-fetch-http"
    }

    fn description(&self) -> &'static str {
        "Fetch, decode, and cache a mesh through the LOD-aware MeshStore"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let fixture_mesh = ctx.mesh_asset();

            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;
            session
                .send(Command::SetThrottle(Throttle::preset_1000()))
                .await?;

            // Resolve a fetchable mesh id: the fixture if configured, else scan
            // the region's object stream for a mesh-shaped prim.
            let mesh_id = match fixture_mesh {
                Some(id) => id,
                None => match scan_for_mesh(session).await? {
                    Some(id) => id,
                    None => {
                        ctx.mark_partial(
                            "no mesh_asset fixture and no mesh-shaped prim in view \
                             (set `mesh_asset` in fixtures.<grid>.toml)",
                        );
                        return Ok(());
                    }
                },
            };

            // Build a store over the live GetMesh2 (fallback GetMesh) capability,
            // backed by its own on-disk cache in a throwaway directory.
            let cap = session
                .cap("GetMesh2")
                .or_else(|| session.cap("GetMesh"))
                .ok_or_else(|| {
                    TestFailure::Assertion("no GetMesh2/GetMesh capability".to_owned())
                })?;
            let fetcher = Arc::new(ReqwestMeshFetcher::with_default_client());
            fetcher.set_cap_url(Some(cap));
            let dir = std::env::temp_dir()
                .join(format!("sl-conformance-meshcache-{}", std::process::id()));
            let _removed = fs_err::remove_dir_all(&dir);
            let store = MeshStore::new(fetcher, Some(dir.clone()), MeshCacheLimits::default())
                .map_err(|error| TestFailure::Assertion(format!("open mesh store: {error}")))?;

            // Finest-LOD fetch + decode.
            let start = Instant::now();
            let entry = store
                .get(mesh_id, MeshLod::High)
                .await
                .map_err(|error| TestFailure::Assertion(format!("get high mesh: {error}")))?;
            let get_secs = start.elapsed().as_secs_f64();

            let decoded = entry
                .mesh()
                .ok_or_else(|| TestFailure::Assertion("mesh decoded to no geometry".to_owned()))?;
            check(
                !decoded.submeshes.is_empty(),
                "decoded mesh has no submeshes",
            )?;
            let vertices = decoded.vertex_count();
            let triangles = decoded.triangle_count();
            check(vertices > 0, "decoded mesh has no vertices")?;
            check(triangles > 0, "decoded mesh has no triangles")?;
            // Every submesh's index list is a whole number of triangles.
            for submesh in &decoded.submeshes {
                check(
                    submesh.indices.len() % 3 == 0,
                    "a submesh index count is not a multiple of 3",
                )?;
            }

            // A second request for the same held mesh returns the same shared
            // entry from memory (no re-fetch, no re-decode).
            let again = store
                .get(mesh_id, MeshLod::High)
                .await
                .map_err(|error| TestFailure::Assertion(format!("second get: {error}")))?;
            check(
                Arc::ptr_eq(&entry, &again),
                "second get did not return the cached entry",
            )?;

            // Switch to a coarser LOD (a separate block fetch + decode), then
            // back to the finest.
            store
                .set_lod(&entry, MeshLod::Low)
                .await
                .map_err(|error| TestFailure::Assertion(format!("set_lod low: {error}")))?;
            store
                .set_lod(&entry, MeshLod::High)
                .await
                .map_err(|error| TestFailure::Assertion(format!("set_lod high: {error}")))?;

            let metrics = ctx.metrics();
            metrics.set_timing("store_get_secs", get_secs);
            metrics.set(
                "mesh_submeshes",
                i64::try_from(decoded.submeshes.len()).unwrap_or(-1),
            );
            metrics.set("mesh_vertices", i64::try_from(vertices).unwrap_or(-1));
            metrics.set("mesh_triangles", i64::try_from(triangles).unwrap_or(-1));

            let _removed = fs_err::remove_dir_all(&dir);
            Ok(())
        })
    }
}

/// Scans the region's object-update stream for the first mesh-shaped prim,
/// returning its [`MeshKey`], or `None` if none appears within the window. A
/// single [`wait_for`](Session::wait_for) drains events until the predicate
/// matches or the window elapses.
async fn scan_for_mesh(session: &mut Session) -> Result<Option<MeshKey>, TestFailure> {
    let found = session
        .wait_for(MESH_SCAN_WINDOW, |event| match event {
            Event::ObjectAdded(object) | Event::ObjectUpdated(object) => object
                .extra
                .sculpt
                .as_ref()
                .and_then(|sculpt| match sculpt.texture {
                    SculptOrMeshKey::Mesh(key) => Some(key),
                    SculptOrMeshKey::Sculpt(_texture) => None,
                }),
            _other => None,
        })
        .await;
    match found {
        Ok(key) => Ok(Some(key)),
        Err(TestFailure::Timeout(_)) => Ok(None),
        Err(other) => Err(other),
    }
}
