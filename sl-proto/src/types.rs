//! Public value types of the sans-I/O session: its inputs and outputs.

use std::net::SocketAddr;

use sl_types::attachment::AttachmentPoint;
use sl_types::lsl::{Rotation, Vector};
use sl_types::money::LindenAmount;
use sl_wire::{
    ExperienceInfo, LoginRequest, MediaEntry, ParcelFlags, ParcelVoiceInfo, RenderMaterialEntry,
    VoiceAccountInfo,
};
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

/// The agent's camera viewpoint, advertised to the simulator in every
/// `AgentUpdate`.
///
/// The simulator uses the camera position and look direction (together with the
/// draw distance — see [`Session::set_draw_distance`](crate::Session::set_draw_distance))
/// to build the agent's **interest list**: which objects, avatars and regions it
/// streams, and how the per-category bandwidth (the throttle) is spent. So the
/// camera follows where the agent actually *looks*, not where it stands.
///
/// The three axes form a right-handed orthonormal frame in the SL convention
/// (`at × left = up`): `at_axis` is the forward look direction, `left_axis`
/// points to the camera's left, and `up_axis` is its up vector. Until a client
/// sets one with [`Session::set_camera`](crate::Session::set_camera), the
/// session advertises [`Camera::region_center`] — the historic region-centre
/// viewpoint looking along +X.
#[derive(Debug, Clone, PartialEq)]
pub struct Camera {
    /// The camera's region-local position (the eye point).
    pub center: Vector,
    /// The unit vector the camera looks along (forward / "at").
    pub at_axis: Vector,
    /// The camera's unit left vector.
    pub left_axis: Vector,
    /// The camera's unit up vector.
    pub up_axis: Vector,
}

impl Camera {
    /// Builds a camera from an explicit position and orthonormal basis. The
    /// caller is responsible for the axes being unit-length and mutually
    /// orthogonal in the SL convention (`at × left = up`); use
    /// [`Camera::looking_at`] to derive them from a target point instead.
    #[must_use]
    pub const fn new(center: Vector, at_axis: Vector, left_axis: Vector, up_axis: Vector) -> Self {
        Self {
            center,
            at_axis,
            left_axis,
            up_axis,
        }
    }

    /// The default camera advertised before any [`Session::set_camera`](crate::Session::set_camera):
    /// positioned at the centre of a standard 256 m region (128, 128, 30),
    /// looking along +X with the world-up basis. This is the viewpoint the
    /// session used unconditionally before camera control existed, so it keeps
    /// the interest list anchored at the region origin until a real viewpoint is
    /// supplied.
    #[must_use]
    pub const fn region_center() -> Self {
        Self::new(
            Vector {
                x: 128.0,
                y: 128.0,
                z: 30.0,
            },
            Vector {
                x: 1.0,
                y: 0.0,
                z: 0.0,
            },
            Vector {
                x: 0.0,
                y: 1.0,
                z: 0.0,
            },
            Vector {
                x: 0.0,
                y: 0.0,
                z: 1.0,
            },
        )
    }

    /// Builds a camera at `eye` looking toward `target`, deriving the
    /// orthonormal `at`/`left`/`up` basis with the world up vector (+Z), exactly
    /// as the reference viewer's `LLCoordFrame::lookAt` does
    /// (`left = up × at`, `up = at × left`).
    ///
    /// Degenerate inputs fall back gracefully: if `eye` and `target` coincide the
    /// camera looks along +X, and if the look direction is vertical (so the
    /// world-up cross product vanishes) the left axis defaults to +Y.
    #[must_use]
    pub fn looking_at(eye: Vector, target: Vector) -> Self {
        const FORWARD: Vector = Vector {
            x: 1.0,
            y: 0.0,
            z: 0.0,
        };
        const SIDE: Vector = Vector {
            x: 0.0,
            y: 1.0,
            z: 0.0,
        };
        const WORLD_UP: Vector = Vector {
            x: 0.0,
            y: 0.0,
            z: 1.0,
        };
        let at = normalize(&sub(&target, &eye)).unwrap_or(FORWARD);
        let left = normalize(&cross(&WORLD_UP, &at)).unwrap_or(SIDE);
        // `at` and `left` are unit and orthogonal, so their cross product is
        // already unit-length — no further normalisation needed.
        let up = cross(&at, &left);
        Self::new(eye, at, left, up)
    }
}

impl Default for Camera {
    /// [`Camera::region_center`] — the region-centre viewpoint used before any
    /// explicit camera was set.
    fn default() -> Self {
        Self::region_center()
    }
}

/// Vector difference `a - b`.
fn sub(a: &Vector, b: &Vector) -> Vector {
    Vector {
        x: a.x - b.x,
        y: a.y - b.y,
        z: a.z - b.z,
    }
}

/// The cross product `a × b`.
fn cross(a: &Vector, b: &Vector) -> Vector {
    Vector {
        x: a.y * b.z - a.z * b.y,
        y: a.z * b.x - a.x * b.z,
        z: a.x * b.y - a.y * b.x,
    }
}

/// Normalises `v` to unit length, returning `None` if it is too short to give a
/// stable direction (so callers can substitute a sensible default axis).
fn normalize(v: &Vector) -> Option<Vector> {
    let length = (v.x * v.x + v.y * v.y + v.z * v.z).sqrt();
    if length < 1e-6 {
        return None;
    }
    Some(Vector {
        x: v.x / length,
        y: v.y / length,
        z: v.z / length,
    })
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
    /// A scripted control of the parcel's streaming media
    /// (`ParcelMediaCommandMessage` — a `llParcelMediaCommandList` from an
    /// in-world script). The simulator pushes this to tell viewers to
    /// play/pause/stop/loop the parcel media surface, or to carry a new
    /// time/agent target; the richer URL/texture/type/size changes arrive as a
    /// separate [`Event::ParcelMediaUpdate`].
    ParcelMediaCommand {
        /// The raw `Flags` bitfield: each set bit (`1 << command`) marks a
        /// [`ParcelMediaCommand`] whose field is meaningful in this message.
        flags: u32,
        /// The media command being issued.
        command: ParcelMediaCommand,
        /// The command argument, when relevant (the seek offset in seconds for
        /// [`ParcelMediaCommand::Time`]; `0.0` otherwise).
        time: f32,
    },
    /// The parcel's media settings changed (`ParcelMediaUpdate`): the streaming
    /// media surface's new URL, replacement texture, MIME type, and dimensions.
    /// Pushed by the simulator when a parcel's media is reconfigured (e.g. via
    /// the About Land dialog or `llSetPrimMediaParams`-adjacent parcel APIs).
    ParcelMediaUpdate(ParcelMediaUpdateInfo),
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
        /// The failure reason (the plain, server-supplied message string).
        reason: String,
        /// The structured alert, if the simulator attached one (`AlertInfo`): a
        /// localizable message *key* plus its substitution parameters, which a
        /// localized client looks up instead of showing the raw
        /// [`reason`](Self::TeleportFailed::reason). `None` for the timeout path
        /// and for simulators that send no alert block.
        alert_info: Option<AlertInfo>,
    },
    /// A teleport completed at the protocol level (`TeleportFinish`, delivered
    /// over UDP or the CAPS event queue): the destination region's identity,
    /// maturity rating, and the flags describing how and why the teleport
    /// happened arrived. The circuit handover then proceeds; once the
    /// destination handshake completes an [`Event::RegionChanged`] follows.
    TeleportFinished {
        /// The destination region handle.
        region_handle: u64,
        /// The destination simulator's UDP address.
        sim: SocketAddr,
        /// The destination region's maturity / content rating (`SimAccess`).
        maturity: Maturity,
        /// How and why the teleport happened (`TeleportFlags`): lure, landmark,
        /// login, telehub, home, and so on.
        flags: TeleportFlags,
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
    /// An avatar's classified ads (`AvatarClassifiedReply`), in response to
    /// [`Session::request_avatar_classifieds`](crate::Session::request_avatar_classifieds).
    AvatarClassifieds {
        /// The avatar whose classifieds these are.
        target_id: Uuid,
        /// The classifieds (id and name only; fetch details separately).
        classifieds: Vec<AvatarClassified>,
    },
    /// The full details of one pick (`PickInfoReply`), in response to
    /// [`Session::request_pick_info`](crate::Session::request_pick_info).
    PickInfo(Box<PickInfo>),
    /// The full details of one classified ad (`ClassifiedInfoReply`), in
    /// response to
    /// [`Session::request_classified_info`](crate::Session::request_classified_info).
    ClassifiedInfo(Box<ClassifiedInfo>),
    /// Account-level facts from the login response (home, start look-at,
    /// maturity ratings, group limit, and the shared Library roots). Emitted
    /// once, right after [`Event::CircuitEstablished`].
    Account(Box<LoginAccount>),
    /// The agent's inventory folder skeleton (every folder, without item
    /// contents), parsed from the login response. Emitted once, right after
    /// [`Event::CircuitEstablished`], when the login provided it.
    InventorySkeleton(Vec<InventoryFolder>),
    /// The shared Library inventory's folder skeleton (every folder, without
    /// item contents), parsed from the login response (`inventory-skel-lib`).
    /// Emitted once, right after [`Event::CircuitEstablished`], when the login
    /// provided a non-empty library tree. The owning agent is
    /// [`LoginAccount::library_owner`].
    LibraryInventory(Vec<InventoryFolder>),
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
    /// A single inventory item was created or its asset replaced by the
    /// simulator (`UpdateCreateInventoryItem`) — typically the reply to a
    /// [`Session::create_inventory_item`](crate::Session::create_inventory_item)
    /// or an item the sim materialised (e.g. an accepted inventory offer). The
    /// item is also merged into the session's live inventory cache.
    InventoryItemCreated {
        /// Whether the simulator approved (accepted) the creation.
        sim_approved: bool,
        /// The transaction id echoed from the originating request (nil if none).
        transaction_id: Uuid,
        /// The async callback id echoed from the originating request (`0` if
        /// none), used by a client to correlate the reply with its request.
        callback_id: u32,
        /// The created/updated item.
        item: InventoryItem,
    },
    /// A batch inventory update the simulator pushed (`BulkUpdateInventory`),
    /// e.g. after a copy, a give, or a server-side reorganisation. The folders
    /// and items are merged into the session's live inventory cache, keeping the
    /// cached tree current without a re-fetch.
    InventoryBulkUpdate {
        /// The transaction id of the originating operation (nil if none).
        transaction_id: Uuid,
        /// Created or updated folders.
        folders: Vec<InventoryFolder>,
        /// Created or updated items.
        items: Vec<InventoryItem>,
        /// Per-item async callback correlation: `(item_id, callback_id)` pairs for
        /// every updated item carrying a non-zero `CallbackID`. The simulator
        /// echoes the callback id allocated by the originating request (e.g.
        /// [`Session::copy_inventory_item`](crate::Session::copy_inventory_item)),
        /// so a client can match the returned callback id to the resulting item
        /// even when the result arrives as a `BulkUpdateInventory` rather than an
        /// [`Event::InventoryItemCreated`]. Empty for delivery paths that carry no
        /// callback id (the CAPS event-queue / AIS3 forms).
        item_callbacks: Vec<(Uuid, u32)>,
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
        /// The total role count the simulator reports across all packets of this
        /// (potentially multi-packet) reply, so a client can tell when the role
        /// set is complete.
        role_count: i32,
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
        /// The total role↔member pair count the simulator reports across all
        /// packets of this (potentially multi-packet) reply, so a client can tell
        /// when the pairing set is complete.
        total_pairs: u32,
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
    /// A message was received in an ad-hoc conference IM session (an
    /// `ImprovedInstantMessage` with the `IM_SESSION_SEND` dialog and
    /// `from_group` clear). The session id distinguishes a conference from a
    /// group session ([`Event::GroupSessionMessage`], where `from_group` is set).
    ConferenceSessionMessage {
        /// The conference's IM session id.
        session_id: Uuid,
        /// The sender's agent id.
        from_agent_id: Uuid,
        /// The sender's display name.
        from_name: String,
        /// The message text.
        message: String,
    },
    /// A participant joined or left an ad-hoc conference IM session (an
    /// `ImprovedInstantMessage` with the `IM_SESSION_INVITE`/`SessionAdd` or
    /// `IM_SESSION_LEAVE` dialog and `from_group` clear).
    ConferenceSessionParticipant {
        /// The conference's IM session id.
        session_id: Uuid,
        /// The participant's agent id.
        agent_id: Uuid,
        /// `true` when the participant joined, `false` when they left.
        joined: bool,
    },
    /// The agent was invited to an ad-hoc conference / group IM session, delivered
    /// over the CAPS event queue as a `ChatterBoxInvitation` (the modern path).
    /// Join by sending into the session
    /// ([`Session::send_conference_message`](crate::Session::send_conference_message)).
    ConferenceInvited {
        /// The IM session id to join.
        session_id: Uuid,
        /// The inviting agent's id.
        from_agent_id: Uuid,
        /// The inviting agent's display name.
        from_name: String,
        /// The session kind multiplexed over the invitation (from the
        /// `message_params.type` dialog byte): a group chat
        /// ([`ImDialog::SessionGroupStart`]), an ad-hoc conference
        /// ([`ImDialog::SessionConferenceStart`]), or a plain session add — so a
        /// client can tell a group IM from an ad-hoc conference before joining.
        dialog: ImDialog,
        /// Whether the invitation comes from a group (a group IM) rather than an
        /// ad-hoc conference of individual agents. For a group IM the
        /// [`session_id`](Self::ConferenceInvited::session_id) is the group id.
        from_group: bool,
        /// The session's human-readable name (the group or conference name),
        /// supplied directly in the event body; for a group IM the same label is
        /// also carried inside
        /// [`binary_bucket`](Self::ConferenceInvited::binary_bucket).
        session_name: String,
        /// The accompanying message text.
        message: String,
        /// The source region's id (nil if not provided — OpenSim sends nil).
        region_id: Uuid,
        /// The inviting agent's region-local position, in metres.
        position: (f32, f32, f32),
        /// The parent estate id of the source.
        parent_estate_id: u32,
        /// The inviting agent's timestamp (`0` when unset).
        timestamp: u32,
        /// The dialog-dependent binary payload. For a group IM this carries the
        /// group/session name used to label the session; empty for an ordinary
        /// conference invite.
        binary_bucket: Vec<u8>,
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
    /// The result of an
    /// [`Session::eject_group_members`](crate::Session::eject_group_members)
    /// (`EjectGroupMemberReply`).
    EjectGroupMemberResult {
        /// The group a member was ejected from.
        group_id: Uuid,
        /// Whether the ejection succeeded.
        success: bool,
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
        sit_position: Vector,
        /// The seated orientation relative to the object — which way the avatar
        /// faces once seated.
        sit_rotation: Rotation,
        /// The scripted-sit camera eye position relative to the seat
        /// (`llSetCameraEyeOffset`); the zero vector when the seat's script sets
        /// no custom camera.
        camera_eye_offset: Vector,
        /// The scripted-sit camera focus point relative to the seat
        /// (`llSetCameraAtOffset`); the zero vector when the seat's script sets
        /// no custom camera.
        camera_at_offset: Vector,
        /// Whether sitting forces the avatar into mouselook (set by vehicles and
        /// weapon huds).
        force_mouselook: bool,
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
    /// The simulator's frame time-dilation changed (`RegionData.TimeDilation`,
    /// carried by every object-update message). The value is the fraction of
    /// real time the region's physics frame is achieving, `0.0`..=`1.0`: `1.0`
    /// is a healthy, fully-keeping-up region, lower values mean the sim is
    /// lagging and an object's interpolated (dead-reckoned) motion should be
    /// scaled down accordingly between updates. Emitted only when the value
    /// changes for a region (the raw 16-bit value is de-duplicated), not on every
    /// object update.
    TimeDilation {
        /// The region whose time dilation this is.
        region_handle: u64,
        /// The time dilation, `0.0`..=`1.0` (the raw `u16` divided by `65535`).
        dilation: f32,
    },
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
    /// An object's per-face **media-on-a-prim** settings, decoded from an
    /// `ObjectMedia` capability GET reply (the runtime `RequestObjectMedia`
    /// command). Each [`faces`](Event::ObjectMedia::faces) slot is the
    /// [`MediaEntry`] for one prim face, or `None` for a face with no media. Set
    /// it with the `SetObjectMedia` command or navigate a single face with
    /// `NavigateObjectMedia`.
    ObjectMedia {
        /// The object the media belongs to.
        object_id: Uuid,
        /// The media version string (`x-mv:<serial>/<uuid>`); the same value the
        /// object's [`Object::media_url`] carries, advanced on every change.
        version: String,
        /// Per-face media, one slot per prim face in order; `None` for a face
        /// that has no media.
        faces: Vec<Option<MediaEntry>>,
    },
    /// A GLTF (PBR) material **override** pushed by the simulator in a
    /// `GenericStreamingMessage` (method `0x4175`): the per-face material
    /// changes layered on an object's base GLTF materials. Per the asset-fetch
    /// scope the per-face GLTF documents are not parsed — each is surfaced as
    /// its raw notation-LLSD bytes in [`overrides`](Event::GltfMaterialOverride::overrides),
    /// positionally correlated with [`faces`](Event::GltfMaterialOverride::faces).
    /// Arrives on the root and neighbouring (child) regions.
    GltfMaterialOverride {
        /// The region the override applies in (the source simulator's handle, or
        /// `0` if not yet known).
        region_handle: u64,
        /// The region-local id of the overridden object.
        local_id: u32,
        /// The face indices carrying an override, in order.
        faces: Vec<u8>,
        /// The raw per-face override LLSD (notation-encoded), one per face in
        /// [`faces`](Event::GltfMaterialOverride::faces); left undecoded.
        overrides: Vec<Vec<u8>>,
    },
    /// The legacy (normal/specular) materials returned by a `RenderMaterials`
    /// capability POST (the runtime `RequestRenderMaterials` command) — the
    /// path stock OpenSim implements. Each [`RenderMaterialEntry`] pairs a
    /// material id (referenced per face by a `TextureEntry`) with its decoded
    /// `LegacyMaterial`.
    RenderMaterials(Vec<RenderMaterialEntry>),
    /// The reply to a `ModifyMaterialParams` capability POST (the runtime
    /// `ModifyMaterialParams` command, which sets GLTF materials on object
    /// faces): whether the simulator accepted the change, and any message.
    MaterialParamsResult {
        /// Whether the modification succeeded.
        success: bool,
        /// The simulator's status message (empty on success).
        message: String,
    },
    /// The reply to a `ProvisionVoiceAccountRequest` capability POST (the runtime
    /// `RequestVoiceAccount` command): the agent's voice-chat account — either
    /// legacy Vivox SIP credentials or a WebRTC JSEP answer (see
    /// [`VoiceAccountInfo`]). This is the grid-side *signalling* only; opening
    /// the Vivox or WebRTC audio session itself is out of this client's scope.
    VoiceAccountProvisioned(VoiceAccountInfo),
    /// The reply to a `ParcelVoiceInfoRequest` capability POST (the runtime
    /// `RequestParcelVoiceInfo` command): the current parcel's voice channel
    /// (its `channel_uri`, absent when the parcel has no voice).
    ParcelVoiceInfo(ParcelVoiceInfo),
    /// The reply to a `GetExperienceInfo` capability GET (the runtime
    /// `RequestExperienceInfo` command): the metadata for the requested
    /// experiences, with any ids the grid could not resolve folded in as
    /// [`missing`](ExperienceInfo::missing) placeholders.
    ExperienceInfo(Vec<ExperienceInfo>),
    /// The reply to a `FindExperienceByName` capability GET (the runtime
    /// `FindExperiences` command): one page of experiences matching the query.
    ExperienceSearchResults(Vec<ExperienceInfo>),
    /// The reply to a `GetExperiences` capability GET or an `ExperiencePreferences`
    /// PUT/DELETE (the runtime `RequestExperiencePermissions` /
    /// `SetExperiencePermission` commands): the agent's per-experience preferences
    /// — the experiences it has `allowed` and those it has `blocked`.
    ExperiencePermissions {
        /// The experiences the agent admits.
        allowed: Vec<Uuid>,
        /// The experiences the agent blocks.
        blocked: Vec<Uuid>,
    },
    /// The reply to an `AgentExperiences` capability GET (the runtime
    /// `RequestOwnedExperiences` command): the experiences the agent owns.
    OwnedExperiences(Vec<Uuid>),
    /// The reply to a `GetAdminExperiences` capability GET (the runtime
    /// `RequestAdminExperiences` command): the experiences the agent administers.
    AdminExperiences(Vec<Uuid>),
    /// The reply to a `GetCreatorExperiences` capability GET (the runtime
    /// `RequestCreatorExperiences` command): the experiences the agent created.
    CreatorExperiences(Vec<Uuid>),
    /// The reply to a `GroupExperiences` capability GET (the runtime
    /// `RequestGroupExperiences` command): the experiences the queried
    /// [`group_id`](Self::GroupExperiences::group_id) owns.
    GroupExperiences {
        /// The group the experiences belong to (the queried id, echoed by the
        /// runtime since the cap reply does not carry it).
        group_id: Uuid,
        /// The experiences the group owns.
        experience_ids: Vec<Uuid>,
    },
    /// The reply to an `IsExperienceAdmin` capability GET (the runtime
    /// `RequestExperienceAdmin` command): whether the agent administers the
    /// queried experience.
    ExperienceAdminStatus {
        /// The queried experience (echoed by the runtime).
        experience_id: Uuid,
        /// Whether the agent is an admin of it.
        is_admin: bool,
    },
    /// The reply to an `IsExperienceContributor` capability GET (the runtime
    /// `RequestExperienceContributor` command): whether the agent contributes to
    /// the queried experience.
    ExperienceContributorStatus {
        /// The queried experience (echoed by the runtime).
        experience_id: Uuid,
        /// Whether the agent is a contributor to it.
        is_contributor: bool,
    },
    /// The reply to an `UpdateExperience` capability POST (the runtime
    /// `UpdateExperience` command): the experience's metadata after the edit.
    ExperienceUpdated(ExperienceInfo),
    /// The reply to a `RegionExperiences` capability GET or POST (the runtime
    /// `RequestRegionExperiences` / `SetRegionExperiences` commands): the region's
    /// experience allow / block / trust lists.
    RegionExperiences {
        /// The experiences the region allows.
        allowed: Vec<Uuid>,
        /// The experiences the region blocks.
        blocked: Vec<Uuid>,
        /// The experiences the region trusts (privileged, key-grid scope).
        trusted: Vec<Uuid>,
    },
    /// A decoded terrain (or wind/cloud/water) patch arrived in a `LayerData`
    /// message and was added to or refreshed in the terrain cache. For a
    /// [`Land`](TerrainLayerType::Land) patch the [`values`](TerrainPatch::values)
    /// are ground heights in metres for one 16×16 block of the region; see
    /// [`Session::terrain_height`](crate::Session::terrain_height) and
    /// [`Session::terrain_patches`](crate::Session::terrain_patches).
    TerrainPatch(Box<TerrainPatch>),
    /// A requested texture finished downloading: the reassembled
    /// [`Texture`] from the legacy UDP image path
    /// ([`Session::request_texture`](crate::Session::request_texture)) or the
    /// HTTP `GetTexture` capability (the runtime `FetchTexture` command). The
    /// image bytes are the raw (usually JPEG-2000) codestream, not pixels.
    TextureReceived(Box<Texture>),
    /// A requested texture does not exist in the asset store
    /// (`ImageNotInDatabase`), or its HTTP fetch returned 404. Carries the
    /// texture's UUID.
    TextureNotFound(Uuid),
    /// A requested generic asset finished downloading: the reassembled
    /// [`Asset`] from the UDP transfer path
    /// ([`Session::request_asset`](crate::Session::request_asset)) or the HTTP
    /// `GetAsset`/`GetMesh` capability.
    AssetReceived(Box<Asset>),
    /// A generic asset [transfer](crate::Session::request_asset) has begun: the
    /// simulator answered the `TransferRequest` with a success `TransferInfo`,
    /// so the asset exists and its bytes will follow as `TransferPacket`s
    /// (surfaced together as a single [`AssetReceived`](Event::AssetReceived)).
    /// Carries the declared total asset [`size`](Event::AssetTransferStarted::size)
    /// in bytes — useful for a progress indicator or buffer preallocation before
    /// the packets arrive. A *non*-success `TransferInfo` instead surfaces
    /// [`AssetTransferFailed`](Event::AssetTransferFailed) and no data follows.
    AssetTransferStarted {
        /// The asset UUID that is being transferred.
        asset_id: Uuid,
        /// The asset class being transferred.
        asset_type: AssetType,
        /// The declared total size of the asset in bytes (the `TransferInfo`
        /// `Size` field). The simulator can send `0` when it does not know the
        /// size up front.
        size: i32,
    },
    /// A generic asset [transfer](crate::Session::request_asset) failed: the
    /// simulator reported a non-success [`TransferStatus`] (e.g. the asset is
    /// missing or permission was denied), or the HTTP fetch failed.
    AssetTransferFailed {
        /// The asset UUID that was requested.
        asset_id: Uuid,
        /// The asset class that was requested.
        asset_type: AssetType,
        /// The failure status.
        status: TransferStatus,
    },
    /// A legacy UDP asset upload finished (`AssetUploadComplete`), in reply to an
    /// [`AssetUploadRequest`](crate::Session::upload_asset_udp) — whether the
    /// asset was inlined in the request or streamed over the `Xfer` path. Carries
    /// the stored asset's UUID (the same value
    /// [`Session::upload_asset_udp`](crate::Session::upload_asset_udp) returned),
    /// its class, and the success flag. The legacy path stores only the asset; it
    /// does not create an inventory item (use the CAPS
    /// [`Command::UploadAsset`](../sl_client_tokio/enum.Command.html) path for
    /// that).
    AssetUploadComplete {
        /// The stored asset's UUID.
        asset_id: Uuid,
        /// The uploaded asset class.
        asset_type: AssetType,
        /// Whether the simulator stored the asset successfully.
        success: bool,
    },
    /// A CAPS asset upload finished successfully (the modern two-step uploader:
    /// `NewFileAgentInventory`, `UploadBakedTexture`, or one of the
    /// `Update*AgentInventory` capabilities). Carries the newly stored asset's
    /// UUID and, when the upload created or updated an inventory item, that item's
    /// UUID (`None` for a temporary baked texture, which has no inventory item).
    AssetUploaded {
        /// The newly stored asset's UUID (`new_asset`).
        new_asset: Uuid,
        /// The created/updated inventory item's UUID (`new_inventory_item`), or
        /// `None` when the upload produced no inventory item (a baked texture).
        new_inventory_item: Option<Uuid>,
    },
    /// A CAPS asset upload failed: the capability POST returned an error state,
    /// omitted the uploader URL, or the HTTP request failed. Carries a
    /// human-readable reason (the grid's error message when one was supplied).
    AssetUploadFailed {
        /// A description of the failure.
        reason: String,
    },
    /// Another avatar's appearance arrived (`AvatarAppearance`): its decoded
    /// baked textures and visual parameters, pushed when the avatar comes into
    /// range or restyles. Use the baked texture ids (see [`avatar_texture`]) with
    /// [`Session::request_texture`](crate::Session::request_texture) to render it.
    AvatarAppearance(Box<AvatarAppearance>),
    /// The agent's own current wearables (`AgentWearablesUpdate`): the simulator's
    /// authoritative view of the outfit, pushed at login and after every wearable
    /// change. Request a refresh with
    /// [`Session::request_wearables`](crate::Session::request_wearables); change
    /// the outfit with [`Session::set_wearing`](crate::Session::set_wearing).
    AgentWearables {
        /// The update's serial number (increments on each change; used to drop
        /// out-of-order updates).
        serial: u32,
        /// The worn wearables.
        wearables: Vec<Wearable>,
    },
    /// The grid's reply to a server-side appearance-bake request (the modern
    /// Second Life `UpdateAvatarAppearance` capability POST; see
    /// [`CAP_UPDATE_AVATAR_APPEARANCE`](crate::CAP_UPDATE_AVATAR_APPEARANCE)).
    /// The baked appearance itself arrives separately as an
    /// [`Event::AvatarAppearance`] over UDP; this only reports whether the bake
    /// request was accepted.
    ServerAppearanceUpdate {
        /// Whether the grid accepted the bake request.
        success: bool,
        /// The grid's error message when [`success`](Event::ServerAppearanceUpdate::success)
        /// is `false`, if any.
        error: Option<String>,
        /// On a Current-Outfit-Folder version mismatch, the COF version the grid
        /// expected — re-request with this version. `None` otherwise.
        expected_cof_version: Option<i32>,
    },
    /// The simulator's reply to a baked-texture cache query
    /// (`AgentCachedTextureResponse`), in response to
    /// [`Session::request_cached_textures`](crate::Session::request_cached_textures):
    /// for each queried slot, the cached baked texture id the simulator already
    /// has (nil if it has none, meaning that bake must be uploaded).
    CachedTextureResponse {
        /// The serial number echoed from the request.
        serial: i32,
        /// The cached baked textures, as `(texture slot index, cached texture id)`
        /// pairs; a nil id means no cached bake for that slot.
        textures: Vec<(u8, Uuid)>,
    },
    /// Another avatar's currently-playing animations (`AvatarAnimation`),
    /// pushed by the simulator whenever an avatar's animation set changes. The
    /// list is the *complete* set of animations that avatar is now playing — an
    /// animation that stops simply drops out of a later update — so a renderer
    /// or bot should treat each event as the authoritative state, not a delta.
    /// Trigger the agent's own animations with
    /// [`Session::play_animation`](crate::Session::play_animation) /
    /// [`Session::stop_animation`](crate::Session::stop_animation).
    AvatarAnimation {
        /// The avatar whose animation state this describes.
        avatar_id: Uuid,
        /// The animations that avatar is currently playing.
        animations: Vec<PlayingAnimation>,
        /// The raw `PhysicalAvatarEventList` blocks — one opaque `TypeData`
        /// byte blob per block. These carry physics/ragdoll events; neither
        /// the reference viewer's `process_avatar_animation` nor OpenSim
        /// assigns the payload any documented structure (the viewer ignores
        /// the block and OpenSim never populates it), so the bytes are
        /// surfaced verbatim rather than decoded. Almost always empty.
        physical_events: Vec<Vec<u8>>,
    },
    /// A one-shot spatial sound the simulator wants played at a fixed location
    /// (`SoundTrigger` — e.g. a scripted `llTriggerSound`, a collision sound, or
    /// a sound from a neighbouring region). Unlike [`Event::AttachedSound`] this
    /// sound is not bound to an object: it plays once at `position` and is then
    /// forgotten. Fetch the clip with
    /// [`Session::request_asset`](crate::Session::request_asset).
    SoundTrigger {
        /// The sound asset to play.
        sound_id: Uuid,
        /// The owner of the object that triggered the sound.
        owner_id: Uuid,
        /// The object that triggered the sound (nil if none).
        object_id: Uuid,
        /// The triggering object's parent (root) id, or `None` when the object
        /// is itself the root (the wire `ParentID` is nil).
        parent_id: Option<Uuid>,
        /// The handle of the region the sound plays in. Because a `SoundTrigger`
        /// can originate in a neighbouring region, this need not be the agent's
        /// current region.
        region_handle: u64,
        /// The sound's position, region-local to `region_handle`.
        position: Vector,
        /// The linear gain (volume), `0.0`..=`1.0`.
        gain: f32,
    },
    /// A looping or one-shot sound attached to an in-world object
    /// (`AttachedSound` — a scripted `llPlaySound`/`llLoopSound`). The sound
    /// follows the object; a later [`Event::AttachedSoundGainChange`] for the
    /// same `object_id` changes its volume, and the object stops the sound by
    /// sending a fresh `AttachedSound` with [`SoundFlags::STOP`]. Fetch the clip
    /// with [`Session::request_asset`](crate::Session::request_asset).
    AttachedSound {
        /// The sound asset to play.
        sound_id: Uuid,
        /// The object the sound is attached to.
        object_id: Uuid,
        /// The object owner's id.
        owner_id: Uuid,
        /// The linear gain (volume), `0.0`..=`1.0`.
        gain: f32,
        /// The playback flags (loop / sync / queue / stop).
        flags: SoundFlags,
    },
    /// The volume of a sound already attached to an object changed
    /// (`AttachedSoundGainChange`). Applies to the current [`Event::AttachedSound`]
    /// for the same `object_id`.
    AttachedSoundGainChange {
        /// The object whose attached-sound volume changed.
        object_id: Uuid,
        /// The new linear gain (volume), `0.0`..=`1.0`.
        gain: f32,
    },
    /// The simulator asks the viewer to pre-fetch one or more sound assets it is
    /// about to play (`PreloadSound`), so playback is not delayed by the fetch.
    /// A client that wants gap-free audio can fetch each clip up front with
    /// [`Session::request_asset`](crate::Session::request_asset); a client that
    /// does not care can ignore this event.
    PreloadSound {
        /// The sounds to pre-fetch (each with its owning object and owner).
        sounds: Vec<SoundPreload>,
    },
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
    /// The avatar collision (foot/standing) plane, present only for avatar
    /// updates and `None` for ordinary objects. The four components are the
    /// plane equation `[nx, ny, nz, d]`: a unit normal and a distance, giving
    /// the surface the avatar is standing on (niche, for inverse-kinematics /
    /// grounding). Decoded from the `LLVector4` prefix the simulator prepends to
    /// an avatar's motion blob.
    pub collision_plane: Option<[f32; 4]>,
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
    /// The raw object `state` byte, passed through verbatim. Its meaning
    /// depends on [`pcode`](Self::pcode): for a tree/grass it is the species,
    /// and for an *attachment* (a non-zero-state prim) it holds the
    /// attachment-point id with its nibbles swapped — read it via
    /// [`attachment_point`](Self::attachment_point), not directly.
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
    /// Decode with
    /// [`decode_texture_entry`](crate::decode_texture_entry).
    pub texture_entry: Vec<u8>,
    /// The raw texture-animation (`TextureAnim`) blob (`llSetTextureAnim`),
    /// undecoded; empty if the object has no texture animation.
    pub texture_anim: Vec<u8>,
    /// The decoded [`TextureAnimation`] (`llSetTextureAnim`) parameters, or `None`
    /// when the object has no texture animation (or the blob is not the expected
    /// 16 bytes). Decoded from [`texture_anim`](Self::texture_anim).
    pub texture_animation: Option<TextureAnimation>,
    /// The decoded path/profile [`shape`](PrimShapeParams) parameters of a volume
    /// prim. Zeroed for object classes that carry no shape (e.g. avatars).
    pub shape: PrimShapeParams,
    /// The raw particle-system (`PSBlock`) blob (`llParticleSystem`), undecoded;
    /// empty if the object has no particle system.
    pub particle_system: Vec<u8>,
    /// The decoded [`ParticleSystem`] (`llParticleSystem`) parameters, or `None`
    /// when the object has no particle system (or the blob fails to decode).
    /// Decoded from [`particle_system`](Self::particle_system).
    pub particles: Option<ParticleSystem>,
    /// The raw generic-`Data` field: tree/grass genome bytes for a tree object,
    /// or the linkset prim count for a root prim (one byte). Empty if absent.
    pub data: Vec<u8>,
    /// The raw `ExtraParams` blob (flexi/light/sculpt/mesh parameters), as
    /// received on the wire.
    pub extra_params: Vec<u8>,
    /// The decoded [`ExtraParams`](ObjectExtraParams) sub-blocks
    /// (flexi/light/sculpt/light-image/extended-mesh/render-material/reflection
    /// probe), populated from both full and compressed `ObjectUpdate`s.
    pub extra: ObjectExtraParams,
    /// The object's extended properties (creator, permissions, name,
    /// description, …) once an [`Event::ObjectProperties`] has been received for
    /// it; `None` until then.
    pub properties: Option<ObjectProperties>,
    /// The deprecated legacy joint type (`JointType`). Part of the long-obsolete
    /// physical-joint mechanism; carried by a full `ObjectUpdate` but virtually
    /// always zero on modern grids. Surfaced verbatim for fidelity. Not carried
    /// by compressed updates (left zero there).
    pub joint_type: u8,
    /// The deprecated legacy joint pivot point (`JointPivot`), in object-local
    /// metres. See [`joint_type`](Self::joint_type); usually the zero vector.
    pub joint_pivot: Vector,
    /// The deprecated legacy joint axis or anchor (`JointAxisOrAnchor`). See
    /// [`joint_type`](Self::joint_type); usually the zero vector.
    pub joint_axis_or_anchor: Vector,
}

/// The `ATTACHMENT_ADD` flag the simulator may OR into a freshly un-swizzled
/// attachment id to mark an "add" (rather than "replace") attach. It is not
/// part of the attachment point itself and is stripped before the point is
/// returned. Mirrors the reference viewer's `ATTACHMENT_ADD` constant.
const ATTACHMENT_ADD: u8 = 0x80;

/// Reverse the simulator's attachment-point nibble-swap on an object `state`
/// byte and strip the [`ATTACHMENT_ADD`] flag, yielding the plain attachment
/// point. For an attachment the point id is hidden in `state` with its upper
/// and lower nibbles swapped (kept for backward compatibility with old objects
/// that used only the upper nibble); this mirrors the reference viewer's
/// `ATTACHMENT_ID_FROM_STATE` macro.
const fn attachment_point_from_state(state: u8) -> u8 {
    (((state & 0xf0) >> 4) | ((state & 0x0f) << 4)) & !ATTACHMENT_ADD
}

impl Object {
    /// The raw, un-swizzled attachment-point id this object is worn on, or
    /// `None` if it is not an attachment.
    ///
    /// For an attachment the simulator hides the attachment-point id inside the
    /// raw [`state`](Self::state) byte with its upper and lower nibbles swapped
    /// (kept for backward compatibility with very old objects that used only the
    /// upper nibble), so reading `state` directly yields the wrong number. This
    /// accessor reverses the swap — the reference viewer's
    /// `ATTACHMENT_ID_FROM_STATE` — and strips the transient `ATTACHMENT_ADD`
    /// (`0x80`) bit, returning the plain attachment-point id (e.g. `1` = chest,
    /// `6` = right hand, `35` = HUD center 1).
    ///
    /// Returns `None` for anything that is not a non-zero-`state` prim: plain
    /// prims (`state == 0`) and trees/grass (whose `state` byte instead carries
    /// the species), mirroring the viewer's `LLVOVolume::isAttachment`.
    ///
    /// Prefer [`attachment_point`](Self::attachment_point) for a named point;
    /// this raw form additionally covers any future id the [`AttachmentPoint`]
    /// enum does not yet name.
    #[must_use]
    pub const fn attachment_point_id(&self) -> Option<u8> {
        if self.pcode == pcode::PRIMITIVE && self.state != 0 {
            Some(attachment_point_from_state(self.state))
        } else {
            None
        }
    }

    /// The named attachment point this object is worn on, or `None` if it is not
    /// an attachment (or is attached to an id the [`AttachmentPoint`] enum does
    /// not name — see [`attachment_point_id`](Self::attachment_point_id) for the
    /// raw form).
    ///
    /// Decodes the un-swizzled [`attachment_point_id`](Self::attachment_point_id)
    /// into the shared [`AttachmentPoint`] enum, covering both avatar points
    /// (e.g. chest, right hand) and HUD points (e.g. top-left, center).
    #[must_use]
    pub fn attachment_point(&self) -> Option<AttachmentPoint> {
        self.attachment_point_id()
            .and_then(|id| AttachmentPoint::from_repr(usize::from(id)))
    }

    /// The object's [`name_value`](Self::name_value) pairs, parsed from the raw
    /// newline-separated string into structured [`NameValue`] entries (e.g. an
    /// attachment's `AttachItemID`). Empty when the object carries none. Lines
    /// that have no name/type are skipped. The parse mirrors the reference
    /// viewer's `LLNameValue` string constructor: each line is
    /// `name type [class] [sendto] data`, where `class` and `sendto` are present
    /// only when the token is one of the recognized keywords.
    #[must_use]
    pub fn name_values(&self) -> Vec<NameValue> {
        self.name_value
            .lines()
            .filter_map(NameValue::parse_line)
            .collect()
    }

    /// The data value of the first [`name_value`](Self::name_value) pair named
    /// `name`, or `None` if the object has no such pair. A convenience over
    /// [`name_values`](Self::name_values) for the common single-key lookup (e.g.
    /// `object.name_value_data("AttachItemID")`).
    #[must_use]
    pub fn name_value_data(&self, name: &str) -> Option<String> {
        self.name_value
            .lines()
            .filter_map(NameValue::parse_line)
            .find(|pair| pair.name == name)
            .map(|pair| pair.value)
    }
}

/// One parsed entry of an object's packed `name_value` string (the reference
/// viewer's `LLNameValue`). Produced by [`Object::name_values`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameValue {
    /// The entry's name (the lookup key, e.g. `AttachItemID`).
    pub name: String,
    /// The declared value type token (`STRING`, `F32`, `S32`, `VEC3`, `U32`,
    /// `ASSET`, `U64`, …); empty if the line carried none.
    pub value_type: String,
    /// The access class token: `R` (read-only) or `RW` (read-write). Defaults to
    /// `RW` when the line omits it (matching the viewer).
    pub class: String,
    /// The send-to token: `S`/`DS`/`SV`/`DSV`. Defaults to `S` (sim only) when the
    /// line omits it (matching the viewer).
    pub sendto: String,
    /// The entry's data value, as the verbatim remainder of the line.
    pub value: String,
}

impl NameValue {
    /// Parses one `name_value` line into a [`NameValue`], or `None` when the line
    /// has no name or no type. The `class` and `sendto` tokens are optional and
    /// recognized only by the viewer's keyword sets; anything else is taken as the
    /// start of the data value.
    fn parse_line(line: &str) -> Option<Self> {
        const CLASS_TOKENS: [&str; 5] = ["R", "RW", "READ_ONLY", "READ_WRITE", "CALLBACK"];
        const SENDTO_TOKENS: [&str; 8] = [
            "S",
            "DS",
            "SV",
            "DSV",
            "SIM",
            "SIM_SPACE",
            "SIM_VIEWER",
            "SIM_SPACE_VIEWER",
        ];
        let mut rest = line.trim_start();
        let take = |rest: &mut &str| -> Option<String> {
            let trimmed = rest.trim_start();
            if trimmed.is_empty() {
                return None;
            }
            let end = trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
            let token = trimmed.get(..end)?.to_owned();
            *rest = trimmed.get(end..).unwrap_or("");
            Some(token)
        };
        let name = take(&mut rest)?;
        let value_type = take(&mut rest)?;
        // `class` is present only if the next token is a recognized keyword.
        let next = rest.trim_start();
        let class = if CLASS_TOKENS
            .iter()
            .any(|token| starts_with_token(next, token))
        {
            take(&mut rest).unwrap_or_default()
        } else {
            "RW".to_owned()
        };
        // `sendto` is likewise present only for a recognized keyword.
        let next = rest.trim_start();
        let sendto = if SENDTO_TOKENS
            .iter()
            .any(|token| starts_with_token(next, token))
        {
            take(&mut rest).unwrap_or_default()
        } else {
            "S".to_owned()
        };
        Some(Self {
            name,
            value_type,
            class,
            sendto,
            value: rest.trim_start().to_owned(),
        })
    }
}

/// Whether `text` begins with the whitespace-delimited token `token` (the token
/// followed by whitespace or end-of-string), used to decide whether an optional
/// `name_value` class/sendto keyword is present.
fn starts_with_token(text: &str, token: &str) -> bool {
    text.strip_prefix(token)
        .is_some_and(|tail| tail.is_empty() || tail.starts_with(char::is_whitespace))
}

/// The path/profile shape parameters of a volume prim, as carried (in raw
/// quantized wire form) by both full and compressed `ObjectUpdate`s. The values
/// are the simulator's quantized integers — the same encoding [`PrimShape`] uses
/// to *send* a shape — not dequantized floats; the quantization for each field
/// matches the like-named [`PrimShape`] field (e.g. `path_begin / 0.00002`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PrimShapeParams {
    /// The path curve byte (`LL_PCODE_PATH_*`).
    pub path_curve: u8,
    /// The profile curve byte (`LL_PCODE_PROFILE_*`, hollow shape in the high
    /// nibble).
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
}

/// The decoded `ExtraParams` sub-blocks of an [`Object`]. The `ExtraParams` blob
/// in an `ObjectUpdate` is a list of optional typed parameters (each a Linden
/// `LLNetworkData` subtype); each field here is present only if the object
/// carries that parameter.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ObjectExtraParams {
    /// Flexible-path ("flexi") parameters (`PARAMS_FLEXIBLE`, `0x10`).
    pub flexible: Option<FlexibleData>,
    /// Point/spot-light parameters (`PARAMS_LIGHT`, `0x20`).
    pub light: Option<LightData>,
    /// Sculpt / mesh parameters (`PARAMS_SCULPT` `0x30` or `PARAMS_MESH`
    /// `0x60` — a mesh is carried in the same block).
    pub sculpt: Option<SculptData>,
    /// Projected-light texture parameters (`PARAMS_LIGHT_IMAGE`, `0x40`).
    pub light_image: Option<LightImage>,
    /// Extended-mesh flags (`PARAMS_EXTENDED_MESH`, `0x70`).
    pub extended_mesh: Option<ExtendedMesh>,
    /// Per-face GLTF (PBR) render-material asset references
    /// (`PARAMS_RENDER_MATERIAL`, `0x80`); empty if the object has none.
    pub render_material: Vec<RenderMaterialRef>,
    /// Reflection-probe parameters (`PARAMS_REFLECTION_PROBE`, `0x90`).
    pub reflection_probe: Option<ReflectionProbe>,
}

/// Flexible-path ("flexi") parameters (`LLFlexibleObjectData`): the prim's path
/// bends under simulated softbody physics.
#[derive(Debug, Clone, PartialEq)]
pub struct FlexibleData {
    /// The softness / simulate-LOD level (0–3): how finely the path flexes.
    pub softness: u8,
    /// Path stiffness (resistance to bending).
    pub tension: f32,
    /// Air friction (how quickly motion damps).
    pub air_friction: f32,
    /// Gravity applied to the path tip.
    pub gravity: f32,
    /// Sensitivity to region wind.
    pub wind_sensitivity: f32,
    /// A constant force pushing the path (zero if the sim did not send it).
    pub user_force: Vector,
}

/// Point/spot-light parameters (`LLLightParams`): the object emits light.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LightData {
    /// The light colour, RGBA as sent on the wire (sRGB).
    pub color: [u8; 4],
    /// The light radius, in metres.
    pub radius: f32,
    /// The spotlight cutoff angle.
    pub cutoff: f32,
    /// The light falloff exponent.
    pub falloff: f32,
}

/// Sculpt or mesh parameters (`LLSculptParams`): the prim's shape comes from a
/// sculpt texture or a mesh asset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SculptData {
    /// The sculpt texture or mesh asset id.
    pub texture: Uuid,
    /// The sculpt type byte (`LL_SCULPT_TYPE_*` in the low bits — sphere/torus/
    /// plane/cylinder/mesh — plus invert/mirror/animesh flag bits).
    pub sculpt_type: u8,
}

/// Projected-light texture parameters (`LLLightImageParams`): a light projects
/// an image.
#[derive(Debug, Clone, PartialEq)]
pub struct LightImage {
    /// The projected texture id.
    pub texture: Uuid,
    /// The projection parameters `(field-of-view, focus, ambiance)`.
    pub params: Vector,
}

/// Extended-mesh flags (`LLExtendedMeshParams`), e.g. animated-mesh
/// (`ANIMATED_MESH_ENABLED_FLAG`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExtendedMesh {
    /// The extended-mesh flag bits.
    pub flags: u32,
}

/// One per-face GLTF (PBR) render-material reference
/// (`LLRenderMaterialParams::Entry`): the material asset applied to a face.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderMaterialRef {
    /// The texture-entry (face) index the material applies to.
    pub face: u8,
    /// The render-material asset id (an `AT_MATERIAL` / GLTF material).
    pub material_id: Uuid,
}

/// A PBR **reflection probe**: a Second Life-specific per-object property
/// (`ExtraParams` type `0x90`, `LLReflectionProbeParams`) marking an object as a
/// probe that captures the surrounding environment for image-based lighting and
/// reflections. The probe itself is rendered by the viewer (there is no asset to
/// fetch); these are just its volume parameters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReflectionProbe {
    /// The probe's ambiance (irradiance) scale.
    pub ambiance: f32,
    /// The near-clip distance of the probe's reflection capture, in metres.
    pub clip_distance: f32,
    /// Whether the influence volume is a box (`true`) rather than a sphere
    /// (`false`) — `FLAG_BOX_VOLUME`.
    pub is_box: bool,
    /// Whether dynamic objects (e.g. avatars) are rendered into the probe —
    /// `FLAG_DYNAMIC`.
    pub is_dynamic: bool,
    /// Whether the probe drives a realtime mirror — `FLAG_MIRROR`.
    pub is_mirror: bool,
}

/// Mode (`mMode`) bit flags for a [`TextureAnimation`] (`LLTextureAnim`), matching
/// the LSL `llSetTextureAnim` flags and the reference viewer's `LLTextureAnim`
/// enum.
pub mod texture_anim_mode {
    /// The animation is running (`ON`); cleared means the prim is static.
    pub const ON: u8 = 0x01;
    /// Loop the animation (`LOOP`).
    pub const LOOP: u8 = 0x02;
    /// Play the animation in reverse (`REVERSE`).
    pub const REVERSE: u8 = 0x04;
    /// Bounce back and forth rather than restart (`PING_PONG`).
    pub const PING_PONG: u8 = 0x08;
    /// Slide smoothly rather than step frame-by-frame (`SMOOTH`).
    pub const SMOOTH: u8 = 0x10;
    /// Rotate the texture instead of paging frames (`ROTATE`); `start`/`length`
    /// are then start/end angles in radians.
    pub const ROTATE: u8 = 0x20;
    /// Scale the texture instead of paging frames (`SCALE`); `start`/`length` are
    /// then start/end scales.
    pub const SCALE: u8 = 0x40;
}

/// A prim's texture-animation parameters (`TextureAnim` / `LLTextureAnim`, set by
/// `llSetTextureAnim`): a 16-byte block driving an animated, rotating, or scaling
/// texture on one or all of a prim's faces. Decoded by
/// [`decode_texture_anim`](crate::decode_texture_anim).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextureAnimation {
    /// The mode bit field (see [`texture_anim_mode`]). With
    /// [`ON`](texture_anim_mode::ON) clear the prim has no active animation.
    pub mode: u8,
    /// The face the animation applies to, or `-1` for all faces.
    pub face: i8,
    /// The number of horizontal frames in the texture grid (the `x` argument of
    /// `llSetTextureAnim`). For a non-[`SMOOTH`](texture_anim_mode::SMOOTH)
    /// animation a zero is treated by the viewer as 1.
    pub size_x: u8,
    /// The number of vertical frames in the texture grid (the `y` argument).
    pub size_y: u8,
    /// The start frame (or, in [`ROTATE`](texture_anim_mode::ROTATE)/
    /// [`SCALE`](texture_anim_mode::SCALE) mode, the start angle/scale).
    pub start: f32,
    /// The number of frames to display (or, in rotate/scale mode, the end
    /// angle/scale).
    pub length: f32,
    /// The playback rate, in frames per second (or radians/second when rotating).
    pub rate: f32,
}

/// Particle-flow pattern (`mPattern`) values for a [`ParticleSystem`], matching
/// the reference viewer's `LLPartSysData::LL_PART_SRC_PATTERN_*` enum.
pub mod particle_pattern {
    /// Particles drop from the source (`LL_PART_SRC_PATTERN_DROP`).
    pub const DROP: u8 = 0x01;
    /// Particles explode outward from the source (`LL_PART_SRC_PATTERN_EXPLODE`).
    pub const EXPLODE: u8 = 0x02;
    /// Particles emit along an angle (`LL_PART_SRC_PATTERN_ANGLE`).
    pub const ANGLE: u8 = 0x04;
    /// Particles emit within a cone (`LL_PART_SRC_PATTERN_ANGLE_CONE`).
    pub const ANGLE_CONE: u8 = 0x08;
    /// Particles emit within an empty (hollow) cone
    /// (`LL_PART_SRC_PATTERN_ANGLE_CONE_EMPTY`).
    pub const ANGLE_CONE_EMPTY: u8 = 0x10;
}

/// A prim's particle system (`PSBlock` / `LLPartSysData`, set by
/// `llParticleSystem`): the source parameters plus the template particle
/// parameters the source emits. Decoded by
/// [`decode_particle_system`](crate::decode_particle_system) from both the legacy
/// (86-byte) and modern (size-prefixed, glow/blend-extended) wire forms.
#[derive(Debug, Clone, PartialEq)]
pub struct ParticleSystem {
    /// The system CRC (a non-zero value marks a live system; zero means "no
    /// system").
    pub crc: u32,
    /// The source flags (`LL_PART_SRC_*` — object-relative accel/velocity and the
    /// new-angle flag).
    pub flags: u32,
    /// The emission pattern (see [`particle_pattern`]).
    pub pattern: u8,
    /// The source's maximum lifetime, in seconds (0 = forever).
    pub max_age: f32,
    /// The age at which the system starts, in seconds.
    pub start_age: f32,
    /// The inner emission angle, in radians (for the angle/cone patterns).
    pub inner_angle: f32,
    /// The outer emission angle, in radians.
    pub outer_angle: f32,
    /// How often a burst of particles is emitted, in seconds.
    pub burst_rate: f32,
    /// The emission radius, in metres.
    pub burst_radius: f32,
    /// The minimum particle launch speed, in metres/second.
    pub burst_speed_min: f32,
    /// The maximum particle launch speed, in metres/second.
    pub burst_speed_max: f32,
    /// How many particles are emitted per burst.
    pub burst_part_count: u8,
    /// The angular velocity of the emission axis, in radians/second per axis.
    pub angular_velocity: Vector,
    /// The acceleration applied to each particle, in metres/second² per axis.
    pub acceleration: Vector,
    /// The particle texture asset id (nil for the default).
    pub texture_id: Uuid,
    /// The target object the particles follow/aim at (for the target patterns and
    /// the `TARGET_POS`/`TARGET_LINEAR` particle flags); nil if none.
    pub target_id: Uuid,
    /// The per-particle flags (`LL_PART_*_MASK` — interpolation, bounce, wind,
    /// follow, emissive, beam, ribbon, and the glow/blend system-set bits).
    pub part_flags: u32,
    /// Each particle's maximum age, in seconds.
    pub part_max_age: f32,
    /// The particle start colour, RGBA as sent on the wire.
    pub part_start_color: [u8; 4],
    /// The particle end colour, RGBA as sent on the wire.
    pub part_end_color: [u8; 4],
    /// The particle start scale `(x, y)`, in metres.
    pub part_start_scale: [f32; 2],
    /// The particle end scale `(x, y)`, in metres.
    pub part_end_scale: [f32; 2],
    /// The particle start glow (0–1); 0 unless the system carries glow data.
    pub part_start_glow: f32,
    /// The particle end glow (0–1); 0 unless the system carries glow data.
    pub part_end_glow: f32,
    /// The source blend function (`LL_PART_BF_*`); defaults to source-alpha unless
    /// the system carries blend data.
    pub part_blend_func_source: u8,
    /// The destination blend function (`LL_PART_BF_*`); defaults to
    /// one-minus-source-alpha unless the system carries blend data.
    pub part_blend_func_dest: u8,
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
    /// The task-inventory serial; bumps whenever the object's contents change,
    /// so a client can detect task-inventory mutations without re-fetching.
    pub inventory_serial: i16,
    /// The inventory item this object was rezzed from (nil if not applicable),
    /// used to correlate an in-world object back to its inventory item — needed
    /// for attachments and "find in inventory".
    pub item_id: Uuid,
    /// The inventory folder the source item lives in (nil if not applicable).
    pub folder_id: Uuid,
    /// The task (object) this item came from, when it was rezzed from another
    /// object's contents (nil if not applicable).
    pub from_task_id: Uuid,
    /// The aggregate permission rollup across the linkset's contents — the
    /// build-floater "next owner can…" summary.
    pub aggregate_perms: u8,
    /// The aggregate permission rollup for textures across the linkset.
    pub aggregate_perm_textures: u8,
    /// The owner-facing aggregate permission rollup for textures.
    pub aggregate_perm_textures_owner: u8,
    /// The object's name.
    pub name: String,
    /// The object's description.
    pub description: String,
    /// The custom touch-action label, empty if none.
    pub touch_name: String,
    /// The custom sit-action label, empty if none.
    pub sit_name: String,
    /// The linkset's concatenated texture-asset ids (the wire carries them as a
    /// run of 16-byte UUIDs); empty if the sim sent none.
    pub texture_ids: Vec<Uuid>,
}

// ---------------------------------------------------------------------------
// Terrain heightmaps (#18): the patched-DCT-compressed `LayerData` layers.
// ---------------------------------------------------------------------------

/// The kind of layer carried in a `LayerData` message, identified by the
/// single-byte type code in the layer's group header. LAND is the terrain
/// heightmap (the one a renderer needs for the ground); WIND/CLOUD/WATER carry
/// the per-region wind field, cloud density, and water height respectively, in
/// the same patched-DCT encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerrainLayerType {
    /// Terrain heightmap (`'L'`). Each cell is a ground height in metres.
    Land,
    /// Wind field (`'7'`). Carries the per-patch wind velocity components.
    Wind,
    /// Cloud density (`'8'`).
    Cloud,
    /// Water height (`'W'`).
    Water,
    /// Terrain heightmap for a variable-sized ("large"/var) region (`'M'`),
    /// which packs the patch coordinates in 32 bits instead of 10.
    LandExtended,
    /// Wind field for a variable-sized region (`'9'`).
    WindExtended,
    /// Cloud density for a variable-sized region (`':'`).
    CloudExtended,
    /// Water height for a variable-sized region (`'X'`).
    WaterExtended,
    /// An unrecognised layer type code.
    Unknown(u8),
}

impl TerrainLayerType {
    /// Classifies a `LayerData` group-header layer-type code.
    #[must_use]
    pub const fn from_code(code: u8) -> Self {
        match code {
            b'L' => Self::Land,
            b'7' => Self::Wind,
            b'8' => Self::Cloud,
            b'W' => Self::Water,
            b'M' => Self::LandExtended,
            b'9' => Self::WindExtended,
            b':' => Self::CloudExtended,
            b'X' => Self::WaterExtended,
            other => Self::Unknown(other),
        }
    }

    /// The wire layer-type code for this layer.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Land => b'L',
            Self::Wind => b'7',
            Self::Cloud => b'8',
            Self::Water => b'W',
            Self::LandExtended => b'M',
            Self::WindExtended => b'9',
            Self::CloudExtended => b':',
            Self::WaterExtended => b'X',
            Self::Unknown(other) => other,
        }
    }

    /// Whether this is a variable-region ("large") layer, whose patches pack
    /// their coordinates in 32 bits rather than 10.
    #[must_use]
    pub const fn is_extended(self) -> bool {
        matches!(
            self,
            Self::LandExtended | Self::WindExtended | Self::CloudExtended | Self::WaterExtended
        )
    }

    /// Whether this is a terrain (ground-height) layer (`Land`/`LandExtended`).
    #[must_use]
    pub const fn is_land(self) -> bool {
        matches!(self, Self::Land | Self::LandExtended)
    }
}

/// One decoded terrain patch: a `size`×`size` block of values (row-major, the
/// row index running along the region's Y axis) at patch grid position
/// (`patch_x`, `patch_y`) within its region. A standard region is 16×16 patches
/// of 16×16 cells (256×256 metres); cell (`x`, `y`) within the patch maps to
/// region cell (`patch_x*size + x`, `patch_y*size + y`). For a [`Land`] patch
/// the values are ground heights in metres.
///
/// [`Land`]: TerrainLayerType::Land
#[derive(Debug, Clone, PartialEq)]
pub struct TerrainPatch {
    /// The region this patch belongs to (its `RegionHandle`), or 0 if not yet
    /// known for the originating simulator.
    pub region_handle: u64,
    /// The layer this patch belongs to.
    pub layer: TerrainLayerType,
    /// The patch column (grid X) within the region.
    pub patch_x: u32,
    /// The patch row (grid Y) within the region.
    pub patch_y: u32,
    /// The patch edge length in cells (16 for a standard region, 32 for a
    /// variable-region "large" patch).
    pub size: u32,
    /// The decoded values, row-major (`row * size + col`), length `size*size`.
    /// For a terrain layer these are ground heights in metres.
    pub values: Vec<f32>,
}

impl TerrainPatch {
    /// The value at cell (`x`, `y`) within the patch (`x`/`y` in `0..size`), or
    /// `None` if out of range. For a terrain layer this is a height in metres.
    #[must_use]
    pub fn value(&self, x: u32, y: u32) -> Option<f32> {
        if x >= self.size || y >= self.size {
            return None;
        }
        let index = usize::try_from(y.wrapping_mul(self.size).wrapping_add(x)).ok()?;
        self.values.get(index).copied()
    }
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

    /// Classifies the short maturity code carried by the login response
    /// `agent_access`/`agent_access_max` fields: `"PG"`, `"M"` (mature), or
    /// `"A"` (adult). Unrecognised or absent codes map to [`Maturity::Unknown`].
    #[must_use]
    pub fn from_login_access(code: Option<&str>) -> Self {
        match code {
            Some("PG") => Self::Pg,
            Some("M") => Self::Mature,
            Some("A") => Self::Adult,
            _ => Self::Unknown,
        }
    }
}

/// The flags describing how and why a teleport happened, carried by
/// `TeleportFinish` (and `TeleportProgress`) as the `TeleportFlags` U32
/// bitfield. Mirrors the reference viewer's `TELEPORT_FLAGS_*`
/// (`indra/llmessage/llteleportflags.h`). Surfaced by
/// [`Event::TeleportFinished`].
///
/// Note: OpenSim collapses the flags it sends on `TeleportFinish` to
/// [`VIA_LOCATION`](Self::VIA_LOCATION) (plus [`IS_FLYING`](Self::IS_FLYING)),
/// so the full set of `VIA_*` reasons is only observable on the Second Life
/// grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TeleportFlags(pub u32);

impl TeleportFlags {
    /// Set the agent's home to the teleport target (`SET_HOME_TO_TARGET`, a
    /// newbie leaving the prelude).
    pub const SET_HOME_TO_TARGET: u32 = 1 << 0;
    /// Set the agent's last location to the target (`SET_LAST_TO_TARGET`).
    pub const SET_LAST_TO_TARGET: u32 = 1 << 1;
    /// Teleport via a lure / teleport offer (`VIA_LURE`).
    pub const VIA_LURE: u32 = 1 << 2;
    /// Teleport via a landmark (`VIA_LANDMARK`).
    pub const VIA_LANDMARK: u32 = 1 << 3;
    /// Teleport via an explicit location (`VIA_LOCATION`).
    pub const VIA_LOCATION: u32 = 1 << 4;
    /// Teleport to the agent's home (`VIA_HOME`).
    pub const VIA_HOME: u32 = 1 << 5;
    /// Teleport via a telehub (`VIA_TELEHUB`).
    pub const VIA_TELEHUB: u32 = 1 << 6;
    /// Teleport as part of logging in (`VIA_LOGIN`).
    pub const VIA_LOGIN: u32 = 1 << 7;
    /// Teleport via a godlike lure (`VIA_GODLIKE_LURE`).
    pub const VIA_GODLIKE_LURE: u32 = 1 << 8;
    /// The teleport was performed with god powers (`GODLIKE`).
    pub const GODLIKE: u32 = 1 << 9;
    /// An emergency ("911") teleport (`FLAGS_911`).
    pub const NINE_ONE_ONE: u32 = 1 << 10;
    /// Cancelling the teleport is disabled (`DISABLE_CANCEL`, used by
    /// `llTeleportAgentHome`).
    pub const DISABLE_CANCEL: u32 = 1 << 11;
    /// Teleport via a region id (`VIA_REGION_ID`).
    pub const VIA_REGION_ID: u32 = 1 << 12;
    /// The agent was flying when the teleport started (`IS_FLYING`).
    pub const IS_FLYING: u32 = 1 << 13;
    /// Show the reset-home UI on arrival (`SHOW_RESET_HOME`).
    pub const SHOW_RESET_HOME: u32 = 1 << 14;
    /// Force a redirect to some location (`FORCE_REDIRECT`, used when kicking
    /// someone from land).
    pub const FORCE_REDIRECT: u32 = 1 << 15;
    /// Teleport via global coordinates (`VIA_GLOBAL_COORDS`).
    pub const VIA_GLOBAL_COORDS: u32 = 1 << 16;
    /// The teleport stays within the same region (`WITHIN_REGION`).
    pub const WITHIN_REGION: u32 = 1 << 17;

    /// Whether all of the bits in `mask` are set.
    #[must_use]
    pub const fn contains(self, mask: u32) -> bool {
        self.0 & mask == mask
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
///
/// (Not `Eq`: `water_height` / `billable_factor` are `f32`.)
#[derive(Debug, Clone, PartialEq)]
pub struct RegionIdentity {
    /// The region (simulator) name.
    pub sim_name: String,
    /// The raw 32-bit `RegionFlags` bitfield (decode with [`sl_wire::RegionFlags`]).
    pub region_flags: u32,
    /// The full 64-bit `RegionFlagsExtended` (from the `RegionInfo4` block); falls
    /// back to the zero-extended 32-bit [`Self::region_flags`] when the grid sends
    /// no `RegionInfo4` (e.g. OpenSim and older simulators).
    pub region_flags_extended: u64,
    /// The `RegionProtocols` capability bitfield (from `RegionInfo4`), or `0` when
    /// the grid sends no `RegionInfo4`.
    pub region_protocols: u64,
    /// The maturity / content rating.
    pub maturity: Maturity,
    /// The inferred product type.
    pub product: ProductType,
    /// The raw `ProductSKU` string (possibly empty, e.g. on OpenSim).
    pub product_sku: String,
    /// The raw `ProductName` string (possibly empty, e.g. on OpenSim).
    pub product_name: String,
    /// The region/estate owner's id.
    pub sim_owner: Uuid,
    /// Whether *this* agent is an estate manager for the region (gates estate UI).
    pub is_estate_manager: bool,
    /// The region's water height, in metres.
    pub water_height: f32,
    /// The billing factor applied to land tier in this region.
    pub billable_factor: f32,
}

/// A region's agent and object capacity plus estate/terrain/chat/combat settings,
/// parsed from `RegionInfo` (the reply to
/// [`Session::request_region_info`](crate::Session::request_region_info)).
///
/// (Not `Eq`: several fields are `f32`.)
#[derive(Debug, Clone, PartialEq)]
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
    /// The raw 32-bit `RegionFlags` bitfield (decode with [`sl_wire::RegionFlags`]).
    pub region_flags: u32,
    /// The full 64-bit `RegionFlagsExtended` (from the `RegionInfo3` block); falls
    /// back to the zero-extended 32-bit [`Self::region_flags`] when the grid sends
    /// no `RegionInfo3`.
    pub region_flags_extended: u64,
    /// The maturity / content rating.
    pub maturity: Maturity,
    /// The estate this region belongs to.
    pub estate_id: u32,
    /// The parent estate (the "mainland" estate is `1`).
    pub parent_estate_id: u32,
    /// The region's water height, in metres.
    pub water_height: f32,
    /// The billing factor applied to land tier in this region.
    pub billable_factor: f32,
    /// The prim-allowance multiplier applied to parcel object limits.
    pub object_bonus_factor: f32,
    /// The maximum height a terrain edit may raise the ground above its baked
    /// value, in metres.
    pub terrain_raise_limit: f32,
    /// The maximum depth a terrain edit may lower the ground below its baked
    /// value, in metres.
    pub terrain_lower_limit: f32,
    /// The land price per square metre, in L$.
    pub price_per_meter: i32,
    /// The grid X this region redirects to, or `0` for none.
    pub redirect_grid_x: i32,
    /// The grid Y this region redirects to, or `0` for none.
    pub redirect_grid_y: i32,
    /// Whether the region uses the estate's sun position rather than its own.
    pub use_estate_sun: bool,
    /// The fixed sun hour (0–24) when [`Self::use_estate_sun`] is `false`; a
    /// negative value means the sun cycles normally.
    pub sun_hour: f32,
    /// The region's chat-range settings, present only when the grid sends a
    /// `RegionInfo5` block (newer Second Life; absent on OpenSim and older grids).
    pub chat_settings: Option<RegionChatSettings>,
    /// The region's combat/damage settings, present only when the grid sends a
    /// `CombatSettings` block.
    pub combat_settings: Option<RegionCombatSettings>,
}

/// A region's chat whisper/normal/shout ranges and offsets, parsed from a
/// `RegionInfo` `RegionInfo5` block.
///
/// (Not `Eq`: the ranges/offsets are `f32`.)
#[derive(Debug, Clone, PartialEq)]
pub struct RegionChatSettings {
    /// The whisper audibility range, in metres.
    pub whisper_range: f32,
    /// The normal-chat audibility range, in metres.
    pub normal_range: f32,
    /// The shout audibility range, in metres.
    pub shout_range: f32,
    /// The whisper range offset.
    pub whisper_offset: f32,
    /// The normal-chat range offset.
    pub normal_offset: f32,
    /// The shout range offset.
    pub shout_offset: f32,
    /// The raw chat-behaviour flag bitfield.
    pub flags: u32,
}

/// A region's combat/damage settings, parsed from a `RegionInfo`
/// `CombatSettings` block.
///
/// (Not `Eq`: several fields are `f32`.)
#[derive(Debug, Clone, PartialEq)]
pub struct RegionCombatSettings {
    /// The raw combat-behaviour flag bitfield.
    pub flags: u32,
    /// The on-death behaviour code.
    pub on_death: u8,
    /// The rate at which damage may be applied.
    pub damage_throttle: f32,
    /// The health regeneration rate.
    pub regeneration_rate: f32,
    /// The post-respawn invulnerability window, in seconds.
    pub invulnerability_time: f32,
    /// The maximum damage applied per hit.
    pub damage_limit: f32,
}

/// The agent's L$ balance and land-tier accounting, parsed from a
/// `MoneyBalanceReply` (a reply to
/// [`Session::request_money_balance`](crate::Session::request_money_balance), or
/// pushed unsolicited by the simulator after a transaction changes the balance).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoneyBalance {
    /// The agent the balance belongs to (the client's own id).
    pub agent_id: Uuid,
    /// The id of the transaction that triggered this reply, correlating it back to
    /// the pay/buy that caused it (e.g. the `TransactionID` echoed by a
    /// [`Session::send_money_transfer`](crate::Session::send_money_transfer)). Nil
    /// for a plain unsolicited balance poll, which has no triggering transaction.
    pub transaction_id: Uuid,
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

/// How many parcels a `ParcelProperties` reply describes, the `RequestResult`
/// field. A "not found / no access" reply arrives as [`NoData`](Self::NoData)
/// and must be distinguished from a normal parcel (mirrors the viewer's
/// `PARCEL_RESULT_*` constants).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ParcelRequestResult {
    /// No parcel data (the query found nothing, or access was denied)
    /// (`PARCEL_RESULT_NO_DATA`, `-1`).
    NoData,
    /// Exactly one parcel was selected (`PARCEL_RESULT_SUCCESS`, `0`).
    #[default]
    Single,
    /// Multiple parcels were selected (`PARCEL_RESULT_MULTIPLE`, `1`).
    Multiple,
    /// An unrecognised result code, preserved verbatim.
    Unknown(i32),
}

impl ParcelRequestResult {
    /// Classifies a `RequestResult` wire value.
    #[must_use]
    pub const fn from_i32(value: i32) -> Self {
        match value {
            -1 => Self::NoData,
            0 => Self::Single,
            1 => Self::Multiple,
            other => Self::Unknown(other),
        }
    }

    /// Whether the reply carries real parcel data (anything but
    /// [`NoData`](Self::NoData)).
    #[must_use]
    pub const fn has_data(self) -> bool {
        !matches!(self, Self::NoData)
    }
}

/// A parcel's ownership status, the `Status` field of `ParcelProperties` (the
/// viewer's `LLParcel::EOwnershipStatus`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ParcelStatus {
    /// The parcel is leased (owned) (`OS_LEASED`, `0`).
    #[default]
    Leased,
    /// A lease is pending (`OS_LEASE_PENDING`, `1`).
    LeasePending,
    /// The parcel has been abandoned (`OS_ABANDONED`, `2`).
    Abandoned,
    /// No ownership status (`OS_NONE`, `-1`).
    None,
    /// An unrecognised status value, preserved verbatim.
    Unknown(i32),
}

impl ParcelStatus {
    /// Classifies a `Status` wire value (the UDP `U8` widened to `i32`, or the
    /// CAPS integer which may be the negative `OS_NONE`).
    #[must_use]
    pub const fn from_i32(value: i32) -> Self {
        match value {
            0 => Self::Leased,
            1 => Self::LeasePending,
            2 => Self::Abandoned,
            -1 => Self::None,
            other => Self::Unknown(other),
        }
    }
}

/// How an avatar arrives on a parcel, the `LandingType` field of
/// `ParcelProperties` (the viewer's `LLParcel::ELandingType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LandingType {
    /// Teleport routing is blocked (`L_NONE`, `0`).
    #[default]
    Blocked,
    /// Arrivals are routed to the parcel's landing point (`L_LANDING_POINT`, `1`).
    LandingPoint,
    /// Arrivals land directly at the requested spot (`L_DIRECT`, `2`).
    Anywhere,
    /// An unrecognised landing type, preserved verbatim.
    Unknown(u8),
}

impl LandingType {
    /// Classifies a `LandingType` wire value.
    #[must_use]
    pub const fn from_u8(value: u8) -> Self {
        match value {
            0 => Self::Blocked,
            1 => Self::LandingPoint,
            2 => Self::Anywhere,
            other => Self::Unknown(other),
        }
    }
}

/// A parcel's geometry, flags, and limits, parsed from `ParcelProperties`.
///
/// The parcel flag bits are exposed through the boolean accessor methods
/// ([`ParcelInfo::create_objects`], [`ParcelInfo::use_ban_list`], …); the raw
/// bitfield is available via [`ParcelInfo::flags`] / [`ParcelInfo::raw_parcel_flags`].
/// The `region_deny_*` / `region_*_override` booleans are *region*-level
/// settings echoed into the parcel reply, distinct from the parcel's own
/// [`ParcelFlags`](sl_wire::ParcelFlags) bits.
#[derive(Debug, Clone, PartialEq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "faithfully mirrors the distinct ParcelProperties wire booleans"
)]
pub struct ParcelInfo {
    /// The request sequence id echoed back (used to match an outstanding query).
    pub sequence_id: i32,
    /// How many parcels this reply describes; [`NoData`](ParcelRequestResult::NoData)
    /// means "not found / no access" rather than a real parcel.
    pub request_result: ParcelRequestResult,
    /// Whether the viewer should snap its selection to the returned parcel.
    pub snap_selection: bool,
    /// The number of the requesting agent's own avatars on the parcel.
    pub self_count: i32,
    /// The number of other agents on the parcel.
    pub other_count: i32,
    /// The number of public/anonymous agents on the parcel.
    pub public_count: i32,
    /// The parcel's region-local id.
    pub local_id: i32,
    /// The parcel owner's id (an agent, or a group when [`is_group_owned`](Self::is_group_owned)).
    pub owner_id: Uuid,
    /// Whether [`owner_id`](Self::owner_id) names a group rather than an agent.
    pub is_group_owned: bool,
    /// The group the parcel is set to (nil if none).
    pub group_id: Uuid,
    /// The auction id, if the parcel is being auctioned (`0` if not).
    pub auction_id: u32,
    /// When the parcel was claimed, as a Unix timestamp (`time_t`).
    pub claim_date: i32,
    /// The price paid to claim the parcel, in L$.
    pub claim_price: i32,
    /// The parcel's rent price, in L$.
    pub rent_price: i32,
    /// The minimum corner of the parcel's axis-aligned bounding box, in metres.
    pub aabb_min: (f32, f32, f32),
    /// The maximum corner of the parcel's axis-aligned bounding box, in metres.
    pub aabb_max: (f32, f32, f32),
    /// The parcel area in square metres.
    pub area: i32,
    /// One bit per 4×4 m region square, marking which squares belong to this
    /// parcel (row-major, least-significant-bit first).
    pub bitmap: Vec<u8>,
    /// The parcel's ownership status (leased / abandoned / …).
    pub status: ParcelStatus,
    /// The parcel's search category.
    pub category: ParcelCategory,
    /// The parcel's maximum object/prim capacity (without bonus).
    pub max_prims: i32,
    /// The region-wide maximum object/prim capacity.
    pub sim_wide_max_prims: i32,
    /// The region-wide current object/prim count.
    pub sim_wide_total_prims: i32,
    /// The total objects/prims currently on the parcel.
    pub total_prims: i32,
    /// The objects/prims on the parcel owned by the parcel owner.
    pub owner_prims: i32,
    /// The objects/prims on the parcel set to the parcel's group.
    pub group_prims: i32,
    /// The objects/prims on the parcel owned by anyone else.
    pub other_prims: i32,
    /// The objects/prims on the parcel that are currently selected.
    pub selected_prims: i32,
    /// The parcel's object-bonus multiplier applied to [`max_prims`](Self::max_prims).
    pub parcel_prim_bonus: f32,
    /// The auto-return time for other people's objects, in minutes (`0` = never).
    pub other_clean_time: i32,
    /// The raw `ParcelFlags` bitfield (decode with [`sl_wire::ParcelFlags`]).
    pub raw_parcel_flags: u32,
    /// The parcel's sale price, in L$.
    pub sale_price: i32,
    /// The parcel's name.
    pub name: String,
    /// The parcel's description.
    pub description: String,
    /// The parcel's streaming-audio URL (the "music" stream), empty if none.
    /// Set it with [`ParcelUpdate::music_url`].
    pub music_url: String,
    /// The parcel's media URL (movie / web page), empty if none. Set it with
    /// [`ParcelUpdate::media_url`]. This is the legacy single-media-URL field;
    /// the per-face media-on-a-prim system is a separate (CAPS) surface.
    pub media_url: String,
    /// The texture id the parcel media replaces while playing (nil if none).
    pub media_id: Uuid,
    /// Whether the media is auto-scaled to fit the surface it replaces.
    pub media_auto_scale: bool,
    /// The only agent allowed to buy the parcel (nil for anyone).
    pub auth_buyer_id: Uuid,
    /// The parcel's snapshot texture id (nil if none).
    pub snapshot_id: Uuid,
    /// The price of a parcel pass, in L$.
    pub pass_price: i32,
    /// How many hours a parcel pass lasts.
    pub pass_hours: f32,
    /// The teleport-landing location within the parcel, in region metres.
    pub user_location: (f32, f32, f32),
    /// The direction an arriving agent faces at the landing point.
    pub user_look_at: (f32, f32, f32),
    /// How an avatar arrives on the parcel (blocked / landing point / anywhere).
    pub landing_type: LandingType,
    /// Region setting: pushing (`llPushObject`) is overridden/blocked region-wide.
    pub region_push_override: bool,
    /// Region setting: anonymous (non-account) avatars are denied region-wide.
    pub region_deny_anonymous: bool,
    /// Region setting: identified-but-not-payment avatars are denied region-wide.
    pub region_deny_identified: bool,
    /// Region setting: avatars without a payment-info-on-file are denied region-wide.
    pub region_deny_transacted: bool,
    /// Region setting: age-unverified avatars are denied region-wide.
    pub region_deny_age_unverified: bool,
    /// Region setting: per-parcel access restrictions are allowed (estate not tax-free).
    pub region_allow_access_override: bool,
    /// The parcel's environment (EEP) version, or `-1` when overrides are off.
    pub parcel_environment_version: i32,
    /// Region setting: per-parcel environment (EEP) overrides are allowed.
    pub region_allow_environment_override: bool,
    /// Whether avatars on the parcel are visible from outside it
    /// (`SeeAVs`); `None` when not provided (the UDP path omits it).
    pub see_avs: Option<bool>,
    /// Whether anyone's avatar sounds (gestures, footsteps) play on the parcel
    /// (`AnyAVSounds`); `None` when not provided (the UDP path omits it).
    pub any_av_sounds: Option<bool>,
    /// Whether group members' avatar sounds play on the parcel
    /// (`GroupAVSounds`); `None` when not provided (the UDP path omits it).
    pub group_av_sounds: Option<bool>,
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

/// A scripted parcel-media control command, the `Command` of a
/// [`Event::ParcelMediaCommand`] (`ParcelMediaCommandMessage`). The values match
/// the viewer's `PARCEL_MEDIA_COMMAND_*` constants and the LSL
/// `PARCEL_MEDIA_COMMAND_*` flags fed to `llParcelMediaCommandList`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParcelMediaCommand {
    /// Stop the media and unload it (`PARCEL_MEDIA_COMMAND_STOP`).
    Stop,
    /// Pause the media, keeping it loaded (`PARCEL_MEDIA_COMMAND_PAUSE`).
    Pause,
    /// Start (or resume) playback once (`PARCEL_MEDIA_COMMAND_PLAY`).
    Play,
    /// Start playback looping (`PARCEL_MEDIA_COMMAND_LOOP`).
    Loop,
    /// Set the media's replacement texture (`PARCEL_MEDIA_COMMAND_TEXTURE`).
    Texture,
    /// Set the media URL (`PARCEL_MEDIA_COMMAND_URL`).
    Url,
    /// Seek to a time offset, in seconds (`PARCEL_MEDIA_COMMAND_TIME`; the value
    /// is the [`time`](Event::ParcelMediaCommand::time) field).
    Time,
    /// Target a single agent rather than the whole parcel
    /// (`PARCEL_MEDIA_COMMAND_AGENT`).
    Agent,
    /// Unload the media from memory (`PARCEL_MEDIA_COMMAND_UNLOAD`).
    Unload,
    /// Auto-align the media to the texture (`PARCEL_MEDIA_COMMAND_AUTO_ALIGN`).
    AutoAlign,
    /// Set the media MIME type (`PARCEL_MEDIA_COMMAND_TYPE`).
    Type,
    /// Set the media surface size in pixels (`PARCEL_MEDIA_COMMAND_SIZE`).
    Size,
    /// Set the media description (`PARCEL_MEDIA_COMMAND_DESC`).
    Desc,
    /// Set whether the media loops (`PARCEL_MEDIA_COMMAND_LOOP_SET`).
    LoopSet,
    /// An unrecognised command code (forward-compatible).
    Other(u32),
}

impl ParcelMediaCommand {
    /// Maps a wire `Command` code to a [`ParcelMediaCommand`], preserving an
    /// unknown code as [`Other`](Self::Other).
    #[must_use]
    pub const fn from_u32(code: u32) -> Self {
        match code {
            0 => Self::Stop,
            1 => Self::Pause,
            2 => Self::Play,
            3 => Self::Loop,
            4 => Self::Texture,
            5 => Self::Url,
            6 => Self::Time,
            7 => Self::Agent,
            8 => Self::Unload,
            9 => Self::AutoAlign,
            10 => Self::Type,
            11 => Self::Size,
            12 => Self::Desc,
            13 => Self::LoopSet,
            other => Self::Other(other),
        }
    }
}

/// The parcel's media settings, parsed from a `ParcelMediaUpdate` and surfaced
/// as [`Event::ParcelMediaUpdate`]. This is the streaming media *surface* (the
/// "media" half of a parcel's media/music split); the streaming-audio URL is the
/// separate [`ParcelInfo::music_url`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParcelMediaUpdateInfo {
    /// The media URL the parcel streams (e.g. an HLS/MP4/web page).
    pub media_url: String,
    /// The texture the media replaces on the parcel surface (nil if none).
    pub media_id: Uuid,
    /// Whether the media is auto-scaled to the surface.
    pub media_auto_scale: bool,
    /// The media MIME type (e.g. `"video/vnd.secondlife.qt.legacy"`,
    /// `"text/html"`); empty if unset.
    pub media_type: String,
    /// The media description; empty if unset.
    pub media_desc: String,
    /// The media surface width in pixels (0 if unset / native).
    pub media_width: i32,
    /// The media surface height in pixels (0 if unset / native).
    pub media_height: i32,
    /// Whether the media loops.
    pub media_loop: bool,
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

/// The per-entry classification flags (`AL_*`) on one parcel access-list entry.
///
/// A bitfield carried by every `List` entry of a `ParcelAccessListReply`
/// (alongside the whole-list [`ParcelAccessScope`]). On Second Life an entry can
/// be flagged as an experience allow/block in addition to the plain
/// access/ban list it belongs to; OpenSim sets the per-entry flags equal to the
/// list's [`ParcelAccessScope`]. Combine the constants with
/// [`ParcelAccessFlags::union`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ParcelAccessFlags(pub u32);

impl ParcelAccessFlags {
    /// No flags set.
    pub const NONE: Self = Self(0);
    /// The entry is on the access (allow) list (`AL_ACCESS`, `1 << 0`).
    pub const ACCESS: Self = Self(1 << 0);
    /// The entry is on the ban list (`AL_BAN`, `1 << 1`).
    pub const BAN: Self = Self(1 << 1);
    /// The entry allows an experience (`AL_ALLOW_EXPERIENCE`, `1 << 3`).
    pub const ALLOW_EXPERIENCE: Self = Self(1 << 3);
    /// The entry blocks an experience (`AL_BLOCK_EXPERIENCE`, `1 << 4`).
    pub const BLOCK_EXPERIENCE: Self = Self(1 << 4);

    /// Combines two sets of access flags.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Whether every bit of `other` is set in `self`.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether no flags are set.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
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
    /// The per-entry classification flags (`AL_*`). On a received reply these
    /// carry the entry's access/ban/experience sub-type; when supplied to
    /// [`Session::update_parcel_access_list`](crate::Session::update_parcel_access_list)
    /// they are OR'd onto the list's [`ParcelAccessScope`] (leave
    /// [`ParcelAccessFlags::NONE`] to send just the scope).
    pub flags: ParcelAccessFlags,
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
    /// The region's water height, in metres (`WaterHeight`; default 20 on most
    /// regions).
    pub water_height: u8,
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

impl InstantMessage {
    /// Decodes the inventory-offer descriptor from this message's binary bucket,
    /// for an [`ImDialog::InventoryOffered`] or [`ImDialog::TaskInventoryOffered`]
    /// message. The bucket is `[asset-type byte] ++ [16-byte item/folder id]`
    /// (a folder offer leads with [`AssetType::Folder`]); only the first entry is
    /// returned. Returns `None` for any other dialog or a malformed bucket.
    #[must_use]
    pub fn inventory_offer(&self) -> Option<InventoryOffer> {
        if !matches!(
            self.dialog,
            ImDialog::InventoryOffered | ImDialog::TaskInventoryOffered
        ) {
            return None;
        }
        let type_byte = *self.binary_bucket.first()?;
        let id_bytes: [u8; 16] = self.binary_bucket.get(1..17)?.try_into().ok()?;
        Some(InventoryOffer {
            asset_type: AssetType::from_code(i32::from(type_byte)),
            item_id: Uuid::from_bytes(id_bytes),
            transaction_id: self.id,
            from_agent_id: self.from_agent_id,
            from_task: matches!(self.dialog, ImDialog::TaskInventoryOffered),
        })
    }
}

/// An inventory offer received over IM, decoded from the binary bucket of an
/// [`ImDialog::InventoryOffered`] / [`ImDialog::TaskInventoryOffered`]
/// [`InstantMessage`] (see [`InstantMessage::inventory_offer`]). Reply with
/// [`Session::accept_inventory_offer`](crate::Session::accept_inventory_offer)
/// or [`Session::decline_inventory_offer`](crate::Session::decline_inventory_offer).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InventoryOffer {
    /// The offered asset's class ([`AssetType::Folder`] for a whole folder).
    pub asset_type: AssetType,
    /// The offered item's (or folder's) id.
    pub item_id: Uuid,
    /// The offer's transaction id (the IM's `id`), echoed back when replying.
    pub transaction_id: Uuid,
    /// The agent (or, for a task offer, the object owner) that made the offer.
    pub from_agent_id: Uuid,
    /// Whether the offer came from an in-world object/task rather than an agent.
    pub from_task: bool,
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

/// One classified ad from an `AvatarClassifiedReply` (header data only: id and
/// name). Fetch the full details with
/// [`Session::request_classified_info`](crate::Session::request_classified_info).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvatarClassified {
    /// The classified id (use to fetch full details).
    pub classified_id: Uuid,
    /// The classified name.
    pub name: String,
}

/// The full details of one pick, parsed from `PickInfoReply` in response to
/// [`Session::request_pick_info`](crate::Session::request_pick_info).
#[derive(Debug, Clone, PartialEq)]
pub struct PickInfo {
    /// The pick id.
    pub pick_id: Uuid,
    /// The avatar that created the pick.
    pub creator_id: Uuid,
    /// Whether this is a "top pick" (a god-only legacy flag, normally `false`).
    pub top_pick: bool,
    /// The parcel the pick points at.
    pub parcel_id: Uuid,
    /// The pick name.
    pub name: String,
    /// The pick description.
    pub description: String,
    /// The pick snapshot texture id.
    pub snapshot_id: Uuid,
    /// The owner's account name, as the grid resolves it.
    pub user: String,
    /// The parcel's original name.
    pub original_name: String,
    /// The region name the pick is in.
    pub sim_name: String,
    /// The pick's global position (metres, grid-wide coordinates).
    pub pos_global: (f64, f64, f64),
    /// The sort order (only meaningful for top picks).
    pub sort_order: i32,
    /// Whether the pick is enabled (shown in the profile).
    pub enabled: bool,
}

/// The full details of one classified ad, parsed from `ClassifiedInfoReply` in
/// response to
/// [`Session::request_classified_info`](crate::Session::request_classified_info).
#[derive(Debug, Clone, PartialEq)]
pub struct ClassifiedInfo {
    /// The classified id.
    pub classified_id: Uuid,
    /// The avatar that created the classified.
    pub creator_id: Uuid,
    /// The creation date (Unix timestamp, seconds).
    pub creation_date: u32,
    /// The expiration date (Unix timestamp, seconds).
    pub expiration_date: u32,
    /// The classified's search category.
    pub category: u32,
    /// The classified name.
    pub name: String,
    /// The classified description.
    pub description: String,
    /// The parcel the classified points at.
    pub parcel_id: Uuid,
    /// The parent estate id.
    pub parent_estate: u32,
    /// The classified snapshot texture id.
    pub snapshot_id: Uuid,
    /// The region name the classified is in.
    pub sim_name: String,
    /// The classified's global position (metres, grid-wide coordinates).
    pub pos_global: (f64, f64, f64),
    /// The parcel name.
    pub parcel_name: String,
    /// The classified flags bitfield (e.g. mature, auto-renew).
    pub classified_flags: u8,
    /// The amount paid to list this classified (L$).
    pub price_for_listing: i32,
}

/// An update to the agent's own profile, sent via
/// [`Session::update_profile`](crate::Session::update_profile)
/// (`AvatarPropertiesUpdate`). This replaces the whole profile, so read the
/// current values with
/// [`Session::request_avatar_properties`](crate::Session::request_avatar_properties)
/// first and edit from there.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProfileUpdate {
    /// The "second life" profile image (texture id).
    pub image_id: Uuid,
    /// The "first life" profile image (texture id).
    pub fl_image_id: Uuid,
    /// The "second life" about text.
    pub about_text: String,
    /// The "first life" about text.
    pub fl_about_text: String,
    /// Whether the profile may be published in search.
    pub allow_publish: bool,
    /// Whether the profile is flagged as "mature".
    pub mature_publish: bool,
    /// The web profile URL.
    pub profile_url: String,
}

/// An update to the agent's own interests, sent via
/// [`Session::update_interests`](crate::Session::update_interests)
/// (`AvatarInterestsUpdate`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InterestsUpdate {
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

/// A create-or-update of one of the agent's picks, sent via
/// [`Session::update_pick`](crate::Session::update_pick) (`PickInfoUpdate`).
/// Supply a fresh [`pick_id`](Self::pick_id) to create a pick, or an existing
/// one to edit it; the simulator fills in [`parcel_id`](Self::parcel_id) from
/// the agent's current parcel when it is nil.
#[derive(Debug, Clone, PartialEq)]
pub struct PickUpdate {
    /// The pick id (a fresh id to create; an existing id to edit).
    pub pick_id: Uuid,
    /// The parcel the pick points at (nil to use the agent's current parcel).
    pub parcel_id: Uuid,
    /// The pick name.
    pub name: String,
    /// The pick description.
    pub description: String,
    /// The pick snapshot texture id.
    pub snapshot_id: Uuid,
    /// The pick's global position (metres; nil/zero to use the agent's).
    pub pos_global: (f64, f64, f64),
    /// The sort order (only meaningful for top picks; normally `0`).
    pub sort_order: i32,
    /// Whether the pick is enabled (shown in the profile).
    pub enabled: bool,
}

impl Default for PickUpdate {
    fn default() -> Self {
        Self {
            pick_id: Uuid::nil(),
            parcel_id: Uuid::nil(),
            name: String::new(),
            description: String::new(),
            snapshot_id: Uuid::nil(),
            pos_global: (0.0, 0.0, 0.0),
            sort_order: 0,
            enabled: true,
        }
    }
}

/// A create-or-update of one of the agent's classifieds, sent via
/// [`Session::update_classified`](crate::Session::update_classified)
/// (`ClassifiedInfoUpdate`). Supply a fresh
/// [`classified_id`](Self::classified_id) to create a classified, or an
/// existing one to edit it; the simulator fills in
/// [`parcel_id`](Self::parcel_id) and the parent estate from the agent's
/// current parcel when the parcel is nil.
#[derive(Debug, Clone, PartialEq)]
pub struct ClassifiedUpdate {
    /// The classified id (a fresh id to create; an existing id to edit).
    pub classified_id: Uuid,
    /// The classified's search category.
    pub category: u32,
    /// The classified name.
    pub name: String,
    /// The classified description.
    pub description: String,
    /// The parcel the classified points at (nil to use the agent's current).
    pub parcel_id: Uuid,
    /// The classified snapshot texture id.
    pub snapshot_id: Uuid,
    /// The classified's global position (metres; nil/zero to use the agent's).
    pub pos_global: (f64, f64, f64),
    /// The classified flags bitfield (e.g. mature, auto-renew).
    pub classified_flags: u8,
    /// The amount to pay to list this classified (L$).
    pub price_for_listing: i32,
}

impl Default for ClassifiedUpdate {
    fn default() -> Self {
        Self {
            classified_id: Uuid::nil(),
            category: 0,
            name: String::new(),
            description: String::new(),
            parcel_id: Uuid::nil(),
            snapshot_id: Uuid::nil(),
            pos_global: (0.0, 0.0, 0.0),
            classified_flags: 0,
            price_for_listing: 0,
        }
    }
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

/// Account-level facts carried by the login response beyond what is needed to
/// bring up the circuit (parsed from the XML-RPC `login_to_simulator` reply).
/// Emitted once as [`Event::Account`] right after [`Event::CircuitEstablished`],
/// and also available from
/// [`Session::login_account`](crate::Session::login_account).
#[derive(Debug, Clone, PartialEq)]
pub struct LoginAccount {
    /// The agent's home location (region handle, position, look-at), if the grid
    /// provided a well-formed `home` field.
    pub home: Option<sl_wire::HomeLocation>,
    /// The camera look-at direction at the start location (`look_at`), if the
    /// grid provided it.
    pub look_at: Option<[f32; 3]>,
    /// The account's current maturity / content rating (`agent_access`).
    pub agent_access: Maturity,
    /// The maximum maturity rating the account is entitled to
    /// (`agent_access_max`); a client may not raise its preference above this.
    pub agent_access_max: Maturity,
    /// The maximum number of groups this account may join (`max-agent-groups`),
    /// or `None` if the grid did not report a limit. Check before joining a
    /// group.
    pub max_agent_groups: Option<u32>,
    /// The shared Library inventory's root folder id (`inventory-lib-root`), if
    /// provided. The folder tree is delivered as [`Event::LibraryInventory`].
    pub library_root: Option<Uuid>,
    /// The agent id owning the shared Library (`inventory-lib-owner`), if
    /// provided. Library folder contents are fetched as this owner's inventory.
    pub library_owner: Option<Uuid>,
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

/// How a [`GroupRoleEdit`] changes a group role (`GroupRoleUpdate`'s
/// `UpdateType`). The wire bytes match the viewer's `LLRoleChangeType`
/// (`roles_constants.h`) and OpenSim's `OpenMetaverse.GroupRoleUpdate` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupRoleUpdateType {
    /// No change (a no-op `RoleData` block).
    NoUpdate,
    /// Update the role's name/title/description only.
    UpdateData,
    /// Update the role's powers only.
    UpdatePowers,
    /// Update both data and powers.
    UpdateAll,
    /// Create a new role (the simulator may assign a fresh `role_id`).
    Create,
    /// Delete the role.
    Delete,
}

impl GroupRoleUpdateType {
    /// The wire byte for this update type.
    #[must_use]
    pub const fn to_u8(self) -> u8 {
        match self {
            Self::NoUpdate => 0,
            Self::UpdateData => 1,
            Self::UpdatePowers => 2,
            Self::UpdateAll => 3,
            Self::Create => 4,
            Self::Delete => 5,
        }
    }
}

/// One role create/update/delete in a `GroupRoleUpdate`, passed to
/// [`Session::update_group_roles`](crate::Session::update_group_roles). For a
/// [`GroupRoleUpdateType::Create`] the `role_id` is the client-chosen id (the
/// simulator may substitute its own); for update/delete it names the existing
/// role. The `powers` bitfield is built from the [`group_powers`] constants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupRoleEdit {
    /// The role id (a fresh id for `Create`, the existing role for the rest).
    pub role_id: Uuid,
    /// The role name.
    pub name: String,
    /// The role description.
    pub description: String,
    /// The title members holding the role wear.
    pub title: String,
    /// The powers granted by the role (see [`group_powers`]).
    pub powers: u64,
    /// Which fields of the role this edit changes.
    pub update_type: GroupRoleUpdateType,
}

/// Whether a [`GroupRoleMemberChange`] adds a member to a role or removes them
/// (`GroupRoleChanges`'s `Change`). Add = 0, Remove = 1, matching OpenSim's
/// `GroupRoleChanges` handler and the viewer's `LLRoleMemberChangeType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupRoleChange {
    /// Assign the member to the role.
    Add,
    /// Remove the member from the role.
    Remove,
}

impl GroupRoleChange {
    /// The wire `Change` value for this role-member change.
    #[must_use]
    pub const fn to_u32(self) -> u32 {
        match self {
            Self::Add => 0,
            Self::Remove => 1,
        }
    }
}

/// One member↔role assignment change in a `GroupRoleChanges`, passed to
/// [`Session::change_group_role_members`](crate::Session::change_group_role_members).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroupRoleMemberChange {
    /// The role to add the member to or remove them from.
    pub role_id: Uuid,
    /// The member's agent id.
    pub member_id: Uuid,
    /// Whether to add or remove the member.
    pub change: GroupRoleChange,
}

/// An inventory item attached to a group notice, passed to
/// [`Session::send_group_notice`](crate::Session::send_group_notice). The item
/// must be copy+transfer for the grid to accept it. The notice's recipients
/// receive an `IM_GROUP_NOTICE` they can accept to copy the item into their
/// inventory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroupNoticeAttachment {
    /// The attached inventory item's id.
    pub item_id: Uuid,
    /// The item owner's agent id (usually the sender).
    pub owner_id: Uuid,
}

/// Group role power bits (`GP_*` in the viewer's `roles_constants.h`), combined
/// into the `powers` bitfield of a [`GroupRoleEdit`]. Only the commonly-set
/// powers are named; any bit may be set directly. Bit 0 is unused (the "none"
/// marker), so the enrollment power is bit 1, etc.
pub mod group_powers {
    /// No powers.
    pub const NONE: u64 = 0x0;
    /// All powers (the owner role).
    pub const ALL: u64 = 0xFFFF_FFFF_FFFF_FFFF;
    /// Invite members to the group.
    pub const MEMBER_INVITE: u64 = 1 << 1;
    /// Eject members from the group.
    pub const MEMBER_EJECT: u64 = 1 << 2;
    /// Toggle "Open Enrollment" and the join fee.
    pub const MEMBER_OPTIONS: u64 = 1 << 3;
    /// Create new roles.
    pub const ROLE_CREATE: u64 = 1 << 4;
    /// Delete roles.
    pub const ROLE_DELETE: u64 = 1 << 5;
    /// Change a role's properties (name, title, description, powers).
    pub const ROLE_PROPERTIES: u64 = 1 << 6;
    /// Assign a member to a role the assigner does not hold "owner" over.
    pub const ROLE_ASSIGN_MEMBER_LIMITED: u64 = 1 << 7;
    /// Assign a member to any role.
    pub const ROLE_ASSIGN_MEMBER: u64 = 1 << 8;
    /// Remove a member from a role.
    pub const ROLE_REMOVE_MEMBER: u64 = 1 << 9;
    /// Change role/actions and members of roles.
    pub const ROLE_CHANGE_ACTIONS: u64 = 1 << 10;
    /// Change the group's identity (charter, insignia, listing, maturity).
    pub const GROUP_CHANGE_IDENTITY: u64 = 1 << 11;
    /// Deed land and buy land for the group.
    pub const LAND_DEED: u64 = 1 << 12;
    /// Send group notices.
    pub const NOTICES_SEND: u64 = 1 << 42;
    /// Receive group notices and view notice history.
    pub const NOTICES_RECEIVE: u64 = 1 << 43;
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
    /// The request's option flags (`Options.Flags`). Reserved by the protocol;
    /// usually zero. The wire message carries a variable list of option blocks —
    /// this is the first block's flags (the only one a simulator sends).
    pub flags: u32,
}

/// A structured, localizable alert (`AlertInfo`): a message *key* the client
/// looks up in its `alerts.xml` (or equivalent) to produce a localized string,
/// together with the substitution parameters for that template. Carried by
/// messages such as `TeleportFailed` and `AlertMessage` alongside a plain
/// fallback string. Mirrors the viewer's `AlertInfo` block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlertInfo {
    /// The localizable message key (`Message`), e.g. `RegionEntryAccessBlocked`.
    /// Empty if the simulator sent no key.
    pub message: String,
    /// The substitution parameters for the keyed template (`ExtraParams`), as the
    /// raw string the simulator sent (a `key=value`/`|`-separated blob the viewer
    /// parses per-alert). Empty when the alert takes no parameters.
    pub extra_params: String,
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
    /// The previous owner's id. Only the CAPS/AIS inventory path carries this;
    /// it is nil for items fetched over the legacy UDP path (and is part of the
    /// item's permissions checksum, the `CRC` of `UpdateInventoryItem`).
    pub last_owner_id: Uuid,
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

/// Parameters for creating a new inventory item via
/// [`Session::create_inventory_item`](crate::Session::create_inventory_item)
/// (`CreateInventoryItem`). The simulator allocates the item's id and replies
/// with an [`Event::InventoryItemCreated`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NewInventoryItem {
    /// The folder the new item is created in.
    pub folder_id: Uuid,
    /// The transaction id associating a freshly uploaded asset with the item
    /// (nil for an item with no backing asset, e.g. a fresh notecard the sim
    /// fills in).
    pub transaction_id: Uuid,
    /// The next-owner permissions mask for the new item.
    pub next_owner_mask: u32,
    /// The asset type (`AssetType`).
    pub asset_type: i8,
    /// The inventory type (`InventoryType`).
    pub inv_type: i8,
    /// The wearable type (only meaningful for clothing/body-part items).
    pub wearable_type: u8,
    /// The item name.
    pub name: String,
    /// The item description.
    pub description: String,
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

// ---------------------------------------------------------------------------
// Asset & texture pipeline (#19): asset/texture fetch value types.
// ---------------------------------------------------------------------------

/// The Second Life asset class (`LLAssetType` / `AT_*`), identifying what kind
/// of asset a UUID names. Used to build a generic asset
/// [transfer](crate::Session::request_asset) and to pick the
/// [`GetAsset`](crate::CAP_GET_ASSET) HTTP query parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetType {
    /// A texture (`AT_TEXTURE`, a JPEG-2000 / `.j2c` image).
    Texture,
    /// A sound clip (`AT_SOUND`).
    Sound,
    /// A calling card (`AT_CALLINGCARD`).
    CallingCard,
    /// A landmark (`AT_LANDMARK`).
    Landmark,
    /// A wearable clothing layer (`AT_CLOTHING`).
    Clothing,
    /// An object / coalesced object (`AT_OBJECT`).
    Object,
    /// A notecard (`AT_NOTECARD`).
    Notecard,
    /// LSL script source text (`AT_LSL_TEXT`).
    LslText,
    /// Compiled LSL bytecode (`AT_LSL_BYTECODE`).
    LslBytecode,
    /// A TGA texture (`AT_TEXTURE_TGA`).
    TextureTga,
    /// A wearable body part (`AT_BODYPART`).
    Bodypart,
    /// A WAV sound (`AT_SOUND_WAV`).
    SoundWav,
    /// A TGA image (`AT_IMAGE_TGA`).
    ImageTga,
    /// A JPEG image (`AT_IMAGE_JPEG`).
    ImageJpeg,
    /// An animation (`AT_ANIMATION`).
    Animation,
    /// A gesture (`AT_GESTURE`).
    Gesture,
    /// A mesh (`AT_MESH`).
    Mesh,
    /// A settings asset (`AT_SETTINGS`).
    Settings,
    /// A render material (`AT_MATERIAL`), an LLSD-wrapped GLTF 2.0 material
    /// document.
    Material,
    /// A glTF document (`AT_GLTF`).
    Gltf,
    /// A glTF binary buffer (`AT_GLTF_BIN`).
    GltfBin,
    /// An inventory folder / category (`AT_CATEGORY`), used as the leading
    /// byte of an inventory-offer binary bucket when a whole folder is offered.
    Folder,
    /// Any other / unrecognised asset class, carrying the raw `AT_*` code.
    Other(i32),
}

impl AssetType {
    /// The numeric `LLAssetType` code for this asset class, as sent in a
    /// `TransferRequest` `Params` block.
    #[must_use]
    pub const fn to_code(self) -> i32 {
        match self {
            Self::Texture => 0,
            Self::Sound => 1,
            Self::CallingCard => 2,
            Self::Landmark => 3,
            Self::Clothing => 5,
            Self::Object => 6,
            Self::Notecard => 7,
            Self::LslText => 10,
            Self::LslBytecode => 11,
            Self::TextureTga => 12,
            Self::Bodypart => 13,
            Self::SoundWav => 17,
            Self::ImageTga => 18,
            Self::ImageJpeg => 19,
            Self::Animation => 20,
            Self::Gesture => 21,
            Self::Mesh => 49,
            Self::Settings => 56,
            Self::Material => 57,
            Self::Gltf => 58,
            Self::GltfBin => 59,
            Self::Folder => 8,
            Self::Other(code) => code,
        }
    }

    /// Classifies an `LLAssetType` code (unknown codes become
    /// [`Other`](Self::Other)).
    #[must_use]
    pub const fn from_code(code: i32) -> Self {
        match code {
            0 => Self::Texture,
            1 => Self::Sound,
            2 => Self::CallingCard,
            3 => Self::Landmark,
            5 => Self::Clothing,
            6 => Self::Object,
            7 => Self::Notecard,
            10 => Self::LslText,
            11 => Self::LslBytecode,
            12 => Self::TextureTga,
            13 => Self::Bodypart,
            17 => Self::SoundWav,
            18 => Self::ImageTga,
            19 => Self::ImageJpeg,
            20 => Self::Animation,
            21 => Self::Gesture,
            49 => Self::Mesh,
            56 => Self::Settings,
            57 => Self::Material,
            58 => Self::Gltf,
            59 => Self::GltfBin,
            8 => Self::Folder,
            other => Self::Other(other),
        }
    }

    /// The query-parameter name the OpenSim/Second Life `GetAsset` capability
    /// expects for this asset class (e.g. `"texture_id"`, `"sound_id"`), or
    /// `None` for classes the cap does not serve by UUID.
    #[must_use]
    pub const fn get_asset_query_key(self) -> Option<&'static str> {
        match self {
            Self::Texture => Some("texture_id"),
            Self::Sound => Some("sound_id"),
            Self::CallingCard => Some("callcard_id"),
            Self::Landmark => Some("landmark_id"),
            Self::Clothing => Some("clothing_id"),
            Self::Object => Some("object_id"),
            Self::Notecard => Some("notecard_id"),
            Self::LslText => Some("lsltext_id"),
            Self::LslBytecode => Some("lslbyte_id"),
            Self::TextureTga => Some("txtr_tga_id"),
            Self::Bodypart => Some("bodypart_id"),
            Self::SoundWav => Some("snd_wav_id"),
            Self::ImageTga => Some("img_tga_id"),
            Self::ImageJpeg => Some("jpeg_id"),
            Self::Animation => Some("animatn_id"),
            Self::Gesture => Some("gesture_id"),
            Self::Mesh => Some("mesh_id"),
            Self::Settings => Some("settings_id"),
            // Second Life serves materials over the `ViewerAsset` cap by
            // `material_id`; the legacy `RenderMaterials` cap is the OpenSim path.
            Self::Material => Some("material_id"),
            Self::Gltf | Self::GltfBin | Self::Folder | Self::Other(_) => None,
        }
    }

    /// The short asset-type name the CAPS upload (`NewFileAgentInventory`)
    /// expects for this asset class (LL's `LLAssetType` `mTypeName`, e.g.
    /// `"texture"`, `"animatn"`, `"lsltext"`), or `None` for classes that are not
    /// uploaded by this capability.
    #[must_use]
    pub const fn caps_asset_name(self) -> Option<&'static str> {
        match self {
            Self::Texture => Some("texture"),
            Self::Sound => Some("sound"),
            Self::CallingCard => Some("callcard"),
            Self::Landmark => Some("landmark"),
            Self::Clothing => Some("clothing"),
            Self::Object => Some("object"),
            Self::Notecard => Some("notecard"),
            Self::LslText => Some("lsltext"),
            Self::LslBytecode => Some("lslbyte"),
            Self::TextureTga => Some("txtr_tga"),
            Self::Bodypart => Some("bodypart"),
            Self::SoundWav => Some("snd_wav"),
            Self::ImageTga => Some("img_tga"),
            Self::ImageJpeg => Some("jpeg"),
            Self::Animation => Some("animatn"),
            Self::Gesture => Some("gesture"),
            Self::Mesh => Some("mesh"),
            Self::Settings => Some("settings"),
            Self::Material => Some("material"),
            Self::Gltf => Some("gltf"),
            Self::GltfBin => Some("glbin"),
            Self::Folder | Self::Other(_) => None,
        }
    }

    /// The name of the capability that updates an *existing* inventory item's
    /// asset for this asset class (the modern in-place edit path:
    /// `UpdateGestureAgentInventory`, `UpdateNotecardAgentInventory`,
    /// `UpdateScriptAgent`, `UpdateSettingsAgentInventory`), or `None` for classes
    /// with no such capability (use the
    /// [`new-asset upload`](Self::caps_asset_name) path instead).
    #[must_use]
    pub const fn update_item_cap(self) -> Option<&'static str> {
        match self {
            Self::Gesture => Some("UpdateGestureAgentInventory"),
            Self::Notecard => Some("UpdateNotecardAgentInventory"),
            Self::LslText => Some("UpdateScriptAgent"),
            Self::Settings => Some("UpdateSettingsAgentInventory"),
            Self::Material => Some("UpdateMaterialAgentInventory"),
            _ => None,
        }
    }
}

/// The Second Life inventory-item class (`LLInventoryType` / `IT_*`), describing
/// how an inventory item behaves (as opposed to [`AssetType`], which describes
/// the underlying asset bytes). One asset class can map to several inventory
/// types — a `Texture` asset can be an ordinary [`Texture`](Self::Texture) or a
/// [`Snapshot`](Self::Snapshot); a `Clothing`/`Bodypart` asset is a
/// [`Wearable`](Self::Wearable). Used to build the CAPS upload
/// (`NewFileAgentInventory`) request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InventoryType {
    /// A texture (`IT_TEXTURE`).
    Texture,
    /// A sound clip (`IT_SOUND`).
    Sound,
    /// A calling card (`IT_CALLINGCARD`).
    CallingCard,
    /// A landmark (`IT_LANDMARK`).
    Landmark,
    /// An object / attachment (`IT_OBJECT`).
    Object,
    /// A notecard (`IT_NOTECARD`).
    Notecard,
    /// A folder / category (`IT_CATEGORY`).
    Category,
    /// An LSL script (`IT_LSL`).
    Script,
    /// A snapshot photo (`IT_SNAPSHOT`).
    Snapshot,
    /// A worn attachment (`IT_ATTACHMENT`).
    Attachment,
    /// A wearable (clothing or body part) (`IT_WEARABLE`).
    Wearable,
    /// An animation (`IT_ANIMATION`).
    Animation,
    /// A gesture (`IT_GESTURE`).
    Gesture,
    /// A mesh (`IT_MESH`).
    Mesh,
    /// A settings asset (`IT_SETTINGS`).
    Settings,
    /// A render material (`IT_MATERIAL`).
    Material,
    /// Any other / unrecognised inventory type, carrying the raw `IT_*` code.
    Other(i32),
}

impl InventoryType {
    /// The numeric `LLInventoryType` code for this inventory class.
    #[must_use]
    pub const fn to_code(self) -> i32 {
        match self {
            Self::Texture => 0,
            Self::Sound => 1,
            Self::CallingCard => 2,
            Self::Landmark => 3,
            Self::Object => 6,
            Self::Notecard => 7,
            Self::Category => 8,
            Self::Script => 10,
            Self::Snapshot => 15,
            Self::Attachment => 17,
            Self::Wearable => 18,
            Self::Animation => 19,
            Self::Gesture => 20,
            Self::Mesh => 22,
            Self::Settings => 25,
            Self::Material => 57,
            Self::Other(code) => code,
        }
    }

    /// Classifies an `LLInventoryType` code (unknown codes become
    /// [`Other`](Self::Other)).
    #[must_use]
    pub const fn from_code(code: i32) -> Self {
        match code {
            0 => Self::Texture,
            1 => Self::Sound,
            2 => Self::CallingCard,
            3 => Self::Landmark,
            6 => Self::Object,
            7 => Self::Notecard,
            8 => Self::Category,
            10 => Self::Script,
            15 => Self::Snapshot,
            17 => Self::Attachment,
            18 => Self::Wearable,
            19 => Self::Animation,
            20 => Self::Gesture,
            22 => Self::Mesh,
            25 => Self::Settings,
            57 => Self::Material,
            other => Self::Other(other),
        }
    }

    /// The short inventory-type name the CAPS upload (`NewFileAgentInventory`)
    /// expects (LL's `LLInventoryType` `mName`, e.g. `"texture"`, `"wearable"`,
    /// `"script"`), or `None` for [`Other`](Self::Other).
    #[must_use]
    pub const fn caps_name(self) -> Option<&'static str> {
        match self {
            Self::Texture => Some("texture"),
            Self::Sound => Some("sound"),
            Self::CallingCard => Some("callcard"),
            Self::Landmark => Some("landmark"),
            Self::Object => Some("object"),
            Self::Notecard => Some("notecard"),
            Self::Category => Some("category"),
            Self::Script => Some("script"),
            Self::Snapshot => Some("snapshot"),
            Self::Attachment => Some("attach"),
            Self::Wearable => Some("wearable"),
            Self::Animation => Some("animation"),
            Self::Gesture => Some("gesture"),
            Self::Mesh => Some("mesh"),
            Self::Settings => Some("settings"),
            Self::Material => Some("material"),
            Self::Other(_) => None,
        }
    }
}

/// The image codec of a texture delivered over the legacy UDP image path
/// (`ImageData`'s `Codec` field / `EImageCodec`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageCodec {
    /// JPEG 2000 codestream (`IMG_CODEC_J2C`) — the normal Second Life texture
    /// format.
    J2c,
    /// Raw RGB (`IMG_CODEC_RGB`).
    Rgb,
    /// Windows bitmap (`IMG_CODEC_BMP`).
    Bmp,
    /// Targa (`IMG_CODEC_TGA`).
    Tga,
    /// JPEG (`IMG_CODEC_JPEG`).
    Jpeg,
    /// S3TC/DXT compressed (`IMG_CODEC_DXT`).
    Dxt,
    /// PNG (`IMG_CODEC_PNG`).
    Png,
    /// An invalid or unrecognised codec, carrying the raw byte.
    Other(u8),
}

impl ImageCodec {
    /// Classifies an `ImageData` `Codec` byte.
    #[must_use]
    pub const fn from_code(code: u8) -> Self {
        match code {
            2 => Self::J2c,
            1 => Self::Rgb,
            3 => Self::Bmp,
            4 => Self::Tga,
            5 => Self::Jpeg,
            6 => Self::Dxt,
            7 => Self::Png,
            other => Self::Other(other),
        }
    }
}

/// The status of a generic asset [transfer](crate::Session::request_asset)
/// (`LLTSCode`), reported in a `TransferInfo`/`TransferPacket`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferStatus {
    /// In progress (`LLTS_OK`).
    Ok,
    /// The transfer completed successfully (`LLTS_DONE`).
    Done,
    /// The source asked to skip (`LLTS_SKIP`).
    Skip,
    /// The transfer was aborted (`LLTS_ABORT`).
    Abort,
    /// A generic error (`LLTS_ERROR`).
    Error,
    /// The asset does not exist — the transfer equivalent of a 404
    /// (`LLTS_UNKNOWN_SOURCE`).
    UnknownSource,
    /// The agent lacks permission to fetch the asset
    /// (`LLTS_INSUFFICIENT_PERMISSIONS`).
    InsufficientPermissions,
    /// Any other / unrecognised status code.
    Other(i32),
}

impl TransferStatus {
    /// Classifies an `LLTSCode` status integer.
    #[must_use]
    pub const fn from_code(code: i32) -> Self {
        match code {
            0 => Self::Ok,
            1 => Self::Done,
            2 => Self::Skip,
            3 => Self::Abort,
            -1 => Self::Error,
            -2 => Self::UnknownSource,
            -3 => Self::InsufficientPermissions,
            other => Self::Other(other),
        }
    }

    /// Whether this status indicates the transfer succeeded (`LLTS_DONE`).
    #[must_use]
    pub const fn is_success(self) -> bool {
        matches!(self, Self::Done)
    }
}

/// A fetched texture: its asset id, the codec the simulator reported (UDP path)
/// and the raw encoded image bytes (a JPEG-2000 codestream for the usual
/// [`J2c`](ImageCodec::J2c) codec). The bytes are **not** decoded into pixels —
/// see [`crate::j2c`] for header parsing / LOD truncation helpers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Texture {
    /// The texture's asset UUID.
    pub id: Uuid,
    /// The codec of [`data`](Self::data). For the HTTP `GetTexture` path this is
    /// always [`J2c`](ImageCodec::J2c) (the cap serves a `.j2c` codestream).
    pub codec: ImageCodec,
    /// The raw encoded image bytes.
    pub data: Vec<u8>,
}

/// A fetched generic asset: its UUID, asset class and raw encoded bytes (a sound
/// clip, animation, notecard, landmark, mesh, …). Delivered over the UDP
/// transfer path or the HTTP `GetAsset`/`GetMesh` capability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    /// The asset's UUID.
    pub id: Uuid,
    /// The asset class.
    pub asset_type: AssetType,
    /// The raw encoded asset bytes.
    pub data: Vec<u8>,
}

/// A wearable's body/clothing slot (LL's `LLWearableType::EType`). Carried by an
/// `AgentWearablesUpdate` (the simulator telling the agent what it is wearing,
/// surfaced as [`Event::AgentWearables`]) and `AgentIsNowWearing` (the agent
/// telling the simulator to change its outfit, sent by
/// [`Session::set_wearing`](crate::Session::set_wearing)).
///
/// The first four slots ([`Shape`](Self::Shape), [`Skin`](Self::Skin),
/// [`Hair`](Self::Hair), [`Eyes`](Self::Eyes)) are *body parts* — an avatar
/// always wears exactly one of each; the rest are *clothing* layers that may be
/// absent or stacked. [`WearableType::is_body_part`] distinguishes them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WearableType {
    /// Body shape (`WT_SHAPE`).
    Shape,
    /// Skin (`WT_SKIN`).
    Skin,
    /// Hair (`WT_HAIR`).
    Hair,
    /// Eyes (`WT_EYES`).
    Eyes,
    /// Shirt (`WT_SHIRT`).
    Shirt,
    /// Pants (`WT_PANTS`).
    Pants,
    /// Shoes (`WT_SHOES`).
    Shoes,
    /// Socks (`WT_SOCKS`).
    Socks,
    /// Jacket (`WT_JACKET`).
    Jacket,
    /// Gloves (`WT_GLOVES`).
    Gloves,
    /// Undershirt (`WT_UNDERSHIRT`).
    Undershirt,
    /// Underpants (`WT_UNDERPANTS`).
    Underpants,
    /// Skirt (`WT_SKIRT`).
    Skirt,
    /// Alpha mask (`WT_ALPHA`).
    Alpha,
    /// Tattoo (`WT_TATTOO`).
    Tattoo,
    /// Physics (`WT_PHYSICS`).
    Physics,
    /// Universal (`WT_UNIVERSAL`).
    Universal,
    /// An unknown / future wearable slot, preserving the raw wire byte.
    Other(u8),
}

impl WearableType {
    /// The `LLWearableType::EType` wire byte for this slot.
    #[must_use]
    pub const fn to_code(self) -> u8 {
        match self {
            Self::Shape => 0,
            Self::Skin => 1,
            Self::Hair => 2,
            Self::Eyes => 3,
            Self::Shirt => 4,
            Self::Pants => 5,
            Self::Shoes => 6,
            Self::Socks => 7,
            Self::Jacket => 8,
            Self::Gloves => 9,
            Self::Undershirt => 10,
            Self::Underpants => 11,
            Self::Skirt => 12,
            Self::Alpha => 13,
            Self::Tattoo => 14,
            Self::Physics => 15,
            Self::Universal => 16,
            Self::Other(code) => code,
        }
    }

    /// Classifies an `LLWearableType::EType` wire byte; codes outside the known
    /// range become [`Other`](Self::Other).
    #[must_use]
    pub const fn from_code(code: u8) -> Self {
        match code {
            0 => Self::Shape,
            1 => Self::Skin,
            2 => Self::Hair,
            3 => Self::Eyes,
            4 => Self::Shirt,
            5 => Self::Pants,
            6 => Self::Shoes,
            7 => Self::Socks,
            8 => Self::Jacket,
            9 => Self::Gloves,
            10 => Self::Undershirt,
            11 => Self::Underpants,
            12 => Self::Skirt,
            13 => Self::Alpha,
            14 => Self::Tattoo,
            15 => Self::Physics,
            16 => Self::Universal,
            other => Self::Other(other),
        }
    }

    /// Whether this is a *body part* (shape/skin/hair/eyes) — worn exactly once —
    /// rather than a clothing layer.
    #[must_use]
    pub const fn is_body_part(self) -> bool {
        matches!(self, Self::Shape | Self::Skin | Self::Hair | Self::Eyes)
    }
}

/// One wearable an avatar has on. From an `AgentWearablesUpdate` (the simulator's
/// view of the agent's outfit) the [`asset_id`](Self::asset_id) names the
/// wearable asset; when passed to
/// [`Session::set_wearing`](crate::Session::set_wearing) (which sends
/// `AgentIsNowWearing`) only the [`item_id`](Self::item_id) and
/// [`wearable_type`](Self::wearable_type) are sent, so the asset id may be nil.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Wearable {
    /// The inventory item id of the worn wearable.
    pub item_id: Uuid,
    /// The wearable's asset id (nil when not known, e.g. when sending).
    pub asset_id: Uuid,
    /// Which body/clothing slot this wearable occupies.
    pub wearable_type: WearableType,
}

/// Avatar texture-entry slot indices (LL's `ETextureIndex`): the faces of the
/// per-avatar `TextureEntry` carried by `AvatarAppearance`. The *baked* slots
/// (`*_BAKED`) hold the composited texture UUIDs other clients fetch and render
/// onto the avatar mesh (see [`Session::request_texture`](crate::Session::request_texture));
/// the remaining slots are the individual per-wearable layer textures used to
/// produce those bakes.
pub mod avatar_texture {
    /// The number of avatar texture slots (`TEX_NUM_INDICES`); the face count of
    /// an avatar `TextureEntry`.
    pub const COUNT: usize = 45;
    /// The baked head texture (`TEX_HEAD_BAKED`).
    pub const HEAD_BAKED: usize = 8;
    /// The baked upper-body texture (`TEX_UPPER_BAKED`).
    pub const UPPER_BAKED: usize = 9;
    /// The baked lower-body texture (`TEX_LOWER_BAKED`).
    pub const LOWER_BAKED: usize = 10;
    /// The baked eyes texture (`TEX_EYES_BAKED`).
    pub const EYES_BAKED: usize = 11;
    /// The baked skirt texture (`TEX_SKIRT_BAKED`).
    pub const SKIRT_BAKED: usize = 19;
    /// The baked hair texture (`TEX_HAIR_BAKED`).
    pub const HAIR_BAKED: usize = 20;
    /// The baked left-arm texture (`TEX_LEFT_ARM_BAKED`), a "universal" bake.
    pub const LEFT_ARM_BAKED: usize = 40;
    /// The baked left-leg texture (`TEX_LEFT_LEG_BAKED`), a "universal" bake.
    pub const LEFT_LEG_BAKED: usize = 41;
    /// The baked aux1 texture (`TEX_AUX1_BAKED`), a "universal" bake.
    pub const AUX1_BAKED: usize = 42;
    /// The baked aux2 texture (`TEX_AUX2_BAKED`), a "universal" bake.
    pub const AUX2_BAKED: usize = 43;
    /// The baked aux3 texture (`TEX_AUX3_BAKED`), a "universal" bake.
    pub const AUX3_BAKED: usize = 44;
    /// The baked-slot indices in order, each with a short human-readable name.
    pub const BAKED: [(usize, &str); 11] = [
        (HEAD_BAKED, "head"),
        (UPPER_BAKED, "upper"),
        (LOWER_BAKED, "lower"),
        (EYES_BAKED, "eyes"),
        (SKIRT_BAKED, "skirt"),
        (HAIR_BAKED, "hair"),
        (LEFT_ARM_BAKED, "left_arm"),
        (LEFT_LEG_BAKED, "left_leg"),
        (AUX1_BAKED, "aux1"),
        (AUX2_BAKED, "aux2"),
        (AUX3_BAKED, "aux3"),
    ];
}

/// One decoded face of a `TextureEntry`: its texture and surface parameters. A
/// `TextureEntry` packs per-face data (texture id, tint colour, repeats/offsets,
/// rotation, bump/shiny/fullbright, media, glow, material) into a run-length
/// encoded blob shared by objects (`ObjectUpdate`) and avatars
/// (`AvatarAppearance`); [`decode_texture_entry`](crate::decode_texture_entry)
/// unpacks it into one of these per face. Values are converted to natural units
/// (matching the reference viewer's `applyParsedTEMessage`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextureFace {
    /// The face's texture asset id. For an avatar's baked slots (see
    /// [`avatar_texture`]) this is the composited bake to fetch and render.
    pub texture_id: Uuid,
    /// The tint colour applied to the texture, as RGBA bytes (un-inverted from
    /// the wire's `255 - value` encoding; `[255; 4]` is opaque white = no tint).
    pub color: [u8; 4],
    /// Horizontal texture repeats.
    pub scale_s: f32,
    /// Vertical texture repeats.
    pub scale_t: f32,
    /// Horizontal texture offset, in the range −1..1.
    pub offset_s: f32,
    /// Vertical texture offset, in the range −1..1.
    pub offset_t: f32,
    /// Texture rotation, in radians.
    pub rotation: f32,
    /// The packed bump/shiny/fullbright byte (LL's `getBumpShinyFullbright`).
    pub bump_shiny_fullbright: u8,
    /// The packed media/texture-generation flags byte (LL's `getMediaTexGen`).
    pub media_flags: u8,
    /// Glow amount, in the range 0..1.
    pub glow: f32,
    /// The material id (a legacy materials asset; nil if none).
    pub material_id: Uuid,
}

impl TextureFace {
    /// The bump-map (normal/emboss) code packed into
    /// [`bump_shiny_fullbright`](Self::bump_shiny_fullbright) — the low 5 bits
    /// (LL's `getBumpmap()`; `0` = none, otherwise a `BE_*` bump type).
    #[must_use]
    pub const fn bumpmap(self) -> u8 {
        self.bump_shiny_fullbright & 0x1f
    }

    /// Whether the face is full-bright (unlit), from bit 5 of
    /// [`bump_shiny_fullbright`](Self::bump_shiny_fullbright) (LL's
    /// `getFullbright()`).
    #[must_use]
    pub const fn fullbright(self) -> bool {
        (self.bump_shiny_fullbright >> 5) & 0x01 != 0
    }

    /// The shininess code, from the top 2 bits of
    /// [`bump_shiny_fullbright`](Self::bump_shiny_fullbright) (LL's `getShiny()`):
    /// `0` = none, `1` = low, `2` = medium, `3` = high.
    #[must_use]
    pub const fn shininess(self) -> u8 {
        (self.bump_shiny_fullbright >> 6) & 0x03
    }

    /// Whether **media** is enabled on the face, from bit 0 of
    /// [`media_flags`](Self::media_flags) (LL's `getMediaFlags()`).
    #[must_use]
    pub const fn media_enabled(self) -> bool {
        self.media_flags & 0x01 != 0
    }

    /// The texture-coordinate generation mode, from bits 1–2 of
    /// [`media_flags`](Self::media_flags) (LL's `getTexGen()`): `0` = default
    /// (per-face), `2` = planar, with the other values reserved.
    #[must_use]
    pub const fn tex_gen(self) -> u8 {
        self.media_flags & 0x06
    }
}

/// A decoded `TextureEntry`: one [`TextureFace`] per face. For an avatar (from
/// `AvatarAppearance`) the faces are indexed by the [`avatar_texture`] slot
/// constants; for an object they follow the prim's face numbering. Decode a raw
/// blob (e.g. [`Object::texture_entry`]) with
/// [`decode_texture_entry`](crate::decode_texture_entry).
#[derive(Debug, Clone, PartialEq)]
pub struct TextureEntry {
    /// The per-face data, in face-index order.
    pub faces: Vec<TextureFace>,
}

impl TextureEntry {
    /// The face at `index`, or `None` if the entry has fewer faces.
    #[must_use]
    pub fn face(&self, index: usize) -> Option<&TextureFace> {
        self.faces.get(index)
    }

    /// The texture id at slot `index`, or `None` if the entry has fewer faces.
    /// Combine with the [`avatar_texture`] baked-slot constants to read an
    /// avatar's baked textures.
    #[must_use]
    pub fn texture_id(&self, index: usize) -> Option<Uuid> {
        self.faces.get(index).map(|face| face.texture_id)
    }
}

/// A decoded `AvatarAppearance`: another avatar's baked textures and visual
/// parameters, pushed by the simulator when an avatar comes into range or
/// changes appearance. Surfaced as [`Event::AvatarAppearance`]. The baked
/// texture ids (read from [`texture_entry`](Self::texture_entry) via the
/// [`avatar_texture`] slot constants) can be fetched with
/// [`Session::request_texture`](crate::Session::request_texture) to render the
/// avatar.
#[derive(Debug, Clone, PartialEq)]
pub struct AvatarAppearance {
    /// The avatar this appearance describes.
    pub avatar_id: Uuid,
    /// Whether the avatar is on a trial account.
    pub is_trial: bool,
    /// The decoded per-face texture entry (the baked avatar textures live in the
    /// [`avatar_texture`] baked slots).
    pub texture_entry: TextureEntry,
    /// The visual parameters, one quantized byte (0..255) per parameter in the
    /// reference viewer's parameter order. Each maps a slider/morph to its range;
    /// the byte is `round((value - min) / (max - min) * 255)`.
    pub visual_params: Vec<u8>,
    /// The appearance message version (`AppearanceData.AppearanceVersion`), or
    /// `None` when the simulator sent no `AppearanceData` block (older path).
    pub appearance_version: Option<u8>,
    /// The Current Outfit Folder version this appearance was baked from
    /// (`AppearanceData.CofVersion`), or `None` when absent.
    pub cof_version: Option<i32>,
    /// The appearance flags (`AppearanceData.Flags`), or `None` when absent.
    pub appearance_flags: Option<u32>,
    /// The avatar's hover height offset, in metres, if the simulator sent an
    /// `AppearanceHover` block.
    pub hover_height: Option<Vector>,
    /// The avatar's HUD/attachment ids and their attachment points, if the
    /// simulator sent an `AttachmentBlock`.
    pub attachments: Vec<AvatarAttachment>,
}

/// One animation an avatar is currently playing, from an `AvatarAnimation`
/// update (surfaced inside [`Event::AvatarAnimation`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayingAnimation {
    /// The animation asset id (a built-in animation UUID or an uploaded
    /// animation asset; fetch custom ones with
    /// [`Session::request_asset`](crate::Session::request_asset)).
    pub anim_id: Uuid,
    /// The simulator's per-avatar animation sequence number. It increments each
    /// time an animation (re)starts, so a viewer can tell a fresh start from an
    /// animation that has merely been re-listed.
    pub sequence_id: i32,
    /// The object that triggered the animation, when the simulator names one
    /// (an `AnimationSourceList` entry — e.g. a scripted `llStartAnimation`).
    /// `None` for animations the agent or simulator started directly.
    pub source_id: Option<Uuid>,
}

/// The playback flags carried by an [`Event::AttachedSound`] (`AttachedSound`'s
/// `Flags` byte). The values match the viewer's `LL_SOUND_FLAG_*` constants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SoundFlags(pub u8);

impl SoundFlags {
    /// The sound loops until stopped (`llLoopSound`) rather than playing once.
    pub const LOOP: u8 = 1 << 0;
    /// This sound is the timing master of a synchronised group.
    pub const SYNC_MASTER: u8 = 1 << 1;
    /// This sound is a slave that follows the group's sync master.
    pub const SYNC_SLAVE: u8 = 1 << 2;
    /// This sound is waiting to be synchronised to a master.
    pub const SYNC_PENDING: u8 = 1 << 3;
    /// Queue this sound behind the currently-playing one rather than interrupting.
    pub const QUEUE: u8 = 1 << 4;
    /// Stop the object's attached sound (rather than starting one).
    pub const STOP: u8 = 1 << 5;

    /// Whether all of the bits in `mask` are set.
    #[must_use]
    pub const fn contains(self, mask: u8) -> bool {
        self.0 & mask == mask
    }

    /// Whether the sound loops ([`Self::LOOP`]).
    #[must_use]
    pub const fn is_loop(self) -> bool {
        self.contains(Self::LOOP)
    }

    /// Whether this message stops the attached sound ([`Self::STOP`]).
    #[must_use]
    pub const fn is_stop(self) -> bool {
        self.contains(Self::STOP)
    }
}

/// One sound the simulator asks the viewer to pre-fetch, from a `PreloadSound`
/// update (surfaced inside [`Event::PreloadSound`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SoundPreload {
    /// The sound asset to pre-fetch.
    pub sound_id: Uuid,
    /// The object that will play the sound.
    pub object_id: Uuid,
    /// The object owner's id.
    pub owner_id: Uuid,
}

/// One entry of an [`AvatarAppearance`] attachment block: an attached object and
/// where it is worn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AvatarAttachment {
    /// The attached object's id.
    pub id: Uuid,
    /// The attachment point byte (LL's attachment-point enumeration).
    pub attachment_point: u8,
}

#[cfg(test)]
mod tests {
    use super::{Camera, Vector, cross};
    use pretty_assertions::assert_eq;

    /// The dot product `a · b`.
    fn dot(a: &Vector, b: &Vector) -> f32 {
        a.x * b.x + a.y * b.y + a.z * b.z
    }

    fn is_unit(v: &Vector) -> bool {
        (dot(v, v) - 1.0).abs() < 1e-5
    }

    fn approx_eq(a: &Vector, b: &Vector) -> bool {
        (a.x - b.x).abs() < 1e-5 && (a.y - b.y).abs() < 1e-5 && (a.z - b.z).abs() < 1e-5
    }

    #[test]
    fn looking_at_builds_right_handed_orthonormal_basis() {
        // An oblique look direction so all three axes are non-trivial.
        let eye = Vector {
            x: 10.0,
            y: 20.0,
            z: 5.0,
        };
        let target = Vector {
            x: 13.0,
            y: 24.0,
            z: 7.0,
        };
        let camera = Camera::looking_at(eye.clone(), target);
        // The eye point is kept verbatim.
        assert_eq!(camera.center, eye);
        // All three axes are unit length.
        assert!(is_unit(&camera.at_axis));
        assert!(is_unit(&camera.left_axis));
        assert!(is_unit(&camera.up_axis));
        // They are mutually orthogonal.
        assert!(dot(&camera.at_axis, &camera.left_axis).abs() < 1e-5);
        assert!(dot(&camera.at_axis, &camera.up_axis).abs() < 1e-5);
        assert!(dot(&camera.left_axis, &camera.up_axis).abs() < 1e-5);
        // Right-handed in the SL convention: at × left = up.
        assert!(approx_eq(
            &cross(&camera.at_axis, &camera.left_axis),
            &camera.up_axis
        ));
    }

    #[test]
    fn looking_straight_down_falls_back_gracefully() {
        // A vertical look direction makes `world_up × at` vanish; the left axis
        // must still come out unit-length (the +Y fallback) and the basis stay
        // orthonormal rather than producing NaNs.
        let eye = Vector {
            x: 0.0,
            y: 0.0,
            z: 10.0,
        };
        let target = Vector {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let camera = Camera::looking_at(eye, target);
        assert!(is_unit(&camera.at_axis));
        assert!(is_unit(&camera.left_axis));
        assert!(is_unit(&camera.up_axis));
        // Looking straight down: at = -Z.
        assert!(approx_eq(
            &camera.at_axis,
            &Vector {
                x: 0.0,
                y: 0.0,
                z: -1.0,
            }
        ));
    }

    #[test]
    fn region_center_is_the_legacy_default() {
        // The default camera matches the historic hardcoded viewpoint: region
        // centre, looking along +X with the world-up basis.
        let camera = Camera::default();
        assert_eq!(camera, Camera::region_center());
        assert_eq!(
            camera.center,
            Vector {
                x: 128.0,
                y: 128.0,
                z: 30.0,
            }
        );
        // Equivalent to looking from the centre toward +X.
        let looked = Camera::looking_at(
            camera.center.clone(),
            Vector {
                x: 129.0,
                y: 128.0,
                z: 30.0,
            },
        );
        assert!(approx_eq(&looked.at_axis, &camera.at_axis));
        assert!(approx_eq(&looked.left_axis, &camera.left_axis));
        assert!(approx_eq(&looked.up_axis, &camera.up_axis));
    }

    /// A bare [`TextureFace`] with a given packed `bump_shiny_fullbright` and
    /// `media_flags`, all other fields irrelevant to the accessor under test.
    fn face(bump_shiny_fullbright: u8, media_flags: u8) -> super::TextureFace {
        super::TextureFace {
            texture_id: super::Uuid::nil(),
            color: [255; 4],
            scale_s: 1.0,
            scale_t: 1.0,
            offset_s: 0.0,
            offset_t: 0.0,
            rotation: 0.0,
            bump_shiny_fullbright,
            media_flags,
            glow: 0.0,
            material_id: super::Uuid::nil(),
        }
    }

    #[test]
    fn texture_face_unpacks_bump_shiny_fullbright() {
        // bump = 0x05 (low 5 bits), fullbright = bit 5, shiny = top 2 bits = 2.
        let f = face(0x05 | 0x20 | (0b10 << 6), 0);
        assert_eq!(f.bumpmap(), 0x05);
        assert!(f.fullbright());
        assert_eq!(f.shininess(), 0b10);
        // A plain face: nothing set.
        let plain = face(0, 0);
        assert_eq!(plain.bumpmap(), 0);
        assert!(!plain.fullbright());
        assert_eq!(plain.shininess(), 0);
    }

    #[test]
    fn texture_face_unpacks_media_flags() {
        // media bit set, tex-gen = planar (0b10 in bits 1-2 -> value 0x02).
        let f = face(0, 0x01 | 0x02);
        assert!(f.media_enabled());
        assert_eq!(f.tex_gen(), 0x02);
        let plain = face(0, 0);
        assert!(!plain.media_enabled());
        assert_eq!(plain.tex_gen(), 0);
    }

    /// An [`Object`] carrying only the given raw `name_value` string; all other
    /// fields are defaulted to values irrelevant to the parser under test.
    fn object_with_name_value(name_value: &str) -> super::Object {
        test_object(0, 0, name_value)
    }

    fn test_object(pcode: u8, state: u8, name_value: &str) -> super::Object {
        super::Object {
            region_handle: 0,
            local_id: 0,
            full_id: super::Uuid::nil(),
            parent_id: 0,
            pcode,
            state,
            crc: 0,
            material: 0,
            click_action: 0,
            update_flags: 0,
            scale: Vector {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            motion: super::ObjectMotion {
                position: Vector {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                velocity: Vector {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                acceleration: Vector {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                rotation: super::Rotation {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    s: 1.0,
                },
                angular_velocity: Vector {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                collision_plane: None,
            },
            owner_id: super::Uuid::nil(),
            sound: super::Uuid::nil(),
            gain: 0.0,
            sound_flags: 0,
            sound_radius: 0.0,
            text: String::new(),
            text_color: [0; 4],
            name_value: name_value.to_owned(),
            media_url: String::new(),
            texture_entry: Vec::new(),
            texture_anim: Vec::new(),
            texture_animation: None,
            shape: super::PrimShapeParams::default(),
            particle_system: Vec::new(),
            particles: None,
            data: Vec::new(),
            extra_params: Vec::new(),
            extra: super::ObjectExtraParams::default(),
            properties: None,
            joint_type: 0,
            joint_pivot: Vector {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            joint_axis_or_anchor: Vector {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
        }
    }

    #[test]
    fn name_value_parses_class_and_sendto() -> Result<(), String> {
        // The common attachment form: name type class sendto data.
        let object = object_with_name_value(
            "AttachItemID STRING RW SV 11111111-2222-3333-4444-555555555555",
        );
        let pairs = object.name_values();
        let pair = pairs.first().ok_or("expected one pair")?;
        assert_eq!(pairs.len(), 1);
        assert_eq!(pair.name, "AttachItemID");
        assert_eq!(pair.value_type, "STRING");
        assert_eq!(pair.class, "RW");
        assert_eq!(pair.sendto, "SV");
        assert_eq!(pair.value, "11111111-2222-3333-4444-555555555555");
        assert_eq!(
            object.name_value_data("AttachItemID").as_deref(),
            Some("11111111-2222-3333-4444-555555555555")
        );
        assert_eq!(object.name_value_data("Missing"), None);
        Ok(())
    }

    #[test]
    fn name_value_defaults_omitted_class_and_sendto() -> Result<(), String> {
        // No class/sendto keyword: the token after the type starts the data, and
        // class/sendto default to RW/S (matching the viewer).
        let object = object_with_name_value("FooBar STRING hello world");
        let pairs = object.name_values();
        let pair = pairs.first().ok_or("expected one pair")?;
        assert_eq!(pairs.len(), 1);
        assert_eq!(pair.name, "FooBar");
        assert_eq!(pair.value_type, "STRING");
        assert_eq!(pair.class, "RW");
        assert_eq!(pair.sendto, "S");
        assert_eq!(pair.value, "hello world");
        Ok(())
    }

    #[test]
    fn name_value_parses_multiple_lines_and_skips_blanks() -> Result<(), String> {
        let object =
            object_with_name_value("A STRING RW S one\n\nB S32 RW DSV 42\n   \nincomplete");
        let pairs = object.name_values();
        // The blank lines are skipped; "incomplete" (name only, no type) too.
        let first = pairs.first().ok_or("expected a first pair")?;
        let second = pairs.get(1).ok_or("expected a second pair")?;
        assert_eq!(pairs.len(), 2);
        assert_eq!(first.name, "A");
        assert_eq!(first.value, "one");
        assert_eq!(second.name, "B");
        assert_eq!(second.sendto, "DSV");
        assert_eq!(second.value, "42");
        Ok(())
    }

    #[test]
    fn attachment_point_unswizzles_state_nibbles() {
        use super::AttachmentPoint;
        use sl_types::attachment::AvatarAttachmentPoint;
        // The simulator swaps the nibbles of the point id. Right hand (6 = 0x06)
        // travels the wire as 0x60; chest (1 = 0x01) as 0x10. The accessor must
        // swap back, both as a raw id and as the named enum variant.
        for (point, wire) in [
            (AvatarAttachmentPoint::RightHand, 0x60_u8),
            (AvatarAttachmentPoint::Chest, 0x10),
        ] {
            let object = test_object(super::pcode::PRIMITIVE, wire, "");
            assert_eq!(
                object.attachment_point(),
                Some(AttachmentPoint::Avatar(point))
            );
        }
        // Right hand (6) as the raw id.
        let hand = test_object(super::pcode::PRIMITIVE, 0x60, "");
        assert_eq!(hand.attachment_point_id(), Some(6));
        // The transient ATTACHMENT_ADD (0x80) bit is stripped: un-swizzling
        // 0x68 gives 0x86, and stripping 0x80 leaves right hand (6).
        let adding = test_object(super::pcode::PRIMITIVE, 0x68, "");
        assert_eq!(adding.attachment_point_id(), Some(6));
        assert_eq!(
            adding.attachment_point(),
            Some(AttachmentPoint::Avatar(AvatarAttachmentPoint::RightHand))
        );
    }

    #[test]
    fn attachment_point_decodes_hud_points() {
        use super::AttachmentPoint;
        use sl_types::attachment::HudAttachmentPoint;
        // HUD center 1 (id 35 = 0x23) travels the wire nibble-swapped as 0x32.
        // The unified AttachmentPoint enum names HUD points, so both accessors
        // surface it.
        let hud = test_object(super::pcode::PRIMITIVE, 0x32, "");
        assert_eq!(hud.attachment_point_id(), Some(35));
        assert_eq!(
            hud.attachment_point(),
            Some(AttachmentPoint::Hud(HudAttachmentPoint::Center))
        );
    }

    #[test]
    fn attachment_point_none_for_non_attachments() {
        // A plain prim (state 0) is not an attachment.
        let prim = test_object(super::pcode::PRIMITIVE, 0, "");
        assert_eq!(prim.attachment_point_id(), None);
        assert_eq!(prim.attachment_point(), None);
        // A tree's non-zero state byte is its species, not an attachment point.
        let tree = test_object(super::pcode::TREE, 3, "");
        assert_eq!(tree.attachment_point_id(), None);
        // Grass likewise.
        let grass = test_object(super::pcode::GRASS, 1, "");
        assert_eq!(grass.attachment_point_id(), None);
    }
}
