#![doc = include_str!("../README.md")]

pub mod accounts;
mod assets;
mod caps_endpoint;
pub mod crossing;
mod driver;
mod economy_endpoint;
pub mod economy_policy;
pub mod error;
pub mod fixtures;
mod http_answer;
mod http_service;
mod login_endpoint;
mod map_tiles;
pub mod marker;
pub mod neighbours;
pub mod runtime;
pub mod scenario;
mod teleport;
pub mod terrain;
pub mod time;
pub mod udp_assets;
pub mod world;

pub use accounts::AccountConfig;
pub use crossing::CROSSING_ARRIVAL_TIMEOUT;
pub use economy_policy::{EconomyConfig, EconomyEvent};
pub use error::Error;
pub use fixtures::{
    CatalogueEntry, FaceStyle, Landmark, NamedScenario, NpcAppearance, NpcBake, NpcFixture,
    PrimFixture, RegionFixture, SculptKind, catalogue, linkset,
};
pub use map_tiles::STOCK_TILE_JPEG;
pub use marker::{
    MARKER_METHOD, NEIGHBOUR_MARKER_PREFIX, marker, marker_name, neighbour_marker,
    neighbour_marker_region,
};
pub use neighbours::NeighbourPolicy;
pub use runtime::{
    CrossingNotice, FakeAgent, FakeGrid, FakeGridBuilder, GridIdentity, LoginNotice, RegionConfig,
    TeleportNotice,
};
pub use scenario::{Scenario, SimEventHook, SimHook};
pub use teleport::TELEPORT_ARRIVAL_TIMEOUT;
pub use terrain::{Heightfield, TerrainFixture};
pub use time::{Now, system_clock, tokio_clock};
pub use udp_assets::{TaskInventoryFixture, UdpAssetFixtures, flat_terrain_raw};
pub use world::{AvatarIdentity, SceneFixtures, avatar_prim, box_prim, region_wide_parcel};
