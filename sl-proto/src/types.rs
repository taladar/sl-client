//! Public value types of the sans-I/O session: its inputs and outputs.

use std::net::SocketAddr;

use sl_wire::LoginRequest;
use uuid::Uuid;

/// The parameters needed to start a session: where to log in and with what.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginParams {
    /// The XML-RPC login endpoint URL (e.g. `http://127.0.0.1:9000/`).
    pub login_uri: String,
    /// The login request to send.
    pub request: LoginRequest,
}

/// An HTTP request the driver must perform on the session's behalf: POST `body`
/// to `url` and feed the response back via
/// [`Session::handle_login_response`](crate::Session::handle_login_response).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginHttpRequest {
    /// The URL to POST to.
    pub url: String,
    /// The XML-RPC request body.
    pub body: String,
    /// The `User-Agent` header to send, identifying the viewer by its channel
    /// and version (see [`LoginRequest::user_agent`](sl_wire::LoginRequest::user_agent)).
    pub user_agent: String,
}

/// How an outgoing message should be delivered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reliability {
    /// Send once, best-effort.
    Unreliable,
    /// Send reliably: track acknowledgement and retransmit until acked.
    Reliable,
}

/// A datagram ready to be sent on the wire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transmit {
    /// Where to send the datagram.
    pub destination: SocketAddr,
    /// The datagram bytes.
    pub payload: Vec<u8>,
}

/// Why a session became disconnected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DisconnectReason {
    /// The login server rejected the credentials.
    LoginFailed {
        /// The machine-readable reason code.
        reason: String,
        /// The human-readable message.
        message: String,
    },
    /// No traffic was received within the inactivity budget.
    Timeout,
    /// A reliable handshake packet exhausted its retransmissions.
    HandshakeFailed,
    /// An unrecoverable wire-protocol error occurred.
    ProtocolError,
}

/// A high-level event surfaced to the driver/application.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// The UDP circuit to the simulator has been opened and the bootstrap
    /// packets queued.
    CircuitEstablished {
        /// The simulator's UDP address.
        sim: SocketAddr,
    },
    /// The region handshake completed; the session is now fully active.
    RegionHandshakeComplete,
    /// The current region's identity, maturity, and product type, parsed from
    /// the `RegionHandshake` (emitted alongside [`Event::RegionHandshakeComplete`]
    /// on entry, and [`Event::RegionChanged`] after a teleport).
    RegionInfoHandshake(Box<RegionIdentity>),
    /// The current region's agent and object limits, parsed from a `RegionInfo`
    /// reply to [`Session::request_region_info`](crate::Session::request_region_info).
    RegionLimits(RegionLimits),
    /// A parcel's geometry, flags, and limits, parsed from a `ParcelProperties`
    /// reply to
    /// [`Session::request_parcel_properties`](crate::Session::request_parcel_properties).
    ParcelProperties(Box<ParcelInfo>),
    /// A region parcel-ownership overlay chunk (one of four), parsed from a
    /// `ParcelOverlay`.
    ParcelOverlay(ParcelOverlayInfo),
    /// A neighbouring simulator was announced via `EnableSimulator`.
    NeighborDiscovered(NeighborInfo),
    /// A region was reported by the world map (a `MapBlockReply` entry), giving
    /// its name and grid coordinates. Sent in response to
    /// [`Session::request_map_blocks`](crate::Session::request_map_blocks).
    MapBlock(Box<MapRegionInfo>),
    /// A teleport has begun (`TeleportStart`).
    TeleportStarted,
    /// A progress update during a teleport (`TeleportProgress`).
    TeleportProgress {
        /// The human-readable progress message.
        message: String,
        /// The teleport flags bitfield.
        teleport_flags: u32,
    },
    /// An intra-region teleport completed (`TeleportLocal`); the circuit did not
    /// change, so no [`Event::RegionChanged`] follows.
    TeleportLocal,
    /// A teleport failed (`TeleportFailed` or a teleport timeout); the session
    /// remains connected to the current region.
    TeleportFailed {
        /// The failure reason.
        reason: String,
    },
    /// A teleport handover completed: the destination region's handshake
    /// arrived and the circuit is now active there.
    RegionChanged {
        /// The destination region handle.
        region_handle: u64,
        /// The destination simulator's UDP address.
        sim: SocketAddr,
    },
    /// The session logged out cleanly (a `LogoutReply` was received).
    LoggedOut,
    /// The session disconnected for the given reason.
    Disconnected(DisconnectReason),
}

/// A region maturity / content rating, from the `SimAccess` byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Maturity {
    /// General ("PG") content.
    Pg,
    /// Moderate ("Mature") content.
    Mature,
    /// Adult content.
    Adult,
    /// Unknown or unrated (the grid did not provide a recognised value).
    Unknown,
}

impl Maturity {
    /// Classifies the `SimAccess` byte from a handshake/region/teleport message.
    #[must_use]
    pub const fn from_sim_access(sim_access: u8) -> Self {
        match sim_access {
            sl_wire::sim_access::PG => Self::Pg,
            sl_wire::sim_access::MATURE => Self::Mature,
            sl_wire::sim_access::ADULT => Self::Adult,
            _ => Self::Unknown,
        }
    }
}

/// A region product type, inferred from the `ProductSKU`/`ProductName` strings.
/// OpenSim grids usually leave these empty, yielding [`ProductType::Unknown`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProductType {
    /// A full ("Estate" / "Standalone") region.
    FullRegion,
    /// A homestead region.
    Homestead,
    /// An openspace ("void") region.
    Openspace,
    /// Unknown / unrecognised (commonly OpenSim, which omits the fields).
    Unknown,
}

impl ProductType {
    /// Classifies a region from its `ProductSKU` and `ProductName` strings.
    #[must_use]
    pub fn classify(product_sku: &str, product_name: &str) -> Self {
        let haystack = format!("{product_sku} {product_name}").to_lowercase();
        if haystack.contains("homestead") {
            Self::Homestead
        } else if haystack.contains("openspace") || haystack.contains("open space") {
            Self::Openspace
        } else if haystack.contains("estate")
            || haystack.contains("full")
            || haystack.contains("standalone")
        {
            Self::FullRegion
        } else {
            Self::Unknown
        }
    }
}

/// A region's identity, maturity, and product type, parsed from `RegionHandshake`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegionIdentity {
    /// The region (simulator) name.
    pub sim_name: String,
    /// The raw `RegionFlags` bitfield (decode with [`sl_wire::RegionFlags`]).
    pub region_flags: u32,
    /// The maturity / content rating.
    pub maturity: Maturity,
    /// The inferred product type.
    pub product: ProductType,
    /// The raw `ProductSKU` string (possibly empty, e.g. on OpenSim).
    pub product_sku: String,
    /// The raw `ProductName` string (possibly empty, e.g. on OpenSim).
    pub product_name: String,
}

/// A region's agent and object capacity, parsed from `RegionInfo`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegionLimits {
    /// The region (simulator) name.
    pub sim_name: String,
    /// The maximum concurrent agents (prefers the 32-bit field, falling back to
    /// the legacy 8-bit `MaxAgents`).
    pub max_agents: u32,
    /// The hard agent cap, or `0` if the grid did not provide it (common for
    /// non-estate-managers on Second Life, and on OpenSim).
    pub hard_max_agents: u32,
    /// The hard region-wide object/prim cap, or `0` if not provided.
    pub hard_max_objects: u32,
    /// The raw `RegionFlags` bitfield (decode with [`sl_wire::RegionFlags`]).
    pub region_flags: u32,
    /// The maturity / content rating.
    pub maturity: Maturity,
}

/// A parcel's geometry, flags, and limits, parsed from `ParcelProperties`.
///
/// The parcel flag bits are exposed through the boolean accessor methods
/// ([`ParcelInfo::create_objects`], [`ParcelInfo::use_ban_list`], …); the raw
/// bitfield is available via [`ParcelInfo::flags`] / [`ParcelInfo::raw_parcel_flags`].
#[derive(Debug, Clone, PartialEq)]
pub struct ParcelInfo {
    /// The request sequence id echoed back (used to match an outstanding query).
    pub sequence_id: i32,
    /// The parcel's region-local id.
    pub local_id: i32,
    /// The minimum corner of the parcel's axis-aligned bounding box, in metres.
    pub aabb_min: (f32, f32, f32),
    /// The maximum corner of the parcel's axis-aligned bounding box, in metres.
    pub aabb_max: (f32, f32, f32),
    /// The parcel area in square metres.
    pub area: i32,
    /// One bit per 4×4 m region square, marking which squares belong to this
    /// parcel (row-major, least-significant-bit first).
    pub bitmap: Vec<u8>,
    /// The parcel's maximum object/prim capacity (without bonus).
    pub max_prims: i32,
    /// The region-wide maximum object/prim capacity.
    pub sim_wide_max_prims: i32,
    /// The region-wide current object/prim count.
    pub sim_wide_total_prims: i32,
    /// The parcel owner's id.
    pub owner_id: Uuid,
    /// The raw `ParcelFlags` bitfield (decode with [`sl_wire::ParcelFlags`]).
    pub raw_parcel_flags: u32,
}

impl ParcelInfo {
    /// The decoded parcel flag bits.
    #[must_use]
    pub const fn flags(&self) -> sl_wire::ParcelFlags {
        sl_wire::ParcelFlags::from_bits(self.raw_parcel_flags)
    }

    /// Anyone may create (rez) objects here — a public rez zone.
    #[must_use]
    pub const fn create_objects(&self) -> bool {
        self.flags().contains(sl_wire::ParcelFlags::CREATE_OBJECTS)
    }

    /// Group members may create (rez) objects here — a group rez zone.
    #[must_use]
    pub const fn create_group_objects(&self) -> bool {
        self.flags()
            .contains(sl_wire::ParcelFlags::CREATE_GROUP_OBJECTS)
    }

    /// A ban list is in effect (banlines).
    #[must_use]
    pub const fn use_ban_list(&self) -> bool {
        self.flags().contains(sl_wire::ParcelFlags::USE_BAN_LIST)
    }

    /// Access is restricted to an allow list.
    #[must_use]
    pub const fn use_access_list(&self) -> bool {
        self.flags().contains(sl_wire::ParcelFlags::USE_ACCESS_LIST)
    }

    /// Anonymous (non-account) avatars are denied access.
    #[must_use]
    pub const fn deny_anonymous(&self) -> bool {
        self.flags().contains(sl_wire::ParcelFlags::DENY_ANONYMOUS)
    }
}

/// A region parcel-ownership overlay chunk, parsed from `ParcelOverlay`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParcelOverlayInfo {
    /// Which of the four overlay chunks this is (0–3).
    pub sequence_id: i32,
    /// The packed overlay bytes: per-square ownership colour and edge/flag bits.
    pub data: Vec<u8>,
}

/// A region reported by the world map (one `MapBlockReply` `Data` entry).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapRegionInfo {
    /// The region name.
    pub name: String,
    /// The region's grid x coordinate (region index).
    pub grid_x: u32,
    /// The region's grid y coordinate (region index).
    pub grid_y: u32,
    /// The region handle (derived from the grid coordinates).
    pub region_handle: u64,
    /// The maturity rating, from the map's access byte.
    pub maturity: Maturity,
    /// The raw region flags bitfield.
    pub region_flags: u32,
    /// The region width in metres (256 for standard regions; larger for
    /// variable-sized OpenSim regions).
    pub size_x: u32,
    /// The region height in metres.
    pub size_y: u32,
    /// The number of agents the map reports in the region (often 0).
    pub agents: u8,
    /// The region's map tile image id.
    pub map_image_id: Uuid,
}

/// A neighbouring simulator announced via `EnableSimulator`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NeighborInfo {
    /// The neighbour's region handle.
    pub region_handle: u64,
    /// The neighbour's UDP address.
    pub sim: SocketAddr,
    /// The neighbour's grid x coordinate (region index, i.e. global metres / 256).
    pub grid_x: u32,
    /// The neighbour's grid y coordinate (region index, i.e. global metres / 256).
    pub grid_y: u32,
}

/// Splits a region handle into its global south-west corner in metres,
/// `(global_x, global_y)`.
#[must_use]
pub fn handle_to_global(handle: u64) -> (u32, u32) {
    let high = handle.checked_shr(32).unwrap_or(0);
    let low = handle & 0xFFFF_FFFF;
    (
        u32::try_from(high).unwrap_or(u32::MAX),
        u32::try_from(low).unwrap_or(u32::MAX),
    )
}

/// Splits a region handle into its grid coordinates (region indices), i.e. the
/// global south-west corner in metres divided by 256.
#[must_use]
pub fn handle_to_grid(handle: u64) -> (u32, u32) {
    let (global_x, global_y) = handle_to_global(handle);
    (
        global_x.checked_div(256).unwrap_or(0),
        global_y.checked_div(256).unwrap_or(0),
    )
}

/// Builds a region handle from grid coordinates (region indices).
#[must_use]
pub fn grid_to_handle(grid_x: u32, grid_y: u32) -> u64 {
    let global_x = u64::from(grid_x).checked_mul(256).unwrap_or(0);
    let global_y = u64::from(grid_y).checked_mul(256).unwrap_or(0);
    global_x.checked_shl(32).unwrap_or(0) | global_y
}
