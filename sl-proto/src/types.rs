//! Public value types of the sans-I/O session: its inputs and outputs.

use std::net::SocketAddr;

use sl_types::lsl::{Rotation, Vector};
use sl_types::money::LindenAmount;
use sl_wire::{LoginRequest, ParcelFlags};
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

/// Per-category bandwidth throttle, in **kilobits per second**, advertised to
/// the simulator with `AgentThrottle`. The seven categories partition the
/// simulator's UDP send budget; the simulator uses these caps to allocate
/// bandwidth across the traffic it pushes to the client.
///
/// Without an explicit throttle the simulator applies conservative defaults
/// that starve the bulk object / terrain / texture streams the world-rendering
/// features (object scene graph, terrain, textures) depend on. Set one with
/// [`Session::set_throttle`](crate::Session::set_throttle) after the circuit is
/// established; it is re-sent automatically on every region change.
///
/// The values are interpreted as a total bandwidth split: the sum across all
/// seven categories is the requested aggregate rate, which the simulator may
/// cap to its own configured maximum. Use [`Throttle::total`] to read the sum
/// and the [`Throttle::preset_300`] / [`Throttle::preset_500`] /
/// [`Throttle::preset_1000`] presets (named for their total kbps) as starting
/// points; they mirror the reference viewer's bandwidth tables.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Throttle {
    /// Resent (reliable retransmit) traffic.
    pub resend: f32,
    /// Land/terrain layer (`LayerData`) traffic.
    pub land: f32,
    /// Wind layer traffic.
    pub wind: f32,
    /// Cloud layer traffic.
    pub cloud: f32,
    /// Task traffic: object updates (the scene graph).
    pub task: f32,
    /// Texture (image) traffic.
    pub texture: f32,
    /// Other asset traffic (sounds, animations, notecards, …).
    pub asset: f32,
}

impl Throttle {
    /// Builds a throttle from the seven per-category rates (kilobits per second),
    /// in wire order: resend, land, wind, cloud, task, texture, asset.
    #[must_use]
    pub const fn new(
        resend: f32,
        land: f32,
        wind: f32,
        cloud: f32,
        task: f32,
        texture: f32,
        asset: f32,
    ) -> Self {
        Self {
            resend,
            land,
            wind,
            cloud,
            task,
            texture,
            asset,
        }
    }

    /// The reference viewer's preset for a 300 kbps total bandwidth.
    #[must_use]
    pub const fn preset_300() -> Self {
        Self::new(30.0, 40.0, 9.0, 9.0, 86.0, 86.0, 40.0)
    }

    /// The reference viewer's preset for a 500 kbps total bandwidth.
    #[must_use]
    pub const fn preset_500() -> Self {
        Self::new(50.0, 70.0, 14.0, 14.0, 136.0, 136.0, 80.0)
    }

    /// The reference viewer's preset for a 1000 kbps total bandwidth.
    #[must_use]
    pub const fn preset_1000() -> Self {
        Self::new(100.0, 100.0, 20.0, 20.0, 310.0, 310.0, 140.0)
    }

    /// The total requested bandwidth (kilobits per second), the sum of all seven
    /// categories.
    #[must_use]
    pub fn total(&self) -> f32 {
        self.resend + self.land + self.wind + self.cloud + self.task + self.texture + self.asset
    }

    /// The seven category rates in wire order (resend, land, wind, cloud, task,
    /// texture, asset), converted to **bits per second** as the `AgentThrottle`
    /// wire encoding expects (the simulator divides by 8 to get bytes/second).
    #[must_use]
    pub fn bits_per_second(&self) -> [f32; 7] {
        // 1 kilobit = 1024 bits, matching the reference viewer's conversion.
        const KILOBIT: f32 = 1024.0;
        [
            self.resend * KILOBIT,
            self.land * KILOBIT,
            self.wind * KILOBIT,
            self.cloud * KILOBIT,
            self.task * KILOBIT,
            self.texture * KILOBIT,
            self.asset * KILOBIT,
        ]
    }
}

impl Default for Throttle {
    /// The 1000 kbps preset — a generous split suited to a client that wants the
    /// full object/terrain/texture firehose.
    fn default() -> Self {
        Self::preset_1000()
    }
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
    /// The agent's L$ balance, parsed from a `MoneyBalanceReply` (a reply to
    /// [`Session::request_money_balance`](crate::Session::request_money_balance),
    /// or pushed by the simulator after a transaction changes the balance).
    MoneyBalance(MoneyBalance),
    /// Grid economy prices and region capacity, parsed from an `EconomyData`
    /// reply to [`Session::request_economy_data`](crate::Session::request_economy_data).
    EconomyData(Box<EconomyData>),
    /// A parcel's geometry, flags, and limits, parsed from a `ParcelProperties`
    /// reply to
    /// [`Session::request_parcel_properties`](crate::Session::request_parcel_properties).
    ParcelProperties(Box<ParcelInfo>),
    /// A region parcel-ownership overlay chunk (one of four), parsed from a
    /// `ParcelOverlay`.
    ParcelOverlay(ParcelOverlayInfo),
    /// A parcel's dwell (traffic) value, from a `ParcelDwellReply` in response to
    /// [`Session::request_parcel_dwell`](crate::Session::request_parcel_dwell).
    ParcelDwell {
        /// The parcel's region-local id.
        local_id: i32,
        /// The parcel's persistent id.
        parcel_id: Uuid,
        /// The dwell (accumulated traffic) value.
        dwell: f32,
    },
    /// A parcel's access (allow) or ban list, from a `ParcelAccessListReply` in
    /// response to
    /// [`Session::request_parcel_access_list`](crate::Session::request_parcel_access_list).
    ParcelAccessList {
        /// The parcel's region-local id.
        local_id: i32,
        /// Which list this is (allow or ban).
        scope: ParcelAccessScope,
        /// The list entries.
        entries: Vec<ParcelAccessEntry>,
    },
    /// An estate's configuration, from an `EstateOwnerMessage` `estateupdateinfo`
    /// reply to [`Session::request_estate_info`](crate::Session::request_estate_info).
    EstateInfo(Box<EstateInfo>),
    /// One of an estate's access lists, from an `EstateOwnerMessage` `setaccess`
    /// reply (to [`Session::request_estate_info`](crate::Session::request_estate_info)
    /// or after an [`Session::update_estate_access`](crate::Session::update_estate_access)).
    /// A large list may arrive split across several events.
    EstateAccessList {
        /// The estate id.
        estate_id: u32,
        /// Which list this is.
        kind: EstateAccessKind,
        /// The agent/group ids in this chunk of the list.
        members: Vec<Uuid>,
    },
    /// A neighbouring simulator was announced via `EnableSimulator`.
    NeighborDiscovered(NeighborInfo),
    /// A neighbouring region's child-agent seed capability arrived
    /// (`EstablishAgentCommunication`). The driver should POST it (the standard
    /// CAPS seed request) so the neighbour marks the agent's capabilities as sent
    /// and begins streaming that region's scene to the child circuit — without
    /// it, OpenSim withholds the neighbour's object updates (its `SendInitialData`
    /// is gated on `SentSeeds`).
    NeighborSeed {
        /// The neighbouring simulator's UDP address (the child circuit's key).
        sim: SocketAddr,
        /// The neighbour's seed capability URL to POST.
        seed_capability: String,
    },
    /// A region was reported by the world map (a `MapBlockReply` entry), giving
    /// its name and grid coordinates. Sent in response to
    /// [`Session::request_map_blocks`](crate::Session::request_map_blocks) and
    /// [`Session::request_map_by_name`](crate::Session::request_map_by_name).
    MapBlock(Box<MapRegionInfo>),
    /// World-map overlay items (avatar locations, telehubs, land for sale,
    /// events) from a `MapItemReply`, in response to
    /// [`Session::request_map_items`](crate::Session::request_map_items). All
    /// items share the requested `item_type`.
    MapItems {
        /// The kind of item these are (echoed from the request).
        item_type: MapItemType,
        /// The items returned for the queried region(s).
        items: Vec<MapItem>,
    },
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
    /// A scripted object showed a dialog (`ScriptDialog`, i.e. `llDialog` or
    /// `llTextBox`). Respond with
    /// [`Session::reply_script_dialog`](crate::Session::reply_script_dialog).
    ScriptDialog(Box<ScriptDialog>),
    /// A scripted object requested permissions (`ScriptQuestion`, i.e.
    /// `llRequestPermissions`). Grant a subset with
    /// [`Session::answer_script_permissions`](crate::Session::answer_script_permissions).
    ScriptPermissionRequest(Box<ScriptPermissionRequest>),
    /// A scripted object asked to open a URL (`LoadURL`, i.e. `llLoadURL`). There
    /// is no protocol reply; the client decides whether to open it.
    LoadUrl(Box<LoadUrlRequest>),
    /// A scripted object asked to teleport the agent (`ScriptTeleportRequest`,
    /// i.e. `llMapDestination`). The client may initiate the teleport itself.
    ScriptTeleport(Box<ScriptTeleportRequest>),
    /// The agent's mute (block) list, in response to
    /// [`Session::request_mute_list`](crate::Session::request_mute_list): the
    /// list was downloaded (via the file-transfer `Xfer` path) and parsed, or is
    /// empty. Edits made with [`Session::mute`](crate::Session::mute) /
    /// [`unmute`](crate::Session::unmute) take effect server-side; re-request to
    /// see the updated list.
    MuteList(Vec<MuteEntry>),
    /// The simulator reported that the agent's cached mute list is still current
    /// (`UseCachedMuteList`), in response to
    /// [`Session::request_mute_list`](crate::Session::request_mute_list) with a
    /// non-zero CRC; no list was re-downloaded.
    MuteListUnchanged,
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
    /// An object entered the current region's scene graph: the first
    /// `ObjectUpdate`/`ObjectUpdateCompressed` seen for its local id (or the
    /// first full update after a [`Session::request_objects`](crate::Session::request_objects)
    /// cache-miss fetch). Carries the full decoded [`Object`].
    ObjectAdded(Box<Object>),
    /// A known object changed: a later full/compressed `ObjectUpdate`, or a
    /// motion-only `ImprovedTerseObjectUpdate` (which updates the
    /// [`Object::motion`] of an already-cached object). Carries the merged
    /// [`Object`].
    ObjectUpdated(Box<Object>),
    /// An object left the scene (`KillObject`): it was removed from the region
    /// cache.
    ObjectRemoved {
        /// The region the object was in (0 if it was never fully cached).
        region_handle: u64,
        /// The object's region-local id.
        local_id: u32,
    },
    /// An object's extended properties (`ObjectProperties`), in response to
    /// [`Session::request_object_properties`](crate::Session::request_object_properties)
    /// (which selects the object). If the object is in the scene cache its
    /// [`Object::properties`] is updated too.
    ObjectProperties(Box<ObjectProperties>),
    /// The session logged out cleanly (a `LogoutReply` was received).
    LoggedOut,
    /// The session disconnected for the given reason.
    Disconnected(DisconnectReason),
}

/// Linden `PCode` constants: the object-class byte (`p_code`) in an object
/// update, identifying what kind of entity an object is.
pub mod pcode {
    /// A primitive (an ordinary in-world object / prim).
    pub const PRIMITIVE: u8 = 9;
    /// An avatar.
    pub const AVATAR: u8 = 47;
    /// A grass patch.
    pub const GRASS: u8 = 95;
    /// A new-style (SL 1.x+) tree.
    pub const NEW_TREE: u8 = 111;
    /// A particle-system legacy object.
    pub const PARTICLE_SYSTEM: u8 = 143;
    /// A legacy tree.
    pub const TREE: u8 = 255;
}

/// An object's kinematic state, decoded from the packed `ObjectData`/`Data`
/// blob of an object update. Linear quantities are region-local; the rotation
/// is the object's orientation in its parent's frame.
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectMotion {
    /// Region-local position, in metres.
    pub position: Vector,
    /// Linear velocity, in metres/second.
    pub velocity: Vector,
    /// Linear acceleration, in metres/second².
    pub acceleration: Vector,
    /// Orientation (a unit quaternion).
    pub rotation: Rotation,
    /// Angular velocity (the rotation axis scaled by radians/second).
    pub angular_velocity: Vector,
}

/// A cached scene object (a primitive or avatar) for the current region,
/// assembled from `ObjectUpdate` / `ObjectUpdateCompressed` and kept current by
/// later full, compressed, and motion-only (`ImprovedTerseObjectUpdate`)
/// updates. Surfaced via [`Event::ObjectAdded`] / [`Event::ObjectUpdated`] and
/// removed via [`Event::ObjectRemoved`].
#[derive(Debug, Clone, PartialEq)]
pub struct Object {
    /// The region the object lives in (its `RegionHandle`).
    pub region_handle: u64,
    /// The region-local id (the transient handle the simulator uses; not stable
    /// across region crossings or relogins).
    pub local_id: u32,
    /// The object's persistent global id.
    pub full_id: Uuid,
    /// The local id of the parent object this is linked/attached to, or 0 if it
    /// has no parent (a root object).
    pub parent_id: u32,
    /// The object class (see the [`pcode`] constants).
    pub pcode: u8,
    /// The object/attachment state byte (e.g. attachment point for attachments).
    pub state: u8,
    /// The simulator's per-object CRC (used for object-cache validation).
    pub crc: u32,
    /// The material code.
    pub material: u8,
    /// The click action (`CLICK_ACTION_*`).
    pub click_action: u8,
    /// The object/prim flags bitfield (`PrimFlags`), from the update's
    /// `UpdateFlags`.
    pub update_flags: u32,
    /// The object's size, in metres along each axis.
    pub scale: Vector,
    /// The object's kinematic state.
    pub motion: ObjectMotion,
    /// The owner's id (only meaningful when the object has sound or particles;
    /// otherwise the simulator sends a null id — see the LL protocol "hack").
    pub owner_id: Uuid,
    /// The attached sound's asset id (null if none).
    pub sound: Uuid,
    /// The attached sound's gain.
    pub gain: f32,
    /// The attached sound's flags.
    pub sound_flags: u8,
    /// The attached sound's cutoff radius, in metres.
    pub sound_radius: f32,
    /// The object's floating text (`llSetText`), empty if none.
    pub text: String,
    /// The floating-text colour as RGBA bytes.
    pub text_color: [u8; 4],
    /// The object's name-value pairs (e.g. an attachment's `AttachItemID`), as
    /// the raw newline-separated string; empty if none.
    pub name_value: String,
    /// The media URL set on the object, empty if none.
    pub media_url: String,
    /// The raw `TextureEntry` blob (per-face texture/colour data), undecoded.
    pub texture_entry: Vec<u8>,
    /// The raw `ExtraParams` blob (flexi/light/sculpt/mesh parameters),
    /// undecoded.
    pub extra_params: Vec<u8>,
    /// The object's extended properties (creator, permissions, name,
    /// description, …) once an [`Event::ObjectProperties`] has been received for
    /// it; `None` until then.
    pub properties: Option<ObjectProperties>,
}

/// An object's extended properties (`ObjectProperties`), delivered after the
/// object is selected (see
/// [`Session::request_object_properties`](crate::Session::request_object_properties)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectProperties {
    /// The object's persistent global id.
    pub object_id: Uuid,
    /// The creator's id.
    pub creator_id: Uuid,
    /// The current owner's id.
    pub owner_id: Uuid,
    /// The group the object is set to.
    pub group_id: Uuid,
    /// The previous owner's id.
    pub last_owner_id: Uuid,
    /// The creation timestamp (seconds since the Unix epoch).
    pub creation_date: u64,
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
    /// The ownership cost, in L$.
    pub ownership_cost: i32,
    /// The sale type (`SALE_TYPE_*`).
    pub sale_type: u8,
    /// The sale price, in L$.
    pub sale_price: i32,
    /// The object category code.
    pub category: u32,
    /// The object's name.
    pub name: String,
    /// The object's description.
    pub description: String,
    /// The custom touch-action label, empty if none.
    pub touch_name: String,
    /// The custom sit-action label, empty if none.
    pub sit_name: String,
}

// ---------------------------------------------------------------------------
// Object interaction & editing (#17): value types for the editing surface.
// ---------------------------------------------------------------------------

/// The left-click behaviour of an object (`ClickAction` / `CLICK_ACTION_*`), as
/// set by [`Session::set_object_click_action`](crate::Session::set_object_click_action).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClickAction {
    /// The default: clicking touches the object (`CLICK_ACTION_TOUCH`, also
    /// `CLICK_ACTION_NONE`).
    #[default]
    Touch,
    /// Clicking sits the avatar on the object (`CLICK_ACTION_SIT`).
    Sit,
    /// Clicking buys the object (`CLICK_ACTION_BUY`).
    Buy,
    /// Clicking pays the object (`CLICK_ACTION_PAY`).
    Pay,
    /// Clicking opens the object's contents (`CLICK_ACTION_OPEN`).
    Open,
    /// Clicking plays the parcel media (`CLICK_ACTION_PLAY`).
    Play,
    /// Clicking opens the object's media (`CLICK_ACTION_OPEN_MEDIA`).
    OpenMedia,
    /// Clicking zooms the camera to the object (`CLICK_ACTION_ZOOM`).
    Zoom,
    /// Clicking is disabled (`CLICK_ACTION_DISABLED`).
    Disabled,
    /// Clicks are ignored / pass through (`CLICK_ACTION_IGNORE`).
    Ignore,
}

impl ClickAction {
    /// The `ClickAction` wire byte for this action.
    #[must_use]
    pub const fn to_code(self) -> u8 {
        match self {
            Self::Touch => 0,
            Self::Sit => 1,
            Self::Buy => 2,
            Self::Pay => 3,
            Self::Open => 4,
            Self::Play => 5,
            Self::OpenMedia => 6,
            Self::Zoom => 7,
            Self::Disabled => 8,
            Self::Ignore => 9,
        }
    }

    /// Classifies a `ClickAction` wire byte (unknown values map to `Touch`).
    #[must_use]
    pub const fn from_code(code: u8) -> Self {
        match code {
            1 => Self::Sit,
            2 => Self::Buy,
            3 => Self::Pay,
            4 => Self::Open,
            5 => Self::Play,
            6 => Self::OpenMedia,
            7 => Self::Zoom,
            8 => Self::Disabled,
            9 => Self::Ignore,
            _ => Self::Touch,
        }
    }
}

/// An object's physical material (`LL_MCODE_*`), as set by
/// [`Session::set_object_material`](crate::Session::set_object_material). The
/// material governs the object's collision sound and default friction/density.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Material {
    /// Stone (`LL_MCODE_STONE`).
    Stone,
    /// Metal (`LL_MCODE_METAL`).
    Metal,
    /// Glass (`LL_MCODE_GLASS`).
    Glass,
    /// Wood (`LL_MCODE_WOOD`) — the viewer's default for a new prim.
    #[default]
    Wood,
    /// Flesh (`LL_MCODE_FLESH`).
    Flesh,
    /// Plastic (`LL_MCODE_PLASTIC`).
    Plastic,
    /// Rubber (`LL_MCODE_RUBBER`).
    Rubber,
    /// Light (`LL_MCODE_LIGHT`).
    Light,
}

impl Material {
    /// The `LL_MCODE_*` wire byte for this material.
    #[must_use]
    pub const fn to_code(self) -> u8 {
        match self {
            Self::Stone => 0,
            Self::Metal => 1,
            Self::Glass => 2,
            Self::Wood => 3,
            Self::Flesh => 4,
            Self::Plastic => 5,
            Self::Rubber => 6,
            Self::Light => 7,
        }
    }

    /// Classifies an `LL_MCODE_*` wire byte (unknown values map to `Wood`).
    #[must_use]
    pub const fn from_code(code: u8) -> Self {
        match code {
            0 => Self::Stone,
            1 => Self::Metal,
            2 => Self::Glass,
            4 => Self::Flesh,
            5 => Self::Plastic,
            6 => Self::Rubber,
            7 => Self::Light,
            _ => Self::Wood,
        }
    }
}

/// How an object is offered for sale (`EForSale`), as set by
/// [`Session::set_object_for_sale`](crate::Session::set_object_for_sale) and
/// reported in [`ObjectProperties::sale_type`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SaleType {
    /// Not for sale (`FS_NOT`).
    #[default]
    NotForSale,
    /// The original object is sold and removed from the world (`FS_ORIGINAL`).
    Original,
    /// A copy is sold, leaving the original in place (`FS_COPY`).
    Copy,
    /// The object's contents are sold (`FS_CONTENTS`).
    Contents,
}

impl SaleType {
    /// The `EForSale` wire byte for this sale type.
    #[must_use]
    pub const fn to_code(self) -> u8 {
        match self {
            Self::NotForSale => 0,
            Self::Original => 1,
            Self::Copy => 2,
            Self::Contents => 3,
        }
    }

    /// Classifies an `EForSale` wire byte (unknown values map to `NotForSale`).
    #[must_use]
    pub const fn from_code(code: u8) -> Self {
        match code {
            1 => Self::Original,
            2 => Self::Copy,
            3 => Self::Contents,
            _ => Self::NotForSale,
        }
    }
}

/// Where a derezzed object should go (the `Destination` of `DeRezObject`, LL's
/// `EDeRezDestination` / `DRD_*`), as passed to
/// [`Session::derez_objects`](crate::Session::derez_objects).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeRezDestination {
    /// Save into agent inventory, leaving a copy in world (`DRD_SAVE_INTO_AGENT_INVENTORY`).
    SaveIntoAgentInventory,
    /// Acquire into agent inventory, trying to leave a copy (`DRD_ACQUIRE_TO_AGENT_INVENTORY`).
    AcquireToAgentInventory,
    /// Save into a task's (prim's) inventory (`DRD_SAVE_INTO_TASK_INVENTORY`); the
    /// destination id is the target task's id.
    SaveIntoTaskInventory,
    /// Wear as an attachment (`DRD_ATTACHMENT`).
    Attachment,
    /// Take into agent inventory, deleting from the world (`DRD_TAKE_INTO_AGENT_INVENTORY`);
    /// the destination id is the inventory folder.
    TakeIntoAgentInventory,
    /// Force take a copy to the god inventory (`DRD_FORCE_TO_GOD_INVENTORY`).
    ForceToGodInventory,
    /// Delete to the trash (`DRD_TRASH`); the destination id is the trash folder.
    Trash,
    /// Detach an attachment to inventory (`DRD_ATTACHMENT_TO_INV`).
    AttachmentToInventory,
    /// An existing attachment (`DRD_ATTACHMENT_EXISTS`).
    AttachmentExists,
    /// Return to the owner's inventory (`DRD_RETURN_TO_OWNER`).
    ReturnToOwner,
    /// Return a deeded object to the last owner's inventory (`DRD_RETURN_TO_LAST_OWNER`).
    ReturnToLastOwner,
}

impl DeRezDestination {
    /// The `DRD_*` wire byte for this destination.
    #[must_use]
    pub const fn to_code(self) -> u8 {
        match self {
            Self::SaveIntoAgentInventory => 0,
            Self::AcquireToAgentInventory => 1,
            Self::SaveIntoTaskInventory => 2,
            Self::Attachment => 3,
            Self::TakeIntoAgentInventory => 4,
            Self::ForceToGodInventory => 5,
            Self::Trash => 6,
            Self::AttachmentToInventory => 7,
            Self::AttachmentExists => 8,
            Self::ReturnToOwner => 9,
            Self::ReturnToLastOwner => 10,
        }
    }
}

/// Which permission mask an `ObjectPermissions` change targets (the `Field`
/// byte; LL's `PERM_BASE`/`PERM_OWNER`/…), passed to
/// [`Session::set_object_permissions`](crate::Session::set_object_permissions).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionField {
    /// The base permissions mask (`PERM_BASE`).
    Base,
    /// The owner permissions mask (`PERM_OWNER`).
    Owner,
    /// The group permissions mask (`PERM_GROUP`).
    Group,
    /// The everyone permissions mask (`PERM_EVERYONE`).
    Everyone,
    /// The next-owner permissions mask (`PERM_NEXT_OWNER`).
    NextOwner,
}

impl PermissionField {
    /// The `Field` wire byte selecting this mask.
    #[must_use]
    pub const fn to_code(self) -> u8 {
        match self {
            Self::Base => 0x01,
            Self::Owner => 0x02,
            Self::Group => 0x04,
            Self::Everyone => 0x08,
            Self::NextOwner => 0x10,
        }
    }
}

/// The shape parameters of a primitive to rez via
/// [`Session::rez_object`](crate::Session::rez_object) (`ObjectAdd`). Start from
/// [`PrimShape::cube`] (a unit box) and adjust as needed; the path/profile
/// fields use the same quantized wire encoding the viewer sends.
#[derive(Debug, Clone, PartialEq)]
pub struct PrimShape {
    /// The object class (almost always [`pcode::PRIMITIVE`], a volume prim).
    pub pcode: u8,
    /// The object material (see [`Material`]).
    pub material: Material,
    /// The `AddFlags` bitfield (`PrimFlags`); 0 for an ordinary, unselected,
    /// non-physical prim.
    pub add_flags: u32,
    /// The path curve byte (`LL_PCODE_PATH_*`).
    pub path_curve: u8,
    /// The profile curve byte (`LL_PCODE_PROFILE_*`, with the hollow shape in the
    /// high nibble).
    pub profile_curve: u8,
    /// The path cut start, quantized (`begin / 0.00002`).
    pub path_begin: u16,
    /// The path cut end, quantized (`50000 - end / 0.00002`).
    pub path_end: u16,
    /// The path top-size X, quantized (`200 - scale_x / 0.01`).
    pub path_scale_x: u8,
    /// The path top-size Y, quantized (`200 - scale_y / 0.01`).
    pub path_scale_y: u8,
    /// The path shear X, quantized (`shear_x / 0.01`).
    pub path_shear_x: u8,
    /// The path shear Y, quantized (`shear_y / 0.01`).
    pub path_shear_y: u8,
    /// The path twist end, quantized (`twist / 0.01`).
    pub path_twist: i8,
    /// The path twist start, quantized (`twist_begin / 0.01`).
    pub path_twist_begin: i8,
    /// The path radius offset, quantized (`radius_offset / 0.01`).
    pub path_radius_offset: i8,
    /// The path taper X, quantized (`taper_x / 0.01`).
    pub path_taper_x: i8,
    /// The path taper Y, quantized (`taper_y / 0.01`).
    pub path_taper_y: i8,
    /// The path revolutions, quantized (`(revolutions - 1) / 0.015`).
    pub path_revolutions: u8,
    /// The path skew, quantized (`skew / 0.01`).
    pub path_skew: i8,
    /// The profile cut start, quantized (`begin / 0.00002`).
    pub profile_begin: u16,
    /// The profile cut end, quantized (`50000 - end / 0.00002`).
    pub profile_end: u16,
    /// The profile hollow fraction, quantized (`hollow / 0.00002`).
    pub profile_hollow: u16,
    /// The size of the prim, in metres along each axis.
    pub scale: Vector,
    /// The orientation of the prim.
    pub rotation: Rotation,
    /// The region-local position to rez at.
    pub position: Vector,
    /// The object/attachment state byte (0 for a plain prim).
    pub state: u8,
}

impl PrimShape {
    /// A unit (0.5 m) cube at `position` with the viewer's default new-prim
    /// settings (wood, square profile, line path, identity rotation). Mutate the
    /// returned struct to change the shape or size before passing it to
    /// [`Session::rez_object`](crate::Session::rez_object).
    #[must_use]
    pub const fn cube(position: Vector) -> Self {
        Self {
            pcode: pcode::PRIMITIVE,
            material: Material::Wood,
            add_flags: 0,
            // LL_PCODE_PATH_LINE
            path_curve: 0x10,
            // LL_PCODE_PROFILE_SQUARE
            profile_curve: 0x01,
            path_begin: 0,
            path_end: 0,
            // 200 - 1.0 / 0.01 = 100 (full top size on both axes)
            path_scale_x: 100,
            path_scale_y: 100,
            path_shear_x: 0,
            path_shear_y: 0,
            path_twist: 0,
            path_twist_begin: 0,
            path_radius_offset: 0,
            path_taper_x: 0,
            path_taper_y: 0,
            path_revolutions: 0,
            path_skew: 0,
            profile_begin: 0,
            profile_end: 0,
            profile_hollow: 0,
            scale: Vector {
                x: 0.5,
                y: 0.5,
                z: 0.5,
            },
            rotation: Rotation {
                x: 0.0,
                y: 0.0,
                z: 0.0,
                s: 1.0,
            },
            position,
            state: 0,
        }
    }
}

/// The physics/flag toggles of an `ObjectFlagUpdate`, set by
/// [`Session::set_object_flags`](crate::Session::set_object_flags). Build with
/// [`ObjectFlagSettings::default`] (all false) and set the flags to change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "mirrors the four independent boolean toggles of the ObjectFlagUpdate wire message"
)]
pub struct ObjectFlagSettings {
    /// Whether the object is physical (`UsePhysics`).
    pub use_physics: bool,
    /// Whether the object is temporary (auto-deleted; `IsTemporary`).
    pub is_temporary: bool,
    /// Whether the object is phantom (no collisions; `IsPhantom`).
    pub is_phantom: bool,
    /// Whether the object casts shadows (`CastsShadows`, legacy/unused).
    pub casts_shadows: bool,
}

/// A move/scale/rotate change applied to an object via
/// [`Session::update_object`](crate::Session::update_object)
/// (`MultipleObjectUpdate`). Set only the components to change; leave the rest
/// `None`. `group` edits the whole linkset (root-relative); `uniform` keeps a
/// scale change proportional about the object's centre.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ObjectTransform {
    /// The new region-local position, if the position is being changed.
    pub position: Option<Vector>,
    /// The new orientation, if the rotation is being changed.
    pub rotation: Option<Rotation>,
    /// The new size in metres, if the scale is being changed.
    pub scale: Option<Vector>,
    /// Apply to the whole linkset rather than the single prim (the `LINK_SET`
    /// bit, `0x08`).
    pub group: bool,
    /// Scale uniformly about the object's centre (the `UNIFORM` bit, `0x10`).
    /// Only meaningful when [`scale`](Self::scale) is set.
    pub uniform: bool,
}

impl ObjectTransform {
    /// The `MultipleObjectUpdate` `Type` byte for this change: the OR of the
    /// position (`0x01`), rotation (`0x02`), scale (`0x04`), group (`0x08`), and
    /// uniform (`0x10`) bits actually present.
    #[must_use]
    pub const fn type_byte(&self) -> u8 {
        let mut flags = 0_u8;
        if self.position.is_some() {
            flags |= 0x01;
        }
        if self.rotation.is_some() {
            flags |= 0x02;
        }
        if self.scale.is_some() {
            flags |= 0x04;
        }
        if self.group {
            flags |= 0x08;
        }
        if self.uniform {
            flags |= 0x10;
        }
        flags
    }
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

    /// The `SimAccess` byte for this maturity (`Unknown` maps to PG), used when
    /// setting a region's maturity via `setregioninfo`.
    #[must_use]
    pub const fn to_sim_access(self) -> u8 {
        match self {
            Self::Mature => sl_wire::sim_access::MATURE,
            Self::Adult => sl_wire::sim_access::ADULT,
            Self::Pg | Self::Unknown => sl_wire::sim_access::PG,
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

/// The agent's L$ balance and land-tier accounting, parsed from a
/// `MoneyBalanceReply` (a reply to
/// [`Session::request_money_balance`](crate::Session::request_money_balance), or
/// pushed unsolicited by the simulator after a transaction changes the balance).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoneyBalance {
    /// The agent the balance belongs to (the client's own id).
    pub agent_id: Uuid,
    /// Whether the transaction that triggered this reply succeeded. Always `true`
    /// for a plain balance poll.
    pub success: bool,
    /// The current L$ balance.
    pub balance: LindenAmount,
    /// Land credit in square metres (owned-land tier accounting).
    pub square_meters_credit: i32,
    /// Land committed in square metres.
    pub square_meters_committed: i32,
    /// A human-readable description of the triggering transaction (empty for a
    /// plain balance poll).
    pub description: String,
    /// Details of the transaction that changed the balance, present only when the
    /// reply carried a non-zero `TransactionInfo` block (servers ≥ 1.40); `None`
    /// for a plain balance poll.
    pub transaction: Option<MoneyTransaction>,
}

/// The transaction details optionally attached to a [`MoneyBalance`], describing
/// the L$ movement that changed the balance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoneyTransaction {
    /// The transaction type code (e.g. `5008` for paying an object); classify
    /// with [`MoneyTransactionType::from_i32`].
    pub transaction_type: i32,
    /// The source of the funds (the payer).
    pub source_id: Uuid,
    /// Whether the source is a group.
    pub source_is_group: bool,
    /// The destination of the funds (the payee).
    pub dest_id: Uuid,
    /// Whether the destination is a group.
    pub dest_is_group: bool,
    /// The L$ amount moved.
    pub amount: LindenAmount,
    /// A description of the item or reason for the transaction.
    pub item_description: String,
}

/// The kind of an L$ transfer, used as the `TransactionType` of a
/// [`Session::send_money_transfer`](crate::Session::send_money_transfer). A small
/// subset of the Second Life transaction codes (`lltransactiontypes.h`); any
/// other code round-trips through [`MoneyTransactionType::Other`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoneyTransactionType {
    /// A direct L$ gift to another avatar (`5001`).
    Gift,
    /// Paying a scripted object — a tip jar, vendor, pay button, etc. (`5008`).
    PayObject,
    /// Buying an object that is set for sale (`5000`).
    ObjectSale,
    /// Any other transaction code, preserved verbatim.
    Other(i32),
}

impl MoneyTransactionType {
    /// Classifies a `TransactionType` wire value.
    #[must_use]
    pub const fn from_i32(value: i32) -> Self {
        match value {
            5000 => Self::ObjectSale,
            5001 => Self::Gift,
            5008 => Self::PayObject,
            other => Self::Other(other),
        }
    }

    /// The wire value for this transaction type.
    #[must_use]
    pub const fn to_i32(self) -> i32 {
        match self {
            Self::ObjectSale => 5000,
            Self::Gift => 5001,
            Self::PayObject => 5008,
            Self::Other(code) => code,
        }
    }
}

/// Grid economy prices and the region's object capacity, parsed from an
/// `EconomyData` reply to
/// [`Session::request_economy_data`](crate::Session::request_economy_data). All
/// prices are in L$ unless noted.
#[derive(Debug, Clone, PartialEq)]
pub struct EconomyData {
    /// The region's total object/prim capacity.
    pub object_capacity: i32,
    /// The region's current object/prim count.
    pub object_count: i32,
    /// Price per energy unit.
    pub price_energy_unit: i32,
    /// Price to claim an object.
    pub price_object_claim: i32,
    /// Price charged for a public object decaying.
    pub price_public_object_decay: i32,
    /// Price charged for deleting a public object.
    pub price_public_object_delete: i32,
    /// Price to claim a parcel.
    pub price_parcel_claim: i32,
    /// Multiplier applied to the parcel-claim price.
    pub price_parcel_claim_factor: f32,
    /// Price to upload an asset (texture, sound, animation, mesh).
    pub price_upload: i32,
    /// Price to rent a light source.
    pub price_rent_light: i32,
    /// Minimum L$ charged for a teleport.
    pub teleport_min_price: i32,
    /// Exponent applied to teleport distance for pricing.
    pub teleport_price_exponent: f32,
    /// Energy-efficiency scalar.
    pub energy_efficiency: f32,
    /// Weekly object-rent price.
    pub price_object_rent: f32,
    /// Scale factor applied to object rent.
    pub price_object_scale_factor: f32,
    /// Weekly parcel-rent price.
    pub price_parcel_rent: i32,
    /// Price to create a group.
    pub price_group_create: i32,
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

/// A parcel category, the `Category` of a [`ParcelUpdate`] (the parcel's search
/// classification).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ParcelCategory {
    /// No category set.
    #[default]
    None,
    /// A Linden-owned location.
    Linden,
    /// Residential land.
    Residential,
    /// Commercial land.
    Commercial,
    /// Industrial land.
    Industrial,
    /// A park or recreation area.
    ParkAndRecreation,
    /// Anything else.
    Other,
    /// Adult-oriented land.
    Adult,
    /// An unrecognised category value, preserved verbatim.
    Unknown(u8),
}

impl ParcelCategory {
    /// Classifies a parcel-category wire value.
    #[must_use]
    pub const fn from_u8(value: u8) -> Self {
        match value {
            0 => Self::None,
            1 => Self::Linden,
            2 => Self::Residential,
            3 => Self::Commercial,
            4 => Self::Industrial,
            5 => Self::ParkAndRecreation,
            6 => Self::Other,
            7 => Self::Adult,
            other => Self::Unknown(other),
        }
    }

    /// The wire value for this category.
    #[must_use]
    pub const fn to_u8(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Linden => 1,
            Self::Residential => 2,
            Self::Commercial => 3,
            Self::Industrial => 4,
            Self::ParkAndRecreation => 5,
            Self::Other => 6,
            Self::Adult => 7,
            Self::Unknown(value) => value,
        }
    }
}

/// Which parcel access list to query or modify: the allow list or the ban list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParcelAccessScope {
    /// The allow list (`AL_ACCESS`, `0x1`).
    Access,
    /// The ban list (`AL_BAN`, `0x2`).
    Ban,
}

impl ParcelAccessScope {
    /// The access-list flag wire value.
    #[must_use]
    pub const fn to_u32(self) -> u32 {
        match self {
            Self::Access => 0x1,
            Self::Ban => 0x2,
        }
    }

    /// Classifies an access-list flag value (preferring `Access` if both bits
    /// are set).
    #[must_use]
    pub const fn from_u32(flags: u32) -> Self {
        if flags & 0x1 != 0 {
            Self::Access
        } else {
            Self::Ban
        }
    }
}

/// One entry of a parcel access (allow) or ban list, from an
/// [`Event::ParcelAccessList`] or supplied to
/// [`Session::update_parcel_access_list`](crate::Session::update_parcel_access_list).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParcelAccessEntry {
    /// The agent the entry applies to.
    pub id: Uuid,
    /// The Unix expiry time (`time_t`); `0` means the entry never expires.
    pub time: i32,
}

/// The kinds of objects to return or select on a parcel, as the `ReturnType` of
/// [`Session::return_parcel_objects`](crate::Session::return_parcel_objects) and
/// [`Session::select_parcel_objects`](crate::Session::select_parcel_objects). A
/// bitfield: combine the constants with [`ParcelReturnType::union`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParcelReturnType(pub u32);

impl ParcelReturnType {
    /// No objects (`RT_NONE`).
    pub const NONE: Self = Self(1 << 0);
    /// Objects owned by the parcel's owner (`RT_OWNER`).
    pub const OWNER: Self = Self(1 << 1);
    /// Objects set to the parcel's group (`RT_GROUP`).
    pub const GROUP: Self = Self(1 << 2);
    /// Objects owned by anyone else (`RT_OTHER`).
    pub const OTHER: Self = Self(1 << 3);
    /// Only the objects in the supplied id list (`RT_LIST`).
    pub const LIST: Self = Self(1 << 4);
    /// Objects that are for sale (`RT_SELL`).
    pub const SELL: Self = Self(1 << 5);

    /// Combines two sets of return-type bits.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

/// The settings to apply to a parcel via
/// [`Session::update_parcel`](crate::Session::update_parcel)
/// (`ParcelPropertiesUpdate`). Start from [`ParcelUpdate::default`] and set the
/// fields to change; `local_id` is required (from [`ParcelInfo::local_id`]).
#[derive(Debug, Clone, PartialEq)]
pub struct ParcelUpdate {
    /// The parcel's region-local id (from [`ParcelInfo::local_id`]).
    pub local_id: i32,
    /// The parcel flags bitfield to set.
    pub parcel_flags: ParcelFlags,
    /// The sale price in L$ (when [`ParcelFlags::FOR_SALE`] is set).
    pub sale_price: i32,
    /// The parcel name.
    pub name: String,
    /// The parcel description.
    pub description: String,
    /// The streaming music URL.
    pub music_url: String,
    /// The streaming media URL.
    pub media_url: String,
    /// The media texture id.
    pub media_id: Uuid,
    /// Whether to auto-scale the media to the prim face.
    pub media_auto_scale: bool,
    /// The group the parcel is set to.
    pub group_id: Uuid,
    /// The price of a parcel pass in L$.
    pub pass_price: i32,
    /// How many hours a parcel pass lasts.
    pub pass_hours: f32,
    /// The parcel's search category.
    pub category: ParcelCategory,
    /// The only agent allowed to buy the parcel (nil for anyone).
    pub auth_buyer_id: Uuid,
    /// The parcel snapshot texture id.
    pub snapshot_id: Uuid,
    /// The teleport-landing location within the parcel.
    pub user_location: Vector,
    /// The direction an arriving agent faces at the landing point.
    pub user_look_at: Vector,
    /// The landing type (`0` = blocked, `1` = landing point, `2` = anywhere).
    pub landing_type: u8,
}

impl Default for ParcelUpdate {
    fn default() -> Self {
        Self {
            local_id: 0,
            parcel_flags: ParcelFlags::from_bits(0),
            sale_price: 0,
            name: String::new(),
            description: String::new(),
            music_url: String::new(),
            media_url: String::new(),
            media_id: Uuid::nil(),
            media_auto_scale: false,
            group_id: Uuid::nil(),
            pass_price: 0,
            pass_hours: 0.0,
            category: ParcelCategory::None,
            auth_buyer_id: Uuid::nil(),
            snapshot_id: Uuid::nil(),
            user_location: Vector {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            user_look_at: Vector {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            landing_type: 0,
        }
    }
}

/// A change to one of an estate's access lists, applied via
/// [`Session::update_estate_access`](crate::Session::update_estate_access)
/// (`EstateOwnerMessage` method `estateaccessdelta`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EstateAccessDelta {
    /// Add an agent to the allowed-access list.
    AllowedAgentAdd,
    /// Remove an agent from the allowed-access list.
    AllowedAgentRemove,
    /// Add a group to the allowed-group list.
    AllowedGroupAdd,
    /// Remove a group from the allowed-group list.
    AllowedGroupRemove,
    /// Add an agent to the ban list.
    BannedAgentAdd,
    /// Remove an agent from the ban list.
    BannedAgentRemove,
    /// Add an estate manager.
    ManagerAdd,
    /// Remove an estate manager.
    ManagerRemove,
}

impl EstateAccessDelta {
    /// The `estateaccessdelta` flag bit for this change (matching the reference
    /// viewer's `ESTATE_ACCESS_*` constants).
    #[must_use]
    pub const fn to_u32(self) -> u32 {
        match self {
            Self::AllowedAgentAdd => 1 << 2,
            Self::AllowedAgentRemove => 1 << 3,
            Self::AllowedGroupAdd => 1 << 4,
            Self::AllowedGroupRemove => 1 << 5,
            Self::BannedAgentAdd => 1 << 6,
            Self::BannedAgentRemove => 1 << 7,
            Self::ManagerAdd => 1 << 8,
            Self::ManagerRemove => 1 << 9,
        }
    }
}

/// Which estate access list a [`Event::EstateAccessList`] carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EstateAccessKind {
    /// The allowed-agents list.
    AllowedAgents,
    /// The allowed-groups list.
    AllowedGroups,
    /// The banned-agents list.
    BannedAgents,
    /// The estate-managers list.
    Managers,
}

/// An estate's configuration, parsed from an `EstateOwnerMessage`
/// `estateupdateinfo` reply to
/// [`Session::request_estate_info`](crate::Session::request_estate_info).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EstateInfo {
    /// The estate name.
    pub estate_name: String,
    /// The estate owner's id.
    pub estate_owner: Uuid,
    /// The estate id.
    pub estate_id: u32,
    /// The raw estate-flags bitfield.
    pub estate_flags: u32,
    /// The sun position (when the estate uses a fixed sun).
    pub sun_position: u32,
    /// The parent estate id.
    pub parent_estate: u32,
    /// The estate covenant's notecard id (nil if none).
    pub covenant_id: Uuid,
    /// When the covenant last changed (Unix timestamp).
    pub covenant_timestamp: u32,
    /// The estate's abuse-report email address.
    pub abuse_email: String,
}

/// The settings to apply to a region via
/// [`Session::set_region_info`](crate::Session::set_region_info)
/// (`EstateOwnerMessage` method `setregioninfo`). Start from
/// [`RegionInfoUpdate::default`] and set the fields to change.
#[derive(Debug, Clone, PartialEq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each bool is a distinct region toggle in the setregioninfo wire message"
)]
pub struct RegionInfoUpdate {
    /// Block terraforming by non-owners.
    pub block_terraform: bool,
    /// Block flying.
    pub block_fly: bool,
    /// Allow damage (enable combat).
    pub allow_damage: bool,
    /// Allow residents to resell land.
    pub allow_land_resell: bool,
    /// The maximum concurrent agents.
    pub agent_limit: i32,
    /// The object (prim) bonus multiplier.
    pub object_bonus: f32,
    /// The region maturity rating.
    pub maturity: Maturity,
    /// Restrict pushing (no-push).
    pub restrict_pushobject: bool,
    /// Allow parcel join/subdivide by owners.
    pub allow_parcel_changes: bool,
}

impl Default for RegionInfoUpdate {
    fn default() -> Self {
        Self {
            block_terraform: false,
            block_fly: false,
            allow_damage: false,
            allow_land_resell: true,
            agent_limit: 40,
            object_bonus: 1.0,
            maturity: Maturity::Pg,
            restrict_pushobject: false,
            allow_parcel_changes: true,
        }
    }
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

/// A kind of world-map overlay item requested via `MapItemRequest` (the
/// `GridItemType`). [`MapItemType::AgentLocations`] gives the avatar "green
/// dots"; the land-for-sale and event types give the corresponding map overlays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapItemType {
    /// The region's telehub, if any (`1`).
    Telehub,
    /// PG-rated events (`2`).
    PgEvent,
    /// Mature-rated events (`3`).
    MatureEvent,
    /// Avatar locations — the map's "green dots" (`6`).
    AgentLocations,
    /// Parcels for sale, non-adult (`7`).
    LandForSale,
    /// Classified ads (`8`).
    Classified,
    /// Adult-rated events (`9`).
    AdultEvent,
    /// Parcels for sale in adult regions (`10`).
    AdultLandForSale,
    /// Any other grid item type, preserved verbatim.
    Other(u32),
}

impl MapItemType {
    /// Classifies a `GridItemType` wire value.
    #[must_use]
    pub const fn from_u32(value: u32) -> Self {
        match value {
            1 => Self::Telehub,
            2 => Self::PgEvent,
            3 => Self::MatureEvent,
            6 => Self::AgentLocations,
            7 => Self::LandForSale,
            8 => Self::Classified,
            9 => Self::AdultEvent,
            10 => Self::AdultLandForSale,
            other => Self::Other(other),
        }
    }

    /// The wire value for this item type.
    #[must_use]
    pub const fn to_u32(self) -> u32 {
        match self {
            Self::Telehub => 1,
            Self::PgEvent => 2,
            Self::MatureEvent => 3,
            Self::AgentLocations => 6,
            Self::LandForSale => 7,
            Self::Classified => 8,
            Self::AdultEvent => 9,
            Self::AdultLandForSale => 10,
            Self::Other(value) => value,
        }
    }
}

/// A single world-map overlay item from a `MapItemReply`. Coordinates are
/// **global** metres (region origin plus the in-region offset).
///
/// The meaning of `extra`/`extra2` depends on the item's [`MapItemType`]:
/// - [`MapItemType::AgentLocations`]: `extra` is the avatar count at this spot.
/// - [`MapItemType::Telehub`]: `extra2` is `0` for a hub, `1` for an infohub.
/// - [`MapItemType::LandForSale`] / [`MapItemType::AdultLandForSale`]: `extra` is
///   the parcel area in m², `extra2` the sale price in L$.
/// - event types: `extra` is the event id, `extra2` packs the event flags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapItem {
    /// The item's global x coordinate in metres.
    pub global_x: u32,
    /// The item's global y coordinate in metres.
    pub global_y: u32,
    /// The item's identifier (a parcel/event id, or nil for avatar dots).
    pub id: Uuid,
    /// Type-specific context (count, area, event id — see [`MapItem`]).
    pub extra: i32,
    /// Type-specific context (sale price, hub kind, flags — see [`MapItem`]).
    pub extra2: i32,
    /// The item's name (region/parcel/event name, or a hash for avatar dots).
    pub name: String,
}

impl MapItem {
    /// The handle of the region this item sits in, derived from its global
    /// coordinates (the global position with the in-region offset masked off).
    #[must_use]
    pub fn region_handle(&self) -> u64 {
        let region_x = u64::from(self.global_x & !0xFF);
        let region_y = u64::from(self.global_y & !0xFF);
        (region_x << 32) | region_y
    }

    /// The item's x offset within its region (0–255 metres).
    #[must_use]
    pub const fn local_x(&self) -> u32 {
        self.global_x & 0xFF
    }

    /// The item's y offset within its region (0–255 metres).
    #[must_use]
    pub const fn local_y(&self) -> u32 {
        self.global_y & 0xFF
    }
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

/// A scripted-object dialog (`llDialog`/`llTextBox`), parsed from a
/// `ScriptDialog`. Reply with
/// [`Session::reply_script_dialog`](crate::Session::reply_script_dialog), passing
/// the chosen button's index/label on [`chat_channel`](ScriptDialog::chat_channel).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptDialog {
    /// The object id that raised the dialog (the reply target).
    pub object_id: Uuid,
    /// The object's name.
    pub object_name: String,
    /// The object owner's first name.
    pub owner_first_name: String,
    /// The object owner's last name.
    pub owner_last_name: String,
    /// The object owner's agent id (nil if the sim did not include it).
    pub owner_id: Uuid,
    /// The dialog message text.
    pub message: String,
    /// The hidden chat channel the button reply is sent on.
    pub chat_channel: i32,
    /// The dialog's icon (texture id).
    pub image_id: Uuid,
    /// The button labels, in order (the reply carries the chosen index/label).
    pub buttons: Vec<String>,
}

impl ScriptDialog {
    /// The magic single-button label an `llTextBox` uses instead of real
    /// buttons. When [`buttons`](Self::buttons) is exactly this, the object is
    /// requesting free-text input rather than a button choice.
    pub const TEXT_BOX_BUTTON: &'static str = "!!llTextBox!!";

    /// Whether this dialog is an `llTextBox` free-text prompt (a single
    /// [`TEXT_BOX_BUTTON`](Self::TEXT_BOX_BUTTON) button).
    #[must_use]
    pub fn is_text_box(&self) -> bool {
        self.buttons.len() == 1
            && self
                .buttons
                .first()
                .is_some_and(|button| button == Self::TEXT_BOX_BUTTON)
    }
}

/// The permissions an in-world script may request via `llRequestPermissions`, a
/// bitfield shared by `ScriptQuestion` (request) and `ScriptAnswerYes` (grant).
/// The flag values match the LSL `PERMISSION_*` constants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScriptPermissions(pub i32);

impl ScriptPermissions {
    /// Debit the agent's account (`PERMISSION_DEBIT`).
    pub const DEBIT: i32 = 1 << 1;
    /// Take control inputs (`PERMISSION_TAKE_CONTROLS`).
    pub const TAKE_CONTROLS: i32 = 1 << 2;
    /// Trigger animations on the agent (`PERMISSION_TRIGGER_ANIMATION`).
    pub const TRIGGER_ANIMATION: i32 = 1 << 4;
    /// Attach to the agent (`PERMISSION_ATTACH`).
    pub const ATTACH: i32 = 1 << 5;
    /// Change link-set membership (`PERMISSION_CHANGE_LINKS`).
    pub const CHANGE_LINKS: i32 = 1 << 7;
    /// Track the agent's camera (`PERMISSION_TRACK_CAMERA`).
    pub const TRACK_CAMERA: i32 = 1 << 10;
    /// Control the agent's camera (`PERMISSION_CONTROL_CAMERA`).
    pub const CONTROL_CAMERA: i32 = 1 << 11;
    /// Teleport the agent (`PERMISSION_TELEPORT`).
    pub const TELEPORT: i32 = 1 << 12;
    /// Participate in an experience (`PERMISSION_EXPERIENCE`).
    pub const EXPERIENCE: i32 = 1 << 13;
    /// Silently manage estate access (`PERMISSION_SILENT_ESTATE_MANAGEMENT`).
    pub const SILENT_ESTATE_MANAGEMENT: i32 = 1 << 14;
    /// Override the agent's animations (`PERMISSION_OVERRIDE_ANIMATIONS`).
    pub const OVERRIDE_ANIMATIONS: i32 = 1 << 15;
    /// Return objects (`PERMISSION_RETURN_OBJECTS`).
    pub const RETURN_OBJECTS: i32 = 1 << 16;

    /// Whether all of the bits in `mask` are granted/requested.
    #[must_use]
    pub const fn contains(self, mask: i32) -> bool {
        self.0 & mask == mask
    }
}

/// A scripted-object permission request (`llRequestPermissions`), parsed from a
/// `ScriptQuestion`. Grant (a subset) with
/// [`Session::answer_script_permissions`](crate::Session::answer_script_permissions).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptPermissionRequest {
    /// The task (object) id holding the script.
    pub task_id: Uuid,
    /// The script item id within the object.
    pub item_id: Uuid,
    /// The object's name.
    pub object_name: String,
    /// The object owner's name.
    pub object_owner: String,
    /// The experience id requesting, or nil if not an experience.
    pub experience_id: Uuid,
    /// The permissions requested.
    pub permissions: ScriptPermissions,
}

/// A scripted-object request to open a URL (`llLoadURL`), parsed from a
/// `LoadURL`. There is no reply; the client decides whether to open the URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadUrlRequest {
    /// The object's name.
    pub object_name: String,
    /// The object id.
    pub object_id: Uuid,
    /// The object owner's agent (or group) id.
    pub owner_id: Uuid,
    /// Whether [`owner_id`](Self::owner_id) is a group rather than an agent.
    pub owner_is_group: bool,
    /// The accompanying message text.
    pub message: String,
    /// The URL the object asks to open.
    pub url: String,
}

/// A scripted-object request to teleport the agent (`llMapDestination` /
/// `ScriptTeleportRequest`). There is no direct reply; the client may initiate a
/// teleport to the named region/position.
#[derive(Debug, Clone, PartialEq)]
pub struct ScriptTeleportRequest {
    /// The requesting object's name.
    pub object_name: String,
    /// The destination region (simulator) name.
    pub region_name: String,
    /// The destination position within the region, in metres.
    pub position: (f32, f32, f32),
    /// The look-at direction on arrival.
    pub look_at: (f32, f32, f32),
}

/// The kind of thing a mute-list entry blocks, from the `MuteType` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MuteType {
    /// A mute by display name only (no specific id).
    ByName,
    /// A muted agent (avatar).
    Agent,
    /// A muted object.
    Object,
    /// A muted group.
    Group,
    /// A muted external (e.g. hypergrid) entity.
    External,
    /// An unrecognised mute-type value, preserved verbatim.
    Unknown(i32),
}

impl MuteType {
    /// Classifies a `MuteType` wire value.
    #[must_use]
    pub const fn from_i32(value: i32) -> Self {
        match value {
            0 => Self::ByName,
            1 => Self::Agent,
            2 => Self::Object,
            3 => Self::Group,
            4 => Self::External,
            other => Self::Unknown(other),
        }
    }

    /// The wire value for this mute type.
    #[must_use]
    pub const fn to_i32(self) -> i32 {
        match self {
            Self::ByName => 0,
            Self::Agent => 1,
            Self::Object => 2,
            Self::Group => 3,
            Self::External => 4,
            Self::Unknown(other) => other,
        }
    }
}

/// The per-entry mute flags bitfield. **Each set bit is an *exception*** — it
/// means "do *not* mute this aspect" — so `MuteFlags(0)` mutes everything (the
/// usual case). The flag values match the viewer's `LLMute::flag*` constants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MuteFlags(pub u32);

impl MuteFlags {
    /// Do not mute the target's text chat.
    pub const ALLOW_TEXT_CHAT: u32 = 0x1;
    /// Do not mute the target's voice chat.
    pub const ALLOW_VOICE_CHAT: u32 = 0x2;
    /// Do not mute the target's particles.
    pub const ALLOW_PARTICLES: u32 = 0x4;
    /// Do not mute the target's object sounds.
    pub const ALLOW_OBJECT_SOUNDS: u32 = 0x8;

    /// Whether all of the bits in `mask` are set.
    #[must_use]
    pub const fn contains(self, mask: u32) -> bool {
        self.0 & mask == mask
    }
}

/// One entry in the agent's mute (block) list, parsed from the downloaded mute
/// file ([`Event::MuteList`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MuteEntry {
    /// The muted entity's id (nil for a [`MuteType::ByName`] mute).
    pub id: Uuid,
    /// The muted entity's name.
    pub name: String,
    /// What kind of entity is muted.
    pub mute_type: MuteType,
    /// The per-entry exception flags.
    pub flags: MuteFlags,
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
