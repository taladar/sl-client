//! The high-level [`Event`] enum surfaced to the driver/application.

use std::net::SocketAddr;

use super::{
    ActiveGroup, AlertInfo, Asset, AssetType, AvatarAppearance, AvatarClassified,
    AvatarGroupMembership, AvatarInterests, AvatarName, AvatarPick, AvatarPickerResult,
    AvatarProperties, ChatMessage, ClassifiedInfo, CoarseLocation, DirClassifiedResult,
    DirEventResult, DirGroupResult, DirLandResult, DirPeopleResult, DirPlaceResult,
    DisconnectReason, EconomyData, EnvironmentSettings, EstateAccessKind, EstateCovenant,
    EstateInfo, EventInfo, FollowCamPropertyValue, Friend, FriendRights, GroupAccountDetails,
    GroupAccountSummary, GroupAccountTransactions, GroupActiveProposalItem, GroupMember,
    GroupMembership, GroupName, GroupNotice, GroupProfile, GroupRole, GroupRoleMember, GroupTitle,
    GroupVoteHistoryItem, ImDialog, InstantMessage, InventoryFolder, InventoryItem, LandStatItem,
    LandStatReportType, LoadUrlRequest, LoginAccount, MapItem, MapItemType, MapLayer,
    MapRegionInfo, Maturity, MeanCollision, MoneyBalance, MuteEntry, NeighborInfo, Object,
    ObjectProperties, ObjectPropertiesFamily, ParcelAccessEntry, ParcelAccessScope, ParcelDetails,
    ParcelInfo, ParcelMediaCommand, ParcelMediaUpdateInfo, ParcelObjectOwner, ParcelOverlayInfo,
    PickInfo, PlacesResult, PlayingAnimation, RegionIdentity, RegionLimits, ScriptControl,
    ScriptDialog, ScriptPermissionRequest, ScriptTeleportRequest, SoundFlags, SoundPreload,
    TelehubInfo, TeleportFlags, TerrainPatch, Texture, TransferStatus, ViewerEffect, Wearable,
};
use sl_types::lsl::Rotation;
use sl_types::lsl::Vector;
use sl_wire::AgentPreferences;
use sl_wire::AttachmentResourcesReport;
use sl_wire::DisplayName;
use sl_wire::ExperienceInfo;
use sl_wire::LandResourcesUrls;
use sl_wire::MediaEntry;
use sl_wire::ObjectCost;
use sl_wire::ObjectPhysicsData;
use sl_wire::ParcelScriptResources;
use sl_wire::ParcelVoiceInfo;
use sl_wire::RenderMaterialEntry;
use sl_wire::ResourceSummary;
use sl_wire::SelectedResourceCost;
use sl_wire::SimulatorFeatures;
use sl_wire::VoiceAccountInfo;
use uuid::Uuid;

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
    /// Legacy avatar names resolved from a `UUIDNameReply` (a reply to
    /// [`Session::request_avatar_names`](crate::Session::request_avatar_names)).
    /// A single reply may batch several ids, and one request may be answered by
    /// several replies.
    AvatarNames(Vec<AvatarName>),
    /// Group names resolved from a `UUIDGroupNameReply` (a reply to
    /// [`Session::request_group_names`](crate::Session::request_group_names)).
    GroupNames(Vec<GroupName>),
    /// Display names resolved from a `GetDisplayNames` capability GET (the reply
    /// to [`Command::RequestDisplayNames`](crate::Command::RequestDisplayNames)):
    /// the mutable display name plus username/SLID and legacy first/last for each
    /// requested id. Ids the grid could not resolve come back as
    /// [`missing`](sl_wire::DisplayName::missing) placeholders.
    DisplayNames(Vec<DisplayName>),
    /// The extended-environment (EEP) sky/water/day-cycle settings for the region
    /// or a parcel, parsed from the `ExtEnvironment` capability (the reply to
    /// [`Command::RequestEnvironment`](crate::Command::RequestEnvironment)).
    Environment(Box<EnvironmentSettings>),
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
    /// A parcel's per-owner object tallies, from a `ParcelObjectOwnersReply` in
    /// response to
    /// [`Session::request_parcel_object_owners`](crate::Session::request_parcel_object_owners).
    ParcelObjectOwners {
        /// One row per owner with objects on the parcel.
        owners: Vec<ParcelObjectOwner>,
    },
    /// A parcel's basic listing, from a `ParcelInfoReply` in response to
    /// [`Session::request_parcel_info`](crate::Session::request_parcel_info).
    ParcelDetails(ParcelDetails),
    /// The grid-wide parcel id covering a region location, from a
    /// `RemoteParcelRequest` capability reply to the runtimes'
    /// [`Command::RequestRemoteParcelId`](crate::Command::RequestRemoteParcelId).
    /// Feed it to [`Session::request_parcel_info`](crate::Session::request_parcel_info).
    RemoteParcelId(Uuid),
    /// The region's feature flags and limits, from a `SimulatorFeatures`
    /// capability GET. The runtimes fetch this automatically once the capability
    /// map is known (at login and on each region change), and on demand via
    /// [`Command::RequestSimulatorFeatures`](crate::Command::RequestSimulatorFeatures).
    /// On OpenSim the grid-specific extras arrive in
    /// [`open_sim_extras`](sl_wire::SimulatorFeatures::open_sim_extras); Second
    /// Life leaves that [`None`].
    SimulatorFeatures(Box<SimulatorFeatures>),
    /// The agent's server-stored preferences (hover height, default object
    /// permission masks, maturity-access ceiling, UI language), from an
    /// `AgentPreferences` capability POST — the reply to
    /// [`Command::SetAgentPreferences`](crate::Command::SetAgentPreferences) or
    /// [`Command::RequestAgentPreferences`](crate::Command::RequestAgentPreferences).
    /// The grid echoes the full stored set, so every field is `Some`.
    AgentPreferences(Box<AgentPreferences>),
    /// The land-impact / physics costs of one or more objects, from a
    /// `GetObjectCost` capability reply to
    /// [`Command::RequestObjectCost`](crate::Command::RequestObjectCost). One
    /// entry per object, keyed by object id (sorted by id).
    ObjectCosts(Vec<(Uuid, ObjectCost)>),
    /// The summed physics/streaming/simulation cost of the current selection,
    /// from a `ResourceCostSelected` capability reply to
    /// [`Command::RequestSelectedCost`](crate::Command::RequestSelectedCost).
    SelectedResourceCost(SelectedResourceCost),
    /// The physics-material parameters of one or more objects, from a
    /// `GetObjectPhysicsData` capability reply to
    /// [`Command::RequestObjectPhysicsData`](crate::Command::RequestObjectPhysicsData).
    /// One entry per object, keyed by object id (sorted by id).
    ObjectPhysicsData(Vec<(Uuid, ObjectPhysicsData)>),
    /// Updated physics-material parameters pushed unsolicited over the event
    /// queue (`ObjectPhysicsProperties`), sent when a prim's physics material
    /// changes. One entry per object, keyed by region-local id.
    ObjectPhysicsProperties(Vec<(u32, ObjectPhysicsData)>),
    /// The agent's attachment resource report, from an `AttachmentResources`
    /// capability reply to
    /// [`Command::RequestAttachmentResources`](crate::Command::RequestAttachmentResources):
    /// the scripted attachments grouped by attachment point, with a summary.
    AttachmentResources(Box<AttachmentResourcesReport>),
    /// The follow-up capability URLs from a `LandResources` capability reply to
    /// [`Command::RequestLandResources`](crate::Command::RequestLandResources).
    /// The runtimes GET these URLs and then surface [`Event::LandResourceSummary`]
    /// and (when present) [`Event::LandResourceDetail`].
    LandResourcesUrls(LandResourcesUrls),
    /// A parcel's script-resource totals, from the `ScriptResourceSummary`
    /// follow-up cap of a [`Command::RequestLandResources`](crate::Command::RequestLandResources).
    LandResourceSummary(ResourceSummary),
    /// A parcel's per-object script-resource breakdown, from the
    /// `ScriptResourceDetails` follow-up cap of a
    /// [`Command::RequestLandResources`](crate::Command::RequestLandResources)
    /// (only sent when the agent may see detail). One entry per parcel.
    LandResourceDetail(Vec<ParcelScriptResources>),
    /// A region's top-scripts / top-colliders report, from a `LandStatReply` in
    /// response to [`Command::RequestLandStat`](crate::Command::RequestLandStat)
    /// (the estate-tools "Top Scripts" / "Top Colliders" panels).
    LandStatReply {
        /// Which report this is (top scripts or top colliders).
        report_type: LandStatReportType,
        /// The request flags echoed from the request.
        request_flags: u32,
        /// The total number of objects in the report (the report itself may carry
        /// only the top rows).
        total_object_count: u32,
        /// The reported objects, highest-scoring first.
        items: Vec<LandStatItem>,
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
    /// The estate covenant summary, from an `EstateCovenantReply` in response to
    /// [`Session::request_estate_covenant`](crate::Session::request_estate_covenant).
    EstateCovenant(EstateCovenant),
    /// The region's telehub configuration, from a `TelehubInfo` reply to
    /// [`Session::request_telehub_info`](crate::Session::request_telehub_info)
    /// (and after each telehub-management command).
    TelehubInfo(TelehubInfo),
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
    /// World-map image-tile layers from a `MapLayerReply`, in response to
    /// [`Session::request_map_layer`](crate::Session::request_map_layer). Each
    /// [`MapLayer`] gives the texture covering a rectangular run of regions.
    MapLayers {
        /// The image-tile layers covering the grid.
        layers: Vec<MapLayer>,
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
    /// A group's financial summary (`GroupAccountSummaryReply`), in response to
    /// [`Command::RequestGroupAccountSummary`](crate::Command::RequestGroupAccountSummary).
    GroupAccountSummary(GroupAccountSummary),
    /// A group's itemised accounting detail (`GroupAccountDetailsReply`), in
    /// response to
    /// [`Command::RequestGroupAccountDetails`](crate::Command::RequestGroupAccountDetails).
    GroupAccountDetails(GroupAccountDetails),
    /// A group's transaction log (`GroupAccountTransactionsReply`), in response to
    /// [`Command::RequestGroupAccountTransactions`](crate::Command::RequestGroupAccountTransactions).
    GroupAccountTransactions(GroupAccountTransactions),
    /// A group's active proposals (`GroupActiveProposalItemReply`), in response to
    /// [`Command::RequestGroupActiveProposals`](crate::Command::RequestGroupActiveProposals).
    GroupActiveProposals {
        /// The group the proposals belong to.
        group_id: Uuid,
        /// The request's transaction id, echoed for correlation.
        transaction_id: Uuid,
        /// The total number of active proposals in the reply set.
        total_num_items: u32,
        /// The proposals in this reply message.
        proposals: Vec<GroupActiveProposalItem>,
    },
    /// One finished proposal from a group's vote history
    /// (`GroupVoteHistoryItemReply`), in response to
    /// [`Command::RequestGroupVoteHistory`](crate::Command::RequestGroupVoteHistory).
    GroupVoteHistory {
        /// The group the proposal belongs to.
        group_id: Uuid,
        /// The request's transaction id, echoed for correlation.
        transaction_id: Uuid,
        /// The total number of history items in the reply set.
        total_num_items: u32,
        /// The finished proposal and its per-candidate tallies.
        item: GroupVoteHistoryItem,
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
    /// A scripted object took or released some of the agent's movement controls
    /// (`ScriptControlChange`, i.e. `llTakeControls`/`llReleaseControls`), after
    /// the agent granted it
    /// [`ScriptPermissions::TAKE_CONTROLS`](crate::ScriptPermissions::TAKE_CONTROLS).
    /// Each entry says which controls and whether they still drive the avatar.
    /// Forcibly release all of them with
    /// [`Session::release_script_controls`](crate::Session::release_script_controls).
    ScriptControlChange(Vec<ScriptControl>),
    /// A scripted object set follow-camera parameters (`SetFollowCamProperties`,
    /// i.e. `llSetCameraParams`), after the agent granted it
    /// [`ScriptPermissions::CONTROL_CAMERA`](crate::ScriptPermissions::CONTROL_CAMERA).
    /// Carries the object id and the list of parameter/value pairs.
    SetFollowCamProperties {
        /// The scripted object that set the camera parameters.
        object_id: Uuid,
        /// The follow-camera parameters and their values.
        properties: Vec<FollowCamPropertyValue>,
    },
    /// A scripted object cleared its follow-camera parameters
    /// (`ClearFollowCamProperties`, i.e. `llClearCameraParams`), releasing
    /// control of the agent's camera.
    ClearFollowCamProperties {
        /// The scripted object that cleared the camera parameters.
        object_id: Uuid,
    },
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
    /// An object's condensed broadcast properties (`ObjectPropertiesFamily`), in
    /// response to a
    /// [`Command::RequestObjectPropertiesFamily`](crate::Command::RequestObjectPropertiesFamily).
    /// Carries just the owner/permissions/sale summary a viewer shows on hover or
    /// in the pay/report dialogs, without needing the object selected.
    ObjectPropertiesFamily {
        /// The object's condensed properties.
        properties: ObjectPropertiesFamily,
    },
    /// An object's pay-button layout (`PayPriceReply`), in response to a
    /// [`Command::RequestPayPrice`](crate::Command::RequestPayPrice): the default
    /// pay amount and the quick-pay button amounts the viewer offers (a value of
    /// `-1`/`-2` is LL's convention for "hide"/"default" buttons).
    PayPriceReply {
        /// The object queried.
        object_id: Uuid,
        /// The default pay amount, in L$ (`-1` if the object sets none).
        default_pay_price: i32,
        /// The quick-pay button amounts, in L$.
        pay_buttons: Vec<i32>,
    },
    /// A task script's run state (`ScriptRunningReply`), in response to a
    /// [`Command::RequestScriptRunning`](crate::Command::RequestScriptRunning).
    ScriptRunning {
        /// The object (task) holding the script.
        object_id: Uuid,
        /// The script inventory item inside that task.
        item_id: Uuid,
        /// Whether the script is currently running.
        running: bool,
    },
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
    /// [`Land`](crate::TerrainLayerType::Land) patch the [`values`](TerrainPatch::values)
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
    /// range or restyles. Use the baked texture ids (see [`avatar_texture`](crate::avatar_texture)) with
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
    /// Coarse (minimap) positions of nearby avatars (`CoarseLocationUpdate`).
    /// The simulator pushes this periodically; each [`CoarseLocation`] gives an
    /// avatar's whole-metre position relative to the region's south-west corner.
    CoarseLocationUpdate {
        /// The nearby avatars' coarse positions.
        locations: Vec<CoarseLocation>,
        /// The index of the agent's own entry in `locations`, if present.
        you: Option<usize>,
        /// The index of the tracked ("prey") agent in `locations`, if any (set
        /// after a [`Command::TrackAgent`](crate::Command::TrackAgent)).
        prey: Option<usize>,
    },
    /// Transient HUD effects from nearby avatars (`ViewerEffect`): look-at /
    /// point-at gaze hints, the editing/touch beam, and the other effects a
    /// viewer renders for a short time. A single message may batch several.
    ViewerEffect(Vec<ViewerEffect>),
    /// The reply to a [`Command::FindAgent`](crate::Command::FindAgent) lookup
    /// (`FindAgent`): the located global positions of the queried agent.
    FindAgentReply {
        /// The requesting agent (the "hunter").
        hunter: Uuid,
        /// The located agent (the "prey").
        prey: Uuid,
        /// The found global `(x, y)` positions, in metres.
        locations: Vec<(f64, f64)>,
    },
    /// The people results of a [`Command::DirFindQuery`](crate::Command::DirFindQuery)
    /// run with [`DirFindFlags::PEOPLE`](crate::DirFindFlags::PEOPLE) (`DirPeopleReply`).
    DirPeopleReply {
        /// The query this answers (echoed from the request).
        query_id: Uuid,
        /// The matched people.
        results: Vec<DirPeopleResult>,
    },
    /// The group results of a [`Command::DirFindQuery`](crate::Command::DirFindQuery)
    /// run with [`DirFindFlags::GROUPS`](crate::DirFindFlags::GROUPS) (`DirGroupsReply`).
    DirGroupsReply {
        /// The query this answers (echoed from the request).
        query_id: Uuid,
        /// The matched groups.
        results: Vec<DirGroupResult>,
    },
    /// The event results of a [`Command::DirFindQuery`](crate::Command::DirFindQuery)
    /// run with [`DirFindFlags::EVENTS`](crate::DirFindFlags::EVENTS) (`DirEventsReply`).
    DirEventsReply {
        /// The query this answers (echoed from the request).
        query_id: Uuid,
        /// The matched events.
        results: Vec<DirEventResult>,
        /// The search-status flags (`STATUS_SEARCH_EVENTS_*`); `0` on success.
        status: u32,
    },
    /// The results of a [`Command::DirClassifiedQuery`](crate::Command::DirClassifiedQuery)
    /// (`DirClassifiedReply`).
    DirClassifiedReply {
        /// The query this answers (echoed from the request).
        query_id: Uuid,
        /// The matched classifieds.
        results: Vec<DirClassifiedResult>,
        /// The search-status flags (`STATUS_SEARCH_CLASSIFIEDS_*`); `0` on success.
        status: u32,
    },
    /// The results of a [`Command::DirPlacesQuery`](crate::Command::DirPlacesQuery)
    /// (`DirPlacesReply`).
    DirPlacesReply {
        /// The query this answers (echoed from the request).
        query_id: Uuid,
        /// The matched places.
        results: Vec<DirPlaceResult>,
        /// The search-status flags (`STATUS_SEARCH_PLACES_*`); `0` on success.
        status: u32,
    },
    /// The results of a [`Command::DirLandQuery`](crate::Command::DirLandQuery)
    /// (`DirLandReply`).
    DirLandReply {
        /// The query this answers (echoed from the request).
        query_id: Uuid,
        /// The matched land parcels.
        results: Vec<DirLandResult>,
    },
    /// The results of an [`Command::AvatarPickerRequest`](crate::Command::AvatarPickerRequest)
    /// name autocomplete (`AvatarPickerReply`).
    AvatarPickerReply {
        /// The query this answers (echoed from the request).
        query_id: Uuid,
        /// The matched names.
        results: Vec<AvatarPickerResult>,
    },
    /// The results of a [`Command::PlacesQuery`](crate::Command::PlacesQuery)
    /// land-holdings lookup (`PlacesReply`).
    PlacesReply {
        /// The query this answers (echoed from the request).
        query_id: Uuid,
        /// The correlation id echoed from the request.
        transaction_id: Uuid,
        /// The matched land holdings.
        results: Vec<PlacesResult>,
    },
    /// The full detail of an in-world event, in response to a
    /// [`Command::EventInfoRequest`](crate::Command::EventInfoRequest)
    /// (`EventInfoReply`).
    EventInfoReply {
        /// The event's full listing.
        info: EventInfo,
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
    /// A general notification from the simulator (`AlertMessage`): a plain,
    /// already-localized message string to show the user, optionally accompanied
    /// by structured [`AlertInfo`] keys (which the viewer would look up in its
    /// `alerts.xml` for a localized rendering) and the agent ids the alert is
    /// directed at (usually empty — a region-wide alert).
    AlertMessage {
        /// The human-readable message text (`AlertData.Message`). Empty if the
        /// simulator sent only a keyed [`AlertInfo`].
        message: String,
        /// Structured, localizable alert keys and their substitution parameters
        /// (`AlertInfo`). Empty when the simulator sent only a plain string.
        alert_info: Vec<AlertInfo>,
        /// The agents this alert is directed at (`AgentInfo`). Usually empty.
        agents: Vec<Uuid>,
    },
    /// A notification directed at a specific agent (`AgentAlertMessage`): like
    /// [`AlertMessage`](Self::AlertMessage) but addressed to one agent and with a
    /// `modal` flag saying whether the viewer should block on a dialog.
    AgentAlertMessage {
        /// The agent the alert is addressed to.
        agent_id: Uuid,
        /// Whether the alert should be shown as a modal (blocking) dialog rather
        /// than a transient notification.
        modal: bool,
        /// The message text.
        message: String,
    },
    /// The simulator reported one or more "mean collisions" (`MeanCollisionAlert`):
    /// the data behind the viewer's "Bumps, Pushes & Hits" panel — avatars
    /// bumped, pushed, or hit with objects.
    MeanCollisionAlert(Vec<MeanCollision>),
    /// The agent's health changed (`HealthMessage`), e.g. in a damage-enabled
    /// region. `100.0` is full health; `0.0` triggers the home teleport.
    HealthMessage {
        /// The agent's current health.
        health: f32,
    },
    /// The simulator constrained the camera distance because of an obstruction
    /// (`CameraConstraint`): a collision plane `[nx, ny, nz, d]` (a unit normal
    /// and a distance) the viewer uses to keep the camera from clipping into
    /// objects.
    CameraConstraint {
        /// The camera collision plane as `[nx, ny, nz, d]`.
        plane: [f32; 4],
    },
    /// The session logged out cleanly (a `LogoutReply` was received).
    LoggedOut,
    /// The session disconnected for the given reason.
    Disconnected(DisconnectReason),
}
