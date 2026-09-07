//! Which live grid this one is imitating.
//!
//! The fake grid exists to fail a viewer the way a real grid would, and there
//! are two real grids that do not agree. Where they differ, one of them has to
//! be picked — and picking it per behaviour, as this crate did until
//! [`ImitatedGrid`] existed, produces a grid that is nobody: a stock fake grid
//! announced `platform: OpenSim`, kept every login field like OpenSim, and
//! withheld a taken object's asset like Second Life, all at once. A viewer
//! passing against that has not been tested against anything.
//!
//! So the grid names the live one it is being, once
//! ([`FakeGridBuilder::imitates`](crate::FakeGridBuilder::imitates)), and every
//! divergent behaviour takes its default from that. A test that wants one
//! deviation still sets that one knob directly; the flavour is what the knob
//! falls back to, not a lock.
//!
//! # What the flavour decides today
//!
//! | behaviour | Second Life | OpenSim |
//! | --- | --- | --- |
//! | a taken object's asset ([`ObjectAssetPolicy`]) | withheld: nil id, unfetchable body | served: minted id, body under it |
//! | the login response's `options` list ([`honor_options`](crate::FakeGridBuilder::honor_options)) | honoured: the response is trimmed to what was asked for | ignored: every field is sent |
//! | the `OpenSimExtras` block in `SimulatorFeatures` ([`advertises_open_sim_extras`](ImitatedGrid::advertises_open_sim_extras)) | absent | sent, carrying the grid's map-tile and currency-helper URLs |
//! | the spatial-voice backend ([`VoiceBackend`]) | WebRTC, named three ways: `SimulatorFeatures.VoiceServerType`, the login `voice-config`, the `RequiredVoiceVersion` push | none: a stock region loads no voice module, and nothing is advertised |
//! | the deprecated UDP inventory fetch ([`LegacyUdpInventory`]) | refused with a `FeatureDisabled` | served out of the session's inventory tree |
//! | how a created inventory item is announced ([`InventoryAnnouncement`]) | a `BulkUpdateInventory` over the event queue | the legacy UDP `UpdateCreateInventoryItem` |
//!
//! **The inventory pair is the divergence a viewer is most likely to trip
//! over**, which is why it is two rows rather than one setting. An inventory
//! implementation that still reaches for the UDP fetch, or that only listens
//! for the legacy create, works against OpenSim and fails against the grid this
//! workspace targets — silently, in both directions. Second Life's refusal is
//! the one deliberate deviation from the measurement in this table: aditi
//! empirically *drops* the fetch without a word (2026-08-12), and
//! [`LegacyUdpInventory::Ignored`] reproduces that, but of the two roads a grid
//! without the path has only the refusal leaves something to assert — silence
//! is indistinguishable from a lost packet.
//!
//! **The map and currency URLs survive losing the extras block**, which is the
//! part that had to be checked rather than assumed. Both are reachable by a
//! route that is not `OpenSimExtras` and that both grids serve: the map-tile
//! server through the login response's `map-server-url`
//! (`LLStartUp::process_login_success_response` reads it there and
//! `LFSimFeatureHandler` only *overrides* it from the extras), the currency
//! symbol through the login response's `currency`, and the currency helper
//! base through `get_grid_info`'s `economy` key, which is where
//! `LLGridManager::getHelperURI` reads it when no extras block overrode it.
//! Dropping the block on the Second Life side therefore hides no URL — it
//! removes a *second* copy of two of them.
//!
//! # What it does not decide yet, and why
//!
//! Each of these is a measured or documented divergence the fake grid takes one
//! side of unconditionally. They are not derived here because the *other* side
//! is not implemented — a flavour that claimed to decide them would be lying
//! about what a viewer meets. Each has a roadmap item of its own:
//!
//! | behaviour | the side the fake grid takes | what the other side needs |
//! | --- | --- | --- |
//! | server-side avatar bakes | Second Life's: `agent_appearance_service` is always named | dropping the service alone leaves every avatar a silent cloud; the "this avatar is server-baked" decision has to flip with it ([[test-fake-grid-imitates-server-bakes]]) |
//! | the economy helper and the price list | OpenSim's: a stock region's zeroes | what Second Life's helper and `EconomyData` actually answer is unmeasured ([[test-fake-grid-imitates-economy]]) |
//! | how an **upload-created** item is announced (as opposed to a taken one) | the legacy UDP message on both flavours | what Second Life sends besides the capability's own HTTP response is unmeasured ([[test-fake-grid-imitates-upload-announcements]]) |
//!
//! One thing is deliberately **not** flavour-decided and is not a to-do:
//! [`GridIdentity::platform`](crate::GridIdentity) stays `OpenSim` whichever
//! grid is being imitated. It is not protocol behaviour — it is what Firestorm's
//! grid manager reads to decide whether it will add the grid at all, and a fake
//! grid nothing can log into tests nothing.

use crate::assets::ObjectAssetPolicy;
use crate::inventory::{InventoryAnnouncement, LegacyUdpInventory};
use crate::voice::VoiceBackend;

/// The live grid a [`FakeGrid`](crate::FakeGrid) imitates where the two real
/// ones disagree.
///
/// The default is [`SecondLife`](Self::SecondLife): it is the grid this
/// workspace targets, and it is the stricter of the two almost everywhere the
/// pair has been measured — so it is the flavour that catches a viewer relying
/// on something only OpenSim allows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImitatedGrid {
    /// Second Life, which this workspace targets.
    #[default]
    SecondLife,
    /// OpenSim, which this workspace treats as a safe grid to test against.
    OpenSim,
}

impl ImitatedGrid {
    /// What this grid does with a taken object's asset.
    ///
    /// Second Life gives a viewer a nil asset id and no way to reach the body;
    /// OpenSim names a minted id and serves the body under it. Measured on
    /// aditi 2026-09-06 — see [`ObjectAssetPolicy`] for the numbers.
    #[must_use]
    pub const fn object_assets(self) -> ObjectAssetPolicy {
        match self {
            Self::SecondLife => ObjectAssetPolicy::Withheld,
            Self::OpenSim => ObjectAssetPolicy::Served,
        }
    }

    /// Whether this grid trims a login response to the `options` list the
    /// request asked for.
    ///
    /// Second Life does; OpenSim sends every field it has whatever was asked
    /// for. A viewer that reads a field it never requested works against one
    /// grid and not the other, which is the whole reason this is observable.
    #[must_use]
    pub const fn honors_login_options(self) -> bool {
        matches!(self, Self::SecondLife)
    }

    /// Whether this grid's `SimulatorFeatures` carries the `OpenSimExtras`
    /// block.
    ///
    /// OpenSim always sends it (`SimulatorFeaturesModule.cs` fills it in
    /// unconditionally, and `GridService` injects the grid-wide URLs into it);
    /// Second Life sends no such key, which is the one structural difference
    /// that reliably tells the two replies apart. What rides in it — the
    /// map-tile server and the currency helper — reaches a viewer on Second
    /// Life by another route, so this is a block to drop rather than a set of
    /// URLs to hide: see the module docs.
    #[must_use]
    pub const fn advertises_open_sim_extras(self) -> bool {
        matches!(self, Self::OpenSim)
    }

    /// The spatial-voice backend this grid's regions serve.
    ///
    /// Second Life is WebRTC. A stock OpenSim region is
    /// [`VoiceBackend::Silent`]: both its voice modules are optional and off by
    /// default, and both answer with the Vivox SIP shape this workspace does
    /// not implement anywhere — so the honest model of the grid nobody
    /// configured is a grid that offers no voice, and every advertisement of it
    /// falls away with the backend. See the [`voice`](crate::voice) module docs.
    #[must_use]
    pub const fn voice_backend(self) -> VoiceBackend {
        match self {
            Self::SecondLife => VoiceBackend::WebRtc,
            Self::OpenSim => VoiceBackend::Silent,
        }
    }

    /// How this grid answers the deprecated UDP inventory fetch.
    ///
    /// OpenSim still serves it (`LLClientView.HandleFetchInventoryDescendents`
    /// is wired to the region's inventory service); Second Life dropped it when
    /// inventory moved behind AIS3. The Second Life side is the *refusal* rather
    /// than the silence aditi was measured giving — see the module docs for why
    /// that deviation is deliberate.
    #[must_use]
    pub const fn legacy_udp_inventory(self) -> LegacyUdpInventory {
        match self {
            Self::SecondLife => LegacyUdpInventory::Refused,
            Self::OpenSim => LegacyUdpInventory::Served,
        }
    }

    /// How this grid announces an inventory item it just created — the item a
    /// take files away.
    ///
    /// OpenSim sends the legacy UDP `UpdateCreateInventoryItem`; Second Life
    /// delivers the new item as a `BulkUpdateInventory` over the event queue,
    /// which is why `object-asset-format`'s take leg waits for either.
    #[must_use]
    pub const fn inventory_announcement(self) -> InventoryAnnouncement {
        match self {
            Self::SecondLife => InventoryAnnouncement::BulkUpdate,
            Self::OpenSim => InventoryAnnouncement::Legacy,
        }
    }
}

#[cfg(test)]
mod test {
    use pretty_assertions::{assert_eq, assert_ne};

    use super::*;

    /// The default is the grid the workspace targets. Every derived knob reads
    /// this, so it is the one place a stock fake grid's whole personality is
    /// decided.
    #[test]
    fn a_grid_nobody_configured_is_second_life() {
        assert_eq!(ImitatedGrid::default(), ImitatedGrid::SecondLife);
    }

    /// The two grids take opposite sides of every knob derived here. That is
    /// not a coincidence worth asserting for its own sake — it is the check
    /// that a knob added later actually *decides* something, rather than
    /// answering both flavours the same way and only looking derived.
    #[test]
    fn every_derived_knob_separates_the_two_grids() {
        let sl = ImitatedGrid::SecondLife;
        let opensim = ImitatedGrid::OpenSim;
        assert_ne!(sl.object_assets(), opensim.object_assets());
        assert_ne!(sl.honors_login_options(), opensim.honors_login_options());
        assert_ne!(
            sl.advertises_open_sim_extras(),
            opensim.advertises_open_sim_extras()
        );
        assert_ne!(sl.voice_backend(), opensim.voice_backend());
        assert_ne!(sl.legacy_udp_inventory(), opensim.legacy_udp_inventory());
        assert_ne!(
            sl.inventory_announcement(),
            opensim.inventory_announcement()
        );
    }

    /// The inventory pair is the one place the flavour decides both ends of the
    /// same divergence, and getting either backwards would let a viewer that
    /// depends on the legacy path pass against a grid claiming to be Second
    /// Life — the exact failure this task existed to make impossible.
    #[test]
    fn only_the_open_sim_flavour_speaks_legacy_inventory() {
        assert_eq!(
            ImitatedGrid::OpenSim.legacy_udp_inventory(),
            LegacyUdpInventory::Served
        );
        assert_eq!(
            ImitatedGrid::OpenSim.inventory_announcement(),
            InventoryAnnouncement::Legacy
        );
        assert_eq!(
            ImitatedGrid::SecondLife.legacy_udp_inventory(),
            LegacyUdpInventory::Refused
        );
        assert_eq!(
            ImitatedGrid::SecondLife.inventory_announcement(),
            InventoryAnnouncement::BulkUpdate
        );
    }

    /// The fake grid's own default has to be the one Second Life actually is,
    /// and the other side has to be silence rather than a Vivox fixture: this
    /// workspace implements no Vivox-shaped voice, so a grid defaulting to it
    /// would serve a path nothing here speaks.
    #[test]
    fn the_stock_grid_speaks_webrtc_and_the_other_one_speaks_nothing() {
        assert_eq!(
            ImitatedGrid::SecondLife.voice_backend(),
            VoiceBackend::WebRtc
        );
        assert_eq!(ImitatedGrid::OpenSim.voice_backend(), VoiceBackend::Silent);
        assert_eq!(VoiceBackend::default(), VoiceBackend::WebRtc);
    }
}
