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
    /// Local chat was received (`ChatFromSimulator`): a nearby agent or object
    /// spoke, or the region/system sent a message. Sent in response to nearby
    /// activity once the session is active. Typing-only messages are surfaced as
    /// [`Event::ChatTyping`] instead.
    ChatReceived(Box<ChatMessage>),
    /// A nearby agent started or stopped typing in local chat (a
    /// `ChatFromSimulator` with a `StartTyping`/`StopTyping` type and no text).
    ChatTyping {
        /// The typist's display name.
        from_name: String,
        /// The typist's id (agent id).
        source_id: Uuid,
        /// `true` when typing started, `false` when it stopped.
        typing: bool,
    },
    /// An instant message was received (`ImprovedInstantMessage`): a 1:1 IM, a
    /// group/conference message, an inventory/teleport/group/friendship offer, an
    /// object IM, and so on — the [`InstantMessage::dialog`] distinguishes the
    /// sub-type. Typing notifications are surfaced as [`Event::ImTyping`] instead.
    InstantMessageReceived(Box<InstantMessage>),
    /// A correspondent started or stopped typing in an instant-message session
    /// (an `ImprovedInstantMessage` with an `IM_TYPING_START`/`IM_TYPING_STOP`
    /// dialog).
    ImTyping {
        /// The typist's id (agent id).
        from_agent_id: Uuid,
        /// The typist's display name.
        from_agent_name: String,
        /// The IM session id the typing belongs to.
        session_id: Uuid,
        /// `true` when typing started, `false` when it stopped.
        typing: bool,
    },
    /// An avatar's profile properties (`AvatarPropertiesReply`), in response to
    /// [`Session::request_avatar_properties`](crate::Session::request_avatar_properties).
    AvatarProperties(Box<AvatarProperties>),
    /// An avatar's interests (`AvatarInterestsReply`), sent alongside
    /// [`Event::AvatarProperties`].
    AvatarInterests(Box<AvatarInterests>),
    /// The groups shown in an avatar's profile (`AvatarGroupsReply`), sent
    /// alongside [`Event::AvatarProperties`].
    AvatarGroups {
        /// The avatar whose groups these are.
        avatar_id: Uuid,
        /// The groups listed in the profile.
        groups: Vec<AvatarGroupMembership>,
        /// Whether the avatar lists groups in their profile.
        list_in_profile: bool,
    },
    /// An avatar's picks (`AvatarPicksReply`), in response to
    /// [`Session::request_avatar_picks`](crate::Session::request_avatar_picks).
    AvatarPicks {
        /// The avatar whose picks these are.
        target_id: Uuid,
        /// The picks (id and name only; fetch details separately).
        picks: Vec<AvatarPick>,
    },
    /// The agent's private notes about an avatar (`AvatarNotesReply`), in response
    /// to [`Session::request_avatar_notes`](crate::Session::request_avatar_notes).
    AvatarNotes {
        /// The avatar the notes are about.
        target_id: Uuid,
        /// The note text.
        notes: String,
    },
    /// The agent's inventory folder skeleton (every folder, without item
    /// contents), parsed from the login response. Emitted once, right after
    /// [`Event::CircuitEstablished`], when the login provided it.
    InventorySkeleton(Vec<InventoryFolder>),
    /// The contents of an inventory folder (`InventoryDescendents`), in response
    /// to [`Session::request_folder_contents`](crate::Session::request_folder_contents):
    /// its immediate sub-folders and items.
    InventoryDescendents {
        /// The folder whose contents these are.
        folder_id: Uuid,
        /// The folder version (for cache validation).
        version: i32,
        /// The total descendent count the simulator reports.
        descendents: i32,
        /// The immediate sub-folders.
        folders: Vec<InventoryFolder>,
        /// The items directly in the folder.
        items: Vec<InventoryItem>,
    },
    /// The agent's friends (the buddy list), parsed from the login response.
    /// Emitted once, right after [`Event::CircuitEstablished`], when the login
    /// provided a non-empty list.
    FriendList(Vec<Friend>),
    /// One or more friends came online (`OnlineNotification`). Only friends who
    /// grant this agent the see-online right are reported.
    FriendsOnline(Vec<Uuid>),
    /// One or more friends went offline (`OfflineNotification`).
    FriendsOffline(Vec<Uuid>),
    /// A friendship's rights changed (`ChangeUserRights`): either a friend
    /// changed the rights they grant this agent, or the simulator echoed a
    /// change this agent made to the rights it grants a friend (see
    /// [`granted_to_us`](Event::FriendRightsChanged::granted_to_us)).
    FriendRightsChanged {
        /// The friend the rights pertain to.
        friend_id: Uuid,
        /// The new rights bitfield.
        rights: FriendRights,
        /// `true` when these are the rights the *friend* now grants this agent;
        /// `false` when they are the rights this agent grants the friend (a
        /// server echo of this agent's own [`Session::grant_user_rights`] call).
        ///
        /// [`Session::grant_user_rights`]: crate::Session::grant_user_rights
        granted_to_us: bool,
    },
    /// The agent's active group, title, and powers changed (`AgentDataUpdate`):
    /// pushed on login and after [`Session::activate_group`](crate::Session::activate_group).
    ActiveGroupChanged(Box<ActiveGroup>),
    /// The agent's group memberships (`AgentGroupDataUpdate`), pushed on login
    /// and when membership changes.
    GroupMemberships(Vec<GroupMembership>),
    /// A group's member roster (`GroupMembersReply`), in response to
    /// [`Session::request_group_members`](crate::Session::request_group_members).
    GroupMembers {
        /// The group whose members these are.
        group_id: Uuid,
        /// The request id echoed from the request.
        request_id: Uuid,
        /// The total member count the simulator reports.
        member_count: i32,
        /// The members in this reply.
        members: Vec<GroupMember>,
    },
    /// A group's roles (`GroupRoleDataReply`), in response to
    /// [`Session::request_group_roles`](crate::Session::request_group_roles).
    GroupRoleData {
        /// The group whose roles these are.
        group_id: Uuid,
        /// The request id echoed from the request.
        request_id: Uuid,
        /// The roles.
        roles: Vec<GroupRole>,
    },
    /// A group's role↔member pairings (`GroupRoleMembersReply`), in response to
    /// [`Session::request_group_role_members`](crate::Session::request_group_role_members).
    GroupRoleMembers {
        /// The group whose pairings these are.
        group_id: Uuid,
        /// The request id echoed from the request.
        request_id: Uuid,
        /// The role↔member pairs in this reply.
        pairs: Vec<GroupRoleMember>,
    },
    /// The agent's selectable titles in a group (`GroupTitlesReply`), in response
    /// to [`Session::request_group_titles`](crate::Session::request_group_titles).
    GroupTitles {
        /// The group whose titles these are.
        group_id: Uuid,
        /// The request id echoed from the request.
        request_id: Uuid,
        /// The titles.
        titles: Vec<GroupTitle>,
    },
    /// A group's profile (`GroupProfileReply`), in response to
    /// [`Session::request_group_profile`](crate::Session::request_group_profile).
    GroupProfileReceived(Box<GroupProfile>),
    /// A group's notices (`GroupNoticesListReply`), in response to
    /// [`Session::request_group_notices`](crate::Session::request_group_notices).
    GroupNotices {
        /// The group whose notices these are.
        group_id: Uuid,
        /// The notice headers.
        notices: Vec<GroupNotice>,
    },
    /// A message was received in a group IM session (an `ImprovedInstantMessage`
    /// with `from_group` set and the `IM_SESSION_SEND` dialog). The session id is
    /// the group id.
    GroupSessionMessage {
        /// The group (and IM session) the message belongs to.
        group_id: Uuid,
        /// The sender's agent id.
        from_agent_id: Uuid,
        /// The sender's display name.
        from_name: String,
        /// The message text.
        message: String,
    },
    /// A participant joined or left a group IM session (an
    /// `ImprovedInstantMessage` with the `IM_SESSION_INVITE`/`SessionAdd` or
    /// `IM_SESSION_LEAVE`/`SessionDrop` dialog and `from_group` set).
    GroupSessionParticipant {
        /// The group (and IM session) id.
        group_id: Uuid,
        /// The participant's agent id.
        agent_id: Uuid,
        /// `true` when the participant joined, `false` when they left.
        joined: bool,
    },
    /// The result of a [`Session::create_group`](crate::Session::create_group)
    /// (`CreateGroupReply`).
    CreateGroupResult {
        /// The new group's id (nil on failure).
        group_id: Uuid,
        /// Whether creation succeeded.
        success: bool,
        /// The grid's human-readable result message.
        message: String,
    },
    /// The result of a [`Session::join_group`](crate::Session::join_group)
    /// (`JoinGroupReply`).
    JoinGroupResult {
        /// The group joined.
        group_id: Uuid,
        /// Whether the join succeeded.
        success: bool,
    },
    /// The result of a [`Session::leave_group`](crate::Session::leave_group)
    /// (`LeaveGroupReply`).
    LeaveGroupResult {
        /// The group left.
        group_id: Uuid,
        /// Whether the leave succeeded.
        success: bool,
    },
    /// The agent was removed from a group (`AgentDropGroup`) — by leaving,
    /// ejection, or the group being dissolved.
    DroppedFromGroup {
        /// The group the agent is no longer in.
        group_id: Uuid,
    },
    /// The simulator answered a sit request (`AvatarSitResponse`) after a
    /// [`Session::sit_on`](crate::Session::sit_on); the session has sent the
    /// completing `AgentSit`.
    SitResult {
        /// The object sat upon.
        sit_object: Uuid,
        /// Whether the simulator wants the viewer to autopilot (walk) to the seat
        /// first (the target was out of immediate sit range).
        autopilot: bool,
        /// The seat position relative to the object, in metres.
        sit_position: (f32, f32, f32),
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

/// The kind of a chat message, from the `Type`/`ChatType` byte shared by
/// `ChatFromViewer` (outgoing) and `ChatFromSimulator` (incoming).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatType {
    /// Whisper: a reduced (10 m) range.
    Whisper,
    /// Normal local say: the default (20 m) range.
    Normal,
    /// Shout: an extended (100 m) range.
    Shout,
    /// "Start typing" animation trigger (no text).
    StartTyping,
    /// "Stop typing" animation trigger (no text).
    StopTyping,
    /// A debug-channel message (script errors; channel `2147483647`).
    DebugChannel,
    /// A region-wide message.
    Region,
    /// A message from an object to its owner.
    Owner,
    /// A directed message to a single agent (`llRegionSayTo`).
    Direct,
    /// An unrecognised type byte, preserved verbatim.
    Unknown(u8),
}

impl ChatType {
    /// Classifies a `Type`/`ChatType` byte.
    #[must_use]
    pub const fn from_u8(byte: u8) -> Self {
        match byte {
            0 => Self::Whisper,
            1 => Self::Normal,
            2 => Self::Shout,
            4 => Self::StartTyping,
            5 => Self::StopTyping,
            6 => Self::DebugChannel,
            7 => Self::Region,
            8 => Self::Owner,
            9 => Self::Direct,
            other => Self::Unknown(other),
        }
    }

    /// The wire byte for this chat type.
    #[must_use]
    pub const fn to_u8(self) -> u8 {
        match self {
            Self::Whisper => 0,
            Self::Normal => 1,
            Self::Shout => 2,
            Self::StartTyping => 4,
            Self::StopTyping => 5,
            Self::DebugChannel => 6,
            Self::Region => 7,
            Self::Owner => 8,
            Self::Direct => 9,
            Self::Unknown(other) => other,
        }
    }
}

/// What kind of source produced a chat message, from the `SourceType` byte of
/// `ChatFromSimulator`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatSourceType {
    /// The system / region (no avatar or object).
    System,
    /// An avatar.
    Agent,
    /// An in-world object.
    Object,
    /// An unrecognised source-type byte, preserved verbatim.
    Unknown(u8),
}

impl ChatSourceType {
    /// Classifies a `SourceType` byte.
    #[must_use]
    pub const fn from_u8(byte: u8) -> Self {
        match byte {
            0 => Self::System,
            1 => Self::Agent,
            2 => Self::Object,
            other => Self::Unknown(other),
        }
    }
}

/// Whether a chat message was audible at the listener, from the `Audible` byte
/// of `ChatFromSimulator` (a signed value: `-1`/`255` means not audible).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatAudible {
    /// Not audible (out of range); the message text may be elided.
    Not,
    /// Barely audible (at the edge of range).
    Barely,
    /// Fully audible.
    Fully,
    /// An unrecognised audibility byte, preserved verbatim.
    Unknown(u8),
}

impl ChatAudible {
    /// Classifies an `Audible` byte (`255`/`-1` = not, `0` = barely, `1` = fully).
    #[must_use]
    pub const fn from_u8(byte: u8) -> Self {
        match byte {
            255 => Self::Not,
            0 => Self::Barely,
            1 => Self::Fully,
            other => Self::Unknown(other),
        }
    }
}

/// A chat message received from the simulator, parsed from `ChatFromSimulator`.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatMessage {
    /// The display name of the speaker (avatar legacy name or object name).
    pub from_name: String,
    /// The speaker's id (agent id or object id), or nil for the system.
    pub source_id: Uuid,
    /// For an object speaker, its owner's agent id; nil otherwise.
    pub owner_id: Uuid,
    /// What kind of source produced the message.
    pub source_type: ChatSourceType,
    /// The chat type (whisper / normal / shout / …).
    pub chat_type: ChatType,
    /// Whether the message was audible at the listener.
    pub audible: ChatAudible,
    /// The speaker's region-local position, in metres.
    pub position: (f32, f32, f32),
    /// The message text (UTF-8, with any trailing NUL padding removed).
    pub message: String,
}

/// The kind of an instant message, from the `Dialog` byte of
/// `ImprovedInstantMessage` (the `EInstantMessage` enum in the protocol). Only
/// the commonly handled dialogs are named; the rest are preserved verbatim via
/// [`ImDialog::Unknown`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImDialog {
    /// An ordinary 1:1 instant message (`IM_NOTHING_SPECIAL`).
    Message,
    /// A modal message box from an object (`IM_MESSAGEBOX`).
    MessageBox,
    /// A group invitation (`IM_GROUP_INVITATION`).
    GroupInvitation,
    /// An inventory item offered to the agent (`IM_INVENTORY_OFFERED`).
    InventoryOffered,
    /// An inventory offer was accepted (`IM_INVENTORY_ACCEPTED`).
    InventoryAccepted,
    /// An inventory offer was declined (`IM_INVENTORY_DECLINED`).
    InventoryDeclined,
    /// An inventory item offered by a task/object (`IM_TASK_INVENTORY_OFFERED`).
    TaskInventoryOffered,
    /// A participant was added to a group/conference session
    /// (`IM_SESSION_INVITE` / OpenMetaverse `SessionAdd`).
    SessionAdd,
    /// An offline participant was added to a session (`IM_SESSION_P2P_INVITE` /
    /// OpenMetaverse `SessionOfflineAdd`).
    SessionOfflineAdd,
    /// A request to start a group IM session (`IM_SESSION_GROUP_START`); the
    /// session id is the group id.
    SessionGroupStart,
    /// A request to start an ad-hoc conference IM session
    /// (`IM_SESSION_CONFERENCE_START`).
    SessionConferenceStart,
    /// A message within a group or conference session (`IM_SESSION_SEND`).
    SessionSend,
    /// A participant left / was dropped from a session (`IM_SESSION_LEAVE` /
    /// OpenMetaverse `SessionDrop`).
    SessionLeave,
    /// A message from an in-world object/task (`IM_FROM_TASK`).
    FromTask,
    /// A "do not disturb" auto-response (`IM_DO_NOT_DISTURB_AUTO_RESPONSE`).
    DoNotDisturbAutoResponse,
    /// A teleport offer / lure (`IM_LURE_USER`).
    LureUser,
    /// A teleport offer was accepted (`IM_LURE_ACCEPTED`).
    LureAccepted,
    /// A teleport offer was declined (`IM_LURE_DECLINED`).
    LureDeclined,
    /// A request to be teleported to the sender (`IM_TELEPORT_REQUEST`).
    TeleportRequest,
    /// A request to open a URL (`IM_GOTO_URL`).
    GotoUrl,
    /// A group notice (`IM_GROUP_NOTICE`).
    GroupNotice,
    /// A friendship offer (`IM_FRIENDSHIP_OFFERED`).
    FriendshipOffered,
    /// A friendship offer was accepted (`IM_FRIENDSHIP_ACCEPTED`).
    FriendshipAccepted,
    /// The correspondent started typing (`IM_TYPING_START`).
    TypingStart,
    /// The correspondent stopped typing (`IM_TYPING_STOP`).
    TypingStop,
    /// An unrecognised dialog byte, preserved verbatim.
    Unknown(u8),
}

impl ImDialog {
    /// Classifies a `Dialog` byte.
    #[must_use]
    pub const fn from_u8(byte: u8) -> Self {
        match byte {
            0 => Self::Message,
            1 => Self::MessageBox,
            3 => Self::GroupInvitation,
            4 => Self::InventoryOffered,
            5 => Self::InventoryAccepted,
            6 => Self::InventoryDeclined,
            9 => Self::TaskInventoryOffered,
            13 => Self::SessionAdd,
            14 => Self::SessionOfflineAdd,
            15 => Self::SessionGroupStart,
            16 => Self::SessionConferenceStart,
            17 => Self::SessionSend,
            18 => Self::SessionLeave,
            19 => Self::FromTask,
            20 => Self::DoNotDisturbAutoResponse,
            22 => Self::LureUser,
            23 => Self::LureAccepted,
            24 => Self::LureDeclined,
            26 => Self::TeleportRequest,
            28 => Self::GotoUrl,
            32 => Self::GroupNotice,
            38 => Self::FriendshipOffered,
            39 => Self::FriendshipAccepted,
            41 => Self::TypingStart,
            42 => Self::TypingStop,
            other => Self::Unknown(other),
        }
    }

    /// The wire byte for this dialog.
    #[must_use]
    pub const fn to_u8(self) -> u8 {
        match self {
            Self::Message => 0,
            Self::MessageBox => 1,
            Self::GroupInvitation => 3,
            Self::InventoryOffered => 4,
            Self::InventoryAccepted => 5,
            Self::InventoryDeclined => 6,
            Self::TaskInventoryOffered => 9,
            Self::SessionAdd => 13,
            Self::SessionOfflineAdd => 14,
            Self::SessionGroupStart => 15,
            Self::SessionConferenceStart => 16,
            Self::SessionSend => 17,
            Self::SessionLeave => 18,
            Self::FromTask => 19,
            Self::DoNotDisturbAutoResponse => 20,
            Self::LureUser => 22,
            Self::LureAccepted => 23,
            Self::LureDeclined => 24,
            Self::TeleportRequest => 26,
            Self::GotoUrl => 28,
            Self::GroupNotice => 32,
            Self::FriendshipOffered => 38,
            Self::FriendshipAccepted => 39,
            Self::TypingStart => 41,
            Self::TypingStop => 42,
            Self::Unknown(other) => other,
        }
    }
}

/// An instant message received from the simulator, parsed from
/// `ImprovedInstantMessage`. Many fields are dialog-dependent (notably
/// [`InstantMessage::id`] and [`InstantMessage::binary_bucket`]); see
/// [`ImDialog`].
#[derive(Debug, Clone, PartialEq)]
pub struct InstantMessage {
    /// The sender's agent id.
    pub from_agent_id: Uuid,
    /// The sender's display name (with any trailing NUL padding removed).
    pub from_agent_name: String,
    /// The recipient's agent id (this agent for a direct IM, or a group id).
    pub to_agent_id: Uuid,
    /// The dialog (sub-type) of the message.
    pub dialog: ImDialog,
    /// Whether the message came from a group (rather than an agent).
    pub from_group: bool,
    /// The source region's id (nil if not provided).
    pub region_id: Uuid,
    /// The sender's region-local position, in metres.
    pub position: (f32, f32, f32),
    /// Whether the message was stored-and-forwarded while the agent was offline.
    pub offline: bool,
    /// The sender's timestamp (`0` when unset; the simulator often fills it).
    pub timestamp: u32,
    /// A dialog-dependent id: the IM session id for chats, or a transaction id
    /// for offers.
    pub id: Uuid,
    /// The parent estate id of the source.
    pub parent_estate_id: u32,
    /// The message text (UTF-8, with any trailing NUL padding removed).
    pub message: String,
    /// Dialog-dependent binary payload (e.g. an inventory offer's asset type and
    /// item id, a group invite's role and fee). Empty for an ordinary IM.
    pub binary_bucket: Vec<u8>,
}

/// An avatar's profile properties, parsed from `AvatarPropertiesReply`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvatarProperties {
    /// The avatar the profile is about.
    pub avatar_id: Uuid,
    /// The "second life" profile image (texture id).
    pub image_id: Uuid,
    /// The "first life" profile image (texture id).
    pub fl_image_id: Uuid,
    /// The avatar's partner, or nil if none.
    pub partner_id: Uuid,
    /// The "second life" about text.
    pub about_text: String,
    /// The "first life" about text.
    pub fl_about_text: String,
    /// The account creation date, as the grid's display string (e.g. `2008-01-15`).
    pub born_on: String,
    /// The web profile URL, if any.
    pub profile_url: String,
    /// The charter-member / account-title field (grid-specific; often empty).
    pub charter_member: String,
    /// The raw account/profile flags bitfield.
    pub flags: u32,
}

/// An avatar's interests, parsed from `AvatarInterestsReply`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvatarInterests {
    /// The avatar the interests are about.
    pub avatar_id: Uuid,
    /// The "want to" category bitmask.
    pub want_to_mask: u32,
    /// The "want to" free text.
    pub want_to_text: String,
    /// The "skills" category bitmask.
    pub skills_mask: u32,
    /// The "skills" free text.
    pub skills_text: String,
    /// The languages free text.
    pub languages_text: String,
}

/// One group listed in an avatar's profile, from an `AvatarGroupsReply` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvatarGroupMembership {
    /// The group id.
    pub group_id: Uuid,
    /// The group name.
    pub group_name: String,
    /// The avatar's title in the group.
    pub group_title: String,
    /// The avatar's group powers bitfield.
    pub group_powers: u64,
    /// Whether the avatar accepts notices from the group.
    pub accept_notices: bool,
    /// The group's insignia (texture id).
    pub group_insignia_id: Uuid,
}

/// One pick from an `AvatarPicksReply` (header data only: id and name).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvatarPick {
    /// The pick id (use to fetch full details).
    pub pick_id: Uuid,
    /// The pick name.
    pub name: String,
}

/// The rights one party grants the other in a Second Life friendship: a
/// bitfield shared by the login `buddy-list`, `GrantUserRights`, and
/// `ChangeUserRights`. The flag values match the viewer's `RIGHTS_*`/`GRANT_*`
/// constants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FriendRights(pub i32);

impl FriendRights {
    /// The other party may see when this party is online (`GRANT_ONLINE_STATUS`).
    pub const CAN_SEE_ONLINE: i32 = 1 << 0;
    /// The other party may see this party's location on the world map
    /// (`GRANT_MAP_LOCATION`).
    pub const CAN_SEE_ON_MAP: i32 = 1 << 1;
    /// The other party may modify this party's objects (`GRANT_MODIFY_OBJECTS`).
    pub const CAN_MODIFY_OBJECTS: i32 = 1 << 2;

    /// Whether the see-online bit is set.
    #[must_use]
    pub const fn can_see_online(self) -> bool {
        self.0 & Self::CAN_SEE_ONLINE != 0
    }

    /// Whether the see-on-map bit is set.
    #[must_use]
    pub const fn can_see_on_map(self) -> bool {
        self.0 & Self::CAN_SEE_ON_MAP != 0
    }

    /// Whether the modify-objects bit is set.
    #[must_use]
    pub const fn can_modify_objects(self) -> bool {
        self.0 & Self::CAN_MODIFY_OBJECTS != 0
    }
}

/// One friend from the login buddy list, with the friendship rights in both
/// directions (parsed from the login `buddy-list`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Friend {
    /// The friend's agent id.
    pub id: Uuid,
    /// The rights this agent grants the friend.
    pub rights_granted: FriendRights,
    /// The rights the friend grants this agent.
    pub rights_received: FriendRights,
}

/// The agent's active group and title, parsed from `AgentDataUpdate` (pushed on
/// login and whenever the active group changes via
/// [`Session::activate_group`](crate::Session::activate_group)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveGroup {
    /// The agent the update is about.
    pub agent_id: Uuid,
    /// The agent's first name.
    pub first_name: String,
    /// The agent's last name.
    pub last_name: String,
    /// The active group's title shown over the avatar (empty if no active group).
    pub group_title: String,
    /// The active group's id (nil if no active group).
    pub active_group_id: Uuid,
    /// The agent's powers bitfield within the active group.
    pub group_powers: u64,
    /// The active group's name (empty if no active group).
    pub group_name: String,
}

/// One of the agent's group memberships, from an `AgentGroupDataUpdate` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupMembership {
    /// The group id.
    pub group_id: Uuid,
    /// The agent's powers bitfield in the group.
    pub group_powers: u64,
    /// Whether the agent accepts notices from the group.
    pub accept_notices: bool,
    /// The group's insignia (texture id).
    pub group_insignia_id: Uuid,
    /// The agent's L$ contribution to the group.
    pub contribution: i32,
    /// The group name.
    pub group_name: String,
}

/// One member of a group, from a `GroupMembersReply` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupMember {
    /// The member's agent id.
    pub agent_id: Uuid,
    /// The member's L$ contribution.
    pub contribution: i32,
    /// The member's online status string (grid-formatted, e.g. `"Online"`).
    pub online_status: String,
    /// The member's powers bitfield.
    pub agent_powers: u64,
    /// The member's group title.
    pub title: String,
    /// Whether the member is a group owner.
    pub is_owner: bool,
}

/// One role within a group, from a `GroupRoleDataReply` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupRole {
    /// The role id (nil for the "Everyone" default role).
    pub role_id: Uuid,
    /// The role name.
    pub name: String,
    /// The role title shown over members holding it.
    pub title: String,
    /// The role description.
    pub description: String,
    /// The powers granted by the role.
    pub powers: u64,
    /// The number of members holding the role.
    pub members: u32,
}

/// One role↔member pairing, from a `GroupRoleMembersReply` entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroupRoleMember {
    /// The role id.
    pub role_id: Uuid,
    /// The member's agent id.
    pub member_id: Uuid,
}

/// One title the agent may wear in a group, from a `GroupTitlesReply` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupTitle {
    /// The title text.
    pub title: String,
    /// The role the title belongs to.
    pub role_id: Uuid,
    /// Whether this is the agent's currently selected title.
    pub selected: bool,
}

/// A group's full profile, parsed from `GroupProfileReply`.
#[expect(
    clippy::struct_excessive_bools,
    reason = "the four booleans mirror distinct wire flags in GroupProfileReply"
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupProfile {
    /// The group id.
    pub group_id: Uuid,
    /// The group name.
    pub name: String,
    /// The group charter text.
    pub charter: String,
    /// Whether the group is shown in search.
    pub show_in_list: bool,
    /// The requesting agent's title in the group.
    pub member_title: String,
    /// The requesting agent's powers bitfield.
    pub powers: u64,
    /// The group insignia (texture id).
    pub insignia_id: Uuid,
    /// The group founder's agent id.
    pub founder_id: Uuid,
    /// The L$ fee to join.
    pub membership_fee: i32,
    /// Whether enrollment is open (no invitation needed).
    pub open_enrollment: bool,
    /// The group's L$ balance (owners only; otherwise 0).
    pub money: i32,
    /// The total member count.
    pub member_count: i32,
    /// The total role count.
    pub role_count: i32,
    /// Whether the group allows publishing on the web.
    pub allow_publish: bool,
    /// Whether the group is flagged mature.
    pub mature_publish: bool,
    /// The owner role id.
    pub owner_role: Uuid,
}

/// The parameters for creating a group via
/// [`Session::create_group`](crate::Session::create_group)
/// (`CreateGroupRequest`).
#[expect(
    clippy::struct_excessive_bools,
    reason = "the four booleans mirror distinct wire flags in CreateGroupRequest"
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateGroupParams {
    /// The group name (must be unique on the grid).
    pub name: String,
    /// The group charter text.
    pub charter: String,
    /// Whether the group is shown in search.
    pub show_in_list: bool,
    /// The group insignia (texture id); nil for none.
    pub insignia_id: Uuid,
    /// The L$ fee to join.
    pub membership_fee: i32,
    /// Whether enrollment is open (no invitation needed).
    pub open_enrollment: bool,
    /// Whether the group allows publishing on the web.
    pub allow_publish: bool,
    /// Whether the group is flagged mature.
    pub mature_publish: bool,
}

/// One group notice header, from a `GroupNoticesListReply` entry. Fetch the full
/// body/attachment with
/// [`Session::request_group_notice`](crate::Session::request_group_notice).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupNotice {
    /// The notice id.
    pub notice_id: Uuid,
    /// The Unix timestamp the notice was posted.
    pub timestamp: u32,
    /// The poster's name.
    pub from_name: String,
    /// The notice subject.
    pub subject: String,
    /// Whether the notice carries an inventory attachment.
    pub has_attachment: bool,
    /// The attachment's asset type (meaningful only if `has_attachment`).
    pub asset_type: u8,
}

/// An inventory folder (category): from the login skeleton
/// ([`Event::InventorySkeleton`]) or an `InventoryDescendents` sub-folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InventoryFolder {
    /// The folder's id.
    pub folder_id: Uuid,
    /// The parent folder's id (nil for the root).
    pub parent_id: Uuid,
    /// The folder name.
    pub name: String,
    /// The folder's default asset/folder type (`FolderType`; `-1` for none).
    pub folder_type: i8,
    /// The folder version, or `0` when not provided (sub-folders of a descendents
    /// reply do not carry their own version).
    pub version: i32,
}

/// An inventory item, from an `InventoryDescendents` item entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InventoryItem {
    /// The item's id.
    pub item_id: Uuid,
    /// The containing folder's id.
    pub folder_id: Uuid,
    /// The item name.
    pub name: String,
    /// The item description.
    pub description: String,
    /// The underlying asset id.
    pub asset_id: Uuid,
    /// The asset type (`AssetType`).
    pub item_type: i8,
    /// The inventory type (`InventoryType`).
    pub inv_type: i8,
    /// The item flags bitfield.
    pub flags: u32,
    /// The sale type (not for sale / original / copy / contents).
    pub sale_type: u8,
    /// The sale price, in L$.
    pub sale_price: i32,
    /// The creation date (Unix seconds).
    pub creation_date: i32,
    /// The current owner's id.
    pub owner_id: Uuid,
    /// The creator's id.
    pub creator_id: Uuid,
    /// The group associated with the item.
    pub group_id: Uuid,
    /// Whether the item is group-owned.
    pub group_owned: bool,
    /// The base permissions mask.
    pub base_mask: u32,
    /// The owner permissions mask.
    pub owner_mask: u32,
    /// The group permissions mask.
    pub group_mask: u32,
    /// The everyone permissions mask.
    pub everyone_mask: u32,
    /// The next-owner permissions mask.
    pub next_owner_mask: u32,
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
