//! Arrive in a region and take a census of every `LayerData` the grid sends:
//! the ground, the wind and the clouds.
//!
//! A simulator streams a region's ground at an arriving viewer as `LayerData`
//! packets of 16 × 16-metre patches, and goes on sending the wind (and, on
//! some grids, the clouds) in the same encoding for as long as the agent
//! stays. Two grids that agree on the codec still differ in everything around
//! it: how many patches one message holds, the order the region is walked in,
//! how long the ground takes to arrive, whether a neighbour's ground comes
//! too, and how often the wind is sent. This case records all of that, message
//! by message, on both live grids and both fake flavours.
//!
//! The circuits are probed from the first datagram
//! ([`GridTest::probes_arrival`]), so a message's time, length and reliability
//! are read off the wire; what it carried comes from the
//! [`Event::TerrainLayerBatch`] the session makes of it.
//!
//! On the fake grid the ground is also a value this workspace declares, so
//! there the case holds the decoded heights to the fixture's: a picture cannot
//! tell a missing patch from one drawn dark, and this can.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use sl_client_tokio::{Event, RegionIdentity, TerrainLayerBatch, TerrainLayerType, TerrainPatch};

use crate::circuit::{Seen, gaps, listen, median, processed_since, tally, timeline};
use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::metrics::Metrics;
use crate::registry::{GridTest, TestFuture};
use crate::support::{check, check_eq, count_metric, secs_metric};

/// Where the measured answers are written down.
const SOURCE: &str = "book/src/gridspec/terrain.md (terrain-layerdata, 2026-10-08)";

/// How long a live arrival is watched: long enough for several of OpenSim's
/// wind updates and for a slow ground to finish.
const LIVE_WATCH: Duration = Duration::from_secs(100);

/// How long a fake grid's arrival is watched: two of the slower flavour's
/// wind updates.
const FAKE_WATCH: Duration = Duration::from_secs(32);

/// The case's budget.
const CASE_TIMEOUT: Duration = Duration::from_secs(240);

/// Patches along each edge of a standard 256 m region: `256 / 16`.
const PATCHES_PER_EDGE: u32 = 16;

/// Every land patch a standard region streams: [`PATCHES_PER_EDGE`] squared,
/// as a count.
const REGION_PATCHES: usize = 16 * 16;

/// The edge length, in cells, of one standard patch.
const PATCH_CELLS: u32 = 16;

/// The ground height the fake grid's stock terrain is flat at, in metres —
/// [`sl_fake_grid::scenario::STOCK_TERRAIN_HEIGHT_M`] as a float, because the
/// wire carries decoded heights as floats.
///
/// Written out rather than converted because the grid's constant is a `u8` and
/// `f32::from` is not a `const fn`; the test below ties the two together.
const STOCK_HEIGHT_M: f32 = 25.0;

/// How far a decoded height may sit from the height the fixture declares, in
/// metres.
///
/// The `LayerData` codec is lossy by construction: heights go over the wire as
/// a quantised, DCT-compressed patch, so a flat 25 m plane comes back as 25 m
/// give or take the quantiser — not as the same bits. A tenth of a metre is far
/// inside "this is the ground the fixture declared" and far outside "this is a
/// different height".
const HEIGHT_TOLERANCE_M: f32 = 0.1;

/// The stride both grids write in a land layer's group header.
const LAND_STRIDE: u32 = 264;

/// How near the ground's last message a wind message has to come to count as
/// sent with it, in seconds. Second Life's came 0.3 ms behind.
const WITH_THE_GROUND: f64 = 0.05;

/// The bounds on the payload of a land message that is not the region's
/// last, in bytes. Second Life keeps a message within 1,200 bytes and its end
/// marker (1,004 to 1,201 measured); OpenSim closes one with the patch that
/// takes it past 890 (899 to 911 measured in the agent's region, up to 1,071
/// in a neighbour's).
const LAND_PAYLOAD: Measured<(usize, usize)> = Measured {
    second_life: (900, 1_201),
    opensim: (891, 1_100),
    source: SOURCE,
};

/// Whether the ground comes strictly nearest patch first, distances counted
/// in whole patches from the one the agent is in. OpenSim's does; Second
/// Life's is nearest first only roughly, and not in the same order twice.
const STRICT_PATCH_ORDER: Measured<bool> = Measured {
    second_life: false,
    opensim: true,
    source: SOURCE,
};

/// Whether any land patch is written with two bits of prequantization: the
/// header-only form OpenSim gives a patch with no relief. Second Life writes
/// every patch with ten.
const FLAT_HEADER_ONLY: Measured<bool> = Measured {
    second_life: false,
    opensim: true,
    source: SOURCE,
};

/// Whether a wind message comes with the last of the ground. Second Life
/// sends an arriving agent its wind there and then; OpenSim's comes when the
/// region's clock next says.
const WIND_WITH_GROUND: Measured<bool> = Measured {
    second_life: true,
    opensim: false,
    source: SOURCE,
};

/// The bounds on the gap between two wind messages of the root region, in
/// seconds: 1.00 on Second Life, 13.63 to 13.64 on OpenSim.
const WIND_INTERVAL: Measured<(f64, f64)> = Measured {
    second_life: (0.8, 1.2),
    opensim: (13.0, 14.3),
    source: SOURCE,
};

/// Whether the wind is sent reliably.
const WIND_RELIABLE: Measured<bool> = Measured {
    second_life: false,
    opensim: true,
    source: SOURCE,
};

/// The stride a wind layer's group header states.
const WIND_STRIDE: Measured<u32> = Measured {
    second_life: 18,
    opensim: 264,
    source: SOURCE,
};

/// The prequantization exponent a wind patch states.
const WIND_PREQUANT: Measured<u32> = Measured {
    second_life: 6,
    opensim: 10,
    source: SOURCE,
};

/// One `LayerData` message: what it carried and how it travelled.
#[derive(Debug, Clone)]
struct Message {
    /// What the session read out of it.
    batch: TerrainLayerBatch,
    /// The datagram it came in, when one could be paired with it.
    datagram: Option<Seen>,
}

impl Message {
    /// Seconds from the arrival's first datagram to this message.
    fn offset(&self) -> Option<f64> {
        self.datagram.map(|datagram| datagram.offset)
    }
}

/// Everything a watch of an arrival held.
#[derive(Debug, Default)]
struct Census {
    /// The root region's identity, from its handshake.
    identity: Option<RegionIdentity>,
    /// Where the simulator placed the agent, in region metres east and north.
    placed: Option<(f32, f32)>,
    /// Every `LayerData` message, in the order the session processed them.
    messages: Vec<Message>,
    /// Every decoded patch, in the same order.
    patches: Vec<TerrainPatch>,
    /// How many `LayerData` datagrams the probe reported, retransmissions the
    /// session discarded left out.
    datagrams: usize,
}

impl Census {
    /// The messages of `layer` on the root circuit (`child == false`) or on
    /// the child circuits.
    fn of(&self, layer: TerrainLayerType, child: bool) -> impl Iterator<Item = &Message> {
        self.messages
            .iter()
            .filter(move |message| message.batch.layer == layer && message.batch.child == child)
    }

    /// Every layer type any message named, root or child.
    fn layers(&self) -> BTreeSet<u8> {
        self.messages
            .iter()
            .map(|message| message.batch.layer.code())
            .collect()
    }

    /// The root region's land patch positions in the order sent, repeats
    /// included.
    fn land_order(&self) -> Vec<(u32, u32)> {
        self.of(TerrainLayerType::Land, false)
            .flat_map(|message| message.batch.patches.iter())
            .map(|patch| (patch.patch_x, patch.patch_y))
            .collect()
    }

    /// The decoded patches of `layer` in the root region.
    fn values_of(&self, layer: TerrainLayerType) -> impl Iterator<Item = &TerrainPatch> {
        let root = self
            .messages
            .iter()
            .find(|message| !message.batch.child)
            .map(|message| message.batch.region_handle);
        self.patches
            .iter()
            .filter(move |patch| patch.layer == layer && Some(patch.region_handle) == root)
    }

    /// The median gap between two root-circuit messages of `layer`.
    fn interval(&self, layer: TerrainLayerType) -> Option<f64> {
        let offsets: Vec<f64> = self.of(layer, false).filter_map(Message::offset).collect();
        median(&gaps(&offsets))
    }
}

/// How long `grid`'s arrival is watched.
const fn watch_for(grid: Grid) -> Duration {
    if grid.is_fake() {
        FAKE_WATCH
    } else {
        LIVE_WATCH
    }
}

/// Watch the arrival for `duration` from the login.
async fn watch(session: &mut Session, duration: Duration) -> Result<Census, TestFailure> {
    let started = Instant::now();
    let mut census = Census::default();
    let mut batches: Vec<TerrainLayerBatch> = Vec::new();
    let ended = listen(session, duration, |event| match event {
        Event::RegionInfoHandshake(identity) if census.identity.is_none() => {
            census.identity = Some((**identity).clone());
        }
        Event::AgentArrived { position, .. } if census.placed.is_none() => {
            census.placed = Some((position.x(), position.y()));
        }
        Event::TerrainLayerBatch(batch) => batches.push((**batch).clone()),
        Event::TerrainPatch(patch) => census.patches.push((**patch).clone()),
        _ => {}
    })
    .await?;
    if let Some(reason) = ended {
        return Err(TestFailure::Disconnected(format!("{reason:?}")));
    }
    // Timed from the first datagram rather than from the case's own start,
    // which is some way into the burst.
    let seen = processed_since(session, 0, started);
    let first = seen.first().map_or(0.0, |datagram| datagram.offset);
    let datagrams: Vec<Seen> = seen
        .into_iter()
        .filter(|datagram| datagram.is("LayerData"))
        .map(|datagram| Seen {
            offset: datagram.offset - first,
            ..datagram
        })
        .collect();
    census.datagrams = datagrams.len();
    // The session makes one batch of each datagram it processes, in order, so
    // the two lists pair up index by index — unless a datagram's payload was
    // refused, in which case nothing after it can be trusted to line up and
    // no message is given a datagram at all.
    let paired = datagrams.len() == batches.len();
    census.messages = batches
        .into_iter()
        .enumerate()
        .map(|(index, batch)| Message {
            batch,
            datagram: paired.then(|| datagrams.get(index).copied()).flatten(),
        })
        .collect();
    Ok(census)
}

/// `values` as one metric string of their distinct values and how often each
/// came (`4:63 2:1`), most frequent first.
fn distribution<T: Ord + core::fmt::Display>(values: impl IntoIterator<Item = T>) -> String {
    let mut counts: BTreeMap<T, usize> = BTreeMap::new();
    for value in values {
        let count = counts.entry(value).or_default();
        *count = count.saturating_add(1);
    }
    let mut ordered: Vec<(T, usize)> = counts.into_iter().collect();
    ordered.sort_by(|(_, left), (_, right)| right.cmp(left));
    ordered
        .iter()
        .map(|(value, count)| format!("{value}:{count}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Patch positions as one metric string (`0,0 1,0 2,0`).
fn positions(order: &[(u32, u32)]) -> String {
    order
        .iter()
        .map(|(x, y)| format!("{x},{y}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The lowest and highest of `values`, or `None` of an empty list.
fn span(values: impl IntoIterator<Item = f32>) -> Option<(f32, f32)> {
    values.into_iter().fold(None, |span, value| {
        Some(span.map_or((value, value), |(low, high): (f32, f32)| {
            (low.min(value), high.max(value))
        }))
    })
}

/// A layer's one-letter name in a metric key.
fn layer_key(layer: TerrainLayerType) -> String {
    match layer {
        TerrainLayerType::Land => "land".to_owned(),
        TerrainLayerType::Wind => "wind".to_owned(),
        TerrainLayerType::Cloud => "cloud".to_owned(),
        TerrainLayerType::Water => "water".to_owned(),
        other => format!("layer_{:#04x}", other.code()),
    }
}

/// Record how the messages of one layer on one kind of circuit travelled and
/// what their headers state.
fn record_layer(metrics: &mut Metrics, census: &Census, layer: TerrainLayerType, child: bool) {
    let messages: Vec<&Message> = census.of(layer, child).collect();
    if messages.is_empty() {
        return;
    }
    let prefix = format!(
        "{}_{}",
        if child { "child" } else { "root" },
        layer_key(layer)
    );
    metrics.set(
        &count_metric(&format!("{prefix}_messages")),
        tally(messages.len()),
    );
    metrics.set(
        &format!("{prefix}_patches_per_message"),
        distribution(messages.iter().map(|message| message.batch.patches.len())),
    );
    metrics.set(
        &format!("{prefix}_payload_bytes"),
        span(messages.iter().map(|message| {
            f32::from(u16::try_from(message.batch.payload_len).unwrap_or(u16::MAX))
        }))
        .map_or_else(String::new, |(low, high)| format!("{low}..{high}")),
    );
    metrics.set(
        &format!("{prefix}_datagram_bytes"),
        span(
            messages
                .iter()
                .filter_map(|message| message.datagram)
                .map(|datagram| f32::from(u16::try_from(datagram.len).unwrap_or(u16::MAX))),
        )
        .map_or_else(String::new, |(low, high)| format!("{low}..{high}")),
    );
    metrics.set(
        &format!("{prefix}_reliable"),
        distribution(
            messages
                .iter()
                .filter_map(|message| message.datagram)
                .map(|datagram| datagram.reliable),
        ),
    );
    metrics.set(
        &format!("{prefix}_stride"),
        distribution(messages.iter().map(|message| message.batch.stride)),
    );
    metrics.set(
        &format!("{prefix}_patch_size"),
        distribution(messages.iter().map(|message| message.batch.patch_size)),
    );
    metrics.set(
        &format!("{prefix}_message_type_agrees"),
        distribution(
            messages
                .iter()
                .map(|message| message.batch.message_layer == message.batch.layer),
        ),
    );
    let headers = || {
        messages
            .iter()
            .flat_map(|message| message.batch.patches.iter())
    };
    metrics.set(
        &format!("{prefix}_prequant"),
        distribution(headers().map(|patch| patch.prequant)),
    );
    metrics.set(
        &format!("{prefix}_word_bits"),
        distribution(headers().map(|patch| patch.word_bits)),
    );
    metrics.set(
        &format!("{prefix}_range"),
        distribution(headers().map(|patch| patch.range)),
    );
    let offsets: Vec<f64> = messages
        .iter()
        .filter_map(|message| message.offset())
        .collect();
    if let (Some(first), Some(last)) = (offsets.first(), offsets.last()) {
        metrics.set(&secs_metric(&format!("{prefix}_first")), *first);
        metrics.set(&secs_metric(&format!("{prefix}_last")), *last);
    }
    if let Some(interval) = median(&gaps(&offsets)) {
        metrics.set(&secs_metric(&format!("{prefix}_interval")), interval);
    }
    // The ground's own timeline is hundreds of entries; the fields' is the
    // cadence itself.
    if !layer.is_land() {
        metrics.set(&format!("{prefix}_offsets"), timeline(&offsets));
        metrics.set(
            &format!("{prefix}_positions"),
            distribution(headers().map(|patch| format!("{},{}", patch.patch_x, patch.patch_y))),
        );
    }
}

/// Record the root region's ground: which patches came, in what order, how
/// often, and what they decode to.
fn record_land(metrics: &mut Metrics, census: &Census) {
    let order = census.land_order();
    let distinct: BTreeSet<(u32, u32)> = order.iter().copied().collect();
    metrics.set(&count_metric("land_patches"), tally(distinct.len()));
    metrics.set(
        &count_metric("land_patches_repeated"),
        tally(order.len().saturating_sub(distinct.len())),
    );
    metrics.set("land_order", positions(&order));
    metrics.set(
        "land_payload_sizes",
        census
            .of(TerrainLayerType::Land, false)
            .map(|message| message.batch.payload_len.to_string())
            .collect::<Vec<_>>()
            .join(" "),
    );
    if let Some((east, north)) = census.placed {
        metrics.set("agent_placed", format!("{east:.1},{north:.1}"));
    }
    if let Some((low, high)) = span(
        census
            .values_of(TerrainLayerType::Land)
            .flat_map(|patch| patch.values.iter().copied()),
    ) {
        metrics.set("land_lowest_m", f64::from(low));
        metrics.set("land_highest_m", f64::from(high));
    }
    // When the whole ground was there: the offset of the message that
    // brought the last patch not seen before.
    let mut seen: BTreeSet<(u32, u32)> = BTreeSet::new();
    let mut complete = None;
    for message in census.of(TerrainLayerType::Land, false) {
        for patch in &message.batch.patches {
            if seen.insert((patch.patch_x, patch.patch_y)) {
                complete = message.offset();
            }
        }
    }
    if let Some(complete) = complete {
        metrics.set(&secs_metric("land_complete"), complete);
    }
}

/// Record a field layer's values in the root region: their span over the
/// whole watch, and whether one send differed from the last.
fn record_field(metrics: &mut Metrics, census: &Census, layer: TerrainLayerType) {
    let key = layer_key(layer);
    let patches: Vec<&TerrainPatch> = census.values_of(layer).collect();
    if let Some((low, high)) = span(
        patches
            .iter()
            .flat_map(|patch| patch.values.iter().copied()),
    ) {
        metrics.set(&format!("{key}_lowest"), f64::from(low));
        metrics.set(&format!("{key}_highest"), f64::from(high));
    }
    if let Some((low, high)) = span(
        patches
            .iter()
            .filter_map(|patch| span(patch.values.iter().copied()).map(|(low, high)| high - low)),
    ) {
        metrics.set(
            &format!("{key}_spread_within_patch"),
            format!("{low}..{high}"),
        );
    }
}

/// Record the children: how many neighbours sent a ground, and how much of
/// it.
fn record_children(metrics: &mut Metrics, census: &Census) {
    let mut by_region: BTreeMap<u64, BTreeSet<(u32, u32)>> = BTreeMap::new();
    for message in census.of(TerrainLayerType::Land, true) {
        let region = by_region.entry(message.batch.region_handle.0).or_default();
        for patch in &message.batch.patches {
            let _new = region.insert((patch.patch_x, patch.patch_y));
        }
    }
    metrics.set(
        &count_metric("child_regions_with_land"),
        tally(by_region.len()),
    );
    metrics.set(
        "child_land_patches",
        by_region
            .values()
            .map(|patches| patches.len().to_string())
            .collect::<Vec<_>>()
            .join(" "),
    );
}

/// Record everything the census holds.
fn record(metrics: &mut Metrics, census: &Census, identity: &RegionIdentity) {
    metrics.set(
        &count_metric("layer_data_messages"),
        tally(census.messages.len()),
    );
    metrics.set(
        &count_metric("layer_data_datagrams"),
        tally(census.datagrams),
    );
    metrics.set(
        "layer_types",
        census
            .layers()
            .iter()
            .map(|code| char::from(*code).to_string())
            .collect::<Vec<_>>()
            .join(" "),
    );
    for child in [false, true] {
        for code in census.layers() {
            record_layer(metrics, census, TerrainLayerType::from_code(code), child);
        }
    }
    record_land(metrics, census);
    record_field(metrics, census, TerrainLayerType::Wind);
    record_field(metrics, census, TerrainLayerType::Cloud);
    record_children(metrics, census);
    metrics.set(
        "sim_name",
        identity
            .sim_name
            .as_ref()
            .map_or_else(String::new, ToString::to_string),
    );
    metrics.set("water_height", f64::from(identity.water_height));
    for (index, texture) in identity.terrain.detail_textures.iter().enumerate() {
        metrics.set(&format!("detail{index}_id"), texture.to_string());
    }
    metrics.set(
        "start_heights",
        format!("{:?}", identity.terrain.start_heights),
    );
    metrics.set(
        "height_ranges",
        format!("{:?}", identity.terrain.height_ranges),
    );
}

/// Hold the fake grid's ground to the fixture it declares.
fn check_fixture(census: &Census) -> Result<(), TestFailure> {
    let patches: Vec<&TerrainPatch> = census.values_of(TerrainLayerType::Land).collect();
    let seen: BTreeSet<(u32, u32)> = patches
        .iter()
        .filter(|patch| patch.size == PATCH_CELLS)
        .map(|patch| (patch.patch_x, patch.patch_y))
        .collect();
    check_eq("land patches", &seen.len(), &REGION_PATCHES)?;
    // Every patch index in the 16 × 16 grid, exactly once: a set of the
    // right *size* could still be the same corner sent 256 times.
    for patch_y in 0..PATCHES_PER_EDGE {
        for patch_x in 0..PATCHES_PER_EDGE {
            check(
                seen.contains(&(patch_x, patch_y)),
                &format!("the ground is missing its patch at ({patch_x}, {patch_y})"),
            )?;
        }
    }
    let worst = patches
        .iter()
        .flat_map(|patch| patch.values.iter())
        .map(|height| (height - STOCK_HEIGHT_M).abs())
        .fold(0.0_f32, f32::max);
    check(
        worst <= HEIGHT_TOLERANCE_M,
        &format!(
            "a decoded ground height is {worst} m from the fixture's {STOCK_HEIGHT_M} m, \
             beyond the codec's {HEIGHT_TOLERANCE_M} m tolerance"
        ),
    )
}

/// The distance from `from` to the centre of the patch at `position`, in
/// metres.
fn centre_distance(position: (u32, u32), from: (f32, f32)) -> f32 {
    let centre =
        |index: u32| f32::from(u16::try_from(index).unwrap_or(u16::MAX)).mul_add(16.0, 8.0);
    (centre(position.0) - from.0).hypot(centre(position.1) - from.1)
}

/// How many patches of `order` are nearer the patch `from` than the one sent
/// before them, distances counted in whole patches.
fn patch_order_inversions(order: &[(u32, u32)], from: (u32, u32)) -> usize {
    let distance = |position: &(u32, u32)| {
        let east = u64::from(position.0.abs_diff(from.0));
        let north = u64::from(position.1.abs_diff(from.1));
        east.saturating_mul(east)
            .saturating_add(north.saturating_mul(north))
    };
    order
        .iter()
        .zip(order.iter().skip(1))
        .filter(|(earlier, later)| distance(later) < distance(earlier))
        .count()
}

/// The patch the region metre `(east, north)` is in.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a region coordinate clamped into 0..256 and divided by 16 is a patch index \
              in 0..16; no From impl exists"
)]
fn patch_of(placed: (f32, f32)) -> (u32, u32) {
    let index = |metres: f32| (metres.clamp(0.0, 255.0) / 16.0).floor() as u32;
    (index(placed.0), index(placed.1))
}

/// Hold the ground to what both grids send and to what `grid` was measured
/// sending.
fn check_land(grid: Grid, census: &Census) -> Result<(), TestFailure> {
    let order = census.land_order();
    let distinct: BTreeSet<(u32, u32)> = order.iter().copied().collect();
    check_eq("land patches", &distinct.len(), &REGION_PATCHES)?;
    check_eq("land patches sent", &order.len(), &REGION_PATCHES)?;
    let messages: Vec<&Message> = census.of(TerrainLayerType::Land, false).collect();
    for message in &messages {
        check_eq("a land layer's stride", &message.batch.stride, &LAND_STRIDE)?;
        check_eq(
            "a land layer's patch size",
            &message.batch.patch_size,
            &PATCH_CELLS,
        )?;
        check(
            message.batch.message_layer == message.batch.layer,
            "a LayerData's LayerID.Type and its group header name different layers",
        )?;
        check(
            message.datagram.is_some_and(|datagram| datagram.reliable),
            "a land LayerData was not sent reliably, or could not be paired with its datagram",
        )?;
    }
    let (low, high) = *LAND_PAYLOAD.on(grid);
    for message in messages.iter().rev().skip(1) {
        let len = message.batch.payload_len;
        check(
            (low..=high).contains(&len),
            &format!("a land payload is {len} bytes, outside {low}–{high} ({SOURCE})"),
        )?;
    }
    let placed = census.placed.ok_or_else(|| {
        TestFailure::Assertion("the simulator never said where it placed the agent".to_owned())
    })?;
    // Both grids open with a patch as near the agent as any.
    let first = order.first().copied().unwrap_or_default();
    let nearest = distinct
        .iter()
        .map(|position| centre_distance(*position, placed))
        .fold(f32::MAX, f32::min);
    check(
        centre_distance(first, placed) <= nearest + 0.01,
        &format!(
            "the ground opened with patch {first:?}, which is not the nearest to the agent at \
             {placed:?}"
        ),
    )?;
    STRICT_PATCH_ORDER.check(
        "whether the ground comes strictly nearest patch first",
        grid,
        &(patch_order_inversions(&order, patch_of(placed)) == 0),
    )?;
    let header_only = messages
        .iter()
        .flat_map(|message| message.batch.patches.iter())
        .any(|patch| patch.prequant == 2);
    FLAT_HEADER_ONLY.check(
        "whether a land patch is written as a header alone",
        grid,
        &header_only,
    )?;
    // A neighbour's ground comes whole on both grids.
    let mut neighbours: BTreeMap<u64, BTreeSet<(u32, u32)>> = BTreeMap::new();
    for message in census.of(TerrainLayerType::Land, true) {
        let region = neighbours.entry(message.batch.region_handle.0).or_default();
        for patch in &message.batch.patches {
            let _new = region.insert((patch.patch_x, patch.patch_y));
        }
    }
    for patches in neighbours.values() {
        check_eq(
            "a neighbour's land patches",
            &patches.len(),
            &REGION_PATCHES,
        )?;
    }
    Ok(())
}

/// Hold the wind to what both grids send and to what `grid` was measured
/// sending.
fn check_wind(grid: Grid, census: &Census) -> Result<(), TestFailure> {
    let messages: Vec<&Message> = census.of(TerrainLayerType::Wind, false).collect();
    for message in &messages {
        check(
            message.batch.patches.len() == 2
                && message
                    .batch
                    .patches
                    .iter()
                    .all(|patch| (patch.patch_x, patch.patch_y) == (0, 0)),
            "a wind LayerData is not two patches at position (0, 0)",
        )?;
        WIND_STRIDE.check("a wind layer's stride", grid, &message.batch.stride)?;
        for patch in &message.batch.patches {
            WIND_PREQUANT.check("a wind patch's prequantization", grid, &patch.prequant)?;
        }
        let datagram = message.datagram.ok_or_else(|| {
            TestFailure::Assertion("a wind LayerData could not be paired with its datagram".into())
        })?;
        WIND_RELIABLE.check(
            "whether the wind is sent reliably",
            grid,
            &datagram.reliable,
        )?;
    }
    let (low, high) = *WIND_INTERVAL.on(grid);
    let interval = census.interval(TerrainLayerType::Wind).ok_or_else(|| {
        TestFailure::Assertion(format!(
            "fewer than two wind messages came in the watch ({SOURCE})"
        ))
    })?;
    check(
        (low..=high).contains(&interval),
        &format!("the wind comes every {interval:.2} s, outside {low}–{high} s ({SOURCE})"),
    )?;
    let ground_done = census
        .of(TerrainLayerType::Land, false)
        .filter_map(Message::offset)
        .fold(None, |last: Option<f64>, offset| {
            Some(last.map_or(offset, |last| last.max(offset)))
        });
    let with_ground = ground_done.is_some_and(|done| {
        messages
            .iter()
            .filter_map(|message| message.offset())
            .any(|offset| (done..=done + WITH_THE_GROUND).contains(&offset))
    });
    WIND_WITH_GROUND.check(
        "whether a wind message comes with the last of the ground",
        grid,
        &with_ground,
    )
}

/// Hold the census to what `grid` was measured sending.
fn check_census(grid: Grid, census: &Census) -> Result<(), TestFailure> {
    // Neither grid sends anything but the ground and the wind.
    let others: Vec<char> = census
        .layers()
        .into_iter()
        .filter(|code| {
            !matches!(
                TerrainLayerType::from_code(*code),
                TerrainLayerType::Land | TerrainLayerType::Wind
            )
        })
        .map(char::from)
        .collect();
    check(
        others.is_empty(),
        &format!("LayerData of a layer neither grid was measured sending came: {others:?}"),
    )?;
    check_land(grid, census)?;
    check_wind(grid, census)
}

/// Takes a census of the `LayerData` an arrival brings and holds it to what
/// each grid was measured sending.
#[derive(Debug)]
pub struct TerrainLayerData;

impl GridTest for TerrainLayerData {
    fn name(&self) -> &'static str {
        "terrain-layerdata"
    }

    fn description(&self) -> &'static str {
        "Take a census of the LayerData an arrival brings: the ground, the wind and the clouds"
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
            let grid = ctx.grid();
            let census = watch(ctx.primary(), watch_for(grid)).await?;
            let identity = census.identity.clone().ok_or_else(|| {
                TestFailure::Assertion("no RegionHandshake came for the root region".to_owned())
            })?;
            record(ctx.metrics(), &census, &identity);
            if grid.is_fake() {
                check_fixture(&census)?;
            }
            check_census(grid, &census)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        PATCH_CELLS, PATCHES_PER_EDGE, REGION_PATCHES, STOCK_HEIGHT_M, centre_distance,
        distribution, patch_of, patch_order_inversions, span,
    };
    use pretty_assertions::assert_eq;

    /// The written-out counts are the squares they claim to be, and the height
    /// this case expects is the height the grid's fixture is flat at. They are
    /// literals because the arithmetic and the conversion are not `const`;
    /// this is what keeps them honest.
    #[expect(
        clippy::float_cmp,
        reason = "both sides are the same small whole number of metres, exactly \
                  representable; the point of the test is that they are the same one"
    )]
    #[test]
    fn the_written_out_constants_match_what_they_stand_for() {
        assert_eq!(
            REGION_PATCHES,
            usize::try_from(PATCHES_PER_EDGE.saturating_mul(PATCHES_PER_EDGE)).unwrap_or(0)
        );
        assert_eq!(PATCH_CELLS, sl_fake_grid::terrain::PATCH_CELLS);
        assert_eq!(
            STOCK_HEIGHT_M,
            f32::from(sl_fake_grid::scenario::STOCK_TERRAIN_HEIGHT_M)
        );
    }

    /// A distribution lists the most frequent value first.
    #[test]
    fn a_distribution_puts_the_commonest_value_first() {
        assert_eq!(distribution([4, 4, 2, 4]), "4:3 2:1");
        assert_eq!(distribution(Vec::<u32>::new()), "");
    }

    /// An order that only ever moves away from a patch has no inversions; one
    /// that comes back towards it has one for each step back.
    #[test]
    fn inversions_count_the_steps_back_towards_the_patch() {
        assert_eq!(
            patch_order_inversions(&[(8, 8), (8, 9), (9, 8), (9, 9), (8, 10)], (8, 8)),
            0
        );
        assert_eq!(
            patch_order_inversions(&[(7, 7), (8, 7), (7, 8), (8, 8)], (8, 8)),
            2
        );
    }

    /// A position is in the patch its metres divide down to, and a patch's
    /// centre is eight metres into it.
    #[test]
    fn a_position_is_in_the_patch_under_it() {
        assert_eq!(patch_of((7.7, 10.1)), (0, 0));
        assert_eq!(patch_of((128.0, 200.0)), (8, 12));
        assert!(centre_distance((0, 0), (8.0, 8.0)) < 0.001);
        assert!((centre_distance((1, 0), (8.0, 8.0)) - 16.0).abs() < 0.001);
    }

    /// A span is the lowest and highest value, and nothing of nothing.
    #[test]
    fn a_span_is_the_extremes() {
        assert_eq!(span([2.0, -1.0, 5.0]), Some((-1.0, 5.0)));
        assert_eq!(span([]), None);
    }
}
