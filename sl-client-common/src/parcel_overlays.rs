//! The reassembled parcel-ownership overlays of every region a session holds.
//!
//! A simulator pushes a region's overlay — the 64×64 grid of per-square
//! ownership colour and boundary/sound flags — as four
//! [`ParcelOverlay`](sl_proto::Event::ParcelOverlay) chunks, and which region a
//! chunk belongs to, and when a held grid stops being true, follows from other
//! events in the same stream. [`ParcelOverlays::apply`] is that sequencing,
//! written once: the Bevy runtime folds its event stream through it into its
//! `SlParcelOverlay` resource, and a tokio consumer folds the events
//! `Client::run` forwards through it the same way.

use std::collections::HashMap;

use sl_proto::{DEFAULT_GRIDS_PER_EDGE, Event, ParcelOverlayGrid, ParcelOverlayInfo, RegionHandle};

/// The reassembled parcel-ownership overlays, one grid per region.
///
/// The simulator sends a region's overlay unprompted on entry (there is no
/// overlay-request message; the reference viewer relies on the same push) and
/// re-broadcasts the whole overlay when a parcel is split, joined, or sold, so
/// the grids stay current simply by folding in each chunk. Chunks arrive on the
/// root circuit *and* on neighbour child circuits, each tagged with its source
/// region ([`ParcelOverlayInfo::region_handle`]), so neighbour regions
/// accumulate their own grids (Second Life pushes them on child-agent
/// establishment; OpenSim only on parcel changes). The **current** region's grid
/// is discarded and rebuilt on every region change, so a consumer never reads a
/// stale grid for the region it stands in: check [`region`](Self::region) /
/// [`is_complete`](Self::is_complete) before trusting [`grid`](Self::grid).
#[derive(Default, Debug, Clone)]
pub struct ParcelOverlays {
    /// The current (root) region, or `None` before the circuit is up. Chunks
    /// arriving without a region tag are attributed here.
    region: Option<RegionHandle>,
    /// The reassembled grid per region.
    grids: HashMap<RegionHandle, ParcelOverlayGrid>,
}

impl ParcelOverlays {
    /// The current region the untagged chunks are attributed to, or `None`
    /// before any region is known.
    #[must_use]
    pub const fn region(&self) -> Option<RegionHandle> {
        self.region
    }

    /// The **current** region's reassembled overlay grid, or `None` before its
    /// first chunk arrives. Pair with [`is_complete`](Self::is_complete) to
    /// know whether every chunk is in.
    #[must_use]
    pub fn grid(&self) -> Option<&ParcelOverlayGrid> {
        self.grids.get(&self.region?)
    }

    /// The reassembled overlay grid of `region` (the current region or a
    /// neighbour), or `None` before its first chunk arrives.
    #[must_use]
    pub fn grid_of(&self, region: RegionHandle) -> Option<&ParcelOverlayGrid> {
        self.grids.get(&region)
    }

    /// Whether the current region's grid exists and every one of its chunks
    /// has arrived.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.grid().is_some_and(ParcelOverlayGrid::is_complete)
    }

    /// Puts an already-assembled grid in place as `region`'s overlay, for unit
    /// tests of a consumer (the property-line bands) that need a region to have
    /// a grid without driving a whole session's chunk stream through it.
    ///
    /// Not `#[cfg(test)]`: the consumers live in other crates, and a
    /// `cfg(test)` item is not compiled when a *dependent* crate runs its
    /// tests. The `_for_test` name is the documentation; nothing in a running
    /// client should call it.
    pub fn insert_grid_for_test(&mut self, region: RegionHandle, grid: ParcelOverlayGrid) {
        drop(self.grids.insert(region, grid));
    }

    /// Folds one session event into the overlays.
    ///
    /// `login_region` is the region the session logged in to, which a
    /// [`CircuitEstablished`](Event::CircuitEstablished) does not name itself:
    /// the Bevy runtime passes its `SlIdentity::region_handle`, a tokio
    /// consumer what `Client::region_handle` returned before `Client::run`.
    ///
    /// - The root circuit coming up, and every region change, make that region
    ///   current and drop any grid held for it: the region pushes its overlay
    ///   afresh on entry.
    /// - An overlay chunk is folded into its source region's grid.
    /// - A neighbour whose child circuit was retired loses its grid, since a
    ///   region announced again sends its overlay afresh; the current region
    ///   keeps its own.
    /// - A disconnect or logout forgets everything.
    pub fn apply(&mut self, event: &Event, login_region: Option<RegionHandle>) {
        match event {
            Event::CircuitEstablished { .. } => {
                if let Some(region) = login_region {
                    self.reset_for_region(region);
                }
            }
            Event::RegionChanged { region_handle, .. } => self.reset_for_region(*region_handle),
            Event::ParcelOverlay(info) => self.ingest(info),
            Event::NeighborRetired {
                region_handle: Some(region),
                ..
            } => {
                if self.region != Some(*region) {
                    drop(self.grids.remove(region));
                }
            }
            Event::Disconnected(_) | Event::LoggedOut => *self = Self::default(),
            _other => {}
        }
    }

    /// Folds one pushed overlay chunk into its region's grid, creating the
    /// grid (sized for a standard 256 m region) on the first chunk. An untagged
    /// chunk (region handle 0, the source circuit not yet associated) goes to
    /// the current region; a malformed chunk is logged and dropped, leaving the
    /// grid intact.
    fn ingest(&mut self, info: &ParcelOverlayInfo) {
        let region = if info.region_handle.0 == 0 {
            match self.region {
                Some(region) => region,
                None => {
                    tracing::warn!("dropping a parcel-overlay chunk with no attributable region");
                    return;
                }
            }
        } else {
            info.region_handle
        };
        let grid = self
            .grids
            .entry(region)
            .or_insert_with(|| ParcelOverlayGrid::new(DEFAULT_GRIDS_PER_EDGE));
        if let Err(error) = grid.ingest_chunk(info.sequence_id, &info.data) {
            tracing::warn!(%error, "dropping malformed parcel-overlay chunk");
        }
    }

    /// Records the region the next untagged chunks describe, discarding any
    /// grid already held for it. Neighbour grids are kept (their regions
    /// re-push on their own churn).
    fn reset_for_region(&mut self, region: RegionHandle) {
        self.region = Some(region);
        drop(self.grids.remove(&region));
    }
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, SocketAddr};

    use sl_proto::{
        CircuitId, DisconnectReason, Event, NeighborRetirement, ParcelOverlayInfo, RegionHandle,
    };

    use pretty_assertions::assert_eq;

    use super::ParcelOverlays;

    /// The login region of every test.
    const HOME: RegionHandle = RegionHandle(0x0000_03e8_0000_03e8);
    /// Its eastern neighbour.
    const EAST: RegionHandle = RegionHandle(0x0000_03e9_0000_03e8);

    /// A simulator address.
    fn sim(port: u16) -> SocketAddr {
        SocketAddr::from((Ipv4Addr::LOCALHOST, port))
    }

    /// The root circuit coming up.
    fn established() -> Event {
        Event::CircuitEstablished {
            sim: sim(9000),
            circuit: CircuitId(1),
        }
    }

    /// The `sequence_id`-th chunk of a standard region, every square of class
    /// `class`, tagged with `region` (`RegionHandle(0)` for an untagged one).
    fn chunk(region: RegionHandle, sequence_id: i32, class: u8) -> Event {
        Event::ParcelOverlay(ParcelOverlayInfo {
            sequence_id,
            data: vec![class; 1024],
            region_handle: region,
        })
    }

    /// Overlays holding a complete grid for both `HOME` (untagged chunks) and
    /// `EAST` (tagged ones).
    fn both_complete() -> ParcelOverlays {
        let mut overlays = ParcelOverlays::default();
        overlays.apply(&established(), Some(HOME));
        for sequence in 0..4 {
            overlays.apply(&chunk(RegionHandle(0), sequence, 0x00), Some(HOME));
            overlays.apply(&chunk(EAST, sequence, 0x03), Some(HOME));
        }
        overlays
    }

    #[test]
    fn untagged_chunks_go_to_the_login_region_and_tagged_ones_to_their_own() {
        let overlays = both_complete();
        assert_eq!(overlays.region(), Some(HOME));
        assert!(overlays.is_complete());
        assert!(
            overlays
                .grid_of(EAST)
                .is_some_and(|grid| grid.is_complete())
        );
    }

    #[test]
    fn an_untagged_chunk_before_the_circuit_is_dropped() {
        let mut overlays = ParcelOverlays::default();
        overlays.apply(&chunk(RegionHandle(0), 0, 0x00), Some(HOME));
        assert_eq!(overlays.region(), None);
        assert!(overlays.grid_of(HOME).is_none());
    }

    #[test]
    fn a_retired_neighbour_loses_its_grid_but_the_current_region_keeps_its_own() {
        let mut overlays = both_complete();
        for region in [EAST, HOME] {
            overlays.apply(
                &Event::NeighborRetired {
                    region_handle: Some(region),
                    sim: sim(9001),
                    circuit: CircuitId(2),
                    reason: NeighborRetirement::Disabled,
                },
                Some(HOME),
            );
        }
        assert!(overlays.grid_of(EAST).is_none());
        assert!(overlays.is_complete());
    }

    #[test]
    fn a_region_change_drops_the_destination_grid_and_keeps_the_others() {
        let mut overlays = both_complete();
        overlays.apply(
            &Event::RegionChanged {
                region_handle: EAST,
                sim: sim(9001),
                circuit: CircuitId(2),
                world_reset: false,
            },
            Some(HOME),
        );
        assert_eq!(overlays.region(), Some(EAST));
        assert!(overlays.grid().is_none());
        assert!(overlays.grid_of(HOME).is_some());
    }

    #[test]
    fn a_disconnect_forgets_everything() {
        let mut overlays = both_complete();
        overlays.apply(
            &Event::Disconnected(DisconnectReason::ProtocolError),
            Some(HOME),
        );
        assert_eq!(overlays.region(), None);
        assert!(overlays.grid_of(HOME).is_none());
        assert!(overlays.grid_of(EAST).is_none());
    }
}
