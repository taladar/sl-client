//! Per-simulator circuit: reliable-UDP bookkeeping and outgoing message encoders.

use super::conversions::{
    OutgoingIm, compute_im_session_id, inventory_item_crc, pack_quaternion_to_vec3, with_nul,
};
use super::{
    ACK_FLUSH_DELAY, Circuit, INACTIVITY_TIMEOUT, MAX_ACKS_PER_PACKET, MAX_RESEND_ATTEMPTS,
    RESEND_TIMEOUT, SeenWindow, Timers, UnackedPacket, deadline,
};
use crate::AssetKey;
use crate::GroupRoleKey;
use crate::bookkeeping_ids::{InventoryCallbackId, PingId, XferId};
use crate::encode_texture_entry;
use crate::extra_params::extra_param_message_blocks;
use crate::scoped_id::CircuitId;
use crate::types::EventId;
use crate::types::directory::category_to_wire;
use crate::types::{
    AssetType, AttachmentMode, AttachmentPoint, Camera, ChatType, ClassifiedCategory,
    ClassifiedUpdate, ClickAction, CreateGroupParams, DeRezDestination, DetachOrder, DirFindFlags,
    GestureActivation, GodRegionUpdate, GroupRoleEdit, GroupRoleMemberChange, ImDialog,
    InterestsUpdate, InventoryItem, InventoryType, LandEdit, LandSearchType, MapRequestFlags,
    Material, MovementMode, NewInventoryItem, NewInventoryLink, NotecardRez, ObjectBuyItem,
    ObjectExtraParams, ObjectFlagSettings, ObjectTransform, ParcelAccessEntry, ParcelCategory,
    ParcelUpdate, PermissionField, PickKey, PickUpdate, Postcard, PrimShape, PrimShapeParams,
    ProfileUpdate, Reliability, RestoreItem, RezAttachment, RezObjectParams, RezScriptParams,
    SaleType, ScriptPermissions, StartLocationSlot, TaskInventoryKey, TeleportFlags, TextureEntry,
    Throttle, UpdateGroupInfoParams, ViewerEffect, Wearable,
};
use crate::types::{GroupNoticeKey, ProposalVoteId};
use sl_types::chat::ChatChannel;
use sl_types::key::{
    AgentKey, ClassifiedKey, FriendKey, GroupKey, InventoryFolderKey, InventoryKey, ObjectKey,
    ParcelKey, TextureKey,
};
use sl_types::lsl::{Rotation, Vector};
use sl_types::map::Distance;
use sl_types::money::LindenAmount;
use sl_wire::AbuseReport;
use sl_wire::Permissions;
use sl_wire::messages::{
    AcceptCallingCard, AcceptCallingCardAgentDataBlock, AcceptCallingCardFolderDataBlock,
    AcceptCallingCardTransactionBlockBlock, AcceptFriendship, AcceptFriendshipAgentDataBlock,
    AcceptFriendshipFolderDataBlock, AcceptFriendshipTransactionBlockBlock, ActivateGestures,
    ActivateGesturesAgentDataBlock, ActivateGesturesDataBlock, ActivateGroup,
    ActivateGroupAgentDataBlock, AgentAnimation, AgentAnimationAgentDataBlock,
    AgentAnimationAnimationListBlock, AgentAnimationPhysicalAvatarEventListBlock,
    AgentCachedTexture, AgentCachedTextureAgentDataBlock, AgentCachedTextureWearableDataBlock,
    AgentIsNowWearing, AgentIsNowWearingAgentDataBlock, AgentIsNowWearingWearableDataBlock,
    AgentRequestSit, AgentRequestSitAgentDataBlock, AgentRequestSitTargetObjectBlock,
    AgentSetAppearance, AgentSetAppearanceAgentDataBlock, AgentSetAppearanceObjectDataBlock,
    AgentSetAppearanceVisualParamBlock, AgentSetAppearanceWearableDataBlock, AgentSit,
    AgentSitAgentDataBlock, AgentThrottle, AgentThrottleAgentDataBlock, AgentThrottleThrottleBlock,
    AgentUpdate, AgentUpdateAgentDataBlock, AgentWearablesRequest,
    AgentWearablesRequestAgentDataBlock, AssetUploadRequest, AssetUploadRequestAssetBlockBlock,
    AvatarInterestsUpdate, AvatarInterestsUpdateAgentDataBlock,
    AvatarInterestsUpdatePropertiesDataBlock, AvatarNotesUpdate, AvatarNotesUpdateAgentDataBlock,
    AvatarNotesUpdateDataBlock, AvatarPickerRequest, AvatarPickerRequestAgentDataBlock,
    AvatarPickerRequestDataBlock, AvatarPropertiesRequest, AvatarPropertiesRequestAgentDataBlock,
    AvatarPropertiesUpdate, AvatarPropertiesUpdateAgentDataBlock,
    AvatarPropertiesUpdatePropertiesDataBlock, ChangeInventoryItemFlags,
    ChangeInventoryItemFlagsAgentDataBlock, ChangeInventoryItemFlagsInventoryDataBlock,
    ChatFromViewer, ChatFromViewerAgentDataBlock, ChatFromViewerChatDataBlock, ClassifiedDelete,
    ClassifiedDeleteAgentDataBlock, ClassifiedDeleteDataBlock, ClassifiedGodDelete,
    ClassifiedGodDeleteAgentDataBlock, ClassifiedGodDeleteDataBlock, ClassifiedInfoRequest,
    ClassifiedInfoRequestAgentDataBlock, ClassifiedInfoRequestDataBlock, ClassifiedInfoUpdate,
    ClassifiedInfoUpdateAgentDataBlock, ClassifiedInfoUpdateDataBlock, CompleteAgentMovement,
    CompleteAgentMovementAgentDataBlock, CompletePingCheck, CompletePingCheckPingIDBlock,
    ConfirmXferPacket, ConfirmXferPacketXferIDBlock, CopyInventoryItem,
    CopyInventoryItemAgentDataBlock, CopyInventoryItemInventoryDataBlock, CreateGroupRequest,
    CreateGroupRequestAgentDataBlock, CreateGroupRequestGroupDataBlock, CreateInventoryFolder,
    CreateInventoryFolderAgentDataBlock, CreateInventoryFolderFolderDataBlock, CreateInventoryItem,
    CreateInventoryItemAgentDataBlock, CreateInventoryItemInventoryBlockBlock, DeRezObject,
    DeRezObjectAgentBlockBlock, DeRezObjectAgentDataBlock, DeRezObjectObjectDataBlock,
    DeactivateGestures, DeactivateGesturesAgentDataBlock, DeactivateGesturesDataBlock,
    DeclineCallingCard, DeclineCallingCardAgentDataBlock, DeclineCallingCardTransactionBlockBlock,
    DeclineFriendship, DeclineFriendshipAgentDataBlock, DeclineFriendshipTransactionBlockBlock,
    DirClassifiedQuery, DirClassifiedQueryAgentDataBlock, DirClassifiedQueryQueryDataBlock,
    DirFindQuery, DirFindQueryAgentDataBlock, DirFindQueryQueryDataBlock, DirLandQuery,
    DirLandQueryAgentDataBlock, DirLandQueryQueryDataBlock, DirPlacesQuery,
    DirPlacesQueryAgentDataBlock, DirPlacesQueryQueryDataBlock, EconomyDataRequest,
    EjectGroupMemberRequest, EjectGroupMemberRequestAgentDataBlock,
    EjectGroupMemberRequestEjectDataBlock, EjectGroupMemberRequestGroupDataBlock,
    EstateCovenantRequest, EstateCovenantRequestAgentDataBlock, EstateOwnerMessage,
    EstateOwnerMessageAgentDataBlock, EstateOwnerMessageMethodDataBlock,
    EstateOwnerMessageParamListBlock, EventInfoRequest, EventInfoRequestAgentDataBlock,
    EventInfoRequestEventDataBlock, EventNotificationAddRequest,
    EventNotificationAddRequestAgentDataBlock, EventNotificationAddRequestEventDataBlock,
    EventNotificationRemoveRequest, EventNotificationRemoveRequestAgentDataBlock,
    EventNotificationRemoveRequestEventDataBlock, FetchInventoryDescendents,
    FetchInventoryDescendentsAgentDataBlock, FetchInventoryDescendentsInventoryDataBlock,
    FindAgent, FindAgentAgentBlockBlock, GenericMessage, GenericMessageAgentDataBlock,
    GenericMessageMethodDataBlock, GenericMessageParamListBlock, GodKickUser,
    GodKickUserUserInfoBlock, GodlikeMessage, GodlikeMessageAgentDataBlock,
    GodlikeMessageMethodDataBlock, GodlikeMessageParamListBlock, GrantUserRights,
    GrantUserRightsAgentDataBlock, GrantUserRightsRightsBlock, GroupMembersRequest,
    GroupMembersRequestAgentDataBlock, GroupMembersRequestGroupDataBlock, GroupNoticeRequest,
    GroupNoticeRequestAgentDataBlock, GroupNoticeRequestDataBlock, GroupNoticesListRequest,
    GroupNoticesListRequestAgentDataBlock, GroupNoticesListRequestDataBlock, GroupProfileRequest,
    GroupProfileRequestAgentDataBlock, GroupProfileRequestGroupDataBlock, GroupRoleChanges,
    GroupRoleChangesAgentDataBlock, GroupRoleChangesRoleChangeBlock, GroupRoleDataRequest,
    GroupRoleDataRequestAgentDataBlock, GroupRoleDataRequestGroupDataBlock,
    GroupRoleMembersRequest, GroupRoleMembersRequestAgentDataBlock,
    GroupRoleMembersRequestGroupDataBlock, GroupRoleUpdate, GroupRoleUpdateAgentDataBlock,
    GroupRoleUpdateRoleDataBlock, GroupTitleUpdate, GroupTitleUpdateAgentDataBlock,
    GroupTitlesRequest, GroupTitlesRequestAgentDataBlock, ImprovedInstantMessage,
    ImprovedInstantMessageAgentDataBlock, ImprovedInstantMessageEstateBlockBlock,
    ImprovedInstantMessageMessageBlockBlock, InviteGroupRequest, InviteGroupRequestAgentDataBlock,
    InviteGroupRequestGroupDataBlock, InviteGroupRequestInviteDataBlock, JoinGroupRequest,
    JoinGroupRequestAgentDataBlock, JoinGroupRequestGroupDataBlock, LandStatRequest,
    LandStatRequestAgentDataBlock, LandStatRequestRequestDataBlock, LeaveGroupRequest,
    LeaveGroupRequestAgentDataBlock, LeaveGroupRequestGroupDataBlock, LinkInventoryItem,
    LinkInventoryItemAgentDataBlock, LinkInventoryItemInventoryBlockBlock, LogoutRequest,
    LogoutRequestAgentDataBlock, MapBlockRequest, MapBlockRequestAgentDataBlock,
    MapBlockRequestPositionDataBlock, MapItemRequest, MapItemRequestAgentDataBlock,
    MapItemRequestRequestDataBlock, MapLayerRequest, MapLayerRequestAgentDataBlock, MapNameRequest,
    MapNameRequestAgentDataBlock, MapNameRequestNameDataBlock, MoneyBalanceRequest,
    MoneyBalanceRequestAgentDataBlock, MoneyBalanceRequestMoneyDataBlock, MoneyTransferRequest,
    MoneyTransferRequestAgentDataBlock, MoneyTransferRequestMoneyDataBlock, MoveInventoryFolder,
    MoveInventoryFolderAgentDataBlock, MoveInventoryFolderInventoryDataBlock, MoveInventoryItem,
    MoveInventoryItemAgentDataBlock, MoveInventoryItemInventoryDataBlock, MultipleObjectUpdate,
    MultipleObjectUpdateAgentDataBlock, MultipleObjectUpdateObjectDataBlock, MuteListRequest,
    MuteListRequestAgentDataBlock, MuteListRequestMuteDataBlock, ObjectAdd,
    ObjectAddAgentDataBlock, ObjectAddObjectDataBlock, ObjectAttach, ObjectAttachAgentDataBlock,
    ObjectAttachObjectDataBlock, ObjectCategory, ObjectCategoryAgentDataBlock,
    ObjectCategoryObjectDataBlock, ObjectClickAction, ObjectClickActionAgentDataBlock,
    ObjectClickActionObjectDataBlock, ObjectDeGrab, ObjectDeGrabAgentDataBlock,
    ObjectDeGrabObjectDataBlock, ObjectDelete, ObjectDeleteAgentDataBlock,
    ObjectDeleteObjectDataBlock, ObjectDelink, ObjectDelinkAgentDataBlock,
    ObjectDelinkObjectDataBlock, ObjectDescription, ObjectDescriptionAgentDataBlock,
    ObjectDescriptionObjectDataBlock, ObjectDeselect, ObjectDeselectAgentDataBlock,
    ObjectDeselectObjectDataBlock, ObjectDetach, ObjectDetachAgentDataBlock,
    ObjectDetachObjectDataBlock, ObjectDrop, ObjectDropAgentDataBlock, ObjectDropObjectDataBlock,
    ObjectDuplicate, ObjectDuplicateAgentDataBlock, ObjectDuplicateObjectDataBlock,
    ObjectDuplicateSharedDataBlock, ObjectExtraParams as ObjectExtraParamsMessage,
    ObjectExtraParamsAgentDataBlock, ObjectExtraParamsObjectDataBlock, ObjectFlagUpdate,
    ObjectFlagUpdateAgentDataBlock, ObjectGrab, ObjectGrabAgentDataBlock,
    ObjectGrabObjectDataBlock, ObjectGrabUpdate, ObjectGrabUpdateAgentDataBlock,
    ObjectGrabUpdateObjectDataBlock, ObjectGroup, ObjectGroupAgentDataBlock,
    ObjectGroupObjectDataBlock, ObjectImage, ObjectImageAgentDataBlock, ObjectImageObjectDataBlock,
    ObjectIncludeInSearch, ObjectIncludeInSearchAgentDataBlock,
    ObjectIncludeInSearchObjectDataBlock, ObjectLink, ObjectLinkAgentDataBlock,
    ObjectLinkObjectDataBlock, ObjectMaterial, ObjectMaterialAgentDataBlock,
    ObjectMaterialObjectDataBlock, ObjectName, ObjectNameAgentDataBlock, ObjectNameObjectDataBlock,
    ObjectPermissions, ObjectPermissionsAgentDataBlock, ObjectPermissionsHeaderDataBlock,
    ObjectPermissionsObjectDataBlock, ObjectSaleInfo, ObjectSaleInfoAgentDataBlock,
    ObjectSaleInfoObjectDataBlock, ObjectSelect, ObjectSelectAgentDataBlock,
    ObjectSelectObjectDataBlock, ObjectShape, ObjectShapeAgentDataBlock,
    ObjectShapeObjectDataBlock, OfferCallingCard, OfferCallingCardAgentBlockBlock,
    OfferCallingCardAgentDataBlock, PacketAck, PacketAckPacketsBlock, ParcelAccessListRequest,
    ParcelAccessListRequestAgentDataBlock, ParcelAccessListRequestDataBlock,
    ParcelAccessListUpdate, ParcelAccessListUpdateAgentDataBlock, ParcelAccessListUpdateDataBlock,
    ParcelAccessListUpdateListBlock, ParcelBuy, ParcelBuyAgentDataBlock, ParcelBuyDataBlock,
    ParcelBuyParcelDataBlock, ParcelBuyPass, ParcelBuyPassAgentDataBlock,
    ParcelBuyPassParcelDataBlock, ParcelDeedToGroup, ParcelDeedToGroupAgentDataBlock,
    ParcelDeedToGroupDataBlock, ParcelDisableObjects, ParcelDisableObjectsAgentDataBlock,
    ParcelDisableObjectsOwnerIDsBlock, ParcelDisableObjectsParcelDataBlock,
    ParcelDisableObjectsTaskIDsBlock, ParcelDivide, ParcelDivideAgentDataBlock,
    ParcelDivideParcelDataBlock, ParcelDwellRequest, ParcelDwellRequestAgentDataBlock,
    ParcelDwellRequestDataBlock, ParcelInfoRequest, ParcelInfoRequestAgentDataBlock,
    ParcelInfoRequestDataBlock, ParcelJoin, ParcelJoinAgentDataBlock, ParcelJoinParcelDataBlock,
    ParcelObjectOwnersRequest, ParcelObjectOwnersRequestAgentDataBlock,
    ParcelObjectOwnersRequestParcelDataBlock, ParcelPropertiesRequest,
    ParcelPropertiesRequestAgentDataBlock, ParcelPropertiesRequestParcelDataBlock,
    ParcelPropertiesUpdate, ParcelPropertiesUpdateAgentDataBlock,
    ParcelPropertiesUpdateParcelDataBlock, ParcelReclaim, ParcelReclaimAgentDataBlock,
    ParcelReclaimDataBlock, ParcelRelease, ParcelReleaseAgentDataBlock, ParcelReleaseDataBlock,
    ParcelReturnObjects, ParcelReturnObjectsAgentDataBlock, ParcelReturnObjectsOwnerIDsBlock,
    ParcelReturnObjectsParcelDataBlock, ParcelReturnObjectsTaskIDsBlock, ParcelSelectObjects,
    ParcelSelectObjectsAgentDataBlock, ParcelSelectObjectsParcelDataBlock,
    ParcelSelectObjectsReturnIDsBlock, PickDelete, PickDeleteAgentDataBlock, PickDeleteDataBlock,
    PickGodDelete, PickGodDeleteAgentDataBlock, PickGodDeleteDataBlock, PickInfoUpdate,
    PickInfoUpdateAgentDataBlock, PickInfoUpdateDataBlock, PlacesQuery, PlacesQueryAgentDataBlock,
    PlacesQueryQueryDataBlock, PlacesQueryTransactionDataBlock, PurgeInventoryDescendents,
    PurgeInventoryDescendentsAgentDataBlock, PurgeInventoryDescendentsInventoryDataBlock,
    RegionHandshakeReply, RegionHandshakeReplyAgentDataBlock, RegionHandshakeReplyRegionInfoBlock,
    RemoveAttachment, RemoveAttachmentAgentDataBlock, RemoveAttachmentAttachmentBlockBlock,
    RemoveInventoryFolder, RemoveInventoryFolderAgentDataBlock,
    RemoveInventoryFolderFolderDataBlock, RemoveInventoryItem, RemoveInventoryItemAgentDataBlock,
    RemoveInventoryItemInventoryDataBlock, RemoveInventoryObjects,
    RemoveInventoryObjectsAgentDataBlock, RemoveInventoryObjectsFolderDataBlock,
    RemoveInventoryObjectsItemDataBlock, RemoveMuteListEntry, RemoveMuteListEntryAgentDataBlock,
    RemoveMuteListEntryMuteDataBlock, RequestImage, RequestImageAgentDataBlock,
    RequestImageRequestImageBlock, RequestMultipleObjects, RequestMultipleObjectsAgentDataBlock,
    RequestMultipleObjectsObjectDataBlock, RequestRegionInfo, RequestRegionInfoAgentDataBlock,
    RequestXfer, RequestXferXferIDBlock, RetrieveInstantMessages,
    RetrieveInstantMessagesAgentDataBlock, RezMultipleAttachmentsFromInv,
    RezMultipleAttachmentsFromInvAgentDataBlock, RezMultipleAttachmentsFromInvHeaderDataBlock,
    RezMultipleAttachmentsFromInvObjectDataBlock, RezSingleAttachmentFromInv,
    RezSingleAttachmentFromInvAgentDataBlock, RezSingleAttachmentFromInvObjectDataBlock,
    ScriptAnswerYes, ScriptAnswerYesAgentDataBlock, ScriptAnswerYesDataBlock, ScriptDialogReply,
    ScriptDialogReplyAgentDataBlock, ScriptDialogReplyDataBlock, SendPostcard,
    SendPostcardAgentDataBlock, SendXferPacket, SendXferPacketDataPacketBlock,
    SendXferPacketXferIDBlock, SetGroupAcceptNotices, SetGroupAcceptNoticesAgentDataBlock,
    SetGroupAcceptNoticesDataBlock, SetGroupAcceptNoticesNewDataBlock, SetGroupContribution,
    SetGroupContributionAgentDataBlock, SetGroupContributionDataBlock, StartLure,
    StartLureAgentDataBlock, StartLureInfoBlock, StartLureTargetDataBlock, StartPingCheck,
    StartPingCheckPingIDBlock, TeleportLocationRequest, TeleportLocationRequestAgentDataBlock,
    TeleportLocationRequestInfoBlock, TeleportLureRequest, TeleportLureRequestInfoBlock,
    TerminateFriendship, TerminateFriendshipAgentDataBlock, TerminateFriendshipExBlockBlock,
    TrackAgent, TrackAgentAgentDataBlock, TrackAgentTargetDataBlock, UUIDGroupNameRequest,
    UUIDGroupNameRequestUUIDNameBlockBlock, UUIDNameRequest, UUIDNameRequestUUIDNameBlockBlock,
    UpdateGroupInfo, UpdateGroupInfoAgentDataBlock, UpdateGroupInfoGroupDataBlock,
    UpdateInventoryFolder, UpdateInventoryFolderAgentDataBlock,
    UpdateInventoryFolderFolderDataBlock, UpdateInventoryItem, UpdateInventoryItemAgentDataBlock,
    UpdateInventoryItemInventoryDataBlock, UpdateMuteListEntry, UpdateMuteListEntryAgentDataBlock,
    UpdateMuteListEntryMuteDataBlock, UseCircuitCode, UseCircuitCodeCircuitCodeBlock, UserReport,
    UserReportAgentDataBlock, UserReportReportDataBlock, ViewerEffect as ViewerEffectMessage,
    ViewerEffectAgentDataBlock, ViewerEffectEffectBlock,
};
use sl_wire::messages::{
    AgentDataUpdateRequest, AgentDataUpdateRequestAgentDataBlock, AgentQuitCopy,
    AgentQuitCopyAgentDataBlock, AgentQuitCopyFuseBlockBlock, SetStartLocationRequest,
    SetStartLocationRequestAgentDataBlock, SetStartLocationRequestStartLocationDataBlock,
    TeleportCancel, TeleportCancelInfoBlock, TeleportLandmarkRequest,
    TeleportLandmarkRequestInfoBlock, VelocityInterpolateOff, VelocityInterpolateOffAgentDataBlock,
    VelocityInterpolateOn, VelocityInterpolateOnAgentDataBlock,
};
use sl_wire::messages::{
    AgentFOV, AgentFOVAgentDataBlock, AgentFOVFOVBlockBlock, AgentHeightWidth,
    AgentHeightWidthAgentDataBlock, AgentHeightWidthHeightWidthBlockBlock, AgentPause,
    AgentPauseAgentDataBlock, AgentResume, AgentResumeAgentDataBlock, ForceScriptControlRelease,
    ForceScriptControlReleaseAgentDataBlock, SetAlwaysRun, SetAlwaysRunAgentDataBlock,
};
use sl_wire::messages::{
    BuyObjectInventory, BuyObjectInventoryAgentDataBlock, BuyObjectInventoryDataBlock, ObjectBuy,
    ObjectBuyAgentDataBlock, ObjectBuyObjectDataBlock, ObjectDuplicateOnRay,
    ObjectDuplicateOnRayAgentDataBlock, ObjectDuplicateOnRayObjectDataBlock, ObjectSpinStart,
    ObjectSpinStartAgentDataBlock, ObjectSpinStartObjectDataBlock, ObjectSpinStop,
    ObjectSpinStopAgentDataBlock, ObjectSpinStopObjectDataBlock, ObjectSpinUpdate,
    ObjectSpinUpdateAgentDataBlock, ObjectSpinUpdateObjectDataBlock, RequestObjectPropertiesFamily,
    RequestObjectPropertiesFamilyAgentDataBlock, RequestObjectPropertiesFamilyObjectDataBlock,
    RequestPayPrice, RequestPayPriceObjectDataBlock, RezObjectFromNotecard,
    RezObjectFromNotecardAgentDataBlock, RezObjectFromNotecardInventoryDataBlock,
    RezObjectFromNotecardNotecardDataBlock, RezObjectFromNotecardRezDataBlock, RezRestoreToWorld,
    RezRestoreToWorldAgentDataBlock, RezRestoreToWorldInventoryDataBlock,
};
use sl_wire::messages::{
    DetachAttachmentIntoInv, DetachAttachmentIntoInvObjectDataBlock, RevokePermissions,
    RevokePermissionsAgentDataBlock, RevokePermissionsDataBlock, RezObject,
    RezObjectAgentDataBlock, RezObjectInventoryDataBlock, RezObjectRezDataBlock, RezScript,
    RezScriptAgentDataBlock, RezScriptInventoryBlockBlock, RezScriptUpdateBlockBlock,
};
use sl_wire::messages::{
    EjectUser, EjectUserAgentDataBlock, EjectUserDataBlock, FreezeUser, FreezeUserAgentDataBlock,
    FreezeUserDataBlock, GodUpdateRegionInfo, GodUpdateRegionInfoAgentDataBlock,
    GodUpdateRegionInfoRegionInfo2Block, GodUpdateRegionInfoRegionInfoBlock, RequestGodlikePowers,
    RequestGodlikePowersAgentDataBlock, RequestGodlikePowersRequestBlockBlock, SimWideDeletes,
    SimWideDeletesAgentDataBlock, SimWideDeletesDataBlockBlock,
};
use sl_wire::messages::{
    EventGodDelete, EventGodDeleteAgentDataBlock, EventGodDeleteEventDataBlock,
    EventGodDeleteQueryDataBlock, ParcelGodForceOwner, ParcelGodForceOwnerAgentDataBlock,
    ParcelGodForceOwnerDataBlock, ParcelGodMarkAsContent, ParcelGodMarkAsContentAgentDataBlock,
    ParcelGodMarkAsContentParcelDataBlock, StateSave, StateSaveAgentDataBlock,
    StateSaveDataBlockBlock, ViewerStartAuction, ViewerStartAuctionAgentDataBlock,
    ViewerStartAuctionParcelDataBlock,
};
use sl_wire::messages::{
    GetScriptRunning, GetScriptRunningScriptBlock, ScriptReset, ScriptResetAgentDataBlock,
    ScriptResetScriptBlock, SetScriptRunning, SetScriptRunningAgentDataBlock,
    SetScriptRunningScriptBlock,
};
use sl_wire::messages::{
    GroupAccountDetailsRequest, GroupAccountDetailsRequestAgentDataBlock,
    GroupAccountDetailsRequestMoneyDataBlock, GroupAccountSummaryRequest,
    GroupAccountSummaryRequestAgentDataBlock, GroupAccountSummaryRequestMoneyDataBlock,
    GroupAccountTransactionsRequest, GroupAccountTransactionsRequestAgentDataBlock,
    GroupAccountTransactionsRequestMoneyDataBlock, GroupActiveProposalsRequest,
    GroupActiveProposalsRequestAgentDataBlock, GroupActiveProposalsRequestGroupDataBlock,
    GroupActiveProposalsRequestTransactionDataBlock, GroupProposalBallot,
    GroupProposalBallotAgentDataBlock, GroupProposalBallotProposalDataBlock,
    GroupVoteHistoryRequest, GroupVoteHistoryRequestAgentDataBlock,
    GroupVoteHistoryRequestGroupDataBlock, GroupVoteHistoryRequestTransactionDataBlock,
    StartGroupProposal, StartGroupProposalAgentDataBlock, StartGroupProposalProposalDataBlock,
};
use sl_wire::messages::{
    ModifyLand, ModifyLandAgentDataBlock, ModifyLandModifyBlockBlock,
    ModifyLandModifyBlockExtendedBlock, ModifyLandParcelDataBlock, ParcelPropertiesRequestByID,
    ParcelPropertiesRequestByIDAgentDataBlock, ParcelPropertiesRequestByIDParcelDataBlock,
    ParcelSetOtherCleanTime, ParcelSetOtherCleanTimeAgentDataBlock,
    ParcelSetOtherCleanTimeParcelDataBlock, UndoLand, UndoLandAgentDataBlock,
};
use sl_wire::messages::{
    MoveTaskInventory, MoveTaskInventoryAgentDataBlock, MoveTaskInventoryInventoryDataBlock,
    RemoveTaskInventory, RemoveTaskInventoryAgentDataBlock, RemoveTaskInventoryInventoryDataBlock,
    RequestTaskInventory, RequestTaskInventoryAgentDataBlock,
    RequestTaskInventoryInventoryDataBlock, UpdateTaskInventory, UpdateTaskInventoryAgentDataBlock,
    UpdateTaskInventoryInventoryDataBlock, UpdateTaskInventoryUpdateDataBlock,
};
use sl_wire::messages::{
    SoundTrigger, SoundTriggerSoundDataBlock, UpdateUserInfo, UpdateUserInfoAgentDataBlock,
    UpdateUserInfoUserDataBlock, UserInfoRequest, UserInfoRequestAgentDataBlock,
};
use sl_wire::{
    AnyMessage, CircuitCode, PacketFlags, RegionLocalObjectId, RegionLocalParcelId, SequenceNumber,
    WireError, Writer, encode_datagram,
};
use std::collections::{BTreeMap, VecDeque};
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use uuid::Uuid;

/// Converts a draw-distance [`Distance`] to the `f32` the `AgentUpdate` `Far`
/// wire field carries. The conversion is the codec boundary for draw distance:
/// a `Distance` is `f64`-backed but the wire field is an `F32`.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "the AgentUpdate Far field is an F32; draw distance is a small, in-range metre value"
)]
const fn far_from_distance(distance: &Distance) -> f32 {
    distance.meters() as f32
}

impl Circuit {
    /// Creates a circuit and arms the inactivity timer. `id` is the freshly
    /// minted [`CircuitId`] for this circuit instance, used to scope the
    /// region-local ids of the objects it streams.
    pub(crate) fn new(
        id: CircuitId,
        sim_addr: SocketAddr,
        agent_id: AgentKey,
        session_id: Uuid,
        circuit_code: CircuitCode,
        draw_distance: Distance,
        now: Instant,
    ) -> Self {
        Self {
            id,
            sim_addr,
            agent_id,
            session_id,
            code: circuit_code,
            next_sequence: SequenceNumber::FIRST,
            pause_serial_num: 0,
            next_ping_id: PingId::default(),
            outstanding_ping: None,
            pending_acks: Vec::new(),
            unacked: BTreeMap::new(),
            seen: SeenWindow::default(),
            out: VecDeque::new(),
            draw_distance,
            timers: Timers {
                inactivity: deadline(now, INACTIVITY_TIMEOUT),
                ack_flush: None,
                agent_update: None,
                ping: None,
                logout: None,
                teleport: None,
                sit: None,
            },
        }
    }

    /// Re-points the circuit at a new simulator after a teleport, resetting the
    /// per-circuit sequence/ack/seen/timer state while keeping the agent
    /// identity and circuit code (both reused across regions).
    pub(crate) fn retarget(&mut self, sim_addr: SocketAddr, now: Instant) {
        self.sim_addr = sim_addr;
        self.next_sequence = SequenceNumber::FIRST;
        self.next_ping_id = PingId::default();
        self.outstanding_ping = None;
        self.pending_acks.clear();
        self.unacked.clear();
        self.seen = SeenWindow::default();
        self.out.clear();
        self.timers = Timers {
            inactivity: deadline(now, INACTIVITY_TIMEOUT),
            ack_flush: None,
            agent_update: None,
            ping: None,
            logout: None,
            teleport: None,
            sit: None,
        };
    }

    /// Allocates the next outgoing sequence number.
    pub(crate) const fn next_sequence(&mut self) -> SequenceNumber {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.wrapping_next();
        sequence
    }

    /// Encodes and queues a message, tracking it for resend when reliable.
    pub(crate) fn send(
        &mut self,
        message: &AnyMessage,
        reliability: Reliability,
        now: Instant,
    ) -> Result<(), WireError> {
        let mut writer = Writer::new();
        message.id().encode(&mut writer);
        message.encode_body(&mut writer)?;
        let body = writer.into_bytes();

        let sequence = self.next_sequence();
        let flags = match reliability {
            Reliability::Reliable => PacketFlags::RELIABLE,
            Reliability::Unreliable => PacketFlags::EMPTY,
        };
        let datagram = encode_datagram(flags, sequence, &body);

        if matches!(reliability, Reliability::Reliable) {
            self.unacked.insert(
                sequence,
                UnackedPacket {
                    datagram: datagram.clone(),
                    sent_at: now,
                    attempts: 1,
                    name: sl_wire::message_name(message.id()),
                },
            );
        }
        self.out.push_back(datagram);
        Ok(())
    }

    /// Queues `UseCircuitCode` reliably.
    pub(crate) fn send_use_circuit_code(&mut self, now: Instant) -> Result<(), WireError> {
        let message = AnyMessage::UseCircuitCode(UseCircuitCode {
            circuit_code: UseCircuitCodeCircuitCodeBlock {
                code: self.code.get(),
                session_id: self.session_id,
                id: self.agent_id.uuid(),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `AgentThrottle` reliably, telling the simulator how to allocate
    /// its UDP send bandwidth across the seven traffic categories. The seven
    /// per-category rates are packed as little-endian `f32` bits-per-second
    /// values (the `Throttle` wire encoding); `GenCounter` is left at zero, as
    /// the reference viewer does (the simulator does not order by it).
    pub(crate) fn send_agent_throttle(
        &mut self,
        throttle: &Throttle,
        now: Instant,
    ) -> Result<(), WireError> {
        let mut writer = Writer::new();
        for rate in throttle.bits_per_second() {
            writer.put_f32(rate);
        }
        let message = AnyMessage::AgentThrottle(AgentThrottle {
            agent_data: AgentThrottleAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
                circuit_code: self.code.get(),
            },
            throttle: AgentThrottleThrottleBlock {
                gen_counter: 0,
                throttles: writer.into_bytes(),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues `CompleteAgentMovement` reliably.
    pub(crate) fn send_complete_agent_movement(&mut self, now: Instant) -> Result<(), WireError> {
        let message = AnyMessage::CompleteAgentMovement(CompleteAgentMovement {
            agent_data: CompleteAgentMovementAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
                circuit_code: self.code.get(),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues `RegionHandshakeReply` reliably.
    pub(crate) fn send_region_handshake_reply(&mut self, now: Instant) -> Result<(), WireError> {
        let message = AnyMessage::RegionHandshakeReply(RegionHandshakeReply {
            agent_data: RegionHandshakeReplyAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            region_info: RegionHandshakeReplyRegionInfoBlock { flags: 0 },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `CompletePingCheck` reply unreliably.
    pub(crate) fn send_complete_ping_check(
        &mut self,
        ping_id: PingId,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::CompletePingCheck(CompletePingCheck {
            ping_id: CompletePingCheckPingIDBlock {
                ping_id: ping_id.get(),
            },
        });
        self.send(&message, Reliability::Unreliable, now)
    }

    /// Queues a keep-alive `StartPingCheck` unreliably, recording it as the
    /// outstanding ping so the matching `CompletePingCheck` can be timed.
    ///
    /// Like the reference viewer, the ping carries the lowest unacked outgoing
    /// sequence number in `OldestUnacked`, letting the simulator drop its own
    /// record of anything older. Returns the ping id sent.
    pub(crate) fn send_start_ping_check(&mut self, now: Instant) -> Result<PingId, WireError> {
        let ping_id = self.next_ping_id;
        self.next_ping_id = self.next_ping_id.wrapping_next();
        let oldest_unacked = self
            .unacked
            .keys()
            .next()
            .copied()
            .map_or(0, SequenceNumber::get);
        let message = AnyMessage::StartPingCheck(StartPingCheck {
            ping_id: StartPingCheckPingIDBlock {
                ping_id: ping_id.get(),
                oldest_unacked,
            },
        });
        self.send(&message, Reliability::Unreliable, now)?;
        self.outstanding_ping = Some((ping_id, now));
        Ok(ping_id)
    }

    /// Records an inbound `CompletePingCheck`, returning the round-trip time when
    /// it answers the outstanding keep-alive ping.
    ///
    /// Returns `None` for an unsolicited reply or one whose id does not match the
    /// ping in flight (a stale or duplicate echo), leaving any genuine
    /// outstanding ping untouched.
    pub(crate) fn record_ping_reply(&mut self, ping_id: PingId, now: Instant) -> Option<Duration> {
        match self.outstanding_ping {
            Some((outstanding, sent_at)) if outstanding == ping_id => {
                self.outstanding_ping = None;
                Some(now.saturating_duration_since(sent_at))
            }
            _ => None,
        }
    }

    /// Queues a `ChatFromViewer` reliably, sending local chat. The wire string
    /// carries a trailing NUL, as a real viewer sends.
    pub(crate) fn send_chat_from_viewer(
        &mut self,
        message: &str,
        chat_type: ChatType,
        channel: ChatChannel,
        now: Instant,
    ) -> Result<(), WireError> {
        let mut bytes = message.as_bytes().to_vec();
        bytes.push(0);
        let message = AnyMessage::ChatFromViewer(ChatFromViewer {
            agent_data: ChatFromViewerAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            chat_data: ChatFromViewerChatDataBlock {
                message: bytes,
                r#type: chat_type.to_u8(),
                channel: channel.0,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ImprovedInstantMessage` reliably to a single agent. The IM
    /// session id is the canonical `agent_id XOR to_agent_id` the viewer uses for
    /// 1:1 sessions; `from_group` is false and the binary bucket is empty (the
    /// shape of an ordinary direct IM or a typing notification). The wire strings
    /// carry trailing NULs, as a real viewer sends.
    pub(crate) fn send_instant_message_raw(
        &mut self,
        to_agent_id: AgentKey,
        dialog: ImDialog,
        message: &str,
        from_name: &str,
        now: Instant,
    ) -> Result<(), WireError> {
        let mut name_bytes = from_name.as_bytes().to_vec();
        name_bytes.push(0);
        let mut message_bytes = message.as_bytes().to_vec();
        message_bytes.push(0);
        let message = AnyMessage::ImprovedInstantMessage(ImprovedInstantMessage {
            agent_data: ImprovedInstantMessageAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            message_block: ImprovedInstantMessageMessageBlockBlock {
                from_group: false,
                to_agent_id: to_agent_id.uuid(),
                parent_estate_id: 0,
                region_id: Uuid::nil(),
                position: Vector {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                offline: 0, // IM_ONLINE
                dialog: dialog.to_u8(),
                id: compute_im_session_id(self.agent_id, to_agent_id),
                timestamp: 0,
                from_agent_name: name_bytes,
                message: message_bytes,
                binary_bucket: Vec::new(),
            },
            estate_block: ImprovedInstantMessageEstateBlockBlock { estate_id: 0 },
            meta_data: Vec::new(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a fully-specified `ImprovedInstantMessage` reliably. Unlike
    /// [`send_instant_message_raw`](Self::send_instant_message_raw) the caller
    /// controls every dialog-dependent field (`id`, `from_group`, the binary
    /// bucket), so this backs the offer-reply, give-inventory and conference
    /// flows (#28). The wire strings carry trailing NULs, as a viewer sends.
    pub(crate) fn send_im(
        &mut self,
        params: &OutgoingIm<'_>,
        now: Instant,
    ) -> Result<(), WireError> {
        let im = AnyMessage::ImprovedInstantMessage(ImprovedInstantMessage {
            agent_data: ImprovedInstantMessageAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            message_block: ImprovedInstantMessageMessageBlockBlock {
                from_group: params.from_group,
                to_agent_id: params.to_agent_id,
                parent_estate_id: 0,
                region_id: Uuid::nil(),
                position: Vector {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                offline: 0, // IM_ONLINE
                dialog: params.dialog.to_u8(),
                id: params.id,
                timestamp: 0,
                from_agent_name: with_nul(params.from_name),
                message: with_nul(params.message),
                binary_bucket: params.binary_bucket.clone(),
            },
            estate_block: ImprovedInstantMessageEstateBlockBlock { estate_id: 0 },
            meta_data: Vec::new(),
        });
        self.send(&im, Reliability::Reliable, now)
    }

    /// Queues a `StartLure` reliably: offers a teleport to each agent in
    /// `targets` (a teleport "lure"). The simulator turns it into an
    /// `IM_LURE_USER` instant message carrying a lure id the recipient echoes
    /// back via [`Session::accept_teleport_lure`].
    pub(crate) fn send_start_lure(
        &mut self,
        targets: &[Uuid],
        message: &str,
        now: Instant,
    ) -> Result<(), WireError> {
        let lure = AnyMessage::StartLure(StartLure {
            agent_data: StartLureAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            info: StartLureInfoBlock {
                // The viewer sends 0; the simulator fills in the real lure type.
                lure_type: 0,
                message: with_nul(message),
            },
            target_data: targets
                .iter()
                .map(|&target_id| StartLureTargetDataBlock { target_id })
                .collect(),
        });
        self.send(&lure, Reliability::Reliable, now)
    }

    /// Queues a `UUIDNameRequest` reliably for the given agent ids. The caller is
    /// responsible for keeping `ids` small enough to fit one packet.
    pub(crate) fn send_uuid_name_request(
        &mut self,
        ids: &[Uuid],
        now: Instant,
    ) -> Result<(), WireError> {
        let request = AnyMessage::UUIDNameRequest(UUIDNameRequest {
            uuid_name_block: ids
                .iter()
                .map(|&id| UUIDNameRequestUUIDNameBlockBlock { id })
                .collect(),
        });
        self.send(&request, Reliability::Reliable, now)
    }

    /// Queues a `UUIDGroupNameRequest` reliably for the given group ids. The
    /// caller is responsible for keeping `ids` small enough to fit one packet.
    pub(crate) fn send_uuid_group_name_request(
        &mut self,
        ids: &[Uuid],
        now: Instant,
    ) -> Result<(), WireError> {
        let request = AnyMessage::UUIDGroupNameRequest(UUIDGroupNameRequest {
            uuid_name_block: ids
                .iter()
                .map(|&id| UUIDGroupNameRequestUUIDNameBlockBlock { id })
                .collect(),
        });
        self.send(&request, Reliability::Reliable, now)
    }

    /// Queues a `TeleportLureRequest` reliably: accepts a teleport lure,
    /// requesting the teleport the offer's `lure_id` (the `IM_LURE_USER` IM's
    /// `id`) describes. `teleport_flags` is the viewer's
    /// [`TeleportFlags::VIA_LURE`].
    pub(crate) fn send_teleport_lure_request(
        &mut self,
        lure_id: Uuid,
        teleport_flags: TeleportFlags,
        now: Instant,
    ) -> Result<(), WireError> {
        let request = AnyMessage::TeleportLureRequest(TeleportLureRequest {
            info: TeleportLureRequestInfoBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
                lure_id,
                teleport_flags: teleport_flags.0,
            },
        });
        self.send(&request, Reliability::Reliable, now)
    }

    /// Queues a `RetrieveInstantMessages` reliably: asks the simulator to flush
    /// the agent's stored offline instant messages, which then arrive as
    /// ordinary `ImprovedInstantMessage`s with the offline flag set.
    pub(crate) fn send_retrieve_instant_messages(&mut self, now: Instant) -> Result<(), WireError> {
        let request = AnyMessage::RetrieveInstantMessages(RetrieveInstantMessages {
            agent_data: RetrieveInstantMessagesAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
        });
        self.send(&request, Reliability::Reliable, now)
    }

    /// Queues an `AgentUpdate` unreliably carrying the given control flags,
    /// body/head rotation and camera viewpoint.
    ///
    /// The `camera` position and axes, together with the configured draw
    /// distance, are how the simulator builds the agent's interest list and
    /// enables the neighbouring regions (which arrive as `EnableSimulator`), so
    /// the streamed scene follows where the agent looks. The simulator moves the
    /// agent according to `control_flags` in the direction of `body_rotation`.
    pub(crate) fn send_agent_update(
        &mut self,
        control_flags: u32,
        body_rotation: Rotation,
        head_rotation: Rotation,
        camera: &Camera,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::AgentUpdate(AgentUpdate {
            agent_data: AgentUpdateAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
                body_rotation,
                head_rotation,
                state: 0,
                camera_center: camera.center.clone(),
                camera_at_axis: camera.at_axis.clone(),
                camera_left_axis: camera.left_axis.clone(),
                camera_up_axis: camera.up_axis.clone(),
                far: far_from_distance(&self.draw_distance),
                control_flags,
                flags: 0,
            },
        });
        self.send(&message, Reliability::Unreliable, now)
    }

    /// Queues an `AgentRequestSit` reliably (ask to sit on `target` at `offset`).
    pub(crate) fn send_agent_request_sit(
        &mut self,
        target: Uuid,
        offset: Vector,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::AgentRequestSit(AgentRequestSit {
            agent_data: AgentRequestSitAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            target_object: AgentRequestSitTargetObjectBlock {
                target_id: target,
                offset,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `AgentSit` reliably (complete a sit after `AvatarSitResponse`).
    pub(crate) fn send_agent_sit(&mut self, now: Instant) -> Result<(), WireError> {
        let message = AnyMessage::AgentSit(AgentSit {
            agent_data: AgentSitAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `GenericMessage` reliably with the given method and string
    /// parameters (used for the server-side `autopilot` walk-to command).
    pub(crate) fn send_generic_message(
        &mut self,
        method: &str,
        params: &[String],
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::GenericMessage(GenericMessage {
            agent_data: GenericMessageAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
                transaction_id: Uuid::nil(),
            },
            method_data: GenericMessageMethodDataBlock {
                method: method.as_bytes().to_vec(),
                invoice: Uuid::nil(),
            },
            param_list: params
                .iter()
                .map(|param| GenericMessageParamListBlock {
                    parameter: param.as_bytes().to_vec(),
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `AvatarPropertiesRequest` reliably for the avatar `target`.
    pub(crate) fn send_avatar_properties_request(
        &mut self,
        target: Uuid,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::AvatarPropertiesRequest(AvatarPropertiesRequest {
            agent_data: AvatarPropertiesRequestAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
                avatar_id: target,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `AvatarPropertiesUpdate` reliably, replacing the agent's own
    /// profile (#29).
    pub(crate) fn send_avatar_properties_update(
        &mut self,
        update: &ProfileUpdate,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::AvatarPropertiesUpdate(AvatarPropertiesUpdate {
            agent_data: AvatarPropertiesUpdateAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            properties_data: AvatarPropertiesUpdatePropertiesDataBlock {
                image_id: update.image_id.uuid(),
                fl_image_id: update.fl_image_id.uuid(),
                about_text: with_nul(&update.about_text),
                fl_about_text: with_nul(&update.fl_about_text),
                allow_publish: update.allow_publish,
                mature_publish: update.mature_publish,
                profile_url: with_nul(&update.profile_url),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `AvatarInterestsUpdate` reliably, replacing the agent's own
    /// interests (#29).
    pub(crate) fn send_avatar_interests_update(
        &mut self,
        update: &InterestsUpdate,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::AvatarInterestsUpdate(AvatarInterestsUpdate {
            agent_data: AvatarInterestsUpdateAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            properties_data: AvatarInterestsUpdatePropertiesDataBlock {
                want_to_mask: update.want_to_mask,
                want_to_text: with_nul(&update.want_to_text),
                skills_mask: update.skills_mask,
                skills_text: with_nul(&update.skills_text),
                languages_text: with_nul(&update.languages_text),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `AvatarNotesUpdate` reliably, setting the agent's private notes
    /// about `target` (#29).
    pub(crate) fn send_avatar_notes_update(
        &mut self,
        target: Uuid,
        notes: &str,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::AvatarNotesUpdate(AvatarNotesUpdate {
            agent_data: AvatarNotesUpdateAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            data: AvatarNotesUpdateDataBlock {
                target_id: target,
                notes: with_nul(notes),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `ClassifiedInfoRequest` reliably for the classified
    /// `classified_id` (#29).
    pub(crate) fn send_classified_info_request(
        &mut self,
        classified_id: ClassifiedKey,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ClassifiedInfoRequest(ClassifiedInfoRequest {
            agent_data: ClassifiedInfoRequestAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            data: ClassifiedInfoRequestDataBlock {
                classified_id: classified_id.uuid(),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `PickInfoUpdate` reliably, creating or editing one of the
    /// agent's picks (#29). `creator_id` is set to the agent itself.
    pub(crate) fn send_pick_info_update(
        &mut self,
        update: &PickUpdate,
        now: Instant,
    ) -> Result<(), WireError> {
        let pos = update.pos_global;
        let message = AnyMessage::PickInfoUpdate(PickInfoUpdate {
            agent_data: PickInfoUpdateAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            data: PickInfoUpdateDataBlock {
                pick_id: update.pick_id.uuid(),
                creator_id: self.agent_id.uuid(),
                // Only gods may set the legacy "top pick" flag; the viewer
                // always sends false.
                top_pick: false,
                parcel_id: update
                    .parcel_id
                    .map_or_else(Uuid::nil, |parcel| parcel.uuid()),
                name: with_nul(&update.name),
                desc: with_nul(&update.description),
                snapshot_id: update.snapshot_id.map_or_else(Uuid::nil, |s| s.uuid()),
                pos_global: [pos.x(), pos.y(), pos.z()],
                sort_order: update.sort_order,
                enabled: update.enabled,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `PickDelete` reliably, removing one of the agent's picks (#29).
    pub(crate) fn send_pick_delete(
        &mut self,
        pick_id: PickKey,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::PickDelete(PickDelete {
            agent_data: PickDeleteAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            data: PickDeleteDataBlock {
                pick_id: pick_id.uuid(),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `PickGodDelete` reliably (god-only; `query_id` lets the
    /// dataserver resend the affected agent's pick list) (#29).
    pub(crate) fn send_pick_god_delete(
        &mut self,
        pick_id: PickKey,
        query_id: Uuid,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::PickGodDelete(PickGodDelete {
            agent_data: PickGodDeleteAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            data: PickGodDeleteDataBlock {
                pick_id: pick_id.uuid(),
                query_id,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `ClassifiedInfoUpdate` reliably, creating or editing one of the
    /// agent's classifieds (#29). The simulator fills in the parent estate.
    pub(crate) fn send_classified_info_update(
        &mut self,
        update: &ClassifiedUpdate,
        now: Instant,
    ) -> Result<(), WireError> {
        let pos = update.pos_global;
        let message = AnyMessage::ClassifiedInfoUpdate(ClassifiedInfoUpdate {
            agent_data: ClassifiedInfoUpdateAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            data: ClassifiedInfoUpdateDataBlock {
                classified_id: update.classified_id.uuid(),
                category: update.category.to_u32(),
                name: with_nul(&update.name),
                desc: with_nul(&update.description),
                parcel_id: update
                    .parcel_id
                    .map_or_else(Uuid::nil, |parcel| parcel.uuid()),
                // Set on the simulator as the message passes through.
                parent_estate: 0,
                snapshot_id: update.snapshot_id.map_or_else(Uuid::nil, |s| s.uuid()),
                pos_global: [pos.x(), pos.y(), pos.z()],
                classified_flags: update.classified_flags,
                price_for_listing: crate::types::linden_to_wire(
                    "PriceForListing",
                    &update.price_for_listing,
                )?,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `ClassifiedDelete` reliably, removing one of the agent's
    /// classifieds (#29).
    pub(crate) fn send_classified_delete(
        &mut self,
        classified_id: ClassifiedKey,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ClassifiedDelete(ClassifiedDelete {
            agent_data: ClassifiedDeleteAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            data: ClassifiedDeleteDataBlock {
                classified_id: classified_id.uuid(),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `ClassifiedGodDelete` reliably (god-only; `query_id` lets the
    /// dataserver resend the affected agent's classified list) (#29).
    pub(crate) fn send_classified_god_delete(
        &mut self,
        classified_id: ClassifiedKey,
        query_id: Uuid,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ClassifiedGodDelete(ClassifiedGodDelete {
            agent_data: ClassifiedGodDeleteAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            data: ClassifiedGodDeleteDataBlock {
                classified_id: classified_id.uuid(),
                query_id,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `GrantUserRights` reliably, setting the rights this agent grants
    /// the friend `target` to `rights`.
    pub(crate) fn send_grant_user_rights(
        &mut self,
        target: FriendKey,
        rights: i32,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::GrantUserRights(GrantUserRights {
            agent_data: GrantUserRightsAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            rights: vec![GrantUserRightsRightsBlock {
                agent_related: target.uuid(),
                related_rights: rights,
            }],
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `TerminateFriendship` reliably, ending the friendship with
    /// `other`.
    pub(crate) fn send_terminate_friendship(
        &mut self,
        other: FriendKey,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::TerminateFriendship(TerminateFriendship {
            agent_data: TerminateFriendshipAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            ex_block: TerminateFriendshipExBlockBlock {
                other_id: other.uuid(),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `AcceptFriendship` reliably for the friendship-offer
    /// `transaction_id`, placing the new calling card in `folder`.
    pub(crate) fn send_accept_friendship(
        &mut self,
        transaction_id: Uuid,
        folder: Uuid,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::AcceptFriendship(AcceptFriendship {
            agent_data: AcceptFriendshipAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            transaction_block: AcceptFriendshipTransactionBlockBlock { transaction_id },
            folder_data: vec![AcceptFriendshipFolderDataBlock { folder_id: folder }],
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `DeclineFriendship` reliably for the friendship-offer
    /// `transaction_id`.
    pub(crate) fn send_decline_friendship(
        &mut self,
        transaction_id: Uuid,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::DeclineFriendship(DeclineFriendship {
            agent_data: DeclineFriendshipAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            transaction_block: DeclineFriendshipTransactionBlockBlock { transaction_id },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `OfferCallingCard` reliably, offering this agent's calling card
    /// to `dest_id` (a reference card to this avatar, filed in the recipient's
    /// Calling Cards folder — not a friendship request). `transaction_id`
    /// correlates the recipient's accept/decline reply.
    pub(crate) fn send_offer_calling_card(
        &mut self,
        dest_id: AgentKey,
        transaction_id: Uuid,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::OfferCallingCard(OfferCallingCard {
            agent_data: OfferCallingCardAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            agent_block: OfferCallingCardAgentBlockBlock {
                dest_id: dest_id.uuid(),
                transaction_id,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `AcceptCallingCard` reliably for the calling-card-offer
    /// `transaction_id`, filing the new card in `folder`.
    pub(crate) fn send_accept_calling_card(
        &mut self,
        transaction_id: Uuid,
        folder: Uuid,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::AcceptCallingCard(AcceptCallingCard {
            agent_data: AcceptCallingCardAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            transaction_block: AcceptCallingCardTransactionBlockBlock { transaction_id },
            folder_data: vec![AcceptCallingCardFolderDataBlock { folder_id: folder }],
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `DeclineCallingCard` reliably for the calling-card-offer
    /// `transaction_id`.
    pub(crate) fn send_decline_calling_card(
        &mut self,
        transaction_id: Uuid,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::DeclineCallingCard(DeclineCallingCard {
            agent_data: DeclineCallingCardAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            transaction_block: DeclineCallingCardTransactionBlockBlock { transaction_id },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ActivateGroup` reliably, making `group_id` the active group
    /// (nil clears the active group).
    pub(crate) fn send_activate_group(
        &mut self,
        group_id: GroupKey,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ActivateGroup(ActivateGroup {
            agent_data: ActivateGroupAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
                group_id: group_id.uuid(),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `GroupMembersRequest` reliably for `group_id`.
    pub(crate) fn send_group_members_request(
        &mut self,
        group_id: GroupKey,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::GroupMembersRequest(GroupMembersRequest {
            agent_data: GroupMembersRequestAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            group_data: GroupMembersRequestGroupDataBlock {
                group_id: group_id.uuid(),
                request_id: Uuid::nil(),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `GroupRoleDataRequest` reliably for `group_id`.
    pub(crate) fn send_group_role_data_request(
        &mut self,
        group_id: GroupKey,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::GroupRoleDataRequest(GroupRoleDataRequest {
            agent_data: GroupRoleDataRequestAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            group_data: GroupRoleDataRequestGroupDataBlock {
                group_id: group_id.uuid(),
                request_id: Uuid::nil(),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `GroupRoleMembersRequest` reliably for `group_id`.
    pub(crate) fn send_group_role_members_request(
        &mut self,
        group_id: GroupKey,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::GroupRoleMembersRequest(GroupRoleMembersRequest {
            agent_data: GroupRoleMembersRequestAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            group_data: GroupRoleMembersRequestGroupDataBlock {
                group_id: group_id.uuid(),
                request_id: Uuid::nil(),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `GroupTitlesRequest` reliably for `group_id`.
    pub(crate) fn send_group_titles_request(
        &mut self,
        group_id: GroupKey,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::GroupTitlesRequest(GroupTitlesRequest {
            agent_data: GroupTitlesRequestAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
                group_id: group_id.uuid(),
                request_id: Uuid::nil(),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `GroupProfileRequest` reliably for `group_id`.
    pub(crate) fn send_group_profile_request(
        &mut self,
        group_id: GroupKey,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::GroupProfileRequest(GroupProfileRequest {
            agent_data: GroupProfileRequestAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            group_data: GroupProfileRequestGroupDataBlock {
                group_id: group_id.uuid(),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `GroupNoticesListRequest` reliably for `group_id`.
    pub(crate) fn send_group_notices_list_request(
        &mut self,
        group_id: GroupKey,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::GroupNoticesListRequest(GroupNoticesListRequest {
            agent_data: GroupNoticesListRequestAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            data: GroupNoticesListRequestDataBlock {
                group_id: group_id.uuid(),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `GroupNoticeRequest` reliably for the notice `notice_id`.
    pub(crate) fn send_group_notice_request(
        &mut self,
        notice_id: GroupNoticeKey,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::GroupNoticeRequest(GroupNoticeRequest {
            agent_data: GroupNoticeRequestAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            data: GroupNoticeRequestDataBlock {
                group_notice_id: notice_id.uuid(),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `CreateGroupRequest` reliably.
    pub(crate) fn send_create_group_request(
        &mut self,
        params: &CreateGroupParams,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::CreateGroupRequest(CreateGroupRequest {
            agent_data: CreateGroupRequestAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            group_data: CreateGroupRequestGroupDataBlock {
                name: with_nul(&params.name),
                charter: with_nul(&params.charter),
                show_in_list: params.show_in_list,
                insignia_id: params.insignia_id.map_or_else(Uuid::nil, |i| i.uuid()),
                membership_fee: crate::types::linden_to_wire(
                    "MembershipFee",
                    &params.membership_fee,
                )?,
                open_enrollment: params.open_enrollment,
                allow_publish: params.allow_publish,
                mature_publish: params.mature_publish,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `UpdateGroupInfo` reliably (edit an existing group's profile).
    pub(crate) fn send_update_group_info(
        &mut self,
        params: &UpdateGroupInfoParams,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::UpdateGroupInfo(UpdateGroupInfo {
            agent_data: UpdateGroupInfoAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            group_data: UpdateGroupInfoGroupDataBlock {
                group_id: params.group_id.uuid(),
                charter: with_nul(&params.charter),
                show_in_list: params.show_in_list,
                insignia_id: params.insignia_id.map_or_else(Uuid::nil, |i| i.uuid()),
                membership_fee: crate::types::linden_to_wire(
                    "MembershipFee",
                    &params.membership_fee,
                )?,
                open_enrollment: params.open_enrollment,
                allow_publish: params.allow_publish,
                mature_publish: params.mature_publish,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `GroupTitleUpdate` reliably (set the agent's active title in
    /// `group_id` to the title carried by `title_role_id`).
    pub(crate) fn send_group_title_update(
        &mut self,
        group_id: GroupKey,
        title_role_id: GroupRoleKey,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::GroupTitleUpdate(GroupTitleUpdate {
            agent_data: GroupTitleUpdateAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
                group_id: group_id.uuid(),
                title_role_id: title_role_id.uuid(),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `JoinGroupRequest` reliably for `group_id`.
    pub(crate) fn send_join_group_request(
        &mut self,
        group_id: GroupKey,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::JoinGroupRequest(JoinGroupRequest {
            agent_data: JoinGroupRequestAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            group_data: JoinGroupRequestGroupDataBlock {
                group_id: group_id.uuid(),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `LeaveGroupRequest` reliably for `group_id`.
    pub(crate) fn send_leave_group_request(
        &mut self,
        group_id: GroupKey,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::LeaveGroupRequest(LeaveGroupRequest {
            agent_data: LeaveGroupRequestAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            group_data: LeaveGroupRequestGroupDataBlock {
                group_id: group_id.uuid(),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `InviteGroupRequest` reliably inviting `invitees` (each an
    /// `(invitee_id, role_id)` pair, nil `role_id` for the default Everyone role)
    /// to `group_id`.
    pub(crate) fn send_invite_group_request(
        &mut self,
        group_id: GroupKey,
        invitees: &[(AgentKey, GroupRoleKey)],
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::InviteGroupRequest(InviteGroupRequest {
            agent_data: InviteGroupRequestAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            group_data: InviteGroupRequestGroupDataBlock {
                group_id: group_id.uuid(),
            },
            invite_data: invitees
                .iter()
                .map(|(invitee_id, role_id)| InviteGroupRequestInviteDataBlock {
                    invitee_id: invitee_id.uuid(),
                    role_id: role_id.uuid(),
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `SetGroupAcceptNotices` reliably for `group_id`.
    pub(crate) fn send_set_group_accept_notices(
        &mut self,
        group_id: GroupKey,
        accept_notices: bool,
        list_in_profile: bool,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::SetGroupAcceptNotices(SetGroupAcceptNotices {
            agent_data: SetGroupAcceptNoticesAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            data: SetGroupAcceptNoticesDataBlock {
                group_id: group_id.uuid(),
                accept_notices,
            },
            new_data: SetGroupAcceptNoticesNewDataBlock { list_in_profile },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `SetGroupContribution` reliably for `group_id`.
    pub(crate) fn send_set_group_contribution(
        &mut self,
        group_id: GroupKey,
        contribution: i32,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::SetGroupContribution(SetGroupContribution {
            agent_data: SetGroupContributionAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            data: SetGroupContributionDataBlock {
                group_id: group_id.uuid(),
                contribution,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `GroupRoleUpdate` reliably, carrying one `RoleData` block per
    /// role create/update/delete in `roles`.
    pub(crate) fn send_group_role_update(
        &mut self,
        group_id: GroupKey,
        roles: &[GroupRoleEdit],
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::GroupRoleUpdate(GroupRoleUpdate {
            agent_data: GroupRoleUpdateAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
                group_id: group_id.uuid(),
            },
            role_data: roles
                .iter()
                .map(|role| GroupRoleUpdateRoleDataBlock {
                    role_id: role.role_id.map_or_else(Uuid::nil, |r| r.uuid()),
                    name: with_nul(&role.name),
                    description: with_nul(&role.description),
                    title: with_nul(&role.title),
                    powers: role.powers,
                    update_type: role.update_type.to_u8(),
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `GroupRoleChanges` reliably, carrying one `RoleChange` block per
    /// member↔role add/remove in `changes`.
    pub(crate) fn send_group_role_changes(
        &mut self,
        group_id: GroupKey,
        changes: &[GroupRoleMemberChange],
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::GroupRoleChanges(GroupRoleChanges {
            agent_data: GroupRoleChangesAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
                group_id: group_id.uuid(),
            },
            role_change: changes
                .iter()
                .map(|change| GroupRoleChangesRoleChangeBlock {
                    role_id: change.role_id.map_or_else(Uuid::nil, |r| r.uuid()),
                    member_id: change.member_id.uuid(),
                    change: change.change.to_u32(),
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `EjectGroupMemberRequest` reliably, ejecting each agent in
    /// `member_ids` from `group_id`.
    pub(crate) fn send_eject_group_members(
        &mut self,
        group_id: GroupKey,
        member_ids: &[AgentKey],
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::EjectGroupMemberRequest(EjectGroupMemberRequest {
            agent_data: EjectGroupMemberRequestAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            group_data: EjectGroupMemberRequestGroupDataBlock {
                group_id: group_id.uuid(),
            },
            eject_data: member_ids
                .iter()
                .map(|ejectee_id| EjectGroupMemberRequestEjectDataBlock {
                    ejectee_id: ejectee_id.uuid(),
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ActivateGestures` reliably, marking each gesture in `gestures`
    /// active for this session.
    pub(crate) fn send_activate_gestures(
        &mut self,
        gestures: &[GestureActivation],
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ActivateGestures(ActivateGestures {
            agent_data: ActivateGesturesAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
                flags: 0,
            },
            data: gestures
                .iter()
                .map(|gesture| ActivateGesturesDataBlock {
                    item_id: gesture.item_id.uuid(),
                    asset_id: gesture.asset_id,
                    gesture_flags: 0,
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `DeactivateGestures` reliably, marking each gesture named in
    /// `item_ids` inactive for this session.
    pub(crate) fn send_deactivate_gestures(
        &mut self,
        item_ids: &[Uuid],
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::DeactivateGestures(DeactivateGestures {
            agent_data: DeactivateGesturesAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
                flags: 0,
            },
            data: item_ids
                .iter()
                .map(|item_id| DeactivateGesturesDataBlock {
                    item_id: *item_id,
                    gesture_flags: 0,
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Allocates the next `AgentPause`/`AgentResume` serial number (a single
    /// monotonic counter shared by both, as the simulator ignores non-increasing
    /// values).
    pub(crate) const fn next_pause_serial(&mut self) -> u32 {
        self.pause_serial_num = self.pause_serial_num.wrapping_add(1);
        self.pause_serial_num
    }

    /// Queues a `SetAlwaysRun` reliably, choosing whether the avatar runs or
    /// walks for ground movement.
    pub(crate) fn send_set_always_run(
        &mut self,
        mode: MovementMode,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::SetAlwaysRun(SetAlwaysRun {
            agent_data: SetAlwaysRunAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
                always_run: mode.is_always_run(),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `AgentPause` reliably, telling the simulator the viewer has
    /// stalled so it stops streaming updates until a resume.
    pub(crate) fn send_agent_pause(&mut self, now: Instant) -> Result<(), WireError> {
        let serial_num = self.next_pause_serial();
        let message = AnyMessage::AgentPause(AgentPause {
            agent_data: AgentPauseAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
                serial_num,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `AgentResume` reliably, telling the simulator the viewer has
    /// resumed reading the network after a pause.
    pub(crate) fn send_agent_resume(&mut self, now: Instant) -> Result<(), WireError> {
        let serial_num = self.next_pause_serial();
        let message = AnyMessage::AgentResume(AgentResume {
            agent_data: AgentResumeAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
                serial_num,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `AgentFOV` reliably, updating the agent's vertical field of view
    /// (radians). The `GenCounter` is fixed at 0, matching the real viewer.
    pub(crate) fn send_agent_fov(
        &mut self,
        vertical_angle: f32,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::AgentFOV(AgentFOV {
            agent_data: AgentFOVAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
                circuit_code: self.code.get(),
            },
            fov_block: AgentFOVFOVBlockBlock {
                gen_counter: 0,
                vertical_angle,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `AgentHeightWidth` reliably, updating the agent's viewport size
    /// (pixels). The `GenCounter` is fixed at 0, matching the real viewer.
    pub(crate) fn send_agent_height_width(
        &mut self,
        height: u16,
        width: u16,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::AgentHeightWidth(AgentHeightWidth {
            agent_data: AgentHeightWidthAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
                circuit_code: self.code.get(),
            },
            height_width_block: AgentHeightWidthHeightWidthBlockBlock {
                gen_counter: 0,
                height,
                width,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `ForceScriptControlRelease` reliably, forcibly releasing any
    /// agent movement controls a script has taken.
    pub(crate) fn send_force_script_control_release(
        &mut self,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ForceScriptControlRelease(ForceScriptControlRelease {
            agent_data: ForceScriptControlReleaseAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `GroupAccountSummaryRequest` reliably for `group_id` over the
    /// accounting interval selected by `interval_days`/`current_interval`.
    pub(crate) fn send_group_account_summary_request(
        &mut self,
        group_id: GroupKey,
        request_id: Uuid,
        interval_days: i32,
        current_interval: i32,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::GroupAccountSummaryRequest(GroupAccountSummaryRequest {
            agent_data: GroupAccountSummaryRequestAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
                group_id: group_id.uuid(),
            },
            money_data: GroupAccountSummaryRequestMoneyDataBlock {
                request_id,
                interval_days,
                current_interval,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `GroupAccountDetailsRequest` reliably for `group_id`.
    pub(crate) fn send_group_account_details_request(
        &mut self,
        group_id: GroupKey,
        request_id: Uuid,
        interval_days: i32,
        current_interval: i32,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::GroupAccountDetailsRequest(GroupAccountDetailsRequest {
            agent_data: GroupAccountDetailsRequestAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
                group_id: group_id.uuid(),
            },
            money_data: GroupAccountDetailsRequestMoneyDataBlock {
                request_id,
                interval_days,
                current_interval,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `GroupAccountTransactionsRequest` reliably for `group_id`.
    pub(crate) fn send_group_account_transactions_request(
        &mut self,
        group_id: GroupKey,
        request_id: Uuid,
        interval_days: i32,
        current_interval: i32,
        now: Instant,
    ) -> Result<(), WireError> {
        let message =
            AnyMessage::GroupAccountTransactionsRequest(GroupAccountTransactionsRequest {
                agent_data: GroupAccountTransactionsRequestAgentDataBlock {
                    agent_id: self.agent_id.uuid(),
                    session_id: self.session_id,
                    group_id: group_id.uuid(),
                },
                money_data: GroupAccountTransactionsRequestMoneyDataBlock {
                    request_id,
                    interval_days,
                    current_interval,
                },
            });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `GroupActiveProposalsRequest` reliably for `group_id`.
    pub(crate) fn send_group_active_proposals_request(
        &mut self,
        group_id: GroupKey,
        transaction_id: Uuid,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::GroupActiveProposalsRequest(GroupActiveProposalsRequest {
            agent_data: GroupActiveProposalsRequestAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            group_data: GroupActiveProposalsRequestGroupDataBlock {
                group_id: group_id.uuid(),
            },
            transaction_data: GroupActiveProposalsRequestTransactionDataBlock { transaction_id },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `GroupVoteHistoryRequest` reliably for `group_id`.
    pub(crate) fn send_group_vote_history_request(
        &mut self,
        group_id: GroupKey,
        transaction_id: Uuid,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::GroupVoteHistoryRequest(GroupVoteHistoryRequest {
            agent_data: GroupVoteHistoryRequestAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            group_data: GroupVoteHistoryRequestGroupDataBlock {
                group_id: group_id.uuid(),
            },
            transaction_data: GroupVoteHistoryRequestTransactionDataBlock { transaction_id },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `StartGroupProposal` reliably (start a new proposal/vote in
    /// `group_id`).
    pub(crate) fn send_start_group_proposal(
        &mut self,
        group_id: GroupKey,
        quorum: i32,
        majority: f32,
        duration: i32,
        proposal_text: &str,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::StartGroupProposal(StartGroupProposal {
            agent_data: StartGroupProposalAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            proposal_data: StartGroupProposalProposalDataBlock {
                group_id: group_id.uuid(),
                quorum,
                majority,
                duration,
                proposal_text: with_nul(proposal_text),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `GroupProposalBallot` reliably (cast `vote_cast` on `proposal_id`
    /// of `group_id`).
    pub(crate) fn send_group_proposal_ballot(
        &mut self,
        proposal_id: ProposalVoteId,
        group_id: GroupKey,
        vote_cast: &str,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::GroupProposalBallot(GroupProposalBallot {
            agent_data: GroupProposalBallotAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            proposal_data: GroupProposalBallotProposalDataBlock {
                proposal_id: proposal_id.uuid(),
                group_id: group_id.uuid(),
                vote_cast: with_nul(vote_cast),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a group IM (`ImprovedInstantMessage`) reliably: the session id and
    /// recipient are both `group_id`, as group chat requires. `dialog` selects
    /// start/send/leave.
    pub(crate) fn send_group_session_im(
        &mut self,
        group_id: GroupKey,
        dialog: ImDialog,
        message: &str,
        from_name: &str,
        now: Instant,
    ) -> Result<(), WireError> {
        let im = AnyMessage::ImprovedInstantMessage(ImprovedInstantMessage {
            agent_data: ImprovedInstantMessageAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            message_block: ImprovedInstantMessageMessageBlockBlock {
                from_group: false,
                to_agent_id: group_id.uuid(),
                parent_estate_id: 0,
                region_id: Uuid::nil(),
                position: Vector {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                offline: 0,
                dialog: dialog.to_u8(),
                id: group_id.uuid(),
                timestamp: 0,
                from_agent_name: with_nul(from_name),
                message: with_nul(message),
                binary_bucket: Vec::new(),
            },
            estate_block: ImprovedInstantMessageEstateBlockBlock { estate_id: 0 },
            meta_data: Vec::new(),
        });
        self.send(&im, Reliability::Reliable, now)
    }

    /// Queues a `ScriptDialogReply` reliably (the chosen `llDialog` button).
    pub(crate) fn send_script_dialog_reply(
        &mut self,
        object_id: ObjectKey,
        chat_channel: ChatChannel,
        button_index: i32,
        button_label: &str,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ScriptDialogReply(ScriptDialogReply {
            agent_data: ScriptDialogReplyAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            data: ScriptDialogReplyDataBlock {
                object_id: object_id.uuid(),
                chat_channel: chat_channel.0,
                button_index,
                button_label: with_nul(button_label),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `ScriptAnswerYes` reliably granting `permissions` to the script
    /// `item_id` in object `task_id` (pass `0` to deny everything).
    pub(crate) fn send_script_answer_yes(
        &mut self,
        task_id: ObjectKey,
        item_id: Uuid,
        permissions: i32,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ScriptAnswerYes(ScriptAnswerYes {
            agent_data: ScriptAnswerYesAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            data: ScriptAnswerYesDataBlock {
                task_id: task_id.uuid(),
                item_id,
                questions: permissions,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `MuteListRequest` reliably. `mute_crc` is the CRC of the cached
    /// mute list (`0` forces a fresh download).
    pub(crate) fn send_mute_list_request(
        &mut self,
        mute_crc: u32,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::MuteListRequest(MuteListRequest {
            agent_data: MuteListRequestAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            mute_data: MuteListRequestMuteDataBlock { mute_crc },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `UpdateMuteListEntry` reliably (add or update a mute).
    pub(crate) fn send_update_mute_list_entry(
        &mut self,
        mute_id: Uuid,
        mute_name: &str,
        mute_type: i32,
        mute_flags: u32,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::UpdateMuteListEntry(UpdateMuteListEntry {
            agent_data: UpdateMuteListEntryAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            mute_data: UpdateMuteListEntryMuteDataBlock {
                mute_id,
                mute_name: with_nul(mute_name),
                mute_type,
                mute_flags,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `RemoveMuteListEntry` reliably (remove a mute).
    pub(crate) fn send_remove_mute_list_entry(
        &mut self,
        mute_id: Uuid,
        mute_name: &str,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::RemoveMuteListEntry(RemoveMuteListEntry {
            agent_data: RemoveMuteListEntryAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            mute_data: RemoveMuteListEntryMuteDataBlock {
                mute_id,
                mute_name: with_nul(mute_name),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `RequestXfer` reliably to download the file `filename` under the
    /// transfer id `xfer_id`.
    pub(crate) fn send_request_xfer(
        &mut self,
        xfer_id: XferId,
        filename: &str,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::RequestXfer(RequestXfer {
            xfer_id: RequestXferXferIDBlock {
                id: xfer_id.get(),
                filename: with_nul(filename),
                file_path: 0,
                delete_on_completion: true,
                use_big_packets: false,
                v_file_id: Uuid::nil(),
                v_file_type: 0,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `ConfirmXferPacket` reliably acknowledging `packet` of `xfer_id`.
    pub(crate) fn send_confirm_xfer_packet(
        &mut self,
        xfer_id: XferId,
        packet: u32,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ConfirmXferPacket(ConfirmXferPacket {
            xfer_id: ConfirmXferPacketXferIDBlock {
                id: xfer_id.get(),
                packet,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `AssetUploadRequest` reliably: a legacy UDP upload of `data` as
    /// asset class `asset_type`, identified by `transaction_id`. `data` is the
    /// inline payload (empty to force the `Xfer` path); `temp_file`/`store_local`
    /// mark a temporary / sim-local-only asset.
    pub(crate) fn send_asset_upload_request(
        &mut self,
        transaction_id: Uuid,
        asset_type: i8,
        temp_file: bool,
        store_local: bool,
        data: Vec<u8>,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::AssetUploadRequest(AssetUploadRequest {
            asset_block: AssetUploadRequestAssetBlockBlock {
                transaction_id,
                r#type: asset_type,
                tempfile: temp_file,
                store_local,
                asset_data: data,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `SendXferPacket` reliably: the chunk `data` for sequence
    /// `packet` of upload `xfer_id`. `packet` already carries the `0x80000000`
    /// last-packet flag for the final chunk.
    pub(crate) fn send_send_xfer_packet(
        &mut self,
        xfer_id: XferId,
        packet: u32,
        data: Vec<u8>,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::SendXferPacket(SendXferPacket {
            xfer_id: SendXferPacketXferIDBlock {
                id: xfer_id.get(),
                packet,
            },
            data_packet: SendXferPacketDataPacketBlock { data },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `RequestImage` reliably to download the texture `image_id` over
    /// the legacy UDP image path, starting at `packet` (0 for a fresh download)
    /// and at the given `discard_level` (0 = full resolution) and download
    /// `priority`. `image_type` is the request channel (0 = normal).
    pub(crate) fn send_request_image(
        &mut self,
        image_id: TextureKey,
        discard_level: i8,
        priority: f32,
        packet: u32,
        image_type: u8,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::RequestImage(RequestImage {
            agent_data: RequestImageAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            request_image: vec![RequestImageRequestImageBlock {
                image: image_id.uuid(),
                discard_level,
                download_priority: priority,
                packet,
                r#type: image_type,
            }],
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `AgentWearablesRequest` reliably, asking the simulator to
    /// (re-)send the agent's current wearables as an `AgentWearablesUpdate`.
    pub(crate) fn send_agent_wearables_request(&mut self, now: Instant) -> Result<(), WireError> {
        let message = AnyMessage::AgentWearablesRequest(AgentWearablesRequest {
            agent_data: AgentWearablesRequestAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `AgentIsNowWearing` reliably, telling the simulator the agent's
    /// new outfit (one `(item id, wearable slot)` per worn wearable).
    pub(crate) fn send_agent_is_now_wearing(
        &mut self,
        wearables: &[Wearable],
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::AgentIsNowWearing(AgentIsNowWearing {
            agent_data: AgentIsNowWearingAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            wearable_data: wearables
                .iter()
                .map(|wearable| AgentIsNowWearingWearableDataBlock {
                    item_id: wearable.item_id.uuid(),
                    wearable_type: wearable.wearable_type.to_code(),
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectAttach` reliably, attaching the in-world object
    /// `local_id` to `attachment_point` at `rotation` (`mode` chooses whether the
    /// attachment is added alongside or replaces what is on the point).
    pub(crate) fn send_object_attach(
        &mut self,
        local_id: RegionLocalObjectId,
        attachment_point: AttachmentPoint,
        mode: AttachmentMode,
        rotation: &Rotation,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectAttach(ObjectAttach {
            agent_data: ObjectAttachAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
                attachment_point: attachment_point.with_mode(mode),
            },
            object_data: vec![ObjectAttachObjectDataBlock {
                object_local_id: local_id.0,
                rotation: rotation.clone(),
            }],
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectDetach` reliably, detaching the attachments `local_ids`
    /// back to inventory.
    pub(crate) fn send_object_detach(
        &mut self,
        local_ids: &[RegionLocalObjectId],
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectDetach(ObjectDetach {
            agent_data: ObjectDetachAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            object_data: local_ids
                .iter()
                .map(|&object_local_id| ObjectDetachObjectDataBlock {
                    object_local_id: object_local_id.0,
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectDrop` reliably, dropping the attachments `local_ids` from
    /// the avatar onto the ground.
    pub(crate) fn send_object_drop(
        &mut self,
        local_ids: &[RegionLocalObjectId],
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectDrop(ObjectDrop {
            agent_data: ObjectDropAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            object_data: local_ids
                .iter()
                .map(|&object_local_id| ObjectDropObjectDataBlock {
                    object_local_id: object_local_id.0,
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `RemoveAttachment` reliably, taking off the worn item `item_id`
    /// (worn on `attachment_point`).
    pub(crate) fn send_remove_attachment(
        &mut self,
        attachment_point: AttachmentPoint,
        item_id: Uuid,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::RemoveAttachment(RemoveAttachment {
            agent_data: RemoveAttachmentAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            attachment_block: RemoveAttachmentAttachmentBlockBlock {
                attachment_point: attachment_point.to_code(),
                item_id,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `RezSingleAttachmentFromInv` reliably, wearing the inventory item
    /// described by `rez` as an attachment. The permission/flags fields are left
    /// zero — the simulator no longer reads them (the viewer's
    /// `pack_permissions_slam` is documented cruft).
    pub(crate) fn send_rez_single_attachment(
        &mut self,
        rez: &RezAttachment,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::RezSingleAttachmentFromInv(RezSingleAttachmentFromInv {
            agent_data: RezSingleAttachmentFromInvAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            object_data: RezSingleAttachmentFromInvObjectDataBlock {
                item_id: rez.item_id.uuid(),
                owner_id: rez.owner_id,
                attachment_pt: rez.attachment_point.with_mode(rez.mode),
                item_flags: 0,
                group_mask: 0,
                everyone_mask: 0,
                next_owner_mask: 0,
                name: with_nul(&rez.name),
                description: with_nul(&rez.description),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `RezMultipleAttachmentsFromInv` reliably, wearing several
    /// inventory items as attachments in one compound message. `compound_id`
    /// correlates the message's parts; `detach` says whether to detach everything
    /// worn first. The permission/flags fields are left zero (see
    /// [`send_rez_single_attachment`](Self::send_rez_single_attachment)).
    pub(crate) fn send_rez_multiple_attachments(
        &mut self,
        compound_id: Uuid,
        detach: DetachOrder,
        attachments: &[RezAttachment],
        now: Instant,
    ) -> Result<(), WireError> {
        let total_objects =
            u8::try_from(attachments.len()).map_err(|_e| WireError::VariableTooLong {
                len: attachments.len(),
                max: 255,
            })?;
        let message = AnyMessage::RezMultipleAttachmentsFromInv(RezMultipleAttachmentsFromInv {
            agent_data: RezMultipleAttachmentsFromInvAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            header_data: RezMultipleAttachmentsFromInvHeaderDataBlock {
                compound_msg_id: compound_id,
                total_objects,
                first_detach_all: detach.detaches_all_first(),
            },
            object_data: attachments
                .iter()
                .map(|rez| RezMultipleAttachmentsFromInvObjectDataBlock {
                    item_id: rez.item_id.uuid(),
                    owner_id: rez.owner_id,
                    attachment_pt: rez.attachment_point.with_mode(rez.mode),
                    item_flags: 0,
                    group_mask: 0,
                    everyone_mask: 0,
                    next_owner_mask: 0,
                    name: with_nul(&rez.name),
                    description: with_nul(&rez.description),
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `ViewerEffect` reliably, batching `effects` into one message
    /// (look-at / point-at gaze hints, the editing/touch beam, and other
    /// transient HUD effects). Each effect's `TypeData` is serialised from its
    /// typed [`ViewerEffectData`](crate::ViewerEffectData).
    pub(crate) fn send_viewer_effect(
        &mut self,
        effects: &[ViewerEffect],
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ViewerEffect(ViewerEffectMessage {
            agent_data: ViewerEffectAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            effect: effects
                .iter()
                .map(|effect| ViewerEffectEffectBlock {
                    id: effect.id,
                    agent_id: effect.agent_id.uuid(),
                    r#type: effect.effect_type.to_code(),
                    duration: effect.duration,
                    color: effect.color,
                    type_data: effect.data.to_wire(),
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `TrackAgent` reliably, asking the simulator to track `prey_id`'s
    /// position (streamed back via `CoarseLocationUpdate`).
    pub(crate) fn send_track_agent(
        &mut self,
        prey_id: AgentKey,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::TrackAgent(TrackAgent {
            agent_data: TrackAgentAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            target_data: TrackAgentTargetDataBlock {
                prey_id: prey_id.uuid(),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `FindAgent` reliably, asking the simulator for `prey`'s global
    /// position on behalf of `hunter`. The request carries an empty location
    /// block and a zero space address; the simulator answers with a `FindAgent`
    /// carrying the found positions.
    pub(crate) fn send_find_agent(
        &mut self,
        hunter: Uuid,
        prey: Uuid,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::FindAgent(FindAgent {
            agent_block: FindAgentAgentBlockBlock {
                hunter,
                prey,
                space_ip: [0, 0, 0, 0],
            },
            location_block: Vec::new(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `DirFindQuery` reliably: the unified people / groups / events
    /// directory search (`flags` selecting which, plus sort/filter bits).
    pub(crate) fn send_dir_find_query(
        &mut self,
        query_id: Uuid,
        query_text: &str,
        flags: DirFindFlags,
        query_start: i32,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::DirFindQuery(DirFindQuery {
            agent_data: DirFindQueryAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            query_data: DirFindQueryQueryDataBlock {
                query_id,
                query_text: with_nul(query_text),
                query_flags: flags.bits(),
                query_start,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `DirPlacesQuery` reliably: the places-directory search.
    #[expect(clippy::too_many_arguments, reason = "mirrors the wire query block")]
    pub(crate) fn send_dir_places_query(
        &mut self,
        query_id: Uuid,
        query_text: &str,
        flags: DirFindFlags,
        category: ParcelCategory,
        sim_name: &str,
        query_start: i32,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::DirPlacesQuery(DirPlacesQuery {
            agent_data: DirPlacesQueryAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            query_data: DirPlacesQueryQueryDataBlock {
                query_id,
                query_text: with_nul(query_text),
                query_flags: flags.bits(),
                category: category_to_wire(category),
                sim_name: with_nul(sim_name),
                query_start,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `DirLandQuery` reliably: the land-for-sale directory search.
    #[expect(clippy::too_many_arguments, reason = "mirrors the wire query block")]
    pub(crate) fn send_dir_land_query(
        &mut self,
        query_id: Uuid,
        flags: DirFindFlags,
        search_type: LandSearchType,
        price: i32,
        area: i32,
        query_start: i32,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::DirLandQuery(DirLandQuery {
            agent_data: DirLandQueryAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            query_data: DirLandQueryQueryDataBlock {
                query_id,
                query_flags: flags.bits(),
                search_type: search_type.bits(),
                price,
                area,
                query_start,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `DirClassifiedQuery` reliably: the classifieds-directory search.
    pub(crate) fn send_dir_classified_query(
        &mut self,
        query_id: Uuid,
        query_text: &str,
        flags: DirFindFlags,
        category: ClassifiedCategory,
        query_start: i32,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::DirClassifiedQuery(DirClassifiedQuery {
            agent_data: DirClassifiedQueryAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            query_data: DirClassifiedQueryQueryDataBlock {
                query_id,
                query_text: with_nul(query_text),
                query_flags: flags.bits(),
                category: category.to_u32(),
                query_start,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `AvatarPickerRequest` reliably: the name-autocomplete lookup.
    pub(crate) fn send_avatar_picker_request(
        &mut self,
        query_id: Uuid,
        name: &str,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::AvatarPickerRequest(AvatarPickerRequest {
            agent_data: AvatarPickerRequestAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
                query_id,
            },
            data: AvatarPickerRequestDataBlock {
                name: with_nul(name),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `PlacesQuery` reliably: the agent/group land-holdings lookup.
    #[expect(clippy::too_many_arguments, reason = "mirrors the wire query block")]
    pub(crate) fn send_places_query(
        &mut self,
        query_id: Uuid,
        transaction_id: Uuid,
        query_text: &str,
        flags: DirFindFlags,
        category: ParcelCategory,
        sim_name: &str,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::PlacesQuery(PlacesQuery {
            agent_data: PlacesQueryAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
                query_id,
            },
            transaction_data: PlacesQueryTransactionDataBlock { transaction_id },
            query_data: PlacesQueryQueryDataBlock {
                query_text: with_nul(query_text),
                query_flags: flags.bits(),
                category: category_to_wire(category),
                sim_name: with_nul(sim_name),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `EventInfoRequest` reliably: the full-detail lookup for an
    /// in-world event by id.
    pub(crate) fn send_event_info_request(
        &mut self,
        event_id: EventId,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::EventInfoRequest(EventInfoRequest {
            agent_data: EventInfoRequestAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            event_data: EventInfoRequestEventDataBlock {
                event_id: event_id.get(),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `EventNotificationAddRequest` reliably: subscribe to a reminder
    /// for an in-world event.
    pub(crate) fn send_event_notification_add_request(
        &mut self,
        event_id: EventId,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::EventNotificationAddRequest(EventNotificationAddRequest {
            agent_data: EventNotificationAddRequestAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            event_data: EventNotificationAddRequestEventDataBlock {
                event_id: event_id.get(),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `EventNotificationRemoveRequest` reliably: cancel a previously
    /// added event reminder.
    pub(crate) fn send_event_notification_remove_request(
        &mut self,
        event_id: EventId,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::EventNotificationRemoveRequest(EventNotificationRemoveRequest {
            agent_data: EventNotificationRemoveRequestAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            event_data: EventNotificationRemoveRequestEventDataBlock {
                event_id: event_id.get(),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `AgentSetAppearance` reliably, advertising the agent's own
    /// appearance: its bounding-box `size`, the baked-texture `texture_entry`
    /// blob, the `visual_params` bytes, and the per-baked-slot `wearable_cache`
    /// hashes (`(cache id, texture slot index)`). `serial` must increase on each
    /// change (0 resets).
    pub(crate) fn send_agent_set_appearance(
        &mut self,
        serial: u32,
        size: Vector,
        texture_entry: &[u8],
        visual_params: &[u8],
        wearable_cache: &[(Uuid, u8)],
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::AgentSetAppearance(AgentSetAppearance {
            agent_data: AgentSetAppearanceAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
                serial_num: serial,
                size,
            },
            wearable_data: wearable_cache
                .iter()
                .map(
                    |&(cache_id, texture_index)| AgentSetAppearanceWearableDataBlock {
                        cache_id,
                        texture_index,
                    },
                )
                .collect(),
            object_data: AgentSetAppearanceObjectDataBlock {
                texture_entry: texture_entry.to_vec(),
            },
            visual_param: visual_params
                .iter()
                .map(|&param_value| AgentSetAppearanceVisualParamBlock { param_value })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `AgentAnimation` reliably, starting or stopping the agent's own
    /// animations. Each `(anim_id, start)` pair starts (`true`) or stops
    /// (`false`) one animation. Mirrors the reference viewer, which always
    /// appends a single empty `PhysicalAvatarEventList` block.
    pub(crate) fn send_agent_animation(
        &mut self,
        animations: &[(Uuid, bool)],
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::AgentAnimation(AgentAnimation {
            agent_data: AgentAnimationAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            animation_list: animations
                .iter()
                .map(|&(anim_id, start_anim)| AgentAnimationAnimationListBlock {
                    anim_id,
                    start_anim,
                })
                .collect(),
            physical_avatar_event_list: vec![AgentAnimationPhysicalAvatarEventListBlock {
                type_data: Vec::new(),
            }],
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `AgentCachedTexture` reliably, asking the simulator which of the
    /// queried baked-texture slots it already has cached (`(cache id, texture
    /// slot index)` per slot). The reply is an `AgentCachedTextureResponse`.
    pub(crate) fn send_agent_cached_texture(
        &mut self,
        serial: i32,
        slots: &[(Uuid, u8)],
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::AgentCachedTexture(AgentCachedTexture {
            agent_data: AgentCachedTextureAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
                serial_num: serial,
            },
            wearable_data: slots
                .iter()
                .map(|&(id, texture_index)| AgentCachedTextureWearableDataBlock {
                    id,
                    texture_index,
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `FetchInventoryDescendents` reliably for the folder `folder_id`
    /// (sorted by name), requesting its sub-folders and items. `owner_id` is the
    /// agent's own id for its inventory, or the Library owner id for a shared
    /// Library folder.
    pub(crate) fn send_fetch_inventory_descendents(
        &mut self,
        folder_id: Uuid,
        owner_id: Uuid,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::FetchInventoryDescendents(FetchInventoryDescendents {
            agent_data: FetchInventoryDescendentsAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            inventory_data: FetchInventoryDescendentsInventoryDataBlock {
                folder_id,
                owner_id,
                sort_order: 0, // 0 = by name
                fetch_folders: true,
                fetch_items: true,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `CreateInventoryFolder` reliably (a new folder `folder_id` named
    /// `name` of `folder_type` under `parent_id`). The simulator sends no reply.
    pub(crate) fn send_create_inventory_folder(
        &mut self,
        folder_id: Uuid,
        parent_id: Uuid,
        folder_type: i8,
        name: &str,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::CreateInventoryFolder(CreateInventoryFolder {
            agent_data: CreateInventoryFolderAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            folder_data: CreateInventoryFolderFolderDataBlock {
                folder_id,
                parent_id,
                r#type: folder_type,
                name: with_nul(name),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `UpdateInventoryFolder` reliably (rename / re-type / re-parent
    /// the existing folder `folder_id`).
    pub(crate) fn send_update_inventory_folder(
        &mut self,
        folder_id: Uuid,
        parent_id: Uuid,
        folder_type: i8,
        name: &str,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::UpdateInventoryFolder(UpdateInventoryFolder {
            agent_data: UpdateInventoryFolderAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            folder_data: vec![UpdateInventoryFolderFolderDataBlock {
                folder_id,
                parent_id,
                r#type: folder_type,
                name: with_nul(name),
            }],
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `MoveInventoryFolder` reliably (re-parent each `(folder, parent)`
    /// pair). `stamp` asks the simulator to re-timestamp the moved children.
    pub(crate) fn send_move_inventory_folders(
        &mut self,
        moves: &[(Uuid, Uuid)],
        stamp: bool,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::MoveInventoryFolder(MoveInventoryFolder {
            agent_data: MoveInventoryFolderAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
                stamp,
            },
            inventory_data: moves
                .iter()
                .map(
                    |&(folder_id, parent_id)| MoveInventoryFolderInventoryDataBlock {
                        folder_id,
                        parent_id,
                    },
                )
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `RemoveInventoryFolder` reliably (delete each folder, moving it to
    /// the trash on the server).
    pub(crate) fn send_remove_inventory_folders(
        &mut self,
        folder_ids: &[Uuid],
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::RemoveInventoryFolder(RemoveInventoryFolder {
            agent_data: RemoveInventoryFolderAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            folder_data: folder_ids
                .iter()
                .map(|&folder_id| RemoveInventoryFolderFolderDataBlock { folder_id })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `CreateInventoryItem` reliably. The simulator answers with an
    /// `UpdateCreateInventoryItem` echoing `callback_id`
    /// ([`Event::InventoryItemCreated`]).
    pub(crate) fn send_create_inventory_item(
        &mut self,
        new: &NewInventoryItem,
        callback_id: InventoryCallbackId,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::CreateInventoryItem(CreateInventoryItem {
            agent_data: CreateInventoryItemAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            inventory_block: CreateInventoryItemInventoryBlockBlock {
                callback_id: callback_id.get(),
                folder_id: new.folder_id.uuid(),
                transaction_id: new.transaction_id,
                next_owner_mask: new.next_owner_mask,
                r#type: i8::try_from(new.asset_type.to_code()).unwrap_or(-1),
                inv_type: i8::try_from(new.inv_type.to_code()).unwrap_or(-1),
                wearable_type: new.wearable_type.to_code(),
                name: with_nul(&new.name),
                description: with_nul(&new.description),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `CreateInventoryItem` reliably for a **new script** item, with the
    /// language `subtype` byte packed into the `WearableType` field (as the viewer
    /// does — `SST_LSL = 0` / `SST_LUA = 1`). The transaction id is nil, so the
    /// simulator fills the item with its default script body (selecting the LSL or
    /// Lua default from the subtype). The reply is an `UpdateCreateInventoryItem`
    /// echoing `callback_id` ([`Event::InventoryItemCreated`]).
    #[expect(
        clippy::too_many_arguments,
        reason = "mirrors the flat CreateInventoryItem wire block fields"
    )]
    pub(crate) fn send_create_script_item(
        &mut self,
        folder_id: InventoryFolderKey,
        name: &str,
        description: &str,
        next_owner_mask: u32,
        subtype: u8,
        callback_id: InventoryCallbackId,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::CreateInventoryItem(CreateInventoryItem {
            agent_data: CreateInventoryItemAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            inventory_block: CreateInventoryItemInventoryBlockBlock {
                callback_id: callback_id.get(),
                folder_id: folder_id.uuid(),
                transaction_id: Uuid::nil(),
                next_owner_mask,
                r#type: i8::try_from(AssetType::ScriptText.to_code()).unwrap_or(-1),
                inv_type: i8::try_from(InventoryType::Script.to_code()).unwrap_or(-1),
                wearable_type: subtype,
                name: with_nul(name),
                description: with_nul(description),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `LinkInventoryItem` reliably (create a link to an existing item
    /// or folder). The simulator answers with an `UpdateCreateInventoryItem`
    /// echoing `callback_id` ([`Event::InventoryItemCreated`]). The `TransactionID`
    /// field is always nil — a link has no backing asset.
    pub(crate) fn send_link_inventory_item(
        &mut self,
        new: &NewInventoryLink,
        callback_id: InventoryCallbackId,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::LinkInventoryItem(LinkInventoryItem {
            agent_data: LinkInventoryItemAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            inventory_block: LinkInventoryItemInventoryBlockBlock {
                callback_id: callback_id.get(),
                folder_id: new.folder_id.uuid(),
                transaction_id: Uuid::nil(),
                old_item_id: new.linked_id.uuid(),
                r#type: i8::try_from(new.link_type.to_code()).unwrap_or(-1),
                inv_type: i8::try_from(new.inv_type.to_code()).unwrap_or(-1),
                name: with_nul(&new.name),
                description: with_nul(&new.description),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `UpdateInventoryItem` reliably (rewrite the metadata /
    /// permissions of `item`). A non-nil `transaction_id` associates a freshly
    /// uploaded asset with the item.
    pub(crate) fn send_update_inventory_item(
        &mut self,
        item: &InventoryItem,
        transaction_id: Uuid,
        callback_id: InventoryCallbackId,
        now: Instant,
    ) -> Result<(), WireError> {
        let (owner_id, group_id) = crate::types::object_owner_to_wire(item.owner, item.group);
        let message = AnyMessage::UpdateInventoryItem(UpdateInventoryItem {
            agent_data: UpdateInventoryItemAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
                transaction_id,
            },
            inventory_data: vec![UpdateInventoryItemInventoryDataBlock {
                item_id: item.item_id.uuid(),
                folder_id: item.folder_id.uuid(),
                callback_id: callback_id.get(),
                creator_id: item.creator_id.uuid(),
                owner_id,
                group_id,
                base_mask: item.permissions.base.bits(),
                owner_mask: item.permissions.owner.bits(),
                group_mask: item.permissions.group.bits(),
                everyone_mask: item.permissions.everyone.bits(),
                next_owner_mask: item.permissions.next_owner.bits(),
                group_owned: item.owner.is_group(),
                transaction_id,
                r#type: item.item_type,
                inv_type: item.inv_type,
                flags: item.flags,
                sale_type: item.sale_type,
                sale_price: crate::types::linden_price_to_wire(
                    "SalePrice",
                    item.sale_price.as_ref(),
                )?,
                name: with_nul(&item.name),
                description: with_nul(&item.description),
                creation_date: item.creation_date,
                crc: inventory_item_crc(item, item.folder_id.uuid())?,
            }],
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `MoveInventoryItem` reliably (re-parent each `(item, folder,
    /// new_name)`; an empty `new_name` keeps the current name). `stamp` asks the
    /// simulator to re-timestamp.
    pub(crate) fn send_move_inventory_items(
        &mut self,
        moves: &[(Uuid, Uuid, String)],
        stamp: bool,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::MoveInventoryItem(MoveInventoryItem {
            agent_data: MoveInventoryItemAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
                stamp,
            },
            inventory_data: moves
                .iter()
                .map(
                    |(item_id, folder_id, new_name)| MoveInventoryItemInventoryDataBlock {
                        item_id: *item_id,
                        folder_id: *folder_id,
                        new_name: with_nul(new_name),
                    },
                )
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `CopyInventoryItem` reliably (copy `old_item_id` owned by
    /// `old_agent_id` into `new_folder_id`). The simulator answers with a
    /// `BulkUpdateInventory` for the new item.
    pub(crate) fn send_copy_inventory_item(
        &mut self,
        old_agent_id: AgentKey,
        old_item_id: Uuid,
        new_folder_id: Uuid,
        new_name: &str,
        callback_id: InventoryCallbackId,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::CopyInventoryItem(CopyInventoryItem {
            agent_data: CopyInventoryItemAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            inventory_data: vec![CopyInventoryItemInventoryDataBlock {
                callback_id: callback_id.get(),
                old_agent_id: old_agent_id.uuid(),
                old_item_id,
                new_folder_id,
                new_name: with_nul(new_name),
            }],
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `RemoveInventoryItem` reliably (delete each item).
    pub(crate) fn send_remove_inventory_items(
        &mut self,
        item_ids: &[Uuid],
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::RemoveInventoryItem(RemoveInventoryItem {
            agent_data: RemoveInventoryItemAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            inventory_data: item_ids
                .iter()
                .map(|&item_id| RemoveInventoryItemInventoryDataBlock { item_id })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `ChangeInventoryItemFlags` reliably (rewrite an item's flags).
    pub(crate) fn send_change_inventory_item_flags(
        &mut self,
        item_id: Uuid,
        flags: u32,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ChangeInventoryItemFlags(ChangeInventoryItemFlags {
            agent_data: ChangeInventoryItemFlagsAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            inventory_data: vec![ChangeInventoryItemFlagsInventoryDataBlock { item_id, flags }],
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `PurgeInventoryDescendents` reliably (empty a folder's contents,
    /// e.g. the Trash).
    pub(crate) fn send_purge_inventory_descendents(
        &mut self,
        folder_id: Uuid,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::PurgeInventoryDescendents(PurgeInventoryDescendents {
            agent_data: PurgeInventoryDescendentsAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            inventory_data: PurgeInventoryDescendentsInventoryDataBlock { folder_id },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `RemoveInventoryObjects` reliably (delete a mixed set of folders
    /// and items in one message).
    pub(crate) fn send_remove_inventory_objects(
        &mut self,
        folder_ids: &[Uuid],
        item_ids: &[Uuid],
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::RemoveInventoryObjects(RemoveInventoryObjects {
            agent_data: RemoveInventoryObjectsAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            folder_data: folder_ids
                .iter()
                .map(|&folder_id| RemoveInventoryObjectsFolderDataBlock { folder_id })
                .collect(),
            item_data: item_ids
                .iter()
                .map(|&item_id| RemoveInventoryObjectsItemDataBlock { item_id })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `TeleportLocationRequest` reliably.
    pub(crate) fn send_teleport_location_request(
        &mut self,
        region_handle: u64,
        position: Vector,
        look_at: Vector,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::TeleportLocationRequest(TeleportLocationRequest {
            agent_data: TeleportLocationRequestAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            info: TeleportLocationRequestInfoBlock {
                region_handle,
                position,
                look_at,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `TeleportLandmarkRequest` reliably. `landmark` is the landmark
    /// asset's id, or [`Uuid::nil`] (the wire encoding of `None`) to teleport to
    /// the agent's home location.
    pub(crate) fn send_teleport_landmark_request(
        &mut self,
        landmark: Option<AssetKey>,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::TeleportLandmarkRequest(TeleportLandmarkRequest {
            info: TeleportLandmarkRequestInfoBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
                landmark_id: landmark.map_or_else(Uuid::nil, |id| id.uuid()),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `TeleportCancel` reliably (abort an in-progress teleport).
    pub(crate) fn send_teleport_cancel(&mut self, now: Instant) -> Result<(), WireError> {
        let message = AnyMessage::TeleportCancel(TeleportCancel {
            info: TeleportCancelInfoBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `SetStartLocationRequest` reliably: records `position` /
    /// `look_at` (region-local) as the agent's `slot` start location. `SimName`
    /// is always sent empty — the simulator fills in the current region's name.
    pub(crate) fn send_set_start_location_request(
        &mut self,
        slot: StartLocationSlot,
        position: Vector,
        look_at: Vector,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::SetStartLocationRequest(SetStartLocationRequest {
            agent_data: SetStartLocationRequestAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            start_location_data: SetStartLocationRequestStartLocationDataBlock {
                sim_name: with_nul(""),
                location_id: slot.to_code(),
                location_pos: position,
                location_look_at: look_at,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `AgentDataUpdateRequest` reliably (poll for a fresh
    /// `AgentDataUpdate` without changing any agent data).
    pub(crate) fn send_agent_data_update_request(&mut self, now: Instant) -> Result<(), WireError> {
        let message = AnyMessage::AgentDataUpdateRequest(AgentDataUpdateRequest {
            agent_data: AgentDataUpdateRequestAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `AgentQuitCopy` reliably: quits the session but leaves the
    /// agent's in-world objects behind. The `FuseBlock` carries this circuit's
    /// own code.
    pub(crate) fn send_agent_quit_copy(&mut self, now: Instant) -> Result<(), WireError> {
        let message = AnyMessage::AgentQuitCopy(AgentQuitCopy {
            agent_data: AgentQuitCopyAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            fuse_block: AgentQuitCopyFuseBlockBlock {
                viewer_circuit_code: self.code.get(),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `VelocityInterpolateOn` reliably (enable simulator-side velocity
    /// interpolation of object motion).
    pub(crate) fn send_velocity_interpolate_on(&mut self, now: Instant) -> Result<(), WireError> {
        let message = AnyMessage::VelocityInterpolateOn(VelocityInterpolateOn {
            agent_data: VelocityInterpolateOnAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `VelocityInterpolateOff` reliably (disable simulator-side
    /// velocity interpolation of object motion).
    pub(crate) fn send_velocity_interpolate_off(&mut self, now: Instant) -> Result<(), WireError> {
        let message = AnyMessage::VelocityInterpolateOff(VelocityInterpolateOff {
            agent_data: VelocityInterpolateOffAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `UserInfoRequest` reliably (poll for the agent's own account
    /// contact preferences; the reply arrives as `UserInfoReply`).
    pub(crate) fn send_user_info_request(&mut self, now: Instant) -> Result<(), WireError> {
        let message = AnyMessage::UserInfoRequest(UserInfoRequest {
            agent_data: UserInfoRequestAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `UpdateUserInfo` reliably: writes the agent's IM-via-email and
    /// directory-visibility preferences. The email address itself is not
    /// settable over this message (the wire block carries no email field).
    pub(crate) fn send_update_user_info(
        &mut self,
        im_via_email: bool,
        directory_visibility: &str,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::UpdateUserInfo(UpdateUserInfo {
            agent_data: UpdateUserInfoAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            user_data: UpdateUserInfoUserDataBlock {
                im_via_e_mail: im_via_email,
                directory_visibility: with_nul(directory_visibility),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `SoundTrigger` unreliably: plays a one-shot sound at `position`
    /// (region-local to `handle`) with the given linear `gain`. The owner /
    /// object / parent ids are left nil — the simulator fills them in for a
    /// viewer-originated trigger. Sent unreliably, as the reference viewer does
    /// (sound triggers are best-effort).
    pub(crate) fn send_sound_trigger(
        &mut self,
        sound: AssetKey,
        gain: f32,
        handle: u64,
        position: Vector,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::SoundTrigger(SoundTrigger {
            sound_data: SoundTriggerSoundDataBlock {
                sound_id: sound.uuid(),
                owner_id: Uuid::nil(),
                object_id: Uuid::nil(),
                parent_id: Uuid::nil(),
                handle,
                position,
                gain,
            },
        });
        self.send(&message, Reliability::Unreliable, now)
    }

    /// Queues a `RequestGodlikePowers` reliably: asks the simulator to grant
    /// (`godlike = true`) or drop (`false`) god powers for this agent. The
    /// `Token` is packed nil, exactly as the reference viewer does (the
    /// simulator fills it in).
    pub(crate) fn send_request_godlike_powers(
        &mut self,
        godlike: bool,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::RequestGodlikePowers(RequestGodlikePowers {
            agent_data: RequestGodlikePowersAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            request_block: RequestGodlikePowersRequestBlockBlock {
                godlike,
                token: Uuid::nil(),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `EjectUser` reliably: removes `target` from the agent's land,
    /// optionally banning them (per `flags`).
    pub(crate) fn send_eject_user(
        &mut self,
        target: Uuid,
        flags: u32,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::EjectUser(EjectUser {
            agent_data: EjectUserAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            data: EjectUserDataBlock {
                target_id: target,
                flags,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `FreezeUser` reliably: freezes or unfreezes `target` on the
    /// agent's land (per `flags`).
    pub(crate) fn send_freeze_user(
        &mut self,
        target: Uuid,
        flags: u32,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::FreezeUser(FreezeUser {
            agent_data: FreezeUserAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            data: FreezeUserDataBlock {
                target_id: target,
                flags,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `SimWideDeletes` reliably: deletes (or returns) the objects
    /// `owner` has across the region, filtered by `flags`. Needs estate/god
    /// rights.
    pub(crate) fn send_sim_wide_deletes(
        &mut self,
        owner: Uuid,
        flags: u32,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::SimWideDeletes(SimWideDeletes {
            agent_data: SimWideDeletesAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            data_block: SimWideDeletesDataBlockBlock {
                target_id: owner,
                flags,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `GodUpdateRegionInfo` reliably: pushes the god-tools region
    /// parameters in `update`. The legacy 32-bit `RegionFlags` block is the low
    /// 32 bits of the extended flags (the reference viewer truncates the same
    /// way); the full 64-bit value goes in `RegionInfo2`. Needs grid-god rights.
    pub(crate) fn send_god_update_region_info(
        &mut self,
        update: &GodRegionUpdate,
        now: Instant,
    ) -> Result<(), WireError> {
        // The legacy field is the low 32 bits of the extended flags.
        let legacy_flags =
            u32::try_from(update.region_flags & u64::from(u32::MAX)).unwrap_or(u32::MAX);
        // The redirect grid coordinates are signed on the wire (`0` for none);
        // region indices never exceed `i32::MAX` in practice.
        let redirect_grid_x = i32::try_from(update.redirect_grid.x()).unwrap_or(i32::MAX);
        let redirect_grid_y = i32::try_from(update.redirect_grid.y()).unwrap_or(i32::MAX);
        let message = AnyMessage::GodUpdateRegionInfo(GodUpdateRegionInfo {
            agent_data: GodUpdateRegionInfoAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            region_info: GodUpdateRegionInfoRegionInfoBlock {
                sim_name: with_nul(update.sim_name.as_ref()),
                estate_id: update.estate_id,
                parent_estate_id: update.parent_estate_id,
                region_flags: legacy_flags,
                billable_factor: update.billable_factor,
                price_per_meter: update.price_per_meter,
                redirect_grid_x,
                redirect_grid_y,
            },
            region_info2: vec![GodUpdateRegionInfoRegionInfo2Block {
                region_flags_extended: update.region_flags,
            }],
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `ParcelGodForceOwner` reliably: force-reassigns the parcel
    /// `local_id` to `owner`. Needs grid-god rights.
    pub(crate) fn send_parcel_god_force_owner(
        &mut self,
        local_id: RegionLocalParcelId,
        owner: Uuid,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ParcelGodForceOwner(ParcelGodForceOwner {
            agent_data: ParcelGodForceOwnerAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            data: ParcelGodForceOwnerDataBlock {
                owner_id: owner,
                local_id: local_id.0,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `ParcelGodMarkAsContent` reliably: marks the parcel `local_id`
    /// (and its content) as owned by the governor/maintenance account. Needs
    /// grid-god rights.
    pub(crate) fn send_parcel_god_mark_as_content(
        &mut self,
        local_id: RegionLocalParcelId,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ParcelGodMarkAsContent(ParcelGodMarkAsContent {
            agent_data: ParcelGodMarkAsContentAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            parcel_data: ParcelGodMarkAsContentParcelDataBlock {
                local_id: local_id.0,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `EventGodDelete` reliably: deletes the events-directory listing
    /// `event_id` and re-runs the search carried in the `query_*` fields so the
    /// simulator returns the refreshed result page. Needs grid-god rights.
    pub(crate) fn send_event_god_delete(
        &mut self,
        event_id: u32,
        query_id: Uuid,
        query_text: &str,
        flags: DirFindFlags,
        query_start: i32,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::EventGodDelete(EventGodDelete {
            agent_data: EventGodDeleteAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            event_data: EventGodDeleteEventDataBlock { event_id },
            query_data: EventGodDeleteQueryDataBlock {
                query_id,
                query_text: with_nul(query_text),
                query_flags: flags.bits(),
                query_start,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `StateSave` reliably: saves the region (world) state to
    /// `filename` (an empty string lets the simulator pick the autosave name,
    /// as the reference viewer does). Needs grid-god rights.
    pub(crate) fn send_state_save(
        &mut self,
        filename: &str,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::StateSave(StateSave {
            agent_data: StateSaveAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            data_block: StateSaveDataBlockBlock {
                filename: with_nul(filename),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `ViewerStartAuction` reliably: starts a land auction on the
    /// parcel `local_id`, optionally advertised by the `snapshot` texture (nil
    /// for none). Needs grid-god rights.
    pub(crate) fn send_viewer_start_auction(
        &mut self,
        local_id: RegionLocalParcelId,
        snapshot: Option<TextureKey>,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ViewerStartAuction(ViewerStartAuction {
            agent_data: ViewerStartAuctionAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            parcel_data: ViewerStartAuctionParcelDataBlock {
                local_id: local_id.0,
                snapshot_id: snapshot.map_or_else(Uuid::nil, |texture| texture.uuid()),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `LogoutRequest` reliably.
    pub(crate) fn send_logout_request(&mut self, now: Instant) -> Result<(), WireError> {
        let message = AnyMessage::LogoutRequest(LogoutRequest {
            agent_data: LogoutRequestAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `RequestRegionInfo` reliably.
    pub(crate) fn send_request_region_info(&mut self, now: Instant) -> Result<(), WireError> {
        let message = AnyMessage::RequestRegionInfo(RequestRegionInfo {
            agent_data: RequestRegionInfoAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `MoneyBalanceRequest` reliably. The transaction id is nil: a
    /// plain balance poll does not need to correlate a specific transaction.
    pub(crate) fn send_money_balance_request(&mut self, now: Instant) -> Result<(), WireError> {
        let message = AnyMessage::MoneyBalanceRequest(MoneyBalanceRequest {
            agent_data: MoneyBalanceRequestAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            money_data: MoneyBalanceRequestMoneyDataBlock {
                transaction_id: Uuid::nil(),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `EconomyDataRequest` reliably (an empty message).
    pub(crate) fn send_economy_data_request(&mut self, now: Instant) -> Result<(), WireError> {
        let message = AnyMessage::EconomyDataRequest(EconomyDataRequest {});
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `MoneyTransferRequest` reliably: pay `amount` L$ to `dest` with
    /// the given transaction type and description. The source is this agent.
    pub(crate) fn send_money_transfer(
        &mut self,
        dest: Uuid,
        amount: i32,
        transaction_type: i32,
        description: &str,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::MoneyTransferRequest(MoneyTransferRequest {
            agent_data: MoneyTransferRequestAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            money_data: MoneyTransferRequestMoneyDataBlock {
                source_id: self.agent_id.uuid(),
                dest_id: dest,
                // Flags and the aggregate-permission hints are unused for a plain
                // avatar/object payment; the simulator ignores them.
                flags: 0,
                amount,
                aggregate_perm_next_owner: 0,
                aggregate_perm_inventory: 0,
                transaction_type,
                description: with_nul(description),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `ParcelPropertiesRequest` reliably for the given metre rectangle.
    pub(crate) fn send_parcel_properties_request(
        &mut self,
        west: f32,
        south: f32,
        east: f32,
        north: f32,
        sequence_id: i32,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ParcelPropertiesRequest(ParcelPropertiesRequest {
            agent_data: ParcelPropertiesRequestAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            parcel_data: ParcelPropertiesRequestParcelDataBlock {
                sequence_id,
                west,
                south,
                east,
                north,
                snap_selection: false,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `ParcelPropertiesUpdate` reliably (edit a parcel's settings).
    pub(crate) fn send_parcel_properties_update(
        &mut self,
        update: &ParcelUpdate,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ParcelPropertiesUpdate(ParcelPropertiesUpdate {
            agent_data: ParcelPropertiesUpdateAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            parcel_data: ParcelPropertiesUpdateParcelDataBlock {
                local_id: update.local_id.0,
                // The message-level flag the reference viewer sends (0x01).
                flags: 0x1,
                parcel_flags: update.parcel_flags.bits(),
                sale_price: crate::types::linden_price_to_wire(
                    "SalePrice",
                    update.sale_price.as_ref(),
                )?,
                name: with_nul(&update.name),
                desc: with_nul(&update.description),
                music_url: with_nul(&sl_wire::optional_url_to_wire(update.music_url.as_ref())),
                media_url: with_nul(&sl_wire::optional_url_to_wire(update.media_url.as_ref())),
                media_id: update.media_id.map_or_else(Uuid::nil, |m| m.uuid()),
                media_auto_scale: u8::from(update.media_auto_scale),
                group_id: update.group_id.map_or_else(Uuid::nil, |g| g.uuid()),
                pass_price: crate::types::linden_to_wire("PassPrice", &update.pass_price)?,
                pass_hours: update.pass_hours,
                category: update.category.to_u8(),
                auth_buyer_id: update.auth_buyer_id.map_or_else(Uuid::nil, |a| a.uuid()),
                snapshot_id: update.snapshot_id.map_or_else(Uuid::nil, |s| s.uuid()),
                user_location: Vector {
                    x: update.user_location.x(),
                    y: update.user_location.y(),
                    z: update.user_location.z(),
                },
                user_look_at: Vector {
                    x: update.user_look_at.x(),
                    y: update.user_look_at.y(),
                    z: update.user_look_at.z(),
                },
                landing_type: update.landing_type,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `ParcelAccessListRequest` reliably (fetch the allow or ban list
    /// selected by `flags`). The reply is a `ParcelAccessListReply`.
    pub(crate) fn send_parcel_access_list_request(
        &mut self,
        local_id: RegionLocalParcelId,
        flags: u32,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ParcelAccessListRequest(ParcelAccessListRequest {
            agent_data: ParcelAccessListRequestAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            data: ParcelAccessListRequestDataBlock {
                sequence_id: 0,
                flags,
                local_id: local_id.0,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `ParcelAccessListUpdate` reliably (replace the allow or ban list
    /// selected by `flags`). An empty list clears it (sent as one empty entry, as
    /// the reference viewer does).
    ///
    /// `transaction_id` groups the packets of one logical update and, on the
    /// reference simulator, triggers a clear-before-add of the existing entries
    /// for `flags` when it differs from the previous update's id — so a caller
    /// that reuses a stale (or nil) id ends up *appending* to the list instead of
    /// replacing it. The runtime therefore mints a fresh id per update.
    pub(crate) fn send_parcel_access_list_update(
        &mut self,
        local_id: RegionLocalParcelId,
        flags: u32,
        entries: &[ParcelAccessEntry],
        transaction_id: Uuid,
        now: Instant,
    ) -> Result<(), WireError> {
        let list = if entries.is_empty() {
            vec![ParcelAccessListUpdateListBlock {
                id: Uuid::nil(),
                time: 0,
                flags: 0,
            }]
        } else {
            entries
                .iter()
                .map(|entry| ParcelAccessListUpdateListBlock {
                    id: entry.id,
                    time: entry.time,
                    flags: flags | entry.flags.0,
                })
                .collect()
        };
        let message = AnyMessage::ParcelAccessListUpdate(ParcelAccessListUpdate {
            agent_data: ParcelAccessListUpdateAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            data: ParcelAccessListUpdateDataBlock {
                flags,
                local_id: local_id.0,
                transaction_id,
                sequence_id: 1,
                sections: 1,
            },
            list,
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `ParcelDwellRequest` reliably. The reply is a `ParcelDwellReply`.
    pub(crate) fn send_parcel_dwell_request(
        &mut self,
        local_id: RegionLocalParcelId,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ParcelDwellRequest(ParcelDwellRequest {
            agent_data: ParcelDwellRequestAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            // The simulator fills in parcel_id from local_id.
            data: ParcelDwellRequestDataBlock {
                local_id: local_id.0,
                parcel_id: Uuid::nil(),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `ParcelBuy` reliably (purchase the parcel).
    pub(crate) fn send_parcel_buy(
        &mut self,
        local_id: RegionLocalParcelId,
        price: i32,
        area: i32,
        group_id: Option<GroupKey>,
        is_group_owned: bool,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ParcelBuy(ParcelBuy {
            agent_data: ParcelBuyAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            data: ParcelBuyDataBlock {
                group_id: group_id.map_or_else(Uuid::nil, |g| g.uuid()),
                is_group_owned,
                remove_contribution: false,
                local_id: local_id.0,
                r#final: true,
            },
            parcel_data: ParcelBuyParcelDataBlock { price, area },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `ParcelReturnObjects` reliably (return objects on the parcel
    /// matching `return_type`, optionally scoped to the given owner/task ids).
    pub(crate) fn send_parcel_return_objects(
        &mut self,
        local_id: RegionLocalParcelId,
        return_type: u32,
        owner_ids: &[Uuid],
        task_ids: &[ObjectKey],
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ParcelReturnObjects(ParcelReturnObjects {
            agent_data: ParcelReturnObjectsAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            parcel_data: ParcelReturnObjectsParcelDataBlock {
                local_id: local_id.0,
                return_type,
            },
            task_i_ds: task_ids
                .iter()
                .map(|id| ParcelReturnObjectsTaskIDsBlock { task_id: id.uuid() })
                .collect(),
            owner_i_ds: owner_ids
                .iter()
                .map(|id| ParcelReturnObjectsOwnerIDsBlock { owner_id: *id })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `ParcelSelectObjects` reliably (select the parcel objects matching
    /// `return_type`, or the explicit `object_ids` when using the list type).
    pub(crate) fn send_parcel_select_objects(
        &mut self,
        local_id: RegionLocalParcelId,
        return_type: u32,
        object_ids: &[ObjectKey],
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ParcelSelectObjects(ParcelSelectObjects {
            agent_data: ParcelSelectObjectsAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            parcel_data: ParcelSelectObjectsParcelDataBlock {
                local_id: local_id.0,
                return_type,
            },
            return_i_ds: object_ids
                .iter()
                .map(|id| ParcelSelectObjectsReturnIDsBlock {
                    return_id: id.uuid(),
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `ParcelDeedToGroup` reliably (deed the parcel to `group_id`).
    pub(crate) fn send_parcel_deed_to_group(
        &mut self,
        local_id: RegionLocalParcelId,
        group_id: GroupKey,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ParcelDeedToGroup(ParcelDeedToGroup {
            agent_data: ParcelDeedToGroupAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            data: ParcelDeedToGroupDataBlock {
                group_id: group_id.uuid(),
                local_id: local_id.0,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `ParcelReclaim` reliably (reclaim the parcel to the estate).
    pub(crate) fn send_parcel_reclaim(
        &mut self,
        local_id: RegionLocalParcelId,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ParcelReclaim(ParcelReclaim {
            agent_data: ParcelReclaimAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            data: ParcelReclaimDataBlock {
                local_id: local_id.0,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `ParcelRelease` reliably (abandon the parcel back to the estate).
    pub(crate) fn send_parcel_release(
        &mut self,
        local_id: RegionLocalParcelId,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ParcelRelease(ParcelRelease {
            agent_data: ParcelReleaseAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            data: ParcelReleaseDataBlock {
                local_id: local_id.0,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `ParcelJoin` reliably (merge all owned, leased parcels within the
    /// metre rectangle into one).
    pub(crate) fn send_parcel_join(
        &mut self,
        west: f32,
        south: f32,
        east: f32,
        north: f32,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ParcelJoin(ParcelJoin {
            agent_data: ParcelJoinAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            parcel_data: ParcelJoinParcelDataBlock {
                west,
                south,
                east,
                north,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `ParcelDivide` reliably (chop the metre rectangle out of its
    /// parcel into a new parcel).
    pub(crate) fn send_parcel_divide(
        &mut self,
        west: f32,
        south: f32,
        east: f32,
        north: f32,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ParcelDivide(ParcelDivide {
            agent_data: ParcelDivideAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            parcel_data: ParcelDivideParcelDataBlock {
                west,
                south,
                east,
                north,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `ParcelObjectOwnersRequest` reliably (the per-owner object tally
    /// for the parcel).
    pub(crate) fn send_parcel_object_owners_request(
        &mut self,
        local_id: RegionLocalParcelId,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ParcelObjectOwnersRequest(ParcelObjectOwnersRequest {
            agent_data: ParcelObjectOwnersRequestAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            parcel_data: ParcelObjectOwnersRequestParcelDataBlock {
                local_id: local_id.0,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `ParcelBuyPass` reliably (purchase a temporary access pass to the
    /// parcel at its configured price).
    pub(crate) fn send_parcel_buy_pass(
        &mut self,
        local_id: RegionLocalParcelId,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ParcelBuyPass(ParcelBuyPass {
            agent_data: ParcelBuyPassAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            parcel_data: ParcelBuyPassParcelDataBlock {
                local_id: local_id.0,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `ParcelDisableObjects` reliably (stop the parcel objects matching
    /// `return_type`, or the explicit `task_ids` when using the list type).
    pub(crate) fn send_parcel_disable_objects(
        &mut self,
        local_id: RegionLocalParcelId,
        return_type: u32,
        owner_ids: &[Uuid],
        task_ids: &[ObjectKey],
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ParcelDisableObjects(ParcelDisableObjects {
            agent_data: ParcelDisableObjectsAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            parcel_data: ParcelDisableObjectsParcelDataBlock {
                local_id: local_id.0,
                return_type,
            },
            task_i_ds: task_ids
                .iter()
                .map(|id| ParcelDisableObjectsTaskIDsBlock { task_id: id.uuid() })
                .collect(),
            owner_i_ds: owner_ids
                .iter()
                .map(|id| ParcelDisableObjectsOwnerIDsBlock { owner_id: *id })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `ParcelInfoRequest` reliably (the basic listing for a grid-wide
    /// parcel id).
    pub(crate) fn send_parcel_info_request(
        &mut self,
        parcel_id: ParcelKey,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ParcelInfoRequest(ParcelInfoRequest {
            agent_data: ParcelInfoRequestAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            data: ParcelInfoRequestDataBlock {
                parcel_id: parcel_id.uuid(),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `LandStatRequest` reliably: a request for the region's (or a
    /// parcel's) top scripts or top colliders. `report_type`/`request_flags` are
    /// raw values; `filter` narrows the report by object/owner name (empty for
    /// none) and `parcel_local_id` scopes it to a parcel (`0` for the region).
    pub(crate) fn send_land_stat_request(
        &mut self,
        report_type: u32,
        request_flags: u32,
        filter: &str,
        parcel_local_id: RegionLocalParcelId,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::LandStatRequest(LandStatRequest {
            agent_data: LandStatRequestAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            request_data: LandStatRequestRequestDataBlock {
                report_type,
                request_flags,
                filter: with_nul(filter),
                parcel_local_id: parcel_local_id.0,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `EstateOwnerMessage` reliably with the given method and string
    /// parameters. An empty parameter list is sent as one empty block (matching
    /// the reference viewer). The invoice is nil — the simulator echoes it back.
    pub(crate) fn send_estate_owner_message(
        &mut self,
        method: &str,
        params: &[String],
        now: Instant,
    ) -> Result<(), WireError> {
        let param_list = if params.is_empty() {
            vec![EstateOwnerMessageParamListBlock {
                parameter: Vec::new(),
            }]
        } else {
            params
                .iter()
                .map(|param| EstateOwnerMessageParamListBlock {
                    parameter: with_nul(param),
                })
                .collect()
        };
        let message = AnyMessage::EstateOwnerMessage(EstateOwnerMessage {
            agent_data: EstateOwnerMessageAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
                transaction_id: Uuid::nil(),
            },
            method_data: EstateOwnerMessageMethodDataBlock {
                method: with_nul(method),
                invoice: Uuid::nil(),
            },
            param_list,
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `EstateCovenantRequest` reliably. The simulator replies with an
    /// `EstateCovenantReply` carrying the covenant notecard id and estate name.
    pub(crate) fn send_estate_covenant_request(&mut self, now: Instant) -> Result<(), WireError> {
        let message = AnyMessage::EstateCovenantRequest(EstateCovenantRequest {
            agent_data: EstateCovenantRequestAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `GodlikeMessage` reliably (a god-level estate/admin command).
    pub(crate) fn send_godlike_message(
        &mut self,
        method: &str,
        params: &[String],
        now: Instant,
    ) -> Result<(), WireError> {
        let param_list = if params.is_empty() {
            vec![GodlikeMessageParamListBlock {
                parameter: Vec::new(),
            }]
        } else {
            params
                .iter()
                .map(|param| GodlikeMessageParamListBlock {
                    parameter: with_nul(param),
                })
                .collect()
        };
        let message = AnyMessage::GodlikeMessage(GodlikeMessage {
            agent_data: GodlikeMessageAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
                transaction_id: Uuid::nil(),
            },
            method_data: GodlikeMessageMethodDataBlock {
                method: with_nul(method),
                invoice: Uuid::nil(),
            },
            param_list,
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `GodKickUser` reliably (god-level eject of `target`).
    pub(crate) fn send_god_kick_user(
        &mut self,
        target: Uuid,
        reason: &str,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::GodKickUser(GodKickUser {
            user_info: GodKickUserUserInfoBlock {
                god_id: self.agent_id.uuid(),
                god_session_id: self.session_id,
                agent_id: target,
                kick_flags: 0,
                reason: with_nul(reason),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `MapBlockRequest` reliably for a grid-coordinate rectangle.
    pub(crate) fn send_map_block_request(
        &mut self,
        min_x: u16,
        max_x: u16,
        min_y: u16,
        max_y: u16,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::MapBlockRequest(MapBlockRequest {
            agent_data: MapBlockRequestAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
                // Flags 0 selects the terrain map layer; estate/godlike unused.
                flags: 0,
                estate_id: 0,
                godlike: false,
            },
            position_data: MapBlockRequestPositionDataBlock {
                min_x,
                max_x,
                min_y,
                max_y,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `MapNameRequest` reliably (search regions by name). The reply is
    /// a `MapBlockReply`, the same as [`Circuit::send_map_block_request`].
    pub(crate) fn send_map_name_request(
        &mut self,
        name: &str,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::MapNameRequest(MapNameRequest {
            agent_data: MapNameRequestAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
                // The viewer's map-layer flag (2); estate/godlike filled by the sim.
                flags: MapRequestFlags::LAYER,
                estate_id: 0,
                godlike: false,
            },
            name_data: MapNameRequestNameDataBlock {
                name: with_nul(name),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `MapItemRequest` reliably for the given item type. `region_handle`
    /// of 0 targets the current region; otherwise it targets that region.
    pub(crate) fn send_map_item_request(
        &mut self,
        item_type: u32,
        region_handle: u64,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::MapItemRequest(MapItemRequest {
            agent_data: MapItemRequestAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
                flags: MapRequestFlags::LAYER,
                estate_id: 0,
                godlike: false,
            },
            request_data: MapItemRequestRequestDataBlock {
                item_type,
                region_handle,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `MapLayerRequest` reliably. The reply is one or more
    /// `MapLayerReply` blocks (decoded into [`Event::MapLayers`]) describing the
    /// world-map image tiles. `flags` selects the map layer (the viewer sends
    /// the map-layer flag); estate/godlike are filled by the sim.
    pub(crate) fn send_map_layer_request(&mut self, now: Instant) -> Result<(), WireError> {
        let message = AnyMessage::MapLayerRequest(MapLayerRequest {
            agent_data: MapLayerRequestAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
                flags: MapRequestFlags::LAYER,
                estate_id: 0,
                godlike: false,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `UserReport` reliably: an abuse / bug report filed over the
    /// legacy UDP path (the modern equivalent is the `SendUserReport`
    /// capability). Fire-and-forget; there is no reply.
    pub(crate) fn send_user_report(
        &mut self,
        report: &AbuseReport,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::UserReport(UserReport {
            agent_data: UserReportAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            report_data: UserReportReportDataBlock {
                report_type: report.report_type.to_u8(),
                category: report.category,
                position: report.position.clone(),
                check_flags: report.check_flags,
                screenshot_id: report.screenshot_id,
                object_id: report.object_id.uuid(),
                abuser_id: report.abuser_id,
                abuse_region_name: with_nul(&sl_wire::region_name_to_wire(
                    report.abuse_region_name.as_ref(),
                )),
                abuse_region_id: report.abuse_region_id,
                summary: with_nul(&report.summary),
                details: with_nul(&report.details),
                version_string: with_nul(&report.version_string),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `SendPostcard` reliably: emails the referenced snapshot asset.
    /// Fire-and-forget; there is no reply.
    pub(crate) fn send_postcard(
        &mut self,
        postcard: &Postcard,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::SendPostcard(SendPostcard {
            agent_data: SendPostcardAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
                asset_id: postcard.asset_id,
                pos_global: [
                    postcard.pos_global.x(),
                    postcard.pos_global.y(),
                    postcard.pos_global.z(),
                ],
                to: with_nul(&postcard.to),
                from: with_nul(&postcard.from),
                name: with_nul(&postcard.name),
                subject: with_nul(&postcard.subject),
                msg: with_nul(&postcard.message),
                allow_publish: postcard.allow_publish,
                mature_publish: postcard.mature_publish,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `RequestMultipleObjects` reliably, asking the simulator to (re)send
    /// the full `ObjectUpdate` for each local id (cache-miss type "full" = 0).
    pub(crate) fn send_request_multiple_objects(
        &mut self,
        local_ids: &[RegionLocalObjectId],
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::RequestMultipleObjects(RequestMultipleObjects {
            agent_data: RequestMultipleObjectsAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            object_data: local_ids
                .iter()
                .map(|id| RequestMultipleObjectsObjectDataBlock {
                    cache_miss_type: 0,
                    id: id.0,
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectSelect` reliably for the given local ids. Selecting an
    /// object makes the simulator send its `ObjectProperties`.
    pub(crate) fn send_object_select(
        &mut self,
        local_ids: &[RegionLocalObjectId],
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectSelect(ObjectSelect {
            agent_data: ObjectSelectAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            object_data: local_ids
                .iter()
                .map(|id| ObjectSelectObjectDataBlock {
                    object_local_id: id.0,
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectDeselect` reliably for the given local ids.
    pub(crate) fn send_object_deselect(
        &mut self,
        local_ids: &[RegionLocalObjectId],
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectDeselect(ObjectDeselect {
            agent_data: ObjectDeselectAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            object_data: local_ids
                .iter()
                .map(|id| ObjectDeselectObjectDataBlock {
                    object_local_id: id.0,
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    // Object interaction & editing (#17) -----------------------------------

    /// Queues an `ObjectGrab` reliably (the start of a touch/click) for `local_id`
    /// with `grab_offset` and an empty surface-info list.
    pub(crate) fn send_object_grab(
        &mut self,
        local_id: RegionLocalObjectId,
        grab_offset: Vector,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectGrab(ObjectGrab {
            agent_data: ObjectGrabAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            object_data: ObjectGrabObjectDataBlock {
                local_id: local_id.0,
                grab_offset,
            },
            surface_info: Vec::new(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectGrabUpdate` reliably (a drag while grabbing) for the
    /// object `object_id`.
    pub(crate) fn send_object_grab_update(
        &mut self,
        object_id: ObjectKey,
        grab_offset_initial: Vector,
        grab_position: Vector,
        time_since_last: u32,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectGrabUpdate(ObjectGrabUpdate {
            agent_data: ObjectGrabUpdateAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            object_data: ObjectGrabUpdateObjectDataBlock {
                object_id: object_id.uuid(),
                grab_offset_initial,
                grab_position,
                time_since_last,
            },
            surface_info: Vec::new(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectDeGrab` reliably (the end of a touch/click) for `local_id`.
    pub(crate) fn send_object_degrab(
        &mut self,
        local_id: RegionLocalObjectId,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectDeGrab(ObjectDeGrab {
            agent_data: ObjectDeGrabAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            object_data: ObjectDeGrabObjectDataBlock {
                local_id: local_id.0,
            },
            surface_info: Vec::new(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectAdd` reliably to rez a new primitive from `shape`.
    pub(crate) fn send_object_add(
        &mut self,
        shape: &PrimShape,
        group_id: Option<GroupKey>,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectAdd(ObjectAdd {
            agent_data: ObjectAddAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
                group_id: group_id.map_or_else(Uuid::nil, |g| g.uuid()),
            },
            object_data: ObjectAddObjectDataBlock {
                p_code: shape.pcode,
                material: shape.material.to_code(),
                add_flags: shape.add_flags,
                path_curve: shape.path_curve,
                profile_curve: shape.profile_curve,
                path_begin: shape.path_begin,
                path_end: shape.path_end,
                path_scale_x: shape.path_scale_x,
                path_scale_y: shape.path_scale_y,
                path_shear_x: shape.path_shear_x,
                path_shear_y: shape.path_shear_y,
                path_twist: shape.path_twist,
                path_twist_begin: shape.path_twist_begin,
                path_radius_offset: shape.path_radius_offset,
                path_taper_x: shape.path_taper_x,
                path_taper_y: shape.path_taper_y,
                path_revolutions: shape.path_revolutions,
                path_skew: shape.path_skew,
                profile_begin: shape.profile_begin,
                profile_end: shape.profile_end,
                profile_hollow: shape.profile_hollow,
                // Rez exactly at `position`: skip the raycast and treat the ray
                // endpoint as the placement point (the viewer's headless rez path).
                bypass_raycast: 1,
                ray_start: shape.position.clone(),
                ray_end: shape.position.clone(),
                ray_target_id: Uuid::nil(),
                ray_end_is_intersection: 0,
                scale: shape.scale.clone(),
                rotation: shape.rotation.clone(),
                state: shape.state,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectDuplicate` reliably (copy `local_ids` by `offset`).
    pub(crate) fn send_object_duplicate(
        &mut self,
        local_ids: &[RegionLocalObjectId],
        offset: Vector,
        group_id: Option<GroupKey>,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectDuplicate(ObjectDuplicate {
            agent_data: ObjectDuplicateAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
                group_id: group_id.map_or_else(Uuid::nil, |g| g.uuid()),
            },
            shared_data: ObjectDuplicateSharedDataBlock {
                offset,
                duplicate_flags: 0,
            },
            object_data: local_ids
                .iter()
                .map(|id| ObjectDuplicateObjectDataBlock {
                    object_local_id: id.0,
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectDelete` reliably for `local_ids` (non-god, non-forced).
    pub(crate) fn send_object_delete(
        &mut self,
        local_ids: &[RegionLocalObjectId],
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectDelete(ObjectDelete {
            agent_data: ObjectDeleteAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
                force: false,
            },
            object_data: local_ids
                .iter()
                .map(|id| ObjectDeleteObjectDataBlock {
                    object_local_id: id.0,
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `DeRezObject` reliably (take/return/trash `local_ids`).
    pub(crate) fn send_derez_object(
        &mut self,
        local_ids: &[RegionLocalObjectId],
        destination: DeRezDestination,
        transaction_id: Uuid,
        group_id: Option<GroupKey>,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::DeRezObject(DeRezObject {
            agent_data: DeRezObjectAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            agent_block: DeRezObjectAgentBlockBlock {
                group_id: group_id.map_or_else(Uuid::nil, |g| g.uuid()),
                destination: destination.to_code(),
                destination_id: destination.destination_id(),
                transaction_id,
                // The whole selection fits in one packet.
                packet_count: 1,
                packet_number: 0,
            },
            object_data: local_ids
                .iter()
                .map(|id| DeRezObjectObjectDataBlock {
                    object_local_id: id.0,
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectName` reliably (rename `local_id`).
    pub(crate) fn send_object_name(
        &mut self,
        local_id: RegionLocalObjectId,
        name: &str,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectName(ObjectName {
            agent_data: ObjectNameAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            object_data: vec![ObjectNameObjectDataBlock {
                local_id: local_id.0,
                name: with_nul(name),
            }],
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectDescription` reliably (re-describe `local_id`).
    pub(crate) fn send_object_description(
        &mut self,
        local_id: RegionLocalObjectId,
        description: &str,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectDescription(ObjectDescription {
            agent_data: ObjectDescriptionAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            object_data: vec![ObjectDescriptionObjectDataBlock {
                local_id: local_id.0,
                description: with_nul(description),
            }],
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectClickAction` reliably (set the left-click behaviour).
    pub(crate) fn send_object_click_action(
        &mut self,
        local_id: RegionLocalObjectId,
        action: ClickAction,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectClickAction(ObjectClickAction {
            agent_data: ObjectClickActionAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            object_data: vec![ObjectClickActionObjectDataBlock {
                object_local_id: local_id.0,
                click_action: action.to_code(),
            }],
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectMaterial` reliably (set the physical material).
    pub(crate) fn send_object_material(
        &mut self,
        local_id: RegionLocalObjectId,
        material: Material,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectMaterial(ObjectMaterial {
            agent_data: ObjectMaterialAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            object_data: vec![ObjectMaterialObjectDataBlock {
                object_local_id: local_id.0,
                material: material.to_code(),
            }],
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectFlagUpdate` reliably (set physics/temporary/phantom).
    pub(crate) fn send_object_flag_update(
        &mut self,
        local_id: RegionLocalObjectId,
        flags: &ObjectFlagSettings,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectFlagUpdate(ObjectFlagUpdate {
            agent_data: ObjectFlagUpdateAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
                object_local_id: local_id.0,
                use_physics: flags.use_physics,
                is_temporary: flags.is_temporary,
                is_phantom: flags.is_phantom,
                casts_shadows: flags.casts_shadows,
            },
            // No extra-physics (shape-type/density/…) overrides.
            extra_physics: Vec::new(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectShape` reliably (set the path/profile geometry of
    /// `local_id`). The `shape` fields are the quantized wire values (see
    /// [`PrimShapeParams`]).
    pub(crate) fn send_object_shape(
        &mut self,
        local_id: RegionLocalObjectId,
        shape: &PrimShapeParams,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectShape(ObjectShape {
            agent_data: ObjectShapeAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            object_data: vec![ObjectShapeObjectDataBlock {
                object_local_id: local_id.0,
                path_curve: shape.path_curve,
                profile_curve: shape.profile_curve,
                path_begin: shape.path_begin,
                path_end: shape.path_end,
                path_scale_x: shape.path_scale_x,
                path_scale_y: shape.path_scale_y,
                path_shear_x: shape.path_shear_x,
                path_shear_y: shape.path_shear_y,
                path_twist: shape.path_twist,
                path_twist_begin: shape.path_twist_begin,
                path_radius_offset: shape.path_radius_offset,
                path_taper_x: shape.path_taper_x,
                path_taper_y: shape.path_taper_y,
                path_revolutions: shape.path_revolutions,
                path_skew: shape.path_skew,
                profile_begin: shape.profile_begin,
                profile_end: shape.profile_end,
                profile_hollow: shape.profile_hollow,
            }],
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectImage` reliably (set the per-face textures of `local_id`).
    /// `texture_entry` is packed to the wire `TextureEntry` blob; `media_url`
    /// carries the legacy parcel-media URL (empty when none).
    pub(crate) fn send_object_image(
        &mut self,
        local_id: RegionLocalObjectId,
        media_url: &str,
        texture_entry: &TextureEntry,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectImage(ObjectImage {
            agent_data: ObjectImageAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            object_data: vec![ObjectImageObjectDataBlock {
                object_local_id: local_id.0,
                media_url: with_nul(media_url),
                texture_entry: encode_texture_entry(texture_entry),
            }],
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectExtraParams` reliably, setting the complete extra-parameter
    /// state of `local_id` from `params` — one block per known subtype, in-use
    /// when `params` carries it (so absent parameters are cleared). See
    /// [`extra_param_message_blocks`].
    pub(crate) fn send_object_extra_params(
        &mut self,
        local_id: RegionLocalObjectId,
        params: &ObjectExtraParams,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectExtraParams(ObjectExtraParamsMessage {
            agent_data: ObjectExtraParamsAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            object_data: extra_param_message_blocks(params)
                .into_iter()
                .map(|block| ObjectExtraParamsObjectDataBlock {
                    object_local_id: local_id.0,
                    param_type: block.param_type,
                    param_in_use: block.in_use,
                    param_size: u32::try_from(block.data.len()).unwrap_or(u32::MAX),
                    param_data: block.data,
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectGroup` reliably (set the group `local_ids` are set to).
    pub(crate) fn send_object_group(
        &mut self,
        local_ids: &[RegionLocalObjectId],
        group_id: GroupKey,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectGroup(ObjectGroup {
            agent_data: ObjectGroupAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
                group_id: group_id.uuid(),
            },
            object_data: local_ids
                .iter()
                .map(|id| ObjectGroupObjectDataBlock {
                    object_local_id: id.0,
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectPermissions` reliably (set/clear the `mask` bits of
    /// `field`).
    pub(crate) fn send_object_permissions(
        &mut self,
        local_ids: &[RegionLocalObjectId],
        field: PermissionField,
        set: bool,
        mask: Permissions,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectPermissions(ObjectPermissions {
            agent_data: ObjectPermissionsAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            header_data: ObjectPermissionsHeaderDataBlock { r#override: false },
            object_data: local_ids
                .iter()
                .map(|id| ObjectPermissionsObjectDataBlock {
                    object_local_id: id.0,
                    field: field.to_code(),
                    set: u8::from(set),
                    mask: mask.bits(),
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectSaleInfo` reliably (set the sale type and price).
    pub(crate) fn send_object_sale_info(
        &mut self,
        local_id: RegionLocalObjectId,
        sale_type: SaleType,
        sale_price: Option<LindenAmount>,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectSaleInfo(ObjectSaleInfo {
            agent_data: ObjectSaleInfoAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            object_data: vec![ObjectSaleInfoObjectDataBlock {
                local_id: local_id.0,
                sale_type: sale_type.to_code(),
                sale_price: crate::types::linden_price_to_wire("SalePrice", sale_price.as_ref())?,
            }],
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectCategory` reliably (set the object's category code).
    pub(crate) fn send_object_category(
        &mut self,
        local_id: RegionLocalObjectId,
        category: u32,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectCategory(ObjectCategory {
            agent_data: ObjectCategoryAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            object_data: vec![ObjectCategoryObjectDataBlock {
                local_id: local_id.0,
                category,
            }],
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectIncludeInSearch` reliably (toggle search visibility).
    pub(crate) fn send_object_include_in_search(
        &mut self,
        local_id: RegionLocalObjectId,
        include: bool,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectIncludeInSearch(ObjectIncludeInSearch {
            agent_data: ObjectIncludeInSearchAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            object_data: vec![ObjectIncludeInSearchObjectDataBlock {
                object_local_id: local_id.0,
                include_in_search: include,
            }],
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectLink` reliably linking `local_ids` (the first id becomes
    /// the linkset root).
    pub(crate) fn send_object_link(
        &mut self,
        local_ids: &[RegionLocalObjectId],
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectLink(ObjectLink {
            agent_data: ObjectLinkAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            object_data: local_ids
                .iter()
                .map(|id| ObjectLinkObjectDataBlock {
                    object_local_id: id.0,
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectDelink` reliably unlinking `local_ids`.
    pub(crate) fn send_object_delink(
        &mut self,
        local_ids: &[RegionLocalObjectId],
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectDelink(ObjectDelink {
            agent_data: ObjectDelinkAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            object_data: local_ids
                .iter()
                .map(|id| ObjectDelinkObjectDataBlock {
                    object_local_id: id.0,
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectBuy` reliably (purchase `objects` into `category_id`).
    pub(crate) fn send_object_buy(
        &mut self,
        group_id: GroupKey,
        category_id: Uuid,
        objects: &[ObjectBuyItem],
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectBuy(ObjectBuy {
            agent_data: ObjectBuyAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
                group_id: group_id.uuid(),
                category_id,
            },
            object_data: objects
                .iter()
                .map(|item| {
                    Ok(ObjectBuyObjectDataBlock {
                        object_local_id: item.local_id.0,
                        sale_type: item.sale_type.to_code(),
                        sale_price: crate::types::linden_to_wire("SalePrice", &item.sale_price)?,
                    })
                })
                .collect::<Result<_, WireError>>()?,
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `BuyObjectInventory` reliably (buy `item_id` out of `object_id`).
    pub(crate) fn send_buy_object_inventory(
        &mut self,
        object_id: ObjectKey,
        item_id: Uuid,
        folder_id: Uuid,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::BuyObjectInventory(BuyObjectInventory {
            agent_data: BuyObjectInventoryAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            data: BuyObjectInventoryDataBlock {
                object_id: object_id.uuid(),
                item_id,
                folder_id,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `RequestPayPrice` reliably (ask `object_id` for its pay buttons).
    pub(crate) fn send_request_pay_price(
        &mut self,
        object_id: ObjectKey,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::RequestPayPrice(RequestPayPrice {
            object_data: RequestPayPriceObjectDataBlock {
                object_id: object_id.uuid(),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `RequestObjectPropertiesFamily` reliably (ask `object_id` for its
    /// condensed broadcast properties).
    pub(crate) fn send_request_object_properties_family(
        &mut self,
        request_flags: u32,
        object_id: ObjectKey,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::RequestObjectPropertiesFamily(RequestObjectPropertiesFamily {
            agent_data: RequestObjectPropertiesFamilyAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            object_data: RequestObjectPropertiesFamilyObjectDataBlock {
                request_flags,
                object_id: object_id.uuid(),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectSpinStart` reliably (begin spinning `object_id`).
    pub(crate) fn send_object_spin_start(
        &mut self,
        object_id: ObjectKey,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectSpinStart(ObjectSpinStart {
            agent_data: ObjectSpinStartAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            object_data: ObjectSpinStartObjectDataBlock {
                object_id: object_id.uuid(),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectSpinUpdate` reliably (the latest spin `rotation`).
    pub(crate) fn send_object_spin_update(
        &mut self,
        object_id: ObjectKey,
        rotation: Rotation,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectSpinUpdate(ObjectSpinUpdate {
            agent_data: ObjectSpinUpdateAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            object_data: ObjectSpinUpdateObjectDataBlock {
                object_id: object_id.uuid(),
                rotation,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectSpinStop` reliably (end spinning `object_id`).
    pub(crate) fn send_object_spin_stop(
        &mut self,
        object_id: ObjectKey,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectSpinStop(ObjectSpinStop {
            agent_data: ObjectSpinStopAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            object_data: ObjectSpinStopObjectDataBlock {
                object_id: object_id.uuid(),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `GetScriptRunning` reliably (query whether `item_id` inside the
    /// task `object_id` is running). This message carries no `AgentData` block.
    pub(crate) fn send_get_script_running(
        &mut self,
        object_id: ObjectKey,
        item_id: Uuid,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::GetScriptRunning(GetScriptRunning {
            script: GetScriptRunningScriptBlock {
                object_id: object_id.uuid(),
                item_id,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `SetScriptRunning` reliably (start or stop the script `item_id`
    /// inside the task `object_id`).
    pub(crate) fn send_set_script_running(
        &mut self,
        object_id: ObjectKey,
        item_id: Uuid,
        running: bool,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::SetScriptRunning(SetScriptRunning {
            agent_data: SetScriptRunningAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            script: SetScriptRunningScriptBlock {
                object_id: object_id.uuid(),
                item_id,
                running,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `ScriptReset` reliably (reset the script `item_id` inside the
    /// task `object_id`).
    pub(crate) fn send_script_reset(
        &mut self,
        object_id: ObjectKey,
        item_id: Uuid,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ScriptReset(ScriptReset {
            agent_data: ScriptResetAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            script: ScriptResetScriptBlock {
                object_id: object_id.uuid(),
                item_id,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectDuplicateOnRay` reliably (copy `local_ids`, dropping the
    /// copies against the surface the ray hits).
    #[expect(
        clippy::too_many_arguments,
        clippy::fn_params_excessive_bools,
        reason = "mirrors the ObjectDuplicateOnRay wire block one-to-one"
    )]
    pub(crate) fn send_object_duplicate_on_ray(
        &mut self,
        local_ids: &[RegionLocalObjectId],
        group_id: Option<GroupKey>,
        ray_start: Vector,
        ray_end: Vector,
        bypass_raycast: bool,
        ray_end_is_intersection: bool,
        copy_centers: bool,
        copy_rotates: bool,
        ray_target_id: Option<ObjectKey>,
        duplicate_flags: u32,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectDuplicateOnRay(ObjectDuplicateOnRay {
            agent_data: ObjectDuplicateOnRayAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
                group_id: group_id.map_or_else(Uuid::nil, |g| g.uuid()),
                ray_start,
                ray_end,
                bypass_raycast,
                ray_end_is_intersection,
                copy_centers,
                copy_rotates,
                ray_target_id: ray_target_id.map_or_else(Uuid::nil, |t| t.uuid()),
                duplicate_flags,
            },
            object_data: local_ids
                .iter()
                .map(|id| ObjectDuplicateOnRayObjectDataBlock {
                    object_local_id: id.0,
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `RezRestoreToWorld` reliably (restore `item` to its last place).
    pub(crate) fn send_rez_restore_to_world(
        &mut self,
        item: &RestoreItem,
        now: Instant,
    ) -> Result<(), WireError> {
        let (owner_id, group_id) = crate::types::object_owner_to_wire(item.owner, item.group);
        let message = AnyMessage::RezRestoreToWorld(RezRestoreToWorld {
            agent_data: RezRestoreToWorldAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            inventory_data: RezRestoreToWorldInventoryDataBlock {
                item_id: item.item_id.uuid(),
                folder_id: item.folder_id.uuid(),
                creator_id: item.creator_id.uuid(),
                owner_id,
                group_id,
                base_mask: item.permissions.base.bits(),
                owner_mask: item.permissions.owner.bits(),
                group_mask: item.permissions.group.bits(),
                everyone_mask: item.permissions.everyone.bits(),
                next_owner_mask: item.permissions.next_owner.bits(),
                group_owned: item.owner.is_group(),
                transaction_id: item.transaction_id,
                r#type: item.asset_type,
                inv_type: item.inv_type,
                flags: item.flags,
                sale_type: item.sale_type.to_code(),
                sale_price: crate::types::linden_price_to_wire(
                    "SalePrice",
                    item.sale_price.as_ref(),
                )?,
                name: with_nul(&item.name),
                description: with_nul(&item.description),
                creation_date: item.creation_date,
                crc: item.crc,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `RezObjectFromNotecard` reliably (rez the items embedded in a
    /// notecard asset).
    pub(crate) fn send_rez_object_from_notecard(
        &mut self,
        rez: &NotecardRez,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::RezObjectFromNotecard(RezObjectFromNotecard {
            agent_data: RezObjectFromNotecardAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
                group_id: rez.group_id.map_or_else(Uuid::nil, |g| g.uuid()),
            },
            rez_data: RezObjectFromNotecardRezDataBlock {
                from_task_id: rez.from_task_id.map_or_else(Uuid::nil, |t| t.uuid()),
                bypass_raycast: u8::from(rez.bypass_raycast),
                ray_start: rez.ray_start.clone(),
                ray_end: rez.ray_end.clone(),
                ray_target_id: rez.ray_target_id.map_or_else(Uuid::nil, |t| t.uuid()),
                ray_end_is_intersection: rez.ray_end_is_intersection,
                rez_selected: rez.rez_selected,
                remove_item: rez.remove_item,
                item_flags: rez.item_flags,
                group_mask: rez.group_mask,
                everyone_mask: rez.everyone_mask,
                next_owner_mask: rez.next_owner_mask,
            },
            notecard_data: RezObjectFromNotecardNotecardDataBlock {
                notecard_item_id: rez.notecard_item_id.uuid(),
                object_id: rez.object_id.uuid(),
            },
            inventory_data: rez
                .item_ids
                .iter()
                .map(|id| RezObjectFromNotecardInventoryDataBlock { item_id: id.uuid() })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `RezObject` reliably (rez the inventory item `params.item` into
    /// the world as a new object).
    pub(crate) fn send_rez_object(
        &mut self,
        params: &RezObjectParams,
        now: Instant,
    ) -> Result<(), WireError> {
        let item = &params.item;
        let (owner_id, group_id) = crate::types::object_owner_to_wire(item.owner, item.group);
        let message = AnyMessage::RezObject(RezObject {
            agent_data: RezObjectAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
                group_id: params.group_id.map_or_else(Uuid::nil, |g| g.uuid()),
            },
            rez_data: RezObjectRezDataBlock {
                from_task_id: params.from_task_id.map_or_else(Uuid::nil, |t| t.uuid()),
                bypass_raycast: u8::from(params.bypass_raycast),
                ray_start: params.ray_start.clone(),
                ray_end: params.ray_end.clone(),
                ray_target_id: params.ray_target_id.map_or_else(Uuid::nil, |t| t.uuid()),
                ray_end_is_intersection: params.ray_end_is_intersection,
                rez_selected: params.rez_selected,
                remove_item: params.remove_item,
                item_flags: params.item_flags,
                group_mask: params.group_mask,
                everyone_mask: params.everyone_mask,
                next_owner_mask: params.next_owner_mask,
            },
            inventory_data: RezObjectInventoryDataBlock {
                item_id: item.item_id.uuid(),
                folder_id: item.folder_id.uuid(),
                creator_id: item.creator_id.uuid(),
                owner_id,
                group_id,
                base_mask: item.permissions.base.bits(),
                owner_mask: item.permissions.owner.bits(),
                group_mask: item.permissions.group.bits(),
                everyone_mask: item.permissions.everyone.bits(),
                next_owner_mask: item.permissions.next_owner.bits(),
                group_owned: item.owner.is_group(),
                transaction_id: item.transaction_id,
                r#type: item.asset_type,
                inv_type: item.inv_type,
                flags: item.flags,
                sale_type: item.sale_type.to_code(),
                sale_price: crate::types::linden_price_to_wire(
                    "SalePrice",
                    item.sale_price.as_ref(),
                )?,
                name: with_nul(&item.name),
                description: with_nul(&item.description),
                creation_date: item.creation_date,
                crc: item.crc,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `RezScript` reliably (drop the script item `params.item` into the
    /// task inventory of the in-world object `local_id`).
    pub(crate) fn send_rez_script(
        &mut self,
        local_id: RegionLocalObjectId,
        params: &RezScriptParams,
        now: Instant,
    ) -> Result<(), WireError> {
        let item = &params.item;
        let (owner_id, group_id) = crate::types::object_owner_to_wire(item.owner, item.group);
        let message = AnyMessage::RezScript(RezScript {
            agent_data: RezScriptAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
                group_id: params.group_id.map_or_else(Uuid::nil, |g| g.uuid()),
            },
            update_block: RezScriptUpdateBlockBlock {
                object_local_id: local_id.0,
                enabled: params.enabled,
            },
            inventory_block: RezScriptInventoryBlockBlock {
                item_id: item.item_id.uuid(),
                folder_id: item.folder_id.uuid(),
                creator_id: item.creator_id.uuid(),
                owner_id,
                group_id,
                base_mask: item.permissions.base.bits(),
                owner_mask: item.permissions.owner.bits(),
                group_mask: item.permissions.group.bits(),
                everyone_mask: item.permissions.everyone.bits(),
                next_owner_mask: item.permissions.next_owner.bits(),
                group_owned: item.owner.is_group(),
                transaction_id: item.transaction_id,
                r#type: item.asset_type,
                inv_type: item.inv_type,
                flags: item.flags,
                sale_type: item.sale_type.to_code(),
                sale_price: crate::types::linden_price_to_wire(
                    "SalePrice",
                    item.sale_price.as_ref(),
                )?,
                name: with_nul(&item.name),
                description: with_nul(&item.description),
                creation_date: item.creation_date,
                crc: item.crc,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `RevokePermissions` reliably (revoke the named LSL script
    /// `permissions` previously granted to the object `object_id`).
    pub(crate) fn send_revoke_permissions(
        &mut self,
        object_id: ObjectKey,
        permissions: ScriptPermissions,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::RevokePermissions(RevokePermissions {
            agent_data: RevokePermissionsAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            data: RevokePermissionsDataBlock {
                object_id: object_id.uuid(),
                object_permissions: permissions.0.cast_unsigned(),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `DetachAttachmentIntoInv` reliably (detach the worn attachment
    /// `item_id` back into inventory).
    pub(crate) fn send_detach_attachment_into_inv(
        &mut self,
        item_id: InventoryKey,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::DetachAttachmentIntoInv(DetachAttachmentIntoInv {
            object_data: DetachAttachmentIntoInvObjectDataBlock {
                agent_id: self.agent_id.uuid(),
                item_id: item_id.uuid(),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `RequestTaskInventory` reliably (ask for the task inventory
    /// listing of the in-world object `local_id`). The reply arrives as a
    /// `ReplyTaskInventory` (surfaced as
    /// [`Event::TaskInventoryReply`](crate::Event::TaskInventoryReply)).
    pub(crate) fn send_request_task_inventory(
        &mut self,
        local_id: RegionLocalObjectId,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::RequestTaskInventory(RequestTaskInventory {
            agent_data: RequestTaskInventoryAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            inventory_data: RequestTaskInventoryInventoryDataBlock {
                local_id: local_id.0,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `UpdateTaskInventory` reliably (write the item `item` into the
    /// task inventory of the in-world object `local_id`, keyed by `key`).
    pub(crate) fn send_update_task_inventory(
        &mut self,
        local_id: RegionLocalObjectId,
        key: TaskInventoryKey,
        item: &RestoreItem,
        now: Instant,
    ) -> Result<(), WireError> {
        let (owner_id, group_id) = crate::types::object_owner_to_wire(item.owner, item.group);
        let message = AnyMessage::UpdateTaskInventory(UpdateTaskInventory {
            agent_data: UpdateTaskInventoryAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            update_data: UpdateTaskInventoryUpdateDataBlock {
                local_id: local_id.0,
                key: key.to_code(),
            },
            inventory_data: UpdateTaskInventoryInventoryDataBlock {
                item_id: item.item_id.uuid(),
                folder_id: item.folder_id.uuid(),
                creator_id: item.creator_id.uuid(),
                owner_id,
                group_id,
                base_mask: item.permissions.base.bits(),
                owner_mask: item.permissions.owner.bits(),
                group_mask: item.permissions.group.bits(),
                everyone_mask: item.permissions.everyone.bits(),
                next_owner_mask: item.permissions.next_owner.bits(),
                group_owned: item.owner.is_group(),
                transaction_id: item.transaction_id,
                r#type: item.asset_type,
                inv_type: item.inv_type,
                flags: item.flags,
                sale_type: item.sale_type.to_code(),
                sale_price: crate::types::linden_price_to_wire(
                    "SalePrice",
                    item.sale_price.as_ref(),
                )?,
                name: with_nul(&item.name),
                description: with_nul(&item.description),
                creation_date: item.creation_date,
                crc: item.crc,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `MoveTaskInventory` reliably (move the task inventory item
    /// `item_id` out of the in-world object `local_id` into the agent inventory
    /// folder `folder_id`).
    pub(crate) fn send_move_task_inventory(
        &mut self,
        local_id: RegionLocalObjectId,
        folder_id: InventoryFolderKey,
        item_id: InventoryKey,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::MoveTaskInventory(MoveTaskInventory {
            agent_data: MoveTaskInventoryAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
                folder_id: folder_id.uuid(),
            },
            inventory_data: MoveTaskInventoryInventoryDataBlock {
                local_id: local_id.0,
                item_id: item_id.uuid(),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `RemoveTaskInventory` reliably (remove the task inventory item
    /// `item_id` from the in-world object `local_id`).
    pub(crate) fn send_remove_task_inventory(
        &mut self,
        local_id: RegionLocalObjectId,
        item_id: InventoryKey,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::RemoveTaskInventory(RemoveTaskInventory {
            agent_data: RemoveTaskInventoryAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            inventory_data: RemoveTaskInventoryInventoryDataBlock {
                local_id: local_id.0,
                item_id: item_id.uuid(),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `ModifyLand` reliably (apply a single terraform brush stroke
    /// `edit`).
    pub(crate) fn send_modify_land(
        &mut self,
        edit: &LandEdit,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ModifyLand(ModifyLand {
            agent_data: ModifyLandAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            modify_block: ModifyLandModifyBlockBlock {
                action: edit.action.to_code(),
                brush_size: edit.brush_size.to_index(),
                seconds: edit.strength,
                height: edit.height,
            },
            parcel_data: vec![ModifyLandParcelDataBlock {
                local_id: edit.parcel.map_or(-1, |parcel| parcel.0),
                west: edit.area.west,
                south: edit.area.south,
                east: edit.area.east,
                north: edit.area.north,
            }],
            modify_block_extended: vec![ModifyLandModifyBlockExtendedBlock {
                brush_size: edit.brush_size.to_metres(),
            }],
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `UndoLand` reliably (undo the agent's last terraform edit).
    pub(crate) fn send_undo_land(&mut self, now: Instant) -> Result<(), WireError> {
        let message = AnyMessage::UndoLand(UndoLand {
            agent_data: UndoLandAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `ParcelPropertiesRequestByID` reliably (fetch the parcel
    /// `local_id` by its region-local id). The reply is a `ParcelProperties`.
    pub(crate) fn send_parcel_properties_request_by_id(
        &mut self,
        local_id: RegionLocalParcelId,
        sequence_id: i32,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ParcelPropertiesRequestByID(ParcelPropertiesRequestByID {
            agent_data: ParcelPropertiesRequestByIDAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            parcel_data: ParcelPropertiesRequestByIDParcelDataBlock {
                sequence_id,
                local_id: local_id.0,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `ParcelSetOtherCleanTime` reliably (set the parcel `local_id`'s
    /// auto-return time for other people's objects to `clean_time` minutes).
    pub(crate) fn send_parcel_set_other_clean_time(
        &mut self,
        local_id: RegionLocalParcelId,
        clean_time: i32,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ParcelSetOtherCleanTime(ParcelSetOtherCleanTime {
            agent_data: ParcelSetOtherCleanTimeAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            parcel_data: ParcelSetOtherCleanTimeParcelDataBlock {
                local_id: local_id.0,
                other_clean_time: clean_time,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `MultipleObjectUpdate` reliably applying `transform` to `local_id`.
    /// The packed `Data` blob carries position/rotation/scale in that fixed
    /// order, matching the simulator's `MultipleObjectUpdate` parser.
    pub(crate) fn send_multiple_object_update(
        &mut self,
        local_id: RegionLocalObjectId,
        transform: &ObjectTransform,
        now: Instant,
    ) -> Result<(), WireError> {
        let mut data = Writer::new();
        if let Some(position) = &transform.position {
            data.put_vector3(position);
        }
        if let Some(rotation) = &transform.rotation {
            let [x, y, z] = pack_quaternion_to_vec3(rotation);
            data.put_f32(x);
            data.put_f32(y);
            data.put_f32(z);
        }
        if let Some(scale) = &transform.scale {
            data.put_vector3(scale);
        }
        let message = AnyMessage::MultipleObjectUpdate(MultipleObjectUpdate {
            agent_data: MultipleObjectUpdateAgentDataBlock {
                agent_id: self.agent_id.uuid(),
                session_id: self.session_id,
            },
            object_data: vec![MultipleObjectUpdateObjectDataBlock {
                object_local_id: local_id.0,
                r#type: transform.type_byte(),
                data: data.into_bytes(),
            }],
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Records that a datagram was received, resetting the inactivity timer.
    pub(crate) fn note_received(&mut self, now: Instant) {
        self.timers.inactivity = deadline(now, INACTIVITY_TIMEOUT);
    }

    /// Records that we owe an acknowledgement for `sequence`, arming the flush.
    pub(crate) fn queue_ack(&mut self, sequence: SequenceNumber, now: Instant) {
        self.pending_acks.push(sequence);
        if self.timers.ack_flush.is_none() {
            self.timers.ack_flush = Some(deadline(now, ACK_FLUSH_DELAY));
        }
    }

    /// Removes the given outgoing sequence numbers from the unacked set.
    pub(crate) fn record_acks(&mut self, ids: &[SequenceNumber]) {
        for id in ids {
            self.unacked.remove(id);
        }
    }

    /// Records an inbound reliable `sequence`; returns `true` if it is new.
    pub(crate) fn mark_seen(&mut self, sequence: SequenceNumber) -> bool {
        self.seen.insert(sequence)
    }

    /// Flushes owed acknowledgements as one or more `PacketAck` messages.
    pub(crate) fn flush_acks(&mut self, now: Instant) -> Result<(), WireError> {
        self.timers.ack_flush = None;
        if self.pending_acks.is_empty() {
            return Ok(());
        }
        let acks = std::mem::take(&mut self.pending_acks);
        for chunk in acks.chunks(MAX_ACKS_PER_PACKET) {
            let packets = chunk
                .iter()
                .map(|id| PacketAckPacketsBlock { id: id.get() })
                .collect();
            let message = AnyMessage::PacketAck(PacketAck { packets });
            self.send(&message, Reliability::Unreliable, now)?;
        }
        Ok(())
    }

    /// Retransmits unacknowledged reliable packets whose timeout has elapsed.
    ///
    /// Returns the `(sequence, message name)` of every packet that has now
    /// exhausted its retransmission budget; such packets are dropped from the
    /// unacked set (so they are reported only once and stop driving the resend
    /// deadline). An empty result means nothing exhausted this tick.
    pub(crate) fn process_resends(
        &mut self,
        now: Instant,
    ) -> Vec<(SequenceNumber, Option<&'static str>)> {
        let mut exhausted = Vec::new();
        let mut to_send = Vec::new();
        for (sequence, packet) in &mut self.unacked {
            if now < deadline(packet.sent_at, RESEND_TIMEOUT) {
                continue;
            }
            if packet.attempts >= MAX_RESEND_ATTEMPTS {
                exhausted.push((*sequence, packet.name));
                continue;
            }
            let mut datagram = packet.datagram.clone();
            if let Some(first) = datagram.first_mut() {
                *first |= PacketFlags::RESENT.bits();
            }
            packet.sent_at = now;
            packet.attempts = packet.attempts.saturating_add(1);
            to_send.push(datagram);
        }
        self.out.extend(to_send);
        for (sequence, _) in &exhausted {
            self.unacked.remove(sequence);
        }
        exhausted
    }

    /// The earliest retransmission deadline across all unacked packets.
    pub(crate) fn next_resend_deadline(&self) -> Option<Instant> {
        self.unacked
            .values()
            .map(|packet| deadline(packet.sent_at, RESEND_TIMEOUT))
            .min()
    }
}
