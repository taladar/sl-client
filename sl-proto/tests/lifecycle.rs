//! Scripted-peer, simulated-clock tests for the full session lifecycle:
//! login -> circuit -> handshake -> keep-alive -> logout.

#[cfg(test)]
mod test {
    use std::error::Error;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::time::{Duration, Instant};

    use pretty_assertions::{assert_eq, assert_ne};
    use sl_proto::{
        AssetType, Camera, ChatAudible, ChatSourceType, ChatType, ClassifiedUpdate, ClickAction,
        ControlFlags, CreateGroupParams, DeRezDestination, DisconnectReason, EstateAccessDelta,
        EstateAccessKind, Event, FriendRights, GroupNoticeAttachment, GroupRoleChange,
        GroupRoleEdit, GroupRoleMemberChange, GroupRoleUpdateType, ImDialog, ImageCodec,
        InterestsUpdate, InventoryItem, LindenAmount, LoginParams, MapItemType, Material, Maturity,
        MoneyTransactionType, MuteFlags, MuteType, NewInventoryItem, ObjectFlagSettings,
        ObjectTransform, ParcelAccessEntry, ParcelAccessScope, ParcelCategory, ParcelFlags,
        ParcelMediaCommand, ParcelReturnType, ParcelUpdate, PermissionField, PickUpdate, PrimShape,
        ProductType, ProfileUpdate, RegionInfoUpdate, Reliability, SaleType, ScriptPermissions,
        Session, SoundFlags, TerrainLayerType, Throttle, TransferStatus, Transmit, WearableType,
        avatar_texture, group_powers, pcode,
    };
    use sl_types::lsl::{Rotation, Vector};
    use sl_wire::messages::{
        AgentDataUpdate, AgentDataUpdateAgentDataBlock, AgentGroupDataUpdate,
        AgentGroupDataUpdateAgentDataBlock, AgentGroupDataUpdateGroupDataBlock,
        AgentMovementComplete, AgentMovementCompleteAgentDataBlock, AgentMovementCompleteDataBlock,
        AgentMovementCompleteSimDataBlock, AgentWearablesUpdate,
        AgentWearablesUpdateAgentDataBlock, AgentWearablesUpdateWearableDataBlock,
        AssetUploadComplete, AssetUploadCompleteAssetBlockBlock, AttachedSound,
        AttachedSoundDataBlockBlock, AvatarAnimation, AvatarAnimationAnimationListBlock,
        AvatarAnimationAnimationSourceListBlock, AvatarAnimationSenderBlock, AvatarAppearance,
        AvatarAppearanceObjectDataBlock, AvatarAppearanceSenderBlock,
        AvatarAppearanceVisualParamBlock, AvatarClassifiedReply,
        AvatarClassifiedReplyAgentDataBlock, AvatarClassifiedReplyDataBlock, AvatarNotesReply,
        AvatarNotesReplyAgentDataBlock, AvatarNotesReplyDataBlock, AvatarPicksReply,
        AvatarPicksReplyAgentDataBlock, AvatarPicksReplyDataBlock, AvatarPropertiesReply,
        AvatarPropertiesReplyAgentDataBlock, AvatarPropertiesReplyPropertiesDataBlock,
        AvatarSitResponse, AvatarSitResponseSitObjectBlock, AvatarSitResponseSitTransformBlock,
        BulkUpdateInventory, BulkUpdateInventoryAgentDataBlock, BulkUpdateInventoryFolderDataBlock,
        BulkUpdateInventoryItemDataBlock, ChangeUserRights, ChangeUserRightsAgentDataBlock,
        ChangeUserRightsRightsBlock, ChatFromSimulator, ChatFromSimulatorChatDataBlock,
        ClassifiedInfoReply, ClassifiedInfoReplyAgentDataBlock, ClassifiedInfoReplyDataBlock,
        ConfirmXferPacket, ConfirmXferPacketXferIDBlock, CrossedRegion,
        CrossedRegionAgentDataBlock, CrossedRegionInfoBlock, CrossedRegionRegionDataBlock,
        DisableSimulator, EconomyData, EconomyDataInfoBlock, EjectGroupMemberReply,
        EjectGroupMemberReplyAgentDataBlock, EjectGroupMemberReplyEjectDataBlock,
        EjectGroupMemberReplyGroupDataBlock, EstateOwnerMessage, EstateOwnerMessageAgentDataBlock,
        EstateOwnerMessageMethodDataBlock, EstateOwnerMessageParamListBlock, GenericMessage,
        GenericMessageAgentDataBlock, GenericMessageMethodDataBlock, GenericStreamingMessage,
        GenericStreamingMessageDataBlockBlock, GenericStreamingMessageMethodDataBlock,
        GroupMembersReply, GroupMembersReplyAgentDataBlock, GroupMembersReplyGroupDataBlock,
        GroupMembersReplyMemberDataBlock, GroupProfileReply, GroupProfileReplyAgentDataBlock,
        GroupProfileReplyGroupDataBlock, ImageData, ImageDataImageDataBlock, ImageDataImageIDBlock,
        ImageNotInDatabase, ImageNotInDatabaseImageIDBlock, ImagePacket, ImagePacketImageDataBlock,
        ImagePacketImageIDBlock, ImprovedInstantMessage, ImprovedInstantMessageAgentDataBlock,
        ImprovedInstantMessageEstateBlockBlock, ImprovedInstantMessageMessageBlockBlock,
        ImprovedTerseObjectUpdate, ImprovedTerseObjectUpdateObjectDataBlock,
        ImprovedTerseObjectUpdateRegionDataBlock, InventoryDescendents,
        InventoryDescendentsAgentDataBlock, InventoryDescendentsFolderDataBlock,
        InventoryDescendentsItemDataBlock, KillObject, KillObjectObjectDataBlock, LayerData,
        LayerDataLayerDataBlock, LayerDataLayerIDBlock, LogoutRequest, LogoutRequestAgentDataBlock,
        MapBlockReply, MapBlockReplyAgentDataBlock, MapBlockReplyDataBlock, MapBlockReplySizeBlock,
        MapItemReply, MapItemReplyAgentDataBlock, MapItemReplyDataBlock,
        MapItemReplyRequestDataBlock, MoneyBalanceReply, MoneyBalanceReplyMoneyDataBlock,
        MoneyBalanceReplyTransactionInfoBlock, MuteListUpdate, MuteListUpdateMuteDataBlock,
        ObjectProperties as WireObjectProperties, ObjectPropertiesObjectDataBlock, ObjectUpdate,
        ObjectUpdateCached, ObjectUpdateCachedObjectDataBlock, ObjectUpdateCachedRegionDataBlock,
        ObjectUpdateCompressed, ObjectUpdateCompressedObjectDataBlock,
        ObjectUpdateCompressedRegionDataBlock, ObjectUpdateObjectDataBlock,
        ObjectUpdateRegionDataBlock, OfflineNotification, OfflineNotificationAgentBlockBlock,
        OnlineNotification, OnlineNotificationAgentBlockBlock, ParcelAccessListReply,
        ParcelAccessListReplyDataBlock, ParcelAccessListReplyListBlock, ParcelDwellReply,
        ParcelDwellReplyAgentDataBlock, ParcelDwellReplyDataBlock, ParcelMediaCommandMessage,
        ParcelMediaCommandMessageCommandBlockBlock, ParcelMediaUpdate,
        ParcelMediaUpdateDataBlockBlock, ParcelMediaUpdateDataBlockExtendedBlock, ParcelProperties,
        ParcelPropertiesAgeVerificationBlockBlock, ParcelPropertiesParcelDataBlock,
        ParcelPropertiesParcelEnvironmentBlockBlock, ParcelPropertiesRegionAllowAccessBlockBlock,
        PickInfoReply, PickInfoReplyAgentDataBlock, PickInfoReplyDataBlock, PreloadSound,
        PreloadSoundDataBlockBlock, RegionHandshake, RegionHandshakeRegionInfo2Block,
        RegionHandshakeRegionInfo3Block, RegionHandshakeRegionInfoBlock, RegionInfo,
        RegionInfoAgentDataBlock, RegionInfoRegionInfo2Block, RegionInfoRegionInfoBlock,
        RequestXfer, RequestXferXferIDBlock, ScriptDialog, ScriptDialogButtonsBlock,
        ScriptDialogDataBlock, ScriptDialogOwnerDataBlock, ScriptQuestion, ScriptQuestionDataBlock,
        ScriptQuestionExperienceBlock, SendXferPacket, SendXferPacketDataPacketBlock,
        SendXferPacketXferIDBlock, SoundTrigger, SoundTriggerSoundDataBlock, TeleportFailed,
        TeleportFailedInfoBlock, TransferInfo, TransferInfoTransferInfoBlock, TransferPacket,
        TransferPacketTransferDataBlock, UpdateCreateInventoryItem,
        UpdateCreateInventoryItemAgentDataBlock, UpdateCreateInventoryItemInventoryDataBlock,
        UseCachedMuteList, UseCachedMuteListAgentDataBlock,
    };
    use sl_wire::{
        AnyMessage, LoginFailure, LoginRequest, LoginResponse, LoginSuccess, MessageId,
        PacketFlags, Reader, SkeletonFolder, Writer, encode_datagram, parse_datagram,
        parse_llsd_xml,
    };

    /// A boxed test error.
    type TestError = Box<dyn Error>;

    /// Decodes a NUL-terminated wire string field for assertions.
    fn trimmed(bytes: &[u8]) -> String {
        String::from_utf8_lossy(bytes)
            .trim_end_matches('\0')
            .to_owned()
    }

    /// The simulator address used throughout these tests.
    fn sim_addr() -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 9000)
    }

    /// Adds milliseconds to an instant.
    fn after(now: Instant, millis: u64) -> Result<Instant, TestError> {
        now.checked_add(Duration::from_millis(millis))
            .ok_or_else(|| "instant out of range".into())
    }

    /// Builds a `Session` with throwaway login parameters.
    fn new_session() -> Session {
        Session::new(LoginParams {
            login_uri: "http://127.0.0.1:9000/".to_owned(),
            request: LoginRequest::new("Test", "User", "secret", "last", "MyViewer", "1.2.3"),
        })
    }

    /// A successful login response pointing at the test simulator.
    fn success() -> LoginResponse {
        LoginResponse::Success(Box::new(LoginSuccess {
            agent_id: uuid::Uuid::from_u128(1),
            session_id: uuid::Uuid::from_u128(2),
            secure_session_id: uuid::Uuid::from_u128(3),
            circuit_code: 0x0011_2233,
            sim_ip: Ipv4Addr::new(127, 0, 0, 1),
            sim_port: 9000,
            seed_capability: "http://127.0.0.1:9000/seed".to_owned(),
            message: None,
            mfa_hash: None,
            inventory_root: None,
            inventory_skeleton: Vec::new(),
            buddy_list: Vec::new(),
        }))
    }

    /// Decodes the message carried by a transmitted datagram.
    fn decode(transmit: &Transmit) -> Result<AnyMessage, TestError> {
        let parsed = parse_datagram(&transmit.payload)?;
        let mut reader = Reader::new(parsed.body);
        let id = MessageId::decode(&mut reader)?;
        Ok(AnyMessage::decode(id, &mut reader)?)
    }

    /// Drains and decodes all currently queued transmissions.
    fn drain(session: &mut Session) -> Result<Vec<AnyMessage>, TestError> {
        let mut out = Vec::new();
        while let Some(transmit) = session.poll_transmit() {
            out.push(decode(&transmit)?);
        }
        Ok(out)
    }

    /// Drains all queued events.
    fn drain_events(session: &mut Session) -> Vec<Event> {
        let mut out = Vec::new();
        while let Some(event) = session.poll_event() {
            out.push(event);
        }
        out
    }

    /// Builds an inbound datagram for a server-sent message.
    fn server_datagram(id: MessageId, body: &[u8], sequence: u32, reliable: bool) -> Vec<u8> {
        let mut writer = Writer::new();
        id.encode(&mut writer);
        writer.bytes(body);
        let flags = if reliable {
            PacketFlags::RELIABLE
        } else {
            PacketFlags::EMPTY
        };
        encode_datagram(flags, sequence, &writer.into_bytes())
    }

    /// Builds an inbound datagram carrying a fully encoded server message.
    fn server_message(
        message: &AnyMessage,
        sequence: u32,
        reliable: bool,
    ) -> Result<Vec<u8>, TestError> {
        let mut writer = Writer::new();
        message.id().encode(&mut writer);
        message.encode_body(&mut writer)?;
        let flags = if reliable {
            PacketFlags::RELIABLE
        } else {
            PacketFlags::EMPTY
        };
        Ok(encode_datagram(flags, sequence, &writer.into_bytes()))
    }

    /// Drives a session from login through the region handshake into the active
    /// state, returning the active session.
    fn established(now: Instant) -> Result<Session, TestError> {
        let mut session = new_session();
        assert!(session.login_http_request().is_some());
        session.handle_login_response(success(), now)?;

        let sent = drain(&mut session)?;
        assert!(matches!(sent.first(), Some(AnyMessage::UseCircuitCode(_))));
        assert!(matches!(
            sent.get(1),
            Some(AnyMessage::CompleteAgentMovement(_))
        ));
        assert_eq!(
            drain_events(&mut session),
            vec![Event::CircuitEstablished { sim: sim_addr() }]
        );

        // RegionHandshake (all-zero body decodes to zeroed fields/empty blocks).
        let handshake = server_datagram(MessageId::Low(148), &[0u8; 600], 1, true);
        session.handle_datagram(sim_addr(), &handshake, now)?;
        let replies = drain(&mut session)?;
        assert!(matches!(
            replies.first(),
            Some(AnyMessage::RegionHandshakeReply(_))
        ));
        let events = drain_events(&mut session);
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::RegionInfoHandshake(_))),
            "expected a RegionInfoHandshake, got {events:?}"
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::RegionHandshakeComplete)),
            "expected a RegionHandshakeComplete, got {events:?}"
        );
        Ok(session)
    }

    #[test]
    fn login_failure_disconnects() -> Result<(), TestError> {
        let mut session = new_session();
        let failure = LoginResponse::Failure(LoginFailure {
            reason: "key".to_owned(),
            message: "bad password".to_owned(),
        });
        session.handle_login_response(failure, Instant::now())?;
        assert!(session.is_closed());
        assert!(matches!(
            drain_events(&mut session).first(),
            Some(Event::Disconnected(DisconnectReason::LoginFailed { .. }))
        ));
        Ok(())
    }

    #[test]
    fn login_brings_up_circuit_and_handshake() -> Result<(), TestError> {
        let session = established(Instant::now())?;
        assert!(!session.is_closed());
        Ok(())
    }

    #[test]
    fn responds_to_ping_with_completed_ping() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        // StartPingCheck High 1: PingID (u8) + OldestUnacked (u32).
        let ping = server_datagram(MessageId::High(1), &[0x2A, 0, 0, 0, 0], 2, false);
        session.handle_datagram(sim_addr(), &ping, now)?;
        let replies = drain(&mut session)?;
        let Some(AnyMessage::CompletePingCheck(reply)) = replies.first() else {
            return Err(format!("expected CompletePingCheck, got {replies:?}").into());
        };
        assert_eq!(reply.ping_id.ping_id, 0x2A);
        Ok(())
    }

    #[test]
    fn sends_agent_update_on_cadence() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // At t0 nothing is yet due.
        session.handle_timeout(now);
        assert!(drain(&mut session)?.is_empty());

        // Past the 1s cadence, an AgentUpdate is sent (alongside any ack flush).
        session.handle_timeout(after(now, 1100)?);
        let sent = drain(&mut session)?;
        assert!(
            sent.iter().any(|m| matches!(m, AnyMessage::AgentUpdate(_))),
            "expected an AgentUpdate, got {sent:?}"
        );
        Ok(())
    }

    #[test]
    fn retransmits_unacknowledged_reliable_packets() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = new_session();
        session.handle_login_response(success(), now)?;
        let _initial = drain(&mut session)?; // UseCircuitCode + CompleteAgentMovement

        // Without any ack, the resend timer eventually fires and retransmits.
        let resend_at = session.poll_timeout().ok_or("resend scheduled")?;
        session.handle_timeout(resend_at);
        let resent = drain(&mut session)?;
        assert!(
            resent
                .iter()
                .any(|m| matches!(m, AnyMessage::UseCircuitCode(_))),
            "expected a retransmitted UseCircuitCode, got {resent:?}"
        );
        Ok(())
    }

    #[test]
    fn flushes_owed_acknowledgements() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // A reliable inbound message must be acknowledged.
        let ping = server_datagram(MessageId::High(1), &[1, 0, 0, 0, 0], 50, true);
        session.handle_datagram(sim_addr(), &ping, now)?;
        drain(&mut session)?; // the ping reply

        let flush_at = session.poll_timeout().ok_or("ack flush scheduled")?;
        session.handle_timeout(flush_at);
        let sent = drain(&mut session)?;
        let ack = sent.iter().find_map(|m| match m {
            AnyMessage::PacketAck(ack) => Some(ack),
            _ => None,
        });
        let ack = ack.ok_or("a PacketAck was sent")?;
        assert!(ack.packets.iter().any(|p| p.id == 50));
        Ok(())
    }

    #[test]
    fn inactivity_times_out() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        session.handle_timeout(after(now, 60_000)?);
        assert!(session.is_closed());
        assert!(matches!(
            drain_events(&mut session).last(),
            Some(Event::Disconnected(DisconnectReason::Timeout))
        ));
        Ok(())
    }

    #[test]
    fn clean_logout_round_trip() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        session.initiate_logout(now);
        let sent = drain(&mut session)?;
        assert!(
            sent.iter()
                .any(|m| matches!(m, AnyMessage::LogoutRequest(_))),
            "expected a LogoutRequest, got {sent:?}"
        );

        // LogoutReply Low 253: AgentData (2 uuids) + InventoryData variable (count).
        let reply = server_datagram(MessageId::Low(253), &[0u8; 33], 2, true);
        session.handle_datagram(sim_addr(), &reply, now)?;
        assert!(session.is_closed());
        assert!(matches!(
            drain_events(&mut session).last(),
            Some(Event::LoggedOut)
        ));
        Ok(())
    }

    #[test]
    fn enqueue_sends_application_messages() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let message = AnyMessage::LogoutRequest(LogoutRequest {
            agent_data: LogoutRequestAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(1),
                session_id: uuid::Uuid::from_u128(2),
            },
        });
        session.enqueue(message, Reliability::Reliable, now)?;
        let sent = drain(&mut session)?;
        assert!(
            sent.iter()
                .any(|m| matches!(m, AnyMessage::LogoutRequest(_)))
        );
        Ok(())
    }

    #[test]
    fn say_sends_chat_from_viewer() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        session.say("hi there", ChatType::Shout, 0, now)?;
        let sent = drain(&mut session)?;
        let chat = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::ChatFromViewer(chat) => Some(chat),
                _ => None,
            })
            .ok_or("expected a ChatFromViewer")?;
        // The wire string carries a trailing NUL, as a real viewer sends.
        assert_eq!(chat.chat_data.message, b"hi there\0");
        assert_eq!(chat.chat_data.r#type, 2); // shout
        assert_eq!(chat.chat_data.channel, 0);
        Ok(())
    }

    #[test]
    fn chat_from_simulator_surfaces_event() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let chat = AnyMessage::ChatFromSimulator(ChatFromSimulator {
            chat_data: ChatFromSimulatorChatDataBlock {
                from_name: b"Resident Tester\0".to_vec(),
                source_id: uuid::Uuid::from_u128(0x42),
                owner_id: uuid::Uuid::from_u128(0x43),
                source_type: 1, // agent
                chat_type: 1,   // normal
                audible: 1,     // fully
                position: vec3(10.0, 20.0, 30.0),
                message: b"hello world\0".to_vec(),
            },
        });
        let datagram = server_message(&chat, 7, false)?;
        session.handle_datagram(sim_addr(), &datagram, now)?;

        let received = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::ChatReceived(chat) => Some(chat),
                _ => None,
            })
            .ok_or("expected a ChatReceived event")?;
        // Trailing NUL padding is stripped from both strings.
        assert_eq!(received.from_name, "Resident Tester");
        assert_eq!(received.message, "hello world");
        assert_eq!(received.source_id, uuid::Uuid::from_u128(0x42));
        assert_eq!(received.owner_id, uuid::Uuid::from_u128(0x43));
        assert_eq!(received.source_type, ChatSourceType::Agent);
        assert_eq!(received.chat_type, ChatType::Normal);
        assert_eq!(received.audible, ChatAudible::Fully);
        assert_eq!(received.position, (10.0, 20.0, 30.0));
        Ok(())
    }

    #[test]
    fn set_typing_sends_typing_chat() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        session.set_typing(true, now)?;
        let sent = drain(&mut session)?;
        let chat = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::ChatFromViewer(chat) => Some(chat),
                _ => None,
            })
            .ok_or("expected a ChatFromViewer")?;
        assert_eq!(chat.chat_data.r#type, 4); // StartTyping
        // Typing carries no text, just the wire NUL terminator.
        assert_eq!(chat.chat_data.message, b"\0");
        Ok(())
    }

    #[test]
    fn typing_chat_from_simulator_surfaces_typing_event() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let chat = AnyMessage::ChatFromSimulator(ChatFromSimulator {
            chat_data: ChatFromSimulatorChatDataBlock {
                from_name: b"Resident Tester\0".to_vec(),
                source_id: uuid::Uuid::from_u128(0x42),
                owner_id: uuid::Uuid::nil(),
                source_type: 1, // agent
                chat_type: 4,   // StartTyping
                audible: 1,     // fully
                position: vec3(1.0, 2.0, 3.0),
                message: b"\0".to_vec(),
            },
        });
        let datagram = server_message(&chat, 8, false)?;
        session.handle_datagram(sim_addr(), &datagram, now)?;

        let events = drain_events(&mut session);
        // A typing message surfaces as ChatTyping, never as ChatReceived.
        assert!(
            !events.iter().any(|e| matches!(e, Event::ChatReceived(_))),
            "typing must not surface as ChatReceived, got {events:?}"
        );
        let typing = events
            .into_iter()
            .find_map(|event| match event {
                Event::ChatTyping {
                    from_name,
                    source_id,
                    typing,
                } => Some((from_name, source_id, typing)),
                _ => None,
            })
            .ok_or("expected a ChatTyping event")?;
        assert_eq!(typing.0, "Resident Tester");
        assert_eq!(typing.1, uuid::Uuid::from_u128(0x42));
        assert!(typing.2, "StartTyping should set typing = true");
        Ok(())
    }

    #[test]
    fn send_instant_message_packs_improved_instant_message() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let target = uuid::Uuid::from_u128(0x99);
        session.send_instant_message(target, "hi there", now)?;
        let sent = drain(&mut session)?;
        let im = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::ImprovedInstantMessage(im) => Some(im),
                _ => None,
            })
            .ok_or("expected an ImprovedInstantMessage")?;
        let block = &im.message_block;
        assert_eq!(block.to_agent_id, target);
        assert_eq!(block.dialog, 0); // IM_NOTHING_SPECIAL
        assert!(!block.from_group);
        // Wire strings carry a trailing NUL; the agent's login name is sent.
        assert_eq!(block.message, b"hi there\0");
        assert_eq!(block.from_agent_name, b"Test User\0");
        // The 1:1 session id is the XOR of the two agent ids (agent id is 1).
        assert_eq!(block.id, uuid::Uuid::from_u128(1u128 ^ 0x99u128));
        // AgentData.SessionID is the login session id (2), not the IM session id.
        assert_eq!(im.agent_data.session_id, uuid::Uuid::from_u128(2));
        Ok(())
    }

    #[test]
    fn send_im_typing_packs_typing_dialog() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        session.send_im_typing(uuid::Uuid::from_u128(0x99), true, now)?;
        let sent = drain(&mut session)?;
        let im = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::ImprovedInstantMessage(im) => Some(im),
                _ => None,
            })
            .ok_or("expected an ImprovedInstantMessage")?;
        assert_eq!(im.message_block.dialog, 41); // IM_TYPING_START
        assert_eq!(im.message_block.message, b"typing\0");
        Ok(())
    }

    /// Builds an inbound `ImprovedInstantMessage` from a sender with the given
    /// dialog, name, and message.
    fn inbound_im(dialog: u8, from_name: &[u8], message: &[u8]) -> AnyMessage {
        AnyMessage::ImprovedInstantMessage(ImprovedInstantMessage {
            agent_data: ImprovedInstantMessageAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(0x55),
                session_id: uuid::Uuid::nil(),
            },
            message_block: ImprovedInstantMessageMessageBlockBlock {
                from_group: false,
                to_agent_id: uuid::Uuid::from_u128(1),
                parent_estate_id: 1,
                region_id: uuid::Uuid::from_u128(0x7),
                position: vec3(1.0, 2.0, 3.0),
                offline: 0,
                dialog,
                id: uuid::Uuid::from_u128(0xABC),
                timestamp: 0,
                from_agent_name: from_name.to_vec(),
                message: message.to_vec(),
                binary_bucket: Vec::new(),
            },
            estate_block: ImprovedInstantMessageEstateBlockBlock { estate_id: 1 },
            meta_data: Vec::new(),
        })
    }

    #[test]
    fn improved_instant_message_surfaces_event() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let im = inbound_im(0, b"Friendly Bot\0", b"hi there\0");
        let datagram = server_message(&im, 9, false)?;
        session.handle_datagram(sim_addr(), &datagram, now)?;

        let received = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::InstantMessageReceived(im) => Some(im),
                _ => None,
            })
            .ok_or("expected an InstantMessageReceived event")?;
        assert_eq!(received.from_agent_id, uuid::Uuid::from_u128(0x55));
        assert_eq!(received.from_agent_name, "Friendly Bot");
        assert_eq!(received.message, "hi there");
        assert_eq!(received.dialog, ImDialog::Message);
        assert!(!received.offline);
        Ok(())
    }

    #[test]
    fn improved_instant_message_typing_surfaces_im_typing() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // Dialog 41 = IM_TYPING_START; the viewer sends "typing" as the text.
        let im = inbound_im(41, b"Friendly Bot\0", b"typing\0");
        let datagram = server_message(&im, 9, false)?;
        session.handle_datagram(sim_addr(), &datagram, now)?;

        let events = drain_events(&mut session);
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Event::InstantMessageReceived(_))),
            "IM typing must not surface as InstantMessageReceived, got {events:?}"
        );
        let typing = events
            .into_iter()
            .find_map(|event| match event {
                Event::ImTyping {
                    from_agent_id,
                    typing,
                    ..
                } => Some((from_agent_id, typing)),
                _ => None,
            })
            .ok_or("expected an ImTyping event")?;
        assert_eq!(typing.0, uuid::Uuid::from_u128(0x55));
        assert!(typing.1, "IM_TYPING_START should set typing = true");
        Ok(())
    }

    /// Finds the control flags of the first `AgentUpdate` in a batch.
    fn agent_update_controls(messages: &[AnyMessage]) -> Option<u32> {
        messages.iter().find_map(|m| match m {
            AnyMessage::AgentUpdate(update) => Some(update.agent_data.control_flags),
            _ => None,
        })
    }

    #[test]
    fn set_controls_sends_and_persists_agent_update() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // Walking forward, flying: an immediate AgentUpdate carries the flags.
        session.set_controls(ControlFlags::AT_POS | ControlFlags::FLY, now)?;
        let sent = drain(&mut session)?;
        assert_eq!(
            agent_update_controls(&sent),
            Some((ControlFlags::AT_POS | ControlFlags::FLY).bits())
        );

        // The flags persist: the next keep-alive AgentUpdate still carries them.
        session.handle_timeout(after(now, 1100)?);
        let keepalive = drain(&mut session)?;
        assert_eq!(
            agent_update_controls(&keepalive),
            Some((ControlFlags::AT_POS | ControlFlags::FLY).bits())
        );
        Ok(())
    }

    /// Finds the agent-data block of the first `AgentUpdate` in a batch.
    fn first_agent_update(
        messages: &[AnyMessage],
    ) -> Option<&sl_wire::messages::AgentUpdateAgentDataBlock> {
        messages.iter().find_map(|m| match m {
            AnyMessage::AgentUpdate(update) => Some(&update.agent_data),
            _ => None,
        })
    }

    #[test]
    fn set_camera_sends_and_persists_agent_update() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // A viewpoint up high looking along +X.
        let eye = Vector {
            x: 64.0,
            y: 96.0,
            z: 50.0,
        };
        let camera = Camera::looking_at(
            eye.clone(),
            Vector {
                x: 65.0,
                y: 96.0,
                z: 50.0,
            },
        );
        session.set_camera(camera.clone(), now)?;
        assert_eq!(session.camera(), &camera);

        // The immediate AgentUpdate carries the camera position and axes.
        let sent = drain(&mut session)?;
        let block = first_agent_update(&sent).ok_or("expected an AgentUpdate")?;
        assert_eq!(block.camera_center, eye);
        assert_eq!(block.camera_at_axis, camera.at_axis);
        assert_eq!(block.camera_left_axis, camera.left_axis);
        assert_eq!(block.camera_up_axis, camera.up_axis);
        // Looking along +X yields the world-up basis (matching the legacy default
        // axes, but at the caller's position rather than the region centre).
        assert_eq!(
            camera.at_axis,
            Vector {
                x: 1.0,
                y: 0.0,
                z: 0.0
            }
        );
        assert_eq!(
            camera.left_axis,
            Vector {
                x: 0.0,
                y: 1.0,
                z: 0.0
            }
        );

        // It persists: the next keep-alive AgentUpdate still carries the camera.
        session.handle_timeout(after(now, 1100)?);
        let keepalive = drain(&mut session)?;
        let block = first_agent_update(&keepalive).ok_or("expected an AgentUpdate")?;
        assert_eq!(block.camera_center, eye);
        assert_eq!(block.camera_at_axis, camera.at_axis);
        Ok(())
    }

    #[test]
    fn stand_is_a_one_shot_control() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        session.set_controls(ControlFlags::AT_POS, now)?;
        drain(&mut session)?;

        // stand() adds STAND_UP to the immediate update without persisting it.
        session.stand(now)?;
        let sent = drain(&mut session)?;
        let controls = ControlFlags::from_bits(agent_update_controls(&sent).ok_or("AgentUpdate")?);
        assert!(controls.contains(ControlFlags::STAND_UP));
        assert!(controls.contains(ControlFlags::AT_POS));

        // The next keep-alive no longer carries STAND_UP.
        session.handle_timeout(after(now, 1100)?);
        let keepalive = drain(&mut session)?;
        let controls =
            ControlFlags::from_bits(agent_update_controls(&keepalive).ok_or("AgentUpdate")?);
        assert!(!controls.contains(ControlFlags::STAND_UP));
        assert!(controls.contains(ControlFlags::AT_POS));
        Ok(())
    }

    #[test]
    fn autopilot_sends_generic_message() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        session.autopilot_to(256_010.0, 256_020.0, 25.0, now)?;
        let sent = drain(&mut session)?;
        let generic = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::GenericMessage(message) => Some(message),
                _ => None,
            })
            .ok_or("expected a GenericMessage")?;
        assert_eq!(generic.method_data.method, b"autopilot");
        let params: Vec<&[u8]> = generic
            .param_list
            .iter()
            .map(|p| p.parameter.as_slice())
            .collect();
        assert_eq!(
            params,
            vec![&b"256010"[..], &b"256020"[..], &b"25"[..]],
            "autopilot params are the global x, global y, and z as strings"
        );
        Ok(())
    }

    /// Encodes a [`Throttle`] as its 28-byte `AgentThrottle` payload (seven
    /// little-endian `f32` bits-per-second values), for asserting the on-wire
    /// bytes without comparing floats directly.
    fn throttle_payload(throttle: &Throttle) -> Vec<u8> {
        let mut writer = Writer::new();
        for rate in throttle.bits_per_second() {
            writer.put_f32(rate);
        }
        writer.into_bytes()
    }

    #[test]
    fn set_throttle_sends_agent_throttle() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        session.set_throttle(Throttle::preset_500(), now)?;
        let sent = drain(&mut session)?;
        let throttle = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::AgentThrottle(message) => Some(message),
                _ => None,
            })
            .ok_or("expected an AgentThrottle")?;

        // The block carries the agent identity and circuit code.
        assert_eq!(throttle.agent_data.agent_id, uuid::Uuid::from_u128(1));
        assert_eq!(throttle.agent_data.session_id, uuid::Uuid::from_u128(2));
        assert_eq!(throttle.agent_data.circuit_code, 0x0011_2233);
        assert_eq!(throttle.throttle.gen_counter, 0);

        // The payload is the seven preset rates packed as little-endian f32
        // bits-per-second (7 * 4 = 28 bytes).
        assert_eq!(throttle.throttle.throttles.len(), 28);
        assert_eq!(
            throttle.throttle.throttles,
            throttle_payload(&Throttle::preset_500())
        );
        Ok(())
    }

    #[test]
    fn throttle_resent_on_region_change() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        // Advertise a throttle on the root region, then discard its transmits.
        session.set_throttle(Throttle::preset_300(), now)?;
        drain(&mut session)?;

        // Pre-open the neighbour and cross into it.
        enable_neighbour_b(&mut session, 9, now)?;
        while session.poll_transmit().is_some() {}

        let handle = 0x0003_E900_0003_E800;
        let crossed = AnyMessage::CrossedRegion(CrossedRegion {
            agent_data: CrossedRegionAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(1),
                session_id: uuid::Uuid::from_u128(2),
            },
            region_data: CrossedRegionRegionDataBlock {
                sim_ip: [127, 0, 0, 1],
                sim_port: 9001u16.swap_bytes(),
                region_handle: handle,
                seed_capability: b"http://x/seedB\0".to_vec(),
            },
            info: CrossedRegionInfoBlock {
                position: vec3(10.0, 128.0, 30.0),
                look_at: vec3(1.0, 0.0, 0.0),
            },
        });
        session.handle_datagram(sim_addr(), &server_message(&crossed, 10, true)?, now)?;
        // Drop the CompleteAgentMovement and any other crossing transmits.
        while session.poll_transmit().is_some() {}

        // The destination confirms arrival, which completes the crossing.
        let amc = AnyMessage::AgentMovementComplete(AgentMovementComplete {
            agent_data: AgentMovementCompleteAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(1),
                session_id: uuid::Uuid::from_u128(2),
            },
            data: AgentMovementCompleteDataBlock {
                position: vec3(10.0, 128.0, 30.0),
                look_at: vec3(1.0, 0.0, 0.0),
                region_handle: handle,
                timestamp: 0,
            },
            sim_data: AgentMovementCompleteSimDataBlock {
                channel_version: b"x\0".to_vec(),
            },
        });
        session.handle_datagram(sim_b(), &server_message(&amc, 1, true)?, now)?;

        // The throttle is re-advertised to the new root region (sim_b).
        let mut to_b = Vec::new();
        while let Some(transmit) = session.poll_transmit() {
            if transmit.destination == sim_b() {
                to_b.push(decode(&transmit)?);
            }
        }
        let throttle = to_b
            .iter()
            .find_map(|m| match m {
                AnyMessage::AgentThrottle(message) => Some(message),
                _ => None,
            })
            .ok_or("expected an AgentThrottle to the new root region")?;
        assert_eq!(
            throttle.throttle.throttles,
            throttle_payload(&Throttle::preset_300())
        );
        Ok(())
    }

    /// Builds an inbound `AvatarSitResponse` for the object `sit_object`.
    fn sit_response(sit_object: uuid::Uuid) -> AnyMessage {
        let zero = vec3(0.0, 0.0, 0.0);
        AnyMessage::AvatarSitResponse(AvatarSitResponse {
            sit_object: AvatarSitResponseSitObjectBlock { id: sit_object },
            sit_transform: AvatarSitResponseSitTransformBlock {
                auto_pilot: false,
                sit_position: vec3(0.0, 0.0, 0.5),
                sit_rotation: Rotation {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    s: 1.0,
                },
                camera_eye_offset: zero.clone(),
                camera_at_offset: zero,
                force_mouselook: false,
            },
        })
    }

    #[test]
    fn sit_request_completes_on_response() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let target = uuid::Uuid::from_u128(0x5117);
        session.sit_on(target, vec3(0.0, 0.0, 0.0), now)?;
        let sent = drain(&mut session)?;
        let request = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::AgentRequestSit(request) => Some(request),
                _ => None,
            })
            .ok_or("expected an AgentRequestSit")?;
        assert_eq!(request.target_object.target_id, target);

        // The simulator's response completes the sit with an AgentSit and a
        // SitResult event.
        let response = server_message(&sit_response(target), 9, false)?;
        session.handle_datagram(sim_addr(), &response, now)?;
        let after_response = drain(&mut session)?;
        assert!(
            after_response
                .iter()
                .any(|m| matches!(m, AnyMessage::AgentSit(_))),
            "expected an AgentSit, got {after_response:?}"
        );
        let result = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::SitResult { sit_object, .. } => Some(sit_object),
                _ => None,
            })
            .ok_or("expected a SitResult event")?;
        assert_eq!(result, target);
        Ok(())
    }

    #[test]
    fn unrequested_sit_response_is_ignored() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // An AvatarSitResponse with no outstanding sit request is a no-op.
        let response = server_message(&sit_response(uuid::Uuid::from_u128(0x1)), 9, false)?;
        session.handle_datagram(sim_addr(), &response, now)?;
        assert!(
            drain(&mut session)?.is_empty(),
            "no AgentSit should be sent"
        );
        assert!(
            !drain_events(&mut session)
                .iter()
                .any(|e| matches!(e, Event::SitResult { .. })),
            "no SitResult should be emitted"
        );
        Ok(())
    }

    #[test]
    fn request_avatar_properties_packs_request() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let target = uuid::Uuid::from_u128(0xA1);
        session.request_avatar_properties(target, now)?;
        let sent = drain(&mut session)?;
        let request = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::AvatarPropertiesRequest(request) => Some(request),
                _ => None,
            })
            .ok_or("expected an AvatarPropertiesRequest")?;
        assert_eq!(request.agent_data.avatar_id, target);
        Ok(())
    }

    #[test]
    fn avatar_properties_reply_surfaces_event() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let target = uuid::Uuid::from_u128(0xA1);
        let reply = AnyMessage::AvatarPropertiesReply(AvatarPropertiesReply {
            agent_data: AvatarPropertiesReplyAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(1),
                avatar_id: target,
            },
            properties_data: AvatarPropertiesReplyPropertiesDataBlock {
                image_id: uuid::Uuid::from_u128(0xB1),
                fl_image_id: uuid::Uuid::from_u128(0xB2),
                partner_id: uuid::Uuid::from_u128(0xB3),
                about_text: b"a test avatar\0".to_vec(),
                fl_about_text: b"first life\0".to_vec(),
                born_on: b"2008-01-15\0".to_vec(),
                profile_url: b"\0".to_vec(),
                charter_member: b"\0".to_vec(),
                flags: 0x10,
            },
        });
        let datagram = server_message(&reply, 9, false)?;
        session.handle_datagram(sim_addr(), &datagram, now)?;

        let props = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::AvatarProperties(props) => Some(props),
                _ => None,
            })
            .ok_or("expected an AvatarProperties event")?;
        assert_eq!(props.avatar_id, target);
        assert_eq!(props.about_text, "a test avatar");
        assert_eq!(props.born_on, "2008-01-15");
        assert_eq!(props.partner_id, uuid::Uuid::from_u128(0xB3));
        assert_eq!(props.flags, 0x10);
        Ok(())
    }

    #[test]
    fn request_avatar_picks_packs_generic_message() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let target = uuid::Uuid::from_u128(0xA1);
        session.request_avatar_picks(target, now)?;
        let sent = drain(&mut session)?;
        let generic = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::GenericMessage(message) => Some(message),
                _ => None,
            })
            .ok_or("expected a GenericMessage")?;
        assert_eq!(generic.method_data.method, b"avatarpicksrequest");
        assert_eq!(
            generic.param_list.first().map(|p| p.parameter.as_slice()),
            Some(target.to_string().as_bytes())
        );
        Ok(())
    }

    #[test]
    fn avatar_picks_reply_surfaces_event() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let target = uuid::Uuid::from_u128(0xA1);
        let reply = AnyMessage::AvatarPicksReply(AvatarPicksReply {
            agent_data: AvatarPicksReplyAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(1),
                target_id: target,
            },
            data: vec![AvatarPicksReplyDataBlock {
                pick_id: uuid::Uuid::from_u128(0xC1),
                pick_name: b"My favourite spot\0".to_vec(),
            }],
        });
        let datagram = server_message(&reply, 9, false)?;
        session.handle_datagram(sim_addr(), &datagram, now)?;

        let picks = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::AvatarPicks { target_id, picks } if target_id == target => Some(picks),
                _ => None,
            })
            .ok_or("expected an AvatarPicks event")?;
        assert_eq!(picks.len(), 1);
        let pick = picks.first().ok_or("expected one pick")?;
        assert_eq!(pick.pick_id, uuid::Uuid::from_u128(0xC1));
        assert_eq!(pick.name, "My favourite spot");
        Ok(())
    }

    #[test]
    fn avatar_notes_reply_surfaces_event() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let target = uuid::Uuid::from_u128(0xA1);
        let reply = AnyMessage::AvatarNotesReply(AvatarNotesReply {
            agent_data: AvatarNotesReplyAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(1),
            },
            data: AvatarNotesReplyDataBlock {
                target_id: target,
                notes: b"met at the welcome area\0".to_vec(),
            },
        });
        let datagram = server_message(&reply, 9, false)?;
        session.handle_datagram(sim_addr(), &datagram, now)?;

        let notes = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::AvatarNotes { target_id, notes } if target_id == target => Some(notes),
                _ => None,
            })
            .ok_or("expected an AvatarNotes event")?;
        assert_eq!(notes, "met at the welcome area");
        Ok(())
    }

    #[test]
    fn request_avatar_classifieds_packs_generic_message() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let target = uuid::Uuid::from_u128(0xA1);
        session.request_avatar_classifieds(target, now)?;
        let sent = drain(&mut session)?;
        let generic = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::GenericMessage(message) => Some(message),
                _ => None,
            })
            .ok_or("expected a GenericMessage")?;
        assert_eq!(generic.method_data.method, b"avatarclassifiedsrequest");
        assert_eq!(
            generic.param_list.first().map(|p| p.parameter.as_slice()),
            Some(target.to_string().as_bytes())
        );
        Ok(())
    }

    #[test]
    fn avatar_classified_reply_surfaces_event() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let target = uuid::Uuid::from_u128(0xA1);
        let reply = AnyMessage::AvatarClassifiedReply(AvatarClassifiedReply {
            agent_data: AvatarClassifiedReplyAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(1),
                target_id: target,
            },
            data: vec![AvatarClassifiedReplyDataBlock {
                classified_id: uuid::Uuid::from_u128(0xD1),
                name: b"Land for rent\0".to_vec(),
            }],
        });
        let datagram = server_message(&reply, 9, false)?;
        session.handle_datagram(sim_addr(), &datagram, now)?;

        let classifieds = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::AvatarClassifieds {
                    target_id,
                    classifieds,
                } if target_id == target => Some(classifieds),
                _ => None,
            })
            .ok_or("expected an AvatarClassifieds event")?;
        assert_eq!(classifieds.len(), 1);
        let classified = classifieds.first().ok_or("expected one classified")?;
        assert_eq!(classified.classified_id, uuid::Uuid::from_u128(0xD1));
        assert_eq!(classified.name, "Land for rent");
        Ok(())
    }

    #[test]
    fn request_pick_info_packs_generic_message() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let creator = uuid::Uuid::from_u128(0xA1);
        let pick = uuid::Uuid::from_u128(0xC1);
        session.request_pick_info(creator, pick, now)?;
        let sent = drain(&mut session)?;
        let generic = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::GenericMessage(message) => Some(message),
                _ => None,
            })
            .ok_or("expected a GenericMessage")?;
        assert_eq!(generic.method_data.method, b"pickinforequest");
        // The viewer sends [creator_id, pick_id] in that order.
        assert_eq!(
            generic.param_list.first().map(|p| p.parameter.as_slice()),
            Some(creator.to_string().as_bytes())
        );
        assert_eq!(
            generic.param_list.get(1).map(|p| p.parameter.as_slice()),
            Some(pick.to_string().as_bytes())
        );
        Ok(())
    }

    #[test]
    fn pick_info_reply_surfaces_event() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let pick = uuid::Uuid::from_u128(0xC1);
        let reply = AnyMessage::PickInfoReply(PickInfoReply {
            agent_data: PickInfoReplyAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(1),
            },
            data: PickInfoReplyDataBlock {
                pick_id: pick,
                creator_id: uuid::Uuid::from_u128(0xA1),
                top_pick: false,
                parcel_id: uuid::Uuid::from_u128(0xB1),
                name: b"My favourite spot\0".to_vec(),
                desc: b"a lovely beach\0".to_vec(),
                snapshot_id: uuid::Uuid::from_u128(0xE1),
                user: b"Resident\0".to_vec(),
                original_name: b"Beach Parcel\0".to_vec(),
                sim_name: b"Sandbox\0".to_vec(),
                pos_global: [256_000.0, 256_128.0, 25.5],
                sort_order: 0,
                enabled: true,
            },
        });
        let datagram = server_message(&reply, 9, false)?;
        session.handle_datagram(sim_addr(), &datagram, now)?;

        let info = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::PickInfo(info) if info.pick_id == pick => Some(info),
                _ => None,
            })
            .ok_or("expected a PickInfo event")?;
        assert_eq!(info.name, "My favourite spot");
        assert_eq!(info.description, "a lovely beach");
        assert_eq!(info.sim_name, "Sandbox");
        let (px, py, pz) = info.pos_global;
        assert!((px - 256_000.0).abs() < f64::EPSILON);
        assert!((py - 256_128.0).abs() < f64::EPSILON);
        assert!((pz - 25.5).abs() < f64::EPSILON);
        assert!(info.enabled);
        Ok(())
    }

    #[test]
    fn classified_info_request_and_reply_round_trip() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let classified = uuid::Uuid::from_u128(0xD1);
        session.request_classified_info(classified, now)?;
        let sent = drain(&mut session)?;
        let request = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::ClassifiedInfoRequest(message) => Some(message),
                _ => None,
            })
            .ok_or("expected a ClassifiedInfoRequest")?;
        assert_eq!(request.agent_data.agent_id, uuid::Uuid::from_u128(1));
        assert_eq!(request.data.classified_id, classified);

        let reply = AnyMessage::ClassifiedInfoReply(ClassifiedInfoReply {
            agent_data: ClassifiedInfoReplyAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(1),
            },
            data: ClassifiedInfoReplyDataBlock {
                classified_id: classified,
                creator_id: uuid::Uuid::from_u128(0xA1),
                creation_date: 1_700_000_000,
                expiration_date: 1_710_000_000,
                category: 3,
                name: b"Land for rent\0".to_vec(),
                desc: b"prime waterfront\0".to_vec(),
                parcel_id: uuid::Uuid::from_u128(0xB1),
                parent_estate: 1,
                snapshot_id: uuid::Uuid::from_u128(0xE1),
                sim_name: b"Sandbox\0".to_vec(),
                pos_global: [256_000.0, 256_128.0, 25.5],
                parcel_name: b"Beach Parcel\0".to_vec(),
                classified_flags: 0x4,
                price_for_listing: 50,
            },
        });
        let datagram = server_message(&reply, 9, false)?;
        session.handle_datagram(sim_addr(), &datagram, now)?;

        let info = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::ClassifiedInfo(info) if info.classified_id == classified => Some(info),
                _ => None,
            })
            .ok_or("expected a ClassifiedInfo event")?;
        assert_eq!(info.name, "Land for rent");
        assert_eq!(info.description, "prime waterfront");
        assert_eq!(info.parcel_name, "Beach Parcel");
        assert_eq!(info.category, 3);
        assert_eq!(info.price_for_listing, 50);
        assert_eq!(info.classified_flags, 0x4);
        Ok(())
    }

    #[test]
    fn profile_and_interests_and_notes_updates_encode() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        session.update_profile(
            &ProfileUpdate {
                image_id: uuid::Uuid::from_u128(0x5E),
                about_text: "Hello world".to_owned(),
                allow_publish: true,
                profile_url: "https://example.com".to_owned(),
                ..ProfileUpdate::default()
            },
            now,
        )?;
        session.update_interests(
            &InterestsUpdate {
                want_to_mask: 0x7,
                want_to_text: "build, explore".to_owned(),
                skills_mask: 0x2,
                skills_text: "scripting".to_owned(),
                languages_text: "English".to_owned(),
            },
            now,
        )?;
        let target = uuid::Uuid::from_u128(0xA1);
        session.update_avatar_notes(target, "a good friend", now)?;
        let sent = drain(&mut session)?;

        let props = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::AvatarPropertiesUpdate(message) => Some(message),
                _ => None,
            })
            .ok_or("expected an AvatarPropertiesUpdate")?;
        assert_eq!(props.agent_data.agent_id, uuid::Uuid::from_u128(1));
        assert_eq!(props.properties_data.image_id, uuid::Uuid::from_u128(0x5E));
        assert_eq!(trimmed(&props.properties_data.about_text), "Hello world");
        assert!(props.properties_data.allow_publish);
        assert_eq!(
            trimmed(&props.properties_data.profile_url),
            "https://example.com"
        );

        let interests = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::AvatarInterestsUpdate(message) => Some(message),
                _ => None,
            })
            .ok_or("expected an AvatarInterestsUpdate")?;
        assert_eq!(interests.properties_data.want_to_mask, 0x7);
        assert_eq!(
            trimmed(&interests.properties_data.want_to_text),
            "build, explore"
        );
        assert_eq!(trimmed(&interests.properties_data.skills_text), "scripting");

        let notes = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::AvatarNotesUpdate(message) => Some(message),
                _ => None,
            })
            .ok_or("expected an AvatarNotesUpdate")?;
        assert_eq!(notes.data.target_id, target);
        assert_eq!(trimmed(&notes.data.notes), "a good friend");
        Ok(())
    }

    #[test]
    fn pick_and_classified_edits_encode() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let pick = uuid::Uuid::from_u128(0xC1);
        session.update_pick(
            &PickUpdate {
                pick_id: pick,
                name: "New pick".to_owned(),
                description: "a place".to_owned(),
                pos_global: (256_000.0, 256_128.0, 25.5),
                ..PickUpdate::default()
            },
            now,
        )?;
        session.delete_pick(pick, now)?;

        let classified = uuid::Uuid::from_u128(0xD1);
        session.update_classified(
            &ClassifiedUpdate {
                classified_id: classified,
                category: 3,
                name: "New classified".to_owned(),
                description: "for sale".to_owned(),
                price_for_listing: 100,
                classified_flags: 0x4,
                ..ClassifiedUpdate::default()
            },
            now,
        )?;
        session.delete_classified(classified, now)?;
        let sent = drain(&mut session)?;

        let pick_update = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::PickInfoUpdate(message) => Some(message),
                _ => None,
            })
            .ok_or("expected a PickInfoUpdate")?;
        assert_eq!(pick_update.data.pick_id, pick);
        // The session fills the creator with the agent itself, and never sets
        // the god-only top-pick flag.
        assert_eq!(pick_update.data.creator_id, uuid::Uuid::from_u128(1));
        assert!(!pick_update.data.top_pick);
        assert_eq!(trimmed(&pick_update.data.name), "New pick");
        let [px, py, pz] = pick_update.data.pos_global;
        assert!((px - 256_000.0).abs() < f64::EPSILON);
        assert!((py - 256_128.0).abs() < f64::EPSILON);
        assert!((pz - 25.5).abs() < f64::EPSILON);

        let pick_delete = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::PickDelete(message) => Some(message),
                _ => None,
            })
            .ok_or("expected a PickDelete")?;
        assert_eq!(pick_delete.data.pick_id, pick);

        let classified_update = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::ClassifiedInfoUpdate(message) => Some(message),
                _ => None,
            })
            .ok_or("expected a ClassifiedInfoUpdate")?;
        assert_eq!(classified_update.data.classified_id, classified);
        assert_eq!(classified_update.data.category, 3);
        assert_eq!(trimmed(&classified_update.data.name), "New classified");
        assert_eq!(classified_update.data.price_for_listing, 100);
        assert_eq!(classified_update.data.classified_flags, 0x4);
        // Set on the simulator as the message passes through.
        assert_eq!(classified_update.data.parent_estate, 0);

        let classified_delete = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::ClassifiedDelete(message) => Some(message),
                _ => None,
            })
            .ok_or("expected a ClassifiedDelete")?;
        assert_eq!(classified_delete.data.classified_id, classified);
        Ok(())
    }

    #[test]
    fn login_buddy_list_emits_friend_list() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = new_session();
        let friend_a = uuid::Uuid::from_u128(0xF1);
        let friend_b = uuid::Uuid::from_u128(0xF2);
        let LoginResponse::Success(mut login_success) = success() else {
            return Err("expected a success response".into());
        };
        login_success.buddy_list = vec![
            sl_wire::BuddyListEntry {
                buddy_id: friend_a,
                rights_granted: FriendRights::CAN_SEE_ONLINE | FriendRights::CAN_SEE_ON_MAP,
                rights_has: FriendRights::CAN_SEE_ONLINE,
            },
            sl_wire::BuddyListEntry {
                buddy_id: friend_b,
                rights_granted: 0,
                rights_has: FriendRights::CAN_MODIFY_OBJECTS,
            },
        ];
        session.handle_login_response(LoginResponse::Success(login_success), now)?;

        let friends = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::FriendList(friends) => Some(friends),
                _ => None,
            })
            .ok_or("expected a FriendList event")?;
        assert_eq!(friends.len(), 2);
        let first = friends.first().ok_or("first friend")?;
        assert_eq!(first.id, friend_a);
        assert!(first.rights_granted.can_see_online());
        assert!(first.rights_granted.can_see_on_map());
        assert!(first.rights_received.can_see_online());
        assert!(!first.rights_received.can_modify_objects());
        let second = friends.get(1).ok_or("second friend")?;
        assert_eq!(second.id, friend_b);
        assert!(second.rights_received.can_modify_objects());
        Ok(())
    }

    #[test]
    fn online_notification_surfaces_event() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let friend_a = uuid::Uuid::from_u128(0xF1);
        let friend_b = uuid::Uuid::from_u128(0xF2);
        let message = AnyMessage::OnlineNotification(OnlineNotification {
            agent_block: vec![
                OnlineNotificationAgentBlockBlock { agent_id: friend_a },
                OnlineNotificationAgentBlockBlock { agent_id: friend_b },
            ],
        });
        let datagram = server_message(&message, 9, true)?;
        session.handle_datagram(sim_addr(), &datagram, now)?;

        let ids = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::FriendsOnline(ids) => Some(ids),
                _ => None,
            })
            .ok_or("expected a FriendsOnline event")?;
        assert_eq!(ids, vec![friend_a, friend_b]);
        Ok(())
    }

    #[test]
    fn offline_notification_surfaces_event() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let friend = uuid::Uuid::from_u128(0xF3);
        let message = AnyMessage::OfflineNotification(OfflineNotification {
            agent_block: vec![OfflineNotificationAgentBlockBlock { agent_id: friend }],
        });
        let datagram = server_message(&message, 9, true)?;
        session.handle_datagram(sim_addr(), &datagram, now)?;

        let ids = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::FriendsOffline(ids) => Some(ids),
                _ => None,
            })
            .ok_or("expected a FriendsOffline event")?;
        assert_eq!(ids, vec![friend]);
        Ok(())
    }

    #[test]
    fn change_user_rights_from_friend_surfaces_event() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // The friend (not our own agent id 1) changed the rights they grant us;
        // `agent_related` is our own id.
        let friend = uuid::Uuid::from_u128(0xF4);
        let message = AnyMessage::ChangeUserRights(ChangeUserRights {
            agent_data: ChangeUserRightsAgentDataBlock { agent_id: friend },
            rights: vec![ChangeUserRightsRightsBlock {
                agent_related: uuid::Uuid::from_u128(1),
                related_rights: FriendRights::CAN_SEE_ONLINE | FriendRights::CAN_SEE_ON_MAP,
            }],
        });
        let datagram = server_message(&message, 9, true)?;
        session.handle_datagram(sim_addr(), &datagram, now)?;

        let event = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::FriendRightsChanged { .. } => Some(event),
                _ => None,
            })
            .ok_or("expected a FriendRightsChanged event")?;
        match event {
            Event::FriendRightsChanged {
                friend_id,
                rights,
                granted_to_us,
            } => {
                assert_eq!(friend_id, friend);
                assert!(granted_to_us);
                assert!(rights.can_see_online());
                assert!(rights.can_see_on_map());
            }
            _ => return Err("expected FriendRightsChanged".into()),
        }
        Ok(())
    }

    #[test]
    fn change_user_rights_echo_surfaces_event() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // The simulator echoes a change *we* made (AgentData id == our agent id
        // 1); `agent_related` names the friend.
        let friend = uuid::Uuid::from_u128(0xF5);
        let message = AnyMessage::ChangeUserRights(ChangeUserRights {
            agent_data: ChangeUserRightsAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(1),
            },
            rights: vec![ChangeUserRightsRightsBlock {
                agent_related: friend,
                related_rights: FriendRights::CAN_MODIFY_OBJECTS,
            }],
        });
        let datagram = server_message(&message, 9, true)?;
        session.handle_datagram(sim_addr(), &datagram, now)?;

        let event = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::FriendRightsChanged { .. } => Some(event),
                _ => None,
            })
            .ok_or("expected a FriendRightsChanged event")?;
        match event {
            Event::FriendRightsChanged {
                friend_id,
                rights,
                granted_to_us,
            } => {
                assert_eq!(friend_id, friend);
                assert!(!granted_to_us);
                assert!(rights.can_modify_objects());
            }
            _ => return Err("expected FriendRightsChanged".into()),
        }
        Ok(())
    }

    #[test]
    fn send_friendship_offer_packs_im() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let friend = uuid::Uuid::from_u128(0xA6);
        session.send_friendship_offer(friend, "be my friend", now)?;
        let sent = drain(&mut session)?;
        let im = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::ImprovedInstantMessage(im) => Some(im),
                _ => None,
            })
            .ok_or("expected an ImprovedInstantMessage")?;
        // IM_FRIENDSHIP_OFFERED == 38.
        assert_eq!(im.message_block.dialog, 38);
        assert_eq!(im.message_block.to_agent_id, friend);
        Ok(())
    }

    #[test]
    fn grant_user_rights_packs_message() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let friend = uuid::Uuid::from_u128(0xA7);
        session.grant_user_rights(
            friend,
            FriendRights(FriendRights::CAN_SEE_ONLINE | FriendRights::CAN_SEE_ON_MAP),
            now,
        )?;
        let sent = drain(&mut session)?;
        let grant = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::GrantUserRights(grant) => Some(grant),
                _ => None,
            })
            .ok_or("expected a GrantUserRights")?;
        let block = grant.rights.first().ok_or("a rights block")?;
        assert_eq!(block.agent_related, friend);
        assert_eq!(block.related_rights, 0b11);
        Ok(())
    }

    #[test]
    fn terminate_friendship_packs_message() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let friend = uuid::Uuid::from_u128(0xA8);
        session.terminate_friendship(friend, now)?;
        let sent = drain(&mut session)?;
        let terminate = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::TerminateFriendship(terminate) => Some(terminate),
                _ => None,
            })
            .ok_or("expected a TerminateFriendship")?;
        assert_eq!(terminate.ex_block.other_id, friend);
        Ok(())
    }

    #[test]
    fn accept_and_decline_friendship_pack_messages() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let transaction = uuid::Uuid::from_u128(0xAA);
        let folder = uuid::Uuid::from_u128(0xBB);
        session.accept_friendship(transaction, folder, now)?;
        let sent = drain(&mut session)?;
        let accept = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::AcceptFriendship(accept) => Some(accept),
                _ => None,
            })
            .ok_or("expected an AcceptFriendship")?;
        assert_eq!(accept.transaction_block.transaction_id, transaction);
        assert_eq!(
            accept.folder_data.first().map(|f| f.folder_id),
            Some(folder)
        );

        let decline_tx = uuid::Uuid::from_u128(0xCC);
        session.decline_friendship(decline_tx, now)?;
        let sent = drain(&mut session)?;
        let decline = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::DeclineFriendship(decline) => Some(decline),
                _ => None,
            })
            .ok_or("expected a DeclineFriendship")?;
        assert_eq!(decline.transaction_block.transaction_id, decline_tx);
        Ok(())
    }

    #[test]
    fn agent_data_update_surfaces_active_group() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let group = uuid::Uuid::from_u128(0x6701);
        let message = AnyMessage::AgentDataUpdate(AgentDataUpdate {
            agent_data: AgentDataUpdateAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(1),
                first_name: b"Avatar\0".to_vec(),
                last_name: b"Tester\0".to_vec(),
                group_title: b"Founder\0".to_vec(),
                active_group_id: group,
                group_powers: 0x1234,
                group_name: b"Test Group\0".to_vec(),
            },
        });
        let datagram = server_message(&message, 9, true)?;
        session.handle_datagram(sim_addr(), &datagram, now)?;

        let active = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::ActiveGroupChanged(active) => Some(active),
                _ => None,
            })
            .ok_or("expected an ActiveGroupChanged event")?;
        assert_eq!(active.active_group_id, group);
        assert_eq!(active.group_title, "Founder");
        assert_eq!(active.group_name, "Test Group");
        assert_eq!(active.group_powers, 0x1234);
        Ok(())
    }

    #[test]
    fn agent_group_data_update_surfaces_memberships() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let group = uuid::Uuid::from_u128(0x6702);
        let message = AnyMessage::AgentGroupDataUpdate(AgentGroupDataUpdate {
            agent_data: AgentGroupDataUpdateAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(1),
            },
            group_data: vec![AgentGroupDataUpdateGroupDataBlock {
                group_id: group,
                group_powers: 0xFF,
                accept_notices: true,
                group_insignia_id: uuid::Uuid::nil(),
                contribution: 50,
                group_name: b"Test Group\0".to_vec(),
            }],
        });
        let datagram = server_message(&message, 9, true)?;
        session.handle_datagram(sim_addr(), &datagram, now)?;

        let groups = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::GroupMemberships(groups) => Some(groups),
                _ => None,
            })
            .ok_or("expected a GroupMemberships event")?;
        assert_eq!(groups.len(), 1);
        let first = groups.first().ok_or("first group")?;
        assert_eq!(first.group_id, group);
        assert_eq!(first.group_name, "Test Group");
        assert!(first.accept_notices);
        assert_eq!(first.contribution, 50);
        Ok(())
    }

    #[test]
    fn group_members_reply_surfaces_event() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let group = uuid::Uuid::from_u128(0x6703);
        let member = uuid::Uuid::from_u128(0x6704);
        let message = AnyMessage::GroupMembersReply(GroupMembersReply {
            agent_data: GroupMembersReplyAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(1),
            },
            group_data: GroupMembersReplyGroupDataBlock {
                group_id: group,
                request_id: uuid::Uuid::nil(),
                member_count: 1,
            },
            member_data: vec![GroupMembersReplyMemberDataBlock {
                agent_id: member,
                contribution: 10,
                online_status: b"Online\0".to_vec(),
                agent_powers: 0xABCD,
                title: b"Owner\0".to_vec(),
                is_owner: true,
            }],
        });
        let datagram = server_message(&message, 9, true)?;
        session.handle_datagram(sim_addr(), &datagram, now)?;

        let (group_id, members) = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::GroupMembers {
                    group_id, members, ..
                } => Some((group_id, members)),
                _ => None,
            })
            .ok_or("expected a GroupMembers event")?;
        assert_eq!(group_id, group);
        assert_eq!(members.len(), 1);
        let first = members.first().ok_or("first member")?;
        assert_eq!(first.agent_id, member);
        assert_eq!(first.title, "Owner");
        assert!(first.is_owner);
        assert_eq!(first.agent_powers, 0xABCD);
        Ok(())
    }

    #[test]
    fn group_profile_reply_surfaces_event() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let group = uuid::Uuid::from_u128(0x6705);
        let founder = uuid::Uuid::from_u128(0x6706);
        let message = AnyMessage::GroupProfileReply(GroupProfileReply {
            agent_data: GroupProfileReplyAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(1),
            },
            group_data: GroupProfileReplyGroupDataBlock {
                group_id: group,
                name: b"Test Group\0".to_vec(),
                charter: b"a charter\0".to_vec(),
                show_in_list: true,
                member_title: b"Member\0".to_vec(),
                powers_mask: 0x7FFF,
                insignia_id: uuid::Uuid::nil(),
                founder_id: founder,
                membership_fee: 0,
                open_enrollment: true,
                money: 0,
                group_membership_count: 2,
                group_roles_count: 1,
                allow_publish: false,
                mature_publish: false,
                owner_role: uuid::Uuid::from_u128(0x6707),
            },
        });
        let datagram = server_message(&message, 9, true)?;
        session.handle_datagram(sim_addr(), &datagram, now)?;

        let profile = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::GroupProfileReceived(profile) => Some(profile),
                _ => None,
            })
            .ok_or("expected a GroupProfileReceived event")?;
        assert_eq!(profile.group_id, group);
        assert_eq!(profile.name, "Test Group");
        assert_eq!(profile.charter, "a charter");
        assert_eq!(profile.founder_id, founder);
        assert_eq!(profile.member_count, 2);
        assert!(profile.open_enrollment);
        Ok(())
    }

    #[test]
    fn group_session_message_surfaces_event() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // A group IM: from_group set, dialog SessionSend (17), session id = group.
        let group = uuid::Uuid::from_u128(0x6708);
        let sender = uuid::Uuid::from_u128(0x6709);
        let message = AnyMessage::ImprovedInstantMessage(ImprovedInstantMessage {
            agent_data: ImprovedInstantMessageAgentDataBlock {
                agent_id: sender,
                session_id: uuid::Uuid::nil(),
            },
            message_block: ImprovedInstantMessageMessageBlockBlock {
                from_group: true,
                to_agent_id: uuid::Uuid::from_u128(1),
                parent_estate_id: 0,
                region_id: uuid::Uuid::nil(),
                position: Vector {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                offline: 0,
                dialog: 17,
                id: group,
                timestamp: 0,
                from_agent_name: b"Friend Tester\0".to_vec(),
                message: b"hello group\0".to_vec(),
                binary_bucket: Vec::new(),
            },
            estate_block: ImprovedInstantMessageEstateBlockBlock { estate_id: 0 },
            meta_data: Vec::new(),
        });
        let datagram = server_message(&message, 9, true)?;
        session.handle_datagram(sim_addr(), &datagram, now)?;

        let event = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::GroupSessionMessage { .. } => Some(event),
                _ => None,
            })
            .ok_or("expected a GroupSessionMessage event")?;
        match event {
            Event::GroupSessionMessage {
                group_id,
                from_agent_id,
                from_name,
                message,
            } => {
                assert_eq!(group_id, group);
                assert_eq!(from_agent_id, sender);
                assert_eq!(from_name, "Friend Tester");
                assert_eq!(message, "hello group");
            }
            _ => return Err("expected GroupSessionMessage".into()),
        }
        Ok(())
    }

    #[test]
    fn activate_group_and_requests_pack_messages() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let group = uuid::Uuid::from_u128(0x670A);
        session.activate_group(group, now)?;
        session.request_group_members(group, now)?;
        let sent = drain(&mut session)?;

        let activate = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::ActivateGroup(a) => Some(a),
                _ => None,
            })
            .ok_or("expected an ActivateGroup")?;
        assert_eq!(activate.agent_data.group_id, group);

        let members = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::GroupMembersRequest(r) => Some(r),
                _ => None,
            })
            .ok_or("expected a GroupMembersRequest")?;
        assert_eq!(members.group_data.group_id, group);
        Ok(())
    }

    #[test]
    fn group_session_send_packs_im() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let group = uuid::Uuid::from_u128(0x670B);
        session.start_group_session(group, now)?;
        session.send_group_message(group, "hi all", now)?;
        let sent = drain(&mut session)?;
        let ims: Vec<_> = sent
            .iter()
            .filter_map(|m| match m {
                AnyMessage::ImprovedInstantMessage(im) => Some(&im.message_block),
                _ => None,
            })
            .collect();
        // Start session: dialog 15, session id = group.
        let start = ims
            .iter()
            .find(|b| b.dialog == 15)
            .ok_or("expected a SessionGroupStart IM")?;
        assert_eq!(start.id, group);
        assert_eq!(start.to_agent_id, group);
        // Send: dialog 17, session id = group, carrying the text.
        let send = ims
            .iter()
            .find(|b| b.dialog == 17)
            .ok_or("expected a SessionSend IM")?;
        assert_eq!(send.id, group);
        assert_eq!(send.to_agent_id, group);
        assert_eq!(trimmed(&send.message), "hi all");
        Ok(())
    }

    #[test]
    fn create_join_leave_invite_pack_messages() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        session.create_group(
            &CreateGroupParams {
                name: "My Group".to_owned(),
                charter: "hi".to_owned(),
                show_in_list: true,
                insignia_id: uuid::Uuid::nil(),
                membership_fee: 0,
                open_enrollment: true,
                allow_publish: false,
                mature_publish: false,
            },
            now,
        )?;
        let group = uuid::Uuid::from_u128(0x670C);
        let invitee = uuid::Uuid::from_u128(0x670D);
        session.join_group(group, now)?;
        session.leave_group(group, now)?;
        session.invite_to_group(group, &[(invitee, uuid::Uuid::nil())], now)?;
        let sent = drain(&mut session)?;

        let create = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::CreateGroupRequest(c) => Some(c),
                _ => None,
            })
            .ok_or("expected a CreateGroupRequest")?;
        assert_eq!(trimmed(&create.group_data.name), "My Group");
        assert!(sent.iter().any(
            |m| matches!(m, AnyMessage::JoinGroupRequest(j) if j.group_data.group_id == group)
        ));
        assert!(sent.iter().any(
            |m| matches!(m, AnyMessage::LeaveGroupRequest(l) if l.group_data.group_id == group)
        ));
        let invite = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::InviteGroupRequest(i) => Some(i),
                _ => None,
            })
            .ok_or("expected an InviteGroupRequest")?;
        assert_eq!(
            invite.invite_data.first().map(|d| d.invitee_id),
            Some(invitee)
        );
        Ok(())
    }

    #[test]
    fn update_group_roles_packs_role_data() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let group = uuid::Uuid::from_u128(0x6711);
        let role = uuid::Uuid::from_u128(0x6712);
        session.update_group_roles(
            group,
            &[
                GroupRoleEdit {
                    role_id: role,
                    name: "Officers".to_owned(),
                    description: "the officers".to_owned(),
                    title: "Officer".to_owned(),
                    powers: group_powers::MEMBER_INVITE | group_powers::NOTICES_SEND,
                    update_type: GroupRoleUpdateType::Create,
                },
                GroupRoleEdit {
                    role_id: uuid::Uuid::from_u128(0x6713),
                    name: String::new(),
                    description: String::new(),
                    title: String::new(),
                    powers: 0,
                    update_type: GroupRoleUpdateType::Delete,
                },
            ],
            now,
        )?;
        let sent = drain(&mut session)?;
        let update = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::GroupRoleUpdate(u) => Some(u),
                _ => None,
            })
            .ok_or("expected a GroupRoleUpdate")?;
        assert_eq!(update.agent_data.group_id, group);
        assert_eq!(update.role_data.len(), 2);
        let create = update.role_data.first().ok_or("first role")?;
        assert_eq!(create.role_id, role);
        assert_eq!(trimmed(&create.name), "Officers");
        assert_eq!(create.update_type, 4); // Create
        assert_eq!(
            create.powers,
            group_powers::MEMBER_INVITE | group_powers::NOTICES_SEND
        );
        let delete = update.role_data.get(1).ok_or("second role")?;
        assert_eq!(delete.update_type, 5); // Delete
        Ok(())
    }

    #[test]
    fn change_group_role_members_packs_changes() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let group = uuid::Uuid::from_u128(0x6714);
        let role = uuid::Uuid::from_u128(0x6715);
        let member = uuid::Uuid::from_u128(0x6716);
        session.change_group_role_members(
            group,
            &[
                GroupRoleMemberChange {
                    role_id: role,
                    member_id: member,
                    change: GroupRoleChange::Add,
                },
                GroupRoleMemberChange {
                    role_id: role,
                    member_id: uuid::Uuid::from_u128(0x6717),
                    change: GroupRoleChange::Remove,
                },
            ],
            now,
        )?;
        let sent = drain(&mut session)?;
        let changes = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::GroupRoleChanges(c) => Some(c),
                _ => None,
            })
            .ok_or("expected a GroupRoleChanges")?;
        assert_eq!(changes.agent_data.group_id, group);
        assert_eq!(changes.role_change.len(), 2);
        let add = changes.role_change.first().ok_or("first change")?;
        assert_eq!(add.role_id, role);
        assert_eq!(add.member_id, member);
        assert_eq!(add.change, 0); // Add
        let remove = changes.role_change.get(1).ok_or("second change")?;
        assert_eq!(remove.change, 1); // Remove
        Ok(())
    }

    #[test]
    fn eject_group_members_packs_request() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let group = uuid::Uuid::from_u128(0x6718);
        let ejectee = uuid::Uuid::from_u128(0x6719);
        session.eject_group_members(group, &[ejectee], now)?;
        let sent = drain(&mut session)?;
        let eject = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::EjectGroupMemberRequest(e) => Some(e),
                _ => None,
            })
            .ok_or("expected an EjectGroupMemberRequest")?;
        assert_eq!(eject.group_data.group_id, group);
        assert_eq!(
            eject.eject_data.first().map(|d| d.ejectee_id),
            Some(ejectee)
        );
        Ok(())
    }

    #[test]
    fn eject_group_member_reply_surfaces_event() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let group = uuid::Uuid::from_u128(0x671A);
        let message = AnyMessage::EjectGroupMemberReply(EjectGroupMemberReply {
            agent_data: EjectGroupMemberReplyAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(1),
            },
            group_data: EjectGroupMemberReplyGroupDataBlock { group_id: group },
            eject_data: EjectGroupMemberReplyEjectDataBlock { success: true },
        });
        let datagram = server_message(&message, 9, true)?;
        session.handle_datagram(sim_addr(), &datagram, now)?;

        let (group_id, success) = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::EjectGroupMemberResult { group_id, success } => Some((group_id, success)),
                _ => None,
            })
            .ok_or("expected an EjectGroupMemberResult event")?;
        assert_eq!(group_id, group);
        assert!(success);
        Ok(())
    }

    #[test]
    fn send_group_notice_packs_im_with_attachment() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let group = uuid::Uuid::from_u128(0x671B);
        let item = uuid::Uuid::from_u128(0x671C);
        let owner = uuid::Uuid::from_u128(0x671D);
        // A plain notice (empty bucket).
        session.send_group_notice(group, "Subject", "Body text", None, now)?;
        // A notice with an inventory attachment (LLSD bucket).
        session.send_group_notice(
            group,
            "Gift",
            "Here you go",
            Some(GroupNoticeAttachment {
                item_id: item,
                owner_id: owner,
            }),
            now,
        )?;
        let sent = drain(&mut session)?;
        let notices: Vec<_> = sent
            .iter()
            .filter_map(|m| match m {
                AnyMessage::ImprovedInstantMessage(im)
                    if im.message_block.dialog == ImDialog::GroupNotice.to_u8() =>
                {
                    Some(&im.message_block)
                }
                _ => None,
            })
            .collect();
        assert_eq!(notices.len(), 2);
        let plain = notices.first().ok_or("plain notice")?;
        // The session id and recipient are the group; subject|body is joined.
        assert_eq!(plain.to_agent_id, group);
        assert!(!plain.from_group);
        assert_eq!(trimmed(&plain.message), "Subject|Body text");
        // No attachment: the one-byte empty bucket.
        assert_eq!(plain.binary_bucket, vec![0_u8]);
        let gift = notices.get(1).ok_or("gift notice")?;
        assert_eq!(trimmed(&gift.message), "Gift|Here you go");
        // The attachment bucket carries the 15-byte LLSD header and both ids.
        let bucket = String::from_utf8_lossy(&gift.binary_bucket);
        assert!(bucket.starts_with("<? LLSD/XML ?>\n"));
        assert!(bucket.contains(&item.to_string()));
        assert!(bucket.contains(&owner.to_string()));
        Ok(())
    }

    #[test]
    fn agent_group_data_update_caps_surfaces_memberships() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // The modern CAPS event-queue delivery of `AgentGroupDataUpdate`.
        let xml = concat!(
            "<llsd><map>",
            "<key>AgentData</key><array><map><key>AgentID</key>",
            "<uuid>00000000-0000-0000-0000-000000000001</uuid></map></array>",
            "<key>GroupData</key><array><map>",
            "<key>GroupID</key><uuid>00000000-0000-0000-0000-000000006701</uuid>",
            "<key>GroupPowers</key><integer>4660</integer>",
            "<key>AcceptNotices</key><boolean>1</boolean>",
            "<key>GroupInsigniaID</key><uuid>00000000-0000-0000-0000-000000000000</uuid>",
            "<key>Contribution</key><integer>25</integer>",
            "<key>GroupName</key><string>CAPS Group</string>",
            "</map></array></map></llsd>",
        );
        let body = parse_llsd_xml(xml)?;
        session.handle_caps_event("AgentGroupDataUpdate", &body, now)?;

        let groups = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::GroupMemberships(groups) => Some(groups),
                _ => None,
            })
            .ok_or("expected a GroupMemberships event")?;
        assert_eq!(groups.len(), 1);
        let first = groups.first().ok_or("first group")?;
        assert_eq!(first.group_id, uuid::Uuid::from_u128(0x6701));
        assert_eq!(first.group_name, "CAPS Group");
        assert_eq!(first.group_powers, 4660);
        assert!(first.accept_notices);
        assert_eq!(first.contribution, 25);
        Ok(())
    }

    #[test]
    fn group_member_data_caps_surfaces_members() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // A `GroupMemberData` capability response: members keyed by id, titles
        // by index, powers as hex strings.
        let xml = concat!(
            "<llsd><map>",
            "<key>group_id</key><uuid>00000000-0000-0000-0000-000000006703</uuid>",
            "<key>titles</key><array><string>Member</string><string>Owner</string></array>",
            "<key>defaults</key><map><key>default_powers</key><string>0x0</string></map>",
            "<key>members</key><map>",
            "<key>00000000-0000-0000-0000-000000006704</key><map>",
            "<key>title</key><integer>1</integer>",
            "<key>powers</key><string>0xabcd</string>",
            "<key>last_login</key><string>Online</string>",
            "<key>donated_square_meters</key><integer>512</integer>",
            "<key>owner</key><integer>1</integer>",
            "</map></map></map></llsd>",
        );
        let body = parse_llsd_xml(xml)?;
        session.handle_caps_event("GroupMemberData", &body, now)?;

        let (group_id, members) = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::GroupMembers {
                    group_id, members, ..
                } => Some((group_id, members)),
                _ => None,
            })
            .ok_or("expected a GroupMembers event")?;
        assert_eq!(group_id, uuid::Uuid::from_u128(0x6703));
        assert_eq!(members.len(), 1);
        let first = members.first().ok_or("first member")?;
        assert_eq!(first.agent_id, uuid::Uuid::from_u128(0x6704));
        assert_eq!(first.title, "Owner");
        assert_eq!(first.agent_powers, 0xabcd);
        assert_eq!(first.online_status, "Online");
        assert_eq!(first.contribution, 512);
        assert!(first.is_owner);
        Ok(())
    }

    #[test]
    fn script_dialog_surfaces_event() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let object = uuid::Uuid::from_u128(0x8001);
        let owner = uuid::Uuid::from_u128(0x8002);
        let message = AnyMessage::ScriptDialog(ScriptDialog {
            data: ScriptDialogDataBlock {
                object_id: object,
                first_name: b"Avatar\0".to_vec(),
                last_name: b"Tester\0".to_vec(),
                object_name: b"Vendor\0".to_vec(),
                message: b"Pick one\0".to_vec(),
                chat_channel: -1234,
                image_id: uuid::Uuid::nil(),
            },
            buttons: vec![
                ScriptDialogButtonsBlock {
                    button_label: b"Yes\0".to_vec(),
                },
                ScriptDialogButtonsBlock {
                    button_label: b"No\0".to_vec(),
                },
            ],
            owner_data: vec![ScriptDialogOwnerDataBlock { owner_id: owner }],
        });
        let datagram = server_message(&message, 9, true)?;
        session.handle_datagram(sim_addr(), &datagram, now)?;

        let dialog = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::ScriptDialog(dialog) => Some(dialog),
                _ => None,
            })
            .ok_or("expected a ScriptDialog event")?;
        assert_eq!(dialog.object_id, object);
        assert_eq!(dialog.owner_id, owner);
        assert_eq!(dialog.object_name, "Vendor");
        assert_eq!(dialog.message, "Pick one");
        assert_eq!(dialog.chat_channel, -1234);
        assert_eq!(dialog.buttons, vec!["Yes".to_owned(), "No".to_owned()]);
        assert!(!dialog.is_text_box());
        Ok(())
    }

    #[test]
    fn script_question_surfaces_permission_request() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let task = uuid::Uuid::from_u128(0x8003);
        let item = uuid::Uuid::from_u128(0x8004);
        let requested = ScriptPermissions::DEBIT | ScriptPermissions::TAKE_CONTROLS;
        let message = AnyMessage::ScriptQuestion(ScriptQuestion {
            data: ScriptQuestionDataBlock {
                task_id: task,
                item_id: item,
                object_name: b"Money Tree\0".to_vec(),
                object_owner: b"Avatar Tester\0".to_vec(),
                questions: requested,
            },
            experience: ScriptQuestionExperienceBlock {
                experience_id: uuid::Uuid::nil(),
            },
        });
        let datagram = server_message(&message, 9, true)?;
        session.handle_datagram(sim_addr(), &datagram, now)?;

        let request = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::ScriptPermissionRequest(request) => Some(request),
                _ => None,
            })
            .ok_or("expected a ScriptPermissionRequest event")?;
        assert_eq!(request.task_id, task);
        assert_eq!(request.item_id, item);
        assert_eq!(request.object_name, "Money Tree");
        assert_eq!(request.permissions.0, requested);
        assert!(request.permissions.contains(ScriptPermissions::DEBIT));
        assert!(
            request
                .permissions
                .contains(ScriptPermissions::TAKE_CONTROLS)
        );
        assert!(!request.permissions.contains(ScriptPermissions::ATTACH));
        Ok(())
    }

    #[test]
    fn reply_script_dialog_packs_message() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let object = uuid::Uuid::from_u128(0x8005);
        session.reply_script_dialog(object, -1234, 1, "No", now)?;
        let sent = drain(&mut session)?;
        let reply = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::ScriptDialogReply(reply) => Some(reply),
                _ => None,
            })
            .ok_or("expected a ScriptDialogReply")?;
        assert_eq!(reply.data.object_id, object);
        assert_eq!(reply.data.chat_channel, -1234);
        assert_eq!(reply.data.button_index, 1);
        assert_eq!(trimmed(&reply.data.button_label), "No");
        Ok(())
    }

    #[test]
    fn answer_script_permissions_packs_message() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let task = uuid::Uuid::from_u128(0x8006);
        let item = uuid::Uuid::from_u128(0x8007);
        session.answer_script_permissions(
            task,
            item,
            ScriptPermissions(ScriptPermissions::TAKE_CONTROLS),
            now,
        )?;
        let sent = drain(&mut session)?;
        let answer = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::ScriptAnswerYes(answer) => Some(answer),
                _ => None,
            })
            .ok_or("expected a ScriptAnswerYes")?;
        assert_eq!(answer.data.task_id, task);
        assert_eq!(answer.data.item_id, item);
        assert_eq!(answer.data.questions, ScriptPermissions::TAKE_CONTROLS);
        Ok(())
    }

    #[test]
    fn mute_request_and_edits_pack_messages() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        session.request_mute_list(now)?;
        let target = uuid::Uuid::from_u128(0x9001);
        session.mute(
            target,
            "Bad Actor",
            MuteType::Agent,
            MuteFlags::default(),
            now,
        )?;
        session.unmute(target, "Bad Actor", now)?;
        let sent = drain(&mut session)?;

        let request = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::MuteListRequest(r) => Some(r),
                _ => None,
            })
            .ok_or("expected a MuteListRequest")?;
        assert_eq!(request.mute_data.mute_crc, 0);

        let update = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::UpdateMuteListEntry(u) => Some(u),
                _ => None,
            })
            .ok_or("expected an UpdateMuteListEntry")?;
        assert_eq!(update.mute_data.mute_id, target);
        assert_eq!(update.mute_data.mute_type, MuteType::Agent.to_i32());
        assert_eq!(trimmed(&update.mute_data.mute_name), "Bad Actor");

        let remove = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::RemoveMuteListEntry(r) => Some(r),
                _ => None,
            })
            .ok_or("expected a RemoveMuteListEntry")?;
        assert_eq!(remove.mute_data.mute_id, target);
        Ok(())
    }

    #[test]
    fn mute_list_update_downloads_and_parses_via_xfer() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // The sim says the mute list changed and names a file to Xfer.
        let update = AnyMessage::MuteListUpdate(MuteListUpdate {
            mute_data: MuteListUpdateMuteDataBlock {
                agent_id: uuid::Uuid::from_u128(1),
                filename: b"mutes00000000\0".to_vec(),
            },
        });
        session.handle_datagram(sim_addr(), &server_message(&update, 9, true)?, now)?;

        // The client should request the file over Xfer.
        let sent = drain(&mut session)?;
        let request = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::RequestXfer(r) => Some(r),
                _ => None,
            })
            .ok_or("expected a RequestXfer")?;
        let xfer_id = request.xfer_id.id;
        assert_ne!(xfer_id, 0);
        assert_eq!(trimmed(&request.xfer_id.filename), "mutes00000000");

        // The sim sends the file in a single (first+last) packet: a 4-byte
        // little-endian length prefix, then two mute lines.
        let muted = uuid::Uuid::from_u128(0x9002);
        let file =
            format!("1 {muted} Bad Actor|0\n0 00000000-0000-0000-0000-000000000000 SpamBot|3\n");
        // A 4-byte length prefix the parser strips and ignores, then the file.
        let mut data = vec![0u8; 4];
        data.extend_from_slice(file.as_bytes());
        let packet = AnyMessage::SendXferPacket(SendXferPacket {
            xfer_id: SendXferPacketXferIDBlock {
                id: xfer_id,
                packet: 0x8000_0000, // sequence 0 + last-packet flag
            },
            data_packet: SendXferPacketDataPacketBlock { data },
        });
        session.handle_datagram(sim_addr(), &server_message(&packet, 10, true)?, now)?;

        // The client confirms the packet and surfaces the parsed list.
        let sent = drain(&mut session)?;
        let confirm = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::ConfirmXferPacket(c) => Some(c),
                _ => None,
            })
            .ok_or("expected a ConfirmXferPacket")?;
        assert_eq!(confirm.xfer_id.id, xfer_id);

        let entries = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::MuteList(entries) => Some(entries),
                _ => None,
            })
            .ok_or("expected a MuteList event")?;
        assert_eq!(entries.len(), 2);
        let first = entries.first().ok_or("first entry")?;
        assert_eq!(first.id, muted);
        assert_eq!(first.name, "Bad Actor");
        assert_eq!(first.mute_type, MuteType::Agent);
        let second = entries.get(1).ok_or("second entry")?;
        assert_eq!(second.name, "SpamBot");
        assert_eq!(second.mute_type, MuteType::ByName);
        assert_eq!(second.flags.0, 3);
        Ok(())
    }

    #[test]
    fn request_texture_reassembles_image_data_and_packets() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let texture = uuid::Uuid::from_u128(0xABCD);
        session.request_texture(texture, 0, 1.0e6, now)?;

        // The client sends a RequestImage for the texture at the discard level.
        let sent = drain(&mut session)?;
        let request = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::RequestImage(r) => Some(r),
                _ => None,
            })
            .ok_or("expected a RequestImage")?;
        let block = request
            .request_image
            .first()
            .ok_or("expected a RequestImage block")?;
        assert_eq!(block.image, texture);
        assert_eq!(block.discard_level, 0);

        // The sim streams the texture in two packets: an ImageData header (codec
        // J2C, 2 packets) carrying packet 0, then one ImagePacket (packet 1).
        let header = AnyMessage::ImageData(ImageData {
            image_id: ImageDataImageIDBlock {
                id: texture,
                codec: 2, // IMG_CODEC_J2C
                size: 6,
                packets: 2,
            },
            image_data: ImageDataImageDataBlock {
                data: vec![1, 2, 3],
            },
        });
        session.handle_datagram(sim_addr(), &server_message(&header, 9, true)?, now)?;
        // Not complete yet — no event after only the first packet.
        assert!(drain_events(&mut session).is_empty());

        let follow = AnyMessage::ImagePacket(ImagePacket {
            image_id: ImagePacketImageIDBlock {
                id: texture,
                packet: 1,
            },
            image_data: ImagePacketImageDataBlock {
                data: vec![4, 5, 6],
            },
        });
        session.handle_datagram(sim_addr(), &server_message(&follow, 10, true)?, now)?;

        let received = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::TextureReceived(texture) => Some(texture),
                _ => None,
            })
            .ok_or("expected a TextureReceived event")?;
        assert_eq!(received.id, texture);
        assert_eq!(received.codec, ImageCodec::J2c);
        assert_eq!(received.data, vec![1, 2, 3, 4, 5, 6]);
        Ok(())
    }

    #[test]
    fn image_not_in_database_surfaces_texture_not_found() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let texture = uuid::Uuid::from_u128(0xDEAD);
        session.request_texture(texture, 3, 1.0e6, now)?;
        drain(&mut session)?;

        let missing = AnyMessage::ImageNotInDatabase(ImageNotInDatabase {
            image_id: ImageNotInDatabaseImageIDBlock { id: texture },
        });
        session.handle_datagram(sim_addr(), &server_message(&missing, 9, true)?, now)?;

        assert!(
            drain_events(&mut session)
                .iter()
                .any(|e| matches!(e, Event::TextureNotFound(id) if *id == texture))
        );
        Ok(())
    }

    #[test]
    fn request_asset_reassembles_transfer_packets() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let sound = uuid::Uuid::from_u128(0x5005);
        session.request_asset(sound, AssetType::Sound, 1.0, now)?;

        // The client sends a TransferRequest on the asset channel/source with a
        // params blob of the asset UUID followed by its little-endian type code.
        let sent = drain(&mut session)?;
        let request = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::TransferRequest(r) => Some(r),
                _ => None,
            })
            .ok_or("expected a TransferRequest")?;
        let transfer_id = request.transfer_info.transfer_id;
        assert_eq!(request.transfer_info.channel_type, 2);
        assert_eq!(request.transfer_info.source_type, 2);
        let mut expected_params = sound.as_bytes().to_vec();
        // AssetType::Sound == 1, little-endian i32.
        expected_params.extend_from_slice(&[1, 0, 0, 0]);
        assert_eq!(request.transfer_info.params, expected_params);

        // The sim acknowledges with a TransferInfo (OK, size 6), then two
        // TransferPackets; the second carries LLTS_DONE (1).
        let info = AnyMessage::TransferInfo(TransferInfo {
            transfer_info: TransferInfoTransferInfoBlock {
                transfer_id,
                channel_type: 2,
                target_type: 0,
                status: 0, // LLTS_OK
                size: 6,
                params: Vec::new(),
            },
        });
        session.handle_datagram(sim_addr(), &server_message(&info, 9, true)?, now)?;
        assert!(drain_events(&mut session).is_empty());

        let packet0 = AnyMessage::TransferPacket(TransferPacket {
            transfer_data: TransferPacketTransferDataBlock {
                transfer_id,
                channel_type: 2,
                packet: 0,
                status: 0, // LLTS_OK (more to come)
                data: vec![9, 8, 7],
            },
        });
        session.handle_datagram(sim_addr(), &server_message(&packet0, 10, true)?, now)?;
        assert!(drain_events(&mut session).is_empty());

        let packet1 = AnyMessage::TransferPacket(TransferPacket {
            transfer_data: TransferPacketTransferDataBlock {
                transfer_id,
                channel_type: 2,
                packet: 1,
                status: 1, // LLTS_DONE
                data: vec![6, 5, 4],
            },
        });
        session.handle_datagram(sim_addr(), &server_message(&packet1, 11, true)?, now)?;

        let asset = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::AssetReceived(asset) => Some(asset),
                _ => None,
            })
            .ok_or("expected an AssetReceived event")?;
        assert_eq!(asset.id, sound);
        assert_eq!(asset.asset_type, AssetType::Sound);
        assert_eq!(asset.data, vec![9, 8, 7, 6, 5, 4]);
        Ok(())
    }

    #[test]
    fn transfer_info_failure_surfaces_asset_transfer_failed() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let missing = uuid::Uuid::from_u128(0x404);
        session.request_asset(missing, AssetType::Animation, 1.0, now)?;
        let sent = drain(&mut session)?;
        let transfer_id = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::TransferRequest(r) => Some(r.transfer_info.transfer_id),
                _ => None,
            })
            .ok_or("expected a TransferRequest")?;

        // LLTS_UNKNOWN_SOURCE (-2): the asset does not exist.
        let info = AnyMessage::TransferInfo(TransferInfo {
            transfer_info: TransferInfoTransferInfoBlock {
                transfer_id,
                channel_type: 2,
                target_type: 0,
                status: -2,
                size: 0,
                params: Vec::new(),
            },
        });
        session.handle_datagram(sim_addr(), &server_message(&info, 9, true)?, now)?;

        assert!(drain_events(&mut session).iter().any(|e| matches!(
            e,
            Event::AssetTransferFailed { asset_id, status, .. }
            if *asset_id == missing && *status == TransferStatus::UnknownSource
        )));
        Ok(())
    }

    #[test]
    fn upload_asset_udp_inlines_small_asset_and_completes() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // A small asset is inlined directly in the AssetUploadRequest.
        let data = b"a tiny notecard".to_vec();
        let asset_id =
            session.upload_asset_udp(AssetType::Notecard, data.clone(), false, false, now)?;
        // The predicted asset id is combine(transaction_id, secure_session_id);
        // the first upload uses transaction id 1 and the test login's secure
        // session id is 3.
        let expected = sl_wire::combine_uuids(uuid::Uuid::from_u128(1), uuid::Uuid::from_u128(3));
        assert_eq!(asset_id, expected);

        let sent = drain(&mut session)?;
        let request = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::AssetUploadRequest(r) => Some(r),
                _ => None,
            })
            .ok_or("expected an AssetUploadRequest")?;
        // AssetType::Notecard == 7, inlined with the asset bytes, no Xfer.
        assert_eq!(request.asset_block.r#type, 7);
        assert_eq!(request.asset_block.asset_data, data);
        assert!(
            !sent
                .iter()
                .any(|m| matches!(m, AnyMessage::SendXferPacket(_)))
        );

        // The sim reports the upload complete.
        let complete = AnyMessage::AssetUploadComplete(AssetUploadComplete {
            asset_block: AssetUploadCompleteAssetBlockBlock {
                uuid: asset_id,
                r#type: 7,
                success: true,
            },
        });
        session.handle_datagram(sim_addr(), &server_message(&complete, 9, true)?, now)?;
        let event = drain_events(&mut session)
            .into_iter()
            .find(|event| matches!(event, Event::AssetUploadComplete { .. }))
            .ok_or("expected an AssetUploadComplete event")?;
        assert_eq!(
            event,
            Event::AssetUploadComplete {
                asset_id,
                asset_type: AssetType::Notecard,
                success: true,
            }
        );
        Ok(())
    }

    #[test]
    fn upload_asset_udp_streams_large_asset_over_xfer() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // An asset larger than the inline cutoff forces the Xfer path: two
        // 1000-byte chunks across two SendXferPackets.
        let data: Vec<u8> = (0..1500_u32)
            .map(|i| u8::try_from(i & 0xff).unwrap_or(0))
            .collect();
        let asset_id =
            session.upload_asset_udp(AssetType::Texture, data.clone(), true, false, now)?;

        let sent = drain(&mut session)?;
        let request = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::AssetUploadRequest(r) => Some(r),
                _ => None,
            })
            .ok_or("expected an AssetUploadRequest")?;
        // The asset is NOT inlined (empty AssetData forces the Xfer), and it is
        // flagged temporary.
        assert!(request.asset_block.asset_data.is_empty());
        assert!(request.asset_block.tempfile);

        // The sim requests the file over Xfer, naming our predicted asset id as
        // the VFileID.
        let xfer_id = 0x5151_u64;
        let request_xfer = AnyMessage::RequestXfer(RequestXfer {
            xfer_id: RequestXferXferIDBlock {
                id: xfer_id,
                filename: Vec::new(),
                file_path: 0,
                delete_on_completion: true,
                use_big_packets: false,
                v_file_id: asset_id,
                v_file_type: 0,
            },
        });
        session.handle_datagram(sim_addr(), &server_message(&request_xfer, 9, true)?, now)?;

        // The client streams packet 0: a 4-byte little-endian length prefix
        // (1500) followed by the first 1000 bytes; not yet the last packet.
        let first = drain(&mut session)?
            .into_iter()
            .find_map(|m| match m {
                AnyMessage::SendXferPacket(p) => Some(p),
                _ => None,
            })
            .ok_or("expected a SendXferPacket")?;
        assert_eq!(first.xfer_id.id, xfer_id);
        assert_eq!(first.xfer_id.packet, 0);
        assert_eq!(first.data_packet.data.len(), 1004);
        assert_eq!(
            first.data_packet.data.get(..4),
            Some([0xdc, 0x05, 0, 0].as_slice())
        );

        // The sim confirms packet 0; the client sends the final packet (sequence
        // 1, last-packet flag) carrying the remaining 500 bytes.
        let confirm = AnyMessage::ConfirmXferPacket(ConfirmXferPacket {
            xfer_id: ConfirmXferPacketXferIDBlock {
                id: xfer_id,
                packet: 0,
            },
        });
        session.handle_datagram(sim_addr(), &server_message(&confirm, 10, true)?, now)?;
        let last = drain(&mut session)?
            .into_iter()
            .find_map(|m| match m {
                AnyMessage::SendXferPacket(p) => Some(p),
                _ => None,
            })
            .ok_or("expected a final SendXferPacket")?;
        assert_eq!(last.xfer_id.packet, 1 | 0x8000_0000);
        assert_eq!(last.data_packet.data.len(), 500);

        // The sim reports completion.
        let complete = AnyMessage::AssetUploadComplete(AssetUploadComplete {
            asset_block: AssetUploadCompleteAssetBlockBlock {
                uuid: asset_id,
                r#type: 0,
                success: true,
            },
        });
        session.handle_datagram(sim_addr(), &server_message(&complete, 11, true)?, now)?;
        assert!(drain_events(&mut session).iter().any(|e| matches!(
            e,
            Event::AssetUploadComplete { asset_id: id, success: true, .. } if *id == asset_id
        )));
        Ok(())
    }

    #[test]
    fn caps_upload_completion_surfaces_asset_uploaded() -> Result<(), TestError> {
        // The runtimes drive the two-step CAPS upload and decode the response
        // with `parse_asset_upload_response`; verify the completion shape used to
        // build `Event::AssetUploaded`.
        let new_asset = uuid::Uuid::from_u128(0x000a_55e7);
        let new_item = uuid::Uuid::from_u128(0x17e3);
        let xml = format!(
            "<llsd><map><key>state</key><string>complete</string>\
             <key>new_asset</key><string>{new_asset}</string>\
             <key>new_inventory_item</key><uuid>{new_item}</uuid></map></llsd>"
        );
        let response = sl_wire::parse_asset_upload_response(&xml)?;
        assert_eq!(response.state, "complete");
        assert_eq!(response.new_asset, Some(new_asset));
        assert_eq!(response.new_inventory_item, Some(new_item));
        Ok(())
    }

    #[test]
    fn server_appearance_update_reply_surfaces_event() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // A COF-version-mismatch failure reply from the UpdateAvatarAppearance cap.
        let xml = concat!(
            "<llsd><map>",
            "<key>success</key><boolean>0</boolean>",
            "<key>error</key><string>cof version mismatch</string>",
            "<key>expected</key><integer>42</integer>",
            "</map></llsd>",
        );
        let body = parse_llsd_xml(xml)?;
        session.handle_caps_event("UpdateAvatarAppearance", &body, now)?;

        let event = drain_events(&mut session)
            .into_iter()
            .find(|event| matches!(event, Event::ServerAppearanceUpdate { .. }))
            .ok_or("expected a ServerAppearanceUpdate event")?;
        assert_eq!(
            event,
            Event::ServerAppearanceUpdate {
                success: false,
                error: Some("cof version mismatch".to_owned()),
                expected_cof_version: Some(42),
            }
        );
        Ok(())
    }

    #[test]
    fn avatar_appearance_decodes_baked_textures_and_params() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let avatar = uuid::Uuid::from_u128(0xA1);
        let head_bake = uuid::Uuid::from_u128(0xBEEF);

        // A minimal packed TextureEntry: a nil default texture with the head-baked
        // slot (index 8) overridden to `head_bake`. The face bitmask for bit 8 is
        // the two-byte base-128 big-endian value `0x82 0x00` (= 256), as the
        // viewer's `packTEField` emits. The remaining TE fields are omitted; the
        // decoder leaves them at their defaults.
        let mut te = Writer::new();
        te.put_uuid(uuid::Uuid::nil()); // default texture for all faces
        te.put_u8(0x82); // face bitmask: continuation + high bits
        te.put_u8(0x00); // face bitmask: low bits -> bit 8
        te.put_uuid(head_bake); // override value for face 8
        te.put_u8(0); // terminator for the texture field

        let message = AnyMessage::AvatarAppearance(AvatarAppearance {
            sender: AvatarAppearanceSenderBlock {
                id: avatar,
                is_trial: false,
            },
            object_data: AvatarAppearanceObjectDataBlock {
                texture_entry: te.into_bytes(),
            },
            visual_param: vec![
                AvatarAppearanceVisualParamBlock { param_value: 10 },
                AvatarAppearanceVisualParamBlock { param_value: 200 },
                AvatarAppearanceVisualParamBlock { param_value: 255 },
            ],
            appearance_data: Vec::new(),
            appearance_hover: Vec::new(),
            attachment_block: Vec::new(),
        });
        session.handle_datagram(sim_addr(), &server_message(&message, 9, true)?, now)?;

        let appearance = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::AvatarAppearance(appearance) => Some(appearance),
                _ => None,
            })
            .ok_or("expected an AvatarAppearance event")?;
        assert_eq!(appearance.avatar_id, avatar);
        assert_eq!(appearance.visual_params, vec![10, 200, 255]);
        // The baked head texture decodes at its slot; an untouched slot is nil.
        assert_eq!(
            appearance
                .texture_entry
                .texture_id(avatar_texture::HEAD_BAKED),
            Some(head_bake)
        );
        assert_eq!(
            appearance
                .texture_entry
                .texture_id(avatar_texture::UPPER_BAKED),
            Some(uuid::Uuid::nil())
        );
        Ok(())
    }

    #[test]
    fn agent_wearables_update_surfaces_worn_wearables() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let shape_item = uuid::Uuid::from_u128(0x11);
        let shape_asset = uuid::Uuid::from_u128(0x12);
        let shirt_item = uuid::Uuid::from_u128(0x21);
        let shirt_asset = uuid::Uuid::from_u128(0x22);

        let message = AnyMessage::AgentWearablesUpdate(AgentWearablesUpdate {
            agent_data: AgentWearablesUpdateAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(1),
                session_id: uuid::Uuid::from_u128(2),
                serial_num: 7,
            },
            wearable_data: vec![
                AgentWearablesUpdateWearableDataBlock {
                    item_id: shape_item,
                    asset_id: shape_asset,
                    wearable_type: 0, // WT_SHAPE
                },
                AgentWearablesUpdateWearableDataBlock {
                    item_id: shirt_item,
                    asset_id: shirt_asset,
                    wearable_type: 4, // WT_SHIRT
                },
            ],
        });
        session.handle_datagram(sim_addr(), &server_message(&message, 9, true)?, now)?;

        let (serial, wearables) = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::AgentWearables { serial, wearables } => Some((serial, wearables)),
                _ => None,
            })
            .ok_or("expected an AgentWearables event")?;
        assert_eq!(serial, 7);
        assert_eq!(wearables.len(), 2);
        let shape = wearables.first().ok_or("first wearable")?;
        assert_eq!(shape.item_id, shape_item);
        assert_eq!(shape.asset_id, shape_asset);
        assert_eq!(shape.wearable_type, WearableType::Shape);
        assert!(shape.wearable_type.is_body_part());
        let shirt = wearables.get(1).ok_or("second wearable")?;
        assert_eq!(shirt.wearable_type, WearableType::Shirt);
        assert!(!shirt.wearable_type.is_body_part());
        Ok(())
    }

    #[test]
    fn avatar_animation_surfaces_playing_animations() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let avatar = uuid::Uuid::from_u128(0xA1);
        let walk = uuid::Uuid::from_u128(0x100);
        let scripted = uuid::Uuid::from_u128(0x200);
        let trigger_object = uuid::Uuid::from_u128(0x300);

        // Two animations: the second one is triggered by an object, named in the
        // (positionally-correlated) AnimationSourceList. The first source slot is
        // nil, so the first animation has no source.
        let message = AnyMessage::AvatarAnimation(AvatarAnimation {
            sender: AvatarAnimationSenderBlock { id: avatar },
            animation_list: vec![
                AvatarAnimationAnimationListBlock {
                    anim_id: walk,
                    anim_sequence_id: 1,
                },
                AvatarAnimationAnimationListBlock {
                    anim_id: scripted,
                    anim_sequence_id: 2,
                },
            ],
            animation_source_list: vec![
                AvatarAnimationAnimationSourceListBlock {
                    object_id: uuid::Uuid::nil(),
                },
                AvatarAnimationAnimationSourceListBlock {
                    object_id: trigger_object,
                },
            ],
            physical_avatar_event_list: Vec::new(),
        });
        session.handle_datagram(sim_addr(), &server_message(&message, 9, true)?, now)?;

        let (avatar_id, animations) = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::AvatarAnimation {
                    avatar_id,
                    animations,
                } => Some((avatar_id, animations)),
                _ => None,
            })
            .ok_or("expected an AvatarAnimation event")?;
        assert_eq!(avatar_id, avatar);
        assert_eq!(animations.len(), 2);
        let first = animations.first().ok_or("first animation")?;
        assert_eq!(first.anim_id, walk);
        assert_eq!(first.sequence_id, 1);
        // A nil source UUID is still a populated source slot; only a *missing*
        // slot decodes to `None`. The viewer treats nil as "no triggering object".
        assert_eq!(first.source_id, Some(uuid::Uuid::nil()));
        let second = animations.get(1).ok_or("second animation")?;
        assert_eq!(second.anim_id, scripted);
        assert_eq!(second.sequence_id, 2);
        assert_eq!(second.source_id, Some(trigger_object));
        Ok(())
    }

    #[test]
    fn sound_trigger_surfaces_spatial_sound() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let sound = uuid::Uuid::from_u128(0x50);
        let owner = uuid::Uuid::from_u128(0x51);
        let object = uuid::Uuid::from_u128(0x52);

        // A SoundTrigger with a nil ParentID (the object is itself the root),
        // which must surface as `parent_id: None`.
        let message = AnyMessage::SoundTrigger(SoundTrigger {
            sound_data: SoundTriggerSoundDataBlock {
                sound_id: sound,
                owner_id: owner,
                object_id: object,
                parent_id: uuid::Uuid::nil(),
                handle: 0x0000_03E8_0000_03E8,
                position: vec3(128.0, 64.0, 25.0),
                gain: 0.5,
            },
        });
        session.handle_datagram(sim_addr(), &server_message(&message, 9, true)?, now)?;

        let event = drain_events(&mut session)
            .into_iter()
            .find(|event| matches!(event, Event::SoundTrigger { .. }))
            .ok_or("expected a SoundTrigger event")?;
        let Event::SoundTrigger {
            sound_id,
            owner_id,
            object_id,
            parent_id,
            region_handle,
            position,
            gain,
        } = event
        else {
            return Err("expected SoundTrigger".into());
        };
        assert_eq!(sound_id, sound);
        assert_eq!(owner_id, owner);
        assert_eq!(object_id, object);
        assert_eq!(parent_id, None);
        assert_eq!(region_handle, 0x0000_03E8_0000_03E8);
        assert_eq!(position, vec3(128.0, 64.0, 25.0));
        assert!((gain - 0.5).abs() < f32::EPSILON);
        Ok(())
    }

    #[test]
    fn gltf_material_override_surfaces_raw_faces() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // A GLTF material override (GenericStreamingMessage method 0x4175):
        // object local id 7, overrides on faces 0 and 2, each per-face override
        // a raw notation document left undecoded.
        let payload = b"{'id':i7,'te':[i0,i2],'od':[{'bc':[r1,r0,r0,r1]},{'mf':r0.25}]}";
        let message = AnyMessage::GenericStreamingMessage(GenericStreamingMessage {
            method_data: GenericStreamingMessageMethodDataBlock { method: 0x4175 },
            data_block: GenericStreamingMessageDataBlockBlock {
                data: payload.to_vec(),
            },
        });
        session.handle_datagram(sim_addr(), &server_message(&message, 9, true)?, now)?;

        let event = drain_events(&mut session)
            .into_iter()
            .find(|event| matches!(event, Event::GltfMaterialOverride { .. }))
            .ok_or("expected a GltfMaterialOverride event")?;
        let Event::GltfMaterialOverride {
            local_id,
            faces,
            overrides,
            ..
        } = event
        else {
            return Err("expected GltfMaterialOverride".into());
        };
        assert_eq!(local_id, 7);
        assert_eq!(faces, vec![0, 2]);
        assert_eq!(
            overrides,
            vec![b"{'bc':[r1,r0,r0,r1]}".to_vec(), b"{'mf':r0.25}".to_vec()]
        );
        Ok(())
    }

    #[test]
    fn modify_material_params_reply_surfaces_result() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // The `{ success, message }` reply to a ModifyMaterialParams POST.
        let xml = "<llsd><map><key>success</key><boolean>true</boolean>\
            <key>message</key><string></string></map></llsd>";
        let body = parse_llsd_xml(xml)?;
        session.handle_caps_event("ModifyMaterialParams", &body, now)?;

        let event = drain_events(&mut session)
            .into_iter()
            .find(|event| matches!(event, Event::MaterialParamsResult { .. }))
            .ok_or("expected a MaterialParamsResult event")?;
        let Event::MaterialParamsResult { success, message } = event else {
            return Err("expected MaterialParamsResult".into());
        };
        assert!(success);
        assert_eq!(message, "");
        Ok(())
    }

    #[test]
    fn parcel_media_command_surfaces_command() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // A scripted `llParcelMediaCommandList([PARCEL_MEDIA_COMMAND_TIME, 12.5])`:
        // command 6 (TIME) with the seek offset in `Time`, flags marking the
        // TIME field meaningful.
        let message = AnyMessage::ParcelMediaCommandMessage(ParcelMediaCommandMessage {
            command_block: ParcelMediaCommandMessageCommandBlockBlock {
                flags: 1 << 6,
                command: 6,
                time: 12.5,
            },
        });
        session.handle_datagram(sim_addr(), &server_message(&message, 9, true)?, now)?;

        let event = drain_events(&mut session)
            .into_iter()
            .find(|event| matches!(event, Event::ParcelMediaCommand { .. }))
            .ok_or("expected a ParcelMediaCommand event")?;
        let Event::ParcelMediaCommand {
            flags,
            command,
            time,
        } = event
        else {
            return Err("expected ParcelMediaCommand".into());
        };
        assert_eq!(flags, 1 << 6);
        assert_eq!(command, ParcelMediaCommand::Time);
        assert!((time - 12.5).abs() < f32::EPSILON);
        Ok(())
    }

    #[test]
    fn parcel_media_update_surfaces_settings() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let media = uuid::Uuid::from_u128(0x33ED);
        let message = AnyMessage::ParcelMediaUpdate(ParcelMediaUpdate {
            data_block: ParcelMediaUpdateDataBlockBlock {
                // The wire strings are NUL-terminated.
                media_url: b"http://example.com/movie\0".to_vec(),
                media_id: media,
                media_auto_scale: 1,
            },
            data_block_extended: ParcelMediaUpdateDataBlockExtendedBlock {
                media_type: b"text/html\0".to_vec(),
                media_desc: b"a web page\0".to_vec(),
                media_width: 1024,
                media_height: 768,
                media_loop: 1,
            },
        });
        session.handle_datagram(sim_addr(), &server_message(&message, 9, true)?, now)?;

        let event = drain_events(&mut session)
            .into_iter()
            .find(|event| matches!(event, Event::ParcelMediaUpdate(_)))
            .ok_or("expected a ParcelMediaUpdate event")?;
        let Event::ParcelMediaUpdate(update) = event else {
            return Err("expected ParcelMediaUpdate".into());
        };
        assert_eq!(update.media_url, "http://example.com/movie");
        assert_eq!(update.media_id, media);
        assert!(update.media_auto_scale);
        assert_eq!(update.media_type, "text/html");
        assert_eq!(update.media_desc, "a web page");
        assert_eq!(update.media_width, 1024);
        assert_eq!(update.media_height, 768);
        assert!(update.media_loop);
        Ok(())
    }

    #[test]
    fn object_media_caps_surfaces_per_face_media() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // An `ObjectMedia` GET reply: a two-face object with media on face 0
        // (a map) and none on face 1 (an LLSD undef).
        let object = uuid::Uuid::from_u128(0x000b_1ec7);
        let xml = format!(
            "<llsd><map>\
             <key>object_id</key><uuid>{object}</uuid>\
             <key>object_media_version</key><string>x-mv:0000000003/{object}</string>\
             <key>object_media_data</key><array>\
             <map>\
             <key>current_url</key><string>http://example.com/stream</string>\
             <key>home_url</key><string>http://example.com/home</string>\
             <key>auto_play</key><boolean>1</boolean>\
             <key>width_pixels</key><integer>1024</integer>\
             <key>height_pixels</key><integer>512</integer>\
             <key>controls</key><integer>1</integer>\
             <key>perms_interact</key><integer>1</integer>\
             </map>\
             <undef />\
             </array></map></llsd>"
        );
        let body = sl_proto::parse_llsd_xml(&xml)?;
        session.handle_caps_event("ObjectMedia", &body, now)?;

        let event = drain_events(&mut session)
            .into_iter()
            .find(|event| matches!(event, Event::ObjectMedia { .. }))
            .ok_or("expected an ObjectMedia event")?;
        let Event::ObjectMedia {
            object_id,
            version,
            faces,
        } = event
        else {
            return Err("expected ObjectMedia".into());
        };
        assert_eq!(object_id, object);
        assert_eq!(version, format!("x-mv:0000000003/{object}"));
        assert_eq!(faces.len(), 2);
        let face0 = faces
            .first()
            .ok_or("face 0")?
            .as_ref()
            .ok_or("face 0 media")?;
        assert_eq!(face0.current_url, "http://example.com/stream");
        assert_eq!(face0.home_url, "http://example.com/home");
        assert!(face0.auto_play);
        assert_eq!(face0.width_pixels, 1024);
        assert_eq!(face0.height_pixels, 512);
        assert_eq!(face0.controls, 1);
        assert_eq!(face0.perms_interact, 1);
        // A field absent from the LLSD falls back to the viewer default.
        assert_eq!(face0.perms_control, sl_proto::MEDIA_PERM_ALL);
        assert_eq!(faces.get(1).ok_or("face 1")?, &None);
        Ok(())
    }

    #[test]
    fn attached_sound_surfaces_flags() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let sound = uuid::Uuid::from_u128(0x60);
        let object = uuid::Uuid::from_u128(0x61);
        let owner = uuid::Uuid::from_u128(0x62);

        let message = AnyMessage::AttachedSound(AttachedSound {
            data_block: AttachedSoundDataBlockBlock {
                sound_id: sound,
                object_id: object,
                owner_id: owner,
                gain: 1.0,
                flags: SoundFlags::LOOP,
            },
        });
        session.handle_datagram(sim_addr(), &server_message(&message, 9, true)?, now)?;

        let event = drain_events(&mut session)
            .into_iter()
            .find(|event| matches!(event, Event::AttachedSound { .. }))
            .ok_or("expected an AttachedSound event")?;
        let Event::AttachedSound {
            sound_id,
            object_id,
            owner_id,
            gain,
            flags,
        } = event
        else {
            return Err("expected AttachedSound".into());
        };
        assert_eq!(sound_id, sound);
        assert_eq!(object_id, object);
        assert_eq!(owner_id, owner);
        assert!((gain - 1.0).abs() < f32::EPSILON);
        assert!(flags.is_loop());
        assert!(!flags.is_stop());
        Ok(())
    }

    #[test]
    fn preload_sound_surfaces_all_entries() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let message = AnyMessage::PreloadSound(PreloadSound {
            data_block: vec![
                PreloadSoundDataBlockBlock {
                    object_id: uuid::Uuid::from_u128(0x71),
                    owner_id: uuid::Uuid::from_u128(0x72),
                    sound_id: uuid::Uuid::from_u128(0x73),
                },
                PreloadSoundDataBlockBlock {
                    object_id: uuid::Uuid::from_u128(0x81),
                    owner_id: uuid::Uuid::from_u128(0x82),
                    sound_id: uuid::Uuid::from_u128(0x83),
                },
            ],
        });
        session.handle_datagram(sim_addr(), &server_message(&message, 9, true)?, now)?;

        let event = drain_events(&mut session)
            .into_iter()
            .find(|event| matches!(event, Event::PreloadSound { .. }))
            .ok_or("expected a PreloadSound event")?;
        let Event::PreloadSound { sounds } = event else {
            return Err("expected PreloadSound".into());
        };
        assert_eq!(sounds.len(), 2);
        let first = sounds.first().ok_or("first preload")?;
        assert_eq!(first.sound_id, uuid::Uuid::from_u128(0x73));
        assert_eq!(first.object_id, uuid::Uuid::from_u128(0x71));
        let second = sounds.get(1).ok_or("second preload")?;
        assert_eq!(second.sound_id, uuid::Uuid::from_u128(0x83));
        Ok(())
    }

    #[test]
    fn set_animations_sends_agent_animation() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let start = uuid::Uuid::from_u128(0x100);
        let stop = uuid::Uuid::from_u128(0x200);
        session.set_animations(&[(start, true), (stop, false)], now)?;
        let sent = drain(&mut session)?;
        let animation = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::AgentAnimation(animation) => Some(animation),
                _ => None,
            })
            .ok_or("expected an AgentAnimation")?;
        let first = animation.animation_list.first().ok_or("first anim")?;
        assert_eq!(first.anim_id, start);
        assert!(first.start_anim);
        let second = animation.animation_list.get(1).ok_or("second anim")?;
        assert_eq!(second.anim_id, stop);
        assert!(!second.start_anim);
        // The reference viewer always appends a single empty PhysicalAvatarEventList
        // block; some simulators reject the message without it.
        assert_eq!(animation.physical_avatar_event_list.len(), 1);
        assert!(
            animation
                .physical_avatar_event_list
                .first()
                .ok_or("event block")?
                .type_data
                .is_empty()
        );
        Ok(())
    }

    #[test]
    fn play_animation_starts_single_animation() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let dance = uuid::Uuid::from_u128(0x300);
        session.play_animation(dance, now)?;
        let sent = drain(&mut session)?;
        let animation = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::AgentAnimation(animation) => Some(animation),
                _ => None,
            })
            .ok_or("expected an AgentAnimation")?;
        assert_eq!(animation.animation_list.len(), 1);
        let block = animation.animation_list.first().ok_or("anim block")?;
        assert_eq!(block.anim_id, dance);
        assert!(block.start_anim);
        Ok(())
    }

    #[test]
    fn use_cached_mute_list_surfaces_event() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let message = AnyMessage::UseCachedMuteList(UseCachedMuteList {
            agent_data: UseCachedMuteListAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(1),
            },
        });
        session.handle_datagram(sim_addr(), &server_message(&message, 9, true)?, now)?;
        assert!(
            drain_events(&mut session)
                .iter()
                .any(|e| matches!(e, Event::MuteListUnchanged))
        );
        Ok(())
    }

    #[test]
    fn empty_mute_list_generic_message_surfaces_event() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let message = AnyMessage::GenericMessage(GenericMessage {
            agent_data: GenericMessageAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(1),
                session_id: uuid::Uuid::from_u128(2),
                transaction_id: uuid::Uuid::nil(),
            },
            method_data: GenericMessageMethodDataBlock {
                // NUL-terminated, as the simulator sends it on the wire.
                method: b"emptymutelist\0".to_vec(),
                invoice: uuid::Uuid::nil(),
            },
            param_list: Vec::new(),
        });
        session.handle_datagram(sim_addr(), &server_message(&message, 9, true)?, now)?;
        let entries = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::MuteList(entries) => Some(entries),
                _ => None,
            })
            .ok_or("expected a MuteList event")?;
        assert!(entries.is_empty());
        Ok(())
    }

    #[test]
    fn login_skeleton_emits_inventory_skeleton() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = new_session();
        let root = uuid::Uuid::from_u128(0xF0);
        let login = LoginResponse::Success(Box::new(LoginSuccess {
            agent_id: uuid::Uuid::from_u128(1),
            session_id: uuid::Uuid::from_u128(2),
            secure_session_id: uuid::Uuid::from_u128(3),
            circuit_code: 0x0011_2233,
            sim_ip: Ipv4Addr::new(127, 0, 0, 1),
            sim_port: 9000,
            seed_capability: "http://127.0.0.1:9000/seed".to_owned(),
            message: None,
            mfa_hash: None,
            inventory_root: Some(root),
            inventory_skeleton: vec![
                SkeletonFolder {
                    folder_id: root,
                    parent_id: uuid::Uuid::nil(),
                    name: "My Inventory".to_owned(),
                    type_default: 8,
                    version: 5,
                },
                SkeletonFolder {
                    folder_id: uuid::Uuid::from_u128(0xF1),
                    parent_id: root,
                    name: "Objects".to_owned(),
                    type_default: 6,
                    version: 2,
                },
            ],
            buddy_list: Vec::new(),
        }));
        session.handle_login_response(login, now)?;

        assert_eq!(session.inventory_root(), Some(root));
        let folders = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::InventorySkeleton(folders) => Some(folders),
                _ => None,
            })
            .ok_or("expected an InventorySkeleton event")?;
        assert_eq!(folders.len(), 2);
        let first = folders.first().ok_or("root folder")?;
        assert_eq!(first.name, "My Inventory");
        assert_eq!(first.folder_id, root);
        assert_eq!(folders.get(1).ok_or("second folder")?.parent_id, root);
        Ok(())
    }

    #[test]
    fn request_folder_contents_packs_fetch() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let folder = uuid::Uuid::from_u128(0xF0);
        session.request_folder_contents(folder, now)?;
        let sent = drain(&mut session)?;
        let fetch = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::FetchInventoryDescendents(fetch) => Some(fetch),
                _ => None,
            })
            .ok_or("expected a FetchInventoryDescendents")?;
        assert_eq!(fetch.inventory_data.folder_id, folder);
        assert!(fetch.inventory_data.fetch_folders);
        assert!(fetch.inventory_data.fetch_items);
        Ok(())
    }

    #[test]
    fn inventory_descendents_surfaces_event() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let folder = uuid::Uuid::from_u128(0xF0);
        let reply = AnyMessage::InventoryDescendents(InventoryDescendents {
            agent_data: InventoryDescendentsAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(1),
                folder_id: folder,
                owner_id: uuid::Uuid::from_u128(1),
                version: 7,
                descendents: 2,
            },
            folder_data: vec![InventoryDescendentsFolderDataBlock {
                folder_id: uuid::Uuid::from_u128(0xF2),
                parent_id: folder,
                r#type: 6,
                name: b"Clothing\0".to_vec(),
            }],
            item_data: vec![InventoryDescendentsItemDataBlock {
                item_id: uuid::Uuid::from_u128(0xD1),
                folder_id: folder,
                creator_id: uuid::Uuid::from_u128(0xC1),
                owner_id: uuid::Uuid::from_u128(1),
                group_id: uuid::Uuid::nil(),
                base_mask: 0x7FFF_FFFF,
                owner_mask: 0x7FFF_FFFF,
                group_mask: 0,
                everyone_mask: 0,
                next_owner_mask: 0x0008_2000,
                group_owned: false,
                asset_id: uuid::Uuid::from_u128(0xA1),
                r#type: 5,
                inv_type: 7,
                flags: 0,
                sale_type: 0,
                sale_price: 0,
                name: b"a notecard\0".to_vec(),
                description: b"2008-01-01\0".to_vec(),
                creation_date: 1_200_000_000,
                crc: 0,
            }],
        });
        let datagram = server_message(&reply, 9, false)?;
        session.handle_datagram(sim_addr(), &datagram, now)?;

        let (folders, items) = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::InventoryDescendents {
                    folder_id,
                    folders,
                    items,
                    ..
                } if folder_id == folder => Some((folders, items)),
                _ => None,
            })
            .ok_or("expected an InventoryDescendents event")?;
        assert_eq!(folders.len(), 1);
        assert_eq!(folders.first().ok_or("folder")?.name, "Clothing");
        assert_eq!(items.len(), 1);
        let item = items.first().ok_or("item")?;
        assert_eq!(item.name, "a notecard");
        assert_eq!(item.asset_id, uuid::Uuid::from_u128(0xA1));
        assert_eq!(item.inv_type, 7);
        Ok(())
    }

    #[test]
    fn caps_inventory_response_surfaces_event() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // A FetchInventoryDescendents2 CAPS response: one folder with a
        // sub-category and one item (with nested permissions and sale_info).
        let xml = r"<llsd><map><key>folders</key><array><map>
            <key>folder_id</key><uuid>00000000-0000-0000-0000-0000000000f0</uuid>
            <key>version</key><integer>7</integer>
            <key>descendents</key><integer>2</integer>
            <key>categories</key><array><map>
                <key>category_id</key><uuid>00000000-0000-0000-0000-0000000000f2</uuid>
                <key>parent_id</key><uuid>00000000-0000-0000-0000-0000000000f0</uuid>
                <key>name</key><string>Clothing</string>
                <key>type_default</key><integer>5</integer>
                <key>version</key><integer>1</integer>
            </map></array>
            <key>items</key><array><map>
                <key>item_id</key><uuid>00000000-0000-0000-0000-0000000000d1</uuid>
                <key>parent_id</key><uuid>00000000-0000-0000-0000-0000000000f0</uuid>
                <key>name</key><string>a notecard</string>
                <key>desc</key><string>my notes</string>
                <key>asset_id</key><uuid>00000000-0000-0000-0000-0000000000a1</uuid>
                <key>type</key><integer>7</integer>
                <key>inv_type</key><integer>7</integer>
                <key>flags</key><integer>0</integer>
                <key>created_at</key><integer>1200000000</integer>
                <key>sale_info</key><map><key>sale_price</key><integer>0</integer><key>sale_type</key><integer>0</integer></map>
                <key>permissions</key><map>
                    <key>creator_id</key><uuid>00000000-0000-0000-0000-0000000000c1</uuid>
                    <key>owner_id</key><uuid>00000000-0000-0000-0000-000000000001</uuid>
                    <key>group_id</key><uuid>00000000-0000-0000-0000-000000000000</uuid>
                    <key>base_mask</key><integer>2147483647</integer>
                    <key>owner_mask</key><integer>2147483647</integer>
                    <key>group_mask</key><integer>0</integer>
                    <key>everyone_mask</key><integer>0</integer>
                    <key>next_owner_mask</key><integer>532480</integer>
                    <key>is_owner_group</key><boolean>0</boolean>
                </map>
            </map></array>
        </map></array></map></llsd>";
        let body = parse_llsd_xml(xml)?;
        session.handle_caps_event("FetchInventoryDescendents2", &body, now)?;

        let (folders, items) = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::InventoryDescendents {
                    folder_id,
                    version,
                    folders,
                    items,
                    ..
                } if folder_id == uuid::Uuid::from_u128(0xF0) && version == 7 => {
                    Some((folders, items))
                }
                _ => None,
            })
            .ok_or("expected an InventoryDescendents event")?;
        let folder = folders.first().ok_or("category")?;
        assert_eq!(folder.name, "Clothing");
        assert_eq!(folder.folder_id, uuid::Uuid::from_u128(0xF2));
        let item = items.first().ok_or("item")?;
        assert_eq!(item.name, "a notecard");
        assert_eq!(item.description, "my notes");
        assert_eq!(item.asset_id, uuid::Uuid::from_u128(0xA1));
        assert_eq!(item.creator_id, uuid::Uuid::from_u128(0xC1));
        assert_eq!(item.inv_type, 7);
        assert_eq!(item.base_mask, 0x7FFF_FFFF);
        assert_eq!(item.next_owner_mask, 532_480);
        Ok(())
    }

    /// A short Vector constructor for test fixtures.
    fn vec3(x: f32, y: f32, z: f32) -> Vector {
        Vector { x, y, z }
    }

    /// Drives a session to the awaiting-handshake state (login answered, but no
    /// `RegionHandshake` received yet), draining the bootstrap traffic/events.
    fn awaiting_handshake(now: Instant) -> Result<Session, TestError> {
        let mut session = new_session();
        session.handle_login_response(success(), now)?;
        drain(&mut session)?;
        drain_events(&mut session);
        Ok(session)
    }

    /// Builds a `RegionHandshake` with the given identity-bearing fields and
    /// zeroes for everything else.
    fn region_handshake_msg(
        sim_access: u8,
        region_flags: u32,
        sim_name: &str,
        product_sku: &str,
        product_name: &str,
    ) -> AnyMessage {
        let nil = uuid::Uuid::nil();
        AnyMessage::RegionHandshake(RegionHandshake {
            region_info: RegionHandshakeRegionInfoBlock {
                region_flags,
                sim_access,
                sim_name: sim_name.as_bytes().to_vec(),
                sim_owner: nil,
                is_estate_manager: false,
                water_height: 0.0,
                billable_factor: 0.0,
                cache_id: nil,
                terrain_base0: nil,
                terrain_base1: nil,
                terrain_base2: nil,
                terrain_base3: nil,
                terrain_detail0: nil,
                terrain_detail1: nil,
                terrain_detail2: nil,
                terrain_detail3: nil,
                terrain_start_height00: 0.0,
                terrain_start_height01: 0.0,
                terrain_start_height10: 0.0,
                terrain_start_height11: 0.0,
                terrain_height_range00: 0.0,
                terrain_height_range01: 0.0,
                terrain_height_range10: 0.0,
                terrain_height_range11: 0.0,
            },
            region_info2: RegionHandshakeRegionInfo2Block { region_id: nil },
            region_info3: RegionHandshakeRegionInfo3Block {
                cpu_class_id: 0,
                cpu_ratio: 0,
                colo_name: Vec::new(),
                product_sku: product_sku.as_bytes().to_vec(),
                product_name: product_name.as_bytes().to_vec(),
            },
            region_info4: Vec::new(),
        })
    }

    /// Builds a `RegionInfo` carrying the given limits and zeroes elsewhere.
    fn region_info_msg(
        sim_name: &str,
        sim_access: u8,
        max_agents: u8,
        max_agents32: u32,
        hard_max_agents: u32,
        hard_max_objects: u32,
    ) -> AnyMessage {
        let nil = uuid::Uuid::nil();
        AnyMessage::RegionInfo(RegionInfo {
            agent_data: RegionInfoAgentDataBlock {
                agent_id: nil,
                session_id: nil,
            },
            region_info: RegionInfoRegionInfoBlock {
                sim_name: sim_name.as_bytes().to_vec(),
                estate_id: 0,
                parent_estate_id: 0,
                region_flags: 0,
                sim_access,
                max_agents,
                billable_factor: 0.0,
                object_bonus_factor: 0.0,
                water_height: 0.0,
                terrain_raise_limit: 0.0,
                terrain_lower_limit: 0.0,
                price_per_meter: 0,
                redirect_grid_x: 0,
                redirect_grid_y: 0,
                use_estate_sun: false,
                sun_hour: 0.0,
            },
            region_info2: RegionInfoRegionInfo2Block {
                product_sku: Vec::new(),
                product_name: Vec::new(),
                max_agents32,
                hard_max_agents,
                hard_max_objects,
            },
            region_info3: Vec::new(),
            region_info5: Vec::new(),
            combat_settings: Vec::new(),
        })
    }

    /// Builds a `ParcelProperties` with the given survey-relevant fields and a
    /// full-region coverage bitmap.
    fn parcel_properties_msg(
        sequence_id: i32,
        local_id: i32,
        area: i32,
        parcel_flags: u32,
        max_prims: i32,
        sim_wide_max_prims: i32,
        aabb_max: Vector,
    ) -> AnyMessage {
        let nil = uuid::Uuid::nil();
        AnyMessage::ParcelProperties(ParcelProperties {
            parcel_data: ParcelPropertiesParcelDataBlock {
                request_result: 0,
                sequence_id,
                snap_selection: false,
                self_count: 0,
                other_count: 0,
                public_count: 0,
                local_id,
                owner_id: nil,
                is_group_owned: false,
                auction_id: 0,
                claim_date: 0,
                claim_price: 0,
                rent_price: 0,
                aabb_min: vec3(0.0, 0.0, 0.0),
                aabb_max,
                bitmap: vec![0xFF; 512],
                area,
                status: 0,
                sim_wide_max_prims,
                sim_wide_total_prims: 0,
                max_prims,
                total_prims: 0,
                owner_prims: 0,
                group_prims: 0,
                other_prims: 0,
                selected_prims: 0,
                parcel_prim_bonus: 0.0,
                other_clean_time: 0,
                parcel_flags,
                sale_price: 0,
                name: Vec::new(),
                desc: Vec::new(),
                music_url: Vec::new(),
                media_url: Vec::new(),
                media_id: nil,
                media_auto_scale: 0,
                group_id: nil,
                pass_price: 0,
                pass_hours: 0.0,
                category: 0,
                auth_buyer_id: nil,
                snapshot_id: nil,
                user_location: vec3(0.0, 0.0, 0.0),
                user_look_at: vec3(0.0, 0.0, 0.0),
                landing_type: 0,
                region_push_override: false,
                region_deny_anonymous: false,
                region_deny_identified: false,
                region_deny_transacted: false,
            },
            age_verification_block: ParcelPropertiesAgeVerificationBlockBlock {
                region_deny_age_unverified: false,
            },
            region_allow_access_block: ParcelPropertiesRegionAllowAccessBlockBlock {
                region_allow_access_override: false,
            },
            parcel_environment_block: ParcelPropertiesParcelEnvironmentBlockBlock {
                parcel_environment_version: 0,
                region_allow_environment_override: false,
            },
        })
    }

    #[test]
    fn region_handshake_reports_identity() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = awaiting_handshake(now)?;

        let handshake = server_message(
            &region_handshake_msg(21, 0x40, "TestRegion", "", "Homestead"),
            1,
            true,
        )?;
        session.handle_datagram(sim_addr(), &handshake, now)?;

        let events = drain_events(&mut session);
        let identity = events
            .iter()
            .find_map(|e| match e {
                Event::RegionInfoHandshake(identity) => Some(identity),
                _ => None,
            })
            .ok_or("expected a RegionInfoHandshake event")?;
        assert_eq!(identity.sim_name, "TestRegion");
        assert_eq!(identity.maturity, Maturity::Mature);
        assert_eq!(identity.product, ProductType::Homestead);
        assert_eq!(identity.region_flags, 0x40);
        Ok(())
    }

    #[test]
    fn region_info_reports_limits_with_agent_fallback() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        // max_agents32 == 0 must fall back to the legacy 8-bit field (40).
        let info = server_message(&region_info_msg("TestRegion", 13, 40, 0, 0, 15000), 9, true)?;
        session.handle_datagram(sim_addr(), &info, now)?;

        let events = drain_events(&mut session);
        let limits = events
            .iter()
            .find_map(|e| match e {
                Event::RegionLimits(limits) => Some(limits),
                _ => None,
            })
            .ok_or("expected a RegionLimits event")?;
        assert_eq!(limits.max_agents, 40);
        assert_eq!(limits.hard_max_objects, 15000);
        assert_eq!(limits.maturity, Maturity::Pg);
        Ok(())
    }

    #[test]
    fn parcel_properties_reports_geometry_and_flags() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        // CREATE_OBJECTS (1<<6) | USE_BAN_LIST (1<<10).
        let flags = (1 << 6) | (1 << 10);
        let props = server_message(
            &parcel_properties_msg(42, 7, 4096, flags, 1000, 5000, vec3(64.0, 64.0, 0.0)),
            9,
            true,
        )?;
        session.handle_datagram(sim_addr(), &props, now)?;

        let events = drain_events(&mut session);
        let parcel = events
            .iter()
            .find_map(|e| match e {
                Event::ParcelProperties(parcel) => Some(parcel),
                _ => None,
            })
            .ok_or("expected a ParcelProperties event")?;
        assert_eq!(parcel.sequence_id, 42);
        assert_eq!(parcel.local_id, 7);
        assert_eq!(parcel.area, 4096);
        assert_eq!(parcel.aabb_max.0.to_bits(), 64.0_f32.to_bits());
        assert_eq!(parcel.aabb_max.1.to_bits(), 64.0_f32.to_bits());
        assert_eq!(parcel.aabb_max.2.to_bits(), 0.0_f32.to_bits());
        assert_eq!(parcel.max_prims, 1000);
        assert_eq!(parcel.sim_wide_max_prims, 5000);
        assert_eq!(parcel.bitmap.len(), 512);
        assert!(parcel.create_objects());
        assert!(parcel.use_ban_list());
        assert!(!parcel.use_access_list());
        // Absent media → empty URLs / nil id / no auto-scale.
        assert_eq!(parcel.music_url, "");
        assert_eq!(parcel.media_url, "");
        assert_eq!(parcel.media_id, uuid::Uuid::nil());
        assert!(!parcel.media_auto_scale);
        Ok(())
    }

    #[test]
    fn parcel_properties_reports_stream_and_media_urls() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        let media_id = uuid::Uuid::from_u128(0x33ED);
        let mut message = parcel_properties_msg(1, 1, 256, 0, 100, 100, vec3(16.0, 16.0, 0.0));
        if let AnyMessage::ParcelProperties(props) = &mut message {
            props.parcel_data.music_url = b"http://stream.example/audio".to_vec();
            // A trailing NUL like a real simulator sends; the decode must trim it.
            props.parcel_data.media_url = b"http://example.com/movie\0".to_vec();
            props.parcel_data.media_id = media_id;
            props.parcel_data.media_auto_scale = 1;
        }
        session.handle_datagram(sim_addr(), &server_message(&message, 9, true)?, now)?;

        let events = drain_events(&mut session);
        let parcel = events
            .iter()
            .find_map(|e| match e {
                Event::ParcelProperties(parcel) => Some(parcel),
                _ => None,
            })
            .ok_or("expected a ParcelProperties event")?;
        assert_eq!(parcel.music_url, "http://stream.example/audio");
        assert_eq!(parcel.media_url, "http://example.com/movie");
        assert_eq!(parcel.media_id, media_id);
        assert!(parcel.media_auto_scale);
        Ok(())
    }

    #[test]
    fn enable_simulator_reports_neighbor_with_host_port() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        // Hand-build the body so the IPPORT is in wire (big-endian) order: the
        // handle encodes global corner (256000, 256256) -> grid (1000, 1001),
        // and port 13000 (0x32C8) is written network-order [0x32, 0xC8].
        let mut body = Writer::new();
        body.put_u64(0x0003_E800_0003_E900);
        body.bytes(&[127, 0, 0, 1]);
        body.bytes(&[0x32, 0xC8]);
        let datagram = server_datagram(MessageId::Low(151), &body.into_bytes(), 9, true);
        session.handle_datagram(sim_addr(), &datagram, now)?;

        let events = drain_events(&mut session);
        let neighbor = events
            .iter()
            .find_map(|e| match e {
                Event::NeighborDiscovered(neighbor) => Some(neighbor),
                _ => None,
            })
            .ok_or("expected a NeighborDiscovered event")?;
        assert_eq!(neighbor.region_handle, 0x0003_E800_0003_E900);
        assert_eq!(neighbor.grid_x, 1000);
        assert_eq!(neighbor.grid_y, 1001);
        assert_eq!(neighbor.sim.port(), 13000);
        assert_eq!(neighbor.sim.ip(), IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)));
        Ok(())
    }

    /// Feeds an `EnableSimulator` for the neighbour at 127.0.0.1:9001 (`sim_b`),
    /// which opens a child-agent circuit there.
    fn enable_neighbour_b(
        session: &mut Session,
        sequence: u32,
        now: Instant,
    ) -> Result<(), TestError> {
        // Handle for grid (1001, 1000); port 9001 (0x2329) in network order.
        let mut body = Writer::new();
        body.put_u64(0x0003_E900_0003_E800);
        body.bytes(&[127, 0, 0, 1]);
        body.bytes(&[0x23, 0x29]);
        let datagram = server_datagram(MessageId::Low(151), &body.into_bytes(), sequence, true);
        session.handle_datagram(sim_addr(), &datagram, now)?;
        Ok(())
    }

    /// Drains transmits, returning the first one destined for `dst`.
    fn take_transmit_to(session: &mut Session, dst: SocketAddr) -> Option<AnyMessage> {
        while let Some(transmit) = session.poll_transmit() {
            if transmit.destination == dst {
                return decode(&transmit).ok();
            }
        }
        None
    }

    #[test]
    fn enable_simulator_opens_child_circuit() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        enable_neighbour_b(&mut session, 9, now)?;
        // A child UseCircuitCode (but no CompleteAgentMovement) goes to the
        // neighbour so it registers the child agent.
        let msg =
            take_transmit_to(&mut session, sim_b()).ok_or("expected a child UseCircuitCode")?;
        assert!(matches!(msg, AnyMessage::UseCircuitCode(_)));
        // Drain the rest of the open burst (an AgentUpdate drives the child agent
        // so the neighbour streams its objects).
        drain(&mut session)?;

        // A second EnableSimulator for the same neighbour is a no-op.
        enable_neighbour_b(&mut session, 10, now)?;
        assert!(
            take_transmit_to(&mut session, sim_b()).is_none(),
            "a child circuit should only be opened once per neighbour"
        );
        Ok(())
    }

    #[test]
    fn child_circuit_replies_to_ping_on_its_own_circuit() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        enable_neighbour_b(&mut session, 9, now)?;
        // Drain the child UseCircuitCode and any root traffic.
        while session.poll_transmit().is_some() {}

        // A ping from the child simulator is answered on the child's circuit.
        let ping = server_datagram(MessageId::High(1), &[0x2A, 0, 0, 0, 0], 2, false);
        session.handle_datagram(sim_b(), &ping, now)?;
        let reply =
            take_transmit_to(&mut session, sim_b()).ok_or("expected a ping reply to sim_b")?;
        let AnyMessage::CompletePingCheck(reply) = reply else {
            return Err("expected CompletePingCheck to the child".into());
        };
        assert_eq!(reply.ping_id.ping_id, 0x2A);
        Ok(())
    }

    #[test]
    fn crossed_region_promotes_child_to_root() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        // Pre-open the child circuit to the neighbour we will cross into.
        enable_neighbour_b(&mut session, 9, now)?;
        while session.poll_transmit().is_some() {}

        // The avatar walks across the border: the source region hands us the
        // destination's details (port in network order; the handler swaps it).
        let handle = 0x0003_E900_0003_E800;
        let crossed = AnyMessage::CrossedRegion(CrossedRegion {
            agent_data: CrossedRegionAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(1),
                session_id: uuid::Uuid::from_u128(2),
            },
            region_data: CrossedRegionRegionDataBlock {
                sim_ip: [127, 0, 0, 1],
                sim_port: 9001u16.swap_bytes(),
                region_handle: handle,
                seed_capability: b"http://x/seedB\0".to_vec(),
            },
            info: CrossedRegionInfoBlock {
                position: vec3(10.0, 128.0, 30.0),
                look_at: vec3(1.0, 0.0, 0.0),
            },
        });
        session.handle_datagram(sim_addr(), &server_message(&crossed, 10, true)?, now)?;

        // We promote the child to root by completing the agent movement there.
        let msg = take_transmit_to(&mut session, sim_b())
            .ok_or("expected a CompleteAgentMovement to sim_b")?;
        assert!(matches!(msg, AnyMessage::CompleteAgentMovement(_)));

        // The new root confirms; the crossing surfaces as a RegionChanged.
        let amc = AnyMessage::AgentMovementComplete(AgentMovementComplete {
            agent_data: AgentMovementCompleteAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(1),
                session_id: uuid::Uuid::from_u128(2),
            },
            data: AgentMovementCompleteDataBlock {
                position: vec3(10.0, 128.0, 30.0),
                look_at: vec3(1.0, 0.0, 0.0),
                region_handle: handle,
                timestamp: 0,
            },
            sim_data: AgentMovementCompleteSimDataBlock {
                channel_version: b"x\0".to_vec(),
            },
        });
        session.handle_datagram(sim_b(), &server_message(&amc, 1, true)?, now)?;
        let changed = drain_events(&mut session)
            .into_iter()
            .find_map(|e| match e {
                Event::RegionChanged { region_handle, sim } => Some((region_handle, sim)),
                _ => None,
            })
            .ok_or("expected a RegionChanged event")?;
        assert_eq!(changed.0, handle);
        assert_eq!(changed.1, sim_b());
        Ok(())
    }

    #[test]
    fn disable_simulator_retires_child_circuit() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        enable_neighbour_b(&mut session, 9, now)?;
        while session.poll_transmit().is_some() {}

        // The simulator retires the child circuit.
        let disable = server_datagram(MessageId::Low(152), &[], 3, true);
        session.handle_datagram(sim_b(), &disable, now)?;

        // A ping from that (now-closed) child is ignored — no reply.
        let ping = server_datagram(MessageId::High(1), &[0x2A, 0, 0, 0, 0], 4, false);
        session.handle_datagram(sim_b(), &ping, now)?;
        assert!(
            take_transmit_to(&mut session, sim_b()).is_none(),
            "a retired child circuit should not answer"
        );
        Ok(())
    }

    #[test]
    fn caps_enable_simulator_opens_child_circuit() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        // OpenSim (and Second Life) announce neighbours over the CAPS event queue,
        // not the UDP EnableSimulator. Handle AAPpAAAD6AA= is the U64
        // 0x0003_E900_0003_E800; IP fwAAAQ== is 127.0.0.1; Port is a plain integer.
        let body = sl_proto::parse_llsd_xml(
            "<llsd><map><key>SimulatorInfo</key><array><map>\
                <key>Handle</key><binary>AAPpAAAD6AA=</binary>\
                <key>IP</key><binary>fwAAAQ==</binary>\
                <key>Port</key><integer>9001</integer>\
                </map></array></map></llsd>",
        )?;
        session.handle_caps_event("EnableSimulator", &body, now)?;

        // The neighbour is surfaced and a child UseCircuitCode is sent to it.
        let neighbour = drain_events(&mut session)
            .into_iter()
            .find_map(|e| match e {
                Event::NeighborDiscovered(info) => Some(info),
                _ => None,
            })
            .ok_or("expected a NeighborDiscovered event")?;
        assert_eq!(neighbour.sim, sim_b());
        assert_eq!(neighbour.region_handle, 0x0003_E900_0003_E800);
        let msg =
            take_transmit_to(&mut session, sim_b()).ok_or("expected a child UseCircuitCode")?;
        assert!(matches!(msg, AnyMessage::UseCircuitCode(_)));
        Ok(())
    }

    #[test]
    fn caps_crossed_region_promotes_child_to_root() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        // Pre-open the child circuit to the neighbour we will cross into.
        enable_neighbour_b(&mut session, 9, now)?;
        while session.poll_transmit().is_some() {}

        // OpenSim signals the actual crossing over the CAPS event queue. Here the
        // SimPort is a plain integer (no byte swap, unlike the UDP CrossedRegion).
        let handle = 0x0003_E900_0003_E800u64;
        let body = sl_proto::parse_llsd_xml(
            "<llsd><map>\
                <key>AgentData</key><array><map>\
                    <key>AgentID</key><uuid>00000000-0000-0000-0000-000000000001</uuid>\
                    <key>SessionID</key><uuid>00000000-0000-0000-0000-000000000002</uuid>\
                    </map></array>\
                <key>Info</key><array><map>\
                    <key>LookAt</key><array><real>1</real><real>0</real><real>0</real></array>\
                    <key>Position</key><array><real>10</real><real>128</real><real>30</real></array>\
                    </map></array>\
                <key>RegionData</key><array><map>\
                    <key>RegionHandle</key><binary>AAPpAAAD6AA=</binary>\
                    <key>SeedCapability</key><string>http://127.0.0.1:9001/seedB</string>\
                    <key>SimIP</key><binary>fwAAAQ==</binary>\
                    <key>SimPort</key><integer>9001</integer>\
                    </map></array></map></llsd>",
        )?;
        session.handle_caps_event("CrossedRegion", &body, now)?;

        // We promote the child to root by completing the agent movement there.
        let msg = take_transmit_to(&mut session, sim_b())
            .ok_or("expected a CompleteAgentMovement to sim_b")?;
        assert!(matches!(msg, AnyMessage::CompleteAgentMovement(_)));

        // The new root confirms; the crossing surfaces as a RegionChanged.
        let amc = AnyMessage::AgentMovementComplete(AgentMovementComplete {
            agent_data: AgentMovementCompleteAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(1),
                session_id: uuid::Uuid::from_u128(2),
            },
            data: AgentMovementCompleteDataBlock {
                position: vec3(10.0, 128.0, 30.0),
                look_at: vec3(1.0, 0.0, 0.0),
                region_handle: handle,
                timestamp: 0,
            },
            sim_data: AgentMovementCompleteSimDataBlock {
                channel_version: b"x\0".to_vec(),
            },
        });
        session.handle_datagram(sim_b(), &server_message(&amc, 1, true)?, now)?;
        let changed = drain_events(&mut session)
            .into_iter()
            .find_map(|e| match e {
                Event::RegionChanged { region_handle, sim } => Some((region_handle, sim)),
                _ => None,
            })
            .ok_or("expected a RegionChanged event")?;
        assert_eq!(changed.0, handle);
        assert_eq!(changed.1, sim_b());
        Ok(())
    }

    #[test]
    fn request_helpers_inject_agent_ids() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        session.request_region_info(now)?;
        session.request_parcel_properties(0.0, 0.0, 64.0, 48.0, 77, now)?;
        let sent = drain(&mut session)?;

        let region = sent.iter().find_map(|m| match m {
            AnyMessage::RequestRegionInfo(request) => Some(request),
            _ => None,
        });
        let region = region.ok_or("expected a RequestRegionInfo")?;
        assert_eq!(region.agent_data.agent_id, uuid::Uuid::from_u128(1));
        assert_eq!(region.agent_data.session_id, uuid::Uuid::from_u128(2));

        let parcel = sent.iter().find_map(|m| match m {
            AnyMessage::ParcelPropertiesRequest(request) => Some(request),
            _ => None,
        });
        let parcel = parcel.ok_or("expected a ParcelPropertiesRequest")?;
        assert_eq!(parcel.agent_data.agent_id, uuid::Uuid::from_u128(1));
        assert_eq!(parcel.parcel_data.sequence_id, 77);
        assert_eq!(parcel.parcel_data.east.to_bits(), 64.0_f32.to_bits());
        assert_eq!(parcel.parcel_data.north.to_bits(), 48.0_f32.to_bits());
        Ok(())
    }

    #[test]
    fn money_requests_inject_agent_and_payment() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        session.request_money_balance(now)?;
        session.request_economy_data(now)?;
        session.send_money_transfer(
            uuid::Uuid::from_u128(0xABCD),
            LindenAmount(250),
            MoneyTransactionType::PayObject,
            "tip",
            now,
        )?;
        let sent = drain(&mut session)?;

        let balance = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::MoneyBalanceRequest(request) => Some(request),
                _ => None,
            })
            .ok_or("expected a MoneyBalanceRequest")?;
        assert_eq!(balance.agent_data.agent_id, uuid::Uuid::from_u128(1));
        assert_eq!(balance.agent_data.session_id, uuid::Uuid::from_u128(2));

        assert!(
            sent.iter()
                .any(|m| matches!(m, AnyMessage::EconomyDataRequest(_))),
            "expected an EconomyDataRequest"
        );

        let transfer = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::MoneyTransferRequest(request) => Some(request),
                _ => None,
            })
            .ok_or("expected a MoneyTransferRequest")?;
        // The source is the agent; the type/amount/description match the request.
        assert_eq!(transfer.money_data.source_id, uuid::Uuid::from_u128(1));
        assert_eq!(transfer.money_data.dest_id, uuid::Uuid::from_u128(0xABCD));
        assert_eq!(transfer.money_data.amount, 250);
        assert_eq!(transfer.money_data.transaction_type, 5008);
        assert_eq!(trimmed(&transfer.money_data.description), "tip");
        Ok(())
    }

    #[test]
    fn money_balance_reply_surfaces_balance() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        // A plain balance poll: the TransactionInfo block is all-zero.
        let reply = AnyMessage::MoneyBalanceReply(MoneyBalanceReply {
            money_data: MoneyBalanceReplyMoneyDataBlock {
                agent_id: uuid::Uuid::from_u128(1),
                transaction_id: uuid::Uuid::nil(),
                transaction_success: true,
                money_balance: 1234,
                square_meters_credit: 512,
                square_meters_committed: 128,
                description: Vec::new(),
            },
            transaction_info: MoneyBalanceReplyTransactionInfoBlock {
                transaction_type: 0,
                source_id: uuid::Uuid::nil(),
                is_source_group: false,
                dest_id: uuid::Uuid::nil(),
                is_dest_group: false,
                amount: 0,
                item_description: Vec::new(),
            },
        });
        session.handle_datagram(sim_addr(), &server_message(&reply, 9, true)?, now)?;

        let balance = drain_events(&mut session)
            .into_iter()
            .find_map(|e| match e {
                Event::MoneyBalance(balance) => Some(balance),
                _ => None,
            })
            .ok_or("expected a MoneyBalance event")?;
        assert_eq!(balance.agent_id, uuid::Uuid::from_u128(1));
        assert!(balance.success);
        assert_eq!(balance.balance, LindenAmount(1234));
        assert_eq!(balance.square_meters_credit, 512);
        assert_eq!(balance.square_meters_committed, 128);
        // A plain poll carries no transaction metadata.
        assert!(balance.transaction.is_none());
        Ok(())
    }

    #[test]
    fn money_balance_reply_surfaces_transaction_details() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        // A reply after a real payment carries a non-zero TransactionInfo block.
        let reply = AnyMessage::MoneyBalanceReply(MoneyBalanceReply {
            money_data: MoneyBalanceReplyMoneyDataBlock {
                agent_id: uuid::Uuid::from_u128(1),
                transaction_id: uuid::Uuid::nil(),
                transaction_success: true,
                money_balance: 750,
                square_meters_credit: 0,
                square_meters_committed: 0,
                description: b"Object: tip jar\0".to_vec(),
            },
            transaction_info: MoneyBalanceReplyTransactionInfoBlock {
                transaction_type: 5008,
                source_id: uuid::Uuid::from_u128(1),
                is_source_group: false,
                dest_id: uuid::Uuid::from_u128(0xBEEF),
                is_dest_group: false,
                amount: 250,
                item_description: b"tip jar\0".to_vec(),
            },
        });
        session.handle_datagram(sim_addr(), &server_message(&reply, 9, true)?, now)?;

        let balance = drain_events(&mut session)
            .into_iter()
            .find_map(|e| match e {
                Event::MoneyBalance(balance) => Some(balance),
                _ => None,
            })
            .ok_or("expected a MoneyBalance event")?;
        assert_eq!(balance.balance, LindenAmount(750));
        let transaction = balance.transaction.ok_or("expected transaction details")?;
        assert_eq!(
            MoneyTransactionType::from_i32(transaction.transaction_type),
            MoneyTransactionType::PayObject
        );
        assert_eq!(transaction.source_id, uuid::Uuid::from_u128(1));
        assert_eq!(transaction.dest_id, uuid::Uuid::from_u128(0xBEEF));
        assert_eq!(transaction.amount, LindenAmount(250));
        assert_eq!(transaction.item_description, "tip jar");
        Ok(())
    }

    #[test]
    fn economy_data_surfaces_prices() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        let data = AnyMessage::EconomyData(EconomyData {
            info: EconomyDataInfoBlock {
                object_capacity: 15000,
                object_count: 1200,
                price_energy_unit: 100,
                price_object_claim: 10,
                price_public_object_decay: 4,
                price_public_object_delete: 4,
                price_parcel_claim: 1,
                price_parcel_claim_factor: 1.0,
                price_upload: 0,
                price_rent_light: 5,
                teleport_min_price: 2,
                teleport_price_exponent: 2.0,
                energy_efficiency: 1.0,
                price_object_rent: 1.0,
                price_object_scale_factor: 10.0,
                price_parcel_rent: 1,
                price_group_create: 0,
            },
        });
        session.handle_datagram(sim_addr(), &server_message(&data, 9, true)?, now)?;

        let economy = drain_events(&mut session)
            .into_iter()
            .find_map(|e| match e {
                Event::EconomyData(data) => Some(data),
                _ => None,
            })
            .ok_or("expected an EconomyData event")?;
        assert_eq!(economy.object_capacity, 15000);
        assert_eq!(economy.price_upload, 0);
        assert_eq!(economy.price_energy_unit, 100);
        assert_eq!(economy.teleport_min_price, 2);
        Ok(())
    }

    #[test]
    fn map_name_and_item_requests_encode() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        session.request_map_by_name("East Region", now)?;
        session.request_map_items(MapItemType::AgentLocations, 0, now)?;
        let sent = drain(&mut session)?;

        let name = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::MapNameRequest(request) => Some(request),
                _ => None,
            })
            .ok_or("expected a MapNameRequest")?;
        assert_eq!(name.agent_data.agent_id, uuid::Uuid::from_u128(1));
        // The viewer's map-layer flag.
        assert_eq!(name.agent_data.flags, 2);
        assert_eq!(trimmed(&name.name_data.name), "East Region");

        let item = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::MapItemRequest(request) => Some(request),
                _ => None,
            })
            .ok_or("expected a MapItemRequest")?;
        assert_eq!(item.agent_data.flags, 2);
        // AgentLocations is grid item type 6; 0 targets the current region.
        assert_eq!(item.request_data.item_type, 6);
        assert_eq!(item.request_data.region_handle, 0);
        Ok(())
    }

    #[test]
    fn map_item_reply_surfaces_agent_locations() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        // Two avatar "green dots" in the region at grid (1000, 1000): global
        // origin 256000, plus in-region offsets.
        let reply = AnyMessage::MapItemReply(MapItemReply {
            agent_data: MapItemReplyAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(1),
                flags: 2,
            },
            request_data: MapItemReplyRequestDataBlock { item_type: 6 },
            data: vec![
                MapItemReplyDataBlock {
                    x: 256_000 + 128,
                    y: 256_000 + 64,
                    id: uuid::Uuid::nil(),
                    extra: 1,
                    extra2: 0,
                    name: b"hash\0".to_vec(),
                },
                MapItemReplyDataBlock {
                    x: 256_000 + 200,
                    y: 256_000 + 10,
                    id: uuid::Uuid::nil(),
                    extra: 1,
                    extra2: 0,
                    name: Vec::new(),
                },
            ],
        });
        session.handle_datagram(sim_addr(), &server_message(&reply, 9, true)?, now)?;

        let (item_type, items) = drain_events(&mut session)
            .into_iter()
            .find_map(|e| match e {
                Event::MapItems { item_type, items } => Some((item_type, items)),
                _ => None,
            })
            .ok_or("expected a MapItems event")?;
        assert_eq!(item_type, MapItemType::AgentLocations);
        assert_eq!(items.len(), 2);
        let first = items.first().ok_or("expected at least one item")?;
        assert_eq!(first.global_x, 256_128);
        assert_eq!(first.global_y, 256_064);
        // The region handle masks off the in-region offset; the locals recover it.
        assert_eq!(first.region_handle(), 0x0003_E800_0003_E800);
        assert_eq!(first.local_x(), 128);
        assert_eq!(first.local_y(), 64);
        assert_eq!(first.extra, 1);
        assert_eq!(first.name, "hash");
        Ok(())
    }

    #[test]
    fn parcel_properties_update_encodes() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let update = ParcelUpdate {
            local_id: 7,
            parcel_flags: ParcelFlags::CREATE_OBJECTS.union(ParcelFlags::USE_BAN_LIST),
            name: "My Parcel".to_owned(),
            description: "A test parcel".to_owned(),
            category: ParcelCategory::Residential,
            sale_price: 100,
            ..ParcelUpdate::default()
        };
        session.update_parcel(&update, now)?;
        let sent = drain(&mut session)?;

        let upd = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::ParcelPropertiesUpdate(message) => Some(message),
                _ => None,
            })
            .ok_or("expected a ParcelPropertiesUpdate")?;
        assert_eq!(upd.agent_data.agent_id, uuid::Uuid::from_u128(1));
        assert_eq!(upd.parcel_data.local_id, 7);
        // The message-level flag the reference viewer sends.
        assert_eq!(upd.parcel_data.flags, 0x1);
        assert_eq!(
            upd.parcel_data.parcel_flags,
            ParcelFlags::CREATE_OBJECTS
                .union(ParcelFlags::USE_BAN_LIST)
                .bits()
        );
        assert_eq!(trimmed(&upd.parcel_data.name), "My Parcel");
        assert_eq!(trimmed(&upd.parcel_data.desc), "A test parcel");
        assert_eq!(upd.parcel_data.category, 2);
        assert_eq!(upd.parcel_data.sale_price, 100);
        Ok(())
    }

    #[test]
    fn parcel_access_dwell_buy_return_encode() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        session.request_parcel_access_list(7, ParcelAccessScope::Ban, now)?;
        session.update_parcel_access_list(
            7,
            ParcelAccessScope::Access,
            &[ParcelAccessEntry {
                id: uuid::Uuid::from_u128(0x55),
                time: 0,
            }],
            now,
        )?;
        session.request_parcel_dwell(7, now)?;
        session.buy_parcel(7, 512, 1024, uuid::Uuid::nil(), false, now)?;
        session.return_parcel_objects(
            7,
            ParcelReturnType::OTHER,
            &[uuid::Uuid::from_u128(0x99)],
            &[],
            now,
        )?;
        let sent = drain(&mut session)?;

        let req = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::ParcelAccessListRequest(message) => Some(message),
                _ => None,
            })
            .ok_or("expected a ParcelAccessListRequest")?;
        // Ban list selector.
        assert_eq!(req.data.flags, 0x2);
        assert_eq!(req.data.local_id, 7);

        let upd = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::ParcelAccessListUpdate(message) => Some(message),
                _ => None,
            })
            .ok_or("expected a ParcelAccessListUpdate")?;
        assert_eq!(upd.data.flags, 0x1);
        let entry = upd.list.first().ok_or("expected one access entry")?;
        assert_eq!(entry.id, uuid::Uuid::from_u128(0x55));
        assert_eq!(entry.flags, 0x1);

        assert!(
            sent.iter()
                .any(|m| matches!(m, AnyMessage::ParcelDwellRequest(_))),
            "expected a ParcelDwellRequest"
        );

        let buy = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::ParcelBuy(message) => Some(message),
                _ => None,
            })
            .ok_or("expected a ParcelBuy")?;
        assert_eq!(buy.parcel_data.price, 512);
        assert_eq!(buy.parcel_data.area, 1024);
        assert!(buy.data.r#final);

        let ret = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::ParcelReturnObjects(message) => Some(message),
                _ => None,
            })
            .ok_or("expected a ParcelReturnObjects")?;
        assert_eq!(ret.parcel_data.return_type, ParcelReturnType::OTHER.0);
        assert_eq!(ret.owner_i_ds.len(), 1);
        Ok(())
    }

    #[test]
    fn parcel_dwell_reply_surfaces_event() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        let reply = AnyMessage::ParcelDwellReply(ParcelDwellReply {
            agent_data: ParcelDwellReplyAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(1),
            },
            data: ParcelDwellReplyDataBlock {
                local_id: 7,
                parcel_id: uuid::Uuid::from_u128(0xABC),
                dwell: 42.5,
            },
        });
        session.handle_datagram(sim_addr(), &server_message(&reply, 9, true)?, now)?;

        let dwell = drain_events(&mut session)
            .into_iter()
            .find_map(|e| match e {
                Event::ParcelDwell {
                    local_id,
                    parcel_id,
                    dwell,
                } => Some((local_id, parcel_id, dwell)),
                _ => None,
            })
            .ok_or("expected a ParcelDwell event")?;
        assert_eq!(dwell.0, 7);
        assert_eq!(dwell.1, uuid::Uuid::from_u128(0xABC));
        assert_eq!(dwell.2.to_bits(), 42.5_f32.to_bits());
        Ok(())
    }

    #[test]
    fn parcel_access_list_reply_surfaces_event() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        // A ban list (flags 0x2) with two entries.
        let reply = AnyMessage::ParcelAccessListReply(ParcelAccessListReply {
            data: ParcelAccessListReplyDataBlock {
                agent_id: uuid::Uuid::from_u128(1),
                sequence_id: 0,
                flags: 0x2,
                local_id: 7,
            },
            list: vec![
                ParcelAccessListReplyListBlock {
                    id: uuid::Uuid::from_u128(0x10),
                    time: 0,
                    flags: 0x2,
                },
                ParcelAccessListReplyListBlock {
                    id: uuid::Uuid::from_u128(0x11),
                    time: 1234,
                    flags: 0x2,
                },
            ],
        });
        session.handle_datagram(sim_addr(), &server_message(&reply, 9, true)?, now)?;

        let (local_id, scope, entries) = drain_events(&mut session)
            .into_iter()
            .find_map(|e| match e {
                Event::ParcelAccessList {
                    local_id,
                    scope,
                    entries,
                } => Some((local_id, scope, entries)),
                _ => None,
            })
            .ok_or("expected a ParcelAccessList event")?;
        assert_eq!(local_id, 7);
        assert_eq!(scope, ParcelAccessScope::Ban);
        assert_eq!(entries.len(), 2);
        let second = entries.get(1).ok_or("expected a second entry")?;
        assert_eq!(second.id, uuid::Uuid::from_u128(0x11));
        assert_eq!(second.time, 1234);
        Ok(())
    }

    /// NUL-terminates a string into wire bytes (as `with_nul` does in the core).
    fn with_nul_bytes(value: &str) -> Vec<u8> {
        let mut bytes = value.as_bytes().to_vec();
        bytes.push(0);
        bytes
    }

    /// The NUL-trimmed Parameter string at `index` of an `EstateOwnerMessage`.
    fn param_at(list: &[EstateOwnerMessageParamListBlock], index: usize) -> String {
        list.get(index)
            .map(|block| trimmed(&block.parameter))
            .unwrap_or_default()
    }

    #[test]
    fn estate_owner_messages_encode() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        session.request_estate_info(now)?;
        session.update_estate_access(
            EstateAccessDelta::BannedAgentAdd,
            uuid::Uuid::from_u128(9),
            now,
        )?;
        session.kick_estate_user(uuid::Uuid::from_u128(9), now)?;
        session.restart_region(-1, now)?;
        session.send_estate_message("hello estate", now)?;
        session.set_region_info(
            &RegionInfoUpdate {
                maturity: Maturity::Adult,
                agent_limit: 50,
                block_fly: true,
                ..RegionInfoUpdate::default()
            },
            now,
        )?;
        session.god_kick_user(uuid::Uuid::from_u128(9), "spam", now)?;
        let sent = drain(&mut session)?;

        let estate: Vec<_> = sent
            .iter()
            .filter_map(|m| match m {
                AnyMessage::EstateOwnerMessage(message) => Some(message),
                _ => None,
            })
            .collect();
        let method = |name: &str| {
            estate
                .iter()
                .find(|m| trimmed(&m.method_data.method) == name)
                .copied()
        };

        let getinfo = method("getinfo").ok_or("expected getinfo")?;
        assert_eq!(getinfo.agent_data.agent_id, uuid::Uuid::from_u128(1));

        let delta = method("estateaccessdelta").ok_or("expected estateaccessdelta")?;
        // ParamList: own id, flags (banned-agent-add = 1<<6 = 64), target id.
        assert_eq!(param_at(&delta.param_list, 1), "64");
        assert_eq!(
            param_at(&delta.param_list, 2),
            uuid::Uuid::from_u128(9).to_string()
        );

        let kick = method("kickestate").ok_or("expected kickestate")?;
        assert_eq!(
            param_at(&kick.param_list, 0),
            uuid::Uuid::from_u128(9).to_string()
        );

        let restart = method("restart").ok_or("expected restart")?;
        assert_eq!(param_at(&restart.param_list, 0), "-1");

        let message = method("simulatormessage").ok_or("expected simulatormessage")?;
        // Last param is the body.
        let body = message.param_list.last().ok_or("expected a message body")?;
        assert_eq!(trimmed(&body.parameter), "hello estate");

        let region = method("setregioninfo").ok_or("expected setregioninfo")?;
        // [1] block_fly = Y; [6] maturity = 42 (Adult).
        assert_eq!(param_at(&region.param_list, 1), "Y");
        assert_eq!(param_at(&region.param_list, 6), "42");

        let god = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::GodKickUser(message) => Some(message),
                _ => None,
            })
            .ok_or("expected a GodKickUser")?;
        assert_eq!(god.user_info.agent_id, uuid::Uuid::from_u128(9));
        assert_eq!(trimmed(&god.user_info.reason), "spam");
        Ok(())
    }

    #[test]
    fn estate_updateinfo_reply_surfaces_event() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        let owner = uuid::Uuid::from_u128(0x42);
        let params = [
            "My Estate".to_owned(),
            owner.to_string(),
            "101".to_owned(),
            "8".to_owned(),
            "0".to_owned(),
            "1".to_owned(),
            uuid::Uuid::nil().to_string(),
            "0".to_owned(),
            "1".to_owned(),
            "abuse@example.com".to_owned(),
        ];
        let message = AnyMessage::EstateOwnerMessage(EstateOwnerMessage {
            agent_data: EstateOwnerMessageAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(1),
                session_id: uuid::Uuid::from_u128(2),
                transaction_id: uuid::Uuid::nil(),
            },
            method_data: EstateOwnerMessageMethodDataBlock {
                method: b"estateupdateinfo\0".to_vec(),
                invoice: uuid::Uuid::nil(),
            },
            param_list: params
                .iter()
                .map(|p| EstateOwnerMessageParamListBlock {
                    parameter: with_nul_bytes(p),
                })
                .collect(),
        });
        session.handle_datagram(sim_addr(), &server_message(&message, 9, true)?, now)?;

        let info = drain_events(&mut session)
            .into_iter()
            .find_map(|e| match e {
                Event::EstateInfo(info) => Some(info),
                _ => None,
            })
            .ok_or("expected an EstateInfo event")?;
        assert_eq!(info.estate_name, "My Estate");
        assert_eq!(info.estate_owner, owner);
        assert_eq!(info.estate_id, 101);
        assert_eq!(info.estate_flags, 8);
        assert_eq!(info.abuse_email, "abuse@example.com");
        Ok(())
    }

    #[test]
    fn estate_setaccess_reply_surfaces_event() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        // A ban list (code 4) with two banned agents as raw 16-byte UUIDs.
        let banned = [uuid::Uuid::from_u128(0x10), uuid::Uuid::from_u128(0x11)];
        let mut param_list = vec![
            EstateOwnerMessageParamListBlock {
                parameter: with_nul_bytes("101"),
            },
            EstateOwnerMessageParamListBlock {
                parameter: with_nul_bytes("4"),
            },
            EstateOwnerMessageParamListBlock {
                parameter: with_nul_bytes("0"),
            },
            EstateOwnerMessageParamListBlock {
                parameter: with_nul_bytes("0"),
            },
            EstateOwnerMessageParamListBlock {
                parameter: with_nul_bytes("2"),
            },
            EstateOwnerMessageParamListBlock {
                parameter: with_nul_bytes("0"),
            },
        ];
        for id in banned {
            param_list.push(EstateOwnerMessageParamListBlock {
                parameter: id.as_bytes().to_vec(),
            });
        }
        let message = AnyMessage::EstateOwnerMessage(EstateOwnerMessage {
            agent_data: EstateOwnerMessageAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(1),
                session_id: uuid::Uuid::from_u128(2),
                transaction_id: uuid::Uuid::nil(),
            },
            method_data: EstateOwnerMessageMethodDataBlock {
                method: b"setaccess\0".to_vec(),
                invoice: uuid::Uuid::nil(),
            },
            param_list,
        });
        session.handle_datagram(sim_addr(), &server_message(&message, 9, true)?, now)?;

        let (estate_id, kind, members) = drain_events(&mut session)
            .into_iter()
            .find_map(|e| match e {
                Event::EstateAccessList {
                    estate_id,
                    kind,
                    members,
                } => Some((estate_id, kind, members)),
                _ => None,
            })
            .ok_or("expected an EstateAccessList event")?;
        assert_eq!(estate_id, 101);
        assert_eq!(kind, EstateAccessKind::BannedAgents);
        assert_eq!(members, banned.to_vec());
        Ok(())
    }

    /// The destination simulator used for handover tests.
    fn sim_b() -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 9001)
    }

    /// Builds a `TeleportFinish` datagram (hand-rolled so the IPPORT is in wire
    /// big-endian order) pointing at [`sim_b`] in the given destination region.
    fn teleport_finish_to_sim_b(region_handle: u64, sequence: u32) -> Result<Vec<u8>, TestError> {
        let mut body = Writer::new();
        body.put_uuid(uuid::Uuid::from_u128(1)); // agent_id
        body.put_u32(0); // location_id
        body.bytes(&[127, 0, 0, 1]); // sim_ip
        body.bytes(&[0x23, 0x29]); // sim_port 9001 (0x2329), network byte order
        body.put_u64(region_handle);
        body.put_variable2(b"http://127.0.0.1:9001/seed")?; // seed_capability
        body.put_u8(13); // sim_access (PG)
        body.put_u32(0); // teleport_flags
        Ok(server_datagram(
            MessageId::Low(69),
            &body.into_bytes(),
            sequence,
            true,
        ))
    }

    #[test]
    fn teleport_handover_rebinds_circuit_to_new_sim() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        let handle = 0x0003_E800_0003_E900;
        session.teleport_to(handle, vec3(128.0, 128.0, 30.0), vec3(1.0, 0.0, 0.0), now)?;
        let sent = drain(&mut session)?;
        assert!(
            sent.iter()
                .any(|m| matches!(m, AnyMessage::TeleportLocationRequest(_))),
            "expected a TeleportLocationRequest, got {sent:?}"
        );

        // TeleportFinish arrives on the old circuit and retargets to sim B.
        let finish = teleport_finish_to_sim_b(handle, 2)?;
        session.handle_datagram(sim_addr(), &finish, now)?;

        // The bootstrap for the new region is now destined to sim B.
        let transmit = session
            .poll_transmit()
            .ok_or("expected a transmit to sim B")?;
        assert_eq!(transmit.destination, sim_b());
        assert!(matches!(decode(&transmit)?, AnyMessage::UseCircuitCode(_)));
        drain(&mut session)?; // CompleteAgentMovement

        // The destination region's handshake completes the handover.
        let handshake = server_message(&region_handshake_msg(13, 0, "RegionB", "", ""), 1, true)?;
        session.handle_datagram(sim_b(), &handshake, now)?;
        let events = drain_events(&mut session);
        let changed = events
            .iter()
            .find_map(|e| match e {
                Event::RegionChanged { region_handle, sim } => Some((*region_handle, *sim)),
                _ => None,
            })
            .ok_or("expected a RegionChanged event")?;
        assert_eq!(changed.0, handle);
        assert_eq!(changed.1, sim_b());

        // Stray traffic from the old simulator is now ignored.
        let stray = server_message(&region_handshake_msg(13, 0, "OldRegion", "", ""), 9, true)?;
        session.handle_datagram(sim_addr(), &stray, now)?;
        assert!(drain_events(&mut session).is_empty());
        Ok(())
    }

    #[test]
    fn teleport_failed_returns_to_active() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        let handle = 0x0003_E800_0003_E900;
        session.teleport_to(handle, vec3(128.0, 128.0, 30.0), vec3(1.0, 0.0, 0.0), now)?;
        drain(&mut session)?;

        let failed = server_message(
            &AnyMessage::TeleportFailed(TeleportFailed {
                info: TeleportFailedInfoBlock {
                    agent_id: uuid::Uuid::from_u128(1),
                    reason: b"no access".to_vec(),
                },
                alert_info: Vec::new(),
            }),
            2,
            true,
        )?;
        session.handle_datagram(sim_addr(), &failed, now)?;
        let events = drain_events(&mut session);
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::TeleportFailed { .. })),
            "expected a TeleportFailed event, got {events:?}"
        );

        // Back in the active state, a second teleport is accepted.
        session.teleport_to(handle, vec3(128.0, 128.0, 30.0), vec3(1.0, 0.0, 0.0), now)?;
        Ok(())
    }

    #[test]
    fn teleport_times_out() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        session.teleport_to(
            0x0003_E800_0003_E900,
            vec3(128.0, 128.0, 30.0),
            vec3(1.0, 0.0, 0.0),
            now,
        )?;
        drain(&mut session)?;

        session.handle_timeout(after(now, 31_000)?);
        let events = drain_events(&mut session);
        let reason = events
            .iter()
            .find_map(|e| match e {
                Event::TeleportFailed { reason } => Some(reason.clone()),
                _ => None,
            })
            .ok_or("expected a TeleportFailed event")?;
        assert!(reason.contains("timed out"), "unexpected reason: {reason}");
        Ok(())
    }

    #[test]
    fn agent_update_advertises_draw_distance() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        session.set_draw_distance(512.0);
        session.handle_timeout(after(now, 1100)?);
        let sent = drain(&mut session)?;
        let update = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::AgentUpdate(update) => Some(update),
                _ => None,
            })
            .ok_or("expected an AgentUpdate")?;
        assert_eq!(update.agent_data.far.to_bits(), 512.0_f32.to_bits());
        // The camera is non-zero so the simulator enables neighbours.
        assert_ne!(
            update.agent_data.camera_center.x.to_bits(),
            0.0_f32.to_bits()
        );
        Ok(())
    }

    #[test]
    fn caps_parcel_properties_becomes_event() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        // A ParcelProperties event as delivered over the CAPS event queue. OpenSim
        // encodes the uint ParcelFlags as a 4-byte big-endian binary element:
        // AAAAQA== is [0, 0, 0, 64] = 64 (CREATE_OBJECTS).
        let xml = "<llsd><map><key>ParcelData</key><array><map>\
            <key>LocalID</key><integer>3</integer>\
            <key>SequenceID</key><integer>9</integer>\
            <key>Area</key><integer>2048</integer>\
            <key>ParcelFlags</key><binary>AAAAQA==</binary>\
            <key>MaxPrims</key><integer>750</integer>\
            <key>AABBMax</key><array><real>32</real><real>16</real><real>0</real></array>\
            <key>Bitmap</key><binary>AQID</binary>\
            <key>MusicURL</key><string>http://stream.example/audio</string>\
            <key>MediaURL</key><string>http://example.com/movie</string>\
            <key>MediaID</key><uuid>00000000-0000-0000-0000-0000000033ed</uuid>\
            <key>MediaAutoScale</key><boolean>1</boolean>\
            </map></array></map></llsd>";
        let body = sl_proto::parse_llsd_xml(xml)?;
        session.handle_caps_event("ParcelProperties", &body, now)?;

        let events = drain_events(&mut session);
        let parcel = events
            .iter()
            .find_map(|e| match e {
                Event::ParcelProperties(parcel) => Some(parcel),
                _ => None,
            })
            .ok_or("expected a ParcelProperties event")?;
        assert_eq!(parcel.local_id, 3);
        assert_eq!(parcel.sequence_id, 9);
        assert_eq!(parcel.area, 2048);
        assert_eq!(parcel.max_prims, 750);
        assert_eq!(parcel.aabb_max.0.to_bits(), 32.0_f32.to_bits());
        assert_eq!(parcel.bitmap, vec![1u8, 2, 3]);
        // ParcelFlags 64 = CREATE_OBJECTS, decoded from the binary element.
        assert_eq!(parcel.raw_parcel_flags, 64);
        assert!(parcel.create_objects());
        // The stream / media URLs decode off the CAPS LLSD too.
        assert_eq!(parcel.music_url, "http://stream.example/audio");
        assert_eq!(parcel.media_url, "http://example.com/movie");
        assert_eq!(parcel.media_id, uuid::Uuid::from_u128(0x33ED));
        assert!(parcel.media_auto_scale);
        Ok(())
    }

    #[test]
    fn region_info_decodes_without_trailing_variable_blocks() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        // Encode a RegionInfo, then drop the two trailing empty variable-block
        // count bytes (RegionInfo5/CombatSettings), as OpenSim's shorter
        // RegionInfo does. The lenient decoder must still succeed.
        let message = region_info_msg("TrimRegion", 13, 25, 0, 80, 12000);
        let mut writer = Writer::new();
        message.id().encode(&mut writer);
        message.encode_body(&mut writer)?;
        let mut body = writer.into_bytes();
        body.truncate(body.len().saturating_sub(2));
        let datagram = encode_datagram(PacketFlags::EMPTY, 9, &body);
        session.handle_datagram(sim_addr(), &datagram, now)?;

        let events = drain_events(&mut session);
        let limits = events
            .iter()
            .find_map(|e| match e {
                Event::RegionLimits(limits) => Some(limits),
                _ => None,
            })
            .ok_or("expected a RegionLimits event")?;
        assert_eq!(limits.max_agents, 25);
        assert_eq!(limits.hard_max_objects, 12000);
        Ok(())
    }

    #[test]
    fn map_block_reply_reports_named_regions() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        session.request_map_blocks(1000, 1001, 1000, 1001, now)?;
        let sent = drain(&mut session)?;
        let request = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::MapBlockRequest(request) => Some(request),
                _ => None,
            })
            .ok_or("expected a MapBlockRequest")?;
        assert_eq!(request.position_data.min_x, 1000);
        assert_eq!(request.position_data.max_y, 1001);

        let reply = AnyMessage::MapBlockReply(MapBlockReply {
            agent_data: MapBlockReplyAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(1),
                flags: 0,
            },
            data: vec![
                MapBlockReplyDataBlock {
                    x: 1000,
                    y: 1001,
                    name: b"TestRegion\0".to_vec(),
                    access: 21,
                    region_flags: 0,
                    water_height: 20,
                    agents: 3,
                    map_image_id: uuid::Uuid::nil(),
                },
                // A sentinel "not found" block, which must be filtered out.
                MapBlockReplyDataBlock {
                    x: 0,
                    y: 0,
                    name: Vec::new(),
                    access: 255,
                    region_flags: 0,
                    water_height: 0,
                    agents: 0,
                    map_image_id: uuid::Uuid::nil(),
                },
            ],
            size: vec![MapBlockReplySizeBlock {
                size_x: 256,
                size_y: 256,
            }],
        });
        let datagram = server_message(&reply, 9, true)?;
        session.handle_datagram(sim_addr(), &datagram, now)?;

        let events = drain_events(&mut session);
        let regions: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                Event::MapBlock(region) => Some(region),
                _ => None,
            })
            .collect();
        assert_eq!(regions.len(), 1, "sentinel block should be filtered");
        let region = regions.first().ok_or("one region")?;
        assert_eq!(region.name, "TestRegion");
        assert_eq!(region.grid_x, 1000);
        assert_eq!(region.grid_y, 1001);
        assert_eq!(region.maturity, Maturity::Mature);
        assert_eq!(region.region_handle, sl_proto::grid_to_handle(1000, 1001));
        Ok(())
    }

    /// The CAPS `TeleportFinish` event body naming [`sim_b`] as the destination.
    fn caps_teleport_finish_xml() -> &'static str {
        // SimIP fwAAAQ== is base64 of [127, 0, 0, 1]; SimPort is a plain integer
        // (host order, no byte swap).
        "<llsd><map><key>Info</key><array><map>\
            <key>SimIP</key><binary>fwAAAQ==</binary>\
            <key>SimPort</key><integer>9001</integer>\
            <key>SeedCapability</key><string>http://127.0.0.1:9001/seed</string>\
            </map></array></map></llsd>"
    }

    #[test]
    fn caps_teleport_finish_hands_over_to_destination() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        let handle = 0x0003_E800_0003_E900;
        session.teleport_to(handle, vec3(128.0, 128.0, 30.0), vec3(1.0, 0.0, 0.0), now)?;
        let sent = drain(&mut session)?;
        assert!(
            sent.iter()
                .any(|m| matches!(m, AnyMessage::TeleportLocationRequest(_))),
            "expected a TeleportLocationRequest"
        );

        // OpenSim delivers TeleportFinish over the CAPS event queue.
        let body = sl_proto::parse_llsd_xml(caps_teleport_finish_xml())?;
        session.handle_caps_event("TeleportFinish", &body, now)?;

        // The handover bootstraps the destination: UseCircuitCode +
        // CompleteAgentMovement to sim_b.
        let mut to_dest = Vec::new();
        while let Some(transmit) = session.poll_transmit() {
            let message = decode(&transmit)?;
            if transmit.destination == sim_b() {
                to_dest.push(message);
            }
        }
        assert!(
            to_dest
                .iter()
                .any(|m| matches!(m, AnyMessage::UseCircuitCode(_))),
            "expected a UseCircuitCode to the destination, got {to_dest:?}"
        );
        assert!(
            to_dest
                .iter()
                .any(|m| matches!(m, AnyMessage::CompleteAgentMovement(_))),
            "expected a CompleteAgentMovement to the destination, got {to_dest:?}"
        );

        // The destination's handshake completes the handover.
        let handshake = server_message(&region_handshake_msg(13, 0, "RegionB", "", ""), 1, true)?;
        session.handle_datagram(sim_b(), &handshake, now)?;
        let events = drain_events(&mut session);
        let changed = events
            .iter()
            .find_map(|e| match e {
                Event::RegionChanged { region_handle, sim } => Some((*region_handle, *sim)),
                _ => None,
            })
            .ok_or("expected a RegionChanged event")?;
        assert_eq!(changed.0, handle);
        assert_eq!(changed.1, sim_b());
        Ok(())
    }

    #[test]
    fn agent_movement_complete_completes_handover_once() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        let handle = 0x0003_E800_0003_E900;
        session.teleport_to(handle, vec3(128.0, 128.0, 30.0), vec3(1.0, 0.0, 0.0), now)?;
        drain(&mut session)?;
        let body = sl_proto::parse_llsd_xml(caps_teleport_finish_xml())?;
        session.handle_caps_event("TeleportFinish", &body, now)?;
        drain(&mut session)?; // UseCircuitCode + CompleteAgentMovement

        // The destination promotes us to root and confirms with
        // AgentMovementComplete; the handover completes even with no RegionHandshake.
        let amc = AnyMessage::AgentMovementComplete(AgentMovementComplete {
            agent_data: AgentMovementCompleteAgentDataBlock {
                agent_id: uuid::Uuid::nil(),
                session_id: uuid::Uuid::nil(),
            },
            data: AgentMovementCompleteDataBlock {
                position: vec3(128.0, 128.0, 30.0),
                look_at: vec3(1.0, 0.0, 0.0),
                region_handle: handle,
                timestamp: 0,
            },
            sim_data: AgentMovementCompleteSimDataBlock {
                channel_version: b"test".to_vec(),
            },
        });
        let datagram = server_message(&amc, 1, true)?;
        session.handle_datagram(sim_b(), &datagram, now)?;
        let events = drain_events(&mut session);
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e, Event::RegionChanged { .. }))
                .count(),
            1,
            "AgentMovementComplete should complete the handover once"
        );

        // A later RegionHandshake must not emit a second RegionChanged.
        let handshake = server_message(&region_handshake_msg(13, 0, "RegionB", "", ""), 2, true)?;
        session.handle_datagram(sim_b(), &handshake, now)?;
        let events = drain_events(&mut session);
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Event::RegionChanged { .. })),
            "handover must not complete twice"
        );
        Ok(())
    }

    // ----- Object / scene graph (#16) ---------------------------------------

    /// The region handle the object tests use.
    const OBJ_REGION: u64 = 0x0000_03e8_0000_03e8;

    /// A zero [`Vector`] (the shared type has no `Default`/`Copy` impl).
    fn zero_vec() -> Vector {
        Vector {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }
    }

    /// Encodes a 60-byte full-precision `ObjectData` motion blob (position,
    /// velocity, acceleration, packed-quaternion rotation, angular velocity).
    fn full_motion_blob(position: Vector) -> Vec<u8> {
        let mut writer = Writer::new();
        writer.put_vector3(&position);
        writer.put_vector3(&zero_vec());
        writer.put_vector3(&zero_vec());
        writer.put_quaternion(&Rotation {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            s: 1.0,
        });
        writer.put_vector3(&zero_vec());
        writer.into_bytes()
    }

    /// Builds a one-object `ObjectUpdate` for a prim in the current region.
    fn object_update(local_id: u32, full_id: u128, position: Vector) -> AnyMessage {
        object_update_in(OBJ_REGION, local_id, full_id, position)
    }

    /// Builds a one-object `ObjectUpdate` for a prim in `region_handle`.
    fn object_update_in(
        region_handle: u64,
        local_id: u32,
        full_id: u128,
        position: Vector,
    ) -> AnyMessage {
        AnyMessage::ObjectUpdate(ObjectUpdate {
            region_data: ObjectUpdateRegionDataBlock {
                region_handle,
                time_dilation: 0xFFFF,
            },
            object_data: vec![ObjectUpdateObjectDataBlock {
                id: local_id,
                state: 0,
                full_id: uuid::Uuid::from_u128(full_id),
                crc: 42,
                p_code: pcode::PRIMITIVE,
                material: 3,
                click_action: 0,
                scale: Vector {
                    x: 1.0,
                    y: 2.0,
                    z: 3.0,
                },
                object_data: full_motion_blob(position),
                parent_id: 0,
                update_flags: 0,
                path_curve: 16,
                profile_curve: 1,
                path_begin: 0,
                path_end: 0,
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
                texture_entry: Vec::new(),
                texture_anim: Vec::new(),
                name_value: Vec::new(),
                data: Vec::new(),
                text: Vec::new(),
                text_color: [0; 4],
                media_url: Vec::new(),
                ps_block: Vec::new(),
                extra_params: Vec::new(),
                sound: uuid::Uuid::nil(),
                owner_id: uuid::Uuid::nil(),
                gain: 0.0,
                flags: 0,
                radius: 0.0,
                joint_type: 0,
                joint_pivot: zero_vec(),
                joint_axis_or_anchor: zero_vec(),
            }],
        })
    }

    #[test]
    fn object_update_adds_then_updates() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let position = Vector {
            x: 10.0,
            y: 20.0,
            z: 30.0,
        };
        let update = object_update(100, 0xABCD, position.clone());
        session.handle_datagram(sim_addr(), &server_message(&update, 5, true)?, now)?;
        let events = drain_events(&mut session);
        let Some(Event::ObjectAdded(object)) =
            events.iter().find(|e| matches!(e, Event::ObjectAdded(_)))
        else {
            return Err(format!("expected ObjectAdded, got {events:?}").into());
        };
        assert_eq!(object.local_id, 100);
        assert_eq!(object.full_id, uuid::Uuid::from_u128(0xABCD));
        assert_eq!(object.pcode, pcode::PRIMITIVE);
        assert_eq!(object.region_handle, OBJ_REGION);
        assert_eq!(object.motion.position, position);
        assert_eq!(object.material, 3);

        // The object is in the public cache.
        assert!(session.object(100).is_some());
        assert_eq!(session.objects().count(), 1);

        // A second update for the same id updates rather than adds.
        let moved = Vector {
            x: 11.0,
            y: 20.0,
            z: 30.0,
        };
        let update = object_update(100, 0xABCD, moved);
        session.handle_datagram(sim_addr(), &server_message(&update, 6, true)?, now)?;
        let events = drain_events(&mut session);
        assert!(
            events.iter().any(|e| matches!(e, Event::ObjectUpdated(_))),
            "expected ObjectUpdated, got {events:?}"
        );
        assert!(
            !events.iter().any(|e| matches!(e, Event::ObjectAdded(_))),
            "must not re-add a known object"
        );
        Ok(())
    }

    /// Appends one `ExtraParams` entry (`u16 type`, `u32 size`, payload) to a
    /// container writer.
    fn push_extra_param(
        extra: &mut Writer,
        param_type: u16,
        payload: &[u8],
    ) -> Result<(), TestError> {
        extra.put_u16(param_type);
        extra.put_u32(u32::try_from(payload.len())?);
        extra.bytes(payload);
        Ok(())
    }

    #[test]
    fn object_update_decodes_extra_params() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // Build a multi-parameter `ExtraParams` blob (`u8 count`, then each
        // entry as `u16 type` / `u32 size` / payload), one of each decoded type.
        let sculpt_tex = uuid::Uuid::from_u128(0x5C01);
        let mat_id = uuid::Uuid::from_u128(0x9A7E);

        let mut flexible = Writer::new();
        flexible.put_u8(0x12); // tension byte: softness bit + tension 0x12 & 0x7f = 18
        flexible.put_u8(0x05); // drag byte
        flexible.put_u8(150); // gravity: 150/10 - 10 = 5.0
        flexible.put_u8(20); // wind: 20/10 = 2.0
        flexible.put_vector3(&vec3(1.0, 0.0, 0.0));

        let mut light = Writer::new();
        light.bytes(&[10, 20, 30, 255]);
        light.put_f32(5.0);
        light.put_f32(0.1);
        light.put_f32(0.75);

        let mut sculpt = Writer::new();
        sculpt.put_uuid(sculpt_tex);
        sculpt.put_u8(0x05); // LL_SCULPT_TYPE_MESH

        let mut extended = Writer::new();
        extended.put_u32(0x0000_0001);

        let mut render = Writer::new();
        render.put_u8(1); // one entry
        render.put_u8(3); // face 3
        render.put_uuid(mat_id);

        let mut probe = Writer::new();
        probe.put_f32(0.5);
        probe.put_f32(2.0);
        probe.put_u8(0x05); // box | mirror

        let mut extra = Writer::new();
        extra.put_u8(6);
        push_extra_param(&mut extra, 0x10, &flexible.into_bytes())?;
        push_extra_param(&mut extra, 0x20, &light.into_bytes())?;
        push_extra_param(&mut extra, 0x30, &sculpt.into_bytes())?;
        push_extra_param(&mut extra, 0x70, &extended.into_bytes())?;
        push_extra_param(&mut extra, 0x80, &render.into_bytes())?;
        push_extra_param(&mut extra, 0x90, &probe.into_bytes())?;
        let extra_params = extra.into_bytes();

        let AnyMessage::ObjectUpdate(mut update) = object_update(300, 0xBEEF, zero_vec()) else {
            return Err("expected ObjectUpdate".into());
        };
        if let Some(block) = update.object_data.first_mut() {
            block.extra_params = extra_params;
        }
        session.handle_datagram(
            sim_addr(),
            &server_message(&AnyMessage::ObjectUpdate(update), 5, true)?,
            now,
        )?;

        let events = drain_events(&mut session);
        let Some(Event::ObjectAdded(object)) =
            events.iter().find(|e| matches!(e, Event::ObjectAdded(_)))
        else {
            return Err(format!("expected ObjectAdded, got {events:?}").into());
        };
        let extra = &object.extra;

        let flexible = extra.flexible.as_ref().ok_or("expected flexi")?;
        assert!((flexible.gravity - 5.0).abs() < f32::EPSILON);
        assert!((flexible.wind_sensitivity - 2.0).abs() < f32::EPSILON);
        assert_eq!(flexible.user_force, vec3(1.0, 0.0, 0.0));

        let light = extra.light.ok_or("expected light")?;
        assert_eq!(light.color, [10, 20, 30, 255]);
        assert!((light.radius - 5.0).abs() < f32::EPSILON);

        let sculpt = extra.sculpt.ok_or("expected sculpt")?;
        assert_eq!(sculpt.texture, sculpt_tex);
        assert_eq!(sculpt.sculpt_type, 0x05);

        assert_eq!(
            extra.extended_mesh.ok_or("expected extended mesh")?.flags,
            1
        );

        let material = extra.render_material.first().ok_or("expected material")?;
        assert_eq!(material.face, 3);
        assert_eq!(material.material_id, mat_id);

        let probe = extra.reflection_probe.ok_or("expected probe")?;
        assert!((probe.ambiance - 0.5).abs() < f32::EPSILON);
        assert!(probe.is_box);
        assert!(!probe.is_dynamic);
        assert!(probe.is_mirror);
        Ok(())
    }

    #[test]
    fn terse_update_moves_known_object() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // First establish the object via a full update.
        let update = object_update(200, 0x1234, zero_vec());
        session.handle_datagram(sim_addr(), &server_message(&update, 5, true)?, now)?;
        drain_events(&mut session);

        // Build a terse blob: local id, state, no collision plane, full-precision
        // position, then quantized velocity/acceleration/rotation/angular velocity.
        let mut writer = Writer::new();
        writer.put_u32(200);
        writer.put_u8(0);
        writer.put_u8(0);
        let new_pos = Vector {
            x: 5.0,
            y: 6.0,
            z: 7.0,
        };
        writer.put_vector3(&new_pos);
        for _ in 0..(3 + 3) {
            writer.put_u16(0x8000);
        }
        for _ in 0..4 {
            writer.put_u16(0xFFFF);
        }
        for _ in 0..3 {
            writer.put_u16(0x8000);
        }
        let terse = AnyMessage::ImprovedTerseObjectUpdate(ImprovedTerseObjectUpdate {
            region_data: ImprovedTerseObjectUpdateRegionDataBlock {
                region_handle: OBJ_REGION,
                time_dilation: 0xFFFF,
            },
            object_data: vec![ImprovedTerseObjectUpdateObjectDataBlock {
                data: writer.into_bytes(),
                texture_entry: Vec::new(),
            }],
        });
        session.handle_datagram(sim_addr(), &server_message(&terse, 6, true)?, now)?;
        let events = drain_events(&mut session);
        let Some(Event::ObjectUpdated(object)) =
            events.iter().find(|e| matches!(e, Event::ObjectUpdated(_)))
        else {
            return Err(format!("expected ObjectUpdated, got {events:?}").into());
        };
        assert_eq!(object.motion.position, new_pos);
        // Cache reflects the new position.
        assert_eq!(
            session.object(200).map(|o| o.motion.position.clone()),
            Some(new_pos)
        );
        Ok(())
    }

    #[test]
    fn terse_update_for_unknown_requests_full() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let mut writer = Writer::new();
        writer.put_u32(999);
        writer.put_u8(0);
        writer.put_u8(0);
        writer.put_vector3(&zero_vec());
        for _ in 0..(3 + 3 + 4 + 3) {
            writer.put_u16(0x8000);
        }
        let terse = AnyMessage::ImprovedTerseObjectUpdate(ImprovedTerseObjectUpdate {
            region_data: ImprovedTerseObjectUpdateRegionDataBlock {
                region_handle: OBJ_REGION,
                time_dilation: 0xFFFF,
            },
            object_data: vec![ImprovedTerseObjectUpdateObjectDataBlock {
                data: writer.into_bytes(),
                texture_entry: Vec::new(),
            }],
        });
        session.handle_datagram(sim_addr(), &server_message(&terse, 6, true)?, now)?;
        let sent = drain(&mut session)?;
        let request = sent.iter().find_map(|m| match m {
            AnyMessage::RequestMultipleObjects(request) => Some(request),
            _ => None,
        });
        let request = request.ok_or("expected a RequestMultipleObjects for the unknown object")?;
        assert!(request.object_data.iter().any(|o| o.id == 999));
        Ok(())
    }

    #[test]
    fn cached_update_requests_full_update() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let cached = AnyMessage::ObjectUpdateCached(ObjectUpdateCached {
            region_data: ObjectUpdateCachedRegionDataBlock {
                region_handle: OBJ_REGION,
                time_dilation: 0xFFFF,
            },
            object_data: vec![ObjectUpdateCachedObjectDataBlock {
                id: 321,
                crc: 7,
                update_flags: 0,
            }],
        });
        session.handle_datagram(sim_addr(), &server_message(&cached, 5, true)?, now)?;
        let sent = drain(&mut session)?;
        let request = sent.iter().find_map(|m| match m {
            AnyMessage::RequestMultipleObjects(request) => Some(request),
            _ => None,
        });
        let request = request.ok_or("expected a RequestMultipleObjects for the cached miss")?;
        assert!(request.object_data.iter().any(|o| o.id == 321));
        Ok(())
    }

    #[test]
    fn kill_object_removes_from_cache() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let update = object_update(400, 0x5678, zero_vec());
        session.handle_datagram(sim_addr(), &server_message(&update, 5, true)?, now)?;
        drain_events(&mut session);
        assert!(session.object(400).is_some());

        let kill = AnyMessage::KillObject(KillObject {
            object_data: vec![KillObjectObjectDataBlock { id: 400 }],
        });
        session.handle_datagram(sim_addr(), &server_message(&kill, 6, true)?, now)?;
        let events = drain_events(&mut session);
        let removed = events.iter().find_map(|e| match e {
            Event::ObjectRemoved {
                local_id,
                region_handle,
            } => Some((*local_id, *region_handle)),
            _ => None,
        });
        assert_eq!(removed, Some((400, OBJ_REGION)));
        assert!(session.object(400).is_none());
        Ok(())
    }

    #[test]
    fn object_properties_surface_and_merge() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // Establish the object so properties can merge into it.
        let update = object_update(500, 0x9ABC, zero_vec());
        session.handle_datagram(sim_addr(), &server_message(&update, 5, true)?, now)?;
        drain_events(&mut session);

        let props = AnyMessage::ObjectProperties(WireObjectProperties {
            object_data: vec![ObjectPropertiesObjectDataBlock {
                object_id: uuid::Uuid::from_u128(0x9ABC),
                creator_id: uuid::Uuid::from_u128(0x11),
                owner_id: uuid::Uuid::from_u128(0x22),
                group_id: uuid::Uuid::nil(),
                creation_date: 1700,
                base_mask: 0x7FFF_FFFF,
                owner_mask: 0x7FFF_FFFF,
                group_mask: 0,
                everyone_mask: 0,
                next_owner_mask: 0,
                ownership_cost: 0,
                sale_type: 0,
                sale_price: 0,
                aggregate_perms: 0,
                aggregate_perm_textures: 0,
                aggregate_perm_textures_owner: 0,
                category: 0,
                inventory_serial: 1,
                item_id: uuid::Uuid::nil(),
                folder_id: uuid::Uuid::nil(),
                from_task_id: uuid::Uuid::nil(),
                last_owner_id: uuid::Uuid::from_u128(0x33),
                name: b"Test Prim\0".to_vec(),
                description: b"a description\0".to_vec(),
                touch_name: Vec::new(),
                sit_name: Vec::new(),
                texture_id: Vec::new(),
            }],
        });
        session.handle_datagram(sim_addr(), &server_message(&props, 6, true)?, now)?;
        let events = drain_events(&mut session);
        let Some(Event::ObjectProperties(properties)) = events
            .iter()
            .find(|e| matches!(e, Event::ObjectProperties(_)))
        else {
            return Err(format!("expected ObjectProperties, got {events:?}").into());
        };
        assert_eq!(properties.name, "Test Prim");
        assert_eq!(properties.description, "a description");
        // Merged into the cached object.
        assert_eq!(
            session
                .object(500)
                .and_then(|o| o.properties.as_ref())
                .map(|p| p.name.clone()),
            Some("Test Prim".to_owned())
        );
        Ok(())
    }

    #[test]
    fn compressed_update_decodes_object() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // Build a minimal compressed blob (no optional fields: cflags = 0).
        let position = Vector {
            x: 40.0,
            y: 50.0,
            z: 60.0,
        };
        let mut writer = Writer::new();
        writer.put_uuid(uuid::Uuid::from_u128(0xDEAD));
        writer.put_u32(600);
        writer.put_u8(pcode::PRIMITIVE);
        writer.put_u8(0);
        writer.put_u32(99);
        writer.put_u8(3);
        writer.put_u8(0);
        writer.put_vector3(&Vector {
            x: 1.0,
            y: 1.0,
            z: 1.0,
        });
        writer.put_vector3(&position);
        writer.put_quaternion(&Rotation {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            s: 1.0,
        });
        writer.put_u32(0);
        writer.put_uuid(uuid::Uuid::from_u128(0x44));
        let compressed = AnyMessage::ObjectUpdateCompressed(ObjectUpdateCompressed {
            region_data: ObjectUpdateCompressedRegionDataBlock {
                region_handle: OBJ_REGION,
                time_dilation: 0xFFFF,
            },
            object_data: vec![ObjectUpdateCompressedObjectDataBlock {
                update_flags: 0,
                data: writer.into_bytes(),
            }],
        });
        session.handle_datagram(sim_addr(), &server_message(&compressed, 5, true)?, now)?;
        let events = drain_events(&mut session);
        let Some(Event::ObjectAdded(object)) =
            events.iter().find(|e| matches!(e, Event::ObjectAdded(_)))
        else {
            return Err(format!("expected ObjectAdded from compressed, got {events:?}").into());
        };
        assert_eq!(object.local_id, 600);
        assert_eq!(object.full_id, uuid::Uuid::from_u128(0xDEAD));
        assert_eq!(object.crc, 99);
        assert_eq!(object.owner_id, uuid::Uuid::from_u128(0x44));
        assert_eq!(object.motion.position, position);
        Ok(())
    }

    /// The neighbour region handle `enable_neighbour_b` announces (grid 1001,1000).
    const NB_REGION: u64 = 0x0003_E900_0003_E800;

    /// Builds a one-object terse update `Data` blob for `local_id` at the origin.
    fn terse_blob(local_id: u32) -> Vec<u8> {
        let mut writer = Writer::new();
        writer.put_u32(local_id);
        writer.put_u8(0);
        writer.put_u8(0);
        writer.put_vector3(&zero_vec());
        for _ in 0..(3 + 3 + 4 + 3) {
            writer.put_u16(0x8000);
        }
        writer.into_bytes()
    }

    #[test]
    fn neighbour_objects_stream_on_child_circuit() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        // Open a child-agent circuit to the neighbour `sim_b`.
        enable_neighbour_b(&mut session, 9, now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        // An object streamed *from the neighbour circuit* is cached and added.
        let pos = Vector {
            x: 1.0,
            y: 2.0,
            z: 3.0,
        };
        let update = object_update_in(NB_REGION, 700, 0xBEEF, pos.clone());
        session.handle_datagram(sim_b(), &server_message(&update, 3, true)?, now)?;
        let events = drain_events(&mut session);
        let Some(Event::ObjectAdded(object)) =
            events.iter().find(|e| matches!(e, Event::ObjectAdded(_)))
        else {
            return Err(format!("expected neighbour ObjectAdded, got {events:?}").into());
        };
        assert_eq!(object.local_id, 700);
        assert_eq!(object.region_handle, NB_REGION);

        // It lives in the neighbour region's set; the root-region `object()`
        // lookup does not see it (local ids share a numeric space across regions).
        assert_eq!(session.objects_in_region(NB_REGION).count(), 1);
        assert!(session.object(700).is_none());
        assert_eq!(session.objects().count(), 1);

        // A terse update for an unknown neighbour id requests the full update on
        // the child circuit (sim_b), not the root.
        let terse = AnyMessage::ImprovedTerseObjectUpdate(ImprovedTerseObjectUpdate {
            region_data: ImprovedTerseObjectUpdateRegionDataBlock {
                region_handle: NB_REGION,
                time_dilation: 0xFFFF,
            },
            object_data: vec![ImprovedTerseObjectUpdateObjectDataBlock {
                data: terse_blob(800),
                texture_entry: Vec::new(),
            }],
        });
        session.handle_datagram(sim_b(), &server_message(&terse, 4, true)?, now)?;
        let mut requested_on_b = false;
        while let Some(transmit) = session.poll_transmit() {
            if transmit.destination == sim_b()
                && let Ok(AnyMessage::RequestMultipleObjects(request)) = decode(&transmit)
                && request.object_data.iter().any(|o| o.id == 800)
            {
                requested_on_b = true;
            }
        }
        assert!(
            requested_on_b,
            "the cache-miss fetch must go to the neighbour circuit"
        );

        // When the neighbour is disabled, its objects are dropped (with events).
        let disable = AnyMessage::DisableSimulator(DisableSimulator {});
        session.handle_datagram(sim_b(), &server_message(&disable, 5, true)?, now)?;
        let events = drain_events(&mut session);
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::ObjectRemoved {
                    local_id: 700,
                    region_handle,
                } if *region_handle == NB_REGION
            )),
            "disabling the neighbour must remove its objects, got {events:?}"
        );
        assert_eq!(session.objects().count(), 0);
        Ok(())
    }

    // Object interaction & editing (#17) -----------------------------------

    #[test]
    fn rez_object_sends_object_add() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let position = Vector {
            x: 128.0,
            y: 64.0,
            z: 25.0,
        };
        session.rez_object(&PrimShape::cube(position.clone()), uuid::Uuid::nil(), now)?;
        let sent = drain(&mut session)?;
        let add = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::ObjectAdd(add) => Some(add),
                _ => None,
            })
            .ok_or("expected an ObjectAdd")?;
        // A default cube: volume prim, wood, square profile, line path.
        assert_eq!(add.object_data.p_code, pcode::PRIMITIVE);
        assert_eq!(add.object_data.material, 3); // LL_MCODE_WOOD
        assert_eq!(add.object_data.path_curve, 0x10); // LL_PCODE_PATH_LINE
        assert_eq!(add.object_data.profile_curve, 0x01); // LL_PCODE_PROFILE_SQUARE
        assert_eq!(add.object_data.path_scale_x, 100);
        assert_eq!(add.object_data.path_scale_y, 100);
        assert_eq!(
            add.object_data.scale,
            Vector {
                x: 0.5,
                y: 0.5,
                z: 0.5
            }
        );
        // Rez exactly at the position: raycast bypassed, ray endpoint = position.
        assert_eq!(add.object_data.bypass_raycast, 1);
        assert_eq!(add.object_data.ray_end, position);
        Ok(())
    }

    #[test]
    fn update_object_packs_position_and_rotation() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let position = Vector {
            x: 1.0,
            y: 2.0,
            z: 3.0,
        };
        // A 90° rotation about Z: (0, 0, sin45, cos45), so w >= 0 and packToVector3
        // leaves the vector part untouched.
        let sin45 = std::f32::consts::FRAC_1_SQRT_2;
        let rotation = Rotation {
            x: 0.0,
            y: 0.0,
            z: sin45,
            s: sin45,
        };
        session.update_object(
            42,
            &ObjectTransform {
                position: Some(position.clone()),
                rotation: Some(rotation),
                ..ObjectTransform::default()
            },
            now,
        )?;
        let sent = drain(&mut session)?;
        let update = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::MultipleObjectUpdate(update) => Some(update),
                _ => None,
            })
            .ok_or("expected a MultipleObjectUpdate")?;
        let block = update
            .object_data
            .first()
            .ok_or("expected one ObjectData block")?;
        assert_eq!(block.object_local_id, 42);
        // POSITION (0x01) | ROTATION (0x02).
        assert_eq!(block.r#type, 0x03);
        // The Data blob is position (12 bytes) then the packed quaternion (12).
        assert_eq!(block.data.len(), 24);
        let mut reader = Reader::new(&block.data);
        assert_eq!(reader.vector3()?, position);
        let qx = reader.f32()?;
        let qy = reader.f32()?;
        let qz = reader.f32()?;
        assert!((qx - 0.0).abs() < 1e-5, "qx = {qx}");
        assert!((qy - 0.0).abs() < 1e-5, "qy = {qy}");
        assert!((qz - sin45).abs() < 1e-5, "qz = {qz}");
        Ok(())
    }

    #[test]
    fn set_object_scale_uniform_group_sets_type_byte() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        session.set_object_scale(
            7,
            Vector {
                x: 2.0,
                y: 2.0,
                z: 2.0,
            },
            true,
            true,
            now,
        )?;
        let sent = drain(&mut session)?;
        let update = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::MultipleObjectUpdate(update) => Some(update),
                _ => None,
            })
            .ok_or("expected a MultipleObjectUpdate")?;
        let block = update
            .object_data
            .first()
            .ok_or("expected one ObjectData block")?;
        // SCALE (0x04) | LINK_SET (0x08) | UNIFORM (0x10) = 0x1C.
        assert_eq!(block.r#type, 0x1C);
        assert_eq!(block.data.len(), 12);
        Ok(())
    }

    #[test]
    fn touch_object_sends_grab_then_degrab() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        session.touch_object(55, now)?;
        let sent = drain(&mut session)?;
        let grab = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::ObjectGrab(grab) => Some(grab),
                _ => None,
            })
            .ok_or("expected an ObjectGrab")?;
        assert_eq!(grab.object_data.local_id, 55);
        assert!(grab.surface_info.is_empty());
        let degrab = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::ObjectDeGrab(degrab) => Some(degrab),
                _ => None,
            })
            .ok_or("expected an ObjectDeGrab")?;
        assert_eq!(degrab.object_data.local_id, 55);
        Ok(())
    }

    #[test]
    fn set_object_name_sends_object_name() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        session.set_object_name(9, "Vendor", now)?;
        let sent = drain(&mut session)?;
        let name = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::ObjectName(name) => Some(name),
                _ => None,
            })
            .ok_or("expected an ObjectName")?;
        let block = name.object_data.first().ok_or("expected one block")?;
        assert_eq!(block.local_id, 9);
        assert_eq!(block.name, b"Vendor");
        Ok(())
    }

    #[test]
    fn delete_objects_sends_object_delete() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        session.delete_objects(&[11, 12], now)?;
        let sent = drain(&mut session)?;
        let delete = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::ObjectDelete(delete) => Some(delete),
                _ => None,
            })
            .ok_or("expected an ObjectDelete")?;
        assert!(!delete.agent_data.force);
        let ids: Vec<u32> = delete
            .object_data
            .iter()
            .map(|b| b.object_local_id)
            .collect();
        assert_eq!(ids, vec![11, 12]);
        Ok(())
    }

    #[test]
    fn derez_objects_sends_derez_object() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let folder = uuid::Uuid::from_u128(0xF0_1DE2);
        session.derez_objects(
            &[21],
            DeRezDestination::TakeIntoAgentInventory,
            folder,
            uuid::Uuid::from_u128(0x7),
            uuid::Uuid::nil(),
            now,
        )?;
        let sent = drain(&mut session)?;
        let derez = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::DeRezObject(derez) => Some(derez),
                _ => None,
            })
            .ok_or("expected a DeRezObject")?;
        assert_eq!(derez.agent_block.destination, 4); // DRD_TAKE_INTO_AGENT_INVENTORY
        assert_eq!(derez.agent_block.destination_id, folder);
        assert_eq!(derez.agent_block.packet_count, 1);
        assert_eq!(derez.agent_block.packet_number, 0);
        Ok(())
    }

    #[test]
    fn set_object_permissions_sends_object_permissions() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // PERM_COPY = 0x8000 in the LSL permission flags.
        session.set_object_permissions(&[31], PermissionField::NextOwner, false, 0x8000, now)?;
        let sent = drain(&mut session)?;
        let perms = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::ObjectPermissions(perms) => Some(perms),
                _ => None,
            })
            .ok_or("expected an ObjectPermissions")?;
        assert!(!perms.header_data.r#override);
        let block = perms.object_data.first().ok_or("expected one block")?;
        assert_eq!(block.object_local_id, 31);
        assert_eq!(block.field, 0x10); // PERM_NEXT_OWNER
        assert_eq!(block.set, 0); // clearing
        assert_eq!(block.mask, 0x8000);
        Ok(())
    }

    #[test]
    fn link_objects_sends_object_link_root_first() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        session.link_objects(&[100, 101, 102], now)?;
        let sent = drain(&mut session)?;
        let link = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::ObjectLink(link) => Some(link),
                _ => None,
            })
            .ok_or("expected an ObjectLink")?;
        let ids: Vec<u32> = link.object_data.iter().map(|b| b.object_local_id).collect();
        assert_eq!(ids, vec![100, 101, 102]);
        Ok(())
    }

    #[test]
    fn edit_helpers_send_their_messages() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        session.set_object_click_action(1, ClickAction::Buy, now)?;
        session.set_object_material(1, Material::Metal, now)?;
        session.set_object_for_sale(1, SaleType::Copy, 250, now)?;
        session.set_object_flags(
            1,
            &ObjectFlagSettings {
                use_physics: true,
                is_phantom: true,
                ..ObjectFlagSettings::default()
            },
            now,
        )?;
        session.set_object_include_in_search(1, true, now)?;
        let sent = drain(&mut session)?;

        let click = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::ObjectClickAction(c) => c.object_data.first(),
                _ => None,
            })
            .ok_or("expected an ObjectClickAction")?;
        assert_eq!(click.click_action, 2); // CLICK_ACTION_BUY

        let material = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::ObjectMaterial(c) => c.object_data.first(),
                _ => None,
            })
            .ok_or("expected an ObjectMaterial")?;
        assert_eq!(material.material, 1); // LL_MCODE_METAL

        let sale = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::ObjectSaleInfo(c) => c.object_data.first(),
                _ => None,
            })
            .ok_or("expected an ObjectSaleInfo")?;
        assert_eq!(sale.sale_type, 2); // FS_COPY
        assert_eq!(sale.sale_price, 250);

        let flags = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::ObjectFlagUpdate(f) => Some(f),
                _ => None,
            })
            .ok_or("expected an ObjectFlagUpdate")?;
        assert!(flags.agent_data.use_physics);
        assert!(flags.agent_data.is_phantom);
        assert!(!flags.agent_data.is_temporary);

        let search = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::ObjectIncludeInSearch(s) => s.object_data.first(),
                _ => None,
            })
            .ok_or("expected an ObjectIncludeInSearch")?;
        assert!(search.include_in_search);
        Ok(())
    }

    /// Emits `count` bits of `value`, MSB first, little-endian by byte (the low
    /// byte's bits first) — mirroring the viewer's `LLBitPack::bitPack`, the
    /// encoder the terrain decoder must invert.
    fn push_bits(bits: &mut Vec<u8>, value: u32, count: u32) {
        let mut remaining = count;
        let mut byte_shift = 0u32;
        while remaining > 0 {
            let take = remaining.min(8);
            remaining = remaining.wrapping_sub(take);
            let chunk = (value >> byte_shift) & 0xff;
            byte_shift = byte_shift.wrapping_add(8);
            let mut index = take;
            while index > 0 {
                index = index.wrapping_sub(1);
                bits.push(u8::try_from((chunk >> index) & 1).unwrap_or(0));
            }
        }
    }

    /// Packs a bit list into bytes (MSB first), padding the final byte.
    fn pack_bits(bits: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut current = 0u8;
        let mut filled = 0u8;
        for &bit in bits {
            current = (current << 1) | (bit & 1);
            filled = filled.wrapping_add(1);
            if filled == 8 {
                out.push(current);
                current = 0;
                filled = 0;
            }
        }
        if filled > 0 {
            current <<= 8u8.wrapping_sub(filled);
            out.push(current);
        }
        out
    }

    /// Builds a `LayerData` LAND message carrying one flat patch at grid
    /// (`patch_x`, `patch_y`): all DCT coefficients zero (an immediate
    /// end-of-block), so every cell decodes to `range/2 + dc_offset`.
    fn flat_land_layer(patch_x: u32, patch_y: u32, dc_offset: f32, range: u32) -> AnyMessage {
        let mut bits = Vec::new();
        // Group header: stride, patch size 16, layer type 'L'.
        push_bits(&mut bits, 264, 16);
        push_bits(&mut bits, 16, 8);
        push_bits(&mut bits, u32::from(b'L'), 8);
        // Patch header: prequant 10 (high nibble 8), wbits 2 (low nibble 0).
        push_bits(&mut bits, 0x80, 8);
        push_bits(&mut bits, dc_offset.to_bits(), 32);
        push_bits(&mut bits, range, 16);
        push_bits(&mut bits, (patch_x << 5) | patch_y, 10);
        // Patch data: `10` => end-of-block (all coefficients zero).
        push_bits(&mut bits, 1, 1);
        push_bits(&mut bits, 0, 1);
        // End of patches.
        push_bits(&mut bits, 97, 8);
        AnyMessage::LayerData(LayerData {
            layer_id: LayerDataLayerIDBlock { r#type: b'L' },
            layer_data: LayerDataLayerDataBlock {
                data: pack_bits(&bits),
            },
        })
    }

    #[test]
    fn layer_data_decodes_terrain_patch() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // An object first, so the session learns the sim's region handle (the
        // LayerData message itself carries none).
        let object = object_update(
            7,
            0x1234,
            Vector {
                x: 1.0,
                y: 2.0,
                z: 3.0,
            },
        );
        session.handle_datagram(sim_addr(), &server_message(&object, 5, true)?, now)?;
        drain_events(&mut session);

        // Patch (1, 2), flat height range/2 + dc_offset = 8/2 + 22 = 26.
        let layer = flat_land_layer(1, 2, 22.0, 8);
        session.handle_datagram(sim_addr(), &server_message(&layer, 6, false)?, now)?;
        let events = drain_events(&mut session);
        let Some(Event::TerrainPatch(patch)) =
            events.iter().find(|e| matches!(e, Event::TerrainPatch(_)))
        else {
            return Err(format!("expected TerrainPatch, got {events:?}").into());
        };
        assert_eq!(patch.layer, TerrainLayerType::Land);
        assert_eq!(patch.patch_x, 1);
        assert_eq!(patch.patch_y, 2);
        assert_eq!(patch.size, 16);
        assert_eq!(patch.region_handle, OBJ_REGION);
        assert!((patch.value(0, 0).ok_or("cell 0,0")? - 26.0).abs() < 1e-3);

        // It is in the public cache, addressable by region-local cell. Patch
        // (1, 2) covers region cells x in 16..32, y in 32..48.
        let height = session.terrain_height(20, 40).ok_or("height at (20,40)")?;
        assert!((height - 26.0).abs() < 1e-3, "height {height} != 26.0");
        assert_eq!(session.terrain_patches().count(), 1);
        assert_eq!(session.terrain_patches_in_region(OBJ_REGION).count(), 1);
        Ok(())
    }

    /// A Vivox `ProvisionVoiceAccountRequest` reply surfaces the SIP account
    /// credentials as an [`Event::VoiceAccountProvisioned`].
    #[test]
    fn voice_provision_vivox_surfaces_credentials() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let xml = "<llsd><map>\
             <key>username</key><string>xMjQ1</string>\
             <key>password</key><string>s3cr3t</string>\
             <key>voice_sip_uri_hostname</key><string>sip.example.com</string>\
             <key>voice_account_server_name</key><string>https://vivox.example/api</string>\
             </map></llsd>";
        let body = sl_proto::parse_llsd_xml(xml)?;
        session.handle_caps_event("ProvisionVoiceAccountRequest", &body, now)?;

        let event = drain_events(&mut session)
            .into_iter()
            .find(|event| matches!(event, Event::VoiceAccountProvisioned(_)))
            .ok_or("expected a VoiceAccountProvisioned event")?;
        let Event::VoiceAccountProvisioned(info) = event else {
            return Err("expected VoiceAccountProvisioned".into());
        };
        assert_eq!(info.username.as_deref(), Some("xMjQ1"));
        assert_eq!(info.password.as_deref(), Some("s3cr3t"));
        assert_eq!(info.sip_uri_hostname.as_deref(), Some("sip.example.com"));
        assert_eq!(
            info.account_server_name.as_deref(),
            Some("https://vivox.example/api")
        );
        assert!(!info.is_webrtc());
        Ok(())
    }

    /// A WebRTC `ProvisionVoiceAccountRequest` reply surfaces the JSEP answer and
    /// viewer session.
    #[test]
    fn voice_provision_webrtc_surfaces_jsep_answer() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let xml = "<llsd><map>\
             <key>viewer_session</key><string>sess-9</string>\
             <key>jsep</key><map>\
             <key>type</key><string>answer</string>\
             <key>sdp</key><string>v=0 answer-sdp</string>\
             </map></map></llsd>";
        let body = sl_proto::parse_llsd_xml(xml)?;
        session.handle_caps_event("ProvisionVoiceAccountRequest", &body, now)?;

        let event = drain_events(&mut session)
            .into_iter()
            .find(|event| matches!(event, Event::VoiceAccountProvisioned(_)))
            .ok_or("expected a VoiceAccountProvisioned event")?;
        let Event::VoiceAccountProvisioned(info) = event else {
            return Err("expected VoiceAccountProvisioned".into());
        };
        assert!(info.is_webrtc());
        assert_eq!(info.viewer_session.as_deref(), Some("sess-9"));
        assert_eq!(info.jsep_type.as_deref(), Some("answer"));
        assert_eq!(info.jsep_sdp.as_deref(), Some("v=0 answer-sdp"));
        assert_eq!(info.username, None);
        Ok(())
    }

    /// A `ParcelVoiceInfoRequest` reply surfaces the parcel's voice channel URI.
    #[test]
    fn parcel_voice_info_surfaces_channel() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let xml = "<llsd><map>\
             <key>parcel_local_id</key><integer>7</integer>\
             <key>region_name</key><string>Default Region</string>\
             <key>voice_credentials</key><map>\
             <key>channel_uri</key><string>sip:Region@sip.example.com</string>\
             </map></map></llsd>";
        let body = sl_proto::parse_llsd_xml(xml)?;
        session.handle_caps_event("ParcelVoiceInfoRequest", &body, now)?;

        let event = drain_events(&mut session)
            .into_iter()
            .find(|event| matches!(event, Event::ParcelVoiceInfo(_)))
            .ok_or("expected a ParcelVoiceInfo event")?;
        let Event::ParcelVoiceInfo(info) = event else {
            return Err("expected ParcelVoiceInfo".into());
        };
        assert_eq!(info.parcel_local_id, 7);
        assert_eq!(info.region_name, "Default Region");
        assert_eq!(
            info.channel_uri.as_deref(),
            Some("sip:Region@sip.example.com")
        );
        Ok(())
    }

    /// A `GetExperienceInfo` reply surfaces the requested experiences' metadata,
    /// with an unresolved id folded in as a `missing` placeholder.
    #[test]
    fn get_experience_info_surfaces_records() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let xml = "<llsd><map><key>experience_keys</key><array><map>\
            <key>public_id</key><uuid>11111111-1111-1111-1111-111111111111</uuid>\
            <key>name</key><string>Treasure Hunt</string>\
            <key>properties</key><integer>16</integer>\
            <key>maturity</key><integer>13</integer>\
            </map></array>\
            <key>error_ids</key><array>\
            <uuid>22222222-2222-2222-2222-222222222222</uuid></array></map></llsd>";
        let body = sl_proto::parse_llsd_xml(xml)?;
        session.handle_caps_event("GetExperienceInfo", &body, now)?;

        let event = drain_events(&mut session)
            .into_iter()
            .find(|event| matches!(event, Event::ExperienceInfo(_)))
            .ok_or("expected an ExperienceInfo event")?;
        let Event::ExperienceInfo(infos) = event else {
            return Err("expected ExperienceInfo".into());
        };
        let [first, second] = infos.as_slice() else {
            return Err("expected two experience records".into());
        };
        assert_eq!(first.name, "Treasure Hunt");
        assert!(first.properties.is_grid());
        assert!(!first.missing);
        assert!(second.missing);
        assert!(second.properties.is_invalid());
        Ok(())
    }

    /// A `GetExperiences` reply surfaces the agent's allowed/blocked experiences.
    #[test]
    fn get_experiences_surfaces_permissions() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let xml = "<llsd><map>\
            <key>experiences</key><array>\
            <uuid>11111111-1111-1111-1111-111111111111</uuid></array>\
            <key>blocked</key><array>\
            <uuid>22222222-2222-2222-2222-222222222222</uuid></array></map></llsd>";
        let body = sl_proto::parse_llsd_xml(xml)?;
        session.handle_caps_event("GetExperiences", &body, now)?;

        let event = drain_events(&mut session)
            .into_iter()
            .find(|event| matches!(event, Event::ExperiencePermissions { .. }))
            .ok_or("expected an ExperiencePermissions event")?;
        let Event::ExperiencePermissions { allowed, blocked } = event else {
            return Err("expected ExperiencePermissions".into());
        };
        assert_eq!(allowed.len(), 1);
        assert_eq!(blocked.len(), 1);
        Ok(())
    }

    /// A `RegionExperiences` reply surfaces the region's allow/block/trust lists.
    #[test]
    fn region_experiences_surfaces_lists() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let xml = "<llsd><map>\
            <key>allowed</key><array>\
            <uuid>11111111-1111-1111-1111-111111111111</uuid></array>\
            <key>blocked</key><array></array>\
            <key>trusted</key><array>\
            <uuid>33333333-3333-3333-3333-333333333333</uuid></array></map></llsd>";
        let body = sl_proto::parse_llsd_xml(xml)?;
        session.handle_caps_event("RegionExperiences", &body, now)?;

        let event = drain_events(&mut session)
            .into_iter()
            .find(|event| matches!(event, Event::RegionExperiences { .. }))
            .ok_or("expected a RegionExperiences event")?;
        let Event::RegionExperiences {
            allowed,
            blocked,
            trusted,
        } = event
        else {
            return Err("expected RegionExperiences".into());
        };
        assert_eq!(allowed.len(), 1);
        assert!(blocked.is_empty());
        assert_eq!(trusted.len(), 1);
        Ok(())
    }

    // -- Complete the IM surface (#28) -------------------------------------

    /// Finds the first `ImprovedInstantMessage` message block in `sent`.
    fn first_im(
        sent: &[AnyMessage],
    ) -> Result<&ImprovedInstantMessageMessageBlockBlock, TestError> {
        sent.iter()
            .find_map(|m| match m {
                AnyMessage::ImprovedInstantMessage(im) => Some(&im.message_block),
                _ => None,
            })
            .ok_or_else(|| "expected an ImprovedInstantMessage".into())
    }

    /// Builds an inbound `ImprovedInstantMessage` with a chosen dialog, sender,
    /// id and binary bucket (for the offer / conference decode paths).
    fn inbound_offer_im(
        dialog: u8,
        from: uuid::Uuid,
        id: uuid::Uuid,
        bucket: Vec<u8>,
    ) -> AnyMessage {
        AnyMessage::ImprovedInstantMessage(ImprovedInstantMessage {
            agent_data: ImprovedInstantMessageAgentDataBlock {
                agent_id: from,
                session_id: uuid::Uuid::nil(),
            },
            message_block: ImprovedInstantMessageMessageBlockBlock {
                from_group: false,
                to_agent_id: uuid::Uuid::from_u128(1),
                parent_estate_id: 1,
                region_id: uuid::Uuid::from_u128(0x7),
                position: vec3(1.0, 2.0, 3.0),
                offline: 0,
                dialog,
                id,
                timestamp: 0,
                from_agent_name: b"Sender Name\0".to_vec(),
                message: b"an item\0".to_vec(),
                binary_bucket: bucket,
            },
            estate_block: ImprovedInstantMessageEstateBlockBlock { estate_id: 1 },
            meta_data: Vec::new(),
        })
    }

    #[test]
    fn offer_teleport_packs_start_lure() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let a = uuid::Uuid::from_u128(0xA1);
        let b = uuid::Uuid::from_u128(0xB2);
        session.offer_teleport(&[a, b], "come over", now)?;
        let sent = drain(&mut session)?;
        let lure = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::StartLure(l) => Some(l),
                _ => None,
            })
            .ok_or("expected a StartLure")?;
        assert_eq!(lure.info.lure_type, 0);
        assert_eq!(trimmed(&lure.info.message), "come over");
        let targets: Vec<_> = lure.target_data.iter().map(|t| t.target_id).collect();
        assert_eq!(targets, vec![a, b]);
        Ok(())
    }

    #[test]
    fn accept_teleport_lure_packs_request() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let lure_id = uuid::Uuid::from_u128(0xCAFE);
        session.accept_teleport_lure(lure_id, now)?;
        let sent = drain(&mut session)?;
        let req = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::TeleportLureRequest(r) => Some(r),
                _ => None,
            })
            .ok_or("expected a TeleportLureRequest")?;
        assert_eq!(req.info.lure_id, lure_id);
        assert_eq!(req.info.teleport_flags, 4); // TELEPORT_FLAGS_VIA_LURE
        Ok(())
    }

    #[test]
    fn decline_teleport_lure_packs_im() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let from = uuid::Uuid::from_u128(0x55);
        let lure_id = uuid::Uuid::from_u128(0xCAFE);
        session.decline_teleport_lure(from, lure_id, now)?;
        let sent = drain(&mut session)?;
        let block = first_im(&sent)?;
        assert_eq!(block.dialog, 24); // IM_LURE_DECLINED
        assert_eq!(block.id, lure_id);
        assert_eq!(block.to_agent_id, from);
        assert_eq!(block.message, b"\0");
        Ok(())
    }

    #[test]
    fn request_teleport_packs_im() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let target = uuid::Uuid::from_u128(0x77);
        session.request_teleport(target, "please tp me", now)?;
        let sent = drain(&mut session)?;
        let block = first_im(&sent)?;
        assert_eq!(block.dialog, 26); // IM_TELEPORT_REQUEST
        assert_eq!(block.id, uuid::Uuid::nil());
        assert_eq!(block.to_agent_id, target);
        assert_eq!(trimmed(&block.message), "please tp me");
        Ok(())
    }

    #[test]
    fn give_inventory_packs_offer() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let to = uuid::Uuid::from_u128(0xD1);
        let item = uuid::Uuid::from_u128(0x1234);
        let tx = uuid::Uuid::from_u128(0x9999);
        session.give_inventory(to, item, AssetType::Notecard, "My Card", tx, now)?;
        let sent = drain(&mut session)?;
        let block = first_im(&sent)?;
        assert_eq!(block.dialog, 4); // IM_INVENTORY_OFFERED
        assert_eq!(block.id, tx);
        assert_eq!(block.to_agent_id, to);
        assert_eq!(trimmed(&block.message), "My Card");
        // Bucket: [asset-type byte][16-byte item id]; AT_NOTECARD = 7.
        assert_eq!(block.binary_bucket.first().copied(), Some(7u8));
        let id_bytes = block
            .binary_bucket
            .get(1..17)
            .ok_or("inventory-offer bucket too short")?;
        assert_eq!(id_bytes, item.as_bytes());
        Ok(())
    }

    #[test]
    fn give_inventory_folder_leads_with_folder_type() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let folder = uuid::Uuid::from_u128(0x4321);
        session.give_inventory_folder(
            uuid::Uuid::from_u128(0xD1),
            folder,
            "My Folder",
            uuid::Uuid::from_u128(0x9999),
            now,
        )?;
        let sent = drain(&mut session)?;
        let block = first_im(&sent)?;
        assert_eq!(block.dialog, 4);
        // A folder offer leads with AT_CATEGORY (8).
        assert_eq!(block.binary_bucket.first().copied(), Some(8u8));
        let id_bytes = block
            .binary_bucket
            .get(1..17)
            .ok_or("folder-offer bucket too short")?;
        assert_eq!(id_bytes, folder.as_bytes());
        Ok(())
    }

    #[test]
    fn inventory_offer_decodes_and_accepts() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let item = uuid::Uuid::from_u128(0x1234);
        let from = uuid::Uuid::from_u128(0x55);
        let tx = uuid::Uuid::from_u128(0xABC);
        let mut bucket = vec![7u8]; // AT_NOTECARD
        bucket.extend_from_slice(item.as_bytes());
        let im = inbound_offer_im(4, from, tx, bucket);
        let datagram = server_message(&im, 9, false)?;
        session.handle_datagram(sim_addr(), &datagram, now)?;
        let received = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::InstantMessageReceived(im) => Some(im),
                _ => None,
            })
            .ok_or("expected an InstantMessageReceived event")?;
        let offer = received
            .inventory_offer()
            .ok_or("expected a decoded inventory offer")?;
        assert_eq!(offer.asset_type, AssetType::Notecard);
        assert_eq!(offer.item_id, item);
        assert_eq!(offer.transaction_id, tx);
        assert_eq!(offer.from_agent_id, from);
        assert!(!offer.from_task);

        // Accept files the item into a destination folder.
        let folder = uuid::Uuid::from_u128(0xF0);
        session.accept_inventory_offer(&offer, folder, now)?;
        let accept = drain(&mut session)?;
        let block = first_im(&accept)?;
        assert_eq!(block.dialog, 5); // IM_INVENTORY_ACCEPTED
        assert_eq!(block.id, tx);
        assert_eq!(block.to_agent_id, from);
        assert_eq!(block.binary_bucket, folder.as_bytes());

        // Decline routes to the trash folder.
        let trash = uuid::Uuid::from_u128(0x7A);
        session.decline_inventory_offer(&offer, trash, now)?;
        let decline = drain(&mut session)?;
        let block = first_im(&decline)?;
        assert_eq!(block.dialog, 6); // IM_INVENTORY_DECLINED
        assert_eq!(block.binary_bucket, trash.as_bytes());
        Ok(())
    }

    #[test]
    fn start_conference_packs_conference_start() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let session_id = uuid::Uuid::from_u128(0x5E51);
        let a = uuid::Uuid::from_u128(0xA1);
        let b = uuid::Uuid::from_u128(0xB2);
        session.start_conference(session_id, &[a, b], "hello all", now)?;
        let sent = drain(&mut session)?;
        let block = first_im(&sent)?;
        assert_eq!(block.dialog, 16); // IM_SESSION_CONFERENCE_START
        assert_eq!(block.id, session_id);
        assert_eq!(block.to_agent_id, a); // first invitee
        assert_eq!(trimmed(&block.message), "hello all");
        let mut expected = a.as_bytes().to_vec();
        expected.extend_from_slice(b.as_bytes());
        assert_eq!(block.binary_bucket, expected);
        Ok(())
    }

    #[test]
    fn send_and_leave_conference_pack_session_ims() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let session_id = uuid::Uuid::from_u128(0x5E51);
        session.send_conference_message(session_id, "hi", now)?;
        session.leave_conference(session_id, now)?;
        let sent = drain(&mut session)?;
        let blocks: Vec<_> = sent
            .iter()
            .filter_map(|m| match m {
                AnyMessage::ImprovedInstantMessage(im) => Some(&im.message_block),
                _ => None,
            })
            .collect();
        let send = blocks
            .iter()
            .find(|b| b.dialog == 17)
            .ok_or("expected a SessionSend IM")?;
        assert!(!send.from_group); // a conference, not a group
        assert_eq!(send.id, session_id);
        assert_eq!(send.to_agent_id, session_id);
        assert_eq!(trimmed(&send.message), "hi");
        let leave = blocks
            .iter()
            .find(|b| b.dialog == 18)
            .ok_or("expected a SessionLeave IM")?;
        assert_eq!(leave.id, session_id);
        Ok(())
    }

    #[test]
    fn inbound_conference_send_surfaces_event() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // Dialog 17 (IM_SESSION_SEND) with from_group clear is a conference.
        let session_id = uuid::Uuid::from_u128(0xABC);
        let im = inbound_offer_im(17, uuid::Uuid::from_u128(0x55), session_id, Vec::new());
        let datagram = server_message(&im, 9, false)?;
        session.handle_datagram(sim_addr(), &datagram, now)?;
        let event = drain_events(&mut session)
            .into_iter()
            .find(|event| matches!(event, Event::ConferenceSessionMessage { .. }))
            .ok_or("expected a ConferenceSessionMessage event")?;
        let Event::ConferenceSessionMessage {
            session_id: got, ..
        } = event
        else {
            return Err("expected ConferenceSessionMessage".into());
        };
        assert_eq!(got, session_id);
        Ok(())
    }

    #[test]
    fn retrieve_instant_messages_packs_request() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        session.retrieve_instant_messages(now)?;
        let sent = drain(&mut session)?;
        assert!(
            sent.iter()
                .any(|m| matches!(m, AnyMessage::RetrieveInstantMessages(_))),
            "expected a RetrieveInstantMessages"
        );
        Ok(())
    }

    #[test]
    fn read_offline_msgs_caps_surfaces_offline_ims() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let from = uuid::Uuid::from_u128(0x55);
        let xml = format!(
            "<llsd><array><map>\
               <key>from_agent_id</key><uuid>{from}</uuid>\
               <key>from_agent_name</key><string>Sender Name</string>\
               <key>to_agent_id</key><uuid>{}</uuid>\
               <key>dialog</key><integer>0</integer>\
               <key>message</key><string>stored hello</string>\
               <key>timestamp</key><integer>1700000000</integer>\
               <key>transaction-id</key><uuid>{}</uuid>\
             </map></array></llsd>",
            uuid::Uuid::from_u128(1),
            uuid::Uuid::from_u128(0xABC),
        );
        let body = parse_llsd_xml(&xml)?;
        session.handle_caps_event("ReadOfflineMsgs", &body, now)?;
        let received = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::InstantMessageReceived(im) => Some(im),
                _ => None,
            })
            .ok_or("expected an InstantMessageReceived event")?;
        assert!(received.offline);
        assert_eq!(received.from_agent_id, from);
        assert_eq!(received.from_agent_name, "Sender Name");
        assert_eq!(received.message, "stored hello");
        assert_eq!(received.timestamp, 1_700_000_000);
        assert_eq!(received.id, uuid::Uuid::from_u128(0xABC));
        Ok(())
    }

    #[test]
    fn chatterbox_invitation_surfaces_conference_invited() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let session_id = uuid::Uuid::from_u128(0x5E51);
        let from = uuid::Uuid::from_u128(0x55);
        let xml = format!(
            "<llsd><map><key>instantmessage</key><map>\
               <key>message_params</key><map>\
                 <key>id</key><uuid>{session_id}</uuid>\
                 <key>from_id</key><uuid>{from}</uuid>\
                 <key>from_name</key><string>Inviter</string>\
                 <key>message</key><string>join us</string>\
               </map></map></map></llsd>"
        );
        let body = parse_llsd_xml(&xml)?;
        session.handle_caps_event("ChatterBoxInvitation", &body, now)?;
        let event = drain_events(&mut session)
            .into_iter()
            .find(|event| matches!(event, Event::ConferenceInvited { .. }))
            .ok_or("expected a ConferenceInvited event")?;
        let Event::ConferenceInvited {
            session_id: got,
            from_agent_id,
            from_name,
            message,
        } = event
        else {
            return Err("expected ConferenceInvited".into());
        };
        assert_eq!(got, session_id);
        assert_eq!(from_agent_id, from);
        assert_eq!(from_name, "Inviter");
        assert_eq!(message, "join us");
        Ok(())
    }

    // ---- Inventory mutation (#30) ------------------------------------------

    /// Builds an [`InventoryItem`] with a single non-default field for tests.
    fn sample_item(item_id: uuid::Uuid, folder_id: uuid::Uuid, name: &str) -> InventoryItem {
        InventoryItem {
            item_id,
            folder_id,
            name: name.to_owned(),
            description: String::new(),
            asset_id: uuid::Uuid::nil(),
            item_type: 0,
            inv_type: 0,
            flags: 0,
            sale_type: 0,
            sale_price: 0,
            creation_date: 0,
            owner_id: uuid::Uuid::nil(),
            last_owner_id: uuid::Uuid::nil(),
            creator_id: uuid::Uuid::nil(),
            group_id: uuid::Uuid::nil(),
            group_owned: false,
            base_mask: 0,
            owner_mask: 0,
            group_mask: 0,
            everyone_mask: 0,
            next_owner_mask: 0,
        }
    }

    #[test]
    fn create_inventory_folder_sends_and_caches() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let folder_id = uuid::Uuid::from_u128(0xF0);
        let parent_id = uuid::Uuid::from_u128(0x10);
        session.create_inventory_folder(folder_id, parent_id, 8, "Toys & Co", now)?;
        let sent = drain(&mut session)?;
        let create = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::CreateInventoryFolder(create) => Some(create),
                _ => None,
            })
            .ok_or("expected a CreateInventoryFolder")?;
        assert_eq!(create.folder_data.folder_id, folder_id);
        assert_eq!(create.folder_data.parent_id, parent_id);
        assert_eq!(create.folder_data.r#type, 8);
        // The name carries a trailing NUL, as a real viewer sends.
        assert_eq!(create.folder_data.name, b"Toys & Co\0");

        // The folder is in the cache optimistically (no reply on this path).
        let cached = session
            .inventory_folder(folder_id)
            .ok_or("folder should be cached")?;
        assert_eq!(cached.name, "Toys & Co");
        assert_eq!(cached.parent_id, parent_id);

        // Removing it drops it from the cache.
        session.remove_inventory_folders(&[folder_id], now)?;
        let sent = drain(&mut session)?;
        assert!(
            sent.iter()
                .any(|m| matches!(m, AnyMessage::RemoveInventoryFolder(_))),
            "expected a RemoveInventoryFolder"
        );
        assert!(
            session.inventory_folder(folder_id).is_none(),
            "folder should be uncached after removal"
        );
        Ok(())
    }

    #[test]
    fn create_inventory_item_sends_with_callback() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let new = NewInventoryItem {
            folder_id: uuid::Uuid::from_u128(0x11),
            transaction_id: uuid::Uuid::nil(),
            next_owner_mask: 0x0008_e000,
            asset_type: 7, // notecard
            inv_type: 7,
            wearable_type: 0,
            name: "Notes".to_owned(),
            description: "a note".to_owned(),
        };
        let callback_id = session.create_inventory_item(&new, now)?;
        assert_eq!(callback_id, 1, "first callback id should be 1");
        let sent = drain(&mut session)?;
        let create = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::CreateInventoryItem(create) => Some(create),
                _ => None,
            })
            .ok_or("expected a CreateInventoryItem")?;
        assert_eq!(create.inventory_block.callback_id, 1);
        assert_eq!(
            create.inventory_block.folder_id,
            uuid::Uuid::from_u128(0x11)
        );
        assert_eq!(create.inventory_block.r#type, 7);
        assert_eq!(create.inventory_block.name, b"Notes\0");
        assert_eq!(create.inventory_block.description, b"a note\0");
        Ok(())
    }

    #[test]
    fn update_inventory_item_sends_golden_crc() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // An item whose only non-nil field is item_id == 1. Its LL checksum is
        // uuid_crc(1) = the low 16 bytes read as four LE u32s, summed: the last
        // chunk [0,0,0,1] => 1 << 24.
        let item = sample_item(uuid::Uuid::from_u128(1), uuid::Uuid::nil(), "Renamed");
        session.update_inventory_item(&item, uuid::Uuid::nil(), now)?;
        let sent = drain(&mut session)?;
        let update = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::UpdateInventoryItem(update) => Some(update),
                _ => None,
            })
            .ok_or("expected an UpdateInventoryItem")?;
        let data = update
            .inventory_data
            .first()
            .ok_or("expected one inventory-data block")?;
        assert_eq!(data.item_id, uuid::Uuid::from_u128(1));
        assert_eq!(data.name, b"Renamed\0");
        assert_eq!(data.crc, 0x0100_0000);

        // The optimistic cache holds the updated item.
        let cached = session
            .inventory_item(uuid::Uuid::from_u128(1))
            .ok_or("item should be cached")?;
        assert_eq!(cached.name, "Renamed");
        Ok(())
    }

    #[test]
    fn move_inventory_item_sends_move() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let item_id = uuid::Uuid::from_u128(0x21);
        let folder_id = uuid::Uuid::from_u128(0x22);
        session.move_inventory_item(item_id, folder_id, "NewName", now)?;
        let sent = drain(&mut session)?;
        let mv = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::MoveInventoryItem(mv) => Some(mv),
                _ => None,
            })
            .ok_or("expected a MoveInventoryItem")?;
        assert!(!mv.agent_data.stamp);
        let data = mv.inventory_data.first().ok_or("expected one move block")?;
        assert_eq!(data.item_id, item_id);
        assert_eq!(data.folder_id, folder_id);
        assert_eq!(data.new_name, b"NewName\0");
        Ok(())
    }

    #[test]
    fn update_create_inventory_item_surfaces_event_and_caches() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let item_id = uuid::Uuid::from_u128(0x31);
        let message = AnyMessage::UpdateCreateInventoryItem(UpdateCreateInventoryItem {
            agent_data: UpdateCreateInventoryItemAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(1),
                sim_approved: true,
                transaction_id: uuid::Uuid::from_u128(0x99),
            },
            inventory_data: vec![UpdateCreateInventoryItemInventoryDataBlock {
                item_id,
                folder_id: uuid::Uuid::from_u128(0x32),
                callback_id: 7,
                creator_id: uuid::Uuid::from_u128(2),
                owner_id: uuid::Uuid::from_u128(1),
                group_id: uuid::Uuid::nil(),
                base_mask: 0x7fff_ffff,
                owner_mask: 0x7fff_ffff,
                group_mask: 0,
                everyone_mask: 0,
                next_owner_mask: 0x0008_e000,
                group_owned: false,
                asset_id: uuid::Uuid::from_u128(0x55),
                r#type: 7,
                inv_type: 7,
                flags: 0,
                sale_type: 0,
                sale_price: 0,
                name: b"Fresh Note\0".to_vec(),
                description: b"\0".to_vec(),
                creation_date: 1234,
                crc: 0,
            }],
        });
        let datagram = server_message(&message, 40, false)?;
        session.handle_datagram(sim_addr(), &datagram, now)?;

        let event = drain_events(&mut session)
            .into_iter()
            .find(|event| matches!(event, Event::InventoryItemCreated { .. }))
            .ok_or("expected an InventoryItemCreated event")?;
        let Event::InventoryItemCreated {
            sim_approved,
            callback_id,
            item,
            ..
        } = event
        else {
            return Err("expected InventoryItemCreated".into());
        };
        assert!(sim_approved);
        assert_eq!(callback_id, 7);
        assert_eq!(item.name, "Fresh Note");
        assert_eq!(item.asset_id, uuid::Uuid::from_u128(0x55));

        let cached = session
            .inventory_item(item_id)
            .ok_or("item should be cached")?;
        assert_eq!(cached.name, "Fresh Note");
        Ok(())
    }

    #[test]
    fn bulk_update_inventory_surfaces_event_and_caches() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let folder_id = uuid::Uuid::from_u128(0x41);
        let item_id = uuid::Uuid::from_u128(0x42);
        let message = AnyMessage::BulkUpdateInventory(BulkUpdateInventory {
            agent_data: BulkUpdateInventoryAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(1),
                transaction_id: uuid::Uuid::from_u128(0xAB),
            },
            folder_data: vec![BulkUpdateInventoryFolderDataBlock {
                folder_id,
                parent_id: uuid::Uuid::from_u128(0x40),
                r#type: 8,
                name: b"Copied Folder\0".to_vec(),
            }],
            item_data: vec![BulkUpdateInventoryItemDataBlock {
                item_id,
                callback_id: 0,
                folder_id,
                creator_id: uuid::Uuid::from_u128(2),
                owner_id: uuid::Uuid::from_u128(1),
                group_id: uuid::Uuid::nil(),
                base_mask: 0x7fff_ffff,
                owner_mask: 0x7fff_ffff,
                group_mask: 0,
                everyone_mask: 0,
                next_owner_mask: 0,
                group_owned: false,
                asset_id: uuid::Uuid::from_u128(0x56),
                r#type: 0,
                inv_type: 0,
                flags: 0,
                sale_type: 0,
                sale_price: 0,
                name: b"Copied Item\0".to_vec(),
                description: b"\0".to_vec(),
                creation_date: 99,
                crc: 0,
            }],
        });
        let datagram = server_message(&message, 41, false)?;
        session.handle_datagram(sim_addr(), &datagram, now)?;

        let event = drain_events(&mut session)
            .into_iter()
            .find(|event| matches!(event, Event::InventoryBulkUpdate { .. }))
            .ok_or("expected an InventoryBulkUpdate event")?;
        let Event::InventoryBulkUpdate {
            transaction_id,
            folders,
            items,
        } = event
        else {
            return Err("expected InventoryBulkUpdate".into());
        };
        assert_eq!(transaction_id, uuid::Uuid::from_u128(0xAB));
        assert_eq!(folders.len(), 1);
        assert_eq!(items.len(), 1);

        assert_eq!(
            session
                .inventory_folder(folder_id)
                .ok_or("folder should be cached")?
                .name,
            "Copied Folder"
        );
        assert_eq!(
            session
                .inventory_item(item_id)
                .ok_or("item should be cached")?
                .name,
            "Copied Item"
        );
        Ok(())
    }
}
