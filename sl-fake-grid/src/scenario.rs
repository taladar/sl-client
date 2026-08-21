//! Scriptable content: what a region's sessions are seeded with and what
//! greets an arriving avatar.
//!
//! A [`Scenario`] is the fake grid's whole content model — everything else
//! in this crate is protocol glue. The `setup` closure runs against a fresh
//! [`SimSession`] before the login response is built, populating the fixture
//! stores the CAPS and UDP surfaces serve (inventory, parcels, simulator
//! features, display names, …); the `on_agent_arrived` closure runs right
//! after the automatic `RegionHandshake` (content pushed at the arriving
//! client); the asset store backs the binary asset-delivery caps
//! (`GetTexture`, `GetMesh`, `ViewerAsset`); the [`UdpAssetFixtures`] back
//! the legacy UDP asset paths (`Xfer`, `Transfer`, task inventory, the
//! estate terrain RAW file); the `on_event` hook sees every drained
//! [`ServerEvent`] for behaviour the stock fixtures do not cover.

use std::sync::Arc;

use sl_proto::{
    AssetKey, AssetType, ChatSource, ChatType, InventoryFolder, InventoryItem, InventoryType,
    LindenAmount, ParcelVoiceInfo, Permissions, Permissions5, RegionLocalObjectId,
    RegionLocalParcelId, SaleType, ServerEvent, SimSession, TaskInventoryItem, VoiceChannelUri,
    WebRtcStub,
};
use sl_types::key::{AgentKey, InventoryFolderKey, InventoryKey, ObjectKey, OwnerKey, ParcelKey};

use crate::udp_assets::{TaskInventoryFixture, UdpAssetFixtures, flat_terrain_raw};

/// A hook run under the session lock against the machine (fixture setup,
/// on-arrival content pushes).
pub type SimHook = Arc<dyn Fn(&mut SimSession) + Send + Sync>;

/// A hook run under the session lock for every drained [`ServerEvent`],
/// after the stock fixture behaviour answered it.
pub type SimEventHook = Arc<dyn Fn(&mut SimSession, &ServerEvent) + Send + Sync>;

/// The scripted content for one region.
#[derive(Clone)]
pub struct Scenario {
    /// Seeds a fresh session's fixture stores before login completes.
    pub setup: SimHook,
    /// Runs after the automatic region handshake when the agent arrives.
    pub on_agent_arrived: Option<SimHook>,
    /// Runs for every drained [`ServerEvent`], after the stock behaviour.
    pub on_event: Option<SimEventHook>,
    /// The binary assets served by the asset-delivery caps.
    pub assets: sl_proto::InMemoryAssetSource,
    /// The content behind the legacy UDP asset paths.
    pub udp_assets: UdpAssetFixtures,
}

impl std::fmt::Debug for Scenario {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Scenario")
            .field("setup", &"<closure>")
            .field(
                "on_agent_arrived",
                &self.on_agent_arrived.as_ref().map(|_| "<closure>"),
            )
            .field("on_event", &self.on_event.as_ref().map(|_| "<closure>"))
            .field("udp_assets", &self.udp_assets)
            .finish_non_exhaustive()
    }
}

impl Scenario {
    /// An empty scenario: no fixtures, no arrival content, no assets.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            setup: Arc::new(|_| {}),
            on_agent_arrived: None,
            on_event: None,
            assets: sl_proto::InMemoryAssetSource::new(),
            udp_assets: UdpAssetFixtures::new(),
        }
    }
}

impl Default for Scenario {
    /// The stock scenario: a small standard inventory and library, one
    /// region-wide parcel, a chat greeting on arrival, and the stock UDP
    /// asset fixtures ([`default_udp_assets`]).
    fn default() -> Self {
        Self {
            setup: Arc::new(default_setup),
            on_agent_arrived: Some(Arc::new(default_arrival)),
            on_event: None,
            assets: sl_proto::InMemoryAssetSource::new(),
            udp_assets: default_udp_assets(),
        }
    }
}

/// The stock agent inventory root folder id.
const AGENT_ROOT: u128 = 0xFA01;
/// The stock "Clothing" folder under the agent root.
const AGENT_CLOTHING: u128 = 0xFA02;
/// The stock "Party Hat" item inside "Clothing".
const AGENT_HAT: u128 = 0xFA11;
/// The stock library root folder id.
const LIB_ROOT: u128 = 0xFB01;
/// The stock "Library Texture" item inside the library root.
const LIB_TEXTURE: u128 = 0xFB11;
/// The stock region-wide parcel id.
const PARCEL: u128 = 0xFC01;
/// The stock creator/owner agent id used for fixture items.
const FIXTURE_CREATOR: u128 = 0xFD01;
/// The stock scripted object's full key.
const SCRIPTED_OBJECT: u128 = 0xFE01;
/// The stock scripted object's region-local id.
pub const STOCK_SCRIPTED_OBJECT_LOCAL_ID: RegionLocalObjectId = RegionLocalObjectId(0xF0);
/// The stock script item inside the scripted object.
const SCRIPT_ITEM: u128 = 0xFE11;
/// The stock script item's asset id.
const SCRIPT_ASSET: u128 = 0xFE21;
/// The stock task-inventory serial.
const SCRIPTED_OBJECT_SERIAL: i16 = 1;
/// The stock named `Xfer` file.
pub const STOCK_XFER_FILE: &str = "motd.txt";
/// The stock named `Xfer` file's contents.
pub const STOCK_XFER_FILE_BODY: &[u8] = b"Welcome to the fake grid.\n";
/// The stock script item's source text.
pub const STOCK_SCRIPT_BODY: &[u8] =
    b"default\n{\n    state_entry()\n    {\n        llSay(0, \"Hello, fake grid!\");\n    }\n}\n";
/// The stock estate covenant notecard body.
pub const STOCK_COVENANT_BODY: &[u8] = b"Fake grid covenant: be excellent to each other.\n";
/// The stock flat terrain height, in metres.
pub const STOCK_TERRAIN_HEIGHT_M: u8 = 25;

/// The stock scripted object's key.
#[must_use]
pub fn stock_scripted_object() -> ObjectKey {
    ObjectKey::from(uuid::Uuid::from_u128(SCRIPTED_OBJECT))
}

/// The stock script item (the one entry of the scripted object's task
/// inventory, whose source is [`STOCK_SCRIPT_BODY`]).
#[must_use]
pub fn stock_script_item() -> TaskInventoryItem {
    let creator = AgentKey::from(uuid::Uuid::from_u128(FIXTURE_CREATOR));
    TaskInventoryItem {
        item_id: InventoryKey::from(uuid::Uuid::from_u128(SCRIPT_ITEM)),
        parent_task: stock_scripted_object(),
        permissions: Permissions5 {
            base: Permissions::from_bits(0x7fff_ffff),
            owner: Permissions::from_bits(0x7fff_ffff),
            group: Permissions::from_bits(0),
            everyone: Permissions::from_bits(0),
            next_owner: Permissions::from_bits(0x0008_e000),
        },
        creator_id: creator,
        last_owner_id: creator,
        owner: OwnerKey::Agent(creator),
        group: None,
        group_owned: false,
        asset_id: Some(AssetKey::from(uuid::Uuid::from_u128(SCRIPT_ASSET))),
        asset_type: AssetType::ScriptText,
        inv_type: InventoryType::Script,
        flags: 0,
        sale_type: SaleType::NotForSale,
        sale_price: LindenAmount(0),
        name: "Hello Script".to_owned(),
        description: String::new(),
        creation_date: 1_700_000_000,
    }
}

/// The stock UDP asset fixtures: the `motd.txt` `Xfer` file, one scripted
/// object ([`STOCK_SCRIPTED_OBJECT_LOCAL_ID`]) whose task inventory holds
/// [`stock_script_item`] with [`STOCK_SCRIPT_BODY`] as its asset, the
/// covenant notecard, and a flat terrain RAW heightmap.
#[must_use]
pub fn default_udp_assets() -> UdpAssetFixtures {
    let script = stock_script_item();
    UdpAssetFixtures::new()
        .with_xfer_file(STOCK_XFER_FILE, STOCK_XFER_FILE_BODY)
        .with_task_item_asset(stock_scripted_object(), script.item_id, STOCK_SCRIPT_BODY)
        .with_task_inventory(
            STOCK_SCRIPTED_OBJECT_LOCAL_ID,
            TaskInventoryFixture {
                task: stock_scripted_object(),
                serial: SCRIPTED_OBJECT_SERIAL,
                items: vec![script],
            },
        )
        .with_estate_covenant(STOCK_COVENANT_BODY)
        .with_terrain_raw(flat_terrain_raw(STOCK_TERRAIN_HEIGHT_M))
}

/// An [`InventoryFolderKey`] from a small fixture constant.
fn folder_key(id: u128) -> InventoryFolderKey {
    InventoryFolderKey::from(uuid::Uuid::from_u128(id))
}

/// A minimal deterministic inventory item for the stock fixtures.
fn stock_item(id: u128, folder: InventoryFolderKey, name: &str) -> InventoryItem {
    InventoryItem {
        item_id: InventoryKey::from(uuid::Uuid::from_u128(id)),
        folder_id: folder,
        name: name.to_owned(),
        description: String::new(),
        asset_id: uuid::Uuid::from_u128(id.wrapping_add(0x1000)),
        item_type: 0,
        inv_type: 0,
        flags: 0,
        sale_type: 0,
        sale_price: None,
        creation_date: 0,
        owner: OwnerKey::Agent(AgentKey::from(uuid::Uuid::from_u128(FIXTURE_CREATOR))),
        last_owner_id: uuid::Uuid::nil(),
        creator_id: AgentKey::from(uuid::Uuid::from_u128(FIXTURE_CREATOR)),
        group: None,
        permissions: sl_proto::Permissions5::default(),
    }
}

/// The region-local id of the stock region-wide parcel.
pub const STOCK_PARCEL_LOCAL_ID: RegionLocalParcelId = RegionLocalParcelId(1);

/// Seeds the stock fixtures on a fresh session: agent inventory (root →
/// Clothing → Party Hat), a library, one region-wide parcel, and WebRTC
/// voice — the stub answerer ([`WebRtcStub::default`]) plus the parcel's
/// estate-wide voice channel (its `channel_uri` is the region id, as
/// Second Life sends it), with the agent standing on that parcel. The
/// runtime advertises the backend from this (`SimulatorFeatures
/// .VoiceServerType`, the login `voice-config`, the arrival
/// `RequiredVoiceVersion` push).
fn default_setup(sim: &mut SimSession) {
    sim.agent_inventory_mut().insert_folder(InventoryFolder {
        folder_id: folder_key(AGENT_ROOT),
        parent_id: None,
        name: "My Inventory".to_owned(),
        folder_type: 8,
        version: 1,
    });
    sim.agent_inventory_mut().insert_folder(InventoryFolder {
        folder_id: folder_key(AGENT_CLOTHING),
        parent_id: Some(folder_key(AGENT_ROOT)),
        name: "Clothing".to_owned(),
        folder_type: 5,
        version: 1,
    });
    sim.agent_inventory_mut().insert_item(stock_item(
        AGENT_HAT,
        folder_key(AGENT_CLOTHING),
        "Party Hat",
    ));
    sim.library_inventory_mut().insert_folder(InventoryFolder {
        folder_id: folder_key(LIB_ROOT),
        parent_id: None,
        name: "Library".to_owned(),
        folder_type: 8,
        version: 1,
    });
    sim.library_inventory_mut().insert_item(stock_item(
        LIB_TEXTURE,
        folder_key(LIB_ROOT),
        "Library Texture",
    ));
    sim.add_parcel(sl_proto::SimParcel {
        parcel_id: ParcelKey::from(uuid::Uuid::from_u128(PARCEL)),
        west: 0.0,
        south: 0.0,
        east: 256.0,
        north: 256.0,
    });
    let region_id = sim.region_id();
    sim.voice_mut().enable_webrtc(WebRtcStub::default());
    sim.voice_mut().set_parcel_voice_info(ParcelVoiceInfo {
        parcel_local_id: STOCK_PARCEL_LOCAL_ID,
        region_name: None,
        channel_uri: Some(VoiceChannelUri::Id(region_id)),
        channel_credentials: None,
    });
    sim.voice_mut()
        .set_agent_parcel(Some(STOCK_PARCEL_LOCAL_ID));
}

/// Greets the arriving avatar with a system chat line.
fn default_arrival(sim: &mut SimSession) {
    let position = sl_types::lsl::Vector {
        x: 128.0,
        y: 128.0,
        z: 25.0,
    };
    if let Err(error) = sim.send_chat_from_simulator(
        "Fake Grid",
        ChatSource::System,
        uuid::Uuid::nil(),
        ChatType::Normal,
        1,
        position,
        "Welcome to the fake grid.",
        std::time::Instant::now(),
    ) {
        tracing::warn!("arrival greeting failed: {error}");
    }
}
