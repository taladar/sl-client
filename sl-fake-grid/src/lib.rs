#![doc = include_str!("../README.md")]

pub mod accounts;
pub mod agent_requests;
mod assets;
mod caps_endpoint;
pub mod crossing;
mod driver;
mod economy_endpoint;
pub mod economy_policy;
pub mod error;
pub mod estate;
pub mod fixtures;
mod http_answer;
mod http_service;
mod login_endpoint;
mod map_tiles;
pub mod marker;
pub mod neighbours;
mod object_edits;
mod parcel_edits;
pub mod runtime;
pub mod scenario;
mod teleport;
pub mod terrain;
pub mod time;
pub mod udp_assets;
mod uploads;
pub mod world;
mod world_map;

pub use accounts::AccountConfig;
pub use agent_requests::{AgentPolicy, LegacyUdpInventory};
pub use crossing::CROSSING_ARRIVAL_TIMEOUT;
pub use economy_policy::{EconomyConfig, EconomyEvent, stock_prices};
pub use error::Error;
pub use estate::EstateFixture;
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
pub use udp_assets::{UdpAssetFixtures, flat_terrain_raw};
pub use world::{
    AvatarIdentity, ParcelAccessLists, ParcelListing, RegionChange, RegionUpdate, RegionWorld,
    SceneFixtures, TaskInventory, avatar_prim, box_prim, default_object_properties,
    prim_from_shape, region_limits, region_wide_parcel,
};
