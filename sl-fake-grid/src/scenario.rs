//! Scriptable content: what a region's sessions are seeded with and what
//! greets an arriving avatar.
//!
//! A [`Scenario`] is the fake grid's whole content model — everything else
//! in this crate is protocol glue. The `setup` closure runs against a fresh
//! [`SimSession`] before the login response is built, populating the fixture
//! stores the CAPS and UDP surfaces serve (inventory, parcels, simulator
//! features, display names, …); the `on_agent_arrived` closure runs right
//! after the agent's movement completes and the arrival world burst went
//! out (content pushed at the arriving client); the asset store backs the binary asset-delivery caps
//! (`GetTexture`, `GetMesh`, `ViewerAsset`); the [`UdpAssetFixtures`] back
//! the legacy UDP asset paths (`Xfer`, `Transfer`, task inventory, the
//! estate terrain RAW file); the [`SceneFixtures`] are the parcels and
//! objects pushed at an arriving agent and replayed on request; the
//! `on_event` hook sees every drained [`ServerEvent`] for behaviour the
//! stock fixtures do not cover.

use std::sync::Arc;
use std::time::Instant;

use sl_proto::{
    AssetKey, AssetType, ChatSource, ChatType, InventoryFolder, InventoryItem, InventoryType,
    LindenAmount, ParcelVoiceInfo, Permissions, Permissions5, RegionLocalObjectId,
    RegionLocalParcelId, SaleType, ServerEvent, SimSession, TaskInventoryItem, VoiceChannelUri,
    WebRtcStub,
};
use sl_types::key::{AgentKey, InventoryFolderKey, InventoryKey, ObjectKey, OwnerKey, ParcelKey};

use crate::udp_assets::{TaskInventoryFixture, UdpAssetFixtures};
use crate::world::{SceneFixtures, box_prim, region_wide_parcel};

/// A hook run under the session lock against the machine (fixture setup,
/// on-arrival content pushes), stamped with the grid's clock
/// ([`crate::time::Now`]) so a hook that sends never has to reach for
/// [`Instant::now`] itself.
pub type SimHook = Arc<dyn Fn(&mut SimSession, Instant) + Send + Sync>;

/// A hook run under the session lock for every drained [`ServerEvent`],
/// after the stock fixture behaviour answered it, with the same stamp the
/// stock behaviour used.
pub type SimEventHook = Arc<dyn Fn(&mut SimSession, &ServerEvent, Instant) + Send + Sync>;

/// The scripted content for one region.
#[derive(Clone)]
pub struct Scenario {
    /// Seeds a fresh session's fixture stores before login completes.
    pub setup: SimHook,
    /// Runs after the arrival world burst when the agent's movement completes.
    pub on_agent_arrived: Option<SimHook>,
    /// Runs for every drained [`ServerEvent`], after the stock behaviour.
    pub on_event: Option<SimEventHook>,
    /// The binary assets this region's content references, folded into the
    /// **grid-wide** store when the grid starts (`assets.rs`).
    ///
    /// A region states what its own content needs; it does not own a store.
    /// An asset id names a blob the whole grid knows, and a viewer fetches
    /// every one of them over its root region's capability — including the
    /// textures of the neighbour it can see across a border.
    pub assets: sl_proto::InMemoryAssetSource,
    /// The content behind the legacy UDP asset paths.
    pub udp_assets: UdpAssetFixtures,
    /// The parcels and objects of the region (pushed on arrival, replayed on
    /// request).
    pub world: SceneFixtures,
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
            .field("world", &self.world)
            .finish_non_exhaustive()
    }
}

impl Scenario {
    /// An empty scenario: no fixtures, no arrival content, no assets.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            setup: Arc::new(|_, _| {}),
            on_agent_arrived: None,
            on_event: None,
            assets: sl_proto::InMemoryAssetSource::new(),
            udp_assets: UdpAssetFixtures::new(),
            world: SceneFixtures::new(),
        }
    }
}

impl Default for Scenario {
    /// The stock scenario: a small standard inventory and library, one
    /// region-wide parcel, a chat greeting on arrival, the stock assets
    /// ([`default_assets`]), the stock UDP asset fixtures
    /// ([`default_udp_assets`]), and the stock world ([`default_world`]: the
    /// region-wide parcel's record and the scripted object as a visible box).
    fn default() -> Self {
        Self {
            setup: Arc::new(default_setup),
            on_agent_arrived: Some(Arc::new(default_arrival)),
            on_event: None,
            assets: default_assets(),
            udp_assets: default_udp_assets(),
            world: default_world(),
        }
    }
}

/// The stock asset store: the **library** textures a viewer asks any grid for
/// before it has been told about a single fixture — one JPEG2000 solid per
/// default Linden terrain detail texture, and one stand-in per built-in sky,
/// water and prim texture
/// ([`sl_test_assets::builtin::library_textures`]).
///
/// A fake grid is a grid *with a library*, so answering a Linden library id
/// under its real UUID is honest — and not answering is expensive: each of the
/// twelve is otherwise a fetch that burns its whole retry budget on every
/// arrival, and the ground shades flat, the sky has no sun in it, and every
/// untextured fixture prim is a hole.
///
/// A texture that cannot be encoded is simply not registered (none of these
/// can fail: they are all small, non-empty and four-component).
#[must_use]
pub fn default_assets() -> sl_proto::InMemoryAssetSource {
    let mut assets = sl_proto::InMemoryAssetSource::new();
    match sl_test_assets::terrain_detail_solids() {
        Ok(solids) => {
            for (id, j2c) in solids {
                let _previous = assets.insert(AssetKey::from(id), j2c);
            }
        }
        Err(error) => tracing::warn!("encoding the terrain detail textures failed: {error}"),
    }
    match sl_test_assets::builtin::library_textures() {
        Ok(builtins) => {
            for (id, j2c) in builtins {
                let _previous = assets.insert(AssetKey::from(id), j2c);
            }
        }
        Err(error) => tracing::warn!("encoding the built-in library textures failed: {error}"),
    }
    assets
}

/// The stock agent inventory root folder id.
const AGENT_ROOT: u128 = 0xFA01;
/// The stock "Clothing" folder under the agent root.
const AGENT_CLOTHING: u128 = 0xFA02;
/// The standard system folders every account has, as `(folder type, name)`.
///
/// A real grid creates these when the account is made, and the reference
/// viewer requires them: `LLInventoryModel::validate` calls the ten
/// asset-type folders (textures, sounds, objects, notecards, scripts, body
/// parts, photo album, lost and found, animations, gestures) **fatal** when
/// absent, because the viewer cannot create those itself — only the grid can.
/// The remainder it will create over AIS if missing, but seeding them keeps a
/// fresh login from generating a burst of folder-creation traffic that has
/// nothing to do with what a test is looking at.
///
/// Ids are `AGENT_SYSTEM_FOLDER_BASE + folder type`, so each is stable and
/// derivable rather than another hand-maintained constant per folder.
///
/// Types are `LLFolderType::EType` (`indra/llinventory/llfoldertype.h`).
const AGENT_SYSTEM_FOLDERS: &[(i8, &str)] = &[
    (0, "Textures"),
    (1, "Sounds"),
    (2, "Calling Cards"),
    (3, "Landmarks"),
    (6, "Objects"),
    (7, "Notecards"),
    (10, "Scripts"),
    (13, "Body Parts"),
    (14, "Trash"),
    (15, "Photo Album"),
    (16, "Lost And Found"),
    (20, "Animations"),
    (21, "Gestures"),
    (23, "Favorites"),
    (46, "Current Outfit"),
    (48, "My Outfits"),
    (50, "Received Items"),
    (56, "Settings"),
    (57, "Materials"),
];

/// The id base for the [`AGENT_SYSTEM_FOLDERS`]; each folder's id is this plus
/// its folder type. Chosen clear of the other `0xFAxx` fixture ids.
const AGENT_SYSTEM_FOLDER_BASE: u128 = 0xFA80;
/// The stock "Party Hat" item inside "Clothing".
const AGENT_HAT: u128 = 0xFA11;

/// The four body-part wearables the stock account wears, as
/// `(wearable type, name, asset id)`.
///
/// The reference viewer will not de-cloud its **own** avatar until the agent
/// wears all four: `LLVOAvatarSelf::getHasMissingParts` counts SHAPE, SKIN,
/// HAIR and EYES and logs "Self is clouded due to missing one or more required
/// body parts" when any is absent. That gate is independent of the bakes — a
/// grid can push a perfectly good `AvatarAppearance`, and the avatar still
/// stays a cloud without these.
///
/// The ids are Linden **library** assets, which every viewer ships in its
/// `app_settings/static_assets` and pre-loads into its cache. Naming them costs
/// the grid no asset to serve: the viewer resolves each locally and never asks.
/// (They are the same files this workspace vendors under
/// `viewer-assets/static_assets/`, so its own viewer resolves them the same
/// way.)
///
/// Wearable types are `LLWearableType::EType`; the ids are the `type` field of
/// the corresponding `.bodypart` asset.
const AGENT_BODY_PARTS: &[(i8, &str, u128)] = &[
    // RASL F LEARN SHAPE (ANNA)
    (0, "Shape", 0x57cb_d4f1_c53e_020f_f455_5ad2_a5ba_b98d),
    // RASL F EXPLORE SKIN (SOFIA)
    (1, "Skin", 0x205a_e4a8_42c6_1c5c_b142_6728_64fa_fe8a),
    // RASL M LEARN EYEBROWSHAPER
    (2, "Hair", 0x51f3_b303_a783_f0bd_9e98_9c09_4be1_3653),
    // New Eyes
    (3, "Eyes", 0x1497_39c0_f677_4b4c_1587_e44b_e72d_d7ef),
];

/// The id base for the body-part inventory items; each item's id is this plus
/// its wearable type.
const AGENT_BODY_PART_ITEM_BASE: u128 = 0xFA_C000;

/// The id base for the Current Outfit Folder links to those items.
const AGENT_COF_LINK_BASE: u128 = 0xFA_D000;

/// `AT_BODYPART`: the asset and inventory type a body-part wearable carries.
const ASSET_TYPE_BODYPART: i8 = 13;

/// `AT_LINK`: the type of a Current Outfit Folder link. A link's `asset_id` is
/// the **item** it points at, not an asset.
const ASSET_TYPE_LINK: i8 = 24;

/// `FT_BODYPART`, the folder the wearables themselves live in.
const FOLDER_TYPE_BODY_PARTS: i8 = 13;

/// `FT_CURRENT_OUTFIT`, the folder whose links say what is being worn.
const FOLDER_TYPE_CURRENT_OUTFIT: i8 = 46;
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
/// [`stock_script_item`] with [`STOCK_SCRIPT_BODY`] as its asset, and the
/// covenant notecard.
///
/// No terrain RAW heightmap: a session whose scenario names none serves the
/// region's own ground
/// ([`RegionConfig::terrain`](crate::RegionConfig::terrain)), so the estate
/// download matches what the viewer is standing on. Name one
/// ([`UdpAssetFixtures::with_terrain_raw`], e.g. from
/// [`flat_terrain_raw`](crate::flat_terrain_raw)) to serve a heightmap that
/// deliberately differs.
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
}

/// The stock parcel's name.
pub const STOCK_PARCEL_NAME: &str = "Fake Grid Parcel";
/// The stock scripted object's region-local position (a box resting on
/// the flat stock terrain, a few metres from the arrival point).
pub const STOCK_SCRIPTED_OBJECT_POSITION: sl_types::lsl::Vector = sl_types::lsl::Vector {
    x: 132.0,
    y: 128.0,
    z: 25.5,
};

/// The stock world: one region-wide public parcel owned by the fixture
/// creator ([`STOCK_PARCEL_LOCAL_ID`], [`STOCK_PARCEL_NAME`]) — the same
/// parcel the stock setup registers for `RemoteParcelRequest` and voice —
/// and the stock scripted object ([`stock_scripted_object`],
/// [`STOCK_SCRIPTED_OBJECT_LOCAL_ID`]) rezzed as a 1 m box at
/// [`STOCK_SCRIPTED_OBJECT_POSITION`], so the task-inventory fixtures
/// describe an object the viewer can actually see and click.
#[must_use]
pub fn default_world() -> SceneFixtures {
    let creator = AgentKey::from(uuid::Uuid::from_u128(FIXTURE_CREATOR));
    let mut world = SceneFixtures::new();
    world.parcels.push(region_wide_parcel(
        STOCK_PARCEL_LOCAL_ID,
        OwnerKey::Agent(creator),
        STOCK_PARCEL_NAME,
    ));
    world.objects.push(box_prim(
        STOCK_SCRIPTED_OBJECT_LOCAL_ID,
        stock_scripted_object(),
        creator,
        STOCK_SCRIPTED_OBJECT_POSITION,
        sl_types::lsl::Vector {
            x: 1.0,
            y: 1.0,
            z: 1.0,
        },
    ));
    world
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
pub(crate) fn default_setup(sim: &mut SimSession, _now: Instant) {
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
    // The standard system folders. Without them the reference viewer's
    // inventory validation fails with ten fatal errors and it dies in
    // STATE_INVENTORY_CALLBACKS -- see AGENT_SYSTEM_FOLDERS.
    for (folder_type, name) in AGENT_SYSTEM_FOLDERS {
        sim.agent_inventory_mut().insert_folder(InventoryFolder {
            folder_id: folder_key(
                AGENT_SYSTEM_FOLDER_BASE.saturating_add(u128::try_from(*folder_type).unwrap_or(0)),
            ),
            parent_id: Some(folder_key(AGENT_ROOT)),
            name: (*name).to_owned(),
            folder_type: *folder_type,
            version: 1,
        });
    }

    // The worn body parts, and the Current Outfit Folder links that say they
    // are worn. Both halves are needed: the items alone leave the wearables in
    // inventory but not on the avatar, and the viewer counts what the COF links
    // resolve to. Without them the agent's own avatar never de-clouds -- see
    // AGENT_BODY_PARTS.
    let body_parts_folder = folder_key(
        AGENT_SYSTEM_FOLDER_BASE
            .saturating_add(u128::try_from(FOLDER_TYPE_BODY_PARTS).unwrap_or(0)),
    );
    let cof_folder = folder_key(
        AGENT_SYSTEM_FOLDER_BASE
            .saturating_add(u128::try_from(FOLDER_TYPE_CURRENT_OUTFIT).unwrap_or(0)),
    );
    for (wearable_type, name, asset) in AGENT_BODY_PARTS {
        let offset = u128::try_from(*wearable_type).unwrap_or(0);
        let item_id = InventoryKey::from(uuid::Uuid::from_u128(
            AGENT_BODY_PART_ITEM_BASE.saturating_add(offset),
        ));

        // The wearable itself, in Body Parts.
        let mut item = stock_item(0, body_parts_folder, name);
        item.item_id = item_id;
        item.asset_id = uuid::Uuid::from_u128(*asset);
        item.item_type = ASSET_TYPE_BODYPART;
        item.inv_type = ASSET_TYPE_BODYPART;
        // `flags` carries the wearable type for a body part, which is how the
        // viewer knows which slot an item fills before fetching its asset.
        item.flags = u32::try_from(*wearable_type).unwrap_or(0);
        sim.agent_inventory_mut().insert_item(item);

        // The COF link naming it. A link's asset_id is the *item* it points at.
        let mut link = stock_item(0, cof_folder, name);
        link.item_id = InventoryKey::from(uuid::Uuid::from_u128(
            AGENT_COF_LINK_BASE.saturating_add(offset),
        ));
        link.asset_id = item_id.uuid();
        link.item_type = ASSET_TYPE_LINK;
        link.inv_type = ASSET_TYPE_LINK;
        sim.agent_inventory_mut().insert_item(link);
    }

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
fn default_arrival(sim: &mut SimSession, now: Instant) {
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
        now,
    ) {
        tracing::warn!("arrival greeting failed: {error}");
    }
}

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn the_stock_assets_hold_every_default_detail_texture() {
        let assets = default_assets();
        for id in sl_proto::DEFAULT_TERRAIN_DETAIL_TEXTURES {
            assert!(
                assets.contains(AssetKey::from(id)),
                "no asset registered for detail texture {id}"
            );
        }
    }

    /// Every library texture a viewer falls back to is answered, so an arrival
    /// costs no failed fetch — the sky's sun and moon in particular, which are
    /// discs a viewer draws and no viewer ships.
    #[test]
    fn the_stock_assets_hold_every_builtin_library_texture() {
        let assets = default_assets();
        for id in sl_proto::BUILTIN_ENVIRONMENT_TEXTURES {
            assert!(
                assets.contains(AssetKey::from(id)),
                "no asset registered for built-in texture {id}"
            );
        }
        assert!(
            assets.contains(AssetKey::from(sl_proto::DEFAULT_PRIM_TEXTURE)),
            "no asset registered for the blank-plywood prim texture"
        );
        // The sets a viewer fetches unconditionally on arrival, whether or not
        // anything in the region uses them.
        for id in sl_proto::BUILTIN_BUMPMAP_TEXTURES {
            assert!(
                assets.contains(AssetKey::from(id)),
                "no asset registered for standard bump map {id}"
            );
        }
        for id in sl_proto::BUILTIN_VIEWER_TEXTURES {
            assert!(
                assets.contains(AssetKey::from(id)),
                "no asset registered for viewer texture {id}"
            );
        }
        for id in sl_proto::BUILTIN_WATER_PLANE_TEXTURES {
            assert!(
                assets.contains(AssetKey::from(id)),
                "no asset registered for water plane texture {id}"
            );
        }
        for id in [
            sl_proto::avatar_texture::IMG_DEFAULT_AVATAR,
            sl_proto::avatar_texture::IMG_INVISIBLE,
        ] {
            assert!(
                assets.contains(AssetKey::from(id)),
                "no asset registered for avatar sentinel {id}"
            );
        }
        for (id, _layer) in sl_proto::avatar_texture::WEARABLE_LAYER_TEXTURES {
            assert!(
                assets.contains(AssetKey::from(id)),
                "no asset registered for wearable layer texture {id}"
            );
        }
        // Four terrain solids, seven environment textures, the prim texture,
        // two avatar sentinels, fifteen bump maps, two viewer textures, two
        // water plane textures and five wearable layer textures — and nothing
        // else: a stock scenario's store is the library, not a fixture dump.
        assert_eq!(assets.len(), 4 + 7 + 1 + 2 + 15 + 2 + 2 + 5);
    }

    #[test]
    fn the_stock_udp_fixtures_name_no_heightmap() {
        // The region's own ground fills this in per session, so the estate
        // download matches what the viewer stands on.
        assert_eq!(default_udp_assets().terrain_raw, None);
    }
}
