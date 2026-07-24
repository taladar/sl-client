//! High-level commands an I/O driver sends to a running [`Session`](crate::Session).
//!
//! Both the tokio and Bevy drivers consume this single command vocabulary; it
//! lives here so the two stay in lock-step rather than maintaining parallel
//! copies.

use crate::j2c::DiscardLevel;
use crate::scoped_id::{ScopedObjectId, ScopedParcelId};
use crate::{
    AbuseReport, AgentKey, AgentPreferences, AnimationKey, AnyMessage, AssetKey, AssetType,
    AttachmentMode, AttachmentPoint, Camera, ChatChannel, ChatSessionKind, ChatType,
    ClassifiedCategory, ClassifiedKey, ClassifiedUpdate, ClickAction, ControlFlags,
    CreateGroupParams, DeRezDestination, DetachOrder, DirFindFlags, DirectoryVisibility, Distance,
    EjectAction, EstateAccessDelta, EventId, ExperienceKey, ExperiencePermission, ExperienceUpdate,
    FolderType, FreezeAction, FriendKey, FriendRights, GestureActivation, GodRegionUpdate,
    GroupKey, GroupNoticeAttachment, GroupNoticeKey, GroupRequestId, GroupRoleEdit, GroupRoleKey,
    GroupRoleMemberChange, IceCandidate, ImSessionId, InterestsUpdate, InventoryCursor,
    InventoryFolderKey, InventoryItem, InventoryKey, InventoryOffer, InventoryType, LandEdit,
    LandSearchType, LandStatReportType, LindenAmount, LureId, MapItemType, Material,
    MaterialOverrideUpdate, MediaEntry, MeshKey, MessageCursor, MoneyTransactionType, MovementMode,
    MuteFlags, MuteType, NewInventoryItem, NewInventoryLink, NotecardRez, ObjectBuyItem,
    ObjectExtraParams, ObjectFlagSettings, ObjectKey, ObjectTransform, OwnerKey, ParcelAccessEntry,
    ParcelAccessScope, ParcelCategory, ParcelKey, ParcelReturnType, ParcelUpdate, PermissionField,
    Permissions, PickKey, PickUpdate, Postcard, PrimShape, PrimShapeParams, ProfileUpdate,
    ProposalVoteId, QueryId, RegionCoordinates, RegionHandle, RegionInfoUpdate, Reliability,
    RestoreItem, RezAttachment, RezObjectParams, RezScriptParams, Rotation, SaleType,
    ScriptLanguage, ScriptPermissions, ScriptTarget, ScriptUploadLocation, SimWideDeleteFlags,
    StartLocationSlot, SurfaceInfo, TaskInventoryKey, TextureEntry, TextureKey, Throttle,
    TransactionId, UpdatableAssetType, UpdateGroupInfoParams, Uuid, Vector, ViewerEffect,
    VoiceProvisionRequest, Wearable,
};

/// A command sent to a running [`Session`](crate::Session) via an I/O driver.
#[derive(Debug)]
pub enum Command {
    /// Send an application message.
    Send {
        /// The message to send.
        message: Box<AnyMessage>,
        /// How to deliver it.
        reliability: Reliability,
    },
    /// Send local chat via `ChatFromViewer`. Incoming chat arrives as an
    /// [`Event::ChatReceived`](crate::Event::ChatReceived).
    Chat {
        /// The message text.
        message: String,
        /// The chat type (whisper / normal / shout / …).
        chat_type: ChatType,
        /// The chat channel (`0` for ordinary local chat).
        channel: ChatChannel,
    },
    /// Broadcast a local-chat typing indicator (`true` = start, `false` = stop).
    /// Other clients see it as an [`Event::ChatTyping`](crate::Event::ChatTyping).
    Typing(bool),
    /// Send a direct (1:1) instant message. Incoming IMs arrive as an
    /// [`Event::InstantMessageReceived`](crate::Event::InstantMessageReceived).
    InstantMessage {
        /// The recipient's agent id.
        to_agent_id: AgentKey,
        /// The message text.
        message: String,
    },
    /// Send an instant-message typing indicator to `to_agent_id` (`true` = start,
    /// `false` = stop). Other clients see it as an [`Event::ImTyping`](crate::Event::ImTyping).
    ImTyping {
        /// The correspondent's agent id.
        to_agent_id: AgentKey,
        /// Whether typing started (`true`) or stopped (`false`).
        typing: bool,
    },
    /// Set the agent control flags (movement); the simulator moves the agent
    /// accordingly. Pass [`ControlFlags::empty`] to stop.
    SetControls(ControlFlags),
    /// Set the per-category bandwidth throttle (`AgentThrottle`); the simulator
    /// allocates its UDP send budget accordingly. Re-sent on every region change.
    SetThrottle(Throttle),
    /// Set the agent's body and head rotation (facing/steering).
    SetRotation {
        /// The body rotation.
        body: Rotation,
        /// The head rotation.
        head: Rotation,
    },
    /// Set the agent's camera viewpoint (position and look axes); the simulator
    /// uses it to build the interest list, so the streamed scene follows where
    /// the agent looks. Build one with [`Camera::looking_at`] or directly.
    SetCamera(Camera),
    /// Stand the agent up (from sitting).
    Stand,
    /// Sit the agent on the ground where it stands.
    SitOnGround,
    /// Sit the agent on the object `target` at the region-local `offset`. The
    /// result arrives as an [`Event::SitResult`](crate::Event::SitResult).
    Sit {
        /// The object to sit on.
        target: ObjectKey,
        /// The seat offset, in region-local metres.
        offset: Vector,
    },
    /// Walk the agent to the global coordinates `(global_x, global_y, z)` using
    /// the simulator's server-side autopilot.
    Autopilot {
        /// The global X coordinate, in metres.
        global_x: f64,
        /// The global Y coordinate, in metres.
        global_y: f64,
        /// The region-local height, in metres.
        z: f64,
    },
    /// Request an avatar's profile. Replies arrive as [`Event::AvatarProperties`](crate::Event::AvatarProperties),
    /// [`Event::AvatarInterests`](crate::Event::AvatarInterests), and [`Event::AvatarGroups`](crate::Event::AvatarGroups).
    RequestAvatarProperties(AgentKey),
    /// Request an avatar's picks. The reply arrives as [`Event::AvatarPicks`](crate::Event::AvatarPicks).
    RequestAvatarPicks(AgentKey),
    /// Request the agent's private notes about an avatar. The reply arrives as
    /// [`Event::AvatarNotes`](crate::Event::AvatarNotes).
    RequestAvatarNotes(AgentKey),
    /// Request an avatar's classified ads. The reply arrives as
    /// [`Event::AvatarClassifieds`](crate::Event::AvatarClassifieds).
    RequestAvatarClassifieds(AgentKey),
    /// Request the full details of one pick. `creator_id` is the pick's owner
    /// (the `target_id` from [`Event::AvatarPicks`](crate::Event::AvatarPicks)). The reply arrives as
    /// [`Event::PickInfo`](crate::Event::PickInfo).
    RequestPickInfo {
        /// The avatar that owns the pick.
        creator_id: AgentKey,
        /// The pick id.
        pick_id: PickKey,
    },
    /// Request the full details of one classified ad. The reply arrives as
    /// [`Event::ClassifiedInfo`](crate::Event::ClassifiedInfo).
    RequestClassifiedInfo(ClassifiedKey),
    /// Replace the agent's own profile (`AvatarPropertiesUpdate`).
    UpdateProfile(ProfileUpdate),
    /// Replace the agent's own interests (`AvatarInterestsUpdate`).
    UpdateInterests(InterestsUpdate),
    /// Set the agent's private notes about an avatar (`AvatarNotesUpdate`).
    UpdateAvatarNotes {
        /// The avatar the notes are about.
        target_id: AgentKey,
        /// The note text.
        notes: String,
    },
    /// Create or edit one of the agent's picks (`PickInfoUpdate`).
    UpdatePick(PickUpdate),
    /// Delete one of the agent's picks (`PickDelete`).
    DeletePick(PickKey),
    /// Delete any agent's pick (`PickGodDelete`, god-only).
    GodDeletePick {
        /// The pick id.
        pick_id: PickKey,
        /// The query id for the dataserver to resend the pick list under.
        query_id: QueryId,
    },
    /// Create or edit one of the agent's classifieds (`ClassifiedInfoUpdate`).
    UpdateClassified(ClassifiedUpdate),
    /// Delete one of the agent's classifieds (`ClassifiedDelete`).
    DeleteClassified(ClassifiedKey),
    /// Delete any agent's classified (`ClassifiedGodDelete`, god-only).
    GodDeleteClassified {
        /// The classified id.
        classified_id: ClassifiedKey,
        /// The query id for the dataserver to resend the classified list under.
        query_id: QueryId,
    },
    /// Request the contents (sub-folders and items) of an inventory folder over
    /// **UDP** (`FetchInventoryDescendents`). The reply arrives as
    /// [`Event::InventoryDescendents`](crate::Event::InventoryDescendents). The full folder skeleton arrives once at
    /// login as [`Event::InventorySkeleton`](crate::Event::InventorySkeleton).
    RequestFolderContents(InventoryFolderKey),
    /// Fetch the contents of one or more inventory folders over the **HTTP CAPS**
    /// path (`FetchInventoryDescendents2`) — the modern path used on Second Life.
    /// Each folder's contents arrive as an [`Event::InventoryDescendents`](crate::Event::InventoryDescendents).
    FetchInventoryFolders(Vec<InventoryFolderKey>),
    /// Query one bounded page of an inventory folder's held contents from the
    /// in-memory model. The runtime replies with
    /// [`Event::InventoryFolderPage`](crate::Event::InventoryFolderPage) built from
    /// [`Session::inventory_folder_page`](crate::Session::inventory_folder_page);
    /// no wire send. If the folder's contents are not yet loaded
    /// ([`FolderState::Unknown`](crate::FolderState::Unknown)) the runtime also
    /// schedules its on-demand fetch (the
    /// [`Command::RequestFolderContents`] path), so a later query sees the loaded
    /// contents. A bevy reader may instead borrow the `Session` and call the
    /// builder directly.
    QueryInventoryFolder {
        /// Which folder to page.
        folder: InventoryFolderKey,
        /// The page boundary: `None` for the first page, or a `prev` cursor from
        /// an earlier reply to continue.
        before: Option<InventoryCursor>,
        /// The maximum number of folders + items in the page.
        limit: usize,
    },
    /// Query the inventory tree roots (the agent root and the read-only Library
    /// root). The runtime replies with
    /// [`Event::InventoryRoots`](crate::Event::InventoryRoots) built from
    /// [`Session::inventory_root`](crate::Session::inventory_root) /
    /// [`Session::library_root`](crate::Session::library_root); no wire send. A
    /// bevy reader may instead borrow the `Session` and call the accessors
    /// directly.
    QueryInventoryRoots,
    /// Query the known inventory **folders** of the agent's own tree (the login
    /// skeleton seeds every folder — with its preferred type and version — into
    /// the held model at login, so the whole folder structure is queryable before
    /// any contents fetch). The runtime replies with
    /// [`Event::InventoryFolders`](crate::Event::InventoryFolders), a snapshot of
    /// [`FolderInfo`](crate::FolderInfo)s built from
    /// [`Session::inventory_folder_infos`](crate::Session::inventory_folder_infos);
    /// no wire send. Use it to locate a system folder by its
    /// [`FolderType`](crate::FolderType) (e.g. the Current Outfit folder) without
    /// racing to capture the one-shot
    /// [`Event::InventorySkeleton`](crate::Event::InventorySkeleton). A bevy reader
    /// may instead borrow the `Session` and iterate
    /// [`Session::inventory_folders`](crate::Session::inventory_folders) directly.
    QueryInventoryFolders,
    /// Create an inventory folder (`CreateInventoryFolder`, UDP). `folder_id` is a
    /// fresh, caller-chosen id. The simulator sends no reply; the folder is added
    /// to the local cache optimistically. Use [`Command::CreateInventoryCategory`]
    /// for a confirmed reply.
    CreateInventoryFolder {
        /// The new folder's id (a fresh, caller-chosen UUID).
        folder_id: InventoryFolderKey,
        /// The parent folder.
        parent_id: InventoryFolderKey,
        /// The folder's preferred type ([`FolderType`]).
        folder_type: FolderType,
        /// The folder name.
        name: String,
    },
    /// Rename / re-type / re-parent an existing folder (`UpdateInventoryFolder`).
    UpdateInventoryFolder {
        /// The folder to update.
        folder_id: InventoryFolderKey,
        /// The (possibly new) parent folder.
        parent_id: InventoryFolderKey,
        /// The folder's preferred type ([`FolderType`]).
        folder_type: FolderType,
        /// The folder name.
        name: String,
    },
    /// Move a folder under a new parent (`MoveInventoryFolder`).
    MoveInventoryFolder {
        /// The folder to move.
        folder_id: InventoryFolderKey,
        /// The new parent folder.
        parent_id: InventoryFolderKey,
    },
    /// Delete folders (to the server trash) via `RemoveInventoryFolder`.
    RemoveInventoryFolders(Vec<InventoryFolderKey>),
    /// Create an inventory item (`CreateInventoryItem`). The simulator allocates
    /// the id and replies with an [`Event::InventoryItemCreated`](crate::Event::InventoryItemCreated).
    CreateInventoryItem(NewInventoryItem),
    /// Create a new **script** inventory item (`CreateInventoryItem` carrying the
    /// language subtype). The simulator allocates the id and fills the item with a
    /// **compilable default body** — the LSL default, or the Lua default on a
    /// SLua-capable grid when `language` is [`ScriptLanguage::Luau`] — never an
    /// empty (non-compiling) script. The reply is an
    /// [`Event::InventoryItemCreated`](crate::Event::InventoryItemCreated); follow
    /// up with [`Command::UploadScript`] to replace the body with compiled custom
    /// source. This is the script-aware counterpart to
    /// [`CreateInventoryItem`](Self::CreateInventoryItem).
    CreateScript {
        /// The folder the new script item is created in.
        folder_id: InventoryFolderKey,
        /// The new item's name.
        name: String,
        /// The new item's description.
        description: String,
        /// The next-owner permission mask for the new item.
        next_owner_mask: u32,
        /// The script language (sets the item's subtype; selects the sim's default
        /// body).
        language: ScriptLanguage,
    },
    /// Create an inventory link to an existing item or folder
    /// (`LinkInventoryItem`). The simulator allocates the link item's id and
    /// replies with an [`Event::InventoryItemCreated`](crate::Event::InventoryItemCreated).
    LinkInventoryItem(NewInventoryLink),
    /// Rewrite an item's metadata / permissions (`UpdateInventoryItem`). A non-nil
    /// `transaction_id` binds a freshly uploaded asset to the item.
    UpdateInventoryItem {
        /// The item, with its fields set to the desired values.
        item: Box<InventoryItem>,
        /// The asset transaction id (nil if not replacing the asset).
        transaction_id: TransactionId,
    },
    /// Move an item into a folder, optionally renaming it (an empty `new_name`
    /// keeps the name), via `MoveInventoryItem`.
    MoveInventoryItem {
        /// The item to move.
        item_id: InventoryKey,
        /// The destination folder.
        folder_id: InventoryFolderKey,
        /// The new name, or empty to keep the current name.
        new_name: String,
    },
    /// Copy an item into a folder (`CopyInventoryItem`). The simulator answers
    /// with an [`Event::InventoryBulkUpdate`](crate::Event::InventoryBulkUpdate) for the new item.
    CopyInventoryItem {
        /// The current owner of the source item.
        old_agent_id: AgentKey,
        /// The source item.
        old_item_id: InventoryKey,
        /// The destination folder.
        new_folder_id: InventoryFolderKey,
        /// The new item's name.
        new_name: String,
    },
    /// Delete items (`RemoveInventoryItem`).
    RemoveInventoryItems(Vec<InventoryKey>),
    /// Rewrite an item's flags (`ChangeInventoryItemFlags`).
    ChangeInventoryItemFlags {
        /// The item to change.
        item_id: InventoryKey,
        /// The new flags bitfield.
        flags: u32,
    },
    /// Empty a folder's contents (e.g. the Trash) via `PurgeInventoryDescendents`.
    PurgeInventoryDescendents(InventoryFolderKey),
    /// Delete a mixed set of folders and items in one `RemoveInventoryObjects`.
    RemoveInventoryObjects {
        /// The folders to delete.
        folder_ids: Vec<InventoryFolderKey>,
        /// The items to delete.
        item_ids: Vec<InventoryKey>,
    },
    /// Create a folder via the `CreateInventoryCategory` capability (served by
    /// both OpenSim and Second Life). Unlike the UDP `CreateInventoryFolder`, the
    /// grid replies synchronously, surfaced as an [`Event::InventoryBulkUpdate`](crate::Event::InventoryBulkUpdate)
    /// with the created folder. The runtime allocates the new folder id.
    CreateInventoryCategory {
        /// The parent folder.
        parent_id: InventoryFolderKey,
        /// The folder's preferred type (`FolderType`, or `-1`).
        folder_type: i32,
        /// The folder name.
        name: String,
    },
    /// Create a folder over the modern **AIS3** (`InventoryAPIv3`) cap
    /// (Second-Life only). The affected objects arrive as an
    /// [`Event::InventoryBulkUpdate`](crate::Event::InventoryBulkUpdate).
    Ais3CreateFolder {
        /// The parent folder.
        parent_id: InventoryFolderKey,
        /// The folder's preferred type (`FolderType`, or `-1`).
        folder_type: i32,
        /// The folder name.
        name: String,
    },
    /// Rename a folder over AIS3 (`PATCH /category/<id>`). Second-Life only.
    Ais3RenameFolder {
        /// The folder to rename.
        folder_id: InventoryFolderKey,
        /// The new name.
        name: String,
    },
    /// Move a folder over AIS3 (`PATCH /category/<id>` with `{ parent_id }`).
    /// Second-Life only.
    Ais3MoveFolder {
        /// The folder to move.
        folder_id: InventoryFolderKey,
        /// The new parent folder.
        parent_id: InventoryFolderKey,
    },
    /// Delete a folder over AIS3 (`DELETE /category/<id>`). Second-Life only.
    Ais3RemoveFolder(InventoryFolderKey),
    /// Empty a folder over AIS3 (`DELETE /category/<id>/children`). Second-Life
    /// only.
    Ais3PurgeFolder(InventoryFolderKey),
    /// Fetch a folder's children over AIS3 (`GET /category/<id>/children?depth=`).
    /// Second-Life only; the result arrives as an [`Event::InventoryBulkUpdate`](crate::Event::InventoryBulkUpdate).
    Ais3FetchFolderChildren {
        /// The folder whose children to fetch.
        folder_id: InventoryFolderKey,
        /// The recursion depth (clamped to the AIS maximum).
        depth: i32,
    },
    /// Update an item's name and description over AIS3 (`PATCH /item/<id>`).
    /// Second-Life only.
    Ais3UpdateItem {
        /// The item to update.
        item_id: InventoryKey,
        /// The new name.
        name: String,
        /// The new description.
        description: String,
    },
    /// Move an item over AIS3 (`PATCH /item/<id>` with `{ parent_id }`).
    /// Second-Life only.
    Ais3MoveItem {
        /// The item to move.
        item_id: InventoryKey,
        /// The new parent folder.
        parent_id: InventoryFolderKey,
    },
    /// Delete an item over AIS3 (`DELETE /item/<id>`). Second-Life only.
    Ais3RemoveItem(InventoryKey),
    /// Fetch a single item over AIS3 (`GET /item/<id>`). Second-Life only; the
    /// item arrives as an [`Event::InventoryBulkUpdate`](crate::Event::InventoryBulkUpdate).
    Ais3FetchItem(InventoryKey),
    /// Set the friendship rights granted to a friend (`GrantUserRights`). The
    /// `rights` bitfield combines the [`FriendRights`] `CAN_*` flags. The change
    /// is echoed back as an [`Event::FriendRightsChanged`](crate::Event::FriendRightsChanged).
    GrantUserRights {
        /// The friend whose granted rights to set.
        target: FriendKey,
        /// The new rights bitfield (combine `FriendRights::CAN_*`).
        rights: FriendRights,
    },
    /// Offer friendship to an agent (`ImprovedInstantMessage`,
    /// `IM_FRIENDSHIP_OFFERED`). The offer arrives at the recipient as an
    /// [`Event::InstantMessageReceived`](crate::Event::InstantMessageReceived) with [`ImDialog::FriendshipOffered`](crate::ImDialog::FriendshipOffered).
    OfferFriendship {
        /// The agent to offer friendship to.
        to_agent_id: AgentKey,
        /// The offer message text.
        message: String,
    },
    /// End the friendship with an agent (`TerminateFriendship`).
    TerminateFriendship(FriendKey),
    /// Accept a friendship offer (`AcceptFriendship`). The `transaction_id` is
    /// the [`InstantMessage::id`](crate::InstantMessage::id) of the incoming friendship-offer IM; `friend_id`
    /// is the offering agent (the `from_agent_id` of that same offer IM), so the
    /// session can add the new friend to its buddy cache on accept; the calling
    /// card goes into `calling_card_folder`.
    AcceptFriendship {
        /// The offer's transaction id (the friendship-offer IM's `id`).
        transaction_id: TransactionId,
        /// The offering agent's id (the offer IM's `from_agent_id`), added to the
        /// buddy cache on accept.
        friend_id: FriendKey,
        /// The inventory folder to place the new calling card in.
        calling_card_folder: InventoryFolderKey,
    },
    /// Decline a friendship offer (`DeclineFriendship`). The `transaction_id` is
    /// the [`InstantMessage::id`](crate::InstantMessage::id) of the incoming friendship-offer IM.
    DeclineFriendship(TransactionId),
    /// Offer this agent's calling card to an agent (`OfferCallingCard`) — a
    /// reference card to this avatar, *not* a friendship request. The recipient
    /// sees an [`Event::CallingCardOffered`](crate::Event::CallingCardOffered).
    OfferCallingCard {
        /// The agent to offer the calling card to.
        to_agent_id: AgentKey,
        /// Correlation id echoed by the recipient's accept/decline reply.
        transaction_id: TransactionId,
    },
    /// Accept a calling-card offer (`AcceptCallingCard`). The `transaction_id` is
    /// the one echoed by the incoming
    /// [`Event::CallingCardOffered`](crate::Event::CallingCardOffered); the card
    /// goes into `calling_card_folder`.
    AcceptCallingCard {
        /// The offer's transaction id.
        transaction_id: TransactionId,
        /// The inventory folder to file the calling card in.
        calling_card_folder: InventoryFolderKey,
    },
    /// Decline a calling-card offer (`DeclineCallingCard`). The `transaction_id`
    /// is the one echoed by the incoming
    /// [`Event::CallingCardOffered`](crate::Event::CallingCardOffered).
    DeclineCallingCard(TransactionId),
    /// Set (or clear) the agent's active group (`ActivateGroup`): `Some(group)`
    /// makes that group active, `None` clears the active group (sent on the wire
    /// as the nil group id). Confirmed by
    /// [`Event::ActiveGroupChanged`](crate::Event::ActiveGroupChanged), whose
    /// [`active_group_id`](crate::ActiveGroup::active_group_id) mirrors this
    /// `Option`.
    ActivateGroup(Option<GroupKey>),
    /// Request a group's member roster over **UDP** (`GroupMembersRequest`).
    /// Replies arrive as [`Event::GroupMembers`](crate::Event::GroupMembers).
    RequestGroupMembers(GroupKey),
    /// Fetch a group's member roster over the **HTTP CAPS** path
    /// (`GroupMemberData`) — the modern path used on Second Life. The roster
    /// arrives as an [`Event::GroupMembers`](crate::Event::GroupMembers).
    FetchGroupMembers(GroupKey),
    /// Request a group's roles. The reply arrives as [`Event::GroupRoleData`](crate::Event::GroupRoleData).
    RequestGroupRoles(GroupKey),
    /// Request a group's role↔member pairings. The reply arrives as
    /// [`Event::GroupRoleMembers`](crate::Event::GroupRoleMembers).
    RequestGroupRoleMembers(GroupKey),
    /// Request the agent's selectable titles in a group. The reply arrives as
    /// [`Event::GroupTitles`](crate::Event::GroupTitles).
    RequestGroupTitles(GroupKey),
    /// Request a group's profile. The reply arrives as
    /// [`Event::GroupProfileReceived`](crate::Event::GroupProfileReceived).
    RequestGroupProfile(GroupKey),
    /// Request a group's notice list. The reply arrives as [`Event::GroupNotices`](crate::Event::GroupNotices).
    RequestGroupNotices(GroupKey),
    /// Request a single group notice's full body (by notice id). Delivered as an
    /// [`Event::InstantMessageReceived`](crate::Event::InstantMessageReceived) with the group-notice dialog.
    RequestGroupNotice(GroupNoticeKey),
    /// Create a new group. The result arrives as [`Event::CreateGroupResult`](crate::Event::CreateGroupResult).
    CreateGroup(CreateGroupParams),
    /// Edit an existing group's profile (`UpdateGroupInfo`): charter, insignia,
    /// search visibility, fee, enrollment, and publish flags.
    UpdateGroupInfo(UpdateGroupInfoParams),
    /// Set the agent's active title in a group (`GroupTitleUpdate`) to the title
    /// carried by the given role.
    UpdateGroupTitle {
        /// The group whose title to set.
        group_id: GroupKey,
        /// The role carrying the desired title.
        title_role_id: GroupRoleKey,
    },
    /// Join an open-enrollment group. The result arrives as
    /// [`Event::JoinGroupResult`](crate::Event::JoinGroupResult).
    JoinGroup(GroupKey),
    /// Leave a group. The result arrives as [`Event::LeaveGroupResult`](crate::Event::LeaveGroupResult).
    LeaveGroup(GroupKey),
    /// Invite agents to a group, each an `(invitee_id, role_id)` pair (nil role
    /// = the default Everyone role).
    InviteToGroup {
        /// The group to invite into.
        group_id: GroupKey,
        /// The `(invitee_id, role_id)` pairs.
        invitees: Vec<(AgentKey, GroupRoleKey)>,
    },
    /// Set whether the agent accepts notices from a group / lists it in profile.
    SetGroupAcceptNotices {
        /// The group.
        group_id: GroupKey,
        /// Whether to accept notices.
        accept_notices: bool,
        /// Whether to list the group in the agent's profile.
        list_in_profile: bool,
    },
    /// Set the agent's L$ contribution to a group.
    SetGroupContribution {
        /// The group.
        group_id: GroupKey,
        /// The new contribution amount.
        contribution: i32,
    },
    /// Start (join) a group's IM session (`IM_SESSION_GROUP_START`). Group
    /// messages then arrive as [`Event::GroupSessionMessage`](crate::Event::GroupSessionMessage).
    StartGroupSession(GroupKey),
    /// Send a message into a group's IM session. Other members receive it as
    /// [`Event::GroupSessionMessage`](crate::Event::GroupSessionMessage).
    SendGroupMessage {
        /// The group (and IM session) to post to.
        group_id: GroupKey,
        /// The message text.
        message: String,
    },
    /// Leave a group's IM session (stop receiving its chat) without leaving the
    /// group itself.
    LeaveGroupSession(GroupKey),
    /// Create, update, or delete group roles (`GroupRoleUpdate`), one
    /// [`GroupRoleEdit`] per role. Re-request the roles to observe the change.
    UpdateGroupRoles {
        /// The group whose roles to edit.
        group_id: GroupKey,
        /// The role create/update/delete edits.
        roles: Vec<GroupRoleEdit>,
    },
    /// Add members to or remove members from group roles (`GroupRoleChanges`).
    ChangeGroupRoleMembers {
        /// The group whose role assignments to change.
        group_id: GroupKey,
        /// The member↔role add/remove changes.
        changes: Vec<GroupRoleMemberChange>,
    },
    /// Eject members from a group (`EjectGroupMemberRequest`). The result arrives
    /// as [`Event::EjectGroupMemberResult`](crate::Event::EjectGroupMemberResult).
    EjectGroupMembers {
        /// The group to eject from.
        group_id: GroupKey,
        /// The agent ids to eject.
        member_ids: Vec<AgentKey>,
    },
    /// Request a group's financial summary (`GroupAccountSummaryRequest`) for an
    /// accounting interval. The reply arrives as
    /// [`Event::GroupAccountSummary`](crate::Event::GroupAccountSummary).
    RequestGroupAccountSummary {
        /// The group to query.
        group_id: GroupKey,
        /// A client-chosen id echoed back in the reply for correlation.
        request_id: GroupRequestId,
        /// The interval length in days.
        interval_days: i32,
        /// Which interval (0 = current, 1 = previous).
        current_interval: i32,
    },
    /// Request a group's itemised accounting detail (`GroupAccountDetailsRequest`)
    /// for an interval. The reply arrives as
    /// [`Event::GroupAccountDetails`](crate::Event::GroupAccountDetails).
    RequestGroupAccountDetails {
        /// The group to query.
        group_id: GroupKey,
        /// A client-chosen id echoed back in the reply for correlation.
        request_id: GroupRequestId,
        /// The interval length in days.
        interval_days: i32,
        /// Which interval (0 = current, 1 = previous).
        current_interval: i32,
    },
    /// Request a group's transaction log (`GroupAccountTransactionsRequest`) for an
    /// interval. The reply arrives as
    /// [`Event::GroupAccountTransactions`](crate::Event::GroupAccountTransactions).
    RequestGroupAccountTransactions {
        /// The group to query.
        group_id: GroupKey,
        /// A client-chosen id echoed back in the reply for correlation.
        request_id: GroupRequestId,
        /// The interval length in days.
        interval_days: i32,
        /// Which interval (0 = current, 1 = previous).
        current_interval: i32,
    },
    /// Request a group's active proposals (`GroupActiveProposalsRequest`). The
    /// reply arrives as
    /// [`Event::GroupActiveProposals`](crate::Event::GroupActiveProposals).
    RequestGroupActiveProposals {
        /// The group to query.
        group_id: GroupKey,
        /// A client-chosen id echoed back in the reply for correlation.
        transaction_id: TransactionId,
    },
    /// Request a group's vote history (`GroupVoteHistoryRequest`). Each finished
    /// proposal arrives as
    /// [`Event::GroupVoteHistory`](crate::Event::GroupVoteHistory).
    RequestGroupVoteHistory {
        /// The group to query.
        group_id: GroupKey,
        /// A client-chosen id echoed back in the reply for correlation.
        transaction_id: TransactionId,
    },
    /// Start a new group proposal/vote (`StartGroupProposal`). It then appears in
    /// the group's active proposals.
    StartGroupProposal {
        /// The group to start the proposal in.
        group_id: GroupKey,
        /// The minimum number of votes required for the result to count.
        quorum: i32,
        /// The fraction of votes needed to pass (0.0–1.0).
        majority: f32,
        /// The voting window length in seconds.
        duration: i32,
        /// The proposal text.
        proposal_text: String,
    },
    /// Cast a vote on an active group proposal (`GroupProposalBallot`).
    GroupProposalBallot {
        /// The proposal's id (the `vote_id` from
        /// [`Event::GroupActiveProposals`](crate::Event::GroupActiveProposals)).
        proposal_id: ProposalVoteId,
        /// The group the proposal belongs to.
        group_id: GroupKey,
        /// The vote to cast (e.g. `"yes"`/`"no"`/`"abstain"`).
        vote_cast: String,
    },
    /// Post a group notice (`IM_GROUP_NOTICE`), optionally attaching an inventory
    /// item. The grid relays it to members who accept notices.
    SendGroupNotice {
        /// The group to post to.
        group_id: GroupKey,
        /// The notice subject.
        subject: String,
        /// The notice body.
        message: String,
        /// An optional inventory item to attach (must be copy+transfer).
        attachment: Option<GroupNoticeAttachment>,
    },
    /// Reply to a scripted-object dialog (`ScriptDialogReply`) from an
    /// [`Event::ScriptDialog`](crate::Event::ScriptDialog) — the chosen button on its hidden `chat_channel`.
    ReplyScriptDialog {
        /// The object that raised the dialog.
        object_id: ObjectKey,
        /// The dialog's hidden chat channel.
        chat_channel: ChatChannel,
        /// The chosen button index.
        button_index: i32,
        /// The chosen button label (or the typed text for an `llTextBox`).
        button_label: String,
    },
    /// Answer a scripted-object permission request (`ScriptAnswerYes`) from an
    /// [`Event::ScriptPermissionRequest`](crate::Event::ScriptPermissionRequest) — grants `permissions` (a subset of
    /// those requested; [`ScriptPermissions::default`] denies everything).
    AnswerScriptPermissions {
        /// The task (object) id holding the script.
        task_id: ObjectKey,
        /// The script item id.
        item_id: InventoryKey,
        /// The permissions to grant.
        permissions: ScriptPermissions,
        /// The experience the answered request was made under (the
        /// `experience_id` of the [`ScriptPermissionRequest`](crate::ScriptPermissionRequest)
        /// being answered, `None` outside an experience). The session keeps no
        /// outstanding-request state, so the driver passes it back from the
        /// request for the grant mirror to record.
        experience_id: Option<ExperienceKey>,
    },
    /// Request the agent's mute (block) list (`MuteListRequest`). The list
    /// arrives as [`Event::MuteList`](crate::Event::MuteList) (or [`Event::MuteListUnchanged`](crate::Event::MuteListUnchanged)).
    RequestMuteList,
    /// Mute (block) an entity (`UpdateMuteListEntry`).
    Mute {
        /// The muted entity's id (nil for a [`MuteType::ByName`] mute).
        id: Uuid,
        /// The muted entity's name.
        name: String,
        /// What kind of entity is muted.
        mute_type: MuteType,
        /// The per-aspect exception flags ([`MuteFlags::default`] mutes all).
        flags: MuteFlags,
    },
    /// Remove a mute (`RemoveMuteListEntry`); `id`/`name` must match the entry.
    Unmute {
        /// The muted entity's id.
        id: Uuid,
        /// The muted entity's name.
        name: String,
    },
    /// Teleport to `position` (region-local) in the region `region_handle`.
    Teleport {
        /// The destination region handle.
        region_handle: RegionHandle,
        /// The destination position within the region.
        position: RegionCoordinates,
        /// The look-at direction on arrival.
        look_at: Vector,
    },
    /// Request the current region's info (agent/object limits).
    RequestRegionInfo,
    /// Resolve agent ids to their legacy names (`UUIDNameRequest`); replies
    /// arrive as [`Event::AvatarNames`](crate::Event::AvatarNames). The session
    /// does not resolve or cache names on its own — a caller asks for the ids it
    /// needs (e.g. an estate's manager list) and decides what to do with the
    /// answers. Large lists are split across several requests automatically.
    RequestAvatarNames(Vec<AgentKey>),
    /// Resolve group ids to their names (`UUIDGroupNameRequest`); replies arrive
    /// as [`Event::GroupNames`](crate::Event::GroupNames). See
    /// [`RequestAvatarNames`](Self::RequestAvatarNames).
    RequestGroupNames(Vec<GroupKey>),
    /// Resolve agent ids to their **display names** over the `GetDisplayNames`
    /// capability, batching every id into one request; the reply arrives as
    /// [`Event::DisplayNames`](crate::Event::DisplayNames). This complements the
    /// always-present legacy-name lookup
    /// ([`RequestAvatarNames`](Self::RequestAvatarNames)) with the mutable,
    /// user-chosen display name, username/SLID, and the legacy first/last names in
    /// one record. The cap is Second-Life-centric (stock OpenSim serves it only
    /// with its user-management component present), so the command is a no-op when
    /// the region seed omits the capability.
    RequestDisplayNames(Vec<AgentKey>),
    /// Request the region's **feature flags** via the `SimulatorFeatures`
    /// capability; the reply arrives as
    /// [`Event::SimulatorFeatures`](crate::Event::SimulatorFeatures). The runtimes
    /// already GET this automatically once the capability map is known (at login
    /// and on each region change), so this is for an explicit re-fetch. A no-op
    /// when the region seed omits the capability.
    RequestSimulatorFeatures,
    /// Request the agent's **server-stored preferences** via the
    /// `AgentPreferences` capability without changing them (a POST with an empty
    /// body); the reply arrives as
    /// [`Event::AgentPreferences`](crate::Event::AgentPreferences) carrying the
    /// full stored set. A no-op when the region seed omits the capability.
    RequestAgentPreferences,
    /// Update the agent's **server-stored preferences** via the `AgentPreferences`
    /// capability. Only the present ([`Some`]) fields are changed (hover height,
    /// default object permission masks, maturity-access ceiling, UI language); the
    /// reply arrives as [`Event::AgentPreferences`](crate::Event::AgentPreferences)
    /// carrying the full stored set after the update. A no-op when the region seed
    /// omits the capability.
    SetAgentPreferences(Box<AgentPreferences>),
    /// Request the **land-impact / physics costs** of one or more objects via the
    /// `GetObjectCost` capability; the reply arrives as
    /// [`Event::ObjectCosts`](crate::Event::ObjectCosts). A no-op when the region
    /// seed omits the capability.
    RequestObjectCost {
        /// The objects to price (the root prim of each linkset, normally).
        object_ids: Vec<ObjectKey>,
    },
    /// Request the summed **physics/streaming/simulation cost of a selection** via
    /// the `ResourceCostSelected` capability; the reply arrives as
    /// [`Event::SelectedResourceCost`](crate::Event::SelectedResourceCost). Pass
    /// the linkset root ids (the usual viewer behaviour) with `roots = true`, or
    /// individual prim ids with `roots = false`. A no-op when the region seed omits
    /// the capability.
    RequestSelectedCost {
        /// The selected object ids.
        object_ids: Vec<ObjectKey>,
        /// Whether the ids are linkset roots (`selected_roots`) rather than
        /// individual prims (`selected_prims`).
        roots: bool,
    },
    /// Request the **physics-material parameters** of one or more objects via the
    /// `GetObjectPhysicsData` capability; the reply arrives as
    /// [`Event::ObjectPhysicsData`](crate::Event::ObjectPhysicsData). The simulator
    /// also pushes the same data unsolicited as
    /// [`Event::ObjectPhysicsProperties`](crate::Event::ObjectPhysicsProperties). A
    /// no-op when the region seed omits the capability.
    RequestObjectPhysicsData {
        /// The objects whose physics parameters to fetch.
        object_ids: Vec<ObjectKey>,
    },
    /// Request the agent's **attachment resource report** via the
    /// `AttachmentResources` capability; the reply arrives as
    /// [`Event::AttachmentResources`](crate::Event::AttachmentResources). A no-op
    /// when the region seed omits the capability.
    RequestAttachmentResources,
    /// Request a parcel's **script resource report** via the `LandResources`
    /// capability. The runtimes POST the parcel id, surface the follow-up cap URLs
    /// as [`Event::LandResourcesUrls`](crate::Event::LandResourcesUrls), then GET
    /// those URLs and surface
    /// [`Event::LandResourceSummary`](crate::Event::LandResourceSummary) and (when
    /// permitted) [`Event::LandResourceDetail`](crate::Event::LandResourceDetail).
    /// `parcel_id` is the region's "fake" parcel id (from a `RemoteParcelRequest`
    /// lookup, [`RequestRemoteParcelId`](Self::RequestRemoteParcelId)). A no-op when
    /// the region seed omits the capability.
    RequestLandResources {
        /// The grid-wide ("fake") parcel id to report on.
        parcel_id: ParcelKey,
    },
    /// Request a region's **top-scripts / top-colliders report** via a UDP
    /// `LandStatRequest`; the reply arrives as
    /// [`Event::LandStatReply`](crate::Event::LandStatReply). Requires
    /// estate-manager rights. `parcel_local_id` scopes the report to a parcel
    /// (`0` for the whole region); `filter` narrows it to objects/owners whose
    /// name contains the string (empty for no filter).
    RequestLandStat {
        /// Which report to fetch (top scripts or top colliders).
        report_type: LandStatReportType,
        /// Request flags (`0` for the default; the estate panel uses these for its
        /// filter/return options).
        request_flags: u32,
        /// A name filter, or empty for none.
        filter: String,
        /// The parcel to scope the report to, or `0` for the whole region.
        parcel_local_id: ScopedParcelId,
    },
    /// Request the extended-environment (EEP) settings via the `ExtEnvironment`
    /// capability; the reply arrives as
    /// [`Event::Environment`](crate::Event::Environment). `parcel_id` selects a
    /// parcel's environment, or [`None`] for the whole region.
    RequestEnvironment {
        /// The parcel's region-local id, or [`None`] for the region environment.
        parcel_id: Option<i32>,
    },
    /// Request `ParcelProperties` for a metre rectangle (region-local).
    RequestParcelProperties {
        /// The western edge (metres).
        west: f32,
        /// The southern edge (metres).
        south: f32,
        /// The eastern edge (metres).
        east: f32,
        /// The northern edge (metres).
        north: f32,
        /// A sequence id echoed back in the reply for matching.
        sequence_id: i32,
    },
    /// Request a parcel's properties by its region-local id
    /// (`ParcelPropertiesRequestByID`); the reply arrives as
    /// [`Event::ParcelProperties`](crate::Event::ParcelProperties).
    RequestParcelPropertiesById {
        /// The parcel's region-local id.
        local_id: ScopedParcelId,
        /// A sequence id echoed back in the reply for matching.
        sequence_id: i32,
    },
    /// Set a parcel's auto-return time for other people's objects
    /// (`ParcelSetOtherCleanTime`).
    SetParcelOtherCleanTime {
        /// The parcel's region-local id.
        local_id: ScopedParcelId,
        /// The auto-return time; whole minutes, [`Duration::ZERO`](std::time::Duration)
        /// disables auto-return.
        clean_time: std::time::Duration,
    },
    /// Terraform a piece of land (`ModifyLand`).
    ModifyLand(LandEdit),
    /// Undo the last terraform edit (`UndoLand`).
    UndoLand,
    /// Edit a parcel's settings (`ParcelPropertiesUpdate`).
    UpdateParcel(ParcelUpdate),
    /// Request a parcel's allow or ban list (`ParcelAccessListRequest`); the
    /// reply arrives as [`Event::ParcelAccessList`](crate::Event::ParcelAccessList).
    RequestParcelAccessList {
        /// The parcel's region-local id.
        local_id: ScopedParcelId,
        /// Which list to fetch (allow or ban).
        scope: ParcelAccessScope,
    },
    /// Replace a parcel's allow or ban list (`ParcelAccessListUpdate`); empty
    /// `entries` clears it.
    UpdateParcelAccessList {
        /// The parcel's region-local id.
        local_id: ScopedParcelId,
        /// Which list to set (allow or ban).
        scope: ParcelAccessScope,
        /// The new entries.
        entries: Vec<ParcelAccessEntry>,
    },
    /// Request a parcel's dwell/traffic value (`ParcelDwellRequest`); the reply
    /// arrives as [`Event::ParcelDwell`](crate::Event::ParcelDwell).
    RequestParcelDwell {
        /// The parcel's region-local id.
        local_id: ScopedParcelId,
    },
    /// Buy a parcel (`ParcelBuy`).
    BuyParcel {
        /// The parcel's region-local id.
        local_id: ScopedParcelId,
        /// The agreed price in L$.
        price: i32,
        /// The parcel area in m².
        area: i32,
        /// The group to buy for (`None` for a personal purchase).
        group_id: Option<GroupKey>,
        /// Whether the purchase is group-owned.
        is_group_owned: bool,
    },
    /// Return objects on a parcel (`ParcelReturnObjects`).
    ReturnParcelObjects {
        /// The parcel's region-local id.
        local_id: ScopedParcelId,
        /// Which objects to return (combine `ParcelReturnType` constants).
        return_type: ParcelReturnType,
        /// Optional owner-id scope.
        owner_ids: Vec<OwnerKey>,
        /// Optional explicit object/task-id scope.
        task_ids: Vec<ObjectKey>,
    },
    /// Select (highlight) objects on a parcel (`ParcelSelectObjects`).
    SelectParcelObjects {
        /// The parcel's region-local id.
        local_id: ScopedParcelId,
        /// Which objects to select (combine `ParcelReturnType` constants).
        return_type: ParcelReturnType,
        /// Explicit object ids (used with `ParcelReturnType::LIST`).
        object_ids: Vec<ObjectKey>,
    },
    /// Deed a parcel to a group (`ParcelDeedToGroup`).
    DeedParcelToGroup {
        /// The parcel's region-local id.
        local_id: ScopedParcelId,
        /// The group to deed the parcel to.
        group_id: GroupKey,
    },
    /// Reclaim a parcel to the estate (`ParcelReclaim`).
    ReclaimParcel {
        /// The parcel's region-local id.
        local_id: ScopedParcelId,
    },
    /// Release (abandon) a parcel back to the estate (`ParcelRelease`).
    ReleaseParcel {
        /// The parcel's region-local id.
        local_id: ScopedParcelId,
    },
    /// Join all owned, leased parcels within a metre rectangle into one parcel
    /// (`ParcelJoin`). Requires land rights over every parcel in the rectangle.
    JoinParcels {
        /// The western edge of the rectangle (metres, region-local).
        west: f32,
        /// The southern edge (metres).
        south: f32,
        /// The eastern edge (metres).
        east: f32,
        /// The northern edge (metres).
        north: f32,
    },
    /// Subdivide a parcel: chop the metre rectangle (which must be a subsection of
    /// exactly one parcel) out into a new parcel (`ParcelDivide`). Requires land
    /// rights over the parcel.
    DivideParcel {
        /// The western edge of the rectangle (metres, region-local).
        west: f32,
        /// The southern edge (metres).
        south: f32,
        /// The eastern edge (metres).
        east: f32,
        /// The northern edge (metres).
        north: f32,
    },
    /// Request the per-owner object tallies for a parcel
    /// (`ParcelObjectOwnersRequest`); the reply arrives as
    /// [`Event::ParcelObjectOwners`](crate::Event::ParcelObjectOwners).
    RequestParcelObjectOwners {
        /// The parcel's region-local id.
        local_id: ScopedParcelId,
    },
    /// Buy a temporary access pass to a parcel (`ParcelBuyPass`) at its configured
    /// pass price.
    BuyParcelPass {
        /// The parcel's region-local id.
        local_id: ScopedParcelId,
    },
    /// Disable (stop) scripted objects on a parcel (`ParcelDisableObjects`).
    /// `return_type` selects which objects (combine [`ParcelReturnType`]
    /// constants); `owner_ids`/`task_ids` optionally scope it (use
    /// [`ParcelReturnType::LIST`] with `task_ids` for specific objects). Requires
    /// parcel ownership / land rights.
    DisableParcelObjects {
        /// The parcel's region-local id.
        local_id: ScopedParcelId,
        /// Which objects to disable.
        return_type: ParcelReturnType,
        /// Optional owner-id scope.
        owner_ids: Vec<OwnerKey>,
        /// Optional explicit object/task-id scope.
        task_ids: Vec<ObjectKey>,
    },
    /// Request a parcel's basic listing by its grid-wide parcel id
    /// (`ParcelInfoRequest`); the reply arrives as
    /// [`Event::ParcelDetails`](crate::Event::ParcelDetails). Resolve the parcel
    /// id from a region location first with [`RequestRemoteParcelId`](Self::RequestRemoteParcelId).
    RequestParcelInfo {
        /// The parcel's grid-wide id.
        parcel_id: ParcelKey,
    },
    /// Resolve a region location to a grid-wide parcel id via the
    /// `RemoteParcelRequest` capability; the reply arrives as
    /// [`Event::RemoteParcelId`](crate::Event::RemoteParcelId), whose id then
    /// feeds [`RequestParcelInfo`](Self::RequestParcelInfo). Pass either a
    /// non-nil `region_id` or a non-zero `region_handle` (the viewer sends the id
    /// when it knows the region, the handle otherwise).
    RequestRemoteParcelId {
        /// The region-relative position whose parcel to resolve.
        location: RegionCoordinates,
        /// The region's grid-wide id (nil to send `region_handle` instead).
        region_id: Uuid,
        /// The 256 m region handle (used when `region_id` is nil).
        region_handle: RegionHandle,
    },
    /// Request the region's estate config + access lists (`getinfo`); replies
    /// arrive as [`Event::EstateInfo`](crate::Event::EstateInfo) and [`Event::EstateAccessList`](crate::Event::EstateAccessList).
    RequestEstateInfo,
    /// Download the current region's terrain as an LL **RAW** heightmap over the
    /// legacy `Xfer` path (`EstateOwnerMessage`/`terrain`
    /// `["download filename", …]`). The simulator serialises the region
    /// heightmap and offers it back via `InitiateDownload`, which the session
    /// follows automatically; the assembled bytes arrive as
    /// [`Event::ServerFileDownloaded`](crate::Event::ServerFileDownloaded).
    /// Region-owner/god gated.
    RequestRegionTerrainDownload {
        /// The local viewer-side filename the download is labelled with; echoed
        /// back on the completion event and otherwise opaque to the simulator.
        viewer_filename: String,
    },
    /// Upload `data` as the current region's terrain, an LL **RAW** heightmap,
    /// over the legacy `Xfer` path (`EstateOwnerMessage`/`terrain`
    /// `["upload filename", …]`). The simulator answers with a `RequestXfer`,
    /// which the session follows to stream the RAW up one `SendXferPacket` at a
    /// time; the final confirmation surfaces as
    /// [`Event::XferUploaded`](crate::Event::XferUploaded), and the simulator
    /// then re-broadcasts the changed terrain as
    /// [`Event::TerrainPatch`](crate::Event::TerrainPatch)es. Region-owner/god
    /// gated.
    RequestRegionTerrainUpload {
        /// The local viewer-side filename the upload is labelled with; echoed by
        /// the simulator in its `RequestXfer` and otherwise opaque.
        viewer_filename: String,
        /// The RAW heightmap bytes to upload.
        data: Vec<u8>,
    },
    /// Add/remove an agent or group on an estate access list (`estateaccessdelta`).
    UpdateEstateAccess {
        /// Which list change to apply.
        delta: EstateAccessDelta,
        /// The target agent or group id.
        target: OwnerKey,
    },
    /// Kick (eject) an agent from the region (`kickestate`).
    KickEstateUser {
        /// The agent to kick.
        target: AgentKey,
    },
    /// Teleport an agent home (`teleporthomeuser`).
    TeleportHomeUser {
        /// The agent to send home.
        target: AgentKey,
    },
    /// Teleport every agent in the region home (`teleporthomeallusers`).
    TeleportHomeAllUsers,
    /// Schedule a region restart in `seconds` (`restart`); `-1` delays a pending
    /// restart by an hour.
    RestartRegion {
        /// Seconds until restart (`-1` to delay).
        seconds: i32,
    },
    /// Send an estate-wide blue-box notice (`simulatormessage`).
    SendEstateMessage {
        /// The message body.
        message: String,
    },
    /// Update the region's settings (`setregioninfo`).
    SetRegionInfo(RegionInfoUpdate),
    /// Request the estate covenant summary (`EstateCovenantRequest`); the reply
    /// arrives as [`Event::EstateCovenant`](crate::Event::EstateCovenant).
    RequestEstateCovenant,
    /// Request the region's telehub configuration (`EstateOwnerMessage`/`telehub`
    /// `info ui`); the reply arrives as
    /// [`Event::TelehubInfo`](crate::Event::TelehubInfo). Needs estate-owner or
    /// god rights.
    RequestTelehubInfo,
    /// Connect the given object as the region's telehub (`EstateOwnerMessage`/
    /// `telehub` `connect`). Needs estate-owner or god rights.
    ConnectTelehub {
        /// The local id of the (in-region) object to make the telehub.
        object_local_id: ScopedObjectId,
    },
    /// Remove the region's telehub (`EstateOwnerMessage`/`telehub` `delete`).
    /// Needs estate-owner or god rights.
    DisconnectTelehub,
    /// Add a telehub spawn point at the given object's position
    /// (`EstateOwnerMessage`/`telehub` `spawnpoint add`). Needs estate-owner or
    /// god rights.
    AddTelehubSpawnPoint {
        /// The local id of the (in-region) object marking the spawn point.
        object_local_id: ScopedObjectId,
    },
    /// Remove a telehub spawn point by index (`EstateOwnerMessage`/`telehub`
    /// `spawnpoint remove`). Needs estate-owner or god rights.
    RemoveTelehubSpawnPoint {
        /// The zero-based index into the telehub's spawn-point list.
        spawn_index: u32,
    },
    /// God-level eject of an agent (`GodKickUser`; needs grid-god rights).
    GodKickUser {
        /// The agent to kick.
        target: AgentKey,
        /// The kick reason.
        reason: String,
    },
    /// Send a generic god-level command (`GodlikeMessage`; needs grid-god rights).
    SendGodlikeMessage {
        /// The god method name.
        method: String,
        /// The string parameters.
        params: Vec<String>,
    },
    /// Request the agent's L$ balance (`MoneyBalanceRequest`); the reply arrives
    /// as [`Event::MoneyBalance`](crate::Event::MoneyBalance).
    RequestMoneyBalance,
    /// Request the grid's economy data (`EconomyDataRequest`); the reply arrives
    /// as [`Event::EconomyData`](crate::Event::EconomyData).
    RequestEconomyData,
    /// Pay L$ to an avatar or object (`MoneyTransferRequest`).
    SendMoneyTransfer {
        /// The payee (avatar or object id).
        dest: Uuid,
        /// The L$ amount to pay.
        amount: LindenAmount,
        /// The kind of transaction (e.g. gift, pay-object).
        kind: MoneyTransactionType,
        /// A description annotating the transaction.
        description: String,
    },
    /// Set the draw distance advertised in keep-alive `AgentUpdate`s.
    SetDrawDistance(Distance),
    /// Request world-map blocks for a grid-coordinate rectangle (region
    /// indices); each region arrives as an [`Event::MapBlock`](crate::Event::MapBlock).
    RequestMapBlocks {
        /// Minimum grid x (inclusive).
        min_x: u32,
        /// Maximum grid x (inclusive).
        max_x: u32,
        /// Minimum grid y (inclusive).
        min_y: u32,
        /// Maximum grid y (inclusive).
        max_y: u32,
    },
    /// Search the world map for regions by name (`MapNameRequest`); matches
    /// arrive as [`Event::MapBlock`](crate::Event::MapBlock).
    RequestMapByName {
        /// The region name (or prefix) to search for.
        name: String,
    },
    /// Request world-map overlay items of a given type (`MapItemRequest`); the
    /// reply arrives as [`Event::MapItems`](crate::Event::MapItems).
    RequestMapItems {
        /// The kind of item to request (avatars, telehubs, land for sale, …).
        item_type: MapItemType,
        /// The target region handle (0 = the current region).
        region_handle: RegionHandle,
    },
    /// Request the world-map image-tile layers (`MapLayerRequest`); the reply
    /// arrives as [`Event::MapLayers`](crate::Event::MapLayers).
    RequestMapLayer,
    /// File an abuse / bug report over the legacy `UserReport` UDP message.
    /// Fire-and-forget; there is no reply.
    SendAbuseReport(Box<AbuseReport>),
    /// File an abuse / bug report over the modern `SendUserReport` capability
    /// (a POST). Falls back to nothing if the cap is absent; prefer
    /// [`Command::SendAbuseReport`] on grids without the cap (e.g. OpenSim).
    /// Fire-and-forget; the simulator returns only an HTTP status.
    ///
    /// When `screenshot` carries snapshot bytes and the region offers the
    /// `SendUserReportWithScreenshot` capability
    /// ([`CAP_SEND_USER_REPORT_WITH_SCREENSHOT`](crate::CAP_SEND_USER_REPORT_WITH_SCREENSHOT)),
    /// the runtime first uploads the snapshot as a texture asset over that cap's
    /// two-step uploader — filling [`AbuseReport::screenshot_id`] with the new
    /// asset id — then completes the report referencing it (mirroring the
    /// viewer's `sendReportViaCaps`). With no screenshot, or on a grid without
    /// the screenshot cap, the plain `SendUserReport` path is used.
    SendAbuseReportViaCaps {
        /// The report to file.
        report: Box<AbuseReport>,
        /// Optional snapshot image bytes (a JPEG-2000 codestream) to upload and
        /// attach via the `SendUserReportWithScreenshot` cap; `None` for the
        /// plain no-screenshot path.
        screenshot: Option<Vec<u8>>,
    },
    /// Email a snapshot postcard over the `SendPostcard` UDP message (the
    /// snapshot must already be uploaded as the referenced asset).
    /// Fire-and-forget; there is no reply.
    SendPostcard(Box<Postcard>),
    /// Request the full `ObjectUpdate` for the given region-local ids
    /// (`RequestMultipleObjects`); updates arrive as [`Event::ObjectAdded`](crate::Event::ObjectAdded) /
    /// [`Event::ObjectUpdated`](crate::Event::ObjectUpdated).
    RequestObjects {
        /// The region-local ids to (re)fetch.
        local_ids: Vec<ScopedObjectId>,
    },
    /// Request objects' extended properties by selecting them (`ObjectSelect`);
    /// the reply arrives as [`Event::ObjectProperties`](crate::Event::ObjectProperties).
    RequestObjectProperties {
        /// The region-local ids to select.
        local_ids: Vec<ScopedObjectId>,
    },
    /// Deselect previously selected objects (`ObjectDeselect`).
    DeselectObjects {
        /// The region-local ids to deselect.
        local_ids: Vec<ScopedObjectId>,
    },
    /// Touch (left-click) an object (`ObjectGrab` + `ObjectDeGrab`).
    TouchObject {
        /// The object's region-local id.
        local_id: ScopedObjectId,
        /// Where on the object the touch landed (a renderer's pick), or `None`
        /// when the caller only knows the object's id — see [`SurfaceInfo`].
        surface: Option<SurfaceInfo>,
    },
    /// Begin grabbing an object (`ObjectGrab`).
    GrabObject {
        /// The object's region-local id.
        local_id: ScopedObjectId,
        /// The grab offset from the object's centre.
        grab_offset: Vector,
        /// Where on the object the grab landed, or `None` — see [`SurfaceInfo`].
        surface: Option<SurfaceInfo>,
    },
    /// Update an in-progress grab as the object is dragged (`ObjectGrabUpdate`).
    GrabObjectUpdate {
        /// The object's persistent global id.
        object_id: ObjectKey,
        /// The initial grab offset.
        grab_offset_initial: Vector,
        /// The current region-local grab position.
        grab_position: Vector,
        /// Milliseconds since the previous update.
        time_since_last: u32,
        /// Where on the object the drag currently is, or `None` — see
        /// [`SurfaceInfo`].
        surface: Option<SurfaceInfo>,
    },
    /// Release a grab on an object (`ObjectDeGrab`).
    DegrabObject {
        /// The object's region-local id.
        local_id: ScopedObjectId,
        /// Where on the object the release landed, or `None` — see
        /// [`SurfaceInfo`].
        surface: Option<SurfaceInfo>,
    },
    /// Rez (create) a new primitive (`ObjectAdd`).
    RezObject {
        /// The shape of the prim to rez.
        shape: PrimShape,
        /// The group the new object is set to (`None` for none).
        group_id: Option<GroupKey>,
    },
    /// Duplicate objects with an offset (`ObjectDuplicate`).
    DuplicateObjects {
        /// The region-local ids to duplicate.
        local_ids: Vec<ScopedObjectId>,
        /// The offset to apply to the copies.
        offset: Vector,
        /// The group the copies are set to (`None` for none).
        group_id: Option<GroupKey>,
    },
    /// Delete objects to the trash (`ObjectDelete`).
    DeleteObjects {
        /// The region-local ids to delete.
        local_ids: Vec<ScopedObjectId>,
    },
    /// Derez objects (take/return/trash; `DeRezObject`).
    DerezObjects {
        /// The region-local ids to derez.
        local_ids: Vec<ScopedObjectId>,
        /// Where the objects should go — the destination carries its own folder,
        /// item, or task id where applicable.
        destination: DeRezDestination,
        /// A caller-chosen id correlating the resulting inventory update.
        transaction_id: TransactionId,
        /// The active group (`None` for none).
        group_id: Option<GroupKey>,
    },
    /// Move/rotate/scale an object (`MultipleObjectUpdate`).
    UpdateObject {
        /// The object's region-local id.
        local_id: ScopedObjectId,
        /// The transform to apply (only set components change).
        transform: ObjectTransform,
    },
    /// Rename an object (`ObjectName`).
    SetObjectName {
        /// The object's region-local id.
        local_id: ScopedObjectId,
        /// The new name.
        name: String,
    },
    /// Re-describe an object (`ObjectDescription`).
    SetObjectDescription {
        /// The object's region-local id.
        local_id: ScopedObjectId,
        /// The new description.
        description: String,
    },
    /// Set an object's left-click behaviour (`ObjectClickAction`).
    SetObjectClickAction {
        /// The object's region-local id.
        local_id: ScopedObjectId,
        /// The new click action.
        action: ClickAction,
    },
    /// Set an object's physical material (`ObjectMaterial`).
    SetObjectMaterial {
        /// The object's region-local id.
        local_id: ScopedObjectId,
        /// The new material.
        material: Material,
    },
    /// Set an object's physics/temporary/phantom flags (`ObjectFlagUpdate`).
    SetObjectFlags {
        /// The object's region-local id.
        local_id: ScopedObjectId,
        /// The flag settings to apply.
        flags: ObjectFlagSettings,
    },
    /// Set the path/profile geometry of an object (`ObjectShape`).
    SetObjectShape {
        /// The object's region-local id.
        local_id: ScopedObjectId,
        /// The quantized path/profile shape parameters.
        shape: PrimShapeParams,
    },
    /// Set the per-face textures of an object (`ObjectImage`).
    SetObjectImage {
        /// The object's region-local id.
        local_id: ScopedObjectId,
        /// The legacy parcel-media URL ([`None`] for none).
        media_url: Option<String>,
        /// The new per-face texture entry.
        texture_entry: TextureEntry,
    },
    /// Set the complete extra-parameter state of an object (`ObjectExtraParams`);
    /// subtypes absent from `params` are cleared.
    SetObjectExtraParams {
        /// The object's region-local id.
        local_id: ScopedObjectId,
        /// The flexi/light/sculpt/… parameters to apply.
        params: ObjectExtraParams,
    },
    /// Set the group objects are set to (`ObjectGroup`).
    SetObjectGroup {
        /// The region-local ids.
        local_ids: Vec<ScopedObjectId>,
        /// The group id.
        group_id: GroupKey,
    },
    /// Deed objects to the group they are set to (`ObjectOwner` with a nil
    /// owner); irreversible for a plain resident.
    DeedObjectsToGroup {
        /// The region-local ids.
        local_ids: Vec<ScopedObjectId>,
        /// The group to deed to (the group the objects are set to).
        group_id: GroupKey,
    },
    /// Set or clear permission bits on objects (`ObjectPermissions`).
    SetObjectPermissions {
        /// The region-local ids.
        local_ids: Vec<ScopedObjectId>,
        /// Which mask to change.
        field: PermissionField,
        /// Whether to set (true) or clear (false) the bits.
        set: bool,
        /// The permission bits to set or clear (e.g. [`Permissions::COPY`]).
        mask: Permissions,
    },
    /// Set an object's sale type and price (`ObjectSaleInfo`).
    SetObjectForSale {
        /// The object's region-local id.
        local_id: ScopedObjectId,
        /// The sale type.
        sale_type: SaleType,
        /// The asking price in L$ when putting the object up for sale, or `None`
        /// when taking it off sale (`sale_type == SaleType::NotForSale`); encoded
        /// as `0` on the wire. A for-sale object may still be free
        /// (`Some(LindenAmount(0))`).
        sale_price: Option<LindenAmount>,
    },
    /// Set an object's category code (`ObjectCategory`).
    SetObjectCategory {
        /// The object's region-local id.
        local_id: ScopedObjectId,
        /// The category code.
        category: u32,
    },
    /// Toggle whether an object is listed in search (`ObjectIncludeInSearch`).
    SetObjectIncludeInSearch {
        /// The object's region-local id.
        local_id: ScopedObjectId,
        /// Whether to include the object in search.
        include: bool,
    },
    /// Link objects into one linkset (`ObjectLink`); the first id is the root.
    LinkObjects {
        /// The region-local ids to link (first = root).
        local_ids: Vec<ScopedObjectId>,
    },
    /// Unlink objects from their linksets (`ObjectDelink`).
    DelinkObjects {
        /// The region-local ids to unlink.
        local_ids: Vec<ScopedObjectId>,
    },
    /// Buy one or more in-world objects offered for sale (`ObjectBuy`). The sale
    /// type and price in each [`ObjectBuyItem`] must match what the object
    /// advertises (see [`Command::RequestObjectPropertiesFamily`]); the simulator
    /// rejects a mismatch. A successful purchase (when derezed) places the object
    /// in `category_id`.
    BuyObject {
        /// The active group ([`Uuid::nil`] for none).
        group_id: GroupKey,
        /// The inventory folder a derezed purchase is placed in.
        category_id: Uuid,
        /// The objects to buy (each with its advertised sale type and price).
        objects: Vec<ObjectBuyItem>,
    },
    /// Buy a single item out of an object's contents (`BuyObjectInventory`): on
    /// success the simulator copies the item into the agent's inventory.
    BuyObjectInventory {
        /// The object whose contents holds the item.
        object_id: ObjectKey,
        /// The inventory item to buy.
        item_id: InventoryKey,
        /// The folder the bought item is placed in.
        folder_id: InventoryFolderKey,
    },
    /// Ask an object for its pay-button layout (`RequestPayPrice`); the simulator
    /// answers with an [`Event::PayPriceReply`](crate::Event::PayPriceReply)
    /// listing the default price and the quick-pay button amounts.
    RequestPayPrice {
        /// The object to query.
        object_id: ObjectKey,
    },
    /// Ask for an object's condensed broadcast properties
    /// (`RequestObjectPropertiesFamily`); the simulator answers with an
    /// [`Event::ObjectPropertiesFamily`](crate::Event::ObjectPropertiesFamily).
    /// Unlike [`Command::RequestObjectProperties`] this needs no prior selection.
    RequestObjectPropertiesFamily {
        /// The request flags (e.g. `OBJECT_PAY_REQUEST` `0x04`), echoed back in
        /// the reply; `0` for a plain hover/info query.
        request_flags: u32,
        /// The object to query.
        object_id: ObjectKey,
    },
    /// Begin an interactive spin (rotate) of an object (`ObjectSpinStart`); pairs
    /// with [`Command::SpinObjectUpdate`] and [`Command::SpinObjectStop`].
    SpinObjectStart {
        /// The object being spun.
        object_id: ObjectKey,
    },
    /// Update an in-progress object spin with the latest rotation
    /// (`ObjectSpinUpdate`).
    SpinObjectUpdate {
        /// The object being spun.
        object_id: ObjectKey,
        /// The new rotation.
        rotation: Rotation,
    },
    /// End an interactive object spin (`ObjectSpinStop`).
    SpinObjectStop {
        /// The object being spun.
        object_id: ObjectKey,
    },
    /// Duplicate objects, placing the copies against the surface a ray hits
    /// (`ObjectDuplicateOnRay`) — the "copy and drop in place" gesture.
    DuplicateObjectsOnRay {
        /// The region-local ids to duplicate.
        local_ids: Vec<ScopedObjectId>,
        /// The active group the copies are set to (`None` for none).
        group_id: Option<GroupKey>,
        /// The ray's start point (region-local).
        ray_start: Vector,
        /// The ray's end point (region-local).
        ray_end: Vector,
        /// When set, the simulator trusts `ray_end` rather than raycasting.
        bypass_raycast: bool,
        /// Whether `ray_end` is the actual intersection point.
        ray_end_is_intersection: bool,
        /// Whether to copy each object's centre offset.
        copy_centers: bool,
        /// Whether to copy each object's rotation.
        copy_rotates: bool,
        /// The object the ray is cast against (`None` for the terrain).
        ray_target_id: Option<ObjectKey>,
        /// The duplicate flags (see `object_flags.h`).
        duplicate_flags: u32,
    },
    /// Restore an inventory item to the world at its last in-world position
    /// (`RezRestoreToWorld`). The message is `UDPDeprecated`, but a viewer may
    /// still send it.
    RezRestoreToWorld {
        /// The full inventory item to restore.
        item: RestoreItem,
    },
    /// Rez an object embedded in a notecard asset (`RezObjectFromNotecard`).
    RezObjectFromNotecard {
        /// The rez parameters (ray placement, permissions, notecard, items).
        rez: NotecardRez,
    },
    /// Rez an inventory item into the world as a new object (`RezObject`).
    RezObjectFromInventory {
        /// The rez parameters (ray placement, permission masks, source item).
        params: Box<RezObjectParams>,
    },
    /// Drop a script inventory item into an in-world object (`RezScript`).
    RezScript {
        /// The object whose task inventory the script is dropped into.
        target: ScopedObjectId,
        /// The rez parameters (running flag, active group, source item).
        params: Box<RezScriptParams>,
    },
    /// Revoke LSL script permissions previously granted to an object
    /// (`RevokePermissions`).
    RevokeScriptPermissions {
        /// The object whose granted permissions are revoked.
        object_id: ObjectKey,
        /// The permissions to revoke.
        permissions: ScriptPermissions,
    },
    /// Detach a worn attachment back into inventory by its inventory item id
    /// (`DetachAttachmentIntoInv`).
    DetachAttachmentIntoInventory {
        /// The attachment's inventory item id.
        item_id: InventoryKey,
    },
    /// Request an in-world object's task (object) inventory listing
    /// (`RequestTaskInventory`); the reply arrives as
    /// [`Event::TaskInventoryReply`](crate::Event::TaskInventoryReply), giving
    /// the contents serial and the `Xfer` filename. To also download and parse
    /// the listing, use [`Command::FetchTaskInventory`].
    RequestTaskInventory {
        /// The object whose task inventory is requested.
        target: ScopedObjectId,
    },
    /// Request *and read* an in-world object's task inventory: sends
    /// `RequestTaskInventory`, then downloads and parses the `Xfer` listing its
    /// reply names, surfacing the parsed items as
    /// [`Event::TaskInventoryContents`](crate::Event::TaskInventoryContents)
    /// (the [`Event::TaskInventoryReply`](crate::Event::TaskInventoryReply) is
    /// still emitted first).
    FetchTaskInventory {
        /// The object whose task inventory is requested.
        target: ScopedObjectId,
    },
    /// Download an arbitrary named file over the legacy `Xfer` path
    /// (`RequestXfer`); the assembled bytes arrive as
    /// [`Event::XferDownloaded`](crate::Event::XferDownloaded). Use this for a
    /// raw `Xfer` `filename` handed to you by another message (for example a
    /// [`Event::TaskInventoryReply`](crate::Event::TaskInventoryReply) filename
    /// whose bytes you want unparsed).
    RequestXfer {
        /// The `Xfer` filename to download.
        filename: String,
    },
    /// Write (add or replace) an item in an in-world object's task inventory
    /// (`UpdateTaskInventory`).
    UpdateTaskInventory {
        /// The object whose task inventory is updated.
        target: ScopedObjectId,
        /// Whether the simulator matches the item by item id or asset id.
        key: TaskInventoryKey,
        /// The full inventory item to write.
        item: Box<RestoreItem>,
    },
    /// Move an item out of an in-world object's task inventory into an agent
    /// inventory folder (`MoveTaskInventory`).
    MoveTaskInventory {
        /// The object whose task inventory the item is moved out of.
        target: ScopedObjectId,
        /// The destination agent inventory folder.
        folder_id: InventoryFolderKey,
        /// The task inventory item to move.
        item_id: InventoryKey,
    },
    /// Remove an item from an in-world object's task inventory
    /// (`RemoveTaskInventory`).
    RemoveTaskInventory {
        /// The object whose task inventory the item is removed from.
        target: ScopedObjectId,
        /// The task inventory item to remove.
        item_id: InventoryKey,
    },
    /// Ask whether a task's script is currently running (`GetScriptRunning`);
    /// the simulator answers with
    /// [`Event::ScriptRunning`](crate::Event::ScriptRunning).
    RequestScriptRunning {
        /// The object (task) holding the script.
        object_id: ObjectKey,
        /// The script inventory item inside that task.
        item_id: InventoryKey,
    },
    /// Start or stop a task's script (`SetScriptRunning`).
    SetScriptRunning {
        /// The object (task) holding the script.
        object_id: ObjectKey,
        /// The script inventory item inside that task.
        item_id: InventoryKey,
        /// `true` to run the script, `false` to stop it.
        running: bool,
    },
    /// Reset a task's script to its initial state (`ScriptReset`), as if it had
    /// just been (re)compiled.
    ResetScript {
        /// The object (task) holding the script.
        object_id: ObjectKey,
        /// The script inventory item inside that task.
        item_id: InventoryKey,
    },
    /// Request a texture over the legacy UDP image path (`RequestImage`); the
    /// reassembled image arrives as [`Event::TextureReceived`](crate::Event::TextureReceived) (or
    /// [`Event::TextureNotFound`](crate::Event::TextureNotFound)).
    RequestTexture {
        /// The texture's asset id.
        texture_id: TextureKey,
        /// The level of detail (0 = full resolution; higher = coarser).
        discard_level: i8,
        /// The download priority (larger is fetched sooner).
        priority: f32,
    },
    /// Fetch a texture over the HTTP `GetTexture` capability; the image arrives
    /// as [`Event::TextureReceived`](crate::Event::TextureReceived) (or [`Event::TextureNotFound`](crate::Event::TextureNotFound)). When
    /// `discard_level` is non-zero the codestream is truncated to that
    /// level-of-detail prefix via [`j2c`](crate::j2c).
    FetchTexture {
        /// The texture's asset id.
        texture_id: TextureKey,
        /// The level of detail ([`DiscardLevel::FULL`] = full resolution; higher
        /// = coarser).
        discard_level: DiscardLevel,
    },
    /// Fetch a mesh asset over the HTTP `GetMesh2`/`GetMesh` capability; the data
    /// arrives as [`Event::AssetReceived`](crate::Event::AssetReceived). An optional `byte_range` (inclusive
    /// `(start, end)` byte offsets) issues an HTTP `Range` request so only that
    /// span is transferred — e.g. a single mesh LOD whose offsets the caller read
    /// from the mesh header. `None` fetches the whole asset.
    FetchMesh {
        /// The mesh asset's id.
        mesh_id: MeshKey,
        /// Optional inclusive `(start, end)` byte range to fetch.
        byte_range: Option<(u32, u32)>,
    },
    /// Fetch a generic asset over the HTTP `GetAsset` capability; the data
    /// arrives as [`Event::AssetReceived`](crate::Event::AssetReceived) (or [`Event::AssetTransferFailed`](crate::Event::AssetTransferFailed)).
    /// An optional `byte_range` (inclusive `(start, end)` byte offsets) issues an
    /// HTTP `Range` request so only that span is transferred; `None` fetches the
    /// whole asset.
    FetchAsset {
        /// The asset's id.
        asset_id: AssetKey,
        /// The asset's class (selects the cap query parameter).
        asset_type: AssetType,
        /// Optional inclusive `(start, end)` byte range to fetch.
        byte_range: Option<(u32, u32)>,
    },
    /// Ask the simulator to (re-)send the agent's own wearables
    /// (`AgentWearablesRequest`); the reply arrives as [`Event::AgentWearables`](crate::Event::AgentWearables).
    RequestWearables,
    /// Set the agent's outfit (`AgentIsNowWearing`): the complete set of
    /// wearables to wear. The simulator acknowledges with a fresh
    /// [`Event::AgentWearables`](crate::Event::AgentWearables).
    SetWearing(Vec<Wearable>),
    /// Advertise the agent's own appearance (`AgentSetAppearance`): the legacy
    /// client-side bake path (used by OpenSim and pre-server-baking regions).
    /// `serial` must strictly increase across calls.
    SetAppearance {
        /// The appearance serial (strictly increasing; 0 resets).
        serial: u32,
        /// The agent's bounding-box size, in metres.
        size: Vector,
        /// The packed `TextureEntry` blob carrying the baked-texture ids.
        texture_entry: Vec<u8>,
        /// The visual parameter bytes (one per parameter, in viewer order).
        visual_params: Vec<u8>,
        /// The per-baked-slot cache hashes (`(cache id, texture slot index)`).
        wearable_cache: Vec<(Uuid, u8)>,
    },
    /// Query the simulator's baked-texture cache (`AgentCachedTexture`): the
    /// reply arrives as [`Event::CachedTextureResponse`](crate::Event::CachedTextureResponse).
    RequestCachedTextures {
        /// The serial echoed back in the reply.
        serial: i32,
        /// The queried slots, as `(cache id, texture slot index)` pairs.
        slots: Vec<(Uuid, u8)>,
    },
    /// Trigger a modern server-side appearance bake over the HTTP
    /// `UpdateAvatarAppearance` capability (Second Life "central baking"): the
    /// grid composites the agent's Current Outfit Folder and broadcasts the
    /// result as [`Event::AvatarAppearance`](crate::Event::AvatarAppearance). The POST's own reply arrives as
    /// [`Event::ServerAppearanceUpdate`](crate::Event::ServerAppearanceUpdate).
    RequestServerAppearanceUpdate {
        /// The Current Outfit Folder version the grid should bake.
        cof_version: i32,
    },
    /// Start and/or stop several of the agent's own animations (`AgentAnimation`):
    /// each `(anim_id, start)` pair starts (`true`) or stops (`false`) one
    /// animation. Other avatars observe the result as an
    /// [`Event::AvatarAnimation`](crate::Event::AvatarAnimation).
    SetAnimations(Vec<(AnimationKey, bool)>),
    /// Start one of the agent's own animations (`AgentAnimation`); convenience
    /// for a single-element [`Command::SetAnimations`].
    PlayAnimation(AnimationKey),
    /// Stop one of the agent's own animations (`AgentAnimation`); convenience for
    /// a single-element [`Command::SetAnimations`].
    StopAnimation(AnimationKey),
    /// Mark one or more gestures active for this session (`ActivateGestures`),
    /// so the simulator preloads them and they fire on their trigger
    /// words/keys. The gesture assets themselves are uploaded separately (via
    /// inventory); this only toggles which are live.
    ActivateGestures {
        /// The gestures to activate (each pairs an inventory item id with its
        /// gesture asset id).
        gestures: Vec<GestureActivation>,
    },
    /// Mark one or more gestures inactive for this session
    /// (`DeactivateGestures`), naming them by inventory item id.
    DeactivateGestures {
        /// The inventory item ids of the gestures to deactivate.
        item_ids: Vec<InventoryKey>,
    },
    /// Choose whether the avatar runs or walks for ground movement
    /// (`SetAlwaysRun`). Fire-and-forget; there is no reply.
    SetAlwaysRun {
        /// Whether the avatar always runs or walks.
        mode: MovementMode,
    },
    /// Tell the simulator the viewer has stalled and is not reading the network
    /// (`AgentPause`), so it stops streaming updates until a
    /// [`Command::ResumeAgent`]. Fire-and-forget; there is no reply.
    PauseAgent,
    /// Tell the simulator the viewer has resumed reading the network
    /// (`AgentResume`) after a [`Command::PauseAgent`]. Fire-and-forget; there is
    /// no reply.
    ResumeAgent,
    /// Update the agent's vertical field of view (`AgentFOV`), in radians. The
    /// simulator uses it for interest-list culling. Fire-and-forget; there is no
    /// reply.
    SetAgentFov {
        /// The vertical field of view, in radians.
        vertical_angle: f32,
    },
    /// Update the agent's viewport size in pixels (`AgentHeightWidth`), sent when
    /// the viewer window is created or resized. Fire-and-forget; there is no
    /// reply.
    SetAgentSize {
        /// The viewport height in pixels.
        height: u16,
        /// The viewport width in pixels.
        width: u16,
    },
    /// Forcibly release any agent movement controls a script has taken
    /// (`ForceScriptControlRelease`), reversing a `ScriptControlChange`.
    /// Fire-and-forget; there is no reply.
    ReleaseScriptControls,
    /// Request a snapshot of the session's script-permission mirror (the recorded
    /// grants/denials and the currently-held movement controls). This is a
    /// local query — it sends nothing on the wire; the runtime answers it by
    /// building a [`ScriptPermissionState`](crate::ScriptPermissionState) from the
    /// session and surfacing it as
    /// [`Event::ScriptPermissionState`](crate::Event::ScriptPermissionState).
    QueryScriptPermissions,
    /// Attach an in-world object (selected by its region-local id) to the avatar
    /// (`ObjectAttach`). The object is worn at `attachment_point`; `mode` chooses
    /// whether it is added alongside anything already on that point or replaces
    /// it ([`AttachmentPoint::Default`] lets the simulator pick the object's
    /// saved/scripted slot). To wear an item straight from inventory instead, use
    /// [`Command::RezAttachment`].
    AttachObject {
        /// The in-world object's region-local id.
        local_id: ScopedObjectId,
        /// The point to attach the object to.
        attachment_point: AttachmentPoint,
        /// Whether to add the attachment or replace what is on the point.
        mode: AttachmentMode,
        /// The rotation to wear the object at, relative to the attachment point.
        rotation: Rotation,
    },
    /// Detach attachments back to inventory by their region-local ids
    /// (`ObjectDetach`), marking each item as no longer "(worn)".
    DetachObjects {
        /// The attachments' region-local ids.
        local_ids: Vec<ScopedObjectId>,
    },
    /// Drop attachments from the avatar onto the ground by their region-local ids
    /// (`ObjectDrop`): they become ordinary in-world objects at the avatar's
    /// location rather than returning to inventory.
    DropAttachments {
        /// The attachments' region-local ids.
        local_ids: Vec<ScopedObjectId>,
    },
    /// Remove (take off) an attachment by its inventory item id
    /// (`RemoveAttachment`). Unlike [`Command::DetachObjects`] this names the
    /// inventory item rather than the rezzed object's region-local id.
    RemoveAttachment {
        /// The attachment point the item is worn on (the simulator resolves the
        /// item by id; [`AttachmentPoint::Default`] is accepted).
        attachment_point: AttachmentPoint,
        /// The worn item's inventory item id.
        item_id: InventoryKey,
    },
    /// Wear an inventory item as an attachment (`RezSingleAttachmentFromInv`):
    /// rez it directly onto the avatar from inventory. To attach an object that
    /// is already rezzed in-world, use [`Command::AttachObject`].
    RezAttachment(RezAttachment),
    /// Wear several inventory items as attachments in one compound message
    /// (`RezMultipleAttachmentsFromInv`).
    RezAttachments {
        /// A fresh, caller-chosen id correlating the compound message's parts
        /// (the viewer generates a new UUID per request).
        compound_id: TransactionId,
        /// Whether to first detach everything currently worn (e.g. when replacing
        /// the whole outfit) or keep it and add these alongside.
        detach: DetachOrder,
        /// The items to wear.
        attachments: Vec<RezAttachment>,
    },
    /// Send one or more viewer effects (`ViewerEffect`): the look-at / point-at
    /// gaze hints, the editing/touch beam, and the other transient HUD effects
    /// other viewers render. The effects are batched into a single message;
    /// each carries its own id, source agent, type, duration, colour and
    /// effect-specific payload.
    ViewerEffect(Vec<ViewerEffect>),
    /// Track an agent's position on the world map (`TrackAgent`): the simulator
    /// streams the tracked ("prey") agent's coarse location back via
    /// `CoarseLocationUpdate` (surfaced as
    /// [`Event::CoarseLocationUpdate`](crate::Event::CoarseLocationUpdate), whose
    /// `prey` index then points at the tracked avatar).
    TrackAgent {
        /// The agent to track.
        prey_id: AgentKey,
    },
    /// Ask the simulator for an agent's global position (`FindAgent`): an
    /// estate/god lookup. The simulator answers with a `FindAgent` carrying the
    /// found positions, surfaced as
    /// [`Event::FindAgentReply`](crate::Event::FindAgentReply).
    FindAgent {
        /// The requesting agent (the "hunter"); usually the agent's own id.
        hunter: AgentKey,
        /// The agent to locate (the "prey").
        prey: AgentKey,
    },
    /// Run a directory people / groups / events search (`DirFindQuery`): the
    /// unified *Search* query whose [`DirFindFlags`] select what is searched
    /// (set [`DirFindFlags::PEOPLE`], [`DirFindFlags::GROUPS`] or
    /// [`DirFindFlags::EVENTS`]) and how the results are sorted/filtered. The
    /// simulator answers with a matching
    /// [`Event::DirPeopleReply`](crate::Event::DirPeopleReply),
    /// [`Event::DirGroupsReply`](crate::Event::DirGroupsReply) or
    /// [`Event::DirEventsReply`](crate::Event::DirEventsReply), correlated by
    /// `query_id`.
    DirFindQuery {
        /// A client-chosen id echoed back in the reply.
        query_id: QueryId,
        /// The search text.
        query_text: String,
        /// What to search and how to sort/filter.
        flags: DirFindFlags,
        /// The 0-based index of the first result to return (for paging).
        query_start: i32,
    },
    /// Search the places directory (`DirPlacesQuery`): named parcels, optionally
    /// filtered by [`ParcelCategory`]. The simulator answers with a
    /// [`Event::DirPlacesReply`](crate::Event::DirPlacesReply).
    DirPlacesQuery {
        /// A client-chosen id echoed back in the reply.
        query_id: QueryId,
        /// The search text.
        query_text: String,
        /// Result inclusion/sort flags (the maturity-inclusion and sort bits).
        flags: DirFindFlags,
        /// The parcel category to filter by ([`ParcelCategory::None`] for any).
        category: ParcelCategory,
        /// An optional region-name filter (empty for any region).
        sim_name: String,
        /// The 0-based index of the first result to return (for paging).
        query_start: i32,
    },
    /// Search the land-for-sale directory (`DirLandQuery`): parcels for sale or
    /// auction, filtered by sale type, price and area. The simulator answers
    /// with a [`Event::DirLandReply`](crate::Event::DirLandReply).
    DirLandQuery {
        /// A client-chosen id echoed back in the reply.
        query_id: QueryId,
        /// Result inclusion/sort and limit flags (e.g.
        /// [`DirFindFlags::FOR_SALE`], [`DirFindFlags::LIMIT_BY_PRICE`]).
        flags: DirFindFlags,
        /// Which sale types to include.
        search_type: LandSearchType,
        /// The price limit, applied when [`DirFindFlags::LIMIT_BY_PRICE`] is set.
        price: i32,
        /// The area limit, applied when [`DirFindFlags::LIMIT_BY_AREA`] is set.
        area: i32,
        /// The 0-based index of the first result to return (for paging).
        query_start: i32,
    },
    /// Search the classifieds directory (`DirClassifiedQuery`). The simulator
    /// answers with a [`Event::DirClassifiedReply`](crate::Event::DirClassifiedReply).
    DirClassifiedQuery {
        /// A client-chosen id echoed back in the reply.
        query_id: QueryId,
        /// The search text.
        query_text: String,
        /// Result inclusion/sort flags (the maturity-inclusion and sort bits).
        flags: DirFindFlags,
        /// The classified category to filter by
        /// ([`ClassifiedCategory::AnyCategory`] for any).
        category: ClassifiedCategory,
        /// The 0-based index of the first result to return (for paging).
        query_start: i32,
    },
    /// Autocomplete avatar names (`AvatarPickerRequest`): the name-picker lookup.
    /// The simulator answers with an
    /// [`Event::AvatarPickerReply`](crate::Event::AvatarPickerReply).
    AvatarPickerRequest {
        /// A client-chosen id echoed back in the reply.
        query_id: QueryId,
        /// The (partial) name to match.
        name: String,
    },
    /// Look up an agent's or group's land holdings (`PlacesQuery`): the land /
    /// group-land panels (distinct from the directory search). The simulator
    /// answers with a [`Event::PlacesReply`](crate::Event::PlacesReply).
    PlacesQuery {
        /// A client-chosen id echoed back in the reply.
        query_id: QueryId,
        /// A correlation id echoed back in the reply (the viewer reuses it to
        /// route the reply to the requesting panel).
        transaction_id: TransactionId,
        /// The search text (empty for all holdings).
        query_text: String,
        /// Result flags (the holdings-selection bits).
        flags: DirFindFlags,
        /// The parcel category to filter by.
        category: ParcelCategory,
        /// An optional region-name filter (empty for any region).
        sim_name: String,
    },
    /// Request the full detail of an in-world event (`EventInfoRequest`), by the
    /// `event_id` from a [`DirEventResult`](crate::DirEventResult) of an events
    /// [`DirFindQuery`](Self::DirFindQuery) (or the events directory). The
    /// simulator answers with an
    /// [`Event::EventInfoReply`](crate::Event::EventInfoReply).
    EventInfoRequest {
        /// The event to look up.
        event_id: EventId,
    },
    /// Subscribe to a reminder for an in-world event
    /// (`EventNotificationAddRequest`): the simulator will notify the agent as
    /// the event approaches. There is no direct reply.
    EventNotificationAddRequest {
        /// The event to be reminded about.
        event_id: EventId,
    },
    /// Cancel a previously-added event reminder
    /// (`EventNotificationRemoveRequest`). There is no direct reply.
    EventNotificationRemoveRequest {
        /// The event whose reminder to cancel.
        event_id: EventId,
    },
    /// Upload a new asset and create an inventory item for it over the modern
    /// `NewFileAgentInventory` capability (the two-step CAPS uploader): POST the
    /// metadata, then the raw bytes. The result arrives as
    /// [`Event::AssetUploaded`](crate::Event::AssetUploaded) (or [`Event::AssetUploadFailed`](crate::Event::AssetUploadFailed)).
    ///
    /// Both grids offer the capability and the modern viewer uploads exclusively
    /// over it, so the upload fails with
    /// [`Event::AssetUploadFailed`](crate::Event::AssetUploadFailed) when the
    /// capability is absent (the legacy UDP `AssetUploadRequest` fallback was
    /// dropped).
    ///
    /// For a mesh, `data` must be the **fully-formed mesh asset bytes** —
    /// uploading does not run the viewer's model-import pipeline (LOD / physics /
    /// cost generation).
    UploadAsset {
        /// The destination inventory folder.
        folder_id: InventoryFolderKey,
        /// The asset class (e.g. [`AssetType::Texture`], [`AssetType::Animation`]).
        asset_type: AssetType,
        /// The inventory-item class (e.g. [`InventoryType::Texture`],
        /// [`InventoryType::Wearable`]).
        inventory_type: InventoryType,
        /// The new item's name.
        name: String,
        /// The new item's description.
        description: String,
        /// The permission bits granted to the next owner.
        next_owner_mask: u32,
        /// The permission bits granted to the group.
        group_mask: u32,
        /// The permission bits granted to everyone.
        everyone_mask: u32,
        /// The L$ price the client expects to be charged (the grid rejects a
        /// mismatch; 0 on free grids such as OpenSim).
        expected_upload_cost: i32,
        /// The raw asset bytes.
        data: Vec<u8>,
    },
    /// Upload a client-computed baked avatar texture over the
    /// `UploadBakedTexture` capability (the legacy appearance path): stores a
    /// *temporary* asset with no inventory item. The result arrives as
    /// [`Event::AssetUploaded`](crate::Event::AssetUploaded) (with `new_inventory_item` = `None`) or
    /// [`Event::AssetUploadFailed`](crate::Event::AssetUploadFailed).
    UploadBakedTexture {
        /// The raw baked-texture bytes (a JPEG-2000 codestream).
        data: Vec<u8>,
    },
    /// Replace the asset of an existing inventory item over the matching
    /// `Update*AgentInventory` capability (gesture / notecard / script /
    /// settings, selected by `asset_type`). The result arrives as
    /// [`Event::AssetUploaded`](crate::Event::AssetUploaded) or [`Event::AssetUploadFailed`](crate::Event::AssetUploadFailed).
    UpdateInventoryAsset {
        /// The inventory item whose asset is being replaced.
        item_id: InventoryKey,
        /// The item's asset class, narrowed to the classes the generic
        /// `Update*AgentInventory` path serves (selects the capability via
        /// [`UpdatableAssetType::cap`]). Scripts are excluded by construction —
        /// use [`Command::UploadScript`] so compile results are surfaced.
        asset_type: UpdatableAssetType,
        /// The new raw asset bytes.
        data: Vec<u8>,
    },
    /// Upload new source into an existing script inventory item and have the
    /// **simulator compile it** (the `UpdateScriptAgent` / `UpdateScriptTask`
    /// capability, chosen by `location`). The viewer never compiles locally — it
    /// POSTs the raw source and a requested [`ScriptTarget`], and the simulator
    /// returns the compile result. That result arrives as
    /// [`Event::ScriptUploaded`](crate::Event::ScriptUploaded) (which carries the
    /// `compiled` flag and any [`ScriptCompileError`](crate::ScriptCompileError)s);
    /// a transport-level failure (missing capability, HTTP/parse error) arrives as
    /// [`Event::AssetUploadFailed`](crate::Event::AssetUploadFailed) instead.
    ///
    /// This is the *only* way to put script source on the wire (the generic
    /// [`UploadAsset`](Self::UploadAsset) / [`UpdateInventoryAsset`](Self::UpdateInventoryAsset)
    /// paths reject scripts) so compile results are never silently discarded. To
    /// create a *new* script item first, use
    /// [`Session::create_inventory_item`](crate::Session::create_inventory_item)
    /// (the simulator supplies a compilable default body).
    UploadScript {
        /// Which script item is being updated (agent inventory or an object's
        /// task inventory) — selects the capability and request body.
        location: ScriptUploadLocation,
        /// The compiler/runtime backend to compile for (`mono`/`lsl2`/`luau`).
        target: ScriptTarget,
        /// The raw script source bytes (UTF-8 text).
        source: Vec<u8>,
    },
    /// Fetch an object's per-face **media-on-a-prim** settings over the
    /// `ObjectMedia` capability (a GET). The result arrives as
    /// [`Event::ObjectMedia`](crate::Event::ObjectMedia).
    RequestObjectMedia {
        /// The object whose media to fetch.
        object_id: ObjectKey,
    },
    /// Set an object's per-face media over the `ObjectMedia` capability (an
    /// UPDATE). `faces` is one entry per prim face in order; a face with no media
    /// is `None`. The simulator advances the object's media version (visible on a
    /// subsequent [`Command::RequestObjectMedia`]) rather than replying.
    SetObjectMedia {
        /// The object whose media to set.
        object_id: ObjectKey,
        /// Per-face media, one slot per prim face in order (`None` = no media).
        faces: Vec<Option<MediaEntry>>,
    },
    /// Navigate the media on a single prim face to a new URL over the
    /// `ObjectMediaNavigate` capability. The simulator advances the object's
    /// media version (visible on a subsequent [`Command::RequestObjectMedia`]).
    NavigateObjectMedia {
        /// The object whose media to navigate.
        object_id: ObjectKey,
        /// The prim face (texture index) to navigate.
        face: u8,
        /// The URL to navigate that face's media to.
        url: String,
    },
    /// Fetch the legacy (normal/specular) materials for `material_ids` over the
    /// `RenderMaterials` capability (the OpenSim-supported path). The result
    /// arrives as [`Event::RenderMaterials`](crate::Event::RenderMaterials). The ids are the per-face
    /// `TextureEntry` material ids carried by scene objects.
    RequestRenderMaterials {
        /// The material ids to fetch.
        material_ids: Vec<Uuid>,
    },
    /// Set GLTF (PBR) materials on object faces over the `ModifyMaterialParams`
    /// capability. Each update applies an opaque `gltf_json` override and/or a
    /// stored material `asset_id` to one face (`side`, or `-1` for all). The
    /// `{ success, message }` reply arrives as [`Event::MaterialParamsResult`](crate::Event::MaterialParamsResult).
    ModifyMaterialParams {
        /// The per-face material assignments to apply.
        updates: Vec<MaterialOverrideUpdate>,
    },
    /// Request voice-chat account credentials over the
    /// `ProvisionVoiceAccountRequest` capability. A [`VoiceProvisionRequest::vivox`]
    /// asks for legacy Vivox SIP credentials; a [`VoiceProvisionRequest::webrtc`]
    /// negotiates a WebRTC session (the JSEP offer SDP is supplied by the
    /// caller's own — out-of-scope — WebRTC engine). The reply arrives as
    /// [`Event::VoiceAccountProvisioned`](crate::Event::VoiceAccountProvisioned). This handles the grid *signalling*
    /// only; the audio session itself is the caller's concern.
    RequestVoiceAccount {
        /// The provision request (backend selection + WebRTC offer/logout).
        request: VoiceProvisionRequest,
    },
    /// Request the current parcel's voice channel over the
    /// `ParcelVoiceInfoRequest` capability. The reply arrives as
    /// [`Event::ParcelVoiceInfo`](crate::Event::ParcelVoiceInfo).
    RequestParcelVoiceInfo,
    /// Trickle WebRTC ICE candidates (or signal end-of-gathering) over the
    /// `VoiceSignalingRequest` capability, keyed by the `viewer_session` from a
    /// prior [`Event::VoiceAccountProvisioned`](crate::Event::VoiceAccountProvisioned). Fire-and-forget — the simulator
    /// returns only an HTTP status. The candidates come from the caller's
    /// out-of-scope WebRTC engine.
    SendVoiceSignaling {
        /// The viewer session id from the provision reply.
        viewer_session: String,
        /// The ICE candidates to trickle (empty with `completed` to end).
        candidates: Vec<IceCandidate>,
        /// Whether this marks the end of ICE gathering.
        completed: bool,
    },
    /// Fetch experience metadata over the `GetExperienceInfo` capability, batching
    /// every id into one request. The reply arrives as [`Event::ExperienceInfo`](crate::Event::ExperienceInfo).
    RequestExperienceInfo {
        /// The experiences whose metadata to fetch.
        experience_ids: Vec<ExperienceKey>,
    },
    /// Search experiences by name over the `FindExperienceByName` capability. The
    /// reply (one page) arrives as [`Event::ExperienceSearchResults`](crate::Event::ExperienceSearchResults).
    FindExperiences {
        /// The search text.
        query: String,
        /// The zero-based result page.
        page: i32,
    },
    /// Fetch the agent's per-experience preferences over the `GetExperiences`
    /// capability. The reply arrives as [`Event::ExperiencePermissions`](crate::Event::ExperiencePermissions).
    RequestExperiencePermissions,
    /// Set (or forget) the agent's preference for one experience over the
    /// `ExperiencePreferences` capability (`Allow`/`Block` via PUT, `Forget` via
    /// DELETE). The updated lists arrive as [`Event::ExperiencePermissions`](crate::Event::ExperiencePermissions).
    SetExperiencePermission {
        /// The experience to set the preference for.
        experience_id: ExperienceKey,
        /// The preference to apply.
        permission: ExperiencePermission,
    },
    /// Fetch the experiences the agent owns over the `AgentExperiences`
    /// capability. The reply arrives as [`Event::OwnedExperiences`](crate::Event::OwnedExperiences).
    RequestOwnedExperiences,
    /// Fetch the experiences the agent administers over the `GetAdminExperiences`
    /// capability. The reply arrives as [`Event::AdminExperiences`](crate::Event::AdminExperiences).
    RequestAdminExperiences,
    /// Fetch the experiences the agent created over the `GetCreatorExperiences`
    /// capability. The reply arrives as [`Event::CreatorExperiences`](crate::Event::CreatorExperiences).
    RequestCreatorExperiences,
    /// Fetch the experiences a group owns over the `GroupExperiences` capability.
    /// The reply arrives as [`Event::GroupExperiences`](crate::Event::GroupExperiences).
    RequestGroupExperiences {
        /// The group to query.
        group_id: GroupKey,
    },
    /// Test whether the agent administers an experience over the
    /// `IsExperienceAdmin` capability. The reply arrives as
    /// [`Event::ExperienceAdminStatus`](crate::Event::ExperienceAdminStatus).
    RequestExperienceAdmin {
        /// The experience to test.
        experience_id: ExperienceKey,
    },
    /// Test whether the agent contributes to an experience over the
    /// `IsExperienceContributor` capability. The reply arrives as
    /// [`Event::ExperienceContributorStatus`](crate::Event::ExperienceContributorStatus).
    RequestExperienceContributor {
        /// The experience to test.
        experience_id: ExperienceKey,
    },
    /// Edit an experience's metadata over the `UpdateExperience` capability. The
    /// updated experience arrives as [`Event::ExperienceUpdated`](crate::Event::ExperienceUpdated).
    UpdateExperience {
        /// The editable experience metadata to write.
        update: ExperienceUpdate,
    },
    /// Read the region's experience allow/block/trust lists over the
    /// `RegionExperiences` capability. The reply arrives as
    /// [`Event::RegionExperiences`](crate::Event::RegionExperiences).
    RequestRegionExperiences,
    /// Replace the region's experience allow/block/trust lists over the
    /// `RegionExperiences` capability (estate-gated). The updated lists arrive as
    /// [`Event::RegionExperiences`](crate::Event::RegionExperiences).
    SetRegionExperiences {
        /// The experiences the region allows.
        allowed: Vec<ExperienceKey>,
        /// The experiences the region blocks.
        blocked: Vec<ExperienceKey>,
        /// The experiences the region trusts.
        trusted: Vec<ExperienceKey>,
    },
    /// Offer a teleport ("lure") to each `targets` agent (`StartLure`, #28). Each
    /// recipient receives an [`Event::InstantMessageReceived`](crate::Event::InstantMessageReceived) with
    /// [`ImDialog::LureUser`](crate::ImDialog::LureUser).
    OfferTeleport {
        /// The agents to invite.
        targets: Vec<AgentKey>,
        /// The accompanying message.
        message: String,
    },
    /// Accept a teleport lure (`TeleportLureRequest`), teleporting to the offer's
    /// location. `lure_id` is the [`InstantMessage::id`](crate::InstantMessage::id) of the received
    /// [`ImDialog::LureUser`](crate::ImDialog::LureUser) IM.
    AcceptTeleportLure {
        /// The lure id from the offer IM.
        lure_id: LureId,
    },
    /// Decline a teleport lure (`IM_LURE_DECLINED`).
    DeclineTeleportLure {
        /// The offer IM's sender.
        from_agent_id: AgentKey,
        /// The lure id from the offer IM.
        lure_id: LureId,
    },
    /// Request a teleport from `to_agent_id` (`IM_TELEPORT_REQUEST`): ask them to
    /// offer this agent a teleport.
    RequestTeleport {
        /// The agent to ask.
        to_agent_id: AgentKey,
        /// The accompanying message.
        message: String,
    },
    /// Offer an inventory item to `to_agent_id` over IM (`IM_INVENTORY_OFFERED`).
    GiveInventory {
        /// The recipient agent.
        to_agent_id: AgentKey,
        /// The offered item's id.
        item_id: InventoryKey,
        /// The offered item's asset class.
        asset_type: AssetType,
        /// The item's name (shown to the recipient).
        item_name: String,
        /// A fresh transaction id echoed back on accept/decline.
        transaction_id: TransactionId,
    },
    /// Offer an inventory folder to `to_agent_id` over IM (`IM_INVENTORY_OFFERED`).
    GiveInventoryFolder {
        /// The recipient agent.
        to_agent_id: AgentKey,
        /// The offered folder's id.
        folder_id: InventoryFolderKey,
        /// The folder's name (shown to the recipient).
        folder_name: String,
        /// A fresh transaction id echoed back on accept/decline.
        transaction_id: TransactionId,
    },
    /// Accept an inventory offer (`IM_INVENTORY_ACCEPTED`), filing it into
    /// `folder_id`. `offer` is decoded from the incoming
    /// [`InstantMessage::inventory_offer`](crate::InstantMessage::inventory_offer).
    AcceptInventoryOffer {
        /// The decoded inventory offer.
        offer: InventoryOffer,
        /// The destination folder to file the item into.
        folder_id: InventoryFolderKey,
    },
    /// Decline an inventory offer (`IM_INVENTORY_DECLINED`); the item is routed to
    /// `trash_folder_id`.
    DeclineInventoryOffer {
        /// The decoded inventory offer.
        offer: InventoryOffer,
        /// The trash folder the simulator routes the declined item to.
        trash_folder_id: InventoryFolderKey,
    },
    /// Start (or add invitees to) an ad-hoc conference IM session
    /// (`IM_SESSION_CONFERENCE_START`). Messages arrive as
    /// [`Event::ConferenceSessionMessage`](crate::Event::ConferenceSessionMessage).
    StartConference {
        /// A fresh, caller-chosen session id naming the conference.
        session_id: ImSessionId,
        /// The agents to invite.
        invitees: Vec<AgentKey>,
        /// The opening message.
        message: String,
    },
    /// Send a message into a conference / ad-hoc IM session (`IM_SESSION_SEND`).
    SendConferenceMessage {
        /// The conference session id.
        session_id: ImSessionId,
        /// The message text.
        message: String,
    },
    /// Leave a conference / ad-hoc IM session (`IM_SESSION_LEAVE`).
    LeaveConference {
        /// The conference session id.
        session_id: ImSessionId,
    },
    /// Mark a chat session as read, resetting its unread-message count to zero
    /// (the counterpart of the unread bump on each inbound message). A no-op if
    /// the session is not open.
    MarkSessionRead {
        /// Which chat session (1:1 direct, group, or conference) to mark read.
        session: ChatSessionKind,
    },
    /// Accept a pending chat-session invitation
    /// ([`Event::ConferenceInvited`](crate::Event::ConferenceInvited)), promoting
    /// its registry entry to joined. Where the `ChatSessionRequest` capability is
    /// present (Second Life) this POSTs `accept invitation` and the reply roster
    /// seeds the participants; on OpenSim the local optimistic join suffices.
    AcceptChatInvite {
        /// The invited session id (the group id for a group IM).
        session_id: ImSessionId,
        /// Whether the invitation is to a group IM (vs. an ad-hoc conference).
        from_group: bool,
    },
    /// Decline a pending chat-session invitation, removing its registry entry.
    /// Where the `ChatSessionRequest` capability is present this POSTs `decline
    /// invitation`; otherwise it sends a UDP `SessionLeave` as the fallback.
    DeclineChatInvite {
        /// The invited session id (the group id for a group IM).
        session_id: ImSessionId,
        /// Whether the invitation is to a group IM (vs. an ad-hoc conference).
        from_group: bool,
    },
    /// Join a chat session's voice channel (signalling only). Sets the session's
    /// optimistic `voice.joined` via
    /// [`Session::join_session_voice`](crate::Session::join_session_voice); the
    /// runtime then ensures a voice account
    /// ([`Command::RequestVoiceAccount`](crate::Command::RequestVoiceAccount)) and
    /// signals into the channel over the `ChatSessionRequest` capability. Models
    /// the grid *signalling* only — the audio session is the caller's concern.
    JoinSessionVoice {
        /// Which chat session (1:1 direct, group, or conference) to join voice on.
        session: ChatSessionKind,
    },
    /// Leave a chat session's voice channel (signalling only), clearing the
    /// session's optimistic `voice.joined` via
    /// [`Session::leave_session_voice`](crate::Session::leave_session_voice)
    /// without leaving the text conversation. The runtime then signals the voice
    /// decline / logout on the wire.
    LeaveSessionVoice {
        /// Which chat session (1:1 direct, group, or conference) to leave voice on.
        session: ChatSessionKind,
    },
    /// Query the light chat-session list (no history). The runtime replies with
    /// [`Event::ChatSessions`](crate::Event::ChatSessions) built from
    /// [`Session::chat_sessions_info`](crate::Session::chat_sessions_info); no wire
    /// send. A bevy reader may instead borrow the `Session` and call the builder
    /// directly.
    QueryChatSessions,
    /// Query one bounded, newest-first page of a chat session's history. The
    /// runtime replies with [`Event::ChatHistoryPage`](crate::Event::ChatHistoryPage)
    /// built from [`Session::history_page`](crate::Session::history_page); no wire
    /// send.
    QueryChatHistoryPage {
        /// Which chat session to page.
        session: ChatSessionKind,
        /// The page boundary: `None` for the newest page, or a `prev` cursor from
        /// an earlier reply to continue older.
        before: Option<MessageCursor>,
        /// The maximum number of messages in the page.
        limit: usize,
    },
    /// Query one bounded, newest-first page of **nearby (local) chat** history
    /// from the on-disk transcript. The runtime replies with
    /// [`Event::NearbyChatHistoryPage`](crate::Event::NearbyChatHistoryPage); no
    /// wire send.
    ///
    /// Nearby chat has no [`ChatSessionKind`] and no in-memory ring
    /// ([`NearbyHistoryLine`](crate::NearbyHistoryLine) explains why), so this is a
    /// separate query from [`QueryChatHistoryPage`](Self::QueryChatHistoryPage):
    /// the whole page comes from the transcript file the runtime is appending live
    /// chat to. `already_shown` is how many of the transcript's **newest** lines
    /// the caller is already displaying from the live
    /// [`ChatReceived`](crate::Event::ChatReceived) stream — those duplicate the
    /// file's tail and are skipped so recall only surfaces lines *older* than what
    /// is on screen.
    QueryNearbyChatHistoryPage {
        /// How many newest transcript lines to skip because the caller already
        /// shows them live (see above); `0` recalls from the very newest line.
        already_shown: usize,
        /// The page boundary: `None` for the newest (un-skipped) page, or a `prev`
        /// cursor from an earlier reply to continue older.
        before: Option<MessageCursor>,
        /// The maximum number of lines in the page.
        limit: usize,
    },
    /// Query the buddy cache with each friend's online flag. The runtime replies
    /// with [`Event::FriendsSnapshot`](crate::Event::FriendsSnapshot) built from
    /// [`Session::friends_presence`](crate::Session::friends_presence); no wire
    /// send.
    QueryFriends,
    /// Flush stored offline instant messages over the legacy UDP trigger
    /// (`RetrieveInstantMessages`); they arrive as offline
    /// [`Event::InstantMessageReceived`](crate::Event::InstantMessageReceived)s.
    RetrieveInstantMessages,
    /// Read stored offline instant messages over the modern `ReadOfflineMsgs`
    /// capability; they arrive as offline [`Event::InstantMessageReceived`](crate::Event::InstantMessageReceived)s.
    RequestOfflineMessages,
    /// Teleport to a landmark (`TeleportLandmarkRequest`). `None` teleports to
    /// the agent's home location.
    TeleportViaLandmark {
        /// The landmark inventory item's asset id, or `None` for home.
        landmark: Option<AssetKey>,
    },
    /// Cancel an in-progress teleport (`TeleportCancel`).
    CancelTeleport,
    /// Record a start location (`SetStartLocationRequest`): store `position` /
    /// `look_at` as the named slot. The everyday use is
    /// [`StartLocationSlot::Home`] ("set home to here").
    SetStartLocation {
        /// Which start-location slot to record.
        slot: StartLocationSlot,
        /// The region-local position to store.
        position: RegionCoordinates,
        /// The region-local look-at direction to store.
        look_at: Vector,
    },
    /// Poll for a fresh `AgentDataUpdate` without changing any agent data
    /// (`AgentDataUpdateRequest`).
    RequestAgentDataUpdate,
    /// Quit leaving the agent's in-world objects behind (`AgentQuitCopy`).
    QuitCopy,
    /// Toggle simulator-side velocity interpolation of object motion
    /// (`VelocityInterpolateOn` / `VelocityInterpolateOff`).
    SetVelocityInterpolation {
        /// Whether to enable (`true`) or disable (`false`) interpolation.
        enabled: bool,
    },
    /// Request the agent's own account contact preferences (`UserInfoRequest`);
    /// the reply arrives as [`Event::UserInfo`](crate::Event::UserInfo).
    RequestUserInfo,
    /// Update the agent's account contact preferences (`UpdateUserInfo`).
    UpdateUserInfo {
        /// Whether offline instant messages are forwarded to the agent's email.
        im_via_email: bool,
        /// The directory/search visibility setting.
        directory_visibility: DirectoryVisibility,
    },
    /// Trigger a one-shot spatial sound (`SoundTrigger`) at a region-local
    /// position.
    TriggerSound {
        /// The sound asset to play.
        sound: AssetKey,
        /// The linear gain (volume), `0.0`..=`1.0`.
        gain: f32,
        /// The handle of the region the sound plays in.
        region_handle: RegionHandle,
        /// The region-local position to play the sound at.
        position: RegionCoordinates,
    },
    /// Request that the simulator grant or drop god powers for this agent
    /// (`RequestGodlikePowers`; needs grid-god rights).
    RequestGodlikePowers {
        /// Whether to request (`true`) or relinquish (`false`) god powers.
        godlike: bool,
    },
    /// Eject an agent from the agent's land (`EjectUser`).
    EjectUser {
        /// The agent to eject.
        target: AgentKey,
        /// Whether to also ban the agent from the parcel.
        action: EjectAction,
    },
    /// Freeze or unfreeze an agent on the agent's land (`FreezeUser`).
    FreezeUser {
        /// The agent to freeze or unfreeze.
        target: AgentKey,
        /// Whether to freeze or unfreeze.
        action: FreezeAction,
    },
    /// Delete or return all objects an owner has across the region
    /// (`SimWideDeletes`; needs estate/god rights).
    SimWideDeletes {
        /// The owner whose objects to delete.
        owner: AgentKey,
        /// Which objects to target.
        flags: SimWideDeleteFlags,
    },
    /// Push god-tools region parameters (`GodUpdateRegionInfo`; needs grid-god
    /// rights).
    GodUpdateRegionInfo {
        /// The region parameters to apply.
        update: GodRegionUpdate,
    },
    /// Force-reassign the ownership of a parcel to `owner` via
    /// `ParcelGodForceOwner` (needs grid-god rights).
    ParcelGodForceOwner {
        /// The parcel to reassign.
        parcel: ScopedParcelId,
        /// The new owner.
        owner: OwnerKey,
    },
    /// Mark a parcel (and the content on it) as owned by the
    /// governor/maintenance account via `ParcelGodMarkAsContent` (needs
    /// grid-god rights).
    ParcelGodMarkAsContent {
        /// The parcel to mark as content.
        parcel: ScopedParcelId,
    },
    /// Delete an events-directory listing and re-run the search via
    /// `EventGodDelete` (needs grid-god rights). The `query_*` fields carry the
    /// current events search so the simulator can send back the refreshed
    /// result page.
    EventGodDelete {
        /// The events-directory listing to delete.
        event: EventId,
        /// The search-query id echoed back in the refreshed reply.
        query_id: QueryId,
        /// The search text to re-run after the deletion.
        query_text: String,
        /// The events-search flags/scope to re-run with.
        flags: DirFindFlags,
        /// The 0-based index of the first result of the refreshed page.
        query_start: i32,
    },
    /// Save the region (world) state via `StateSave` (needs grid-god rights).
    StateSave {
        /// The destination filename; an empty string lets the simulator pick
        /// the autosave name, exactly as the reference viewer does.
        filename: String,
    },
    /// Start a land auction on a parcel via `ViewerStartAuction` (needs
    /// grid-god rights).
    ViewerStartAuction {
        /// The parcel to auction.
        parcel: ScopedParcelId,
        /// An optional snapshot texture advertising the parcel (`None` for no
        /// snapshot).
        snapshot: Option<TextureKey>,
    },
    /// Begin a clean logout.
    Logout,
}
