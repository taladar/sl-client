#![doc = include_str!("../README.md")]

pub mod accounts;
pub mod agent_requests;
pub mod assets;
pub mod bakes;
pub mod benefits;
mod caps_endpoint;
pub mod chat;
pub mod crossing;
mod driver;
mod economy_endpoint;
pub mod economy_policy;
pub mod error;
pub mod estate;
pub mod experiences;
pub mod fixtures;
pub mod god_powers;
mod grabs;
pub mod groups;
mod http_answer;
mod http_service;
pub mod imitates;
pub mod inventory;
mod land_sales;
mod link_sets;
mod login_endpoint;
pub mod login_sections;
mod map_tiles;
pub mod marker;
pub mod neighbours;
mod object_edits;
mod parcel_edits;
pub mod relays;
pub mod runtime;
pub mod scenario;
pub mod social;
mod task_contents;
mod teleport;
pub mod terrain;
pub mod time;
pub mod timeline;
pub mod udp_assets;
mod uploads;
pub mod voice;
pub mod world;
mod world_map;

pub use accounts::AccountConfig;
pub use agent_requests::AgentPolicy;
pub use assets::ObjectAssetPolicy;
pub use bakes::{BakePolicy, REGION_PROTOCOL_BAKES_ON_MESH, REGION_PROTOCOL_SERVER_BAKES};
pub use benefits::{DEFAULT_PACKAGE, second_life_packages};
pub use crossing::CROSSING_ARRIVAL_TIMEOUT;
pub use economy_policy::{EconomyConfig, EconomyEvent, open_sim_prices, second_life_prices};
pub use error::Error;
pub use estate::EstateFixture;
pub use fixtures::{
    CatalogueEntry, FaceStyle, Landmark, NamedScenario, NpcAppearance, NpcBake, NpcFixture,
    PrimFixture, RegionFixture, SculptKind, catalogue, linkset,
};
pub use groups::{GroupConfig, GroupsPolicy, InvitationForm, StockRoles};
pub use imitates::{
    AbsentTile, ChildSale, ContentsAudience, ContentsPolicy, CopiedSerial, DerezKill, DotName,
    EditEcho, EditPolicy, EmptyCells, EnvironmentChangeReply, EstatePolicy, GroupObjectTrash,
    ImitatedGrid, LandOrigin, LastOwner, LinkPolicy, LoginFields, LoginRefusals, LogoutReply,
    MapLayerAnswer, MapPolicy, MaskRules, NameMatch, Newcomers, NoBuildAlert,
    OPENSIM_ALL_PERMISSIONS, OPENSIM_FOLDED_BITS, OPENSIM_MAX_AGENT_GROUPS, ObjectDelete,
    OutOfWorld, ParcelPolicy, Placement, PrimResent, Promoted, PropertiesPolicy, RayAtNothing,
    RegionCapacity, RemovedItem, RestartNoticeForm, RestoreToWorld, ReturnNotice, RezPolicy,
    RootRecord, SELECTED_PARCEL_SEQUENCE_ID, SIT_TARGET_RAISE_M, SaveTold, ScriptStart,
    SecondLogin, SentOrder, SitPolicy, SitRefusal, StockDay, StoppedScript, StoredText,
    TakenItemMasks, TerrainPolicy, TrashFolder, WindStart,
};
pub use inventory::{
    InventoryAnnouncement, LegacyUdpInventory, UploadAnnouncement, UploadAnnouncements,
};
pub use map_tiles::{BLANK_TILE_JPEG, STOCK_TILE_JPEG};
pub use marker::{
    MARKER_METHOD, NEIGHBOUR_MARKER_PREFIX, marker, marker_name, neighbour_marker,
    neighbour_marker_region,
};
pub use neighbours::NeighbourPolicy;
pub use relays::{
    EffectsSent, EffectsVetting, OPENSIM_GAZE_RANGE_M, OPENSIM_SOUND_RANGE_M, RelayPolicy,
};
pub use runtime::{
    CrossingNotice, FakeAgent, FakeGrid, FakeGridBuilder, GridIdentity, LoginNotice, RegionConfig,
    TeleportNotice,
};
pub use scenario::{Scenario, SimEventHook, SimHook};
pub use teleport::TELEPORT_ARRIVAL_TIMEOUT;
pub use terrain::{Heightfield, TerrainFixture};
pub use time::{Now, system_clock, tokio_clock};
pub use timeline::{
    Action, At, EventPredicate, ObjectEdit, ParcelEdit, RegionEdit, Step, Timeline,
};
pub use udp_assets::{UdpAssetFixtures, flat_terrain_raw};
pub use uploads::UpdateCompletionItem;
pub use voice::VoiceBackend;
pub use world::{
    AVATAR_CENTRE_ABOVE_GROUND_M, AvatarIdentity, ParcelAccessLists, ParcelListing, RegionChange,
    RegionUpdate, RegionWorld, SceneFixtures, TaskInventory, avatar_prim, box_prim,
    default_object_properties, prim_from_shape, region_limits, region_wide_parcel,
};
