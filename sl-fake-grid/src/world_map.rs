//! The world map: what the grid answers a viewer's `MapBlockRequest`,
//! `MapNameRequest`, `MapItemRequest` and `MapLayerRequest` with.
//!
//! The map is the one surface that is *not* about the region the agent is
//! standing in: a viewer opening its world map asks its current simulator about
//! every region on the grid, which is why the catalogue is built once from the
//! whole region table ([`catalogue`]) and handed to every session. A grid that
//! cannot answer this has no world map at all — no region under the cursor, no
//! green dots, and no way for a client to find the name of anywhere to teleport
//! to.
//!
//! The two live grids answer all four requests, and not alike
//! ([`MapPolicy`], `book/src/gridspec/world-map.md`): which empty cells are
//! reported, how large a rectangle may be, what a search matches and what it
//! says when it matches nothing, and whether there are map layers at all.
//!
//! What this grid has nothing to say about is left unanswered, as both live
//! grids leave it: it has no telehubs, no land for sale and no events, and a
//! request for those items gets no reply. And its residents are not shown to
//! each other (`server-fake-grid-agent-avatars-shared`), so a region's agent
//! locations are always the one item both grids send for a region with
//! nobody to show.
//!
//! [`crate::map_tiles`] is the other half, and a different protocol: the JPEG
//! tiles a modern viewer fetches over HTTP. This module is the legacy UDP
//! catalogue those tiles are drawn under.

use std::time::Instant;

use sl_proto::{
    MapBlockBatch, MapBlockRecord, MapItem, MapItemType, MapLayer, MapRegionInfo, MapRequestFlags,
    RegionHandle, ServerEvent, SimSession,
};
use sl_types::key::TextureKey;
use sl_types::map::{GlobalCoordinates, GridCoordinates, GridRectangle};

use crate::imitates::{DotName, EmptyCells, MapLayerAnswer, MapPolicy, NameMatch};
use crate::runtime::RegionEntry;

/// The width and height, in metres, of every region a fake grid serves.
///
/// The fake grid has no variable-sized regions: a `RegionConfig` names a grid
/// index and nothing else, so every entry reports the standard 256.
const REGION_SIZE_M: u32 = 256;

/// The bits of a request's flags a reply echoes: both grids drop the upper
/// sixteen, the null-sims flag among them.
const ECHOED_FLAGS: u32 = 0xFFFF;

/// The map catalogue for the whole grid: one [`MapRegionInfo`] per configured
/// region, in builder order.
///
/// The map image id is the **region id**, which is what OpenSim reports when a
/// region has no separately-uploaded map asset. It is a real id a client can
/// carry around and compare; it is not a texture this grid serves, because the
/// tiles go out over HTTP ([`crate::map_tiles`]) as they do on every modern
/// grid.
pub(crate) fn catalogue(regions: &[RegionEntry]) -> Vec<MapRegionInfo> {
    regions.iter().map(block_for).collect()
}

/// One region's map block.
fn block_for(entry: &RegionEntry) -> MapRegionInfo {
    let grid_coordinates = GridCoordinates::new(entry.config.grid_x, entry.config.grid_y);
    MapRegionInfo {
        name: sl_proto::region_name_from_wire("fake-grid", &entry.config.name)
            .ok()
            .flatten(),
        grid_coordinates,
        region_handle: entry.handle(),
        maturity: entry.config.maturity,
        region_flags: 0,
        size_x: REGION_SIZE_M,
        size_y: REGION_SIZE_M,
        agents: 0,
        water_height: crate::terrain::round_to_u8(entry.config.water_height),
        map_image_id: TextureKey::from(entry.region_id),
    }
}

/// Answers one world-map request from `map`, the grid's whole region
/// catalogue, as `policy`'s grid does. `here` is the region handle of the
/// session answering.
pub(crate) fn answer_map_request(
    policy: &MapPolicy,
    map: &[MapRegionInfo],
    here: RegionHandle,
    sim: &mut SimSession,
    event: &ServerEvent,
    now: Instant,
) {
    match event {
        ServerEvent::MapBlockRequested {
            min_x,
            max_x,
            min_y,
            max_y,
            flags,
        } => {
            for batch in block_answer(policy, map, (*min_x, *max_x), (*min_y, *max_y), *flags) {
                if let Err(error) = sim.send_map_block_batch(&batch, now) {
                    tracing::warn!("answering a map block request failed: {error}");
                }
            }
        }
        ServerEvent::MapNameRequested { name, flags } => {
            let (batch, alert) = name_answer(policy, map, name, *flags);
            if let Err(error) = sim.send_map_block_batch(&batch, now) {
                tracing::warn!("answering a map name request failed: {error}");
            }
            let sent = match alert {
                Some(SearchAlert::TooShort(text)) => sim.send_alert_message(text, &[], &[], now),
                Some(SearchAlert::NoMatch(text)) => match sim.agent_id() {
                    Some(agent) => sim.send_agent_alert_message(agent, true, text, now),
                    None => Ok(()),
                },
                None => Ok(()),
            };
            if let Err(error) = sent {
                tracing::warn!("alerting about a map name request failed: {error}");
            }
        }
        ServerEvent::MapItemRequested {
            item_type,
            region_handle,
            flags,
        } => {
            for (kind, items) in item_answer(policy, map, here, *item_type, *region_handle) {
                if let Err(error) = sim.send_map_item_reply(*flags, kind, &items, now) {
                    tracing::warn!("answering a map item request failed: {error}");
                }
            }
        }
        ServerEvent::MapLayerRequested { flags } => {
            let (echoed, layers) = layer_answer(policy, *flags);
            if let Err(error) = sim.send_map_layer_reply(echoed, &layers, now) {
                tracing::warn!("answering a map layer request failed: {error}");
            }
        }
        _other => {}
    }
}

/// The replies to a `MapBlockRequest` for the inclusive rectangle `x` by `y`:
/// none at all for a rectangle the grid does not answer, otherwise the
/// regions in it and whichever empty cells the grid reports, cut into
/// replies of the size the grid sends.
fn block_answer(
    policy: &MapPolicy,
    map: &[MapRegionInfo],
    x: (u16, u16),
    y: (u16, u16),
    flags: MapRequestFlags,
) -> Vec<MapBlockBatch> {
    // A rectangle whose bounds are the wrong way round holds nothing on
    // either grid.
    if x.0 > x.1 || y.0 > y.1 {
        return Vec::new();
    }
    let wide = u32::from(x.1.saturating_sub(x.0)).saturating_add(1);
    let high = u32::from(y.1.saturating_sub(y.0)).saturating_add(1);
    let cells = wide.saturating_mul(high);
    if policy
        .largest_block_request
        .is_some_and(|largest| cells > largest)
    {
        return Vec::new();
    }
    let echoed = MapRequestFlags(flags.0 & ECHOED_FLAGS);
    let regions: Vec<&MapRegionInfo> = map.iter().filter(|block| within(block, x, y)).collect();
    let mut blocks: Vec<MapBlockRecord> = regions
        .iter()
        .map(|block| region_record(block, echoed))
        .collect();
    if flags.contains(MapRequestFlags::RETURN_NULL_SIMS) {
        match policy.empty_cells {
            EmptyCells::Every => {
                for cell_x in x.0..=x.1 {
                    for cell_y in y.0..=y.1 {
                        let cell = GridCoordinates::new(u32::from(cell_x), u32::from(cell_y));
                        if !regions.iter().any(|block| block.grid_coordinates == cell) {
                            blocks.push(empty_record(cell));
                        }
                    }
                }
            }
            EmptyCells::LoneCell => {
                if regions.is_empty() && cells == 1 {
                    blocks.push(empty_record(GridCoordinates::new(
                        u32::from(x.0),
                        u32::from(y.0),
                    )));
                }
            }
        }
    }
    blocks
        .chunks(policy.blocks_per_reply.max(1))
        .map(|chunk| MapBlockBatch {
            flags: echoed,
            blocks: chunk.to_vec(),
        })
        .collect()
}

/// A region's entry in a reply echoing `echoed`.
///
/// Both grids send the region's name, cell and rating and nothing else that
/// is true of it: the water height, the agent count and the region flags are
/// zero, and the map image id is there only for a request with no flags —
/// the reference viewer always sets the layer flag, and so never sees one.
fn region_record(block: &MapRegionInfo, echoed: MapRequestFlags) -> MapBlockRecord {
    MapBlockRecord {
        grid_coordinates: block.grid_coordinates,
        name: block
            .name
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default(),
        access: block.maturity.to_sim_access(),
        region_flags: 0,
        water_height: 0,
        agents: 0,
        map_image_id: if echoed.0 == 0 {
            block.map_image_id.uuid()
        } else {
            uuid::Uuid::nil()
        },
        size: None,
    }
}

/// The entry for a cell with no region in it. Second Life leaves the fields
/// after the rating uninitialised; this sends zeros, as OpenSim does.
const fn empty_record(cell: GridCoordinates) -> MapBlockRecord {
    MapBlockRecord {
        grid_coordinates: cell,
        name: String::new(),
        access: MapPolicy::NON_EXISTENT_ACCESS,
        region_flags: 0,
        water_height: 0,
        agents: 0,
        map_image_id: uuid::Uuid::nil(),
        size: None,
    }
}

/// The entry that ends a name search on both grids: cell `(0, 0)`, marked
/// non-existent, carrying the text that was searched for.
fn terminator(searched: &str) -> MapBlockRecord {
    MapBlockRecord {
        name: searched.to_owned(),
        ..empty_record(GridCoordinates::new(0, 0))
    }
}

/// What a search that found nothing to list is told beside its reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchAlert {
    /// The search text was too short to run: an `AlertMessage`.
    TooShort(&'static str),
    /// Nothing matched: a modal `AgentAlertMessage`.
    NoMatch(&'static str),
}

/// The reply to a `MapNameRequest` for `name`, and the alert that goes with
/// it on a grid that sends one.
fn name_answer(
    policy: &MapPolicy,
    map: &[MapRegionInfo],
    name: &str,
    flags: MapRequestFlags,
) -> (MapBlockBatch, Option<SearchAlert>) {
    let echoed = MapRequestFlags(flags.0 & ECHOED_FLAGS);
    if name.chars().count() < policy.shortest_search {
        return (
            MapBlockBatch {
                flags: echoed,
                blocks: vec![terminator(name)],
            },
            policy.short_search_alert.map(SearchAlert::TooShort),
        );
    }
    let limit = match policy.name_match {
        NameMatch::Prefix => usize::MAX,
        NameMatch::Anywhere => MapPolicy::OPENSIM_SEARCH_LIMIT,
    };
    let mut blocks: Vec<MapBlockRecord> = map
        .iter()
        .filter(|block| named(policy.name_match, block, name))
        .take(limit)
        .map(|block| region_record(block, echoed))
        .collect();
    // OpenSim alerts only a search carrying the layer flag and nothing else,
    // which is every search a viewer sends.
    let alert = (blocks.is_empty() && flags.0 == MapRequestFlags::LAYER)
        .then_some(policy.no_match_alert)
        .flatten()
        .map(SearchAlert::NoMatch);
    blocks.push(terminator(name));
    (
        MapBlockBatch {
            flags: echoed,
            blocks,
        },
        alert,
    )
}

/// The replies to a `MapItemRequest`: each a type and its items.
///
/// Only agent locations are ever answered, since this grid has no other
/// item, and with the one item both grids send for a region with nobody to
/// show. A region the grid does not have is not answered about on either.
fn item_answer(
    policy: &MapPolicy,
    map: &[MapRegionInfo],
    here: RegionHandle,
    item_type: MapItemType,
    region_handle: RegionHandle,
) -> Vec<(MapItemType, Vec<MapItem>)> {
    // A zero handle means "the region I am in".
    let named = region_handle.0 != 0;
    let target = if named { region_handle } else { here };
    if !map.iter().any(|block| block.region_handle == target) {
        return Vec::new();
    }
    let agents = matches!(item_type, MapItemType::AgentLocations)
        || (policy.named_region_sends_agents && named && target == here);
    if !agents {
        return Vec::new();
    }
    vec![(
        MapItemType::AgentLocations,
        vec![empty_region_dot(policy, target)],
    )]
}

/// The agent-location item of a region with nobody to show: an `Extra` of
/// zero, at or beside the region's south-west corner.
fn empty_region_dot(policy: &MapPolicy, region: RegionHandle) -> MapItem {
    let inset = f32::from(u8::try_from(policy.empty_region_dot_m).unwrap_or(u8::MAX));
    MapItem {
        position: GlobalCoordinates::from_grid_and_region(
            GridCoordinates::from(region),
            sl_types::map::RegionCoordinates::new(inset, inset, 0.0),
        ),
        id: None,
        extra: 0,
        extra2: 0,
        name: match policy.agent_dot_name {
            DotName::Uuid => uuid::Uuid::from_u128(u128::from(region.0)).to_string(),
            DotName::Hash => format!("{:032x}", region.0),
        },
    }
}

/// The reply to a `MapLayerRequest`: the flags it carries and its layers.
fn layer_answer(policy: &MapPolicy, flags: MapRequestFlags) -> (MapRequestFlags, Vec<MapLayer>) {
    match policy.layers {
        MapLayerAnswer::None => (flags, Vec::new()),
        MapLayerAnswer::WholeGrid => (
            MapRequestFlags(0),
            vec![MapLayer {
                rect: GridRectangle::new(
                    GridCoordinates::new(0, 0),
                    GridCoordinates::new(
                        MapPolicy::OPENSIM_LAYER_EXTENT,
                        MapPolicy::OPENSIM_LAYER_EXTENT,
                    ),
                ),
                image_id: TextureKey::from(MapPolicy::OPENSIM_LAYER_IMAGE),
            }],
        ),
    }
}

/// Whether `block` sits inside the inclusive grid rectangle a `MapBlockRequest`
/// names.
///
/// The request's bounds are `u16` — the wire field — while a grid coordinate is
/// a `u32`, so a region beyond the sixteen-bit grid can never be inside any
/// rectangle a client can ask about, and saturating it to `u16::MAX` says so.
fn within(block: &MapRegionInfo, x: (u16, u16), y: (u16, u16)) -> bool {
    let at = block.grid_coordinates;
    let block_x = u16::try_from(at.x()).unwrap_or(u16::MAX);
    let block_y = u16::try_from(at.y()).unwrap_or(u16::MAX);
    block_x >= x.0 && block_x <= x.1 && block_y >= y.0 && block_y <= y.1
}

/// Whether `block`'s name matches `wanted` the way `how` matches, ignoring
/// case.
fn named(how: NameMatch, block: &MapRegionInfo, wanted: &str) -> bool {
    let wanted = wanted.to_lowercase();
    block.name.as_ref().is_some_and(|found| {
        let found = found.to_string().to_lowercase();
        match how {
            NameMatch::Prefix => found.starts_with(&wanted),
            NameMatch::Anywhere => found.contains(&wanted),
        }
    })
}

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;
    use sl_proto::{
        MapBlockKind, MapItemType, MapRegionInfo, MapRequestFlags, Maturity, RegionHandle,
    };
    use sl_types::key::TextureKey;
    use sl_types::map::{GridCoordinates, GridRectangleLike as _, RegionName};

    use super::{
        REGION_SIZE_M, SearchAlert, block_answer, item_answer, layer_answer, name_answer, named,
        within,
    };
    use crate::imitates::{ImitatedGrid, NameMatch};

    /// A map block for a region called `name` at grid `(x, y)`.
    fn block(name: &str, x: u32, y: u32) -> MapRegionInfo {
        MapRegionInfo {
            name: RegionName::try_new(name.to_owned()).ok(),
            grid_coordinates: GridCoordinates::new(x, y),
            region_handle: RegionHandle::from_grid(x, y),
            maturity: Maturity::Pg,
            region_flags: 0,
            size_x: REGION_SIZE_M,
            size_y: REGION_SIZE_M,
            agents: 0,
            water_height: 20,
            map_image_id: TextureKey::from(uuid::Uuid::from_u128(u128::from(x))),
        }
    }

    /// A grid of two regions side by side.
    fn pair() -> [MapRegionInfo; 2] {
        [
            block("Fake Region", 1000, 1000),
            block("Fake Region East", 1001, 1000),
        ]
    }

    /// The layer flag and the null-sims flag, as a request carries each.
    const LAYER: MapRequestFlags = MapRequestFlags(MapRequestFlags::LAYER);
    /// The null-sims flag.
    const NULL: MapRequestFlags = MapRequestFlags(MapRequestFlags::RETURN_NULL_SIMS);

    /// The block rectangle is inclusive on both bounds, and a region outside it
    /// on either axis is left out.
    #[test]
    fn a_block_request_takes_an_inclusive_rectangle() {
        let here = block("Fake Region", 1000, 1000);
        assert!(within(&here, (1000, 1000), (1000, 1000)), "its own cell");
        assert!(
            within(&here, (999, 1001), (999, 1001)),
            "a margin around it"
        );
        assert!(!within(&here, (1001, 1002), (999, 1001)), "east of it");
        assert!(!within(&here, (999, 1001), (1001, 1002)), "north of it");
    }

    /// Both grids echo the low half of the flags, send an image id only for a
    /// request with none, and state no water, agents or region flags.
    #[test]
    fn a_block_states_a_name_a_cell_and_a_rating() -> Result<(), String> {
        for grid in [ImitatedGrid::SecondLife, ImitatedGrid::OpenSim] {
            let policy = grid.map_policy();
            let layered = block_answer(&policy, &pair(), (1000, 1000), (1000, 1000), LAYER);
            let record = layered
                .first()
                .and_then(|batch| batch.blocks.first())
                .ok_or("the region's own cell is answered")?;
            assert_eq!(layered.first().map(|batch| batch.flags), Some(LAYER));
            assert!(record.map_image_id.is_nil(), "no image with the layer flag");
            assert_eq!(
                (record.water_height, record.agents, record.region_flags),
                (0, 0, 0)
            );
            let plain = block_answer(
                &policy,
                &pair(),
                (1000, 1000),
                (1000, 1000),
                MapRequestFlags(0),
            );
            assert!(
                plain
                    .first()
                    .and_then(|batch| batch.blocks.first())
                    .is_some_and(|record| !record.map_image_id.is_nil()),
                "an image without it"
            );
            let null = block_answer(&policy, &pair(), (1000, 1000), (1000, 1000), NULL);
            assert_eq!(
                null.first().map(|batch| batch.flags),
                Some(MapRequestFlags(0)),
                "the null-sims flag is not echoed"
            );
            assert!(
                block_answer(&policy, &pair(), (1001, 1000), (1000, 1000), LAYER).is_empty(),
                "bounds the wrong way round are not answered"
            );
            assert!(
                block_answer(&policy, &pair(), (100, 100), (100, 100), LAYER).is_empty(),
                "an empty cell without the null-sims flag is not answered"
            );
        }
        Ok(())
    }

    /// Second Life reports every empty cell of a null-sims rectangle; OpenSim
    /// only a cell asked about alone.
    #[test]
    fn empty_cells_are_reported_as_each_grid_does() {
        let kinds = |grid: ImitatedGrid, x: (u16, u16), y: (u16, u16)| -> Vec<MapBlockKind> {
            block_answer(&grid.map_policy(), &pair(), x, y, NULL)
                .iter()
                .flat_map(|batch| batch.blocks.iter())
                .map(sl_proto::MapBlockRecord::kind)
                .collect()
        };
        let region = MapBlockKind::Region;
        let empty = MapBlockKind::EmptyCell;
        for grid in [ImitatedGrid::SecondLife, ImitatedGrid::OpenSim] {
            assert_eq!(kinds(grid, (100, 100), (100, 100)), vec![empty], "{grid:?}");
        }
        assert_eq!(
            kinds(ImitatedGrid::SecondLife, (1000, 1001), (1000, 1001)),
            vec![region, region, empty, empty]
        );
        assert_eq!(
            kinds(ImitatedGrid::OpenSim, (1000, 1001), (1000, 1001)),
            vec![region, region]
        );
        assert_eq!(
            kinds(ImitatedGrid::SecondLife, (100, 101), (100, 101)).len(),
            4
        );
        assert!(kinds(ImitatedGrid::OpenSim, (100, 101), (100, 101)).is_empty());
    }

    /// Second Life leaves a rectangle of more than 256 cells unanswered;
    /// OpenSim answers it.
    #[test]
    fn a_large_rectangle_is_answered_by_opensim_alone() {
        let ask = |grid: ImitatedGrid, side: u16| {
            let far = 989_u16.saturating_add(side);
            block_answer(&grid.map_policy(), &pair(), (990, far), (990, far), LAYER).len()
        };
        assert_eq!(ask(ImitatedGrid::SecondLife, 16), 1);
        assert_eq!(ask(ImitatedGrid::SecondLife, 17), 0);
        assert_eq!(ask(ImitatedGrid::OpenSim, 25), 1);
    }

    /// A search matches a prefix on Second Life and anywhere in the name on
    /// OpenSim, both ignoring case.
    #[test]
    fn a_name_request_matches_as_each_grid_does() {
        let east = block("Fake Region East", 1001, 1000);
        for how in [NameMatch::Prefix, NameMatch::Anywhere] {
            assert!(named(how, &east, "Fake"));
            assert!(named(how, &east, "FAKE REGION EAST"));
            assert!(!named(how, &east, "Fake Region West"));
        }
        assert!(!named(NameMatch::Prefix, &east, "Region East"));
        assert!(named(NameMatch::Anywhere, &east, "Region East"));
    }

    /// Every search ends with an entry at `(0, 0)` carrying the search text;
    /// OpenSim refuses a short one and says so, and says when nothing matched.
    #[test]
    fn a_search_ends_with_its_own_text() {
        let sl = ImitatedGrid::SecondLife.map_policy();
        let os = ImitatedGrid::OpenSim.map_policy();
        for policy in [&sl, &os] {
            let (batch, alert) = name_answer(policy, &pair(), "Fake Region East", LAYER);
            assert_eq!(alert, None);
            assert_eq!(batch.flags, LAYER);
            let kinds: Vec<MapBlockKind> = batch
                .blocks
                .iter()
                .map(sl_proto::MapBlockRecord::kind)
                .collect();
            assert_eq!(kinds, vec![MapBlockKind::Region, MapBlockKind::Terminator]);
            assert_eq!(
                batch
                    .blocks
                    .last()
                    .map(|end| (end.name.as_str(), end.access)),
                Some(("Fake Region East", 255))
            );
        }
        let (short, alert) = name_answer(&sl, &pair(), "Fa", LAYER);
        assert_eq!((short.blocks.len(), alert), (3, None));
        let (short, alert) = name_answer(&os, &pair(), "Fa", LAYER);
        assert_eq!(short.blocks.len(), 1);
        assert!(matches!(alert, Some(SearchAlert::TooShort(_))));

        let (none, alert) = name_answer(&sl, &pair(), "Nowhere", LAYER);
        assert_eq!((none.blocks.len(), alert), (1, None));
        let (none, alert) = name_answer(&os, &pair(), "Nowhere", LAYER);
        assert_eq!(none.blocks.len(), 1);
        assert!(matches!(alert, Some(SearchAlert::NoMatch(_))));
    }

    /// A region with nobody to show has one agent-location item with an
    /// `Extra` of zero: on the corner on Second Life, a metre in on OpenSim.
    /// OpenSim sends it for any type asked of the agent's own region by name.
    #[test]
    fn an_empty_region_has_one_dot_that_counts_nobody() -> Result<(), String> {
        let here = RegionHandle::from_grid(1000, 1000);
        let east = RegionHandle::from_grid(1001, 1000);
        for (grid, inset) in [
            (ImitatedGrid::SecondLife, 0.0),
            (ImitatedGrid::OpenSim, 1.0),
        ] {
            let policy = grid.map_policy();
            for handle in [RegionHandle(0), here, east] {
                let answer =
                    item_answer(&policy, &pair(), here, MapItemType::AgentLocations, handle);
                let (kind, items) = answer.first().ok_or("agent locations are answered")?;
                assert_eq!(*kind, MapItemType::AgentLocations);
                let item = items.first().ok_or("one item")?;
                assert_eq!((items.len(), item.extra, item.id), (1, 0, None));
                let region = if handle == east { 1001.0 } else { 1000.0 };
                assert_eq!(
                    (item.position.x(), item.position.y()),
                    (region * 256.0 + inset, 256_000.0 + inset)
                );
            }
            assert!(
                item_answer(
                    &policy,
                    &pair(),
                    here,
                    MapItemType::AgentLocations,
                    RegionHandle::from_grid(100, 100)
                )
                .is_empty(),
                "a region the grid does not have is not answered about"
            );
            assert!(
                item_answer(
                    &policy,
                    &pair(),
                    here,
                    MapItemType::Telehub,
                    RegionHandle(0)
                )
                .is_empty()
            );
        }
        let named = |grid: ImitatedGrid, handle| {
            item_answer(
                &grid.map_policy(),
                &pair(),
                here,
                MapItemType::Telehub,
                handle,
            )
            .len()
        };
        assert_eq!(named(ImitatedGrid::SecondLife, here), 0);
        assert_eq!(named(ImitatedGrid::OpenSim, here), 1);
        assert_eq!(named(ImitatedGrid::OpenSim, east), 0);
        Ok(())
    }

    /// Second Life has no map layers and echoes the flags; OpenSim has one
    /// over the whole grid and echoes none.
    #[test]
    fn layers_are_answered_as_each_grid_does() -> Result<(), String> {
        let (flags, layers) = layer_answer(&ImitatedGrid::SecondLife.map_policy(), LAYER);
        assert_eq!((flags, layers.len()), (LAYER, 0));
        let (flags, layers) = layer_answer(&ImitatedGrid::OpenSim.map_policy(), LAYER);
        assert_eq!(flags, MapRequestFlags(0));
        let layer = layers.first().ok_or("one layer")?;
        assert_eq!(
            layer.rect.upper_right_corner().to_owned(),
            GridCoordinates::new(30_000, 30_000)
        );
        assert_eq!(
            layer.image_id.uuid().to_string(),
            "00000000-0000-1111-9999-000000000006"
        );
        Ok(())
    }
}
