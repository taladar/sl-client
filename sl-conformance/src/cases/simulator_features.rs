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
//! the capability answered but decoded to nothing.
//!
//! Everything else it asserts is **per grid**, because the two live grids
//! introduce themselves differently and this reply is where they do it:
//!
//! - `OpenSimExtras` is the one structural difference that reliably tells the
//!   two replies apart. OpenSim always sends it; Second Life has no such key.
//! - `VoiceServerType` is the reverse: Second Life names its spatial-voice
//!   backend here, OpenSim names it nowhere at all (the string appears in none
//!   of its sources) and leaves the viewer to fall back to Vivox on its own.
//!
//! So the case runs on **both** fake flavours and holds each to the grid it
//! says it is — a survey of exactly what the two disagree about, which is the
//! shape that is worth running twice.
//! `1av`, `[both, fake]`.

use std::time::{Duration, Instant};

use sl_client_tokio::{Command, Event, SimulatorFeatures};
use sl_fake_grid::ImitatedGrid;

use crate::context::TestContext;
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, check, count_metric, secs_metric};

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
        &[Grid::Opensim, Grid::Aditi, Grid::FakeSl, Grid::FakeOpensim]
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
            // How a region introduces itself is where the two live grids
            // disagree, so each is held to the grid it says it is — including
            // the fake one, which is whichever flavour was asked for.
            match grid.behaves_like() {
                ImitatedGrid::OpenSim => {
                    // OpenSim fills in the `OpenSimExtras` subtree (currency,
                    // chat ranges, prim-scale limits, grid URLs) and names no
                    // voice backend anywhere.
                    check(
                        features.open_sim_extras.is_some(),
                        "expected an OpenSim-flavoured grid to advertise the OpenSimExtras subtree",
                    )?;
                    check(
                        features.voice_server_type.is_none(),
                        "expected an OpenSim-flavoured grid to advertise no VoiceServerType",
                    )?;
                }
                ImitatedGrid::SecondLife => {
                    // Second Life has no such key, and names its spatial-voice
                    // backend here instead. Which backend is not asserted: it
                    // was Vivox until 2024 and is a thing the grid may change
                    // again, so the case records it rather than pinning it.
                    check(
                        features.open_sim_extras.is_none(),
                        "expected a Second-Life-flavoured grid to omit the OpenSimExtras subtree",
                    )?;
                    check(
                        features.voice_server_type.is_some(),
                        "expected a Second-Life-flavoured grid to name its VoiceServerType",
                    )?;
                }
            }

            let metrics = ctx.metrics();
            metrics.set_timing(&secs_metric("sim_features"), elapsed);
            metrics.set(
                &count_metric("advertised_features"),
                i64::try_from(advertised).unwrap_or(-1),
            );
            metrics.set("has_open_sim_extras", features.open_sim_extras.is_some());
            metrics.set(
                "voice_server_type",
                features.voice_server_type.as_deref().unwrap_or("(none)"),
            );
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
