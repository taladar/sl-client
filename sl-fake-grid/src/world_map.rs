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
//! [`crate::map_tiles`] is the other half, and a different protocol: the JPEG
//! tiles a modern viewer fetches over HTTP. This module is the legacy UDP
//! catalogue those tiles are drawn under.

use std::time::Instant;

use sl_proto::{MapItem, MapItemType, MapRegionInfo, RegionHandle, ServerEvent, SimSession};
use sl_types::key::TextureKey;
use sl_types::map::{GlobalCoordinates, GridCoordinates, GridRectangle};

use crate::runtime::RegionEntry;
use crate::world::AvatarIdentity;

/// The width and height, in metres, of every region a fake grid serves.
///
/// The fake grid has no variable-sized regions: a `RegionConfig` names a grid
/// index and nothing else, so every entry reports the standard 256.
const REGION_SIZE_M: u32 = 256;

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
        // The count a map block reports is the *map service's* stale idea of
        // how busy a region is; the live answer is the `AgentLocations` item
        // reply below, which is what a viewer actually draws dots from.
        agents: 0,
        // The wire field is whole metres, so a region whose water sits at
        // 20.5 m is reported at 21 — the same rounding the terrain RAW file
        // uses for a ground height.
        water_height: crate::terrain::round_to_u8(entry.config.water_height),
        map_image_id: TextureKey::from(entry.region_id),
    }
}

/// Answers one world-map request from `map`, the grid's whole region
/// catalogue.
///
/// `here` is the region handle of the session answering, and `avatar` the agent
/// on it: between them they place the one green dot this grid has to report.
pub(crate) fn answer_map_request(
    map: &[MapRegionInfo],
    here: RegionHandle,
    avatar: &AvatarIdentity,
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
            let blocks: Vec<MapRegionInfo> = map
                .iter()
                .filter(|block| within(block, (*min_x, *max_x), (*min_y, *max_y)))
                .cloned()
                .collect();
            if let Err(error) = sim.send_map_block_reply(*flags, &blocks, now) {
                tracing::warn!("answering a map block request failed: {error}");
            }
        }
        ServerEvent::MapNameRequested { name, flags } => {
            let blocks: Vec<MapRegionInfo> = map
                .iter()
                .filter(|block| named(block, name))
                .cloned()
                .collect();
            if let Err(error) = sim.send_map_block_reply(*flags, &blocks, now) {
                tracing::warn!("answering a map name request failed: {error}");
            }
        }
        ServerEvent::MapItemRequested {
            item_type,
            region_handle,
            flags,
        } => {
            // A zero handle means "the region I am in"; anything else asks
            // about a named one, and this grid only knows where its own agent
            // is.
            let asked_about_here = region_handle.0 == 0 || *region_handle == here;
            let items = if matches!(item_type, MapItemType::AgentLocations) && asked_about_here {
                vec![agent_dot(here, avatar, sim)]
            } else {
                Vec::new()
            };
            if let Err(error) = sim.send_map_item_reply(*flags, *item_type, &items, now) {
                tracing::warn!("answering a map item request failed: {error}");
            }
        }
        ServerEvent::MapLayerRequested { flags } => {
            let layers = whole_grid_layer(map)
                .map(|layer| vec![layer])
                .unwrap_or_default();
            if let Err(error) = sim.send_map_layer_reply(*flags, &layers, now) {
                tracing::warn!("answering a map layer request failed: {error}");
            }
        }
        _other => {}
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

/// Whether `block`'s name starts with `wanted`, case-insensitively.
///
/// A prefix match, as every grid's map search is: the viewer's search box sends
/// whatever has been typed so far, so an exact match would find nothing until
/// the last keystroke.
fn named(block: &MapRegionInfo, wanted: &str) -> bool {
    let wanted = wanted.trim().to_lowercase();
    block
        .name
        .as_ref()
        .is_some_and(|found| found.to_string().to_lowercase().starts_with(&wanted))
}

/// One layer covering every region in `map`, or `None` for a grid with no
/// regions at all.
///
/// The image is the first region's tile id. A real grid composites a whole-grid
/// image at each zoom; this one has a handful of regions in a row, and the
/// layer exists so a viewer's map has *something* to draw the blocks over.
fn whole_grid_layer(map: &[MapRegionInfo]) -> Option<sl_proto::MapLayer> {
    let first = map.first()?;
    let (low, high) = map.iter().fold(
        (first.grid_coordinates, first.grid_coordinates),
        |(low, high), block| {
            let at = block.grid_coordinates;
            (
                GridCoordinates::new(low.x().min(at.x()), low.y().min(at.y())),
                GridCoordinates::new(high.x().max(at.x()), high.y().max(at.y())),
            )
        },
    );
    Some(sl_proto::MapLayer {
        rect: GridRectangle::new(low, high),
        image_id: first.map_image_id,
    })
}

/// The one green dot this region has: the agent standing on it, at wherever the
/// session last placed it.
fn agent_dot(here: RegionHandle, avatar: &AvatarIdentity, sim: &SimSession) -> MapItem {
    let position = sim.arrival_position().position;
    MapItem {
        position: GlobalCoordinates::from_grid_and_region(GridCoordinates::from(here), position),
        // Avatar dots carry no id: the map draws them anonymously, and the
        // reference viewer reads the id field of an `AgentLocations` item as
        // nothing at all.
        id: None,
        // `Extra` is how many avatars this dot stands for.
        extra: 1,
        extra2: 0,
        name: format!("{} {}", avatar.first_name, avatar.last_name),
    }
}

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;
    use sl_proto::{MapRegionInfo, Maturity, RegionHandle};
    use sl_types::key::TextureKey;
    use sl_types::map::{GridCoordinates, GridRectangleLike as _, RegionName};

    use super::{REGION_SIZE_M, named, whole_grid_layer, within};

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

    /// A name search matches a prefix, ignores case and surrounding space, and
    /// an empty search matches everything — which is what the viewer's search
    /// box sends before the first keystroke.
    #[test]
    fn a_name_request_matches_a_prefix() {
        let east = block("Fake Region East", 1001, 1000);
        assert!(named(&east, "Fake"));
        assert!(named(&east, "  fake region  "));
        assert!(named(&east, "FAKE REGION EAST"));
        assert!(named(&east, ""));
        assert!(!named(&east, "Region East"), "a prefix, not a substring");
        assert!(!named(&east, "Fake Region West"));
    }

    /// The layer covers every region on the grid, however they are laid out,
    /// and a grid with no regions offers no layer rather than a degenerate one.
    #[test]
    fn the_layer_covers_every_region() -> Result<(), String> {
        assert!(
            whole_grid_layer(&[]).is_none(),
            "an empty grid has no layer"
        );
        let map = [
            block("Fake Region", 1000, 1000),
            block("Fake Region East", 1001, 1000),
            block("Fake Region South", 1000, 999),
        ];
        let layer = whole_grid_layer(&map).ok_or("a populated grid has a layer")?;
        assert_eq!(
            layer.rect.lower_left_corner().to_owned(),
            GridCoordinates::new(1000, 999)
        );
        assert_eq!(
            layer.rect.upper_right_corner().to_owned(),
            GridCoordinates::new(1001, 1000)
        );
        assert_eq!(layer.image_id, map[0].map_image_id);
        Ok(())
    }
}
