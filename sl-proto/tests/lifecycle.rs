//! Scripted-peer, simulated-clock tests for the full session lifecycle:
//! login -> circuit -> handshake -> keep-alive -> logout.

#[cfg(test)]
mod test {
    use std::error::Error;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::time::{Duration, Instant};

    use pretty_assertions::{assert_eq, assert_ne};
    use sl_proto::{
        AbuseReport, AbuseReportType, AgentKey, AnimationKey, AssetKey, AssetType, AttachmentMode,
        AttachmentPoint, Camera, ChatAudible, ChatChannel, ChatLifecycleView, ChatSessionInfo,
        ChatSessionKind, ChatSessionLifecycle, ChatSource, ChatType, Child, ClassifiedCategory,
        ClassifiedKey, ClassifiedUpdate, ClickAction, CloudPosDensity, CoarseLocation, Color,
        ColorAlpha, ControlFlags, CreateGroupParams, DayCycle, DayCycleFrame, DeRezDestination,
        DetachOrder, Diagnostic, DirFindFlags, Direction, DirectoryVisibility, DisconnectReason,
        DisplayName, DisplayNameUpdate, Distance, EjectAction, EnvironmentSettings,
        EstateAccessDelta, EstateAccessKind, Event, EventId, FolderInfo, FolderState, FolderType,
        FollowCamProperty, FreezeAction, FriendKey, FriendPresence, FriendRights,
        GestureActivation, GlobalCoordinates, Glow, GodRegionUpdate, GridCoordinates, GroupKey,
        GroupNoticeAttachment, GroupRequestId, GroupRoleChange, GroupRoleEdit, GroupRoleKey,
        GroupRoleMemberChange, GroupRoleUpdateType, INVENTORY_FETCH_MAX_IN_FLIGHT, ImDialog,
        ImSessionId, ImageCodec, InterestsUpdate, InventoryCallbackId, InventoryFolder,
        InventoryFolderKey, InventoryItem, InventoryItemMove, InventoryItemOrFolderKey,
        InventoryKey, InventoryOwner, InventoryType, InviteChannel, ItemInfo, LandArea,
        LandBrushAction, LandBrushSize, LandEdit, LandingType, LightData, LindenAmount,
        LindenBalance, LoginAccount, LoginParams, LookAtType, LureId, MapItemType, Material,
        Maturity, MeanCollisionType, MeshKey, MoneyTransactionType, MovementMode, MuteFlags,
        MuteType, NavMeshBuildStatus, NavMeshStatus, NewInventoryItem, NewInventoryLink,
        NotecardRez, ObjectBuyItem, ObjectExtraParams, ObjectFlagSettings, ObjectKey,
        ObjectTransform, OwnerKey, ParcelAccessEntry, ParcelAccessFlags, ParcelAccessScope,
        ParcelCategory, ParcelFlags, ParcelKey, ParcelMediaCommand, ParcelRequestResult,
        ParcelReturnType, ParcelStatus, ParcelUpdate, PendingInvite, PermissionField, Permissions,
        Permissions5, PickUpdate, PointAtType, Postcard, PrimShape, PrimShapeParams, ProductType,
        ProfileUpdate, QueryId, ReflectionProbeFlags, RegionCoordinates, RegionHandle,
        RegionInfoUpdate, RegionName, Reliability, RequiredVoiceVersion, RestoreItem,
        RezAttachment, RezObjectParams, RezScriptParams, SaleType, Scale, ScopedObjectId,
        ScopedParcelId, ScriptControlAction, ScriptPermissionStatus, ScriptPermissions,
        SculptOrMeshKey, Session, SessionMessage, SetDisplayNameReply, SimStatId,
        SimWideDeleteFlags, SimulatorTime, SkySettings, SoundFlags, StartLocationSlot,
        TaskInventoryKey, TaskInventoryReply, TeleportFlags, TerraformArea, TerrainLayerType,
        TextureEntry, TextureFace, TextureKey, Throttle, TransactionId, TransferStatus, Transmit,
        UpdateGroupInfoParams, UserInfo, ViewerEffect, ViewerEffectData, ViewerEffectType,
        WaterSettings, WearableType, avatar_texture, chat_session_request_body,
        decode_texture_entry, group_powers, pcode,
    };
    use sl_types::lsl::{Rotation, Vector};
    use sl_wire::messages::{
        AcceptCallingCard, AcceptCallingCardAgentDataBlock, AcceptCallingCardFolderDataBlock,
        AcceptCallingCardTransactionBlockBlock, AgentDataUpdate, AgentDataUpdateAgentDataBlock,
        AgentGroupDataUpdate, AgentGroupDataUpdateAgentDataBlock,
        AgentGroupDataUpdateGroupDataBlock, AgentMovementComplete,
        AgentMovementCompleteAgentDataBlock, AgentMovementCompleteDataBlock,
        AgentMovementCompleteSimDataBlock, AgentWearablesUpdate,
        AgentWearablesUpdateAgentDataBlock, AgentWearablesUpdateWearableDataBlock,
        AssetUploadComplete, AssetUploadCompleteAssetBlockBlock, AttachedSound,
        AttachedSoundDataBlockBlock, AvatarAnimation, AvatarAnimationAnimationListBlock,
        AvatarAnimationAnimationSourceListBlock, AvatarAnimationPhysicalAvatarEventListBlock,
        AvatarAnimationSenderBlock, AvatarAppearance, AvatarAppearanceObjectDataBlock,
        AvatarAppearanceSenderBlock, AvatarAppearanceVisualParamBlock, AvatarClassifiedReply,
        AvatarClassifiedReplyAgentDataBlock, AvatarClassifiedReplyDataBlock, AvatarNotesReply,
        AvatarNotesReplyAgentDataBlock, AvatarNotesReplyDataBlock, AvatarPicksReply,
        AvatarPicksReplyAgentDataBlock, AvatarPicksReplyDataBlock, AvatarPropertiesReply,
        AvatarPropertiesReplyAgentDataBlock, AvatarPropertiesReplyPropertiesDataBlock,
        AvatarSitResponse, AvatarSitResponseSitObjectBlock, AvatarSitResponseSitTransformBlock,
        BulkUpdateInventory, BulkUpdateInventoryAgentDataBlock, BulkUpdateInventoryFolderDataBlock,
        BulkUpdateInventoryItemDataBlock, ChangeUserRights, ChangeUserRightsAgentDataBlock,
        ChangeUserRightsRightsBlock, ChatFromSimulator, ChatFromSimulatorChatDataBlock,
        ClassifiedInfoReply, ClassifiedInfoReplyAgentDataBlock, ClassifiedInfoReplyDataBlock,
        CoarseLocationUpdate, CoarseLocationUpdateAgentDataBlock, CoarseLocationUpdateIndexBlock,
        CoarseLocationUpdateLocationBlock, ConfirmXferPacket, ConfirmXferPacketXferIDBlock,
        CrossedRegion, CrossedRegionAgentDataBlock, CrossedRegionInfoBlock,
        CrossedRegionRegionDataBlock, DeRezAck, DeRezAckTransactionDataBlock, DeclineCallingCard,
        DeclineCallingCardAgentDataBlock, DeclineCallingCardTransactionBlockBlock, DirPeopleReply,
        DirPeopleReplyAgentDataBlock, DirPeopleReplyQueryDataBlock,
        DirPeopleReplyQueryRepliesBlock, DisableSimulator, EconomyData, EconomyDataInfoBlock,
        EjectGroupMemberReply, EjectGroupMemberReplyAgentDataBlock,
        EjectGroupMemberReplyEjectDataBlock, EjectGroupMemberReplyGroupDataBlock,
        Error as ErrorMessage, ErrorAgentDataBlock, ErrorDataBlock, EstateCovenantReply,
        EstateCovenantReplyDataBlock, EstateOwnerMessage, EstateOwnerMessageAgentDataBlock,
        EstateOwnerMessageMethodDataBlock, EstateOwnerMessageParamListBlock, EventInfoReply,
        EventInfoReplyAgentDataBlock, EventInfoReplyEventDataBlock,
        FeatureDisabled as WireFeatureDisabled, FeatureDisabledFailureInfoBlock, FindAgent,
        FindAgentAgentBlockBlock, FindAgentLocationBlockBlock, ForceObjectSelect,
        ForceObjectSelectDataBlock, ForceObjectSelectHeaderBlock, GenericMessage,
        GenericMessageAgentDataBlock, GenericMessageMethodDataBlock, GenericMessageParamListBlock,
        GenericStreamingMessage, GenericStreamingMessageDataBlockBlock,
        GenericStreamingMessageMethodDataBlock, GrantGodlikePowers,
        GrantGodlikePowersAgentDataBlock, GrantGodlikePowersGrantDataBlock,
        GroupAccountSummaryReply, GroupAccountSummaryReplyAgentDataBlock,
        GroupAccountSummaryReplyMoneyDataBlock, GroupActiveProposalItemReply,
        GroupActiveProposalItemReplyAgentDataBlock, GroupActiveProposalItemReplyProposalDataBlock,
        GroupActiveProposalItemReplyTransactionDataBlock, GroupMembersReply,
        GroupMembersReplyAgentDataBlock, GroupMembersReplyGroupDataBlock,
        GroupMembersReplyMemberDataBlock, GroupProfileReply, GroupProfileReplyAgentDataBlock,
        GroupProfileReplyGroupDataBlock, GroupRoleDataReply, GroupRoleDataReplyAgentDataBlock,
        GroupRoleDataReplyGroupDataBlock, GroupRoleDataReplyRoleDataBlock, GroupRoleMembersReply,
        GroupRoleMembersReplyAgentDataBlock, GroupRoleMembersReplyMemberDataBlock, ImageData,
        ImageDataImageDataBlock, ImageDataImageIDBlock, ImageNotInDatabase,
        ImageNotInDatabaseImageIDBlock, ImagePacket, ImagePacketImageDataBlock,
        ImagePacketImageIDBlock, ImprovedInstantMessage, ImprovedInstantMessageAgentDataBlock,
        ImprovedInstantMessageEstateBlockBlock, ImprovedInstantMessageMessageBlockBlock,
        ImprovedTerseObjectUpdate, ImprovedTerseObjectUpdateObjectDataBlock,
        ImprovedTerseObjectUpdateRegionDataBlock, InventoryDescendents,
        InventoryDescendentsAgentDataBlock, InventoryDescendentsFolderDataBlock,
        InventoryDescendentsItemDataBlock, KickUser, KickUserTargetBlockBlock,
        KickUserUserInfoBlock, KillObject, KillObjectObjectDataBlock, LargeGenericMessage,
        LargeGenericMessageAgentDataBlock, LargeGenericMessageMethodDataBlock,
        LargeGenericMessageParamListBlock, LayerData, LayerDataLayerDataBlock,
        LayerDataLayerIDBlock, LogoutRequest, LogoutRequestAgentDataBlock, MapBlockReply,
        MapBlockReplyAgentDataBlock, MapBlockReplyDataBlock, MapBlockReplySizeBlock, MapItemReply,
        MapItemReplyAgentDataBlock, MapItemReplyDataBlock, MapItemReplyRequestDataBlock,
        MapLayerReply, MapLayerReplyAgentDataBlock, MapLayerReplyLayerDataBlock, MoneyBalanceReply,
        MoneyBalanceReplyMoneyDataBlock, MoneyBalanceReplyTransactionInfoBlock, MoveInventoryItem,
        MoveInventoryItemAgentDataBlock, MoveInventoryItemInventoryDataBlock, MuteListUpdate,
        MuteListUpdateMuteDataBlock, ObjectAnimation, ObjectAnimationAnimationListBlock,
        ObjectAnimationSenderBlock, ObjectProperties as WireObjectProperties,
        ObjectPropertiesFamily as ObjectPropertiesFamilyMessage,
        ObjectPropertiesFamilyObjectDataBlock, ObjectPropertiesObjectDataBlock, ObjectUpdate,
        ObjectUpdateCached, ObjectUpdateCachedObjectDataBlock, ObjectUpdateCachedRegionDataBlock,
        ObjectUpdateCompressed, ObjectUpdateCompressedObjectDataBlock,
        ObjectUpdateCompressedRegionDataBlock, ObjectUpdateObjectDataBlock,
        ObjectUpdateRegionDataBlock, OfferCallingCard, OfferCallingCardAgentBlockBlock,
        OfferCallingCardAgentDataBlock, OfflineNotification, OfflineNotificationAgentBlockBlock,
        OnlineNotification, OnlineNotificationAgentBlockBlock, ParcelAccessListReply,
        ParcelAccessListReplyDataBlock, ParcelAccessListReplyListBlock, ParcelDwellReply,
        ParcelDwellReplyAgentDataBlock, ParcelDwellReplyDataBlock, ParcelInfoReply,
        ParcelInfoReplyAgentDataBlock, ParcelInfoReplyDataBlock, ParcelMediaCommandMessage,
        ParcelMediaCommandMessageCommandBlockBlock, ParcelMediaUpdate,
        ParcelMediaUpdateDataBlockBlock, ParcelMediaUpdateDataBlockExtendedBlock,
        ParcelObjectOwnersReply, ParcelObjectOwnersReplyDataBlock, ParcelProperties,
        ParcelPropertiesAgeVerificationBlockBlock, ParcelPropertiesParcelDataBlock,
        ParcelPropertiesParcelEnvironmentBlockBlock, ParcelPropertiesRegionAllowAccessBlockBlock,
        PayPriceReply, PayPriceReplyButtonDataBlock, PayPriceReplyObjectDataBlock, PickInfoReply,
        PickInfoReplyAgentDataBlock, PickInfoReplyDataBlock, PreloadSound,
        PreloadSoundDataBlockBlock, RebakeAvatarTextures, RebakeAvatarTexturesTextureDataBlock,
        RegionHandshake, RegionHandshakeRegionInfo2Block, RegionHandshakeRegionInfo3Block,
        RegionHandshakeRegionInfo4Block, RegionHandshakeRegionInfoBlock, RegionInfo,
        RegionInfoAgentDataBlock, RegionInfoCombatSettingsBlock, RegionInfoRegionInfo2Block,
        RegionInfoRegionInfo3Block, RegionInfoRegionInfo5Block, RegionInfoRegionInfoBlock,
        RemoveInventoryFolder, RemoveInventoryFolderAgentDataBlock,
        RemoveInventoryFolderFolderDataBlock, RemoveInventoryItem,
        RemoveInventoryItemAgentDataBlock, RemoveInventoryItemInventoryDataBlock,
        RemoveInventoryObjects, RemoveInventoryObjectsAgentDataBlock,
        RemoveInventoryObjectsFolderDataBlock, RemoveInventoryObjectsItemDataBlock,
        ReplyTaskInventory, ReplyTaskInventoryInventoryDataBlock, RequestXfer,
        RequestXferXferIDBlock, ScriptDialog, ScriptDialogButtonsBlock, ScriptDialogDataBlock,
        ScriptDialogOwnerDataBlock, ScriptQuestion, ScriptQuestionDataBlock,
        ScriptQuestionExperienceBlock, ScriptRunningReply, ScriptRunningReplyScriptBlock,
        ScriptTeleportRequest, ScriptTeleportRequestDataBlock, ScriptTeleportRequestOptionsBlock,
        SendXferPacket, SendXferPacketDataPacketBlock, SendXferPacketXferIDBlock, SimStats,
        SimStatsPidStatBlock, SimStatsRegionBlock, SimStatsRegionInfoBlock, SimStatsStatBlock,
        SimulatorViewerTimeMessage, SimulatorViewerTimeMessageTimeInfoBlock, SoundTrigger,
        SoundTriggerSoundDataBlock, TelehubInfo as TelehubInfoMessage,
        TelehubInfoSpawnPointBlockBlock, TelehubInfoTelehubBlockBlock, TeleportFailed,
        TeleportFailedAlertInfoBlock, TeleportFailedInfoBlock, TeleportFinish,
        TeleportFinishInfoBlock, TerminateFriendship, TerminateFriendshipAgentDataBlock,
        TerminateFriendshipExBlockBlock, TransferInfo, TransferInfoTransferInfoBlock,
        TransferPacket, TransferPacketTransferDataBlock, UUIDNameReply,
        UUIDNameReplyUUIDNameBlockBlock, UpdateCreateInventoryItem,
        UpdateCreateInventoryItemAgentDataBlock, UpdateCreateInventoryItemInventoryDataBlock,
        UseCachedMuteList, UseCachedMuteListAgentDataBlock, UserInfoReply,
        UserInfoReplyAgentDataBlock, UserInfoReplyUserDataBlock,
        ViewerEffect as ViewerEffectMessage, ViewerEffectAgentDataBlock, ViewerEffectEffectBlock,
    };
    use sl_wire::{
        AnyMessage, CircuitCode, HomeLocation, Llsd, LoginFailure, LoginRequest, LoginResponse,
        LoginSuccess, MessageId, PacketFlags, Reader, SequenceNumber, SkeletonFolder,
        StartLocation, WireError, Writer, encode_datagram, parse_datagram, parse_llsd_xml,
    };

    /// A boxed test error.
    type TestError = Box<dyn Error>;

    /// Wrap a (valid) region name for a test fixture or assertion (`None` if it
    /// does not satisfy the region-name grammar, which the fixtures never trip).
    fn region_name(name: &str) -> Option<sl_proto::RegionName> {
        sl_proto::region_name_from_wire("test", name).ok().flatten()
    }

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
    fn new_session() -> Result<Session, TestError> {
        Ok(Session::new(LoginParams {
            login_uri: "http://127.0.0.1:9000/".parse()?,
            request: LoginRequest::new(
                "Test",
                "User",
                "secret",
                StartLocation::Last,
                "MyViewer",
                "1.2.3",
            ),
        }))
    }

    /// A successful login response pointing at the test simulator.
    fn success() -> Result<LoginResponse, TestError> {
        Ok(LoginResponse::Success(Box::new(LoginSuccess {
            agent_id: AgentKey::from(uuid::Uuid::from_u128(1)),
            session_id: uuid::Uuid::from_u128(2),
            secure_session_id: uuid::Uuid::from_u128(3),
            circuit_code: CircuitCode(0x0011_2233),
            sim_ip: Ipv4Addr::new(127, 0, 0, 1),
            sim_port: 9000,
            seed_capability: "http://127.0.0.1:9000/seed".parse()?,
            message: None,
            mfa_hash: None,
            inventory_root: None,
            inventory_skeleton: Vec::new(),
            buddy_list: Vec::new(),
            home: None,
            look_at: None,
            region_x: None,
            region_y: None,
            agent_access: None,
            agent_access_max: None,
            max_agent_groups: None,
            library_root: None,
            library_owner: None,
            library_skeleton: Vec::new(),
        })))
    }

    /// Decodes the message carried by a transmitted datagram.
    fn decode(transmit: &Transmit) -> Result<AnyMessage, TestError> {
        let parsed = parse_datagram(&transmit.payload)?;
        let mut reader = Reader::new(parsed.body);
        let id = MessageId::decode(&mut reader)?;
        Ok(AnyMessage::decode(id, &mut reader)?)
    }

    /// Split a folder's borrowed [`Child`] iterator into `(folders, items)`, the
    /// shape the pre-B4 tuple accessor returned, for the assertions below.
    fn split_children(
        session: &Session,
        folder: InventoryFolderKey,
    ) -> (Vec<&InventoryFolder>, Vec<&InventoryItem>) {
        let mut folders = Vec::new();
        let mut items = Vec::new();
        for child in session.inventory_children(folder) {
            match child {
                Child::Folder(folder) => folders.push(folder),
                Child::Item(item) => items.push(item),
            }
        }
        (folders, items)
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

    /// Drains all queued diagnostics.
    fn drain_diagnostics(session: &mut Session) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        while let Some(diagnostic) = session.poll_diagnostic() {
            out.push(diagnostic);
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
        encode_datagram(flags, SequenceNumber(sequence), &writer.into_bytes())
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
        Ok(encode_datagram(
            flags,
            SequenceNumber(sequence),
            &writer.into_bytes(),
        ))
    }

    /// Drives a session from login through the region handshake into the active
    /// state, returning the active session.
    fn established(now: Instant) -> Result<Session, TestError> {
        let mut session = new_session()?;
        assert!(session.login_http_request().is_some());
        session.handle_login_response(success()?, now)?;

        let sent = drain(&mut session)?;
        assert!(matches!(sent.first(), Some(AnyMessage::UseCircuitCode(_))));
        assert!(matches!(
            sent.get(1),
            Some(AnyMessage::CompleteAgentMovement(_))
        ));
        // Login always emits the account facts right after the circuit comes up;
        // the `success()` fixture carries none, so they are all empty/unknown.
        let circuit = session.root_circuit_id().ok_or("no circuit")?;
        assert_eq!(
            drain_events(&mut session),
            vec![
                Event::CircuitEstablished {
                    sim: sim_addr(),
                    circuit,
                },
                Event::Account(Box::new(LoginAccount {
                    home: None,
                    look_at: None,
                    agent_access: Maturity::Unknown,
                    agent_access_max: Maturity::Unknown,
                    max_agent_groups: None,
                    library_root: None,
                    library_owner: None,
                })),
            ]
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
        let mut session = new_session()?;
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
    fn closed_session_rejects_relogin() -> Result<(), TestError> {
        // Drive a fresh session to its terminal closed state via a login
        // failure.
        let mut session = new_session()?;
        let failure = LoginResponse::Failure(LoginFailure {
            reason: "key".to_owned(),
            message: "bad password".to_owned(),
        });
        session.handle_login_response(failure, Instant::now())?;
        assert!(session.is_closed());
        let _disconnect_events = drain_events(&mut session);

        // A closed `Session` is never revived: a fresh login attempt is rejected
        // rather than half-reusing stale per-session state.
        let result = session.handle_login_response(success()?, Instant::now());
        assert!(matches!(result, Err(sl_proto::Error::SessionClosed)));
        // The reject leaves the session closed and brings up no circuit.
        assert!(session.is_closed());
        assert!(
            !drain_events(&mut session)
                .iter()
                .any(|e| matches!(e, Event::CircuitEstablished { .. })),
            "a rejected relogin must not establish a circuit"
        );
        Ok(())
    }

    #[test]
    fn live_session_rejects_relogin() -> Result<(), TestError> {
        // An established session is logged in (Active). Login is valid only once,
        // from the freshly-constructed state, so a second response is rejected
        // rather than tearing down the live circuit and half-rebuilding.
        let now = Instant::now();
        let mut session = established(now)?;
        let _established_events = drain_events(&mut session);

        let result = session.handle_login_response(success()?, now);
        assert!(matches!(result, Err(sl_proto::Error::AlreadyLoggedIn)));
        // The reject leaves the live session intact and rebuilds no circuit.
        assert!(!session.is_closed());
        assert!(
            !drain_events(&mut session)
                .iter()
                .any(|e| matches!(e, Event::CircuitEstablished { .. })),
            "a rejected relogin must not rebuild the circuit"
        );
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
        let mut session = new_session()?;
        session.handle_login_response(success()?, now)?;
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
    fn exhausted_resend_reports_expected_reply_missing() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = new_session()?;
        session.set_diagnostics(true);
        session.handle_login_response(success()?, now)?;
        let _initial = drain(&mut session)?; // UseCircuitCode + CompleteAgentMovement

        // Nothing is ever acked: drive the resend clock until the reliable
        // handshake packets exhaust their retransmission budget and the session
        // gives up on the circuit.
        for _ in 0..16 {
            if session.is_closed() {
                break;
            }
            let next = session.poll_timeout().ok_or("a timeout is scheduled")?;
            session.handle_timeout(next);
        }
        assert!(
            session.is_closed(),
            "the session should give up after exhausting its resends"
        );

        let diagnostics = drain_diagnostics(&mut session);
        assert!(
            diagnostics.iter().any(|d| matches!(
                d,
                Diagnostic::ExpectedReplyMissing { request, sequence: Some(_) }
                    if request == "UseCircuitCode"
            )),
            "expected an ExpectedReplyMissing for UseCircuitCode, got {diagnostics:?}"
        );
        Ok(())
    }

    #[test]
    fn exhausted_resend_is_silent_without_diagnostics() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = new_session()?;
        session.handle_login_response(success()?, now)?;
        let _initial = drain(&mut session)?;

        for _ in 0..16 {
            if session.is_closed() {
                break;
            }
            let next = session.poll_timeout().ok_or("a timeout is scheduled")?;
            session.handle_timeout(next);
        }
        assert!(session.is_closed());
        assert!(
            drain_diagnostics(&mut session).is_empty(),
            "diagnostics must stay off the normal path when not enabled"
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
    fn logout_timeout_reports_expected_reply_missing() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        session.set_diagnostics(true);
        drain(&mut session)?;

        session.initiate_logout(now);
        drain(&mut session)?;

        // No LogoutReply ever arrives; the logout timer fires and the session
        // closes anyway, surfacing the missing reply.
        session.handle_timeout(after(now, 6_000)?);
        assert!(session.is_closed());
        assert!(matches!(
            drain_events(&mut session).last(),
            Some(Event::LoggedOut)
        ));

        let diagnostics = drain_diagnostics(&mut session);
        assert!(
            diagnostics.iter().any(|d| matches!(
                d,
                Diagnostic::ExpectedReplyMissing { request, sequence: None } if request == "Logout"
            )),
            "expected an ExpectedReplyMissing for Logout, got {diagnostics:?}"
        );
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

        session.say("hi there", ChatType::Shout, ChatChannel(0), now)?;
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
        assert_eq!(
            received.source,
            ChatSource::Agent(AgentKey::from(uuid::Uuid::from_u128(0x42)))
        );
        assert_eq!(received.owner_id, Some(uuid::Uuid::from_u128(0x43)));
        assert_eq!(received.chat_type, ChatType::Normal);
        assert_eq!(received.audible, ChatAudible::Fully);
        assert_eq!(received.position, RegionCoordinates::new(10.0, 20.0, 30.0));
        Ok(())
    }

    #[test]
    fn sim_stats_surfaces_region_telemetry() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let stats = AnyMessage::SimStats(SimStats {
            region: SimStatsRegionBlock {
                region_x: 1000,
                region_y: 1001,
                region_flags: 0x1234,
                object_capacity: 15_000,
            },
            stat: vec![
                SimStatsStatBlock {
                    stat_id: 0, // TimeDilation
                    stat_value: 0.97,
                },
                SimStatsStatBlock {
                    stat_id: 13, // Agents
                    stat_value: 5.0,
                },
                SimStatsStatBlock {
                    stat_id: 9999, // unknown id, preserved verbatim
                    stat_value: 1.0,
                },
            ],
            pid_stat: SimStatsPidStatBlock { pid: 4242 },
            region_info: vec![SimStatsRegionInfoBlock {
                region_flags_extended: 0xDEAD_BEEF_0000_0001,
            }],
        });
        let datagram = server_message(&stats, 7, false)?;
        session.handle_datagram(sim_addr(), &datagram, now)?;

        let received = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::SimStats(stats) => Some(stats),
                _ => None,
            })
            .ok_or("expected a SimStats event")?;
        assert_eq!(received.grid_coordinates, GridCoordinates::new(1000, 1001));
        assert_eq!(received.region_flags, 0x1234);
        assert_eq!(received.object_capacity, 15_000);
        // The extended flags come from the RegionInfo block, not the 32-bit field.
        assert_eq!(received.region_flags_extended, 0xDEAD_BEEF_0000_0001);
        assert_eq!(
            received.stats,
            vec![
                (SimStatId::TimeDilation, 0.97),
                (SimStatId::Agents, 5.0),
                (SimStatId::Unknown(9999), 1.0),
            ]
        );
        Ok(())
    }

    #[test]
    fn sim_stats_without_region_info_falls_back_to_32bit_flags() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let stats = AnyMessage::SimStats(SimStats {
            region: SimStatsRegionBlock {
                region_x: 1,
                region_y: 2,
                region_flags: 0x00AB,
                object_capacity: 20_000,
            },
            stat: Vec::new(),
            pid_stat: SimStatsPidStatBlock { pid: 0 },
            region_info: Vec::new(),
        });
        session.handle_datagram(sim_addr(), &server_message(&stats, 8, false)?, now)?;

        let received = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::SimStats(stats) => Some(stats),
                _ => None,
            })
            .ok_or("expected a SimStats event")?;
        // No RegionInfo block: extended flags are the zero-extended 32-bit field.
        assert_eq!(received.region_flags_extended, 0x00AB);
        assert!(received.stats.is_empty());
        Ok(())
    }

    #[test]
    fn simulator_viewer_time_surfaces_world_time() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let time = AnyMessage::SimulatorViewerTimeMessage(SimulatorViewerTimeMessage {
            time_info: SimulatorViewerTimeMessageTimeInfoBlock {
                usec_since_start: 1_234_567_890,
                sec_per_day: 86_400,
                sec_per_year: 31_536_000,
                sun_direction: vec3(0.0, 0.0, 1.0),
                sun_phase: 1.5,
                sun_ang_velocity: vec3(0.0, 0.1, 0.0),
            },
        });
        session.handle_datagram(sim_addr(), &server_message(&time, 9, false)?, now)?;

        let received = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::SimulatorTime(time) => Some(time),
                _ => None,
            })
            .ok_or("expected a SimulatorTime event")?;
        assert_eq!(
            *received,
            SimulatorTime {
                usec_since_start: 1_234_567_890,
                sec_per_day: 86_400,
                sec_per_year: 31_536_000,
                sun_direction: vec3(0.0, 0.0, 1.0),
                sun_phase: 1.5,
                sun_ang_velocity: vec3(0.0, 0.1, 0.0),
            }
        );
        Ok(())
    }

    #[test]
    fn generic_message_surfaces_method_and_params() -> Result<(), TestError> {
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
                // A method with no special handler falls through to the generic
                // envelope event; NUL-terminated as the sim sends it.
                method: b"GrantUserRights\0".to_vec(),
                invoice: uuid::Uuid::from_u128(0xABCD),
            },
            param_list: vec![
                GenericMessageParamListBlock {
                    parameter: b"first".to_vec(),
                },
                GenericMessageParamListBlock {
                    parameter: b"second".to_vec(),
                },
            ],
        });
        session.handle_datagram(sim_addr(), &server_message(&message, 9, true)?, now)?;
        let received = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::GenericMessage(generic) => Some(generic),
                _ => None,
            })
            .ok_or("expected a GenericMessage event")?;
        assert_eq!(
            received,
            sl_proto::GenericMessage {
                method: "GrantUserRights".to_owned(),
                invoice: sl_proto::InvoiceId::from(uuid::Uuid::from_u128(0xABCD)),
                params: vec![b"first".to_vec(), b"second".to_vec()],
            }
        );
        Ok(())
    }

    #[test]
    fn large_generic_message_surfaces_method_and_params() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let message = AnyMessage::LargeGenericMessage(LargeGenericMessage {
            agent_data: LargeGenericMessageAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(1),
                session_id: uuid::Uuid::from_u128(2),
                transaction_id: uuid::Uuid::nil(),
            },
            method_data: LargeGenericMessageMethodDataBlock {
                method: b"BigFeature\0".to_vec(),
                invoice: uuid::Uuid::nil(),
            },
            param_list: vec![LargeGenericMessageParamListBlock {
                parameter: b"payload".to_vec(),
            }],
        });
        session.handle_datagram(sim_addr(), &server_message(&message, 10, false)?, now)?;
        let received = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::LargeGenericMessage(generic) => Some(generic),
                _ => None,
            })
            .ok_or("expected a LargeGenericMessage event")?;
        assert_eq!(
            received,
            sl_proto::GenericMessage {
                method: "BigFeature".to_owned(),
                invoice: sl_proto::InvoiceId::default(),
                params: vec![b"payload".to_vec()],
            }
        );
        Ok(())
    }

    #[test]
    fn generic_streaming_message_surfaces_method_and_data() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // A non-GLTF streaming method falls through to the generic envelope event
        // (the GLTF material-override method has a dedicated handler arm).
        let message = AnyMessage::GenericStreamingMessage(GenericStreamingMessage {
            method_data: GenericStreamingMessageMethodDataBlock { method: 0x9999 },
            data_block: GenericStreamingMessageDataBlockBlock {
                data: vec![1, 2, 3, 4],
            },
        });
        session.handle_datagram(sim_addr(), &server_message(&message, 11, false)?, now)?;
        let received = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::GenericStreamingMessage(streaming) => Some(streaming),
                _ => None,
            })
            .ok_or("expected a GenericStreamingMessage event")?;
        assert_eq!(
            received,
            sl_proto::GenericStreamingMessage {
                method: 0x9999,
                data: vec![1, 2, 3, 4],
            }
        );
        Ok(())
    }

    #[test]
    fn error_message_surfaces_typed_server_error() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let message = AnyMessage::Error(ErrorMessage {
            agent_data: ErrorAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(1),
            },
            data: ErrorDataBlock {
                code: 402,
                token: b"InsufficientFunds\0".to_vec(),
                id: uuid::Uuid::from_u128(0xFEED),
                system: b"money/transfer\0".to_vec(),
                message: b"You do not have enough money.\0".to_vec(),
                data: vec![1, 2, 3],
            },
        });
        session.handle_datagram(sim_addr(), &server_message(&message, 12, false)?, now)?;
        let received = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::ServerError(error) => Some(error),
                _ => None,
            })
            .ok_or("expected a ServerError event")?;
        assert_eq!(
            *received,
            sl_proto::ServerError {
                agent: AgentKey::from(uuid::Uuid::from_u128(1)),
                code: 402,
                token: "InsufficientFunds".to_owned(),
                id: uuid::Uuid::from_u128(0xFEED),
                system: "money/transfer".to_owned(),
                message: "You do not have enough money.".to_owned(),
                data: vec![1, 2, 3],
            }
        );
        Ok(())
    }

    #[test]
    fn feature_disabled_surfaces_typed_event() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let message = AnyMessage::FeatureDisabled(WireFeatureDisabled {
            failure_info: FeatureDisabledFailureInfoBlock {
                error_message: b"That feature is disabled here.\0".to_vec(),
                agent_id: uuid::Uuid::from_u128(1),
                transaction_id: uuid::Uuid::from_u128(0xBEEF),
            },
        });
        session.handle_datagram(sim_addr(), &server_message(&message, 13, false)?, now)?;
        let received = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::FeatureDisabled(disabled) => Some(disabled),
                _ => None,
            })
            .ok_or("expected a FeatureDisabled event")?;
        assert_eq!(
            received,
            sl_proto::FeatureDisabled {
                message: "That feature is disabled here.".to_owned(),
                agent: AgentKey::from(uuid::Uuid::from_u128(1)),
                transaction: TransactionId::from(uuid::Uuid::from_u128(0xBEEF)),
            }
        );
        Ok(())
    }

    #[test]
    fn kick_user_surfaces_kick_and_disconnects() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let message = AnyMessage::KickUser(KickUser {
            target_block: KickUserTargetBlockBlock {
                target_ip: [127, 0, 0, 1],
                target_port: 13000,
            },
            user_info: KickUserUserInfoBlock {
                agent_id: uuid::Uuid::from_u128(1),
                session_id: uuid::Uuid::from_u128(2),
                reason: b"Logged in from another location.\0".to_vec(),
            },
        });
        session.handle_datagram(sim_addr(), &server_message(&message, 14, false)?, now)?;
        let events = drain_events(&mut session);
        // The kick is surfaced with its details...
        let kick = events
            .iter()
            .find_map(|event| match event {
                Event::Kicked(kick) => Some(kick.clone()),
                _ => None,
            })
            .ok_or("expected a Kicked event")?;
        assert_eq!(
            kick,
            sl_proto::Kick {
                agent: AgentKey::from(uuid::Uuid::from_u128(1)),
                reason: "Logged in from another location.".to_owned(),
            }
        );
        // ...and the session drives itself to a terminal kicked disconnect.
        assert!(events.iter().any(|event| matches!(
            event,
            Event::Disconnected(DisconnectReason::Kicked { message })
                if message == "Logged in from another location."
        )));
        assert!(session.is_closed());
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
        session.send_instant_message(AgentKey::from(target), "hi there", now)?;
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

        session.send_im_typing(AgentKey::from(uuid::Uuid::from_u128(0x99)), true, now)?;
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
        assert_eq!(
            received.from_agent_id,
            AgentKey::from(uuid::Uuid::from_u128(0x55))
        );
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
        assert_eq!(typing.0, AgentKey::from(uuid::Uuid::from_u128(0x55)));
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

    #[test]
    fn teleport_finish_surfaces_maturity_and_flags() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        // Begin a teleport so the session is awaiting a `TeleportFinish`.
        let handle = 0x0003_E900_0003_E800;
        session.teleport_to(
            RegionHandle(handle),
            region_coords(128.0, 128.0, 30.0),
            vec3(1.0, 0.0, 0.0),
            now,
        )?;
        drain(&mut session)?;
        drain_events(&mut session);

        // The simulator confirms the teleport, naming the destination's maturity
        // (Mature) and the flags describing why it happened (a lure, flying).
        let finish = AnyMessage::TeleportFinish(TeleportFinish {
            info: TeleportFinishInfoBlock {
                agent_id: uuid::Uuid::from_u128(1),
                location_id: 4,
                sim_ip: [127, 0, 0, 1],
                // IPPORT is big-endian on the wire; the swap mirrors the decoder.
                sim_port: 9100u16.swap_bytes(),
                region_handle: handle,
                seed_capability: b"http://x/seedTP\0".to_vec(),
                sim_access: sl_wire::sim_access::MATURE,
                teleport_flags: TeleportFlags::VIA_LURE | TeleportFlags::IS_FLYING,
            },
        });
        session.handle_datagram(sim_addr(), &server_message(&finish, 10, true)?, now)?;

        let finished = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::TeleportFinished {
                    region_handle,
                    sim,
                    maturity,
                    flags,
                } => Some((region_handle, sim, maturity, flags)),
                _ => None,
            })
            .ok_or("expected a TeleportFinished event")?;

        let (region_handle, sim, maturity, flags) = finished;
        assert_eq!(region_handle, RegionHandle(handle));
        assert_eq!(
            sim,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 9100)
        );
        assert_eq!(maturity, Maturity::Mature);
        assert!(flags.contains(TeleportFlags::VIA_LURE));
        assert!(flags.contains(TeleportFlags::IS_FLYING));
        assert!(!flags.contains(TeleportFlags::VIA_LANDMARK));
        Ok(())
    }

    /// Builds an inbound `AvatarSitResponse` for the object `sit_object`.
    fn sit_response(sit_object: uuid::Uuid) -> AnyMessage {
        AnyMessage::AvatarSitResponse(AvatarSitResponse {
            sit_object: AvatarSitResponseSitObjectBlock { id: sit_object },
            sit_transform: AvatarSitResponseSitTransformBlock {
                auto_pilot: false,
                sit_position: vec3(0.0, 0.0, 0.5),
                sit_rotation: Rotation {
                    x: 0.0,
                    y: 0.0,
                    z: 0.707,
                    s: 0.707,
                },
                camera_eye_offset: vec3(1.0, 2.0, 3.0),
                camera_at_offset: vec3(4.0, 5.0, 6.0),
                force_mouselook: true,
            },
        })
    }

    #[test]
    fn sit_request_completes_on_response() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let target = uuid::Uuid::from_u128(0x5117);
        session.sit_on(ObjectKey::from(target), vec3(0.0, 0.0, 0.0), now)?;
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
                Event::SitResult { .. } => Some(event),
                _ => None,
            })
            .ok_or("expected a SitResult event")?;
        let Event::SitResult {
            sit_object,
            autopilot,
            sit_position,
            sit_rotation,
            camera_eye_offset,
            camera_at_offset,
            force_mouselook,
        } = result
        else {
            return Err("expected a SitResult event".into());
        };
        assert_eq!(sit_object, ObjectKey::from(target));
        assert!(!autopilot);
        assert_eq!(sit_position, vec3(0.0, 0.0, 0.5));
        // The full SitTransform — rotation, scripted-sit camera offsets, and the
        // force-mouselook flag — must reach the caller, not just the position.
        // The wire stores a quaternion as (x, y, z) and reconstructs `s`, so the
        // decoded `s` differs slightly from the sent value; compare with epsilon.
        assert!((sit_rotation.x - 0.0).abs() < 1e-4);
        assert!((sit_rotation.y - 0.0).abs() < 1e-4);
        assert!((sit_rotation.z - 0.707).abs() < 1e-4);
        assert!((sit_rotation.s - 0.707).abs() < 1e-3);
        assert_eq!(camera_eye_offset, vec3(1.0, 2.0, 3.0));
        assert_eq!(camera_at_offset, vec3(4.0, 5.0, 6.0));
        assert!(force_mouselook);
        // The completed sit is now recorded and queryable via `seat`.
        assert_eq!(session.seat(), Some(ObjectKey::from(target)));
        Ok(())
    }

    #[test]
    fn teleport_clears_seat() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        // Sit on an object and let the sit complete.
        let target = uuid::Uuid::from_u128(0x5EA7);
        session.sit_on(ObjectKey::from(target), vec3(0.0, 0.0, 0.0), now)?;
        drain(&mut session)?;
        session.handle_datagram(
            sim_addr(),
            &server_message(&sit_response(target), 9, false)?,
            now,
        )?;
        drain(&mut session)?;
        drain_events(&mut session);
        assert_eq!(
            session.seat(),
            Some(ObjectKey::from(target)),
            "the agent should be seated once the sit completes"
        );

        // Teleporting unseats the agent: the recorded seat must clear so it does
        // not dangle pointing at an object in the region just left.
        let handle = 0x0003_E900_0003_E800;
        session.teleport_to(
            RegionHandle(handle),
            region_coords(128.0, 128.0, 30.0),
            vec3(1.0, 0.0, 0.0),
            now,
        )?;
        drain(&mut session)?;
        drain_events(&mut session);
        let finish = AnyMessage::TeleportFinish(TeleportFinish {
            info: TeleportFinishInfoBlock {
                agent_id: uuid::Uuid::from_u128(1),
                location_id: 4,
                sim_ip: [127, 0, 0, 1],
                sim_port: 9100u16.swap_bytes(),
                region_handle: handle,
                seed_capability: b"http://x/seatTP\0".to_vec(),
                sim_access: sl_wire::sim_access::MATURE,
                teleport_flags: TeleportFlags::VIA_LURE,
            },
        });
        session.handle_datagram(sim_addr(), &server_message(&finish, 10, true)?, now)?;
        assert_eq!(
            session.seat(),
            None,
            "the teleport should have cleared the recorded seat"
        );
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
    fn sit_timeout_reports_expected_reply_missing() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        session.set_diagnostics(true);
        drain(&mut session)?;

        session.sit_on(
            ObjectKey::from(uuid::Uuid::from_u128(0x5117)),
            vec3(0.0, 0.0, 0.0),
            now,
        )?;
        drain(&mut session)?;

        // No AvatarSitResponse arrives; the sit timer fires. Sit is best-effort,
        // so the session stays up and no SitResult is produced — only a
        // diagnostic.
        session.handle_timeout(after(now, 16_000)?);
        assert!(
            !session.is_closed(),
            "a sit timeout must not close the session"
        );
        assert!(
            !drain_events(&mut session)
                .iter()
                .any(|e| matches!(e, Event::SitResult { .. })),
            "no SitResult should be emitted on a sit timeout"
        );

        let diagnostics = drain_diagnostics(&mut session);
        assert!(
            diagnostics.iter().any(|d| matches!(
                d,
                Diagnostic::ExpectedReplyMissing { request, sequence: None } if request == "Sit"
            )),
            "expected an ExpectedReplyMissing for Sit, got {diagnostics:?}"
        );
        Ok(())
    }

    #[test]
    fn request_avatar_properties_packs_request() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let target = uuid::Uuid::from_u128(0xA1);
        session.request_avatar_properties(AgentKey::from(target), now)?;
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
        assert_eq!(props.avatar_id, AgentKey::from(target));
        assert_eq!(props.about_text, "a test avatar");
        assert_eq!(props.born_on, "2008-01-15");
        assert_eq!(
            props.partner_id,
            Some(AgentKey::from(uuid::Uuid::from_u128(0xB3)))
        );
        assert_eq!(props.flags, 0x10);
        Ok(())
    }

    #[test]
    fn request_avatar_picks_packs_generic_message() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let target = uuid::Uuid::from_u128(0xA1);
        session.request_avatar_picks(AgentKey::from(target), now)?;
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
        assert_eq!(pick.pick_id.uuid(), uuid::Uuid::from_u128(0xC1));
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
        session.request_avatar_classifieds(AgentKey::from(target), now)?;
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
        assert_eq!(
            classified.classified_id,
            ClassifiedKey::from(uuid::Uuid::from_u128(0xD1))
        );
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
        session.request_pick_info(AgentKey::from(creator), sl_proto::PickKey::from(pick), now)?;
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
                Event::PickInfo(info) if info.pick_id.uuid() == pick => Some(info),
                _ => None,
            })
            .ok_or("expected a PickInfo event")?;
        assert_eq!(info.name, "My favourite spot");
        assert_eq!(info.description, "a lovely beach");
        assert_eq!(info.sim_name, region_name("Sandbox"));
        let (px, py, pz) = (
            info.pos_global.x(),
            info.pos_global.y(),
            info.pos_global.z(),
        );
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
        session.request_classified_info(ClassifiedKey::from(classified), now)?;
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
                Event::ClassifiedInfo(info) if info.classified_id.uuid() == classified => {
                    Some(info)
                }
                _ => None,
            })
            .ok_or("expected a ClassifiedInfo event")?;
        assert_eq!(info.name, "Land for rent");
        assert_eq!(info.description, "prime waterfront");
        assert_eq!(info.parcel_name, "Beach Parcel");
        assert_eq!(info.category, ClassifiedCategory::PropertyRental);
        assert_eq!(info.price_for_listing, LindenAmount(50));
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
                image_id: TextureKey::from(uuid::Uuid::from_u128(0x5E)),
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
        session.update_avatar_notes(AgentKey::from(target), "a good friend", now)?;
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
                pick_id: sl_proto::PickKey::from(pick),
                name: "New pick".to_owned(),
                description: "a place".to_owned(),
                pos_global: GlobalCoordinates::new(256_000.0, 256_128.0, 25.5),
                ..PickUpdate::default()
            },
            now,
        )?;
        session.delete_pick(sl_proto::PickKey::from(pick), now)?;

        let classified = uuid::Uuid::from_u128(0xD1);
        session.update_classified(
            &ClassifiedUpdate {
                classified_id: ClassifiedKey::from(classified),
                category: ClassifiedCategory::PropertyRental,
                name: "New classified".to_owned(),
                description: "for sale".to_owned(),
                price_for_listing: LindenAmount(100),
                classified_flags: 0x4,
                ..ClassifiedUpdate::default()
            },
            now,
        )?;
        session.delete_classified(ClassifiedKey::from(classified), now)?;
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
        let mut session = new_session()?;
        let friend_a = uuid::Uuid::from_u128(0xF1);
        let friend_b = uuid::Uuid::from_u128(0xF2);
        let LoginResponse::Success(mut login_success) = success()? else {
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
        assert_eq!(first.id, FriendKey::from(friend_a));
        assert!(first.rights_granted.can_see_online());
        assert!(first.rights_granted.can_see_on_map());
        assert!(first.rights_received.can_see_online());
        assert!(!first.rights_received.can_modify_objects());
        let second = friends.get(1).ok_or("second friend")?;
        assert_eq!(second.id, FriendKey::from(friend_b));
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
        assert_eq!(
            ids,
            vec![FriendKey::from(friend_a), FriendKey::from(friend_b)]
        );
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
        assert_eq!(ids, vec![FriendKey::from(friend)]);
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
                assert_eq!(friend_id, FriendKey::from(friend));
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
                assert_eq!(friend_id, FriendKey::from(friend));
                assert!(!granted_to_us);
                assert!(rights.can_modify_objects());
            }
            _ => return Err("expected FriendRightsChanged".into()),
        }
        Ok(())
    }

    /// Builds an inbound `ImprovedInstantMessage` from a specific sender with the
    /// given 1:1 dialog (used by the buddy-cache presence/live-add tests, which
    /// need to control the sender id `inbound_im` hardcodes).
    fn inbound_im_from(from: uuid::Uuid, dialog: u8) -> AnyMessage {
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
                position: vec3(0.0, 0.0, 0.0),
                offline: 0,
                dialog,
                id: uuid::Uuid::from_u128(0xABC),
                timestamp: 0,
                from_agent_name: b"Friend Tester\0".to_vec(),
                message: b"\0".to_vec(),
                binary_bucket: Vec::new(),
            },
            estate_block: ImprovedInstantMessageEstateBlockBlock { estate_id: 1 },
            meta_data: Vec::new(),
        })
    }

    #[test]
    fn login_buddy_list_seeds_friend_cache() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = new_session()?;
        let friend_a = uuid::Uuid::from_u128(0xF1);
        let friend_b = uuid::Uuid::from_u128(0xF2);
        let LoginResponse::Success(mut login_success) = success()? else {
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

        // The cache is seeded from the same data the FriendList event carries…
        let cached: Vec<FriendKey> = session.friends().map(|f| f.id).collect();
        assert_eq!(
            cached,
            vec![FriendKey::from(friend_a), FriendKey::from(friend_b)]
        );
        let a = session
            .friend(FriendKey::from(friend_a))
            .ok_or("friend_a cached")?;
        assert!(a.rights_granted.can_see_on_map());
        assert!(a.rights_received.can_see_online());
        // …and `online` starts empty (the buddy list carries rights, not presence).
        assert!(!session.is_online(FriendKey::from(friend_a)));
        assert_eq!(session.online_friends().count(), 0);
        Ok(())
    }

    #[test]
    fn online_offline_notifications_drive_presence() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let friend_a = uuid::Uuid::from_u128(0xF1);
        let friend_b = uuid::Uuid::from_u128(0xF2);
        let online = AnyMessage::OnlineNotification(OnlineNotification {
            agent_block: vec![
                OnlineNotificationAgentBlockBlock { agent_id: friend_a },
                OnlineNotificationAgentBlockBlock { agent_id: friend_b },
            ],
        });
        session.handle_datagram(sim_addr(), &server_message(&online, 9, true)?, now)?;
        assert!(session.is_online(FriendKey::from(friend_a)));
        assert!(session.is_online(FriendKey::from(friend_b)));
        assert_eq!(
            session.online_friends().collect::<Vec<_>>(),
            vec![FriendKey::from(friend_a), FriendKey::from(friend_b)]
        );

        let offline = AnyMessage::OfflineNotification(OfflineNotification {
            agent_block: vec![OfflineNotificationAgentBlockBlock { agent_id: friend_a }],
        });
        session.handle_datagram(sim_addr(), &server_message(&offline, 10, true)?, now)?;
        assert!(!session.is_online(FriendKey::from(friend_a)));
        assert!(session.is_online(FriendKey::from(friend_b)));
        Ok(())
    }

    #[test]
    fn change_user_rights_updates_cached_friend_by_direction() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // Seed a friend via the inbound accept (they accepted our offer).
        let friend = uuid::Uuid::from_u128(0xF4);
        session.handle_datagram(
            sim_addr(),
            &server_message(&inbound_im_from(friend, 39), 9, true)?,
            now,
        )?;

        // The friend grants us more rights (AgentData id == friend, not our 1):
        // updates `rights_received`.
        let granted = AnyMessage::ChangeUserRights(ChangeUserRights {
            agent_data: ChangeUserRightsAgentDataBlock { agent_id: friend },
            rights: vec![ChangeUserRightsRightsBlock {
                agent_related: uuid::Uuid::from_u128(1),
                related_rights: FriendRights::CAN_SEE_ONLINE | FriendRights::CAN_SEE_ON_MAP,
            }],
        });
        session.handle_datagram(sim_addr(), &server_message(&granted, 10, true)?, now)?;
        let cached = session.friend(FriendKey::from(friend)).ok_or("cached")?;
        assert!(cached.rights_received.can_see_on_map());
        assert!(!cached.rights_granted.can_see_on_map());

        // We change the rights we grant (echo: AgentData id == our 1): updates
        // `rights_granted`.
        let echoed = AnyMessage::ChangeUserRights(ChangeUserRights {
            agent_data: ChangeUserRightsAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(1),
            },
            rights: vec![ChangeUserRightsRightsBlock {
                agent_related: friend,
                related_rights: FriendRights::CAN_MODIFY_OBJECTS,
            }],
        });
        session.handle_datagram(sim_addr(), &server_message(&echoed, 11, true)?, now)?;
        let cached = session.friend(FriendKey::from(friend)).ok_or("cached")?;
        assert!(cached.rights_granted.can_modify_objects());
        Ok(())
    }

    #[test]
    fn change_user_rights_for_unknown_friend_is_ignored() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // A rights change for an agent that is not a cached friend must not
        // synthesise a half-known entry.
        let stranger = uuid::Uuid::from_u128(0xF6);
        let change = AnyMessage::ChangeUserRights(ChangeUserRights {
            agent_data: ChangeUserRightsAgentDataBlock { agent_id: stranger },
            rights: vec![ChangeUserRightsRightsBlock {
                agent_related: uuid::Uuid::from_u128(1),
                related_rights: FriendRights::CAN_SEE_ONLINE,
            }],
        });
        session.handle_datagram(sim_addr(), &server_message(&change, 9, true)?, now)?;
        assert!(session.friend(FriendKey::from(stranger)).is_none());
        Ok(())
    }

    #[test]
    fn terminate_friendship_drops_both_stores() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let friend = uuid::Uuid::from_u128(0xF7);
        // Add the friend (their accept) and mark them online.
        session.handle_datagram(
            sim_addr(),
            &server_message(&inbound_im_from(friend, 39), 9, true)?,
            now,
        )?;
        let online = AnyMessage::OnlineNotification(OnlineNotification {
            agent_block: vec![OnlineNotificationAgentBlockBlock { agent_id: friend }],
        });
        session.handle_datagram(sim_addr(), &server_message(&online, 10, true)?, now)?;
        assert!(session.friend(FriendKey::from(friend)).is_some());
        assert!(session.is_online(FriendKey::from(friend)));

        let terminate = AnyMessage::TerminateFriendship(TerminateFriendship {
            agent_data: TerminateFriendshipAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(1),
                session_id: uuid::Uuid::nil(),
            },
            ex_block: TerminateFriendshipExBlockBlock { other_id: friend },
        });
        session.handle_datagram(sim_addr(), &server_message(&terminate, 11, true)?, now)?;
        assert!(session.friend(FriendKey::from(friend)).is_none());
        assert!(!session.is_online(FriendKey::from(friend)));
        Ok(())
    }

    #[test]
    fn friendship_accepted_im_adds_friend() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // They accepted *our* offer: the IM's from_agent_id is the new friend.
        let friend = uuid::Uuid::from_u128(0xF8);
        session.handle_datagram(
            sim_addr(),
            &server_message(&inbound_im_from(friend, 39), 9, true)?,
            now,
        )?;

        // The IM surface is unchanged (still emitted)…
        assert!(
            drain_events(&mut session)
                .into_iter()
                .any(|event| matches!(event, Event::InstantMessageReceived(_)))
        );
        // …and the friend is now cached with the default CAN_SEE_ONLINE both ways.
        let cached = session.friend(FriendKey::from(friend)).ok_or("cached")?;
        assert!(cached.rights_granted.can_see_online());
        assert!(cached.rights_received.can_see_online());
        // A new friendship is not a presence signal.
        assert!(!session.is_online(FriendKey::from(friend)));
        Ok(())
    }

    #[test]
    fn accept_friendship_adds_friend() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // We accepted *their* offer: the caller supplies the offerer's id.
        let friend = uuid::Uuid::from_u128(0xF9);
        session.accept_friendship(
            TransactionId::from(uuid::Uuid::from_u128(0xAA)),
            FriendKey::from(friend),
            InventoryFolderKey::from(uuid::Uuid::from_u128(0xBB)),
            now,
        )?;
        let cached = session.friend(FriendKey::from(friend)).ok_or("cached")?;
        assert!(cached.rights_granted.can_see_online());
        assert!(cached.rights_received.can_see_online());
        Ok(())
    }

    #[test]
    fn im_after_offline_does_not_resurrect_presence() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let friend = uuid::Uuid::from_u128(0xFA);
        let online = AnyMessage::OnlineNotification(OnlineNotification {
            agent_block: vec![OnlineNotificationAgentBlockBlock { agent_id: friend }],
        });
        session.handle_datagram(sim_addr(), &server_message(&online, 9, true)?, now)?;
        let offline = AnyMessage::OfflineNotification(OfflineNotification {
            agent_block: vec![OfflineNotificationAgentBlockBlock { agent_id: friend }],
        });
        session.handle_datagram(sim_addr(), &server_message(&offline, 10, true)?, now)?;
        assert!(!session.is_online(FriendKey::from(friend)));

        // A 1:1 IM from the now-offline friend must NOT re-mark them online: IM
        // traffic is never a presence signal.
        session.handle_datagram(
            sim_addr(),
            &server_message(&inbound_im_from(friend, 0), 11, false)?,
            now,
        )?;
        assert!(!session.is_online(FriendKey::from(friend)));
        Ok(())
    }

    #[test]
    fn send_friendship_offer_packs_im() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let friend = uuid::Uuid::from_u128(0xA6);
        session.send_friendship_offer(AgentKey::from(friend), "be my friend", now)?;
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
            FriendKey::from(friend),
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
        session.terminate_friendship(FriendKey::from(friend), now)?;
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
        session.accept_friendship(
            TransactionId::from(transaction),
            FriendKey::from(uuid::Uuid::from_u128(0xDD)),
            InventoryFolderKey::from(folder),
            now,
        )?;
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
        session.decline_friendship(TransactionId::from(decline_tx), now)?;
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
    fn offer_accept_decline_calling_card_pack_messages() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let dest = uuid::Uuid::from_u128(0xCA11);
        let offer_tx = uuid::Uuid::from_u128(0xCA12);
        session.offer_calling_card(AgentKey::from(dest), TransactionId::from(offer_tx), now)?;
        let sent = drain(&mut session)?;
        let offer = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::OfferCallingCard(offer) => Some(offer),
                _ => None,
            })
            .ok_or("expected an OfferCallingCard")?;
        assert_eq!(offer.agent_block.dest_id, dest);
        assert_eq!(offer.agent_block.transaction_id, offer_tx);

        let accept_tx = uuid::Uuid::from_u128(0xCA13);
        let folder = uuid::Uuid::from_u128(0xCA14);
        session.accept_calling_card(
            TransactionId::from(accept_tx),
            InventoryFolderKey::from(folder),
            now,
        )?;
        let sent = drain(&mut session)?;
        let accept = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::AcceptCallingCard(accept) => Some(accept),
                _ => None,
            })
            .ok_or("expected an AcceptCallingCard")?;
        assert_eq!(accept.transaction_block.transaction_id, accept_tx);
        assert_eq!(
            accept.folder_data.first().map(|f| f.folder_id),
            Some(folder)
        );

        let decline_tx = uuid::Uuid::from_u128(0xCA15);
        session.decline_calling_card(TransactionId::from(decline_tx), now)?;
        let sent = drain(&mut session)?;
        let decline = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::DeclineCallingCard(decline) => Some(decline),
                _ => None,
            })
            .ok_or("expected a DeclineCallingCard")?;
        assert_eq!(decline.transaction_block.transaction_id, decline_tx);
        Ok(())
    }

    #[test]
    fn set_object_shape_packs_object_shape() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        let circuit = session.root_circuit_id().ok_or("no circuit")?;
        drain(&mut session)?;

        // Start from a cube and hollow it out / twist it.
        let shape = PrimShapeParams {
            profile_hollow: 25000,
            path_twist: 50,
            ..PrimShapeParams::default()
        };
        session.set_object_shape(
            ScopedObjectId::new(circuit, sl_proto::RegionLocalObjectId(42)),
            &shape,
            now,
        )?;
        let sent = drain(&mut session)?;
        let object_shape = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::ObjectShape(message) => Some(message),
                _ => None,
            })
            .ok_or("expected an ObjectShape")?;
        let block = object_shape.object_data.first().ok_or("first object")?;
        assert_eq!(block.object_local_id, 42);
        assert_eq!(block.profile_hollow, 25000);
        assert_eq!(block.path_twist, 50);
        Ok(())
    }

    #[test]
    fn set_object_image_packs_texture_entry() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        let circuit = session.root_circuit_id().ok_or("no circuit")?;
        drain(&mut session)?;

        let texture = uuid::Uuid::from_u128(0x7E07);
        session.set_object_image(
            ScopedObjectId::new(circuit, sl_proto::RegionLocalObjectId(7)),
            Some("http://example.com/media"),
            &TextureEntry {
                faces: vec![TextureFace::new(TextureKey::from(texture))],
            },
            now,
        )?;
        let sent = drain(&mut session)?;
        let object_image = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::ObjectImage(message) => Some(message),
                _ => None,
            })
            .ok_or("expected an ObjectImage")?;
        let block = object_image.object_data.first().ok_or("first object")?;
        assert_eq!(block.object_local_id, 7);
        assert_eq!(block.media_url, b"http://example.com/media");
        // The packed TextureEntry round-trips back to the requested texture id
        // (a single face's value is the default applied to every face).
        let decoded = decode_texture_entry(&block.texture_entry, 1);
        assert_eq!(decoded.texture_id(0), Some(TextureKey::from(texture)));
        Ok(())
    }

    #[test]
    fn set_object_extra_params_packs_all_subtypes() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        let circuit = session.root_circuit_id().ok_or("no circuit")?;
        drain(&mut session)?;

        // A light-only set: the light subtype is in use, every other subtype is
        // sent not-in-use (clearing it).
        let params = ObjectExtraParams {
            light: Some(LightData {
                color: [255, 128, 0, 255],
                radius: 5.0,
                cutoff: 0.0,
                falloff: 1.0,
            }),
            ..ObjectExtraParams::default()
        };
        session.set_object_extra_params(
            ScopedObjectId::new(circuit, sl_proto::RegionLocalObjectId(9)),
            &params,
            now,
        )?;
        let sent = drain(&mut session)?;
        let extra = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::ObjectExtraParams(message) => Some(message),
                _ => None,
            })
            .ok_or("expected an ObjectExtraParams")?;
        // One block per known subtype, all scoped to the same object.
        assert_eq!(extra.object_data.len(), 7);
        assert!(
            extra
                .object_data
                .iter()
                .all(|block| block.object_local_id == 9)
        );
        // The light block (0x20) is in use and carries a payload.
        let light = extra
            .object_data
            .iter()
            .find(|block| block.param_type == 0x20)
            .ok_or("expected a light block")?;
        assert!(light.param_in_use);
        assert!(!light.param_data.is_empty());
        assert_eq!(light.param_size, u32::try_from(light.param_data.len())?);
        // The flexi block (0x10) is absent → not in use, empty payload.
        let flexi = extra
            .object_data
            .iter()
            .find(|block| block.param_type == 0x10)
            .ok_or("expected a flexi block")?;
        assert!(!flexi.param_in_use);
        assert!(flexi.param_data.is_empty());
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
        assert_eq!(active.active_group_id, Some(GroupKey::from(group)));
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
        assert_eq!(first.group_id, GroupKey::from(group));
        assert_eq!(first.group_name, "Test Group");
        assert!(first.accept_notices);
        assert_eq!(first.contribution, LandArea(50));
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
        assert_eq!(group_id, GroupKey::from(group));
        assert_eq!(members.len(), 1);
        let first = members.first().ok_or("first member")?;
        assert_eq!(first.agent_id, AgentKey::from(member));
        assert_eq!(first.title, "Owner");
        assert!(first.is_owner);
        assert_eq!(first.agent_powers, 0xABCD);
        Ok(())
    }

    #[test]
    fn group_role_data_reply_surfaces_role_count() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let group = uuid::Uuid::from_u128(0x6707);
        let role = uuid::Uuid::from_u128(0x6708);
        // The simulator reports 5 roles in the header but splits them across
        // packets; this packet carries only one, so a client needs the header
        // count to know the set is incomplete.
        let message = AnyMessage::GroupRoleDataReply(GroupRoleDataReply {
            agent_data: GroupRoleDataReplyAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(1),
            },
            group_data: GroupRoleDataReplyGroupDataBlock {
                group_id: group,
                request_id: uuid::Uuid::nil(),
                role_count: 5,
            },
            role_data: vec![GroupRoleDataReplyRoleDataBlock {
                role_id: role,
                name: b"Officers\0".to_vec(),
                title: b"Officer\0".to_vec(),
                description: b"can manage the group\0".to_vec(),
                powers: 0xABCD,
                members: 3,
            }],
        });
        let datagram = server_message(&message, 9, true)?;
        session.handle_datagram(sim_addr(), &datagram, now)?;

        let (group_id, role_count, roles) = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::GroupRoleData {
                    group_id,
                    role_count,
                    roles,
                    ..
                } => Some((group_id, role_count, roles)),
                _ => None,
            })
            .ok_or("expected a GroupRoleData event")?;
        assert_eq!(group_id, GroupKey::from(group));
        assert_eq!(role_count, 5);
        assert_eq!(roles.len(), 1);
        let first = roles.first().ok_or("first role")?;
        assert_eq!(first.role_id, Some(GroupRoleKey::from(role)));
        assert_eq!(first.name, "Officers");
        Ok(())
    }

    #[test]
    fn group_role_members_reply_surfaces_total_pairs() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let group = uuid::Uuid::from_u128(0x6709);
        let role = uuid::Uuid::from_u128(0x670A);
        let member = uuid::Uuid::from_u128(0x670B);
        // 12 pairings in total across the multi-packet reply; this packet has one.
        let message = AnyMessage::GroupRoleMembersReply(GroupRoleMembersReply {
            agent_data: GroupRoleMembersReplyAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(1),
                group_id: group,
                request_id: uuid::Uuid::nil(),
                total_pairs: 12,
            },
            member_data: vec![GroupRoleMembersReplyMemberDataBlock {
                role_id: role,
                member_id: member,
            }],
        });
        let datagram = server_message(&message, 9, true)?;
        session.handle_datagram(sim_addr(), &datagram, now)?;

        let (group_id, total_pairs, pairs) = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::GroupRoleMembers {
                    group_id,
                    total_pairs,
                    pairs,
                    ..
                } => Some((group_id, total_pairs, pairs)),
                _ => None,
            })
            .ok_or("expected a GroupRoleMembers event")?;
        assert_eq!(group_id, GroupKey::from(group));
        assert_eq!(total_pairs, 12);
        assert_eq!(pairs.len(), 1);
        let first = pairs.first().ok_or("first pair")?;
        assert_eq!(first.role_id, Some(GroupRoleKey::from(role)));
        assert_eq!(first.member_id, AgentKey::from(member));
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
        assert_eq!(profile.group_id, GroupKey::from(group));
        assert_eq!(profile.name, "Test Group");
        assert_eq!(profile.charter, "a charter");
        assert_eq!(profile.founder_id, AgentKey::from(founder));
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
                assert_eq!(group_id, GroupKey::from(group));
                assert_eq!(from_agent_id, AgentKey::from(sender));
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
        session.activate_group(GroupKey::from(group), now)?;
        session.request_group_members(GroupKey::from(group), now)?;
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
        session.start_group_session(GroupKey::from(group), now)?;
        session.send_group_message(GroupKey::from(group), "hi all", now)?;
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
                insignia_id: None,
                membership_fee: LindenAmount(0),
                open_enrollment: true,
                allow_publish: false,
                mature_publish: false,
            },
            now,
        )?;
        let group = uuid::Uuid::from_u128(0x670C);
        let invitee = uuid::Uuid::from_u128(0x670D);
        session.join_group(GroupKey::from(group), now)?;
        session.leave_group(GroupKey::from(group), now)?;
        session.invite_to_group(
            GroupKey::from(group),
            &[(
                AgentKey::from(invitee),
                GroupRoleKey::from(uuid::Uuid::nil()),
            )],
            now,
        )?;
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
    fn link_inventory_item_packs_link() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let folder = uuid::Uuid::from_u128(0x6720);
        let linked = uuid::Uuid::from_u128(0x6721);
        session.link_inventory_item(
            &NewInventoryLink {
                folder_id: InventoryFolderKey::from(folder),
                linked_id: InventoryItemOrFolderKey::Item(InventoryKey::from(linked)),
                link_type: AssetType::Other(24), // AT_LINK
                inv_type: InventoryType::Script,
                name: "My Link".to_owned(),
                description: "a link".to_owned(),
            },
            now,
        )?;
        let sent = drain(&mut session)?;
        let link = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::LinkInventoryItem(l) => Some(l),
                _ => None,
            })
            .ok_or("expected a LinkInventoryItem")?;
        assert_eq!(link.inventory_block.folder_id, folder);
        assert_eq!(link.inventory_block.old_item_id, linked);
        assert_eq!(link.inventory_block.transaction_id, uuid::Uuid::nil());
        assert_eq!(link.inventory_block.r#type, 24);
        assert_eq!(link.inventory_block.inv_type, 10);
        assert_eq!(trimmed(&link.inventory_block.name), "My Link");
        assert_eq!(trimmed(&link.inventory_block.description), "a link");
        Ok(())
    }

    #[test]
    fn update_group_info_and_title_pack_messages() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let group = uuid::Uuid::from_u128(0x6730);
        let title_role = uuid::Uuid::from_u128(0x6731);
        session.update_group_info(
            &UpdateGroupInfoParams {
                group_id: GroupKey::from(group),
                charter: "new charter".to_owned(),
                show_in_list: false,
                insignia_id: Some(TextureKey::from(uuid::Uuid::from_u128(0x6732))),
                membership_fee: LindenAmount(42),
                open_enrollment: true,
                allow_publish: false,
                mature_publish: true,
            },
            now,
        )?;
        session.update_group_title(GroupKey::from(group), GroupRoleKey::from(title_role), now)?;
        let sent = drain(&mut session)?;

        let info = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::UpdateGroupInfo(u) => Some(u),
                _ => None,
            })
            .ok_or("expected an UpdateGroupInfo")?;
        assert_eq!(info.group_data.group_id, group);
        assert_eq!(trimmed(&info.group_data.charter), "new charter");
        assert!(!info.group_data.show_in_list);
        assert_eq!(info.group_data.membership_fee, 42);
        assert!(info.group_data.open_enrollment);
        assert!(info.group_data.mature_publish);

        let title = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::GroupTitleUpdate(t) => Some(t),
                _ => None,
            })
            .ok_or("expected a GroupTitleUpdate")?;
        assert_eq!(title.agent_data.group_id, group);
        assert_eq!(title.agent_data.title_role_id, title_role);
        Ok(())
    }

    #[test]
    fn teleport_via_landmark_packs_and_cancel_returns_to_active() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        // A landmark teleport carries the asset id and enters the teleporting
        // state with no destination hint (resolved sim-side).
        let landmark = uuid::Uuid::from_u128(0x6740);
        session.teleport_via_landmark(Some(AssetKey::from(landmark)), now)?;
        let sent = drain(&mut session)?;
        let request = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::TeleportLandmarkRequest(r) => Some(r),
                _ => None,
            })
            .ok_or("expected a TeleportLandmarkRequest")?;
        assert_eq!(request.info.landmark_id, landmark);

        // Cancelling returns the session to the active state and packs a
        // TeleportCancel.
        session.cancel_teleport(now)?;
        let sent = drain(&mut session)?;
        assert!(
            sent.iter()
                .any(|m| matches!(m, AnyMessage::TeleportCancel(_))),
            "expected a TeleportCancel"
        );

        // A home teleport (None) packs a nil landmark id; the session is active
        // again so the request is accepted.
        session.teleport_via_landmark(None, now)?;
        let sent = drain(&mut session)?;
        let home = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::TeleportLandmarkRequest(r) => Some(r),
                _ => None,
            })
            .ok_or("expected a home TeleportLandmarkRequest")?;
        assert_eq!(home.info.landmark_id, uuid::Uuid::nil());
        Ok(())
    }

    #[test]
    fn set_start_location_packs_slot_and_position() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        session.set_start_location(
            StartLocationSlot::Home,
            region_coords(64.0, 96.0, 25.0),
            vec3(1.0, 0.0, 0.0),
            now,
        )?;
        let sent = drain(&mut session)?;
        let request = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::SetStartLocationRequest(r) => Some(r),
                _ => None,
            })
            .ok_or("expected a SetStartLocationRequest")?;
        assert_eq!(request.start_location_data.location_id, 1); // HOME
        let pos = &request.start_location_data.location_pos;
        assert_eq!(pos.x.to_bits(), 64.0_f32.to_bits());
        assert_eq!(pos.y.to_bits(), 96.0_f32.to_bits());
        assert_eq!(pos.z.to_bits(), 25.0_f32.to_bits());
        assert_eq!(
            request.start_location_data.location_look_at.x.to_bits(),
            1.0_f32.to_bits()
        );
        // SimName is left empty for the simulator to fill in.
        assert_eq!(trimmed(&request.start_location_data.sim_name), "");
        Ok(())
    }

    #[test]
    fn agent_prefs_pack_data_request_quit_and_velocity_interp() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        session.request_agent_data_update(now)?;
        session.quit_copy(now)?;
        session.set_velocity_interpolation(true, now)?;
        session.set_velocity_interpolation(false, now)?;
        let sent = drain(&mut session)?;

        assert!(
            sent.iter()
                .any(|m| matches!(m, AnyMessage::AgentDataUpdateRequest(_))),
            "expected an AgentDataUpdateRequest"
        );
        let quit = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::AgentQuitCopy(q) => Some(q),
                _ => None,
            })
            .ok_or("expected an AgentQuitCopy")?;
        // The fuse block echoes this circuit's own code (non-zero once
        // established).
        assert_ne!(quit.fuse_block.viewer_circuit_code, 0);
        assert!(
            sent.iter()
                .any(|m| matches!(m, AnyMessage::VelocityInterpolateOn(_))),
            "expected a VelocityInterpolateOn"
        );
        assert!(
            sent.iter()
                .any(|m| matches!(m, AnyMessage::VelocityInterpolateOff(_))),
            "expected a VelocityInterpolateOff"
        );
        Ok(())
    }

    #[test]
    fn user_info_request_and_update_pack_prefs() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        session.request_user_info(now)?;
        session.update_user_info(true, DirectoryVisibility::Hidden, now)?;
        let sent = drain(&mut session)?;

        assert!(
            sent.iter()
                .any(|m| matches!(m, AnyMessage::UserInfoRequest(_))),
            "expected a UserInfoRequest"
        );
        let update = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::UpdateUserInfo(u) => Some(u),
                _ => None,
            })
            .ok_or("expected an UpdateUserInfo")?;
        assert!(update.user_data.im_via_e_mail);
        assert_eq!(trimmed(&update.user_data.directory_visibility), "hidden");
        Ok(())
    }

    #[test]
    fn trigger_sound_packs_asset_gain_handle_and_position() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let sound = uuid::Uuid::from_u128(0x5074_6e64);
        session.trigger_sound(
            AssetKey::from(sound),
            0.5,
            RegionHandle(1000),
            region_coords(64.0, 96.0, 25.0),
            now,
        )?;
        let sent = drain(&mut session)?;
        let trigger = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::SoundTrigger(t) => Some(t),
                _ => None,
            })
            .ok_or("expected a SoundTrigger")?;
        assert_eq!(trigger.sound_data.sound_id, sound);
        assert_eq!(trigger.sound_data.gain.to_bits(), 0.5_f32.to_bits());
        assert_eq!(trigger.sound_data.handle, 1000);
        assert_eq!(trigger.sound_data.position.x.to_bits(), 64.0_f32.to_bits());
        // The owner/object/parent ids are left nil for the simulator to fill in.
        assert!(trigger.sound_data.owner_id.is_nil());
        assert!(trigger.sound_data.object_id.is_nil());
        assert!(trigger.sound_data.parent_id.is_nil());
        Ok(())
    }

    #[test]
    fn god_region_estate_admin_messages_pack_targets_and_flags() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let target = AgentKey::from(uuid::Uuid::from_u128(0x0B10_CCED));
        session.request_godlike_powers(true, now)?;
        session.eject_user(target, EjectAction::EjectAndBan, now)?;
        session.freeze_user(target, FreezeAction::Freeze, now)?;
        session.sim_wide_deletes(
            target,
            SimWideDeleteFlags {
                others_land_only: true,
                always_return_objects: false,
                scripted_only: true,
            },
            now,
        )?;
        let sent = drain(&mut session)?;

        let godlike = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::RequestGodlikePowers(g) => Some(g),
                _ => None,
            })
            .ok_or("expected a RequestGodlikePowers")?;
        assert!(godlike.request_block.godlike);
        // The viewer packs a nil token; the simulator fills it in.
        assert!(godlike.request_block.token.is_nil());

        let eject = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::EjectUser(e) => Some(e),
                _ => None,
            })
            .ok_or("expected an EjectUser")?;
        assert_eq!(eject.data.target_id, target.uuid());
        assert_eq!(eject.data.flags, 0x1);

        let freeze = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::FreezeUser(f) => Some(f),
                _ => None,
            })
            .ok_or("expected a FreezeUser")?;
        assert_eq!(freeze.data.target_id, target.uuid());
        assert_eq!(freeze.data.flags, 0x0);

        let deletes = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::SimWideDeletes(d) => Some(d),
                _ => None,
            })
            .ok_or("expected a SimWideDeletes")?;
        assert_eq!(deletes.data_block.target_id, target.uuid());
        // SWD_OTHERS_LAND_ONLY (0x1) | SWD_SCRIPTED_ONLY (0x4).
        assert_eq!(deletes.data_block.flags, 0x5);
        Ok(())
    }

    #[test]
    fn god_update_region_info_packs_legacy_and_extended_flags() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let update = GodRegionUpdate {
            sim_name: RegionName::try_new("Da Boom").map_err(|_invalid| "invalid region name")?,
            estate_id: 1,
            parent_estate_id: 1,
            region_flags: 0x1_0000_0007,
            billable_factor: 1.0,
            price_per_meter: 5,
            redirect_grid: GridCoordinates::new(1000, 1001),
        };
        session.god_update_region_info(&update, now)?;
        let sent = drain(&mut session)?;
        let god = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::GodUpdateRegionInfo(g) => Some(g),
                _ => None,
            })
            .ok_or("expected a GodUpdateRegionInfo")?;
        assert_eq!(trimmed(&god.region_info.sim_name), "Da Boom");
        assert_eq!(god.region_info.estate_id, 1);
        // The legacy field is the low 32 bits of the extended flags.
        assert_eq!(god.region_info.region_flags, 0x0000_0007);
        assert_eq!(god.region_info.redirect_grid_x, 1000);
        assert_eq!(god.region_info.redirect_grid_y, 1001);
        let extended = god
            .region_info2
            .first()
            .ok_or("expected a RegionInfo2 block")?;
        assert_eq!(extended.region_flags_extended, 0x1_0000_0007);
        Ok(())
    }

    #[test]
    fn god_parcel_object_land_admin_messages_pack_targets() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        let circuit = session.root_circuit_id().ok_or("no circuit")?;
        drain(&mut session)?;

        let owner = AgentKey::from(uuid::Uuid::from_u128(0x0_F0E));
        let snapshot = TextureKey::from(uuid::Uuid::from_u128(0x5_4A9));
        session.parcel_god_force_owner(
            ScopedParcelId::new(circuit, sl_proto::RegionLocalParcelId(12)),
            OwnerKey::Agent(owner),
            now,
        )?;
        session.parcel_god_mark_as_content(
            ScopedParcelId::new(circuit, sl_proto::RegionLocalParcelId(7)),
            now,
        )?;
        session.event_god_delete(
            EventId::new(99),
            QueryId::from(uuid::Uuid::from_u128(0xA17)),
            "music fest",
            DirFindFlags::from_bits(32),
            10,
            now,
        )?;
        session.state_save("", now)?;
        session.viewer_start_auction(
            ScopedParcelId::new(circuit, sl_proto::RegionLocalParcelId(5)),
            Some(snapshot),
            now,
        )?;
        let sent = drain(&mut session)?;

        let force_owner = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::ParcelGodForceOwner(p) => Some(p),
                _ => None,
            })
            .ok_or("expected a ParcelGodForceOwner")?;
        assert_eq!(force_owner.data.local_id, 12);
        assert_eq!(force_owner.data.owner_id, owner.uuid());

        let mark = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::ParcelGodMarkAsContent(p) => Some(p),
                _ => None,
            })
            .ok_or("expected a ParcelGodMarkAsContent")?;
        assert_eq!(mark.parcel_data.local_id, 7);

        let event = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::EventGodDelete(e) => Some(e),
                _ => None,
            })
            .ok_or("expected an EventGodDelete")?;
        assert_eq!(event.event_data.event_id, 99);
        assert_eq!(trimmed(&event.query_data.query_text), "music fest");
        assert_eq!(event.query_data.query_flags, 32);
        assert_eq!(event.query_data.query_start, 10);

        let state = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::StateSave(s) => Some(s),
                _ => None,
            })
            .ok_or("expected a StateSave")?;
        // An empty filename packs as a lone nul terminator, as the viewer sends.
        assert_eq!(trimmed(&state.data_block.filename), "");

        let auction = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::ViewerStartAuction(a) => Some(a),
                _ => None,
            })
            .ok_or("expected a ViewerStartAuction")?;
        assert_eq!(auction.parcel_data.local_id, 5);
        assert_eq!(auction.parcel_data.snapshot_id, snapshot.uuid());
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
            GroupKey::from(group),
            &[
                GroupRoleEdit {
                    role_id: Some(GroupRoleKey::from(role)),
                    name: "Officers".to_owned(),
                    description: "the officers".to_owned(),
                    title: "Officer".to_owned(),
                    powers: group_powers::MEMBER_INVITE | group_powers::NOTICES_SEND,
                    update_type: GroupRoleUpdateType::Create,
                },
                GroupRoleEdit {
                    role_id: Some(GroupRoleKey::from(uuid::Uuid::from_u128(0x6713))),
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
            GroupKey::from(group),
            &[
                GroupRoleMemberChange {
                    role_id: Some(GroupRoleKey::from(role)),
                    member_id: AgentKey::from(member),
                    change: GroupRoleChange::Add,
                },
                GroupRoleMemberChange {
                    role_id: Some(GroupRoleKey::from(role)),
                    member_id: AgentKey::from(uuid::Uuid::from_u128(0x6717)),
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
        session.eject_group_members(GroupKey::from(group), &[AgentKey::from(ejectee)], now)?;
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
    fn activate_gestures_packs_request() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let item = uuid::Uuid::from_u128(0x6E5_7001);
        let asset = uuid::Uuid::from_u128(0x6E5_7002);
        session.activate_gestures(
            &[GestureActivation {
                item_id: InventoryKey::from(item),
                asset_id: asset,
            }],
            now,
        )?;
        let sent = drain(&mut session)?;
        let activate = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::ActivateGestures(a) => Some(a),
                _ => None,
            })
            .ok_or("expected an ActivateGestures")?;
        let entry = activate.data.first().ok_or("first gesture")?;
        assert_eq!(entry.item_id, item);
        assert_eq!(entry.asset_id, asset);
        Ok(())
    }

    #[test]
    fn deactivate_gestures_packs_request() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let item = uuid::Uuid::from_u128(0x6E5_7003);
        session.deactivate_gestures(&[InventoryKey::from(item)], now)?;
        let sent = drain(&mut session)?;
        let deactivate = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::DeactivateGestures(d) => Some(d),
                _ => None,
            })
            .ok_or("expected a DeactivateGestures")?;
        assert_eq!(deactivate.data.first().map(|d| d.item_id), Some(item));
        Ok(())
    }

    #[test]
    fn set_always_run_packs_request() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        session.set_always_run(MovementMode::AlwaysRun, now)?;
        let sent = drain(&mut session)?;
        let set = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::SetAlwaysRun(s) => Some(s),
                _ => None,
            })
            .ok_or("expected a SetAlwaysRun")?;
        assert!(set.agent_data.always_run);
        Ok(())
    }

    #[test]
    fn agent_pause_resume_increment_serial() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        session.pause_agent(now)?;
        session.resume_agent(now)?;
        let sent = drain(&mut session)?;
        let pause_serial = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::AgentPause(p) => Some(p.agent_data.serial_num),
                _ => None,
            })
            .ok_or("expected an AgentPause")?;
        let resume_serial = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::AgentResume(r) => Some(r.agent_data.serial_num),
                _ => None,
            })
            .ok_or("expected an AgentResume")?;
        // The serial is monotonic and shared by both messages.
        assert!(resume_serial > pause_serial);
        Ok(())
    }

    #[test]
    fn agent_fov_packs_request() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        session.set_agent_fov(1.25, now)?;
        let sent = drain(&mut session)?;
        let fov = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::AgentFOV(f) => Some(f),
                _ => None,
            })
            .ok_or("expected an AgentFOV")?;
        assert_eq!(fov.fov_block.vertical_angle.to_bits(), 1.25_f32.to_bits());
        assert_eq!(fov.fov_block.gen_counter, 0);
        Ok(())
    }

    #[test]
    fn agent_height_width_packs_request() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        session.set_agent_size(768, 1024, now)?;
        let sent = drain(&mut session)?;
        let size = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::AgentHeightWidth(s) => Some(s),
                _ => None,
            })
            .ok_or("expected an AgentHeightWidth")?;
        assert_eq!(size.height_width_block.height, 768);
        assert_eq!(size.height_width_block.width, 1024);
        Ok(())
    }

    #[test]
    fn force_script_control_release_packs_request() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        session.release_script_controls(now)?;
        let sent = drain(&mut session)?;
        assert!(
            sent.iter()
                .any(|m| matches!(m, AnyMessage::ForceScriptControlRelease(_)))
        );
        Ok(())
    }

    #[test]
    fn script_control_change_surfaces_event() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let message = AnyMessage::ScriptControlChange(sl_wire::messages::ScriptControlChange {
            data: vec![sl_wire::messages::ScriptControlChangeDataBlock {
                take_controls: true,
                controls: ControlFlags::AT_POS.bits() | ControlFlags::FLY.bits(),
                pass_to_agent: false,
            }],
        });
        let datagram = server_message(&message, 9001, true)?;
        session.handle_datagram(sim_addr(), &datagram, now)?;
        let control = drain_events(&mut session)
            .into_iter()
            .find_map(|e| match e {
                Event::ScriptControlChange(controls) => controls.into_iter().next(),
                _ => None,
            })
            .ok_or("expected a ScriptControlChange event")?;
        assert_eq!(control.action, ScriptControlAction::Take);
        assert!(!control.pass_to_agent);
        assert_eq!(control.controls, ControlFlags::AT_POS | ControlFlags::FLY);
        Ok(())
    }

    /// Feeds the client one inbound `ScriptControlChange` with a single data
    /// block (`take` controls / release them, optionally also passed to the
    /// agent), so the taken-controls tracker folds it.
    fn feed_script_control_change(
        session: &mut Session,
        now: Instant,
        sequence: u32,
        take: bool,
        controls: ControlFlags,
        pass_to_agent: bool,
    ) -> Result<(), TestError> {
        let message = AnyMessage::ScriptControlChange(sl_wire::messages::ScriptControlChange {
            data: vec![sl_wire::messages::ScriptControlChangeDataBlock {
                take_controls: take,
                controls: controls.bits(),
                pass_to_agent,
            }],
        });
        let datagram = server_message(&message, sequence, true)?;
        session.handle_datagram(sim_addr(), &datagram, now)?;
        drain_events(session);
        Ok(())
    }

    #[test]
    fn taken_controls_track_take_and_release() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // A Take records the named controls in the consumed (taken) set.
        let held = ControlFlags::AT_POS | ControlFlags::FLY;
        feed_script_control_change(&mut session, now, 9101, true, held, false)?;
        assert_eq!(session.script_controls().taken, held);
        assert_eq!(
            session.script_controls().passed_to_agent,
            ControlFlags::empty()
        );

        // A matching Release empties the set.
        feed_script_control_change(&mut session, now, 9102, false, held, false)?;
        assert_eq!(session.script_controls().taken, ControlFlags::empty());
        Ok(())
    }

    #[test]
    fn taken_controls_use_a_count_model() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // Two scripts take the same control bit; one releasing it must not clear
        // it for the other (the per-bit count survives the first release).
        feed_script_control_change(&mut session, now, 9111, true, ControlFlags::AT_POS, false)?;
        feed_script_control_change(&mut session, now, 9112, true, ControlFlags::AT_POS, false)?;
        feed_script_control_change(&mut session, now, 9113, false, ControlFlags::AT_POS, false)?;
        assert_eq!(session.script_controls().taken, ControlFlags::AT_POS);

        // The second release finally clears it.
        feed_script_control_change(&mut session, now, 9114, false, ControlFlags::AT_POS, false)?;
        assert_eq!(session.script_controls().taken, ControlFlags::empty());
        Ok(())
    }

    #[test]
    fn taken_controls_split_pass_to_agent() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // A take with `PassToAgent = true` lands in the passed-to-agent set, not
        // the consumed (taken) set.
        feed_script_control_change(&mut session, now, 9121, true, ControlFlags::LEFT_POS, true)?;
        assert_eq!(session.script_controls().taken, ControlFlags::empty());
        assert_eq!(
            session.script_controls().passed_to_agent,
            ControlFlags::LEFT_POS
        );
        Ok(())
    }

    #[test]
    fn release_script_controls_clears_taken_but_keeps_grant() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // Record a TAKE_CONTROLS grant and a live taken control (both sets).
        let task = ObjectKey::from(uuid::Uuid::from_u128(0xB3C1));
        let item = InventoryKey::from(uuid::Uuid::from_u128(0xB3C2));
        let take_controls = ScriptPermissions(ScriptPermissions::TAKE_CONTROLS);
        session.answer_script_permissions(task, item, take_controls, None, now)?;
        feed_script_control_change(&mut session, now, 9131, true, ControlFlags::AT_POS, false)?;
        feed_script_control_change(&mut session, now, 9132, true, ControlFlags::UP_POS, true)?;
        drain(&mut session)?;

        // Releasing controls clears both taken sets immediately on send.
        session.release_script_controls(now)?;
        assert_eq!(session.script_controls().taken, ControlFlags::empty());
        assert_eq!(
            session.script_controls().passed_to_agent,
            ControlFlags::empty()
        );

        // The TAKE_CONTROLS grant persists (only the live taken set resets).
        assert_eq!(session.granted_permissions(task, item), take_controls);
        Ok(())
    }

    #[test]
    fn script_permission_state_bundles_grants_and_controls() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // Record one grant and take one control (both permission stores).
        let task = ObjectKey::from(uuid::Uuid::from_u128(0xB401));
        let item = InventoryKey::from(uuid::Uuid::from_u128(0xB402));
        let granted = ScriptPermissions(
            ScriptPermissions::TAKE_CONTROLS | ScriptPermissions::TRIGGER_ANIMATION,
        );
        session.answer_script_permissions(task, item, granted, None, now)?;
        feed_script_control_change(&mut session, now, 9401, true, ControlFlags::AT_POS, false)?;
        drain(&mut session)?;

        // The snapshot reflects both stores at once.
        let state = session.script_permission_state();
        let grant = state
            .grants
            .iter()
            .find(|g| g.task_id == task && g.item_id == item)
            .ok_or("expected the recorded grant in the snapshot")?;
        assert_eq!(grant.granted, granted);
        assert!(!grant.denied);
        assert_eq!(state.controls.taken, ControlFlags::AT_POS);
        assert_eq!(state.controls.passed_to_agent, ControlFlags::empty());

        // The snapshot agrees with the individual accessors.
        assert_eq!(state.grants, session.script_grants().collect::<Vec<_>>());
        assert_eq!(state.controls, session.script_controls());
        Ok(())
    }

    #[test]
    fn follow_cam_properties_surface_events() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let object = uuid::Uuid::from_u128(0xCA3_0001);
        let set = AnyMessage::SetFollowCamProperties(sl_wire::messages::SetFollowCamProperties {
            object_data: sl_wire::messages::SetFollowCamPropertiesObjectDataBlock {
                object_id: object,
            },
            camera_property: vec![
                sl_wire::messages::SetFollowCamPropertiesCameraPropertyBlock {
                    r#type: FollowCamProperty::Distance.to_i32(),
                    value: 4.5,
                },
            ],
        });
        let set_datagram = server_message(&set, 9002, true)?;
        session.handle_datagram(sim_addr(), &set_datagram, now)?;
        let (set_object, properties) = drain_events(&mut session)
            .into_iter()
            .find_map(|e| match e {
                Event::SetFollowCamProperties {
                    object_id,
                    properties,
                } => Some((object_id, properties)),
                _ => None,
            })
            .ok_or("expected a SetFollowCamProperties event")?;
        assert_eq!(set_object, ObjectKey::from(object));
        let first = properties.first().ok_or("first property")?;
        assert_eq!(first.property, FollowCamProperty::Distance);
        assert_eq!(first.value.to_bits(), 4.5_f32.to_bits());

        let clear =
            AnyMessage::ClearFollowCamProperties(sl_wire::messages::ClearFollowCamProperties {
                object_data: sl_wire::messages::ClearFollowCamPropertiesObjectDataBlock {
                    object_id: object,
                },
            });
        let clear_datagram = server_message(&clear, 9003, true)?;
        session.handle_datagram(sim_addr(), &clear_datagram, now)?;
        let cleared = drain_events(&mut session)
            .into_iter()
            .find_map(|e| match e {
                Event::ClearFollowCamProperties { object_id } => Some(object_id),
                _ => None,
            })
            .ok_or("expected a ClearFollowCamProperties event")?;
        assert_eq!(cleared, ObjectKey::from(object));
        Ok(())
    }

    #[test]
    fn alert_messages_surface_events() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // A general AlertMessage with both a plain string and a keyed AlertInfo.
        let agent = uuid::Uuid::from_u128(0xA1E_0001);
        let alert = AnyMessage::AlertMessage(sl_wire::messages::AlertMessage {
            alert_data: sl_wire::messages::AlertMessageAlertDataBlock {
                message: b"You have been warned".to_vec(),
            },
            alert_info: vec![sl_wire::messages::AlertMessageAlertInfoBlock {
                message: b"RegionEntryAccessBlocked".to_vec(),
                extra_params: b"REGION=Foo".to_vec(),
            }],
            agent_info: vec![sl_wire::messages::AlertMessageAgentInfoBlock { agent_id: agent }],
        });
        let datagram = server_message(&alert, 9101, false)?;
        session.handle_datagram(sim_addr(), &datagram, now)?;
        let (message, alert_info, agents) = drain_events(&mut session)
            .into_iter()
            .find_map(|e| match e {
                Event::AlertMessage {
                    message,
                    alert_info,
                    agents,
                } => Some((message, alert_info, agents)),
                _ => None,
            })
            .ok_or("expected an AlertMessage event")?;
        assert_eq!(message, "You have been warned");
        let info = alert_info.first().ok_or("first alert info")?;
        assert_eq!(info.message, "RegionEntryAccessBlocked");
        assert_eq!(info.extra_params, "REGION=Foo");
        assert_eq!(agents.first().copied(), Some(agent));

        // An AgentAlertMessage directed at a specific agent.
        let agent_alert = AnyMessage::AgentAlertMessage(sl_wire::messages::AgentAlertMessage {
            agent_data: sl_wire::messages::AgentAlertMessageAgentDataBlock { agent_id: agent },
            alert_data: sl_wire::messages::AgentAlertMessageAlertDataBlock {
                modal: true,
                message: b"Please confirm".to_vec(),
            },
        });
        let datagram = server_message(&agent_alert, 9102, false)?;
        session.handle_datagram(sim_addr(), &datagram, now)?;
        let (agent_id, modal, message) = drain_events(&mut session)
            .into_iter()
            .find_map(|e| match e {
                Event::AgentAlertMessage {
                    agent_id,
                    modal,
                    message,
                } => Some((agent_id, modal, message)),
                _ => None,
            })
            .ok_or("expected an AgentAlertMessage event")?;
        assert_eq!(agent_id, AgentKey::from(agent));
        assert!(modal);
        assert_eq!(message, "Please confirm");
        Ok(())
    }

    #[test]
    fn mean_collision_alert_surfaces_event() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let victim = uuid::Uuid::from_u128(0xC011_DE01);
        let perp = uuid::Uuid::from_u128(0xC011_DE02);
        let message = AnyMessage::MeanCollisionAlert(sl_wire::messages::MeanCollisionAlert {
            mean_collision: vec![sl_wire::messages::MeanCollisionAlertMeanCollisionBlock {
                victim,
                perp,
                time: 1_700_000_000,
                mag: 12.5,
                r#type: MeanCollisionType::Bump.to_u8(),
            }],
        });
        let datagram = server_message(&message, 9103, true)?;
        session.handle_datagram(sim_addr(), &datagram, now)?;
        let collision = drain_events(&mut session)
            .into_iter()
            .find_map(|e| match e {
                Event::MeanCollisionAlert(collisions) => collisions.into_iter().next(),
                _ => None,
            })
            .ok_or("expected a MeanCollisionAlert event")?;
        assert_eq!(collision.victim, victim);
        assert_eq!(collision.perp, perp);
        assert_eq!(collision.time, 1_700_000_000);
        assert_eq!(collision.magnitude.to_bits(), 12.5_f32.to_bits());
        assert_eq!(collision.collision_type, MeanCollisionType::Bump);
        Ok(())
    }

    #[test]
    fn health_and_camera_constraint_surface_events() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let health = AnyMessage::HealthMessage(sl_wire::messages::HealthMessage {
            health_data: sl_wire::messages::HealthMessageHealthDataBlock { health: 87.5 },
        });
        let datagram = server_message(&health, 9104, true)?;
        session.handle_datagram(sim_addr(), &datagram, now)?;
        let value = drain_events(&mut session)
            .into_iter()
            .find_map(|e| match e {
                Event::HealthMessage { health } => Some(health),
                _ => None,
            })
            .ok_or("expected a HealthMessage event")?;
        assert_eq!(value.to_bits(), 87.5_f32.to_bits());

        let plane = [0.0_f32, 0.0, 1.0, 5.0];
        let constraint = AnyMessage::CameraConstraint(sl_wire::messages::CameraConstraint {
            camera_collide_plane: sl_wire::messages::CameraConstraintCameraCollidePlaneBlock {
                plane,
            },
        });
        let datagram = server_message(&constraint, 9105, true)?;
        session.handle_datagram(sim_addr(), &datagram, now)?;
        let got = drain_events(&mut session)
            .into_iter()
            .find_map(|e| match e {
                Event::CameraConstraint { plane } => Some(plane),
                _ => None,
            })
            .ok_or("expected a CameraConstraint event")?;
        assert_eq!(got.map(f32::to_bits), plane.map(f32::to_bits));
        Ok(())
    }

    #[test]
    fn viewer_frozen_surfaces_event() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        for (data, seq) in [(true, 9106), (false, 9107)] {
            let frozen = AnyMessage::ViewerFrozenMessage(sl_wire::messages::ViewerFrozenMessage {
                frozen_data: sl_wire::messages::ViewerFrozenMessageFrozenDataBlock { data },
            });
            let datagram = server_message(&frozen, seq, true)?;
            session.handle_datagram(sim_addr(), &datagram, now)?;
            let value = drain_events(&mut session)
                .into_iter()
                .find_map(|e| match e {
                    Event::ViewerFrozen { frozen } => Some(frozen),
                    _ => None,
                })
                .ok_or("expected a ViewerFrozen event")?;
            assert_eq!(value, data);
        }
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
        assert_eq!(group_id, GroupKey::from(group));
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
        session.send_group_notice(GroupKey::from(group), "Subject", "Body text", None, now)?;
        // A notice with an inventory attachment (LLSD bucket).
        session.send_group_notice(
            GroupKey::from(group),
            "Gift",
            "Here you go",
            Some(GroupNoticeAttachment {
                item_id: InventoryKey::from(item),
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
        assert_eq!(
            first.group_id,
            GroupKey::from(uuid::Uuid::from_u128(0x6701))
        );
        assert_eq!(first.group_name, "CAPS Group");
        assert_eq!(first.group_powers, 4660);
        assert!(first.accept_notices);
        assert_eq!(first.contribution, LandArea(25));
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
        assert_eq!(group_id, GroupKey::from(uuid::Uuid::from_u128(0x6703)));
        assert_eq!(members.len(), 1);
        let first = members.first().ok_or("first member")?;
        assert_eq!(
            first.agent_id,
            AgentKey::from(uuid::Uuid::from_u128(0x6704))
        );
        assert_eq!(first.title, "Owner");
        assert_eq!(first.agent_powers, 0xabcd);
        assert_eq!(first.online_status, "Online");
        assert_eq!(first.contribution, LandArea(512));
        assert!(first.is_owner);
        Ok(())
    }

    #[test]
    fn agent_state_update_caps_surfaces_navmesh_permission() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // The pathfinding `AgentStateUpdate` push: a single capability flag.
        let body = parse_llsd_xml(concat!(
            "<llsd><map>",
            "<key>can_modify_navmesh</key><boolean>1</boolean>",
            "</map></llsd>",
        ))?;
        session.handle_caps_event("AgentStateUpdate", &body, now)?;

        let can_modify = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::AgentStateUpdate { can_modify_navmesh } => Some(can_modify_navmesh),
                _ => None,
            })
            .ok_or("expected an AgentStateUpdate event")?;
        assert!(can_modify);
        Ok(())
    }

    #[test]
    fn nav_mesh_status_update_caps_surfaces_status() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // The pathfinding `NavMeshStatusUpdate` push: region, version, status.
        let body = parse_llsd_xml(concat!(
            "<llsd><map>",
            "<key>region_id</key><uuid>00000000-0000-0000-0000-000000009a01</uuid>",
            "<key>version</key><integer>7</integer>",
            "<key>status</key><string>building</string>",
            "</map></llsd>",
        ))?;
        session.handle_caps_event("NavMeshStatusUpdate", &body, now)?;

        let status = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::NavMeshStatus(status) => Some(status),
                _ => None,
            })
            .ok_or("expected a NavMeshStatus event")?;
        assert_eq!(
            status,
            NavMeshStatus {
                region_id: uuid::Uuid::from_u128(0x9a01),
                version: 7,
                status: NavMeshBuildStatus::Building,
            }
        );
        Ok(())
    }

    #[test]
    fn agent_drop_group_caps_surfaces_dropped_group() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // The `AgentDropGroup` push wraps a single `AgentData` element; only the
        // `GroupID` matters (the echoed `AgentID` is this agent itself).
        let body = parse_llsd_xml(concat!(
            "<llsd><map><key>AgentData</key><array><map>",
            "<key>AgentID</key><uuid>00000000-0000-0000-0000-0000000000a1</uuid>",
            "<key>GroupID</key><uuid>00000000-0000-0000-0000-0000000067b2</uuid>",
            "</map></array></map></llsd>",
        ))?;
        session.handle_caps_event("AgentDropGroup", &body, now)?;

        let group = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::AgentDroppedFromGroup { group } => Some(group),
                _ => None,
            })
            .ok_or("expected an AgentDroppedFromGroup event")?;
        assert_eq!(group, GroupKey::from(uuid::Uuid::from_u128(0x67b2)));
        Ok(())
    }

    #[test]
    fn display_name_update_caps_surfaces_new_record() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // The `DisplayNameUpdate` push: old name plus the new `agent` record
        // (People API fields, no embedded id — it comes from `agent_id`).
        let body = parse_llsd_xml(concat!(
            "<llsd><map>",
            "<key>agent_id</key><uuid>00000000-0000-0000-0000-0000000000a1</uuid>",
            "<key>old_display_name</key><string>Old Name</string>",
            "<key>agent</key><map>",
            "<key>username</key><string>james.linden</string>",
            "<key>display_name</key><string>James the Great</string>",
            "<key>legacy_first_name</key><string>James</string>",
            "<key>legacy_last_name</key><string>Linden</string>",
            "<key>is_display_name_default</key><boolean>0</boolean>",
            "</map></map></llsd>",
        ))?;
        session.handle_caps_event("DisplayNameUpdate", &body, now)?;

        let update = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::DisplayNameUpdate(update) => Some(update),
                _ => None,
            })
            .ok_or("expected a DisplayNameUpdate event")?;
        let expected = DisplayName {
            id: AgentKey::from(uuid::Uuid::from_u128(0xa1)),
            username: "james.linden".to_owned(),
            display_name: "James the Great".to_owned(),
            legacy_first_name: "James".to_owned(),
            legacy_last_name: "Linden".to_owned(),
            is_display_name_default: false,
            display_name_expires: String::new(),
            display_name_next_update: String::new(),
            missing: false,
        };
        assert_eq!(
            *update,
            DisplayNameUpdate {
                old_display_name: "Old Name".to_owned(),
                name: expected,
            }
        );
        Ok(())
    }

    #[test]
    fn set_display_name_reply_caps_surfaces_result() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // A successful `SetDisplayNameReply`: status 200, the new name in
        // `content.display_name`.
        let body = parse_llsd_xml(concat!(
            "<llsd><map>",
            "<key>status</key><integer>200</integer>",
            "<key>reason</key><string>OK</string>",
            "<key>content</key><map>",
            "<key>display_name</key><string>James the Great</string>",
            "</map></map></llsd>",
        ))?;
        session.handle_caps_event("SetDisplayNameReply", &body, now)?;

        let reply = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::SetDisplayNameReply(reply) => Some(reply),
                _ => None,
            })
            .ok_or("expected a SetDisplayNameReply event")?;
        assert_eq!(
            *reply,
            SetDisplayNameReply {
                status: 200,
                reason: "OK".to_owned(),
                new_display_name: Some("James the Great".to_owned()),
                error_tag: None,
            }
        );
        assert!(reply.succeeded());
        Ok(())
    }

    #[test]
    fn windlight_refresh_caps_surfaces_interpolate_flag() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // The `WindLightRefresh` push: a single `Interpolate` flag.
        let body = parse_llsd_xml(concat!(
            "<llsd><map>",
            "<key>Interpolate</key><integer>1</integer>",
            "</map></llsd>",
        ))?;
        session.handle_caps_event("WindLightRefresh", &body, now)?;

        let interpolate = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::WindLightRefresh { interpolate } => Some(interpolate),
                _ => None,
            })
            .ok_or("expected a WindLightRefresh event")?;
        assert!(interpolate);
        Ok(())
    }

    #[test]
    fn sim_console_response_caps_surfaces_output_string() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // The `SimConsoleResponse` body is a bare LLSD string, not a map.
        let body = parse_llsd_xml("<llsd><string>Region restart scheduled.</string></llsd>")?;
        session.handle_caps_event("SimConsoleResponse", &body, now)?;

        let output = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::SimConsoleResponse { output } => Some(output),
                _ => None,
            })
            .ok_or("expected a SimConsoleResponse event")?;
        assert_eq!(output, "Region restart scheduled.");
        Ok(())
    }

    #[test]
    fn required_voice_version_caps_surfaces_version() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // The `RequiredVoiceVersion` push: voice protocol version + backend.
        let body = parse_llsd_xml(concat!(
            "<llsd><map>",
            "<key>major_version</key><integer>1</integer>",
            "<key>region_name</key><string>Hippo Hollow</string>",
            "<key>voice_server_type</key><string>webrtc</string>",
            "</map></llsd>",
        ))?;
        session.handle_caps_event("RequiredVoiceVersion", &body, now)?;

        let version = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::RequiredVoiceVersion(version) => Some(version),
                _ => None,
            })
            .ok_or("expected a RequiredVoiceVersion event")?;
        assert_eq!(
            version,
            RequiredVoiceVersion {
                major_version: 1,
                region_name: "Hippo Hollow".to_owned(),
                voice_server_type: Some("webrtc".to_owned()),
            }
        );
        Ok(())
    }

    #[test]
    fn open_region_info_caps_surfaces_present_fields_only() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // OpenSim sends only the keys it overrides; absent keys stay `None`.
        // Cover an int flag, a real, an int limit, and a grouped position bound.
        let body = parse_llsd_xml(concat!(
            "<llsd><map>",
            "<key>AllowMinimap</key><integer>0</integer>",
            "<key>DrawDistance</key><real>512.0</real>",
            "<key>MaxLinkCount</key><integer>64</integer>",
            "<key>MaxPosX</key><real>256.0</real>",
            "<key>MaxPosY</key><real>256.0</real>",
            "<key>MaxPosZ</key><real>4096.0</real>",
            "</map></llsd>",
        ))?;
        session.handle_caps_event("OpenRegionInfo", &body, now)?;

        let info = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::OpenRegionInfo(info) => Some(info),
                _ => None,
            })
            .ok_or("expected an OpenRegionInfo event")?;
        assert_eq!(info.allow_minimap, Some(false));
        assert_eq!(info.draw_distance, Some(512.0));
        assert_eq!(info.max_link_count, Some(64));
        assert_eq!(
            info.max_position,
            Some(RegionCoordinates::new(256.0, 256.0, 4096.0))
        );
        // Keys the sim did not send stay absent.
        assert_eq!(info.min_position, None);
        assert_eq!(info.max_groups, None);
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
        assert_eq!(dialog.object_id, ObjectKey::from(object));
        assert_eq!(dialog.owner_id, Some(owner));
        assert_eq!(dialog.object_name, "Vendor");
        assert_eq!(dialog.message, "Pick one");
        assert_eq!(dialog.chat_channel, ChatChannel(-1234));
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
        assert_eq!(request.task_id, ObjectKey::from(task));
        assert_eq!(request.item_id, InventoryKey::from(item));
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
        session.reply_script_dialog(ObjectKey::from(object), ChatChannel(-1234), 1, "No", now)?;
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
            ObjectKey::from(task),
            InventoryKey::from(item),
            ScriptPermissions(ScriptPermissions::TAKE_CONTROLS),
            None,
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
        session.request_texture(TextureKey::from(texture), 0, 1.0e6, now)?;

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
        assert_eq!(received.id, TextureKey::from(texture));
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
        session.request_texture(TextureKey::from(texture), 3, 1.0e6, now)?;
        drain(&mut session)?;

        let missing = AnyMessage::ImageNotInDatabase(ImageNotInDatabase {
            image_id: ImageNotInDatabaseImageIDBlock { id: texture },
        });
        session.handle_datagram(sim_addr(), &server_message(&missing, 9, true)?, now)?;

        assert!(
            drain_events(&mut session).iter().any(
                |e| matches!(e, Event::TextureNotFound(id) if *id == TextureKey::from(texture))
            )
        );
        Ok(())
    }

    #[test]
    fn request_asset_reassembles_transfer_packets() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let sound = uuid::Uuid::from_u128(0x5005);
        session.request_asset(AssetKey::from(sound), AssetType::Sound, 1.0, now)?;

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
        // A success TransferInfo surfaces a transfer-started event carrying the
        // declared total size; the asset bytes still follow as TransferPackets.
        let started = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::AssetTransferStarted {
                    asset_id,
                    asset_type,
                    size,
                } => Some((asset_id, asset_type, size)),
                _ => None,
            })
            .ok_or("expected an AssetTransferStarted event")?;
        assert_eq!(started, (sound, AssetType::Sound, 6));

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
        session.request_asset(AssetKey::from(missing), AssetType::Animation, 1.0, now)?;
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
        assert_eq!(appearance.avatar_id, AgentKey::from(avatar));
        assert_eq!(appearance.visual_params, vec![10, 200, 255]);
        // The baked head texture decodes at its slot; an untouched slot is nil.
        assert_eq!(
            appearance
                .texture_entry
                .texture_id(avatar_texture::HEAD_BAKED),
            Some(TextureKey::from(head_bake))
        );
        assert_eq!(
            appearance
                .texture_entry
                .texture_id(avatar_texture::UPPER_BAKED),
            Some(TextureKey::from(uuid::Uuid::nil()))
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
        assert_eq!(shape.item_id, InventoryKey::from(shape_item));
        assert_eq!(shape.asset_id, Some(shape_asset));
        assert_eq!(shape.wearable_type, WearableType::Shape);
        assert!(shape.wearable_type.is_body_part());
        let shirt = wearables.get(1).ok_or("second wearable")?;
        assert_eq!(shirt.wearable_type, WearableType::Shirt);
        assert!(!shirt.wearable_type.is_body_part());
        Ok(())
    }

    #[test]
    fn attach_object_encodes_object_attach() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        let circuit = session.root_circuit_id().ok_or("no circuit")?;
        drain(&mut session)?;

        session.attach_object(
            ScopedObjectId::new(circuit, sl_proto::RegionLocalObjectId(42)),
            AttachmentPoint::RightHand,
            AttachmentMode::Add,
            &Rotation {
                x: 0.0,
                y: 0.0,
                z: 0.0,
                s: 1.0,
            },
            now,
        )?;
        let sent = drain(&mut session)?;
        let attach = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::ObjectAttach(attach) => Some(attach),
                _ => None,
            })
            .ok_or("expected an ObjectAttach")?;
        // The add flag (0x80) is OR'd onto the right-hand point code (6).
        assert_eq!(attach.agent_data.attachment_point, 0x80 | 6);
        let object = attach.object_data.first().ok_or("first object")?;
        assert_eq!(object.object_local_id, 42);
        Ok(())
    }

    #[test]
    fn detach_and_drop_encode_local_ids() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        let circuit = session.root_circuit_id().ok_or("no circuit")?;
        drain(&mut session)?;

        session.detach_objects(
            &[
                ScopedObjectId::new(circuit, sl_proto::RegionLocalObjectId(7)),
                ScopedObjectId::new(circuit, sl_proto::RegionLocalObjectId(8)),
            ],
            now,
        )?;
        session.drop_attachments(
            &[ScopedObjectId::new(
                circuit,
                sl_proto::RegionLocalObjectId(9),
            )],
            now,
        )?;
        let sent = drain(&mut session)?;
        let detach = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::ObjectDetach(detach) => Some(detach),
                _ => None,
            })
            .ok_or("expected an ObjectDetach")?;
        let detach_ids: Vec<u32> = detach
            .object_data
            .iter()
            .map(|object| object.object_local_id)
            .collect();
        assert_eq!(detach_ids, vec![7, 8]);
        let drop = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::ObjectDrop(drop) => Some(drop),
                _ => None,
            })
            .ok_or("expected an ObjectDrop")?;
        let drop_ids: Vec<u32> = drop
            .object_data
            .iter()
            .map(|object| object.object_local_id)
            .collect();
        assert_eq!(drop_ids, vec![9]);
        Ok(())
    }

    #[test]
    fn remove_attachment_encodes_item_and_point() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let item = uuid::Uuid::from_u128(0xAA);
        session.remove_attachment(AttachmentPoint::Skull, InventoryKey::from(item), now)?;
        let sent = drain(&mut session)?;
        let remove = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::RemoveAttachment(remove) => Some(remove),
                _ => None,
            })
            .ok_or("expected a RemoveAttachment")?;
        assert_eq!(remove.attachment_block.attachment_point, 2); // Skull, no add flag
        assert_eq!(remove.attachment_block.item_id, item);
        Ok(())
    }

    #[test]
    fn rez_attachment_encodes_rez_single() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let item = uuid::Uuid::from_u128(0xB1);
        let owner = uuid::Uuid::from_u128(0xB2);
        session.rez_attachment(
            &RezAttachment {
                item_id: InventoryKey::from(item),
                owner_id: owner,
                attachment_point: AttachmentPoint::Chest,
                mode: AttachmentMode::Replace,
                name: "hat".to_owned(),
                description: "a hat".to_owned(),
            },
            now,
        )?;
        let sent = drain(&mut session)?;
        let rez = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::RezSingleAttachmentFromInv(rez) => Some(rez),
                _ => None,
            })
            .ok_or("expected a RezSingleAttachmentFromInv")?;
        assert_eq!(rez.object_data.item_id, item);
        assert_eq!(rez.object_data.owner_id, owner);
        assert_eq!(rez.object_data.attachment_pt, 1); // Chest, no add flag
        assert_eq!(rez.object_data.name, b"hat\0");
        Ok(())
    }

    #[test]
    fn rez_attachments_encodes_compound_message() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let compound = uuid::Uuid::from_u128(0xC0);
        let attachments = vec![
            RezAttachment {
                item_id: InventoryKey::from(uuid::Uuid::from_u128(0xD1)),
                owner_id: uuid::Uuid::from_u128(0xD0),
                attachment_point: AttachmentPoint::LeftHand,
                mode: AttachmentMode::Add,
                name: String::new(),
                description: String::new(),
            },
            RezAttachment {
                item_id: InventoryKey::from(uuid::Uuid::from_u128(0xD2)),
                owner_id: uuid::Uuid::from_u128(0xD0),
                attachment_point: AttachmentPoint::Default,
                mode: AttachmentMode::Replace,
                name: String::new(),
                description: String::new(),
            },
        ];
        session.rez_attachments(
            TransactionId::from(compound),
            DetachOrder::DetachAllFirst,
            &attachments,
            now,
        )?;
        let sent = drain(&mut session)?;
        let rez = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::RezMultipleAttachmentsFromInv(rez) => Some(rez),
                _ => None,
            })
            .ok_or("expected a RezMultipleAttachmentsFromInv")?;
        assert_eq!(rez.header_data.compound_msg_id, compound);
        assert_eq!(rez.header_data.total_objects, 2);
        assert!(rez.header_data.first_detach_all);
        assert_eq!(rez.object_data.len(), 2);
        let first = rez.object_data.first().ok_or("first object")?;
        assert_eq!(first.attachment_pt, 0x80 | 5); // LeftHand + add flag
        Ok(())
    }

    #[test]
    fn viewer_effect_encodes_lookat() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let self_id = uuid::Uuid::from_u128(0xE0);
        let target = uuid::Uuid::from_u128(0xE1);
        session.send_viewer_effect(
            &[ViewerEffect {
                id: uuid::Uuid::from_u128(0xEF),
                agent_id: AgentKey::from(self_id),
                effect_type: ViewerEffectType::LookAt,
                duration: 2.0,
                color: [255, 0, 0, 255],
                data: ViewerEffectData::LookAt {
                    source: Some(AgentKey::from(self_id)),
                    target: Some(ObjectKey::from(target)),
                    target_position: GlobalCoordinates::new(1.0, 2.0, 3.0),
                    look_at_type: LookAtType::Focus,
                },
            }],
            now,
        )?;
        let sent = drain(&mut session)?;
        let effect = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::ViewerEffect(effect) => Some(effect),
                _ => None,
            })
            .ok_or("expected a ViewerEffect")?;
        let block = effect.effect.first().ok_or("first effect")?;
        assert_eq!(block.r#type, 14); // LL_HUD_EFFECT_LOOKAT
        assert_eq!(block.color, [255, 0, 0, 255]);
        // The 57-byte LookAt TypeData round-trips back to the typed form.
        assert_eq!(
            ViewerEffectData::from_wire(ViewerEffectType::LookAt, &block.type_data),
            ViewerEffectData::LookAt {
                source: Some(AgentKey::from(self_id)),
                target: Some(ObjectKey::from(target)),
                target_position: GlobalCoordinates::new(1.0, 2.0, 3.0),
                look_at_type: LookAtType::Focus,
            },
        );
        Ok(())
    }

    #[test]
    fn track_and_find_agent_encode() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let prey = uuid::Uuid::from_u128(0xF1);
        let hunter = uuid::Uuid::from_u128(0xF0);
        session.track_agent(AgentKey::from(prey), now)?;
        session.find_agent(AgentKey::from(hunter), AgentKey::from(prey), now)?;
        let sent = drain(&mut session)?;
        let track = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::TrackAgent(track) => Some(track),
                _ => None,
            })
            .ok_or("expected a TrackAgent")?;
        assert_eq!(track.target_data.prey_id, prey);
        let find = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::FindAgent(find) => Some(find),
                _ => None,
            })
            .ok_or("expected a FindAgent")?;
        assert_eq!(find.agent_block.hunter, hunter);
        assert_eq!(find.agent_block.prey, prey);
        assert!(find.location_block.is_empty()); // request carries no locations
        Ok(())
    }

    #[test]
    fn coarse_location_update_surfaces_nearby_avatars() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let me = uuid::Uuid::from_u128(0x1);
        let other = uuid::Uuid::from_u128(0x2);
        let message = AnyMessage::CoarseLocationUpdate(CoarseLocationUpdate {
            location: vec![
                CoarseLocationUpdateLocationBlock {
                    x: 128,
                    y: 64,
                    z: 5, // metres / 4 → 20 m
                },
                CoarseLocationUpdateLocationBlock { x: 10, y: 20, z: 6 },
            ],
            index: CoarseLocationUpdateIndexBlock { you: 0, prey: 1 },
            agent_data: vec![
                CoarseLocationUpdateAgentDataBlock { agent_id: me },
                CoarseLocationUpdateAgentDataBlock { agent_id: other },
            ],
        });
        session.handle_datagram(sim_addr(), &server_message(&message, 9, true)?, now)?;

        let (locations, you, prey) = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::CoarseLocationUpdate {
                    locations,
                    you,
                    prey,
                } => Some((locations, you, prey)),
                _ => None,
            })
            .ok_or("expected a CoarseLocationUpdate event")?;
        assert_eq!(you, Some(0));
        assert_eq!(prey, Some(1));
        assert_eq!(
            locations,
            vec![
                CoarseLocation {
                    agent_id: AgentKey::from(me),
                    x: 128,
                    y: 64,
                    z: 20,
                },
                CoarseLocation {
                    agent_id: AgentKey::from(other),
                    x: 10,
                    y: 20,
                    z: 24,
                },
            ],
        );
        Ok(())
    }

    #[test]
    fn viewer_effect_surfaces_received_effect() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let source = uuid::Uuid::from_u128(0x10);
        let target = uuid::Uuid::from_u128(0x11);
        let data = ViewerEffectData::PointAt {
            source: Some(AgentKey::from(source)),
            target: Some(ObjectKey::from(target)),
            target_position: GlobalCoordinates::new(4.0, 5.0, 6.0),
            point_at_type: PointAtType::Grab,
        };
        let message = AnyMessage::ViewerEffect(ViewerEffectMessage {
            agent_data: ViewerEffectAgentDataBlock {
                agent_id: source,
                session_id: uuid::Uuid::nil(),
            },
            effect: vec![ViewerEffectEffectBlock {
                id: uuid::Uuid::from_u128(0x12),
                agent_id: source,
                r#type: 15, // LL_HUD_EFFECT_POINTAT
                duration: 1.5,
                color: [0, 255, 0, 255],
                type_data: data.to_wire(),
            }],
        });
        session.handle_datagram(sim_addr(), &server_message(&message, 9, true)?, now)?;

        let effects = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::ViewerEffect(effects) => Some(effects),
                _ => None,
            })
            .ok_or("expected a ViewerEffect event")?;
        let effect = effects.first().ok_or("first effect")?;
        assert_eq!(effect.effect_type, ViewerEffectType::PointAt);
        assert_eq!(effect.data, data);
        Ok(())
    }

    #[test]
    fn find_agent_reply_surfaces_locations() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let hunter = uuid::Uuid::from_u128(0x20);
        let prey = uuid::Uuid::from_u128(0x21);
        let message = AnyMessage::FindAgent(FindAgent {
            agent_block: FindAgentAgentBlockBlock {
                hunter,
                prey,
                space_ip: [0, 0, 0, 0],
            },
            location_block: vec![FindAgentLocationBlockBlock {
                global_x: 256_000.0,
                global_y: 257_000.0,
            }],
        });
        session.handle_datagram(sim_addr(), &server_message(&message, 9, true)?, now)?;

        let (reply_prey, locations) = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::FindAgentReply {
                    prey, locations, ..
                } => Some((prey, locations)),
                _ => None,
            })
            .ok_or("expected a FindAgentReply event")?;
        assert_eq!(reply_prey, prey);
        assert_eq!(locations, vec![(256_000.0, 257_000.0)]);
        Ok(())
    }

    #[test]
    fn dir_find_query_encode() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let query_id = uuid::Uuid::from_u128(0x30);
        session.dir_find_query(
            QueryId::from(query_id),
            "alice",
            DirFindFlags::PEOPLE.union(DirFindFlags::ONLINE),
            20,
            now,
        )?;
        let sent = drain(&mut session)?;
        let query = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::DirFindQuery(query) => Some(query),
                _ => None,
            })
            .ok_or("expected a DirFindQuery")?;
        assert_eq!(query.query_data.query_id, query_id);
        assert_eq!(query.query_data.query_text, b"alice\0");
        assert_eq!(
            query.query_data.query_flags,
            DirFindFlags::PEOPLE.union(DirFindFlags::ONLINE).bits()
        );
        assert_eq!(query.query_data.query_start, 20);
        Ok(())
    }

    #[test]
    fn dir_people_reply_surfaces_results() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let query_id = uuid::Uuid::from_u128(0x31);
        let agent = uuid::Uuid::from_u128(0x32);
        let message = AnyMessage::DirPeopleReply(DirPeopleReply {
            agent_data: DirPeopleReplyAgentDataBlock {
                agent_id: uuid::Uuid::nil(),
            },
            query_data: DirPeopleReplyQueryDataBlock { query_id },
            query_replies: vec![DirPeopleReplyQueryRepliesBlock {
                agent_id: agent,
                first_name: b"Alice\0".to_vec(),
                last_name: b"Resident\0".to_vec(),
                group: Vec::new(),
                online: true,
                reputation: 0,
            }],
        });
        session.handle_datagram(sim_addr(), &server_message(&message, 9, true)?, now)?;

        let (reply_query, results) = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::DirPeopleReply { query_id, results } => Some((query_id, results)),
                _ => None,
            })
            .ok_or("expected a DirPeopleReply event")?;
        assert_eq!(reply_query, query_id);
        let person = results.first().ok_or("first person")?;
        assert_eq!(person.agent_id, AgentKey::from(agent));
        assert_eq!(person.first_name, "Alice");
        assert_eq!(person.last_name, "Resident");
        assert!(person.online);
        Ok(())
    }

    #[test]
    fn event_info_request_encode() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        session.event_info_request(EventId::new(42), now)?;
        session.event_notification_add_request(EventId::new(42), now)?;
        session.event_notification_remove_request(EventId::new(7), now)?;
        let sent = drain(&mut session)?;

        let info = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::EventInfoRequest(request) => Some(request),
                _ => None,
            })
            .ok_or("expected an EventInfoRequest")?;
        assert_eq!(info.event_data.event_id, 42);

        let add = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::EventNotificationAddRequest(request) => Some(request),
                _ => None,
            })
            .ok_or("expected an EventNotificationAddRequest")?;
        assert_eq!(add.event_data.event_id, 42);

        let remove = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::EventNotificationRemoveRequest(request) => Some(request),
                _ => None,
            })
            .ok_or("expected an EventNotificationRemoveRequest")?;
        assert_eq!(remove.event_data.event_id, 7);
        Ok(())
    }

    #[test]
    fn event_info_reply_surfaces_detail() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let creator = uuid::Uuid::from_u128(0x33);
        let message = AnyMessage::EventInfoReply(EventInfoReply {
            agent_data: EventInfoReplyAgentDataBlock {
                agent_id: uuid::Uuid::nil(),
            },
            event_data: EventInfoReplyEventDataBlock {
                event_id: 42,
                creator: format!("{creator}\0").into_bytes(),
                name: b"Beach Party\0".to_vec(),
                category: b"Discussion\0".to_vec(),
                desc: b"Come along\0".to_vec(),
                date: b"2026-06-20 12:00:00\0".to_vec(),
                date_utc: 1_750_000_000,
                duration: 60,
                cover: 1,
                amount: 50,
                sim_name: b"Sandbox\0".to_vec(),
                global_pos: [256_000.0, 257_000.0, 30.0],
                event_flags: 0,
            },
        });
        session.handle_datagram(sim_addr(), &server_message(&message, 9, true)?, now)?;

        let info = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::EventInfoReply { info } => Some(info),
                _ => None,
            })
            .ok_or("expected an EventInfoReply event")?;
        assert_eq!(info.event_id, EventId::new(42));
        assert_eq!(info.creator, AgentKey::from(creator));
        assert_eq!(info.name, "Beach Party");
        assert_eq!(info.category, "Discussion");
        assert_eq!(info.description, "Come along");
        assert_eq!(info.date_utc, 1_750_000_000);
        assert_eq!(info.duration, 60);
        assert_eq!(info.cover, 1);
        assert_eq!(info.amount, Some(LindenAmount(50)));
        assert_eq!(info.sim_name, region_name("Sandbox"));
        assert_eq!(
            info.global_position,
            GlobalCoordinates::new(256_000.0, 257_000.0, 30.0)
        );
        Ok(())
    }

    #[test]
    fn object_commerce_commands_encode() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        let circuit = session.root_circuit_id().ok_or("no circuit")?;
        drain(&mut session)?;

        let object = uuid::Uuid::from_u128(0xB0B);
        session.buy_object(
            GroupKey::from(uuid::Uuid::nil()),
            uuid::Uuid::from_u128(0xCA7),
            &[ObjectBuyItem {
                local_id: sl_proto::RegionLocalObjectId(99),
                sale_type: SaleType::Copy,
                sale_price: LindenAmount(250),
            }],
            now,
        )?;
        session.buy_object_inventory(
            ObjectKey::from(object),
            InventoryKey::from(uuid::Uuid::from_u128(0x17E)),
            InventoryFolderKey::from(uuid::Uuid::nil()),
            now,
        )?;
        session.request_pay_price(ObjectKey::from(object), now)?;
        session.request_object_properties_family(0x04, ObjectKey::from(object), now)?;
        session.spin_object_start(ObjectKey::from(object), now)?;
        session.spin_object_update(
            ObjectKey::from(object),
            Rotation {
                x: 0.0,
                y: 0.0,
                z: 0.0,
                s: 1.0,
            },
            now,
        )?;
        session.spin_object_stop(ObjectKey::from(object), now)?;
        session.duplicate_objects_on_ray(
            &[ScopedObjectId::new(
                circuit,
                sl_proto::RegionLocalObjectId(99),
            )],
            None,
            Vector {
                x: 1.0,
                y: 2.0,
                z: 3.0,
            },
            Vector {
                x: 4.0,
                y: 5.0,
                z: 6.0,
            },
            false,
            true,
            true,
            false,
            None,
            0,
            now,
        )?;
        let sent = drain(&mut session)?;

        let buy = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::ObjectBuy(buy) => Some(buy),
                _ => None,
            })
            .ok_or("expected an ObjectBuy")?;
        assert_eq!(buy.agent_data.category_id, uuid::Uuid::from_u128(0xCA7));
        let item = buy.object_data.first().ok_or("expected one buy item")?;
        assert_eq!(item.object_local_id, 99);
        assert_eq!(item.sale_type, SaleType::Copy.to_code());
        assert_eq!(item.sale_price, 250);

        let inv = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::BuyObjectInventory(buy) => Some(buy),
                _ => None,
            })
            .ok_or("expected a BuyObjectInventory")?;
        assert_eq!(inv.data.object_id, object);
        assert_eq!(inv.data.item_id, uuid::Uuid::from_u128(0x17E));

        let pay = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::RequestPayPrice(request) => Some(request),
                _ => None,
            })
            .ok_or("expected a RequestPayPrice")?;
        assert_eq!(pay.object_data.object_id, object);

        let family = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::RequestObjectPropertiesFamily(request) => Some(request),
                _ => None,
            })
            .ok_or("expected a RequestObjectPropertiesFamily")?;
        assert_eq!(family.object_data.request_flags, 0x04);
        assert_eq!(family.object_data.object_id, object);

        assert!(
            sent.iter()
                .any(|m| matches!(m, AnyMessage::ObjectSpinStart(_))),
            "expected an ObjectSpinStart"
        );
        assert!(
            sent.iter()
                .any(|m| matches!(m, AnyMessage::ObjectSpinUpdate(_))),
            "expected an ObjectSpinUpdate"
        );
        assert!(
            sent.iter()
                .any(|m| matches!(m, AnyMessage::ObjectSpinStop(_))),
            "expected an ObjectSpinStop"
        );

        let dup = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::ObjectDuplicateOnRay(dup) => Some(dup),
                _ => None,
            })
            .ok_or("expected an ObjectDuplicateOnRay")?;
        assert!(dup.agent_data.ray_end_is_intersection);
        assert!(dup.agent_data.copy_centers);
        assert!(!dup.agent_data.copy_rotates);
        let dup_item = dup.object_data.first().ok_or("expected one dup item")?;
        assert_eq!(dup_item.object_local_id, 99);
        Ok(())
    }

    #[test]
    fn rez_commands_encode() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        session.rez_restore_to_world(
            &RestoreItem {
                item_id: InventoryKey::from(uuid::Uuid::from_u128(0x17E)),
                folder_id: InventoryFolderKey::from(uuid::Uuid::nil()),
                creator_id: AgentKey::from(uuid::Uuid::nil()),
                owner: sl_proto::OwnerKey::Agent(AgentKey::from(uuid::Uuid::nil())),
                group: None,
                permissions: Permissions5 {
                    base: Permissions::from_bits(0x0008_e000),
                    owner: Permissions::from_bits(0x0008_e000),
                    group: Permissions::NONE,
                    everyone: Permissions::NONE,
                    next_owner: Permissions::from_bits(0x0008_e000),
                },
                transaction_id: uuid::Uuid::nil(),
                asset_type: 6,
                inv_type: 6,
                flags: 0,
                sale_type: SaleType::NotForSale,
                sale_price: Some(LindenAmount(0)),
                name: "Cube".to_owned(),
                description: "a cube".to_owned(),
                creation_date: 1_750_000_000,
                crc: 0,
            },
            now,
        )?;
        session.rez_object_from_notecard(
            &NotecardRez {
                group_id: None,
                from_task_id: None,
                bypass_raycast: false,
                ray_start: Vector {
                    x: 1.0,
                    y: 2.0,
                    z: 3.0,
                },
                ray_end: Vector {
                    x: 4.0,
                    y: 5.0,
                    z: 6.0,
                },
                ray_target_id: None,
                ray_end_is_intersection: true,
                rez_selected: false,
                remove_item: false,
                item_flags: 0,
                group_mask: 0,
                everyone_mask: 0,
                next_owner_mask: 0x0008_e000,
                notecard_item_id: InventoryKey::from(uuid::Uuid::from_u128(0xCA5E)),
                object_id: ObjectKey::from(uuid::Uuid::nil()),
                item_ids: vec![
                    InventoryKey::from(uuid::Uuid::from_u128(0x1)),
                    InventoryKey::from(uuid::Uuid::from_u128(0x2)),
                ],
            },
            now,
        )?;
        let sent = drain(&mut session)?;

        let restore = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::RezRestoreToWorld(restore) => Some(restore),
                _ => None,
            })
            .ok_or("expected a RezRestoreToWorld")?;
        assert_eq!(restore.inventory_data.item_id, uuid::Uuid::from_u128(0x17E));
        assert_eq!(restore.inventory_data.r#type, 6);
        assert_eq!(restore.inventory_data.creation_date, 1_750_000_000);

        let rez = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::RezObjectFromNotecard(rez) => Some(rez),
                _ => None,
            })
            .ok_or("expected a RezObjectFromNotecard")?;
        assert_eq!(
            rez.notecard_data.notecard_item_id,
            uuid::Uuid::from_u128(0xCA5E)
        );
        assert!(rez.rez_data.ray_end_is_intersection);
        assert_eq!(rez.inventory_data.len(), 2);
        Ok(())
    }

    #[test]
    fn rez_inventory_and_revoke_commands_encode() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        let circuit = session.root_circuit_id().ok_or("no circuit")?;
        drain(&mut session)?;

        let item = RestoreItem {
            item_id: InventoryKey::from(uuid::Uuid::from_u128(0x17E)),
            folder_id: InventoryFolderKey::from(uuid::Uuid::nil()),
            creator_id: AgentKey::from(uuid::Uuid::nil()),
            owner: OwnerKey::Agent(AgentKey::from(uuid::Uuid::nil())),
            group: None,
            permissions: Permissions5 {
                base: Permissions::from_bits(0x0008_e000),
                owner: Permissions::from_bits(0x0008_e000),
                group: Permissions::NONE,
                everyone: Permissions::NONE,
                next_owner: Permissions::from_bits(0x0008_e000),
            },
            transaction_id: uuid::Uuid::nil(),
            asset_type: 6,
            inv_type: 6,
            flags: 0,
            sale_type: SaleType::NotForSale,
            sale_price: Some(LindenAmount(0)),
            name: "Cube".to_owned(),
            description: "a cube".to_owned(),
            creation_date: 1_750_000_000,
            crc: 0,
        };

        session.rez_object_from_inventory(
            &RezObjectParams {
                group_id: None,
                from_task_id: None,
                bypass_raycast: false,
                ray_start: Vector {
                    x: 1.0,
                    y: 2.0,
                    z: 3.0,
                },
                ray_end: Vector {
                    x: 4.0,
                    y: 5.0,
                    z: 6.0,
                },
                ray_target_id: None,
                ray_end_is_intersection: true,
                rez_selected: false,
                remove_item: false,
                item_flags: 0,
                group_mask: 0,
                everyone_mask: 0,
                next_owner_mask: 0x0008_e000,
                item: item.clone(),
            },
            now,
        )?;
        session.rez_script(
            ScopedObjectId::new(circuit, sl_proto::RegionLocalObjectId(77)),
            &RezScriptParams {
                group_id: None,
                enabled: true,
                item,
            },
            now,
        )?;
        session.revoke_script_permissions(
            ObjectKey::from(uuid::Uuid::from_u128(0x0B1E)),
            ScriptPermissions(
                ScriptPermissions::TAKE_CONTROLS | ScriptPermissions::TRIGGER_ANIMATION,
            ),
            now,
        )?;
        session.detach_attachment_into_inventory(
            InventoryKey::from(uuid::Uuid::from_u128(0xA77AC)),
            now,
        )?;
        let sent = drain(&mut session)?;

        let rez = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::RezObject(rez) => Some(rez),
                _ => None,
            })
            .ok_or("expected a RezObject")?;
        assert_eq!(rez.inventory_data.item_id, uuid::Uuid::from_u128(0x17E));
        assert_eq!(rez.inventory_data.r#type, 6);
        assert!(rez.rez_data.ray_end_is_intersection);
        assert_eq!(rez.rez_data.next_owner_mask, 0x0008_e000);

        let script = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::RezScript(script) => Some(script),
                _ => None,
            })
            .ok_or("expected a RezScript")?;
        assert_eq!(script.update_block.object_local_id, 77);
        assert!(script.update_block.enabled);
        assert_eq!(script.inventory_block.item_id, uuid::Uuid::from_u128(0x17E));

        let revoke = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::RevokePermissions(revoke) => Some(revoke),
                _ => None,
            })
            .ok_or("expected a RevokePermissions")?;
        assert_eq!(revoke.data.object_id, uuid::Uuid::from_u128(0x0B1E));
        assert_eq!(
            revoke.data.object_permissions,
            (ScriptPermissions::TAKE_CONTROLS | ScriptPermissions::TRIGGER_ANIMATION)
                .cast_unsigned()
        );

        let detach = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::DetachAttachmentIntoInv(detach) => Some(detach),
                _ => None,
            })
            .ok_or("expected a DetachAttachmentIntoInv")?;
        assert_eq!(detach.object_data.item_id, uuid::Uuid::from_u128(0xA77AC));
        Ok(())
    }

    #[test]
    fn task_inventory_commands_encode() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        let circuit = session.root_circuit_id().ok_or("no circuit")?;
        drain(&mut session)?;

        let target = ScopedObjectId::new(circuit, sl_proto::RegionLocalObjectId(55));
        let item = RestoreItem {
            item_id: InventoryKey::from(uuid::Uuid::from_u128(0x17E)),
            folder_id: InventoryFolderKey::from(uuid::Uuid::from_u128(0xF01D)),
            creator_id: AgentKey::from(uuid::Uuid::nil()),
            owner: OwnerKey::Agent(AgentKey::from(uuid::Uuid::nil())),
            group: None,
            permissions: Permissions5 {
                base: Permissions::from_bits(0x0008_e000),
                owner: Permissions::from_bits(0x0008_e000),
                group: Permissions::NONE,
                everyone: Permissions::NONE,
                next_owner: Permissions::from_bits(0x0008_e000),
            },
            transaction_id: uuid::Uuid::nil(),
            asset_type: 10,
            inv_type: 10,
            flags: 0,
            sale_type: SaleType::NotForSale,
            sale_price: Some(LindenAmount(0)),
            name: "script".to_owned(),
            description: "a script".to_owned(),
            creation_date: 1_750_000_000,
            crc: 0,
        };

        session.request_task_inventory(target, now)?;
        session.update_task_inventory(target, TaskInventoryKey::Asset, &item, now)?;
        session.move_task_inventory(
            target,
            InventoryFolderKey::from(uuid::Uuid::from_u128(0xF01D)),
            InventoryKey::from(uuid::Uuid::from_u128(0x17E)),
            now,
        )?;
        session.remove_task_inventory(
            target,
            InventoryKey::from(uuid::Uuid::from_u128(0x17E)),
            now,
        )?;
        let sent = drain(&mut session)?;

        let request = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::RequestTaskInventory(request) => Some(request),
                _ => None,
            })
            .ok_or("expected a RequestTaskInventory")?;
        assert_eq!(request.inventory_data.local_id, 55);

        let update = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::UpdateTaskInventory(update) => Some(update),
                _ => None,
            })
            .ok_or("expected an UpdateTaskInventory")?;
        assert_eq!(update.update_data.local_id, 55);
        assert_eq!(update.update_data.key, TaskInventoryKey::Asset.to_code());
        assert_eq!(update.inventory_data.item_id, uuid::Uuid::from_u128(0x17E));
        assert_eq!(update.inventory_data.r#type, 10);

        let move_msg = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::MoveTaskInventory(move_msg) => Some(move_msg),
                _ => None,
            })
            .ok_or("expected a MoveTaskInventory")?;
        assert_eq!(move_msg.inventory_data.local_id, 55);
        assert_eq!(move_msg.agent_data.folder_id, uuid::Uuid::from_u128(0xF01D));
        assert_eq!(
            move_msg.inventory_data.item_id,
            uuid::Uuid::from_u128(0x17E)
        );

        let remove = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::RemoveTaskInventory(remove) => Some(remove),
                _ => None,
            })
            .ok_or("expected a RemoveTaskInventory")?;
        assert_eq!(remove.inventory_data.local_id, 55);
        assert_eq!(remove.inventory_data.item_id, uuid::Uuid::from_u128(0x17E));
        Ok(())
    }

    #[test]
    fn land_and_parcel_commands_encode() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        let circuit = session.root_circuit_id().ok_or("no circuit")?;
        drain(&mut session)?;

        let parcel = ScopedParcelId::new(circuit, sl_proto::RegionLocalParcelId(12));
        session.modify_land(
            &LandEdit {
                action: LandBrushAction::Raise,
                brush_size: LandBrushSize::Large,
                strength: 2.5,
                height: 21.0,
                parcel: Some(sl_proto::RegionLocalParcelId(7)),
                area: TerraformArea::new(16.0, 32.0, 48.0, 64.0),
            },
            now,
        )?;
        session.undo_land(now)?;
        session.request_parcel_properties_by_id(parcel, 3, now)?;
        session.set_parcel_other_clean_time(
            parcel,
            std::time::Duration::from_secs(30 * 60),
            now,
        )?;
        let sent = drain(&mut session)?;

        let modify = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::ModifyLand(modify) => Some(modify),
                _ => None,
            })
            .ok_or("expected a ModifyLand")?;
        assert_eq!(modify.modify_block.action, LandBrushAction::Raise.to_code());
        assert_eq!(
            modify.modify_block.brush_size,
            LandBrushSize::Large.to_index()
        );
        assert_eq!(modify.modify_block.seconds.to_bits(), 2.5_f32.to_bits());
        assert_eq!(modify.modify_block.height.to_bits(), 21.0_f32.to_bits());
        let block = modify.parcel_data.first().ok_or("expected ParcelData")?;
        assert_eq!(block.local_id, 7);
        assert_eq!(block.west.to_bits(), 16.0_f32.to_bits());
        assert_eq!(block.north.to_bits(), 64.0_f32.to_bits());
        let extended = modify
            .modify_block_extended
            .first()
            .ok_or("expected ModifyBlockExtended")?;
        assert_eq!(
            extended.brush_size.to_bits(),
            LandBrushSize::Large.to_metres().to_bits()
        );

        assert!(
            sent.iter().any(|m| matches!(m, AnyMessage::UndoLand(_))),
            "expected an UndoLand"
        );

        let by_id = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::ParcelPropertiesRequestByID(by_id) => Some(by_id),
                _ => None,
            })
            .ok_or("expected a ParcelPropertiesRequestByID")?;
        assert_eq!(by_id.parcel_data.local_id, 12);
        assert_eq!(by_id.parcel_data.sequence_id, 3);

        let clean = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::ParcelSetOtherCleanTime(clean) => Some(clean),
                _ => None,
            })
            .ok_or("expected a ParcelSetOtherCleanTime")?;
        assert_eq!(clean.parcel_data.local_id, 12);
        assert_eq!(clean.parcel_data.other_clean_time, 30);
        Ok(())
    }

    #[test]
    fn script_g9_commands_encode() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let object_id = uuid::Uuid::from_u128(0x0B1E);
        let item_id = uuid::Uuid::from_u128(0x17E3);
        session.request_script_running(
            ObjectKey::from(object_id),
            InventoryKey::from(item_id),
            now,
        )?;
        session.set_script_running(
            ObjectKey::from(object_id),
            InventoryKey::from(item_id),
            true,
            now,
        )?;
        session.reset_script(ObjectKey::from(object_id), InventoryKey::from(item_id), now)?;
        let sent = drain(&mut session)?;

        let get = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::GetScriptRunning(get) => Some(get),
                _ => None,
            })
            .ok_or("expected a GetScriptRunning")?;
        assert_eq!(get.script.object_id, object_id);
        assert_eq!(get.script.item_id, item_id);

        let set = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::SetScriptRunning(set) => Some(set),
                _ => None,
            })
            .ok_or("expected a SetScriptRunning")?;
        assert_eq!(set.script.object_id, object_id);
        assert_eq!(set.script.item_id, item_id);
        assert!(set.script.running);

        let reset = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::ScriptReset(reset) => Some(reset),
                _ => None,
            })
            .ok_or("expected a ScriptReset")?;
        assert_eq!(reset.script.object_id, object_id);
        assert_eq!(reset.script.item_id, item_id);
        Ok(())
    }

    #[test]
    fn script_running_reply_surfaces_event() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        let reply = AnyMessage::ScriptRunningReply(ScriptRunningReply {
            script: ScriptRunningReplyScriptBlock {
                object_id: uuid::Uuid::from_u128(0x0B1E),
                item_id: uuid::Uuid::from_u128(0x17E3),
                running: true,
            },
        });
        session.handle_datagram(sim_addr(), &server_message(&reply, 9, true)?, now)?;

        let running = drain_events(&mut session)
            .into_iter()
            .find_map(|e| match e {
                Event::ScriptRunning {
                    object_id,
                    item_id,
                    running,
                } => Some((object_id, item_id, running)),
                _ => None,
            })
            .ok_or("expected a ScriptRunning event")?;
        assert_eq!(running.0, ObjectKey::from(uuid::Uuid::from_u128(0x0B1E)));
        assert_eq!(running.1, InventoryKey::from(uuid::Uuid::from_u128(0x17E3)));
        assert!(running.2);
        Ok(())
    }

    #[test]
    fn group_finance_g10_commands_encode() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let group_id = uuid::Uuid::from_u128(0x6A0D);
        let request_id = uuid::Uuid::from_u128(0xF00D);
        let transaction_id = uuid::Uuid::from_u128(0x7AC7);
        let proposal_id = uuid::Uuid::from_u128(0x9A0E);
        session.request_group_account_summary(
            GroupKey::from(group_id),
            GroupRequestId::from(request_id),
            60,
            0,
            now,
        )?;
        session.request_group_account_details(
            GroupKey::from(group_id),
            GroupRequestId::from(request_id),
            60,
            0,
            now,
        )?;
        session.request_group_account_transactions(
            GroupKey::from(group_id),
            GroupRequestId::from(request_id),
            60,
            0,
            now,
        )?;
        session.request_group_active_proposals(
            GroupKey::from(group_id),
            TransactionId::from(transaction_id),
            now,
        )?;
        session.request_group_vote_history(
            GroupKey::from(group_id),
            TransactionId::from(transaction_id),
            now,
        )?;
        session.start_group_proposal(
            GroupKey::from(group_id),
            3,
            0.5,
            86_400,
            "Adopt the bylaws?",
            now,
        )?;
        session.cast_group_proposal_ballot(
            sl_proto::ProposalVoteId::from(proposal_id),
            GroupKey::from(group_id),
            "yes",
            now,
        )?;
        let sent = drain(&mut session)?;

        let summary = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::GroupAccountSummaryRequest(req) => Some(req),
                _ => None,
            })
            .ok_or("expected a GroupAccountSummaryRequest")?;
        assert_eq!(summary.agent_data.group_id, group_id);
        assert_eq!(summary.money_data.request_id, request_id);
        assert_eq!(summary.money_data.interval_days, 60);

        assert!(
            sent.iter()
                .any(|m| matches!(m, AnyMessage::GroupAccountDetailsRequest(_))),
            "expected a GroupAccountDetailsRequest"
        );
        assert!(
            sent.iter()
                .any(|m| matches!(m, AnyMessage::GroupAccountTransactionsRequest(_))),
            "expected a GroupAccountTransactionsRequest"
        );

        let proposals = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::GroupActiveProposalsRequest(req) => Some(req),
                _ => None,
            })
            .ok_or("expected a GroupActiveProposalsRequest")?;
        assert_eq!(proposals.group_data.group_id, group_id);
        assert_eq!(proposals.transaction_data.transaction_id, transaction_id);

        assert!(
            sent.iter()
                .any(|m| matches!(m, AnyMessage::GroupVoteHistoryRequest(_))),
            "expected a GroupVoteHistoryRequest"
        );

        let start = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::StartGroupProposal(req) => Some(req),
                _ => None,
            })
            .ok_or("expected a StartGroupProposal")?;
        assert_eq!(start.proposal_data.group_id, group_id);
        assert_eq!(start.proposal_data.quorum, 3);
        assert_eq!(start.proposal_data.duration, 86_400);
        assert!((start.proposal_data.majority - 0.5).abs() < f32::EPSILON);

        let ballot = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::GroupProposalBallot(req) => Some(req),
                _ => None,
            })
            .ok_or("expected a GroupProposalBallot")?;
        assert_eq!(ballot.proposal_data.proposal_id, proposal_id);
        assert_eq!(ballot.proposal_data.group_id, group_id);
        Ok(())
    }

    #[test]
    fn group_finance_replies_surface_events() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        let group_id = uuid::Uuid::from_u128(0x6A0D);
        let request_id = uuid::Uuid::from_u128(0xF00D);
        let summary = AnyMessage::GroupAccountSummaryReply(GroupAccountSummaryReply {
            agent_data: GroupAccountSummaryReplyAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(0xA6E),
                group_id,
            },
            money_data: GroupAccountSummaryReplyMoneyDataBlock {
                request_id,
                interval_days: 7,
                current_interval: 0,
                start_date: b"2026-06-01\0".to_vec(),
                balance: 1234,
                total_credits: 50,
                total_debits: 20,
                object_tax_current: 1,
                light_tax_current: 2,
                land_tax_current: 3,
                group_tax_current: 4,
                parcel_dir_fee_current: 5,
                object_tax_estimate: 6,
                light_tax_estimate: 7,
                land_tax_estimate: 8,
                group_tax_estimate: 9,
                parcel_dir_fee_estimate: 10,
                non_exempt_members: 11,
                last_tax_date: b"2026-05-25\0".to_vec(),
                tax_date: b"2026-06-08\0".to_vec(),
            },
        });
        session.handle_datagram(sim_addr(), &server_message(&summary, 20, true)?, now)?;

        let got = drain_events(&mut session)
            .into_iter()
            .find_map(|e| match e {
                Event::GroupAccountSummary(summary) => Some(summary),
                _ => None,
            })
            .ok_or("expected a GroupAccountSummary event")?;
        assert_eq!(got.group_id, GroupKey::from(group_id));
        assert_eq!(got.request_id, request_id);
        assert_eq!(got.balance, LindenBalance::from_i32(1234));
        assert_eq!(got.start_date, "2026-06-01");
        assert_eq!(got.non_exempt_members, 11);

        let transaction_id = uuid::Uuid::from_u128(0x7AC7);
        let vote_id = uuid::Uuid::from_u128(0x9A0E);
        let proposals = AnyMessage::GroupActiveProposalItemReply(GroupActiveProposalItemReply {
            agent_data: GroupActiveProposalItemReplyAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(0xA6E),
                group_id,
            },
            transaction_data: GroupActiveProposalItemReplyTransactionDataBlock {
                transaction_id,
                total_num_items: 1,
            },
            proposal_data: vec![GroupActiveProposalItemReplyProposalDataBlock {
                vote_id,
                vote_initiator: uuid::Uuid::from_u128(0x1217),
                terse_date_id: b"td\0".to_vec(),
                start_date_time: b"2026-06-01\0".to_vec(),
                end_date_time: b"2026-06-08\0".to_vec(),
                already_voted: false,
                vote_cast: b"\0".to_vec(),
                majority: 0.5,
                quorum: 3,
                proposal_text: b"Adopt the bylaws?\0".to_vec(),
            }],
        });
        session.handle_datagram(sim_addr(), &server_message(&proposals, 21, true)?, now)?;

        let active = drain_events(&mut session)
            .into_iter()
            .find_map(|e| match e {
                Event::GroupActiveProposals {
                    group_id,
                    transaction_id,
                    total_num_items,
                    proposals,
                } => Some((group_id, transaction_id, total_num_items, proposals)),
                _ => None,
            })
            .ok_or("expected a GroupActiveProposals event")?;
        assert_eq!(active.0, GroupKey::from(group_id));
        assert_eq!(active.1, transaction_id);
        assert_eq!(active.2, 1);
        let proposal = active.3.first().ok_or("expected one proposal")?;
        assert_eq!(proposal.vote_id.uuid(), vote_id);
        assert_eq!(proposal.quorum, 3);
        assert_eq!(proposal.proposal_text, "Adopt the bylaws?");
        assert!((proposal.majority - 0.5).abs() < f32::EPSILON);
        Ok(())
    }

    #[test]
    fn pay_price_reply_surfaces_buttons() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let object = uuid::Uuid::from_u128(0xB0B);
        let message = AnyMessage::PayPriceReply(PayPriceReply {
            object_data: PayPriceReplyObjectDataBlock {
                object_id: object,
                default_pay_price: 10,
            },
            button_data: vec![
                PayPriceReplyButtonDataBlock { pay_button: 1 },
                PayPriceReplyButtonDataBlock { pay_button: 5 },
                PayPriceReplyButtonDataBlock { pay_button: 20 },
            ],
        });
        session.handle_datagram(sim_addr(), &server_message(&message, 9, true)?, now)?;

        let (object_id, default_pay_price, pay_buttons) = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::PayPriceReply {
                    object_id,
                    default_pay_price,
                    pay_buttons,
                } => Some((object_id, default_pay_price, pay_buttons)),
                _ => None,
            })
            .ok_or("expected a PayPriceReply event")?;
        assert_eq!(object_id, ObjectKey::from(object));
        assert_eq!(default_pay_price, 10);
        assert_eq!(pay_buttons, vec![1, 5, 20]);
        Ok(())
    }

    #[test]
    fn object_properties_family_surfaces() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let object = uuid::Uuid::from_u128(0xB0B);
        let owner = uuid::Uuid::from_u128(0x0E);
        let message = AnyMessage::ObjectPropertiesFamily(ObjectPropertiesFamilyMessage {
            object_data: ObjectPropertiesFamilyObjectDataBlock {
                request_flags: 0x04,
                object_id: object,
                owner_id: owner,
                group_id: uuid::Uuid::nil(),
                base_mask: 0x0008_e000,
                owner_mask: 0x0008_e000,
                group_mask: 0,
                everyone_mask: 0,
                next_owner_mask: 0x0008_e000,
                ownership_cost: 0,
                sale_type: SaleType::Copy.to_code(),
                sale_price: 250,
                category: 0,
                last_owner_id: uuid::Uuid::nil(),
                name: b"Vendor\0".to_vec(),
                description: b"A vendor\0".to_vec(),
            },
        });
        session.handle_datagram(sim_addr(), &server_message(&message, 9, true)?, now)?;

        let properties = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::ObjectPropertiesFamily { properties } => Some(properties),
                _ => None,
            })
            .ok_or("expected an ObjectPropertiesFamily event")?;
        assert_eq!(properties.request_flags, 0x04);
        assert_eq!(properties.object_id, ObjectKey::from(object));
        assert_eq!(
            properties.owner,
            sl_proto::OwnerKey::Agent(sl_proto::AgentKey::from(owner))
        );
        assert_eq!(properties.group, None);
        assert_eq!(properties.sale_type, SaleType::Copy.to_code());
        assert_eq!(properties.sale_price, Some(LindenAmount(250)));
        assert_eq!(properties.name, "Vendor");
        assert_eq!(properties.description, "A vendor");
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
            // A single opaque PhysicalAvatarEventList block: the simulator
            // assigns no documented structure to TypeData, so we surface the
            // raw bytes verbatim.
            physical_avatar_event_list: vec![AvatarAnimationPhysicalAvatarEventListBlock {
                type_data: vec![0xDE, 0xAD, 0xBE, 0xEF],
            }],
        });
        session.handle_datagram(sim_addr(), &server_message(&message, 9, true)?, now)?;

        let (avatar_id, animations, physical_events) = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::AvatarAnimation {
                    avatar_id,
                    animations,
                    physical_events,
                } => Some((avatar_id, animations, physical_events)),
                _ => None,
            })
            .ok_or("expected an AvatarAnimation event")?;
        assert_eq!(avatar_id, AgentKey::from(avatar));
        assert_eq!(animations.len(), 2);
        assert_eq!(physical_events, vec![vec![0xDE, 0xAD, 0xBE, 0xEF]]);
        let first = animations.first().ok_or("first animation")?;
        assert_eq!(first.anim_id, walk);
        assert_eq!(first.sequence_id, 1);
        // A nil source UUID is still a populated source slot; only a *missing*
        // slot decodes to `None`. The viewer treats nil as "no triggering object".
        assert_eq!(first.source_id, Some(ObjectKey::from(uuid::Uuid::nil())));
        let second = animations.get(1).ok_or("second animation")?;
        assert_eq!(second.anim_id, scripted);
        assert_eq!(second.sequence_id, 2);
        assert_eq!(second.source_id, Some(ObjectKey::from(trigger_object)));
        Ok(())
    }

    #[test]
    fn object_animation_surfaces_signalled_animations() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let object = uuid::Uuid::from_u128(0xB1);
        let dance = uuid::Uuid::from_u128(0x400);
        let wave = uuid::Uuid::from_u128(0x401);

        let message = AnyMessage::ObjectAnimation(ObjectAnimation {
            sender: ObjectAnimationSenderBlock { id: object },
            animation_list: vec![
                ObjectAnimationAnimationListBlock {
                    anim_id: dance,
                    anim_sequence_id: 3,
                },
                ObjectAnimationAnimationListBlock {
                    anim_id: wave,
                    anim_sequence_id: 4,
                },
            ],
        });
        session.handle_datagram(sim_addr(), &server_message(&message, 9, true)?, now)?;

        let (object_id, animations) = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::ObjectAnimation {
                    object_id,
                    animations,
                } => Some((object_id, animations)),
                _ => None,
            })
            .ok_or("expected an ObjectAnimation event")?;
        assert_eq!(object_id, ObjectKey::from(object));
        assert_eq!(animations.len(), 2);
        let first = animations.first().ok_or("first animation")?;
        assert_eq!(first.anim_id, AnimationKey::from(dance));
        assert_eq!(first.sequence_id, 3);
        let second = animations.get(1).ok_or("second animation")?;
        assert_eq!(second.anim_id, AnimationKey::from(wave));
        assert_eq!(second.sequence_id, 4);
        Ok(())
    }

    #[test]
    fn rebake_avatar_textures_surfaces_texture_id() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let baked = uuid::Uuid::from_u128(0xBA4E);
        let message = AnyMessage::RebakeAvatarTextures(RebakeAvatarTextures {
            texture_data: RebakeAvatarTexturesTextureDataBlock { texture_id: baked },
        });
        session.handle_datagram(sim_addr(), &server_message(&message, 9, true)?, now)?;

        let texture_id = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::RebakeAvatarTextures { texture_id } => Some(texture_id),
                _ => None,
            })
            .ok_or("expected a RebakeAvatarTextures event")?;
        assert_eq!(texture_id, TextureKey::from(baked));
        Ok(())
    }

    #[test]
    fn terminate_friendship_surfaces_former_friend() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let other = uuid::Uuid::from_u128(0xF21E);
        let message = AnyMessage::TerminateFriendship(TerminateFriendship {
            agent_data: TerminateFriendshipAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(0x1),
                session_id: uuid::Uuid::from_u128(0x2),
            },
            ex_block: TerminateFriendshipExBlockBlock { other_id: other },
        });
        session.handle_datagram(sim_addr(), &server_message(&message, 9, true)?, now)?;

        let former = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::FriendshipTerminated { other } => Some(other),
                _ => None,
            })
            .ok_or("expected a FriendshipTerminated event")?;
        assert_eq!(former, FriendKey::from(other));
        Ok(())
    }

    #[test]
    fn offer_calling_card_surfaces_offering_agent() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let offerer = uuid::Uuid::from_u128(0xCA11);
        let transaction = uuid::Uuid::from_u128(0x7AC);
        let message = AnyMessage::OfferCallingCard(OfferCallingCard {
            agent_data: OfferCallingCardAgentDataBlock {
                agent_id: offerer,
                session_id: uuid::Uuid::from_u128(0x2),
            },
            agent_block: OfferCallingCardAgentBlockBlock {
                dest_id: uuid::Uuid::from_u128(0x3),
                transaction_id: transaction,
            },
        });
        session.handle_datagram(sim_addr(), &server_message(&message, 9, true)?, now)?;

        let (offering_agent, tx) = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::CallingCardOffered {
                    offering_agent,
                    transaction,
                } => Some((offering_agent, transaction)),
                _ => None,
            })
            .ok_or("expected a CallingCardOffered event")?;
        assert_eq!(offering_agent, AgentKey::from(offerer));
        assert_eq!(tx, TransactionId::from(transaction));
        Ok(())
    }

    #[test]
    fn accept_calling_card_surfaces_accepter() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let accepter = uuid::Uuid::from_u128(0xACC7);
        let transaction = uuid::Uuid::from_u128(0x7AC);
        let message = AnyMessage::AcceptCallingCard(AcceptCallingCard {
            agent_data: AcceptCallingCardAgentDataBlock {
                agent_id: accepter,
                session_id: uuid::Uuid::from_u128(0x2),
            },
            transaction_block: AcceptCallingCardTransactionBlockBlock {
                transaction_id: transaction,
            },
            folder_data: vec![AcceptCallingCardFolderDataBlock {
                folder_id: uuid::Uuid::from_u128(0x4),
            }],
        });
        session.handle_datagram(sim_addr(), &server_message(&message, 9, true)?, now)?;

        let (agent, tx) = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::CallingCardAccepted { agent, transaction } => Some((agent, transaction)),
                _ => None,
            })
            .ok_or("expected a CallingCardAccepted event")?;
        assert_eq!(agent, AgentKey::from(accepter));
        assert_eq!(tx, TransactionId::from(transaction));
        Ok(())
    }

    #[test]
    fn decline_calling_card_surfaces_decliner() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let decliner = uuid::Uuid::from_u128(0xDEC7);
        let transaction = uuid::Uuid::from_u128(0x7AC);
        let message = AnyMessage::DeclineCallingCard(DeclineCallingCard {
            agent_data: DeclineCallingCardAgentDataBlock {
                agent_id: decliner,
                session_id: uuid::Uuid::from_u128(0x2),
            },
            transaction_block: DeclineCallingCardTransactionBlockBlock {
                transaction_id: transaction,
            },
        });
        session.handle_datagram(sim_addr(), &server_message(&message, 9, true)?, now)?;

        let (agent, tx) = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::CallingCardDeclined { agent, transaction } => Some((agent, transaction)),
                _ => None,
            })
            .ok_or("expected a CallingCardDeclined event")?;
        assert_eq!(agent, AgentKey::from(decliner));
        assert_eq!(tx, TransactionId::from(transaction));
        Ok(())
    }

    #[test]
    fn remove_inventory_item_surfaces_removed_items() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let item_a = uuid::Uuid::from_u128(0x17EA);
        let item_b = uuid::Uuid::from_u128(0x17EB);
        let message = AnyMessage::RemoveInventoryItem(RemoveInventoryItem {
            agent_data: RemoveInventoryItemAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(0x1),
                session_id: uuid::Uuid::from_u128(0x2),
            },
            inventory_data: vec![
                RemoveInventoryItemInventoryDataBlock { item_id: item_a },
                RemoveInventoryItemInventoryDataBlock { item_id: item_b },
            ],
        });
        session.handle_datagram(sim_addr(), &server_message(&message, 9, true)?, now)?;

        let items = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::InventoryItemsRemoved { items } => Some(items),
                _ => None,
            })
            .ok_or("expected an InventoryItemsRemoved event")?;
        assert_eq!(
            items,
            vec![InventoryKey::from(item_a), InventoryKey::from(item_b)]
        );
        Ok(())
    }

    #[test]
    fn remove_inventory_folder_surfaces_removed_folders() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let folder = uuid::Uuid::from_u128(0xF01DE);
        let message = AnyMessage::RemoveInventoryFolder(RemoveInventoryFolder {
            agent_data: RemoveInventoryFolderAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(0x1),
                session_id: uuid::Uuid::from_u128(0x2),
            },
            folder_data: vec![RemoveInventoryFolderFolderDataBlock { folder_id: folder }],
        });
        session.handle_datagram(sim_addr(), &server_message(&message, 9, true)?, now)?;

        let folders = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::InventoryFoldersRemoved { folders } => Some(folders),
                _ => None,
            })
            .ok_or("expected an InventoryFoldersRemoved event")?;
        assert_eq!(folders, vec![InventoryFolderKey::from(folder)]);
        Ok(())
    }

    #[test]
    fn remove_inventory_objects_surfaces_folders_and_items() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let folder = uuid::Uuid::from_u128(0xF01DE);
        let item = uuid::Uuid::from_u128(0x17E);
        let message = AnyMessage::RemoveInventoryObjects(RemoveInventoryObjects {
            agent_data: RemoveInventoryObjectsAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(0x1),
                session_id: uuid::Uuid::from_u128(0x2),
            },
            folder_data: vec![RemoveInventoryObjectsFolderDataBlock { folder_id: folder }],
            item_data: vec![RemoveInventoryObjectsItemDataBlock { item_id: item }],
        });
        session.handle_datagram(sim_addr(), &server_message(&message, 9, true)?, now)?;

        let (folders, items) = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::InventoryObjectsRemoved { folders, items } => Some((folders, items)),
                _ => None,
            })
            .ok_or("expected an InventoryObjectsRemoved event")?;
        assert_eq!(folders, vec![InventoryFolderKey::from(folder)]);
        assert_eq!(items, vec![InventoryKey::from(item)]);
        Ok(())
    }

    #[test]
    fn move_inventory_item_surfaces_moves_and_rename() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let item_named = uuid::Uuid::from_u128(0x17E1);
        let item_unnamed = uuid::Uuid::from_u128(0x17E2);
        let folder = uuid::Uuid::from_u128(0xF01DE);
        let message = AnyMessage::MoveInventoryItem(MoveInventoryItem {
            agent_data: MoveInventoryItemAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(0x1),
                session_id: uuid::Uuid::from_u128(0x2),
                stamp: true,
            },
            inventory_data: vec![
                MoveInventoryItemInventoryDataBlock {
                    item_id: item_named,
                    folder_id: folder,
                    new_name: b"Renamed\0".to_vec(),
                },
                MoveInventoryItemInventoryDataBlock {
                    item_id: item_unnamed,
                    folder_id: folder,
                    new_name: Vec::new(),
                },
            ],
        });
        session.handle_datagram(sim_addr(), &server_message(&message, 9, true)?, now)?;

        let (stamp, moves) = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::InventoryItemsMoved { stamp, moves } => Some((stamp, moves)),
                _ => None,
            })
            .ok_or("expected an InventoryItemsMoved event")?;
        assert!(stamp);
        assert_eq!(
            moves,
            vec![
                InventoryItemMove {
                    item: InventoryKey::from(item_named),
                    folder: InventoryFolderKey::from(folder),
                    new_name: Some("Renamed".to_owned()),
                },
                InventoryItemMove {
                    item: InventoryKey::from(item_unnamed),
                    folder: InventoryFolderKey::from(folder),
                    new_name: None,
                },
            ]
        );
        Ok(())
    }

    #[test]
    fn reply_task_inventory_surfaces_serial_and_filename() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let task = uuid::Uuid::from_u128(0x7A5C);
        let message = AnyMessage::ReplyTaskInventory(ReplyTaskInventory {
            inventory_data: ReplyTaskInventoryInventoryDataBlock {
                task_id: task,
                serial: 7,
                filename: b"inventory.tmp\0".to_vec(),
            },
        });
        session.handle_datagram(sim_addr(), &server_message(&message, 9, true)?, now)?;

        let reply = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::TaskInventoryReply(reply) => Some(reply),
                _ => None,
            })
            .ok_or("expected a TaskInventoryReply event")?;
        assert_eq!(
            reply,
            TaskInventoryReply {
                task: ObjectKey::from(task),
                serial: 7,
                filename: "inventory.tmp".to_owned(),
            }
        );
        Ok(())
    }

    #[test]
    fn user_info_reply_surfaces_contact_prefs() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let message = AnyMessage::UserInfoReply(UserInfoReply {
            agent_data: UserInfoReplyAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(0x1),
            },
            user_data: UserInfoReplyUserDataBlock {
                im_via_e_mail: true,
                directory_visibility: b"default\0".to_vec(),
                e_mail: b"resident@example.com\0".to_vec(),
            },
        });
        session.handle_datagram(sim_addr(), &server_message(&message, 9, true)?, now)?;

        let info = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::UserInfo(info) => Some(info),
                _ => None,
            })
            .ok_or("expected a UserInfo event")?;
        assert_eq!(
            info,
            UserInfo {
                im_via_email: true,
                directory_visibility: DirectoryVisibility::Default,
                email: "resident@example.com".to_owned(),
            }
        );
        Ok(())
    }

    #[test]
    fn derez_ack_surfaces_transaction_and_success() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let transaction = uuid::Uuid::from_u128(0xDE_E2);
        let message = AnyMessage::DeRezAck(DeRezAck {
            transaction_data: DeRezAckTransactionDataBlock {
                transaction_id: transaction,
                success: true,
            },
        });
        session.handle_datagram(sim_addr(), &server_message(&message, 9, true)?, now)?;

        let (tx, success) = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::DeRezAck {
                    transaction,
                    success,
                } => Some((transaction, success)),
                _ => None,
            })
            .ok_or("expected a DeRezAck event")?;
        assert_eq!(tx, TransactionId::from(transaction));
        assert!(success);
        Ok(())
    }

    #[test]
    fn force_object_select_surfaces_scoped_objects() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        let circuit = session.root_circuit_id().ok_or("no circuit")?;
        drain(&mut session)?;

        let message = AnyMessage::ForceObjectSelect(ForceObjectSelect {
            header: ForceObjectSelectHeaderBlock { reset_list: true },
            data: vec![
                ForceObjectSelectDataBlock { local_id: 42 },
                ForceObjectSelectDataBlock { local_id: 43 },
            ],
        });
        session.handle_datagram(sim_addr(), &server_message(&message, 9, true)?, now)?;

        let (reset_list, objects) = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::ForceObjectSelect {
                    reset_list,
                    objects,
                } => Some((reset_list, objects)),
                _ => None,
            })
            .ok_or("expected a ForceObjectSelect event")?;
        assert!(reset_list);
        assert_eq!(
            objects,
            vec![
                ScopedObjectId::new(circuit, sl_proto::RegionLocalObjectId(42)),
                ScopedObjectId::new(circuit, sl_proto::RegionLocalObjectId(43)),
            ]
        );
        Ok(())
    }

    #[test]
    fn grant_godlike_powers_surfaces_level() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let message = AnyMessage::GrantGodlikePowers(GrantGodlikePowers {
            agent_data: GrantGodlikePowersAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(0x1),
                session_id: uuid::Uuid::from_u128(0x2),
            },
            grant_data: GrantGodlikePowersGrantDataBlock {
                god_level: 200,
                token: uuid::Uuid::from_u128(0x70CE),
            },
        });
        session.handle_datagram(sim_addr(), &server_message(&message, 9, true)?, now)?;

        let god_level = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::GodlikePowersGranted { god_level } => Some(god_level),
                _ => None,
            })
            .ok_or("expected a GodlikePowersGranted event")?;
        assert_eq!(god_level, 200);
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
        assert_eq!(object_id, ObjectKey::from(object));
        assert_eq!(parent_id, None);
        assert_eq!(region_handle, RegionHandle(0x0000_03E8_0000_03E8));
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
        assert_eq!(local_id.id, sl_proto::RegionLocalObjectId(7));
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
        assert_eq!(
            update.media_url.as_ref().map(url::Url::as_str),
            Some("http://example.com/movie")
        );
        assert_eq!(update.media_id, Some(TextureKey::from(media)));
        assert!(update.media_auto_scale);
        assert_eq!(update.media_type, "text/html");
        assert_eq!(update.media_desc, "a web page");
        assert_eq!(update.media_width, Some(1024));
        assert_eq!(update.media_height, Some(768));
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
        assert_eq!(object_id, ObjectKey::from(object));
        assert_eq!(version, format!("x-mv:0000000003/{object}"));
        assert_eq!(faces.len(), 2);
        let face0 = faces
            .first()
            .ok_or("face 0")?
            .as_ref()
            .ok_or("face 0 media")?;
        assert_eq!(
            face0.current_url.as_ref().map(url::Url::as_str),
            Some("http://example.com/stream")
        );
        assert_eq!(
            face0.home_url.as_ref().map(url::Url::as_str),
            Some("http://example.com/home")
        );
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
        assert_eq!(object_id, ObjectKey::from(object));
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
        assert_eq!(
            first.object_id,
            ObjectKey::from(uuid::Uuid::from_u128(0x71))
        );
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
        session.set_animations(
            &[
                (AnimationKey::from(start), true),
                (AnimationKey::from(stop), false),
            ],
            now,
        )?;
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
        session.play_animation(AnimationKey::from(dance), now)?;
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
        let mut session = new_session()?;
        let root = InventoryFolderKey::from(uuid::Uuid::from_u128(0xF0));
        let login = LoginResponse::Success(Box::new(LoginSuccess {
            agent_id: AgentKey::from(uuid::Uuid::from_u128(1)),
            session_id: uuid::Uuid::from_u128(2),
            secure_session_id: uuid::Uuid::from_u128(3),
            circuit_code: CircuitCode(0x0011_2233),
            sim_ip: Ipv4Addr::new(127, 0, 0, 1),
            sim_port: 9000,
            seed_capability: "http://127.0.0.1:9000/seed".parse()?,
            message: None,
            mfa_hash: None,
            inventory_root: Some(root),
            inventory_skeleton: vec![
                SkeletonFolder {
                    folder_id: root,
                    parent_id: InventoryFolderKey::from(uuid::Uuid::nil()),
                    name: "My Inventory".to_owned(),
                    type_default: 8,
                    version: 5,
                },
                SkeletonFolder {
                    folder_id: InventoryFolderKey::from(uuid::Uuid::from_u128(0xF1)),
                    parent_id: root,
                    name: "Objects".to_owned(),
                    type_default: 6,
                    version: 2,
                },
            ],
            buddy_list: Vec::new(),
            home: None,
            look_at: None,
            region_x: None,
            region_y: None,
            agent_access: None,
            agent_access_max: None,
            max_agent_groups: None,
            library_root: None,
            library_owner: None,
            library_skeleton: Vec::new(),
        }));
        session.handle_login_response(login, now)?;

        assert_eq!(session.inventory_root(), Some(root));
        // The skeleton seeds the held model: both folders land in the agent tree
        // `Unknown` (contents unfetched) and the parent→children index links the
        // sub-folder under the root.
        let objects = InventoryFolderKey::from(uuid::Uuid::from_u128(0xF1));
        assert_eq!(session.folder_fetch_state(root), Some(FolderState::Unknown));
        assert_eq!(
            session.folder_fetch_state(objects),
            Some(FolderState::Unknown)
        );
        assert_eq!(session.inventory_owner(root), Some(InventoryOwner::Agent));
        let (child_folders, child_items) = split_children(&session, root);
        assert_eq!(child_folders.len(), 1);
        assert_eq!(
            child_folders.first().ok_or("child folder")?.folder_id,
            objects
        );
        assert!(child_items.is_empty());

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
        assert_eq!(folders.get(1).ok_or("second folder")?.parent_id, Some(root));
        Ok(())
    }

    #[test]
    fn login_emits_account_and_library_and_stores_them() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = new_session()?;
        let lib_root = InventoryFolderKey::from(uuid::Uuid::from_u128(0x0112));
        let lib_owner = AgentKey::from(uuid::Uuid::from_u128(0xAB));
        let login = LoginResponse::Success(Box::new(LoginSuccess {
            agent_id: AgentKey::from(uuid::Uuid::from_u128(1)),
            session_id: uuid::Uuid::from_u128(2),
            secure_session_id: uuid::Uuid::from_u128(3),
            circuit_code: CircuitCode(0x0011_2233),
            sim_ip: Ipv4Addr::new(127, 0, 0, 1),
            sim_port: 9000,
            seed_capability: "http://127.0.0.1:9000/seed".parse()?,
            message: None,
            mfa_hash: None,
            inventory_root: None,
            inventory_skeleton: Vec::new(),
            buddy_list: Vec::new(),
            home: Some(HomeLocation {
                region_handle: RegionHandle::from_global(256_000, 256_256),
                position: RegionCoordinates::new(128.0, 127.0, 25.0),
                look_at: Direction::new(1.0, 0.0, 0.0),
            }),
            look_at: Some(Direction::new(0.5, 0.5, 0.0)),
            region_x: Some(256_000),
            region_y: Some(256_256),
            agent_access: Some("M".to_owned()),
            agent_access_max: Some("A".to_owned()),
            max_agent_groups: Some(42),
            library_root: Some(lib_root),
            library_owner: Some(lib_owner),
            library_skeleton: vec![SkeletonFolder {
                folder_id: lib_root,
                parent_id: InventoryFolderKey::from(uuid::Uuid::nil()),
                name: "Library".to_owned(),
                type_default: 8,
                version: 1,
            }],
        }));
        session.handle_login_response(login, now)?;

        // The account facts are stored on the session...
        let stored: &LoginAccount = session.login_account().ok_or("expected login account")?;
        assert_eq!(stored.agent_access, Maturity::Mature);
        assert_eq!(stored.agent_access_max, Maturity::Adult);
        assert_eq!(stored.max_agent_groups, Some(42));
        assert_eq!(stored.library_root, Some(lib_root));
        assert_eq!(stored.library_owner, Some(lib_owner));
        assert_eq!(
            stored.home.ok_or("home")?.region_handle,
            RegionHandle::from_global(256_000, 256_256)
        );

        // ...and also emitted as events.
        let events = drain_events(&mut session);
        let account = events
            .iter()
            .find_map(|event| match event {
                Event::Account(account) => Some(account),
                _ => None,
            })
            .ok_or("expected an Account event")?;
        assert_eq!(account.max_agent_groups, Some(42));
        let library = events
            .iter()
            .find_map(|event| match event {
                Event::LibraryInventory(folders) => Some(folders),
                _ => None,
            })
            .ok_or("expected a LibraryInventory event")?;
        assert_eq!(library.len(), 1);
        assert_eq!(library.first().ok_or("library root")?.name, "Library");

        // The Library skeleton is also folded into the held model under its own
        // root and owner, queryable apart from the agent tree (B7). The agent root
        // is absent here, so the Library tree stands entirely on its own.
        assert_eq!(session.library_root(), Some(lib_root));
        assert_eq!(session.library_owner(), Some(OwnerKey::Agent(lib_owner)));
        assert_eq!(session.inventory_root(), None);
        assert_eq!(
            session.inventory_owner(lib_root),
            Some(InventoryOwner::Library)
        );
        assert_eq!(
            session.folder_fetch_state(lib_root),
            Some(FolderState::Unknown)
        );
        Ok(())
    }

    /// B7: the shared Library tree is held apart from the agent tree, fetched
    /// with the **Library owner** id (not the agent), and a descendents reply for
    /// a Library folder folds back under [`InventoryOwner::Library`]; the
    /// fully-fetched Library tree round-trips through its own owner-keyed cache.
    #[test]
    fn library_inventory_holds_fetches_and_caches_apart() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = new_session()?;
        let agent_root = InventoryFolderKey::from(uuid::Uuid::from_u128(0x0A00));
        let lib_root = InventoryFolderKey::from(uuid::Uuid::from_u128(0x0B00));
        let lib_owner = AgentKey::from(uuid::Uuid::from_u128(0xAB));
        let login = LoginResponse::Success(Box::new(LoginSuccess {
            agent_id: AgentKey::from(uuid::Uuid::from_u128(1)),
            session_id: uuid::Uuid::from_u128(2),
            secure_session_id: uuid::Uuid::from_u128(3),
            circuit_code: CircuitCode(0x0011_2233),
            sim_ip: Ipv4Addr::new(127, 0, 0, 1),
            sim_port: 9000,
            seed_capability: "http://127.0.0.1:9000/seed".parse()?,
            message: None,
            mfa_hash: None,
            inventory_root: Some(agent_root),
            inventory_skeleton: vec![SkeletonFolder {
                folder_id: agent_root,
                parent_id: InventoryFolderKey::from(uuid::Uuid::nil()),
                name: "My Inventory".to_owned(),
                type_default: 8,
                version: 1,
            }],
            buddy_list: Vec::new(),
            home: None,
            look_at: None,
            region_x: None,
            region_y: None,
            agent_access: None,
            agent_access_max: None,
            max_agent_groups: None,
            library_root: Some(lib_root),
            library_owner: Some(lib_owner),
            library_skeleton: vec![SkeletonFolder {
                folder_id: lib_root,
                parent_id: InventoryFolderKey::from(uuid::Uuid::nil()),
                name: "Library".to_owned(),
                type_default: 8,
                version: 3,
            }],
        }));
        session.handle_login_response(login, now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        // The two roots are distinct and each lands under its own owner.
        assert_ne!(session.inventory_root(), session.library_root());
        assert_eq!(
            session.inventory_owner(agent_root),
            Some(InventoryOwner::Agent)
        );
        assert_eq!(
            session.inventory_owner(lib_root),
            Some(InventoryOwner::Library)
        );
        assert_eq!(session.library_owner(), Some(OwnerKey::Agent(lib_owner)));

        // An on-demand fetch of the Library root is addressed to the Library
        // owner, not the agent id — the one wire difference that routes the
        // read-only tree — and flips the folder `Fetching`.
        session.request_folder_contents(lib_root, now)?;
        let sent = drain(&mut session)?;
        let fetch = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::FetchInventoryDescendents(fetch) => Some(fetch),
                _ => None,
            })
            .ok_or("expected a FetchInventoryDescendents")?;
        assert_eq!(fetch.inventory_data.folder_id, lib_root.uuid());
        assert_eq!(fetch.inventory_data.owner_id, lib_owner.uuid());
        assert_eq!(
            session.folder_fetch_state(lib_root),
            Some(FolderState::Fetching)
        );

        // The descendents reply folds its contents under the Library owner and
        // marks the root `Loaded`; the named sub-folder is held in the Library
        // tree, not the agent tree.
        let lib_child = uuid::Uuid::from_u128(0x0B01);
        let reply = AnyMessage::InventoryDescendents(InventoryDescendents {
            agent_data: InventoryDescendentsAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(1),
                folder_id: lib_root.uuid(),
                owner_id: lib_owner.uuid(),
                version: 3,
                descendents: 1,
            },
            folder_data: vec![InventoryDescendentsFolderDataBlock {
                folder_id: lib_child,
                parent_id: lib_root.uuid(),
                r#type: 6,
                name: b"Animations\0".to_vec(),
            }],
            item_data: Vec::new(),
        });
        let datagram = server_message(&reply, 9, false)?;
        session.handle_datagram(sim_addr(), &datagram, now)?;
        drain_events(&mut session);

        assert_eq!(
            session.folder_fetch_state(lib_root),
            Some(FolderState::Loaded { version: 3 })
        );
        assert_eq!(
            session.inventory_owner(InventoryFolderKey::from(lib_child)),
            Some(InventoryOwner::Library)
        );
        let (lib_children, _) = split_children(&session, lib_root);
        assert_eq!(lib_children.len(), 1);
        assert_eq!(
            lib_children.first().ok_or("library child")?.folder_id,
            InventoryFolderKey::from(lib_child)
        );

        // The fully-fetched Library tree round-trips through its own owner-keyed
        // cache, separate from the agent tree, restoring the root `Loaded`.
        let bytes = session.inventory_cache_bytes(InventoryOwner::Library)?;
        let mut restored = new_session()?;
        assert!(restored.load_inventory_cache(InventoryOwner::Library, &bytes)?);
        assert_eq!(
            restored.inventory_owner(lib_root),
            Some(InventoryOwner::Library)
        );
        assert_eq!(
            restored.folder_fetch_state(lib_root),
            Some(FolderState::Loaded { version: 3 })
        );
        Ok(())
    }

    #[test]
    fn request_folder_contents_packs_fetch() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let folder = uuid::Uuid::from_u128(0xF0);
        session.request_folder_contents(InventoryFolderKey::from(folder), now)?;
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
                } if folder_id == InventoryFolderKey::from(folder) => Some((folders, items)),
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

        // The descendents reply flips the fetched folder to `Loaded` at the
        // reply's authoritative version and fills the index with its children;
        // the sub-folder it names stays `Unknown` (its own contents unfetched).
        let folder_key = InventoryFolderKey::from(folder);
        let sub = InventoryFolderKey::from(uuid::Uuid::from_u128(0xF2));
        assert_eq!(
            session.folder_fetch_state(folder_key),
            Some(FolderState::Loaded { version: 7 })
        );
        assert_eq!(session.folder_fetch_state(sub), Some(FolderState::Unknown));
        let (cached_folders, cached_items) = split_children(&session, folder_key);
        assert_eq!(cached_folders.len(), 1);
        assert_eq!(
            cached_folders.first().ok_or("cached folder")?.folder_id,
            sub
        );
        assert_eq!(cached_items.len(), 1);
        assert_eq!(
            cached_items.first().ok_or("cached item")?.item_id,
            InventoryKey::from(uuid::Uuid::from_u128(0xD1))
        );
        Ok(())
    }

    /// OpenSim emits a single nil-id placeholder `FolderData` block for an empty
    /// folder (an LLUDP "stuffing" quirk a real viewer ignores). That phantom
    /// sub-folder must be filtered out of the descendents reply — otherwise a
    /// crawl would try to fetch the nil folder (which never answers) and the
    /// background scheduler would mark it `Fetching` forever. Surfaced live by the
    /// `library-tree-fetch` conformance case crawling the OpenSim Library tree.
    #[test]
    fn inventory_descendents_drops_nil_placeholder_subfolder() -> Result<(), TestError> {
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
                descendents: 0,
            },
            // The sole block is the nil-id stuffing block an empty folder carries.
            folder_data: vec![InventoryDescendentsFolderDataBlock {
                folder_id: uuid::Uuid::nil(),
                parent_id: folder,
                r#type: -1,
                name: b"\0".to_vec(),
            }],
            item_data: vec![],
        });
        let datagram = server_message(&reply, 9, false)?;
        session.handle_datagram(sim_addr(), &datagram, now)?;

        let folders = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::InventoryDescendents {
                    folder_id, folders, ..
                } if folder_id == InventoryFolderKey::from(folder) => Some(folders),
                _ => None,
            })
            .ok_or("expected an InventoryDescendents event")?;
        // The phantom nil folder is gone: the empty folder reports no sub-folders.
        assert!(
            folders.is_empty(),
            "nil placeholder sub-folder should be filtered out, got {folders:?}"
        );
        // And the nil folder was never seeded into the model (so the background
        // crawl can never pick it up and stall).
        assert_eq!(
            session.folder_fetch_state(InventoryFolderKey::from(uuid::Uuid::nil())),
            None
        );
        Ok(())
    }

    /// B11: the cache-merge relogin path. A fully-`Loaded` agent tree round-trips
    /// through the on-disk cache bytes; reconciling the restored tree against the
    /// login skeleton on relogin refetches *only* the folders whose version moved
    /// (or that the skeleton dropped). A version-matching folder keeps its cached
    /// contents and is absent from the refetch queue, so its descendents are never
    /// re-requested — the whole point of the disk cache.
    #[test]
    fn relogin_merge_skips_version_matching_folders() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = new_session()?;
        let unchanged = 0xE0;
        let bumped = 0xE1;
        let unchanged_key = InventoryFolderKey::from(uuid::Uuid::from_u128(unchanged));
        let bumped_key = InventoryFolderKey::from(uuid::Uuid::from_u128(bumped));

        // Log in with a skeleton listing the two folders as top-level roots
        // (parent nil) at versions 5 and 7, so the cached tree carries no phantom
        // parent to confuse the merge. They seed `Unknown` (metadata only).
        let nil = InventoryFolderKey::from(uuid::Uuid::nil());
        let login = LoginResponse::Success(Box::new(LoginSuccess {
            agent_id: AgentKey::from(uuid::Uuid::from_u128(1)),
            session_id: uuid::Uuid::from_u128(2),
            secure_session_id: uuid::Uuid::from_u128(3),
            circuit_code: CircuitCode(0x0011_2233),
            sim_ip: Ipv4Addr::new(127, 0, 0, 1),
            sim_port: 9000,
            seed_capability: "http://127.0.0.1:9000/seed".parse()?,
            message: None,
            mfa_hash: None,
            inventory_root: Some(unchanged_key),
            inventory_skeleton: vec![
                SkeletonFolder {
                    folder_id: unchanged_key,
                    parent_id: nil,
                    name: "Kept".to_owned(),
                    type_default: 8,
                    version: 5,
                },
                SkeletonFolder {
                    folder_id: bumped_key,
                    parent_id: nil,
                    name: "Bumped".to_owned(),
                    type_default: 8,
                    version: 7,
                },
            ],
            buddy_list: Vec::new(),
            home: None,
            look_at: None,
            region_x: None,
            region_y: None,
            agent_access: None,
            agent_access_max: None,
            max_agent_groups: None,
            library_root: None,
            library_owner: None,
            library_skeleton: Vec::new(),
        }));
        session.handle_login_response(login, now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        // Fold a descendents reply for each, marking it `Loaded` at its skeleton
        // version with one filed item — a fully-fetched cacheable tree.
        feed_descendents(
            &mut session,
            now,
            unchanged,
            5,
            Vec::new(),
            vec![desc_item(0xEE0, unchanged, 7, 7, 0, 0, "kept note")],
            10,
        )?;
        feed_descendents(
            &mut session,
            now,
            bumped,
            7,
            Vec::new(),
            vec![desc_item(0xEE1, bumped, 7, 7, 0, 0, "stale note")],
            11,
        )?;

        // Round-trip the agent tree through the cache bytes into a fresh session,
        // loaded *before* the skeleton (the relogin order). Each `Loaded` folder
        // comes back `Loaded` at its stored version, awaiting the skeleton.
        let bytes = session.inventory_cache_bytes(InventoryOwner::Agent)?;
        let mut relogin = new_session()?;
        assert!(relogin.load_inventory_cache(InventoryOwner::Agent, &bytes)?);
        assert_eq!(
            relogin.folder_fetch_state(unchanged_key),
            Some(FolderState::Loaded { version: 5 })
        );

        // The login skeleton: `unchanged` is still version 5; `bumped` moved to 8.
        let skeleton = vec![
            InventoryFolder {
                folder_id: unchanged_key,
                parent_id: None,
                name: "Kept".to_owned(),
                folder_type: -1,
                version: 5,
            },
            InventoryFolder {
                folder_id: bumped_key,
                parent_id: None,
                name: "Bumped".to_owned(),
                folder_type: -1,
                version: 8,
            },
        ];
        let needing = relogin.merge_inventory_skeleton(InventoryOwner::Agent, &skeleton);

        // Only the version-bumped folder is queued for a refetch; the unchanged
        // one is skipped and keeps its cached `Loaded` contents.
        assert_eq!(needing, vec![bumped_key]);
        assert_eq!(
            relogin.folder_fetch_state(unchanged_key),
            Some(FolderState::Loaded { version: 5 })
        );
        let (_, kept_items) = split_children(&relogin, unchanged_key);
        assert_eq!(kept_items.len(), 1, "the cached item survives the merge");
        assert_eq!(kept_items.first().ok_or("kept item")?.name, "kept note");
        // The version-bumped folder is invalidated: its stale cached contents are
        // dropped and it is back to `Unknown`, to be refetched from the queue.
        assert_eq!(
            relogin.folder_fetch_state(bumped_key),
            Some(FolderState::Unknown)
        );
        let (_, stale_items) = split_children(&relogin, bumped_key);
        assert!(stale_items.is_empty(), "stale cached contents are dropped");
        Ok(())
    }

    /// B6: the background-fetch scheduler is gated off by default (a consumer
    /// that ignores inventory issues no fetches), the explicit on-demand pull
    /// still schedules its one folder while the gate is off, and once enabled the
    /// scheduler sweeps the rest of the `Unknown` tree breadth-first until the
    /// descendents replies fold in and the tree is fully `Loaded`.
    #[test]
    fn background_inventory_fetch_gate_and_drain() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = new_session()?;
        let root = InventoryFolderKey::from(uuid::Uuid::from_u128(0xF0));
        let sub = InventoryFolderKey::from(uuid::Uuid::from_u128(0xF1));
        let login = LoginResponse::Success(Box::new(LoginSuccess {
            agent_id: AgentKey::from(uuid::Uuid::from_u128(1)),
            session_id: uuid::Uuid::from_u128(2),
            secure_session_id: uuid::Uuid::from_u128(3),
            circuit_code: CircuitCode(0x0011_2233),
            sim_ip: Ipv4Addr::new(127, 0, 0, 1),
            sim_port: 9000,
            seed_capability: "http://127.0.0.1:9000/seed".parse()?,
            message: None,
            mfa_hash: None,
            inventory_root: Some(root),
            inventory_skeleton: vec![
                SkeletonFolder {
                    folder_id: root,
                    parent_id: InventoryFolderKey::from(uuid::Uuid::nil()),
                    name: "My Inventory".to_owned(),
                    type_default: 8,
                    version: 5,
                },
                SkeletonFolder {
                    folder_id: sub,
                    parent_id: root,
                    name: "Objects".to_owned(),
                    type_default: 6,
                    version: 2,
                },
            ],
            buddy_list: Vec::new(),
            home: None,
            look_at: None,
            region_x: None,
            region_y: None,
            agent_access: None,
            agent_access_max: None,
            max_agent_groups: None,
            library_root: None,
            library_owner: None,
            library_skeleton: Vec::new(),
        }));
        session.handle_login_response(login, now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        // Both skeleton folders are `Unknown`; the tree is not fully loaded.
        assert_eq!(session.folder_fetch_state(root), Some(FolderState::Unknown));
        assert_eq!(session.folder_fetch_state(sub), Some(FolderState::Unknown));
        assert!(!session.inventory_fully_loaded(InventoryOwner::Agent));

        // Gate off (the default): the scheduler returns nothing and touches no
        // state even though `Unknown` folders are present.
        assert!(!session.background_inventory_fetch());
        assert!(
            session
                .next_inventory_fetch_batch(INVENTORY_FETCH_MAX_IN_FLIGHT)
                .is_empty()
        );
        assert_eq!(session.folder_fetch_state(root), Some(FolderState::Unknown));

        // The explicit on-demand pull still schedules exactly its one folder
        // (gate still off), flipping it `Fetching` and issuing the UDP fetch.
        session.request_folder_contents(sub, now)?;
        let sent = drain(&mut session)?;
        assert!(sent.iter().any(|message| matches!(
            message,
            AnyMessage::FetchInventoryDescendents(fetch)
                if fetch.inventory_data.folder_id == sub.uuid()
        )));
        assert_eq!(session.folder_fetch_state(sub), Some(FolderState::Fetching));
        assert_eq!(session.folder_fetch_state(root), Some(FolderState::Unknown));

        // Enable the crawl: the scheduler sweeps the remaining `Unknown` folder
        // (the in-flight `sub` is skipped) and flips it `Fetching`.
        session.set_background_inventory_fetch(true);
        assert!(session.background_inventory_fetch());
        let batch = session.next_inventory_fetch_batch(INVENTORY_FETCH_MAX_IN_FLIGHT);
        assert_eq!(batch, vec![root]);
        assert_eq!(
            session.folder_fetch_state(root),
            Some(FolderState::Fetching)
        );

        // The descendents replies fold in: each fetched folder becomes `Loaded`
        // at its authoritative version, and the tree is then fully loaded.
        for (sequence, folder, version) in [(40, root, 5), (41, sub, 2)] {
            let reply = AnyMessage::InventoryDescendents(InventoryDescendents {
                agent_data: InventoryDescendentsAgentDataBlock {
                    agent_id: uuid::Uuid::from_u128(1),
                    folder_id: folder.uuid(),
                    owner_id: uuid::Uuid::from_u128(1),
                    version,
                    descendents: 0,
                },
                folder_data: Vec::new(),
                item_data: Vec::new(),
            });
            let datagram = server_message(&reply, sequence, false)?;
            session.handle_datagram(sim_addr(), &datagram, now)?;
            drain_events(&mut session);
        }

        assert_eq!(
            session.folder_fetch_state(root),
            Some(FolderState::Loaded { version: 5 })
        );
        assert_eq!(
            session.folder_fetch_state(sub),
            Some(FolderState::Loaded { version: 2 })
        );
        assert!(session.inventory_fully_loaded(InventoryOwner::Agent));
        // Nothing left to sweep.
        assert!(
            session
                .next_inventory_fetch_batch(INVENTORY_FETCH_MAX_IN_FLIGHT)
                .is_empty()
        );
        Ok(())
    }

    // ---- B4: idiomatic read + write API -----------------------------------

    /// A descendents sub-folder wire block for the B4 read tests.
    fn desc_folder(
        id: u128,
        parent: u128,
        folder_type: i8,
        name: &str,
    ) -> InventoryDescendentsFolderDataBlock {
        InventoryDescendentsFolderDataBlock {
            folder_id: uuid::Uuid::from_u128(id),
            parent_id: uuid::Uuid::from_u128(parent),
            r#type: folder_type,
            name: with_nul_bytes(name),
        }
    }

    /// A descendents item wire block for the B4 read tests; the asset id is
    /// derived from `id` so it can be asserted against.
    fn desc_item(
        id: u128,
        folder: u128,
        asset_type: i8,
        inv_type: i8,
        sale_type: u8,
        sale_price: i32,
        name: &str,
    ) -> InventoryDescendentsItemDataBlock {
        InventoryDescendentsItemDataBlock {
            item_id: uuid::Uuid::from_u128(id),
            folder_id: uuid::Uuid::from_u128(folder),
            creator_id: uuid::Uuid::from_u128(0xC1),
            owner_id: uuid::Uuid::from_u128(1),
            group_id: uuid::Uuid::nil(),
            base_mask: 0x7FFF_FFFF,
            owner_mask: 0x7FFF_FFFF,
            group_mask: 0,
            everyone_mask: 0,
            next_owner_mask: 0x0008_2000,
            group_owned: false,
            asset_id: uuid::Uuid::from_u128(0xA000_u128.wrapping_add(id)),
            r#type: asset_type,
            inv_type,
            flags: 0,
            sale_type,
            sale_price,
            name: with_nul_bytes(name),
            description: with_nul_bytes(""),
            creation_date: 1_200_000_000,
            crc: 0,
        }
    }

    /// Feeds one `InventoryDescendents` reply for `folder` at `version`, seeding
    /// the held model (the folder becomes `Loaded`, its children indexed), and
    /// drains the resulting events.
    fn feed_descendents(
        session: &mut Session,
        now: Instant,
        folder: u128,
        version: i32,
        folders: Vec<InventoryDescendentsFolderDataBlock>,
        items: Vec<InventoryDescendentsItemDataBlock>,
        seq: u32,
    ) -> Result<(), TestError> {
        let descendents = i32::try_from(folders.len().saturating_add(items.len())).unwrap_or(0);
        let reply = AnyMessage::InventoryDescendents(InventoryDescendents {
            agent_data: InventoryDescendentsAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(1),
                folder_id: uuid::Uuid::from_u128(folder),
                owner_id: uuid::Uuid::from_u128(1),
                version,
                descendents,
            },
            folder_data: folders,
            item_data: items,
        });
        let datagram = server_message(&reply, seq, false)?;
        session.handle_datagram(sim_addr(), &datagram, now)?;
        drain_events(session);
        Ok(())
    }

    /// The borrowed `Child` tree-walk yields a folder's sub-folders first (in key
    /// order), then its items; the owning `inventory_folder_page` window paginates
    /// the same combined sequence and a single page can span the folder/item
    /// boundary. The owning view types resolve the raw type bytes into typed enums.
    #[test]
    fn b4_tree_walk_pagination_and_view_types() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let parent = 0xB0;
        let parent_key = InventoryFolderKey::from(uuid::Uuid::from_u128(parent));
        feed_descendents(
            &mut session,
            now,
            parent,
            5,
            vec![
                desc_folder(0xB1, parent, 14, "Trash"),  // FT_TRASH
                desc_folder(0xB2, parent, -1, "Stuff"),  // FT_NONE
                desc_folder(0xB3, parent, 5, "Clothes"), // FT_CLOTHING
            ],
            vec![
                desc_item(0xC1, parent, 7, 7, 0, 0, "Note"), // notecard, not for sale
                desc_item(0xC2, parent, 6, 6, 2, 250, "Cube"), // object, copy sale L$250
            ],
            9,
        )?;

        // Borrowed tree-walk: 3 folders (key order) then 2 items.
        let walk: Vec<_> = session.inventory_children(parent_key).collect();
        assert_eq!(walk.len(), 5);
        let folder_names: Vec<&str> = walk
            .iter()
            .filter_map(|child| match child {
                Child::Folder(folder) => Some(folder.name.as_str()),
                Child::Item(_) => None,
            })
            .collect();
        assert_eq!(folder_names, vec!["Trash", "Stuff", "Clothes"]);
        let item_names: Vec<&str> = walk
            .iter()
            .filter_map(|child| match child {
                Child::Item(item) => Some(item.name.as_str()),
                Child::Folder(_) => None,
            })
            .collect();
        assert_eq!(item_names, vec!["Note", "Cube"]);

        // Paginate the combined sequence two at a time: page 2 spans the boundary.
        let (folders1, items1, cursor1) = session.inventory_folder_page(parent_key, None, 2);
        assert_eq!(folders1.len(), 2);
        assert!(items1.is_empty());
        let cursor1 = cursor1.ok_or("expected a second page")?;
        assert_eq!(cursor1.consumed_count(), 2);

        let (folders2, items2, cursor2) =
            session.inventory_folder_page(parent_key, Some(cursor1), 2);
        assert_eq!(folders2.len(), 1); // the last folder…
        assert_eq!(items2.len(), 1); // …and the first item, in one page
        let cursor2 = cursor2.ok_or("expected a third page")?;

        let (folders3, items3, cursor3) =
            session.inventory_folder_page(parent_key, Some(cursor2), 2);
        assert!(folders3.is_empty());
        assert_eq!(items3.len(), 1);
        assert!(cursor3.is_none()); // exhausted

        // View types resolve the raw bytes into typed enums.
        let trash: &FolderInfo = folders1.first().ok_or("trash folder")?;
        assert_eq!(trash.folder_type, FolderType::Trash);
        assert_eq!(trash.state, FolderState::Unknown); // a sub-folder, contents unfetched
        assert_eq!(
            trash.folder_id,
            InventoryFolderKey::from(uuid::Uuid::from_u128(0xB1))
        );

        let cube: &ItemInfo = items3.first().ok_or("cube item")?;
        assert_eq!(cube.asset_type, AssetType::Object);
        assert_eq!(cube.inv_type, InventoryType::Object);
        assert_eq!(cube.sale, Some((SaleType::Copy, LindenAmount(250))));
        assert_eq!(
            cube.asset_id,
            uuid::Uuid::from_u128(0xA000_u128.wrapping_add(0xC2))
        );

        // A not-for-sale item resolves to `sale: None`.
        let note: &ItemInfo = items2.first().ok_or("note item")?;
        assert_eq!(note.asset_type, AssetType::Notecard);
        assert_eq!(note.sale, None);
        Ok(())
    }

    /// `create_inventory_folder` rejects a nil or duplicate id (leaving the model
    /// untouched) and returns the new key for a valid create.
    #[test]
    fn b4_create_inventory_folder_validates_id() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let parent = InventoryFolderKey::from(uuid::Uuid::from_u128(0x10));
        let nil = InventoryFolderKey::from(uuid::Uuid::nil());
        assert!(matches!(
            session.create_inventory_folder(nil, parent, FolderType::None, "x", now),
            Err(sl_proto::Error::InvalidInventoryOperation(_))
        ));
        // The rejected nil create sent nothing.
        assert!(drain(&mut session)?.is_empty());

        let folder = InventoryFolderKey::from(uuid::Uuid::from_u128(0xF0));
        let key =
            session.create_inventory_folder(folder, parent, FolderType::Trash, "Trash", now)?;
        assert_eq!(key, folder);
        assert!(session.inventory_folder(folder).is_some());
        drain(&mut session)?;

        // A second create of the same id is rejected and changes nothing.
        assert!(matches!(
            session.create_inventory_folder(folder, parent, FolderType::None, "again", now),
            Err(sl_proto::Error::InvalidInventoryOperation(_))
        ));
        assert!(drain(&mut session)?.is_empty());
        assert_eq!(
            session.inventory_folder(folder).ok_or("folder")?.name,
            "Trash"
        );
        Ok(())
    }

    /// `move_inventory_folders` rejects a cycle (into self or a descendant) and a
    /// move to a parent not in the model, sending nothing and leaving the tree
    /// unchanged.
    #[test]
    fn b4_move_inventory_folders_rejects_cycle_and_unknown_parent() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let a0 = InventoryFolderKey::from(uuid::Uuid::from_u128(0xA0));
        let a1 = InventoryFolderKey::from(uuid::Uuid::from_u128(0xA1));
        let a2 = InventoryFolderKey::from(uuid::Uuid::from_u128(0xA2));
        session.create_inventory_folder(a1, a0, FolderType::None, "A1", now)?;
        session.create_inventory_folder(a2, a1, FolderType::None, "A2", now)?;
        drain(&mut session)?;

        // Into a descendant, into itself, and to an unknown parent: all rejected.
        for target in [a2, a1] {
            assert!(matches!(
                session.move_inventory_folders(&[(a1, target)], false, now),
                Err(sl_proto::Error::InvalidInventoryOperation(_))
            ));
        }
        let unknown = InventoryFolderKey::from(uuid::Uuid::from_u128(0xDEAD));
        assert!(matches!(
            session.move_inventory_folders(&[(a1, unknown)], false, now),
            Err(sl_proto::Error::InvalidInventoryOperation(_))
        ));

        // Nothing was sent and A1 is still under A0.
        assert!(drain(&mut session)?.is_empty());
        assert_eq!(
            session.inventory_folder(a1).and_then(|f| f.parent_id),
            Some(a0)
        );
        Ok(())
    }

    /// The clobber-free helpers change exactly one attribute, reading the rest
    /// from the cache: `rename_inventory_folder` keeps the type/parent,
    /// `rename_inventory_item` keeps the asset/folder/permissions, and
    /// `set_inventory_item_permissions` keeps the name.
    #[test]
    fn b4_clobber_free_helpers_preserve_other_fields() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let a0 = InventoryFolderKey::from(uuid::Uuid::from_u128(0xA0));
        let a1 = InventoryFolderKey::from(uuid::Uuid::from_u128(0xA1));
        session.create_inventory_folder(a1, a0, FolderType::Trash, "Old", now)?;
        feed_descendents(
            &mut session,
            now,
            0xA0,
            3,
            Vec::new(),
            vec![desc_item(0xD1, 0xA0, 7, 7, 0, 0, "OldItem")],
            10,
        )?;
        drain(&mut session)?;

        // Folder rename keeps the (Trash) type and the parent.
        session.rename_inventory_folder(a1, "New", now)?;
        let folder = session.inventory_folder(a1).ok_or("folder")?;
        assert_eq!(folder.name, "New");
        assert_eq!(folder.folder_type, FolderType::Trash.to_code());
        assert_eq!(folder.parent_id, Some(a0));
        drain(&mut session)?;

        let d1 = InventoryKey::from(uuid::Uuid::from_u128(0xD1));
        let original_asset = session.inventory_item(d1).ok_or("item")?.asset_id;

        // Item rename keeps the asset, folder, and permissions.
        session.rename_inventory_item(d1, "NewItem", now)?;
        let item = session.inventory_item(d1).ok_or("item")?;
        assert_eq!(item.name, "NewItem");
        assert_eq!(item.asset_id, original_asset);
        assert_eq!(item.folder_id, a0);
        drain(&mut session)?;

        // Setting permissions keeps the (renamed) name.
        session.set_inventory_item_permissions(d1, Permissions5::empty(), now)?;
        let item = session.inventory_item(d1).ok_or("item")?;
        assert_eq!(item.permissions, Permissions5::empty());
        assert_eq!(item.name, "NewItem");
        Ok(())
    }

    /// An optimistic local create is overwritten last-write-wins when the
    /// authoritative `BulkUpdateInventory` reply folds in.
    #[test]
    fn b4_optimistic_create_reconciled_by_authoritative_reply() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let parent = InventoryFolderKey::from(uuid::Uuid::from_u128(0x40));
        let folder = InventoryFolderKey::from(uuid::Uuid::from_u128(0x41));
        session.create_inventory_folder(folder, parent, FolderType::Trash, "Optimistic", now)?;
        drain(&mut session)?;
        assert_eq!(
            session.inventory_folder(folder).ok_or("folder")?.name,
            "Optimistic"
        );

        // The grid's authoritative reply renames and re-types the same folder.
        let message = AnyMessage::BulkUpdateInventory(BulkUpdateInventory {
            agent_data: BulkUpdateInventoryAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(1),
                transaction_id: uuid::Uuid::from_u128(0xAB),
            },
            folder_data: vec![BulkUpdateInventoryFolderDataBlock {
                folder_id: folder.uuid(),
                parent_id: parent.uuid(),
                r#type: 8,
                name: with_nul_bytes("Authoritative"),
            }],
            item_data: Vec::new(),
        });
        let datagram = server_message(&message, 42, false)?;
        session.handle_datagram(sim_addr(), &datagram, now)?;
        drain_events(&mut session);

        let folder = session.inventory_folder(folder).ok_or("folder")?;
        assert_eq!(folder.name, "Authoritative");
        assert_eq!(folder.folder_type, FolderType::RootInventory.to_code());
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
                } if folder_id == InventoryFolderKey::from(uuid::Uuid::from_u128(0xF0))
                    && version == 7 =>
                {
                    Some((folders, items))
                }
                _ => None,
            })
            .ok_or("expected an InventoryDescendents event")?;
        let folder = folders.first().ok_or("category")?;
        assert_eq!(folder.name, "Clothing");
        assert_eq!(
            folder.folder_id,
            InventoryFolderKey::from(uuid::Uuid::from_u128(0xF2))
        );
        let item = items.first().ok_or("item")?;
        assert_eq!(item.name, "a notecard");
        assert_eq!(item.description, "my notes");
        assert_eq!(item.asset_id, uuid::Uuid::from_u128(0xA1));
        assert_eq!(item.creator_id, AgentKey::from(uuid::Uuid::from_u128(0xC1)));
        assert_eq!(item.inv_type, 7);
        assert_eq!(item.permissions.base, Permissions::from_bits(0x7FFF_FFFF));
        assert_eq!(item.permissions.next_owner, Permissions::from_bits(532_480));
        Ok(())
    }

    /// A short Vector constructor for test fixtures.
    fn vec3(x: f32, y: f32, z: f32) -> Vector {
        Vector { x, y, z }
    }

    /// A short region-local coordinate constructor for teleport test fixtures.
    fn region_coords(x: f32, y: f32, z: f32) -> RegionCoordinates {
        RegionCoordinates::new(x, y, z)
    }

    /// Drives a session to the awaiting-handshake state (login answered, but no
    /// `RegionHandshake` received yet), draining the bootstrap traffic/events.
    fn awaiting_handshake(now: Instant) -> Result<Session, TestError> {
        let mut session = new_session()?;
        session.handle_login_response(success()?, now)?;
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
        assert_eq!(identity.sim_name, region_name("TestRegion"));
        assert_eq!(identity.maturity, Maturity::Mature);
        assert_eq!(identity.product, ProductType::Homestead);
        assert_eq!(identity.region_flags, 0x40);
        // No RegionInfo4 block: the 64-bit flags fall back to the zero-extended
        // 32-bit flags, and protocols default to 0.
        assert_eq!(identity.region_flags_extended, 0x40);
        assert_eq!(identity.region_protocols, 0);
        Ok(())
    }

    #[test]
    fn region_handshake_surfaces_extended_fields() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = awaiting_handshake(now)?;

        let owner = uuid::Uuid::from_u128(0x1234);
        let region_id = uuid::Uuid::from_u128(0xABCD);
        let mut msg = region_handshake_msg(13, 0x40, "TestRegion", "", "");
        if let AnyMessage::RegionHandshake(ref mut handshake) = msg {
            handshake.region_info.sim_owner = owner;
            handshake.region_info.is_estate_manager = true;
            handshake.region_info.water_height = 20.5;
            handshake.region_info.billable_factor = 1.0;
            handshake.region_info2.region_id = region_id;
            handshake.region_info3.cpu_class_id = 4;
            handshake.region_info3.cpu_ratio = 8;
            handshake.region_info4 = vec![RegionHandshakeRegionInfo4Block {
                region_flags_extended: 0x1_0000_0040,
                region_protocols: 0x5,
            }];
        }
        let datagram = server_message(&msg, 1, true)?;
        session.handle_datagram(sim_addr(), &datagram, now)?;

        let events = drain_events(&mut session);
        let identity = events
            .iter()
            .find_map(|e| match e {
                Event::RegionInfoHandshake(identity) => Some(identity),
                _ => None,
            })
            .ok_or("expected a RegionInfoHandshake event")?;
        assert_eq!(identity.sim_owner, owner);
        assert!(identity.is_estate_manager);
        assert!((identity.water_height - 20.5).abs() < f32::EPSILON);
        assert!((identity.billable_factor - 1.0).abs() < f32::EPSILON);
        // RegionInfo2 / RegionInfo3 supply the region id and CPU metrics.
        assert_eq!(identity.region_id, region_id);
        assert_eq!(identity.cpu_class_id, 4);
        assert_eq!(identity.cpu_ratio, 8);
        // RegionInfo4 supplies the full 64-bit flags and the protocols bitfield.
        assert_eq!(identity.region_flags_extended, 0x1_0000_0040);
        assert_eq!(identity.region_protocols, 0x5);
        Ok(())
    }

    #[test]
    fn region_handshake_carries_grid_coordinates_from_login() -> Result<(), TestError> {
        let now = Instant::now();
        // Log in with a start region at global (256000, 256512) metres — grid
        // (1000, 1002). The handshake does not carry the handle, so the session
        // must surface the one seeded from the login response.
        let mut session = new_session()?;
        let LoginResponse::Success(mut boxed) = success()? else {
            return Err("expected a success fixture".into());
        };
        boxed.region_x = Some(256_000);
        boxed.region_y = Some(256_512);
        session.handle_login_response(LoginResponse::Success(boxed), now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        let handshake =
            server_message(&region_handshake_msg(13, 0, "TestRegion", "", ""), 1, true)?;
        session.handle_datagram(sim_addr(), &handshake, now)?;

        let events = drain_events(&mut session);
        let identity = events
            .iter()
            .find_map(|e| match e {
                Event::RegionInfoHandshake(identity) => Some(identity),
                _ => None,
            })
            .ok_or("expected a RegionInfoHandshake event")?;
        assert_eq!(
            identity.region_handle,
            RegionHandle(sl_proto::global_to_handle(256_000, 256_512))
        );
        assert_eq!(identity.grid_coordinates.x(), 1000);
        assert_eq!(identity.grid_coordinates.y(), 1002);
        Ok(())
    }

    #[test]
    fn ext_environment_caps_surfaces_day_cycle() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        // An `ExtEnvironment` GET reply: a region environment with one sky frame
        // and one water frame scheduled by a water track and one sky track.
        let xml = concat!(
            "<llsd><map>",
            "<key>environment</key><map>",
            "<key>parcel_id</key><integer>-1</integer>",
            "<key>region_id</key><uuid>00000000-0000-0000-0000-000000000042</uuid>",
            "<key>day_length</key><integer>14400</integer>",
            "<key>day_offset</key><integer>0</integer>",
            "<key>env_version</key><integer>3</integer>",
            "<key>track_altitudes</key><array><real>1000</real><real>2000</real><real>3000</real></array>",
            "<key>day_cycle</key><map>",
            "<key>name</key><string>Test Cycle</string>",
            "<key>type</key><string>daycycle</string>",
            "<key>frames</key><map>",
            "<key>Sunrise</key><map>",
            "<key>type</key><string>sky</string>",
            "<key>max_y</key><real>1605</real>",
            "<key>star_brightness</key><real>0.5</real>",
            "<key>sun_rotation</key><array><real>0</real><real>0</real><real>0</real><real>1</real></array>",
            "<key>cloud_id</key><uuid>00000000-0000-0000-0000-0000000000cc</uuid>",
            "<key>legacy_haze</key><map>",
            "<key>ambient</key><array><real>0.25</real><real>0.25</real><real>0.25</real></array>",
            "<key>haze_density</key><real>0.75</real>",
            "</map></map>",
            "<key>Default</key><map>",
            "<key>type</key><string>water</string>",
            "<key>water_fog_density</key><real>2</real>",
            "<key>normal_map</key><uuid>00000000-0000-0000-0000-0000000000aa</uuid>",
            "<key>wave1_direction</key><array><real>1.5</real><real>-0.5</real></array>",
            "</map></map>",
            "<key>tracks</key><array>",
            "<array><map><key>key_keyframe</key><real>0</real><key>key_name</key><string>Default</string></map></array>",
            "<array><map><key>key_keyframe</key><real>0.25</real><key>key_name</key><string>Sunrise</string></map></array>",
            "</array></map></map>",
            "<key>parcel_id</key><integer>-1</integer>",
            "<key>success</key><boolean>1</boolean>",
            "</map></llsd>",
        );
        let body = parse_llsd_xml(xml)?;
        session.handle_caps_event(sl_proto::CAP_EXT_ENVIRONMENT, &body, now)?;

        let env = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::Environment(env) => Some(env),
                _ => None,
            })
            .ok_or("expected an Environment event")?;
        assert_eq!(env.parcel_id, -1);
        assert_eq!(env.region_id, uuid::Uuid::from_u128(0x42));
        assert_eq!(env.day_length, 14400);
        assert_eq!(env.env_version, 3);
        assert!(
            env.track_altitudes
                .iter()
                .zip([1000.0, 2000.0, 3000.0])
                .all(|(actual, expected)| (actual - expected).abs() < f32::EPSILON)
        );

        let cycle = &env.day_cycle;
        assert_eq!(cycle.name, "Test Cycle");
        // Track 0 is the water track; the remaining tracks are sky tracks.
        let water_frame = cycle.water_track.first().ok_or("water keyframe")?;
        assert_eq!(water_frame.name, "Default");
        assert!((water_frame.keyframe - 0.0).abs() < f32::EPSILON);
        let sky_frame = cycle
            .sky_tracks
            .first()
            .and_then(|track| track.first())
            .ok_or("sky keyframe")?;
        assert_eq!(sky_frame.name, "Sunrise");
        assert!((sky_frame.keyframe - 0.25).abs() < f32::EPSILON);

        let sky = cycle.sky_frames.get("Sunrise").ok_or("sky frame")?;
        assert!((sky.max_y - 1605.0).abs() < f32::EPSILON);
        assert!((sky.star_brightness - 0.5).abs() < f32::EPSILON);
        assert!((sky.sun_rotation.s - 1.0).abs() < f32::EPSILON);
        assert_eq!(
            sky.cloud_texture,
            Some(TextureKey::from(uuid::Uuid::from_u128(0xcc)))
        );
        // Haze colours/scalars come from the `legacy_haze` sub-map.
        assert!((sky.ambient.red() - 0.25).abs() < f32::EPSILON);
        assert!((sky.ambient.green() - 0.25).abs() < f32::EPSILON);
        assert!((sky.ambient.blue() - 0.25).abs() < f32::EPSILON);
        assert!((sky.haze_density - 0.75).abs() < f32::EPSILON);

        let water = cycle.water_frames.get("Default").ok_or("water frame")?;
        assert!((water.water_fog_density - 2.0).abs() < f32::EPSILON);
        assert_eq!(
            water.normal_map,
            Some(TextureKey::from(uuid::Uuid::from_u128(0xaa)))
        );
        assert!(
            water
                .wave1_direction
                .iter()
                .zip([1.5, -0.5])
                .all(|(actual, expected)| (actual - expected).abs() < f32::EPSILON)
        );
        Ok(())
    }

    /// A fully-populated sky frame with exactly-representable `f32` values, so an
    /// encode→decode round trip is bit-exact.
    fn sky_fixture(name: &str) -> SkySettings {
        SkySettings {
            name: name.to_owned(),
            sun_rotation: Rotation {
                x: 0.0,
                y: 0.0,
                z: 0.0,
                s: 1.0,
            },
            moon_rotation: Rotation {
                x: 0.5,
                y: 0.5,
                z: 0.5,
                s: 0.5,
            },
            sunlight_color: ColorAlpha::new(0.25, 0.5, 0.75, 1.0),
            ambient: Color::new(0.125, 0.25, 0.5),
            blue_horizon: Color::new(0.25, 0.5, 1.0),
            blue_density: Color::new(0.5, 0.25, 0.125),
            haze_horizon: 0.75,
            haze_density: 2.0,
            density_multiplier: 0.25,
            distance_multiplier: 4.0,
            max_y: 1605.0,
            gamma: 1.0,
            cloud_color: Color::new(0.5, 0.5, 0.5),
            cloud_pos_density1: CloudPosDensity::new(1.0, 0.5, 0.25),
            cloud_pos_density2: CloudPosDensity::new(0.125, 0.25, 0.5),
            cloud_scale: 0.5,
            cloud_scroll_rate: [10.0, 10.25],
            cloud_shadow: 0.25,
            cloud_variance: 0.0,
            glow: Glow::new(5.0, 0.0, -2.5),
            star_brightness: 0.5,
            sun_scale: 1.0,
            moon_scale: 1.0,
            moon_brightness: 0.5,
            sun_arc_radians: 0.125,
            droplet_radius: 800.0,
            ice_level: 0.0,
            moisture_level: 0.5,
            sky_top_radius: 6400.0,
            sky_bottom_radius: 6360.0,
            planet_radius: 6360.0,
            sun_texture: Some(TextureKey::from(uuid::Uuid::from_u128(0x511))),
            moon_texture: Some(TextureKey::from(uuid::Uuid::from_u128(0x110))),
            cloud_texture: Some(TextureKey::from(uuid::Uuid::from_u128(0xc10))),
            bloom_texture: Some(TextureKey::from(uuid::Uuid::from_u128(0xb1))),
            halo_texture: Some(TextureKey::from(uuid::Uuid::from_u128(0xa10))),
            rainbow_texture: Some(TextureKey::from(uuid::Uuid::from_u128(0x4a1))),
        }
    }

    /// A fully-populated water frame with exactly-representable `f32` values.
    fn water_fixture(name: &str) -> WaterSettings {
        WaterSettings {
            name: name.to_owned(),
            blur_multiplier: 0.25,
            fresnel_offset: 0.5,
            fresnel_scale: 0.75,
            normal_scale: Scale::new(2.0, 2.0, 2.0),
            normal_map: Some(TextureKey::from(uuid::Uuid::from_u128(0x404))),
            scale_above: 0.125,
            scale_below: 0.25,
            transparent_texture: Some(TextureKey::from(uuid::Uuid::from_u128(0x7a))),
            underwater_fog_mod: 0.25,
            water_fog_color: Color::new(0.0, 0.25, 0.5),
            water_fog_density: 16.0,
            wave1_direction: [1.5, -0.5],
            wave2_direction: [-1.0, 0.25],
        }
    }

    #[test]
    fn environment_round_trips_through_llsd() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        let mut sky_frames = std::collections::BTreeMap::new();
        sky_frames.insert("Sunrise".to_owned(), sky_fixture("Sunrise"));
        sky_frames.insert("Noon".to_owned(), sky_fixture("Noon"));
        let mut water_frames = std::collections::BTreeMap::new();
        water_frames.insert("Default".to_owned(), water_fixture("Default"));
        let original = EnvironmentSettings {
            parcel_id: -1,
            region_id: uuid::Uuid::from_u128(0x42),
            day_length: 14400,
            day_offset: 0,
            flags: 0,
            env_version: 3,
            track_altitudes: [1000.0, 2000.0, 3000.0],
            day_cycle: DayCycle {
                name: "Test Cycle".to_owned(),
                water_track: vec![DayCycleFrame {
                    keyframe: 0.0,
                    name: "Default".to_owned(),
                }],
                sky_tracks: vec![vec![
                    DayCycleFrame {
                        keyframe: 0.25,
                        name: "Sunrise".to_owned(),
                    },
                    DayCycleFrame {
                        keyframe: 0.5,
                        name: "Noon".to_owned(),
                    },
                ]],
                sky_frames,
                water_frames,
            },
        };

        // Encode with the server-side encoder, decode with the client path.
        let body = sl_proto::environment_to_llsd(&original);
        session.handle_caps_event(sl_proto::CAP_EXT_ENVIRONMENT, &body, now)?;
        let decoded = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::Environment(env) => Some(env),
                _ => None,
            })
            .ok_or("expected an Environment event")?;
        assert_eq!(*decoded, original);
        Ok(())
    }

    #[test]
    fn uuid_name_reply_surfaces_avatar_names() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        let alice = uuid::Uuid::from_u128(0xA11CE);
        let bob = uuid::Uuid::from_u128(0xB0B);
        let reply = AnyMessage::UUIDNameReply(UUIDNameReply {
            uuid_name_block: vec![
                UUIDNameReplyUUIDNameBlockBlock {
                    id: alice,
                    first_name: b"Alice".to_vec(),
                    last_name: b"Liddell".to_vec(),
                },
                UUIDNameReplyUUIDNameBlockBlock {
                    id: bob,
                    first_name: b"Bob".to_vec(),
                    last_name: b"Resident".to_vec(),
                },
            ],
        });
        let datagram = server_message(&reply, 9, true)?;
        session.handle_datagram(sim_addr(), &datagram, now)?;

        let events = drain_events(&mut session);
        let names = events
            .iter()
            .find_map(|e| match e {
                Event::AvatarNames(names) => Some(names),
                _ => None,
            })
            .ok_or("expected an AvatarNames event")?;
        assert_eq!(names.len(), 2);
        let alice_name = names.iter().find(|n| n.id.uuid() == alice).ok_or("alice")?;
        assert_eq!(alice_name.legacy_name(), "Alice Liddell");
        // The "Resident" placeholder last name collapses to the first name.
        let bob_name = names.iter().find(|n| n.id.uuid() == bob).ok_or("bob")?;
        assert_eq!(bob_name.legacy_name(), "Bob");
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
        // No RegionInfo3/5/CombatSettings blocks: the 64-bit flags fall back to
        // the 32-bit flags and the optional blocks are absent.
        assert_eq!(limits.region_flags_extended, 0);
        assert!(limits.chat_settings.is_none());
        assert!(limits.combat_settings.is_none());
        Ok(())
    }

    #[test]
    fn region_info_surfaces_extended_fields() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        let mut msg = region_info_msg("TestRegion", 13, 0, 50, 60, 15000);
        if let AnyMessage::RegionInfo(ref mut info) = msg {
            info.region_info.estate_id = 101;
            info.region_info.parent_estate_id = 1;
            info.region_info.region_flags = 0x40;
            info.region_info.water_height = 20.0;
            info.region_info.billable_factor = 1.0;
            info.region_info.object_bonus_factor = 2.0;
            info.region_info.terrain_raise_limit = 4.0;
            info.region_info.terrain_lower_limit = -4.0;
            info.region_info.price_per_meter = 1;
            info.region_info.use_estate_sun = true;
            info.region_info.sun_hour = 12.0;
            info.region_info3 = vec![RegionInfoRegionInfo3Block {
                region_flags_extended: 0x1_0000_0040,
            }];
            info.region_info5 = vec![RegionInfoRegionInfo5Block {
                chat_whisper_range: 10.0,
                chat_normal_range: 20.0,
                chat_shout_range: 100.0,
                chat_whisper_offset: 0.0,
                chat_normal_offset: 0.0,
                chat_shout_offset: 0.0,
                chat_flags: 3,
            }];
            info.combat_settings = vec![RegionInfoCombatSettingsBlock {
                combat_flags: 7,
                on_death: 2,
                damage_throttle: 1.5,
                regeneration_rate: 0.25,
                invulnerabily_time: 5.0,
                damage_limit: 100.0,
            }];
        }
        let datagram = server_message(&msg, 9, true)?;
        session.handle_datagram(sim_addr(), &datagram, now)?;

        let events = drain_events(&mut session);
        let limits = events
            .iter()
            .find_map(|e| match e {
                Event::RegionLimits(limits) => Some(limits),
                _ => None,
            })
            .ok_or("expected a RegionLimits event")?;
        assert_eq!(limits.max_agents, 50);
        assert_eq!(limits.hard_max_agents, 60);
        assert_eq!(limits.estate_id, 101);
        assert_eq!(limits.parent_estate_id, 1);
        assert!((limits.water_height - 20.0).abs() < f32::EPSILON);
        assert!((limits.object_bonus_factor - 2.0).abs() < f32::EPSILON);
        assert!((limits.terrain_raise_limit - 4.0).abs() < f32::EPSILON);
        assert!((limits.terrain_lower_limit + 4.0).abs() < f32::EPSILON);
        assert!(limits.use_estate_sun);
        assert!((limits.sun_hour - 12.0).abs() < f32::EPSILON);
        assert_eq!(limits.region_flags_extended, 0x1_0000_0040);
        let chat = limits
            .chat_settings
            .as_ref()
            .ok_or("expected chat settings")?;
        assert!((chat.shout_range - 100.0).abs() < f32::EPSILON);
        assert_eq!(chat.flags, 3);
        let combat = limits
            .combat_settings
            .as_ref()
            .ok_or("expected combat settings")?;
        assert_eq!(combat.flags, 7);
        assert_eq!(combat.on_death, 2);
        assert!((combat.invulnerability_time - 5.0).abs() < f32::EPSILON);
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
        assert_eq!(parcel.local_id, sl_proto::RegionLocalParcelId(7));
        assert_eq!(parcel.area, LandArea(4096));
        assert_eq!(parcel.aabb_max.x().to_bits(), 64.0_f32.to_bits());
        assert_eq!(parcel.aabb_max.y().to_bits(), 64.0_f32.to_bits());
        assert_eq!(parcel.aabb_max.z().to_bits(), 0.0_f32.to_bits());
        assert_eq!(parcel.max_prims, 1000);
        assert_eq!(parcel.sim_wide_max_prims, 5000);
        assert_eq!(parcel.bitmap.len(), 512);
        assert!(parcel.create_objects());
        assert!(parcel.use_ban_list());
        assert!(!parcel.use_access_list());
        // Absent media → empty URLs / nil id / no auto-scale.
        assert_eq!(parcel.music_url, None);
        assert_eq!(parcel.media_url, None);
        assert_eq!(parcel.media_id, None);
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
        assert_eq!(
            parcel.music_url.as_ref().map(url::Url::as_str),
            Some("http://stream.example/audio")
        );
        assert_eq!(
            parcel.media_url.as_ref().map(url::Url::as_str),
            Some("http://example.com/movie")
        );
        assert_eq!(parcel.media_id, Some(TextureKey::from(media_id)));
        assert!(parcel.media_auto_scale);
        Ok(())
    }

    #[test]
    fn parcel_properties_reports_full_field_surface() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        let owner = uuid::Uuid::from_u128(0x0E1);
        let group = uuid::Uuid::from_u128(0x0E2);
        let buyer = uuid::Uuid::from_u128(0x0E3);
        let snapshot = uuid::Uuid::from_u128(0x0E4);
        let mut message = parcel_properties_msg(5, 9, 1024, 0, 200, 4000, vec3(32.0, 32.0, 0.0));
        if let AnyMessage::ParcelProperties(props) = &mut message {
            let data = &mut props.parcel_data;
            data.request_result = 0;
            data.name = b"Sunset Cove\0".to_vec();
            data.desc = b"A quiet beach parcel".to_vec();
            data.owner_id = owner;
            data.is_group_owned = true;
            data.group_id = group;
            data.auction_id = 7;
            data.claim_date = 1_700_000_000;
            data.claim_price = 512;
            data.rent_price = 30;
            data.status = 2; // OS_ABANDONED
            data.category = 2; // Residential
            data.total_prims = 150;
            data.owner_prims = 100;
            data.group_prims = 20;
            data.other_prims = 30;
            data.selected_prims = 4;
            data.parcel_prim_bonus = 1.5;
            data.other_clean_time = 15;
            // FOR_SALE (0x04), so the sale price decodes as `Some`.
            data.parcel_flags = 0x04;
            data.sale_price = 9999;
            data.auth_buyer_id = buyer;
            data.snapshot_id = snapshot;
            data.pass_price = 25;
            data.pass_hours = 4.0;
            data.user_location = vec3(12.0, 13.0, 14.0);
            data.user_look_at = vec3(1.0, 0.0, 0.0);
            data.landing_type = 2; // L_DIRECT (anywhere)
            data.region_push_override = true;
            data.region_deny_anonymous = true;
            data.region_deny_identified = false;
            data.region_deny_transacted = true;
            props.age_verification_block.region_deny_age_unverified = true;
            props.region_allow_access_block.region_allow_access_override = true;
            props.parcel_environment_block.parcel_environment_version = 3;
            props
                .parcel_environment_block
                .region_allow_environment_override = true;
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
        assert_eq!(parcel.request_result, ParcelRequestResult::Single);
        assert!(parcel.request_result.has_data());
        assert_eq!(parcel.name, "Sunset Cove");
        assert_eq!(parcel.description, "A quiet beach parcel");
        assert_eq!(
            parcel.owner,
            sl_proto::OwnerKey::Group(GroupKey::from(owner))
        );
        assert_eq!(parcel.group, Some(GroupKey::from(group)));
        assert_eq!(parcel.auction_id, 7);
        assert_eq!(parcel.claim_date, 1_700_000_000);
        assert_eq!(parcel.claim_price, LindenAmount(512));
        assert_eq!(parcel.rent_price, LindenAmount(30));
        assert_eq!(parcel.status, ParcelStatus::Abandoned);
        assert_eq!(parcel.category, ParcelCategory::Residential);
        assert_eq!(parcel.total_prims, 150);
        assert_eq!(parcel.owner_prims, 100);
        assert_eq!(parcel.group_prims, 20);
        assert_eq!(parcel.other_prims, 30);
        assert_eq!(parcel.selected_prims, 4);
        assert_eq!(parcel.parcel_prim_bonus.to_bits(), 1.5_f32.to_bits());
        assert_eq!(parcel.other_clean_time, 15);
        assert_eq!(parcel.sale_price, Some(LindenAmount(9999)));
        assert_eq!(parcel.auth_buyer_id, Some(AgentKey::from(buyer)));
        assert_eq!(parcel.snapshot_id, Some(TextureKey::from(snapshot)));
        assert_eq!(parcel.pass_price, LindenAmount(25));
        assert_eq!(parcel.pass_hours.to_bits(), 4.0_f32.to_bits());
        assert_eq!(parcel.user_location.x().to_bits(), 12.0_f32.to_bits());
        assert_eq!(parcel.user_look_at.x().to_bits(), 1.0_f32.to_bits());
        assert_eq!(parcel.landing_type, LandingType::Anywhere);
        assert!(parcel.region_push_override);
        assert!(parcel.region_deny_anonymous);
        assert!(!parcel.region_deny_identified);
        assert!(parcel.region_deny_transacted);
        assert!(parcel.region_deny_age_unverified);
        assert!(parcel.region_allow_access_override);
        assert_eq!(parcel.parcel_environment_version, 3);
        assert!(parcel.region_allow_environment_override);
        // The UDP message omits the per-parcel AV-sound booleans.
        assert_eq!(parcel.see_avs, None);
        assert_eq!(parcel.any_av_sounds, None);
        assert_eq!(parcel.group_av_sounds, None);
        Ok(())
    }

    #[test]
    fn parcel_properties_no_data_result_is_distinguished() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        let mut message = parcel_properties_msg(2, 0, 0, 0, 0, 0, vec3(0.0, 0.0, 0.0));
        if let AnyMessage::ParcelProperties(props) = &mut message {
            props.parcel_data.request_result = -1; // PARCEL_RESULT_NO_DATA
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
        assert_eq!(parcel.request_result, ParcelRequestResult::NoData);
        assert!(!parcel.request_result.has_data());
        Ok(())
    }

    #[test]
    fn parcel_properties_caps_llsd_full_field_surface() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        // The CAPS event-queue form OpenSim emits: a `ParcelData` block plus the
        // three trailing single-element blocks. `ClaimDate` is an LLSD `date`,
        // `Category`/`Status`/`LandingType` are integers, and the per-parcel
        // AV-sound booleans (`SeeAVs`/…) are present only here.
        let xml = "<llsd><map>\
            <key>ParcelData</key><array><map>\
            <key>SequenceID</key><integer>3</integer>\
            <key>RequestResult</key><integer>0</integer>\
            <key>LocalID</key><integer>11</integer>\
            <key>Name</key><string>Harbor Lot</string>\
            <key>Desc</key><string>dockside</string>\
            <key>OwnerID</key><uuid>00000000-0000-0000-0000-000000000111</uuid>\
            <key>IsGroupOwned</key><boolean>true</boolean>\
            <key>GroupID</key><uuid>00000000-0000-0000-0000-000000000222</uuid>\
            <key>ClaimDate</key><date>2023-11-14T22:13:20Z</date>\
            <key>ParcelFlags</key><integer>4</integer>\
            <key>SalePrice</key><integer>4500</integer>\
            <key>Status</key><integer>1</integer>\
            <key>Category</key><integer>3</integer>\
            <key>TotalPrims</key><integer>77</integer>\
            <key>LandingType</key><integer>1</integer>\
            <key>RegionDenyAnonymous</key><boolean>true</boolean>\
            <key>SeeAVs</key><boolean>false</boolean>\
            <key>AnyAVSounds</key><boolean>true</boolean>\
            <key>GroupAVSounds</key><boolean>false</boolean>\
            </map></array>\
            <key>AgeVerificationBlock</key><array><map>\
            <key>RegionDenyAgeUnverified</key><boolean>true</boolean>\
            </map></array>\
            <key>RegionAllowAccessBlock</key><array><map>\
            <key>RegionAllowAccessOverride</key><boolean>true</boolean>\
            </map></array>\
            <key>ParcelEnvironmentBlock</key><array><map>\
            <key>ParcelEnvironmentVersion</key><integer>5</integer>\
            <key>RegionAllowEnvironmentOverride</key><boolean>true</boolean>\
            </map></array>\
            </map></llsd>";
        let body = parse_llsd_xml(xml)?;
        session.handle_caps_event("ParcelProperties", &body, now)?;

        let events = drain_events(&mut session);
        let parcel = events
            .iter()
            .find_map(|e| match e {
                Event::ParcelProperties(parcel) => Some(parcel),
                _ => None,
            })
            .ok_or("expected a ParcelProperties event")?;
        assert_eq!(parcel.sequence_id, 3);
        assert_eq!(parcel.request_result, ParcelRequestResult::Single);
        assert_eq!(parcel.local_id, sl_proto::RegionLocalParcelId(11));
        assert_eq!(parcel.name, "Harbor Lot");
        assert_eq!(parcel.description, "dockside");
        assert_eq!(
            parcel.owner,
            sl_proto::OwnerKey::Group(GroupKey::from(uuid::Uuid::from_u128(0x111)))
        );
        assert_eq!(
            parcel.group,
            Some(GroupKey::from(uuid::Uuid::from_u128(0x222)))
        );
        // 2023-11-14T22:13:20Z == 1_700_000_000 Unix seconds.
        assert_eq!(parcel.claim_date, 1_700_000_000);
        assert_eq!(parcel.sale_price, Some(LindenAmount(4500)));
        assert_eq!(parcel.status, ParcelStatus::LeasePending);
        assert_eq!(parcel.category, ParcelCategory::Commercial);
        assert_eq!(parcel.total_prims, 77);
        assert_eq!(parcel.landing_type, LandingType::LandingPoint);
        assert!(parcel.region_deny_anonymous);
        assert!(parcel.region_deny_age_unverified);
        assert!(parcel.region_allow_access_override);
        assert_eq!(parcel.parcel_environment_version, 5);
        assert!(parcel.region_allow_environment_override);
        assert_eq!(parcel.see_avs, Some(false));
        assert_eq!(parcel.any_av_sounds, Some(true));
        assert_eq!(parcel.group_av_sounds, Some(false));
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
        assert_eq!(neighbor.region_handle, RegionHandle(0x0003_E800_0003_E900));
        assert_eq!(neighbor.grid_coordinates.x(), 1000);
        assert_eq!(neighbor.grid_coordinates.y(), 1001);
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
    fn child_circuit_sends_keepalive_ping_and_times_the_reply() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);
        enable_neighbour_b(&mut session, 9, now)?;
        // Drain the child open burst (UseCircuitCode, AgentUpdate) and root traffic.
        while session.poll_transmit().is_some() {}

        // One ping interval later (the session's 5 s `PING_INTERVAL`) the child
        // circuit's keep-alive timer fires and it transmits its own
        // `StartPingCheck` to the neighbour, numbered from the child's own ping id
        // sequence (first id 0).
        let sent_at = after(now, 5_000)?;
        session.handle_timeout(sent_at);
        // The same tick also re-sends the child `AgentUpdate`, so scan all
        // transmits to the neighbour for the `StartPingCheck`.
        let mut child_ping_id = None;
        while let Some(transmit) = session.poll_transmit() {
            if transmit.destination == sim_b()
                && let AnyMessage::StartPingCheck(ping) = decode(&transmit)?
            {
                child_ping_id = Some(ping.ping_id.ping_id);
            }
        }
        assert_eq!(
            child_ping_id,
            Some(0),
            "expected a child keep-alive StartPingCheck (ping id 0) to sim_b"
        );

        // The neighbour answers 200ms later; the child times the round trip and
        // surfaces it as a child-circuit `Event::Ping`.
        let replied_at = after(now, 5_200)?;
        let complete = server_datagram(MessageId::High(2), &[0], 3, false);
        session.handle_datagram(sim_b(), &complete, replied_at)?;
        let ping_event = drain_events(&mut session)
            .into_iter()
            .find_map(|e| match e {
                Event::Ping { sim, child, rtt } => Some((sim, child, rtt)),
                _other => None,
            });
        assert_eq!(
            ping_event,
            Some((sim_b(), true, Duration::from_millis(200))),
            "expected a child Event::Ping for sim_b with the measured RTT"
        );
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
                Event::RegionChanged {
                    region_handle, sim, ..
                } => Some((region_handle, sim)),
                _ => None,
            })
            .ok_or("expected a RegionChanged event")?;
        assert_eq!(changed.0, RegionHandle(handle));
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
        assert_eq!(neighbour.region_handle, RegionHandle(0x0003_E900_0003_E800));
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
                Event::RegionChanged {
                    region_handle, sim, ..
                } => Some((region_handle, sim)),
                _ => None,
            })
            .ok_or("expected a RegionChanged event")?;
        assert_eq!(changed.0, RegionHandle(handle));
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
        assert_eq!(balance.agent_id, AgentKey::from(uuid::Uuid::from_u128(1)));
        assert!(balance.success);
        assert_eq!(balance.balance, LindenAmount(1234));
        assert_eq!(balance.square_meters_credit, LandArea(512));
        assert_eq!(balance.square_meters_committed, LandArea(128));
        // A plain poll carries no transaction metadata and a nil transaction id.
        assert_eq!(balance.transaction_id, uuid::Uuid::nil());
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
                transaction_id: uuid::Uuid::from_u128(0x7A11),
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
        // The transaction id correlates the reply back to the triggering pay/buy.
        assert_eq!(balance.transaction_id, uuid::Uuid::from_u128(0x7A11));
        let transaction = balance.transaction.ok_or("expected transaction details")?;
        assert_eq!(
            MoneyTransactionType::from_i32(transaction.transaction_type),
            MoneyTransactionType::PayObject
        );
        assert_eq!(
            transaction.source,
            sl_proto::OwnerKey::Agent(sl_proto::AgentKey::from(uuid::Uuid::from_u128(1)))
        );
        assert_eq!(
            transaction.dest,
            sl_proto::OwnerKey::Agent(sl_proto::AgentKey::from(uuid::Uuid::from_u128(0xBEEF)))
        );
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
        assert_eq!(economy.price_upload, LindenAmount(0));
        assert_eq!(economy.price_energy_unit, LindenAmount(100));
        assert_eq!(economy.teleport_min_price, LindenAmount(2));
        Ok(())
    }

    #[test]
    fn map_name_and_item_requests_encode() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        session.request_map_by_name("East Region", now)?;
        session.request_map_items(MapItemType::AgentLocations, RegionHandle(0), now)?;
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
        assert_eq!(
            first.position,
            GlobalCoordinates::new(256_128.0, 256_064.0, 0.0)
        );
        // The region handle splits off the in-region offset; the region position
        // recovers it (the typed replacement for the old `& 0xFF` masking).
        assert_eq!(
            first.region_handle(),
            Some(RegionHandle(0x0003_E800_0003_E800))
        );
        let region_position = first.region_position().ok_or("region position")?;
        assert_eq!(region_position.x().to_bits(), 128.0_f32.to_bits());
        assert_eq!(region_position.y().to_bits(), 64.0_f32.to_bits());
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
            local_id: sl_proto::RegionLocalParcelId(7),
            parcel_flags: ParcelFlags::CREATE_OBJECTS.union(ParcelFlags::USE_BAN_LIST),
            name: "My Parcel".to_owned(),
            description: "A test parcel".to_owned(),
            category: ParcelCategory::Residential,
            sale_price: Some(LindenAmount(100)),
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
        let circuit = session.root_circuit_id().ok_or("no circuit")?;
        drain(&mut session)?;

        session.request_parcel_access_list(
            ScopedParcelId::new(circuit, sl_proto::RegionLocalParcelId(7)),
            ParcelAccessScope::Ban,
            now,
        )?;
        session.update_parcel_access_list(
            ScopedParcelId::new(circuit, sl_proto::RegionLocalParcelId(7)),
            ParcelAccessScope::Access,
            &[ParcelAccessEntry {
                id: uuid::Uuid::from_u128(0x55),
                time: 0,
                // An experience allow flag must be OR'd onto the scope on the wire.
                flags: ParcelAccessFlags::ALLOW_EXPERIENCE,
            }],
            now,
        )?;
        session.request_parcel_dwell(
            ScopedParcelId::new(circuit, sl_proto::RegionLocalParcelId(7)),
            now,
        )?;
        session.buy_parcel(
            ScopedParcelId::new(circuit, sl_proto::RegionLocalParcelId(7)),
            512,
            1024,
            None,
            false,
            now,
        )?;
        session.return_parcel_objects(
            ScopedParcelId::new(circuit, sl_proto::RegionLocalParcelId(7)),
            ParcelReturnType::OTHER,
            &[OwnerKey::Agent(AgentKey::from(uuid::Uuid::from_u128(0x99)))],
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
        // Scope (AL_ACCESS, 0x1) OR'd with the per-entry AL_ALLOW_EXPERIENCE (0x8).
        assert_eq!(entry.flags, 0x1 | 0x8);

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
        assert_eq!(dwell.0.id, sl_proto::RegionLocalParcelId(7));
        assert_eq!(dwell.1, ParcelKey::from(uuid::Uuid::from_u128(0xABC)));
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
                    // A banned experience entry: AL_BAN (0x2) | AL_BLOCK_EXPERIENCE (0x10).
                    flags: 0x2 | 0x10,
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
        assert_eq!(local_id.id, sl_proto::RegionLocalParcelId(7));
        assert_eq!(scope, ParcelAccessScope::Ban);
        assert_eq!(entries.len(), 2);
        let first = entries.first().ok_or("expected a first entry")?;
        assert_eq!(first.flags, ParcelAccessFlags::BAN);
        let second = entries.get(1).ok_or("expected a second entry")?;
        assert_eq!(second.id, uuid::Uuid::from_u128(0x11));
        assert_eq!(second.time, 1234);
        assert_eq!(
            second.flags,
            ParcelAccessFlags::BAN.union(ParcelAccessFlags::BLOCK_EXPERIENCE)
        );
        assert!(second.flags.contains(ParcelAccessFlags::BLOCK_EXPERIENCE));
        Ok(())
    }

    #[test]
    fn parcel_g7_commands_encode() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        let circuit = session.root_circuit_id().ok_or("no circuit")?;
        drain(&mut session)?;

        session.join_parcels(16.0, 32.0, 48.0, 64.0, now)?;
        session.divide_parcel(1.0, 2.0, 3.0, 4.0, now)?;
        session.request_parcel_object_owners(
            ScopedParcelId::new(circuit, sl_proto::RegionLocalParcelId(7)),
            now,
        )?;
        session.buy_parcel_pass(
            ScopedParcelId::new(circuit, sl_proto::RegionLocalParcelId(7)),
            now,
        )?;
        session.disable_parcel_objects(
            ScopedParcelId::new(circuit, sl_proto::RegionLocalParcelId(7)),
            ParcelReturnType::OTHER,
            &[OwnerKey::Agent(AgentKey::from(uuid::Uuid::from_u128(0x99)))],
            &[ObjectKey::from(uuid::Uuid::from_u128(0xAB))],
            now,
        )?;
        session.request_parcel_info(ParcelKey::from(uuid::Uuid::from_u128(0x00C0_FFEE)), now)?;
        let sent = drain(&mut session)?;

        let join = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::ParcelJoin(message) => Some(message),
                _ => None,
            })
            .ok_or("expected a ParcelJoin")?;
        assert_eq!(join.parcel_data.west.to_bits(), 16.0_f32.to_bits());
        assert_eq!(join.parcel_data.north.to_bits(), 64.0_f32.to_bits());

        let divide = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::ParcelDivide(message) => Some(message),
                _ => None,
            })
            .ok_or("expected a ParcelDivide")?;
        assert_eq!(divide.parcel_data.east.to_bits(), 3.0_f32.to_bits());

        let owners = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::ParcelObjectOwnersRequest(message) => Some(message),
                _ => None,
            })
            .ok_or("expected a ParcelObjectOwnersRequest")?;
        assert_eq!(owners.parcel_data.local_id, 7);

        assert!(
            sent.iter()
                .any(|m| matches!(m, AnyMessage::ParcelBuyPass(_))),
            "expected a ParcelBuyPass"
        );

        let disable = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::ParcelDisableObjects(message) => Some(message),
                _ => None,
            })
            .ok_or("expected a ParcelDisableObjects")?;
        assert_eq!(disable.parcel_data.return_type, ParcelReturnType::OTHER.0);
        assert_eq!(disable.owner_i_ds.len(), 1);
        assert_eq!(disable.task_i_ds.len(), 1);

        let info = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::ParcelInfoRequest(message) => Some(message),
                _ => None,
            })
            .ok_or("expected a ParcelInfoRequest")?;
        assert_eq!(info.data.parcel_id, uuid::Uuid::from_u128(0x00C0_FFEE));
        Ok(())
    }

    #[test]
    fn parcel_object_owners_reply_surfaces_event() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        let reply = AnyMessage::ParcelObjectOwnersReply(ParcelObjectOwnersReply {
            data: vec![
                ParcelObjectOwnersReplyDataBlock {
                    owner_id: uuid::Uuid::from_u128(0x21),
                    is_group_owned: false,
                    count: 12,
                    online_status: true,
                },
                ParcelObjectOwnersReplyDataBlock {
                    owner_id: uuid::Uuid::from_u128(0x22),
                    is_group_owned: true,
                    count: 3,
                    online_status: false,
                },
            ],
        });
        session.handle_datagram(sim_addr(), &server_message(&reply, 9, true)?, now)?;

        let owners = drain_events(&mut session)
            .into_iter()
            .find_map(|e| match e {
                Event::ParcelObjectOwners { owners } => Some(owners),
                _ => None,
            })
            .ok_or("expected a ParcelObjectOwners event")?;
        assert_eq!(owners.len(), 2);
        let first = owners.first().ok_or("expected a first owner")?;
        assert_eq!(
            first.owner,
            sl_proto::OwnerKey::Agent(sl_proto::AgentKey::from(uuid::Uuid::from_u128(0x21)))
        );
        assert_eq!(first.count, 12);
        assert!(first.online_status);
        let second = owners.get(1).ok_or("expected a second owner")?;
        assert_eq!(
            second.owner,
            sl_proto::OwnerKey::Group(sl_proto::GroupKey::from(uuid::Uuid::from_u128(0x22)))
        );
        Ok(())
    }

    #[test]
    fn parcel_info_reply_surfaces_event() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        let reply = AnyMessage::ParcelInfoReply(ParcelInfoReply {
            agent_data: ParcelInfoReplyAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(1),
            },
            data: ParcelInfoReplyDataBlock {
                parcel_id: uuid::Uuid::from_u128(0x00C0_FFEE),
                owner_id: uuid::Uuid::from_u128(0x55),
                name: with_nul_bytes("Sunny Plaza"),
                desc: with_nul_bytes("A nice spot"),
                actual_area: 512,
                billable_area: 480,
                flags: 0x4,
                global_x: 256_000.0,
                global_y: 257_024.0,
                global_z: 23.5,
                sim_name: with_nul_bytes("Default Region"),
                snapshot_id: uuid::Uuid::from_u128(0x77),
                dwell: 88.0,
                sale_price: 1000,
                auction_id: 0,
            },
        });
        session.handle_datagram(sim_addr(), &server_message(&reply, 9, true)?, now)?;

        let details = drain_events(&mut session)
            .into_iter()
            .find_map(|e| match e {
                Event::ParcelDetails(details) => Some(details),
                _ => None,
            })
            .ok_or("expected a ParcelDetails event")?;
        assert_eq!(
            details.parcel_id,
            ParcelKey::from(uuid::Uuid::from_u128(0x00C0_FFEE))
        );
        assert_eq!(details.name, "Sunny Plaza");
        assert_eq!(details.sim_name, region_name("Default Region"));
        assert_eq!(details.actual_area, LandArea(512));
        assert_eq!(details.sale_price, Some(LindenAmount(1000)));
        assert_eq!(details.global_position.z().to_bits(), 23.5_f64.to_bits());
        Ok(())
    }

    #[test]
    fn remote_parcel_request_surfaces_parcel_id() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        let body = parse_llsd_xml(concat!(
            "<llsd><map><key>parcel_id</key>",
            "<uuid>00000000-0000-0000-0000-000000c0ffee</uuid></map></llsd>",
        ))?;
        session.handle_caps_event(sl_proto::CAP_REMOTE_PARCEL_REQUEST, &body, now)?;

        let parcel_id = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::RemoteParcelId(parcel_id) => Some(parcel_id),
                _ => None,
            })
            .ok_or("expected a RemoteParcelId event")?;
        assert_eq!(
            parcel_id,
            ParcelKey::from(uuid::Uuid::from_u128(0x00C0_FFEE))
        );
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
            OwnerKey::Agent(AgentKey::from(uuid::Uuid::from_u128(9))),
            now,
        )?;
        session.kick_estate_user(AgentKey::from(uuid::Uuid::from_u128(9)), now)?;
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
        session.god_kick_user(AgentKey::from(uuid::Uuid::from_u128(9)), "spam", now)?;
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

    #[test]
    fn estate_g8_commands_encode() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        let circuit = session.root_circuit_id().ok_or("no circuit")?;
        drain(&mut session)?;

        session.request_estate_covenant(now)?;
        session.request_telehub_info(now)?;
        session.connect_telehub(
            ScopedObjectId::new(circuit, sl_proto::RegionLocalObjectId(42)),
            now,
        )?;
        session.disconnect_telehub(now)?;
        session.add_telehub_spawn_point(
            ScopedObjectId::new(circuit, sl_proto::RegionLocalObjectId(43)),
            now,
        )?;
        session.remove_telehub_spawn_point(2, now)?;
        let sent = drain(&mut session)?;

        assert!(
            sent.iter()
                .any(|m| matches!(m, AnyMessage::EstateCovenantRequest(_))),
            "expected an EstateCovenantRequest"
        );

        let telehub: Vec<_> = sent
            .iter()
            .filter_map(|m| match m {
                AnyMessage::EstateOwnerMessage(message)
                    if trimmed(&message.method_data.method) == "telehub" =>
                {
                    Some(message)
                }
                _ => None,
            })
            .collect();
        // info ui, connect, delete, spawnpoint add, spawnpoint remove.
        assert_eq!(telehub.len(), 5);
        let at = |index: usize| telehub.get(index).ok_or("missing telehub command");
        let command =
            |index: usize| -> Result<String, TestError> { Ok(param_at(&at(index)?.param_list, 0)) };
        assert_eq!(command(0)?, "info ui");
        assert_eq!(command(1)?, "connect");
        assert_eq!(param_at(&at(1)?.param_list, 1), "42");
        assert_eq!(command(2)?, "delete");
        assert_eq!(command(3)?, "spawnpoint add");
        assert_eq!(param_at(&at(3)?.param_list, 1), "43");
        assert_eq!(command(4)?, "spawnpoint remove");
        assert_eq!(param_at(&at(4)?.param_list, 1), "2");
        Ok(())
    }

    #[test]
    fn estate_covenant_reply_surfaces_event() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        let reply = AnyMessage::EstateCovenantReply(EstateCovenantReply {
            data: EstateCovenantReplyDataBlock {
                covenant_id: uuid::Uuid::from_u128(0xC0FE),
                covenant_timestamp: 1_700_000_000,
                estate_name: with_nul_bytes("My Estate"),
                estate_owner_id: uuid::Uuid::from_u128(0x42),
            },
        });
        session.handle_datagram(sim_addr(), &server_message(&reply, 9, true)?, now)?;

        let covenant = drain_events(&mut session)
            .into_iter()
            .find_map(|e| match e {
                Event::EstateCovenant(covenant) => Some(covenant),
                _ => None,
            })
            .ok_or("expected an EstateCovenant event")?;
        assert_eq!(covenant.covenant_id, Some(uuid::Uuid::from_u128(0xC0FE)));
        assert_eq!(covenant.covenant_timestamp, 1_700_000_000);
        assert_eq!(covenant.estate_name, "My Estate");
        assert_eq!(covenant.estate_owner_id, uuid::Uuid::from_u128(0x42));
        Ok(())
    }

    #[test]
    fn telehub_info_surfaces_event() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        let info = AnyMessage::TelehubInfo(TelehubInfoMessage {
            telehub_block: TelehubInfoTelehubBlockBlock {
                object_id: uuid::Uuid::from_u128(0x7E1E),
                object_name: with_nul_bytes("Welcome Hub"),
                telehub_pos: Vector {
                    x: 128.0,
                    y: 129.0,
                    z: 25.0,
                },
                telehub_rot: Rotation {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    s: 1.0,
                },
            },
            spawn_point_block: vec![
                TelehubInfoSpawnPointBlockBlock {
                    spawn_point_pos: Vector {
                        x: 1.0,
                        y: 2.0,
                        z: 3.0,
                    },
                },
                TelehubInfoSpawnPointBlockBlock {
                    spawn_point_pos: Vector {
                        x: -4.0,
                        y: -5.0,
                        z: -6.0,
                    },
                },
            ],
        });
        session.handle_datagram(sim_addr(), &server_message(&info, 9, true)?, now)?;

        let telehub = drain_events(&mut session)
            .into_iter()
            .find_map(|e| match e {
                Event::TelehubInfo(telehub) => Some(telehub),
                _ => None,
            })
            .ok_or("expected a TelehubInfo event")?;
        assert_eq!(
            telehub.object_id,
            Some(ObjectKey::from(uuid::Uuid::from_u128(0x7E1E)))
        );
        assert_eq!(telehub.object_name, "Welcome Hub");
        assert_eq!(telehub.position.x.to_bits(), 128.0_f32.to_bits());
        assert_eq!(telehub.spawn_points.len(), 2);
        let second = telehub.spawn_points.get(1).ok_or("expected 2 spawns")?;
        assert_eq!(second.x.to_bits(), (-4.0_f32).to_bits());
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
        session.teleport_to(
            RegionHandle(handle),
            region_coords(128.0, 128.0, 30.0),
            vec3(1.0, 0.0, 0.0),
            now,
        )?;
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
                Event::RegionChanged {
                    region_handle, sim, ..
                } => Some((*region_handle, *sim)),
                _ => None,
            })
            .ok_or("expected a RegionChanged event")?;
        assert_eq!(changed.0, RegionHandle(handle));
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
        session.teleport_to(
            RegionHandle(handle),
            region_coords(128.0, 128.0, 30.0),
            vec3(1.0, 0.0, 0.0),
            now,
        )?;
        drain(&mut session)?;

        let failed = server_message(
            &AnyMessage::TeleportFailed(TeleportFailed {
                info: TeleportFailedInfoBlock {
                    agent_id: uuid::Uuid::from_u128(1),
                    reason: b"no access".to_vec(),
                },
                alert_info: vec![TeleportFailedAlertInfoBlock {
                    message: b"RegionEntryAccessBlocked".to_vec(),
                    extra_params: b"[REGION_NAME]=Foo".to_vec(),
                }],
            }),
            2,
            true,
        )?;
        session.handle_datagram(sim_addr(), &failed, now)?;
        let events = drain_events(&mut session);
        let alert = events
            .iter()
            .find_map(|e| match e {
                Event::TeleportFailed { reason, alert_info } => {
                    Some((reason.clone(), alert_info.clone()))
                }
                _ => None,
            })
            .ok_or("expected a TeleportFailed event")?;
        assert_eq!(alert.0, "no access");
        let info = alert.1.ok_or("expected an AlertInfo block")?;
        assert_eq!(info.message, "RegionEntryAccessBlocked");
        assert_eq!(info.extra_params, "[REGION_NAME]=Foo");

        // Back in the active state, a second teleport is accepted.
        session.teleport_to(
            RegionHandle(handle),
            region_coords(128.0, 128.0, 30.0),
            vec3(1.0, 0.0, 0.0),
            now,
        )?;
        Ok(())
    }

    #[test]
    fn teleport_times_out() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        session.teleport_to(
            RegionHandle(0x0003_E800_0003_E900),
            region_coords(128.0, 128.0, 30.0),
            vec3(1.0, 0.0, 0.0),
            now,
        )?;
        drain(&mut session)?;

        session.handle_timeout(after(now, 31_000)?);
        let events = drain_events(&mut session);
        let reason = events
            .iter()
            .find_map(|e| match e {
                Event::TeleportFailed { reason, .. } => Some(reason.clone()),
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

        session.set_draw_distance(Distance::new(512.0));
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
        assert_eq!(parcel.local_id, sl_proto::RegionLocalParcelId(3));
        assert_eq!(parcel.sequence_id, 9);
        assert_eq!(parcel.area, LandArea(2048));
        assert_eq!(parcel.max_prims, 750);
        assert_eq!(parcel.aabb_max.x().to_bits(), 32.0_f32.to_bits());
        assert_eq!(parcel.bitmap, vec![1u8, 2, 3]);
        // ParcelFlags 64 = CREATE_OBJECTS, decoded from the binary element.
        assert_eq!(parcel.raw_parcel_flags, 64);
        assert!(parcel.create_objects());
        // The stream / media URLs decode off the CAPS LLSD too.
        assert_eq!(
            parcel.music_url.as_ref().map(url::Url::as_str),
            Some("http://stream.example/audio")
        );
        assert_eq!(
            parcel.media_url.as_ref().map(url::Url::as_str),
            Some("http://example.com/movie")
        );
        assert_eq!(
            parcel.media_id,
            Some(TextureKey::from(uuid::Uuid::from_u128(0x33ED)))
        );
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
        let datagram = encode_datagram(PacketFlags::EMPTY, SequenceNumber(9), &body);
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
        assert_eq!(region.name, region_name("TestRegion"));
        assert_eq!(region.grid_coordinates.x(), 1000);
        assert_eq!(region.grid_coordinates.y(), 1001);
        assert_eq!(region.maturity, Maturity::Mature);
        assert_eq!(region.water_height, 20);
        assert_eq!(region.agents, 3);
        assert_eq!(region.region_handle, RegionHandle::from_grid(1000, 1001));
        Ok(())
    }

    /// A `MapLayerReply` surfaces as an [`Event::MapLayers`] with each tile's
    /// grid rectangle and texture, after the client sends `request_map_layer`.
    #[test]
    fn map_layer_reply_reports_tiles() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        session.request_map_layer(now)?;
        let sent = drain(&mut session)?;
        sent.iter()
            .find_map(|m| match m {
                AnyMessage::MapLayerRequest(request) => Some(request),
                _ => None,
            })
            .ok_or("expected a MapLayerRequest")?;

        let reply = AnyMessage::MapLayerReply(MapLayerReply {
            agent_data: MapLayerReplyAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(1),
                flags: 2,
            },
            layer_data: vec![MapLayerReplyLayerDataBlock {
                left: 0,
                right: 9999,
                top: 9999,
                bottom: 0,
                image_id: uuid::Uuid::from_u128(0xABCD),
            }],
        });
        session.handle_datagram(sim_addr(), &server_message(&reply, 9, true)?, now)?;

        let layers = drain_events(&mut session)
            .into_iter()
            .find_map(|e| match e {
                Event::MapLayers { layers } => Some(layers),
                _ => None,
            })
            .ok_or("expected a MapLayers event")?;
        assert_eq!(layers.len(), 1);
        let layer = layers.first().ok_or("one layer")?;
        assert_eq!(
            layer.rect,
            sl_proto::GridRectangle::new(
                sl_proto::GridCoordinates::new(0, 0),
                sl_proto::GridCoordinates::new(9999, 9999),
            )
        );
        assert_eq!(
            layer.image_id,
            TextureKey::from(uuid::Uuid::from_u128(0xABCD))
        );
        Ok(())
    }

    /// `send_abuse_report` encodes a `UserReport` carrying the report fields.
    #[test]
    fn abuse_report_encodes_user_report() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let report = AbuseReport {
            report_type: AbuseReportType::Complaint,
            category: 66,
            position: Vector {
                x: 128.0,
                y: 64.0,
                z: 22.0,
            },
            check_flags: 0,
            screenshot_id: uuid::Uuid::nil(),
            object_id: ObjectKey::from(uuid::Uuid::from_u128(0x22)),
            abuser_id: uuid::Uuid::from_u128(0x33),
            abuse_region_name: region_name("TestRegion"),
            abuse_region_id: uuid::Uuid::nil(),
            summary: "Griefing".to_owned(),
            details: "Detail".to_owned(),
            version_string: "7.1 Lnx".to_owned(),
        };
        session.send_abuse_report(&report, now)?;
        let sent = drain(&mut session)?;
        let message = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::UserReport(report) => Some(report),
                _ => None,
            })
            .ok_or("expected a UserReport")?;
        assert_eq!(message.report_data.report_type, 2);
        assert_eq!(message.report_data.category, 66);
        assert_eq!(message.report_data.abuser_id, uuid::Uuid::from_u128(0x33));
        assert_eq!(message.report_data.summary, b"Griefing\0");
        assert_eq!(message.report_data.details, b"Detail\0");
        Ok(())
    }

    /// `send_postcard` encodes a `SendPostcard` carrying the email fields.
    #[test]
    fn postcard_encodes_send_postcard() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let postcard = Postcard {
            asset_id: uuid::Uuid::from_u128(0x55),
            pos_global: GlobalCoordinates::new(256_128.0, 256_064.0, 22.0),
            to: "friend@example.com".to_owned(),
            from: "me@example.com".to_owned(),
            name: "Me".to_owned(),
            subject: "Hi".to_owned(),
            message: "Wish you were here".to_owned(),
            allow_publish: true,
            mature_publish: false,
        };
        session.send_postcard(&postcard, now)?;
        let sent = drain(&mut session)?;
        let message = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::SendPostcard(postcard) => Some(postcard),
                _ => None,
            })
            .ok_or("expected a SendPostcard")?;
        assert_eq!(message.agent_data.asset_id, uuid::Uuid::from_u128(0x55));
        assert_eq!(message.agent_data.to, b"friend@example.com\0");
        assert_eq!(message.agent_data.subject, b"Hi\0");
        assert!(message.agent_data.allow_publish);
        assert!(!message.agent_data.mature_publish);
        Ok(())
    }

    /// The CAPS `TeleportFinish` event body naming [`sim_b`] as the destination.
    fn caps_teleport_finish_xml() -> &'static str {
        // SimIP fwAAAQ== is base64 of [127, 0, 0, 1]; SimPort is a plain integer
        // (host order, no byte swap).
        // SimAccess 21 is Mature; TeleportFlags 12 is VIA_LURE (4) | VIA_LANDMARK (8).
        "<llsd><map><key>Info</key><array><map>\
            <key>SimIP</key><binary>fwAAAQ==</binary>\
            <key>SimPort</key><integer>9001</integer>\
            <key>SeedCapability</key><string>http://127.0.0.1:9001/seed</string>\
            <key>SimAccess</key><integer>21</integer>\
            <key>TeleportFlags</key><integer>12</integer>\
            </map></array></map></llsd>"
    }

    #[test]
    fn caps_teleport_finish_hands_over_to_destination() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        let handle = 0x0003_E800_0003_E900;
        session.teleport_to(
            RegionHandle(handle),
            region_coords(128.0, 128.0, 30.0),
            vec3(1.0, 0.0, 0.0),
            now,
        )?;
        let sent = drain(&mut session)?;
        assert!(
            sent.iter()
                .any(|m| matches!(m, AnyMessage::TeleportLocationRequest(_))),
            "expected a TeleportLocationRequest"
        );

        // OpenSim delivers TeleportFinish over the CAPS event queue.
        let body = sl_proto::parse_llsd_xml(caps_teleport_finish_xml())?;
        session.handle_caps_event("TeleportFinish", &body, now)?;

        // The CAPS path surfaces the destination maturity and teleport flags too.
        let finished = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::TeleportFinished {
                    region_handle,
                    sim,
                    maturity,
                    flags,
                } => Some((region_handle, sim, maturity, flags)),
                _ => None,
            })
            .ok_or("expected a TeleportFinished event from the CAPS path")?;
        assert_eq!(finished.0, RegionHandle(handle));
        assert_eq!(finished.1, sim_b());
        assert_eq!(finished.2, Maturity::Mature);
        assert!(finished.3.contains(TeleportFlags::VIA_LURE));
        assert!(finished.3.contains(TeleportFlags::VIA_LANDMARK));
        assert!(!finished.3.contains(TeleportFlags::IS_FLYING));

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
                Event::RegionChanged {
                    region_handle, sim, ..
                } => Some((*region_handle, *sim)),
                _ => None,
            })
            .ok_or("expected a RegionChanged event")?;
        assert_eq!(changed.0, RegionHandle(handle));
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
        session.teleport_to(
            RegionHandle(handle),
            region_coords(128.0, 128.0, 30.0),
            vec3(1.0, 0.0, 0.0),
            now,
        )?;
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
        let circuit = session.root_circuit_id().ok_or("no circuit")?;
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
        assert_eq!(object.local_id, sl_proto::RegionLocalObjectId(100));
        assert_eq!(
            object.full_id,
            ObjectKey::from(uuid::Uuid::from_u128(0xABCD))
        );
        assert_eq!(object.pcode, pcode::PRIMITIVE);
        assert_eq!(object.region_handle, RegionHandle(OBJ_REGION));
        assert_eq!(object.motion.position, position);
        assert_eq!(object.material, 3);
        // The path/profile shape is decoded from the full update's fields.
        assert_eq!(object.shape.path_curve, 16);
        assert_eq!(object.shape.profile_curve, 1);
        assert_eq!(object.shape.path_scale_x, 100);
        assert_eq!(object.shape.path_scale_y, 100);

        // The object is in the public cache.
        assert!(
            session
                .object(ScopedObjectId::new(
                    circuit,
                    sl_proto::RegionLocalObjectId(100)
                ))
                .is_some()
        );
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

    /// A scoped object id is only valid against the circuit it was learned on:
    /// the right circuit resolves the cached object and a send succeeds, while a
    /// stale/foreign circuit resolves to nothing and a send fails with
    /// [`Error::UnknownCircuit`] instead of silently targeting the wrong region.
    #[test]
    fn scoped_object_id_is_circuit_bound() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        let circuit = session.root_circuit_id().ok_or("no circuit")?;
        drain(&mut session)?;

        let update = object_update(
            100,
            0xABCD,
            Vector {
                x: 1.0,
                y: 2.0,
                z: 3.0,
            },
        );
        session.handle_datagram(sim_addr(), &server_message(&update, 5, true)?, now)?;
        let events = drain_events(&mut session);
        let Some(Event::ObjectAdded(object)) =
            events.iter().find(|e| matches!(e, Event::ObjectAdded(_)))
        else {
            return Err(format!("expected ObjectAdded, got {events:?}").into());
        };

        // The cached object carries the root circuit, and its scoped id resolves.
        let scoped = object.scoped_id();
        assert_eq!(scoped.circuit, circuit);
        assert_eq!(scoped.id, sl_proto::RegionLocalObjectId(100));
        assert!(session.object(scoped).is_some());

        // The same numeric id scoped to a *different* circuit instance (e.g. one
        // captured before a reconnect) resolves to nothing.
        let stale = ScopedObjectId::new(
            sl_proto::CircuitId(circuit.get().wrapping_add(1)),
            sl_proto::RegionLocalObjectId(100),
        );
        assert!(session.object(stale).is_none());

        // A send with the right circuit succeeds; with a stale circuit it fails
        // with `UnknownCircuit` rather than acting on the wrong region.
        session.touch_object(scoped, now)?;
        assert!(matches!(
            session.touch_object(stale, now),
            Err(sl_proto::Error::UnknownCircuit)
        ));
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
        // The sculpt-type byte is `0x05` (`LL_SCULPT_TYPE_MESH`), so the id is
        // typed as a mesh asset.
        assert_eq!(
            sculpt.texture,
            SculptOrMeshKey::Mesh(MeshKey::from(sculpt_tex))
        );
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
        assert!(probe.flags.contains(ReflectionProbeFlags::BOX_VOLUME));
        assert!(!probe.flags.contains(ReflectionProbeFlags::DYNAMIC));
        assert!(probe.flags.contains(ReflectionProbeFlags::MIRROR));
        Ok(())
    }

    #[test]
    fn object_update_decodes_texture_anim_and_particles() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // A 16-byte TextureAnim block: ON | ROTATE on all faces, 2×3 grid.
        let mut anim = Writer::new();
        anim.put_u8(0x01 | 0x20); // ON | ROTATE
        anim.put_i8(-1); // all faces
        anim.put_u8(2);
        anim.put_u8(3);
        anim.put_f32(0.0); // start
        anim.put_f32(5.0); // length
        anim.put_f32(4.0); // rate

        // An 86-byte legacy particle system (68-byte source + 18-byte particle).
        let part_image = uuid::Uuid::from_u128(0x9A1D);
        let target = uuid::Uuid::from_u128(0x7A26);
        let mut ps = Writer::new();
        ps.put_u32(0x00CA_FE00); // crc (non-zero)
        ps.put_u32(0x02); // flags: USE_NEW_ANGLE
        ps.put_u8(0x02); // pattern EXPLODE
        ps.put_u16(256); // max_age 1.0
        ps.put_u16(0); // start_age 0.0
        ps.put_u8(0); // inner_angle 0.0
        ps.put_u8(32); // outer_angle 1.0
        ps.put_u16(256); // burst_rate 1.0
        ps.put_u16(0); // burst_radius 0.0
        ps.put_u16(256); // burst_speed_min 1.0
        ps.put_u16(512); // burst_speed_max 2.0
        ps.put_u8(20); // burst_part_count
        for _ in 0..6 {
            ps.put_u16(0x8000); // angvel + accel = 0.0
        }
        ps.put_uuid(part_image);
        ps.put_uuid(target);
        // Legacy particle block.
        ps.put_u32(0x40); // part flags: TARGET_POS
        ps.put_u16(2560); // part_max_age 10.0
        ps.bytes(&[255, 255, 255, 255]); // start color
        ps.bytes(&[0, 0, 0, 0]); // end color
        ps.put_u8(32); // start scale x 1.0
        ps.put_u8(32); // start scale y 1.0
        ps.put_u8(0); // end scale x 0.0
        ps.put_u8(0); // end scale y 0.0

        let AnyMessage::ObjectUpdate(mut update) = object_update(310, 0xA117, zero_vec()) else {
            return Err("expected ObjectUpdate".into());
        };
        if let Some(block) = update.object_data.first_mut() {
            block.texture_anim = anim.into_bytes();
            block.ps_block = ps.into_bytes();
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

        let anim = object
            .texture_animation
            .as_ref()
            .ok_or("expected texture animation")?;
        assert_eq!(anim.mode, 0x01 | 0x20);
        assert_eq!(anim.face, -1);
        assert_eq!((anim.size_x, anim.size_y), (2, 3));
        assert!((anim.rate - 4.0).abs() < f32::EPSILON);

        let ps = object.particles.as_ref().ok_or("expected particles")?;
        assert_eq!(ps.crc, 0x00CA_FE00);
        assert_eq!(ps.pattern, 0x02);
        assert!((ps.max_age - 1.0).abs() < f32::EPSILON);
        assert!((ps.outer_angle - 1.0).abs() < f32::EPSILON);
        assert!((ps.burst_speed_max - 2.0).abs() < f32::EPSILON);
        assert_eq!(ps.burst_part_count, 20);
        assert_eq!(ps.texture_id, Some(TextureKey::from(part_image)));
        assert_eq!(ps.target_id, Some(ObjectKey::from(target)));
        assert_eq!(ps.part_flags, 0x40);
        assert!((ps.part_max_age - 10.0).abs() < f32::EPSILON);
        assert_eq!(ps.part_start_color, [255, 255, 255, 255]);
        assert!((ps.part_start_scale[0] - 1.0).abs() < f32::EPSILON);
        assert!((ps.part_start_scale[1] - 1.0).abs() < f32::EPSILON);
        Ok(())
    }

    #[test]
    fn terse_update_moves_known_object() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        let circuit = session.root_circuit_id().ok_or("no circuit")?;
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
            session
                .object(ScopedObjectId::new(
                    circuit,
                    sl_proto::RegionLocalObjectId(200)
                ))
                .map(|o| o.motion.position.clone()),
            Some(new_pos)
        );
        Ok(())
    }

    #[test]
    fn script_teleport_request_surfaces_options_flags() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        let msg = AnyMessage::ScriptTeleportRequest(ScriptTeleportRequest {
            data: ScriptTeleportRequestDataBlock {
                object_name: b"Beacon\0".to_vec(),
                sim_name: b"Foo\0".to_vec(),
                sim_position: Vector {
                    x: 128.0,
                    y: 64.0,
                    z: 30.0,
                },
                look_at: zero_vec(),
            },
            options: vec![ScriptTeleportRequestOptionsBlock { flags: 7 }],
        });
        session.handle_datagram(sim_addr(), &server_message(&msg, 5, true)?, now)?;
        let events = drain_events(&mut session);
        let request = events
            .iter()
            .find_map(|e| match e {
                Event::ScriptTeleport(request) => Some(request.clone()),
                _ => None,
            })
            .ok_or("expected a ScriptTeleport event")?;
        assert_eq!(request.region_name, region_name("Foo"));
        assert_eq!(request.object_name, "Beacon");
        assert_eq!(request.flags, 7);
        Ok(())
    }

    #[test]
    fn object_update_surfaces_time_dilation_on_change() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        // A half-dilation update emits a TimeDilation event for the region.
        let mut update = object_update(300, 0x1, zero_vec());
        if let AnyMessage::ObjectUpdate(message) = &mut update {
            message.region_data.time_dilation = 0x8000;
        }
        session.handle_datagram(sim_addr(), &server_message(&update, 5, true)?, now)?;
        let events = drain_events(&mut session);
        let dilation = events
            .iter()
            .find_map(|e| match e {
                Event::TimeDilation {
                    region_handle,
                    dilation,
                } => Some((*region_handle, *dilation)),
                _ => None,
            })
            .ok_or("expected a TimeDilation event")?;
        assert_eq!(dilation.0, RegionHandle(OBJ_REGION));
        // 0x8000 / 0xFFFF ≈ 0.5.
        assert!(
            (dilation.1 - 0.5).abs() < 1e-4,
            "unexpected dilation {}",
            dilation.1
        );

        // A second update with the *same* dilation does not re-emit.
        session.handle_datagram(sim_addr(), &server_message(&update, 6, true)?, now)?;
        let events = drain_events(&mut session);
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Event::TimeDilation { .. })),
            "an unchanged dilation must not re-emit, got {events:?}"
        );

        // A changed dilation emits again.
        let mut full = object_update(300, 0x1, zero_vec());
        if let AnyMessage::ObjectUpdate(message) = &mut full {
            message.region_data.time_dilation = 0xFFFF;
        }
        session.handle_datagram(sim_addr(), &server_message(&full, 7, true)?, now)?;
        let events = drain_events(&mut session);
        let again = events
            .iter()
            .find_map(|e| match e {
                Event::TimeDilation { dilation, .. } => Some(*dilation),
                _ => None,
            })
            .ok_or("expected a second TimeDilation event")?;
        assert!((again - 1.0).abs() < f32::EPSILON, "expected full dilation");
        Ok(())
    }

    #[test]
    fn object_update_surfaces_joint_fields() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        let pivot = Vector {
            x: 1.0,
            y: 2.0,
            z: 3.0,
        };
        let axis = Vector {
            x: 4.0,
            y: 5.0,
            z: 6.0,
        };
        let mut update = object_update(310, 0x2, zero_vec());
        if let AnyMessage::ObjectUpdate(message) = &mut update {
            let block = message.object_data.first_mut().ok_or("one block")?;
            block.joint_type = 2;
            block.joint_pivot = pivot.clone();
            block.joint_axis_or_anchor = axis.clone();
        }
        session.handle_datagram(sim_addr(), &server_message(&update, 5, true)?, now)?;
        let events = drain_events(&mut session);
        let Some(Event::ObjectAdded(object)) =
            events.iter().find(|e| matches!(e, Event::ObjectAdded(_)))
        else {
            return Err(format!("expected ObjectAdded, got {events:?}").into());
        };
        assert_eq!(object.joint_type, 2);
        assert_eq!(object.joint_pivot, pivot);
        assert_eq!(object.joint_axis_or_anchor, axis);
        Ok(())
    }

    #[test]
    fn terse_update_surfaces_avatar_collision_plane() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        let circuit = session.root_circuit_id().ok_or("no circuit")?;
        drain(&mut session)?;

        // Establish the object via a full update first.
        let update = object_update(320, 0x3, zero_vec());
        session.handle_datagram(sim_addr(), &server_message(&update, 5, true)?, now)?;
        drain_events(&mut session);

        // A terse blob *with* a collision plane (the has-plane flag set, then an
        // LLVector4) — as the simulator sends for an avatar.
        let plane = [0.0, 0.0, 1.0, 0.5];
        let mut writer = Writer::new();
        writer.put_u32(320);
        writer.put_u8(0);
        writer.put_u8(1); // has collision plane
        writer.put_vector4(plane);
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
        drain_events(&mut session);
        assert_eq!(
            session
                .object(ScopedObjectId::new(
                    circuit,
                    sl_proto::RegionLocalObjectId(320)
                ))
                .and_then(|o| o.motion.collision_plane),
            Some(plane)
        );
        // A plain prim (the full update above) carries no collision plane.
        let prim = object_update(321, 0x4, zero_vec());
        session.handle_datagram(sim_addr(), &server_message(&prim, 7, true)?, now)?;
        drain_events(&mut session);
        assert_eq!(
            session
                .object(ScopedObjectId::new(
                    circuit,
                    sl_proto::RegionLocalObjectId(321)
                ))
                .and_then(|o| o.motion.collision_plane),
            None
        );
        Ok(())
    }

    /// Builds a one-object `ObjectUpdate` for an avatar (`pcode::AVATAR`) with
    /// the given region-local id and full id, in the current region.
    fn avatar_update(local_id: u32, full_id: u128) -> AnyMessage {
        let AnyMessage::ObjectUpdate(mut update) = object_update(local_id, full_id, zero_vec())
        else {
            unreachable!("object_update builds an ObjectUpdate");
        };
        if let Some(block) = update.object_data.first_mut() {
            block.p_code = pcode::AVATAR;
        }
        AnyMessage::ObjectUpdate(update)
    }

    /// Builds an `ObjectUpdate` for an attachment prim worn on attachment point
    /// 1 (chest), parented to `parent_local_id` (our own avatar's region-local
    /// id, for the own-attachment classification used by `holder_kind`).
    fn attachment_update(local_id: u32, full_id: u128, parent_local_id: u32) -> AnyMessage {
        let AnyMessage::ObjectUpdate(mut update) = object_update(local_id, full_id, zero_vec())
        else {
            unreachable!("object_update builds an ObjectUpdate");
        };
        if let Some(block) = update.object_data.first_mut() {
            // State 0x10 decodes (nibble-swapped, per `ATTACHMENT_ID_FROM_STATE`)
            // to attachment point 1 (chest); a non-zero state marks an attachment.
            block.state = 0x10;
            block.parent_id = parent_local_id;
        }
        AnyMessage::ObjectUpdate(update)
    }

    #[test]
    fn own_avatar_id_learned_from_object_update() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        let circuit = session.root_circuit_id().ok_or("no circuit")?;
        drain(&mut session)?;

        // Not known until our own avatar object is seen.
        assert_eq!(session.own_avatar_id(), None);

        // The session's agent id is `from_u128(1)` (see `established`). An avatar
        // object whose full id matches ours fills the slot.
        let avatar = avatar_update(500, 1);
        session.handle_datagram(sim_addr(), &server_message(&avatar, 5, true)?, now)?;
        drain_events(&mut session);

        assert_eq!(
            session.own_avatar_id(),
            Some(ScopedObjectId::new(
                circuit,
                sl_proto::RegionLocalObjectId(500)
            ))
        );
        Ok(())
    }

    #[test]
    fn own_avatar_id_ignores_other_avatars_and_prims() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // Another avatar (different full id) is not ours.
        let other = avatar_update(510, 0x999);
        session.handle_datagram(sim_addr(), &server_message(&other, 5, true)?, now)?;
        drain_events(&mut session);
        assert_eq!(session.own_avatar_id(), None);

        // A prim carrying our own id (which never happens on the wire) is not an
        // avatar, so the `pcode` guard rejects it.
        let prim = object_update(511, 1, zero_vec());
        session.handle_datagram(sim_addr(), &server_message(&prim, 6, true)?, now)?;
        drain_events(&mut session);
        assert_eq!(session.own_avatar_id(), None);
        Ok(())
    }

    #[test]
    fn own_avatar_id_is_set_once() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        let circuit = session.root_circuit_id().ok_or("no circuit")?;
        drain(&mut session)?;

        let first = avatar_update(520, 1);
        session.handle_datagram(sim_addr(), &server_message(&first, 5, true)?, now)?;
        drain_events(&mut session);

        // A later own-avatar update with a different region-local id (the id is
        // really stable for a circuit's life) must not overwrite the slot.
        let again = avatar_update(521, 1);
        session.handle_datagram(sim_addr(), &server_message(&again, 6, true)?, now)?;
        drain_events(&mut session);

        assert_eq!(
            session.own_avatar_id(),
            Some(ScopedObjectId::new(
                circuit,
                sl_proto::RegionLocalObjectId(520)
            ))
        );
        Ok(())
    }

    #[test]
    fn own_avatar_id_backstop_at_movement_complete() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        let circuit = session.root_circuit_id().ok_or("no circuit")?;
        drain(&mut session)?;

        // Cache our own avatar object, then an `AgentMovementComplete` reads it
        // back from the cache (the message carries no region-local id) and fills
        // the slot — the backstop for attachment detection.
        let avatar = avatar_update(530, 1);
        session.handle_datagram(sim_addr(), &server_message(&avatar, 5, true)?, now)?;
        drain_events(&mut session);

        let amc = AnyMessage::AgentMovementComplete(AgentMovementComplete {
            agent_data: AgentMovementCompleteAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(1),
                session_id: uuid::Uuid::from_u128(2),
            },
            data: AgentMovementCompleteDataBlock {
                position: vec3(10.0, 128.0, 30.0),
                look_at: vec3(1.0, 0.0, 0.0),
                region_handle: OBJ_REGION,
                timestamp: 0,
            },
            sim_data: AgentMovementCompleteSimDataBlock {
                channel_version: b"x\0".to_vec(),
            },
        });
        session.handle_datagram(sim_addr(), &server_message(&amc, 6, true)?, now)?;
        drain_events(&mut session);

        assert_eq!(
            session.own_avatar_id(),
            Some(ScopedObjectId::new(
                circuit,
                sl_proto::RegionLocalObjectId(530)
            ))
        );
        Ok(())
    }

    #[test]
    fn answer_records_grant_and_empty_denies() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let task = ObjectKey::from(uuid::Uuid::from_u128(0xB201));
        let item = InventoryKey::from(uuid::Uuid::from_u128(0xB202));
        let granted =
            ScriptPermissions(ScriptPermissions::TRIGGER_ANIMATION | ScriptPermissions::TELEPORT);
        session.answer_script_permissions(task, item, granted, None, now)?;
        drain(&mut session)?;

        assert_eq!(session.granted_permissions(task, item), granted);
        let grant = session
            .script_grants()
            .next()
            .ok_or("expected one recorded grant")?;
        assert_eq!(session.script_grants().count(), 1);
        assert_eq!(grant.task_id, task);
        assert_eq!(grant.item_id, item);
        assert_eq!(grant.granted, granted);
        assert!(!grant.denied);
        // An unseen holder is classified in-world (the conservative default).
        assert!(!grant.is_attachment);
        assert_eq!(grant.experience_id, None);
        assert_eq!(
            session.script_permission_status(task, item),
            ScriptPermissionStatus::Granted(granted)
        );

        // An empty answer denies: the mirror records an explicit `Denied` entry
        // (distinct from never-asked), with no granted permissions.
        session.answer_script_permissions(task, item, ScriptPermissions(0), None, now)?;
        drain(&mut session)?;
        assert_eq!(
            session.granted_permissions(task, item),
            ScriptPermissions(0)
        );
        assert_eq!(
            session.script_permission_status(task, item),
            ScriptPermissionStatus::Denied
        );
        let denial = session
            .script_grants()
            .next()
            .ok_or("expected the denial recorded")?;
        assert_eq!(session.script_grants().count(), 1);
        assert!(denial.denied);
        assert_eq!(denial.granted, ScriptPermissions(0));
        Ok(())
    }

    #[test]
    fn re_grant_replaces_prior_grant() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let task = ObjectKey::from(uuid::Uuid::from_u128(0xB211));
        let item = InventoryKey::from(uuid::Uuid::from_u128(0xB212));
        let first = ScriptPermissions(ScriptPermissions::TAKE_CONTROLS);
        let second =
            ScriptPermissions(ScriptPermissions::TRIGGER_ANIMATION | ScriptPermissions::DEBIT);
        session.answer_script_permissions(task, item, first, None, now)?;
        session.answer_script_permissions(task, item, second, None, now)?;
        drain(&mut session)?;

        // A later answer for the same holder supersedes the earlier grant.
        assert_eq!(session.granted_permissions(task, item), second);
        assert_eq!(session.script_grants().count(), 1);
        Ok(())
    }

    #[test]
    fn revoke_clears_only_honoured_animation_bits() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let task = ObjectKey::from(uuid::Uuid::from_u128(0xB221));
        let item = InventoryKey::from(uuid::Uuid::from_u128(0xB222));
        let granted =
            ScriptPermissions(ScriptPermissions::TRIGGER_ANIMATION | ScriptPermissions::TELEPORT);
        session.answer_script_permissions(task, item, granted, None, now)?;
        drain(&mut session)?;

        // Revoking the full set only drops the honoured animation bit; the sim
        // keeps enforcing `TELEPORT`, so the conservative mirror keeps it.
        session.revoke_script_permissions(task, granted, now)?;
        drain(&mut session)?;
        assert_eq!(
            session.granted_permissions(task, item),
            ScriptPermissions(ScriptPermissions::TELEPORT)
        );

        // Revoking a non-honoured bit (`TELEPORT`) changes nothing.
        session.revoke_script_permissions(
            task,
            ScriptPermissions(ScriptPermissions::TELEPORT),
            now,
        )?;
        drain(&mut session)?;
        assert_eq!(
            session.granted_permissions(task, item),
            ScriptPermissions(ScriptPermissions::TELEPORT)
        );

        // A grant of only animation bits is removed entirely when revoked.
        let anim_task = ObjectKey::from(uuid::Uuid::from_u128(0xB223));
        let anim = ScriptPermissions(
            ScriptPermissions::TRIGGER_ANIMATION | ScriptPermissions::OVERRIDE_ANIMATIONS,
        );
        session.answer_script_permissions(anim_task, item, anim, None, now)?;
        drain(&mut session)?;
        session.revoke_script_permissions(anim_task, anim, now)?;
        drain(&mut session)?;
        assert_eq!(
            session.granted_permissions(anim_task, item),
            ScriptPermissions(0)
        );
        assert!(
            !session.script_grants().any(|g| g.task_id == anim_task),
            "a grant emptied by revoke must be removed"
        );
        Ok(())
    }

    #[test]
    fn teleport_drops_inworld_grants_keeps_attachment() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // Seed our own avatar so an attachment parented to it is recognised.
        let avatar = avatar_update(500, 1);
        session.handle_datagram(sim_addr(), &server_message(&avatar, 5, true)?, now)?;
        // An attachment worn by us, and a separate in-world prim.
        let attach = attachment_update(600, 0xA77, 500);
        session.handle_datagram(sim_addr(), &server_message(&attach, 6, true)?, now)?;
        let world = object_update(700, 0xB88, zero_vec());
        session.handle_datagram(sim_addr(), &server_message(&world, 7, true)?, now)?;
        drain_events(&mut session);
        drain(&mut session)?;

        let attach_task = ObjectKey::from(uuid::Uuid::from_u128(0xA77));
        let world_task = ObjectKey::from(uuid::Uuid::from_u128(0xB88));
        let item = InventoryKey::from(uuid::Uuid::from_u128(0x00C0_FFEE));
        let anim = ScriptPermissions(ScriptPermissions::TRIGGER_ANIMATION);
        session.answer_script_permissions(attach_task, item, anim, None, now)?;
        session.answer_script_permissions(world_task, item, anim, None, now)?;
        drain(&mut session)?;

        assert!(
            session
                .script_grants()
                .any(|g| g.task_id == attach_task && g.is_attachment),
            "the attachment holder should be classified as ours"
        );
        assert!(
            session
                .script_grants()
                .any(|g| g.task_id == world_task && !g.is_attachment),
            "the in-world holder should be classified in-world"
        );

        // A real (cross-region) teleport leaves the in-world object behind but
        // carries the attachment across the border.
        let handle = 0x0003_E900_0003_E800;
        session.teleport_to(
            RegionHandle(handle),
            region_coords(128.0, 128.0, 30.0),
            vec3(1.0, 0.0, 0.0),
            now,
        )?;
        drain(&mut session)?;
        drain_events(&mut session);
        let finish = AnyMessage::TeleportFinish(TeleportFinish {
            info: TeleportFinishInfoBlock {
                agent_id: uuid::Uuid::from_u128(1),
                location_id: 4,
                sim_ip: [127, 0, 0, 1],
                sim_port: 9100u16.swap_bytes(),
                region_handle: handle,
                seed_capability: b"http://x/permTP\0".to_vec(),
                sim_access: sl_wire::sim_access::MATURE,
                teleport_flags: TeleportFlags::VIA_LURE,
            },
        });
        session.handle_datagram(sim_addr(), &server_message(&finish, 20, true)?, now)?;

        assert_eq!(
            session.granted_permissions(world_task, item),
            ScriptPermissions(0),
            "the in-world grant should be dropped on a real teleport"
        );
        assert_eq!(
            session.granted_permissions(attach_task, item),
            anim,
            "the attachment grant should survive the teleport"
        );
        Ok(())
    }

    #[test]
    fn neighbour_crossing_keeps_grants() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        // Grant on a root in-world object.
        let task = ObjectKey::from(uuid::Uuid::from_u128(0xB231));
        let item = InventoryKey::from(uuid::Uuid::from_u128(0xB232));
        let obj = object_update(810, 0xB231, zero_vec());
        session.handle_datagram(sim_addr(), &server_message(&obj, 5, true)?, now)?;
        drain_events(&mut session);
        let anim = ScriptPermissions(ScriptPermissions::TRIGGER_ANIMATION);
        session.answer_script_permissions(task, item, anim, None, now)?;
        drain(&mut session)?;

        // Cross into a neighbour (promote the child to root) — grants are kept,
        // since an in-world object may still be visible and a vehicle crosses.
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
        while session.poll_transmit().is_some() {}
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
        drain_events(&mut session);

        assert_eq!(
            session.granted_permissions(task, item),
            anim,
            "a neighbour crossing should keep all grants"
        );
        Ok(())
    }

    #[test]
    fn disable_simulator_drops_child_circuit_grants() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        let item = InventoryKey::from(uuid::Uuid::from_u128(0xB242));
        let anim = ScriptPermissions(ScriptPermissions::TRIGGER_ANIMATION);

        // A grant on a root in-world object.
        let root_task = ObjectKey::from(uuid::Uuid::from_u128(0xB241));
        let root_obj = object_update(820, 0xB241, zero_vec());
        session.handle_datagram(sim_addr(), &server_message(&root_obj, 5, true)?, now)?;
        drain_events(&mut session);
        session.answer_script_permissions(root_task, item, anim, None, now)?;
        drain(&mut session)?;

        // A grant on an object cached on a child (neighbour) circuit.
        enable_neighbour_b(&mut session, 9, now)?;
        while session.poll_transmit().is_some() {}
        let child_task = ObjectKey::from(uuid::Uuid::from_u128(0xB243));
        let child_obj = object_update_in(0x0003_E900_0003_E800, 920, 0xB243, zero_vec());
        session.handle_datagram(sim_b(), &server_message(&child_obj, 1, true)?, now)?;
        drain_events(&mut session);
        session.answer_script_permissions(child_task, item, anim, None, now)?;
        drain(&mut session)?;
        assert!(
            session.script_grants().any(|g| g.task_id == child_task),
            "the child-circuit grant should be recorded"
        );

        // Retiring the child circuit drops only its grants.
        let disable = server_datagram(MessageId::Low(152), &[], 3, true);
        session.handle_datagram(sim_b(), &disable, now)?;
        assert_eq!(
            session.granted_permissions(child_task, item),
            ScriptPermissions(0),
            "the child-circuit grant should drop on DisableSimulator"
        );
        assert_eq!(
            session.granted_permissions(root_task, item),
            anim,
            "the root grant should be kept"
        );
        Ok(())
    }

    #[test]
    fn kill_object_drops_grant() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let task = ObjectKey::from(uuid::Uuid::from_u128(0x5678));
        let item = InventoryKey::from(uuid::Uuid::from_u128(0xB252));
        let obj = object_update(400, 0x5678, zero_vec());
        session.handle_datagram(sim_addr(), &server_message(&obj, 5, true)?, now)?;
        drain_events(&mut session);
        session.answer_script_permissions(
            task,
            item,
            ScriptPermissions(ScriptPermissions::TRIGGER_ANIMATION),
            None,
            now,
        )?;
        drain(&mut session)?;
        assert!(session.script_grants().any(|g| g.task_id == task));

        // The object going away (the detach path echoes a `KillObject` too) drops
        // its grant.
        let kill = AnyMessage::KillObject(KillObject {
            object_data: vec![KillObjectObjectDataBlock { id: 400 }],
        });
        session.handle_datagram(sim_addr(), &server_message(&kill, 6, true)?, now)?;
        drain_events(&mut session);
        assert_eq!(
            session.granted_permissions(task, item),
            ScriptPermissions(0),
            "the grant should drop when the object is killed"
        );
        Ok(())
    }

    #[test]
    fn never_asked_denied_and_granted_are_distinct() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let task = ObjectKey::from(uuid::Uuid::from_u128(0xB261));
        let item = InventoryKey::from(uuid::Uuid::from_u128(0xB262));

        // A holder no request has been answered for is never-asked, not denied.
        assert_eq!(
            session.script_permission_status(task, item),
            ScriptPermissionStatus::NeverAsked
        );

        // An empty answer records an explicit denial (distinct from never-asked).
        session.answer_script_permissions(task, item, ScriptPermissions(0), None, now)?;
        drain(&mut session)?;
        assert_eq!(
            session.script_permission_status(task, item),
            ScriptPermissionStatus::Denied
        );
        // `granted_permissions` cannot tell the two empty states apart.
        assert_eq!(
            session.granted_permissions(task, item),
            ScriptPermissions(0)
        );

        // A grant supersedes the denial; a later denial supersedes the grant —
        // one live state per script, the latest answer winning.
        let granted = ScriptPermissions(ScriptPermissions::TRIGGER_ANIMATION);
        session.answer_script_permissions(task, item, granted, None, now)?;
        drain(&mut session)?;
        assert_eq!(
            session.script_permission_status(task, item),
            ScriptPermissionStatus::Granted(granted)
        );
        assert_eq!(session.script_grants().count(), 1);

        session.answer_script_permissions(task, item, ScriptPermissions(0), None, now)?;
        drain(&mut session)?;
        assert_eq!(
            session.script_permission_status(task, item),
            ScriptPermissionStatus::Denied
        );
        assert_eq!(session.script_grants().count(), 1);
        Ok(())
    }

    #[test]
    fn teleport_drops_inworld_denial_keeps_attachment_denial() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // Seed our own avatar so an attachment parented to it is recognised.
        let avatar = avatar_update(500, 1);
        session.handle_datagram(sim_addr(), &server_message(&avatar, 5, true)?, now)?;
        let attach = attachment_update(600, 0xA78, 500);
        session.handle_datagram(sim_addr(), &server_message(&attach, 6, true)?, now)?;
        let world = object_update(700, 0xB89, zero_vec());
        session.handle_datagram(sim_addr(), &server_message(&world, 7, true)?, now)?;
        drain_events(&mut session);
        drain(&mut session)?;

        // Deny both holders (empty answers) — a denial is region-scoped exactly
        // like a grant: in-world is left behind, an attachment crosses.
        let attach_task = ObjectKey::from(uuid::Uuid::from_u128(0xA78));
        let world_task = ObjectKey::from(uuid::Uuid::from_u128(0xB89));
        let item = InventoryKey::from(uuid::Uuid::from_u128(0x00C0_FFEF));
        session.answer_script_permissions(attach_task, item, ScriptPermissions(0), None, now)?;
        session.answer_script_permissions(world_task, item, ScriptPermissions(0), None, now)?;
        drain(&mut session)?;
        assert_eq!(
            session.script_permission_status(attach_task, item),
            ScriptPermissionStatus::Denied
        );
        assert_eq!(
            session.script_permission_status(world_task, item),
            ScriptPermissionStatus::Denied
        );

        // A real teleport drops the in-world denial and keeps the attachment one.
        let handle = 0x0003_E900_0003_E800;
        session.teleport_to(
            RegionHandle(handle),
            region_coords(128.0, 128.0, 30.0),
            vec3(1.0, 0.0, 0.0),
            now,
        )?;
        drain(&mut session)?;
        drain_events(&mut session);
        let finish = AnyMessage::TeleportFinish(TeleportFinish {
            info: TeleportFinishInfoBlock {
                agent_id: uuid::Uuid::from_u128(1),
                location_id: 4,
                sim_ip: [127, 0, 0, 1],
                sim_port: 9100u16.swap_bytes(),
                region_handle: handle,
                seed_capability: b"http://x/permTPd\0".to_vec(),
                sim_access: sl_wire::sim_access::MATURE,
                teleport_flags: TeleportFlags::VIA_LURE,
            },
        });
        session.handle_datagram(sim_addr(), &server_message(&finish, 20, true)?, now)?;

        assert_eq!(
            session.script_permission_status(world_task, item),
            ScriptPermissionStatus::NeverAsked,
            "the in-world denial should be dropped on a real teleport"
        );
        assert_eq!(
            session.script_permission_status(attach_task, item),
            ScriptPermissionStatus::Denied,
            "the attachment denial should survive the teleport"
        );
        Ok(())
    }

    #[test]
    fn teleport_resets_grants_across_both_permission_stores() -> Result<(), TestError> {
        // The cross-cutting two-store case no single B-task owns: a real teleport
        // touches the grant registry (drop in-world, keep attachment) but must
        // leave the taken-controls tracker untouched (controls are agent-global
        // and the viewer keeps them across a teleport). Drive a grant *and* a
        // taken control through the same teleport and assert each store's rule.
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // Seed our own avatar, an attachment worn by us, and an in-world prim.
        let avatar = avatar_update(500, 1);
        session.handle_datagram(sim_addr(), &server_message(&avatar, 5, true)?, now)?;
        let attach = attachment_update(600, 0xA79, 500);
        session.handle_datagram(sim_addr(), &server_message(&attach, 6, true)?, now)?;
        let world = object_update(700, 0xB8A, zero_vec());
        session.handle_datagram(sim_addr(), &server_message(&world, 7, true)?, now)?;
        drain_events(&mut session);
        drain(&mut session)?;

        // Grant registry: one attachment grant (kept) and one in-world grant
        // (dropped). Both grant TAKE_CONTROLS so the registry and the live
        // taken-controls tracker are genuinely separate concerns here.
        let attach_task = ObjectKey::from(uuid::Uuid::from_u128(0xA79));
        let world_task = ObjectKey::from(uuid::Uuid::from_u128(0xB8A));
        let item = InventoryKey::from(uuid::Uuid::from_u128(0x00C0_FFF0));
        let granted = ScriptPermissions(ScriptPermissions::TAKE_CONTROLS);
        session.answer_script_permissions(attach_task, item, granted, None, now)?;
        session.answer_script_permissions(world_task, item, granted, None, now)?;

        // Taken-controls tracker: one consumed control and one passed to the
        // agent, so both halves of the tracker carry state across the teleport.
        feed_script_control_change(&mut session, now, 9501, true, ControlFlags::AT_POS, false)?;
        feed_script_control_change(&mut session, now, 9502, true, ControlFlags::UP_POS, true)?;
        drain(&mut session)?;
        assert_eq!(session.script_controls().taken, ControlFlags::AT_POS);
        assert_eq!(
            session.script_controls().passed_to_agent,
            ControlFlags::UP_POS
        );

        // A real (cross-region) teleport.
        let handle = 0x0003_E900_0003_E800;
        session.teleport_to(
            RegionHandle(handle),
            region_coords(128.0, 128.0, 30.0),
            vec3(1.0, 0.0, 0.0),
            now,
        )?;
        drain(&mut session)?;
        drain_events(&mut session);
        let finish = AnyMessage::TeleportFinish(TeleportFinish {
            info: TeleportFinishInfoBlock {
                agent_id: uuid::Uuid::from_u128(1),
                location_id: 4,
                sim_ip: [127, 0, 0, 1],
                sim_port: 9100u16.swap_bytes(),
                region_handle: handle,
                seed_capability: b"http://x/permTP2\0".to_vec(),
                sim_access: sl_wire::sim_access::MATURE,
                teleport_flags: TeleportFlags::VIA_LURE,
            },
        });
        session.handle_datagram(sim_addr(), &server_message(&finish, 20, true)?, now)?;

        // Grant registry: in-world dropped, attachment kept.
        assert_eq!(
            session.granted_permissions(world_task, item),
            ScriptPermissions(0),
            "the in-world grant should be dropped on a real teleport"
        );
        assert_eq!(
            session.granted_permissions(attach_task, item),
            granted,
            "the attachment grant should survive the teleport"
        );

        // Taken-controls tracker: untouched — a teleport clears only in-world
        // grants, never controls (the conservative-mirror invariant).
        assert_eq!(
            session.script_controls().taken,
            ControlFlags::AT_POS,
            "taken controls must survive a teleport (agent-global)"
        );
        assert_eq!(
            session.script_controls().passed_to_agent,
            ControlFlags::UP_POS,
            "passed-to-agent controls must survive a teleport too"
        );
        Ok(())
    }

    #[test]
    fn terse_update_applies_trailing_texture_entry() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        let circuit = session.root_circuit_id().ok_or("no circuit")?;
        drain(&mut session)?;

        // Establish the object via a full update (no texture entry).
        let update = object_update(210, 0x1234, zero_vec());
        session.handle_datagram(sim_addr(), &server_message(&update, 5, true)?, now)?;
        drain_events(&mut session);
        assert!(
            session
                .object(ScopedObjectId::new(
                    circuit,
                    sl_proto::RegionLocalObjectId(210)
                ))
                .is_some_and(|o| o.texture_entry.is_empty())
        );

        // A terse update flagged `Textures`: motion blob plus the trailing
        // `TextureEntry` field, which the simulator wraps as inner-length + two
        // zero bytes + the blob (the codec strips the outer field length).
        let mut writer = Writer::new();
        writer.put_u32(210);
        writer.put_u8(0);
        writer.put_u8(0);
        writer.put_vector3(&zero_vec());
        for _ in 0..(3 + 3 + 4 + 3) {
            writer.put_u16(0x8000);
        }

        // The bare TextureEntry blob: one nil default texture for all faces.
        let mut te = Writer::new();
        te.put_uuid(uuid::Uuid::nil());
        te.put_u8(0); // terminator for the texture field
        let te_blob = te.into_bytes();

        // Wrap it as the terse field: 2-byte inner length, two zero bytes, blob.
        let mut field = Writer::new();
        let inner_len = u16::try_from(te_blob.len())?;
        field.put_u16(inner_len);
        field.put_u8(0);
        field.put_u8(0);
        field.bytes(&te_blob);

        let terse = AnyMessage::ImprovedTerseObjectUpdate(ImprovedTerseObjectUpdate {
            region_data: ImprovedTerseObjectUpdateRegionDataBlock {
                region_handle: OBJ_REGION,
                time_dilation: 0xFFFF,
            },
            object_data: vec![ImprovedTerseObjectUpdateObjectDataBlock {
                data: writer.into_bytes(),
                texture_entry: field.into_bytes(),
            }],
        });
        session.handle_datagram(sim_addr(), &server_message(&terse, 6, true)?, now)?;
        let events = drain_events(&mut session);
        let Some(Event::ObjectUpdated(object)) =
            events.iter().find(|e| matches!(e, Event::ObjectUpdated(_)))
        else {
            return Err(format!("expected ObjectUpdated, got {events:?}").into());
        };
        // The unwrapped TextureEntry blob reached the object (event and cache).
        assert_eq!(object.texture_entry, te_blob);
        assert_eq!(
            session
                .object(ScopedObjectId::new(
                    circuit,
                    sl_proto::RegionLocalObjectId(210)
                ))
                .map(|o| o.texture_entry.clone()),
            Some(te_blob)
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
        let circuit = session.root_circuit_id().ok_or("no circuit")?;
        drain(&mut session)?;

        let update = object_update(400, 0x5678, zero_vec());
        session.handle_datagram(sim_addr(), &server_message(&update, 5, true)?, now)?;
        drain_events(&mut session);
        assert!(
            session
                .object(ScopedObjectId::new(
                    circuit,
                    sl_proto::RegionLocalObjectId(400)
                ))
                .is_some()
        );

        let kill = AnyMessage::KillObject(KillObject {
            object_data: vec![KillObjectObjectDataBlock { id: 400 }],
        });
        session.handle_datagram(sim_addr(), &server_message(&kill, 6, true)?, now)?;
        let events = drain_events(&mut session);
        let removed = events.iter().find_map(|e| match e {
            Event::ObjectRemoved {
                local_id,
                region_handle,
            } => Some((local_id.id, *region_handle)),
            _ => None,
        });
        assert_eq!(
            removed,
            Some((sl_proto::RegionLocalObjectId(400), RegionHandle(OBJ_REGION)))
        );
        assert!(
            session
                .object(ScopedObjectId::new(
                    circuit,
                    sl_proto::RegionLocalObjectId(400)
                ))
                .is_none()
        );
        Ok(())
    }

    #[test]
    fn object_properties_surface_and_merge() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        let circuit = session.root_circuit_id().ok_or("no circuit")?;
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
                aggregate_perms: 0x0E,
                aggregate_perm_textures: 0x0F,
                aggregate_perm_textures_owner: 0x0D,
                category: 0,
                inventory_serial: 7,
                item_id: uuid::Uuid::from_u128(0x44),
                folder_id: uuid::Uuid::from_u128(0x55),
                from_task_id: uuid::Uuid::from_u128(0x66),
                last_owner_id: uuid::Uuid::from_u128(0x33),
                name: b"Test Prim\0".to_vec(),
                description: b"a description\0".to_vec(),
                touch_name: Vec::new(),
                sit_name: Vec::new(),
                texture_id: {
                    let mut blob = uuid::Uuid::from_u128(0x77).as_bytes().to_vec();
                    blob.extend_from_slice(uuid::Uuid::from_u128(0x88).as_bytes());
                    blob
                },
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
        // Recovered ObjectData fields (#36).
        assert_eq!(properties.inventory_serial, 7);
        assert_eq!(
            properties.item_id,
            InventoryKey::from(uuid::Uuid::from_u128(0x44))
        );
        assert_eq!(
            properties.folder_id,
            Some(InventoryFolderKey::from(uuid::Uuid::from_u128(0x55)))
        );
        assert_eq!(
            properties.from_task_id,
            Some(ObjectKey::from(uuid::Uuid::from_u128(0x66)))
        );
        assert_eq!(properties.aggregate_perms, 0x0E);
        assert_eq!(properties.aggregate_perm_textures, 0x0F);
        assert_eq!(properties.aggregate_perm_textures_owner, 0x0D);
        assert_eq!(
            properties.texture_ids,
            vec![
                TextureKey::from(uuid::Uuid::from_u128(0x77)),
                TextureKey::from(uuid::Uuid::from_u128(0x88))
            ]
        );
        // Merged into the cached object.
        assert_eq!(
            session
                .object(ScopedObjectId::new(
                    circuit,
                    sl_proto::RegionLocalObjectId(500)
                ))
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
        assert_eq!(object.local_id, sl_proto::RegionLocalObjectId(600));
        assert_eq!(
            object.full_id,
            ObjectKey::from(uuid::Uuid::from_u128(0xDEAD))
        );
        assert_eq!(object.crc, 99);
        assert_eq!(object.owner_id, uuid::Uuid::from_u128(0x44));
        assert_eq!(object.motion.position, position);
        Ok(())
    }

    #[test]
    fn compressed_update_decodes_trailing_fields() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let position = Vector {
            x: 10.0,
            y: 20.0,
            z: 30.0,
        };
        let sound_id = uuid::Uuid::from_u128(0x5011);
        let te_bytes: Vec<u8> = vec![0xAA, 0xBB, 0xCC, 0xDD, 0xEE];

        // Fixed prefix.
        let mut writer = Writer::new();
        writer.put_uuid(uuid::Uuid::from_u128(0xDEAD));
        writer.put_u32(700);
        writer.put_u8(pcode::PRIMITIVE);
        writer.put_u8(0);
        writer.put_u32(99);
        writer.put_u8(3);
        writer.put_u8(0);
        writer.put_vector3(&Vector {
            x: 1.0,
            y: 2.0,
            z: 3.0,
        });
        writer.put_vector3(&position);
        writer.put_quaternion(&Rotation {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            s: 1.0,
        });
        // text | legacy-particles | sound | texture-anim | name-values | media-url
        let cflags: u32 = 0x04 | 0x08 | 0x10 | 0x40 | 0x100 | 0x200;
        writer.put_u32(cflags);
        writer.put_uuid(uuid::Uuid::from_u128(0x44));
        // Floating text (NUL-terminated) + RGBA colour.
        writer.bytes(b"hello\0");
        writer.bytes(&[10, 20, 30, 200]);
        // Media URL (NUL-terminated).
        writer.bytes(b"http://example/\0");
        // Legacy particle system: a fixed 86-byte block (a recognisable fill).
        let particle_bytes: Vec<u8> = (0..86_u8).collect();
        writer.bytes(&particle_bytes);
        // ExtraParams: one light parameter (RGBA + radius/cutoff/falloff = 16 bytes).
        let extra_params_start = writer.as_bytes().len();
        writer.put_u8(1);
        writer.put_u16(0x20);
        writer.put_u32(16);
        writer.bytes(&[255, 128, 64, 255]);
        writer.put_f32(5.0);
        writer.put_f32(0.5);
        writer.put_f32(1.0);
        let extra_params: Vec<u8> = writer
            .as_bytes()
            .get(extra_params_start..)
            .ok_or("extra-params slice")?
            .to_vec();
        // Attached sound: id, gain, flags, radius.
        writer.put_uuid(sound_id);
        writer.put_f32(0.75);
        writer.put_u8(0x01);
        writer.put_f32(20.0);
        // Name-value pairs (NUL-terminated).
        writer.bytes(b"AttachItemID STRING RW SV abc\0");
        // Path+profile shape: a fixed 23-byte block (path block then profile
        // block, in the simulator's pack order).
        writer.put_u8(0x10); // path_curve
        writer.put_u16(100); // path_begin
        writer.put_u16(200); // path_end
        writer.put_u8(50); // path_scale_x
        writer.put_u8(60); // path_scale_y
        writer.put_u8(0); // path_shear_x
        writer.put_u8(0); // path_shear_y
        writer.put_i8(0); // path_twist
        writer.put_i8(0); // path_twist_begin
        writer.put_i8(0); // path_radius_offset
        writer.put_i8(0); // path_taper_x
        writer.put_i8(0); // path_taper_y
        writer.put_u8(0); // path_revolutions
        writer.put_i8(0); // path_skew
        writer.put_u8(0x01); // profile_curve
        writer.put_u16(300); // profile_begin
        writer.put_u16(400); // profile_end
        writer.put_u16(500); // profile_hollow
        // Packed texture entry: a little-endian u32 length then that many bytes.
        writer.put_u32(u32::try_from(te_bytes.len())?);
        writer.bytes(&te_bytes);
        // Texture animation: a u32 length then that many bytes.
        let texture_anim: Vec<u8> = vec![1, 2, 3, 4];
        writer.put_u32(u32::try_from(texture_anim.len())?);
        writer.bytes(&texture_anim);

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
        session.handle_datagram(sim_addr(), &server_message(&compressed, 6, true)?, now)?;
        let events = drain_events(&mut session);
        let Some(Event::ObjectAdded(object)) =
            events.iter().find(|e| matches!(e, Event::ObjectAdded(_)))
        else {
            return Err(format!("expected ObjectAdded from compressed, got {events:?}").into());
        };
        assert_eq!(object.local_id, sl_proto::RegionLocalObjectId(700));
        assert_eq!(object.text, "hello");
        assert_eq!(object.text_color, [10, 20, 30, 200]);
        assert_eq!(
            object.media_url.as_ref().map(url::Url::as_str),
            Some("http://example/")
        );
        assert_eq!(object.sound, sound_id);
        assert!((object.gain - 0.75).abs() < f32::EPSILON);
        assert_eq!(object.sound_flags, 0x01);
        assert!((object.sound_radius - 20.0).abs() < f32::EPSILON);
        assert_eq!(object.name_value, "AttachItemID STRING RW SV abc");
        assert_eq!(object.texture_entry, te_bytes);
        assert_eq!(object.texture_anim, texture_anim);
        assert_eq!(object.particle_system, particle_bytes);
        assert_eq!(object.extra_params, extra_params);
        let light = object.extra.light.as_ref().ok_or("decoded light param")?;
        assert!((light.radius - 5.0).abs() < f32::EPSILON);
        // The path/profile shape decoded in the simulator's pack order.
        assert_eq!(object.shape.path_curve, 0x10);
        assert_eq!(object.shape.path_begin, 100);
        assert_eq!(object.shape.path_end, 200);
        assert_eq!(object.shape.path_scale_x, 50);
        assert_eq!(object.shape.path_scale_y, 60);
        assert_eq!(object.shape.profile_curve, 0x01);
        assert_eq!(object.shape.profile_begin, 300);
        assert_eq!(object.shape.profile_end, 400);
        assert_eq!(object.shape.profile_hollow, 500);
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
        let circuit = session.root_circuit_id().ok_or("no circuit")?;
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
        assert_eq!(object.local_id, sl_proto::RegionLocalObjectId(700));
        assert_eq!(object.region_handle, RegionHandle(NB_REGION));

        // It lives in the neighbour region's set; the root-region `object()`
        // lookup does not see it (local ids share a numeric space across regions).
        assert_eq!(
            session.objects_in_region(RegionHandle(NB_REGION)).count(),
            1
        );
        assert!(
            session
                .object(ScopedObjectId::new(
                    circuit,
                    sl_proto::RegionLocalObjectId(700)
                ))
                .is_none()
        );
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
                    local_id,
                    region_handle,
                } if local_id.id == sl_proto::RegionLocalObjectId(700)
                    && *region_handle == RegionHandle(NB_REGION)
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
        session.rez_object(&PrimShape::cube(position.clone()), None, now)?;
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
        let circuit = session.root_circuit_id().ok_or("no circuit")?;
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
            ScopedObjectId::new(circuit, sl_proto::RegionLocalObjectId(42)),
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
        let circuit = session.root_circuit_id().ok_or("no circuit")?;
        drain(&mut session)?;

        session.set_object_scale(
            ScopedObjectId::new(circuit, sl_proto::RegionLocalObjectId(7)),
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
        let circuit = session.root_circuit_id().ok_or("no circuit")?;
        drain(&mut session)?;

        session.touch_object(
            ScopedObjectId::new(circuit, sl_proto::RegionLocalObjectId(55)),
            now,
        )?;
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
        let circuit = session.root_circuit_id().ok_or("no circuit")?;
        drain(&mut session)?;

        session.set_object_name(
            ScopedObjectId::new(circuit, sl_proto::RegionLocalObjectId(9)),
            "Vendor",
            now,
        )?;
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
        let circuit = session.root_circuit_id().ok_or("no circuit")?;
        drain(&mut session)?;

        session.delete_objects(
            &[
                ScopedObjectId::new(circuit, sl_proto::RegionLocalObjectId(11)),
                ScopedObjectId::new(circuit, sl_proto::RegionLocalObjectId(12)),
            ],
            now,
        )?;
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
        let circuit = session.root_circuit_id().ok_or("no circuit")?;
        drain(&mut session)?;

        let folder = uuid::Uuid::from_u128(0xF0_1DE2);
        session.derez_objects(
            &[ScopedObjectId::new(
                circuit,
                sl_proto::RegionLocalObjectId(21),
            )],
            DeRezDestination::TakeIntoAgentInventory(InventoryFolderKey::from(folder)),
            TransactionId::from(uuid::Uuid::from_u128(0x7)),
            None,
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
        let circuit = session.root_circuit_id().ok_or("no circuit")?;
        drain(&mut session)?;

        // PERM_COPY = 0x8000 in the LSL permission flags.
        session.set_object_permissions(
            &[ScopedObjectId::new(
                circuit,
                sl_proto::RegionLocalObjectId(31),
            )],
            PermissionField::NextOwner,
            false,
            0x8000,
            now,
        )?;
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
        let circuit = session.root_circuit_id().ok_or("no circuit")?;
        drain(&mut session)?;

        session.link_objects(
            &[
                ScopedObjectId::new(circuit, sl_proto::RegionLocalObjectId(100)),
                ScopedObjectId::new(circuit, sl_proto::RegionLocalObjectId(101)),
                ScopedObjectId::new(circuit, sl_proto::RegionLocalObjectId(102)),
            ],
            now,
        )?;
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
        let circuit = session.root_circuit_id().ok_or("no circuit")?;
        drain(&mut session)?;

        session.set_object_click_action(
            ScopedObjectId::new(circuit, sl_proto::RegionLocalObjectId(1)),
            ClickAction::Buy,
            now,
        )?;
        session.set_object_material(
            ScopedObjectId::new(circuit, sl_proto::RegionLocalObjectId(1)),
            Material::Metal,
            now,
        )?;
        session.set_object_for_sale(
            ScopedObjectId::new(circuit, sl_proto::RegionLocalObjectId(1)),
            SaleType::Copy,
            Some(LindenAmount(250)),
            now,
        )?;
        session.set_object_flags(
            ScopedObjectId::new(circuit, sl_proto::RegionLocalObjectId(1)),
            &ObjectFlagSettings {
                use_physics: true,
                is_phantom: true,
                ..ObjectFlagSettings::default()
            },
            now,
        )?;
        session.set_object_include_in_search(
            ScopedObjectId::new(circuit, sl_proto::RegionLocalObjectId(1)),
            true,
            now,
        )?;
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
        assert_eq!(patch.region_handle, RegionHandle(OBJ_REGION));
        assert!((patch.value(0, 0).ok_or("cell 0,0")? - 26.0).abs() < 1e-3);

        // It is in the public cache, addressable by region-local cell. Patch
        // (1, 2) covers region cells x in 16..32, y in 32..48.
        let height = session.terrain_height(20, 40).ok_or("height at (20,40)")?;
        assert!((height - 26.0).abs() < 1e-3, "height {height} != 26.0");
        assert_eq!(session.terrain_patches().count(), 1);
        assert_eq!(
            session
                .terrain_patches_in_region(RegionHandle(OBJ_REGION))
                .count(),
            1
        );
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
            info.account_server_name.as_ref().map(url::Url::as_str),
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
        assert_eq!(info.parcel_local_id, sl_proto::RegionLocalParcelId(7));
        assert_eq!(info.region_name, region_name("Default Region"));
        assert_eq!(
            info.channel_uri.as_ref().map(url::Url::as_str),
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

    /// A `GetDisplayNames` reply surfaces the requested agents' display names,
    /// with an unresolved id folded in as a `missing` placeholder.
    #[test]
    fn get_display_names_surfaces_records() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let xml = "<llsd><map><key>agents</key><array><map>\
            <key>id</key><uuid>11111111-1111-1111-1111-111111111111</uuid>\
            <key>username</key><string>james.linden</string>\
            <key>display_name</key><string>James the Great</string>\
            <key>legacy_first_name</key><string>James</string>\
            <key>legacy_last_name</key><string>Linden</string>\
            <key>is_display_name_default</key><boolean>false</boolean>\
            </map></array>\
            <key>bad_ids</key><array>\
            <uuid>22222222-2222-2222-2222-222222222222</uuid></array></map></llsd>";
        let body = sl_proto::parse_llsd_xml(xml)?;
        session.handle_caps_event(sl_proto::CAP_GET_DISPLAY_NAMES, &body, now)?;

        let event = drain_events(&mut session)
            .into_iter()
            .find(|event| matches!(event, Event::DisplayNames(_)))
            .ok_or("expected a DisplayNames event")?;
        let Event::DisplayNames(names) = event else {
            return Err("expected DisplayNames".into());
        };
        let [first, second] = names.as_slice() else {
            return Err("expected two display-name records".into());
        };
        assert_eq!(first.username, "james.linden");
        assert_eq!(first.display_name, "James the Great");
        assert_eq!(first.legacy_name(), "James Linden");
        assert!(!first.missing);
        assert!(second.missing);
        Ok(())
    }

    /// A `SimulatorFeatures` GET reply surfaces the region's feature flags,
    /// including the OpenSim-only grid extras when present.
    #[test]
    fn simulator_features_surfaces_flags() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        let features = sl_proto::SimulatorFeatures {
            mesh_upload_enabled: Some(true),
            physics_materials_enabled: Some(true),
            max_agent_attachments: Some(38),
            open_sim_extras: Some(sl_proto::OpenSimExtras {
                say_range: Some(20),
                currency: Some("OS$".to_owned()),
                ..sl_proto::OpenSimExtras::default()
            }),
            ..sl_proto::SimulatorFeatures::default()
        };
        let xml = sl_proto::build_simulator_features_response(&features);
        let body = sl_proto::parse_llsd_xml(&xml)?;
        session.handle_caps_event(sl_proto::CAP_SIMULATOR_FEATURES, &body, now)?;

        let event = drain_events(&mut session)
            .into_iter()
            .find(|event| matches!(event, Event::SimulatorFeatures(_)))
            .ok_or("expected a SimulatorFeatures event")?;
        let Event::SimulatorFeatures(decoded) = event else {
            return Err("expected SimulatorFeatures".into());
        };
        assert_eq!(decoded.mesh_upload_enabled, Some(true));
        assert_eq!(decoded.physics_materials_enabled, Some(true));
        assert_eq!(decoded.max_agent_attachments, Some(38));
        // A flag the reply omitted stays `None` (not advertised), not `Some(false)`.
        assert_eq!(decoded.gltf_enabled, None);
        let extras = decoded.open_sim_extras.ok_or("expected OpenSim extras")?;
        assert_eq!(extras.say_range, Some(20));
        assert_eq!(extras.currency, Some("OS$".to_owned()));
        Ok(())
    }

    /// An `AgentPreferences` POST reply surfaces the agent's full stored
    /// preferences (echoed by the grid after the update).
    #[test]
    fn agent_preferences_surfaces_stored_set() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        let prefs = sl_proto::AgentPreferences {
            hover_height: Some(0.5),
            default_object_perm_masks: Some(sl_proto::ObjectPermMasks {
                group: 0,
                everyone: 0,
                next_owner: 0x0008_2000,
            }),
            max_access_pref: Some("M".to_owned()),
            language: Some("en-us".to_owned()),
            language_is_public: Some(true),
            god_level: Some(0),
        };
        let xml = sl_proto::build_agent_preferences_response(&prefs);
        let body = sl_proto::parse_llsd_xml(&xml)?;
        session.handle_caps_event(sl_proto::CAP_AGENT_PREFERENCES, &body, now)?;

        let event = drain_events(&mut session)
            .into_iter()
            .find(|event| matches!(event, Event::AgentPreferences(_)))
            .ok_or("expected an AgentPreferences event")?;
        let Event::AgentPreferences(decoded) = event else {
            return Err("expected AgentPreferences".into());
        };
        assert_eq!(
            decoded.hover_height.map(f64::to_bits),
            Some(0.5_f64.to_bits())
        );
        assert_eq!(
            decoded
                .default_object_perm_masks
                .map(|masks| masks.next_owner),
            Some(0x0008_2000)
        );
        assert_eq!(decoded.max_access_pref.as_deref(), Some("M"));
        assert_eq!(decoded.language.as_deref(), Some("en-us"));
        assert_eq!(decoded.language_is_public, Some(true));
        Ok(())
    }

    /// A `GetObjectCost` reply surfaces the per-object land-impact / physics
    /// costs, keyed and sorted by object id.
    #[test]
    fn object_cost_surfaces_costs() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        let id = uuid::Uuid::from_u128(0x000B_7EC0);
        let costs = vec![(
            ObjectKey::from(id),
            sl_proto::ObjectCost {
                linked_set_resource_cost: 12.5,
                resource_cost: 3.5,
                physics_cost: 1.0,
                linked_set_physics_cost: 4.0,
                resource_limiting_type: "legacy".to_owned(),
            },
        )];
        let xml = sl_proto::build_get_object_cost_response(&costs);
        let body = sl_proto::parse_llsd_xml(&xml)?;
        session.handle_caps_event(sl_proto::CAP_GET_OBJECT_COST, &body, now)?;

        let event = drain_events(&mut session)
            .into_iter()
            .find(|event| matches!(event, Event::ObjectCosts(_)))
            .ok_or("expected an ObjectCosts event")?;
        let Event::ObjectCosts(decoded) = event else {
            return Err("expected ObjectCosts".into());
        };
        assert_eq!(decoded.len(), 1);
        assert_eq!(
            decoded.first().map(|entry| entry.0),
            Some(ObjectKey::from(id))
        );
        assert_eq!(
            decoded
                .first()
                .map(|entry| entry.1.linked_set_resource_cost.to_bits()),
            Some(12.5_f32.to_bits())
        );
        Ok(())
    }

    /// A `GetObjectPhysicsData` reply surfaces the per-object physics material
    /// parameters, and an `ObjectPhysicsProperties` event-queue push surfaces the
    /// same data keyed by region-local id.
    #[test]
    fn object_physics_surfaces_data() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        let id = uuid::Uuid::from_u128(0x0B7E_DA7A);
        let data = sl_proto::ObjectPhysicsData {
            physics_shape_type: sl_proto::PhysicsShapeType::ConvexHull,
            density: 1000.0,
            friction: 0.6,
            restitution: 0.5,
            gravity_multiplier: 1.0,
        };
        let xml = sl_proto::build_get_object_physics_data_response(&[(ObjectKey::from(id), data)]);
        let body = sl_proto::parse_llsd_xml(&xml)?;
        session.handle_caps_event(sl_proto::CAP_GET_OBJECT_PHYSICS_DATA, &body, now)?;

        let decoded = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::ObjectPhysicsData(data) => Some(data),
                _ => None,
            })
            .ok_or("expected an ObjectPhysicsData event")?;
        assert_eq!(
            decoded.first().map(|entry| entry.0),
            Some(ObjectKey::from(id))
        );
        assert_eq!(
            decoded.first().map(|entry| entry.1.physics_shape_type),
            Some(sl_proto::PhysicsShapeType::ConvexHull)
        );

        let eq_body =
            sl_proto::build_object_physics_properties(&[(sl_proto::RegionLocalObjectId(42), data)])
                .to_llsd_xml();
        let eq = sl_proto::parse_llsd_xml(&eq_body)?;
        session.handle_caps_event("ObjectPhysicsProperties", &eq, now)?;
        let pushed = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::ObjectPhysicsProperties(data) => Some(data),
                _ => None,
            })
            .ok_or("expected an ObjectPhysicsProperties event")?;
        assert_eq!(
            pushed.first().map(|entry| entry.0.id),
            Some(sl_proto::RegionLocalObjectId(42))
        );
        Ok(())
    }

    /// An `AttachmentResources` reply surfaces the agent's scripted attachments
    /// and the resource summary.
    #[test]
    fn attachment_resources_surfaces_report() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        let report = sl_proto::AttachmentResourcesReport {
            attachments: vec![sl_proto::AttachmentLocation {
                location: "Right Hand".to_owned(),
                objects: vec![sl_proto::ScriptedObjectInfo {
                    id: uuid::Uuid::from_u128(0xA77),
                    location: RegionCoordinates::new(1.0, 2.0, 3.0),
                    name: "HUD".to_owned(),
                    owner: sl_proto::OwnerKey::Agent(sl_proto::AgentKey::from(
                        uuid::Uuid::from_u128(0x0411),
                    )),
                    resources: sl_proto::ScriptedObjectResources {
                        memory: Some(0x1_0000),
                        urls: Some(1),
                    },
                }],
            }],
            summary: sl_proto::ResourceSummary {
                available: vec![sl_proto::ResourceAmount {
                    resource_type: "urls".to_owned(),
                    amount: 38,
                }],
                used: vec![sl_proto::ResourceAmount {
                    resource_type: "memory".to_owned(),
                    amount: 0x1_0000,
                }],
            },
        };
        let xml = sl_proto::build_attachment_resources_response(&report);
        let body = sl_proto::parse_llsd_xml(&xml)?;
        session.handle_caps_event(sl_proto::CAP_ATTACHMENT_RESOURCES, &body, now)?;

        let decoded = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::AttachmentResources(report) => Some(report),
                _ => None,
            })
            .ok_or("expected an AttachmentResources event")?;
        assert_eq!(
            decoded.attachments.first().map(|a| a.location.as_str()),
            Some("Right Hand")
        );
        assert_eq!(
            decoded.summary.available.first().map(|a| a.amount),
            Some(38)
        );
        Ok(())
    }

    /// A `LandResources` POST reply hands back the follow-up cap URLs, and the
    /// follow-up summary / detail reports surface their respective events.
    #[test]
    fn land_resources_surfaces_reports() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        let urls = sl_proto::LandResourcesUrls {
            script_resource_summary: Some("http://sim/cap/srs".parse()?),
            script_resource_details: Some("http://sim/cap/srd".parse()?),
        };
        let body = sl_proto::parse_llsd_xml(&sl_proto::build_land_resources_response(&urls))?;
        session.handle_caps_event(sl_proto::CAP_LAND_RESOURCES, &body, now)?;
        let decoded = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::LandResourcesUrls(urls) => Some(urls),
                _ => None,
            })
            .ok_or("expected a LandResourcesUrls event")?;
        assert_eq!(
            decoded
                .script_resource_details
                .as_ref()
                .map(url::Url::as_str),
            Some("http://sim/cap/srd")
        );

        let summary = sl_proto::ResourceSummary {
            available: vec![sl_proto::ResourceAmount {
                resource_type: "memory".to_owned(),
                amount: -1,
            }],
            used: vec![sl_proto::ResourceAmount {
                resource_type: "memory".to_owned(),
                amount: 0x2_0000,
            }],
        };
        let body =
            sl_proto::parse_llsd_xml(&sl_proto::build_land_resource_summary_response(&summary))?;
        session.handle_caps_event(sl_proto::LAND_RESOURCE_SUMMARY_TAG, &body, now)?;
        let decoded = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::LandResourceSummary(summary) => Some(summary),
                _ => None,
            })
            .ok_or("expected a LandResourceSummary event")?;
        assert_eq!(decoded.used.first().map(|a| a.amount), Some(0x2_0000));

        let parcels = vec![sl_proto::ParcelScriptResources {
            name: "Home".to_owned(),
            id: uuid::Uuid::from_u128(0x55),
            local_id: sl_proto::RegionLocalParcelId(3),
            objects: Vec::new(),
        }];
        let body =
            sl_proto::parse_llsd_xml(&sl_proto::build_land_resource_detail_response(&parcels))?;
        session.handle_caps_event(sl_proto::LAND_RESOURCE_DETAIL_TAG, &body, now)?;
        let decoded = drain_events(&mut session)
            .into_iter()
            .find_map(|event| match event {
                Event::LandResourceDetail(parcels) => Some(parcels),
                _ => None,
            })
            .ok_or("expected a LandResourceDetail event")?;
        assert_eq!(
            decoded.first().map(|p| p.local_id),
            Some(sl_proto::RegionLocalParcelId(3))
        );
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
        session.offer_teleport(&[AgentKey::from(a), AgentKey::from(b)], "come over", now)?;
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
        session.accept_teleport_lure(LureId::from(lure_id), now)?;
        let sent = drain(&mut session)?;
        let req = sent
            .iter()
            .find_map(|m| match m {
                AnyMessage::TeleportLureRequest(r) => Some(r),
                _ => None,
            })
            .ok_or("expected a TeleportLureRequest")?;
        assert_eq!(req.info.lure_id, lure_id);
        assert_eq!(req.info.teleport_flags, TeleportFlags::VIA_LURE); // 1 << 2
        Ok(())
    }

    #[test]
    fn decline_teleport_lure_packs_im() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let from = uuid::Uuid::from_u128(0x55);
        let lure_id = uuid::Uuid::from_u128(0xCAFE);
        session.decline_teleport_lure(AgentKey::from(from), LureId::from(lure_id), now)?;
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
        session.request_teleport(AgentKey::from(target), "please tp me", now)?;
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
        session.give_inventory(
            AgentKey::from(to),
            InventoryKey::from(item),
            AssetType::Notecard,
            "My Card",
            TransactionId::from(tx),
            now,
        )?;
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
            AgentKey::from(uuid::Uuid::from_u128(0xD1)),
            InventoryFolderKey::from(folder),
            "My Folder",
            TransactionId::from(uuid::Uuid::from_u128(0x9999)),
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
        assert_eq!(
            offer.item_id,
            InventoryItemOrFolderKey::Item(InventoryKey::from(item))
        );
        assert_eq!(offer.transaction_id, tx);
        assert_eq!(offer.from_agent_id, AgentKey::from(from));
        assert!(!offer.from_task);

        // Accept files the item into a destination folder.
        let folder = uuid::Uuid::from_u128(0xF0);
        session.accept_inventory_offer(&offer, InventoryFolderKey::from(folder), now)?;
        let accept = drain(&mut session)?;
        let block = first_im(&accept)?;
        assert_eq!(block.dialog, 5); // IM_INVENTORY_ACCEPTED
        assert_eq!(block.id, tx);
        assert_eq!(block.to_agent_id, from);
        assert_eq!(block.binary_bucket, folder.as_bytes());

        // Decline routes to the trash folder.
        let trash = uuid::Uuid::from_u128(0x7A);
        session.decline_inventory_offer(&offer, InventoryFolderKey::from(trash), now)?;
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
        session.start_conference(
            ImSessionId::from(session_id),
            &[AgentKey::from(a), AgentKey::from(b)],
            "hello all",
            now,
        )?;
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
        session.send_conference_message(ImSessionId::from(session_id), "hi", now)?;
        session.leave_conference(ImSessionId::from(session_id), now)?;
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
        assert_eq!(received.from_agent_id, AgentKey::from(from));
        assert_eq!(received.from_agent_name, "Sender Name");
        assert_eq!(received.message, "stored hello");
        assert_eq!(received.timestamp, Some(1_700_000_000));
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
        let region = uuid::Uuid::from_u128(0x4E61);
        // The `binary_bucket` is nested under `message_params.data`, and the
        // `type` is the group-start dialog (15) — exactly OpenSim's encoding.
        let xml = format!(
            "<llsd><map>\
               <key>session_name</key><string>My Group</string>\
               <key>instantmessage</key><map>\
               <key>message_params</key><map>\
                 <key>id</key><uuid>{session_id}</uuid>\
                 <key>from_id</key><uuid>{from}</uuid>\
                 <key>from_name</key><string>Inviter</string>\
                 <key>type</key><integer>15</integer>\
                 <key>from_group</key><boolean>1</boolean>\
                 <key>region_id</key><uuid>{region}</uuid>\
                 <key>position</key><array><real>1.5</real><real>2.5</real><real>3.5</real></array>\
                 <key>parent_estate_id</key><integer>101</integer>\
                 <key>timestamp</key><integer>1700000000</integer>\
                 <key>data</key><map><key>binary_bucket</key>\
                   <binary>TXkgR3JvdXA=</binary></map>\
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
            dialog,
            from_group,
            session_name,
            message,
            region_id,
            position,
            parent_estate_id,
            timestamp,
            binary_bucket,
        } = event
        else {
            return Err("expected ConferenceInvited".into());
        };
        assert_eq!(got, session_id);
        assert_eq!(from_agent_id, AgentKey::from(from));
        assert_eq!(from_name, "Inviter");
        assert_eq!(dialog, ImDialog::SessionGroupStart);
        assert!(from_group);
        assert_eq!(session_name, "My Group");
        assert_eq!(message, "join us");
        assert_eq!(region_id, region);
        assert_eq!(position, RegionCoordinates::new(1.5, 2.5, 3.5));
        assert_eq!(parent_estate_id, 101);
        assert_eq!(timestamp, Some(1_700_000_000));
        // "My Group" base64-decoded — the group/session label.
        assert_eq!(binary_bucket, b"My Group");
        Ok(())
    }

    /// A text `ChatterBoxInvitation` records a pending `Invited` chat-session
    /// entry — keyed by the group id for a group IM (from_group, type 15) and by
    /// the conference id otherwise (type 16) — carrying the inviter / name and the
    /// text channel classification.
    #[test]
    fn chatterbox_invitation_records_pending_invite() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let group = uuid::Uuid::from_u128(0x60_01);
        let inviter = uuid::Uuid::from_u128(0x60_02);
        let group_xml = format!(
            "<llsd><map>\
               <key>session_name</key><string>My Group</string>\
               <key>instantmessage</key><map><key>message_params</key><map>\
                 <key>id</key><uuid>{group}</uuid>\
                 <key>from_id</key><uuid>{inviter}</uuid>\
                 <key>from_name</key><string>Inviter</string>\
                 <key>type</key><integer>15</integer>\
                 <key>from_group</key><boolean>1</boolean>\
               </map></map></map></llsd>"
        );
        session.handle_caps_event("ChatterBoxInvitation", &parse_llsd_xml(&group_xml)?, now)?;
        let kind = ChatSessionKind::Group {
            group_id: GroupKey::from(group),
        };
        match session.chat_session_lifecycle(kind) {
            Some(ChatSessionLifecycle::Invited(PendingInvite {
                inviter: got,
                session_name,
                channel,
            })) => {
                assert_eq!(*got, AgentKey::from(inviter));
                assert_eq!(session_name, "My Group");
                assert_eq!(*channel, InviteChannel::Text);
            }
            other => return Err(format!("expected Invited, got {other:?}").into()),
        }

        // An ad-hoc conference invite (type 16, from_group clear) keys by the
        // conference id instead.
        let conf = uuid::Uuid::from_u128(0x60_03);
        let conf_xml = format!(
            "<llsd><map>\
               <key>session_name</key><string>Chat</string>\
               <key>instantmessage</key><map><key>message_params</key><map>\
                 <key>id</key><uuid>{conf}</uuid>\
                 <key>from_id</key><uuid>{inviter}</uuid>\
                 <key>from_name</key><string>Inviter</string>\
                 <key>type</key><integer>16</integer>\
                 <key>from_group</key><boolean>0</boolean>\
               </map></map></map></llsd>"
        );
        session.handle_caps_event("ChatterBoxInvitation", &parse_llsd_xml(&conf_xml)?, now)?;
        let conf_kind = ChatSessionKind::Conference {
            id: ImSessionId::from(conf),
        };
        assert!(matches!(
            session.chat_session_lifecycle(conf_kind),
            Some(ChatSessionLifecycle::Invited(_))
        ));
        Ok(())
    }

    /// A `ChatterBoxSessionAgentListUpdates` push that arrives for a still-pending
    /// `Invited` session (as OpenSim sends alongside the invitation itself) folds
    /// the voice roster **without** promoting the session to `Joined` — the
    /// lifecycle stays `Invited` until an explicit accept or real session traffic.
    #[test]
    fn agent_list_update_does_not_join_invited_session() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // Record a pending group invitation (type 15, from_group).
        let group = uuid::Uuid::from_u128(0x62_01);
        let inviter = uuid::Uuid::from_u128(0x62_02);
        let group_xml = format!(
            "<llsd><map>\
               <key>session_name</key><string>My Group</string>\
               <key>instantmessage</key><map><key>message_params</key><map>\
                 <key>id</key><uuid>{group}</uuid>\
                 <key>from_id</key><uuid>{inviter}</uuid>\
                 <key>from_name</key><string>Inviter</string>\
                 <key>type</key><integer>15</integer>\
                 <key>from_group</key><boolean>1</boolean>\
               </map></map></map></llsd>"
        );
        session.handle_caps_event("ChatterBoxInvitation", &parse_llsd_xml(&group_xml)?, now)?;
        let kind = ChatSessionKind::Group {
            group_id: GroupKey::from(group),
        };

        // The agent-list update that follows the invitation must not join us.
        let voice_agent = uuid::Uuid::from_u128(0x62_0A);
        let updates = format!(
            "<llsd><map>\
               <key>session_id</key><uuid>{group}</uuid>\
               <key>agent_updates</key><map>\
                 <key>{voice_agent}</key><map>\
                   <key>info</key><map><key>can_voice_chat</key><boolean>1</boolean></map>\
                   <key>transition</key><string>ENTER</string></map>\
               </map></map></llsd>"
        );
        session.handle_caps_event(
            "ChatterBoxSessionAgentListUpdates",
            &parse_llsd_xml(&updates)?,
            now,
        )?;

        // Still a pending invitation — the roster push is informational only.
        assert!(
            matches!(
                session.chat_session_lifecycle(kind),
                Some(ChatSessionLifecycle::Invited(_))
            ),
            "an agent-list update must not promote an Invited session to Joined"
        );
        // …but the voice roster was folded.
        assert_eq!(
            voice_members(&session, kind),
            vec![AgentKey::from(voice_agent)]
        );
        Ok(())
    }

    /// The invite channel is classified from the body's `instantmessage` /
    /// `voice` sub-maps: both present is [`InviteChannel::Both`]; a `voice`-only
    /// body (no `instantmessage`, fields at the top level) is
    /// [`InviteChannel::Voice`].
    #[test]
    fn chatterbox_invitation_classifies_voice_channels() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // Both channels: an `instantmessage` text body plus a `voice` sub-map.
        let both = uuid::Uuid::from_u128(0x61_01);
        let both_xml = format!(
            "<llsd><map>\
               <key>session_name</key><string>Call</string>\
               <key>voice</key><map><key>invitation_type</key><integer>0</integer></map>\
               <key>instantmessage</key><map><key>message_params</key><map>\
                 <key>id</key><uuid>{both}</uuid>\
                 <key>from_id</key><uuid>{both}</uuid>\
                 <key>type</key><integer>16</integer>\
                 <key>from_group</key><boolean>0</boolean>\
               </map></map></map></llsd>"
        );
        session.handle_caps_event("ChatterBoxInvitation", &parse_llsd_xml(&both_xml)?, now)?;
        let both_kind = ChatSessionKind::Conference {
            id: ImSessionId::from(both),
        };
        assert!(matches!(
            session.chat_session_lifecycle(both_kind),
            Some(ChatSessionLifecycle::Invited(PendingInvite {
                channel: InviteChannel::Both,
                ..
            }))
        ));

        // Voice only: no `instantmessage`; the session fields live at the top
        // level of the body (the SL voice-invite shape).
        let voice = uuid::Uuid::from_u128(0x61_02);
        let voice_xml = format!(
            "<llsd><map>\
               <key>session_id</key><uuid>{voice}</uuid>\
               <key>from_id</key><uuid>{voice}</uuid>\
               <key>session_name</key><string>Call</string>\
               <key>voice</key><map><key>invitation_type</key><integer>0</integer></map>\
             </map></llsd>"
        );
        session.handle_caps_event("ChatterBoxInvitation", &parse_llsd_xml(&voice_xml)?, now)?;
        let voice_kind = ChatSessionKind::Conference {
            id: ImSessionId::from(voice),
        };
        assert!(matches!(
            session.chat_session_lifecycle(voice_kind),
            Some(ChatSessionLifecycle::Invited(PendingInvite {
                channel: InviteChannel::Voice,
                ..
            }))
        ));
        Ok(())
    }

    /// `accept_chat_invite` promotes a pending `Invited` entry to `Joined`;
    /// `decline_chat_invite` removes the entry entirely.
    #[test]
    fn accept_and_decline_chat_invite_transition() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let group = uuid::Uuid::from_u128(0x62_01);
        let inviter = uuid::Uuid::from_u128(0x62_02);
        let xml = format!(
            "<llsd><map>\
               <key>session_name</key><string>My Group</string>\
               <key>instantmessage</key><map><key>message_params</key><map>\
                 <key>id</key><uuid>{group}</uuid>\
                 <key>from_id</key><uuid>{inviter}</uuid>\
                 <key>type</key><integer>15</integer>\
                 <key>from_group</key><boolean>1</boolean>\
               </map></map></map></llsd>"
        );
        let body = parse_llsd_xml(&xml)?;
        let kind = ChatSessionKind::Group {
            group_id: GroupKey::from(group),
        };

        // Accept → Joined.
        session.handle_caps_event("ChatterBoxInvitation", &body, now)?;
        session.accept_chat_invite(ImSessionId::from(group), true, now);
        assert_eq!(
            session.chat_session_lifecycle(kind),
            Some(&ChatSessionLifecycle::Joined)
        );

        // Re-invite then decline → entry removed.
        session.handle_caps_event("ChatterBoxInvitation", &body, now)?;
        session.decline_chat_invite(ImSessionId::from(group), true, now);
        assert_eq!(session.chat_session_lifecycle(kind), None);
        Ok(())
    }

    /// Inbound group-session traffic promotes a still-pending `Invited` entry to
    /// `Joined` without an explicit accept (traffic *is* the join signal).
    #[test]
    fn inbound_traffic_promotes_invited_to_joined() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let group = uuid::Uuid::from_u128(0x63_01);
        let inviter = uuid::Uuid::from_u128(0x63_02);
        let xml = format!(
            "<llsd><map>\
               <key>session_name</key><string>My Group</string>\
               <key>instantmessage</key><map><key>message_params</key><map>\
                 <key>id</key><uuid>{group}</uuid>\
                 <key>from_id</key><uuid>{inviter}</uuid>\
                 <key>type</key><integer>15</integer>\
                 <key>from_group</key><boolean>1</boolean>\
               </map></map></map></llsd>"
        );
        session.handle_caps_event("ChatterBoxInvitation", &parse_llsd_xml(&xml)?, now)?;
        let kind = ChatSessionKind::Group {
            group_id: GroupKey::from(group),
        };
        assert!(matches!(
            session.chat_session_lifecycle(kind),
            Some(ChatSessionLifecycle::Invited(_))
        ));

        // A group message in that session promotes it to Joined.
        let message = AnyMessage::ImprovedInstantMessage(ImprovedInstantMessage {
            agent_data: ImprovedInstantMessageAgentDataBlock {
                agent_id: inviter,
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
                from_agent_name: b"Inviter\0".to_vec(),
                message: b"welcome\0".to_vec(),
                binary_bucket: Vec::new(),
            },
            estate_block: ImprovedInstantMessageEstateBlockBlock { estate_id: 0 },
            meta_data: Vec::new(),
        });
        let datagram = server_message(&message, 9, true)?;
        session.handle_datagram(sim_addr(), &datagram, now)?;
        assert_eq!(
            session.chat_session_lifecycle(kind),
            Some(&ChatSessionLifecycle::Joined)
        );
        Ok(())
    }

    /// A `ChatSessionRequest` accept reply carrying the session's agent roster
    /// (tagged with the answered session id + `from_group`, as the runtime does)
    /// seeds that session's participants. Both the modern `agent_info` map and the
    /// deprecated `agents` array are decoded.
    #[test]
    fn chat_session_request_roster_seeds_participants() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let conf = uuid::Uuid::from_u128(0x64_01);
        let agent_a = uuid::Uuid::from_u128(0x64_0A);
        let agent_b = uuid::Uuid::from_u128(0x64_0B);
        let xml = format!(
            "<llsd><map>\
               <key>session-id</key><uuid>{conf}</uuid>\
               <key>from_group</key><boolean>0</boolean>\
               <key>agent_info</key><map>\
                 <key>{agent_a}</key><map><key>is_moderator</key><boolean>0</boolean></map>\
                 <key>{agent_b}</key><map><key>is_moderator</key><boolean>1</boolean></map>\
               </map></map></llsd>"
        );
        session.handle_caps_event("ChatSessionRequest", &parse_llsd_xml(&xml)?, now)?;
        let kind = ChatSessionKind::Conference {
            id: ImSessionId::from(conf),
        };
        let mut participants: Vec<AgentKey> = session.participants(kind).collect();
        participants.sort();
        let mut expected = vec![AgentKey::from(agent_a), AgentKey::from(agent_b)];
        expected.sort();
        assert_eq!(participants, expected);
        Ok(())
    }

    /// The `ChatSessionRequest` accept / decline POST body encodes the method and
    /// the flat session id (the shape Firestorm's `chatterBoxInvitationCoro`
    /// sends), round-tripping through the LLSD parser.
    #[test]
    fn chat_session_request_body_encodes_method_and_session() -> Result<(), TestError> {
        let session_id = uuid::Uuid::from_u128(0x65_01);
        let body = chat_session_request_body("accept invitation", session_id);
        let parsed = parse_llsd_xml(&body)?;
        assert_eq!(
            parsed.get("method").and_then(Llsd::as_str),
            Some("accept invitation")
        );
        assert_eq!(
            parsed.get("session-id").and_then(Llsd::as_uuid),
            Some(session_id)
        );
        Ok(())
    }

    // ---- Inventory mutation (#30) ------------------------------------------

    /// Builds an [`InventoryItem`] with a single non-default field for tests.
    fn sample_item(item_id: uuid::Uuid, folder_id: uuid::Uuid, name: &str) -> InventoryItem {
        InventoryItem {
            item_id: InventoryKey::from(item_id),
            folder_id: InventoryFolderKey::from(folder_id),
            name: name.to_owned(),
            description: String::new(),
            asset_id: uuid::Uuid::nil(),
            item_type: 0,
            inv_type: 0,
            flags: 0,
            sale_type: 0,
            sale_price: Some(LindenAmount(0)),
            creation_date: 0,
            owner: sl_proto::OwnerKey::Agent(AgentKey::from(uuid::Uuid::nil())),
            last_owner_id: uuid::Uuid::nil(),
            creator_id: AgentKey::from(uuid::Uuid::nil()),
            group: None,
            permissions: Permissions5::empty(),
        }
    }

    #[test]
    fn create_inventory_folder_sends_and_caches() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let folder_id = uuid::Uuid::from_u128(0xF0);
        let parent_id = uuid::Uuid::from_u128(0x10);
        session.create_inventory_folder(
            InventoryFolderKey::from(folder_id),
            InventoryFolderKey::from(parent_id),
            FolderType::RootInventory,
            "Toys & Co",
            now,
        )?;
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
            .inventory_folder(InventoryFolderKey::from(folder_id))
            .ok_or("folder should be cached")?;
        assert_eq!(cached.name, "Toys & Co");
        assert_eq!(cached.parent_id, Some(InventoryFolderKey::from(parent_id)));

        // Removing it drops it from the cache.
        session.remove_inventory_folders(&[InventoryFolderKey::from(folder_id)], now)?;
        let sent = drain(&mut session)?;
        assert!(
            sent.iter()
                .any(|m| matches!(m, AnyMessage::RemoveInventoryFolder(_))),
            "expected a RemoveInventoryFolder"
        );
        assert!(
            session
                .inventory_folder(InventoryFolderKey::from(folder_id))
                .is_none(),
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
            folder_id: InventoryFolderKey::from(uuid::Uuid::from_u128(0x11)),
            transaction_id: uuid::Uuid::nil(),
            next_owner_mask: 0x0008_e000,
            asset_type: AssetType::Notecard,
            inv_type: InventoryType::Notecard,
            wearable_type: WearableType::Shape,
            name: "Notes".to_owned(),
            description: "a note".to_owned(),
        };
        let callback_id = session.create_inventory_item(&new, now)?;
        assert_eq!(
            callback_id,
            InventoryCallbackId(1),
            "first callback id should be 1"
        );
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
        session.update_inventory_item(&item, TransactionId::from(uuid::Uuid::nil()), now)?;
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
            .inventory_item(InventoryKey::from(uuid::Uuid::from_u128(1)))
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
        session.move_inventory_item(
            InventoryKey::from(item_id),
            InventoryFolderKey::from(folder_id),
            "NewName",
            now,
        )?;
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
        let second_item_id = uuid::Uuid::from_u128(0x33);
        let block = |item_id, callback_id, asset_id, name: &[u8]| {
            UpdateCreateInventoryItemInventoryDataBlock {
                item_id,
                folder_id: uuid::Uuid::from_u128(0x32),
                callback_id,
                creator_id: uuid::Uuid::from_u128(2),
                owner_id: uuid::Uuid::from_u128(1),
                group_id: uuid::Uuid::nil(),
                base_mask: 0x7fff_ffff,
                owner_mask: 0x7fff_ffff,
                group_mask: 0,
                everyone_mask: 0,
                next_owner_mask: 0x0008_e000,
                group_owned: false,
                asset_id,
                r#type: 7,
                inv_type: 7,
                flags: 0,
                sale_type: 0,
                sale_price: 0,
                name: name.to_vec(),
                description: b"\0".to_vec(),
                creation_date: 1234,
                crc: 0,
            }
        };
        let message = AnyMessage::UpdateCreateInventoryItem(UpdateCreateInventoryItem {
            agent_data: UpdateCreateInventoryItemAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(1),
                sim_approved: true,
                transaction_id: uuid::Uuid::from_u128(0x99),
            },
            // The simulator may batch several created items into one message;
            // every entry must surface and cache, not just the first.
            inventory_data: vec![
                block(item_id, 7, uuid::Uuid::from_u128(0x55), b"Fresh Note\0"),
                block(
                    second_item_id,
                    8,
                    uuid::Uuid::from_u128(0x57),
                    b"Second Note\0",
                ),
            ],
        });
        let datagram = server_message(&message, 40, false)?;
        session.handle_datagram(sim_addr(), &datagram, now)?;

        let created: Vec<_> = drain_events(&mut session)
            .into_iter()
            .filter_map(|event| match event {
                Event::InventoryItemCreated {
                    sim_approved,
                    callback_id,
                    item,
                    ..
                } => Some((sim_approved, callback_id, item)),
                _ => None,
            })
            .collect();
        assert_eq!(created.len(), 2);
        let (sim_approved, callback_id, item) = created.first().ok_or("first item")?;
        assert!(*sim_approved);
        assert_eq!(*callback_id, Some(InventoryCallbackId(7)));
        assert_eq!(item.name, "Fresh Note");
        assert_eq!(item.asset_id, uuid::Uuid::from_u128(0x55));
        let (_, second_callback, second_item) = created.get(1).ok_or("second item")?;
        assert_eq!(*second_callback, Some(InventoryCallbackId(8)));
        assert_eq!(second_item.name, "Second Note");

        let cached = session
            .inventory_item(InventoryKey::from(item_id))
            .ok_or("item should be cached")?;
        assert_eq!(cached.name, "Fresh Note");
        let second_cached = session
            .inventory_item(InventoryKey::from(second_item_id))
            .ok_or("second item should be cached")?;
        assert_eq!(second_cached.name, "Second Note");
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
                callback_id: 13,
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
            item_callbacks,
        } = event
        else {
            return Err("expected InventoryBulkUpdate".into());
        };
        assert_eq!(transaction_id, uuid::Uuid::from_u128(0xAB));
        assert_eq!(folders.len(), 1);
        assert_eq!(items.len(), 1);
        // The per-item async callback id round-trips so a copy/create can be
        // correlated even when its result arrives as a `BulkUpdateInventory`.
        assert_eq!(
            item_callbacks,
            vec![(InventoryKey::from(item_id), InventoryCallbackId(13))]
        );

        assert_eq!(
            session
                .inventory_folder(InventoryFolderKey::from(folder_id))
                .ok_or("folder should be cached")?
                .name,
            "Copied Folder"
        );
        assert_eq!(
            session
                .inventory_item(InventoryKey::from(item_id))
                .ok_or("item should be cached")?
                .name,
            "Copied Item"
        );
        Ok(())
    }

    #[test]
    fn unknown_message_id_surfaces_decode_failed_with_offset() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        session.set_diagnostics(true);

        // High id 0 maps to no template message, so `AnyMessage::decode` rejects
        // it after consuming only the single id byte.
        let datagram = server_datagram(MessageId::High(0), &[0xAA, 0xBB], 2, false);
        session.handle_datagram(sim_addr(), &datagram, now)?;

        let diagnostics = drain_diagnostics(&mut session);
        let Some(Diagnostic::DecodeFailed {
            id,
            name,
            error,
            raw,
            failed_offset,
        }) = diagnostics.first()
        else {
            return Err(format!("expected a DecodeFailed, got {diagnostics:?}").into());
        };
        assert_eq!(*id, MessageId::High(0));
        // No template message owns this id.
        assert_eq!(*name, None);
        assert_eq!(
            *error,
            WireError::UnknownMessage {
                id: MessageId::High(0)
            }
        );
        // Decoding stopped right after the one-byte id prefix.
        assert_eq!(*failed_offset, 1);
        // The (post zero-decode) body was captured for a hexdump: id byte + body.
        assert_eq!(raw.as_slice(), &[0x00, 0xAA, 0xBB]);
        Ok(())
    }

    #[test]
    fn truncated_known_message_surfaces_named_decode_failed() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        session.set_diagnostics(true);

        // RegionHandshake (Low 148) with a one-byte body: the id decodes, but the
        // body runs out before its fields, so decoding fails partway through.
        let datagram = server_datagram(MessageId::Low(148), &[0x00], 2, false);
        session.handle_datagram(sim_addr(), &datagram, now)?;

        let diagnostics = drain_diagnostics(&mut session);
        let Some(Diagnostic::DecodeFailed {
            id,
            name,
            error,
            failed_offset,
            ..
        }) = diagnostics.first()
        else {
            return Err(format!("expected a DecodeFailed, got {diagnostics:?}").into());
        };
        assert_eq!(*id, MessageId::Low(148));
        assert_eq!(*name, Some("RegionHandshake"));
        assert!(matches!(error, WireError::UnexpectedEof { .. }));
        // Past the 4-byte Low-id prefix, into the body.
        assert!(
            *failed_offset >= 4,
            "offset {failed_offset} past the id prefix"
        );
        Ok(())
    }

    #[test]
    fn diagnostics_off_by_default_emits_nothing() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // Collection is off until explicitly enabled.
        assert!(!session.diagnostics_enabled());

        // The same undecodable datagram that produces a DecodeFailed when
        // diagnostics are on must produce nothing while they are off.
        let datagram = server_datagram(MessageId::High(0), &[0xAA, 0xBB], 2, false);
        session.handle_datagram(sim_addr(), &datagram, now)?;
        assert!(drain_diagnostics(&mut session).is_empty());
        Ok(())
    }

    #[test]
    fn unknown_caps_event_surfaces_diagnostic() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        session.set_diagnostics(true);

        // An event name the session does not handle.
        session.handle_caps_event("TotallyUnknownEvent", &Llsd::Undef, now)?;
        assert_eq!(
            drain_diagnostics(&mut session),
            vec![Diagnostic::UnknownCapsEvent {
                message: "TotallyUnknownEvent".to_owned(),
            }]
        );
        Ok(())
    }

    #[test]
    fn malformed_caps_body_surfaces_decode_failed() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        session.set_diagnostics(true);

        // A handled event whose body is the wrong shape: `from_llsd` returns None.
        session.handle_caps_event("ParcelProperties", &Llsd::Undef, now)?;
        assert_eq!(
            drain_diagnostics(&mut session),
            vec![Diagnostic::CapsDecodeFailed {
                message: "ParcelProperties".to_owned(),
                reason: None,
            }]
        );
        Ok(())
    }

    // -- B2: chat-session registry + open/track mechanics ------------------

    /// Builds an inbound group IM (`from_group` set) with a chosen dialog,
    /// sender, and group id (the group id is the session id on the wire).
    fn inbound_group_im(dialog: u8, from: uuid::Uuid, group: uuid::Uuid) -> AnyMessage {
        AnyMessage::ImprovedInstantMessage(ImprovedInstantMessage {
            agent_data: ImprovedInstantMessageAgentDataBlock {
                agent_id: from,
                session_id: uuid::Uuid::nil(),
            },
            message_block: ImprovedInstantMessageMessageBlockBlock {
                from_group: true,
                to_agent_id: uuid::Uuid::from_u128(1),
                parent_estate_id: 0,
                region_id: uuid::Uuid::nil(),
                position: vec3(0.0, 0.0, 0.0),
                offline: 0,
                dialog,
                id: group,
                timestamp: 0,
                from_agent_name: b"Group Member\0".to_vec(),
                message: b"hello group\0".to_vec(),
                binary_bucket: Vec::new(),
            },
            estate_block: ImprovedInstantMessageEstateBlockBlock { estate_id: 0 },
            meta_data: Vec::new(),
        })
    }

    /// The chat sessions currently in the registry, in the accessor's
    /// newest-first order.
    fn chat_sessions(session: &Session) -> Vec<ChatSessionKind> {
        session.chat_sessions().collect()
    }

    #[test]
    fn inbound_one_to_one_im_opens_direct_session() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // An ordinary 1:1 IM (dialog 0) from 0x55 opens a Direct session keyed by
        // the peer (the sender), not the wire XOR id.
        let im = inbound_im(0, b"Friendly Bot\0", b"hi there\0");
        let datagram = server_message(&im, 9, false)?;
        session.handle_datagram(sim_addr(), &datagram, now)?;

        assert_eq!(
            chat_sessions(&session),
            vec![ChatSessionKind::Direct {
                peer: AgentKey::from(uuid::Uuid::from_u128(0x55)),
            }]
        );
        Ok(())
    }

    #[test]
    fn non_message_im_does_not_open_a_session() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // A friendship offer (dialog 38) is not session traffic: no session opens.
        let im = inbound_offer_im(
            38,
            uuid::Uuid::from_u128(0x55),
            uuid::Uuid::from_u128(0x1),
            Vec::new(),
        );
        let datagram = server_message(&im, 9, false)?;
        session.handle_datagram(sim_addr(), &datagram, now)?;

        assert!(chat_sessions(&session).is_empty());
        Ok(())
    }

    #[test]
    fn outbound_one_to_one_im_opens_direct_session() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let peer = uuid::Uuid::from_u128(0x99);
        session.send_instant_message(AgentKey::from(peer), "hi there", now)?;

        assert_eq!(
            chat_sessions(&session),
            vec![ChatSessionKind::Direct {
                peer: AgentKey::from(peer),
            }]
        );
        Ok(())
    }

    #[test]
    fn one_to_one_session_has_no_leave_and_persists() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // Open a 1:1, then leave a group and a conference: the 1:1 survives (there
        // is no SessionLeave for a direct IM).
        let peer = uuid::Uuid::from_u128(0x99);
        session.send_instant_message(AgentKey::from(peer), "hi", now)?;
        session.start_group_session(GroupKey::from(uuid::Uuid::from_u128(0x6700)), now)?;
        session.leave_group_session(GroupKey::from(uuid::Uuid::from_u128(0x6700)), now)?;
        drain(&mut session)?;

        assert_eq!(
            chat_sessions(&session),
            vec![ChatSessionKind::Direct {
                peer: AgentKey::from(peer),
            }]
        );
        Ok(())
    }

    #[test]
    fn inbound_group_traffic_opens_group_session() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let group = uuid::Uuid::from_u128(0x6708);
        // A group send (dialog 17) opens the session…
        let send = inbound_group_im(17, uuid::Uuid::from_u128(0x6709), group);
        session.handle_datagram(sim_addr(), &server_message(&send, 9, true)?, now)?;
        assert_eq!(
            chat_sessions(&session),
            vec![ChatSessionKind::Group {
                group_id: GroupKey::from(group),
            }]
        );

        // …and a participant change (dialog 13) on the same group does not open a
        // second entry.
        let add = inbound_group_im(13, uuid::Uuid::from_u128(0x670A), group);
        session.handle_datagram(sim_addr(), &server_message(&add, 10, true)?, now)?;
        assert_eq!(chat_sessions(&session).len(), 1);
        Ok(())
    }

    #[test]
    fn outbound_group_session_opens_then_leave_removes() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let group = GroupKey::from(uuid::Uuid::from_u128(0x670B));
        session.start_group_session(group, now)?;
        session.send_group_message(group, "hi all", now)?;
        assert_eq!(
            chat_sessions(&session),
            vec![ChatSessionKind::Group { group_id: group }]
        );

        session.leave_group_session(group, now)?;
        assert!(chat_sessions(&session).is_empty());
        Ok(())
    }

    #[test]
    fn inbound_conference_traffic_opens_conference_session() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // Dialog 17 with from_group clear is a conference; the id is the session id.
        let conference = uuid::Uuid::from_u128(0xABC);
        let im = inbound_offer_im(17, uuid::Uuid::from_u128(0x55), conference, Vec::new());
        session.handle_datagram(sim_addr(), &server_message(&im, 9, false)?, now)?;

        assert_eq!(
            chat_sessions(&session),
            vec![ChatSessionKind::Conference {
                id: ImSessionId::from(conference),
            }]
        );
        Ok(())
    }

    #[test]
    fn outbound_conference_opens_then_leave_removes() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let conference = ImSessionId::from(uuid::Uuid::from_u128(0x5E51));
        session.start_conference(
            conference,
            &[AgentKey::from(uuid::Uuid::from_u128(0xA1))],
            "hi",
            now,
        )?;
        assert_eq!(
            chat_sessions(&session),
            vec![ChatSessionKind::Conference { id: conference }]
        );

        session.leave_conference(conference, now)?;
        assert!(chat_sessions(&session).is_empty());
        Ok(())
    }

    #[test]
    fn chat_session_creates_once_and_restamps_for_ordering() -> Result<(), TestError> {
        let start = Instant::now();
        let mut session = established(start)?;
        drain(&mut session)?;

        // Open Direct at t0, then Group at t1 (Group is newest → leads).
        let peer = AgentKey::from(uuid::Uuid::from_u128(0x99));
        let group = GroupKey::from(uuid::Uuid::from_u128(0x6700));
        session.send_instant_message(peer, "first", start)?;
        let t1 = after(start, 10)?;
        session.start_group_session(group, t1)?;
        drain(&mut session)?;
        assert_eq!(
            chat_sessions(&session),
            vec![
                ChatSessionKind::Group { group_id: group },
                ChatSessionKind::Direct { peer },
            ]
        );

        // Touching the Direct session again at t2 restamps its activity (so it now
        // leads) without creating a second Direct entry.
        let t2 = after(start, 20)?;
        session.send_instant_message(peer, "second", t2)?;
        drain(&mut session)?;
        assert_eq!(
            chat_sessions(&session),
            vec![
                ChatSessionKind::Direct { peer },
                ChatSessionKind::Group { group_id: group },
            ]
        );
        Ok(())
    }

    #[test]
    fn canonical_session_id_round_trips_per_kind() -> Result<(), TestError> {
        let own = AgentKey::from(uuid::Uuid::from_u128(1));
        let peer = AgentKey::from(uuid::Uuid::from_u128(0x99));
        let group = GroupKey::from(uuid::Uuid::from_u128(0x6700));
        let conference = ImSessionId::from(uuid::Uuid::from_u128(0x5E51));

        // Group → the group id; Conference → the minted id; Direct → the XOR of
        // the two agent ids (self-inverse, so XOR-ing again recovers the peer).
        assert_eq!(
            ChatSessionKind::Group { group_id: group }.canonical_session_id(own),
            uuid::Uuid::from_u128(0x6700)
        );
        assert_eq!(
            ChatSessionKind::Conference { id: conference }.canonical_session_id(own),
            uuid::Uuid::from_u128(0x5E51)
        );
        let direct_id = ChatSessionKind::Direct { peer }.canonical_session_id(own);
        assert_eq!(direct_id, uuid::Uuid::from_u128(1u128 ^ 0x99u128));
        Ok(())
    }

    /// The roster of `session`, in the accessor's order.
    fn participants(session: &Session, kind: ChatSessionKind) -> Vec<AgentKey> {
        session.participants(kind).collect()
    }

    /// The live typers in `session`, in the accessor's order.
    fn typers(session: &Session, kind: ChatSessionKind) -> Vec<AgentKey> {
        session.typing(kind).collect()
    }

    #[test]
    fn group_participant_events_fold_roster_and_open_session() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let group = uuid::Uuid::from_u128(0x6708);
        let member = uuid::Uuid::from_u128(0x670A);
        let kind = ChatSessionKind::Group {
            group_id: GroupKey::from(group),
        };

        // A SessionAdd (dialog 13) opens the session and inserts the member.
        let add = inbound_group_im(13, member, group);
        session.handle_datagram(sim_addr(), &server_message(&add, 9, true)?, now)?;
        assert_eq!(chat_sessions(&session), vec![kind]);
        assert_eq!(participants(&session, kind), vec![AgentKey::from(member)]);

        // A SessionLeave (dialog 18) removes the member again.
        let leave = inbound_group_im(18, member, group);
        session.handle_datagram(sim_addr(), &server_message(&leave, 10, true)?, now)?;
        assert!(participants(&session, kind).is_empty());
        Ok(())
    }

    #[test]
    fn direct_session_participants_synthesised_as_peer() -> Result<(), TestError> {
        let peer = AgentKey::from(uuid::Uuid::from_u128(0x99));
        let session = new_session()?;

        // A 1:1's roster is never materialised — it is synthesised `{ peer }`
        // straight from the key, so no traffic is needed.
        assert_eq!(
            participants(&session, ChatSessionKind::Direct { peer }),
            vec![peer]
        );
        Ok(())
    }

    #[test]
    fn inbound_typing_start_and_stop_track_one_to_one() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let peer = AgentKey::from(uuid::Uuid::from_u128(0x55));
        let kind = ChatSessionKind::Direct { peer };

        // Open the 1:1 with an ordinary message (typing alone never opens one).
        let message = inbound_im(0, b"Friendly Bot\0", b"hi\0");
        session.handle_datagram(sim_addr(), &server_message(&message, 9, false)?, now)?;

        // TypingStart (dialog 41) marks the peer typing; the 1:1 is keyed by the
        // sender, not the wire `id` field.
        let start = inbound_im(41, b"Friendly Bot\0", b"typing\0");
        session.handle_datagram(sim_addr(), &server_message(&start, 10, false)?, now)?;
        assert_eq!(typers(&session, kind), vec![peer]);

        // TypingStop (dialog 42) clears it immediately (no waiting on expiry).
        let stop = inbound_im(42, b"Friendly Bot\0", b"\0");
        session.handle_datagram(sim_addr(), &server_message(&stop, 11, false)?, now)?;
        assert!(typers(&session, kind).is_empty());
        Ok(())
    }

    #[test]
    fn typing_does_not_open_a_session() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // A typing notification for an unopened 1:1 stores nothing and conjures
        // no session (the event still fires for the driver to react to).
        let start = inbound_im(41, b"Friendly Bot\0", b"typing\0");
        session.handle_datagram(sim_addr(), &server_message(&start, 9, false)?, now)?;

        assert!(chat_sessions(&session).is_empty());
        assert!(
            typers(
                &session,
                ChatSessionKind::Direct {
                    peer: AgentKey::from(uuid::Uuid::from_u128(0x55)),
                }
            )
            .is_empty()
        );
        Ok(())
    }

    #[test]
    fn typing_expires_after_timeout() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let peer = AgentKey::from(uuid::Uuid::from_u128(0x55));
        let kind = ChatSessionKind::Direct { peer };

        // Open the 1:1 and mark the peer typing.
        let message = inbound_im(0, b"Friendly Bot\0", b"hi\0");
        session.handle_datagram(sim_addr(), &server_message(&message, 9, false)?, now)?;
        let start = inbound_im(41, b"Friendly Bot\0", b"typing\0");
        session.handle_datagram(sim_addr(), &server_message(&start, 10, false)?, now)?;

        // A timed tick before the 9 s timeout keeps the entry…
        session.handle_timeout(after(now, 8_000)?);
        assert_eq!(typers(&session, kind), vec![peer]);

        // …and one at the timeout prunes the stale typer (a lost TypingStop).
        session.handle_timeout(after(now, 9_000)?);
        assert!(typers(&session, kind).is_empty());
        Ok(())
    }

    #[test]
    fn group_typing_keys_by_session() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let group = uuid::Uuid::from_u128(0x6708);
        let typer = uuid::Uuid::from_u128(0x670A);
        let kind = ChatSessionKind::Group {
            group_id: GroupKey::from(group),
        };

        // Open the group session, then a typing notification whose `id` is the
        // group id resolves to that session (keyed by the typer).
        let send = inbound_group_im(17, uuid::Uuid::from_u128(0x6709), group);
        session.handle_datagram(sim_addr(), &server_message(&send, 9, true)?, now)?;
        let start = inbound_group_im(41, typer, group);
        session.handle_datagram(sim_addr(), &server_message(&start, 10, true)?, now)?;

        assert_eq!(typers(&session, kind), vec![AgentKey::from(typer)]);
        Ok(())
    }

    #[test]
    fn offline_notification_clears_typing_and_roster_keeping_sessions() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // A friend who is both a conference participant and typing in a 1:1.
        let friend = uuid::Uuid::from_u128(0xF3);
        let friend_key = AgentKey::from(friend);
        let conference = uuid::Uuid::from_u128(0xABC);
        let conf_kind = ChatSessionKind::Conference {
            id: ImSessionId::from(conference),
        };
        let direct_kind = ChatSessionKind::Direct { peer: friend_key };

        // Seed the conference roster (SessionAdd, dialog 13) and a 1:1 typing
        // entry (open the 1:1 with a message, then TypingStart, dialog 41).
        let add = inbound_offer_im(13, friend, conference, Vec::new());
        session.handle_datagram(sim_addr(), &server_message(&add, 9, false)?, now)?;
        // Open the 1:1 with a message from the friend, then a TypingStart — both
        // keyed by the sender, so they must come *from* the friend's id.
        let open = inbound_offer_im(0, friend, uuid::Uuid::nil(), Vec::new());
        session.handle_datagram(sim_addr(), &server_message(&open, 10, false)?, now)?;
        let start = inbound_offer_im(41, friend, uuid::Uuid::nil(), Vec::new());
        session.handle_datagram(sim_addr(), &server_message(&start, 11, false)?, now)?;
        assert_eq!(participants(&session, conf_kind), vec![friend_key]);
        assert_eq!(typers(&session, direct_kind), vec![friend_key]);

        // The friend goes offline.
        let offline = AnyMessage::OfflineNotification(OfflineNotification {
            agent_block: vec![OfflineNotificationAgentBlockBlock { agent_id: friend }],
        });
        session.handle_datagram(sim_addr(), &server_message(&offline, 12, true)?, now)?;

        // The friend is dropped from the conference roster and the 1:1 typing
        // set, but neither session is removed and presence reads offline.
        assert!(participants(&session, conf_kind).is_empty());
        assert!(typers(&session, direct_kind).is_empty());
        assert_eq!(
            chat_sessions(&session),
            vec![direct_kind, conf_kind],
            "both sessions still exist after the offline notification"
        );
        assert!(!session.is_online(FriendKey::from(friend)));
        Ok(())
    }

    #[test]
    fn offline_notification_leaves_other_participants_untouched() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // Two participants in one conference; only the friend who goes offline is
        // removed — a non-offlined participant (a non-friend) relies on the sim's
        // SessionLeave, which the presence fast path must not pre-empt.
        let friend = uuid::Uuid::from_u128(0xF3);
        let other = uuid::Uuid::from_u128(0xF4);
        let conference = uuid::Uuid::from_u128(0xABC);
        let conf_kind = ChatSessionKind::Conference {
            id: ImSessionId::from(conference),
        };
        session.handle_datagram(
            sim_addr(),
            &server_message(
                &inbound_offer_im(13, friend, conference, Vec::new()),
                9,
                false,
            )?,
            now,
        )?;
        session.handle_datagram(
            sim_addr(),
            &server_message(
                &inbound_offer_im(13, other, conference, Vec::new()),
                10,
                false,
            )?,
            now,
        )?;

        let offline = AnyMessage::OfflineNotification(OfflineNotification {
            agent_block: vec![OfflineNotificationAgentBlockBlock { agent_id: friend }],
        });
        session.handle_datagram(sim_addr(), &server_message(&offline, 11, true)?, now)?;

        assert_eq!(
            participants(&session, conf_kind),
            vec![AgentKey::from(other)],
            "only the offlined agent is dropped; the other participant remains"
        );
        Ok(())
    }

    #[test]
    fn online_notification_changes_no_session() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let friend = uuid::Uuid::from_u128(0xF3);
        let friend_key = AgentKey::from(friend);
        let conference = uuid::Uuid::from_u128(0xABC);
        let conf_kind = ChatSessionKind::Conference {
            id: ImSessionId::from(conference),
        };
        session.handle_datagram(
            sim_addr(),
            &server_message(
                &inbound_offer_im(13, friend, conference, Vec::new()),
                9,
                false,
            )?,
            now,
        )?;

        // A FriendsOnline notification only updates presence — no chat action.
        let online = AnyMessage::OnlineNotification(OnlineNotification {
            agent_block: vec![OnlineNotificationAgentBlockBlock { agent_id: friend }],
        });
        session.handle_datagram(sim_addr(), &server_message(&online, 10, true)?, now)?;

        assert_eq!(participants(&session, conf_kind), vec![friend_key]);
        assert_eq!(chat_sessions(&session), vec![conf_kind]);
        assert!(session.is_online(FriendKey::from(friend)));
        Ok(())
    }

    // ---- Per-session voice-channel state (B8) -------------------------------

    /// The voice-connected members of `session`, sorted for a stable assertion.
    fn voice_members(session: &Session, kind: ChatSessionKind) -> Vec<AgentKey> {
        let mut members: Vec<AgentKey> = session.session_voice_members(kind).collect();
        members.sort();
        members
    }

    /// A voice `ChatterBoxInvitation` (a top-level `voice` sub-map) records that
    /// the session offers voice and decodes the channel coordinates it carries.
    #[test]
    fn voice_invitation_sets_has_voice_and_channel() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let conf = uuid::Uuid::from_u128(0x68_01);
        let from = uuid::Uuid::from_u128(0x68_02);
        let xml = format!(
            "<llsd><map>\
               <key>session_id</key><uuid>{conf}</uuid>\
               <key>from_id</key><uuid>{from}</uuid>\
               <key>session_name</key><string>Call</string>\
               <key>voice</key><map>\
                 <key>channel_uri</key><string>sip:conf@example.com</string>\
                 <key>channel_credentials</key><string>tok123</string>\
                 <key>voice_server_type</key><string>vivox</string>\
                 <key>session_handle</key><string>handle-1</string>\
               </map></map></llsd>"
        );
        session.handle_caps_event("ChatterBoxInvitation", &parse_llsd_xml(&xml)?, now)?;
        let kind = ChatSessionKind::Conference {
            id: ImSessionId::from(conf),
        };
        assert!(session.session_has_voice(kind), "the session offers voice");
        let channel = session
            .session_voice_channel(kind)
            .ok_or("expected a voice channel")?;
        assert_eq!(
            channel.channel_uri,
            url::Url::parse("sip:conf@example.com").ok()
        );
        assert_eq!(channel.channel_credentials.as_deref(), Some("tok123"));
        assert_eq!(channel.voice_server_type.as_deref(), Some("vivox"));
        assert_eq!(channel.session_handle.as_deref(), Some("handle-1"));
        // No voice join has been signalled yet.
        assert!(!session.session_voice_joined(kind));
        Ok(())
    }

    /// A `ChatSessionRequest` accept reply carrying a `voice_channel_info` block
    /// records the channel coordinates and marks the session voice-capable, even
    /// when the reply carries no agent roster.
    #[test]
    fn accept_reply_populates_voice_channel() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let conf = uuid::Uuid::from_u128(0x69_01);
        let xml = format!(
            "<llsd><map>\
               <key>session-id</key><uuid>{conf}</uuid>\
               <key>from_group</key><boolean>0</boolean>\
               <key>voice_channel_info</key><map>\
                 <key>channel_uri</key><string>sip:room@example.com</string>\
                 <key>voice_server_type</key><string>webrtc</string>\
               </map></map></llsd>"
        );
        session.handle_caps_event("ChatSessionRequest", &parse_llsd_xml(&xml)?, now)?;
        let kind = ChatSessionKind::Conference {
            id: ImSessionId::from(conf),
        };
        assert!(session.session_has_voice(kind));
        let channel = session
            .session_voice_channel(kind)
            .ok_or("expected a voice channel")?;
        assert_eq!(
            channel.channel_uri,
            url::Url::parse("sip:room@example.com").ok()
        );
        assert_eq!(channel.voice_server_type.as_deref(), Some("webrtc"));
        assert_eq!(channel.channel_credentials, None);
        Ok(())
    }

    /// `join_session_voice` / `leave_session_voice` flip the optimistic
    /// `voice.joined` flag without removing the text conversation.
    #[test]
    fn join_and_leave_session_voice_flip_joined() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let group = uuid::Uuid::from_u128(0x6A_01);
        let kind = ChatSessionKind::Group {
            group_id: GroupKey::from(group),
        };
        // Open the group session with an inbound message.
        let send = inbound_group_im(17, uuid::Uuid::from_u128(0x6A_02), group);
        session.handle_datagram(sim_addr(), &server_message(&send, 9, true)?, now)?;
        assert!(!session.session_voice_joined(kind));

        session.join_session_voice(kind, now);
        assert!(session.session_voice_joined(kind), "join sets voice.joined");
        assert!(
            session.session_has_voice(kind),
            "join marks the session voice-capable"
        );

        session.leave_session_voice(kind);
        assert!(
            !session.session_voice_joined(kind),
            "leave clears voice.joined"
        );
        // Leaving voice does not leave the conversation.
        assert_eq!(chat_sessions(&session), vec![kind]);
        Ok(())
    }

    /// A `ChatterBoxSessionAgentListUpdates` push folds the per-agent voice flag
    /// into `voice.members`: a voice-capable agent is added (regardless of the
    /// out-of-scope speaking flag), a text-only agent is not, and a `LEAVE`
    /// transition removes a member.
    #[test]
    fn agent_list_voice_update_folds_members() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let conf = uuid::Uuid::from_u128(0x6B_01);
        let kind = ChatSessionKind::Conference {
            id: ImSessionId::from(conf),
        };
        // The session must already be open for the update to resolve to it.
        let add = inbound_offer_im(13, uuid::Uuid::from_u128(0x6B_09), conf, Vec::new());
        session.handle_datagram(sim_addr(), &server_message(&add, 9, false)?, now)?;

        let voice_agent = uuid::Uuid::from_u128(0x6B_0A);
        let text_agent = uuid::Uuid::from_u128(0x6B_0B);
        // `voice_agent` is voice-capable but *not* speaking; `text_agent` is
        // speaking but not voice-capable — only the former is a voice member.
        let updates = format!(
            "<llsd><map>\
               <key>session_id</key><uuid>{conf}</uuid>\
               <key>agent_updates</key><map>\
                 <key>{voice_agent}</key><map>\
                   <key>info</key><map>\
                     <key>can_voice_chat</key><boolean>1</boolean>\
                     <key>is_now_speaking</key><boolean>0</boolean></map>\
                   <key>transition</key><string>ENTER</string></map>\
                 <key>{text_agent}</key><map>\
                   <key>info</key><map>\
                     <key>can_voice_chat</key><boolean>0</boolean>\
                     <key>is_now_speaking</key><boolean>1</boolean></map>\
                   <key>transition</key><string>ENTER</string></map>\
               </map></map></llsd>"
        );
        session.handle_caps_event(
            "ChatterBoxSessionAgentListUpdates",
            &parse_llsd_xml(&updates)?,
            now,
        )?;
        assert_eq!(
            voice_members(&session, kind),
            vec![AgentKey::from(voice_agent)]
        );

        // A LEAVE transition drops the voice member.
        let leave = format!(
            "<llsd><map>\
               <key>session_id</key><uuid>{conf}</uuid>\
               <key>agent_updates</key><map>\
                 <key>{voice_agent}</key><map>\
                   <key>info</key><map><key>can_voice_chat</key><boolean>1</boolean></map>\
                   <key>transition</key><string>LEAVE</string></map>\
               </map></map></llsd>"
        );
        session.handle_caps_event(
            "ChatterBoxSessionAgentListUpdates",
            &parse_llsd_xml(&leave)?,
            now,
        )?;
        assert!(voice_members(&session, kind).is_empty());
        Ok(())
    }

    /// A 1:1 P2P voice call's members are implicitly `{ self, peer }` once we have
    /// joined, and empty before.
    #[test]
    fn direct_voice_members_are_self_and_peer() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let peer = AgentKey::from(uuid::Uuid::from_u128(0x6C_01));
        let own = AgentKey::from(uuid::Uuid::from_u128(1));
        let kind = ChatSessionKind::Direct { peer };
        assert!(
            voice_members(&session, kind).is_empty(),
            "no members before joining"
        );

        session.join_session_voice(kind, now);
        let mut expected = vec![own, peer];
        expected.sort();
        assert_eq!(voice_members(&session, kind), expected);
        Ok(())
    }

    /// An `OfflineNotification` drops the offlined friend from `voice.members` on
    /// the same fan-out as `typing` / `participants`, without removing the session.
    #[test]
    fn offline_notification_drops_voice_member() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let friend = uuid::Uuid::from_u128(0xF3);
        let conf = uuid::Uuid::from_u128(0x6D_01);
        let kind = ChatSessionKind::Conference {
            id: ImSessionId::from(conf),
        };
        // Open the conference, then mark the friend voice-connected.
        let add = inbound_offer_im(13, friend, conf, Vec::new());
        session.handle_datagram(sim_addr(), &server_message(&add, 9, false)?, now)?;
        let updates = format!(
            "<llsd><map>\
               <key>session_id</key><uuid>{conf}</uuid>\
               <key>agent_updates</key><map>\
                 <key>{friend}</key><map>\
                   <key>info</key><map><key>can_voice_chat</key><boolean>1</boolean></map>\
                   <key>transition</key><string>ENTER</string></map>\
               </map></map></llsd>"
        );
        session.handle_caps_event(
            "ChatterBoxSessionAgentListUpdates",
            &parse_llsd_xml(&updates)?,
            now,
        )?;
        assert_eq!(voice_members(&session, kind), vec![AgentKey::from(friend)]);

        let offline = AnyMessage::OfflineNotification(OfflineNotification {
            agent_block: vec![OfflineNotificationAgentBlockBlock { agent_id: friend }],
        });
        session.handle_datagram(sim_addr(), &server_message(&offline, 10, true)?, now)?;
        assert!(voice_members(&session, kind).is_empty());
        assert_eq!(
            chat_sessions(&session),
            vec![kind],
            "the session still exists"
        );
        Ok(())
    }

    /// The voice facet (joined / channel / members) persists across a teleport,
    /// like the rest of the chat-session state.
    #[test]
    fn teleport_preserves_voice_facet() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);

        let conf = uuid::Uuid::from_u128(0x6E_01);
        let kind = ChatSessionKind::Conference {
            id: ImSessionId::from(conf),
        };
        // Seed a voice channel (accept reply) and a joined state.
        let xml = format!(
            "<llsd><map>\
               <key>session-id</key><uuid>{conf}</uuid>\
               <key>from_group</key><boolean>0</boolean>\
               <key>voice_channel_info</key><map>\
                 <key>channel_uri</key><string>sip:room@example.com</string>\
               </map></map></llsd>"
        );
        session.handle_caps_event("ChatSessionRequest", &parse_llsd_xml(&xml)?, now)?;
        session.join_session_voice(kind, now);
        assert!(session.session_voice_joined(kind));
        drain(&mut session)?;
        drain_events(&mut session);

        // Teleport to another region.
        let handle = 0x0003_E900_0003_E800;
        session.teleport_to(
            RegionHandle(handle),
            region_coords(128.0, 128.0, 30.0),
            vec3(1.0, 0.0, 0.0),
            now,
        )?;
        drain(&mut session)?;
        drain_events(&mut session);
        let finish = AnyMessage::TeleportFinish(TeleportFinish {
            info: TeleportFinishInfoBlock {
                agent_id: uuid::Uuid::from_u128(1),
                location_id: 4,
                sim_ip: [127, 0, 0, 1],
                sim_port: 9100u16.swap_bytes(),
                region_handle: handle,
                seed_capability: b"http://x/voiceTP\0".to_vec(),
                sim_access: sl_wire::sim_access::MATURE,
                teleport_flags: TeleportFlags::VIA_LURE,
            },
        });
        session.handle_datagram(sim_addr(), &server_message(&finish, 10, true)?, now)?;

        // The voice facet survives the teleport.
        assert!(
            session.session_voice_joined(kind),
            "voice.joined survives teleport"
        );
        assert!(session.session_has_voice(kind));
        let channel = session
            .session_voice_channel(kind)
            .ok_or("expected the voice channel to survive")?;
        assert_eq!(
            channel.channel_uri,
            url::Url::parse("sip:room@example.com").ok()
        );
        Ok(())
    }

    /// The logged conversation history of `session`, oldest-first.
    fn history(session: &Session, kind: ChatSessionKind) -> Vec<SessionMessage> {
        session.history(kind).cloned().collect()
    }

    #[test]
    fn inbound_one_to_one_im_logs_and_bumps_unread() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let peer = AgentKey::from(uuid::Uuid::from_u128(0x55));
        let kind = ChatSessionKind::Direct { peer };
        let im = inbound_im(0, b"Friendly Bot\0", b"hi there\0");
        session.handle_datagram(sim_addr(), &server_message(&im, 9, false)?, now)?;

        let logged = history(&session, kind);
        assert_eq!(logged.len(), 1);
        let entry = logged.first().ok_or("expected a logged message")?;
        assert_eq!(entry.sender, peer);
        assert_eq!(entry.dialog, ImDialog::Message);
        assert_eq!(entry.text, "hi there");
        assert_eq!(entry.timestamp, None);
        assert_eq!(session.unread(kind), 1);
        Ok(())
    }

    #[test]
    fn inbound_group_send_logs_to_group_session() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let group = uuid::Uuid::from_u128(0x6708);
        let sender = uuid::Uuid::from_u128(0x6709);
        let kind = ChatSessionKind::Group {
            group_id: GroupKey::from(group),
        };
        let send = inbound_group_im(17, sender, group);
        session.handle_datagram(sim_addr(), &server_message(&send, 9, true)?, now)?;

        let logged = history(&session, kind);
        assert_eq!(logged.len(), 1);
        let entry = logged.first().ok_or("expected a logged message")?;
        assert_eq!(entry.sender, AgentKey::from(sender));
        assert_eq!(entry.dialog, ImDialog::SessionSend);
        assert_eq!(entry.text, "hello group");
        assert_eq!(session.unread(kind), 1);
        Ok(())
    }

    #[test]
    fn outbound_one_to_one_im_logs_as_self_and_resets_unread() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let peer = AgentKey::from(uuid::Uuid::from_u128(0x55));
        let kind = ChatSessionKind::Direct { peer };
        // An inbound message first leaves one unread…
        let im = inbound_im(0, b"Friendly Bot\0", b"hi there\0");
        session.handle_datagram(sim_addr(), &server_message(&im, 9, false)?, now)?;
        assert_eq!(session.unread(kind), 1);

        // …then our own reply logs as self (the login agent id) and clears unread.
        session.send_instant_message(peer, "hello back", now)?;
        let logged = history(&session, kind);
        assert_eq!(logged.len(), 2);
        let reply = logged.get(1).ok_or("expected our reply")?;
        assert_eq!(reply.sender, AgentKey::from(uuid::Uuid::from_u128(1)));
        assert_eq!(reply.dialog, ImDialog::Message);
        assert_eq!(reply.text, "hello back");
        assert_eq!(reply.timestamp, None);
        assert_eq!(session.unread(kind), 0);
        Ok(())
    }

    #[test]
    fn mark_session_read_resets_unread_keeping_history() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let peer = AgentKey::from(uuid::Uuid::from_u128(0x55));
        let kind = ChatSessionKind::Direct { peer };
        let im = inbound_im(0, b"Friendly Bot\0", b"hi\0");
        session.handle_datagram(sim_addr(), &server_message(&im, 9, false)?, now)?;
        assert_eq!(session.unread(kind), 1);

        session.mark_session_read(kind);
        assert_eq!(session.unread(kind), 0);
        // Marking read clears the counter but leaves the logged history intact.
        assert_eq!(history(&session, kind).len(), 1);
        Ok(())
    }

    #[test]
    fn history_is_capped_dropping_the_oldest() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let peer = AgentKey::from(uuid::Uuid::from_u128(0x55));
        let kind = ChatSessionKind::Direct { peer };
        // HISTORY_CAP is 256; sending one more (messages 0..=256) drops message 0.
        for index in 0..=256u32 {
            let text = format!("msg {index}\0");
            let im = inbound_im(0, b"Friendly Bot\0", text.as_bytes());
            let sequence = index.checked_add(100).ok_or("sequence overflow")?;
            session.handle_datagram(sim_addr(), &server_message(&im, sequence, false)?, now)?;
        }

        let logged = history(&session, kind);
        assert_eq!(logged.len(), 256);
        // The oldest surviving entry is message 1 (message 0 was evicted), and the
        // newest is message 256.
        assert_eq!(
            logged.first().ok_or("expected a first entry")?.text,
            "msg 1"
        );
        assert_eq!(
            logged.last().ok_or("expected a last entry")?.text,
            "msg 256"
        );
        Ok(())
    }

    #[test]
    fn offline_im_logs_wire_timestamp_and_bumps_unread() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let from = uuid::Uuid::from_u128(0x55);
        let kind = ChatSessionKind::Direct {
            peer: AgentKey::from(from),
        };
        // A stored offline IM (dialog 0 = Message) replayed over the CAPS path.
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

        let logged = history(&session, kind);
        assert_eq!(logged.len(), 1);
        let entry = logged.first().ok_or("expected a logged offline message")?;
        assert_eq!(entry.sender, AgentKey::from(from));
        assert_eq!(entry.text, "stored hello");
        assert_eq!(entry.timestamp, Some(1_700_000_000));
        assert_eq!(session.unread(kind), 1);
        Ok(())
    }

    #[test]
    fn total_unread_sums_across_sessions() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // Two inbound 1:1 messages from one peer and one group send: 3 unread.
        let im = inbound_im(0, b"Friendly Bot\0", b"hi\0");
        session.handle_datagram(sim_addr(), &server_message(&im, 9, false)?, now)?;
        let im2 = inbound_im(0, b"Friendly Bot\0", b"again\0");
        session.handle_datagram(sim_addr(), &server_message(&im2, 10, false)?, now)?;
        let group = uuid::Uuid::from_u128(0x6708);
        let send = inbound_group_im(17, uuid::Uuid::from_u128(0x6709), group);
        session.handle_datagram(sim_addr(), &server_message(&send, 11, true)?, now)?;

        assert_eq!(session.total_unread(), 3);

        // Reading the 1:1 (2 unread) leaves only the group's 1.
        session.mark_session_read(ChatSessionKind::Direct {
            peer: AgentKey::from(uuid::Uuid::from_u128(0x55)),
        });
        assert_eq!(session.total_unread(), 1);
        Ok(())
    }

    /// Like [`established`] but seeds the login buddy list, so the friend cache is
    /// populated for the presence read tests.
    fn established_with_friends(
        now: Instant,
        buddies: Vec<sl_wire::BuddyListEntry>,
    ) -> Result<Session, TestError> {
        let mut session = new_session()?;
        let LoginResponse::Success(mut login) = success()? else {
            return Err("expected a success response".into());
        };
        login.buddy_list = buddies;
        session.handle_login_response(LoginResponse::Success(login), now)?;
        drain(&mut session)?;
        drain_events(&mut session);
        let handshake = server_datagram(MessageId::Low(148), &[0u8; 600], 1, true);
        session.handle_datagram(sim_addr(), &handshake, now)?;
        drain(&mut session)?;
        drain_events(&mut session);
        Ok(session)
    }

    #[test]
    fn chat_sessions_info_lists_newest_first_and_flattens_invited() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // Oldest: a 1:1 direct session (one inbound IM → one unread).
        let peer = AgentKey::from(uuid::Uuid::from_u128(0x55));
        let direct_kind = ChatSessionKind::Direct { peer };
        let im = inbound_im(0, b"Friendly Bot\0", b"hi\0");
        session.handle_datagram(sim_addr(), &server_message(&im, 9, false)?, now)?;

        // Newer: a group session opened by an inbound send.
        let group = uuid::Uuid::from_u128(0x6708);
        let group_kind = ChatSessionKind::Group {
            group_id: GroupKey::from(group),
        };
        let send = inbound_group_im(17, uuid::Uuid::from_u128(0x6709), group);
        session.handle_datagram(
            sim_addr(),
            &server_message(&send, 10, true)?,
            after(now, 1_000)?,
        )?;

        // Newest: a still-pending conference invitation (lifecycle `Invited`).
        let conf = uuid::Uuid::from_u128(0x6801);
        let inviter = uuid::Uuid::from_u128(0x6802);
        let conf_kind = ChatSessionKind::Conference {
            id: ImSessionId::from(conf),
        };
        let conf_xml = format!(
            "<llsd><map>\
               <key>session_name</key><string>Chat</string>\
               <key>instantmessage</key><map><key>message_params</key><map>\
                 <key>id</key><uuid>{conf}</uuid>\
                 <key>from_id</key><uuid>{inviter}</uuid>\
                 <key>from_name</key><string>Inviter</string>\
                 <key>type</key><integer>16</integer>\
                 <key>from_group</key><boolean>0</boolean>\
               </map></map></map></llsd>"
        );
        session.handle_caps_event(
            "ChatterBoxInvitation",
            &parse_llsd_xml(&conf_xml)?,
            after(now, 2_000)?,
        )?;

        let infos: Vec<ChatSessionInfo> = session.chat_sessions_info().collect();
        // Newest-first: the invited conference, then the group, then the 1:1.
        let kinds: Vec<ChatSessionKind> = infos.iter().map(|info| info.kind).collect();
        assert_eq!(kinds, vec![conf_kind, group_kind, direct_kind]);

        // The conference's lifecycle is the flattened `Invited` view.
        let conf_info = infos.first().ok_or("conference info")?;
        assert_eq!(
            conf_info.lifecycle,
            ChatLifecycleView::Invited {
                inviter: AgentKey::from(inviter),
                session_name: "Chat".to_owned(),
                channel: InviteChannel::Text,
            }
        );
        assert!(conf_info.participants.is_empty());

        // The group is `Joined` and carries the inbound send's unread.
        let group_info = infos.get(1).ok_or("group info")?;
        assert_eq!(group_info.lifecycle, ChatLifecycleView::Joined);
        assert_eq!(group_info.unread, 1);

        // The 1:1 synthesises its roster as `{peer}` and carries the unread inbound.
        let direct_info = infos.get(2).ok_or("direct info")?;
        assert_eq!(direct_info.lifecycle, ChatLifecycleView::Joined);
        assert_eq!(direct_info.participants, vec![peer]);
        assert_eq!(direct_info.unread, 1);
        Ok(())
    }

    #[test]
    fn history_page_pages_newest_first_through_older_windows() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let peer = AgentKey::from(uuid::Uuid::from_u128(0x55));
        let kind = ChatSessionKind::Direct { peer };
        // Five inbound messages m0..m4, oldest-first in storage.
        for index in 0..5u32 {
            let text = format!("m{index}\0");
            let im = inbound_im(0, b"Friendly Bot\0", text.as_bytes());
            let sequence = index.checked_add(9).ok_or("sequence overflow")?;
            session.handle_datagram(sim_addr(), &server_message(&im, sequence, false)?, now)?;
        }

        // The newest page (limit 2) is m4, m3, with a cursor pointing older. Every
        // page is bounded to the requested window — never the whole history.
        let (page, prev) = session.history_page(kind, None, 2);
        let first: Vec<String> = page.map(|m| m.text.clone()).collect();
        assert_eq!(first, vec!["m4".to_owned(), "m3".to_owned()]);
        let prev = prev.ok_or("expected an older page after the newest two")?;

        // The next older window is m2, m1; still more remain.
        let (page, prev2) = session.history_page(kind, Some(prev), 2);
        let second: Vec<String> = page.map(|m| m.text.clone()).collect();
        assert_eq!(second, vec!["m2".to_owned(), "m1".to_owned()]);
        let prev2 = prev2.ok_or("expected one more older page")?;

        // The last window holds only m0 and ends the walk (no cursor).
        let (page, prev3) = session.history_page(kind, Some(prev2), 2);
        let third: Vec<String> = page.map(|m| m.text.clone()).collect();
        assert_eq!(third, vec!["m0".to_owned()]);
        assert!(
            prev3.is_none(),
            "the oldest in-memory message ends the in-memory walk"
        );
        Ok(())
    }

    #[test]
    fn history_page_on_unopened_session_is_empty() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let kind = ChatSessionKind::Direct {
            peer: AgentKey::from(uuid::Uuid::from_u128(0x99)),
        };
        let (page, prev) = session.history_page(kind, None, 10);
        assert_eq!(page.count(), 0);
        assert!(prev.is_none());
        Ok(())
    }

    #[test]
    fn friends_presence_reports_each_friends_online_flag() -> Result<(), TestError> {
        let now = Instant::now();
        let friend_a = uuid::Uuid::from_u128(0xF1);
        let friend_b = uuid::Uuid::from_u128(0xF2);
        let mut session = established_with_friends(
            now,
            vec![
                sl_wire::BuddyListEntry {
                    buddy_id: friend_a,
                    rights_granted: FriendRights::CAN_SEE_ONLINE,
                    rights_has: FriendRights::CAN_SEE_ONLINE,
                },
                sl_wire::BuddyListEntry {
                    buddy_id: friend_b,
                    rights_granted: 0,
                    rights_has: 0,
                },
            ],
        )?;

        // Only friend_a is reported online.
        let online = AnyMessage::OnlineNotification(OnlineNotification {
            agent_block: vec![OnlineNotificationAgentBlockBlock { agent_id: friend_a }],
        });
        session.handle_datagram(sim_addr(), &server_message(&online, 9, true)?, now)?;

        let snapshot: Vec<FriendPresence> = session.friends_presence().collect();
        // Ordered by friend id; friend_a online, friend_b not.
        let ids: Vec<FriendKey> = snapshot.iter().map(|p| p.friend.id).collect();
        assert_eq!(
            ids,
            vec![FriendKey::from(friend_a), FriendKey::from(friend_b)]
        );
        let flags: Vec<bool> = snapshot.iter().map(|p| p.online).collect();
        assert_eq!(flags, vec![true, false]);
        Ok(())
    }

    #[test]
    fn chat_sessions_reply_shares_an_arc_without_deep_copy() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        let im = inbound_im(0, b"Friendly Bot\0", b"hi\0");
        session.handle_datagram(sim_addr(), &server_message(&im, 9, false)?, now)?;

        // The reply payload is an `Arc<[…]>`; both runtimes build it from this one
        // sans-IO builder, so a bevy-direct read and a tokio reply carry identical
        // data by construction.
        let infos: std::sync::Arc<[ChatSessionInfo]> = session.chat_sessions_info().collect();
        let direct: Vec<ChatSessionInfo> = session.chat_sessions_info().collect();
        assert_eq!(infos.as_ref(), direct.as_slice());

        // Handing the payload across the channel is an `Arc` clone, never a deep
        // copy: the shipped event shares the same allocation.
        let event = Event::ChatSessions(std::sync::Arc::clone(&infos));
        let Event::ChatSessions(shipped) = &event else {
            return Err("expected a ChatSessions event".into());
        };
        assert!(std::sync::Arc::ptr_eq(&infos, shipped));
        Ok(())
    }

    /// The `QueryFriends` and `QueryChatHistoryPage` reply payloads are also
    /// `Arc<[…]>` shared across the channel, never deep-copied — the friends and
    /// history-page counterparts to the `ChatSessions` assertion above. Together
    /// the three cover every chat/presence query command.
    #[test]
    fn friends_and_history_replies_share_an_arc_without_deep_copy() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established_with_friends(
            now,
            vec![sl_wire::BuddyListEntry {
                buddy_id: uuid::Uuid::from_u128(0xF1),
                rights_granted: FriendRights::CAN_SEE_ONLINE,
                rights_has: FriendRights::CAN_SEE_ONLINE,
            }],
        )?;
        drain(&mut session)?;

        // The friends snapshot: built once, shipped as an `Arc` clone.
        let friends: std::sync::Arc<[FriendPresence]> = session.friends_presence().collect();
        let friends_direct: Vec<FriendPresence> = session.friends_presence().collect();
        assert_eq!(friends.as_ref(), friends_direct.as_slice());
        let friends_event = Event::FriendsSnapshot(std::sync::Arc::clone(&friends));
        let Event::FriendsSnapshot(shipped) = &friends_event else {
            return Err("expected a FriendsSnapshot event".into());
        };
        assert!(std::sync::Arc::ptr_eq(&friends, shipped));

        // A history page: the bounded window is an `Arc` clone too.
        let peer = AgentKey::from(uuid::Uuid::from_u128(0x55));
        let kind = ChatSessionKind::Direct { peer };
        let im = inbound_im(0, b"Friendly Bot\0", b"hi\0");
        session.handle_datagram(sim_addr(), &server_message(&im, 9, false)?, now)?;
        let (page, prev) = session.history_page(kind, None, 10);
        let messages: std::sync::Arc<[SessionMessage]> = page.cloned().collect();
        let page_event = Event::ChatHistoryPage {
            session: kind,
            messages: std::sync::Arc::clone(&messages),
            prev,
        };
        let Event::ChatHistoryPage {
            messages: shipped, ..
        } = &page_event
        else {
            return Err("expected a ChatHistoryPage event".into());
        };
        assert!(std::sync::Arc::ptr_eq(&messages, shipped));
        Ok(())
    }

    /// The B8 inventory pull-bridge: both runtimes synthesize the reply from the
    /// same sans-IO read methods. A folder query yields the paged view-type
    /// window + cursor as an `Arc<[…]>` shared across the channel (never a deep
    /// copy); a roots query echoes the typed accessors; a query for an unfetched
    /// (`Unknown`) folder schedules its on-demand fetch (the bridge's
    /// `RequestFolderContents` step, `Unknown → Fetching`); and a `&Session`
    /// reader walks the cache borrowed, with no clone (the bevy-direct path).
    #[test]
    fn b8_inventory_pull_bridge_synthesizes_page_and_roots() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = established(now)?;
        drain(&mut session)?;

        // Seed a `Loaded` folder with three sub-folders and two items.
        let parent = 0xD0;
        let parent_key = InventoryFolderKey::from(uuid::Uuid::from_u128(parent));
        feed_descendents(
            &mut session,
            now,
            parent,
            4,
            vec![
                desc_folder(0xD1, parent, -1, "Alpha"),
                desc_folder(0xD2, parent, -1, "Beta"),
                desc_folder(0xD3, parent, -1, "Gamma"),
            ],
            vec![
                desc_item(0xE1, parent, 7, 7, 0, 0, "Note"), // notecard, not for sale
                desc_item(0xE2, parent, 6, 6, 2, 250, "Cube"), // object, copy sale
            ],
            7,
        )?;

        // The page the bridge ships: the bounded window from
        // `inventory_folder_page`, materialised into `Arc<[…]>` exactly as both
        // runtime dispatch arms do (one borrow→owned transform at the channel).
        let (folders, items, prev) = session.inventory_folder_page(parent_key, None, 4);
        let folders: std::sync::Arc<[FolderInfo]> = folders.into();
        let items: std::sync::Arc<[ItemInfo]> = items.into();
        let page_event = Event::InventoryFolderPage {
            folder: parent_key,
            folders: std::sync::Arc::clone(&folders),
            items: std::sync::Arc::clone(&items),
            prev,
        };
        let Event::InventoryFolderPage {
            folder,
            folders: shipped_folders,
            items: shipped_items,
            prev: shipped_prev,
        } = &page_event
        else {
            return Err("expected an InventoryFolderPage event".into());
        };
        assert_eq!(*folder, parent_key);
        // Handing the window across the channel is an `Arc` clone, never a deep
        // copy: the shipped event shares the same allocation.
        assert!(std::sync::Arc::ptr_eq(&folders, shipped_folders));
        assert!(std::sync::Arc::ptr_eq(&items, shipped_items));
        // The limit of 4 fills with all 3 sub-folders + the first item; the
        // second item carries over to the next page.
        assert_eq!(shipped_folders.len(), 3);
        assert_eq!(shipped_items.len(), 1);
        let cursor = shipped_prev.ok_or("expected a continuation cursor")?;
        assert_eq!(cursor.consumed_count(), 4);
        // The view types resolve the raw bytes into typed enums.
        let alpha = shipped_folders.first().ok_or("alpha folder")?;
        assert_eq!(
            alpha.folder_id,
            InventoryFolderKey::from(uuid::Uuid::from_u128(0xD1))
        );
        assert_eq!(alpha.state, FolderState::Unknown); // a sub-folder, unfetched
        let note = shipped_items.first().ok_or("note item")?;
        assert_eq!(note.asset_type, AssetType::Notecard);

        // The roots reply echoes the typed accessors (both `Copy` keys, no `Arc`).
        let roots_event = Event::InventoryRoots {
            agent_root: session.inventory_root(),
            library_root: session.library_root(),
        };
        let Event::InventoryRoots {
            agent_root,
            library_root,
        } = &roots_event
        else {
            return Err("expected an InventoryRoots event".into());
        };
        assert_eq!(*agent_root, session.inventory_root());
        assert_eq!(*library_root, session.library_root());

        // On-demand: a query for an `Unknown` sub-folder schedules its fetch (the
        // bridge's `RequestFolderContents` step), flipping `Unknown → Fetching`.
        let alpha_key = InventoryFolderKey::from(uuid::Uuid::from_u128(0xD1));
        assert_eq!(
            session.folder_fetch_state(alpha_key),
            Some(FolderState::Unknown)
        );
        session.request_folder_contents(alpha_key, now)?;
        assert_eq!(
            session.folder_fetch_state(alpha_key),
            Some(FolderState::Fetching)
        );

        // The direct-borrow reader (the bevy path) walks the cache borrowed, with
        // no clone: `inventory_children` yields `Child<'_>` straight out of an
        // immutable `&Session`, no owned copy of the tree.
        let reader: &Session = &session;
        let borrowed: usize = reader.inventory_children(parent_key).count();
        assert_eq!(borrowed, 5);
        Ok(())
    }

    // ---- Persistence / region guard (B10) -----------------------------------
    //
    // The grid-level chat/presence stores (`chat_sessions` / `friends` /
    // `online`) are routed by the grid's IM / group / presence services, never
    // by the region simulator, so they survive every region boundary — the exact
    // inverse of `teleport_clears_seat` (which proves the *region-local* seat is
    // dropped). These tests seed the stores, drive each of the four reset sites,
    // and assert nothing changed.

    /// The 1:1 peer the persistence fixture seeds (its inbound IM's sender id).
    const GUARD_PEER: u128 = 0x55;
    /// The buddy the fixture marks online.
    const GUARD_FRIEND: u128 = 0xF1;
    /// The conference the fixture seeds a roster in.
    const GUARD_CONF: u128 = 0xABC;
    /// The agent who is both a conference participant and typing in a 1:1.
    const GUARD_TYPER: u128 = 0xF3;

    /// Seeds the grid-level chat/presence stores on a fresh active session: a 1:1
    /// direct session (one inbound IM → history + unread), a conference roster
    /// member who is also typing in a separate 1:1, and a buddy who is online.
    fn seed_chat_and_presence(now: Instant) -> Result<Session, TestError> {
        let mut session = established_with_friends(
            now,
            vec![sl_wire::BuddyListEntry {
                buddy_id: uuid::Uuid::from_u128(GUARD_FRIEND),
                rights_granted: FriendRights::CAN_SEE_ONLINE,
                rights_has: FriendRights::CAN_SEE_ONLINE,
            }],
        )?;
        drain(&mut session)?;
        drain_events(&mut session);

        // A 1:1 direct session with one inbound message (history + unread).
        let im = inbound_im(0, b"Friendly Bot\0", b"hi there\0");
        session.handle_datagram(sim_addr(), &server_message(&im, 9, false)?, now)?;

        // A conference roster member (SessionAdd, dialog 13) who is also typing in
        // a separate 1:1 (open it with a message, then TypingStart, dialog 41 —
        // both keyed by the sender, so they come *from* the typer).
        let typer = uuid::Uuid::from_u128(GUARD_TYPER);
        let conference = uuid::Uuid::from_u128(GUARD_CONF);
        let add = inbound_offer_im(13, typer, conference, Vec::new());
        session.handle_datagram(sim_addr(), &server_message(&add, 10, false)?, now)?;
        let open = inbound_offer_im(0, typer, uuid::Uuid::nil(), Vec::new());
        session.handle_datagram(sim_addr(), &server_message(&open, 11, false)?, now)?;
        let start = inbound_offer_im(41, typer, uuid::Uuid::nil(), Vec::new());
        session.handle_datagram(sim_addr(), &server_message(&start, 12, false)?, now)?;

        // The buddy comes online.
        let online = AnyMessage::OnlineNotification(OnlineNotification {
            agent_block: vec![OnlineNotificationAgentBlockBlock {
                agent_id: uuid::Uuid::from_u128(GUARD_FRIEND),
            }],
        });
        session.handle_datagram(sim_addr(), &server_message(&online, 13, true)?, now)?;
        drain(&mut session)?;
        drain_events(&mut session);
        Ok(session)
    }

    /// Asserts the full `seed_chat_and_presence` seed is present and unchanged —
    /// after a region transition (which must leave the grid-level stores intact)
    /// or on a logged-out session (which keeps them readable).
    fn assert_chat_and_presence_intact(session: &Session) -> Result<(), TestError> {
        let direct = ChatSessionKind::Direct {
            peer: AgentKey::from(uuid::Uuid::from_u128(GUARD_PEER)),
        };
        let conf = ChatSessionKind::Conference {
            id: ImSessionId::from(uuid::Uuid::from_u128(GUARD_CONF)),
        };
        let typer = AgentKey::from(uuid::Uuid::from_u128(GUARD_TYPER));
        let typer_direct = ChatSessionKind::Direct { peer: typer };

        // The 1:1 history and unread survive.
        assert_eq!(history(session, direct).len(), 1, "1:1 history survives");
        assert_eq!(session.unread(direct), 1, "1:1 unread survives");
        // The conference roster and the 1:1 typing survive.
        assert_eq!(participants(session, conf), vec![typer], "roster survives");
        assert_eq!(
            typers(session, typer_direct),
            vec![typer],
            "typing survives"
        );
        // The buddy presence and the buddy cache survive.
        assert!(
            session.is_online(FriendKey::from(uuid::Uuid::from_u128(GUARD_FRIEND))),
            "friend presence survives"
        );
        assert_eq!(session.friends().count(), 1, "the friend cache survives");
        Ok(())
    }

    /// A real (cross-region) teleport via `TeleportFinish` (`begin_handover`)
    /// leaves every chat/presence store untouched — the inverse of
    /// `teleport_clears_seat`.
    #[test]
    fn teleport_preserves_chat_and_presence() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = seed_chat_and_presence(now)?;

        let handle = 0x0003_E900_0003_E800;
        session.teleport_to(
            RegionHandle(handle),
            region_coords(128.0, 128.0, 30.0),
            vec3(1.0, 0.0, 0.0),
            now,
        )?;
        drain(&mut session)?;
        drain_events(&mut session);
        let finish = AnyMessage::TeleportFinish(TeleportFinish {
            info: TeleportFinishInfoBlock {
                agent_id: uuid::Uuid::from_u128(1),
                location_id: 4,
                sim_ip: [127, 0, 0, 1],
                sim_port: 9100u16.swap_bytes(),
                region_handle: handle,
                seed_capability: b"http://x/chatTP\0".to_vec(),
                sim_access: sl_wire::sim_access::MATURE,
                teleport_flags: TeleportFlags::VIA_LURE,
            },
        });
        session.handle_datagram(sim_addr(), &server_message(&finish, 20, true)?, now)?;

        assert_chat_and_presence_intact(&session)
    }

    /// A neighbour crossing (`CrossedRegion` → `promote_child_to_root`) keeps the
    /// chat/presence stores, like the seat it carries across the border.
    #[test]
    fn neighbour_crossing_preserves_chat_and_presence() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = seed_chat_and_presence(now)?;

        enable_neighbour_b(&mut session, 20, now)?;
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
        session.handle_datagram(sim_addr(), &server_message(&crossed, 21, true)?, now)?;
        while session.poll_transmit().is_some() {}
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
        drain_events(&mut session);

        assert_chat_and_presence_intact(&session)
    }

    /// An intra-region teleport (`TeleportLocal`) leaves the chat/presence stores
    /// untouched (it only unseats the agent and drops in-world grants).
    #[test]
    fn local_teleport_preserves_chat_and_presence() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = seed_chat_and_presence(now)?;

        // Begin a teleport so the session is `Teleporting`, then the simulator
        // answers with a `TeleportLocal` (same-region). The handler reads no
        // fields, so a well-formed zeroed Info block is enough.
        session.teleport_to(
            RegionHandle(0x0003_E800_0003_E800),
            region_coords(64.0, 64.0, 30.0),
            vec3(1.0, 0.0, 0.0),
            now,
        )?;
        drain(&mut session)?;
        drain_events(&mut session);
        let local = server_datagram(MessageId::Low(64), &[0u8; 48], 20, true);
        session.handle_datagram(sim_addr(), &local, now)?;
        drain_events(&mut session);

        assert_chat_and_presence_intact(&session)
    }

    /// The folder the inventory-survival fixture loads with children.
    const GUARD_INV_FOLDER: u128 = 0x0001_0000;
    /// A sub-folder indexed under the loaded folder.
    const GUARD_INV_SUBFOLDER: u128 = 0x0001_0001;
    /// An item indexed under the loaded folder.
    const GUARD_INV_ITEM: u128 = 0x0001_00D1;
    /// The authoritative version the fixture's descendents reply carries.
    const GUARD_INV_VERSION: i32 = 9;

    /// Seeds a `Loaded` inventory folder (one sub-folder and one item indexed
    /// under it) on a fresh active session — the inventory mirror of
    /// `seed_chat_and_presence` for the grid-level held model.
    fn seed_loaded_inventory(now: Instant) -> Result<Session, TestError> {
        let mut session = established(now)?;
        drain(&mut session)?;
        drain_events(&mut session);
        feed_descendents(
            &mut session,
            now,
            GUARD_INV_FOLDER,
            GUARD_INV_VERSION,
            vec![desc_folder(
                GUARD_INV_SUBFOLDER,
                GUARD_INV_FOLDER,
                6,
                "Objects",
            )],
            vec![desc_item(
                GUARD_INV_ITEM,
                GUARD_INV_FOLDER,
                7,
                7,
                0,
                0,
                "a notecard",
            )],
            9,
        )?;
        Ok(session)
    }

    /// Asserts the full `seed_loaded_inventory` seed is present and unchanged —
    /// after a region transition (which must leave the grid-level inventory model
    /// intact, like the chat/presence stores).
    fn assert_inventory_intact(session: &Session) -> Result<(), TestError> {
        let folder = InventoryFolderKey::from(uuid::Uuid::from_u128(GUARD_INV_FOLDER));
        let sub = InventoryFolderKey::from(uuid::Uuid::from_u128(GUARD_INV_SUBFOLDER));
        let item = InventoryKey::from(uuid::Uuid::from_u128(GUARD_INV_ITEM));
        // The loaded folder keeps its authoritative version (it is not refetched).
        assert_eq!(
            session.folder_fetch_state(folder),
            Some(FolderState::Loaded {
                version: GUARD_INV_VERSION
            }),
            "the loaded folder survives"
        );
        // Its parent→children index survives: the sub-folder and the item.
        let (child_folders, child_items) = split_children(session, folder);
        assert_eq!(child_folders.len(), 1, "the sub-folder survives");
        assert_eq!(child_folders.first().ok_or("sub-folder")?.folder_id, sub);
        assert_eq!(child_items.len(), 1, "the item survives");
        assert_eq!(child_items.first().ok_or("item")?.item_id, item);
        // The child metadata (carried by the descendents reply) remains directly
        // addressable — the sub-folder by name and the item by id.
        assert_eq!(
            session
                .inventory_folder(sub)
                .ok_or("sub-folder metadata")?
                .name,
            "Objects"
        );
        assert!(
            session.inventory_item(item).is_some(),
            "the item metadata survives"
        );
        Ok(())
    }

    /// A real (cross-region) teleport via `TeleportFinish` (`begin_handover`)
    /// leaves the grid-level inventory model untouched — the loaded tree, its
    /// `Loaded` version, and the parent→children index all survive (INVENTORY
    /// A10/B3, the inventory mirror of `teleport_preserves_chat_and_presence`).
    #[test]
    fn teleport_preserves_inventory() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = seed_loaded_inventory(now)?;

        let handle = 0x0003_E900_0003_E800;
        session.teleport_to(
            RegionHandle(handle),
            region_coords(128.0, 128.0, 30.0),
            vec3(1.0, 0.0, 0.0),
            now,
        )?;
        drain(&mut session)?;
        drain_events(&mut session);
        let finish = AnyMessage::TeleportFinish(TeleportFinish {
            info: TeleportFinishInfoBlock {
                agent_id: uuid::Uuid::from_u128(1),
                location_id: 4,
                sim_ip: [127, 0, 0, 1],
                sim_port: 9100u16.swap_bytes(),
                region_handle: handle,
                seed_capability: b"http://x/invTP\0".to_vec(),
                sim_access: sl_wire::sim_access::MATURE,
                teleport_flags: TeleportFlags::VIA_LURE,
            },
        });
        session.handle_datagram(sim_addr(), &server_message(&finish, 20, true)?, now)?;

        assert_inventory_intact(&session)
    }

    /// An intra-region teleport (`TeleportLocal`) likewise leaves the grid-level
    /// inventory model untouched.
    #[test]
    fn local_teleport_preserves_inventory() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = seed_loaded_inventory(now)?;

        session.teleport_to(
            RegionHandle(0x0003_E800_0003_E800),
            region_coords(64.0, 64.0, 30.0),
            vec3(1.0, 0.0, 0.0),
            now,
        )?;
        drain(&mut session)?;
        drain_events(&mut session);
        let local = server_datagram(MessageId::Low(64), &[0u8; 48], 20, true);
        session.handle_datagram(sim_addr(), &local, now)?;
        drain_events(&mut session);

        assert_inventory_intact(&session)
    }

    /// Retiring a child circuit (`DisableSimulator`) touches only that neighbour's
    /// region-local caches; the grid-level chat/presence stores are untouched.
    #[test]
    fn disable_simulator_preserves_chat_and_presence() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = seed_chat_and_presence(now)?;

        enable_neighbour_b(&mut session, 20, now)?;
        while session.poll_transmit().is_some() {}
        let disable = server_datagram(MessageId::Low(152), &[], 1, true);
        session.handle_datagram(sim_b(), &disable, now)?;

        assert_chat_and_presence_intact(&session)
    }

    /// Logout is terminal, not a reset: the chat/presence stores stay readable on
    /// the `Closed` session so the user can inspect the conversation from just
    /// before logout. A relogin builds a fresh, empty `Session`. (The read
    /// accessors are pure getters that do not gate on `state`; this pins it.)
    #[test]
    fn logout_keeps_chat_and_presence_readable() -> Result<(), TestError> {
        let now = Instant::now();
        let mut session = seed_chat_and_presence(now)?;

        session.initiate_logout(now);
        drain(&mut session)?;
        // LogoutReply Low 253: AgentData (2 uuids) + InventoryData variable (count).
        let reply = server_datagram(MessageId::Low(253), &[0u8; 33], 30, true);
        session.handle_datagram(sim_addr(), &reply, now)?;
        assert!(session.is_closed(), "the session is closed after logout");

        // Everything seeded before logout is still readable on the closed session.
        assert_chat_and_presence_intact(&session)?;

        // A relogin constructs a fresh session that starts empty — there is no
        // logout-time clearing; the stores die only with the dropped struct.
        let fresh = new_session()?;
        assert_eq!(fresh.chat_sessions().count(), 0, "a fresh session is empty");
        assert_eq!(fresh.friends().count(), 0, "a fresh session has no friends");
        assert_eq!(
            fresh.online_friends().count(),
            0,
            "a fresh session has no presence"
        );
        Ok(())
    }
}
