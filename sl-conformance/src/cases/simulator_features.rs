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
//! of [`SimulatorFeatures`](sl_client_tokio::SimulatorFeatures) is an
//! [`Option`]: [`None`] means "not advertised" (distinct from an advertised
//! `Some(false)`).
//!
//! The case records the **whole reply** — every key, nested ones by dotted
//! path, as a `feature.<path>` metric — and holds each grid to the keys it was
//! measured sending and the LLSD kind of each (`ADVERTISED`): the reply is
//! where the two live grids introduce themselves, and they share fourteen of
//! their keys. A key a grid stops sending, a key it adds, and a key that
//! changes kind (OpenSim's `ExportSupported` is a string) each fail the case
//! by name, so the table in `book/src/gridspec/region-arrival.md` cannot go
//! stale quietly. The values are recorded, not held: most are a region's own.
//!
//! It runs on **both** fake flavours and holds each to the grid it says it
//! is, but for the script syntax: a live region advertises an `LSLSyntaxId`
//! (and Second Life its version) beside a syntax document it serves, and the
//! fake grid serves none.
//! `1av`, `[both, fake]`.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use sl_client_tokio::{Command, Event, Llsd};

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::record::MetricValue;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, count_metric, secs_metric};

/// How long to wait for the `SimulatorFeatures` reply.
const REPLY_TIMEOUT: Duration = Duration::from_secs(30);

/// The keys the fake grid leaves out of either flavour's reply: the script
/// syntax a region may only advertise beside a document it serves.
const FAKE_GRID_OMITS: &[&str] = &["LSLSyntaxId", "LSLSyntaxVersion"];

/// Every key each grid's reply carries, by dotted path, with its LLSD kind.
const ADVERTISED: Measured<&[(&str, &str)]> = Measured {
    second_life: &[
        ("AnimatedObjects", "map"),
        ("AnimatedObjects.AnimatedObjectMaxTris", "integer"),
        (
            "AnimatedObjects.MaxAgentAnimatedObjectAttachments",
            "integer",
        ),
        ("AvatarHoverHeightEnabled", "boolean"),
        ("BakesOnMeshEnabled", "boolean"),
        ("DeadReckoningDistance", "real"),
        ("DeadReckoningTime", "real"),
        ("DynamicPathfindingEnabled", "boolean"),
        ("GLTFEnabled", "boolean"),
        ("HostName", "string"),
        ("LSLSyntaxId", "uuid"),
        ("LSLSyntaxVersion", "string"),
        ("LuaScriptsEnabled", "boolean"),
        ("MaxAgentAttachments", "integer"),
        ("MaxAgentGroups", "integer"),
        ("MaxAgentGroupsBasic", "integer"),
        ("MaxAgentGroupsPremium", "integer"),
        ("MaxEstateAccessIds", "integer"),
        ("MaxEstateManagers", "integer"),
        ("MaxMaterialsPerTransaction", "integer"),
        ("MaxTextureResolution", "integer"),
        ("MeshRezEnabled", "boolean"),
        ("MeshUploadEnabled", "boolean"),
        ("MeshXferEnabled", "boolean"),
        ("MirrorsEnabled", "boolean"),
        ("NoModBypassSupport", "boolean"),
        ("PBRMaterialSwatchEnabled", "boolean"),
        ("PBRTerrainEnabled", "boolean"),
        ("PBRTerrainTransformsEnabled", "boolean"),
        ("PhysicsMaterialsEnabled", "boolean"),
        ("PhysicsShapeTypes", "map"),
        ("PhysicsShapeTypes.convex", "boolean"),
        ("PhysicsShapeTypes.none", "boolean"),
        ("PhysicsShapeTypes.prim", "boolean"),
        ("RenderMaterialsCapability", "real"),
        ("VoiceServerType", "string"),
    ],
    opensim: &[
        ("AnimatedObjects", "map"),
        ("AnimatedObjects.AnimatedObjectMaxTris", "integer"),
        (
            "AnimatedObjects.MaxAgentAnimatedObjectAttachments",
            "integer",
        ),
        ("AvatarHoverHeightEnabled", "boolean"),
        ("BakesOnMeshEnabled", "boolean"),
        ("LSLSyntaxId", "uuid"),
        ("MaxAgentAttachments", "integer"),
        ("MaxAgentGroupsBasic", "integer"),
        ("MaxAgentGroupsPremium", "integer"),
        ("MaxMaterialsPerTransaction", "integer"),
        ("MeshRezEnabled", "boolean"),
        ("MeshUploadEnabled", "boolean"),
        ("MeshXferEnabled", "boolean"),
        ("OpenSimExtras", "map"),
        ("OpenSimExtras.AnimationSet", "boolean"),
        ("OpenSimExtras.AvatarSkeleton", "boolean"),
        ("OpenSimExtras.ExportSupported", "string"),
        ("OpenSimExtras.GridName", "string"),
        ("OpenSimExtras.GridNick", "string"),
        ("OpenSimExtras.GridURL", "string"),
        ("OpenSimExtras.GridURLAlias", "string"),
        ("OpenSimExtras.MaxHeightmap", "real"),
        ("OpenSimExtras.MaxPhysPrimScale", "real"),
        ("OpenSimExtras.MaxPrimScale", "real"),
        ("OpenSimExtras.MaxSimHeight", "real"),
        ("OpenSimExtras.MinHeightmap", "real"),
        ("OpenSimExtras.MinPhysPrimScale", "real"),
        ("OpenSimExtras.MinPrimScale", "real"),
        ("OpenSimExtras.MinSimHeight", "real"),
        ("OpenSimExtras.SimulatorFPS", "real"),
        ("OpenSimExtras.SimulatorFPSCritPercent", "integer"),
        ("OpenSimExtras.SimulatorFPSFactor", "real"),
        ("OpenSimExtras.SimulatorFPSWarnPercent", "integer"),
        ("OpenSimExtras.currency-base-uri", "string"),
        ("OpenSimExtras.map-server-url", "string"),
        ("OpenSimExtras.say-range", "integer"),
        ("OpenSimExtras.shout-range", "integer"),
        ("OpenSimExtras.whisper-range", "integer"),
        ("PhysicsMaterialsEnabled", "boolean"),
        ("PhysicsShapeTypes", "map"),
        ("PhysicsShapeTypes.convex", "boolean"),
        ("PhysicsShapeTypes.none", "boolean"),
        ("PhysicsShapeTypes.prim", "boolean"),
        ("RenderMaterialsCapability", "real"),
        ("menus", "map"),
        ("menus.admin", "map"),
        ("menus.advanced", "map"),
        ("menus.agent", "map"),
        ("menus.tools", "map"),
        ("menus.world", "map"),
    ],
    source: "book/src/gridspec/region-arrival.md (simulator-features, 2026-10-07)",
};

/// Holds the reply's keys and their kinds to what `grid` was measured sending.
///
/// # Errors
///
/// Returns [`TestFailure::Assertion`] naming every key that is missing, every
/// key that was not expected, and every key whose kind differs.
fn check_advertised(
    grid: Grid,
    advertised: &BTreeMap<String, &'static str>,
) -> Result<(), TestFailure> {
    let expected: BTreeMap<&str, &str> = ADVERTISED
        .on(grid)
        .iter()
        .filter(|(key, _kind)| !(grid.is_fake() && FAKE_GRID_OMITS.contains(key)))
        .copied()
        .collect();
    let mut differences: Vec<String> = Vec::new();
    for (key, kind) in &expected {
        match advertised.get(*key) {
            None => differences.push(format!("{key} is missing")),
            Some(actual) if actual != kind => {
                differences.push(format!("{key} is a {actual}, not a {kind}"));
            }
            Some(_) => {}
        }
    }
    for (key, kind) in advertised {
        if !expected.contains_key(key.as_str()) {
            differences.push(format!("{key} ({kind}) was not expected"));
        }
    }
    if differences.is_empty() {
        return Ok(());
    }
    let verdict = if grid.is_fake() {
        "the fake grid no longer imitates"
    } else {
        "the grid no longer matches"
    };
    Err(TestFailure::Assertion(format!(
        "the SimulatorFeatures reply on {grid}: {} — {verdict} {}",
        differences.join("; "),
        ADVERTISED.source
    )))
}

/// Records every scalar of the reply as a `feature.<path>` metric.
fn record_values(prefix: &str, value: &Llsd, out: &mut BTreeMap<String, MetricValue>) {
    let Llsd::Map(members) = value else {
        return;
    };
    for (key, member) in members {
        let path = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        let metric = match member {
            Llsd::Boolean(flag) => MetricValue::Bool(*flag),
            Llsd::Integer(number) => MetricValue::Int(i64::from(*number)),
            Llsd::Real(number) => MetricValue::Float(*number),
            Llsd::String(text) | Llsd::Uri(text) | Llsd::Date(text) => {
                MetricValue::Text(text.clone())
            }
            Llsd::Uuid(id) => MetricValue::Text(id.to_string()),
            Llsd::Map(_) => {
                record_values(&path, member, out);
                continue;
            }
            Llsd::Undef | Llsd::Binary(_) | Llsd::Array(_) => {
                MetricValue::Text(format!("({})", member.kind()))
            }
        };
        let _previous = out.insert(format!("feature.{path}"), metric);
    }
}

/// Requests the region's `SimulatorFeatures` capability and records the reply's
/// advertised flags and limits.
///
/// Named `…Case` rather than `SimulatorFeatures` to avoid clashing with the
/// [`SimulatorFeatures`](sl_client_tokio::SimulatorFeatures) reply type this
/// case decodes.
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

            let advertised = features.advertised();
            check_advertised(grid, &advertised)?;

            let mut values = BTreeMap::new();
            record_values("", &features.to_llsd(), &mut values);
            let metrics = ctx.metrics();
            metrics.set_timing(&secs_metric("sim_features"), elapsed);
            metrics.set(
                &count_metric("advertised_features"),
                i64::try_from(advertised.len()).unwrap_or(-1),
            );
            metrics.set("has_open_sim_extras", features.open_sim_extras.is_some());
            metrics.set(
                "voice_server_type",
                features.voice_server_type.as_deref().unwrap_or("(none)"),
            );
            for (key, value) in values {
                metrics.set(&key, value);
            }
            Ok(())
        })
    }
}
