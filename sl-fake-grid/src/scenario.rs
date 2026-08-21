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
//! (`GetTexture`, `GetMesh`, `ViewerAsset`).

use std::sync::Arc;

use sl_proto::{ChatSource, ChatType, InventoryFolder, InventoryItem, SimSession};
use sl_types::key::{AgentKey, InventoryFolderKey, InventoryKey, OwnerKey, ParcelKey};

/// A hook run under the session lock against the machine (fixture setup,
/// on-arrival content pushes).
pub type SimHook = Arc<dyn Fn(&mut SimSession) + Send + Sync>;

/// The scripted content for one region.
#[derive(Clone)]
pub struct Scenario {
    /// Seeds a fresh session's fixture stores before login completes.
    pub setup: SimHook,
    /// Runs after the automatic region handshake when the agent arrives.
    pub on_agent_arrived: Option<SimHook>,
    /// The binary assets served by the asset-delivery caps.
    pub assets: sl_proto::InMemoryAssetSource,
}

impl std::fmt::Debug for Scenario {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Scenario")
            .field("setup", &"<closure>")
            .field(
                "on_agent_arrived",
                &self.on_agent_arrived.as_ref().map(|_| "<closure>"),
            )
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
            assets: sl_proto::InMemoryAssetSource::new(),
        }
    }
}

impl Default for Scenario {
    /// The stock scenario: a small standard inventory and library, one
    /// region-wide parcel, and a chat greeting on arrival.
    fn default() -> Self {
        Self {
            setup: Arc::new(default_setup),
            on_agent_arrived: Some(Arc::new(default_arrival)),
            assets: sl_proto::InMemoryAssetSource::new(),
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

/// Seeds the stock fixtures on a fresh session: agent inventory (root →
/// Clothing → Party Hat), a library, and one region-wide parcel.
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
