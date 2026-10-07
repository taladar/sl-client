//! Arrive in a region and record everything the grid says unasked in the
//! first half minute: who the region is, in what order the arrival comes, and
//! what it then goes on telling the viewer on a timer.
//!
//! The circuits are probed from the first datagram
//! ([`GridTest::probes_arrival`]), so the order and the cadences are read off
//! the wire rather than off the events the session makes of it.

use std::collections::{BTreeMap, BTreeSet};
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use sl_client_tokio::{Event, RegionIdentity, RegionStats, SimulatorTime};

use crate::circuit::{Seen, gaps, histogram, listen, median, offsets_of, seen_since, tally};
use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::metrics::Metrics;
use crate::registry::{GridTest, TestFuture};
use crate::support::{check, count_metric, secs_metric};

/// Where the measured answers are written down.
const SOURCE: &str = "book/src/gridspec/region-arrival.md (region-arrival, 2026-10-07)";

/// How long the arrival is watched: three of the slowest periodic message
/// either grid sends, Second Life's ten-second time message.
const WATCH: Duration = Duration::from_secs(32);

/// The case's budget.
const CASE_TIMEOUT: Duration = Duration::from_secs(120);

/// The three messages an arrival opens with, in the order both grids send
/// them.
const OPENING: &str = "AgentDataUpdate RegionHandshake AgentMovementComplete";

/// The bounds on the gap between two `SimStats`, in seconds: 2.00–2.02 on
/// Second Life, 2.99–3.00 on OpenSim.
const STATS_INTERVAL: Measured<(f64, f64)> = Measured {
    second_life: (1.7, 2.4),
    opensim: (2.6, 3.4),
    source: SOURCE,
};

/// The bounds on the gap between two `SimulatorViewerTimeMessage`s, in
/// seconds: 10.0 on Second Life, 2.53–2.55 on OpenSim.
const TIME_INTERVAL: Measured<(f64, f64)> = Measured {
    second_life: (9.0, 11.0),
    opensim: (2.2, 2.9),
    source: SOURCE,
};

/// The statistic ids a `SimStats` carries, in numeric order. OpenSim sends
/// every id from 0 to 40; Second Life leaves six of them out.
const STAT_IDS: Measured<&[u32]> = Measured {
    second_life: &[
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 17, 18, 19, 20, 24, 25, 26, 27, 28,
        29, 30, 31, 32, 33, 34, 35, 38, 39, 40,
    ],
    opensim: &[
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
        25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40,
    ],
    source: SOURCE,
};

/// The handshake's `RegionProtocols`: the central-bake bit on Second Life,
/// bit 63 ("more than six baked textures") alone on OpenSim.
const REGION_PROTOCOLS: Measured<u64> = Measured {
    second_life: 1,
    opensim: 1 << 63,
    source: SOURCE,
};

/// Whether the time message carries a sun direction. OpenSim sends a zero
/// vector and leaves the sun to the environment.
const SUN_DIRECTION_SENT: Measured<bool> = Measured {
    second_life: true,
    opensim: false,
    source: SOURCE,
};

/// Whether the handshake names the region's product and data centre
/// (`ProductName`, `ProductSKU`, `ColoName`). OpenSim sends all three empty.
const NAMES_PRODUCT: Measured<bool> = Measured {
    second_life: true,
    opensim: false,
    source: SOURCE,
};

/// How many `RegionHandshake`s a child circuit is sent: two on Second Life,
/// within a tenth of a millisecond of each other, and one on OpenSim.
const CHILD_HANDSHAKES: Measured<usize> = Measured {
    second_life: 2,
    opensim: 1,
    source: SOURCE,
};

/// Whether the arrival carries a `HealthMessage`.
const SENDS_HEALTH: Measured<bool> = Measured {
    second_life: true,
    opensim: false,
    source: SOURCE,
};

/// Whether the event queue pushes an `AgentStateUpdate` on arrival.
const SENDS_AGENT_STATE: Measured<bool> = Measured {
    second_life: true,
    opensim: false,
    source: SOURCE,
};

/// What [`WATCH`] of an arrival held.
#[derive(Debug, Default)]
struct Arrived {
    /// The root region's identity, from its handshake.
    identity: Option<RegionIdentity>,
    /// The simulator's channel and version.
    version: Option<String>,
    /// The first `SimStats`.
    stats: Option<RegionStats>,
    /// The first time message.
    time: Option<SimulatorTime>,
    /// Whether a `HealthMessage` came.
    health: bool,
    /// Whether an `AgentStateUpdate` came.
    agent_state: bool,
    /// How many times each neighbour was announced (`EnableSimulator`).
    announced: BTreeMap<SocketAddr, usize>,
    /// Every datagram of the watch.
    seen: Vec<Seen>,
}

impl Arrived {
    /// The datagrams of the root circuit.
    fn root(&self) -> impl Iterator<Item = &Seen> {
        self.seen.iter().filter(|datagram| !datagram.child)
    }

    /// The first three messages of the root circuit that are not the
    /// transport's own, as one string.
    fn opening(&self) -> String {
        self.root()
            .filter_map(|datagram| datagram.name)
            .filter(|name| !matches!(*name, "PacketAck" | "StartPingCheck" | "CompletePingCheck"))
            .take(3)
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// How many `RegionHandshake`s each child circuit was sent, not counting
    /// retransmissions.
    fn child_handshakes(&self) -> BTreeMap<SocketAddr, usize> {
        let mut handshakes: BTreeMap<SocketAddr, usize> = BTreeMap::new();
        for datagram in self
            .seen
            .iter()
            .filter(|datagram| datagram.child && !datagram.resent && datagram.is("RegionHandshake"))
        {
            let count = handshakes.entry(datagram.from).or_default();
            *count = count.saturating_add(1);
        }
        handshakes
    }

    /// The median gap between two root-circuit messages called `name`.
    fn interval(&self, name: &str) -> Option<f64> {
        median(&gaps(&offsets_of(self.root(), name)))
    }

    /// The first `SimStats`' statistic ids, in numeric order.
    fn stat_ids(&self) -> Vec<u32> {
        let ids: BTreeSet<u32> = self
            .stats
            .iter()
            .flat_map(|stats| stats.stats.iter().map(|(id, _value)| id.id()))
            .collect();
        ids.into_iter().collect()
    }
}

/// Watch the arrival for [`WATCH`] from the login.
async fn watch(session: &mut Session) -> Result<Arrived, TestFailure> {
    let started = Instant::now();
    let mut arrived = Arrived::default();
    let ended = listen(session, WATCH, |event| match event {
        Event::RegionInfoHandshake(identity) if arrived.identity.is_none() => {
            arrived.identity = Some((**identity).clone());
        }
        Event::SimulatorVersion(version) if arrived.version.is_none() => {
            arrived.version = Some(version.clone());
        }
        Event::SimStats(stats) if arrived.stats.is_none() => {
            arrived.stats = Some((**stats).clone());
        }
        Event::SimulatorTime(time) if arrived.time.is_none() => {
            arrived.time = Some((**time).clone());
        }
        Event::HealthMessage { .. } => arrived.health = true,
        Event::AgentStateUpdate { .. } => arrived.agent_state = true,
        Event::NeighborDiscovered(neighbour) => {
            let count = arrived.announced.entry(neighbour.sim).or_default();
            *count = count.saturating_add(1);
        }
        _ => {}
    })
    .await?;
    if let Some(reason) = ended {
        return Err(TestFailure::Disconnected(format!("{reason:?}")));
    }
    // Timed from the first datagram rather than from the case's own start,
    // which is some way into the burst.
    let seen = seen_since(session, 0, started);
    let first = seen.first().map_or(0.0, |datagram| datagram.offset);
    arrived.seen = seen
        .into_iter()
        .map(|datagram| Seen {
            offset: datagram.offset - first,
            ..datagram
        })
        .collect();
    Ok(arrived)
}

/// Record the region's identity as its handshake gave it.
fn record_identity(metrics: &mut Metrics, identity: &RegionIdentity, version: Option<&str>) {
    metrics.set("region_flags", format!("{:#010x}", identity.region_flags));
    metrics.set(
        "region_flags_extended",
        format!("{:#018x}", identity.region_flags_extended),
    );
    metrics.set(
        "region_protocols",
        format!("{:#018x}", identity.region_protocols),
    );
    metrics.set("maturity", format!("{:?}", identity.maturity));
    metrics.set("product", format!("{:?}", identity.product));
    metrics.set("product_sku", identity.product_sku.clone());
    metrics.set("product_name", identity.product_name.clone());
    metrics.set("colo_name", identity.colo_name.clone());
    metrics.set("cpu_class_id", identity.cpu_class_id);
    metrics.set("cpu_ratio", identity.cpu_ratio);
    metrics.set("sim_owner_is_nil", identity.sim_owner.is_nil());
    metrics.set("is_estate_manager", identity.is_estate_manager);
    metrics.set("water_height", f64::from(identity.water_height));
    metrics.set("billable_factor", f64::from(identity.billable_factor));
    metrics.set(
        &count_metric("terrain_detail_textures_nil"),
        tally(
            identity
                .terrain
                .detail_textures
                .iter()
                .filter(|texture| texture.is_nil())
                .count(),
        ),
    );
    metrics.set(
        "terrain_start_heights",
        format!("{:?}", identity.terrain.start_heights),
    );
    metrics.set(
        "terrain_height_ranges",
        format!("{:?}", identity.terrain.height_ranges),
    );
    metrics.set("simulator_version", version.unwrap_or("absent").to_owned());
}

/// Record the order of the arrival and what the grid went on sending.
fn record_traffic(metrics: &mut Metrics, arrived: &Arrived) {
    metrics.set("opening", arrived.opening());
    metrics.set("root_messages", histogram(arrived.root()));
    metrics.set(
        "child_messages",
        histogram(arrived.seen.iter().filter(|datagram| datagram.child)),
    );
    for (name, key) in [
        ("SimStats", "sim_stats_interval"),
        ("SimulatorViewerTimeMessage", "time_message_interval"),
        ("CoarseLocationUpdate", "coarse_location_interval"),
    ] {
        if let Some(interval) = arrived.interval(name) {
            metrics.set(&secs_metric(key), interval);
        }
        if let Some(first) = offsets_of(arrived.root(), name).first() {
            metrics.set(&secs_metric(&format!("{key}_first")), *first);
        }
    }
    metrics.set(
        "sim_stat_ids",
        arrived
            .stat_ids()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" "),
    );
    if let Some(stats) = &arrived.stats {
        metrics.set(
            "sim_stats_region_flags",
            format!("{:#010x}", stats.region_flags),
        );
        metrics.set("sim_stats_object_capacity", stats.object_capacity);
    }
    if let Some(time) = &arrived.time {
        metrics.set("sec_per_day", time.sec_per_day);
        metrics.set("sec_per_year", time.sec_per_year);
        metrics.set("sun_direction", format!("{:?}", time.sun_direction));
        metrics.set("sun_phase", f64::from(time.sun_phase));
    }
    metrics.set("health_message", arrived.health);
    metrics.set("agent_state_update", arrived.agent_state);
    let handshakes = arrived.child_handshakes();
    metrics.set(&count_metric("child_circuits"), tally(handshakes.len()));
    metrics.set(
        "child_handshakes",
        handshakes
            .values()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" "),
    );
    metrics.set(
        "neighbour_announcements",
        arrived
            .announced
            .values()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" "),
    );
}

/// Hold an interval to its measured bounds.
fn check_interval(
    what: &str,
    grid: Grid,
    bounds: &Measured<(f64, f64)>,
    interval: Option<f64>,
) -> Result<(), TestFailure> {
    let (low, high) = *bounds.on(grid);
    let interval = interval.ok_or_else(|| {
        TestFailure::Assertion(format!(
            "fewer than two {what} came in the watch ({SOURCE})"
        ))
    })?;
    check(
        (low..=high).contains(&interval),
        &format!("{what} comes every {interval:.2} s, outside {low}–{high} s ({SOURCE})"),
    )
}

/// Hold the arrival to what `grid` was measured sending.
fn check_arrival(
    grid: Grid,
    arrived: &Arrived,
    identity: &RegionIdentity,
) -> Result<(), TestFailure> {
    check(
        arrived.opening() == OPENING,
        &format!(
            "the arrival opened with {}, not {OPENING}",
            arrived.opening()
        ),
    )?;
    REGION_PROTOCOLS.check(
        "the handshake's RegionProtocols",
        grid,
        &identity.region_protocols,
    )?;
    let names_product = !identity.product_name.is_empty()
        && !identity.product_sku.is_empty()
        && !identity.colo_name.is_empty();
    NAMES_PRODUCT.check(
        "whether the handshake names the product and the data centre",
        grid,
        &names_product,
    )?;
    check_interval(
        "SimStats",
        grid,
        &STATS_INTERVAL,
        arrived.interval("SimStats"),
    )?;
    check_interval(
        "SimulatorViewerTimeMessage",
        grid,
        &TIME_INTERVAL,
        arrived.interval("SimulatorViewerTimeMessage"),
    )?;
    STAT_IDS.check(
        "the statistic ids a SimStats carries",
        grid,
        &arrived.stat_ids().as_slice(),
    )?;
    let sun_sent = arrived.time.as_ref().is_some_and(|time| {
        time.sun_direction.x != 0.0 || time.sun_direction.y != 0.0 || time.sun_direction.z != 0.0
    });
    SUN_DIRECTION_SENT.check(
        "whether the time message carries a sun direction",
        grid,
        &sun_sent,
    )?;
    SENDS_HEALTH.check(
        "whether the arrival carries a HealthMessage",
        grid,
        &arrived.health,
    )?;
    SENDS_AGENT_STATE.check(
        "whether the event queue pushes an AgentStateUpdate",
        grid,
        &arrived.agent_state,
    )?;
    for handshakes in arrived.child_handshakes().values() {
        CHILD_HANDSHAKES.check(
            "how many RegionHandshakes a child circuit is sent",
            grid,
            handshakes,
        )?;
    }
    Ok(())
}

/// Watches an arrival and records the region's identity, the order of the
/// burst and the cadence of what follows.
#[derive(Debug)]
pub struct RegionArrival;

impl GridTest for RegionArrival {
    fn name(&self) -> &'static str {
        "region-arrival"
    }

    fn description(&self) -> &'static str {
        "Record the handshake identity, the arrival's order and the region's periodic telemetry"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi, Grid::FakeSl, Grid::FakeOpensim]
    }

    fn probes_arrival(&self) -> bool {
        true
    }

    fn timeout(&self) -> Duration {
        CASE_TIMEOUT
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let arrived = watch(ctx.primary()).await?;
            let identity = arrived.identity.clone().ok_or_else(|| {
                TestFailure::Assertion("no RegionHandshake came for the root region".to_owned())
            })?;
            let metrics = ctx.metrics();
            record_identity(metrics, &identity, arrived.version.as_deref());
            record_traffic(metrics, &arrived);
            check_arrival(ctx.grid(), &arrived, &identity)
        })
    }
}
