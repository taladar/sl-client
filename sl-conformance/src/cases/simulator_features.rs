//! Request the region's `SimulatorFeatures` capability and record the flags.
//!
//! On arriving in a region a viewer GETs the region's `SimulatorFeatures`
//! capability to learn which optional features and limits the simulator
//! advertises — mesh rez/upload, bakes-on-mesh, physics materials, the max
//! attachment/texture limits, the LSL syntax id, and (on OpenSim) the grid's
//! `OpenSimExtras` subtree (currency symbol, chat ranges, prim-scale limits,
//! grid service URLs). The runtime fetches it automatically at login and on each
//! region change, surfacing the decoded map as
//! [`Event::SimulatorFeatures`]; this case additionally drives it on demand with
//! [`Command::RequestSimulatorFeatures`] and asserts a decodable reply arrives.
//!
//! A grid advertises only the subset its configuration enables, so every field
//! of [`SimulatorFeatures`] is an
//! [`Option`]: [`None`] means "not advertised" (distinct from an advertised
//! `Some(false)`). The one cross-grid invariant the case asserts is that the
//! reply carries **at least one** advertised feature — an empty map would mean
//! the capability answered but decoded to nothing. On OpenSim it additionally
//! asserts the `OpenSimExtras` subtree is present (Second Life omits it), the
//! one structural difference that reliably distinguishes the two grids' replies.
//! `1av`, `[both]`.

use std::time::{Duration, Instant};

use sl_client_tokio::{Command, Event, SimulatorFeatures};

use crate::context::TestContext;
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, check, count_metric, is_opensim, secs_metric};

/// How long to wait for the `SimulatorFeatures` reply.
const REPLY_TIMEOUT: Duration = Duration::from_secs(30);

/// Counts how many of a [`SimulatorFeatures`] reply's top-level fields the grid
/// advertised (each `Some` field counts one). A richer reply advertises more;
/// the count is recorded so the reporter can trend "feature richness" per grid
/// and a caller can tell an empty decode (`0`) from a populated one.
fn advertised_count(features: &SimulatorFeatures) -> usize {
    [
        features.mesh_rez_enabled.is_some(),
        features.mesh_upload_enabled.is_some(),
        features.mesh_xfer_enabled.is_some(),
        features.bakes_on_mesh_enabled.is_some(),
        features.physics_materials_enabled.is_some(),
        features.physics_shape_types.is_some(),
        features.animated_objects.is_some(),
        features.max_agent_attachments.is_some(),
        features.max_agent_groups_basic.is_some(),
        features.max_agent_groups_premium.is_some(),
        features.max_texture_resolution.is_some(),
        features.pbr_terrain_enabled.is_some(),
        features.gltf_enabled.is_some(),
        features.lsl_syntax_id.is_some(),
        features.open_sim_extras.is_some(),
    ]
    .into_iter()
    .filter(|advertised| *advertised)
    .count()
}

/// Requests the region's `SimulatorFeatures` capability and records the reply's
/// advertised flags and limits.
///
/// Named `…Case` rather than `SimulatorFeatures` to avoid clashing with the
/// [`SimulatorFeatures`] reply type this case decodes.
#[expect(
    clippy::module_name_repetitions,
    reason = "the bare `SimulatorFeatures` name is the reply type; the case struct needs a distinct name"
)]
#[derive(Debug)]
pub struct SimulatorFeaturesCase;

impl GridTest for SimulatorFeaturesCase {
    fn name(&self) -> &'static str {
        "simulator-features"
    }

    fn description(&self) -> &'static str {
        "Request the region's SimulatorFeatures capability and record its flags"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;

            // The runtime fetches `SimulatorFeatures` automatically on arriving
            // in the region, but drive it explicitly too so the case does not
            // depend on the timing of that fetch relative to login; `wait_for`
            // returns the first matching reply either way.
            let start = Instant::now();
            session.send(Command::RequestSimulatorFeatures).await?;
            let features = session
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::SimulatorFeatures(features) => Some(features.as_ref().clone()),
                    _ => None,
                })
                .await?;
            let elapsed = start.elapsed().as_secs_f64();

            let advertised = advertised_count(&features);
            check(
                advertised >= 1,
                "expected the SimulatorFeatures reply to advertise at least one feature",
            )?;
            if is_opensim(grid) {
                // OpenSim fills in the `OpenSimExtras` subtree (currency, chat
                // ranges, prim-scale limits, grid URLs); Second Life omits it.
                check(
                    features.open_sim_extras.is_some(),
                    "expected OpenSim to advertise the OpenSimExtras subtree",
                )?;
            }

            let metrics = ctx.metrics();
            metrics.set_timing(&secs_metric("sim_features"), elapsed);
            metrics.set(
                &count_metric("advertised_features"),
                i64::try_from(advertised).unwrap_or(-1),
            );
            metrics.set("has_open_sim_extras", features.open_sim_extras.is_some());
            if let Some(mesh_upload_enabled) = features.mesh_upload_enabled {
                metrics.set("mesh_upload_enabled", mesh_upload_enabled);
            }
            if let Some(physics_materials_enabled) = features.physics_materials_enabled {
                metrics.set("physics_materials_enabled", physics_materials_enabled);
            }
            if let Some(max_agent_attachments) = features.max_agent_attachments {
                metrics.set("max_agent_attachments", i64::from(max_agent_attachments));
            }
            if let Some(max_texture_resolution) = features.max_texture_resolution {
                metrics.set("max_texture_resolution", i64::from(max_texture_resolution));
            }
            Ok(())
        })
    }
}
