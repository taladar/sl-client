//! Per-simulator circuit: reliable-UDP bookkeeping and outgoing message encoders.

use super::conversions::{
    OutgoingIm, compute_im_session_id, inventory_item_crc, pack_quaternion_to_vec3, with_nul,
};
use super::{
    ACK_FLUSH_DELAY, Circuit, INACTIVITY_TIMEOUT, MAP_LAYER_FLAG, MAX_ACKS_PER_PACKET,
    MAX_RESEND_ATTEMPTS, RESEND_TIMEOUT, SeenWindow, Timers, UnackedPacket, deadline,
};
use crate::types::{
    AssetType, Camera, ChatType, ClassifiedUpdate, ClickAction, CreateGroupParams,
    DeRezDestination, GroupRoleEdit, GroupRoleMemberChange, ImDialog, InterestsUpdate,
    InventoryItem, Material, NewInventoryItem, ObjectFlagSettings, ObjectTransform,
    ParcelAccessEntry, ParcelUpdate, PermissionField, PickUpdate, PrimShape, ProfileUpdate,
    Reliability, SaleType, Throttle, Wearable,
};
use sl_types::lsl::{Rotation, Vector};
use sl_wire::messages::{
    AcceptFriendship, AcceptFriendshipAgentDataBlock, AcceptFriendshipFolderDataBlock,
    AcceptFriendshipTransactionBlockBlock, ActivateGroup, ActivateGroupAgentDataBlock,
    AgentAnimation, AgentAnimationAgentDataBlock, AgentAnimationAnimationListBlock,
    AgentAnimationPhysicalAvatarEventListBlock, AgentCachedTexture,
    AgentCachedTextureAgentDataBlock, AgentCachedTextureWearableDataBlock, AgentIsNowWearing,
    AgentIsNowWearingAgentDataBlock, AgentIsNowWearingWearableDataBlock, AgentRequestSit,
    AgentRequestSitAgentDataBlock, AgentRequestSitTargetObjectBlock, AgentSetAppearance,
    AgentSetAppearanceAgentDataBlock, AgentSetAppearanceObjectDataBlock,
    AgentSetAppearanceVisualParamBlock, AgentSetAppearanceWearableDataBlock, AgentSit,
    AgentSitAgentDataBlock, AgentThrottle, AgentThrottleAgentDataBlock, AgentThrottleThrottleBlock,
    AgentUpdate, AgentUpdateAgentDataBlock, AgentWearablesRequest,
    AgentWearablesRequestAgentDataBlock, AssetUploadRequest, AssetUploadRequestAssetBlockBlock,
    AvatarInterestsUpdate, AvatarInterestsUpdateAgentDataBlock,
    AvatarInterestsUpdatePropertiesDataBlock, AvatarNotesUpdate, AvatarNotesUpdateAgentDataBlock,
    AvatarNotesUpdateDataBlock, AvatarPropertiesRequest, AvatarPropertiesRequestAgentDataBlock,
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
    DeclineFriendship, DeclineFriendshipAgentDataBlock, DeclineFriendshipTransactionBlockBlock,
    EconomyDataRequest, EjectGroupMemberRequest, EjectGroupMemberRequestAgentDataBlock,
    EjectGroupMemberRequestEjectDataBlock, EjectGroupMemberRequestGroupDataBlock,
    EstateOwnerMessage, EstateOwnerMessageAgentDataBlock, EstateOwnerMessageMethodDataBlock,
    EstateOwnerMessageParamListBlock, FetchInventoryDescendents,
    FetchInventoryDescendentsAgentDataBlock, FetchInventoryDescendentsInventoryDataBlock,
    GenericMessage, GenericMessageAgentDataBlock, GenericMessageMethodDataBlock,
    GenericMessageParamListBlock, GodKickUser, GodKickUserUserInfoBlock, GodlikeMessage,
    GodlikeMessageAgentDataBlock, GodlikeMessageMethodDataBlock, GodlikeMessageParamListBlock,
    GrantUserRights, GrantUserRightsAgentDataBlock, GrantUserRightsRightsBlock,
    GroupMembersRequest, GroupMembersRequestAgentDataBlock, GroupMembersRequestGroupDataBlock,
    GroupNoticeRequest, GroupNoticeRequestAgentDataBlock, GroupNoticeRequestDataBlock,
    GroupNoticesListRequest, GroupNoticesListRequestAgentDataBlock,
    GroupNoticesListRequestDataBlock, GroupProfileRequest, GroupProfileRequestAgentDataBlock,
    GroupProfileRequestGroupDataBlock, GroupRoleChanges, GroupRoleChangesAgentDataBlock,
    GroupRoleChangesRoleChangeBlock, GroupRoleDataRequest, GroupRoleDataRequestAgentDataBlock,
    GroupRoleDataRequestGroupDataBlock, GroupRoleMembersRequest,
    GroupRoleMembersRequestAgentDataBlock, GroupRoleMembersRequestGroupDataBlock, GroupRoleUpdate,
    GroupRoleUpdateAgentDataBlock, GroupRoleUpdateRoleDataBlock, GroupTitlesRequest,
    GroupTitlesRequestAgentDataBlock, ImprovedInstantMessage, ImprovedInstantMessageAgentDataBlock,
    ImprovedInstantMessageEstateBlockBlock, ImprovedInstantMessageMessageBlockBlock,
    InviteGroupRequest, InviteGroupRequestAgentDataBlock, InviteGroupRequestGroupDataBlock,
    InviteGroupRequestInviteDataBlock, JoinGroupRequest, JoinGroupRequestAgentDataBlock,
    JoinGroupRequestGroupDataBlock, LeaveGroupRequest, LeaveGroupRequestAgentDataBlock,
    LeaveGroupRequestGroupDataBlock, LogoutRequest, LogoutRequestAgentDataBlock, MapBlockRequest,
    MapBlockRequestAgentDataBlock, MapBlockRequestPositionDataBlock, MapItemRequest,
    MapItemRequestAgentDataBlock, MapItemRequestRequestDataBlock, MapNameRequest,
    MapNameRequestAgentDataBlock, MapNameRequestNameDataBlock, MoneyBalanceRequest,
    MoneyBalanceRequestAgentDataBlock, MoneyBalanceRequestMoneyDataBlock, MoneyTransferRequest,
    MoneyTransferRequestAgentDataBlock, MoneyTransferRequestMoneyDataBlock, MoveInventoryFolder,
    MoveInventoryFolderAgentDataBlock, MoveInventoryFolderInventoryDataBlock, MoveInventoryItem,
    MoveInventoryItemAgentDataBlock, MoveInventoryItemInventoryDataBlock, MultipleObjectUpdate,
    MultipleObjectUpdateAgentDataBlock, MultipleObjectUpdateObjectDataBlock, MuteListRequest,
    MuteListRequestAgentDataBlock, MuteListRequestMuteDataBlock, ObjectAdd,
    ObjectAddAgentDataBlock, ObjectAddObjectDataBlock, ObjectCategory,
    ObjectCategoryAgentDataBlock, ObjectCategoryObjectDataBlock, ObjectClickAction,
    ObjectClickActionAgentDataBlock, ObjectClickActionObjectDataBlock, ObjectDeGrab,
    ObjectDeGrabAgentDataBlock, ObjectDeGrabObjectDataBlock, ObjectDelete,
    ObjectDeleteAgentDataBlock, ObjectDeleteObjectDataBlock, ObjectDelink,
    ObjectDelinkAgentDataBlock, ObjectDelinkObjectDataBlock, ObjectDescription,
    ObjectDescriptionAgentDataBlock, ObjectDescriptionObjectDataBlock, ObjectDeselect,
    ObjectDeselectAgentDataBlock, ObjectDeselectObjectDataBlock, ObjectDuplicate,
    ObjectDuplicateAgentDataBlock, ObjectDuplicateObjectDataBlock, ObjectDuplicateSharedDataBlock,
    ObjectFlagUpdate, ObjectFlagUpdateAgentDataBlock, ObjectGrab, ObjectGrabAgentDataBlock,
    ObjectGrabObjectDataBlock, ObjectGrabUpdate, ObjectGrabUpdateAgentDataBlock,
    ObjectGrabUpdateObjectDataBlock, ObjectGroup, ObjectGroupAgentDataBlock,
    ObjectGroupObjectDataBlock, ObjectIncludeInSearch, ObjectIncludeInSearchAgentDataBlock,
    ObjectIncludeInSearchObjectDataBlock, ObjectLink, ObjectLinkAgentDataBlock,
    ObjectLinkObjectDataBlock, ObjectMaterial, ObjectMaterialAgentDataBlock,
    ObjectMaterialObjectDataBlock, ObjectName, ObjectNameAgentDataBlock, ObjectNameObjectDataBlock,
    ObjectPermissions, ObjectPermissionsAgentDataBlock, ObjectPermissionsHeaderDataBlock,
    ObjectPermissionsObjectDataBlock, ObjectSaleInfo, ObjectSaleInfoAgentDataBlock,
    ObjectSaleInfoObjectDataBlock, ObjectSelect, ObjectSelectAgentDataBlock,
    ObjectSelectObjectDataBlock, PacketAck, PacketAckPacketsBlock, ParcelAccessListRequest,
    ParcelAccessListRequestAgentDataBlock, ParcelAccessListRequestDataBlock,
    ParcelAccessListUpdate, ParcelAccessListUpdateAgentDataBlock, ParcelAccessListUpdateDataBlock,
    ParcelAccessListUpdateListBlock, ParcelBuy, ParcelBuyAgentDataBlock, ParcelBuyDataBlock,
    ParcelBuyParcelDataBlock, ParcelDeedToGroup, ParcelDeedToGroupAgentDataBlock,
    ParcelDeedToGroupDataBlock, ParcelDwellRequest, ParcelDwellRequestAgentDataBlock,
    ParcelDwellRequestDataBlock, ParcelPropertiesRequest, ParcelPropertiesRequestAgentDataBlock,
    ParcelPropertiesRequestParcelDataBlock, ParcelPropertiesUpdate,
    ParcelPropertiesUpdateAgentDataBlock, ParcelPropertiesUpdateParcelDataBlock, ParcelReclaim,
    ParcelReclaimAgentDataBlock, ParcelReclaimDataBlock, ParcelRelease,
    ParcelReleaseAgentDataBlock, ParcelReleaseDataBlock, ParcelReturnObjects,
    ParcelReturnObjectsAgentDataBlock, ParcelReturnObjectsOwnerIDsBlock,
    ParcelReturnObjectsParcelDataBlock, ParcelReturnObjectsTaskIDsBlock, ParcelSelectObjects,
    ParcelSelectObjectsAgentDataBlock, ParcelSelectObjectsParcelDataBlock,
    ParcelSelectObjectsReturnIDsBlock, PickDelete, PickDeleteAgentDataBlock, PickDeleteDataBlock,
    PickGodDelete, PickGodDeleteAgentDataBlock, PickGodDeleteDataBlock, PickInfoUpdate,
    PickInfoUpdateAgentDataBlock, PickInfoUpdateDataBlock, PurgeInventoryDescendents,
    PurgeInventoryDescendentsAgentDataBlock, PurgeInventoryDescendentsInventoryDataBlock,
    RegionHandshakeReply, RegionHandshakeReplyAgentDataBlock, RegionHandshakeReplyRegionInfoBlock,
    RemoveInventoryFolder, RemoveInventoryFolderAgentDataBlock,
    RemoveInventoryFolderFolderDataBlock, RemoveInventoryItem, RemoveInventoryItemAgentDataBlock,
    RemoveInventoryItemInventoryDataBlock, RemoveInventoryObjects,
    RemoveInventoryObjectsAgentDataBlock, RemoveInventoryObjectsFolderDataBlock,
    RemoveInventoryObjectsItemDataBlock, RemoveMuteListEntry, RemoveMuteListEntryAgentDataBlock,
    RemoveMuteListEntryMuteDataBlock, RequestImage, RequestImageAgentDataBlock,
    RequestImageRequestImageBlock, RequestMultipleObjects, RequestMultipleObjectsAgentDataBlock,
    RequestMultipleObjectsObjectDataBlock, RequestRegionInfo, RequestRegionInfoAgentDataBlock,
    RequestXfer, RequestXferXferIDBlock, RetrieveInstantMessages,
    RetrieveInstantMessagesAgentDataBlock, ScriptAnswerYes, ScriptAnswerYesAgentDataBlock,
    ScriptAnswerYesDataBlock, ScriptDialogReply, ScriptDialogReplyAgentDataBlock,
    ScriptDialogReplyDataBlock, SendXferPacket, SendXferPacketDataPacketBlock,
    SendXferPacketXferIDBlock, SetGroupAcceptNotices, SetGroupAcceptNoticesAgentDataBlock,
    SetGroupAcceptNoticesDataBlock, SetGroupAcceptNoticesNewDataBlock, SetGroupContribution,
    SetGroupContributionAgentDataBlock, SetGroupContributionDataBlock, StartLure,
    StartLureAgentDataBlock, StartLureInfoBlock, StartLureTargetDataBlock, TeleportLocationRequest,
    TeleportLocationRequestAgentDataBlock, TeleportLocationRequestInfoBlock, TeleportLureRequest,
    TeleportLureRequestInfoBlock, TerminateFriendship, TerminateFriendshipAgentDataBlock,
    TerminateFriendshipExBlockBlock, TransferRequest, TransferRequestTransferInfoBlock,
    UpdateInventoryFolder, UpdateInventoryFolderAgentDataBlock,
    UpdateInventoryFolderFolderDataBlock, UpdateInventoryItem, UpdateInventoryItemAgentDataBlock,
    UpdateInventoryItemInventoryDataBlock, UpdateMuteListEntry, UpdateMuteListEntryAgentDataBlock,
    UpdateMuteListEntryMuteDataBlock, UseCircuitCode, UseCircuitCodeCircuitCodeBlock,
};
use sl_wire::{AnyMessage, PacketFlags, WireError, Writer, encode_datagram};
use std::collections::{BTreeMap, VecDeque};
use std::net::SocketAddr;
use std::time::Instant;
use uuid::Uuid;

impl Circuit {
    /// Creates a circuit and arms the inactivity timer.
    pub(crate) fn new(
        sim_addr: SocketAddr,
        agent_id: Uuid,
        session_id: Uuid,
        circuit_code: u32,
        draw_distance: f32,
        now: Instant,
    ) -> Self {
        Self {
            sim_addr,
            agent_id,
            session_id,
            code: circuit_code,
            next_sequence: 1,
            pending_acks: Vec::new(),
            unacked: BTreeMap::new(),
            seen: SeenWindow::default(),
            out: VecDeque::new(),
            draw_distance,
            timers: Timers {
                inactivity: deadline(now, INACTIVITY_TIMEOUT),
                ack_flush: None,
                agent_update: None,
                logout: None,
                teleport: None,
            },
        }
    }

    /// Re-points the circuit at a new simulator after a teleport, resetting the
    /// per-circuit sequence/ack/seen/timer state while keeping the agent
    /// identity and circuit code (both reused across regions).
    pub(crate) fn retarget(&mut self, sim_addr: SocketAddr, now: Instant) {
        self.sim_addr = sim_addr;
        self.next_sequence = 1;
        self.pending_acks.clear();
        self.unacked.clear();
        self.seen = SeenWindow::default();
        self.out.clear();
        self.timers = Timers {
            inactivity: deadline(now, INACTIVITY_TIMEOUT),
            ack_flush: None,
            agent_update: None,
            logout: None,
            teleport: None,
        };
    }

    /// Allocates the next outgoing sequence number.
    pub(crate) const fn next_sequence(&mut self) -> u32 {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.wrapping_add(1);
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
                code: self.code,
                session_id: self.session_id,
                id: self.agent_id,
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
                agent_id: self.agent_id,
                session_id: self.session_id,
                circuit_code: self.code,
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
                agent_id: self.agent_id,
                session_id: self.session_id,
                circuit_code: self.code,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues `RegionHandshakeReply` reliably.
    pub(crate) fn send_region_handshake_reply(&mut self, now: Instant) -> Result<(), WireError> {
        let message = AnyMessage::RegionHandshakeReply(RegionHandshakeReply {
            agent_data: RegionHandshakeReplyAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            region_info: RegionHandshakeReplyRegionInfoBlock { flags: 0 },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `CompletePingCheck` reply unreliably.
    pub(crate) fn send_complete_ping_check(
        &mut self,
        ping_id: u8,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::CompletePingCheck(CompletePingCheck {
            ping_id: CompletePingCheckPingIDBlock { ping_id },
        });
        self.send(&message, Reliability::Unreliable, now)
    }

    /// Queues a `ChatFromViewer` reliably, sending local chat. The wire string
    /// carries a trailing NUL, as a real viewer sends.
    pub(crate) fn send_chat_from_viewer(
        &mut self,
        message: &str,
        chat_type: ChatType,
        channel: i32,
        now: Instant,
    ) -> Result<(), WireError> {
        let mut bytes = message.as_bytes().to_vec();
        bytes.push(0);
        let message = AnyMessage::ChatFromViewer(ChatFromViewer {
            agent_data: ChatFromViewerAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            chat_data: ChatFromViewerChatDataBlock {
                message: bytes,
                r#type: chat_type.to_u8(),
                channel,
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
        to_agent_id: Uuid,
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
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            message_block: ImprovedInstantMessageMessageBlockBlock {
                from_group: false,
                to_agent_id,
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
                agent_id: self.agent_id,
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
                agent_id: self.agent_id,
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

    /// Queues a `TeleportLureRequest` reliably: accepts a teleport lure,
    /// requesting the teleport the offer's `lure_id` (the `IM_LURE_USER` IM's
    /// `id`) describes. `teleport_flags` is the viewer's `TELEPORT_FLAGS_VIA_LURE`.
    pub(crate) fn send_teleport_lure_request(
        &mut self,
        lure_id: Uuid,
        teleport_flags: u32,
        now: Instant,
    ) -> Result<(), WireError> {
        let request = AnyMessage::TeleportLureRequest(TeleportLureRequest {
            info: TeleportLureRequestInfoBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
                lure_id,
                teleport_flags,
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
                agent_id: self.agent_id,
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
                agent_id: self.agent_id,
                session_id: self.session_id,
                body_rotation,
                head_rotation,
                state: 0,
                camera_center: camera.center.clone(),
                camera_at_axis: camera.at_axis.clone(),
                camera_left_axis: camera.left_axis.clone(),
                camera_up_axis: camera.up_axis.clone(),
                far: self.draw_distance,
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
                agent_id: self.agent_id,
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
                agent_id: self.agent_id,
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
                agent_id: self.agent_id,
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
                agent_id: self.agent_id,
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
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            properties_data: AvatarPropertiesUpdatePropertiesDataBlock {
                image_id: update.image_id,
                fl_image_id: update.fl_image_id,
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
                agent_id: self.agent_id,
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
                agent_id: self.agent_id,
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
        classified_id: Uuid,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ClassifiedInfoRequest(ClassifiedInfoRequest {
            agent_data: ClassifiedInfoRequestAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            data: ClassifiedInfoRequestDataBlock { classified_id },
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
        let (x, y, z) = update.pos_global;
        let message = AnyMessage::PickInfoUpdate(PickInfoUpdate {
            agent_data: PickInfoUpdateAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            data: PickInfoUpdateDataBlock {
                pick_id: update.pick_id,
                creator_id: self.agent_id,
                // Only gods may set the legacy "top pick" flag; the viewer
                // always sends false.
                top_pick: false,
                parcel_id: update.parcel_id,
                name: with_nul(&update.name),
                desc: with_nul(&update.description),
                snapshot_id: update.snapshot_id,
                pos_global: [x, y, z],
                sort_order: update.sort_order,
                enabled: update.enabled,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `PickDelete` reliably, removing one of the agent's picks (#29).
    pub(crate) fn send_pick_delete(
        &mut self,
        pick_id: Uuid,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::PickDelete(PickDelete {
            agent_data: PickDeleteAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            data: PickDeleteDataBlock { pick_id },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `PickGodDelete` reliably (god-only; `query_id` lets the
    /// dataserver resend the affected agent's pick list) (#29).
    pub(crate) fn send_pick_god_delete(
        &mut self,
        pick_id: Uuid,
        query_id: Uuid,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::PickGodDelete(PickGodDelete {
            agent_data: PickGodDeleteAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            data: PickGodDeleteDataBlock { pick_id, query_id },
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
        let (x, y, z) = update.pos_global;
        let message = AnyMessage::ClassifiedInfoUpdate(ClassifiedInfoUpdate {
            agent_data: ClassifiedInfoUpdateAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            data: ClassifiedInfoUpdateDataBlock {
                classified_id: update.classified_id,
                category: update.category,
                name: with_nul(&update.name),
                desc: with_nul(&update.description),
                parcel_id: update.parcel_id,
                // Set on the simulator as the message passes through.
                parent_estate: 0,
                snapshot_id: update.snapshot_id,
                pos_global: [x, y, z],
                classified_flags: update.classified_flags,
                price_for_listing: update.price_for_listing,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `ClassifiedDelete` reliably, removing one of the agent's
    /// classifieds (#29).
    pub(crate) fn send_classified_delete(
        &mut self,
        classified_id: Uuid,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ClassifiedDelete(ClassifiedDelete {
            agent_data: ClassifiedDeleteAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            data: ClassifiedDeleteDataBlock { classified_id },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `ClassifiedGodDelete` reliably (god-only; `query_id` lets the
    /// dataserver resend the affected agent's classified list) (#29).
    pub(crate) fn send_classified_god_delete(
        &mut self,
        classified_id: Uuid,
        query_id: Uuid,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ClassifiedGodDelete(ClassifiedGodDelete {
            agent_data: ClassifiedGodDeleteAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            data: ClassifiedGodDeleteDataBlock {
                classified_id,
                query_id,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `GrantUserRights` reliably, setting the rights this agent grants
    /// the friend `target` to `rights`.
    pub(crate) fn send_grant_user_rights(
        &mut self,
        target: Uuid,
        rights: i32,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::GrantUserRights(GrantUserRights {
            agent_data: GrantUserRightsAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            rights: vec![GrantUserRightsRightsBlock {
                agent_related: target,
                related_rights: rights,
            }],
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `TerminateFriendship` reliably, ending the friendship with
    /// `other`.
    pub(crate) fn send_terminate_friendship(
        &mut self,
        other: Uuid,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::TerminateFriendship(TerminateFriendship {
            agent_data: TerminateFriendshipAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            ex_block: TerminateFriendshipExBlockBlock { other_id: other },
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
                agent_id: self.agent_id,
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
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            transaction_block: DeclineFriendshipTransactionBlockBlock { transaction_id },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ActivateGroup` reliably, making `group_id` the active group
    /// (nil clears the active group).
    pub(crate) fn send_activate_group(
        &mut self,
        group_id: Uuid,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ActivateGroup(ActivateGroup {
            agent_data: ActivateGroupAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
                group_id,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `GroupMembersRequest` reliably for `group_id`.
    pub(crate) fn send_group_members_request(
        &mut self,
        group_id: Uuid,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::GroupMembersRequest(GroupMembersRequest {
            agent_data: GroupMembersRequestAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            group_data: GroupMembersRequestGroupDataBlock {
                group_id,
                request_id: Uuid::nil(),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `GroupRoleDataRequest` reliably for `group_id`.
    pub(crate) fn send_group_role_data_request(
        &mut self,
        group_id: Uuid,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::GroupRoleDataRequest(GroupRoleDataRequest {
            agent_data: GroupRoleDataRequestAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            group_data: GroupRoleDataRequestGroupDataBlock {
                group_id,
                request_id: Uuid::nil(),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `GroupRoleMembersRequest` reliably for `group_id`.
    pub(crate) fn send_group_role_members_request(
        &mut self,
        group_id: Uuid,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::GroupRoleMembersRequest(GroupRoleMembersRequest {
            agent_data: GroupRoleMembersRequestAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            group_data: GroupRoleMembersRequestGroupDataBlock {
                group_id,
                request_id: Uuid::nil(),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `GroupTitlesRequest` reliably for `group_id`.
    pub(crate) fn send_group_titles_request(
        &mut self,
        group_id: Uuid,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::GroupTitlesRequest(GroupTitlesRequest {
            agent_data: GroupTitlesRequestAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
                group_id,
                request_id: Uuid::nil(),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `GroupProfileRequest` reliably for `group_id`.
    pub(crate) fn send_group_profile_request(
        &mut self,
        group_id: Uuid,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::GroupProfileRequest(GroupProfileRequest {
            agent_data: GroupProfileRequestAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            group_data: GroupProfileRequestGroupDataBlock { group_id },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `GroupNoticesListRequest` reliably for `group_id`.
    pub(crate) fn send_group_notices_list_request(
        &mut self,
        group_id: Uuid,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::GroupNoticesListRequest(GroupNoticesListRequest {
            agent_data: GroupNoticesListRequestAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            data: GroupNoticesListRequestDataBlock { group_id },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `GroupNoticeRequest` reliably for the notice `notice_id`.
    pub(crate) fn send_group_notice_request(
        &mut self,
        notice_id: Uuid,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::GroupNoticeRequest(GroupNoticeRequest {
            agent_data: GroupNoticeRequestAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            data: GroupNoticeRequestDataBlock {
                group_notice_id: notice_id,
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
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            group_data: CreateGroupRequestGroupDataBlock {
                name: with_nul(&params.name),
                charter: with_nul(&params.charter),
                show_in_list: params.show_in_list,
                insignia_id: params.insignia_id,
                membership_fee: params.membership_fee,
                open_enrollment: params.open_enrollment,
                allow_publish: params.allow_publish,
                mature_publish: params.mature_publish,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `JoinGroupRequest` reliably for `group_id`.
    pub(crate) fn send_join_group_request(
        &mut self,
        group_id: Uuid,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::JoinGroupRequest(JoinGroupRequest {
            agent_data: JoinGroupRequestAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            group_data: JoinGroupRequestGroupDataBlock { group_id },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `LeaveGroupRequest` reliably for `group_id`.
    pub(crate) fn send_leave_group_request(
        &mut self,
        group_id: Uuid,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::LeaveGroupRequest(LeaveGroupRequest {
            agent_data: LeaveGroupRequestAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            group_data: LeaveGroupRequestGroupDataBlock { group_id },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `InviteGroupRequest` reliably inviting `invitees` (each an
    /// `(invitee_id, role_id)` pair, nil `role_id` for the default Everyone role)
    /// to `group_id`.
    pub(crate) fn send_invite_group_request(
        &mut self,
        group_id: Uuid,
        invitees: &[(Uuid, Uuid)],
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::InviteGroupRequest(InviteGroupRequest {
            agent_data: InviteGroupRequestAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            group_data: InviteGroupRequestGroupDataBlock { group_id },
            invite_data: invitees
                .iter()
                .map(|(invitee_id, role_id)| InviteGroupRequestInviteDataBlock {
                    invitee_id: *invitee_id,
                    role_id: *role_id,
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `SetGroupAcceptNotices` reliably for `group_id`.
    pub(crate) fn send_set_group_accept_notices(
        &mut self,
        group_id: Uuid,
        accept_notices: bool,
        list_in_profile: bool,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::SetGroupAcceptNotices(SetGroupAcceptNotices {
            agent_data: SetGroupAcceptNoticesAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            data: SetGroupAcceptNoticesDataBlock {
                group_id,
                accept_notices,
            },
            new_data: SetGroupAcceptNoticesNewDataBlock { list_in_profile },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `SetGroupContribution` reliably for `group_id`.
    pub(crate) fn send_set_group_contribution(
        &mut self,
        group_id: Uuid,
        contribution: i32,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::SetGroupContribution(SetGroupContribution {
            agent_data: SetGroupContributionAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            data: SetGroupContributionDataBlock {
                group_id,
                contribution,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `GroupRoleUpdate` reliably, carrying one `RoleData` block per
    /// role create/update/delete in `roles`.
    pub(crate) fn send_group_role_update(
        &mut self,
        group_id: Uuid,
        roles: &[GroupRoleEdit],
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::GroupRoleUpdate(GroupRoleUpdate {
            agent_data: GroupRoleUpdateAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
                group_id,
            },
            role_data: roles
                .iter()
                .map(|role| GroupRoleUpdateRoleDataBlock {
                    role_id: role.role_id,
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
        group_id: Uuid,
        changes: &[GroupRoleMemberChange],
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::GroupRoleChanges(GroupRoleChanges {
            agent_data: GroupRoleChangesAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
                group_id,
            },
            role_change: changes
                .iter()
                .map(|change| GroupRoleChangesRoleChangeBlock {
                    role_id: change.role_id,
                    member_id: change.member_id,
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
        group_id: Uuid,
        member_ids: &[Uuid],
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::EjectGroupMemberRequest(EjectGroupMemberRequest {
            agent_data: EjectGroupMemberRequestAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            group_data: EjectGroupMemberRequestGroupDataBlock { group_id },
            eject_data: member_ids
                .iter()
                .map(|ejectee_id| EjectGroupMemberRequestEjectDataBlock {
                    ejectee_id: *ejectee_id,
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a group IM (`ImprovedInstantMessage`) reliably: the session id and
    /// recipient are both `group_id`, as group chat requires. `dialog` selects
    /// start/send/leave.
    pub(crate) fn send_group_session_im(
        &mut self,
        group_id: Uuid,
        dialog: ImDialog,
        message: &str,
        from_name: &str,
        now: Instant,
    ) -> Result<(), WireError> {
        let im = AnyMessage::ImprovedInstantMessage(ImprovedInstantMessage {
            agent_data: ImprovedInstantMessageAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            message_block: ImprovedInstantMessageMessageBlockBlock {
                from_group: false,
                to_agent_id: group_id,
                parent_estate_id: 0,
                region_id: Uuid::nil(),
                position: Vector {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                offline: 0,
                dialog: dialog.to_u8(),
                id: group_id,
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
        object_id: Uuid,
        chat_channel: i32,
        button_index: i32,
        button_label: &str,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ScriptDialogReply(ScriptDialogReply {
            agent_data: ScriptDialogReplyAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            data: ScriptDialogReplyDataBlock {
                object_id,
                chat_channel,
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
        task_id: Uuid,
        item_id: Uuid,
        permissions: i32,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ScriptAnswerYes(ScriptAnswerYes {
            agent_data: ScriptAnswerYesAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            data: ScriptAnswerYesDataBlock {
                task_id,
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
                agent_id: self.agent_id,
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
                agent_id: self.agent_id,
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
                agent_id: self.agent_id,
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
        xfer_id: u64,
        filename: &str,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::RequestXfer(RequestXfer {
            xfer_id: RequestXferXferIDBlock {
                id: xfer_id,
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
        xfer_id: u64,
        packet: u32,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ConfirmXferPacket(ConfirmXferPacket {
            xfer_id: ConfirmXferPacketXferIDBlock {
                id: xfer_id,
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
        xfer_id: u64,
        packet: u32,
        data: Vec<u8>,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::SendXferPacket(SendXferPacket {
            xfer_id: SendXferPacketXferIDBlock {
                id: xfer_id,
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
        image_id: Uuid,
        discard_level: i8,
        priority: f32,
        packet: u32,
        image_type: u8,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::RequestImage(RequestImage {
            agent_data: RequestImageAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            request_image: vec![RequestImageRequestImageBlock {
                image: image_id,
                discard_level,
                download_priority: priority,
                packet,
                r#type: image_type,
            }],
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `TransferRequest` reliably to download a generic asset over the
    /// transfer path: channel `LLTCT_ASSET` (2), source `LLTST_ASSET` (2), and a
    /// `Params` block of the asset id (16 bytes) followed by its `LLAssetType`
    /// code (a little-endian `i32`), matching the viewer's `LLTransferSourceAsset`.
    pub(crate) fn send_transfer_request(
        &mut self,
        transfer_id: Uuid,
        asset_id: Uuid,
        asset_type: AssetType,
        priority: f32,
        now: Instant,
    ) -> Result<(), WireError> {
        // LLTCT_ASSET / LLTST_ASSET.
        const CHANNEL_ASSET: i32 = 2;
        const SOURCE_ASSET: i32 = 2;
        // The viewer's `LLTransferSourceAsset` params: the asset UUID followed
        // by its `LLAssetType` code as a little-endian `i32`.
        let mut writer = Writer::new();
        writer.put_uuid(asset_id);
        writer.put_i32(asset_type.to_code());
        let message = AnyMessage::TransferRequest(TransferRequest {
            transfer_info: TransferRequestTransferInfoBlock {
                transfer_id,
                channel_type: CHANNEL_ASSET,
                source_type: SOURCE_ASSET,
                priority,
                params: writer.into_bytes(),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `AgentWearablesRequest` reliably, asking the simulator to
    /// (re-)send the agent's current wearables as an `AgentWearablesUpdate`.
    pub(crate) fn send_agent_wearables_request(&mut self, now: Instant) -> Result<(), WireError> {
        let message = AnyMessage::AgentWearablesRequest(AgentWearablesRequest {
            agent_data: AgentWearablesRequestAgentDataBlock {
                agent_id: self.agent_id,
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
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            wearable_data: wearables
                .iter()
                .map(|wearable| AgentIsNowWearingWearableDataBlock {
                    item_id: wearable.item_id,
                    wearable_type: wearable.wearable_type.to_code(),
                })
                .collect(),
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
                agent_id: self.agent_id,
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
                agent_id: self.agent_id,
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
                agent_id: self.agent_id,
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
    /// (sorted by name), requesting its sub-folders and items.
    pub(crate) fn send_fetch_inventory_descendents(
        &mut self,
        folder_id: Uuid,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::FetchInventoryDescendents(FetchInventoryDescendents {
            agent_data: FetchInventoryDescendentsAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            inventory_data: FetchInventoryDescendentsInventoryDataBlock {
                folder_id,
                // Own inventory: the owner is the agent itself.
                owner_id: self.agent_id,
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
                agent_id: self.agent_id,
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
                agent_id: self.agent_id,
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
                agent_id: self.agent_id,
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
                agent_id: self.agent_id,
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
        callback_id: u32,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::CreateInventoryItem(CreateInventoryItem {
            agent_data: CreateInventoryItemAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            inventory_block: CreateInventoryItemInventoryBlockBlock {
                callback_id,
                folder_id: new.folder_id,
                transaction_id: new.transaction_id,
                next_owner_mask: new.next_owner_mask,
                r#type: new.asset_type,
                inv_type: new.inv_type,
                wearable_type: new.wearable_type,
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
        callback_id: u32,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::UpdateInventoryItem(UpdateInventoryItem {
            agent_data: UpdateInventoryItemAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
                transaction_id,
            },
            inventory_data: vec![UpdateInventoryItemInventoryDataBlock {
                item_id: item.item_id,
                folder_id: item.folder_id,
                callback_id,
                creator_id: item.creator_id,
                owner_id: item.owner_id,
                group_id: item.group_id,
                base_mask: item.base_mask,
                owner_mask: item.owner_mask,
                group_mask: item.group_mask,
                everyone_mask: item.everyone_mask,
                next_owner_mask: item.next_owner_mask,
                group_owned: item.group_owned,
                transaction_id,
                r#type: item.item_type,
                inv_type: item.inv_type,
                flags: item.flags,
                sale_type: item.sale_type,
                sale_price: item.sale_price,
                name: with_nul(&item.name),
                description: with_nul(&item.description),
                creation_date: item.creation_date,
                crc: inventory_item_crc(item),
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
                agent_id: self.agent_id,
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
        old_agent_id: Uuid,
        old_item_id: Uuid,
        new_folder_id: Uuid,
        new_name: &str,
        callback_id: u32,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::CopyInventoryItem(CopyInventoryItem {
            agent_data: CopyInventoryItemAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            inventory_data: vec![CopyInventoryItemInventoryDataBlock {
                callback_id,
                old_agent_id,
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
                agent_id: self.agent_id,
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
                agent_id: self.agent_id,
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
                agent_id: self.agent_id,
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
                agent_id: self.agent_id,
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
                agent_id: self.agent_id,
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

    /// Queues a `LogoutRequest` reliably.
    pub(crate) fn send_logout_request(&mut self, now: Instant) -> Result<(), WireError> {
        let message = AnyMessage::LogoutRequest(LogoutRequest {
            agent_data: LogoutRequestAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `RequestRegionInfo` reliably.
    pub(crate) fn send_request_region_info(&mut self, now: Instant) -> Result<(), WireError> {
        let message = AnyMessage::RequestRegionInfo(RequestRegionInfo {
            agent_data: RequestRegionInfoAgentDataBlock {
                agent_id: self.agent_id,
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
                agent_id: self.agent_id,
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
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            money_data: MoneyTransferRequestMoneyDataBlock {
                source_id: self.agent_id,
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
                agent_id: self.agent_id,
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
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            parcel_data: ParcelPropertiesUpdateParcelDataBlock {
                local_id: update.local_id,
                // The message-level flag the reference viewer sends (0x01).
                flags: 0x1,
                parcel_flags: update.parcel_flags.bits(),
                sale_price: update.sale_price,
                name: with_nul(&update.name),
                desc: with_nul(&update.description),
                music_url: with_nul(&update.music_url),
                media_url: with_nul(&update.media_url),
                media_id: update.media_id,
                media_auto_scale: u8::from(update.media_auto_scale),
                group_id: update.group_id,
                pass_price: update.pass_price,
                pass_hours: update.pass_hours,
                category: update.category.to_u8(),
                auth_buyer_id: update.auth_buyer_id,
                snapshot_id: update.snapshot_id,
                user_location: update.user_location.clone(),
                user_look_at: update.user_look_at.clone(),
                landing_type: update.landing_type,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `ParcelAccessListRequest` reliably (fetch the allow or ban list
    /// selected by `flags`). The reply is a `ParcelAccessListReply`.
    pub(crate) fn send_parcel_access_list_request(
        &mut self,
        local_id: i32,
        flags: u32,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ParcelAccessListRequest(ParcelAccessListRequest {
            agent_data: ParcelAccessListRequestAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            data: ParcelAccessListRequestDataBlock {
                sequence_id: 0,
                flags,
                local_id,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `ParcelAccessListUpdate` reliably (replace the allow or ban list
    /// selected by `flags`). An empty list clears it (sent as one empty entry, as
    /// the reference viewer does).
    pub(crate) fn send_parcel_access_list_update(
        &mut self,
        local_id: i32,
        flags: u32,
        entries: &[ParcelAccessEntry],
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
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            data: ParcelAccessListUpdateDataBlock {
                flags,
                local_id,
                transaction_id: Uuid::nil(),
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
        local_id: i32,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ParcelDwellRequest(ParcelDwellRequest {
            agent_data: ParcelDwellRequestAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            // The simulator fills in parcel_id from local_id.
            data: ParcelDwellRequestDataBlock {
                local_id,
                parcel_id: Uuid::nil(),
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `ParcelBuy` reliably (purchase the parcel).
    pub(crate) fn send_parcel_buy(
        &mut self,
        local_id: i32,
        price: i32,
        area: i32,
        group_id: Uuid,
        is_group_owned: bool,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ParcelBuy(ParcelBuy {
            agent_data: ParcelBuyAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            data: ParcelBuyDataBlock {
                group_id,
                is_group_owned,
                remove_contribution: false,
                local_id,
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
        local_id: i32,
        return_type: u32,
        owner_ids: &[Uuid],
        task_ids: &[Uuid],
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ParcelReturnObjects(ParcelReturnObjects {
            agent_data: ParcelReturnObjectsAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            parcel_data: ParcelReturnObjectsParcelDataBlock {
                local_id,
                return_type,
            },
            task_i_ds: task_ids
                .iter()
                .map(|id| ParcelReturnObjectsTaskIDsBlock { task_id: *id })
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
        local_id: i32,
        return_type: u32,
        object_ids: &[Uuid],
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ParcelSelectObjects(ParcelSelectObjects {
            agent_data: ParcelSelectObjectsAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            parcel_data: ParcelSelectObjectsParcelDataBlock {
                local_id,
                return_type,
            },
            return_i_ds: object_ids
                .iter()
                .map(|id| ParcelSelectObjectsReturnIDsBlock { return_id: *id })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `ParcelDeedToGroup` reliably (deed the parcel to `group_id`).
    pub(crate) fn send_parcel_deed_to_group(
        &mut self,
        local_id: i32,
        group_id: Uuid,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ParcelDeedToGroup(ParcelDeedToGroup {
            agent_data: ParcelDeedToGroupAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            data: ParcelDeedToGroupDataBlock { group_id, local_id },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `ParcelReclaim` reliably (reclaim the parcel to the estate).
    pub(crate) fn send_parcel_reclaim(
        &mut self,
        local_id: i32,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ParcelReclaim(ParcelReclaim {
            agent_data: ParcelReclaimAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            data: ParcelReclaimDataBlock { local_id },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `ParcelRelease` reliably (abandon the parcel back to the estate).
    pub(crate) fn send_parcel_release(
        &mut self,
        local_id: i32,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ParcelRelease(ParcelRelease {
            agent_data: ParcelReleaseAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            data: ParcelReleaseDataBlock { local_id },
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
                agent_id: self.agent_id,
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
                agent_id: self.agent_id,
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
                god_id: self.agent_id,
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
                agent_id: self.agent_id,
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
                agent_id: self.agent_id,
                session_id: self.session_id,
                // The viewer's map-layer flag (2); estate/godlike filled by the sim.
                flags: MAP_LAYER_FLAG,
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
                agent_id: self.agent_id,
                session_id: self.session_id,
                flags: MAP_LAYER_FLAG,
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

    /// Queues a `RequestMultipleObjects` reliably, asking the simulator to (re)send
    /// the full `ObjectUpdate` for each local id (cache-miss type "full" = 0).
    pub(crate) fn send_request_multiple_objects(
        &mut self,
        local_ids: &[u32],
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::RequestMultipleObjects(RequestMultipleObjects {
            agent_data: RequestMultipleObjectsAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            object_data: local_ids
                .iter()
                .map(|id| RequestMultipleObjectsObjectDataBlock {
                    cache_miss_type: 0,
                    id: *id,
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectSelect` reliably for the given local ids. Selecting an
    /// object makes the simulator send its `ObjectProperties`.
    pub(crate) fn send_object_select(
        &mut self,
        local_ids: &[u32],
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectSelect(ObjectSelect {
            agent_data: ObjectSelectAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            object_data: local_ids
                .iter()
                .map(|id| ObjectSelectObjectDataBlock {
                    object_local_id: *id,
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectDeselect` reliably for the given local ids.
    pub(crate) fn send_object_deselect(
        &mut self,
        local_ids: &[u32],
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectDeselect(ObjectDeselect {
            agent_data: ObjectDeselectAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            object_data: local_ids
                .iter()
                .map(|id| ObjectDeselectObjectDataBlock {
                    object_local_id: *id,
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
        local_id: u32,
        grab_offset: Vector,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectGrab(ObjectGrab {
            agent_data: ObjectGrabAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            object_data: ObjectGrabObjectDataBlock {
                local_id,
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
        object_id: Uuid,
        grab_offset_initial: Vector,
        grab_position: Vector,
        time_since_last: u32,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectGrabUpdate(ObjectGrabUpdate {
            agent_data: ObjectGrabUpdateAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            object_data: ObjectGrabUpdateObjectDataBlock {
                object_id,
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
        local_id: u32,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectDeGrab(ObjectDeGrab {
            agent_data: ObjectDeGrabAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            object_data: ObjectDeGrabObjectDataBlock { local_id },
            surface_info: Vec::new(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectAdd` reliably to rez a new primitive from `shape`.
    pub(crate) fn send_object_add(
        &mut self,
        shape: &PrimShape,
        group_id: Uuid,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectAdd(ObjectAdd {
            agent_data: ObjectAddAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
                group_id,
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
        local_ids: &[u32],
        offset: Vector,
        group_id: Uuid,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectDuplicate(ObjectDuplicate {
            agent_data: ObjectDuplicateAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
                group_id,
            },
            shared_data: ObjectDuplicateSharedDataBlock {
                offset,
                duplicate_flags: 0,
            },
            object_data: local_ids
                .iter()
                .map(|id| ObjectDuplicateObjectDataBlock {
                    object_local_id: *id,
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectDelete` reliably for `local_ids` (non-god, non-forced).
    pub(crate) fn send_object_delete(
        &mut self,
        local_ids: &[u32],
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectDelete(ObjectDelete {
            agent_data: ObjectDeleteAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
                force: false,
            },
            object_data: local_ids
                .iter()
                .map(|id| ObjectDeleteObjectDataBlock {
                    object_local_id: *id,
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `DeRezObject` reliably (take/return/trash `local_ids`).
    pub(crate) fn send_derez_object(
        &mut self,
        local_ids: &[u32],
        destination: DeRezDestination,
        destination_id: Uuid,
        transaction_id: Uuid,
        group_id: Uuid,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::DeRezObject(DeRezObject {
            agent_data: DeRezObjectAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            agent_block: DeRezObjectAgentBlockBlock {
                group_id,
                destination: destination.to_code(),
                destination_id,
                transaction_id,
                // The whole selection fits in one packet.
                packet_count: 1,
                packet_number: 0,
            },
            object_data: local_ids
                .iter()
                .map(|id| DeRezObjectObjectDataBlock {
                    object_local_id: *id,
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectName` reliably (rename `local_id`).
    pub(crate) fn send_object_name(
        &mut self,
        local_id: u32,
        name: &str,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectName(ObjectName {
            agent_data: ObjectNameAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            object_data: vec![ObjectNameObjectDataBlock {
                local_id,
                name: name.as_bytes().to_vec(),
            }],
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectDescription` reliably (re-describe `local_id`).
    pub(crate) fn send_object_description(
        &mut self,
        local_id: u32,
        description: &str,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectDescription(ObjectDescription {
            agent_data: ObjectDescriptionAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            object_data: vec![ObjectDescriptionObjectDataBlock {
                local_id,
                description: description.as_bytes().to_vec(),
            }],
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectClickAction` reliably (set the left-click behaviour).
    pub(crate) fn send_object_click_action(
        &mut self,
        local_id: u32,
        action: ClickAction,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectClickAction(ObjectClickAction {
            agent_data: ObjectClickActionAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            object_data: vec![ObjectClickActionObjectDataBlock {
                object_local_id: local_id,
                click_action: action.to_code(),
            }],
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectMaterial` reliably (set the physical material).
    pub(crate) fn send_object_material(
        &mut self,
        local_id: u32,
        material: Material,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectMaterial(ObjectMaterial {
            agent_data: ObjectMaterialAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            object_data: vec![ObjectMaterialObjectDataBlock {
                object_local_id: local_id,
                material: material.to_code(),
            }],
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectFlagUpdate` reliably (set physics/temporary/phantom).
    pub(crate) fn send_object_flag_update(
        &mut self,
        local_id: u32,
        flags: &ObjectFlagSettings,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectFlagUpdate(ObjectFlagUpdate {
            agent_data: ObjectFlagUpdateAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
                object_local_id: local_id,
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

    /// Queues an `ObjectGroup` reliably (set the group `local_ids` are set to).
    pub(crate) fn send_object_group(
        &mut self,
        local_ids: &[u32],
        group_id: Uuid,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectGroup(ObjectGroup {
            agent_data: ObjectGroupAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
                group_id,
            },
            object_data: local_ids
                .iter()
                .map(|id| ObjectGroupObjectDataBlock {
                    object_local_id: *id,
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectPermissions` reliably (set/clear `mask` bits of `field`).
    pub(crate) fn send_object_permissions(
        &mut self,
        local_ids: &[u32],
        field: PermissionField,
        set: bool,
        mask: u32,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectPermissions(ObjectPermissions {
            agent_data: ObjectPermissionsAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            header_data: ObjectPermissionsHeaderDataBlock { r#override: false },
            object_data: local_ids
                .iter()
                .map(|id| ObjectPermissionsObjectDataBlock {
                    object_local_id: *id,
                    field: field.to_code(),
                    set: u8::from(set),
                    mask,
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectSaleInfo` reliably (set the sale type and price).
    pub(crate) fn send_object_sale_info(
        &mut self,
        local_id: u32,
        sale_type: SaleType,
        sale_price: i32,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectSaleInfo(ObjectSaleInfo {
            agent_data: ObjectSaleInfoAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            object_data: vec![ObjectSaleInfoObjectDataBlock {
                local_id,
                sale_type: sale_type.to_code(),
                sale_price,
            }],
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectCategory` reliably (set the object's category code).
    pub(crate) fn send_object_category(
        &mut self,
        local_id: u32,
        category: u32,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectCategory(ObjectCategory {
            agent_data: ObjectCategoryAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            object_data: vec![ObjectCategoryObjectDataBlock { local_id, category }],
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectIncludeInSearch` reliably (toggle search visibility).
    pub(crate) fn send_object_include_in_search(
        &mut self,
        local_id: u32,
        include: bool,
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectIncludeInSearch(ObjectIncludeInSearch {
            agent_data: ObjectIncludeInSearchAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            object_data: vec![ObjectIncludeInSearchObjectDataBlock {
                object_local_id: local_id,
                include_in_search: include,
            }],
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectLink` reliably linking `local_ids` (the first id becomes
    /// the linkset root).
    pub(crate) fn send_object_link(
        &mut self,
        local_ids: &[u32],
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectLink(ObjectLink {
            agent_data: ObjectLinkAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            object_data: local_ids
                .iter()
                .map(|id| ObjectLinkObjectDataBlock {
                    object_local_id: *id,
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues an `ObjectDelink` reliably unlinking `local_ids`.
    pub(crate) fn send_object_delink(
        &mut self,
        local_ids: &[u32],
        now: Instant,
    ) -> Result<(), WireError> {
        let message = AnyMessage::ObjectDelink(ObjectDelink {
            agent_data: ObjectDelinkAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            object_data: local_ids
                .iter()
                .map(|id| ObjectDelinkObjectDataBlock {
                    object_local_id: *id,
                })
                .collect(),
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `MultipleObjectUpdate` reliably applying `transform` to `local_id`.
    /// The packed `Data` blob carries position/rotation/scale in that fixed
    /// order, matching the simulator's `MultipleObjectUpdate` parser.
    pub(crate) fn send_multiple_object_update(
        &mut self,
        local_id: u32,
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
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
            object_data: vec![MultipleObjectUpdateObjectDataBlock {
                object_local_id: local_id,
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
    pub(crate) fn queue_ack(&mut self, sequence: u32, now: Instant) {
        self.pending_acks.push(sequence);
        if self.timers.ack_flush.is_none() {
            self.timers.ack_flush = Some(deadline(now, ACK_FLUSH_DELAY));
        }
    }

    /// Removes the given outgoing sequence numbers from the unacked set.
    pub(crate) fn record_acks(&mut self, ids: &[u32]) {
        for id in ids {
            self.unacked.remove(id);
        }
    }

    /// Records an inbound reliable `sequence`; returns `true` if it is new.
    pub(crate) fn mark_seen(&mut self, sequence: u32) -> bool {
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
                .map(|id| PacketAckPacketsBlock { id: *id })
                .collect();
            let message = AnyMessage::PacketAck(PacketAck { packets });
            self.send(&message, Reliability::Unreliable, now)?;
        }
        Ok(())
    }

    /// Retransmits unacknowledged reliable packets whose timeout has elapsed.
    ///
    /// Returns `true` if any packet has exhausted its retransmission budget.
    pub(crate) fn process_resends(&mut self, now: Instant) -> bool {
        let mut exhausted = false;
        let mut to_send = Vec::new();
        for packet in self.unacked.values_mut() {
            if now < deadline(packet.sent_at, RESEND_TIMEOUT) {
                continue;
            }
            if packet.attempts >= MAX_RESEND_ATTEMPTS {
                exhausted = true;
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
