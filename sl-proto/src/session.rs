//! The sans-I/O session state machine: login, circuit establishment,
//! keep-alive, and clean logout, driven entirely by passed-in time.

use std::collections::{BTreeMap, HashSet, VecDeque};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::{Duration, Instant};

use sl_types::lsl::{Rotation, Vector};
use sl_types::money::LindenAmount;
use sl_wire::messages::{
    AcceptFriendship, AcceptFriendshipAgentDataBlock, AcceptFriendshipFolderDataBlock,
    AcceptFriendshipTransactionBlockBlock, AgentRequestSit, AgentRequestSitAgentDataBlock,
    AgentRequestSitTargetObjectBlock, AgentSit, AgentSitAgentDataBlock, AgentUpdate,
    AgentUpdateAgentDataBlock, AvatarGroupsReplyGroupDataBlock,
    AvatarInterestsReplyPropertiesDataBlock, AvatarPropertiesReplyPropertiesDataBlock,
    AvatarPropertiesRequest, AvatarPropertiesRequestAgentDataBlock, ChatFromSimulatorChatDataBlock,
    ChatFromViewer, ChatFromViewerAgentDataBlock, ChatFromViewerChatDataBlock,
    CompleteAgentMovement, CompleteAgentMovementAgentDataBlock, CompletePingCheck,
    CompletePingCheckPingIDBlock, DeclineFriendship, DeclineFriendshipAgentDataBlock,
    DeclineFriendshipTransactionBlockBlock, EnableSimulatorSimulatorInfoBlock,
    FetchInventoryDescendents, FetchInventoryDescendentsAgentDataBlock,
    FetchInventoryDescendentsInventoryDataBlock, GenericMessage, GenericMessageAgentDataBlock,
    GenericMessageMethodDataBlock, GenericMessageParamListBlock, GrantUserRights,
    GrantUserRightsAgentDataBlock, GrantUserRightsRightsBlock, ImprovedInstantMessage,
    ImprovedInstantMessageAgentDataBlock, ImprovedInstantMessageEstateBlockBlock,
    ImprovedInstantMessageMessageBlockBlock, InventoryDescendentsFolderDataBlock,
    InventoryDescendentsItemDataBlock, LogoutRequest, LogoutRequestAgentDataBlock,
    MapBlockReplyDataBlock, MapBlockReplySizeBlock, MapBlockRequest, MapBlockRequestAgentDataBlock,
    MapBlockRequestPositionDataBlock, MapItemRequest, MapItemRequestAgentDataBlock,
    MapItemRequestRequestDataBlock, MapNameRequest, MapNameRequestAgentDataBlock,
    MapNameRequestNameDataBlock, PacketAck, PacketAckPacketsBlock, ParcelPropertiesParcelDataBlock,
    ParcelPropertiesRequest, ParcelPropertiesRequestAgentDataBlock,
    ParcelPropertiesRequestParcelDataBlock, RegionHandshakeRegionInfo3Block,
    RegionHandshakeRegionInfoBlock, RegionHandshakeReply, RegionHandshakeReplyAgentDataBlock,
    RegionHandshakeReplyRegionInfoBlock, RegionInfoRegionInfo2Block, RegionInfoRegionInfoBlock,
    RequestRegionInfo, RequestRegionInfoAgentDataBlock, TeleportLocationRequest,
    TeleportLocationRequestAgentDataBlock, TeleportLocationRequestInfoBlock, TerminateFriendship,
    TerminateFriendshipAgentDataBlock, TerminateFriendshipExBlockBlock, UseCircuitCode,
    UseCircuitCodeCircuitCodeBlock,
};
// Script dialogs & permissions (#8): the outgoing reply messages.
use sl_wire::messages::{
    ScriptAnswerYes, ScriptAnswerYesAgentDataBlock, ScriptAnswerYesDataBlock, ScriptDialogReply,
    ScriptDialogReplyAgentDataBlock, ScriptDialogReplyDataBlock,
};
// Mute list (#9): the outgoing mute-edit messages and the Xfer download messages.
use sl_wire::messages::{
    ConfirmXferPacket, ConfirmXferPacketXferIDBlock, MuteListRequest,
    MuteListRequestAgentDataBlock, MuteListRequestMuteDataBlock, RemoveMuteListEntry,
    RemoveMuteListEntryAgentDataBlock, RemoveMuteListEntryMuteDataBlock, RequestXfer,
    RequestXferXferIDBlock, UpdateMuteListEntry, UpdateMuteListEntryAgentDataBlock,
    UpdateMuteListEntryMuteDataBlock,
};
// Money / economy (#11): the outgoing balance/economy/transfer requests.
use sl_wire::messages::{
    EconomyDataRequest, MoneyBalanceRequest, MoneyBalanceRequestAgentDataBlock,
    MoneyBalanceRequestMoneyDataBlock, MoneyTransferRequest, MoneyTransferRequestAgentDataBlock,
    MoneyTransferRequestMoneyDataBlock,
};
// Group support (#7): incoming reply blocks consumed by the converter helpers.
use sl_wire::messages::{
    AgentDataUpdateAgentDataBlock, AgentGroupDataUpdateGroupDataBlock,
    GroupMembersReplyMemberDataBlock, GroupNoticesListReplyDataBlock,
    GroupProfileReplyGroupDataBlock, GroupRoleDataReplyRoleDataBlock,
    GroupTitlesReplyGroupDataBlock,
};
// Group support (#7): the outgoing group messages and their blocks.
use sl_wire::messages::{
    ActivateGroup, ActivateGroupAgentDataBlock, CreateGroupRequest,
    CreateGroupRequestAgentDataBlock, CreateGroupRequestGroupDataBlock, GroupMembersRequest,
    GroupMembersRequestAgentDataBlock, GroupMembersRequestGroupDataBlock, GroupNoticeRequest,
    GroupNoticeRequestAgentDataBlock, GroupNoticeRequestDataBlock, GroupNoticesListRequest,
    GroupNoticesListRequestAgentDataBlock, GroupNoticesListRequestDataBlock, GroupProfileRequest,
    GroupProfileRequestAgentDataBlock, GroupProfileRequestGroupDataBlock, GroupRoleDataRequest,
    GroupRoleDataRequestAgentDataBlock, GroupRoleDataRequestGroupDataBlock,
    GroupRoleMembersRequest, GroupRoleMembersRequestAgentDataBlock,
    GroupRoleMembersRequestGroupDataBlock, GroupTitlesRequest, GroupTitlesRequestAgentDataBlock,
    InviteGroupRequest, InviteGroupRequestAgentDataBlock, InviteGroupRequestGroupDataBlock,
    InviteGroupRequestInviteDataBlock, JoinGroupRequest, JoinGroupRequestAgentDataBlock,
    JoinGroupRequestGroupDataBlock, LeaveGroupRequest, LeaveGroupRequestAgentDataBlock,
    LeaveGroupRequestGroupDataBlock, SetGroupAcceptNotices, SetGroupAcceptNoticesAgentDataBlock,
    SetGroupAcceptNoticesDataBlock, SetGroupAcceptNoticesNewDataBlock, SetGroupContribution,
    SetGroupContributionAgentDataBlock, SetGroupContributionDataBlock,
};
use sl_wire::{
    AnyMessage, ControlFlags, Llsd, MessageId, PacketFlags, Reader, SkeletonFolder, WireError,
    Writer, build_login_request, encode_datagram, parse_datagram, zero_decode,
};
use uuid::Uuid;

use crate::error::Error;
use crate::types::{
    ActiveGroup, AvatarGroupMembership, AvatarInterests, AvatarPick, AvatarProperties, ChatAudible,
    ChatMessage, ChatSourceType, ChatType, CreateGroupParams, DisconnectReason, EconomyData, Event,
    Friend, FriendRights, GroupMember, GroupMembership, GroupNotice, GroupProfile, GroupRole,
    GroupRoleMember, GroupTitle, ImDialog, InstantMessage, InventoryFolder, InventoryItem,
    LoadUrlRequest, LoginHttpRequest, LoginParams, MapItem, MapItemType, MapRegionInfo, Maturity,
    MoneyBalance, MoneyTransaction, MoneyTransactionType, MuteEntry, MuteFlags, MuteType,
    NeighborInfo, ParcelInfo, ParcelOverlayInfo, ProductType, RegionIdentity, RegionLimits,
    Reliability, ScriptDialog, ScriptPermissionRequest, ScriptPermissions, ScriptTeleportRequest,
    Transmit, grid_to_handle, handle_to_grid,
};

/// How often an `AgentUpdate` is sent to keep the agent active.
const AGENT_UPDATE_INTERVAL: Duration = Duration::from_millis(1000);
/// How long owed acknowledgements may wait before being flushed as a `PacketAck`.
const ACK_FLUSH_DELAY: Duration = Duration::from_millis(150);
/// How long without inbound traffic before the link is considered dead. Kept
/// well under OpenSim's 60-second `AckTimeout`.
const INACTIVITY_TIMEOUT: Duration = Duration::from_secs(45);
/// How long to wait for a `LogoutReply` before giving up on a clean logout.
const LOGOUT_TIMEOUT: Duration = Duration::from_secs(5);
/// The retransmission timeout for an unacknowledged reliable packet.
const RESEND_TIMEOUT: Duration = Duration::from_millis(1500);
/// The maximum number of times a reliable packet is sent before giving up.
const MAX_RESEND_ATTEMPTS: u32 = 6;
/// The maximum number of inbound reliable sequence numbers remembered for
/// duplicate suppression.
const SEEN_CAPACITY: usize = 4096;
/// The maximum number of acknowledgements packed into a single `PacketAck`.
const MAX_ACKS_PER_PACKET: usize = 255;
/// How long to wait for a `TeleportFinish` before declaring the teleport failed.
const TELEPORT_TIMEOUT: Duration = Duration::from_secs(30);
/// The default draw distance (metres) advertised in keep-alive `AgentUpdate`s,
/// large enough that the simulator enables the neighbouring regions.
const DEFAULT_DRAW_DISTANCE: f32 = 256.0;
/// The world-map layer flag the viewer sends on map name/item requests (the
/// terrain layer; `LAYER_FLAG` in the reference viewer).
const MAP_LAYER_FLAG: u32 = 2;
/// The identity (no-op) rotation: the default body/head facing.
const IDENTITY_ROTATION: Rotation = Rotation {
    x: 0.0,
    y: 0.0,
    z: 0.0,
    s: 1.0,
};

/// The HTTP capability for fetching inventory folder contents (a POST of an LLSD
/// folder list). Used as the seed capability name, the request cap, and the
/// message tag a driver feeds back via [`Session::handle_caps_event`].
pub const CAP_FETCH_INVENTORY: &str = "FetchInventoryDescendents2";

/// The HTTP capability for fetching a group's full member roster (a POST of an
/// LLSD `{ group_id }` map — the modern Second Life path that replaces the UDP
/// `GroupMembersRequest`/`Reply`). The LLSD response is decoded by
/// [`Session::handle_caps_event`] into [`Event::GroupMembers`].
pub const CAP_GROUP_MEMBER_DATA: &str = "GroupMemberData";

/// The capability names the client requests from the region seed. A driver POSTs
/// these to the seed URL to obtain the capability map, then uses `EventQueueGet`
/// for the event-queue long-poll, [`CAP_FETCH_INVENTORY`] for inventory fetches,
/// and [`CAP_GROUP_MEMBER_DATA`] for group rosters.
pub const REQUESTED_CAPABILITIES: &[&str] =
    &["EventQueueGet", CAP_FETCH_INVENTORY, CAP_GROUP_MEMBER_DATA];

/// Computes `now + duration`, saturating at `now` on (impossible) overflow.
fn deadline(now: Instant, duration: Duration) -> Instant {
    now.checked_add(duration).unwrap_or(now)
}

/// Updates `earliest` to the minimum of itself and `candidate`.
fn merge_deadline(earliest: &mut Option<Instant>, candidate: Option<Instant>) {
    if let Some(candidate) = candidate {
        *earliest = Some(match *earliest {
            Some(current) => current.min(candidate),
            None => candidate,
        });
    }
}

/// A reliable packet awaiting acknowledgement.
#[derive(Debug, Clone)]
struct UnackedPacket {
    /// The fully encoded datagram, ready to resend.
    datagram: Vec<u8>,
    /// When the packet was last sent.
    sent_at: Instant,
    /// How many times the packet has been sent so far.
    attempts: u32,
}

/// A bounded set of recently seen inbound reliable sequence numbers, used to
/// suppress duplicate processing of retransmitted reliable packets.
#[derive(Debug, Default)]
struct SeenWindow {
    /// Membership set for O(1) lookup.
    set: HashSet<u32>,
    /// Insertion order, for evicting the oldest entries.
    order: VecDeque<u32>,
}

impl SeenWindow {
    /// Records `sequence`; returns `true` if it was not seen before.
    fn insert(&mut self, sequence: u32) -> bool {
        if !self.set.insert(sequence) {
            return false;
        }
        self.order.push_back(sequence);
        if self.order.len() > SEEN_CAPACITY
            && let Some(evicted) = self.order.pop_front()
        {
            self.set.remove(&evicted);
        }
        true
    }
}

/// The per-connection timers, expressed as absolute deadlines.
#[derive(Debug)]
struct Timers {
    /// When the link is declared dead for lack of inbound traffic.
    inactivity: Instant,
    /// When to flush owed acknowledgements, if any are pending.
    ack_flush: Option<Instant>,
    /// When to send the next `AgentUpdate`, once the session is active.
    agent_update: Option<Instant>,
    /// When to give up waiting for a `LogoutReply`, once logging out.
    logout: Option<Instant>,
    /// When to give up waiting for a `TeleportFinish`, once teleporting.
    teleport: Option<Instant>,
}

/// The UDP circuit to a single simulator.
#[derive(Debug)]
struct Circuit {
    /// The simulator's UDP address.
    sim_addr: SocketAddr,
    /// The agent/avatar id.
    agent_id: Uuid,
    /// The session id.
    session_id: Uuid,
    /// The circuit code.
    code: u32,
    /// The next outgoing sequence number.
    next_sequence: u32,
    /// Inbound reliable sequence numbers we still owe acknowledgements for.
    pending_acks: Vec<u32>,
    /// Outgoing reliable packets awaiting acknowledgement, keyed by sequence.
    unacked: BTreeMap<u32, UnackedPacket>,
    /// Recently seen inbound reliable sequence numbers.
    seen: SeenWindow,
    /// Datagrams ready to be transmitted.
    out: VecDeque<Vec<u8>>,
    /// The draw distance (metres) advertised in keep-alive `AgentUpdate`s.
    draw_distance: f32,
    /// The connection timers.
    timers: Timers,
}

impl Circuit {
    /// Creates a circuit and arms the inactivity timer.
    fn new(
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
    fn retarget(&mut self, sim_addr: SocketAddr, now: Instant) {
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
    const fn next_sequence(&mut self) -> u32 {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.wrapping_add(1);
        sequence
    }

    /// Encodes and queues a message, tracking it for resend when reliable.
    fn send(
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
    fn send_use_circuit_code(&mut self, now: Instant) -> Result<(), WireError> {
        let message = AnyMessage::UseCircuitCode(UseCircuitCode {
            circuit_code: UseCircuitCodeCircuitCodeBlock {
                code: self.code,
                session_id: self.session_id,
                id: self.agent_id,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues `CompleteAgentMovement` reliably.
    fn send_complete_agent_movement(&mut self, now: Instant) -> Result<(), WireError> {
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
    fn send_region_handshake_reply(&mut self, now: Instant) -> Result<(), WireError> {
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
    fn send_complete_ping_check(&mut self, ping_id: u8, now: Instant) -> Result<(), WireError> {
        let message = AnyMessage::CompletePingCheck(CompletePingCheck {
            ping_id: CompletePingCheckPingIDBlock { ping_id },
        });
        self.send(&message, Reliability::Unreliable, now)
    }

    /// Queues a `ChatFromViewer` reliably, sending local chat. The wire string
    /// carries a trailing NUL, as a real viewer sends.
    fn send_chat_from_viewer(
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
    fn send_instant_message_raw(
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

    /// Queues an `AgentUpdate` unreliably carrying the given control flags and
    /// body/head rotation.
    ///
    /// The camera is placed at the region centre with an orthonormal basis and
    /// the configured draw distance, so the simulator builds an interest list
    /// and enables the neighbouring regions (which arrive as `EnableSimulator`).
    /// The simulator moves the agent according to `control_flags` in the
    /// direction of `body_rotation`.
    fn send_agent_update(
        &mut self,
        control_flags: u32,
        body_rotation: Rotation,
        head_rotation: Rotation,
        now: Instant,
    ) -> Result<(), WireError> {
        let camera_center = Vector {
            x: 128.0,
            y: 128.0,
            z: 30.0,
        };
        let camera_at_axis = Vector {
            x: 1.0,
            y: 0.0,
            z: 0.0,
        };
        let camera_left_axis = Vector {
            x: 0.0,
            y: 1.0,
            z: 0.0,
        };
        let camera_up_axis = Vector {
            x: 0.0,
            y: 0.0,
            z: 1.0,
        };
        let message = AnyMessage::AgentUpdate(AgentUpdate {
            agent_data: AgentUpdateAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
                body_rotation,
                head_rotation,
                state: 0,
                camera_center,
                camera_at_axis,
                camera_left_axis,
                camera_up_axis,
                far: self.draw_distance,
                control_flags,
                flags: 0,
            },
        });
        self.send(&message, Reliability::Unreliable, now)
    }

    /// Queues an `AgentRequestSit` reliably (ask to sit on `target` at `offset`).
    fn send_agent_request_sit(
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
    fn send_agent_sit(&mut self, now: Instant) -> Result<(), WireError> {
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
    fn send_generic_message(
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
    fn send_avatar_properties_request(
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

    /// Queues a `GrantUserRights` reliably, setting the rights this agent grants
    /// the friend `target` to `rights`.
    fn send_grant_user_rights(
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
    fn send_terminate_friendship(&mut self, other: Uuid, now: Instant) -> Result<(), WireError> {
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
    fn send_accept_friendship(
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
    fn send_decline_friendship(
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
    fn send_activate_group(&mut self, group_id: Uuid, now: Instant) -> Result<(), WireError> {
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
    fn send_group_members_request(
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
    fn send_group_role_data_request(
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
    fn send_group_role_members_request(
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
    fn send_group_titles_request(&mut self, group_id: Uuid, now: Instant) -> Result<(), WireError> {
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
    fn send_group_profile_request(
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
    fn send_group_notices_list_request(
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
    fn send_group_notice_request(
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
    fn send_create_group_request(
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
    fn send_join_group_request(&mut self, group_id: Uuid, now: Instant) -> Result<(), WireError> {
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
    fn send_leave_group_request(&mut self, group_id: Uuid, now: Instant) -> Result<(), WireError> {
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
    fn send_invite_group_request(
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
    fn send_set_group_accept_notices(
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
    fn send_set_group_contribution(
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

    /// Queues a group IM (`ImprovedInstantMessage`) reliably: the session id and
    /// recipient are both `group_id`, as group chat requires. `dialog` selects
    /// start/send/leave.
    fn send_group_session_im(
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
    fn send_script_dialog_reply(
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
    fn send_script_answer_yes(
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
    fn send_mute_list_request(&mut self, mute_crc: u32, now: Instant) -> Result<(), WireError> {
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
    fn send_update_mute_list_entry(
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
    fn send_remove_mute_list_entry(
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
    fn send_request_xfer(
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
    fn send_confirm_xfer_packet(
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

    /// Queues a `FetchInventoryDescendents` reliably for the folder `folder_id`
    /// (sorted by name), requesting its sub-folders and items.
    fn send_fetch_inventory_descendents(
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

    /// Queues a `TeleportLocationRequest` reliably.
    fn send_teleport_location_request(
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
    fn send_logout_request(&mut self, now: Instant) -> Result<(), WireError> {
        let message = AnyMessage::LogoutRequest(LogoutRequest {
            agent_data: LogoutRequestAgentDataBlock {
                agent_id: self.agent_id,
                session_id: self.session_id,
            },
        });
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `RequestRegionInfo` reliably.
    fn send_request_region_info(&mut self, now: Instant) -> Result<(), WireError> {
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
    fn send_money_balance_request(&mut self, now: Instant) -> Result<(), WireError> {
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
    fn send_economy_data_request(&mut self, now: Instant) -> Result<(), WireError> {
        let message = AnyMessage::EconomyDataRequest(EconomyDataRequest {});
        self.send(&message, Reliability::Reliable, now)
    }

    /// Queues a `MoneyTransferRequest` reliably: pay `amount` L$ to `dest` with
    /// the given transaction type and description. The source is this agent.
    fn send_money_transfer(
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
    fn send_parcel_properties_request(
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

    /// Queues a `MapBlockRequest` reliably for a grid-coordinate rectangle.
    fn send_map_block_request(
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
    fn send_map_name_request(&mut self, name: &str, now: Instant) -> Result<(), WireError> {
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
    fn send_map_item_request(
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

    /// Records that a datagram was received, resetting the inactivity timer.
    fn note_received(&mut self, now: Instant) {
        self.timers.inactivity = deadline(now, INACTIVITY_TIMEOUT);
    }

    /// Records that we owe an acknowledgement for `sequence`, arming the flush.
    fn queue_ack(&mut self, sequence: u32, now: Instant) {
        self.pending_acks.push(sequence);
        if self.timers.ack_flush.is_none() {
            self.timers.ack_flush = Some(deadline(now, ACK_FLUSH_DELAY));
        }
    }

    /// Removes the given outgoing sequence numbers from the unacked set.
    fn record_acks(&mut self, ids: &[u32]) {
        for id in ids {
            self.unacked.remove(id);
        }
    }

    /// Records an inbound reliable `sequence`; returns `true` if it is new.
    fn mark_seen(&mut self, sequence: u32) -> bool {
        self.seen.insert(sequence)
    }

    /// Flushes owed acknowledgements as one or more `PacketAck` messages.
    fn flush_acks(&mut self, now: Instant) -> Result<(), WireError> {
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
    fn process_resends(&mut self, now: Instant) -> bool {
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
    fn next_resend_deadline(&self) -> Option<Instant> {
        self.unacked
            .values()
            .map(|packet| deadline(packet.sent_at, RESEND_TIMEOUT))
            .min()
    }
}

/// The lifecycle state of a [`Session`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionState {
    /// Constructed; the login request is available but not yet answered.
    New,
    /// Login succeeded; bootstrap packets sent, awaiting the region handshake.
    AwaitingHandshake,
    /// The region handshake completed; keep-alives are flowing.
    Active,
    /// A `TeleportLocationRequest` was sent; awaiting the `TeleportFinish`.
    Teleporting,
    /// A `LogoutRequest` was sent; awaiting the `LogoutReply`.
    LoggingOut,
    /// The session is finished.
    Closed,
}

/// Bookkeeping for an in-progress teleport handover, so the next
/// `RegionHandshake` is reported as a [`Event::RegionChanged`].
#[derive(Debug)]
struct HandoverPending {
    /// The destination region handle reported by `TeleportFinish`.
    region_handle: u64,
}

/// A single agent session: login bookkeeping plus one simulator circuit.
///
/// This is a pure state machine. Feed it bytes and the current [`Instant`] via
/// the `handle_*` methods; drain datagrams, timeouts, and events via the
/// `poll_*` methods. It performs no I/O and never reads a clock.
#[derive(Debug)]
pub struct Session {
    /// The login parameters.
    login: LoginParams,
    /// The current lifecycle state.
    state: SessionState,
    /// The active (root) circuit, once login has succeeded.
    circuit: Option<Circuit>,
    /// Child-agent circuits to neighbouring regions, keyed by simulator address.
    /// Opened from `EnableSimulator` so a neighbour already holds the agent's
    /// presence when the avatar crosses the border (promoted to root on
    /// `CrossedRegion`).
    children: BTreeMap<SocketAddr, Circuit>,
    /// The capability-seed URL for each child region (from the CAPS
    /// `EstablishAgentCommunication` event), keyed by simulator address; used as
    /// the new seed when a child is promoted to root.
    child_seeds: BTreeMap<SocketAddr, String>,
    /// The draw distance (metres) advertised in keep-alive `AgentUpdate`s.
    draw_distance: f32,
    /// The agent control flags advertised in keep-alive `AgentUpdate`s; the
    /// simulator moves the agent accordingly.
    controls: ControlFlags,
    /// The agent's body rotation (facing) sent in `AgentUpdate`s.
    body_rotation: Rotation,
    /// The agent's head rotation sent in `AgentUpdate`s.
    head_rotation: Rotation,
    /// Set between an `AgentRequestSit` and the `AvatarSitResponse` that follows,
    /// so the response is completed with an `AgentSit`.
    sit_requested: bool,
    /// In-progress teleport handover bookkeeping, if any.
    handover: Option<HandoverPending>,
    /// The destination region handle of an in-flight teleport (between sending
    /// `TeleportLocationRequest` and receiving `TeleportFinish`/failure).
    teleport_target: Option<u64>,
    /// The current region's capability-seed URL (from login or a teleport), for
    /// the driver to fetch the CAPS map and event queue.
    seed_capability: Option<String>,
    /// The agent's inventory root ("My Inventory") folder id, from the login
    /// response.
    inventory_root: Option<Uuid>,
    /// In-flight mute-list file downloads (`Xfer` id → accumulated file bytes),
    /// started when a `MuteListUpdate` arrives.
    mute_xfers: BTreeMap<u64, Vec<u8>>,
    /// A monotonic counter for generating `Xfer` ids (never zero).
    next_xfer_id: u64,
    /// Pending high-level events for the driver.
    events: VecDeque<Event>,
}

impl Session {
    /// Creates a new session for the given login parameters.
    #[must_use]
    pub const fn new(login: LoginParams) -> Self {
        Self {
            login,
            state: SessionState::New,
            circuit: None,
            children: BTreeMap::new(),
            child_seeds: BTreeMap::new(),
            draw_distance: DEFAULT_DRAW_DISTANCE,
            controls: ControlFlags::empty(),
            body_rotation: IDENTITY_ROTATION,
            head_rotation: IDENTITY_ROTATION,
            sit_requested: false,
            handover: None,
            teleport_target: None,
            seed_capability: None,
            inventory_root: None,
            mute_xfers: BTreeMap::new(),
            next_xfer_id: 1,
            events: VecDeque::new(),
        }
    }

    /// Sets the draw distance (metres) advertised in keep-alive `AgentUpdate`s.
    /// A larger value makes the simulator enable more neighbouring regions
    /// (surfaced as [`Event::NeighborDiscovered`]). Takes effect on the next
    /// keep-alive, including for the current circuit.
    pub const fn set_draw_distance(&mut self, draw_distance: f32) {
        self.draw_distance = draw_distance;
        if let Some(circuit) = self.circuit.as_mut() {
            circuit.draw_distance = draw_distance;
        }
    }

    /// The current region's capability-seed URL, once login (or a teleport) has
    /// provided one. The driver POSTs this to obtain the capability map and the
    /// `EventQueueGet` URL. It changes on each region change.
    #[must_use]
    pub fn seed_capability(&self) -> Option<&str> {
        self.seed_capability.as_deref()
    }

    /// Feeds a parsed CAPS response into the session, surfacing any recognised
    /// payload. Handles `ParcelProperties` and `TeleportFinish` (delivered over
    /// the event queue, not UDP) and [`CAP_FETCH_INVENTORY`] (the LLSD response to
    /// a `FetchInventoryDescendents2` POST the driver performed on the client's
    /// behalf), surfaced as [`Event::InventoryDescendents`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::Wire`] if a teleport-handover bootstrap packet fails to
    /// encode.
    pub fn handle_caps_event(
        &mut self,
        message: &str,
        body: &Llsd,
        now: Instant,
    ) -> Result<(), Error> {
        match message {
            "ParcelProperties" => {
                if let Some(parcel) = parcel_info_from_llsd(body) {
                    self.events
                        .push_back(Event::ParcelProperties(Box::new(parcel)));
                }
            }
            "TeleportFinish" => {
                if let Some((dest, seed)) = teleport_finish_from_llsd(body) {
                    let region_handle = self.teleport_target.unwrap_or(0);
                    self.begin_handover(dest, region_handle, Some(seed), now)?;
                }
            }
            // A neighbouring region is announced over the CAPS event queue (the
            // modern path; OpenSim does not use the UDP `EnableSimulator`). Open a
            // child-agent circuit so it holds the agent's presence before a
            // crossing.
            "EnableSimulator" => {
                if let Some((handle, sim)) = enable_simulator_from_caps_llsd(body) {
                    self.open_child_circuit(sim, now)?;
                    let (grid_x, grid_y) = handle_to_grid(handle);
                    self.events
                        .push_back(Event::NeighborDiscovered(NeighborInfo {
                            region_handle: handle,
                            sim,
                            grid_x,
                            grid_y,
                        }));
                }
            }
            // A neighbouring region's child-agent seed capability, sent after we
            // open the child circuit; cache it for when the child is promoted to
            // root on a border crossing.
            "EstablishAgentCommunication" => {
                if let Some((sim, seed)) = establish_agent_communication_from_llsd(body) {
                    self.child_seeds.insert(sim, seed);
                }
            }
            // The agent has physically crossed a region border; OpenSim signals
            // the handover over the CAPS event queue (not the UDP `CrossedRegion`).
            // Promote the pre-opened child circuit for the destination to root.
            "CrossedRegion" if matches!(self.state, SessionState::Active) => {
                if let Some((handle, dest, seed)) = crossed_region_from_caps_llsd(body) {
                    self.promote_child_to_root(dest, handle, Some(seed), now)?;
                }
            }
            CAP_FETCH_INVENTORY => {
                for event in inventory_descendents_from_llsd(body) {
                    self.events.push_back(event);
                }
            }
            // The modern (CAPS event-queue) delivery of group memberships; the
            // UDP `AgentGroupDataUpdate` is deprecated on Second Life.
            "AgentGroupDataUpdate" => {
                if let Some(event) = group_memberships_from_caps_llsd(body) {
                    self.events.push_back(event);
                }
            }
            // The response to a `GroupMemberData` capability POST (the modern
            // group roster fetch).
            CAP_GROUP_MEMBER_DATA => {
                if let Some(event) = group_members_from_caps_llsd(body) {
                    self.events.push_back(event);
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Hands the circuit over to a teleport destination `dest`: retargets the
    /// circuit, sends `UseCircuitCode` + `CompleteAgentMovement` (creating the
    /// child presence then promoting it to root, as a viewer does on
    /// `TeleportFinish`), records the seed capability, and awaits the
    /// destination's handshake / `AgentMovementComplete`. No-op unless a teleport
    /// is in flight.
    fn begin_handover(
        &mut self,
        dest: SocketAddr,
        region_handle: u64,
        seed_capability: Option<String>,
        now: Instant,
    ) -> Result<(), Error> {
        if !matches!(self.state, SessionState::Teleporting) {
            return Ok(());
        }
        // Retarget synchronously: it resets the circuit's sequence/ack/seen/timer
        // state to the new simulator, after which the source check accepts only
        // the destination.
        if let Some(circuit) = self.circuit.as_mut() {
            circuit.retarget(dest, now);
            circuit.send_use_circuit_code(now)?;
            circuit.send_complete_agent_movement(now)?;
        }
        // Any child circuits were neighbours of the source region; drop them.
        self.children.clear();
        self.child_seeds.clear();
        if seed_capability.is_some() {
            self.seed_capability = seed_capability;
        }
        self.teleport_target = None;
        self.handover = Some(HandoverPending { region_handle });
        self.state = SessionState::AwaitingHandshake;
        Ok(())
    }

    /// Completes the initial login handshake or a teleport handover: arms the
    /// keep-alive `AgentUpdate`, transitions to `Active`, and emits
    /// `RegionHandshakeComplete` (login) or `RegionChanged` (handover). Idempotent
    /// — only acts while still `AwaitingHandshake`, so it may be driven by
    /// whichever of `RegionHandshake` / `AgentMovementComplete` arrives first.
    fn complete_arrival(&mut self, now: Instant) {
        if !matches!(self.state, SessionState::AwaitingHandshake) {
            return;
        }
        if let Some(circuit) = self.circuit.as_mut() {
            circuit.timers.agent_update = Some(deadline(now, AGENT_UPDATE_INTERVAL));
        }
        self.state = SessionState::Active;
        match self.handover.take() {
            Some(handover) => {
                if let Some(sim) = self.circuit.as_ref().map(|c| c.sim_addr) {
                    self.events.push_back(Event::RegionChanged {
                        region_handle: handover.region_handle,
                        sim,
                    });
                }
            }
            None => self.events.push_back(Event::RegionHandshakeComplete),
        }
    }

    /// Opens a child-agent circuit to a neighbouring simulator `sim`: a fresh
    /// circuit reusing the agent identity and circuit code, with `UseCircuitCode`
    /// sent but **not** `CompleteAgentMovement` (so it stays a child agent). A
    /// no-op if `sim` is already the root or an existing child, or if there is no
    /// root circuit yet to copy the identity from.
    fn open_child_circuit(&mut self, sim: SocketAddr, now: Instant) -> Result<(), Error> {
        if self.circuit.as_ref().map(|c| c.sim_addr) == Some(sim)
            || self.children.contains_key(&sim)
        {
            return Ok(());
        }
        let Some(root) = self.circuit.as_ref() else {
            return Ok(());
        };
        let mut child = Circuit::new(
            sim,
            root.agent_id,
            root.session_id,
            root.code,
            self.draw_distance,
            now,
        );
        child.send_use_circuit_code(now)?;
        self.children.insert(sim, child);
        Ok(())
    }

    /// Promotes a child-agent circuit at `dest` to the root after the avatar
    /// crosses a region border (`CrossedRegion`): completes the agent movement so
    /// the neighbour makes us a root agent, swaps it in as the active circuit
    /// (demoting the old root to a child), drops the now-stale neighbour
    /// circuits, records the new seed, and awaits arrival (so `complete_arrival`
    /// emits `RegionChanged`). Falls back to a fresh circuit if no child was
    /// pre-opened.
    fn promote_child_to_root(
        &mut self,
        dest: SocketAddr,
        region_handle: u64,
        seed: Option<String>,
        now: Instant,
    ) -> Result<(), Error> {
        let Some(root) = self.circuit.as_ref() else {
            return Ok(());
        };
        let (agent_id, session_id, code) = (root.agent_id, root.session_id, root.code);
        // Prefer the seed from `CrossedRegion`; fall back to the one cached from
        // the child's `EstablishAgentCommunication`.
        let seed = seed
            .filter(|s| !s.is_empty())
            .or_else(|| self.child_seeds.get(&dest).cloned());
        let mut new_root = self.children.remove(&dest).unwrap_or_else(|| {
            Circuit::new(dest, agent_id, session_id, code, self.draw_distance, now)
        });
        self.child_seeds.remove(&dest);
        new_root.send_complete_agent_movement(now)?;
        // The old root becomes a child agent of the new region. The *other*
        // children stay open: a neighbour of the old region is often also a
        // neighbour of the new one (regions can border on every side), so
        // tearing them down would be wrong. The simulator retires the ones that
        // no longer apply via `DisableSimulator`; any that go silent expire on
        // inactivity, and the new region announces any genuinely new neighbours
        // via `EnableSimulator`.
        let old_root = self.circuit.replace(new_root);
        if let Some(old) = old_root {
            self.children.insert(old.sim_addr, old);
        }
        if seed.is_some() {
            self.seed_capability = seed;
        }
        self.handover = Some(HandoverPending { region_handle });
        self.state = SessionState::AwaitingHandshake;
        Ok(())
    }

    /// The XML-RPC login request the driver must perform, or `None` once login
    /// has already been answered.
    #[must_use]
    pub fn login_http_request(&self) -> Option<LoginHttpRequest> {
        if matches!(self.state, SessionState::New) {
            Some(LoginHttpRequest {
                url: self.login.login_uri.clone(),
                body: build_login_request(&self.login.request),
                user_agent: self.login.request.user_agent(),
            })
        } else {
            None
        }
    }

    /// Feeds back the parsed login response, bootstrapping the circuit on
    /// success or closing the session on failure.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Wire`] if a bootstrap packet fails to encode.
    pub fn handle_login_response(
        &mut self,
        response: sl_wire::LoginResponse,
        now: Instant,
    ) -> Result<(), Error> {
        match response {
            sl_wire::LoginResponse::Failure(failure) => {
                self.close(DisconnectReason::LoginFailed {
                    reason: failure.reason,
                    message: failure.message,
                });
            }
            // A driver performs the interactive MFA retry during its login
            // phase; if a challenge reaches the session it is treated as a
            // login failure.
            sl_wire::LoginResponse::MfaChallenge(challenge) => {
                self.close(DisconnectReason::LoginFailed {
                    reason: "mfa_challenge".to_owned(),
                    message: challenge.message,
                });
            }
            sl_wire::LoginResponse::Success(success) => {
                let sim_addr = SocketAddr::new(IpAddr::V4(success.sim_ip), success.sim_port);
                let mut circuit = Circuit::new(
                    sim_addr,
                    success.agent_id,
                    success.session_id,
                    success.circuit_code,
                    self.draw_distance,
                    now,
                );
                circuit.send_use_circuit_code(now)?;
                circuit.send_complete_agent_movement(now)?;
                self.circuit = Some(circuit);
                self.seed_capability = Some(success.seed_capability.clone());
                self.inventory_root = success.inventory_root;
                self.state = SessionState::AwaitingHandshake;
                self.events
                    .push_back(Event::CircuitEstablished { sim: sim_addr });
                if !success.inventory_skeleton.is_empty() {
                    let folders = success
                        .inventory_skeleton
                        .iter()
                        .map(skeleton_folder)
                        .collect();
                    self.events.push_back(Event::InventorySkeleton(folders));
                }
                if !success.buddy_list.is_empty() {
                    let friends = success.buddy_list.iter().map(friend).collect();
                    self.events.push_back(Event::FriendList(friends));
                }
            }
        }
        Ok(())
    }

    /// Processes an inbound datagram received from `from` at `now`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Wire`] if the datagram cannot be parsed or a reply fails
    /// to encode.
    pub fn handle_datagram(
        &mut self,
        from: SocketAddr,
        datagram: &[u8],
        now: Instant,
    ) -> Result<(), Error> {
        if matches!(self.state, SessionState::Closed | SessionState::New) {
            return Ok(());
        }
        // Accept traffic from the root circuit or any open child circuit; ignore
        // anything else.
        let is_root = self.circuit.as_ref().map(|c| c.sim_addr) == Some(from);
        if !is_root && !self.children.contains_key(&from) {
            return Ok(());
        }

        let parsed = parse_datagram(datagram)?;

        let process = {
            let circuit = if is_root {
                self.circuit.as_mut()
            } else {
                self.children.get_mut(&from)
            };
            let Some(circuit) = circuit else {
                return Ok(());
            };
            circuit.note_received(now);
            circuit.record_acks(&parsed.acks);
            if parsed.flags.contains(PacketFlags::RELIABLE) {
                circuit.queue_ack(parsed.sequence, now);
                circuit.mark_seen(parsed.sequence)
            } else {
                true
            }
        };
        if !process {
            return Ok(());
        }

        let decoded;
        let body = if parsed.flags.contains(PacketFlags::ZEROCODED) {
            decoded = zero_decode(parsed.body)?;
            decoded.as_slice()
        } else {
            parsed.body
        };

        let mut reader = Reader::new(body);
        let id = MessageId::decode(&mut reader)?;
        // Unrecognized messages are ignored rather than failing the datagram.
        let Ok(message) = AnyMessage::decode(id, &mut reader) else {
            return Ok(());
        };
        if is_root {
            self.dispatch(&message, now)
        } else {
            self.dispatch_child(from, &message, now)
        }
    }

    /// Handles a message that arrived on a child-agent circuit. Children carry
    /// limited traffic; we keep the circuit healthy (ping replies, region
    /// handshake acknowledgement) and otherwise ignore it — the crossing into a
    /// child region is driven by `CrossedRegion` on the root circuit.
    fn dispatch_child(
        &mut self,
        from: SocketAddr,
        message: &AnyMessage,
        now: Instant,
    ) -> Result<(), Error> {
        match message {
            AnyMessage::StartPingCheck(ping) => {
                if let Some(circuit) = self.children.get_mut(&from) {
                    circuit.send_complete_ping_check(ping.ping_id.ping_id, now)?;
                }
            }
            AnyMessage::RegionHandshake(_) => {
                if let Some(circuit) = self.children.get_mut(&from) {
                    circuit.send_region_handshake_reply(now)?;
                }
            }
            AnyMessage::PacketAck(ack) => {
                if let Some(circuit) = self.children.get_mut(&from) {
                    for packet in &ack.packets {
                        circuit.record_acks(&[packet.id]);
                    }
                }
            }
            AnyMessage::DisableSimulator(_) => {
                // The simulator is retiring this child circuit.
                self.children.remove(&from);
                self.child_seeds.remove(&from);
            }
            _ => {}
        }
        Ok(())
    }

    /// Acts on a decoded inbound message.
    fn dispatch(&mut self, message: &AnyMessage, now: Instant) -> Result<(), Error> {
        match message {
            AnyMessage::RegionHandshake(handshake) => {
                if matches!(self.state, SessionState::AwaitingHandshake) {
                    if let Some(circuit) = self.circuit.as_mut() {
                        circuit.send_region_handshake_reply(now)?;
                    }
                    self.events
                        .push_back(Event::RegionInfoHandshake(Box::new(region_identity(
                            &handshake.region_info,
                            &handshake.region_info3,
                        ))));
                    self.complete_arrival(now);
                }
            }
            AnyMessage::AgentMovementComplete(_) => {
                // After a teleport handover the destination promotes us to root
                // and confirms with AgentMovementComplete; it may not re-send a
                // RegionHandshake, so complete the arrival here too (idempotent).
                self.complete_arrival(now);
            }
            AnyMessage::RegionInfo(info) => {
                self.events.push_back(Event::RegionLimits(region_limits(
                    &info.region_info,
                    &info.region_info2,
                )));
            }
            AnyMessage::MoneyBalanceReply(reply) => {
                self.events
                    .push_back(Event::MoneyBalance(money_balance(reply)));
            }
            AnyMessage::EconomyData(data) => {
                self.events
                    .push_back(Event::EconomyData(Box::new(economy_data(data))));
            }
            AnyMessage::ParcelProperties(props) => {
                self.events
                    .push_back(Event::ParcelProperties(Box::new(parcel_info(
                        &props.parcel_data,
                    ))));
            }
            AnyMessage::ParcelOverlay(overlay) => {
                self.events
                    .push_back(Event::ParcelOverlay(ParcelOverlayInfo {
                        sequence_id: overlay.parcel_data.sequence_id,
                        data: overlay.parcel_data.data.clone(),
                    }));
            }
            AnyMessage::ChatFromSimulator(chat) => {
                let data = &chat.chat_data;
                match ChatType::from_u8(data.chat_type) {
                    // A typing animation trigger carries no text; surface it as a
                    // distinct typing signal rather than an empty chat line.
                    chat_type @ (ChatType::StartTyping | ChatType::StopTyping) => {
                        self.events.push_back(Event::ChatTyping {
                            from_name: trimmed_string(&data.from_name),
                            source_id: data.source_id,
                            typing: matches!(chat_type, ChatType::StartTyping),
                        });
                    }
                    _ => self
                        .events
                        .push_back(Event::ChatReceived(Box::new(chat_message(data)))),
                }
            }
            AnyMessage::ImprovedInstantMessage(im) => {
                let block = &im.message_block;
                match ImDialog::from_u8(block.dialog) {
                    // Typing notifications carry no real text; surface them as a
                    // distinct signal rather than an empty instant message.
                    dialog @ (ImDialog::TypingStart | ImDialog::TypingStop) => {
                        self.events.push_back(Event::ImTyping {
                            from_agent_id: im.agent_data.agent_id,
                            from_agent_name: trimmed_string(&block.from_agent_name),
                            session_id: block.id,
                            typing: matches!(dialog, ImDialog::TypingStart),
                        });
                    }
                    // Group IM session traffic (the session id is the group id).
                    ImDialog::SessionSend if block.from_group => {
                        self.events.push_back(Event::GroupSessionMessage {
                            group_id: block.id,
                            from_agent_id: im.agent_data.agent_id,
                            from_name: trimmed_string(&block.from_agent_name),
                            message: trimmed_string(&block.message),
                        });
                    }
                    dialog @ (ImDialog::SessionAdd | ImDialog::SessionLeave)
                        if block.from_group =>
                    {
                        self.events.push_back(Event::GroupSessionParticipant {
                            group_id: block.id,
                            agent_id: im.agent_data.agent_id,
                            joined: matches!(dialog, ImDialog::SessionAdd),
                        });
                    }
                    _ => self
                        .events
                        .push_back(Event::InstantMessageReceived(Box::new(instant_message(
                            &im.agent_data,
                            block,
                        )))),
                }
            }
            AnyMessage::AvatarSitResponse(response) => {
                // Only act on a response to our own AgentRequestSit; complete the
                // sit with an AgentSit and surface the result.
                if self.sit_requested {
                    self.sit_requested = false;
                    if let Some(circuit) = self.circuit.as_mut() {
                        circuit.send_agent_sit(now)?;
                    }
                    let transform = &response.sit_transform;
                    self.events.push_back(Event::SitResult {
                        sit_object: response.sit_object.id,
                        autopilot: transform.auto_pilot,
                        sit_position: (
                            transform.sit_position.x,
                            transform.sit_position.y,
                            transform.sit_position.z,
                        ),
                    });
                }
            }
            AnyMessage::AvatarPropertiesReply(reply) => {
                self.events
                    .push_back(Event::AvatarProperties(Box::new(avatar_properties(
                        reply.agent_data.avatar_id,
                        &reply.properties_data,
                    ))));
            }
            AnyMessage::AvatarInterestsReply(reply) => {
                self.events
                    .push_back(Event::AvatarInterests(Box::new(avatar_interests(
                        reply.agent_data.avatar_id,
                        &reply.properties_data,
                    ))));
            }
            AnyMessage::AvatarGroupsReply(reply) => {
                self.events.push_back(Event::AvatarGroups {
                    avatar_id: reply.agent_data.avatar_id,
                    groups: reply.group_data.iter().map(avatar_group).collect(),
                    list_in_profile: reply.new_group_data.list_in_profile,
                });
            }
            AnyMessage::AvatarPicksReply(reply) => {
                self.events.push_back(Event::AvatarPicks {
                    target_id: reply.agent_data.target_id,
                    picks: reply
                        .data
                        .iter()
                        .map(|pick| AvatarPick {
                            pick_id: pick.pick_id,
                            name: trimmed_string(&pick.pick_name),
                        })
                        .collect(),
                });
            }
            AnyMessage::AvatarNotesReply(reply) => {
                self.events.push_back(Event::AvatarNotes {
                    target_id: reply.data.target_id,
                    notes: trimmed_string(&reply.data.notes),
                });
            }
            AnyMessage::InventoryDescendents(reply) => {
                self.events.push_back(Event::InventoryDescendents {
                    folder_id: reply.agent_data.folder_id,
                    version: reply.agent_data.version,
                    descendents: reply.agent_data.descendents,
                    folders: reply.folder_data.iter().map(inventory_folder).collect(),
                    items: reply.item_data.iter().map(inventory_item).collect(),
                });
            }
            AnyMessage::EnableSimulator(sim) => {
                let info = neighbor_info(&sim.simulator_info);
                // Pre-open a child-agent circuit to the neighbour so it holds the
                // agent's presence before the avatar crosses the border.
                self.open_child_circuit(info.sim, now)?;
                self.events.push_back(Event::NeighborDiscovered(info));
            }
            AnyMessage::MapBlockReply(reply) => {
                for (index, data) in reply.data.iter().enumerate() {
                    if let Some(region) = map_region_info(data, reply.size.get(index)) {
                        self.events.push_back(Event::MapBlock(Box::new(region)));
                    }
                }
            }
            AnyMessage::MapItemReply(reply) => {
                self.events.push_back(Event::MapItems {
                    item_type: MapItemType::from_u32(reply.request_data.item_type),
                    items: reply.data.iter().map(map_item).collect(),
                });
            }
            AnyMessage::TeleportStart(_) => {
                self.events.push_back(Event::TeleportStarted);
            }
            AnyMessage::TeleportProgress(progress) => {
                self.events.push_back(Event::TeleportProgress {
                    message: String::from_utf8_lossy(&progress.info.message).into_owned(),
                    teleport_flags: progress.info.teleport_flags,
                });
            }
            AnyMessage::TeleportLocal(_) => {
                // An intra-region teleport: no new circuit, just resume activity.
                if matches!(self.state, SessionState::Teleporting) {
                    self.state = SessionState::Active;
                    if let Some(circuit) = self.circuit.as_mut() {
                        circuit.timers.teleport = None;
                    }
                    self.events.push_back(Event::TeleportLocal);
                }
            }
            AnyMessage::TeleportFailed(failed) => {
                if matches!(self.state, SessionState::Teleporting) {
                    self.state = SessionState::Active;
                    self.teleport_target = None;
                    if let Some(circuit) = self.circuit.as_mut() {
                        circuit.timers.teleport = None;
                    }
                }
                self.events.push_back(Event::TeleportFailed {
                    reason: String::from_utf8_lossy(&failed.info.reason).into_owned(),
                });
            }
            AnyMessage::TeleportFinish(finish) => {
                // The UDP TeleportFinish path (grids without an event queue).
                // OpenSim normally delivers TeleportFinish over the CAPS event
                // queue instead; see `handle_caps_event`.
                if matches!(self.state, SessionState::Teleporting) {
                    let info = &finish.info;
                    // IPPORT is big-endian on the wire; the generated decoder
                    // reads it little-endian, so swap back to host order.
                    let port = info.sim_port.swap_bytes();
                    let dest = SocketAddr::new(IpAddr::V4(Ipv4Addr::from(info.sim_ip)), port);
                    let seed = Some(String::from_utf8_lossy(&info.seed_capability).into_owned());
                    self.begin_handover(dest, info.region_handle, seed, now)?;
                }
            }
            AnyMessage::CrossedRegion(crossed) => {
                // The avatar walked across a region border; the source region
                // hands us the destination's details. Promote the pre-opened
                // child circuit there to root.
                if matches!(self.state, SessionState::Active) {
                    let region = &crossed.region_data;
                    // IPPORT is big-endian on the wire; the generated decoder
                    // reads it little-endian, so swap back to host order.
                    let port = region.sim_port.swap_bytes();
                    let dest = SocketAddr::new(IpAddr::V4(Ipv4Addr::from(region.sim_ip)), port);
                    let seed = Some(String::from_utf8_lossy(&region.seed_capability).into_owned());
                    self.promote_child_to_root(dest, region.region_handle, seed, now)?;
                }
            }
            AnyMessage::StartPingCheck(ping) => {
                if let Some(circuit) = self.circuit.as_mut() {
                    circuit.send_complete_ping_check(ping.ping_id.ping_id, now)?;
                }
            }
            AnyMessage::PacketAck(ack) => {
                if let Some(circuit) = self.circuit.as_mut() {
                    for packet in &ack.packets {
                        circuit.record_acks(&[packet.id]);
                    }
                }
            }
            AnyMessage::MuteListUpdate(update) => {
                // The mute list changed; download the named file over Xfer.
                let filename = trimmed_string(&update.mute_data.filename);
                if filename.is_empty() {
                    self.events.push_back(Event::MuteList(Vec::new()));
                } else {
                    let xfer_id = self.next_xfer_id;
                    self.next_xfer_id = self.next_xfer_id.checked_add(1).unwrap_or(1);
                    self.mute_xfers.insert(xfer_id, Vec::new());
                    if let Some(circuit) = self.circuit.as_mut() {
                        circuit.send_request_xfer(xfer_id, &filename, now)?;
                    }
                }
            }
            AnyMessage::UseCachedMuteList(_) => {
                self.events.push_back(Event::MuteListUnchanged);
            }
            AnyMessage::SendXferPacket(packet) => {
                let xfer_id = packet.xfer_id.id;
                let packet_num = packet.xfer_id.packet;
                // The high bit marks the final packet; the low 31 bits are the
                // sequence number (the first packet is sequence 0).
                let is_last = packet_num & 0x8000_0000 != 0;
                let sequence = packet_num & 0x7fff_ffff;
                if self.mute_xfers.contains_key(&xfer_id) {
                    // The first packet carries a 4-byte little-endian length
                    // prefix before the file data; later packets are raw.
                    let chunk: &[u8] = if sequence == 0 {
                        packet.data_packet.data.get(4..).unwrap_or(&[])
                    } else {
                        &packet.data_packet.data
                    };
                    if let Some(buffer) = self.mute_xfers.get_mut(&xfer_id) {
                        buffer.extend_from_slice(chunk);
                    }
                    if let Some(circuit) = self.circuit.as_mut() {
                        circuit.send_confirm_xfer_packet(xfer_id, packet_num, now)?;
                    }
                    if is_last && let Some(buffer) = self.mute_xfers.remove(&xfer_id) {
                        self.events
                            .push_back(Event::MuteList(parse_mute_list(&buffer)));
                    }
                }
            }
            AnyMessage::GenericMessage(generic)
                // The sim NUL-terminates the method name on the wire.
                if trimmed_string(&generic.method_data.method) == "emptymutelist" =>
            {
                self.events.push_back(Event::MuteList(Vec::new()));
            }
            AnyMessage::ScriptDialog(dialog) => {
                self.events
                    .push_back(Event::ScriptDialog(Box::new(script_dialog(dialog))));
            }
            AnyMessage::ScriptQuestion(question) => {
                self.events
                    .push_back(Event::ScriptPermissionRequest(Box::new(
                        script_permission_request(question),
                    )));
            }
            AnyMessage::LoadURL(load) => {
                let data = &load.data;
                self.events
                    .push_back(Event::LoadUrl(Box::new(LoadUrlRequest {
                        object_name: trimmed_string(&data.object_name),
                        object_id: data.object_id,
                        owner_id: data.owner_id,
                        owner_is_group: data.owner_is_group,
                        message: trimmed_string(&data.message),
                        url: trimmed_string(&data.url),
                    })));
            }
            AnyMessage::ScriptTeleportRequest(request) => {
                let data = &request.data;
                self.events
                    .push_back(Event::ScriptTeleport(Box::new(ScriptTeleportRequest {
                        object_name: trimmed_string(&data.object_name),
                        region_name: trimmed_string(&data.sim_name),
                        position: (
                            data.sim_position.x,
                            data.sim_position.y,
                            data.sim_position.z,
                        ),
                        look_at: (data.look_at.x, data.look_at.y, data.look_at.z),
                    })));
            }
            AnyMessage::AgentDataUpdate(update) => {
                self.events
                    .push_back(Event::ActiveGroupChanged(Box::new(active_group(
                        &update.agent_data,
                    ))));
            }
            AnyMessage::AgentGroupDataUpdate(update) => {
                self.events.push_back(Event::GroupMemberships(
                    update.group_data.iter().map(group_membership).collect(),
                ));
            }
            AnyMessage::GroupMembersReply(reply) => {
                self.events.push_back(Event::GroupMembers {
                    group_id: reply.group_data.group_id,
                    request_id: reply.group_data.request_id,
                    member_count: reply.group_data.member_count,
                    members: reply.member_data.iter().map(group_member).collect(),
                });
            }
            AnyMessage::GroupRoleDataReply(reply) => {
                self.events.push_back(Event::GroupRoleData {
                    group_id: reply.group_data.group_id,
                    request_id: reply.group_data.request_id,
                    roles: reply.role_data.iter().map(group_role).collect(),
                });
            }
            AnyMessage::GroupRoleMembersReply(reply) => {
                self.events.push_back(Event::GroupRoleMembers {
                    group_id: reply.agent_data.group_id,
                    request_id: reply.agent_data.request_id,
                    pairs: reply
                        .member_data
                        .iter()
                        .map(|pair| GroupRoleMember {
                            role_id: pair.role_id,
                            member_id: pair.member_id,
                        })
                        .collect(),
                });
            }
            AnyMessage::GroupTitlesReply(reply) => {
                self.events.push_back(Event::GroupTitles {
                    group_id: reply.agent_data.group_id,
                    request_id: reply.agent_data.request_id,
                    titles: reply.group_data.iter().map(group_title).collect(),
                });
            }
            AnyMessage::GroupProfileReply(reply) => {
                self.events
                    .push_back(Event::GroupProfileReceived(Box::new(group_profile(
                        &reply.group_data,
                    ))));
            }
            AnyMessage::GroupNoticesListReply(reply) => {
                self.events.push_back(Event::GroupNotices {
                    group_id: reply.agent_data.group_id,
                    notices: reply.data.iter().map(group_notice).collect(),
                });
            }
            AnyMessage::CreateGroupReply(reply) => {
                self.events.push_back(Event::CreateGroupResult {
                    group_id: reply.reply_data.group_id,
                    success: reply.reply_data.success,
                    message: trimmed_string(&reply.reply_data.message),
                });
            }
            AnyMessage::JoinGroupReply(reply) => {
                self.events.push_back(Event::JoinGroupResult {
                    group_id: reply.group_data.group_id,
                    success: reply.group_data.success,
                });
            }
            AnyMessage::LeaveGroupReply(reply) => {
                self.events.push_back(Event::LeaveGroupResult {
                    group_id: reply.group_data.group_id,
                    success: reply.group_data.success,
                });
            }
            AnyMessage::AgentDropGroup(drop) => {
                self.events.push_back(Event::DroppedFromGroup {
                    group_id: drop.agent_data.group_id,
                });
            }
            AnyMessage::OnlineNotification(notification) => {
                let ids = notification
                    .agent_block
                    .iter()
                    .map(|block| block.agent_id)
                    .collect::<Vec<_>>();
                if !ids.is_empty() {
                    self.events.push_back(Event::FriendsOnline(ids));
                }
            }
            AnyMessage::OfflineNotification(notification) => {
                let ids = notification
                    .agent_block
                    .iter()
                    .map(|block| block.agent_id)
                    .collect::<Vec<_>>();
                if !ids.is_empty() {
                    self.events.push_back(Event::FriendsOffline(ids));
                }
            }
            AnyMessage::ChangeUserRights(change) => {
                // The AgentData id distinguishes the direction: when it is our
                // own id, each rights block echoes a change *we* made to a
                // friend (`agent_related` is the friend); otherwise the friend
                // (`AgentData.AgentID`) changed the rights they grant us, and
                // `agent_related` is our own id.
                let own = self
                    .circuit
                    .as_ref()
                    .map_or_else(Uuid::nil, |circuit| circuit.agent_id);
                for block in &change.rights {
                    let granted_to_us = change.agent_data.agent_id != own;
                    let friend_id = if granted_to_us {
                        change.agent_data.agent_id
                    } else {
                        block.agent_related
                    };
                    self.events.push_back(Event::FriendRightsChanged {
                        friend_id,
                        rights: FriendRights(block.related_rights),
                        granted_to_us,
                    });
                }
            }
            AnyMessage::LogoutReply(_) => {
                self.state = SessionState::Closed;
                self.events.push_back(Event::LoggedOut);
            }
            _ => {}
        }
        Ok(())
    }

    /// Advances all timers to `now`, sending keep-alives, retransmissions, and
    /// acknowledgements and detecting timeouts.
    pub fn handle_timeout(&mut self, now: Instant) {
        if self.run_timeout(now).is_err() {
            self.close(DisconnectReason::ProtocolError);
        }
    }

    /// The fallible body of [`Self::handle_timeout`].
    fn run_timeout(&mut self, now: Instant) -> Result<(), Error> {
        if matches!(self.state, SessionState::Closed) {
            return Ok(());
        }

        if self
            .circuit
            .as_ref()
            .is_some_and(|c| now >= c.timers.inactivity)
        {
            self.close(DisconnectReason::Timeout);
            return Ok(());
        }

        if self
            .circuit
            .as_ref()
            .and_then(|c| c.timers.logout)
            .is_some_and(|d| now >= d)
        {
            self.state = SessionState::Closed;
            self.events.push_back(Event::LoggedOut);
            return Ok(());
        }

        if matches!(self.state, SessionState::Teleporting)
            && self
                .circuit
                .as_ref()
                .and_then(|c| c.timers.teleport)
                .is_some_and(|d| now >= d)
        {
            self.state = SessionState::Active;
            self.teleport_target = None;
            if let Some(circuit) = self.circuit.as_mut() {
                circuit.timers.teleport = None;
            }
            self.events.push_back(Event::TeleportFailed {
                reason: "teleport timed out".to_owned(),
            });
            return Ok(());
        }

        let exhausted = self
            .circuit
            .as_mut()
            .is_some_and(|c| c.process_resends(now));
        if exhausted {
            self.close(DisconnectReason::HandshakeFailed);
            return Ok(());
        }

        if self
            .circuit
            .as_ref()
            .and_then(|c| c.timers.ack_flush)
            .is_some_and(|d| now >= d)
            && let Some(circuit) = self.circuit.as_mut()
        {
            circuit.flush_acks(now)?;
        }

        if self
            .circuit
            .as_ref()
            .and_then(|c| c.timers.agent_update)
            .is_some_and(|d| now >= d)
        {
            let controls = self.controls.bits();
            let body = self.body_rotation.clone();
            let head = self.head_rotation.clone();
            if let Some(circuit) = self.circuit.as_mut() {
                circuit.send_agent_update(controls, body, head, now)?;
                circuit.timers.agent_update = Some(deadline(now, AGENT_UPDATE_INTERVAL));
            }
        }

        // Keep child circuits healthy: flush owed acks and retransmit, and drop
        // any that have gone silent (a dead child never fails the session).
        let mut dead = Vec::new();
        for (addr, child) in &mut self.children {
            if now >= child.timers.inactivity {
                dead.push(*addr);
                continue;
            }
            child.process_resends(now);
            if child.timers.ack_flush.is_some_and(|d| now >= d) {
                child.flush_acks(now)?;
            }
        }
        for addr in dead {
            self.children.remove(&addr);
            self.child_seeds.remove(&addr);
        }

        Ok(())
    }

    /// Enqueues an application message for delivery.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the message fails to encode.
    pub fn enqueue(
        &mut self,
        message: AnyMessage,
        reliability: Reliability,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send(&message, reliability, now)?;
        Ok(())
    }

    /// Sends local chat via `ChatFromViewer`. `chat_type` selects the range
    /// (whisper / normal / shout); `channel` is `0` for ordinary local chat or a
    /// non-zero channel for scripted listeners. Incoming chat is surfaced as
    /// [`Event::ChatReceived`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the message fails to encode.
    pub fn say(
        &mut self,
        message: &str,
        chat_type: ChatType,
        channel: i32,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_chat_from_viewer(message, chat_type, channel, now)?;
        Ok(())
    }

    /// Broadcasts a local-chat typing indicator via `ChatFromViewer`: a
    /// `StartTyping` message when `typing`, otherwise `StopTyping` (both with no
    /// text). Nearby viewers show or clear the typing animation; the counterpart
    /// is surfaced to other clients as [`Event::ChatTyping`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the message fails to encode.
    pub fn set_typing(&mut self, typing: bool, now: Instant) -> Result<(), Error> {
        let chat_type = if typing {
            ChatType::StartTyping
        } else {
            ChatType::StopTyping
        };
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_chat_from_viewer("", chat_type, 0, now)?;
        Ok(())
    }

    /// The agent's legacy name (`"First Last"`), used as the `FromAgentName` of
    /// outgoing instant messages.
    fn agent_name(&self) -> String {
        format!(
            "{} {}",
            self.login.request.first_name, self.login.request.last_name
        )
    }

    /// Sends a direct (1:1) instant message to `to_agent_id` via
    /// `ImprovedInstantMessage`. Incoming IMs are surfaced as
    /// [`Event::InstantMessageReceived`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the message fails to encode.
    pub fn send_instant_message(
        &mut self,
        to_agent_id: Uuid,
        message: &str,
        now: Instant,
    ) -> Result<(), Error> {
        let from_name = self.agent_name();
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_instant_message_raw(
            to_agent_id,
            ImDialog::Message,
            message,
            &from_name,
            now,
        )?;
        Ok(())
    }

    /// Sends an instant-message typing indicator to `to_agent_id`: an
    /// `IM_TYPING_START` message when `typing`, otherwise `IM_TYPING_STOP`. The
    /// counterpart is surfaced to other clients as [`Event::ImTyping`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the message fails to encode.
    pub fn send_im_typing(
        &mut self,
        to_agent_id: Uuid,
        typing: bool,
        now: Instant,
    ) -> Result<(), Error> {
        let dialog = if typing {
            ImDialog::TypingStart
        } else {
            ImDialog::TypingStop
        };
        let from_name = self.agent_name();
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        // The viewer sends the literal text "typing" with a typing IM.
        circuit.send_instant_message_raw(to_agent_id, dialog, "typing", &from_name, now)?;
        Ok(())
    }

    /// Offers friendship to `to_agent_id` via an `ImprovedInstantMessage` with
    /// the `IM_FRIENDSHIP_OFFERED` dialog. The recipient sees it as an
    /// [`Event::InstantMessageReceived`] with [`ImDialog::FriendshipOffered`] and
    /// replies with [`Session::accept_friendship`] or
    /// [`Session::decline_friendship`], echoing the offer's
    /// [`InstantMessage::id`] as the transaction id.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the message fails to encode.
    pub fn send_friendship_offer(
        &mut self,
        to_agent_id: Uuid,
        message: &str,
        now: Instant,
    ) -> Result<(), Error> {
        let from_name = self.agent_name();
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_instant_message_raw(
            to_agent_id,
            ImDialog::FriendshipOffered,
            message,
            &from_name,
            now,
        )?;
        Ok(())
    }

    /// Sends an `AgentUpdate` immediately with the current control state, plus the
    /// transient `extra` control bits (e.g. a one-shot `STAND_UP`). The extra bits
    /// are not persisted, so the next keep-alive clears them.
    fn send_agent_update_now(&mut self, extra: ControlFlags, now: Instant) -> Result<(), Error> {
        let controls = self.controls.union(extra).bits();
        let body = self.body_rotation.clone();
        let head = self.head_rotation.clone();
        if let Some(circuit) = self.circuit.as_mut() {
            circuit.send_agent_update(controls, body, head, now)?;
        } else {
            return Err(Error::NoCircuit);
        }
        Ok(())
    }

    /// Sets the agent control flags advertised in `AgentUpdate`s and sends one
    /// immediately. The simulator moves the agent accordingly (e.g.
    /// [`ControlFlags::AT_POS`] walks forward in the body-rotation direction,
    /// `| `[`ControlFlags::FLY`] flies); pass [`ControlFlags::empty`] to stop.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the message fails to encode.
    pub fn set_controls(&mut self, controls: ControlFlags, now: Instant) -> Result<(), Error> {
        self.controls = controls;
        self.send_agent_update_now(ControlFlags::empty(), now)
    }

    /// Sets the agent's body and head rotation (facing) advertised in
    /// `AgentUpdate`s and sends one immediately. This steers the direction the
    /// agent walks/flies under [`ControlFlags::AT_POS`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the message fails to encode.
    pub fn set_rotation(
        &mut self,
        body_rotation: Rotation,
        head_rotation: Rotation,
        now: Instant,
    ) -> Result<(), Error> {
        self.body_rotation = body_rotation;
        self.head_rotation = head_rotation;
        self.send_agent_update_now(ControlFlags::empty(), now)
    }

    /// Stands the agent up (from sitting), sending one `AgentUpdate` with the
    /// transient `STAND_UP` control bit. Does not change the persistent controls.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the message fails to encode.
    pub fn stand(&mut self, now: Instant) -> Result<(), Error> {
        self.send_agent_update_now(ControlFlags::STAND_UP, now)
    }

    /// Sits the agent on the ground where it stands, sending one `AgentUpdate`
    /// with the transient `SIT_ON_GROUND` control bit.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the message fails to encode.
    pub fn sit_on_ground(&mut self, now: Instant) -> Result<(), Error> {
        self.send_agent_update_now(ControlFlags::SIT_ON_GROUND, now)
    }

    /// Requests to sit on the object `target` at the given region-local `offset`
    /// via `AgentRequestSit`. The simulator replies with an `AvatarSitResponse`,
    /// which the session completes with an `AgentSit` and surfaces as
    /// [`Event::SitResult`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn sit_on(&mut self, target: Uuid, offset: Vector, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_agent_request_sit(target, offset, now)?;
        self.sit_requested = true;
        Ok(())
    }

    /// Walks the agent to the global coordinates `(global_x, global_y, z)` using
    /// the simulator's server-side autopilot (a `GenericMessage` with method
    /// `autopilot`). The X/Y are global metres (region south-west corner plus the
    /// region-local offset — see [`handle_to_global`](crate::handle_to_global));
    /// Z is the region-local height. Movement happens without the client needing
    /// any scene knowledge.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the message fails to encode.
    pub fn autopilot_to(
        &mut self,
        global_x: f64,
        global_y: f64,
        z: f64,
        now: Instant,
    ) -> Result<(), Error> {
        let params = [global_x.to_string(), global_y.to_string(), z.to_string()];
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_generic_message("autopilot", &params, now)?;
        Ok(())
    }

    /// Requests the profile of the avatar `target` via `AvatarPropertiesRequest`.
    /// The simulator replies with [`Event::AvatarProperties`], and usually also
    /// [`Event::AvatarInterests`] and [`Event::AvatarGroups`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_avatar_properties(&mut self, target: Uuid, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_avatar_properties_request(target, now)?;
        Ok(())
    }

    /// Requests the picks of the avatar `target` (a `GenericMessage`
    /// `avatarpicksrequest`). The reply arrives as [`Event::AvatarPicks`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_avatar_picks(&mut self, target: Uuid, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_generic_message("avatarpicksrequest", &[target.to_string()], now)?;
        Ok(())
    }

    /// Requests the agent's private notes about the avatar `target` (a
    /// `GenericMessage` `avatarnotesrequest`). The reply arrives as
    /// [`Event::AvatarNotes`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_avatar_notes(&mut self, target: Uuid, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_generic_message("avatarnotesrequest", &[target.to_string()], now)?;
        Ok(())
    }

    /// Sets the friendship rights this agent grants the friend `target` via
    /// `GrantUserRights`. `rights` is a [`FriendRights`] bitfield (combine the
    /// `FriendRights::CAN_*` flags). The simulator echoes the change back as an
    /// [`Event::FriendRightsChanged`] with `granted_to_us = false`.
    ///
    /// The agent's friend list (with the current rights) arrives at login as
    /// [`Event::FriendList`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn grant_user_rights(
        &mut self,
        target: Uuid,
        rights: FriendRights,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_grant_user_rights(target, rights.0, now)?;
        Ok(())
    }

    /// Ends the friendship with `other` via `TerminateFriendship`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn terminate_friendship(&mut self, other: Uuid, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_terminate_friendship(other, now)?;
        Ok(())
    }

    /// Accepts a friendship offer via `AcceptFriendship`. The `transaction_id`
    /// is the [`InstantMessage::id`] of the incoming
    /// [`ImDialog::FriendshipOffered`] IM; `calling_card_folder` is the
    /// inventory folder to place the new friend's calling card in (use the
    /// Calling Cards system folder, or the inventory root).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn accept_friendship(
        &mut self,
        transaction_id: Uuid,
        calling_card_folder: Uuid,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_accept_friendship(transaction_id, calling_card_folder, now)?;
        Ok(())
    }

    /// Declines a friendship offer via `DeclineFriendship`. The `transaction_id`
    /// is the [`InstantMessage::id`] of the incoming
    /// [`ImDialog::FriendshipOffered`] IM.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn decline_friendship(&mut self, transaction_id: Uuid, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_decline_friendship(transaction_id, now)?;
        Ok(())
    }

    /// Makes `group_id` the agent's active group (`ActivateGroup`); pass
    /// [`Uuid::nil`] to clear it. The simulator confirms with an
    /// [`Event::ActiveGroupChanged`]. The agent's memberships arrive at login as
    /// [`Event::GroupMemberships`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn activate_group(&mut self, group_id: Uuid, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_activate_group(group_id, now)?;
        Ok(())
    }

    /// Requests a group's member roster (`GroupMembersRequest`). The reply
    /// arrives as [`Event::GroupMembers`] (the simulator may split large rosters
    /// across several events).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_group_members(&mut self, group_id: Uuid, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_group_members_request(group_id, now)?;
        Ok(())
    }

    /// Requests a group's roles (`GroupRoleDataRequest`). The reply arrives as
    /// [`Event::GroupRoleData`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_group_roles(&mut self, group_id: Uuid, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_group_role_data_request(group_id, now)?;
        Ok(())
    }

    /// Requests a group's role↔member pairings (`GroupRoleMembersRequest`). The
    /// reply arrives as [`Event::GroupRoleMembers`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_group_role_members(
        &mut self,
        group_id: Uuid,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_group_role_members_request(group_id, now)?;
        Ok(())
    }

    /// Requests the agent's selectable titles in a group (`GroupTitlesRequest`).
    /// The reply arrives as [`Event::GroupTitles`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_group_titles(&mut self, group_id: Uuid, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_group_titles_request(group_id, now)?;
        Ok(())
    }

    /// Requests a group's profile (`GroupProfileRequest`). The reply arrives as
    /// [`Event::GroupProfileReceived`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_group_profile(&mut self, group_id: Uuid, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_group_profile_request(group_id, now)?;
        Ok(())
    }

    /// Requests a group's notice list (`GroupNoticesListRequest`). The reply
    /// arrives as [`Event::GroupNotices`] (headers only; fetch a notice's body
    /// with [`Session::request_group_notice`]).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_group_notices(&mut self, group_id: Uuid, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_group_notices_list_request(group_id, now)?;
        Ok(())
    }

    /// Requests a single group notice's full body and attachment
    /// (`GroupNoticeRequest`); the notice is delivered as an
    /// [`Event::InstantMessageReceived`] with the `GroupNotice` dialog.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_group_notice(&mut self, notice_id: Uuid, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_group_notice_request(notice_id, now)?;
        Ok(())
    }

    /// Creates a new group (`CreateGroupRequest`). The result arrives as
    /// [`Event::CreateGroupResult`] (with the new group id on success). Note the
    /// grid may charge an L$ creation fee.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn create_group(&mut self, params: &CreateGroupParams, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_create_group_request(params, now)?;
        Ok(())
    }

    /// Joins an open-enrollment group (`JoinGroupRequest`). The result arrives as
    /// [`Event::JoinGroupResult`]. Closed groups require an invitation instead
    /// (see [`Session::invite_to_group`]).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn join_group(&mut self, group_id: Uuid, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_join_group_request(group_id, now)?;
        Ok(())
    }

    /// Leaves a group (`LeaveGroupRequest`). The result arrives as
    /// [`Event::LeaveGroupResult`], followed by an [`Event::DroppedFromGroup`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn leave_group(&mut self, group_id: Uuid, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_leave_group_request(group_id, now)?;
        Ok(())
    }

    /// Invites agents to a group (`InviteGroupRequest`). Each invitee is an
    /// `(invitee_id, role_id)` pair; use [`Uuid::nil`] for the role to assign the
    /// default "Everyone" role. Invitees receive a group-invitation IM.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn invite_to_group(
        &mut self,
        group_id: Uuid,
        invitees: &[(Uuid, Uuid)],
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_invite_group_request(group_id, invitees, now)?;
        Ok(())
    }

    /// Sets whether the agent accepts notices from a group and lists it in their
    /// profile (`SetGroupAcceptNotices`).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn set_group_accept_notices(
        &mut self,
        group_id: Uuid,
        accept_notices: bool,
        list_in_profile: bool,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_set_group_accept_notices(group_id, accept_notices, list_in_profile, now)?;
        Ok(())
    }

    /// Sets the agent's L$ contribution to a group (`SetGroupContribution`).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn set_group_contribution(
        &mut self,
        group_id: Uuid,
        contribution: i32,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_set_group_contribution(group_id, contribution, now)?;
        Ok(())
    }

    /// Starts (joins) a group's IM session (`ImprovedInstantMessage`,
    /// `IM_SESSION_GROUP_START`), so the agent receives the group's chat. Group
    /// messages arrive as [`Event::GroupSessionMessage`]. Sending a message with
    /// [`Session::send_group_message`] also joins the session implicitly.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the message fails to encode.
    pub fn start_group_session(&mut self, group_id: Uuid, now: Instant) -> Result<(), Error> {
        let from_name = self.agent_name();
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_group_session_im(
            group_id,
            ImDialog::SessionGroupStart,
            "",
            &from_name,
            now,
        )?;
        Ok(())
    }

    /// Sends a message to a group's IM session (`ImprovedInstantMessage`,
    /// `IM_SESSION_SEND`, session id = group id). Other members receive it as
    /// [`Event::GroupSessionMessage`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the message fails to encode.
    pub fn send_group_message(
        &mut self,
        group_id: Uuid,
        message: &str,
        now: Instant,
    ) -> Result<(), Error> {
        let from_name = self.agent_name();
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_group_session_im(group_id, ImDialog::SessionSend, message, &from_name, now)?;
        Ok(())
    }

    /// Leaves a group's IM session (`ImprovedInstantMessage`,
    /// `IM_SESSION_LEAVE`), so the agent stops receiving the group's chat without
    /// leaving the group itself.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the message fails to encode.
    pub fn leave_group_session(&mut self, group_id: Uuid, now: Instant) -> Result<(), Error> {
        let from_name = self.agent_name();
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_group_session_im(group_id, ImDialog::SessionLeave, "", &from_name, now)?;
        Ok(())
    }

    /// Replies to a scripted-object dialog (`ScriptDialogReply`): the chosen
    /// `button_index`/`button_label` (from the [`Event::ScriptDialog`]'s
    /// [`ScriptDialog::buttons`]) is sent back to `object_id` on the dialog's
    /// hidden `chat_channel`. For an `llTextBox`, pass the typed text as
    /// `button_label` with `button_index` `0`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the reply fails to encode.
    pub fn reply_script_dialog(
        &mut self,
        object_id: Uuid,
        chat_channel: i32,
        button_index: i32,
        button_label: &str,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_script_dialog_reply(
            object_id,
            chat_channel,
            button_index,
            button_label,
            now,
        )?;
        Ok(())
    }

    /// Answers a scripted-object permission request (`ScriptAnswerYes`) from the
    /// [`Event::ScriptPermissionRequest`]: grants the `permissions` bitfield (a
    /// subset of those requested) to the script `item_id` in object `task_id`.
    /// Pass [`ScriptPermissions::default`] (an empty set) to deny everything.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the reply fails to encode.
    pub fn answer_script_permissions(
        &mut self,
        task_id: Uuid,
        item_id: Uuid,
        permissions: ScriptPermissions,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_script_answer_yes(task_id, item_id, permissions.0, now)?;
        Ok(())
    }

    /// Requests the agent's mute (block) list (`MuteListRequest` with a zero
    /// CRC, forcing a fresh download). The simulator replies with the list (the
    /// file is downloaded over the `Xfer` path and surfaced as
    /// [`Event::MuteList`]), or with [`Event::MuteListUnchanged`] /
    /// [`Event::MuteList`]`([])` for an unchanged or empty list.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_mute_list(&mut self, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_mute_list_request(0, now)?;
        Ok(())
    }

    /// Mutes (blocks) an entity (`UpdateMuteListEntry`). `mute_type` selects what
    /// is muted (use [`MuteType::Agent`] for an avatar); `name` is its display
    /// name (required, especially for [`MuteType::ByName`] where `id` is nil);
    /// `flags` are the per-aspect *exceptions* (use [`MuteFlags::default`] to mute
    /// everything). Re-request the list to see the change.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn mute(
        &mut self,
        id: Uuid,
        name: &str,
        mute_type: MuteType,
        flags: MuteFlags,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_update_mute_list_entry(id, name, mute_type.to_i32(), flags.0, now)?;
        Ok(())
    }

    /// Removes a mute (`RemoveMuteListEntry`). `id` and `name` must match the
    /// existing entry (from [`Event::MuteList`]).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn unmute(&mut self, id: Uuid, name: &str, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_remove_mute_list_entry(id, name, now)?;
        Ok(())
    }

    /// The agent's own id, once login has established the circuit. Useful as the
    /// `owner_id` for inventory fetches and for recognising the client's own
    /// messages.
    #[must_use]
    pub fn agent_id(&self) -> Option<Uuid> {
        self.circuit.as_ref().map(|circuit| circuit.agent_id)
    }

    /// The agent's inventory root ("My Inventory") folder id, from the login
    /// response, or `None` if the grid did not provide it. Use it as the starting
    /// point for [`Session::request_folder_contents`].
    #[must_use]
    pub const fn inventory_root(&self) -> Option<Uuid> {
        self.inventory_root
    }

    /// Requests the contents (sub-folders and items) of the inventory folder
    /// `folder_id` via `FetchInventoryDescendents`. The reply arrives as
    /// [`Event::InventoryDescendents`]. The folder structure as a whole is also
    /// available upfront from [`Event::InventorySkeleton`] (login).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_folder_contents(&mut self, folder_id: Uuid, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_fetch_inventory_descendents(folder_id, now)?;
        Ok(())
    }

    /// Requests the region's info (agent and object limits) via
    /// `RequestRegionInfo`. The reply arrives as an [`Event::RegionLimits`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_region_info(&mut self, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_request_region_info(now)?;
        Ok(())
    }

    /// Requests the agent's current L$ balance via `MoneyBalanceRequest`. The
    /// reply arrives as an [`Event::MoneyBalance`]. The simulator also pushes a
    /// `MoneyBalanceReply` unsolicited whenever a transaction changes the balance.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_money_balance(&mut self, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_money_balance_request(now)?;
        Ok(())
    }

    /// Requests the grid's economy data (upload/claim/group prices and region
    /// object capacity) via `EconomyDataRequest`. The reply arrives as an
    /// [`Event::EconomyData`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_economy_data(&mut self, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_economy_data_request(now)?;
        Ok(())
    }

    /// Pays `amount` L$ to another avatar or object via `MoneyTransferRequest`.
    /// `kind` selects the transaction type (e.g. [`MoneyTransactionType::Gift`]
    /// for a direct avatar payment, [`MoneyTransactionType::PayObject`] for a
    /// scripted object); `description` annotates the transaction. The grid pushes
    /// a fresh [`Event::MoneyBalance`] once the transfer settles. The amount is
    /// clamped to the `i32` wire range.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn send_money_transfer(
        &mut self,
        dest: Uuid,
        amount: LindenAmount,
        kind: MoneyTransactionType,
        description: &str,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        let amount = i32::try_from(amount.0).unwrap_or(i32::MAX);
        circuit.send_money_transfer(dest, amount, kind.to_i32(), description, now)?;
        Ok(())
    }

    /// Requests `ParcelProperties` for the parcel overlapping the given metre
    /// rectangle (region-local coordinates). `sequence_id` is echoed back in the
    /// reply ([`Event::ParcelProperties`]) so callers can match outstanding
    /// queries.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_parcel_properties(
        &mut self,
        west: f32,
        south: f32,
        east: f32,
        north: f32,
        sequence_id: i32,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_parcel_properties_request(west, south, east, north, sequence_id, now)?;
        Ok(())
    }

    /// Requests world-map blocks for the inclusive grid-coordinate rectangle
    /// `[min_x, max_x] x [min_y, max_y]` (region indices). Each region in range
    /// arrives as an [`Event::MapBlock`], giving its name, coordinates, and
    /// maturity. Coordinates are clamped to the protocol's 16-bit range.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_map_blocks(
        &mut self,
        min_x: u32,
        max_x: u32,
        min_y: u32,
        max_y: u32,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        let clamp = |value: u32| u16::try_from(value).unwrap_or(u16::MAX);
        circuit.send_map_block_request(
            clamp(min_x),
            clamp(max_x),
            clamp(min_y),
            clamp(max_y),
            now,
        )?;
        Ok(())
    }

    /// Searches the world map for regions whose name matches `name` via
    /// `MapNameRequest`. Each match arrives as an [`Event::MapBlock`] (the same
    /// reply as [`Session::request_map_blocks`]). Useful for resolving a region
    /// name to its handle/coordinates without knowing where it sits on the grid.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_map_by_name(&mut self, name: &str, now: Instant) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_map_name_request(name, now)?;
        Ok(())
    }

    /// Requests world-map overlay items of the given [`MapItemType`] (avatar
    /// locations, telehubs, land for sale, events) via `MapItemRequest`.
    /// `region_handle` of 0 targets the current region; any other handle targets
    /// that region. The reply arrives as an [`Event::MapItems`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if no circuit is established yet, or
    /// [`Error::Wire`] if the request fails to encode.
    pub fn request_map_items(
        &mut self,
        item_type: MapItemType,
        region_handle: u64,
        now: Instant,
    ) -> Result<(), Error> {
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_map_item_request(item_type.to_u32(), region_handle, now)?;
        Ok(())
    }

    /// Requests an in-world teleport to `position` (region-local) in the region
    /// identified by `region_handle`, looking towards `look_at`. On success the
    /// session re-establishes its circuit at the destination simulator and emits
    /// [`Event::RegionChanged`]; on failure it emits [`Event::TeleportFailed`]
    /// and stays connected to the current region.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NotActive`] if the session is not in the active state,
    /// [`Error::NoCircuit`] if no circuit is established, or [`Error::Wire`] if
    /// the request fails to encode.
    pub fn teleport_to(
        &mut self,
        region_handle: u64,
        position: Vector,
        look_at: Vector,
        now: Instant,
    ) -> Result<(), Error> {
        if !matches!(self.state, SessionState::Active) {
            return Err(Error::NotActive);
        }
        let circuit = self.circuit.as_mut().ok_or(Error::NoCircuit)?;
        circuit.send_teleport_location_request(region_handle, position, look_at, now)?;
        circuit.timers.teleport = Some(deadline(now, TELEPORT_TIMEOUT));
        self.teleport_target = Some(region_handle);
        self.state = SessionState::Teleporting;
        Ok(())
    }

    /// Begins a clean logout: queues a `LogoutRequest` and arms the logout
    /// timeout. Does nothing if the session is already closing or closed.
    pub fn initiate_logout(&mut self, now: Instant) {
        if matches!(self.state, SessionState::Closed | SessionState::LoggingOut) {
            return;
        }
        match self.circuit.as_mut() {
            Some(circuit) => {
                if circuit.send_logout_request(now).is_err() {
                    self.close(DisconnectReason::ProtocolError);
                    return;
                }
                circuit.timers.logout = Some(deadline(now, LOGOUT_TIMEOUT));
                self.state = SessionState::LoggingOut;
            }
            None => self.close(DisconnectReason::ProtocolError),
        }
    }

    /// The next datagram to transmit, if any: the root circuit's queue first,
    /// then each child circuit's, so the driver can multiplex all circuits onto
    /// one socket using [`Transmit::destination`].
    pub fn poll_transmit(&mut self) -> Option<Transmit> {
        if let Some(circuit) = self.circuit.as_mut()
            && let Some(payload) = circuit.out.pop_front()
        {
            return Some(Transmit {
                destination: circuit.sim_addr,
                payload,
            });
        }
        for circuit in self.children.values_mut() {
            if let Some(payload) = circuit.out.pop_front() {
                return Some(Transmit {
                    destination: circuit.sim_addr,
                    payload,
                });
            }
        }
        None
    }

    /// The earliest instant at which [`Self::handle_timeout`] should next run.
    #[must_use]
    pub fn poll_timeout(&self) -> Option<Instant> {
        if matches!(self.state, SessionState::Closed) {
            return None;
        }
        let circuit = self.circuit.as_ref()?;
        let mut earliest = Some(circuit.timers.inactivity);
        merge_deadline(&mut earliest, circuit.timers.ack_flush);
        merge_deadline(&mut earliest, circuit.timers.agent_update);
        merge_deadline(&mut earliest, circuit.timers.logout);
        merge_deadline(&mut earliest, circuit.timers.teleport);
        merge_deadline(&mut earliest, circuit.next_resend_deadline());
        for child in self.children.values() {
            merge_deadline(&mut earliest, Some(child.timers.inactivity));
            merge_deadline(&mut earliest, child.timers.ack_flush);
            merge_deadline(&mut earliest, child.next_resend_deadline());
        }
        earliest
    }

    /// The next high-level event, if any.
    pub fn poll_event(&mut self) -> Option<Event> {
        self.events.pop_front()
    }

    /// Returns `true` once the session has reached its terminal state.
    #[must_use]
    pub const fn is_closed(&self) -> bool {
        matches!(self.state, SessionState::Closed)
    }

    /// Transitions to the closed state, emitting a disconnect event once.
    fn close(&mut self, reason: DisconnectReason) {
        if !matches!(self.state, SessionState::Closed) {
            self.state = SessionState::Closed;
            self.events.push_back(Event::Disconnected(reason));
        }
    }
}

/// Decodes name/SKU bytes to a `String`, dropping any trailing NUL padding the
/// simulator appends to fixed-width string fields.
fn trimmed_string(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .trim_end_matches('\0')
        .to_owned()
}

/// Encodes a string as NUL-terminated UTF-8 bytes, as the viewer sends variable
/// string fields on the wire.
fn with_nul(s: &str) -> Vec<u8> {
    let mut bytes = s.as_bytes().to_vec();
    bytes.push(0);
    bytes
}

/// Parses a downloaded mute-list file into [`MuteEntry`] values. Each non-empty
/// line is `<type> <uuid> <name>|<flags>` (the viewer's on-disk format).
fn parse_mute_list(bytes: &[u8]) -> Vec<MuteEntry> {
    String::from_utf8_lossy(bytes)
        .lines()
        .filter_map(parse_mute_line)
        .collect()
}

/// Parses one mute-list line, or `None` if it is blank/malformed.
fn parse_mute_line(line: &str) -> Option<MuteEntry> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    // The flags follow the last '|'; everything before is "<type> <uuid> <name>".
    let (head, flags) = line.rsplit_once('|').map_or((line, 0), |(head, tail)| {
        (head, tail.trim().parse().unwrap_or(0))
    });
    let mut parts = head.splitn(3, ' ');
    let mute_type = parts.next()?.trim().parse::<i32>().ok()?;
    let id = Uuid::parse_str(parts.next()?.trim()).unwrap_or_else(|_| Uuid::nil());
    let name = parts.next().unwrap_or("").trim().to_owned();
    Some(MuteEntry {
        id,
        name,
        mute_type: MuteType::from_i32(mute_type),
        flags: MuteFlags(flags),
    })
}

/// Builds a [`RegionIdentity`] from a `RegionHandshake`'s region-info blocks.
fn region_identity(
    info: &RegionHandshakeRegionInfoBlock,
    info3: &RegionHandshakeRegionInfo3Block,
) -> RegionIdentity {
    let product_sku = trimmed_string(&info3.product_sku);
    let product_name = trimmed_string(&info3.product_name);
    RegionIdentity {
        sim_name: trimmed_string(&info.sim_name),
        region_flags: info.region_flags,
        maturity: Maturity::from_sim_access(info.sim_access),
        product: ProductType::classify(&product_sku, &product_name),
        product_sku,
        product_name,
    }
}

/// Builds [`RegionLimits`] from a `RegionInfo` message's region-info blocks.
fn region_limits(
    info: &RegionInfoRegionInfoBlock,
    info2: &RegionInfoRegionInfo2Block,
) -> RegionLimits {
    // Prefer the 32-bit agent cap; fall back to the legacy 8-bit field when the
    // grid leaves the wider one at zero.
    let max_agents = if info2.max_agents32 == 0 {
        u32::from(info.max_agents)
    } else {
        info2.max_agents32
    };
    RegionLimits {
        sim_name: trimmed_string(&info.sim_name),
        max_agents,
        hard_max_agents: info2.hard_max_agents,
        hard_max_objects: info2.hard_max_objects,
        region_flags: info.region_flags,
        maturity: Maturity::from_sim_access(info.sim_access),
    }
}

/// Builds a [`MoneyBalance`] from a `MoneyBalanceReply`. The optional
/// `TransactionInfo` block is all-zero for a plain balance poll; it is surfaced
/// only when it describes a real transaction (non-zero type).
fn money_balance(reply: &sl_wire::messages::MoneyBalanceReply) -> MoneyBalance {
    let data = &reply.money_data;
    let info = &reply.transaction_info;
    let transaction = (info.transaction_type != 0).then(|| MoneyTransaction {
        transaction_type: info.transaction_type,
        source_id: info.source_id,
        source_is_group: info.is_source_group,
        dest_id: info.dest_id,
        dest_is_group: info.is_dest_group,
        amount: LindenAmount(u64::try_from(info.amount).unwrap_or(0)),
        item_description: trimmed_string(&info.item_description),
    });
    MoneyBalance {
        agent_id: data.agent_id,
        success: data.transaction_success,
        balance: LindenAmount(u64::try_from(data.money_balance).unwrap_or(0)),
        square_meters_credit: data.square_meters_credit,
        square_meters_committed: data.square_meters_committed,
        description: trimmed_string(&data.description),
        transaction,
    }
}

/// Builds [`EconomyData`] from an `EconomyData` message's info block.
const fn economy_data(data: &sl_wire::messages::EconomyData) -> EconomyData {
    let info = &data.info;
    EconomyData {
        object_capacity: info.object_capacity,
        object_count: info.object_count,
        price_energy_unit: info.price_energy_unit,
        price_object_claim: info.price_object_claim,
        price_public_object_decay: info.price_public_object_decay,
        price_public_object_delete: info.price_public_object_delete,
        price_parcel_claim: info.price_parcel_claim,
        price_parcel_claim_factor: info.price_parcel_claim_factor,
        price_upload: info.price_upload,
        price_rent_light: info.price_rent_light,
        teleport_min_price: info.teleport_min_price,
        teleport_price_exponent: info.teleport_price_exponent,
        energy_efficiency: info.energy_efficiency,
        price_object_rent: info.price_object_rent,
        price_object_scale_factor: info.price_object_scale_factor,
        price_parcel_rent: info.price_parcel_rent,
        price_group_create: info.price_group_create,
    }
}

/// Builds a [`ParcelInfo`] from a `ParcelProperties` parcel-data block.
fn parcel_info(data: &ParcelPropertiesParcelDataBlock) -> ParcelInfo {
    ParcelInfo {
        sequence_id: data.sequence_id,
        local_id: data.local_id,
        aabb_min: (data.aabb_min.x, data.aabb_min.y, data.aabb_min.z),
        aabb_max: (data.aabb_max.x, data.aabb_max.y, data.aabb_max.z),
        area: data.area,
        bitmap: data.bitmap.clone(),
        max_prims: data.max_prims,
        sim_wide_max_prims: data.sim_wide_max_prims,
        sim_wide_total_prims: data.sim_wide_total_prims,
        owner_id: data.owner_id,
        raw_parcel_flags: data.parcel_flags,
    }
}

/// Builds a [`ChatMessage`] from a `ChatFromSimulator` chat-data block. The
/// `FromName` and `Message` strings carry trailing NUL padding, which is removed.
fn chat_message(data: &ChatFromSimulatorChatDataBlock) -> ChatMessage {
    ChatMessage {
        from_name: trimmed_string(&data.from_name),
        source_id: data.source_id,
        owner_id: data.owner_id,
        source_type: ChatSourceType::from_u8(data.source_type),
        chat_type: ChatType::from_u8(data.chat_type),
        audible: ChatAudible::from_u8(data.audible),
        position: (data.position.x, data.position.y, data.position.z),
        message: trimmed_string(&data.message),
    }
}

/// Computes the canonical 1:1 IM session id the viewer uses: the byte-wise XOR
/// of the two agent ids, except an IM to oneself (where the XOR would be nil)
/// uses the agent id directly.
fn compute_im_session_id(agent_id: Uuid, other: Uuid) -> Uuid {
    if agent_id == other {
        return agent_id;
    }
    let mut out = [0u8; 16];
    for (slot, (a, b)) in out
        .iter_mut()
        .zip(agent_id.as_bytes().iter().zip(other.as_bytes()))
    {
        *slot = a ^ b;
    }
    Uuid::from_bytes(out)
}

/// Builds an [`InstantMessage`] from an `ImprovedInstantMessage`'s agent-data and
/// message blocks. The `FromAgentName` and `Message` strings carry trailing NUL
/// padding, which is removed.
fn instant_message(
    agent_data: &ImprovedInstantMessageAgentDataBlock,
    block: &ImprovedInstantMessageMessageBlockBlock,
) -> InstantMessage {
    InstantMessage {
        from_agent_id: agent_data.agent_id,
        from_agent_name: trimmed_string(&block.from_agent_name),
        to_agent_id: block.to_agent_id,
        dialog: ImDialog::from_u8(block.dialog),
        from_group: block.from_group,
        region_id: block.region_id,
        position: (block.position.x, block.position.y, block.position.z),
        offline: block.offline != 0,
        timestamp: block.timestamp,
        id: block.id,
        parent_estate_id: block.parent_estate_id,
        message: trimmed_string(&block.message),
        binary_bucket: block.binary_bucket.clone(),
    }
}

/// Builds [`AvatarProperties`] from an `AvatarPropertiesReply` properties block.
fn avatar_properties(
    avatar_id: Uuid,
    data: &AvatarPropertiesReplyPropertiesDataBlock,
) -> AvatarProperties {
    AvatarProperties {
        avatar_id,
        image_id: data.image_id,
        fl_image_id: data.fl_image_id,
        partner_id: data.partner_id,
        about_text: trimmed_string(&data.about_text),
        fl_about_text: trimmed_string(&data.fl_about_text),
        born_on: trimmed_string(&data.born_on),
        profile_url: trimmed_string(&data.profile_url),
        charter_member: trimmed_string(&data.charter_member),
        flags: data.flags,
    }
}

/// Builds [`AvatarInterests`] from an `AvatarInterestsReply` properties block.
fn avatar_interests(
    avatar_id: Uuid,
    data: &AvatarInterestsReplyPropertiesDataBlock,
) -> AvatarInterests {
    AvatarInterests {
        avatar_id,
        want_to_mask: data.want_to_mask,
        want_to_text: trimmed_string(&data.want_to_text),
        skills_mask: data.skills_mask,
        skills_text: trimmed_string(&data.skills_text),
        languages_text: trimmed_string(&data.languages_text),
    }
}

/// Builds an [`AvatarGroupMembership`] from an `AvatarGroupsReply` group entry.
fn avatar_group(data: &AvatarGroupsReplyGroupDataBlock) -> AvatarGroupMembership {
    AvatarGroupMembership {
        group_id: data.group_id,
        group_name: trimmed_string(&data.group_name),
        group_title: trimmed_string(&data.group_title),
        group_powers: data.group_powers,
        accept_notices: data.accept_notices,
        group_insignia_id: data.group_insignia_id,
    }
}

/// Converts a login [`SkeletonFolder`] into an [`InventoryFolder`].
fn skeleton_folder(folder: &SkeletonFolder) -> InventoryFolder {
    InventoryFolder {
        folder_id: folder.folder_id,
        parent_id: folder.parent_id,
        name: folder.name.clone(),
        folder_type: folder.type_default,
        version: folder.version,
    }
}

/// Builds a [`Friend`] from a login `buddy-list` entry.
const fn friend(entry: &sl_wire::BuddyListEntry) -> Friend {
    Friend {
        id: entry.buddy_id,
        rights_granted: FriendRights(entry.rights_granted),
        rights_received: FriendRights(entry.rights_has),
    }
}

/// Builds [`ActiveGroup`] from an `AgentDataUpdate` block.
fn active_group(data: &AgentDataUpdateAgentDataBlock) -> ActiveGroup {
    ActiveGroup {
        agent_id: data.agent_id,
        first_name: trimmed_string(&data.first_name),
        last_name: trimmed_string(&data.last_name),
        group_title: trimmed_string(&data.group_title),
        active_group_id: data.active_group_id,
        group_powers: data.group_powers,
        group_name: trimmed_string(&data.group_name),
    }
}

/// Builds [`GroupMembership`] from an `AgentGroupDataUpdate` entry.
fn group_membership(data: &AgentGroupDataUpdateGroupDataBlock) -> GroupMembership {
    GroupMembership {
        group_id: data.group_id,
        group_powers: data.group_powers,
        accept_notices: data.accept_notices,
        group_insignia_id: data.group_insignia_id,
        contribution: data.contribution,
        group_name: trimmed_string(&data.group_name),
    }
}

/// Builds [`GroupMember`] from a `GroupMembersReply` entry.
fn group_member(data: &GroupMembersReplyMemberDataBlock) -> GroupMember {
    GroupMember {
        agent_id: data.agent_id,
        contribution: data.contribution,
        online_status: trimmed_string(&data.online_status),
        agent_powers: data.agent_powers,
        title: trimmed_string(&data.title),
        is_owner: data.is_owner,
    }
}

/// Builds [`GroupRole`] from a `GroupRoleDataReply` entry.
fn group_role(data: &GroupRoleDataReplyRoleDataBlock) -> GroupRole {
    GroupRole {
        role_id: data.role_id,
        name: trimmed_string(&data.name),
        title: trimmed_string(&data.title),
        description: trimmed_string(&data.description),
        powers: data.powers,
        members: data.members,
    }
}

/// Builds [`GroupTitle`] from a `GroupTitlesReply` entry.
fn group_title(data: &GroupTitlesReplyGroupDataBlock) -> GroupTitle {
    GroupTitle {
        title: trimmed_string(&data.title),
        role_id: data.role_id,
        selected: data.selected,
    }
}

/// Builds [`GroupProfile`] from a `GroupProfileReply` block.
fn group_profile(data: &GroupProfileReplyGroupDataBlock) -> GroupProfile {
    GroupProfile {
        group_id: data.group_id,
        name: trimmed_string(&data.name),
        charter: trimmed_string(&data.charter),
        show_in_list: data.show_in_list,
        member_title: trimmed_string(&data.member_title),
        powers: data.powers_mask,
        insignia_id: data.insignia_id,
        founder_id: data.founder_id,
        membership_fee: data.membership_fee,
        open_enrollment: data.open_enrollment,
        money: data.money,
        member_count: data.group_membership_count,
        role_count: data.group_roles_count,
        allow_publish: data.allow_publish,
        mature_publish: data.mature_publish,
        owner_role: data.owner_role,
    }
}

/// Builds [`GroupNotice`] from a `GroupNoticesListReply` entry.
fn group_notice(data: &GroupNoticesListReplyDataBlock) -> GroupNotice {
    GroupNotice {
        notice_id: data.notice_id,
        timestamp: data.timestamp,
        from_name: trimmed_string(&data.from_name),
        subject: trimmed_string(&data.subject),
        has_attachment: data.has_attachment,
        asset_type: data.asset_type,
    }
}

/// Builds a [`ScriptDialog`] value from a `ScriptDialog` message.
fn script_dialog(message: &sl_wire::messages::ScriptDialog) -> ScriptDialog {
    let data = &message.data;
    ScriptDialog {
        object_id: data.object_id,
        object_name: trimmed_string(&data.object_name),
        owner_first_name: trimmed_string(&data.first_name),
        owner_last_name: trimmed_string(&data.last_name),
        owner_id: message
            .owner_data
            .first()
            .map_or_else(Uuid::nil, |owner| owner.owner_id),
        message: trimmed_string(&data.message),
        chat_channel: data.chat_channel,
        image_id: data.image_id,
        buttons: message
            .buttons
            .iter()
            .map(|button| trimmed_string(&button.button_label))
            .collect(),
    }
}

/// Builds a [`ScriptPermissionRequest`] value from a `ScriptQuestion` message.
fn script_permission_request(
    message: &sl_wire::messages::ScriptQuestion,
) -> ScriptPermissionRequest {
    let data = &message.data;
    ScriptPermissionRequest {
        task_id: data.task_id,
        item_id: data.item_id,
        object_name: trimmed_string(&data.object_name),
        object_owner: trimmed_string(&data.object_owner),
        experience_id: message.experience.experience_id,
        permissions: ScriptPermissions(data.questions),
    }
}

/// Builds an [`InventoryFolder`] from an `InventoryDescendents` folder entry.
/// Such entries carry no per-folder version, so it is reported as `0`.
fn inventory_folder(data: &InventoryDescendentsFolderDataBlock) -> InventoryFolder {
    InventoryFolder {
        folder_id: data.folder_id,
        parent_id: data.parent_id,
        name: trimmed_string(&data.name),
        folder_type: data.r#type,
        version: 0,
    }
}

/// Builds an [`InventoryItem`] from an `InventoryDescendents` item entry.
fn inventory_item(data: &InventoryDescendentsItemDataBlock) -> InventoryItem {
    InventoryItem {
        item_id: data.item_id,
        folder_id: data.folder_id,
        name: trimmed_string(&data.name),
        description: trimmed_string(&data.description),
        asset_id: data.asset_id,
        item_type: data.r#type,
        inv_type: data.inv_type,
        flags: data.flags,
        sale_type: data.sale_type,
        sale_price: data.sale_price,
        creation_date: data.creation_date,
        owner_id: data.owner_id,
        creator_id: data.creator_id,
        group_id: data.group_id,
        group_owned: data.group_owned,
        base_mask: data.base_mask,
        owner_mask: data.owner_mask,
        group_mask: data.group_mask,
        everyone_mask: data.everyone_mask,
        next_owner_mask: data.next_owner_mask,
    }
}

/// Builds a [`NeighborInfo`] from an `EnableSimulator` simulator-info block.
fn neighbor_info(info: &EnableSimulatorSimulatorInfoBlock) -> NeighborInfo {
    // IPPORT is big-endian (network order) on the wire, but the generated field
    // decoder reads it as a little-endian U16, so swap the bytes back to host
    // order here. (IPADDR is raw octets in order and needs no swap.)
    let port = info.port.swap_bytes();
    let sim = SocketAddr::new(IpAddr::V4(Ipv4Addr::from(info.ip)), port);
    let (grid_x, grid_y) = handle_to_grid(info.handle);
    NeighborInfo {
        region_handle: info.handle,
        sim,
        grid_x,
        grid_y,
    }
}

/// Builds a [`MapRegionInfo`] from a `MapBlockReply` data block (with its
/// optional size block), or `None` for a sentinel/empty entry.
fn map_region_info(
    data: &MapBlockReplyDataBlock,
    size: Option<&MapBlockReplySizeBlock>,
) -> Option<MapRegionInfo> {
    // The map sends a sentinel block (0,0 / empty name) for "not found".
    if data.x == 0 && data.y == 0 {
        return None;
    }
    let name = trimmed_string(&data.name);
    if name.is_empty() {
        return None;
    }
    let grid_x = u32::from(data.x);
    let grid_y = u32::from(data.y);
    Some(MapRegionInfo {
        name,
        grid_x,
        grid_y,
        region_handle: grid_to_handle(grid_x, grid_y),
        maturity: Maturity::from_sim_access(data.access),
        region_flags: data.region_flags,
        size_x: size
            .map(|block| u32::from(block.size_x))
            .filter(|&value| value != 0)
            .unwrap_or(256),
        size_y: size
            .map(|block| u32::from(block.size_y))
            .filter(|&value| value != 0)
            .unwrap_or(256),
        agents: data.agents,
        map_image_id: data.map_image_id,
    })
}

/// Builds a [`MapItem`] from a `MapItemReply` data block. Coordinates are global
/// metres; `extra`/`extra2` are type-specific (see [`MapItem`]).
fn map_item(data: &sl_wire::messages::MapItemReplyDataBlock) -> MapItem {
    MapItem {
        global_x: data.x,
        global_y: data.y,
        id: data.id,
        extra: data.extra,
        extra2: data.extra2,
        name: trimmed_string(&data.name),
    }
}

/// Extracts the destination UDP address and seed capability from a CAPS
/// `TeleportFinish` event body: `{ "Info": [ { "SimIP": <binary 4 bytes>,
/// "SimPort": <integer>, "SeedCapability": <string>, … } ] }`. The CAPS `SimPort`
/// is a plain host-order integer port (unlike the byte-swapped generated-UDP field).
fn teleport_finish_from_llsd(body: &Llsd) -> Option<(SocketAddr, String)> {
    let info = body.get("Info").and_then(|info| info.index(0))?;
    let octets: [u8; 4] = info
        .get("SimIP")
        .and_then(Llsd::as_binary)?
        .try_into()
        .ok()?;
    let port = u16::try_from(info.get("SimPort").and_then(Llsd::as_i32)?).ok()?;
    let seed = info
        .get("SeedCapability")
        .and_then(Llsd::as_str)
        .unwrap_or("")
        .to_owned();
    Some((
        SocketAddr::new(IpAddr::V4(Ipv4Addr::from(octets)), port),
        seed,
    ))
}

/// Extracts a neighbour's region handle and simulator address from a CAPS
/// `EnableSimulator` event body: `{ "SimulatorInfo": [{ "Handle": <u64 binary>,
/// "IP": <4 bytes>, "Port": <integer> }] }`. Unlike the UDP message the port is
/// a plain integer (no byte swap).
fn enable_simulator_from_caps_llsd(body: &Llsd) -> Option<(u64, SocketAddr)> {
    let info = body.get("SimulatorInfo").and_then(|s| s.index(0))?;
    let handle = info.get("Handle").map(llsd_u64)?;
    let octets: [u8; 4] = info.get("IP").and_then(Llsd::as_binary)?.try_into().ok()?;
    let port = u16::try_from(info.get("Port").and_then(Llsd::as_i32)?).ok()?;
    Some((
        handle,
        SocketAddr::new(IpAddr::V4(Ipv4Addr::from(octets)), port),
    ))
}

/// Extracts the destination region handle, simulator address and seed capability
/// from a CAPS `CrossedRegion` event body: the `RegionData` array carries
/// `RegionHandle` (u64), `SimIP` (4 bytes), `SimPort` (plain integer, no swap)
/// and `SeedCapability` (url).
fn crossed_region_from_caps_llsd(body: &Llsd) -> Option<(u64, SocketAddr, String)> {
    let region = body.get("RegionData").and_then(|r| r.index(0))?;
    let handle = region.get("RegionHandle").map(llsd_u64)?;
    let octets: [u8; 4] = region
        .get("SimIP")
        .and_then(Llsd::as_binary)?
        .try_into()
        .ok()?;
    let port = u16::try_from(region.get("SimPort").and_then(Llsd::as_i32)?).ok()?;
    let seed = region
        .get("SeedCapability")
        .and_then(Llsd::as_str)
        .unwrap_or("")
        .to_owned();
    Some((
        handle,
        SocketAddr::new(IpAddr::V4(Ipv4Addr::from(octets)), port),
        seed,
    ))
}

/// Extracts the child region's simulator address and seed capability from a CAPS
/// `EstablishAgentCommunication` event body: `{ "sim-ip-and-port": "ip:port",
/// "seed-capability": url }`.
fn establish_agent_communication_from_llsd(body: &Llsd) -> Option<(SocketAddr, String)> {
    let sim = body.get("sim-ip-and-port").and_then(Llsd::as_str)?;
    let sim: SocketAddr = sim.parse().ok()?;
    let seed = body
        .get("seed-capability")
        .and_then(Llsd::as_str)
        .unwrap_or("")
        .to_owned();
    Some((sim, seed))
}

/// Builds a [`ParcelInfo`] from a CAPS `ParcelProperties` event body.
fn parcel_info_from_llsd(body: &Llsd) -> Option<ParcelInfo> {
    let data = body
        .get("ParcelData")
        .and_then(|parcel_data| parcel_data.index(0))?;
    Some(ParcelInfo {
        sequence_id: data.get("SequenceID").and_then(Llsd::as_i32).unwrap_or(0),
        local_id: data.get("LocalID").and_then(Llsd::as_i32).unwrap_or(0),
        aabb_min: vec3_from_llsd(data.get("AABBMin")),
        aabb_max: vec3_from_llsd(data.get("AABBMax")),
        area: data.get("Area").and_then(Llsd::as_i32).unwrap_or(0),
        bitmap: data
            .get("Bitmap")
            .and_then(Llsd::as_binary)
            .map(<[u8]>::to_vec)
            .unwrap_or_default(),
        max_prims: data.get("MaxPrims").and_then(Llsd::as_i32).unwrap_or(0),
        sim_wide_max_prims: data
            .get("SimWideMaxPrims")
            .and_then(Llsd::as_i32)
            .unwrap_or(0),
        sim_wide_total_prims: data
            .get("SimWideTotalPrims")
            .and_then(Llsd::as_i32)
            .unwrap_or(0),
        owner_id: data
            .get("OwnerID")
            .and_then(Llsd::as_uuid)
            .unwrap_or_else(Uuid::nil),
        raw_parcel_flags: data
            .get("ParcelFlags")
            .and_then(Llsd::as_i32)
            .unwrap_or(0)
            .cast_unsigned(),
    })
}

/// Reads a three-component vector (`[x, y, z]` reals) from an LLSD array.
fn vec3_from_llsd(value: Option<&Llsd>) -> (f32, f32, f32) {
    let component = |index: usize| {
        value
            .and_then(|vector| vector.index(index))
            .and_then(Llsd::as_f32)
            .unwrap_or(0.0)
    };
    (component(0), component(1), component(2))
}

/// Reads a UUID from an LLSD map member, defaulting to nil.
fn uuid_member(map: &Llsd, key: &str) -> Uuid {
    map.get(key)
        .and_then(Llsd::as_uuid)
        .unwrap_or_else(Uuid::nil)
}

/// Reads an `i32` from an LLSD map member, defaulting to `0`.
fn i32_member(map: &Llsd, key: &str) -> i32 {
    map.get(key).and_then(Llsd::as_i32).unwrap_or(0)
}

/// Reads a string from an LLSD map member, defaulting to empty.
fn string_member(map: &Llsd, key: &str) -> String {
    map.get(key).and_then(Llsd::as_str).unwrap_or("").to_owned()
}

/// Decodes a `u64` from an LLSD value as the viewer's `ll_U64_from_sd` does:
/// from an 8-byte big-endian binary, a hex/decimal string, or an integer.
fn llsd_u64(value: &Llsd) -> u64 {
    match value {
        Llsd::Binary(bytes) if bytes.len() >= 8 => bytes
            .iter()
            .take(8)
            .fold(0u64, |acc, &byte| (acc << 8) | u64::from(byte)),
        Llsd::String(s) => {
            let trimmed = s.trim().trim_start_matches("0x");
            u64::from_str_radix(trimmed, 16)
                .ok()
                .or_else(|| s.trim().parse().ok())
                .unwrap_or(0)
        }
        Llsd::Integer(i) => u64::try_from(*i).unwrap_or(0),
        _ => 0,
    }
}

/// Decodes the CAPS event-queue `AgentGroupDataUpdate` event (the modern
/// delivery of the agent's group memberships) into [`Event::GroupMemberships`].
/// The LLSD mirrors the UDP message: a `GroupData` array of per-group maps.
fn group_memberships_from_caps_llsd(body: &Llsd) -> Option<Event> {
    // The sim sometimes double-wraps the payload in a nested `body`.
    let body = body.get("body").unwrap_or(body);
    let groups = body.get("GroupData").and_then(Llsd::as_array)?;
    let memberships = groups
        .iter()
        .filter_map(|group| {
            let group_id = group.get("GroupID").and_then(Llsd::as_uuid)?;
            Some(GroupMembership {
                group_id,
                group_powers: group.get("GroupPowers").map_or(0, llsd_u64),
                accept_notices: group
                    .get("AcceptNotices")
                    .and_then(Llsd::as_bool)
                    .unwrap_or(false),
                group_insignia_id: group
                    .get("GroupInsigniaID")
                    .and_then(Llsd::as_uuid)
                    .unwrap_or_else(Uuid::nil),
                contribution: group
                    .get("Contribution")
                    .and_then(Llsd::as_i32)
                    .unwrap_or(0),
                group_name: group
                    .get("GroupName")
                    .and_then(Llsd::as_str)
                    .unwrap_or_default()
                    .to_owned(),
            })
        })
        .collect();
    Some(Event::GroupMemberships(memberships))
}

/// Decodes a `GroupMemberData` capability response into [`Event::GroupMembers`].
/// The LLSD is `{ group_id, members: { <id>: {...} }, titles: [...],
/// defaults: { default_powers } }`; per-member fields fall back to the defaults.
fn group_members_from_caps_llsd(body: &Llsd) -> Option<Event> {
    let group_id = body.get("group_id").and_then(Llsd::as_uuid)?;
    let Llsd::Map(members) = body.get("members")? else {
        return None;
    };
    let titles = body.get("titles").and_then(Llsd::as_array);
    let default_title = titles
        .and_then(|t| t.first())
        .and_then(Llsd::as_str)
        .unwrap_or_default();
    let default_powers = body
        .get("defaults")
        .and_then(|d| d.get("default_powers"))
        .map_or(0, llsd_u64);

    let mut roster: Vec<GroupMember> = members
        .iter()
        .filter_map(|(member_id, info)| {
            let agent_id = Uuid::parse_str(member_id).ok()?;
            let title = info
                .get("title")
                .and_then(Llsd::as_i32)
                .and_then(|index| titles?.get(usize::try_from(index).ok()?))
                .and_then(Llsd::as_str)
                .unwrap_or(default_title)
                .to_owned();
            Some(GroupMember {
                agent_id,
                contribution: info
                    .get("donated_square_meters")
                    .and_then(Llsd::as_i32)
                    .unwrap_or(0),
                online_status: info
                    .get("last_login")
                    .and_then(Llsd::as_str)
                    .unwrap_or("unknown")
                    .to_owned(),
                agent_powers: info.get("powers").map_or(default_powers, llsd_u64),
                title,
                is_owner: info.get("owner").is_some(),
            })
        })
        .collect();
    // The members map is unordered; sort by id for deterministic output.
    roster.sort_by_key(|member| member.agent_id);
    let member_count = i32::try_from(roster.len()).unwrap_or(i32::MAX);
    Some(Event::GroupMembers {
        group_id,
        request_id: Uuid::nil(),
        member_count,
        members: roster,
    })
}

/// Parses a `FetchInventoryDescendents2` CAPS response body into one
/// [`Event::InventoryDescendents`] per returned folder. The HTTP response shape
/// differs from the UDP `InventoryDescendents`, but yields the same value types.
fn inventory_descendents_from_llsd(body: &Llsd) -> Vec<Event> {
    let Some(folders) = body.get("folders").and_then(Llsd::as_array) else {
        return Vec::new();
    };
    folders
        .iter()
        .map(|folder| {
            let categories = folder
                .get("categories")
                .and_then(Llsd::as_array)
                .unwrap_or(&[]);
            let items = folder.get("items").and_then(Llsd::as_array).unwrap_or(&[]);
            Event::InventoryDescendents {
                folder_id: uuid_member(folder, "folder_id"),
                version: i32_member(folder, "version"),
                descendents: i32_member(folder, "descendents"),
                folders: categories.iter().map(inventory_folder_from_llsd).collect(),
                items: items.iter().map(inventory_item_from_llsd).collect(),
            }
        })
        .collect()
}

/// Builds an [`InventoryFolder`] from a CAPS `categories` entry.
fn inventory_folder_from_llsd(category: &Llsd) -> InventoryFolder {
    InventoryFolder {
        folder_id: uuid_member(category, "category_id"),
        parent_id: uuid_member(category, "parent_id"),
        name: string_member(category, "name"),
        folder_type: i8::try_from(i32_member(category, "type_default")).unwrap_or(-1),
        version: i32_member(category, "version"),
    }
}

/// Builds an [`InventoryItem`] from a CAPS `items` entry (with nested
/// `permissions` and `sale_info` maps).
fn inventory_item_from_llsd(item: &Llsd) -> InventoryItem {
    let permissions = item.get("permissions");
    let sale_info = item.get("sale_info");
    let perm = |key: &str| {
        permissions
            .map_or(0, |p| i32_member(p, key))
            .cast_unsigned()
    };
    let perm_uuid = |key: &str| permissions.map_or_else(Uuid::nil, |p| uuid_member(p, key));
    InventoryItem {
        item_id: uuid_member(item, "item_id"),
        folder_id: uuid_member(item, "parent_id"),
        name: string_member(item, "name"),
        description: string_member(item, "desc"),
        asset_id: uuid_member(item, "asset_id"),
        item_type: i8::try_from(i32_member(item, "type")).unwrap_or(-1),
        inv_type: i8::try_from(i32_member(item, "inv_type")).unwrap_or(-1),
        flags: i32_member(item, "flags").cast_unsigned(),
        sale_type: sale_info.map_or(0, |s| u8::try_from(i32_member(s, "sale_type")).unwrap_or(0)),
        sale_price: sale_info.map_or(0, |s| i32_member(s, "sale_price")),
        creation_date: i32_member(item, "created_at"),
        owner_id: perm_uuid("owner_id"),
        creator_id: perm_uuid("creator_id"),
        group_id: perm_uuid("group_id"),
        group_owned: permissions
            .and_then(|p| p.get("is_owner_group"))
            .and_then(Llsd::as_bool)
            .unwrap_or(false),
        base_mask: perm("base_mask"),
        owner_mask: perm("owner_mask"),
        group_mask: perm("group_mask"),
        everyone_mask: perm("everyone_mask"),
        next_owner_mask: perm("next_owner_mask"),
    }
}
