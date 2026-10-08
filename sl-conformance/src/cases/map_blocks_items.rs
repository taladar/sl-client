//! Ask the world map everything a viewer asks it, in every form, and record
//! each answer whole.
//!
//! A viewer draws its world map from four UDP requests, one capability and a
//! tile server, and this case drives all of them from one resident's session
//! while a second resident stands by to be a dot on the map:
//!
//! - **blocks** — `MapBlockRequest` around the agent's own region with each
//!   value of `Flags` a viewer sends (nothing, the layer flag, the
//!   return-null-sims flag), for the agent's own cell alone, for a cell with
//!   no region in it, for a rectangle of such cells, and for a rectangle wide
//!   enough to need more than one reply: which fields a block carries, what an
//!   empty cell is answered with and when, the flags a reply echoes, and how
//!   a long answer is cut into datagrams;
//! - **names** — `MapNameRequest` for the region's exact name, the same in
//!   another case, a prefix, two letters and a name no region has: what
//!   matches, what closes a reply, and which alerts come with it;
//! - **items** — `MapItemRequest` for every item type, once for "the region I
//!   am in" (handle zero) and once naming the region: which types are
//!   answered at all, with how many replies, and what an item of each type
//!   carries. Agent locations are asked for again about a neighbouring region
//!   and about a cell with no region, and again once the second resident has
//!   gone next door: whether the agent is its own dot, what `Extra` counts,
//!   and how soon the map notices somebody leaving;
//! - **layers** — `MapLayerRequest` over UDP and the `MapLayer` capability;
//! - **tiles** — the tile server the login named: the region's own tile at
//!   every zoom, a tile of empty ocean, and the headers they come with.
//!
//! Every request is followed by a fixed listening window rather than a wait
//! for a reply, because no reply is one of the answers.
//!
//! The fake grid's residents are not shown to each other
//! (`server-fake-grid-agent-avatars-shared`), so there the second resident's
//! part is left out.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use sl_client_tokio::{
    CircuitProbe, Command, Event, GridCoordinates, MapBlockBatch, MapBlockKind, MapItem,
    MapItemType, MapLayer, MapRequestFlags, RegionHandle,
};

use crate::circuit::{Seen, seen_since};
use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::metrics::Metrics;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, check, is_fake, settle_scene_with_avatar};
use crate::teleport_trace::{request_teleport, watch_teleport};

/// Where the measured answers are written down.
const SOURCE: &str = "book/src/gridspec/world-map.md (map-blocks-items, 2026-10-08)";

/// How long the case listens after a request about the region it is in.
const WINDOW: Duration = Duration::from_secs(4);

/// How long it listens after a request the simulator has to ask another
/// region or the grid about.
const FAR_WINDOW: Duration = Duration::from_secs(8);

/// How many cells each side of the agent's region the wide rectangle reaches.
const WIDE_MARGIN: u32 = 12;

/// A grid cell no region occupies on any grid the case runs against.
const EMPTY_CELL: (u32, u32) = (100, 100);

/// How long after the second resident leaves the map is asked again, in
/// seconds from its arrival next door.
const DEPARTURE_CHECKS: [u64; 4] = [2, 30, 75, 150];

/// The item types asked for: every one the reference viewer names, and one it
/// does not.
const ITEM_TYPES: [u32; 11] = [1, 2, 3, 4, 5, 6, 8, 9, 11, 7, 10];

/// How long the case listens after asking for land for sale, which Second
/// Life answers for the whole grid, in a few hundred replies.
const LAND_WINDOW: Duration = Duration::from_secs(45);

/// The case's overall budget.
const CASE_TIMEOUT: Duration = Duration::from_secs(25 * 60);

/// The flags a `MapBlockReply` echoes for a request that carried the layer
/// flag.
const LAYER_ECHO: Measured<u32> = Measured {
    second_life: MapRequestFlags::LAYER,
    opensim: MapRequestFlags::LAYER,
    source: SOURCE,
};

/// One answer for both grids.
const fn both<T: Copy>(answer: T) -> Measured<T> {
    Measured {
        second_life: answer,
        opensim: answer,
        source: SOURCE,
    }
}

/// An answer for each grid.
const fn each<T>(second_life: T, opensim: T) -> Measured<T> {
    Measured {
        second_life,
        opensim,
        source: SOURCE,
    }
}

/// Whether an empty cell asked about alone, with the null-sims flag, is
/// reported.
const LONE_EMPTY_CELL_REPORTED: Measured<bool> = both(true);

/// Whether the empty cells of a larger null-sims rectangle are reported.
const EMPTY_CELLS_OF_A_RECTANGLE_REPORTED: Measured<bool> = each(true, false);

/// Whether a rectangle of 25 by 25 cells is answered at all.
const WIDE_RECTANGLE_ANSWERED: Measured<bool> = each(false, true);

/// Whether a block carries a map image id: for a request with no flags, and
/// for one with the layer flag.
const IMAGE_WITHOUT_AND_WITH_THE_LAYER_FLAG: Measured<(bool, bool)> = both((true, false));

/// Whether a name search ends with an entry at cell `(0, 0)`.
const SEARCH_ENDS_WITH_A_TERMINATOR: Measured<bool> = both(true);

/// Whether a search for a region's name less its first letter finds it.
const SEARCH_MATCHES_INSIDE_A_NAME: Measured<bool> = each(false, true);

/// Whether a search for the first two letters of a region's name finds it.
const TWO_LETTER_SEARCH_FINDS: Measured<bool> = each(true, false);

/// How many alerts follow a two-letter search, and a search that matches
/// nothing.
const SEARCH_ALERTS: Measured<(usize, usize)> = each((0, 0), (1, 1));

/// Whether a telehub request naming the agent's own region is answered with
/// that region's agent locations.
const NAMED_REGION_SENDS_AGENTS: Measured<bool> = each(false, true);

/// How many layers a `MapLayerReply` carries, and the flags it echoes for a
/// request with the layer flag.
const LAYERS: Measured<(usize, u32)> = each((0, MapRequestFlags::LAYER), (1, 0));

/// The status a tile of open ocean is answered with.
const EMPTY_TILE_STATUS: Measured<u16> = each(403, 200);

/// Whether a region's tile comes with a `Cache-Control`.
const TILE_STATES_ITS_CACHING: Measured<bool> = each(true, false);

/// How far inside its south-west corner the agent-location item of a region
/// with nobody to show sits, in metres.
const EMPTY_REGION_DOT_M: Measured<f64> = each(0.0, 1.0);

/// Every entry of an answer's block replies.
fn entries(answer: &Answer) -> impl Iterator<Item = &sl_client_tokio::MapBlockRecord> {
    answer.batches.iter().flat_map(|batch| batch.blocks.iter())
}

/// Whether an answer reports a cell as empty.
fn reports_empty(answer: &Answer) -> bool {
    entries(answer).any(|block| block.kind() == MapBlockKind::EmptyCell)
}

/// Whether the entry for `cell` in an answer carries a map image id.
fn has_image(answer: &Answer, cell: GridCoordinates) -> bool {
    entries(answer).any(|block| block.grid_coordinates == cell && !block.map_image_id.is_nil())
}

/// Whether an answer names the region at `cell`.
fn finds(answer: &Answer, cell: GridCoordinates) -> bool {
    entries(answer)
        .any(|block| block.kind() == MapBlockKind::Region && block.grid_coordinates == cell)
}

/// Holds what was recorded to what each grid was measured to answer.
fn hold(grid: Grid, notes: &Notes, place: &Place) -> Result<(), TestFailure> {
    let lone = notes.answer("blocks.empty.null")?;
    LONE_EMPTY_CELL_REPORTED.check(
        "whether a lone empty cell is reported",
        grid,
        &reports_empty(lone),
    )?;
    check(
        entries(lone).all(|block| block.access == 255),
        "a lone empty cell was not marked non-existent (access 255)",
    )?;
    check(
        notes.answer("blocks.empty.layer")?.replies() == 0,
        "an empty cell asked about without the null-sims flag was answered",
    )?;
    check(
        notes.answer("blocks.inverted.layer")?.replies() == 0,
        "a rectangle with its bounds the wrong way round was answered",
    )?;
    EMPTY_CELLS_OF_A_RECTANGLE_REPORTED.check(
        "whether the empty cells of a rectangle are reported",
        grid,
        &reports_empty(notes.answer("blocks.empty-rect.null")?),
    )?;
    WIDE_RECTANGLE_ANSWERED.check(
        "whether a 25 by 25 rectangle is answered",
        grid,
        &(notes.answer("blocks.wide.layer")?.replies() > 0),
    )?;
    IMAGE_WITHOUT_AND_WITH_THE_LAYER_FLAG.check(
        "whether a block carries an image id without and with the layer flag",
        grid,
        &(
            has_image(notes.answer("blocks.around.none")?, place.own),
            has_image(notes.answer("blocks.around.layer")?, place.own),
        ),
    )?;
    check(
        notes
            .answer("blocks.here.null")?
            .batches
            .iter()
            .all(|batch| batch.flags.0 == 0),
        "a reply echoed the null-sims flag",
    )?;

    for label in ["names.exact", "names.lower", "names.upper"] {
        check(
            finds(notes.answer(label)?, place.own),
            "a search for the region's own name in some case did not find it",
        )?;
    }
    for label in [
        "names.exact",
        "names.two",
        "names.tail",
        "names.none",
        "names.prefix",
    ] {
        SEARCH_ENDS_WITH_A_TERMINATOR.check(
            "whether a name search ends with a terminator",
            grid,
            &entries(notes.answer(label)?).any(|block| block.kind() == MapBlockKind::Terminator),
        )?;
    }
    SEARCH_MATCHES_INSIDE_A_NAME.check(
        "whether a search matches inside a name",
        grid,
        &finds(notes.answer("names.tail")?, place.own),
    )?;
    TWO_LETTER_SEARCH_FINDS.check(
        "whether a two-letter search finds the region",
        grid,
        &finds(notes.answer("names.two")?, place.own),
    )?;
    SEARCH_ALERTS.check(
        "the alerts after a two-letter search and after one matching nothing",
        grid,
        &(
            notes.answer("names.two")?.alerts.len(),
            notes.answer("names.none")?.alerts.len(),
        ),
    )?;

    let agents = MapItemType::AgentLocations;
    for label in ["items.type6.zero", "items.type6.own"] {
        check(
            notes
                .answer(label)?
                .items
                .iter()
                .any(|(kind, _, _)| *kind == agents),
            "a request for the region's agent locations was not answered",
        )?;
    }
    check(
        notes.answer("items.type6.nowhere")?.replies() == 0,
        "a request about a cell with no region was answered",
    )?;
    NAMED_REGION_SENDS_AGENTS.check(
        "whether a telehub request naming the region brings its agent locations",
        grid,
        &notes
            .answer("items.type1.own")?
            .items
            .iter()
            .any(|(kind, _, _)| *kind == agents),
    )?;
    if let Some(neighbour) = place.neighbour {
        let origin = (
            f64::from(neighbour.x()) * 256.0_f64,
            f64::from(neighbour.y()) * 256.0_f64,
        );
        // Only a region with nobody in it says where the grid puts the item
        // that counts nobody.
        let nobody = notes
            .answer("items.type6.neighbour")?
            .items
            .iter()
            .filter(|(kind, _, _)| *kind == agents)
            .flat_map(|(_, _, items)| items.iter())
            .find(|item| item.extra == 0);
        if let Some(item) = nobody {
            EMPTY_REGION_DOT_M.check(
                "how far inside the corner an empty region's dot sits, eastwards",
                grid,
                &(item.position.x() - origin.0),
            )?;
            EMPTY_REGION_DOT_M.check(
                "how far inside the corner an empty region's dot sits, northwards",
                grid,
                &(item.position.y() - origin.1),
            )?;
        }
    }

    let layers = notes.answer("layers.udp")?;
    let (flags, found) = layers.layers.first().ok_or_else(|| {
        TestFailure::Assertion("the map layer request was not answered".to_owned())
    })?;
    LAYERS.check(
        "the layers of a layer reply and the flags it echoes",
        grid,
        &(found.len(), flags.0),
    )?;

    for (label, status, caching) in &notes.tiles {
        match label.as_str() {
            "tiles.own.zoom1" => {
                check(*status == 200, "the region's own tile was not served")?;
                TILE_STATES_ITS_CACHING.check(
                    "whether a tile states its caching",
                    grid,
                    caching,
                )?;
            }
            "tiles.empty.zoom1" => {
                EMPTY_TILE_STATUS.check("the status of a tile of open ocean", grid, status)?;
            }
            _ => {}
        }
    }
    Ok(())
}

/// Everything that arrived in the window after one request.
#[derive(Debug, Default, Clone)]
struct Answer {
    /// Each `MapBlockReply`, whole.
    batches: Vec<MapBlockBatch>,
    /// Each `MapItemReply`: the type and flags it echoed, and its items.
    items: Vec<(MapItemType, MapRequestFlags, Vec<MapItem>)>,
    /// Each `MapLayerReply`: its flags and layers.
    layers: Vec<(MapRequestFlags, Vec<MapLayer>)>,
    /// Each alert, as `kind: text [keys]`.
    alerts: Vec<String>,
    /// The map datagrams of the window, as the circuit probe saw them.
    datagrams: Vec<Seen>,
}

impl Answer {
    /// The number of map replies of any kind.
    const fn replies(&self) -> usize {
        self.batches
            .len()
            .saturating_add(self.items.len())
            .saturating_add(self.layers.len())
    }

    /// Seconds from the request to the first map datagram.
    fn first_secs(&self) -> Option<f64> {
        self.datagrams.first().map(|seen| seen.offset)
    }
}

/// Whether the grid is in this process, and answers at once: the windows the
/// case listens for are then cut to [`OFFLINE_WINDOW`], since a wait of
/// seconds for a reply that cannot be late only makes the offline suite slow.
static OFFLINE: AtomicBool = AtomicBool::new(false);

/// How long the case listens after a request to a grid in its own process.
const OFFLINE_WINDOW: Duration = Duration::from_millis(150);

/// What the case writes down: the metrics of the record, and the answers
/// themselves for the checks at the end.
#[derive(Debug, Default)]
struct Notes {
    /// The record's metrics.
    metrics: Metrics,
    /// Each recorded answer by its label.
    answers: BTreeMap<String, Answer>,
    /// Each tile asked for: its label, the status, and whether the answer
    /// came with a `Cache-Control`.
    tiles: Vec<(String, u16, bool)>,
}

impl Notes {
    /// Records a metric.
    fn set(&mut self, key: &str, value: impl Into<crate::record::MetricValue>) {
        self.metrics.set(key, value);
    }

    /// Records a duration.
    fn set_timing(&mut self, key: &str, seconds: f64) {
        self.metrics.set_timing(key, seconds);
    }

    /// The answer recorded under `label`.
    fn answer(&self, label: &str) -> Result<&Answer, TestFailure> {
        self.answers
            .get(label)
            .ok_or_else(|| TestFailure::Assertion(format!("no answer was recorded for {label}")))
    }
}

/// Sends `command` and listens for `window`, collecting every map reply and
/// alert.
async fn ask(
    session: &mut Session,
    command: Command,
    window: Duration,
) -> Result<Answer, TestFailure> {
    let window = if OFFLINE.load(Ordering::Relaxed) {
        OFFLINE_WINDOW.min(window)
    } else {
        window
    };
    let skip = session.diagnostics().len();
    let origin = Instant::now();
    session.send(command).await?;
    let mut answer = Answer::default();
    let outcome = session
        .wait_for(window, |event| {
            match event {
                Event::MapBlockBatch(batch) => answer.batches.push((**batch).clone()),
                Event::MapItems {
                    item_type,
                    flags,
                    items,
                } => answer.items.push((*item_type, *flags, items.clone())),
                Event::MapLayers { flags, layers } => answer.layers.push((*flags, layers.clone())),
                Event::AlertMessage {
                    message,
                    alert_info,
                    ..
                } => {
                    let keys: Vec<&str> = alert_info
                        .iter()
                        .map(|info| info.message.as_str())
                        .collect();
                    answer.alerts.push(format!("alert: {message:?} {keys:?}"));
                }
                Event::AgentAlertMessage { modal, message, .. } => {
                    answer
                        .alerts
                        .push(format!("agent-alert modal={modal}: {message:?}"));
                }
                _ => {}
            }
            None::<()>
        })
        .await;
    match outcome {
        Ok(()) | Err(TestFailure::Timeout(_)) => {}
        Err(other) => return Err(other),
    }
    // The probe's diagnostics come down their own channel; give the last of
    // them a moment to land.
    tokio::time::sleep(OFFLINE_WINDOW.min(Duration::from_millis(200))).await;
    answer.datagrams = seen_since(session, skip, origin)
        .into_iter()
        .filter(|seen| {
            seen.is("MapBlockReply") || seen.is("MapItemReply") || seen.is("MapLayerReply")
        })
        .collect();
    Ok(answer)
}

/// One `MapBlockReply` as text: its flags, then each entry.
fn describe_batch(batch: &MapBlockBatch, own: GridCoordinates) -> String {
    let entries: Vec<String> = batch
        .blocks
        .iter()
        .map(|block| {
            let kind = match block.kind() {
                MapBlockKind::Region => "region",
                MapBlockKind::EmptyCell => "empty",
                MapBlockKind::Terminator => "end",
            };
            // A region is named by where it is relative to the agent's own, so
            // a record says the same thing wherever on the grid it was made.
            let dx = i64::from(block.grid_coordinates.x()).saturating_sub(i64::from(own.x()));
            let dy = i64::from(block.grid_coordinates.y()).saturating_sub(i64::from(own.y()));
            let place = if block.kind() == MapBlockKind::Terminator {
                "0,0".to_owned()
            } else {
                format!("{dx:+},{dy:+}")
            };
            let image = if block.map_image_id.is_nil() {
                "nil"
            } else {
                "set"
            };
            format!(
                "{kind}@{place} name={} access={} flags={:#x} water={} agents={} image={image} \
                 size={:?}",
                if block.name.is_empty() { "-" } else { "set" },
                block.access,
                block.region_flags,
                block.water_height,
                block.agents,
                block.size,
            )
        })
        .collect();
    format!("flags={:#x} [{}]", batch.flags.0, entries.join("; "))
}

/// The datagrams of an answer as text: how long after the request each came,
/// its length and whether it was sent reliably.
fn describe_datagrams(datagrams: &[Seen]) -> String {
    let mut parts: Vec<String> = datagrams
        .iter()
        .take(6)
        .map(|seen| {
            format!(
                "{:.3}s {}B {}",
                seen.offset,
                seen.len,
                if seen.reliable {
                    "reliable"
                } else {
                    "unreliable"
                }
            )
        })
        .collect();
    if let (Some(last), true) = (datagrams.last(), datagrams.len() > 6) {
        parts.push(format!(
            "and {} more, the last at {:.3}s",
            datagrams.len().saturating_sub(6),
            last.offset
        ));
    }
    parts.join(", ")
}

/// The items of one reply as text, each placed relative to `origin_m`, the
/// global position of the south-west corner of the agent's region.
fn describe_items(items: &[MapItem], origin_m: (f64, f64)) -> String {
    items
        .iter()
        .take(6)
        .map(|item| {
            format!(
                "({:+.0},{:+.0}) id={} extra={} extra2={} name={}",
                item.position.x() - origin_m.0,
                item.position.y() - origin_m.1,
                if item.id.is_some() { "set" } else { "nil" },
                item.extra,
                item.extra2,
                name_shape(&item.name),
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// The index of the region a global coordinate lies in.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "a map coordinate is a u32 of metres widened to f64"
)]
fn region_index(metres: f64) -> i64 {
    (metres / 256.0_f64).floor() as i64
}

/// What kind of text a map item's name is, without recording the text: a
/// record must not carry a resident's parcel name.
fn name_shape(name: &str) -> String {
    if name.is_empty() {
        "empty".to_owned()
    } else if name.len() == 32 && name.chars().all(|c| c.is_ascii_hexdigit()) {
        "32-hex".to_owned()
    } else if uuid_like(name) {
        "uuid".to_owned()
    } else if name.chars().all(|c| c.is_ascii_digit()) {
        format!("digits({})", name.len())
    } else {
        format!("text({})", name.chars().count())
    }
}

/// Whether `text` reads as a UUID.
fn uuid_like(text: &str) -> bool {
    text.len() == 36 && text.parse::<sl_client_tokio::Uuid>().is_ok()
}

/// Records an answer under `label`: how many replies, how soon, and each
/// reply's shape.
fn record(
    metrics: &mut Notes,
    label: &str,
    answer: &Answer,
    own: GridCoordinates,
    origin_m: (f64, f64),
) {
    let mut parts: Vec<String> = answer
        .batches
        .iter()
        .map(|batch| format!("blocks {}", describe_batch(batch, own)))
        .collect();
    // A grid may answer one request with hundreds of replies; a record keeps
    // the first few whole and the rest as a count.
    for (item_type, flags, items) in answer.items.iter().take(3) {
        parts.push(format!(
            "items type={} flags={:#x} n={} {}",
            item_type.to_u32(),
            flags.0,
            items.len(),
            describe_items(items, origin_m)
        ));
    }
    if answer.items.len() > 3 {
        parts.push(format!(
            "and {} more replies",
            answer.items.len().saturating_sub(3)
        ));
    }
    if !answer.items.is_empty() {
        let mut types: Vec<u32> = answer
            .items
            .iter()
            .map(|(item_type, _, _)| item_type.to_u32())
            .collect();
        types.sort_unstable();
        types.dedup();
        let total: usize = answer.items.iter().map(|(_, _, items)| items.len()).sum();
        // How many regions the items lie in says whether the answer is about
        // one region or the grid.
        let mut regions: Vec<(i64, i64)> = answer
            .items
            .iter()
            .flat_map(|(_, _, items)| items.iter())
            .map(|item| {
                (
                    region_index(item.position.x()),
                    region_index(item.position.y()),
                )
            })
            .collect();
        regions.sort_unstable();
        regions.dedup();
        let per_reply_most = answer
            .items
            .iter()
            .map(|(_, _, items)| items.len())
            .max()
            .unwrap_or(0);
        metrics.set(
            &format!("{label}.items"),
            format!(
                "types={types:?} replies={} items={total} most-in-one-reply={per_reply_most} regions={}",
                answer.items.len(),
                regions.len()
            ),
        );
    }
    for (flags, layers) in &answer.layers {
        let each: Vec<String> = layers
            .iter()
            .map(|layer| format!(" {:?} image={}", layer.rect, layer.image_id))
            .collect();
        parts.push(format!(
            "layers flags={:#x} n={}{}",
            flags.0,
            layers.len(),
            each.concat()
        ));
    }
    parts.extend(answer.alerts.iter().cloned());
    let shape = if parts.is_empty() {
        "nothing".to_owned()
    } else {
        parts.join(" | ")
    };
    tracing::info!("{label}: {shape}");
    let _previous = metrics.answers.insert(label.to_owned(), answer.clone());
    metrics.set(
        &format!("{label}.replies"),
        i64::try_from(answer.replies()).unwrap_or(-1),
    );
    metrics.set(&format!("{label}.shape"), shape);
    metrics.set(
        &format!("{label}.datagrams"),
        describe_datagrams(&answer.datagrams),
    );
    if let Some(first) = answer.first_secs() {
        metrics.set_timing(&format!("{label}.first_secs"), first);
    }
}

/// A `MapBlockRequest` for the inclusive rectangle of cells `x` by `y`.
const fn blocks(x: (u32, u32), y: (u32, u32), flags: u32) -> Command {
    Command::RequestMapBlocks {
        min_x: x.0,
        max_x: x.1,
        min_y: y.0,
        max_y: y.1,
        flags: MapRequestFlags(flags),
    }
}

/// What the case knows about where it stands.
#[derive(Debug, Clone)]
struct Place {
    /// The agent's region's cell.
    own: GridCoordinates,
    /// The global position of that region's south-west corner, in metres.
    origin_m: (f64, f64),
    /// The region's name.
    name: String,
    /// A region next to it, if the map reported one.
    neighbour: Option<GridCoordinates>,
}

/// The block legs.
async fn block_legs(
    session: &mut Session,
    metrics: &mut Notes,
    grid: Grid,
) -> Result<Place, TestFailure> {
    let region_handle = session
        .region_handle()
        .ok_or_else(|| TestFailure::Assertion("login reported no region handle".to_owned()))?;
    let own = GridCoordinates::from(region_handle);
    let origin_m = (
        f64::from(own.x()) * 256.0_f64,
        f64::from(own.y()) * 256.0_f64,
    );
    let around_x = (own.x().saturating_sub(1), own.x().saturating_add(1));
    let around_y = (own.y().saturating_sub(1), own.y().saturating_add(1));
    let here = (own.x(), own.x());
    let here_y = (own.y(), own.y());
    let null = MapRequestFlags::RETURN_NULL_SIMS;
    let layer = MapRequestFlags::LAYER;

    let around = ask(session, blocks(around_x, around_y, layer), WINDOW).await?;
    record(metrics, "blocks.around.layer", &around, own, origin_m);
    let own_block = around
        .batches
        .iter()
        .flat_map(|batch| batch.blocks.iter())
        .find(|block| block.grid_coordinates == own)
        .cloned();
    check(
        own_block.is_some(),
        "the map block reply did not include the agent's own region",
    )?;
    let name = own_block.map(|block| block.name).unwrap_or_default();
    if let Some(batch) = around.batches.first() {
        LAYER_ECHO.check(
            "the flags a block reply echoes for the layer flag",
            grid,
            &batch.flags.0,
        )?;
    }
    let neighbour = around
        .batches
        .iter()
        .flat_map(|batch| batch.blocks.iter())
        .find(|block| block.kind() == MapBlockKind::Region && block.grid_coordinates != own)
        .map(|block| block.grid_coordinates);

    let legs: [(&str, Command); 9] = [
        ("blocks.around.none", blocks(around_x, around_y, 0)),
        ("blocks.around.null", blocks(around_x, around_y, null)),
        ("blocks.here.null", blocks(here, here_y, null)),
        (
            "blocks.here.null-and-layer",
            blocks(here, here_y, null | layer),
        ),
        (
            "blocks.empty.null",
            blocks(
                (EMPTY_CELL.0, EMPTY_CELL.0),
                (EMPTY_CELL.1, EMPTY_CELL.1),
                null,
            ),
        ),
        (
            "blocks.empty.layer",
            blocks(
                (EMPTY_CELL.0, EMPTY_CELL.0),
                (EMPTY_CELL.1, EMPTY_CELL.1),
                layer,
            ),
        ),
        (
            "blocks.empty-rect.null",
            blocks(
                (EMPTY_CELL.0, EMPTY_CELL.0.saturating_add(1)),
                (EMPTY_CELL.1, EMPTY_CELL.1.saturating_add(1)),
                null,
            ),
        ),
        (
            "blocks.inverted.layer",
            blocks((around_x.1, around_x.0), (around_y.1, around_y.0), layer),
        ),
        (
            "blocks.wide.layer",
            blocks(
                (
                    own.x().saturating_sub(WIDE_MARGIN),
                    own.x().saturating_add(WIDE_MARGIN),
                ),
                (
                    own.y().saturating_sub(WIDE_MARGIN),
                    own.y().saturating_add(WIDE_MARGIN),
                ),
                layer,
            ),
        ),
    ];
    for (label, command) in legs {
        let answer = ask(session, command, FAR_WINDOW).await?;
        record(metrics, label, &answer, own, origin_m);
        if label == "blocks.wide.layer" {
            let per_reply: Vec<String> = answer
                .batches
                .iter()
                .map(|batch| batch.blocks.len().to_string())
                .collect();
            metrics.set("blocks.wide.per_reply", per_reply.join(","));
            // The shape of several hundred regions says nothing the smaller
            // rectangles did not; the count and the cut are the answer.
            metrics.set(
                "blocks.wide.layer.shape",
                format!(
                    "{} regions",
                    answer
                        .batches
                        .iter()
                        .map(|batch| batch.blocks.len())
                        .sum::<usize>()
                ),
            );
        }
    }
    // How large a rectangle is answered at all: squares and strips either
    // side of the sixty-four regions the reference viewer keeps to.
    for (wide, high) in [
        (8_u32, 8_u32),
        (9, 8),
        (65, 1),
        (16, 16),
        (18, 18),
        (20, 20),
        (22, 22),
        (24, 24),
        (400, 1),
        (1, 400),
    ] {
        let x = (
            own.x().saturating_sub(wide.checked_div(2).unwrap_or(0)),
            own.x()
                .saturating_sub(wide.checked_div(2).unwrap_or(0))
                .saturating_add(wide.saturating_sub(1)),
        );
        let y = (
            own.y().saturating_sub(high.checked_div(2).unwrap_or(0)),
            own.y()
                .saturating_sub(high.checked_div(2).unwrap_or(0))
                .saturating_add(high.saturating_sub(1)),
        );
        let answer = ask(session, blocks(x, y, layer), WINDOW).await?;
        let per_reply: Vec<String> = answer
            .batches
            .iter()
            .map(|batch| batch.blocks.len().to_string())
            .collect();
        let text = format!(
            "replies={} blocks-per-reply=[{}] {}",
            answer.batches.len(),
            per_reply.join(","),
            describe_datagrams(&answer.datagrams)
        );
        tracing::info!("blocks.size.{wide}x{high}: {text}");
        metrics.set(&format!("blocks.size.{wide}x{high}"), text);
    }
    Ok(Place {
        own,
        origin_m,
        name,
        neighbour,
    })
}

/// The name-search legs.
async fn name_legs(
    session: &mut Session,
    metrics: &mut Notes,
    place: &Place,
) -> Result<(), TestFailure> {
    let prefix: String = place.name.chars().take(3).collect();
    let two: String = place.name.chars().take(2).collect();
    let tail: String = place.name.chars().skip(1).collect();
    let legs: [(&str, String); 7] = [
        ("names.exact", place.name.clone()),
        ("names.lower", place.name.to_lowercase()),
        ("names.upper", place.name.to_uppercase()),
        ("names.prefix", prefix),
        ("names.two", two),
        ("names.tail", tail),
        ("names.none", "Zzqxj Nowhere Qq".to_owned()),
    ];
    for (label, name) in legs {
        let answer = ask(session, Command::RequestMapByName { name }, FAR_WINDOW).await?;
        let found_own = answer
            .batches
            .iter()
            .flat_map(|batch| batch.blocks.iter())
            .any(|block| block.grid_coordinates == place.own);
        metrics.set(&format!("{label}.found_own"), found_own);
        let terminators: Vec<String> = answer
            .batches
            .iter()
            .flat_map(|batch| batch.blocks.iter())
            .filter(|block| block.kind() == MapBlockKind::Terminator)
            .map(|block| {
                format!(
                    "access={} name={}",
                    block.access,
                    if block.name.is_empty() {
                        "empty"
                    } else {
                        "the search text"
                    }
                )
            })
            .collect();
        metrics.set(&format!("{label}.terminators"), terminators.join("; "));
        let counts: Vec<String> = answer
            .batches
            .iter()
            .map(|batch| batch.blocks.len().to_string())
            .collect();
        metrics.set(&format!("{label}.per_reply"), counts.join(","));
        // A prefix search on a real grid names other people's regions; the
        // record keeps the counts and the alerts, and the shape only where it
        // is the agent's own region or nothing.
        if matches!(
            label,
            "names.exact" | "names.lower" | "names.upper" | "names.none" | "names.two"
        ) {
            record(metrics, label, &answer, place.own, place.origin_m);
        } else {
            let _previous = metrics.answers.insert(label.to_owned(), answer.clone());
            metrics.set(
                &format!("{label}.replies"),
                i64::try_from(answer.replies()).unwrap_or(-1),
            );
            metrics.set(&format!("{label}.alerts"), answer.alerts.join(" | "));
            metrics.set(
                &format!("{label}.datagrams"),
                describe_datagrams(&answer.datagrams),
            );
        }
    }
    Ok(())
}

/// The item legs that need nobody else: every type, about "here" and about
/// the region by name, and agent locations about a neighbour and about
/// nowhere.
async fn item_legs(
    session: &mut Session,
    metrics: &mut Notes,
    place: &Place,
) -> Result<(), TestFailure> {
    let own_handle = RegionHandle::from(place.own);
    for code in ITEM_TYPES {
        let item_type = MapItemType::from_u32(code);
        for (suffix, region_handle) in [("zero", RegionHandle(0)), ("own", own_handle)] {
            let answer = ask(
                session,
                Command::RequestMapItems {
                    item_type,
                    region_handle,
                },
                if matches!(code, 7 | 10) {
                    LAND_WINDOW
                } else {
                    WINDOW
                },
            )
            .await?;
            record(
                metrics,
                &format!("items.type{code}.{suffix}"),
                &answer,
                place.own,
                place.origin_m,
            );
        }
    }
    if let Some(neighbour) = place.neighbour {
        for (code, window) in [(6_u32, FAR_WINDOW), (1, FAR_WINDOW), (7, LAND_WINDOW)] {
            let answer = ask(
                session,
                Command::RequestMapItems {
                    item_type: MapItemType::from_u32(code),
                    region_handle: RegionHandle::from(neighbour),
                },
                window,
            )
            .await?;
            record(
                metrics,
                &format!("items.type{code}.neighbour"),
                &answer,
                neighbour,
                (
                    f64::from(neighbour.x()) * 256.0_f64,
                    f64::from(neighbour.y()) * 256.0_f64,
                ),
            );
        }
    }
    let nowhere = GridCoordinates::new(EMPTY_CELL.0, EMPTY_CELL.1);
    let answer = ask(
        session,
        Command::RequestMapItems {
            item_type: MapItemType::AgentLocations,
            region_handle: RegionHandle::from(nowhere),
        },
        FAR_WINDOW,
    )
    .await?;
    record(
        metrics,
        "items.type6.nowhere",
        &answer,
        place.own,
        place.origin_m,
    );
    Ok(())
}

/// The layer legs: the UDP request and the capability.
async fn layer_legs(
    session: &mut Session,
    metrics: &mut Notes,
    place: &Place,
) -> Result<(), TestFailure> {
    let answer = ask(session, Command::RequestMapLayer, WINDOW).await?;
    record(metrics, "layers.udp", &answer, place.own, place.origin_m);
    let Some(cap) = session.cap("MapLayer") else {
        metrics.set("layers.cap", "not granted");
        return Ok(());
    };
    let client = http_client()?;
    let body = "<llsd><map><key>Flags</key><integer>2</integer></map></llsd>";
    let reply = client
        .post(&cap)
        .header("content-type", "application/llsd+xml")
        .body(body)
        .send()
        .await;
    let text = match reply {
        Ok(response) => {
            let status = response.status().as_u16();
            let kind = header(&response, "content-type");
            let body = response.text().await.unwrap_or_default();
            format!(
                "{status} {kind} {}",
                body.chars().take(600).collect::<String>()
            )
        }
        Err(error) => format!("failed: {error}"),
    };
    tracing::info!("layers.cap: {text}");
    metrics.set("layers.cap", text);
    Ok(())
}

/// An HTTP client for the tile server and the layer capability.
fn http_client() -> Result<reqwest::Client, TestFailure> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|error| TestFailure::Assertion(format!("building an HTTP client failed: {error}")))
}

/// One response header as text, `-` when absent.
fn header(response: &reqwest::Response, name: &str) -> String {
    response
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("-")
        .to_owned()
}

/// The tile legs: the login's tile server, asked for the region's tile at
/// each zoom and for a tile of open ocean.
async fn tile_legs(
    session: &Session,
    metrics: &mut Notes,
    place: &Place,
) -> Result<(), TestFailure> {
    let base = session
        .login_success()
        .and_then(|success| success.map_server_url.clone());
    metrics.set(
        "tiles.login_map_server_url",
        base.as_ref()
            .map_or_else(|| "absent".to_owned(), host_shape),
    );
    let Some(base) = base else {
        return Ok(());
    };
    let client = http_client()?;
    let mut asks: Vec<(String, u8, u32, u32)> = Vec::new();
    for zoom in 1_u8..=9 {
        // A tile at zoom `n` covers 2^(n-1) regions a side and is named by
        // its south-west region, which is a multiple of that width.
        let width = 1_u32
            .checked_shl(u32::from(zoom.saturating_sub(1)))
            .unwrap_or(1);
        let x = place
            .own
            .x()
            .checked_div(width)
            .unwrap_or(0)
            .saturating_mul(width);
        let y = place
            .own
            .y()
            .checked_div(width)
            .unwrap_or(0)
            .saturating_mul(width);
        asks.push((format!("tiles.own.zoom{zoom}"), zoom, x, y));
    }
    asks.push((
        "tiles.empty.zoom1".to_owned(),
        1,
        EMPTY_CELL.0,
        EMPTY_CELL.1,
    ));
    // A zoom-2 tile named by a region that is not its corner.
    asks.push((
        "tiles.misaligned.zoom2".to_owned(),
        2,
        place.own.x() | 1,
        place.own.y() | 1,
    ));
    for (label, zoom, x, y) in asks {
        let url = format!(
            "{}map-{zoom}-{x}-{y}-objects.jpg",
            with_trailing_slash(base.as_str())
        );
        let started = Instant::now();
        let text = match client.get(&url).send().await {
            Ok(response) => {
                let status = response.status().as_u16();
                let kind = header(&response, "content-type");
                let cache = header(&response, "cache-control");
                metrics.tiles.push((label.clone(), status, cache != "-"));
                let etag = if response.headers().contains_key("etag") {
                    "etag"
                } else {
                    "no-etag"
                };
                let modified = if response.headers().contains_key("last-modified") {
                    "last-modified"
                } else {
                    "no-last-modified"
                };
                let server = header(&response, "server");
                let body = response.bytes().await.unwrap_or_default();
                let bytes = body.len();
                let size = jpeg_size(&body)
                    .map_or_else(|| "not a JPEG".to_owned(), |(w, h)| format!("{w}x{h}"));
                format!(
                    "{status} {kind} {bytes}B {size} cache-control={cache:?} {etag} {modified} server={server:?}"
                )
            }
            Err(error) => format!("failed: {error}"),
        };
        tracing::info!("{label}: {text}");
        metrics.set(&label, text);
        metrics.set_timing(&format!("{label}.secs"), started.elapsed().as_secs_f64());
    }
    Ok(())
}

/// `base` ending in exactly one slash.
fn with_trailing_slash(base: &str) -> String {
    format!("{}/", base.trim_end_matches('/'))
}

/// A URL's scheme, host and path, which is what a record may say about a
/// server.
fn host_shape(url: &url::Url) -> String {
    let port = url
        .port()
        .map_or_else(String::new, |port| format!(":{port}"));
    format!(
        "{}://{}{port}{}",
        url.scheme(),
        url.host_str().unwrap_or("-"),
        url.path()
    )
}

/// The sixteen-bit number whose bytes are `high` then `low`, as a JPEG
/// writes its lengths and sizes.
fn big_endian(high: u8, low: u8) -> u16 {
    (u16::from(high) << 8_u32) | u16::from(low)
}

/// The width and height a JPEG states for itself in its frame header, or
/// `None` for bytes that are not a JPEG with one.
fn jpeg_size(bytes: &[u8]) -> Option<(u16, u16)> {
    let mut rest = bytes.strip_prefix(&[0xFF, 0xD8])?;
    loop {
        let (marker, after) = match rest {
            [0xFF, marker, after @ ..] => (*marker, after),
            _ => return None,
        };
        let (length, _) = after.split_first_chunk::<2>()?;
        let [length_high, length_low] = *length;
        let length = usize::from(big_endian(length_high, length_low));
        // The start-of-frame markers, less the three in that range that are
        // tables rather than frames.
        if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
            let (frame, _) = after.get(3..)?.split_first_chunk::<4>()?;
            let [height_high, height_low, width_high, width_low] = *frame;
            return Some((
                big_endian(width_high, width_low),
                big_endian(height_high, height_low),
            ));
        }
        rest = after.get(length..)?;
    }
}

/// The legs that take a second resident: agent locations with both in the
/// region, and again after the second has gone next door.
async fn company_legs(
    ctx: &mut TestContext,
    metrics: &mut Notes,
    place: &Place,
) -> Result<(), TestFailure> {
    let own_handle = RegionHandle::from(place.own);
    let (primary, secondary) = ctx.primary_and_secondary().ok_or_else(|| {
        TestFailure::Assertion("this case needs a second avatar (--secondary)".to_owned())
    })?;
    secondary.wait_for_region(REGION_TIMEOUT).await?;
    let together = secondary.region_handle() == Some(own_handle);
    metrics.set("company.together", together);
    for (suffix, region_handle) in [("zero", RegionHandle(0)), ("own", own_handle)] {
        let answer = ask(
            primary,
            Command::RequestMapItems {
                item_type: MapItemType::AgentLocations,
                region_handle,
            },
            WINDOW,
        )
        .await?;
        record(
            metrics,
            &format!("company.together.type6.{suffix}"),
            &answer,
            place.own,
            place.origin_m,
        );
    }
    let here = (place.own.x(), place.own.x());
    let here_y = (place.own.y(), place.own.y());
    let answer = ask(
        primary,
        blocks(here, here_y, MapRequestFlags::LAYER),
        WINDOW,
    )
    .await?;
    record(
        metrics,
        "company.together.block",
        &answer,
        place.own,
        place.origin_m,
    );
    let Some(neighbour) = place.neighbour else {
        metrics.set("company.apart", "no neighbouring region to go to");
        return Ok(());
    };
    request_teleport(
        secondary,
        RegionHandle::from(neighbour),
        (128.0, 128.0, 30.0),
        (1.0, 0.0, 0.0),
    )
    .await?;
    let trace = watch_teleport(secondary, Duration::from_secs(60)).await?;
    if trace.arrival.is_none() {
        metrics.set("company.apart", "the second resident's teleport failed");
        return Ok(());
    }
    let left = Instant::now();
    let neighbour_origin = (
        f64::from(neighbour.x()) * 256.0_f64,
        f64::from(neighbour.y()) * 256.0_f64,
    );
    for after in DEPARTURE_CHECKS {
        let due = Duration::from_secs(after);
        if let Some(wait) = due.checked_sub(left.elapsed()) {
            // Both sessions have to be drained while the case waits, or the
            // one not being listened to backs up.
            ask_nothing(secondary, wait).await?;
        }
        for (suffix, region_handle, origin_m, cell) in [
            ("zero", RegionHandle(0), place.origin_m, place.own),
            ("own", own_handle, place.origin_m, place.own),
            (
                "neighbour",
                RegionHandle::from(neighbour),
                neighbour_origin,
                neighbour,
            ),
        ] {
            let answer = ask(
                primary,
                Command::RequestMapItems {
                    item_type: MapItemType::AgentLocations,
                    region_handle,
                },
                WINDOW,
            )
            .await?;
            record(
                metrics,
                &format!("company.apart.{after}s.type6.{suffix}"),
                &answer,
                cell,
                origin_m,
            );
        }
    }
    // The second resident, asking about the region it left.
    let answer = ask(
        secondary,
        Command::RequestMapItems {
            item_type: MapItemType::AgentLocations,
            region_handle: own_handle,
        },
        FAR_WINDOW,
    )
    .await?;
    record(
        metrics,
        "company.apart.from-next-door.type6",
        &answer,
        place.own,
        place.origin_m,
    );
    Ok(())
}

/// Listens on `session` for `window` and keeps nothing.
async fn ask_nothing(session: &mut Session, window: Duration) -> Result<(), TestFailure> {
    match session.wait_for(window, |_event| None::<()>).await {
        Ok(()) | Err(TestFailure::Timeout(_)) => Ok(()),
        Err(other) => Err(other),
    }
}

/// Asks the world map everything a viewer asks it and records each answer.
#[derive(Debug)]
pub struct MapBlocksItems;

impl GridTest for MapBlocksItems {
    fn name(&self) -> &'static str {
        "map-blocks-items"
    }

    fn description(&self) -> &'static str {
        "Ask the world map for blocks, names, items, layers and tiles in every form"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi, Grid::FakeSl, Grid::FakeOpensim]
    }

    fn accounts(&self) -> u8 {
        2
    }

    fn start_location(&self, grid: Grid) -> &'static str {
        match grid {
            Grid::Aditi => super::teleport_cross_region::ADITI_START,
            Grid::Opensim => "uri:Default Region&128&128&30",
            Grid::FakeSl | Grid::FakeOpensim => "last",
        }
    }

    fn timeout(&self) -> Duration {
        CASE_TIMEOUT
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();
            let mut metrics = Notes::default();
            if is_fake(grid) {
                OFFLINE.store(true, Ordering::Relaxed);
            }
            let place = {
                let session = ctx.primary();
                session.wait_for_region(REGION_TIMEOUT).await?;
                session
                    .send(Command::ProbeCircuits(CircuitProbe::Observe))
                    .await?;
                // Where the agent stands, so that a dot on the map can be told
                // from a count stated at a region's corner.
                let settled = settle_scene_with_avatar(
                    session,
                    grid,
                    None,
                    Duration::from_secs(15),
                    Duration::from_secs(2),
                )
                .await?;
                metrics.set(
                    "own_position",
                    settled.avatar.map_or_else(
                        || "never seen".to_owned(),
                        |at| format!("({:.0},{:.0})", at.x, at.y),
                    ),
                );
                let place = block_legs(session, &mut metrics, grid).await?;
                name_legs(session, &mut metrics, &place).await?;
                item_legs(session, &mut metrics, &place).await?;
                layer_legs(session, &mut metrics, &place).await?;
                tile_legs(session, &mut metrics, &place).await?;
                place
            };
            if !is_fake(grid) {
                company_legs(ctx, &mut metrics, &place).await?;
            }
            ctx.primary()
                .send(Command::ProbeCircuits(CircuitProbe::Off))
                .await?;
            metrics.set("grid_x", i64::from(place.own.x()));
            metrics.set("grid_y", i64::from(place.own.y()));
            let held = hold(grid, &metrics, &place);
            ctx.metrics().merge(metrics.metrics);
            held?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;

    use super::{jpeg_size, name_shape, with_trailing_slash};

    /// A JPEG's size is read from its frame header, past the segments before
    /// it, and bytes that are no JPEG have none.
    #[test]
    fn a_jpeg_states_its_size() {
        let jpeg = [
            0xFF, 0xD8, // start of image
            0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00, // an application segment
            0xFF, 0xC0, 0x00, 0x0B, 0x08, 0x01, 0x00, 0x02, 0x00, 0x03, // frame
        ];
        assert_eq!(jpeg_size(&jpeg), Some((512, 256)));
        assert_eq!(jpeg_size(b"<html>"), None);
        assert_eq!(jpeg_size(&[0xFF, 0xD8, 0xFF]), None);
    }

    /// A name is recorded by its kind, never its text.
    #[test]
    fn a_name_is_recorded_by_kind() {
        assert_eq!(name_shape(""), "empty");
        assert_eq!(name_shape("0123456789abcdef0123456789ABCDEF"), "32-hex");
        assert_eq!(name_shape("11111111-2222-3333-4444-555555555555"), "uuid");
        assert_eq!(name_shape("A parcel"), "text(8)");
        assert_eq!(name_shape("1234567"), "digits(7)");
    }

    /// A tile URL is built on a base with exactly one slash.
    #[test]
    fn a_base_url_ends_in_one_slash() {
        assert_eq!(with_trailing_slash("http://a/"), "http://a/");
        assert_eq!(with_trailing_slash("http://a"), "http://a/");
        assert_eq!(with_trailing_slash("http://a/map//"), "http://a/map/");
    }
}
