//! In-memory loopback tests: a simulator-side [`SimSession`] driven against a
//! client-side [`Session`] through the real framing/ack/zerocode path, plus
//! focused unit tests of the [`SimSession`] inputs in isolation.

#[cfg(test)]
mod test {
    use std::error::Error;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::time::{Duration, Instant};

    use pretty_assertions::assert_eq;
    use sl_proto::{
        AbuseReport, AbuseReportType, AgentKey, AlertInfo, AnimationKey, AssetKey, AssetType,
        AttachmentMode, AttachmentPoint, AvatarName, AvatarPickerResult, ChatChannel, ChatSource,
        ChatType, ClassifiedCategory, ClassifiedKey, CoarseLocation, ControlFlags, DetachOrder,
        DirClassifiedResult, DirEventResult, DirFindFlags, DirGroupResult, DirLandResult,
        DirPeopleResult, DirPlaceResult, DirectoryVisibility, DisplayName, DisplayNameUpdate,
        EjectAction, EstateCovenant, Event, EventId, EventInfo, FeatureDisabled, FollowCamProperty,
        FollowCamPropertyValue, FreezeAction, FriendKey, FriendRights, GenericMessage,
        GenericStreamingMessage, GestureActivation, GlobalCoordinates, GodRegionUpdate,
        GridCoordinates, GridRectangle, GroupAccountDetails, GroupAccountDetailsEntry,
        GroupAccountSummary, GroupAccountTransaction, GroupAccountTransactions,
        GroupActiveProposalItem, GroupKey, GroupName, GroupRequestId, GroupRoleKey, GroupVote,
        GroupVoteHistoryItem, ImDialog, InstantMessage, InventoryFolderKey, InventoryItem,
        InventoryItemMove, InventoryItemOrFolderKey, InventoryKey, InventoryType, InvoiceId, Kick,
        LandArea, LandBrushAction, LandBrushSize, LandEdit, LandSearchType, LandStatItem,
        LandStatReportType, LandingType, LightData, LindenAmount, LindenBalance, LoginParams,
        MAX_FACES, MapItem, MapItemType, MapLayer, MapRegionInfo, MapRequestFlags, Maturity,
        MeanCollision, MeanCollisionType, MovementMode, NavMeshBuildStatus, NavMeshStatus,
        NewInventoryLink, NotecardRez, ObjectBuyItem, ObjectExtraParams, ObjectKey,
        ObjectPlayingAnimation, ObjectPropertiesFamily, OpenRegionInfo, OwnerKey, ParcelCategory,
        ParcelDetails, ParcelInfo, ParcelKey, ParcelObjectOwner, ParcelRequestResult,
        ParcelReturnType, ParcelStatus, Permissions, Permissions5, PingId, PlacesResult,
        PointAtType, Postcard, PrimShapeParams, ProductType, QueryId, RegionCoordinates,
        RegionHandle, RegionIdentity, RegionLocalObjectId, RegionLocalParcelId, RegionStats,
        RegionTerrainComposition, RejectionReason, RequiredVoiceVersion, RestoreItem,
        RezAttachment, RezObjectParams, RezScriptParams, SaleType, ScopedObjectId, ScopedParcelId,
        ScriptControl, ScriptControlAction, ScriptPermissionRequest, ScriptPermissionStatus,
        ScriptPermissions, ServerError, ServerEvent, Session, SetDisplayNameReply, SimSession,
        SimStatId, SimWideDeleteFlags, SimulatorTime, SitTransform, StartLocationSlot,
        TERRAIN_PATCHES_PER_MESSAGE, TaskInventoryItem, TaskInventoryKey, TaskInventoryReply,
        TelehubInfo, TerraformArea, TerrainLayerType, TerrainPatch, TextureEntry, TextureFace,
        TextureKey, Throttle, TransactionId, TransferId, TransferRequestSource, TransferStatus,
        Transmit, UpdateGroupInfoParams, UserInfo, ViewerEffect, ViewerEffectData,
        ViewerEffectType, XferId, enable_simulator_to_caps_llsd, parse_event_queue_response,
    };
    use sl_proto::{
        AgentPresence, FlowMirrorStatus, SESSION_FLOW_COVERAGE, SimChatSessionKind, UserRightsEntry,
    };
    use sl_proto::{
        ChatLifecycleView, ChatSessionKind, ImSessionId, InviteChannel, Reliability,
        chatterbox_invitation_to_llsd,
    };
    use sl_proto::{STANDARD_REGION_SIZE_METRES, TELEPORT_FINISH_LOCATION_ID, TeleportFinishInfo};
    use sl_wire::messages::{
        AbortXfer, AbortXferXferIDBlock, CompleteAgentMovement,
        CompleteAgentMovementAgentDataBlock, CompletePingCheck, CompletePingCheckPingIDBlock,
        EstateOwnerMessage, EstateOwnerMessageAgentDataBlock, EstateOwnerMessageMethodDataBlock,
        EstateOwnerMessageParamListBlock, ImprovedInstantMessage,
        ImprovedInstantMessageAgentDataBlock, ImprovedInstantMessageEstateBlockBlock,
        ImprovedInstantMessageMessageBlockBlock, OfflineNotification,
        OfflineNotificationAgentBlockBlock, OnlineNotification, OnlineNotificationAgentBlockBlock,
        PacketAck, PacketAckPacketsBlock, SendXferPacket, SendXferPacketDataPacketBlock,
        SendXferPacketXferIDBlock, StartPingCheck, StartPingCheckPingIDBlock, TransferRequest,
        TransferRequestTransferInfoBlock, UseCircuitCode, UseCircuitCodeCircuitCodeBlock,
    };
    use sl_wire::{
        AnyMessage, CircuitCode, LoginRequest, LoginResponse, LoginSuccess, MessageId, PacketFlags,
        Reader, SequenceNumber, StartLocation, Writer, encode_datagram, parse_datagram,
    };

    /// A boxed test error.
    type TestError = Box<dyn Error>;

    /// Wrap a (valid) region name for a test fixture (`None` if it does not
    /// satisfy the region-name grammar, which the fixtures never trip).
    fn region(name: &str) -> Option<sl_proto::RegionName> {
        sl_proto::region_name_from_wire("test", name).ok().flatten()
    }

    /// The region handle the simulator serves throughout these tests.
    const REGION_HANDLE: u64 = 0x0000_03e8_0000_03e8;

    /// The simulator's UDP address (matches the [`success`] login fixture).
    fn sim_addr() -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 9000)
    }

    /// The client's UDP address, as the simulator sees it.
    fn client_addr() -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 40000)
    }

    /// `now + millis`, for advancing the simulated clock.
    fn after(now: Instant, millis: u64) -> Result<Instant, TestError> {
        now.checked_add(Duration::from_millis(millis))
            .ok_or_else(|| "clock overflow".into())
    }

    /// A fresh client session pointing at the test simulator.
    fn new_client() -> Result<Session, TestError> {
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
        Ok(LoginResponse::Success(Box::new(LoginSuccess::minimal(
            AgentKey::from(uuid::Uuid::from_u128(1)),
            uuid::Uuid::from_u128(2),
            uuid::Uuid::from_u128(3),
            CircuitCode(0x0011_2233),
            Ipv4Addr::new(127, 0, 0, 1),
            9000,
            "http://127.0.0.1:9000/seed".parse()?,
        ))))
    }

    /// Builds an inbound datagram carrying a fully encoded client message.
    fn client_datagram(
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

    /// Decodes the message carried by a transmitted datagram.
    fn decode(transmit: &Transmit) -> Result<AnyMessage, TestError> {
        let parsed = parse_datagram(&transmit.payload)?;
        let mut reader = Reader::new(parsed.body);
        let id = MessageId::decode(&mut reader)?;
        Ok(AnyMessage::decode(id, &mut reader)?)
    }

    /// Delivers all queued datagrams between a client and simulator (in both
    /// directions) at explicit endpoint addresses until neither has anything
    /// more to send — the address-parameterized core of [`pump`], shared with
    /// the two-avatar [`PairEnd`] topology.
    fn pump_at(
        client: &mut Session,
        sim: &mut SimSession,
        client_addr: SocketAddr,
        sim_addr: SocketAddr,
        now: Instant,
    ) -> Result<(), TestError> {
        loop {
            let mut moved = false;
            while let Some(transmit) = client.poll_transmit() {
                sim.handle_datagram(client_addr, &transmit.payload, now)?;
                moved = true;
            }
            while let Some(transmit) = sim.poll_transmit() {
                client.handle_datagram(sim_addr, &transmit.payload, now)?;
                moved = true;
            }
            if !moved {
                break;
            }
        }
        Ok(())
    }

    /// Delivers all queued datagrams between the client and simulator (in both
    /// directions) until neither has anything more to send.
    fn pump(client: &mut Session, sim: &mut SimSession, now: Instant) -> Result<(), TestError> {
        pump_at(client, sim, client_addr(), sim_addr(), now)
    }

    /// Drains all queued server events.
    fn drain_server(sim: &mut SimSession) -> Vec<ServerEvent> {
        let mut out = Vec::new();
        while let Some(event) = sim.poll_event() {
            out.push(event);
        }
        out
    }

    /// Drains all queued client events.
    fn drain_client(client: &mut Session) -> Vec<Event> {
        let mut out = Vec::new();
        while let Some(event) = client.poll_event() {
            out.push(event);
        }
        out
    }

    /// Delivers all queued datagrams between the client and SEVERAL simulators
    /// — the multi-region topology an inter-region teleport needs — routing
    /// each client transmit to the simulator whose address matches its
    /// [`Transmit::destination`], until nothing moves. The single-sim
    /// [`pump`] stays as the common fast path.
    fn pump_multi(
        client: &mut Session,
        sims: &mut [(SocketAddr, &mut SimSession)],
        now: Instant,
    ) -> Result<(), TestError> {
        loop {
            let mut moved = false;
            while let Some(transmit) = client.poll_transmit() {
                for (addr, sim) in sims.iter_mut() {
                    if *addr == transmit.destination {
                        sim.handle_datagram(client_addr(), &transmit.payload, now)?;
                        moved = true;
                    }
                }
            }
            for (addr, sim) in sims.iter_mut() {
                while let Some(transmit) = sim.poll_transmit() {
                    client.handle_datagram(*addr, &transmit.payload, now)?;
                    moved = true;
                }
            }
            if !moved {
                break;
            }
        }
        Ok(())
    }

    /// Delivers the simulator's queued CAPS events to the client over the real
    /// `EventQueueGet` long-poll path — drain the queue into the response XML,
    /// parse it, and feed each `{message, body}` to the client's CAPS dispatch —
    /// then returns the resulting client events. This is the event-queue mirror
    /// of [`pump`], which carries UDP datagrams.
    fn deliver_caps(
        client: &mut Session,
        sim: &mut SimSession,
        now: Instant,
    ) -> Result<Vec<Event>, TestError> {
        let xml = sim
            .take_event_queue_response()
            .ok_or("the simulator queued at least one CAPS event")?;
        for event in parse_event_queue_response(&xml)?.events {
            client.handle_caps_event(&event.message, &event.body, now)?;
        }
        Ok(drain_client(client))
    }

    /// Logs a client in and drives both peers through circuit setup and arrival,
    /// returning the active pair.
    fn setup(now: Instant) -> Result<(Session, SimSession), TestError> {
        let mut client = new_client()?;
        client.handle_login_response(success()?, now)?;
        // The client defers `CompleteAgentMovement` until its driver reports the
        // region's capabilities are ready; release it so the sim sees the arrival.
        client.notify_capabilities_ready(now)?;
        let mut sim = SimSession::new(RegionHandle(REGION_HANDLE), now);
        pump(&mut client, &mut sim, now)?;
        Ok((client, sim))
    }

    /// The login-fixture identity for one end of a two-avatar pair.
    struct EndParams {
        /// The avatar's first name (the last name is always "User").
        first_name: &'static str,
        /// The agent id (as a `u128`).
        agent: u128,
        /// The session id (as a `u128`).
        session: u128,
        /// The secure session id (as a `u128`).
        secure: u128,
        /// The circuit code.
        circuit: u32,
        /// The simulator's UDP port on 127.0.0.1.
        sim_port: u16,
        /// The client's UDP port on 127.0.0.1, as the simulator sees it.
        client_port: u16,
    }

    /// One avatar's client `Session` wired to its **own** [`SimSession`] — the
    /// two-avatar relay topology (one simulator session per client; the test
    /// body plays the driver, relaying `ServerEvent`s off one end's sim into
    /// `send_*` calls on the other's).
    struct PairEnd {
        /// The avatar's client session.
        client: Session,
        /// The simulator session serving this client.
        sim: SimSession,
        /// The client's UDP address, as the simulator sees it.
        client_addr: SocketAddr,
        /// The simulator's UDP address.
        sim_addr: SocketAddr,
    }

    /// [`pump`] for one [`PairEnd`]: delivers this end's queued datagrams in
    /// both directions until quiet.
    fn pump_end(end: &mut PairEnd, now: Instant) -> Result<(), TestError> {
        pump_at(
            &mut end.client,
            &mut end.sim,
            end.client_addr,
            end.sim_addr,
            now,
        )
    }

    /// Logs one avatar in against its own fresh [`SimSession`] (the
    /// [`setup`] dance, parameterized by [`EndParams`]) and returns the
    /// active [`PairEnd`].
    fn setup_end(params: &EndParams, now: Instant) -> Result<PairEnd, TestError> {
        let mut client = Session::new(LoginParams {
            login_uri: format!("http://127.0.0.1:{}/", params.sim_port).parse()?,
            request: LoginRequest::new(
                params.first_name,
                "User",
                "secret",
                StartLocation::Last,
                "MyViewer",
                "1.2.3",
            ),
        });
        client.handle_login_response(
            LoginResponse::Success(Box::new(LoginSuccess::minimal(
                AgentKey::from(uuid::Uuid::from_u128(params.agent)),
                uuid::Uuid::from_u128(params.session),
                uuid::Uuid::from_u128(params.secure),
                CircuitCode(params.circuit),
                Ipv4Addr::new(127, 0, 0, 1),
                params.sim_port,
                format!("http://127.0.0.1:{}/seed", params.sim_port).parse()?,
            ))),
            now,
        )?;
        client.notify_capabilities_ready(now)?;
        let sim = SimSession::new(RegionHandle(REGION_HANDLE), now);
        let mut end = PairEnd {
            client,
            sim,
            client_addr: SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
                params.client_port,
            ),
            sim_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), params.sim_port),
        };
        pump_end(&mut end, now)?;
        Ok(end)
    }

    /// The agent id of pair end A ("Test User" — the [`success`] fixture
    /// identity).
    const PAIR_A_AGENT: u128 = 1;

    /// The agent id of pair end B ("Peer User").
    const PAIR_B_AGENT: u128 = 0xB1;

    /// Sets up the two-avatar relay topology: end A is the [`success`]
    /// fixture identity on the usual ports, end B a second avatar on its own
    /// simulator and ports.
    fn setup_pair(now: Instant) -> Result<(PairEnd, PairEnd), TestError> {
        let a = setup_end(
            &EndParams {
                first_name: "Test",
                agent: PAIR_A_AGENT,
                session: 2,
                secure: 3,
                circuit: 0x0011_2233,
                sim_port: 9000,
                client_port: 40000,
            },
            now,
        )?;
        let b = setup_end(
            &EndParams {
                first_name: "Peer",
                agent: PAIR_B_AGENT,
                session: 0xB2,
                secure: 0xB3,
                circuit: 0x0011_2234,
                sim_port: 9001,
                client_port: 40001,
            },
            now,
        )?;
        Ok((a, b))
    }

    #[test]
    fn circuit_setup_and_arrival() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;

        let server_events = drain_server(&mut sim);
        assert!(
            server_events.iter().any(|e| matches!(
                e,
                ServerEvent::CircuitOpened {
                    agent_id,
                    session_id,
                    circuit_code,
                } if *agent_id == AgentKey::from(uuid::Uuid::from_u128(1))
                    && *session_id == uuid::Uuid::from_u128(2)
                    && *circuit_code == CircuitCode(0x0011_2233)
            )),
            "expected CircuitOpened, got {server_events:?}"
        );
        assert!(
            server_events
                .iter()
                .any(|e| matches!(e, ServerEvent::AgentArrived)),
            "expected AgentArrived, got {server_events:?}"
        );
        assert_eq!(
            sim.agent_id(),
            Some(AgentKey::from(uuid::Uuid::from_u128(1)))
        );
        assert_eq!(sim.client_addr(), Some(client_addr()));

        // The client reached the active state off the AgentMovementComplete reply.
        let client_events = drain_client(&mut client);
        assert!(
            client_events
                .iter()
                .any(|e| matches!(e, Event::RegionHandshakeComplete)),
            "expected RegionHandshakeComplete, got {client_events:?}"
        );
        assert!(!client.is_closed());
        assert!(!sim.is_closed());
        Ok(())
    }

    /// Once active, the client sends a periodic keep-alive `StartPingCheck` on
    /// the root circuit and surfaces the simulator's `CompletePingCheck` as
    /// [`Event::Ping`] carrying the measured round-trip time.
    #[test]
    fn keepalive_ping_round_trip_measures_rtt() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        // Flush arrival traffic so only the ping exchange is left in flight.
        pump(&mut client, &mut sim, now)?;
        let _arrival_events = drain_client(&mut client);
        let _arrival_server_events = drain_server(&mut sim);

        // One ping interval after arrival the keep-alive timer fires and the
        // client transmits its `StartPingCheck`; hand it to the simulator.
        let sent_at = after(now, 5_000)?;
        client.handle_timeout(sent_at);
        let mut start_ping_seen = false;
        while let Some(transmit) = client.poll_transmit() {
            if matches!(decode(&transmit)?, AnyMessage::StartPingCheck(_)) {
                start_ping_seen = true;
            }
            sim.handle_datagram(client_addr(), &transmit.payload, sent_at)?;
        }
        assert!(
            start_ping_seen,
            "the client should send a keep-alive StartPingCheck once active"
        );

        // The simulator answers; deliver its `CompletePingCheck` 200ms later so
        // the round-trip time is observable rather than zero.
        let replied_at = after(now, 5_200)?;
        let mut complete_ping_seen = false;
        while let Some(transmit) = sim.poll_transmit() {
            if matches!(decode(&transmit)?, AnyMessage::CompletePingCheck(_)) {
                complete_ping_seen = true;
            }
            client.handle_datagram(sim_addr(), &transmit.payload, replied_at)?;
        }
        assert!(
            complete_ping_seen,
            "the simulator should answer StartPingCheck with CompletePingCheck"
        );

        let client_events = drain_client(&mut client);
        let rtt = client_events.iter().find_map(|event| match event {
            Event::Ping {
                child: false, rtt, ..
            } => Some(*rtt),
            _other => None,
        });
        assert_eq!(
            rtt,
            Some(Duration::from_millis(200)),
            "expected a root Event::Ping carrying the measured RTT, got {client_events:?}"
        );
        Ok(())
    }

    #[test]
    fn client_chat_reaches_simulator() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);

        client.say("hello sim", ChatType::Shout, ChatChannel(7), now)?;
        pump(&mut client, &mut sim, now)?;

        let events = drain_server(&mut sim);
        let chat = events
            .iter()
            .find_map(|e| match e {
                ServerEvent::Chat {
                    message,
                    channel,
                    chat_type,
                } => Some((message.clone(), *channel, *chat_type)),
                _ => None,
            })
            .ok_or("expected a Chat server event")?;
        assert_eq!(
            chat,
            ("hello sim".to_owned(), ChatChannel(7), ChatType::Shout)
        );
        Ok(())
    }

    #[test]
    fn client_attach_object_reaches_simulator() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);
        let circuit = client.root_circuit_id().ok_or("no circuit")?;

        client.attach_object(
            ScopedObjectId::new(circuit, RegionLocalObjectId(55)),
            AttachmentPoint::RightHand,
            AttachmentMode::Add,
            &sl_types::lsl::Rotation {
                x: 0.0,
                y: 0.0,
                z: 0.0,
                s: 1.0,
            },
            now,
        )?;
        pump(&mut client, &mut sim, now)?;

        let events = drain_server(&mut sim);
        let attach = events
            .iter()
            .find_map(|e| match e {
                ServerEvent::AttachObject {
                    local_id,
                    attachment_point,
                    mode,
                    ..
                } => Some((*local_id, *attachment_point, *mode)),
                _ => None,
            })
            .ok_or("expected an AttachObject server event")?;
        assert_eq!(
            attach,
            (
                RegionLocalObjectId(55),
                AttachmentPoint::RightHand,
                AttachmentMode::Add
            )
        );
        Ok(())
    }

    #[test]
    fn client_detach_objects_reaches_simulator() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);
        let circuit = client.root_circuit_id().ok_or("no circuit")?;

        client.detach_objects(
            &[
                ScopedObjectId::new(circuit, RegionLocalObjectId(3)),
                ScopedObjectId::new(circuit, RegionLocalObjectId(4)),
            ],
            now,
        )?;
        pump(&mut client, &mut sim, now)?;

        let events = drain_server(&mut sim);
        let ids = events
            .iter()
            .find_map(|e| match e {
                ServerEvent::DetachObjects(ids) => Some(ids.clone()),
                _ => None,
            })
            .ok_or("expected a DetachObjects server event")?;
        assert_eq!(ids, vec![RegionLocalObjectId(3), RegionLocalObjectId(4)]);
        Ok(())
    }

    #[test]
    fn client_remove_attachment_reaches_simulator() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);

        let item = uuid::Uuid::from_u128(0x5151);
        client.remove_attachment(AttachmentPoint::Skull, InventoryKey::from(item), now)?;
        pump(&mut client, &mut sim, now)?;

        let events = drain_server(&mut sim);
        let removed = events
            .iter()
            .find_map(|e| match e {
                ServerEvent::RemoveAttachment {
                    attachment_point,
                    item_id,
                } => Some((*attachment_point, *item_id)),
                _ => None,
            })
            .ok_or("expected a RemoveAttachment server event")?;
        assert_eq!(removed, (AttachmentPoint::Skull, item));
        Ok(())
    }

    #[test]
    fn client_rez_attachments_round_trips() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);

        let compound = uuid::Uuid::from_u128(0x9001);
        let attachments = vec![RezAttachment {
            item_id: InventoryKey::from(uuid::Uuid::from_u128(0x9002)),
            owner_id: uuid::Uuid::from_u128(0x9000),
            attachment_point: AttachmentPoint::LeftHand,
            mode: AttachmentMode::Add,
            name: String::new(),
            description: String::new(),
        }];
        client.rez_attachments(
            TransactionId::from(compound),
            DetachOrder::Keep,
            &attachments,
            now,
        )?;
        pump(&mut client, &mut sim, now)?;

        let events = drain_server(&mut sim);
        let rez = events
            .iter()
            .find_map(|e| match e {
                ServerEvent::RezAttachments {
                    compound_id,
                    detach,
                    attachments,
                } => Some((*compound_id, *detach, attachments.clone())),
                _ => None,
            })
            .ok_or("expected a RezAttachments server event")?;
        assert_eq!(rez.0, compound);
        assert_eq!(rez.1, DetachOrder::Keep);
        let first = rez.2.first().ok_or("first attachment")?;
        assert_eq!(first.attachment_point, AttachmentPoint::LeftHand);
        assert_eq!(first.mode, AttachmentMode::Add);
        assert_eq!(
            first.item_id,
            InventoryKey::from(uuid::Uuid::from_u128(0x9002))
        );
        Ok(())
    }

    #[test]
    fn client_viewer_effect_round_trips() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);

        let source = uuid::Uuid::from_u128(0xA00);
        let data = ViewerEffectData::PointAt {
            source: Some(AgentKey::from(source)),
            target: Some(ObjectKey::from(uuid::Uuid::from_u128(0xA01))),
            target_position: GlobalCoordinates::new(1.0, 2.0, 3.0),
            point_at_type: PointAtType::Grab,
        };
        client.send_viewer_effect(
            &[ViewerEffect {
                id: uuid::Uuid::from_u128(0xA0F),
                agent_id: AgentKey::from(source),
                effect_type: ViewerEffectType::PointAt,
                duration: 1.0,
                color: [1, 2, 3, 4],
                data: data.clone(),
            }],
            now,
        )?;
        pump(&mut client, &mut sim, now)?;

        let effects = drain_server(&mut sim)
            .into_iter()
            .find_map(|e| match e {
                ServerEvent::ViewerEffect(effects) => Some(effects),
                _ => None,
            })
            .ok_or("expected a ViewerEffect server event")?;
        let effect = effects.first().ok_or("first effect")?;
        assert_eq!(effect.effect_type, ViewerEffectType::PointAt);
        assert_eq!(effect.data, data);
        Ok(())
    }

    #[test]
    fn client_track_and_find_agent_round_trip() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);

        let prey = uuid::Uuid::from_u128(0xB01);
        let hunter = uuid::Uuid::from_u128(0xB00);
        client.track_agent(AgentKey::from(prey), now)?;
        client.find_agent(AgentKey::from(hunter), AgentKey::from(prey), now)?;
        pump(&mut client, &mut sim, now)?;

        let events = drain_server(&mut sim);
        let tracked = events
            .iter()
            .find_map(|e| match e {
                ServerEvent::TrackAgent { prey_id } => Some(*prey_id),
                _ => None,
            })
            .ok_or("expected a TrackAgent server event")?;
        assert_eq!(tracked, AgentKey::from(prey));
        let found = events
            .iter()
            .find_map(|e| match e {
                ServerEvent::FindAgent { hunter, prey } => Some((*hunter, *prey)),
                _ => None,
            })
            .ok_or("expected a FindAgent server event")?;
        assert_eq!(found, (hunter, prey));
        Ok(())
    }

    #[test]
    fn server_coarse_location_update_reaches_client() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_client(&mut client);

        let me = uuid::Uuid::from_u128(0xC00);
        let other = uuid::Uuid::from_u128(0xC01);
        sim.send_coarse_location_update(
            &[
                CoarseLocation {
                    agent_id: AgentKey::from(me),
                    x: 100,
                    y: 50,
                    z: 80, // sent as 80/4 = 20 on the wire, decoded back to 80
                },
                CoarseLocation {
                    agent_id: AgentKey::from(other),
                    x: 1,
                    y: 2,
                    z: 4,
                },
            ],
            Some(0),
            Some(1),
            now,
        )?;
        pump(&mut client, &mut sim, now)?;

        let (locations, you, prey) = drain_client(&mut client)
            .into_iter()
            .find_map(|e| match e {
                Event::CoarseLocationUpdate {
                    locations,
                    you,
                    prey,
                    region_handle: _,
                } => Some((locations, you, prey)),
                _ => None,
            })
            .ok_or("expected a CoarseLocationUpdate client event")?;
        assert_eq!(you, Some(0));
        assert_eq!(prey, Some(1));
        let first = locations.first().ok_or("first location")?;
        assert_eq!(first.agent_id, AgentKey::from(me));
        assert_eq!(first.z, 80);
        Ok(())
    }

    #[test]
    fn server_find_agent_reply_reaches_client() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_client(&mut client);

        let hunter = uuid::Uuid::from_u128(0xD00);
        let prey = uuid::Uuid::from_u128(0xD01);
        sim.send_find_agent_reply(hunter, prey, &[(300_000.0, 301_000.0)], now)?;
        pump(&mut client, &mut sim, now)?;

        let (reply_prey, locations) = drain_client(&mut client)
            .into_iter()
            .find_map(|e| match e {
                Event::FindAgentReply {
                    prey, locations, ..
                } => Some((prey, locations)),
                _ => None,
            })
            .ok_or("expected a FindAgentReply client event")?;
        assert_eq!(reply_prey, prey);
        assert_eq!(locations, vec![(300_000.0, 301_000.0)]);
        Ok(())
    }

    #[test]
    fn client_directory_queries_round_trip() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);

        let qid = uuid::Uuid::from_u128(0xE01);
        let txn = uuid::Uuid::from_u128(0xE02);
        client.dir_find_query(
            QueryId::from(qid),
            "alice",
            DirFindFlags::PEOPLE.union(DirFindFlags::ONLINE),
            0,
            now,
        )?;
        client.dir_places_query(
            QueryId::from(qid),
            "sandbox",
            DirFindFlags::INC_PG,
            ParcelCategory::Commercial,
            "Region",
            10,
            now,
        )?;
        client.dir_land_query(
            QueryId::from(qid),
            DirFindFlags::FOR_SALE.union(DirFindFlags::LIMIT_BY_PRICE),
            LandSearchType::MAINLAND,
            5000,
            512,
            0,
            now,
        )?;
        client.dir_classified_query(
            QueryId::from(qid),
            "shoes",
            DirFindFlags::INC_MATURE,
            ClassifiedCategory::PropertyRental,
            0,
            now,
        )?;
        client.avatar_picker_request(QueryId::from(qid), "bob", now)?;
        client.places_query(
            QueryId::from(qid),
            TransactionId::from(txn),
            "",
            DirFindFlags::NONE,
            ParcelCategory::None,
            "",
            now,
        )?;
        pump(&mut client, &mut sim, now)?;

        let events = drain_server(&mut sim);
        let find = events
            .iter()
            .find_map(|e| match e {
                ServerEvent::DirFindQuery {
                    query_text, flags, ..
                } => Some((query_text.clone(), *flags)),
                _ => None,
            })
            .ok_or("expected a DirFindQuery server event")?;
        assert_eq!(find.0, "alice");
        assert!(find.1.contains(DirFindFlags::PEOPLE));
        assert!(find.1.contains(DirFindFlags::ONLINE));

        let places = events
            .iter()
            .find_map(|e| match e {
                ServerEvent::DirPlacesQuery {
                    category, sim_name, ..
                } => Some((*category, sim_name.clone())),
                _ => None,
            })
            .ok_or("expected a DirPlacesQuery server event")?;
        assert_eq!(places.0, ParcelCategory::Commercial);
        assert_eq!(places.1, "Region");

        let land = events
            .iter()
            .find_map(|e| match e {
                ServerEvent::DirLandQuery {
                    search_type,
                    price,
                    area,
                    ..
                } => Some((*search_type, *price, *area)),
                _ => None,
            })
            .ok_or("expected a DirLandQuery server event")?;
        assert_eq!(land, (LandSearchType::MAINLAND, 5000, 512));

        let classified = events
            .iter()
            .find_map(|e| match e {
                ServerEvent::DirClassifiedQuery {
                    query_text,
                    category,
                    ..
                } => Some((query_text.clone(), *category)),
                _ => None,
            })
            .ok_or("expected a DirClassifiedQuery server event")?;
        assert_eq!(
            classified,
            ("shoes".to_owned(), ClassifiedCategory::PropertyRental)
        );

        let picker = events
            .iter()
            .find_map(|e| match e {
                ServerEvent::AvatarPickerRequest { name, .. } => Some(name.clone()),
                _ => None,
            })
            .ok_or("expected an AvatarPickerRequest server event")?;
        assert_eq!(picker, "bob");

        let holdings = events
            .iter()
            .find_map(|e| match e {
                ServerEvent::PlacesQuery { transaction_id, .. } => Some(*transaction_id),
                _ => None,
            })
            .ok_or("expected a PlacesQuery server event")?;
        assert_eq!(holdings, txn);
        Ok(())
    }

    #[test]
    fn server_directory_replies_reach_client() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_client(&mut client);

        let qid = uuid::Uuid::from_u128(0xF01);
        let txn = uuid::Uuid::from_u128(0xF02);
        sim.send_dir_people_reply(
            qid,
            &[DirPeopleResult {
                agent_id: AgentKey::from(uuid::Uuid::from_u128(0xF10)),
                first_name: "Alice".to_owned(),
                last_name: "Resident".to_owned(),
                group: String::new(),
                online: true,
                reputation: 0,
            }],
            now,
        )?;
        sim.send_dir_groups_reply(
            qid,
            &[DirGroupResult {
                group_id: GroupKey::from(uuid::Uuid::from_u128(0xF11)),
                group_name: "Builders".to_owned(),
                members: 42,
                search_order: 1.5,
            }],
            now,
        )?;
        sim.send_dir_events_reply(
            qid,
            &[DirEventResult {
                owner_id: uuid::Uuid::from_u128(0xF12),
                name: "Party".to_owned(),
                event_id: EventId::new(7),
                date: "2026-06-20".to_owned(),
                unix_time: 1_750_000_000,
                event_flags: 0,
            }],
            0,
            now,
        )?;
        sim.send_dir_classified_reply(
            qid,
            &[DirClassifiedResult {
                classified_id: ClassifiedKey::from(uuid::Uuid::from_u128(0xF13)),
                name: "Shoes".to_owned(),
                classified_flags: 0,
                creation_date: 1,
                expiration_date: 2,
                price_for_listing: LindenAmount(50),
            }],
            0,
            now,
        )?;
        sim.send_dir_places_reply(
            qid,
            &[DirPlaceResult {
                parcel_id: ParcelKey::from(uuid::Uuid::from_u128(0xF14)),
                name: "Sandbox".to_owned(),
                for_sale: false,
                auction: false,
                dwell: 12.0,
            }],
            0,
            now,
        )?;
        sim.send_dir_land_reply(
            qid,
            &[DirLandResult {
                parcel_id: ParcelKey::from(uuid::Uuid::from_u128(0xF15)),
                name: "For Sale".to_owned(),
                auction: false,
                for_sale: true,
                sale_price: Some(LindenAmount(1000)),
                actual_area: LandArea(1024),
            }],
            now,
        )?;
        sim.send_avatar_picker_reply(
            qid,
            &[AvatarPickerResult {
                avatar_id: AgentKey::from(uuid::Uuid::from_u128(0xF16)),
                first_name: "Bob".to_owned(),
                last_name: "Resident".to_owned(),
                username: String::new(),
                display_name: String::new(),
            }],
            now,
        )?;
        sim.send_places_reply(
            qid,
            txn,
            &[PlacesResult {
                owner_id: uuid::Uuid::from_u128(0xF17),
                name: "Holding".to_owned(),
                description: "mine".to_owned(),
                actual_area: LandArea(512),
                billable_area: LandArea(512),
                flags: 0,
                global_position: GlobalCoordinates::new(1000.0, 2000.0, 30.0),
                sim_name: region("Region"),
                snapshot_id: None,
                dwell: 3.0,
                price: LindenAmount(0),
            }],
            now,
        )?;
        pump(&mut client, &mut sim, now)?;

        let events = drain_client(&mut client);
        let people = events
            .iter()
            .find_map(|e| match e {
                Event::DirPeopleReply { results, .. } => Some(results.clone()),
                _ => None,
            })
            .ok_or("expected a DirPeopleReply client event")?;
        assert_eq!(people.first().ok_or("person")?.first_name, "Alice");

        let groups = events
            .iter()
            .find_map(|e| match e {
                Event::DirGroupsReply { results, .. } => Some(results.clone()),
                _ => None,
            })
            .ok_or("expected a DirGroupsReply client event")?;
        assert_eq!(groups.first().ok_or("group")?.members, 42);

        let dir_events = events
            .iter()
            .find_map(|e| match e {
                Event::DirEventsReply { results, .. } => Some(results.clone()),
                _ => None,
            })
            .ok_or("expected a DirEventsReply client event")?;
        assert_eq!(dir_events.first().ok_or("event")?.event_id, EventId::new(7));

        let classifieds = events
            .iter()
            .find_map(|e| match e {
                Event::DirClassifiedReply { results, .. } => Some(results.clone()),
                _ => None,
            })
            .ok_or("expected a DirClassifiedReply client event")?;
        assert_eq!(classifieds.first().ok_or("classified")?.name, "Shoes");

        let places = events
            .iter()
            .find_map(|e| match e {
                Event::DirPlacesReply { results, .. } => Some(results.clone()),
                _ => None,
            })
            .ok_or("expected a DirPlacesReply client event")?;
        assert_eq!(places.first().ok_or("place")?.name, "Sandbox");

        let land = events
            .iter()
            .find_map(|e| match e {
                Event::DirLandReply { results, .. } => Some(results.clone()),
                _ => None,
            })
            .ok_or("expected a DirLandReply client event")?;
        assert_eq!(
            land.first().ok_or("land")?.sale_price,
            Some(LindenAmount(1000))
        );

        let picker = events
            .iter()
            .find_map(|e| match e {
                Event::AvatarPickerReply { results, .. } => Some(results.clone()),
                _ => None,
            })
            .ok_or("expected an AvatarPickerReply client event")?;
        assert_eq!(picker.first().ok_or("picker")?.first_name, "Bob");

        let (reply_txn, holdings) = events
            .iter()
            .find_map(|e| match e {
                Event::PlacesReply {
                    transaction_id,
                    results,
                    ..
                } => Some((*transaction_id, results.clone())),
                _ => None,
            })
            .ok_or("expected a PlacesReply client event")?;
        assert_eq!(reply_txn, txn);
        let holding = holdings.first().ok_or("holding")?;
        assert_eq!(
            holding.global_position,
            GlobalCoordinates::new(1000.0, 2000.0, 30.0)
        );
        assert_eq!(holding.sim_name, region("Region"));
        Ok(())
    }

    #[test]
    fn event_directory_round_trips() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);
        drain_client(&mut client);

        // Client -> sim: the three events-directory requests.
        client.event_info_request(EventId::new(42), now)?;
        client.event_notification_add_request(EventId::new(42), now)?;
        client.event_notification_remove_request(EventId::new(7), now)?;
        pump(&mut client, &mut sim, now)?;

        let server_events = drain_server(&mut sim);
        let info_event = server_events
            .iter()
            .find_map(|e| match e {
                ServerEvent::EventInfoRequest { event_id } => Some(*event_id),
                _ => None,
            })
            .ok_or("expected an EventInfoRequest server event")?;
        assert_eq!(info_event, EventId::new(42));
        let added = server_events
            .iter()
            .find_map(|e| match e {
                ServerEvent::EventNotificationAddRequest { event_id } => Some(*event_id),
                _ => None,
            })
            .ok_or("expected an EventNotificationAddRequest server event")?;
        assert_eq!(added, EventId::new(42));
        let removed = server_events
            .iter()
            .find_map(|e| match e {
                ServerEvent::EventNotificationRemoveRequest { event_id } => Some(*event_id),
                _ => None,
            })
            .ok_or("expected an EventNotificationRemoveRequest server event")?;
        assert_eq!(removed, EventId::new(7));

        // Sim -> client: the filled-in reply.
        let creator = uuid::Uuid::from_u128(0xE0E);
        sim.send_event_info_reply(
            &EventInfo {
                event_id: EventId::new(42),
                creator: AgentKey::from(creator),
                name: "Beach Party".to_owned(),
                category: "Discussion".to_owned(),
                description: "Come along".to_owned(),
                date: "2026-06-20 12:00:00".to_owned(),
                date_utc: 1_750_000_000,
                duration: 60,
                cover: 1,
                amount: Some(LindenAmount(50)),
                sim_name: region("Sandbox"),
                global_position: GlobalCoordinates::new(256_000.0, 257_000.0, 30.0),
                flags: 0,
            },
            now,
        )?;
        pump(&mut client, &mut sim, now)?;

        let info = drain_client(&mut client)
            .into_iter()
            .find_map(|e| match e {
                Event::EventInfoReply { info } => Some(info),
                _ => None,
            })
            .ok_or("expected an EventInfoReply client event")?;
        assert_eq!(info.event_id, EventId::new(42));
        assert_eq!(info.creator, AgentKey::from(creator));
        assert_eq!(info.name, "Beach Party");
        assert_eq!(info.amount, Some(LindenAmount(50)));
        assert_eq!(
            info.global_position,
            GlobalCoordinates::new(256_000.0, 257_000.0, 30.0)
        );
        Ok(())
    }

    #[test]
    fn object_commerce_round_trips() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);
        drain_client(&mut client);

        let object = ObjectKey::from(uuid::Uuid::from_u128(0xB0B));

        // Client -> sim: the full commerce/spin/rez command surface.
        client.buy_object(
            GroupKey::from(uuid::Uuid::nil()),
            uuid::Uuid::from_u128(0xCA7),
            &[ObjectBuyItem {
                local_id: RegionLocalObjectId(99),
                sale_type: SaleType::Copy,
                sale_price: LindenAmount(250),
            }],
            now,
        )?;
        client.buy_object_inventory(
            object,
            InventoryKey::from(uuid::Uuid::from_u128(0x17E)),
            InventoryFolderKey::from(uuid::Uuid::nil()),
            now,
        )?;
        client.request_pay_price(object, now)?;
        client.request_object_properties_family(0x04, object, now)?;
        client.spin_object_start(object, now)?;
        client.spin_object_stop(object, now)?;
        client.rez_restore_to_world(
            &RestoreItem {
                item_id: InventoryKey::from(uuid::Uuid::from_u128(0x17E)),
                folder_id: InventoryFolderKey::from(uuid::Uuid::nil()),
                creator_id: AgentKey::from(uuid::Uuid::nil()),
                owner: sl_proto::OwnerKey::Agent(sl_proto::AgentKey::from(uuid::Uuid::nil())),
                group: None,
                permissions: Permissions5::empty(),
                transaction_id: uuid::Uuid::nil(),
                asset_type: 6,
                inv_type: 6,
                flags: 0,
                sale_type: SaleType::NotForSale,
                sale_price: Some(LindenAmount(0)),
                name: "Cube".to_owned(),
                description: String::new(),
                creation_date: 0,
                crc: 0,
            },
            now,
        )?;
        client.rez_object_from_notecard(
            &NotecardRez {
                group_id: None,
                from_task_id: None,
                bypass_raycast: false,
                ray_start: sl_types::lsl::Vector {
                    x: 1.0,
                    y: 2.0,
                    z: 3.0,
                },
                ray_end: sl_types::lsl::Vector {
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
                next_owner_mask: 0,
                notecard_item_id: InventoryKey::from(uuid::Uuid::from_u128(0xCA5E)),
                object_id: ObjectKey::from(uuid::Uuid::nil()),
                item_ids: vec![InventoryKey::from(uuid::Uuid::from_u128(0x1))],
            },
            now,
        )?;
        pump(&mut client, &mut sim, now)?;

        let server_events = drain_server(&mut sim);
        let buy = server_events
            .iter()
            .find_map(|e| match e {
                ServerEvent::BuyObject { objects, .. } => Some(objects),
                _ => None,
            })
            .ok_or("expected a BuyObject server event")?;
        assert_eq!(
            buy.first().ok_or("expected one buy item")?.local_id,
            RegionLocalObjectId(99)
        );
        assert_eq!(
            buy.first().ok_or("expected one buy item")?.sale_type,
            SaleType::Copy
        );
        assert!(
            server_events
                .iter()
                .any(|e| matches!(e, ServerEvent::BuyObjectInventory { .. })),
            "expected a BuyObjectInventory server event"
        );
        let pay = server_events
            .iter()
            .find_map(|e| match e {
                ServerEvent::RequestPayPrice { object_id } => Some(*object_id),
                _ => None,
            })
            .ok_or("expected a RequestPayPrice server event")?;
        assert_eq!(pay, object);
        let family = server_events
            .iter()
            .find_map(|e| match e {
                ServerEvent::RequestObjectPropertiesFamily {
                    request_flags,
                    object_id,
                } => Some((*request_flags, *object_id)),
                _ => None,
            })
            .ok_or("expected a RequestObjectPropertiesFamily server event")?;
        assert_eq!(family, (0x04, object));
        assert!(
            server_events
                .iter()
                .any(|e| matches!(e, ServerEvent::SpinObjectStart { .. })),
            "expected a SpinObjectStart server event"
        );
        let restore = server_events
            .iter()
            .find_map(|e| match e {
                ServerEvent::RezRestoreToWorld { item } => Some(item),
                _ => None,
            })
            .ok_or("expected a RezRestoreToWorld server event")?;
        assert_eq!(
            restore.item_id,
            InventoryKey::from(uuid::Uuid::from_u128(0x17E))
        );
        assert_eq!(restore.asset_type, 6);
        let rez = server_events
            .iter()
            .find_map(|e| match e {
                ServerEvent::RezObjectFromNotecard { rez } => Some(rez),
                _ => None,
            })
            .ok_or("expected a RezObjectFromNotecard server event")?;
        assert_eq!(
            rez.notecard_item_id,
            InventoryKey::from(uuid::Uuid::from_u128(0xCA5E))
        );
        assert_eq!(rez.item_ids.len(), 1);

        // Sim -> client: the two reply encoders.
        sim.send_pay_price_reply(object, 10, &[1, 5, 20], now)?;
        sim.send_object_properties_family(
            &ObjectPropertiesFamily {
                request_flags: 0x04,
                object_id: object,
                owner: sl_proto::OwnerKey::Agent(sl_proto::AgentKey::from(uuid::Uuid::from_u128(
                    0x0E,
                ))),
                group: None,
                permissions: Permissions5::empty(),
                ownership_cost: LindenAmount(0),
                sale_type: SaleType::Copy.to_code(),
                sale_price: Some(LindenAmount(250)),
                category: 0,
                last_owner_id: uuid::Uuid::nil(),
                name: "Vendor".to_owned(),
                description: "A vendor".to_owned(),
            },
            now,
        )?;
        pump(&mut client, &mut sim, now)?;

        let client_events = drain_client(&mut client);
        let (default_pay_price, pay_buttons) = client_events
            .iter()
            .find_map(|e| match e {
                Event::PayPriceReply {
                    default_pay_price,
                    pay_buttons,
                    ..
                } => Some((*default_pay_price, pay_buttons.clone())),
                _ => None,
            })
            .ok_or("expected a PayPriceReply client event")?;
        assert_eq!(default_pay_price, 10);
        assert_eq!(pay_buttons, vec![1, 5, 20]);
        let properties = client_events
            .iter()
            .find_map(|e| match e {
                Event::ObjectPropertiesFamily { properties } => Some(properties),
                _ => None,
            })
            .ok_or("expected an ObjectPropertiesFamily client event")?;
        assert_eq!(properties.object_id, object);
        assert_eq!(
            properties.owner,
            sl_proto::OwnerKey::Agent(sl_proto::AgentKey::from(uuid::Uuid::from_u128(0x0E)))
        );
        assert_eq!(properties.group, None);
        assert_eq!(properties.sale_price, Some(LindenAmount(250)));
        assert_eq!(properties.name, "Vendor");
        Ok(())
    }

    #[test]
    fn parcel_management_round_trips() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);
        drain_client(&mut client);
        let circuit = client.root_circuit_id().ok_or("no circuit")?;

        // Client -> sim: the G7 parcel command surface.
        client.join_parcels(16.0, 32.0, 48.0, 64.0, now)?;
        client.divide_parcel(1.0, 2.0, 3.0, 4.0, now)?;
        client.request_parcel_object_owners(
            ScopedParcelId::new(circuit, RegionLocalParcelId(7)),
            now,
        )?;
        client.buy_parcel_pass(ScopedParcelId::new(circuit, RegionLocalParcelId(7)), now)?;
        client.disable_parcel_objects(
            ScopedParcelId::new(circuit, RegionLocalParcelId(7)),
            ParcelReturnType::OTHER,
            &[OwnerKey::Agent(AgentKey::from(uuid::Uuid::from_u128(0x99)))],
            &[ObjectKey::from(uuid::Uuid::from_u128(0xAB))],
            now,
        )?;
        client.request_parcel_info(ParcelKey::from(uuid::Uuid::from_u128(0x00C0_FFEE)), now)?;
        client.request_parcel_dwell(ScopedParcelId::new(circuit, RegionLocalParcelId(7)), now)?;
        pump(&mut client, &mut sim, now)?;

        let server_events = drain_server(&mut sim);
        let join = server_events
            .iter()
            .find_map(|e| match e {
                ServerEvent::JoinParcels { west, north, .. } => Some((*west, *north)),
                _ => None,
            })
            .ok_or("expected a JoinParcels server event")?;
        assert_eq!(join.0.to_bits(), 16.0_f32.to_bits());
        assert_eq!(join.1.to_bits(), 64.0_f32.to_bits());
        assert!(
            server_events
                .iter()
                .any(|e| matches!(e, ServerEvent::DivideParcel { .. })),
            "expected a DivideParcel server event"
        );
        let owners = server_events
            .iter()
            .find_map(|e| match e {
                ServerEvent::RequestParcelObjectOwners { local_id } => Some(*local_id),
                _ => None,
            })
            .ok_or("expected a RequestParcelObjectOwners server event")?;
        assert_eq!(owners, RegionLocalParcelId(7));
        assert!(
            server_events.iter().any(|e| matches!(
                e,
                ServerEvent::BuyParcelPass {
                    local_id: RegionLocalParcelId(7)
                }
            )),
            "expected a BuyParcelPass server event"
        );
        let disable = server_events
            .iter()
            .find_map(|e| match e {
                ServerEvent::DisableParcelObjects {
                    return_type,
                    owner_ids,
                    task_ids,
                    ..
                } => Some((*return_type, owner_ids.len(), task_ids.len())),
                _ => None,
            })
            .ok_or("expected a DisableParcelObjects server event")?;
        assert_eq!(disable, (ParcelReturnType::OTHER.0, 1, 1));
        let info = server_events
            .iter()
            .find_map(|e| match e {
                ServerEvent::RequestParcelInfo { parcel_id } => Some(*parcel_id),
                _ => None,
            })
            .ok_or("expected a RequestParcelInfo server event")?;
        assert_eq!(info.uuid(), uuid::Uuid::from_u128(0x00C0_FFEE));
        // The dwell request names the parcel by its *region-local* id: the
        // grid-wide one is the field the template marks "filled in on sim",
        // which is why the event carries only the local id.
        let dwell_request = server_events
            .iter()
            .find_map(|e| match e {
                ServerEvent::RequestParcelDwell { local_id } => Some(*local_id),
                _ => None,
            })
            .ok_or("expected a RequestParcelDwell server event")?;
        assert_eq!(dwell_request, RegionLocalParcelId(7));

        // Sim -> client: the two reply encoders.
        sim.send_parcel_object_owners_reply(
            &[ParcelObjectOwner {
                owner: sl_proto::OwnerKey::Agent(sl_proto::AgentKey::from(uuid::Uuid::from_u128(
                    0x21,
                ))),
                count: 9,
                online_status: true,
            }],
            now,
        )?;
        sim.send_parcel_info_reply(
            &ParcelDetails {
                parcel_id: ParcelKey::from(uuid::Uuid::from_u128(0x00C0_FFEE)),
                owner_id: uuid::Uuid::from_u128(0x55),
                name: "Sunny Plaza".to_owned(),
                description: "A nice spot".to_owned(),
                actual_area: LandArea(512),
                billable_area: LandArea(480),
                flags: 0x4,
                global_position: GlobalCoordinates::new(256_000.0, 257_024.0, 23.5),
                sim_name: region("Default Region"),
                snapshot_id: Some(TextureKey::from(uuid::Uuid::from_u128(0x77))),
                dwell: 88.0,
                sale_price: Some(LindenAmount(1000)),
                auction_id: 0,
            },
            now,
        )?;
        sim.send_parcel_dwell_reply(
            RegionLocalParcelId(7),
            ParcelKey::from(uuid::Uuid::from_u128(0x00C0_FFEE)),
            88.0,
            now,
        )?;
        pump(&mut client, &mut sim, now)?;

        let client_events = drain_client(&mut client);
        let owners = client_events
            .iter()
            .find_map(|e| match e {
                Event::ParcelObjectOwners { owners } => Some(owners),
                _ => None,
            })
            .ok_or("expected a ParcelObjectOwners client event")?;
        assert_eq!(owners.first().ok_or("expected one owner")?.count, 9);
        let details = client_events
            .iter()
            .find_map(|e| match e {
                Event::ParcelDetails(details) => Some(details),
                _ => None,
            })
            .ok_or("expected a ParcelDetails client event")?;
        assert_eq!(details.name, "Sunny Plaza");
        assert_eq!(
            details.parcel_id,
            ParcelKey::from(uuid::Uuid::from_u128(0x00C0_FFEE))
        );
        assert_eq!(details.sale_price, Some(LindenAmount(1000)));
        let dwell = client_events
            .iter()
            .find_map(|e| match e {
                Event::ParcelDwell {
                    local_id,
                    parcel_id,
                    dwell,
                } => Some((local_id.id(), *parcel_id, *dwell)),
                _ => None,
            })
            .ok_or("expected a ParcelDwell client event")?;
        assert_eq!(dwell.0, RegionLocalParcelId(7));
        assert_eq!(dwell.1, ParcelKey::from(uuid::Uuid::from_u128(0x00C0_FFEE)));
        assert_eq!(dwell.2.to_bits(), 88.0_f32.to_bits());
        Ok(())
    }

    #[test]
    fn estate_covenant_and_telehub_round_trip() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);
        drain_client(&mut client);
        let circuit = client.root_circuit_id().ok_or("no circuit")?;

        // Client -> sim: the covenant request and the telehub command surface.
        client.request_estate_covenant(now)?;
        client.request_telehub_info(now)?;
        client.connect_telehub(ScopedObjectId::new(circuit, RegionLocalObjectId(42)), now)?;
        client.disconnect_telehub(now)?;
        client
            .add_telehub_spawn_point(ScopedObjectId::new(circuit, RegionLocalObjectId(43)), now)?;
        client.remove_telehub_spawn_point(2, now)?;
        pump(&mut client, &mut sim, now)?;

        let server_events = drain_server(&mut sim);
        assert!(
            server_events
                .iter()
                .any(|e| matches!(e, ServerEvent::RequestEstateCovenant)),
            "expected a RequestEstateCovenant server event"
        );
        assert!(
            server_events
                .iter()
                .any(|e| matches!(e, ServerEvent::RequestTelehubInfo)),
            "expected a RequestTelehubInfo server event"
        );
        let connect = server_events
            .iter()
            .find_map(|e| match e {
                ServerEvent::ConnectTelehub { object_local_id } => Some(*object_local_id),
                _ => None,
            })
            .ok_or("expected a ConnectTelehub server event")?;
        assert_eq!(connect, RegionLocalObjectId(42));
        assert!(
            server_events
                .iter()
                .any(|e| matches!(e, ServerEvent::DisconnectTelehub)),
            "expected a DisconnectTelehub server event"
        );
        let add = server_events
            .iter()
            .find_map(|e| match e {
                ServerEvent::AddTelehubSpawnPoint { object_local_id } => Some(*object_local_id),
                _ => None,
            })
            .ok_or("expected an AddTelehubSpawnPoint server event")?;
        assert_eq!(add, RegionLocalObjectId(43));
        let remove = server_events
            .iter()
            .find_map(|e| match e {
                ServerEvent::RemoveTelehubSpawnPoint { spawn_index } => Some(*spawn_index),
                _ => None,
            })
            .ok_or("expected a RemoveTelehubSpawnPoint server event")?;
        assert_eq!(remove, 2);

        // Sim -> client: the two reply encoders.
        sim.send_estate_covenant_reply(
            &EstateCovenant {
                covenant_id: Some(uuid::Uuid::from_u128(0xC0FE)),
                covenant_timestamp: 1_700_000_000,
                estate_name: "My Estate".to_owned(),
                estate_owner_id: uuid::Uuid::from_u128(0x42),
            },
            now,
        )?;
        sim.send_telehub_info(
            &TelehubInfo {
                object_id: Some(ObjectKey::from(uuid::Uuid::from_u128(0x7E1E))),
                object_name: "Welcome Hub".to_owned(),
                position: sl_types::lsl::Vector {
                    x: 128.0,
                    y: 129.0,
                    z: 25.0,
                },
                rotation: sl_types::lsl::Rotation {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    s: 1.0,
                },
                spawn_points: vec![sl_types::lsl::Vector {
                    x: 1.0,
                    y: 2.0,
                    z: 3.0,
                }],
            },
            now,
        )?;
        pump(&mut client, &mut sim, now)?;

        let client_events = drain_client(&mut client);
        let covenant = client_events
            .iter()
            .find_map(|e| match e {
                Event::EstateCovenant(covenant) => Some(covenant),
                _ => None,
            })
            .ok_or("expected an EstateCovenant client event")?;
        assert_eq!(covenant.estate_name, "My Estate");
        assert_eq!(covenant.covenant_id, Some(uuid::Uuid::from_u128(0xC0FE)));
        let telehub = client_events
            .iter()
            .find_map(|e| match e {
                Event::TelehubInfo(telehub) => Some(telehub),
                _ => None,
            })
            .ok_or("expected a TelehubInfo client event")?;
        assert_eq!(telehub.object_name, "Welcome Hub");
        assert_eq!(telehub.spawn_points.len(), 1);
        assert_eq!(telehub.position.z.to_bits(), 25.0_f32.to_bits());
        Ok(())
    }

    #[test]
    fn script_running_round_trip() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);
        drain_client(&mut client);

        let object_id = ObjectKey::from(uuid::Uuid::from_u128(0x0B1E));
        let item_id = uuid::Uuid::from_u128(0x17E3);

        // Client -> sim: the three task-script control messages surface.
        client.request_script_running(object_id, InventoryKey::from(item_id), now)?;
        client.set_script_running(object_id, InventoryKey::from(item_id), true, now)?;
        client.reset_script(object_id, InventoryKey::from(item_id), now)?;
        pump(&mut client, &mut sim, now)?;

        let server_events = drain_server(&mut sim);
        let get = server_events
            .iter()
            .find_map(|e| match e {
                ServerEvent::RequestScriptRunning { object_id, item_id } => {
                    Some((*object_id, *item_id))
                }
                _ => None,
            })
            .ok_or("expected a RequestScriptRunning server event")?;
        assert_eq!(get, (object_id, item_id));
        let set = server_events
            .iter()
            .find_map(|e| match e {
                ServerEvent::SetScriptRunning {
                    object_id,
                    item_id,
                    running,
                } => Some((*object_id, *item_id, *running)),
                _ => None,
            })
            .ok_or("expected a SetScriptRunning server event")?;
        assert_eq!(set, (object_id, item_id, true));
        let reset = server_events
            .iter()
            .find_map(|e| match e {
                ServerEvent::ResetScript { object_id, item_id } => Some((*object_id, *item_id)),
                _ => None,
            })
            .ok_or("expected a ResetScript server event")?;
        assert_eq!(reset, (object_id, item_id));

        // Sim -> client: the run-state reply.
        sim.send_script_running_reply(object_id, item_id, true, now)?;
        pump(&mut client, &mut sim, now)?;

        let running = drain_client(&mut client)
            .into_iter()
            .find_map(|e| match e {
                Event::ScriptRunning {
                    object_id,
                    item_id,
                    running,
                } => Some((object_id, item_id, running)),
                _ => None,
            })
            .ok_or("expected a ScriptRunning client event")?;
        assert_eq!(running, (object_id, InventoryKey::from(item_id), true));
        Ok(())
    }

    #[test]
    fn group_finance_round_trips() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);
        drain_client(&mut client);

        let group_id = uuid::Uuid::from_u128(0x6A0D);
        let request_id = uuid::Uuid::from_u128(0xF00D);
        let transaction_id = uuid::Uuid::from_u128(0x7AC7);
        let proposal_id = sl_proto::ProposalVoteId::from(uuid::Uuid::from_u128(0x9A0E));

        // Client -> sim: every G10 request surfaces a matching server event.
        client.request_group_account_summary(
            GroupKey::from(group_id),
            GroupRequestId::from(request_id),
            60,
            0,
            now,
        )?;
        client.request_group_account_details(
            GroupKey::from(group_id),
            GroupRequestId::from(request_id),
            60,
            0,
            now,
        )?;
        client.request_group_account_transactions(
            GroupKey::from(group_id),
            GroupRequestId::from(request_id),
            60,
            0,
            now,
        )?;
        client.request_group_active_proposals(
            GroupKey::from(group_id),
            TransactionId::from(transaction_id),
            now,
        )?;
        client.request_group_vote_history(
            GroupKey::from(group_id),
            TransactionId::from(transaction_id),
            now,
        )?;
        client.start_group_proposal(
            GroupKey::from(group_id),
            3,
            0.5,
            86_400,
            "Adopt the bylaws?",
            now,
        )?;
        client.cast_group_proposal_ballot(proposal_id, GroupKey::from(group_id), "yes", now)?;
        pump(&mut client, &mut sim, now)?;

        let server_events = drain_server(&mut sim);
        assert!(
            server_events.iter().any(|e| matches!(
                e,
                ServerEvent::RequestGroupAccountSummary { group_id: g, request_id: r, .. }
                    if *g == GroupKey::from(group_id) && *r == request_id
            )),
            "expected a RequestGroupAccountSummary server event"
        );
        assert!(
            server_events
                .iter()
                .any(|e| matches!(e, ServerEvent::RequestGroupAccountDetails { .. })),
            "expected a RequestGroupAccountDetails server event"
        );
        assert!(
            server_events
                .iter()
                .any(|e| matches!(e, ServerEvent::RequestGroupAccountTransactions { .. })),
            "expected a RequestGroupAccountTransactions server event"
        );
        assert!(
            server_events.iter().any(|e| matches!(
                e,
                ServerEvent::RequestGroupActiveProposals { transaction_id: t, .. }
                    if *t == transaction_id
            )),
            "expected a RequestGroupActiveProposals server event"
        );
        assert!(
            server_events
                .iter()
                .any(|e| matches!(e, ServerEvent::RequestGroupVoteHistory { .. })),
            "expected a RequestGroupVoteHistory server event"
        );
        assert!(
            server_events.iter().any(|e| matches!(
                e,
                ServerEvent::StartGroupProposal { quorum, duration, .. }
                    if *quorum == 3 && *duration == 86_400
            )),
            "expected a StartGroupProposal server event"
        );
        assert!(
            server_events.iter().any(|e| matches!(
                e,
                ServerEvent::GroupProposalBallot { proposal_id: p, vote_cast, .. }
                    if *p == proposal_id && vote_cast == "yes"
            )),
            "expected a GroupProposalBallot server event"
        );

        // Sim -> client: every G10 reply surfaces a matching client event.
        let summary = GroupAccountSummary {
            group_id: GroupKey::from(group_id),
            request_id,
            interval_days: 7,
            current_interval: 0,
            start_date: "2026-06-01".to_owned(),
            balance: LindenBalance::from_i32(1234),
            total_credits: LindenAmount(50),
            total_debits: LindenAmount(20),
            object_tax_current: LindenAmount(1),
            light_tax_current: LindenAmount(2),
            land_tax_current: LindenAmount(3),
            group_tax_current: LindenAmount(4),
            parcel_dir_fee_current: LindenAmount(5),
            object_tax_estimate: LindenAmount(6),
            light_tax_estimate: LindenAmount(7),
            land_tax_estimate: LindenAmount(8),
            group_tax_estimate: LindenAmount(9),
            parcel_dir_fee_estimate: LindenAmount(10),
            non_exempt_members: 11,
            last_tax_date: "2026-05-25".to_owned(),
            tax_date: "2026-06-08".to_owned(),
        };
        sim.send_group_account_summary_reply(&summary, now)?;
        let details = GroupAccountDetails {
            group_id: GroupKey::from(group_id),
            request_id,
            interval_days: 7,
            current_interval: 0,
            start_date: "2026-06-01".to_owned(),
            entries: vec![GroupAccountDetailsEntry {
                description: "Object tax".to_owned(),
                amount: LindenBalance::from_i32(-3),
            }],
        };
        sim.send_group_account_details_reply(&details, now)?;
        let transactions = GroupAccountTransactions {
            group_id: GroupKey::from(group_id),
            request_id,
            interval_days: 7,
            current_interval: 0,
            start_date: "2026-06-01".to_owned(),
            entries: vec![GroupAccountTransaction {
                time: "12:00".to_owned(),
                user: "Resident Tester".to_owned(),
                transaction_type: 5,
                item: "Group dues".to_owned(),
                amount: LindenBalance::from_i32(10),
            }],
        };
        sim.send_group_account_transactions_reply(&transactions, now)?;
        let proposal = GroupActiveProposalItem {
            vote_id: proposal_id,
            vote_initiator: AgentKey::from(uuid::Uuid::from_u128(0x1217)),
            terse_date_id: "td".to_owned(),
            start_date_time: "2026-06-01".to_owned(),
            end_date_time: "2026-06-08".to_owned(),
            already_voted: false,
            vote_cast: String::new(),
            majority: 0.5,
            quorum: 3,
            proposal_text: "Adopt the bylaws?".to_owned(),
        };
        sim.send_group_active_proposals_reply(
            GroupKey::from(group_id),
            transaction_id,
            1,
            &[proposal],
            now,
        )?;
        let history = GroupVoteHistoryItem {
            vote_id: proposal_id,
            terse_date_id: "td".to_owned(),
            start_date_time: "2026-05-01".to_owned(),
            end_date_time: "2026-05-08".to_owned(),
            vote_initiator: AgentKey::from(uuid::Uuid::from_u128(0x1217)),
            vote_type: "Proposal".to_owned(),
            vote_result: "Success".to_owned(),
            majority: 0.5,
            quorum: 3,
            proposal_text: "Past proposal".to_owned(),
            votes: vec![GroupVote {
                candidate_id: sl_proto::ProposalCandidateId::from(uuid::Uuid::from_u128(0xC0DE)),
                vote_cast: "yes".to_owned(),
                num_votes: 7,
            }],
        };
        sim.send_group_vote_history_reply(
            GroupKey::from(group_id),
            transaction_id,
            1,
            &history,
            now,
        )?;
        pump(&mut client, &mut sim, now)?;

        let client_events = drain_client(&mut client);
        let got_summary = client_events
            .iter()
            .find_map(|e| match e {
                Event::GroupAccountSummary(summary) => Some(summary),
                _ => None,
            })
            .ok_or("expected a GroupAccountSummary client event")?;
        assert_eq!(got_summary, &summary);

        let got_details = client_events
            .iter()
            .find_map(|e| match e {
                Event::GroupAccountDetails(details) => Some(details),
                _ => None,
            })
            .ok_or("expected a GroupAccountDetails client event")?;
        assert_eq!(got_details, &details);

        let got_transactions = client_events
            .iter()
            .find_map(|e| match e {
                Event::GroupAccountTransactions(transactions) => Some(transactions),
                _ => None,
            })
            .ok_or("expected a GroupAccountTransactions client event")?;
        assert_eq!(got_transactions, &transactions);

        assert!(
            client_events.iter().any(|e| matches!(
                e,
                Event::GroupActiveProposals { proposals, .. }
                    if proposals.first().is_some_and(|p| p.proposal_text == "Adopt the bylaws?")
            )),
            "expected a GroupActiveProposals client event"
        );
        assert!(
            client_events.iter().any(|e| matches!(
                e,
                Event::GroupVoteHistory { item, .. }
                    if item.vote_result == "Success"
                        && item.votes.first().is_some_and(|v| v.num_votes == 7)
            )),
            "expected a GroupVoteHistory client event"
        );
        Ok(())
    }

    /// The two "what does the grid hold about me" asks that had a decode and a
    /// carrier type but no simulator half: the grid's price list and the
    /// agent's own outfit. Each goes out as a request, arrives as a
    /// [`ServerEvent`], is answered from the simulator's own state, and
    /// decodes back into the client event a viewer reads.
    #[test]
    fn economy_and_wearables_round_trip() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);
        drain_client(&mut client);

        let shape_item = InventoryKey::from(uuid::Uuid::from_u128(0x0EA2_0001));
        let shape_asset = uuid::Uuid::from_u128(0x0EA2_0002);
        let shirt_item = InventoryKey::from(uuid::Uuid::from_u128(0x0EA2_0003));
        // What the simulator holds the agent to be wearing. The serial the
        // update carries is the session's, not the caller's: `set_agent_wearables`
        // advances it, so a second outfit is never dropped as stale.
        sim.set_agent_wearables(vec![
            sl_proto::Wearable {
                item_id: shape_item,
                asset_id: Some(shape_asset),
                wearable_type: sl_proto::WearableType::Shape,
            },
            sl_proto::Wearable {
                item_id: shirt_item,
                // A layer whose asset the simulator has not resolved: it goes
                // out nil, and the client keeps it as such rather than
                // inventing one.
                asset_id: None,
                wearable_type: sl_proto::WearableType::Shirt,
            },
        ]);
        let (serial, _worn) = sim.agent_wearables();
        assert_eq!(serial, 1, "the first outfit is serial one");

        client.request_economy_data(now)?;
        client.request_wearables(now)?;
        pump(&mut client, &mut sim, now)?;

        let server_events = drain_server(&mut sim);
        assert!(
            server_events
                .iter()
                .any(|e| matches!(e, ServerEvent::RequestEconomyData)),
            "expected a RequestEconomyData server event"
        );
        assert!(
            server_events
                .iter()
                .any(|e| matches!(e, ServerEvent::RequestAgentWearables)),
            "expected a RequestAgentWearables server event"
        );

        let economy = sl_proto::EconomyData {
            object_capacity: sl_proto::LandImpact(15_000),
            object_count: sl_proto::LandImpact(250),
            price_energy_unit: LindenAmount(1),
            price_object_claim: LindenAmount(2),
            price_public_object_decay: LindenAmount(3),
            price_public_object_delete: LindenAmount(4),
            price_parcel_claim: LindenAmount(5),
            price_parcel_claim_factor: 1.0,
            price_upload: LindenAmount(10),
            price_rent_light: LindenAmount(6),
            teleport_min_price: LindenAmount(7),
            teleport_price_exponent: 2.0,
            energy_efficiency: 1.0,
            price_object_rent: 8.0,
            price_object_scale_factor: 10.0,
            price_parcel_rent: LindenAmount(9),
            price_group_create: LindenAmount(100),
        };
        sim.send_economy_data(&economy, now)?;
        let (serial, worn) = sim.agent_wearables();
        let worn = worn.to_vec();
        sim.send_agent_wearables_update(serial, &worn, now)?;
        pump(&mut client, &mut sim, now)?;

        let client_events = drain_client(&mut client);
        let decoded = client_events
            .iter()
            .find_map(|e| match e {
                Event::EconomyData(data) => Some((**data).clone()),
                _ => None,
            })
            .ok_or("expected an EconomyData client event")?;
        // Every price is distinct in the fixture, so a field the encoder wrote
        // into the wrong slot fails here rather than reading as plausible.
        assert_eq!(decoded, economy);

        let (decoded_serial, decoded_worn) = client_events
            .iter()
            .find_map(|e| match e {
                Event::AgentWearables { serial, wearables } => Some((*serial, wearables.clone())),
                _ => None,
            })
            .ok_or("expected an AgentWearables client event")?;
        assert_eq!(decoded_serial, serial);
        assert_eq!(decoded_worn.len(), 2);
        let shape = decoded_worn
            .iter()
            .find(|wearable| wearable.wearable_type == sl_proto::WearableType::Shape)
            .ok_or("expected the shape in the decoded outfit")?;
        assert_eq!(shape.item_id, shape_item);
        assert_eq!(shape.asset_id, Some(shape_asset));
        let shirt = decoded_worn
            .iter()
            .find(|wearable| wearable.wearable_type == sl_proto::WearableType::Shirt)
            .ok_or("expected the shirt in the decoded outfit")?;
        assert_eq!(shirt.item_id, shirt_item);
        assert!(
            shirt.asset_id.is_none_or(|id| id.is_nil()),
            "an unresolved layer keeps its nil asset id"
        );
        Ok(())
    }

    #[test]
    fn gesture_activation_round_trips() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);
        drain_client(&mut client);

        let item_a = InventoryKey::from(uuid::Uuid::from_u128(0x6E5_A001));
        let asset_a = uuid::Uuid::from_u128(0x6E5_A002);
        let item_b = uuid::Uuid::from_u128(0x6E5_A003);

        // Client -> sim: activating then deactivating gestures each surface a
        // matching server event carrying the item (and, for activation, asset) ids.
        client.activate_gestures(
            &[GestureActivation {
                item_id: item_a,
                asset_id: asset_a,
            }],
            now,
        )?;
        client.deactivate_gestures(&[InventoryKey::from(item_b)], now)?;
        pump(&mut client, &mut sim, now)?;

        let server_events = drain_server(&mut sim);
        assert!(
            server_events.iter().any(|e| matches!(
                e,
                ServerEvent::ActivateGestures { gestures }
                    if gestures.first().is_some_and(|g| g.item_id == item_a && g.asset_id == asset_a)
            )),
            "expected an ActivateGestures server event"
        );
        assert!(
            server_events.iter().any(|e| matches!(
                e,
                ServerEvent::DeactivateGestures { item_ids }
                    if item_ids.first() == Some(&item_b)
            )),
            "expected a DeactivateGestures server event"
        );
        Ok(())
    }

    #[test]
    fn agent_state_messages_round_trip() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);
        drain_client(&mut client);

        // Client -> sim: each agent-state message surfaces a matching server event.
        client.set_always_run(MovementMode::AlwaysRun, now)?;
        client.pause_agent(now)?;
        client.resume_agent(now)?;
        client.set_agent_fov(1.5, now)?;
        client.set_agent_size(600, 800, now)?;
        client.release_script_controls(now)?;
        pump(&mut client, &mut sim, now)?;

        let server_events = drain_server(&mut sim);
        assert!(
            server_events.iter().any(|e| matches!(
                e,
                ServerEvent::SetAlwaysRun {
                    mode: MovementMode::AlwaysRun
                }
            )),
            "expected a SetAlwaysRun server event"
        );
        let pause_serial = server_events
            .iter()
            .find_map(|e| match e {
                ServerEvent::AgentPause { serial_num } => Some(*serial_num),
                _ => None,
            })
            .ok_or("expected an AgentPause server event")?;
        let resume_serial = server_events
            .iter()
            .find_map(|e| match e {
                ServerEvent::AgentResume { serial_num } => Some(*serial_num),
                _ => None,
            })
            .ok_or("expected an AgentResume server event")?;
        assert!(resume_serial > pause_serial);
        assert!(
            server_events
                .iter()
                .any(|e| matches!(e, ServerEvent::AgentFov { vertical_angle } if vertical_angle.to_bits() == 1.5_f32.to_bits())),
            "expected an AgentFov server event"
        );
        assert!(
            server_events.iter().any(|e| matches!(
                e,
                ServerEvent::AgentHeightWidth {
                    height: 600,
                    width: 800
                }
            )),
            "expected an AgentHeightWidth server event"
        );
        assert!(
            server_events
                .iter()
                .any(|e| matches!(e, ServerEvent::ForceScriptControlRelease)),
            "expected a ForceScriptControlRelease server event"
        );
        Ok(())
    }

    #[test]
    fn script_camera_and_controls_reach_client() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_client(&mut client);

        let object = ObjectKey::from(uuid::Uuid::from_u128(0xCA3_1001));
        // Sim -> client: a script takes controls, sets follow-cam, then clears it.
        sim.send_script_control_change(
            &[ScriptControl {
                action: ScriptControlAction::Take,
                controls: ControlFlags::AT_POS | ControlFlags::UP_POS,
                pass_to_agent: true,
            }],
            now,
        )?;
        sim.send_set_follow_cam_properties(
            object,
            &[FollowCamPropertyValue {
                property: FollowCamProperty::Distance,
                value: 6.0,
            }],
            now,
        )?;
        sim.send_clear_follow_cam_properties(object, now)?;
        pump(&mut client, &mut sim, now)?;

        let events = drain_client(&mut client);
        let control = events
            .iter()
            .find_map(|e| match e {
                Event::ScriptControlChange(controls) => controls.first().copied(),
                _ => None,
            })
            .ok_or("expected a ScriptControlChange client event")?;
        assert_eq!(control.action, ScriptControlAction::Take);
        assert!(control.pass_to_agent);
        assert_eq!(
            control.controls,
            ControlFlags::AT_POS | ControlFlags::UP_POS
        );

        let (set_object, properties) = events
            .iter()
            .find_map(|e| match e {
                Event::SetFollowCamProperties {
                    object_id,
                    properties,
                } => Some((*object_id, properties.clone())),
                _ => None,
            })
            .ok_or("expected a SetFollowCamProperties client event")?;
        assert_eq!(set_object, object);
        assert_eq!(
            properties.first().map(|p| p.property),
            Some(FollowCamProperty::Distance)
        );

        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::ClearFollowCamProperties { object_id } if *object_id == object
            )),
            "expected a ClearFollowCamProperties client event"
        );
        Ok(())
    }

    #[test]
    fn taken_controls_tracker_folds_sim_control_change() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_client(&mut client);

        // Sim -> client: a script takes a control (consumed), then releases it.
        // The client's taken-controls tracker folds the real server-built block.
        sim.send_script_control_change(
            &[ScriptControl {
                action: ScriptControlAction::Take,
                controls: ControlFlags::AT_POS,
                pass_to_agent: false,
            }],
            now,
        )?;
        pump(&mut client, &mut sim, now)?;
        drain_client(&mut client);
        assert_eq!(client.script_controls().taken, ControlFlags::AT_POS);

        sim.send_script_control_change(
            &[ScriptControl {
                action: ScriptControlAction::Release,
                controls: ControlFlags::AT_POS,
                pass_to_agent: false,
            }],
            now,
        )?;
        pump(&mut client, &mut sim, now)?;
        drain_client(&mut client);
        assert_eq!(client.script_controls().taken, ControlFlags::empty());
        Ok(())
    }

    #[test]
    fn alerts_collisions_health_camera_frozen_reach_client() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_client(&mut client);

        let agent = uuid::Uuid::from_u128(0xA1E_2001);
        let victim = uuid::Uuid::from_u128(0xC011_DE11);
        let perp = uuid::Uuid::from_u128(0xC011_DE12);
        let plane = [0.0_f32, 1.0, 0.0, 3.25];

        // Sim -> client: the five receive-only notifications G13 wraps plus the
        // G17 viewer-freeze toggle.
        sim.send_alert_message(
            "region restarting",
            &[AlertInfo {
                message: "RegionRestartMinutes".to_owned(),
                extra_params: "MINUTES=2".to_owned(),
            }],
            &[agent],
            now,
        )?;
        sim.send_agent_alert_message(AgentKey::from(agent), true, "you were teleported home", now)?;
        sim.send_mean_collision_alert(
            &[MeanCollision {
                victim,
                perp,
                time: 1_700_000_500,
                magnitude: 4.0,
                collision_type: MeanCollisionType::PushObject,
            }],
            now,
        )?;
        sim.send_health_message(42.0, now)?;
        sim.send_camera_constraint(plane, now)?;
        sim.send_viewer_frozen(true, now)?;
        pump(&mut client, &mut sim, now)?;

        let events = drain_client(&mut client);

        let (message, alert_info, agents) = events
            .iter()
            .find_map(|e| match e {
                Event::AlertMessage {
                    message,
                    alert_info,
                    agents,
                } => Some((message.clone(), alert_info.clone(), agents.clone())),
                _ => None,
            })
            .ok_or("expected an AlertMessage client event")?;
        assert_eq!(message, "region restarting");
        assert_eq!(
            alert_info.first().map(|i| i.message.as_str()),
            Some("RegionRestartMinutes")
        );
        assert_eq!(agents.first().copied(), Some(agent));

        let (alert_agent, modal, alert_message) = events
            .iter()
            .find_map(|e| match e {
                Event::AgentAlertMessage {
                    agent_id,
                    modal,
                    message,
                } => Some((*agent_id, *modal, message.clone())),
                _ => None,
            })
            .ok_or("expected an AgentAlertMessage client event")?;
        assert_eq!(alert_agent, AgentKey::from(agent));
        assert!(modal);
        assert_eq!(alert_message, "you were teleported home");

        let collision = events
            .iter()
            .find_map(|e| match e {
                Event::MeanCollisionAlert(collisions) => collisions.first().copied(),
                _ => None,
            })
            .ok_or("expected a MeanCollisionAlert client event")?;
        assert_eq!(collision.victim, victim);
        assert_eq!(collision.perp, perp);
        assert_eq!(collision.collision_type, MeanCollisionType::PushObject);

        let health = events
            .iter()
            .find_map(|e| match e {
                Event::HealthMessage { health } => Some(*health),
                _ => None,
            })
            .ok_or("expected a HealthMessage client event")?;
        assert_eq!(health.to_bits(), 42.0_f32.to_bits());

        let got_plane = events
            .iter()
            .find_map(|e| match e {
                Event::CameraConstraint { plane } => Some(*plane),
                _ => None,
            })
            .ok_or("expected a CameraConstraint client event")?;
        assert_eq!(got_plane.map(f32::to_bits), plane.map(f32::to_bits));

        let frozen = events
            .iter()
            .find_map(|e| match e {
                Event::ViewerFrozen { frozen } => Some(*frozen),
                _ => None,
            })
            .ok_or("expected a ViewerFrozen client event")?;
        assert!(frozen);
        Ok(())
    }

    #[test]
    fn land_stat_reply_reaches_client() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_client(&mut client);

        let task = ObjectKey::from(uuid::Uuid::from_u128(0x70B_5C0E));
        sim.send_land_stat_reply(
            LandStatReportType::TopScripts,
            0,
            7,
            &[LandStatItem {
                task_local_id: RegionLocalObjectId(4_294_967_000),
                task_id: task,
                location: RegionCoordinates::new(128.0, 64.5, 25.0),
                score: 0.85,
                task_name: "busy script".to_owned(),
                owner_name: "Test Resident".to_owned(),
            }],
            now,
        )?;
        pump(&mut client, &mut sim, now)?;

        let (report_type, total, items) = drain_client(&mut client)
            .into_iter()
            .find_map(|e| match e {
                Event::LandStatReply {
                    report_type,
                    total_object_count,
                    items,
                    ..
                } => Some((report_type, total_object_count, items)),
                _ => None,
            })
            .ok_or("expected a LandStatReply client event")?;
        assert_eq!(report_type, LandStatReportType::TopScripts);
        assert_eq!(total, 7);
        let item = items.first().ok_or("expected one report item")?;
        assert_eq!(item.task_local_id, RegionLocalObjectId(4_294_967_000));
        assert_eq!(item.task_id, task);
        assert_eq!(item.task_name, "busy script");
        assert_eq!(item.owner_name, "Test Resident");
        assert_eq!(item.score.to_bits(), 0.85_f32.to_bits());
        Ok(())
    }

    #[test]
    fn sim_stats_and_time_reach_client() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_client(&mut client);

        let stats = RegionStats {
            grid_coordinates: GridCoordinates::new(1000, 1100),
            region_flags: 0x0000_0001,
            object_capacity: 15_000,
            region_flags_extended: 0x0000_0001_0000_0002,
            stats: vec![
                (SimStatId::TimeDilation, 0.98),
                (SimStatId::SimFps, 44.5),
                (SimStatId::Agents, 7.0),
            ],
        };
        let time = SimulatorTime {
            usec_since_start: 1_700_000_000_000,
            sec_per_day: 14_400,
            sec_per_year: 5_256_000,
            sun_direction: sl_types::lsl::Vector {
                x: 0.0,
                y: 0.5,
                z: 0.866,
            },
            sun_phase: 1.25,
            sun_ang_velocity: sl_types::lsl::Vector {
                x: 0.0,
                y: 0.0,
                z: 0.0024,
            },
        };
        sim.send_sim_stats(&stats, now)?;
        sim.send_simulator_time(&time, now)?;
        pump(&mut client, &mut sim, now)?;

        let events = drain_client(&mut client);
        let got_stats = events
            .iter()
            .find_map(|e| match e {
                Event::SimStats(stats) => Some(stats.clone()),
                _ => None,
            })
            .ok_or("expected a SimStats client event")?;
        assert_eq!(got_stats.grid_coordinates, GridCoordinates::new(1000, 1100));
        assert_eq!(got_stats.region_flags, 0x0000_0001);
        assert_eq!(got_stats.object_capacity, 15_000);
        assert_eq!(got_stats.region_flags_extended, 0x0000_0001_0000_0002);
        assert_eq!(got_stats.stats.len(), 3);
        assert_eq!(
            got_stats.stats.first().map(|s| s.0),
            Some(SimStatId::TimeDilation)
        );
        assert_eq!(
            got_stats.stats.first().map(|s| s.1.to_bits()),
            Some(0.98_f32.to_bits())
        );

        let got_time = events
            .iter()
            .find_map(|e| match e {
                Event::SimulatorTime(time) => Some(time.clone()),
                _ => None,
            })
            .ok_or("expected a SimulatorTime client event")?;
        assert_eq!(got_time.usec_since_start, 1_700_000_000_000);
        assert_eq!(got_time.sec_per_day, 14_400);
        assert_eq!(got_time.sec_per_year, 5_256_000);
        assert_eq!(got_time.sun_phase.to_bits(), 1.25_f32.to_bits());
        assert_eq!(got_time.sun_direction.z.to_bits(), 0.866_f32.to_bits());
        Ok(())
    }

    #[test]
    fn generic_message_family_reaches_client() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_client(&mut client);

        let invoice = InvoiceId::from(uuid::Uuid::from_u128(0x4242));
        let generic = GenericMessage {
            method: "GrantUserRights".to_owned(),
            invoice,
            params: vec![b"first".to_vec(), b"second".to_vec()],
        };
        let large = GenericMessage {
            method: "BigPayload".to_owned(),
            invoice: InvoiceId::default(),
            params: vec![vec![0xAB; 300]],
        };
        // A non-GLTF method id so the client surfaces it as the generic
        // streaming event rather than the dedicated material-override handler.
        let streaming = GenericStreamingMessage {
            method: 0x1234,
            data: b"opaque-streamed-blob".to_vec(),
        };
        sim.send_generic_message(&generic, now)?;
        sim.send_large_generic_message(&large, now)?;
        sim.send_generic_streaming_message(&streaming, now)?;
        pump(&mut client, &mut sim, now)?;

        let events = drain_client(&mut client);
        let got_generic = events
            .iter()
            .find_map(|e| match e {
                Event::GenericMessage(generic) => Some(generic.clone()),
                _ => None,
            })
            .ok_or("expected a GenericMessage client event")?;
        assert_eq!(got_generic, generic);

        let got_large = events
            .iter()
            .find_map(|e| match e {
                Event::LargeGenericMessage(generic) => Some(generic.clone()),
                _ => None,
            })
            .ok_or("expected a LargeGenericMessage client event")?;
        assert_eq!(got_large, large);

        let got_streaming = events
            .iter()
            .find_map(|e| match e {
                Event::GenericStreamingMessage(streaming) => Some(streaming.clone()),
                _ => None,
            })
            .ok_or("expected a GenericStreamingMessage client event")?;
        assert_eq!(got_streaming, streaming);
        Ok(())
    }

    #[test]
    fn session_error_and_feature_disabled_reach_client() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_client(&mut client);

        let agent = AgentKey::from(uuid::Uuid::from_u128(1));
        let error = ServerError {
            agent,
            code: 402,
            token: "PaymentRequired".to_owned(),
            id: uuid::Uuid::from_u128(0xDEAD),
            system: "message/handler".to_owned(),
            message: "transaction failed".to_owned(),
            data: vec![0x01, 0x02, 0x03],
        };
        let disabled = FeatureDisabled {
            message: "feature unavailable here".to_owned(),
            agent,
            transaction: TransactionId::from(uuid::Uuid::from_u128(0xBEEF)),
        };
        sim.send_error(&error, now)?;
        sim.send_feature_disabled(&disabled, now)?;
        pump(&mut client, &mut sim, now)?;

        let events = drain_client(&mut client);
        let got_error = events
            .iter()
            .find_map(|e| match e {
                Event::ServerError(error) => Some((**error).clone()),
                _ => None,
            })
            .ok_or("expected a ServerError client event")?;
        assert_eq!(got_error, error);

        let got_disabled = events
            .iter()
            .find_map(|e| match e {
                Event::FeatureDisabled(disabled) => Some(disabled.clone()),
                _ => None,
            })
            .ok_or("expected a FeatureDisabled client event")?;
        assert_eq!(got_disabled, disabled);
        Ok(())
    }

    #[test]
    fn kick_user_reaches_client_and_disconnects() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_client(&mut client);

        let kick = Kick {
            agent: AgentKey::from(uuid::Uuid::from_u128(1)),
            reason: "logged in elsewhere".to_owned(),
        };
        sim.send_kick_user(&kick, now)?;
        pump(&mut client, &mut sim, now)?;

        let events = drain_client(&mut client);
        let got_kick = events
            .iter()
            .find_map(|e| match e {
                Event::Kicked(kick) => Some(kick.clone()),
                _ => None,
            })
            .ok_or("expected a Kicked client event")?;
        assert_eq!(got_kick, kick);
        // The kick also drives the client to its terminal disconnected state.
        assert!(
            events.iter().any(|e| matches!(e, Event::Disconnected(_))),
            "expected a Disconnected client event after a kick"
        );
        Ok(())
    }

    #[test]
    fn object_animation_and_rebake_reach_client() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_client(&mut client);

        let object = ObjectKey::from(uuid::Uuid::from_u128(0xB1));
        let dance = AnimationKey::from(uuid::Uuid::from_u128(0x400));
        let wave = AnimationKey::from(uuid::Uuid::from_u128(0x401));
        let animations = vec![
            ObjectPlayingAnimation {
                anim_id: dance,
                sequence_id: 3,
            },
            ObjectPlayingAnimation {
                anim_id: wave,
                sequence_id: 4,
            },
        ];
        let baked = TextureKey::from(uuid::Uuid::from_u128(0xBA4E));
        sim.send_object_animation(object, &animations, now)?;
        sim.send_rebake_avatar_textures(baked, now)?;
        pump(&mut client, &mut sim, now)?;

        let events = drain_client(&mut client);
        let (object_id, got_animations) = events
            .iter()
            .find_map(|e| match e {
                Event::ObjectAnimation {
                    object_id,
                    animations,
                } => Some((*object_id, animations.clone())),
                _ => None,
            })
            .ok_or("expected an ObjectAnimation client event")?;
        assert_eq!(object_id, object);
        assert_eq!(got_animations, animations);

        let texture_id = events
            .iter()
            .find_map(|e| match e {
                Event::RebakeAvatarTextures { texture_id } => Some(*texture_id),
                _ => None,
            })
            .ok_or("expected a RebakeAvatarTextures client event")?;
        assert_eq!(texture_id, baked);
        Ok(())
    }

    /// Another avatar's appearance, its playing animations and the terse
    /// motion updates that move it all reach the client as the events a
    /// renderer reads: the bakes come back in their `avatar_texture` slots, the
    /// animation sources survive the positional correlation, and the terse
    /// update lands on the object the full update introduced.
    #[test]
    fn avatar_appearance_animation_and_terse_motion_reach_client() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_client(&mut client);

        // 1. Appearance: a full-width avatar texture entry with two bakes.
        let npc = AgentKey::from(uuid::Uuid::from_u128(0x0BC1));
        let head_bake = TextureKey::from(uuid::Uuid::from_u128(0xBA4E_0001));
        let upper_bake = TextureKey::from(uuid::Uuid::from_u128(0xBA4E_0002));
        let mut entry = TextureEntry {
            faces: vec![
                TextureFace::new(TextureKey::from(
                    sl_proto::avatar_texture::IMG_DEFAULT_AVATAR
                ));
                sl_proto::avatar_texture::COUNT
            ],
        };
        if let Some(face) = entry.faces.get_mut(sl_proto::avatar_texture::HEAD_BAKED) {
            face.texture_id = head_bake;
        }
        if let Some(face) = entry.faces.get_mut(sl_proto::avatar_texture::UPPER_BAKED) {
            face.texture_id = upper_bake;
        }
        let attachment = ObjectKey::from(uuid::Uuid::from_u128(0xA77A));
        let appearance = sl_proto::AvatarAppearance {
            avatar_id: npc,
            is_trial: false,
            texture_entry: entry,
            visual_params: vec![128; 32],
            appearance_version: Some(1),
            cof_version: Some(7),
            appearance_flags: Some(0),
            hover_height: Some(sl_proto::Vector {
                x: 0.0,
                y: 0.0,
                z: 0.25,
            }),
            attachments: vec![sl_proto::AvatarAttachment {
                id: attachment,
                attachment_point: 6,
            }],
        };
        sim.send_avatar_appearance(&appearance, now)?;

        // 2. Animation: one agent-started, one triggered by a scripted object.
        let animations = vec![
            sl_proto::PlayingAnimation {
                anim_id: uuid::Uuid::from_u128(0x5741_0001),
                sequence_id: 1,
                source_id: None,
            },
            sl_proto::PlayingAnimation {
                anim_id: uuid::Uuid::from_u128(0x5741_0002),
                sequence_id: 2,
                source_id: Some(attachment),
            },
        ];
        sim.send_avatar_animation(npc, &animations, now)?;
        pump(&mut client, &mut sim, now)?;

        let events = drain_client(&mut client);
        let got = events
            .iter()
            .find_map(|e| match e {
                Event::AvatarAppearance(appearance) => Some(appearance.clone()),
                _ => None,
            })
            .ok_or("expected an AvatarAppearance client event")?;
        assert_eq!(got.avatar_id, npc);
        assert_eq!(
            got.texture_entry
                .texture_id(sl_proto::avatar_texture::HEAD_BAKED),
            Some(head_bake)
        );
        assert_eq!(
            got.texture_entry
                .texture_id(sl_proto::avatar_texture::UPPER_BAKED),
            Some(upper_bake)
        );
        assert_eq!(got.visual_params, vec![128; 32]);
        assert_eq!(got.cof_version, Some(7));
        assert_eq!(
            got.hover_height.map(|hover| hover.z),
            Some(0.25),
            "the hover block did not survive"
        );
        assert_eq!(
            got.attachments,
            vec![sl_proto::AvatarAttachment {
                id: attachment,
                attachment_point: 6,
            }]
        );

        let (avatar_id, got_animations) = events
            .iter()
            .find_map(|e| match e {
                Event::AvatarAnimation {
                    avatar_id,
                    animations,
                    ..
                } => Some((*avatar_id, animations.clone())),
                _ => None,
            })
            .ok_or("expected an AvatarAnimation client event")?;
        assert_eq!(avatar_id, npc);
        let played: Vec<(uuid::Uuid, i32)> = got_animations
            .iter()
            .map(|animation| (animation.anim_id, animation.sequence_id))
            .collect();
        assert_eq!(
            played,
            vec![
                (uuid::Uuid::from_u128(0x5741_0001), 1),
                (uuid::Uuid::from_u128(0x5741_0002), 2)
            ]
        );
        // The scripted animation keeps its trigger; the agent-started one is
        // stamped with the avatar's own id, as a simulator sends it.
        let sources: Vec<Option<ObjectKey>> = got_animations
            .iter()
            .map(|animation| animation.source_id)
            .collect();
        assert_eq!(
            sources,
            vec![Some(ObjectKey::from(npc.uuid())), Some(attachment)]
        );

        // 3. Terse motion: the client applies it to the object it already has.
        let prim = box_prim(
            0x30,
            0x3030,
            sl_proto::Vector {
                x: 1.0,
                y: 2.0,
                z: 3.0,
            },
        );
        sim.send_object_update(std::slice::from_ref(&prim), 0xFFFF, now)?;
        pump(&mut client, &mut sim, now)?;
        drain_client(&mut client);

        let moved = sl_proto::Vector {
            x: 10.0,
            y: 20.0,
            z: 30.0,
        };
        let mut motion = prim.motion.clone();
        motion.position = moved.clone();
        sim.send_terse_update(
            &[sl_proto::TerseUpdate {
                local_id: RegionLocalObjectId(0x30),
                state: 0,
                motion,
            }],
            0xFFFF,
            now,
        )?;
        pump(&mut client, &mut sim, now)?;

        let updated = drain_client(&mut client)
            .into_iter()
            .find_map(|e| match e {
                Event::ObjectUpdated(object) if object.local_id == RegionLocalObjectId(0x30) => {
                    Some(object)
                }
                _ => None,
            })
            .ok_or("expected an ObjectUpdated from the terse update")?;
        assert_eq!(updated.motion.position, moved);
        // A terse update carries only motion, so the identity the full update
        // introduced is still the client's.
        assert_eq!(updated.full_id, prim.full_id);
        Ok(())
    }

    #[test]
    fn friendship_and_calling_cards_reach_client() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_client(&mut client);

        let former_friend = FriendKey::from(uuid::Uuid::from_u128(0xF1E0));
        let offerer = AgentKey::from(uuid::Uuid::from_u128(0x0FFE));
        let offer_txn = TransactionId::from(uuid::Uuid::from_u128(0x701));
        let accepter = AgentKey::from(uuid::Uuid::from_u128(0xACCE));
        let accept_txn = TransactionId::from(uuid::Uuid::from_u128(0x702));
        let decliner = AgentKey::from(uuid::Uuid::from_u128(0xDEC1));
        let decline_txn = TransactionId::from(uuid::Uuid::from_u128(0x703));

        sim.send_terminate_friendship(former_friend, now)?;
        sim.send_offer_calling_card(offerer, offer_txn, now)?;
        sim.send_accept_calling_card(accepter, accept_txn, now)?;
        sim.send_decline_calling_card(decliner, decline_txn, now)?;
        pump(&mut client, &mut sim, now)?;

        let events = drain_client(&mut client);
        let other = events
            .iter()
            .find_map(|e| match e {
                Event::FriendshipTerminated { other } => Some(*other),
                _ => None,
            })
            .ok_or("expected a FriendshipTerminated client event")?;
        assert_eq!(other, former_friend);

        let (offering_agent, transaction) = events
            .iter()
            .find_map(|e| match e {
                Event::CallingCardOffered {
                    offering_agent,
                    transaction,
                } => Some((*offering_agent, *transaction)),
                _ => None,
            })
            .ok_or("expected a CallingCardOffered client event")?;
        assert_eq!(offering_agent, offerer);
        assert_eq!(transaction, offer_txn);

        let (agent, transaction) = events
            .iter()
            .find_map(|e| match e {
                Event::CallingCardAccepted { agent, transaction } => Some((*agent, *transaction)),
                _ => None,
            })
            .ok_or("expected a CallingCardAccepted client event")?;
        assert_eq!(agent, accepter);
        assert_eq!(transaction, accept_txn);

        let (agent, transaction) = events
            .iter()
            .find_map(|e| match e {
                Event::CallingCardDeclined { agent, transaction } => Some((*agent, *transaction)),
                _ => None,
            })
            .ok_or("expected a CallingCardDeclined client event")?;
        assert_eq!(agent, decliner);
        assert_eq!(transaction, decline_txn);
        Ok(())
    }

    #[test]
    fn client_calling_cards_reach_simulator() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);

        let dest = AgentKey::from(uuid::Uuid::from_u128(0x0FFE));
        let offer_txn = TransactionId::from(uuid::Uuid::from_u128(0x701));
        let accept_txn = TransactionId::from(uuid::Uuid::from_u128(0x702));
        let folder = InventoryFolderKey::from(uuid::Uuid::from_u128(0xCA11));
        let decline_txn = TransactionId::from(uuid::Uuid::from_u128(0x703));

        client.offer_calling_card(dest, offer_txn, now)?;
        client.accept_calling_card(accept_txn, folder, now)?;
        client.decline_calling_card(decline_txn, now)?;
        pump(&mut client, &mut sim, now)?;

        let events = drain_server(&mut sim);
        let (offered_dest, transaction) = events
            .iter()
            .find_map(|e| match e {
                ServerEvent::CallingCardOffered { dest, transaction } => {
                    Some((*dest, *transaction))
                }
                _ => None,
            })
            .ok_or("expected a CallingCardOffered server event")?;
        assert_eq!(offered_dest, dest);
        assert_eq!(transaction, offer_txn);

        let (transaction, accepted_folder) = events
            .iter()
            .find_map(|e| match e {
                ServerEvent::CallingCardAccepted {
                    transaction,
                    folder,
                } => Some((*transaction, *folder)),
                _ => None,
            })
            .ok_or("expected a CallingCardAccepted server event")?;
        assert_eq!(transaction, accept_txn);
        assert_eq!(accepted_folder, folder);

        let transaction = events
            .iter()
            .find_map(|e| match e {
                ServerEvent::CallingCardDeclined { transaction } => Some(*transaction),
                _ => None,
            })
            .ok_or("expected a CallingCardDeclined server event")?;
        assert_eq!(transaction, decline_txn);
        Ok(())
    }

    #[test]
    fn client_object_prim_edits_reach_simulator() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);

        let circuit = client.root_circuit_id().ok_or("no circuit")?;
        let shape_target = ScopedObjectId::new(circuit, RegionLocalObjectId(101));
        let image_target = ScopedObjectId::new(circuit, RegionLocalObjectId(102));
        let extra_target = ScopedObjectId::new(circuit, RegionLocalObjectId(103));

        // A distinctive shape so the round-trip cannot pass by accident.
        let shape = PrimShapeParams {
            path_curve: 16,
            profile_curve: 1,
            path_begin: 1000,
            path_end: 2000,
            path_scale_x: 50,
            path_scale_y: 60,
            path_shear_x: 70,
            path_shear_y: 80,
            path_twist: -5,
            path_twist_begin: 5,
            path_radius_offset: -3,
            path_taper_x: 2,
            path_taper_y: -2,
            path_revolutions: 10,
            path_skew: 4,
            profile_begin: 3000,
            profile_end: 4000,
            profile_hollow: 5000,
        };
        client.set_object_shape(shape_target, &shape, now)?;

        // A single neutral face retextures the whole object; the media URL is set.
        let texture = TextureKey::from(uuid::Uuid::from_u128(0xABCD_1234));
        let texture_entry = TextureEntry {
            faces: vec![TextureFace::new(texture)],
        };
        let media_url = "http://example.test/media";
        client.set_object_image(image_target, Some(media_url), &texture_entry, now)?;

        // Extra parameters whose float fields are exactly representable, so the
        // decode round-trips bit-for-bit.
        let params = ObjectExtraParams {
            light: Some(LightData {
                color: [10, 20, 30, 255],
                radius: 8.0,
                cutoff: 0.0,
                falloff: 1.0,
            }),
            ..ObjectExtraParams::default()
        };
        client.set_object_extra_params(extra_target, &params, now)?;
        pump(&mut client, &mut sim, now)?;

        let events = drain_server(&mut sim);

        let (local_id, set_shape) = events
            .iter()
            .find_map(|e| match e {
                ServerEvent::ObjectShapeSet { local_id, shape } => Some((*local_id, *shape)),
                _ => None,
            })
            .ok_or("expected an ObjectShapeSet server event")?;
        assert_eq!(local_id, RegionLocalObjectId(101));
        assert_eq!(set_shape, shape);

        let (local_id, set_media, set_entry) = events
            .iter()
            .find_map(|e| match e {
                ServerEvent::ObjectImageSet {
                    local_id,
                    media_url,
                    texture_entry,
                } => Some((*local_id, media_url.clone(), texture_entry.clone())),
                _ => None,
            })
            .ok_or("expected an ObjectImageSet server event")?;
        assert_eq!(local_id, RegionLocalObjectId(102));
        assert_eq!(set_media.as_deref(), Some(media_url));
        // The wire run-length default makes the single sent face cover every face,
        // so the simulator decodes a full set of faces all carrying that texture.
        assert_eq!(set_entry.faces.len(), MAX_FACES);
        assert!(
            set_entry
                .faces
                .iter()
                .all(|face| face.texture_id == texture)
        );

        let (local_id, set_params) = events
            .iter()
            .find_map(|e| match e {
                ServerEvent::ObjectExtraParamsSet { local_id, params } => {
                    Some((*local_id, params.clone()))
                }
                _ => None,
            })
            .ok_or("expected an ObjectExtraParamsSet server event")?;
        assert_eq!(local_id, RegionLocalObjectId(103));
        assert_eq!(set_params, params);
        Ok(())
    }

    #[test]
    fn client_rez_and_script_permission_edits_reach_simulator() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);

        let circuit = client.root_circuit_id().ok_or("no circuit")?;

        // A fully populated for-sale inventory item, so every RestoreItem field
        // round-trips (a for-sale item carries the sale price back).
        let item = RestoreItem {
            item_id: InventoryKey::from(uuid::Uuid::from_u128(0x17E)),
            folder_id: InventoryFolderKey::from(uuid::Uuid::from_u128(0xF01DE)),
            creator_id: AgentKey::from(uuid::Uuid::from_u128(0xC0EA)),
            owner: OwnerKey::Agent(AgentKey::from(uuid::Uuid::from_u128(0x0E))),
            group: Some(GroupKey::from(uuid::Uuid::from_u128(0x6))),
            permissions: Permissions5 {
                base: Permissions::from_bits(0x0008_0000),
                owner: Permissions::from_bits(0x0008_0000),
                group: Permissions::from_bits(0),
                everyone: Permissions::from_bits(0),
                next_owner: Permissions::from_bits(0x0008_2000),
            },
            transaction_id: uuid::Uuid::from_u128(0x77A),
            asset_type: 10,
            inv_type: 10,
            flags: 0x21,
            sale_type: SaleType::Copy,
            sale_price: Some(LindenAmount(250)),
            name: "Hello World".to_owned(),
            description: "a greeting script".to_owned(),
            creation_date: 1_700_000_000,
            crc: 0xDEAD_BEEF,
        };

        // RezObject: rez the item into the world as a new object.
        let rez_params = RezObjectParams {
            group_id: Some(GroupKey::from(uuid::Uuid::from_u128(0x6))),
            from_task_id: Some(ObjectKey::from(uuid::Uuid::from_u128(0x7A5C))),
            bypass_raycast: true,
            ray_start: sl_types::lsl::Vector {
                x: 1.0,
                y: 2.0,
                z: 3.0,
            },
            ray_end: sl_types::lsl::Vector {
                x: 4.0,
                y: 5.0,
                z: 6.0,
            },
            ray_target_id: Some(ObjectKey::from(uuid::Uuid::from_u128(0x7A46))),
            ray_end_is_intersection: true,
            rez_selected: true,
            remove_item: false,
            item_flags: 0x21,
            group_mask: 0x0008_0000,
            everyone_mask: 0,
            next_owner_mask: 0x0008_2000,
            item: item.clone(),
        };
        client.rez_object_from_inventory(&rez_params, now)?;

        // RezScript: drop the script item into an in-world object's task inventory.
        let script_target = ScopedObjectId::new(circuit, RegionLocalObjectId(202));
        let script_params = RezScriptParams {
            group_id: Some(GroupKey::from(uuid::Uuid::from_u128(0x6))),
            enabled: true,
            item: item.clone(),
        };
        client.rez_script(script_target, &script_params, now)?;

        // RevokePermissions: revoke a couple of granted permissions.
        let revoke_object = ObjectKey::from(uuid::Uuid::from_u128(0x5C217));
        let revoked =
            ScriptPermissions(ScriptPermissions::DEBIT | ScriptPermissions::TAKE_CONTROLS);
        client.revoke_script_permissions(revoke_object, revoked, now)?;

        // DetachAttachmentIntoInv: detach a worn attachment by its item id.
        let detach_item = InventoryKey::from(uuid::Uuid::from_u128(0xA77AC));
        client.detach_attachment_into_inventory(detach_item, now)?;
        pump(&mut client, &mut sim, now)?;

        let events = drain_server(&mut sim);

        let rezzed = events
            .iter()
            .find_map(|e| match e {
                ServerEvent::RezObjectFromInventory { params } => Some(params.clone()),
                _ => None,
            })
            .ok_or("expected a RezObjectFromInventory server event")?;
        assert_eq!(rezzed, rez_params);

        let (local_id, script) = events
            .iter()
            .find_map(|e| match e {
                ServerEvent::RezScript { local_id, params } => Some((*local_id, params.clone())),
                _ => None,
            })
            .ok_or("expected a RezScript server event")?;
        assert_eq!(local_id, RegionLocalObjectId(202));
        assert_eq!(script, script_params);

        let (object_id, permissions) = events
            .iter()
            .find_map(|e| match e {
                ServerEvent::RevokeScriptPermissions {
                    object_id,
                    permissions,
                } => Some((*object_id, *permissions)),
                _ => None,
            })
            .ok_or("expected a RevokeScriptPermissions server event")?;
        assert_eq!(object_id, revoke_object);
        assert_eq!(permissions, revoked);

        let detached = events
            .iter()
            .find_map(|e| match e {
                ServerEvent::DetachAttachmentIntoInventory { item_id } => Some(*item_id),
                _ => None,
            })
            .ok_or("expected a DetachAttachmentIntoInventory server event")?;
        assert_eq!(detached, detach_item);
        Ok(())
    }

    #[test]
    fn client_task_inventory_edits_reach_simulator() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);

        let circuit = client.root_circuit_id().ok_or("no circuit")?;

        // A fully populated for-sale inventory item, so every RestoreItem field
        // round-trips through the UpdateTaskInventory item block.
        let item = RestoreItem {
            item_id: InventoryKey::from(uuid::Uuid::from_u128(0x17E)),
            folder_id: InventoryFolderKey::from(uuid::Uuid::from_u128(0xF01DE)),
            creator_id: AgentKey::from(uuid::Uuid::from_u128(0xC0EA)),
            owner: OwnerKey::Agent(AgentKey::from(uuid::Uuid::from_u128(0x0E))),
            group: Some(GroupKey::from(uuid::Uuid::from_u128(0x6))),
            permissions: Permissions5 {
                base: Permissions::from_bits(0x0008_0000),
                owner: Permissions::from_bits(0x0008_0000),
                group: Permissions::from_bits(0),
                everyone: Permissions::from_bits(0),
                next_owner: Permissions::from_bits(0x0008_2000),
            },
            transaction_id: uuid::Uuid::from_u128(0x77A),
            asset_type: 10,
            inv_type: 10,
            flags: 0x21,
            sale_type: SaleType::Copy,
            sale_price: Some(LindenAmount(250)),
            name: "Hello World".to_owned(),
            description: "a greeting script".to_owned(),
            creation_date: 1_700_000_000,
            crc: 0xDEAD_BEEF,
        };

        // RequestTaskInventory: ask for an object's task inventory listing.
        let request_target = ScopedObjectId::new(circuit, RegionLocalObjectId(301));
        client.request_task_inventory(request_target, now)?;

        // UpdateTaskInventory: write the item into an object's task inventory.
        let update_target = ScopedObjectId::new(circuit, RegionLocalObjectId(302));
        client.update_task_inventory(update_target, TaskInventoryKey::Asset, &item, now)?;

        // MoveTaskInventory: move a task item back into an agent inventory folder.
        let move_target = ScopedObjectId::new(circuit, RegionLocalObjectId(303));
        let move_folder = InventoryFolderKey::from(uuid::Uuid::from_u128(0xF01D3));
        let move_item = InventoryKey::from(uuid::Uuid::from_u128(0x17E3));
        client.move_task_inventory(move_target, move_folder, move_item, now)?;

        // RemoveTaskInventory: delete a task item from an object.
        let remove_target = ScopedObjectId::new(circuit, RegionLocalObjectId(304));
        let remove_item = InventoryKey::from(uuid::Uuid::from_u128(0x17E4));
        client.remove_task_inventory(remove_target, remove_item, now)?;
        pump(&mut client, &mut sim, now)?;

        let events = drain_server(&mut sim);

        let requested = events
            .iter()
            .find_map(|e| match e {
                ServerEvent::RequestTaskInventory { local_id } => Some(*local_id),
                _ => None,
            })
            .ok_or("expected a RequestTaskInventory server event")?;
        assert_eq!(requested, RegionLocalObjectId(301));

        let (update_local, update_key, update_item) = events
            .iter()
            .find_map(|e| match e {
                ServerEvent::UpdateTaskInventory {
                    local_id,
                    key,
                    item,
                } => Some((*local_id, *key, item.clone())),
                _ => None,
            })
            .ok_or("expected an UpdateTaskInventory server event")?;
        assert_eq!(update_local, RegionLocalObjectId(302));
        assert_eq!(update_key, TaskInventoryKey::Asset);
        assert_eq!(update_item, item);

        let (move_local, moved_folder, moved_item) = events
            .iter()
            .find_map(|e| match e {
                ServerEvent::MoveTaskInventory {
                    local_id,
                    folder_id,
                    item_id,
                } => Some((*local_id, *folder_id, *item_id)),
                _ => None,
            })
            .ok_or("expected a MoveTaskInventory server event")?;
        assert_eq!(move_local, RegionLocalObjectId(303));
        assert_eq!(moved_folder, move_folder);
        assert_eq!(moved_item, move_item);

        let (remove_local, removed_item) = events
            .iter()
            .find_map(|e| match e {
                ServerEvent::RemoveTaskInventory { local_id, item_id } => {
                    Some((*local_id, *item_id))
                }
                _ => None,
            })
            .ok_or("expected a RemoveTaskInventory server event")?;
        assert_eq!(remove_local, RegionLocalObjectId(304));
        assert_eq!(removed_item, remove_item);
        Ok(())
    }

    /// The client out-batch-5 land & parcel edits decode into their matching
    /// [`ServerEvent`] variants on the simulator side.
    #[test]
    fn client_land_and_parcel_edits_reach_simulator() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);

        let circuit = client.root_circuit_id().ok_or("no circuit")?;

        // ModifyLand: a whole-parcel raise stroke with a large brush.
        let edit = LandEdit {
            action: LandBrushAction::Raise,
            brush_size: LandBrushSize::Large,
            strength: 0.5,
            height: 23.0,
            parcel: Some(RegionLocalParcelId(9)),
            area: TerraformArea::new(16.0, 32.0, 48.0, 64.0),
        };
        client.modify_land(&edit, now)?;

        // UndoLand: revert the last stroke.
        client.undo_land(now)?;

        // ParcelPropertiesRequestByID: fetch a parcel by local id.
        client.request_parcel_properties_by_id(
            ScopedParcelId::new(circuit, RegionLocalParcelId(9)),
            42,
            now,
        )?;

        // ParcelSetOtherCleanTime: 15 minutes (rounded down on the wire).
        client.set_parcel_other_clean_time(
            ScopedParcelId::new(circuit, RegionLocalParcelId(9)),
            std::time::Duration::from_secs(15 * 60 + 30),
            now,
        )?;
        pump(&mut client, &mut sim, now)?;

        let events = drain_server(&mut sim);

        let modified = events
            .iter()
            .find_map(|e| match e {
                ServerEvent::ModifyLand { edit } => Some(*edit),
                _ => None,
            })
            .ok_or("expected a ModifyLand server event")?;
        assert_eq!(modified, edit);

        assert!(
            events.iter().any(|e| matches!(e, ServerEvent::UndoLand)),
            "expected an UndoLand server event"
        );

        let (requested, sequence) = events
            .iter()
            .find_map(|e| match e {
                ServerEvent::RequestParcelPropertiesById {
                    local_id,
                    sequence_id,
                } => Some((*local_id, *sequence_id)),
                _ => None,
            })
            .ok_or("expected a RequestParcelPropertiesById server event")?;
        assert_eq!(requested, RegionLocalParcelId(9));
        assert_eq!(sequence, 42);

        let (clean_parcel, clean_time) = events
            .iter()
            .find_map(|e| match e {
                ServerEvent::SetParcelOtherCleanTime {
                    local_id,
                    clean_time,
                } => Some((*local_id, *clean_time)),
                _ => None,
            })
            .ok_or("expected a SetParcelOtherCleanTime server event")?;
        assert_eq!(clean_parcel, RegionLocalParcelId(9));
        // The 30 seconds over 15 minutes are dropped by the whole-minute wire field.
        assert_eq!(clean_time, std::time::Duration::from_secs(15 * 60));
        Ok(())
    }

    #[test]
    fn client_inventory_link_and_group_info_reach_simulator() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);

        // LinkInventoryItem: an item link (AT_LINK = 24).
        let item_link = NewInventoryLink {
            folder_id: InventoryFolderKey::from(uuid::Uuid::from_u128(0x3001)),
            linked_id: InventoryItemOrFolderKey::Item(InventoryKey::from(uuid::Uuid::from_u128(
                0x3002,
            ))),
            link_type: AssetType::Other(24),
            inv_type: InventoryType::Script,
            name: "my link".to_owned(),
            description: "a link to an item".to_owned(),
        };
        let item_callback = client.link_inventory_item(&item_link, now)?;

        // LinkInventoryItem: a folder link (AT_LINK_FOLDER = 25).
        let folder_link = NewInventoryLink {
            folder_id: InventoryFolderKey::from(uuid::Uuid::from_u128(0x3003)),
            linked_id: InventoryItemOrFolderKey::Folder(InventoryFolderKey::from(
                uuid::Uuid::from_u128(0x3004),
            )),
            link_type: AssetType::Other(25),
            inv_type: InventoryType::Other(-1),
            name: "my folder link".to_owned(),
            description: String::new(),
        };
        client.link_inventory_item(&folder_link, now)?;

        // UpdateGroupInfo: edit an existing group's profile.
        let params = UpdateGroupInfoParams {
            group_id: GroupKey::from(uuid::Uuid::from_u128(0x4001)),
            charter: "be excellent to each other".to_owned(),
            show_in_list: true,
            insignia_id: Some(TextureKey::from(uuid::Uuid::from_u128(0x4002))),
            membership_fee: LindenAmount(42),
            open_enrollment: true,
            allow_publish: false,
            mature_publish: true,
        };
        client.update_group_info(&params, now)?;

        // GroupTitleUpdate: set the active title to a role.
        let group_id = GroupKey::from(uuid::Uuid::from_u128(0x4001));
        let title_role_id = GroupRoleKey::from(uuid::Uuid::from_u128(0x4003));
        client.update_group_title(group_id, title_role_id, now)?;
        pump(&mut client, &mut sim, now)?;

        let events = drain_server(&mut sim);

        let (decoded_item, decoded_item_callback) = events
            .iter()
            .find_map(|e| match e {
                ServerEvent::LinkInventoryItem { link, callback_id }
                    if link.linked_id.is_item() =>
                {
                    Some((link.clone(), *callback_id))
                }
                _ => None,
            })
            .ok_or("expected an item LinkInventoryItem server event")?;
        assert_eq!(decoded_item, item_link);
        assert_eq!(decoded_item_callback, item_callback.get());

        let decoded_folder = events
            .iter()
            .find_map(|e| match e {
                ServerEvent::LinkInventoryItem { link, .. } if link.linked_id.is_folder() => {
                    Some(link.clone())
                }
                _ => None,
            })
            .ok_or("expected a folder LinkInventoryItem server event")?;
        assert_eq!(decoded_folder, folder_link);

        let decoded_params = events
            .iter()
            .find_map(|e| match e {
                ServerEvent::UpdateGroupInfo { params } => Some(params.clone()),
                _ => None,
            })
            .ok_or("expected an UpdateGroupInfo server event")?;
        assert_eq!(decoded_params, params);

        let (decoded_group, decoded_role) = events
            .iter()
            .find_map(|e| match e {
                ServerEvent::UpdateGroupTitle {
                    group_id,
                    title_role_id,
                } => Some((*group_id, *title_role_id)),
                _ => None,
            })
            .ok_or("expected an UpdateGroupTitle server event")?;
        assert_eq!(decoded_group, group_id);
        assert_eq!(decoded_role, title_role_id);
        Ok(())
    }

    #[test]
    fn client_teleport_and_agent_prefs_reach_simulator() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);

        // TeleportLandmarkRequest: a landmark teleport carries the asset id.
        let landmark = AssetKey::from(uuid::Uuid::from_u128(0x5001));
        client.teleport_via_landmark(Some(landmark), now)?;
        // Cancelling returns the client to the active state so the following
        // requests are accepted.
        client.cancel_teleport(now)?;
        // TeleportLandmarkRequest: a home teleport (None) carries a nil asset id.
        client.teleport_via_landmark(None, now)?;
        client.cancel_teleport(now)?;

        // SetStartLocationRequest: record "home" at a region-local position.
        let position = RegionCoordinates::new(64.0, 96.0, 25.0);
        let look_at = sl_types::lsl::Vector {
            x: 1.0,
            y: 0.0,
            z: 0.0,
        };
        client.set_start_location(StartLocationSlot::Home, position, look_at.clone(), now)?;

        // AgentDataUpdateRequest, AgentQuitCopy, VelocityInterpolateOn/Off.
        client.request_agent_data_update(now)?;
        client.quit_copy(now)?;
        client.set_velocity_interpolation(true, now)?;
        client.set_velocity_interpolation(false, now)?;
        pump(&mut client, &mut sim, now)?;

        let events = drain_server(&mut sim);

        // Both the landmark teleport and the home teleport decode.
        let landmarks: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                ServerEvent::TeleportViaLandmark { landmark } => Some(*landmark),
                _ => None,
            })
            .collect();
        assert_eq!(landmarks, vec![Some(landmark), None]);

        // Two cancels arrive.
        let cancels = events
            .iter()
            .filter(|e| matches!(e, ServerEvent::CancelTeleport))
            .count();
        assert_eq!(cancels, 2);

        let (decoded_slot, decoded_position, decoded_look_at) = events
            .iter()
            .find_map(|e| match e {
                ServerEvent::SetStartLocation {
                    slot,
                    position,
                    look_at,
                } => Some((*slot, *position, look_at.clone())),
                _ => None,
            })
            .ok_or("expected a SetStartLocation server event")?;
        assert_eq!(decoded_slot, StartLocationSlot::Home);
        assert_eq!(decoded_position, position);
        assert_eq!(decoded_look_at.x.to_bits(), look_at.x.to_bits());

        assert!(
            events
                .iter()
                .any(|e| matches!(e, ServerEvent::RequestAgentDataUpdate)),
            "expected a RequestAgentDataUpdate server event"
        );

        // AgentQuitCopy's FuseBlock echoes the client's own (non-zero) circuit
        // code.
        let quit_code = events
            .iter()
            .find_map(|e| match e {
                ServerEvent::QuitCopy {
                    viewer_circuit_code,
                } => Some(*viewer_circuit_code),
                _ => None,
            })
            .ok_or("expected a QuitCopy server event")?;
        assert_eq!(quit_code, CircuitCode(0x0011_2233));

        // Both velocity-interpolation toggles decode, in order.
        let toggles: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                ServerEvent::SetVelocityInterpolation { enabled } => Some(*enabled),
                _ => None,
            })
            .collect();
        assert_eq!(toggles, vec![true, false]);
        Ok(())
    }

    #[test]
    fn client_user_info_and_sound_reach_simulator() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);

        // UserInfoRequest: poll for the agent's own account preferences.
        client.request_user_info(now)?;
        // UpdateUserInfo: forward offline IMs to email and hide from search.
        client.update_user_info(true, DirectoryVisibility::Hidden, now)?;
        // SoundTrigger: play a one-shot sound at a region-local position.
        let sound = AssetKey::from(uuid::Uuid::from_u128(0x5002));
        let position = RegionCoordinates::new(128.0, 64.0, 30.0);
        client.trigger_sound(sound, 0.75, RegionHandle(REGION_HANDLE), position, now)?;
        pump(&mut client, &mut sim, now)?;

        let events = drain_server(&mut sim);

        assert!(
            events
                .iter()
                .any(|e| matches!(e, ServerEvent::RequestUserInfo)),
            "expected a RequestUserInfo server event"
        );

        let (decoded_im, decoded_visibility) = events
            .iter()
            .find_map(|e| match e {
                ServerEvent::UpdateUserInfo {
                    im_via_email,
                    directory_visibility,
                } => Some((*im_via_email, *directory_visibility)),
                _ => None,
            })
            .ok_or("expected an UpdateUserInfo server event")?;
        assert!(decoded_im);
        assert_eq!(decoded_visibility, DirectoryVisibility::Hidden);

        let (decoded_sound, decoded_gain, decoded_handle, decoded_position) = events
            .iter()
            .find_map(|e| match e {
                ServerEvent::TriggerSound {
                    sound,
                    gain,
                    region_handle,
                    position,
                } => Some((*sound, *gain, *region_handle, *position)),
                _ => None,
            })
            .ok_or("expected a TriggerSound server event")?;
        assert_eq!(decoded_sound, sound);
        assert_eq!(decoded_gain.to_bits(), 0.75_f32.to_bits());
        assert_eq!(decoded_handle, RegionHandle(REGION_HANDLE));
        assert_eq!(decoded_position, position);
        Ok(())
    }

    #[test]
    fn client_god_region_admin_reaches_simulator() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);

        // RequestGodlikePowers: ask the simulator to grant god powers.
        client.request_godlike_powers(true, now)?;
        // EjectUser: eject and ban an avatar from the agent's land.
        let ejected = AgentKey::from(uuid::Uuid::from_u128(0x9001));
        client.eject_user(ejected, EjectAction::EjectAndBan, now)?;
        // FreezeUser: unfreeze an avatar on the agent's land.
        let frozen = AgentKey::from(uuid::Uuid::from_u128(0x9002));
        client.freeze_user(frozen, FreezeAction::Unfreeze, now)?;
        // SimWideDeletes: return a scripted owner's objects region-wide.
        let owner = AgentKey::from(uuid::Uuid::from_u128(0x9003));
        let delete_flags = SimWideDeleteFlags {
            others_land_only: false,
            always_return_objects: true,
            scripted_only: true,
        };
        client.sim_wide_deletes(owner, delete_flags, now)?;
        // GodUpdateRegionInfo: push god-tools region parameters.
        let update = GodRegionUpdate {
            sim_name: sl_proto::RegionName::try_new("Da Boom")
                .map_err(|_invalid| "invalid region name")?,
            estate_id: 1,
            parent_estate_id: 1,
            region_flags: 0x1_0000_0007,
            billable_factor: 1.0,
            price_per_meter: 5,
            redirect_grid: GridCoordinates::new(1000, 1001),
        };
        client.god_update_region_info(&update, now)?;
        pump(&mut client, &mut sim, now)?;

        let events = drain_server(&mut sim);

        let decoded_godlike = events
            .iter()
            .find_map(|e| match e {
                ServerEvent::RequestGodlikePowers { godlike } => Some(*godlike),
                _ => None,
            })
            .ok_or("expected a RequestGodlikePowers server event")?;
        assert!(decoded_godlike);

        let (eject_target, eject_action) = events
            .iter()
            .find_map(|e| match e {
                ServerEvent::EjectUser { target, action } => Some((*target, *action)),
                _ => None,
            })
            .ok_or("expected an EjectUser server event")?;
        assert_eq!(eject_target, ejected);
        assert_eq!(eject_action, EjectAction::EjectAndBan);

        let (freeze_target, freeze_action) = events
            .iter()
            .find_map(|e| match e {
                ServerEvent::FreezeUser { target, action } => Some((*target, *action)),
                _ => None,
            })
            .ok_or("expected a FreezeUser server event")?;
        assert_eq!(freeze_target, frozen);
        assert_eq!(freeze_action, FreezeAction::Unfreeze);

        let (delete_owner, decoded_delete_flags) = events
            .iter()
            .find_map(|e| match e {
                ServerEvent::SimWideDeletes { owner, flags } => Some((*owner, *flags)),
                _ => None,
            })
            .ok_or("expected a SimWideDeletes server event")?;
        assert_eq!(delete_owner, owner);
        assert_eq!(decoded_delete_flags, delete_flags);

        let decoded_update = events
            .iter()
            .find_map(|e| match e {
                ServerEvent::GodUpdateRegionInfo { update } => Some(update.clone()),
                _ => None,
            })
            .ok_or("expected a GodUpdateRegionInfo server event")?;
        // The extended flags are recovered from the RegionInfo2 block.
        assert_eq!(decoded_update, update);
        Ok(())
    }

    /// The client out-batch-10 god parcel/object/land-admin edits decode into
    /// their matching [`ServerEvent`] variants on the simulator side.
    #[test]
    fn client_god_parcel_admin_reaches_simulator() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);

        let circuit = client.root_circuit_id().ok_or("no circuit")?;

        // ParcelGodForceOwner: force-reassign a parcel to a new owner.
        let new_owner = AgentKey::from(uuid::Uuid::from_u128(0xA001));
        client.parcel_god_force_owner(
            ScopedParcelId::new(circuit, RegionLocalParcelId(11)),
            OwnerKey::Agent(new_owner),
            now,
        )?;
        // ParcelGodMarkAsContent: mark a parcel as governor-owned content.
        client.parcel_god_mark_as_content(
            ScopedParcelId::new(circuit, RegionLocalParcelId(12)),
            now,
        )?;
        // EventGodDelete: delete an events listing and re-run the search.
        let query_id = QueryId::new(uuid::Uuid::from_u128(0xA002));
        client.event_god_delete(
            EventId::new(54_321),
            query_id,
            "fun event",
            DirFindFlags::EVENTS.union(DirFindFlags::INC_ADULT),
            20,
            now,
        )?;
        // StateSave: save the region state with an explicit filename.
        client.state_save("backup.oar", now)?;
        // StateSave again with an empty filename (autosave name).
        client.state_save("", now)?;
        // ViewerStartAuction: start a land auction advertised by a snapshot.
        let snapshot = TextureKey::from(uuid::Uuid::from_u128(0xA003));
        client.viewer_start_auction(
            ScopedParcelId::new(circuit, RegionLocalParcelId(13)),
            Some(snapshot),
            now,
        )?;
        pump(&mut client, &mut sim, now)?;

        let events = drain_server(&mut sim);

        let (force_parcel, force_owner) = events
            .iter()
            .find_map(|e| match e {
                ServerEvent::ParcelGodForceOwner { local_id, owner } => Some((*local_id, *owner)),
                _ => None,
            })
            .ok_or("expected a ParcelGodForceOwner server event")?;
        assert_eq!(force_parcel, RegionLocalParcelId(11));
        assert_eq!(force_owner, OwnerKey::Agent(new_owner));

        let mark_parcel = events
            .iter()
            .find_map(|e| match e {
                ServerEvent::ParcelGodMarkAsContent { local_id } => Some(*local_id),
                _ => None,
            })
            .ok_or("expected a ParcelGodMarkAsContent server event")?;
        assert_eq!(mark_parcel, RegionLocalParcelId(12));

        let decoded_delete = events
            .iter()
            .find_map(|e| match e {
                ServerEvent::EventGodDelete {
                    event,
                    query_id,
                    query_text,
                    flags,
                    query_start,
                } => Some((*event, *query_id, query_text.clone(), *flags, *query_start)),
                _ => None,
            })
            .ok_or("expected an EventGodDelete server event")?;
        assert_eq!(decoded_delete.0, EventId::new(54_321));
        assert_eq!(decoded_delete.1, query_id);
        assert_eq!(decoded_delete.2, "fun event");
        assert_eq!(
            decoded_delete.3,
            DirFindFlags::EVENTS.union(DirFindFlags::INC_ADULT)
        );
        assert_eq!(decoded_delete.4, 20);

        let filenames: Vec<Option<String>> = events
            .iter()
            .filter_map(|e| match e {
                ServerEvent::StateSave { filename } => Some(filename.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            filenames,
            vec![Some("backup.oar".to_owned()), None],
            "explicit filename then autosave (empty -> None)"
        );

        let (auction_parcel, auction_snapshot) = events
            .iter()
            .find_map(|e| match e {
                ServerEvent::ViewerStartAuction { local_id, snapshot } => {
                    Some((*local_id, *snapshot))
                }
                _ => None,
            })
            .ok_or("expected a ViewerStartAuction server event")?;
        assert_eq!(auction_parcel, RegionLocalParcelId(13));
        assert_eq!(auction_snapshot, Some(snapshot));
        Ok(())
    }

    #[test]
    fn inventory_sync_reaches_client() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_client(&mut client);

        let removed_item = InventoryKey::from(uuid::Uuid::from_u128(0x1001));
        let removed_folder = InventoryFolderKey::from(uuid::Uuid::from_u128(0x2001));
        let mixed_folder = InventoryFolderKey::from(uuid::Uuid::from_u128(0x2002));
        let mixed_item = InventoryKey::from(uuid::Uuid::from_u128(0x1002));
        let moved_item = InventoryKey::from(uuid::Uuid::from_u128(0x1003));
        let dest_folder = InventoryFolderKey::from(uuid::Uuid::from_u128(0x2003));
        let renamed_item = InventoryKey::from(uuid::Uuid::from_u128(0x1004));
        let renamed_folder = InventoryFolderKey::from(uuid::Uuid::from_u128(0x2004));

        let moves = vec![
            InventoryItemMove {
                item: moved_item,
                folder: dest_folder,
                new_name: None,
            },
            InventoryItemMove {
                item: renamed_item,
                folder: renamed_folder,
                new_name: Some("renamed".to_owned()),
            },
        ];

        sim.send_remove_inventory_item(&[removed_item], now)?;
        sim.send_remove_inventory_folder(&[removed_folder], now)?;
        sim.send_remove_inventory_objects(&[mixed_folder], &[mixed_item], now)?;
        sim.send_move_inventory_item(true, &moves, now)?;
        pump(&mut client, &mut sim, now)?;

        let events = drain_client(&mut client);
        let items = events
            .iter()
            .find_map(|e| match e {
                Event::InventoryItemsRemoved { items } => Some(items.clone()),
                _ => None,
            })
            .ok_or("expected an InventoryItemsRemoved client event")?;
        assert_eq!(items, vec![removed_item]);

        let folders = events
            .iter()
            .find_map(|e| match e {
                Event::InventoryFoldersRemoved { folders } => Some(folders.clone()),
                _ => None,
            })
            .ok_or("expected an InventoryFoldersRemoved client event")?;
        assert_eq!(folders, vec![removed_folder]);

        let (folders, items) = events
            .iter()
            .find_map(|e| match e {
                Event::InventoryObjectsRemoved { folders, items } => {
                    Some((folders.clone(), items.clone()))
                }
                _ => None,
            })
            .ok_or("expected an InventoryObjectsRemoved client event")?;
        assert_eq!(folders, vec![mixed_folder]);
        assert_eq!(items, vec![mixed_item]);

        let (stamp, got_moves) = events
            .iter()
            .find_map(|e| match e {
                Event::InventoryItemsMoved { stamp, moves } => Some((*stamp, moves.clone())),
                _ => None,
            })
            .ok_or("expected an InventoryItemsMoved client event")?;
        assert!(stamp);
        assert_eq!(got_moves, moves);
        Ok(())
    }

    #[test]
    fn task_inventory_user_info_and_misc_reach_client() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_client(&mut client);

        let reply = TaskInventoryReply {
            task: ObjectKey::from(uuid::Uuid::from_u128(0x7A5C)),
            serial: 7,
            filename: "inventory_7A5C.tmp".to_owned(),
        };
        let info = UserInfo {
            im_via_email: true,
            directory_visibility: DirectoryVisibility::Hidden,
            email: "agent@example.com".to_owned(),
        };
        let derez_txn = TransactionId::from(uuid::Uuid::from_u128(0xDE7E));
        let selected = [RegionLocalObjectId(101), RegionLocalObjectId(202)];

        sim.send_reply_task_inventory(&reply, now)?;
        sim.send_user_info_reply(&info, now)?;
        sim.send_derez_ack(derez_txn, true, now)?;
        sim.send_force_object_select(true, &selected, now)?;
        sim.send_grant_godlike_powers(200, now)?;
        pump(&mut client, &mut sim, now)?;

        let events = drain_client(&mut client);
        let got_reply = events
            .iter()
            .find_map(|e| match e {
                Event::TaskInventoryReply(reply) => Some(reply.clone()),
                _ => None,
            })
            .ok_or("expected a TaskInventoryReply client event")?;
        assert_eq!(got_reply, reply);

        let got_info = events
            .iter()
            .find_map(|e| match e {
                Event::UserInfo(info) => Some(info.clone()),
                _ => None,
            })
            .ok_or("expected a UserInfo client event")?;
        assert_eq!(got_info, info);

        let (transaction, success) = events
            .iter()
            .find_map(|e| match e {
                Event::DeRezAck {
                    transaction,
                    success,
                } => Some((*transaction, *success)),
                _ => None,
            })
            .ok_or("expected a DeRezAck client event")?;
        assert_eq!(transaction, derez_txn);
        assert!(success);

        let (reset_list, objects) = events
            .iter()
            .find_map(|e| match e {
                Event::ForceObjectSelect {
                    reset_list,
                    objects,
                } => Some((*reset_list, objects.clone())),
                _ => None,
            })
            .ok_or("expected a ForceObjectSelect client event")?;
        assert!(reset_list);
        let local_ids: Vec<RegionLocalObjectId> = objects.iter().map(|o| o.id()).collect();
        assert_eq!(local_ids, selected.to_vec());

        let god_level = events
            .iter()
            .find_map(|e| match e {
                Event::GodlikePowersGranted { god_level } => Some(*god_level),
                _ => None,
            })
            .ok_or("expected a GodlikePowersGranted client event")?;
        assert_eq!(god_level, 200);
        Ok(())
    }

    #[test]
    fn simulator_chat_reaches_client() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_client(&mut client);

        sim.send_chat_from_simulator(
            "Region",
            ChatSource::System,
            uuid::Uuid::nil(),
            ChatType::Normal,
            1,
            sl_types::lsl::Vector {
                x: 1.0,
                y: 2.0,
                z: 3.0,
            },
            "welcome",
            now,
        )?;
        pump(&mut client, &mut sim, now)?;

        let events = drain_client(&mut client);
        let chat = events
            .iter()
            .find_map(|e| match e {
                Event::ChatReceived(chat) => Some(chat.clone()),
                _ => None,
            })
            .ok_or("expected a ChatReceived client event")?;
        assert_eq!(chat.message, "welcome");
        assert_eq!(chat.from_name, "Region");
        assert_eq!(chat.chat_type, ChatType::Normal);
        Ok(())
    }

    // ---- Inbound chat/presence reach the client store (B10) -----------------
    //
    // The inbound mirror of `friendship_and_calling_cards_reach_client`: a real
    // `SimSession` sends an IM / presence notification / `ChatterBoxInvitation`
    // and the client's grid-level chat/presence stores reflect it. These guard
    // the wire decode + fold under a real peer, not just the in-memory fold that
    // `lifecycle.rs` exercises directly.

    /// A simulator-sent 1:1 IM opens a `Direct` session on the client keyed by the
    /// sender, logs the message, and bumps the unread count.
    #[test]
    fn inbound_instant_message_reaches_client_store() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_client(&mut client);

        let peer = uuid::Uuid::from_u128(0x77);
        let im = AnyMessage::ImprovedInstantMessage(ImprovedInstantMessage {
            agent_data: ImprovedInstantMessageAgentDataBlock {
                agent_id: peer,
                session_id: uuid::Uuid::nil(),
            },
            message_block: ImprovedInstantMessageMessageBlockBlock {
                from_group: false,
                to_agent_id: uuid::Uuid::from_u128(1),
                parent_estate_id: 1,
                region_id: uuid::Uuid::from_u128(0x7),
                position: sl_types::lsl::Vector {
                    x: 1.0,
                    y: 2.0,
                    z: 3.0,
                },
                offline: 0,
                dialog: 0,
                id: uuid::Uuid::from_u128(0xABC),
                timestamp: 0,
                from_agent_name: b"Sim Peer\0".to_vec(),
                message: b"ping from the sim\0".to_vec(),
                binary_bucket: Vec::new(),
            },
            estate_block: ImprovedInstantMessageEstateBlockBlock { estate_id: 1 },
            meta_data: Vec::new(),
        });
        sim.push(&im, Reliability::Reliable, now)?;
        pump(&mut client, &mut sim, now)?;

        let kind = ChatSessionKind::Direct {
            peer: AgentKey::from(peer),
        };
        let logged: Vec<_> = client.history(kind).cloned().collect();
        assert_eq!(logged.len(), 1, "the IM was logged to the 1:1 session");
        let entry = logged.first().ok_or("expected a logged message")?;
        assert_eq!(entry.sender, AgentKey::from(peer));
        assert_eq!(entry.dialog, ImDialog::Message);
        assert_eq!(entry.text, "ping from the sim");
        assert_eq!(client.unread(kind), 1);
        Ok(())
    }

    /// Simulator-sent `OnlineNotification` / `OfflineNotification` toggle the
    /// client's presence store.
    #[test]
    fn inbound_presence_notifications_reach_client_store() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_client(&mut client);

        let friend = uuid::Uuid::from_u128(0xF1);
        let online = AnyMessage::OnlineNotification(OnlineNotification {
            agent_block: vec![OnlineNotificationAgentBlockBlock { agent_id: friend }],
        });
        sim.push(&online, Reliability::Reliable, now)?;
        pump(&mut client, &mut sim, now)?;
        assert!(
            client.is_online(FriendKey::from(friend)),
            "the OnlineNotification marked the buddy online"
        );

        let offline = AnyMessage::OfflineNotification(OfflineNotification {
            agent_block: vec![OfflineNotificationAgentBlockBlock { agent_id: friend }],
        });
        sim.push(&offline, Reliability::Reliable, after(now, 10)?)?;
        pump(&mut client, &mut sim, after(now, 10)?)?;
        assert!(
            !client.is_online(FriendKey::from(friend)),
            "the OfflineNotification marked the buddy offline"
        );
        Ok(())
    }

    /// A simulator-queued `ChatterBoxInvitation` (over the CAPS event queue)
    /// records a pending `Invited` conference session on the client.
    #[test]
    fn inbound_chatterbox_invitation_reaches_client_store() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_client(&mut client);

        let conference = uuid::Uuid::from_u128(0x6801);
        let inviter = AgentKey::from(uuid::Uuid::from_u128(0x6802));
        let invitation = Event::ConferenceInvited {
            session_id: conference,
            from_agent_id: inviter,
            from_name: "Inviter".to_owned(),
            dialog: ImDialog::SessionConferenceStart,
            from_group: false,
            session_name: "Chat".to_owned(),
            message: "join us".to_owned(),
            region_id: uuid::Uuid::nil(),
            position: RegionCoordinates::new(1.0, 2.0, 3.0),
            parent_estate_id: 1,
            timestamp: None,
            binary_bucket: Vec::new(),
        };
        sim.enqueue_caps_event(
            "ChatterBoxInvitation",
            chatterbox_invitation_to_llsd(&invitation),
        );
        deliver_caps(&mut client, &mut sim, now)?;

        let kind = ChatSessionKind::Conference {
            id: ImSessionId::from(conference),
        };
        let info = client
            .chat_sessions_info()
            .find(|info| info.kind == kind)
            .ok_or("expected the invited conference session on the client")?;
        assert_eq!(
            info.lifecycle,
            ChatLifecycleView::Invited {
                inviter,
                session_name: "Chat".to_owned(),
                channel: InviteChannel::Text,
            }
        );
        Ok(())
    }

    #[test]
    fn client_instant_message_reaches_simulator() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);

        let target = uuid::Uuid::from_u128(99);
        client.send_instant_message(AgentKey::from(target), "psst", now)?;
        pump(&mut client, &mut sim, now)?;

        let events = drain_server(&mut sim);
        let im = events
            .iter()
            .find_map(|e| match e {
                ServerEvent::InstantMessage(im) => Some(im.clone()),
                _ => None,
            })
            .ok_or("expected an InstantMessage server event")?;
        assert_eq!(im.message, "psst");
        assert_eq!(im.to_agent_id, AgentKey::from(target));
        assert_eq!(im.dialog, ImDialog::Message);
        Ok(())
    }

    #[test]
    fn client_throttle_reaches_simulator() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);

        let throttle = Throttle::preset_500();
        client.set_throttle(throttle, now)?;
        pump(&mut client, &mut sim, now)?;

        let events = drain_server(&mut sim);
        let decoded = events
            .iter()
            .find_map(|e| match e {
                ServerEvent::Throttle(throttle) => Some(*throttle),
                _ => None,
            })
            .ok_or("expected a Throttle server event")?;
        // The seven preset rates are exact in `f32`, so the bits-per-second
        // round-trip reproduces the throttle exactly.
        assert_eq!(decoded, throttle);
        Ok(())
    }

    #[test]
    fn replies_to_client_ping() -> Result<(), TestError> {
        let now = Instant::now();
        let (_client, mut sim) = setup(now)?;
        drain_server(&mut sim);

        // The client pings the link; the simulator answers with CompletePingCheck.
        let ping = AnyMessage::StartPingCheck(StartPingCheck {
            ping_id: StartPingCheckPingIDBlock {
                ping_id: 0x2A,
                oldest_unacked: 0,
            },
        });
        let datagram = client_datagram(&ping, 500, false)?;
        sim.handle_datagram(client_addr(), &datagram, now)?;

        let reply = sim.poll_transmit().ok_or("a CompletePingCheck was sent")?;
        let Some(AnyMessage::CompletePingCheck(reply)) = decode(&reply).ok() else {
            return Err("expected a CompletePingCheck".into());
        };
        assert_eq!(reply.ping_id.ping_id, 0x2A);

        let events = drain_server(&mut sim);
        assert!(
            events.iter().any(|e| matches!(
                e,
                ServerEvent::PingRequested {
                    ping_id: PingId(0x2A)
                }
            )),
            "expected PingRequested, got {events:?}"
        );
        Ok(())
    }

    #[test]
    fn periodic_ping_is_answered_by_client() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;

        // Past the ping cadence the simulator pings the client.
        let later = after(now, 6000)?;
        sim.handle_timeout(later);
        let sent = {
            let mut out = Vec::new();
            while let Some(transmit) = sim.poll_transmit() {
                out.push(decode(&transmit)?);
            }
            out
        };
        assert!(
            sent.iter()
                .any(|m| matches!(m, AnyMessage::StartPingCheck(_))),
            "expected a StartPingCheck, got {sent:?}"
        );

        // The client answers, and the simulator consumes it without surfacing an
        // event or closing.
        for message in &sent {
            if let AnyMessage::StartPingCheck(_) = message {
                let datagram = client_datagram(message, 1, false)?;
                client.handle_datagram(sim_addr(), &datagram, later)?;
            }
        }
        pump(&mut client, &mut sim, later)?;
        assert!(!sim.is_closed());
        Ok(())
    }

    #[test]
    fn clean_logout_round_trip() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);
        drain_client(&mut client);

        client.initiate_logout(now);
        pump(&mut client, &mut sim, now)?;

        let events = drain_server(&mut sim);
        assert!(
            events.iter().any(|e| matches!(e, ServerEvent::LoggedOut)),
            "expected LoggedOut, got {events:?}"
        );
        assert!(sim.is_closed());
        assert!(client.is_closed());
        Ok(())
    }

    #[test]
    fn acknowledges_reliable_inbound() -> Result<(), TestError> {
        let now = Instant::now();
        let (_client, mut sim) = setup(now)?;
        // Circuit setup already exchanged reliable packets (UseCircuitCode,
        // CompleteAgentMovement); flushing the ack timer sends the owed
        // acknowledgements back to the client.
        let flush_at = sim.poll_timeout().ok_or("a timeout is scheduled")?;
        sim.handle_timeout(flush_at);
        let acked = {
            let mut out = Vec::new();
            while let Some(transmit) = sim.poll_transmit() {
                out.push(decode(&transmit)?);
            }
            out
        };
        assert!(
            acked.iter().any(|m| matches!(m, AnyMessage::PacketAck(_))),
            "expected a PacketAck, got {acked:?}"
        );
        Ok(())
    }

    /// Losing a reliable client datagram's first transmission must not lose
    /// the message: the resend timer re-emits the same sequence with the
    /// `RESENT` wire flag, the simulator processes the retransmitted copy
    /// normally, and a duplicate delivery of the same datagram is
    /// acknowledged but not dispatched a second time.
    #[test]
    fn client_reliable_resend_survives_loss_and_sim_deduplicates() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);

        client.say("resend me", ChatType::Normal, ChatChannel(0), now)?;

        // "Lose" the chat datagram: keep its sequence but do not deliver it;
        // everything else flows normally.
        let mut lost_sequence = None;
        while let Some(transmit) = client.poll_transmit() {
            let parsed = parse_datagram(&transmit.payload)?;
            if matches!(decode(&transmit)?, AnyMessage::ChatFromViewer(_)) {
                assert!(
                    parsed.flags.contains(PacketFlags::RELIABLE),
                    "chat is sent reliably"
                );
                assert!(
                    !parsed.flags.contains(PacketFlags::RESENT),
                    "the first transmission is not flagged RESENT"
                );
                lost_sequence = Some(parsed.sequence);
            } else {
                sim.handle_datagram(client_addr(), &transmit.payload, now)?;
            }
        }
        let lost_sequence = lost_sequence.ok_or("expected the chat datagram")?;
        assert!(
            !drain_server(&mut sim)
                .iter()
                .any(|e| matches!(e, ServerEvent::Chat { .. })),
            "the dropped chat must not have reached the simulator"
        );

        // Past the resend timeout the client re-emits the same sequence,
        // now carrying the RESENT flag. The timeout follows the circuit's
        // averaged round trip (five times it, floored at a second), so this
        // clears even the untested-circuit default of one second.
        let later = after(now, 5_600)?;
        client.handle_timeout(later);
        let mut resent_payload = None;
        while let Some(transmit) = client.poll_transmit() {
            let parsed = parse_datagram(&transmit.payload)?;
            if matches!(decode(&transmit)?, AnyMessage::ChatFromViewer(_)) {
                assert_eq!(
                    parsed.sequence, lost_sequence,
                    "a resend reuses the original sequence number"
                );
                assert!(
                    parsed.flags.contains(PacketFlags::RESENT),
                    "a resend carries the RESENT wire flag"
                );
                assert!(
                    parsed.flags.contains(PacketFlags::RELIABLE),
                    "a resend stays reliable"
                );
                resent_payload = Some(transmit.payload.clone());
            } else {
                sim.handle_datagram(client_addr(), &transmit.payload, later)?;
            }
        }
        let resent_payload = resent_payload.ok_or("expected the chat to be retransmitted")?;

        // The retransmitted copy dispatches normally...
        sim.handle_datagram(client_addr(), &resent_payload, later)?;
        let delivered = drain_server(&mut sim)
            .iter()
            .filter(|e| matches!(e, ServerEvent::Chat { .. }))
            .count();
        assert_eq!(
            delivered, 1,
            "the retransmitted chat dispatches exactly once"
        );

        // ...and a duplicate delivery of the very same datagram is
        // deduplicated by the inbound seen-window (still acked, not
        // re-dispatched).
        sim.handle_datagram(client_addr(), &resent_payload, later)?;
        let duplicated = drain_server(&mut sim)
            .iter()
            .filter(|e| matches!(e, ServerEvent::Chat { .. }))
            .count();
        assert_eq!(
            duplicated, 0,
            "a duplicate reliable datagram is not re-dispatched"
        );

        // The ack flow settles the circuit (the client stops resending).
        pump(&mut client, &mut sim, later)?;
        Ok(())
    }

    /// The simulator-side mirror: a lost reliable simulator datagram is
    /// retransmitted with the `RESENT` flag and the same sequence, the client
    /// processes the retransmitted copy normally, and a duplicate delivery is
    /// not surfaced twice.
    #[test]
    fn sim_reliable_resend_survives_loss_and_client_deduplicates() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_client(&mut client);

        sim.send_alert_message("resent notice", &[], &[], now)?;

        // "Lose" the alert datagram: keep its sequence but do not deliver it.
        let mut lost_sequence = None;
        while let Some(transmit) = sim.poll_transmit() {
            let parsed = parse_datagram(&transmit.payload)?;
            if matches!(decode(&transmit)?, AnyMessage::AlertMessage(_)) {
                assert!(
                    parsed.flags.contains(PacketFlags::RELIABLE),
                    "the alert is sent reliably"
                );
                lost_sequence = Some(parsed.sequence);
            } else {
                client.handle_datagram(sim_addr(), &transmit.payload, now)?;
            }
        }
        let lost_sequence = lost_sequence.ok_or("expected the alert datagram")?;
        assert!(
            !drain_client(&mut client)
                .iter()
                .any(|e| matches!(e, Event::AlertMessage { .. })),
            "the dropped alert must not have reached the client"
        );

        // Past the resend timeout the simulator re-emits the same sequence
        // with the RESENT flag. The timeout tracks the measured round trip
        // (`RELIABLE_TIMEOUT_FACTOR` x the ping average, five seconds on a
        // circuit that has measured nothing yet), so wait past that.
        let later = after(now, 5_100)?;
        sim.handle_timeout(later);
        let mut resent_payload = None;
        while let Some(transmit) = sim.poll_transmit() {
            let parsed = parse_datagram(&transmit.payload)?;
            if matches!(decode(&transmit)?, AnyMessage::AlertMessage(_)) {
                assert_eq!(
                    parsed.sequence, lost_sequence,
                    "a resend reuses the original sequence number"
                );
                assert!(
                    parsed.flags.contains(PacketFlags::RESENT),
                    "a resend carries the RESENT wire flag"
                );
                resent_payload = Some(transmit.payload.clone());
            } else {
                client.handle_datagram(sim_addr(), &transmit.payload, later)?;
            }
        }
        let resent_payload = resent_payload.ok_or("expected the alert to be retransmitted")?;

        // The retransmitted copy dispatches normally; a duplicate delivery of
        // the same datagram is deduplicated.
        client.handle_datagram(sim_addr(), &resent_payload, later)?;
        let delivered = drain_client(&mut client)
            .iter()
            .filter(|e| matches!(e, Event::AlertMessage { .. }))
            .count();
        assert_eq!(
            delivered, 1,
            "the retransmitted alert dispatches exactly once"
        );
        client.handle_datagram(sim_addr(), &resent_payload, later)?;
        let duplicated = drain_client(&mut client)
            .iter()
            .filter(|e| matches!(e, Event::AlertMessage { .. }))
            .count();
        assert_eq!(
            duplicated, 0,
            "a duplicate reliable datagram is not re-surfaced"
        );

        pump(&mut client, &mut sim, later)?;
        Ok(())
    }

    #[test]
    fn inactivity_times_out() -> Result<(), TestError> {
        let now = Instant::now();
        let mut sim = SimSession::new(RegionHandle(REGION_HANDLE), now);
        sim.handle_timeout(after(now, 60_000)?);
        assert!(sim.is_closed());
        assert!(
            drain_server(&mut sim)
                .iter()
                .any(|e| matches!(e, ServerEvent::Disconnected)),
            "expected a Disconnected event"
        );
        Ok(())
    }

    #[test]
    fn caps_event_queue_round_trips() -> Result<(), TestError> {
        let now = Instant::now();
        let (_client, mut sim) = setup(now)?;

        assert!(!sim.has_caps_events());
        sim.enqueue_caps_event(
            "EnableSimulator",
            enable_simulator_to_caps_llsd(REGION_HANDLE, sim_addr(), (256, 256)),
        );
        assert!(sim.has_caps_events());

        let xml = sim
            .take_event_queue_response()
            .ok_or("a response is built")?;
        let parsed = parse_event_queue_response(&xml)?;
        assert_eq!(parsed.events.len(), 1);
        assert_eq!(
            parsed.events.first().map(|event| event.message.as_str()),
            Some("EnableSimulator")
        );
        // The queue is drained after a take.
        assert!(!sim.has_caps_events());
        assert!(sim.take_event_queue_response().is_none());
        Ok(())
    }

    #[test]
    fn sim_eq_batch_1_pathfinding_round_trips() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_client(&mut client);

        // The agent may rebake the navmesh, which is mid-build at version 7.
        sim.enqueue_agent_state_update(true);
        let status = NavMeshStatus {
            region_id: uuid::Uuid::from_u128(0x9a01),
            version: 7,
            status: NavMeshBuildStatus::Building,
        };
        sim.enqueue_nav_mesh_status(&status);

        let events = deliver_caps(&mut client, &mut sim, now)?;
        assert!(
            events.iter().any(|event| matches!(
                event,
                Event::AgentStateUpdate {
                    can_modify_navmesh: true
                }
            )),
            "expected AgentStateUpdate, got {events:?}"
        );
        assert!(
            events
                .iter()
                .any(|event| matches!(event, Event::NavMeshStatus(decoded) if *decoded == status)),
            "expected NavMeshStatus, got {events:?}"
        );
        Ok(())
    }

    #[test]
    fn sim_eq_batch_2_group_and_display_names_round_trip() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_client(&mut client);

        let group = GroupKey::from(uuid::Uuid::from_u128(0x67b2));
        sim.enqueue_agent_drop_group(group);

        let update = DisplayNameUpdate {
            old_display_name: "Old Name".to_owned(),
            name: DisplayName {
                id: AgentKey::from(uuid::Uuid::from_u128(0xa1)),
                username: "james.linden".to_owned(),
                display_name: "James the Great".to_owned(),
                legacy_first_name: "James".to_owned(),
                legacy_last_name: "Linden".to_owned(),
                is_display_name_default: false,
                display_name_expires: String::new(),
                display_name_next_update: String::new(),
                missing: false,
            },
        };
        sim.enqueue_display_name_update(&update);

        let reply = SetDisplayNameReply {
            status: 200,
            reason: "OK".to_owned(),
            new_display_name: Some("James the Great".to_owned()),
            error_tag: None,
        };
        sim.enqueue_set_display_name_reply(&reply);

        let events = deliver_caps(&mut client, &mut sim, now)?;
        assert!(
            events
                .iter()
                .any(|event| matches!(event, Event::AgentDroppedFromGroup { group: dropped } if *dropped == group)),
            "expected AgentDroppedFromGroup, got {events:?}"
        );
        assert!(
            events.iter().any(
                |event| matches!(event, Event::DisplayNameUpdate(decoded) if **decoded == update)
            ),
            "expected DisplayNameUpdate, got {events:?}"
        );
        assert!(
            events.iter().any(
                |event| matches!(event, Event::SetDisplayNameReply(decoded) if **decoded == reply)
            ),
            "expected SetDisplayNameReply, got {events:?}"
        );
        Ok(())
    }

    #[test]
    fn sim_eq_batch_3_region_env_voice_round_trips() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_client(&mut client);

        sim.enqueue_windlight_refresh(true);
        sim.enqueue_sim_console_response("Region restart scheduled.");

        let voice = RequiredVoiceVersion {
            major_version: 1,
            region_name: "Testville".to_owned(),
            voice_server_type: Some("webrtc".to_owned()),
        };
        sim.enqueue_required_voice_version(&voice);

        // A representative subset of the OpenSim per-region overrides: a flag, a
        // real, an int, and a position triple — enough to exercise each encoder
        // arm while leaving the rest `None`.
        let info = OpenRegionInfo {
            allow_minimap: Some(true),
            allow_physical_prims: None,
            draw_distance: Some(256.0),
            force_draw_distance: None,
            terrain_detail_scale: None,
            max_drag_distance: None,
            min_hole_size: None,
            max_hollow_size: None,
            max_inventory_items_transfer: Some(42),
            max_link_count: None,
            max_link_count_phys: None,
            max_position: Some(RegionCoordinates::new(255.0, 255.0, 4096.0)),
            min_position: None,
            max_prim_scale: None,
            max_phys_prim_scale: None,
            min_prim_scale: None,
            offset_of_utc: None,
            offset_of_utc_dst: None,
            render_water: None,
            say_distance: None,
            shout_distance: None,
            whisper_distance: None,
            teen_mode: None,
            show_tags: None,
            enforce_max_build: None,
            max_groups: None,
            allow_parcel_windlight: None,
        };
        sim.enqueue_open_region_info(&info);

        let events = deliver_caps(&mut client, &mut sim, now)?;
        assert!(
            events
                .iter()
                .any(|event| matches!(event, Event::WindLightRefresh { interpolate: true })),
            "expected WindLightRefresh, got {events:?}"
        );
        assert!(
            events.iter().any(|event| matches!(
                event,
                Event::SimConsoleResponse { output } if output == "Region restart scheduled."
            )),
            "expected SimConsoleResponse, got {events:?}"
        );
        assert!(
            events.iter().any(
                |event| matches!(event, Event::RequiredVoiceVersion(decoded) if *decoded == voice)
            ),
            "expected RequiredVoiceVersion, got {events:?}"
        );
        assert!(
            events
                .iter()
                .any(|event| matches!(event, Event::OpenRegionInfo(decoded) if **decoded == info)),
            "expected OpenRegionInfo, got {events:?}"
        );
        Ok(())
    }

    #[test]
    fn unhandled_client_message_is_surfaced() -> Result<(), TestError> {
        let now = Instant::now();
        let (_client, mut sim) = setup(now)?;
        drain_server(&mut sim);

        // A RequestRegionInfo is a client message with no dedicated ServerEvent
        // variant; it must be surfaced verbatim as ClientMessage.
        let request = AnyMessage::RequestRegionInfo(sl_wire::messages::RequestRegionInfo {
            agent_data: sl_wire::messages::RequestRegionInfoAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(1),
                session_id: uuid::Uuid::from_u128(2),
            },
        });
        let datagram = client_datagram(&request, 600, false)?;
        sim.handle_datagram(client_addr(), &datagram, now)?;

        let events = drain_server(&mut sim);
        assert!(
            events.iter().any(|e| matches!(
                e,
                ServerEvent::ClientMessage(message) if matches!(**message, AnyMessage::RequestRegionInfo(_))
            )),
            "expected a ClientMessage(RequestRegionInfo), got {events:?}"
        );
        Ok(())
    }

    #[test]
    fn world_map_requests_surface_server_events() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);

        // Drive each world-map request from the real client paths so the wire
        // encoding matches a viewer; the simulator must decode each into its
        // dedicated ServerEvent rather than the ClientMessage catch-all.
        client.request_map_blocks(1000, 1002, 1000, 1002, now)?;
        client.request_map_by_name("Foo", now)?;
        client.request_map_items(
            MapItemType::Telehub,
            RegionHandle::from_grid(1000, 1000),
            now,
        )?;
        client.request_map_layer(now)?;
        pump(&mut client, &mut sim, now)?;

        let events = drain_server(&mut sim);
        assert!(
            events.iter().any(|e| matches!(
                e,
                ServerEvent::MapBlockRequested {
                    min_x: 1000,
                    max_x: 1002,
                    min_y: 1000,
                    max_y: 1002,
                    ..
                }
            )),
            "expected a MapBlockRequested, got {events:?}"
        );
        assert!(
            events.iter().any(|e| matches!(
                e,
                ServerEvent::MapNameRequested { name, .. } if name == "Foo"
            )),
            "expected a MapNameRequested(Foo), got {events:?}"
        );
        assert!(
            events.iter().any(|e| matches!(
                e,
                ServerEvent::MapItemRequested {
                    item_type: MapItemType::Telehub,
                    region_handle,
                    ..
                } if *region_handle == RegionHandle::from_grid(1000, 1000)
            )),
            "expected a MapItemRequested(Telehub), got {events:?}"
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, ServerEvent::MapLayerRequested { .. })),
            "expected a MapLayerRequested, got {events:?}"
        );
        Ok(())
    }

    #[test]
    fn simulator_map_block_reply_reaches_client() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_client(&mut client);

        // A standard 256 m region and a variable-sized 512×512 region: the latter
        // forces the parallel Size block to be emitted for both entries.
        let regions = vec![
            MapRegionInfo {
                name: region("Standard"),
                grid_coordinates: GridCoordinates::new(1000, 1000),
                region_handle: RegionHandle::from_grid(1000, 1000),
                maturity: Maturity::Mature,
                region_flags: 0x0000_0345,
                size_x: 256,
                size_y: 256,
                agents: 3,
                water_height: 20,
                map_image_id: TextureKey::from(uuid::Uuid::from_u128(0xABCD)),
            },
            MapRegionInfo {
                name: region("Variable"),
                grid_coordinates: GridCoordinates::new(1100, 1200),
                region_handle: RegionHandle::from_grid(1100, 1200),
                maturity: Maturity::Adult,
                region_flags: 0x0000_0007,
                size_x: 512,
                size_y: 512,
                agents: 0,
                water_height: 25,
                map_image_id: TextureKey::from(uuid::Uuid::from_u128(0x1234)),
            },
        ];
        sim.send_map_block_reply(MapRequestFlags(MapRequestFlags::LAYER), &regions, now)?;
        pump(&mut client, &mut sim, now)?;

        let decoded: Vec<MapRegionInfo> = drain_client(&mut client)
            .into_iter()
            .filter_map(|event| match event {
                Event::MapBlock(region) => Some(*region),
                _ => None,
            })
            .collect();
        // The full MapRegionInfo round-trips, including the variable region size.
        assert_eq!(decoded, regions);
        Ok(())
    }

    #[test]
    fn simulator_map_item_reply_reaches_client() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_client(&mut client);

        let items = vec![
            MapItem {
                position: GlobalCoordinates::new(256_000.0, 256_128.0, 0.0),
                id: None,
                extra: 4,
                extra2: 0,
                name: "dots".to_owned(),
            },
            MapItem {
                position: GlobalCoordinates::new(257_000.0, 256_200.0, 0.0),
                id: Some(uuid::Uuid::from_u128(0x55AA)),
                extra: 1024,
                extra2: 250,
                name: "Parcel For Sale".to_owned(),
            },
        ];
        sim.send_map_item_reply(
            MapRequestFlags(MapRequestFlags::LAYER),
            MapItemType::AgentLocations,
            &items,
            now,
        )?;
        pump(&mut client, &mut sim, now)?;

        let reply = drain_client(&mut client)
            .into_iter()
            .find_map(|event| match event {
                Event::MapItems { item_type, items } => Some((item_type, items)),
                _ => None,
            })
            .ok_or("expected a MapItems client event")?;
        assert_eq!(reply.0, MapItemType::AgentLocations);
        assert_eq!(reply.1, items);
        Ok(())
    }

    #[test]
    fn simulator_map_layer_reply_reaches_client() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_client(&mut client);

        let layers = vec![
            MapLayer {
                rect: GridRectangle::new(
                    GridCoordinates::new(0, 0),
                    GridCoordinates::new(9999, 9999),
                ),
                image_id: TextureKey::from(uuid::Uuid::from_u128(0xABCD)),
            },
            MapLayer {
                rect: GridRectangle::new(
                    GridCoordinates::new(1000, 1000),
                    GridCoordinates::new(1100, 1200),
                ),
                image_id: TextureKey::from(uuid::Uuid::from_u128(0x1234)),
            },
        ];
        sim.send_map_layer_reply(MapRequestFlags(MapRequestFlags::LAYER), &layers, now)?;
        pump(&mut client, &mut sim, now)?;

        let decoded: Vec<MapLayer> = drain_client(&mut client)
            .into_iter()
            .find_map(|event| match event {
                Event::MapLayers { layers } => Some(layers),
                _ => None,
            })
            .ok_or("expected a MapLayers client event")?;
        assert_eq!(decoded, layers);
        Ok(())
    }

    #[test]
    fn client_abuse_report_reaches_server() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);

        let report = AbuseReport {
            report_type: AbuseReportType::Complaint,
            category: 66,
            position: sl_types::lsl::Vector {
                x: 128.0,
                y: 64.0,
                z: 22.0,
            },
            check_flags: 0,
            screenshot_id: uuid::Uuid::nil(),
            object_id: ObjectKey::from(uuid::Uuid::from_u128(0x22)),
            abuser_id: uuid::Uuid::from_u128(0x33),
            abuse_region_name: region("TestRegion"),
            abuse_region_id: uuid::Uuid::nil(),
            summary: "Griefing".to_owned(),
            details: "Detail".to_owned(),
            version_string: "7.1 Lnx".to_owned(),
        };
        client.send_abuse_report(&report, now)?;
        pump(&mut client, &mut sim, now)?;

        let received = drain_server(&mut sim)
            .into_iter()
            .find_map(|event| match event {
                ServerEvent::AbuseReportReceived(report) => Some(*report),
                _ => None,
            })
            .ok_or("expected an AbuseReportReceived server event")?;
        assert_eq!(received, report);
        Ok(())
    }

    #[test]
    fn client_postcard_reaches_server() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);

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
        client.send_postcard(&postcard, now)?;
        pump(&mut client, &mut sim, now)?;

        let received = drain_server(&mut sim)
            .into_iter()
            .find_map(|event| match event {
                ServerEvent::PostcardReceived(postcard) => Some(*postcard),
                _ => None,
            })
            .ok_or("expected a PostcardReceived server event")?;
        assert_eq!(received, postcard);
        Ok(())
    }

    #[test]
    fn send_region_handshake_encodes_the_identity() -> Result<(), TestError> {
        let now = Instant::now();
        let (_client, mut sim) = setup(now)?;

        let identity = RegionIdentity {
            sim_name: region("Server Region"),
            region_id: uuid::Uuid::from_u128(0xBEEF),
            // Grid coordinates / handle are not wire fields of the handshake.
            region_handle: RegionHandle(0),
            grid_coordinates: GridCoordinates::new(0, 0),
            region_flags: 0x40,
            region_flags_extended: 0x1_0000_0040,
            region_protocols: 0x5,
            maturity: Maturity::Mature,
            product: ProductType::Homestead,
            product_sku: String::new(),
            product_name: "Homestead".to_owned(),
            cpu_class_id: 4,
            cpu_ratio: 8,
            sim_owner: uuid::Uuid::from_u128(0x0411),
            is_estate_manager: true,
            water_height: 20.0,
            billable_factor: 1.0,
            terrain: RegionTerrainComposition {
                detail_textures: [
                    uuid::Uuid::from_u128(0xD0),
                    uuid::Uuid::from_u128(0xD1),
                    uuid::Uuid::from_u128(0xD2),
                    uuid::Uuid::from_u128(0xD3),
                ],
                start_heights: [1.0, 2.0, 3.0, 4.0],
                height_ranges: [10.0, 20.0, 30.0, 40.0],
            },
        };
        sim.send_region_handshake(&identity, now)?;

        let mut handshake = None;
        while let Some(transmit) = sim.poll_transmit() {
            if let AnyMessage::RegionHandshake(decoded) = decode(&transmit)? {
                handshake = Some(decoded);
            }
        }
        let handshake = handshake.ok_or("a RegionHandshake datagram was sent")?;
        assert_eq!(
            handshake.region_info2.region_id,
            uuid::Uuid::from_u128(0xBEEF)
        );
        assert_eq!(handshake.region_info3.cpu_class_id, 4);
        assert_eq!(handshake.region_info3.cpu_ratio, 8);
        assert_eq!(handshake.region_info.region_flags, 0x40);
        assert_eq!(
            handshake.region_info.sim_access,
            Maturity::Mature.to_sim_access()
        );
        assert_eq!(
            String::from_utf8_lossy(&handshake.region_info.sim_name).trim_end_matches('\0'),
            "Server Region"
        );
        let info4 = handshake
            .region_info4
            .first()
            .ok_or("a RegionInfo4 block")?;
        assert_eq!(info4.region_flags_extended, 0x1_0000_0040);
        assert_eq!(info4.region_protocols, 0x5);
        // The terrain detail textures and per-corner elevation bands survive the
        // encode, in the `00, 01, 10, 11` corner order.
        assert_eq!(
            handshake.region_info.terrain_detail0,
            uuid::Uuid::from_u128(0xD0)
        );
        assert_eq!(
            handshake.region_info.terrain_detail3,
            uuid::Uuid::from_u128(0xD3)
        );
        assert_eq!(
            handshake.region_info.terrain_start_height00.to_bits(),
            1.0_f32.to_bits()
        );
        assert_eq!(
            handshake.region_info.terrain_height_range10.to_bits(),
            30.0_f32.to_bits()
        );
        Ok(())
    }

    #[test]
    fn uuid_name_request_round_trips_through_the_simulator() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);
        drain_client(&mut client);

        let alice = uuid::Uuid::from_u128(0xA11CE);
        let club = uuid::Uuid::from_u128(0xC1B);
        client.request_avatar_names(&[AgentKey::from(alice)], now)?;
        client.request_group_names(&[GroupKey::from(club)], now)?;
        pump(&mut client, &mut sim, now)?;

        // The simulator surfaces both lookups for the application to answer.
        let server_events = drain_server(&mut sim);
        assert!(
            server_events.iter().any(
                |event| matches!(event, ServerEvent::AvatarNamesRequested(ids) if ids == &[alice])
            ),
            "expected AvatarNamesRequested, got {server_events:?}"
        );
        assert!(
            server_events.iter().any(
                |event| matches!(event, ServerEvent::GroupNamesRequested(ids) if ids == &[club])
            ),
            "expected GroupNamesRequested, got {server_events:?}"
        );

        // The simulator answers; the client decodes the names.
        sim.send_avatar_names(
            &[AvatarName {
                id: alice.into(),
                first_name: "Alice".to_owned(),
                last_name: "Liddell".to_owned(),
            }],
            now,
        )?;
        sim.send_group_names(
            &[GroupName {
                id: club.into(),
                name: "The Club".to_owned(),
            }],
            now,
        )?;
        pump(&mut client, &mut sim, now)?;

        let client_events = drain_client(&mut client);
        let avatar = client_events
            .iter()
            .find_map(|event| match event {
                Event::AvatarNames(names) => names.iter().find(|name| name.id.uuid() == alice),
                _ => None,
            })
            .ok_or("expected the avatar name on the client")?;
        assert_eq!(avatar.legacy_name(), "Alice Liddell");
        let group = client_events
            .iter()
            .find_map(|event| match event {
                Event::GroupNames(names) => names.iter().find(|name| name.id.uuid() == club),
                _ => None,
            })
            .ok_or("expected the group name on the client")?;
        assert_eq!(group.name, "The Club");
        Ok(())
    }

    /// The secure session id of the [`success`] login fixture, which the
    /// simulator needs to derive legacy-upload asset ids the same way the
    /// client predicts them.
    fn secure_session() -> uuid::Uuid {
        uuid::Uuid::from_u128(3)
    }

    /// A minimal wearable inventory item for the legacy asset-upload tests.
    fn wearable_item() -> InventoryItem {
        InventoryItem {
            item_id: InventoryKey::from(uuid::Uuid::from_u128(0x11)),
            folder_id: InventoryFolderKey::from(uuid::Uuid::from_u128(0x12)),
            name: "Tattered Shirt".to_owned(),
            description: String::new(),
            asset_id: uuid::Uuid::nil(),
            item_type: 5,
            inv_type: 18,
            flags: 0,
            sale_type: 0,
            sale_price: None,
            creation_date: 1_700_000_000,
            owner: OwnerKey::Agent(AgentKey::from(uuid::Uuid::from_u128(1))),
            last_owner_id: uuid::Uuid::nil(),
            creator_id: AgentKey::from(uuid::Uuid::from_u128(1)),
            group: None,
            permissions: Permissions5 {
                base: Permissions::ALL,
                owner: Permissions::ALL,
                group: Permissions::NONE,
                everyone: Permissions::NONE,
                next_owner: Permissions::ALL,
            },
        }
    }

    /// A task-inventory item fixture for the serving round-trip.
    fn task_script_item(task: ObjectKey) -> TaskInventoryItem {
        let creator = AgentKey::from(uuid::Uuid::from_u128(0x44));
        TaskInventoryItem {
            item_id: InventoryKey::from(uuid::Uuid::from_u128(0x33)),
            parent_task: task,
            permissions: Permissions5 {
                base: Permissions::from_bits(0x7fff_ffff),
                owner: Permissions::from_bits(0x7fff_ffff),
                group: Permissions::from_bits(0),
                everyone: Permissions::from_bits(0),
                next_owner: Permissions::from_bits(0x0008_e000),
            },
            creator_id: creator,
            last_owner_id: creator,
            owner: OwnerKey::Agent(creator),
            group: None,
            group_owned: false,
            asset_id: Some(AssetKey::from(uuid::Uuid::from_u128(0x55))),
            asset_type: AssetType::ScriptText,
            inv_type: InventoryType::Script,
            flags: 0,
            sale_type: SaleType::NotForSale,
            sale_price: LindenAmount(0),
            name: "Hello World Script".to_owned(),
            description: String::new(),
            creation_date: 1_700_000_000,
        }
    }

    /// A small legacy asset upload round-trips inline: the client's
    /// `save_inventory_asset` carries the bytes in the `AssetUploadRequest`
    /// itself, the simulator derives the stored asset id as
    /// `combine(transaction, secure_session)` exactly as the client predicts
    /// it, replies with an `AssetUploadComplete`, and both sides surface their
    /// completion events.
    #[test]
    fn inline_asset_upload_round_trips() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);
        sim.set_secure_session_id(secure_session());

        let txn = TransactionId::new(uuid::Uuid::from_u128(0xABCD));
        let data = vec![7_u8; 100];
        client.save_inventory_asset(
            &wearable_item(),
            AssetType::Clothing,
            data.clone(),
            txn,
            now,
        )?;
        pump(&mut client, &mut sim, now)?;

        let expected_asset = sl_wire::combine_uuids(txn.get(), secure_session());
        let server_events = drain_server(&mut sim);
        assert!(
            server_events.iter().any(|e| matches!(
                e,
                ServerEvent::AssetUploadRequested {
                    transaction_id,
                    asset_type: AssetType::Clothing,
                    inline: true,
                    ..
                } if *transaction_id == txn
            )),
            "expected an inline AssetUploadRequested, got {server_events:?}"
        );
        assert!(
            server_events.iter().any(|e| matches!(
                e,
                ServerEvent::AssetUploaded {
                    asset_id,
                    asset_type: AssetType::Clothing,
                    transaction_id,
                    data: got,
                } if asset_id.uuid() == expected_asset && *transaction_id == txn && *got == data
            )),
            "expected the uploaded asset bytes, got {server_events:?}"
        );
        let client_events = drain_client(&mut client);
        assert!(
            client_events.iter().any(|e| matches!(
                e,
                Event::InventoryAssetSaved { asset_id, success: true } if *asset_id == expected_asset
            )),
            "expected InventoryAssetSaved, got {client_events:?}"
        );
        Ok(())
    }

    /// An oversized legacy asset upload is pulled over `Xfer`: the client sends
    /// an empty `AssetData`, the simulator issues a `RequestXfer` keyed by the
    /// predicted `VFileID`, the client streams the bytes (seq-0 size prefix,
    /// high-bit end marker, one packet per confirmation), and the reassembled
    /// asset is byte-identical before the `AssetUploadComplete` closes the
    /// client side.
    #[test]
    fn oversize_asset_upload_pulls_via_xfer() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);
        sim.set_secure_session_id(secure_session());

        let txn = TransactionId::new(uuid::Uuid::from_u128(0xBEEF));
        // Several Xfer chunks (chunk size is 1000 bytes).
        let data: Vec<u8> = (0..3500_u32)
            .map(|i| u8::try_from(i % 251).unwrap_or(0))
            .collect();
        client.save_inventory_asset(
            &wearable_item(),
            AssetType::Bodypart,
            data.clone(),
            txn,
            now,
        )?;
        pump(&mut client, &mut sim, now)?;

        let expected_asset = sl_wire::combine_uuids(txn.get(), secure_session());
        let server_events = drain_server(&mut sim);
        assert!(
            server_events
                .iter()
                .any(|e| matches!(e, ServerEvent::AssetUploadRequested { inline: false, .. })),
            "expected an Xfer-pull AssetUploadRequested, got {server_events:?}"
        );
        assert!(
            server_events.iter().any(|e| matches!(
                e,
                ServerEvent::AssetUploaded {
                    asset_id,
                    asset_type: AssetType::Bodypart,
                    transaction_id,
                    data: got,
                } if asset_id.uuid() == expected_asset && *transaction_id == txn && *got == data
            )),
            "expected the reassembled asset bytes, got {server_events:?}"
        );
        let client_events = drain_client(&mut client);
        assert!(
            client_events.iter().any(|e| matches!(
                e,
                Event::InventoryAssetSaved { asset_id, success: true } if *asset_id == expected_asset
            )),
            "expected InventoryAssetSaved, got {client_events:?}"
        );
        Ok(())
    }

    /// A registered file serves over `Xfer`: the client's `request_xfer`
    /// downloads it chunk-by-chunk (paced by its own confirmations) and
    /// receives the exact registered bytes; the simulator surfaces the request
    /// and the completed send.
    #[test]
    fn xfer_file_serving_round_trips() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);

        // More than two chunks, so the pacing loop is exercised.
        let bytes: Vec<u8> = (0..2500_u32)
            .map(|i| u8::try_from(i % 199).unwrap_or(0))
            .collect();
        sim.register_xfer_file("motd.txt", bytes.clone());
        let xfer_id = client.request_xfer("motd.txt", now)?;
        pump(&mut client, &mut sim, now)?;

        let client_events = drain_client(&mut client);
        assert!(
            client_events.iter().any(|e| matches!(
                e,
                Event::XferDownloaded { xfer_id: got, data } if *got == xfer_id && *data == bytes
            )),
            "expected the downloaded bytes, got {client_events:?}"
        );
        let server_events = drain_server(&mut sim);
        assert!(
            server_events.iter().any(|e| matches!(
                e,
                ServerEvent::XferRequested { xfer_id: got, filename, served: true }
                    if *got == xfer_id && filename == "motd.txt"
            )),
            "expected XferRequested, got {server_events:?}"
        );
        assert!(
            server_events.iter().any(|e| matches!(
                e,
                ServerEvent::XferServed { xfer_id: got, filename, byte_count }
                    if *got == xfer_id && filename == "motd.txt" && *byte_count == bytes.len()
            )),
            "expected XferServed, got {server_events:?}"
        );
        Ok(())
    }

    /// The estate terrain RAW download round-trips: the client's
    /// `request_region_terrain_download` surfaces
    /// [`ServerEvent::TerrainDownloadRequested`]; the driver's
    /// `send_initiate_download` registers the heightmap and offers it, the
    /// client follows the `InitiateDownload` automatically over `Xfer` and
    /// surfaces the exact bytes as [`Event::ServerFileDownloaded`] tagged with
    /// the viewer filename it asked for.
    #[test]
    fn terrain_download_round_trips() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);
        drain_client(&mut client);

        client.request_region_terrain_download("terrain.raw", now)?;
        pump(&mut client, &mut sim, now)?;
        let server_events = drain_server(&mut sim);
        assert!(
            server_events.iter().any(|e| *e
                == ServerEvent::TerrainDownloadRequested {
                    viewer_filename: "terrain.raw".to_owned(),
                }),
            "expected TerrainDownloadRequested, got {server_events:?}"
        );

        // More than two chunks, so the pacing loop is exercised.
        let heightmap: Vec<u8> = (0..2500_u32)
            .map(|i| u8::try_from(i % 211).unwrap_or(0))
            .collect();
        sim.send_initiate_download(
            "0badc0de-terrain.raw",
            "terrain.raw",
            heightmap.clone(),
            now,
        )?;
        pump(&mut client, &mut sim, now)?;

        let client_events = drain_client(&mut client);
        assert!(
            client_events.iter().any(|e| matches!(
                e,
                Event::ServerFileDownloaded { viewer_filename, data }
                    if viewer_filename == "terrain.raw" && *data == heightmap
            )),
            "expected ServerFileDownloaded, got {client_events:?}"
        );
        let server_events = drain_server(&mut sim);
        assert!(
            server_events.iter().any(|e| matches!(
                e,
                ServerEvent::XferRequested { filename, served: true, .. }
                    if filename == "0badc0de-terrain.raw"
            )),
            "expected the served XferRequested, got {server_events:?}"
        );
        assert!(
            server_events.iter().any(|e| matches!(
                e,
                ServerEvent::XferServed { filename, byte_count, .. }
                    if filename == "0badc0de-terrain.raw" && *byte_count == heightmap.len()
            )),
            "expected XferServed, got {server_events:?}"
        );
        Ok(())
    }

    /// The estate terrain RAW upload round-trips: the client's
    /// `request_region_terrain_upload` surfaces
    /// [`ServerEvent::TerrainUploadRequested`]; the driver's
    /// `request_xfer_upload` pulls the named file, the client streams it
    /// (paced by the sim's confirmations) and reports [`Event::XferUploaded`],
    /// and the sim surfaces the exact bytes as [`ServerEvent::XferReceived`].
    /// A client abort mid-pull drops the receive.
    #[test]
    fn terrain_upload_round_trips() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);
        drain_client(&mut client);

        let heightmap: Vec<u8> = (0..2500_u32)
            .map(|i| u8::try_from(i % 223).unwrap_or(0))
            .collect();
        client.request_region_terrain_upload("terrain.raw", heightmap.clone(), now)?;
        pump(&mut client, &mut sim, now)?;
        let server_events = drain_server(&mut sim);
        assert!(
            server_events.iter().any(|e| *e
                == ServerEvent::TerrainUploadRequested {
                    viewer_filename: "terrain.raw".to_owned(),
                }),
            "expected TerrainUploadRequested, got {server_events:?}"
        );

        let xfer_id = sim.request_xfer_upload("terrain.raw", now)?;
        pump(&mut client, &mut sim, now)?;
        let server_events = drain_server(&mut sim);
        assert!(
            server_events.iter().any(|e| *e
                == ServerEvent::XferReceived {
                    xfer_id,
                    filename: "terrain.raw".to_owned(),
                    data: heightmap.clone(),
                }),
            "expected XferReceived, got {server_events:?}"
        );
        let client_events = drain_client(&mut client);
        assert!(
            client_events.iter().any(|e| matches!(
                e,
                Event::XferUploaded { xfer_id: got, viewer_filename, byte_count }
                    if *got == xfer_id && viewer_filename == "terrain.raw"
                        && *byte_count == heightmap.len()
            )),
            "expected XferUploaded, got {client_events:?}"
        );
        assert!(
            matches!(
                sim.abort_xfer(xfer_id, 0, now),
                Err(sl_proto::Error::UnknownXfer)
            ),
            "a completed pull is no longer in flight"
        );

        // Abort mid-pull: deliver only the pull + first packet, then the
        // client aborts; the sim drops the receive and never completes it.
        client.request_region_terrain_upload("again.raw", vec![7_u8; 2500], now)?;
        pump(&mut client, &mut sim, now)?;
        drain_server(&mut sim);
        let second = sim.request_xfer_upload("again.raw", now)?;
        if let Some(transmit) = sim.poll_transmit() {
            client.handle_datagram(sim_addr(), &transmit.payload, now)?;
        }
        while sim.poll_transmit().is_some() {}
        let abort = AnyMessage::AbortXfer(AbortXfer {
            xfer_id: AbortXferXferIDBlock {
                id: second.get(),
                result: -3,
            },
        });
        sim.handle_datagram(client_addr(), &client_datagram(&abort, 9200, false)?, now)?;
        let server_events = drain_server(&mut sim);
        assert!(
            server_events.iter().any(|e| matches!(
                e,
                ServerEvent::XferAborted { xfer_id: got, result: -3 } if *got == second
            )),
            "expected XferAborted, got {server_events:?}"
        );
        pump(&mut client, &mut sim, now)?;
        assert!(
            !drain_server(&mut sim)
                .iter()
                .any(|e| matches!(e, ServerEvent::XferReceived { .. })),
            "an aborted pull must not complete"
        );
        Ok(())
    }

    /// An `EstateOwnerMessage` without a typed event of its own surfaces as
    /// [`ServerEvent::EstateOwnerRequest`] with its method and parameters
    /// (never the `ClientMessage` catch-all), and the `terrain` `bake`
    /// sub-command gets its typed event.
    #[test]
    fn untyped_estate_owner_message_is_surfaced() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);

        let target = AgentKey::from(uuid::Uuid::from_u128(0xE57A));
        client.kick_estate_user(target, now)?;
        pump(&mut client, &mut sim, now)?;
        let server_events = drain_server(&mut sim);
        assert!(
            server_events.iter().any(|e| matches!(
                e,
                ServerEvent::EstateOwnerRequest { method, params, .. }
                    if method == "kickestate" && *params == vec![target.uuid().to_string()]
            )),
            "expected EstateOwnerRequest, got {server_events:?}"
        );
        assert!(
            !server_events
                .iter()
                .any(|e| matches!(e, ServerEvent::ClientMessage(_))),
            "an estate command must not fall through to ClientMessage"
        );

        // No client helper sends `terrain`/`bake` (the viewer's revert
        // baseline), so inject it as a wire datagram.
        let bake = AnyMessage::EstateOwnerMessage(EstateOwnerMessage {
            agent_data: EstateOwnerMessageAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(1),
                // The simulator refuses a message that asserts any session but
                // the one the circuit was opened with (the `success` fixture's).
                session_id: uuid::Uuid::from_u128(2),
                transaction_id: uuid::Uuid::nil(),
            },
            method_data: EstateOwnerMessageMethodDataBlock {
                method: b"terrain\0".to_vec(),
                invoice: uuid::Uuid::nil(),
            },
            param_list: vec![EstateOwnerMessageParamListBlock {
                parameter: b"bake\0".to_vec(),
            }],
        });
        sim.handle_datagram(client_addr(), &client_datagram(&bake, 9300, false)?, now)?;
        let server_events = drain_server(&mut sim);
        assert!(
            server_events.contains(&ServerEvent::TerrainBakeRequested),
            "expected TerrainBakeRequested, got {server_events:?}"
        );
        Ok(())
    }

    /// A `RequestXfer` for a name that was never registered is refused with an
    /// `AbortXfer`, so the requesting client is not left waiting.
    #[test]
    fn xfer_request_for_unknown_file_aborts() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);

        let xfer_id = client.request_xfer("nope.txt", now)?;
        pump(&mut client, &mut sim, now)?;

        let client_events = drain_client(&mut client);
        assert!(
            client_events.iter().any(|e| matches!(
                e,
                Event::XferAborted { xfer_id: got, result: -1 } if *got == xfer_id
            )),
            "expected XferAborted, got {client_events:?}"
        );
        let server_events = drain_server(&mut sim);
        assert!(
            server_events
                .iter()
                .any(|e| matches!(e, ServerEvent::XferRequested { served: false, .. })),
            "expected a refused XferRequested, got {server_events:?}"
        );
        Ok(())
    }

    /// Both abort directions: a simulator-side `abort_xfer` mid-send reaches
    /// the client as [`Event::XferAborted`] (and the download never
    /// completes), and a client-sent `AbortXfer` drops the simulator's send
    /// and surfaces [`ServerEvent::XferAborted`].
    #[test]
    fn abort_xfer_paths() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);

        // Simulator-side abort: deliver only the request, then abort before
        // the client sees the remaining packets.
        sim.register_xfer_file("big.bin", vec![9_u8; 2500]);
        let xfer_id = client.request_xfer("big.bin", now)?;
        while let Some(transmit) = client.poll_transmit() {
            sim.handle_datagram(client_addr(), &transmit.payload, now)?;
        }
        sim.abort_xfer(xfer_id, 3, now)?;
        pump(&mut client, &mut sim, now)?;
        let client_events = drain_client(&mut client);
        assert!(
            client_events.iter().any(|e| matches!(
                e,
                Event::XferAborted { xfer_id: got, result: 3 } if *got == xfer_id
            )),
            "expected the sim abort on the client, got {client_events:?}"
        );
        assert!(
            !client_events
                .iter()
                .any(|e| matches!(e, Event::XferDownloaded { .. })),
            "the aborted download must not complete, got {client_events:?}"
        );
        drain_server(&mut sim);

        // Client-side abort (no client helper API sends `AbortXfer`, so it is
        // injected as a wire datagram): the sim drops its in-flight send and
        // surfaces the abort.
        sim.register_xfer_file("cancelme.bin", vec![4_u8; 2500]);
        let second = client.request_xfer("cancelme.bin", now)?;
        while let Some(transmit) = client.poll_transmit() {
            sim.handle_datagram(client_addr(), &transmit.payload, now)?;
        }
        let abort = AnyMessage::AbortXfer(AbortXfer {
            xfer_id: AbortXferXferIDBlock {
                id: second.get(),
                result: -7,
            },
        });
        sim.handle_datagram(client_addr(), &client_datagram(&abort, 9000, false)?, now)?;
        let server_events = drain_server(&mut sim);
        assert!(
            server_events.iter().any(|e| matches!(
                e,
                ServerEvent::XferAborted { xfer_id: got, result: -7 } if *got == second
            )),
            "expected the client abort on the sim, got {server_events:?}"
        );

        // Aborting an id that is no longer (or never was) in flight is an
        // observable error and sends nothing — a driver typo must not pass
        // silently.
        while sim.poll_transmit().is_some() {}
        assert!(
            matches!(
                sim.abort_xfer(second, 0, now),
                Err(sl_proto::Error::UnknownXfer)
            ),
            "aborting an already-aborted xfer is UnknownXfer"
        );
        assert!(
            matches!(
                sim.abort_xfer(XferId::new(0xFFFF), 0, now),
                Err(sl_proto::Error::UnknownXfer)
            ),
            "aborting a never-started xfer is UnknownXfer"
        );
        assert!(
            sim.poll_transmit().is_none(),
            "an UnknownXfer abort sends nothing"
        );
        Ok(())
    }

    /// A task-inventory item's asset round-trips over the legacy UDP Transfer
    /// path: the client's `fetch_task_item_asset` sends a `TransferRequest`
    /// whose params the simulator decodes field-for-field, the driver answers
    /// with `send_transfer_asset`, and the multi-packet stream reassembles
    /// byte-identically on the client (a single-packet body works too).
    #[test]
    fn task_item_asset_transfer_round_trips() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);

        let task = ObjectKey::from(uuid::Uuid::from_u128(0xAAAA));
        let item = InventoryKey::from(uuid::Uuid::from_u128(0xBBBB));
        let asset = AssetKey::from(uuid::Uuid::from_u128(0xCCCC));
        let transfer_id =
            client.fetch_task_item_asset(task, item, asset, AssetType::ScriptText, now)?;
        pump(&mut client, &mut sim, now)?;

        let server_events = drain_server(&mut sim);
        let params = server_events
            .iter()
            .find_map(|e| match e {
                ServerEvent::TransferRequested {
                    transfer_id: got,
                    source: TransferRequestSource::TaskInventoryItem(params),
                    ..
                } if *got == transfer_id => Some(*params),
                _ => None,
            })
            .ok_or("expected a task-item TransferRequested")?;
        assert_eq!(params.agent_id, uuid::Uuid::from_u128(1));
        assert_eq!(params.session_id, uuid::Uuid::from_u128(2));
        assert_eq!(params.task_id, task.uuid());
        assert_eq!(params.item_id, item.uuid());
        assert_eq!(params.asset_id, asset.uuid());
        assert_eq!(params.asset_type, AssetType::ScriptText.to_code());

        // Multi-packet body (chunk size is 1000 bytes).
        let body: Vec<u8> = (0..2500_u32)
            .map(|i| u8::try_from(i % 241).unwrap_or(0))
            .collect();
        sim.send_transfer_asset(transfer_id, &body, now)?;
        pump(&mut client, &mut sim, now)?;
        let client_events = drain_client(&mut client);
        assert!(
            client_events.iter().any(|e| matches!(
                e,
                Event::TaskItemAssetReceived {
                    transfer_id: got,
                    task: got_task,
                    item: got_item,
                    asset_type: AssetType::ScriptText,
                    data,
                } if *got == transfer_id && *got_task == task && *got_item == item && *data == body
            )),
            "expected the assembled task-item asset, got {client_events:?}"
        );

        // A body that fits one packet round-trips too.
        let second = client.fetch_task_item_asset(task, item, asset, AssetType::Notecard, now)?;
        pump(&mut client, &mut sim, now)?;
        drain_server(&mut sim);
        sim.send_transfer_asset(second, b"tiny", now)?;
        pump(&mut client, &mut sim, now)?;
        let client_events = drain_client(&mut client);
        assert!(
            client_events.iter().any(|e| matches!(
                e,
                Event::TaskItemAssetReceived { transfer_id: got, data, .. }
                    if *got == second && data == b"tiny"
            )),
            "expected the single-packet asset, got {client_events:?}"
        );
        Ok(())
    }

    /// The estate covenant notecard round-trips over the legacy UDP Transfer
    /// path: `fetch_estate_covenant_asset` sends the `SimEstate` request
    /// (estate asset type `covenant`), and the served bytes surface as
    /// [`Event::EstateCovenantAssetReceived`].
    #[test]
    fn estate_covenant_transfer_round_trips() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);

        let transfer_id = client.fetch_estate_covenant_asset(now)?;
        pump(&mut client, &mut sim, now)?;
        let server_events = drain_server(&mut sim);
        let params = server_events
            .iter()
            .find_map(|e| match e {
                ServerEvent::TransferRequested {
                    transfer_id: got,
                    source: TransferRequestSource::Estate(params),
                    ..
                } if *got == transfer_id => Some(*params),
                _ => None,
            })
            .ok_or("expected an estate TransferRequested")?;
        assert_eq!(params.agent_id, uuid::Uuid::from_u128(1));
        assert_eq!(params.estate_asset_type, sl_wire::ESTATE_ASSET_COVENANT);

        sim.send_transfer_asset(transfer_id, b"Covenant text.", now)?;
        pump(&mut client, &mut sim, now)?;
        let client_events = drain_client(&mut client);
        assert!(
            client_events.iter().any(|e| matches!(
                e,
                Event::EstateCovenantAssetReceived { transfer_id: got, data }
                    if *got == transfer_id && data == b"Covenant text."
            )),
            "expected the covenant asset, got {client_events:?}"
        );
        Ok(())
    }

    /// Transfer failure and abort paths: a `send_transfer_fail` refusal
    /// surfaces as [`Event::TransferFailed`]; a client `abort_transfer`
    /// surfaces as [`ServerEvent::TransferAborted`] and invalidates the
    /// pending answer; and a legacy plain-asset (source 2) request is
    /// auto-refused without surfacing a server event.
    #[test]
    fn transfer_fail_and_abort_paths() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);
        let task = ObjectKey::from(uuid::Uuid::from_u128(0xAAAA));
        let item = InventoryKey::from(uuid::Uuid::from_u128(0xBBBB));
        let asset = AssetKey::from(uuid::Uuid::from_u128(0xCCCC));

        // Refusal: the client learns the asset is missing.
        let refused =
            client.fetch_task_item_asset(task, item, asset, AssetType::ScriptText, now)?;
        pump(&mut client, &mut sim, now)?;
        drain_server(&mut sim);
        sim.send_transfer_fail(refused, TransferStatus::UnknownSource, now)?;
        pump(&mut client, &mut sim, now)?;
        let client_events = drain_client(&mut client);
        assert!(
            client_events.iter().any(|e| matches!(
                e,
                Event::TransferFailed {
                    transfer_id: got,
                    status: TransferStatus::UnknownSource,
                } if *got == refused
            )),
            "expected TransferFailed, got {client_events:?}"
        );

        // Client-side abort: the sim surfaces it and the pending answer dies.
        let aborted =
            client.fetch_task_item_asset(task, item, asset, AssetType::ScriptText, now)?;
        pump(&mut client, &mut sim, now)?;
        drain_server(&mut sim);
        client.abort_transfer(aborted, now)?;
        pump(&mut client, &mut sim, now)?;
        let server_events = drain_server(&mut sim);
        assert!(
            server_events.iter().any(|e| matches!(
                e,
                ServerEvent::TransferAborted { transfer_id: got } if *got == aborted
            )),
            "expected TransferAborted, got {server_events:?}"
        );
        assert!(matches!(
            sim.send_transfer_asset(aborted, b"late", now),
            Err(sl_proto::Error::UnknownTransfer)
        ));

        // Legacy plain-asset source: auto-refused with an unknown-source
        // `TransferInfo` (never a `TransferRequested`), but surfaced as the
        // typed refusal so a driver can log what the client tried.
        let legacy_id = uuid::Uuid::from_u128(0xDEAD);
        let legacy_params = sl_wire::TransferSourceParamsAsset {
            asset_id: asset.uuid(),
            asset_type: AssetType::ScriptText.to_code(),
        };
        let legacy = AnyMessage::TransferRequest(TransferRequest {
            transfer_info: TransferRequestTransferInfoBlock {
                transfer_id: legacy_id,
                channel_type: sl_wire::TRANSFER_CHANNEL_ASSET,
                source_type: sl_wire::TRANSFER_SOURCE_ASSET,
                priority: 100.0,
                params: legacy_params.encode(),
            },
        });
        sim.handle_datagram(client_addr(), &client_datagram(&legacy, 9100, false)?, now)?;
        let server_events = drain_server(&mut sim);
        assert!(
            !server_events
                .iter()
                .any(|e| matches!(e, ServerEvent::TransferRequested { .. })),
            "a legacy plain-asset request must not surface, got {server_events:?}"
        );
        assert!(
            server_events.iter().any(|e| *e
                == ServerEvent::LegacyAssetTransferRefused {
                    transfer_id: TransferId::new(legacy_id),
                    params: Some(legacy_params),
                }),
            "expected LegacyAssetTransferRefused, got {server_events:?}"
        );
        let refusal = sim
            .poll_transmit()
            .ok_or("expected the unknown-source TransferInfo")?;
        assert!(
            matches!(
                decode(&refusal)?,
                AnyMessage::TransferInfo(info)
                    if info.transfer_info.transfer_id == legacy_id
                        && info.transfer_info.status == TransferStatus::UnknownSource.to_code()
            ),
            "expected an UnknownSource TransferInfo"
        );

        // A garbage source type is refused silently.
        let garbage = AnyMessage::TransferRequest(TransferRequest {
            transfer_info: TransferRequestTransferInfoBlock {
                transfer_id: uuid::Uuid::from_u128(0xBEEF),
                channel_type: sl_wire::TRANSFER_CHANNEL_ASSET,
                source_type: 99,
                priority: 100.0,
                params: Vec::new(),
            },
        });
        sim.handle_datagram(client_addr(), &client_datagram(&garbage, 9101, false)?, now)?;
        assert_eq!(
            drain_server(&mut sim),
            Vec::new(),
            "garbage source surfaces nothing"
        );
        Ok(())
    }

    /// The object-sit flow round-trips: the client's `sit_on` surfaces
    /// [`ServerEvent::SitRequested`], the driver's `send_avatar_sit_response`
    /// seats the client — whose completing `AgentSit` surfaces
    /// [`ServerEvent::SitConfirmed`] and marks the sim-side machine seated —
    /// and standing up via the transient `STAND_UP` control flag surfaces
    /// [`ServerEvent::StoodUp`] and resets both mirrors.
    #[test]
    fn object_sit_flow_round_trips() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);
        drain_client(&mut client);

        let target = ObjectKey::from(uuid::Uuid::from_u128(0x5EA7));
        let offset = sl_types::lsl::Vector {
            x: 0.5,
            y: -0.25,
            z: 1.0,
        };
        client.sit_on(target, offset.clone(), now)?;
        pump(&mut client, &mut sim, now)?;
        let server_events = drain_server(&mut sim);
        assert!(
            server_events.iter().any(|e| matches!(
                e,
                ServerEvent::SitRequested { target: t, offset: o }
                    if *t == target && *o == offset
            )),
            "expected SitRequested, got {server_events:?}"
        );
        assert_eq!(sim.seated_on(), None);

        let transform = SitTransform {
            autopilot: true,
            sit_position: sl_types::lsl::Vector {
                x: 0.1,
                y: 0.2,
                z: 0.6,
            },
            sit_rotation: sl_types::lsl::Rotation {
                x: 0.0,
                y: 0.0,
                z: 1.0,
                s: 0.0,
            },
            camera_eye_offset: sl_types::lsl::Vector {
                x: -3.0,
                y: 0.0,
                z: 1.5,
            },
            camera_at_offset: sl_types::lsl::Vector {
                x: 0.0,
                y: 0.0,
                z: 0.5,
            },
            force_mouselook: true,
        };
        sim.send_avatar_sit_response(target, &transform, now)?;
        // One pump both delivers the response and returns the client's
        // completing `AgentSit`.
        pump(&mut client, &mut sim, now)?;
        let events = drain_client(&mut client);
        let result = events
            .iter()
            .find_map(|e| match e {
                Event::SitResult {
                    sit_object,
                    autopilot,
                    sit_position,
                    sit_rotation,
                    camera_eye_offset,
                    camera_at_offset,
                    force_mouselook,
                } => Some((
                    *sit_object,
                    *autopilot,
                    sit_position.clone(),
                    sit_rotation.clone(),
                    camera_eye_offset.clone(),
                    camera_at_offset.clone(),
                    *force_mouselook,
                )),
                _ => None,
            })
            .ok_or("expected a SitResult client event")?;
        assert_eq!(
            result,
            (
                target,
                transform.autopilot,
                transform.sit_position.clone(),
                transform.sit_rotation.clone(),
                transform.camera_eye_offset.clone(),
                transform.camera_at_offset.clone(),
                transform.force_mouselook,
            )
        );
        assert_eq!(client.seat(), Some(target));
        let server_events = drain_server(&mut sim);
        assert!(
            server_events
                .iter()
                .any(|e| matches!(e, ServerEvent::SitConfirmed { on: Some(on) } if *on == target)),
            "expected SitConfirmed, got {server_events:?}"
        );
        assert_eq!(sim.seated_on(), Some(target));

        client.stand(now)?;
        pump(&mut client, &mut sim, now)?;
        let server_events = drain_server(&mut sim);
        assert!(
            server_events
                .iter()
                .any(|e| matches!(e, ServerEvent::StoodUp)),
            "expected StoodUp, got {server_events:?}"
        );
        assert_eq!(sim.seated_on(), None);
        assert_eq!(client.seat(), None);
        Ok(())
    }

    /// An `AgentSit` with no outstanding sit response is surfaced with
    /// `on: None` and leaves the sim-side machine not sitting (the mirror of
    /// the client ignoring an unsolicited `AvatarSitResponse`).
    #[test]
    fn unsolicited_agent_sit_leaves_sim_not_sitting() -> Result<(), TestError> {
        let now = Instant::now();
        let (_client, mut sim) = setup(now)?;
        drain_server(&mut sim);

        let message = AnyMessage::AgentSit(sl_wire::messages::AgentSit {
            agent_data: sl_wire::messages::AgentSitAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(1),
                session_id: uuid::Uuid::from_u128(2),
            },
        });
        sim.handle_datagram(client_addr(), &client_datagram(&message, 9000, false)?, now)?;
        let server_events = drain_server(&mut sim);
        assert!(
            server_events
                .iter()
                .any(|e| matches!(e, ServerEvent::SitConfirmed { on: None })),
            "expected an unsolicited SitConfirmed, got {server_events:?}"
        );
        assert_eq!(sim.seated_on(), None);
        Ok(())
    }

    /// The script permission/control flow round-trips: the sim's
    /// `send_script_question` surfaces on the client, the client's
    /// `answer_script_permissions` converges both grant mirrors
    /// ([`ServerEvent::ScriptPermissionAnswer`]), the sim's
    /// `send_script_control_change` folds the client's taken-controls
    /// tracker, and `release_script_controls` surfaces
    /// [`ServerEvent::ForceScriptControlRelease`].
    #[test]
    fn script_permission_and_control_flow_round_trips() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);
        drain_client(&mut client);

        let task = ObjectKey::from(uuid::Uuid::from_u128(0x5C41));
        let item = InventoryKey::from(uuid::Uuid::from_u128(0x5C42));
        let asked = ScriptPermissions(
            ScriptPermissions::TAKE_CONTROLS | ScriptPermissions::TRIGGER_ANIMATION,
        );
        let question = ScriptPermissionRequest {
            task_id: task,
            item_id: item,
            object_name: "Dance Ball".to_owned(),
            object_owner: "Test User".to_owned(),
            experience_id: None,
            permissions: asked,
        };
        sim.send_script_question(&question, now)?;
        assert_eq!(sim.script_question(task, item), Some(asked));
        pump(&mut client, &mut sim, now)?;
        let events = drain_client(&mut client);
        let received = events
            .iter()
            .find_map(|e| match e {
                Event::ScriptPermissionRequest(request) => Some((**request).clone()),
                _ => None,
            })
            .ok_or("expected a ScriptPermissionRequest client event")?;
        assert_eq!(received, question);

        // Grant a subset; both mirrors converge on the answer.
        let granted = ScriptPermissions(ScriptPermissions::TAKE_CONTROLS);
        client.answer_script_permissions(task, item, granted, None, now)?;
        pump(&mut client, &mut sim, now)?;
        let server_events = drain_server(&mut sim);
        assert!(
            server_events.iter().any(|e| matches!(
                e,
                ServerEvent::ScriptPermissionAnswer {
                    task_id,
                    item_id,
                    permissions,
                } if *task_id == task && *item_id == item && *permissions == granted
            )),
            "expected ScriptPermissionAnswer, got {server_events:?}"
        );
        assert_eq!(sim.script_question(task, item), None);
        assert_eq!(sim.script_grant(task, item), Some(granted));
        assert_eq!(client.granted_permissions(task, item), granted);

        // The granted TAKE_CONTROLS now lets a script take controls.
        sim.send_script_control_change(
            &[ScriptControl {
                action: ScriptControlAction::Take,
                controls: ControlFlags::AT_POS,
                pass_to_agent: false,
            }],
            now,
        )?;
        pump(&mut client, &mut sim, now)?;
        let events = drain_client(&mut client);
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::ScriptControlChange(changes)
                    if changes.iter().any(|change| change.controls == ControlFlags::AT_POS)
            )),
            "expected ScriptControlChange, got {events:?}"
        );
        assert_eq!(client.script_controls().taken, ControlFlags::AT_POS);

        client.release_script_controls(now)?;
        pump(&mut client, &mut sim, now)?;
        let server_events = drain_server(&mut sim);
        assert!(
            server_events
                .iter()
                .any(|e| matches!(e, ServerEvent::ForceScriptControlRelease)),
            "expected ForceScriptControlRelease, got {server_events:?}"
        );
        assert_eq!(client.script_controls().taken, ControlFlags::empty());
        Ok(())
    }

    /// An all-clear `ScriptAnswerYes` is recorded as an explicit deny on both
    /// mirrors — `Some(empty)` on the sim, `Denied` on the client — distinct
    /// from a never-asked holder.
    #[test]
    fn script_permission_deny_is_recorded() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);
        drain_client(&mut client);

        let task = ObjectKey::from(uuid::Uuid::from_u128(0xDE41));
        let item = InventoryKey::from(uuid::Uuid::from_u128(0xDE42));
        sim.send_script_question(
            &ScriptPermissionRequest {
                task_id: task,
                item_id: item,
                object_name: "Grabby Cube".to_owned(),
                object_owner: "Test User".to_owned(),
                experience_id: None,
                permissions: ScriptPermissions(ScriptPermissions::DEBIT),
            },
            now,
        )?;
        pump(&mut client, &mut sim, now)?;
        drain_client(&mut client);

        client.answer_script_permissions(task, item, ScriptPermissions(0), None, now)?;
        pump(&mut client, &mut sim, now)?;
        let server_events = drain_server(&mut sim);
        assert!(
            server_events.iter().any(|e| matches!(
                e,
                ServerEvent::ScriptPermissionAnswer {
                    permissions: ScriptPermissions(0),
                    ..
                }
            )),
            "expected an explicit-deny ScriptPermissionAnswer, got {server_events:?}"
        );
        assert_eq!(sim.script_grant(task, item), Some(ScriptPermissions(0)));
        assert_eq!(
            client.script_permission_status(task, item),
            ScriptPermissionStatus::Denied
        );
        Ok(())
    }

    /// Drives A's friendship offer through to an accepted friendship on both
    /// [`setup_pair`] ends, playing the relaying driver: the offer IM off A's
    /// sim is delivered via B's sim, B accepts, and the acceptance is relayed
    /// back to A as a [`ImDialog::FriendshipAccepted`] IM. Returns the offer
    /// transaction id.
    fn befriend(a: &mut PairEnd, b: &mut PairEnd, now: Instant) -> Result<uuid::Uuid, TestError> {
        let a_agent = AgentKey::from(uuid::Uuid::from_u128(PAIR_A_AGENT));
        let b_agent = AgentKey::from(uuid::Uuid::from_u128(PAIR_B_AGENT));

        a.client
            .send_friendship_offer(b_agent, "be my friend", now)?;
        pump_end(a, now)?;
        let offer = drain_server(&mut a.sim)
            .into_iter()
            .find_map(|e| match e {
                ServerEvent::InstantMessage(im) if im.dialog == ImDialog::FriendshipOffered => {
                    Some(*im)
                }
                _ => None,
            })
            .ok_or("expected the friendship-offer IM on A's sim")?;
        assert_eq!(offer.to_agent_id, b_agent);

        // Relay the offer to B unchanged; B sees it and accepts, echoing the
        // offer IM's id as the transaction.
        b.sim.send_instant_message(&offer, now)?;
        pump_end(b, now)?;
        let received = drain_client(&mut b.client)
            .into_iter()
            .find_map(|e| match e {
                Event::InstantMessageReceived(im) if im.dialog == ImDialog::FriendshipOffered => {
                    Some(*im)
                }
                _ => None,
            })
            .ok_or("expected the relayed friendship offer on B's client")?;
        assert_eq!(received.from_agent_id, a_agent);

        b.client.accept_friendship(
            TransactionId::from(received.id),
            FriendKey::from(a_agent.uuid()),
            InventoryFolderKey::from(uuid::Uuid::from_u128(0xCA11)),
            now,
        )?;
        pump_end(b, now)?;
        let transaction = drain_server(&mut b.sim)
            .into_iter()
            .find_map(|e| match e {
                ServerEvent::FriendshipAccepted { transaction, .. } => Some(transaction),
                _ => None,
            })
            .ok_or("expected FriendshipAccepted on B's sim")?;
        assert_eq!(transaction.get(), received.id);

        // Relay the acceptance back to the offerer as a FriendshipAccepted IM
        // (the simulators notify only the original offerer).
        let accepted = InstantMessage {
            from_agent_id: b_agent,
            from_agent_name: "Peer User".to_owned(),
            to_agent_id: a_agent,
            dialog: ImDialog::FriendshipAccepted,
            from_group: false,
            region_id: None,
            position: RegionCoordinates::new(0.0, 0.0, 0.0),
            offline: false,
            timestamp: None,
            id: received.id,
            parent_estate_id: 0,
            message: "Peer User accepted your friendship offer.".to_owned(),
            binary_bucket: Vec::new(),
        };
        a.sim.send_instant_message(&accepted, now)?;
        pump_end(a, now)?;
        drain_client(&mut a.client);
        Ok(received.id)
    }

    /// The friendship offer/accept handshake relays end-to-end between two
    /// avatars (one `SimSession` per client, the test as the relaying
    /// driver), leaving both buddy caches with the default rights; presence
    /// pushes then mark each end's new friend online.
    #[test]
    fn friendship_offer_accept_relays_between_avatars() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut a, mut b) = setup_pair(now)?;
        let a_agent = AgentKey::from(uuid::Uuid::from_u128(PAIR_A_AGENT));
        let b_agent = AgentKey::from(uuid::Uuid::from_u128(PAIR_B_AGENT));
        befriend(&mut a, &mut b, now)?;

        let default_rights = FriendRights(FriendRights::CAN_SEE_ONLINE);
        let a_friend = a
            .client
            .friend(FriendKey::from(b_agent.uuid()))
            .ok_or("expected B in A's buddy cache")?;
        assert_eq!(a_friend.rights_granted, default_rights);
        assert_eq!(a_friend.rights_received, default_rights);
        let b_friend = b
            .client
            .friend(FriendKey::from(a_agent.uuid()))
            .ok_or("expected A in B's buddy cache")?;
        assert_eq!(b_friend.rights_granted, default_rights);
        assert_eq!(b_friend.rights_received, default_rights);

        // The grid-level presence service (the driver) pushes each end's new
        // friend online.
        a.sim
            .send_online_notification(&[FriendKey::from(b_agent.uuid())], now)?;
        b.sim
            .send_online_notification(&[FriendKey::from(a_agent.uuid())], now)?;
        pump_end(&mut a, now)?;
        pump_end(&mut b, now)?;
        assert!(a.client.is_online(FriendKey::from(b_agent.uuid())));
        assert!(b.client.is_online(FriendKey::from(a_agent.uuid())));
        Ok(())
    }

    /// A rights change and a termination relay between two avatars: the
    /// `GrantUserRights` surfaces on the granter's sim, the driver's
    /// `send_change_user_rights` echo + push update both buddy caches, and a
    /// `TerminateFriendship` request confirms on both ends.
    #[test]
    fn friendship_rights_and_termination_relay() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut a, mut b) = setup_pair(now)?;
        let a_agent = AgentKey::from(uuid::Uuid::from_u128(PAIR_A_AGENT));
        let b_agent = AgentKey::from(uuid::Uuid::from_u128(PAIR_B_AGENT));
        befriend(&mut a, &mut b, now)?;

        // B grants A object-modify rights on top of the default.
        let new_rights =
            FriendRights(FriendRights::CAN_SEE_ONLINE | FriendRights::CAN_MODIFY_OBJECTS);
        b.client
            .grant_user_rights(FriendKey::from(a_agent.uuid()), new_rights, now)?;
        pump_end(&mut b, now)?;
        let granted = drain_server(&mut b.sim)
            .into_iter()
            .find_map(|e| match e {
                ServerEvent::UserRightsGranted { rights } => Some(rights),
                _ => None,
            })
            .ok_or("expected UserRightsGranted on B's sim")?;
        assert_eq!(
            granted,
            vec![UserRightsEntry {
                agent: FriendKey::from(a_agent.uuid()),
                rights: new_rights,
            }]
        );

        // Driver: echo the change to the granter (changer = B's own agent,
        // entry names the friend)…
        b.sim.send_change_user_rights(
            b_agent,
            &[UserRightsEntry {
                agent: FriendKey::from(a_agent.uuid()),
                rights: new_rights,
            }],
            now,
        )?;
        pump_end(&mut b, now)?;
        let events = drain_client(&mut b.client);
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::FriendRightsChanged {
                    friend_id,
                    rights,
                    granted_to_us: false,
                } if *friend_id == FriendKey::from(a_agent.uuid()) && *rights == new_rights
            )),
            "expected the echo FriendRightsChanged on B, got {events:?}"
        );
        assert_eq!(
            b.client
                .friend(FriendKey::from(a_agent.uuid()))
                .map(|f| f.rights_granted),
            Some(new_rights)
        );

        // …and push it to the affected friend (changer = the friend B, the
        // entry names the receiving agent, as the reference simulators send).
        a.sim.send_change_user_rights(
            b_agent,
            &[UserRightsEntry {
                agent: FriendKey::from(a_agent.uuid()),
                rights: new_rights,
            }],
            now,
        )?;
        pump_end(&mut a, now)?;
        let events = drain_client(&mut a.client);
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::FriendRightsChanged {
                    friend_id,
                    rights,
                    granted_to_us: true,
                } if *friend_id == FriendKey::from(b_agent.uuid()) && *rights == new_rights
            )),
            "expected the push FriendRightsChanged on A, got {events:?}"
        );
        assert_eq!(
            a.client
                .friend(FriendKey::from(b_agent.uuid()))
                .map(|f| f.rights_received),
            Some(new_rights)
        );

        // B removes the friendship; the driver confirms on both ends.
        b.client
            .terminate_friendship(FriendKey::from(a_agent.uuid()), now)?;
        pump_end(&mut b, now)?;
        let server_events = drain_server(&mut b.sim);
        assert!(
            server_events.iter().any(|e| matches!(
                e,
                ServerEvent::FriendshipTerminationRequested { other }
                    if *other == FriendKey::from(a_agent.uuid())
            )),
            "expected FriendshipTerminationRequested, got {server_events:?}"
        );
        b.sim
            .send_terminate_friendship(FriendKey::from(a_agent.uuid()), now)?;
        a.sim
            .send_terminate_friendship(FriendKey::from(b_agent.uuid()), now)?;
        pump_end(&mut a, now)?;
        pump_end(&mut b, now)?;
        assert!(
            drain_client(&mut a.client)
                .iter()
                .any(|e| matches!(e, Event::FriendshipTerminated { .. })),
            "expected FriendshipTerminated on A"
        );
        assert!(
            drain_client(&mut b.client)
                .iter()
                .any(|e| matches!(e, Event::FriendshipTerminated { .. })),
            "expected FriendshipTerminated on B"
        );
        assert_eq!(a.client.friend(FriendKey::from(b_agent.uuid())), None);
        assert_eq!(b.client.friend(FriendKey::from(a_agent.uuid())), None);
        Ok(())
    }

    /// A declined friendship offer relays back to the offerer as an
    /// [`ImDialog::FriendshipDeclined`] IM (dialog 40 — the byte both
    /// OpenSim and the reference viewer use for the decline notification).
    #[test]
    fn friendship_decline_relays_between_avatars() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut a, mut b) = setup_pair(now)?;
        let a_agent = AgentKey::from(uuid::Uuid::from_u128(PAIR_A_AGENT));
        let b_agent = AgentKey::from(uuid::Uuid::from_u128(PAIR_B_AGENT));

        a.client.send_friendship_offer(b_agent, "friends?", now)?;
        pump_end(&mut a, now)?;
        let offer = drain_server(&mut a.sim)
            .into_iter()
            .find_map(|e| match e {
                ServerEvent::InstantMessage(im) if im.dialog == ImDialog::FriendshipOffered => {
                    Some(*im)
                }
                _ => None,
            })
            .ok_or("expected the friendship-offer IM on A's sim")?;
        b.sim.send_instant_message(&offer, now)?;
        pump_end(&mut b, now)?;
        drain_client(&mut b.client);

        b.client
            .decline_friendship(TransactionId::from(offer.id), now)?;
        pump_end(&mut b, now)?;
        let server_events = drain_server(&mut b.sim);
        assert!(
            server_events.iter().any(|e| matches!(
                e,
                ServerEvent::FriendshipDeclined { transaction }
                    if transaction.get() == offer.id
            )),
            "expected FriendshipDeclined, got {server_events:?}"
        );

        let declined = InstantMessage {
            from_agent_id: b_agent,
            from_agent_name: "Peer User".to_owned(),
            to_agent_id: a_agent,
            dialog: ImDialog::FriendshipDeclined,
            from_group: false,
            region_id: None,
            position: RegionCoordinates::new(0.0, 0.0, 0.0),
            offline: false,
            timestamp: None,
            id: offer.id,
            parent_estate_id: 0,
            message: "Peer User declined your friendship offer.".to_owned(),
            binary_bucket: Vec::new(),
        };
        a.sim.send_instant_message(&declined, now)?;
        pump_end(&mut a, now)?;
        let events = drain_client(&mut a.client);
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::InstantMessageReceived(im)
                    if im.dialog == ImDialog::FriendshipDeclined
                        && im.from_agent_id == b_agent
            )),
            "expected the relayed decline IM on A, got {events:?}"
        );
        assert_eq!(a.client.friend(FriendKey::from(b_agent.uuid())), None);
        Ok(())
    }

    /// A group chat session's lifecycle on the sim side: the client's
    /// start/send/leave fold the sim's registry (roster + server history),
    /// and the sim's `send_session_message` / `send_session_participant`
    /// surface as the client's group-session events.
    #[test]
    fn group_session_lifecycle_and_history_on_sim() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);
        drain_client(&mut client);

        let own_agent = AgentKey::from(uuid::Uuid::from_u128(1));
        let peer = AgentKey::from(uuid::Uuid::from_u128(0x9EE7));
        let group = GroupKey::from(uuid::Uuid::from_u128(0x64019));
        let session_id = ImSessionId::from(group.uuid());

        client.start_group_session(group, now)?;
        pump(&mut client, &mut sim, now)?;
        let server_events = drain_server(&mut sim);
        assert!(
            server_events.iter().any(|e| matches!(
                e,
                ServerEvent::GroupSessionStartRequested { group_id } if *group_id == group
            )),
            "expected GroupSessionStartRequested, got {server_events:?}"
        );
        let session = sim
            .chat_session(session_id)
            .ok_or("expected the group session in the sim registry")?;
        assert_eq!(session.kind, SimChatSessionKind::Group { group_id: group });
        assert!(session.participants.contains(&own_agent));

        // The sim announces a peer joining the group channel.
        sim.send_session_participant(session_id, peer, "Peer User", true, true, now)?;
        pump(&mut client, &mut sim, now)?;
        let events = drain_client(&mut client);
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::GroupSessionParticipant {
                    group_id,
                    agent_id,
                    joined: true,
                } if *group_id == group && *agent_id == peer
            )),
            "expected the joined GroupSessionParticipant, got {events:?}"
        );

        // The client's own message lands in the sim's server history…
        client.send_group_message(group, "hello group", now)?;
        pump(&mut client, &mut sim, now)?;
        let server_events = drain_server(&mut sim);
        assert!(
            server_events.iter().any(|e| matches!(
                e,
                ServerEvent::SessionMessageSent { session_id: sid, message }
                    if *sid == session_id && message == "hello group"
            )),
            "expected SessionMessageSent, got {server_events:?}"
        );
        // …as does a relayed peer message, which the client folds as group chat.
        sim.send_session_message(session_id, peer, "Peer User", "hi back", true, now)?;
        pump(&mut client, &mut sim, now)?;
        let events = drain_client(&mut client);
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::GroupSessionMessage {
                    group_id,
                    from_agent_id,
                    from_name,
                    message,
                } if *group_id == group
                    && *from_agent_id == peer
                    && from_name == "Peer User"
                    && message == "hi back"
            )),
            "expected GroupSessionMessage, got {events:?}"
        );
        let history: Vec<(String, String)> = sim
            .chat_session(session_id)
            .ok_or("expected the group session in the sim registry")?
            .history
            .iter()
            .map(|entry| (entry.sender_name.clone(), entry.text.clone()))
            .collect();
        assert_eq!(
            history,
            vec![
                ("Test User".to_owned(), "hello group".to_owned()),
                ("Peer User".to_owned(), "hi back".to_owned()),
            ]
        );

        // Leaving drops the client from the roster (the peer keeps the
        // session alive).
        client.leave_group_session(group, now)?;
        pump(&mut client, &mut sim, now)?;
        let server_events = drain_server(&mut sim);
        assert!(
            server_events.iter().any(|e| matches!(
                e,
                ServerEvent::SessionLeaveRequested { session_id: sid } if *sid == session_id
            )),
            "expected SessionLeaveRequested, got {server_events:?}"
        );
        let session = sim
            .chat_session(session_id)
            .ok_or("expected the session to survive while the peer remains")?;
        assert!(!session.participants.contains(&own_agent));
        assert!(session.participants.contains(&peer));
        Ok(())
    }

    /// An ad-hoc conference relays between two avatars: A's conference start
    /// registers on A's sim, the driver materialises it on B's sim and
    /// delivers the `ChatterBoxInvitation` over B's event queue, B joins and
    /// speaks, and the relayed message lands in both sims' server histories
    /// and A's client events.
    #[test]
    fn conference_relays_between_avatars() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut a, mut b) = setup_pair(now)?;
        let a_agent = AgentKey::from(uuid::Uuid::from_u128(PAIR_A_AGENT));
        let b_agent = AgentKey::from(uuid::Uuid::from_u128(PAIR_B_AGENT));
        let session_id = ImSessionId::from(uuid::Uuid::from_u128(0xC04F));

        a.client
            .start_conference(session_id, &[b_agent], "join us", now)?;
        pump_end(&mut a, now)?;
        let server_events = drain_server(&mut a.sim);
        assert!(
            server_events.iter().any(|e| matches!(
                e,
                ServerEvent::ConferenceStartRequested {
                    session_id: sid,
                    invitees,
                    message,
                } if *sid == session_id && *invitees == vec![b_agent] && message == "join us"
            )),
            "expected ConferenceStartRequested, got {server_events:?}"
        );
        let session = a
            .sim
            .chat_session(session_id)
            .ok_or("expected the conference on A's sim")?;
        assert_eq!(session.kind, SimChatSessionKind::Conference);
        assert!(session.participants.contains(&a_agent));
        assert!(session.participants.contains(&b_agent));

        // Driver: materialise the conference on B's sim and deliver the
        // invitation over B's event queue.
        b.sim.open_chat_session(
            session_id,
            SimChatSessionKind::Conference,
            &[a_agent, b_agent],
        );
        b.sim
            .enqueue_chatterbox_invitation(&Event::ConferenceInvited {
                session_id: session_id.get(),
                from_agent_id: a_agent,
                from_name: "Test User".to_owned(),
                dialog: ImDialog::SessionConferenceStart,
                from_group: false,
                session_name: "join us".to_owned(),
                message: "join us".to_owned(),
                region_id: uuid::Uuid::nil(),
                position: RegionCoordinates::new(1.0, 2.0, 3.0),
                parent_estate_id: 1,
                timestamp: None,
                binary_bucket: Vec::new(),
            });
        deliver_caps(&mut b.client, &mut b.sim, now)?;
        let kind = ChatSessionKind::Conference { id: session_id };
        let info = b
            .client
            .chat_sessions_info()
            .find(|info| info.kind == kind)
            .ok_or("expected the invited conference on B's client")?;
        assert!(matches!(info.lifecycle, ChatLifecycleView::Invited { .. }));

        // B accepts and speaks; the message reaches B's sim (and history).
        b.client.accept_chat_invite(session_id, false, now);
        b.client.send_conference_message(session_id, "here", now)?;
        pump_end(&mut b, now)?;
        let server_events = drain_server(&mut b.sim);
        assert!(
            server_events.iter().any(|e| matches!(
                e,
                ServerEvent::SessionMessageSent { session_id: sid, message }
                    if *sid == session_id && message == "here"
            )),
            "expected SessionMessageSent on B's sim, got {server_events:?}"
        );

        // Driver relays B's message to A, whose client folds it as a
        // conference message.
        a.sim
            .send_session_message(session_id, b_agent, "Peer User", "here", false, now)?;
        pump_end(&mut a, now)?;
        let events = drain_client(&mut a.client);
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::ConferenceSessionMessage {
                    session_id: sid,
                    from_agent_id,
                    from_name,
                    message,
                } if *sid == session_id.get()
                    && *from_agent_id == b_agent
                    && from_name == "Peer User"
                    && message == "here"
            )),
            "expected ConferenceSessionMessage on A, got {events:?}"
        );
        for (label, sim) in [("A", &a.sim), ("B", &b.sim)] {
            let history = &sim
                .chat_session(session_id)
                .ok_or_else(|| format!("expected the conference on {label}'s sim"))?
                .history;
            assert!(
                history
                    .iter()
                    .any(|entry| entry.sender == b_agent && entry.text == "here"),
                "expected the message in {label}'s server history, got {history:?}"
            );
        }

        // B leaves; the driver notifies A's client via A's sim.
        b.client.leave_conference(session_id, now)?;
        pump_end(&mut b, now)?;
        let server_events = drain_server(&mut b.sim);
        assert!(
            server_events.iter().any(|e| matches!(
                e,
                ServerEvent::SessionLeaveRequested { session_id: sid } if *sid == session_id
            )),
            "expected SessionLeaveRequested on B's sim, got {server_events:?}"
        );
        a.sim
            .send_session_participant(session_id, b_agent, "Peer User", false, false, now)?;
        pump_end(&mut a, now)?;
        let events = drain_client(&mut a.client);
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::ConferenceSessionParticipant {
                    session_id: sid,
                    agent_id,
                    joined: false,
                } if *sid == session_id.get() && *agent_id == b_agent
            )),
            "expected the left ConferenceSessionParticipant on A, got {events:?}"
        );
        Ok(())
    }

    /// **The `Session` ↔ `SimSession` flow-mirroring coverage table, pinned.**
    /// One row per flow-level (multi-message) state machine the client
    /// `Session` implements — the committed audit the `protocol-sim-udp-flows`
    /// task opened with. `Mirrored` rows are proven by the loopback tests in
    /// this file; `Pending` rows await a follow-up `protocol-sim-*` task;
    /// `Legacy` rows are deliberately skipped because BOTH grids offer a
    /// modern (CAPS) alternative. Any drift is a loud diff — if intended,
    /// bless it by editing this table (and `SESSION_FLOW_COVERAGE`).
    #[test]
    fn flow_coverage_table_is_pinned() {
        let expected: Vec<(&str, FlowMirrorStatus)> = vec![
            ("root circuit lifecycle", FlowMirrorStatus::Mirrored),
            ("child-agent circuits", FlowMirrorStatus::Mirrored),
            ("teleport / region handover", FlowMirrorStatus::Mirrored),
            ("object sit", FlowMirrorStatus::Mirrored),
            ("Xfer download", FlowMirrorStatus::Mirrored),
            ("Xfer upload", FlowMirrorStatus::Mirrored),
            ("terrain RAW download", FlowMirrorStatus::Mirrored),
            ("terrain RAW upload", FlowMirrorStatus::Mirrored),
            (
                "legacy transaction asset upload",
                FlowMirrorStatus::Mirrored,
            ),
            ("task-inventory fetch", FlowMirrorStatus::Mirrored),
            (
                "UDP asset Transfer (task item + estate covenant)",
                FlowMirrorStatus::Mirrored,
            ),
            ("UDP texture download", FlowMirrorStatus::Legacy),
            ("UDP inventory-folder fetch", FlowMirrorStatus::Legacy),
            (
                "chat-session lifecycle + server history",
                FlowMirrorStatus::Mirrored,
            ),
            ("friendship / presence", FlowMirrorStatus::Mirrored),
            (
                "script permission / control mirror",
                FlowMirrorStatus::Mirrored,
            ),
        ];
        assert_eq!(
            SESSION_FLOW_COVERAGE.to_vec(),
            expected,
            "a Session flow's server-side mirror status changed — if \
             intended, bless it by editing this table"
        );
    }

    /// The destination simulator's UDP address for two-region tests.
    fn dest_sim_addr() -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 9001)
    }

    /// The destination region handle for two-region tests (one region east of
    /// [`REGION_HANDLE`]).
    const DEST_HANDLE: u64 = 0x0000_03e9_0000_03e8;

    /// An intra-region teleport round-trips: the client's `teleport_to`
    /// surfaces [`ServerEvent::TeleportRequested`] with the requested handle
    /// and position, and the driver's `send_teleport_start` +
    /// `send_teleport_local` bring the client back to active with
    /// [`Event::TeleportStarted`] and [`Event::TeleportLocal`].
    #[test]
    fn teleport_local_round_trips() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);

        let position = RegionCoordinates::new(10.0, 20.0, 30.0);
        let look_at = sl_types::lsl::Vector {
            x: 1.0,
            y: 0.0,
            z: 0.0,
        };
        client.teleport_to(RegionHandle(REGION_HANDLE), position, look_at.clone(), now)?;
        pump(&mut client, &mut sim, now)?;
        let server_events = drain_server(&mut sim);
        assert!(
            server_events.iter().any(|e| matches!(
                e,
                ServerEvent::TeleportRequested {
                    region_handle,
                    position: got,
                    ..
                } if *region_handle == RegionHandle(REGION_HANDLE) && *got == position
            )),
            "expected TeleportRequested, got {server_events:?}"
        );

        sim.send_teleport_start(0, now)?;
        sim.send_teleport_local(position, look_at, 0, now)?;
        pump(&mut client, &mut sim, now)?;
        let client_events = drain_client(&mut client);
        assert!(
            client_events
                .iter()
                .any(|e| matches!(e, Event::TeleportStarted)),
            "expected TeleportStarted, got {client_events:?}"
        );
        assert!(
            client_events.iter().any(|e| matches!(
                e,
                Event::TeleportLocal { position: got } if *got == position
            )),
            "expected TeleportLocal, got {client_events:?}"
        );
        Ok(())
    }

    /// A failed teleport surfaces its progress and reason and returns the
    /// client to the active state (a follow-up request is accepted again).
    #[test]
    fn teleport_failed_returns_client_to_active() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);

        let position = RegionCoordinates::new(1.0, 2.0, 3.0);
        let look_at = sl_types::lsl::Vector {
            x: 0.0,
            y: 1.0,
            z: 0.0,
        };
        client.teleport_to(RegionHandle(DEST_HANDLE), position, look_at, now)?;
        pump(&mut client, &mut sim, now)?;
        drain_server(&mut sim);
        sim.send_teleport_start(0, now)?;
        sim.send_teleport_progress("resolving destination", 0, now)?;
        sim.send_teleport_failed("no such region", now)?;
        pump(&mut client, &mut sim, now)?;

        let client_events = drain_client(&mut client);
        assert!(
            client_events.iter().any(|e| matches!(
                e,
                Event::TeleportProgress { message, .. } if message == "resolving destination"
            )),
            "expected TeleportProgress, got {client_events:?}"
        );
        assert!(
            client_events.iter().any(|e| matches!(
                e,
                Event::TeleportFailed { reason, .. } if reason == "no such region"
            )),
            "expected TeleportFailed, got {client_events:?}"
        );
        // Back to active: a fresh request goes out again.
        client.teleport_to(
            RegionHandle(REGION_HANDLE),
            position,
            sl_types::lsl::Vector {
                x: 1.0,
                y: 0.0,
                z: 0.0,
            },
            now,
        )?;
        pump(&mut client, &mut sim, now)?;
        let server_events = drain_server(&mut sim);
        assert!(
            server_events
                .iter()
                .any(|e| matches!(e, ServerEvent::TeleportRequested { .. })),
            "expected the follow-up TeleportRequested, got {server_events:?}"
        );
        Ok(())
    }

    /// The full inter-region teleport across TWO simulator sessions: the
    /// source surfaces the request and drives the CAPS event-queue trio
    /// (`EnableSimulator` + `EstablishAgentCommunication` opening a child
    /// circuit on the destination, then `TeleportFinish`); the client promotes
    /// the child, the destination confirms the arrival
    /// (`CompleteAgentMovement` → root agent), and the client lands with
    /// `RegionChanged`.
    #[test]
    fn inter_region_teleport_two_sims() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut source) = setup(now)?;
        drain_server(&mut source);
        let mut dest = SimSession::new(RegionHandle(DEST_HANDLE), now);

        let position = RegionCoordinates::new(128.0, 128.0, 25.0);
        let look_at = sl_types::lsl::Vector {
            x: 1.0,
            y: 0.0,
            z: 0.0,
        };
        client.teleport_to(RegionHandle(DEST_HANDLE), position, look_at, now)?;
        pump(&mut client, &mut source, now)?;
        let server_events = drain_server(&mut source);
        assert!(
            server_events.iter().any(|e| matches!(
                e,
                ServerEvent::TeleportRequested { region_handle, .. }
                    if *region_handle == RegionHandle(DEST_HANDLE)
            )),
            "expected TeleportRequested on the source, got {server_events:?}"
        );

        // The source accepts and announces the destination region.
        source.send_teleport_start(0, now)?;
        source.enqueue_enable_simulator(RegionHandle(DEST_HANDLE), dest_sim_addr());
        source.enqueue_establish_agent_communication(
            dest_sim_addr(),
            "http://127.0.0.1:9001/child-seed",
        );
        let caps_events = deliver_caps(&mut client, &mut source, now)?;
        assert!(
            caps_events
                .iter()
                .any(|e| matches!(e, Event::NeighborSeed { sim, .. } if *sim == dest_sim_addr())),
            "expected the child seed, got {caps_events:?}"
        );
        pump_multi(
            &mut client,
            &mut [(sim_addr(), &mut source), (dest_sim_addr(), &mut dest)],
            now,
        )?;
        let dest_events = drain_server(&mut dest);
        assert!(
            dest_events
                .iter()
                .any(|e| matches!(e, ServerEvent::CircuitOpened { .. })),
            "expected the child circuit on the destination, got {dest_events:?}"
        );
        assert_eq!(dest.agent_presence(), AgentPresence::Child);

        // The source finishes the teleport; the client promotes the child. The
        // finish carries the destination handle (the reference record), and the
        // client reports that wire handle rather than the one it requested.
        let agent_id = source.agent_id().ok_or("source has no agent")?;
        source.enqueue_teleport_finish(&TeleportFinishInfo {
            agent_id,
            location_id: TELEPORT_FINISH_LOCATION_ID,
            dest: dest_sim_addr(),
            region_handle: RegionHandle(DEST_HANDLE),
            seed: "http://127.0.0.1:9001/seed".to_owned(),
            sim_access: 21,
            teleport_flags: 16,
            region_size: (STANDARD_REGION_SIZE_METRES, STANDARD_REGION_SIZE_METRES),
        });
        let finish_events = deliver_caps(&mut client, &mut source, now)?;
        assert!(
            finish_events.iter().any(|e| matches!(
                e,
                Event::TeleportFinished { sim, region_handle, .. }
                    if *sim == dest_sim_addr() && *region_handle == RegionHandle(DEST_HANDLE)
            )),
            "expected TeleportFinished naming the destination, got {finish_events:?}"
        );
        pump_multi(
            &mut client,
            &mut [(sim_addr(), &mut source), (dest_sim_addr(), &mut dest)],
            now,
        )?;
        let dest_events = drain_server(&mut dest);
        assert!(
            dest_events
                .iter()
                .any(|e| matches!(e, ServerEvent::AgentArrived)),
            "expected the arrival on the destination, got {dest_events:?}"
        );
        assert!(dest.is_root_agent());
        let client_events = drain_client(&mut client);
        assert!(
            client_events.iter().any(|e| matches!(
                e,
                Event::RegionChanged { region_handle, sim, .. }
                    if *region_handle == RegionHandle(DEST_HANDLE) && *sim == dest_sim_addr()
            )),
            "expected RegionChanged, got {client_events:?}"
        );

        // The source retires the now-child circuit; the client tears it down
        // without disturbing the new root, and the source session is closed
        // (its driver's pumps exit) with the retirement as the last event.
        source.retire_circuit(now)?;
        pump_multi(
            &mut client,
            &mut [(sim_addr(), &mut source), (dest_sim_addr(), &mut dest)],
            now,
        )?;
        assert!(source.is_closed(), "the retired source should be closed");
        let source_events = drain_server(&mut source);
        assert!(
            source_events
                .iter()
                .any(|e| matches!(e, ServerEvent::CircuitRetired)),
            "expected CircuitRetired, got {source_events:?}"
        );
        // The new root is untouched by the retirement.
        assert!(client.root_circuit_id().is_some());
        assert!(dest.is_root_agent());
        Ok(())
    }

    /// A teleport the **simulator** decides on (`llTeleportAgent`, a god
    /// "teleport home", a grid-side push): no client request, the source's
    /// remote `TeleportStart` opens the teleport on the client, the
    /// event-queue trio lands it in the destination, and the client reports
    /// the destination handle from the wire (it never knew a target).
    #[test]
    fn remote_initiated_teleport_two_sims() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut source) = setup(now)?;
        drain_server(&mut source);
        drain_client(&mut client);
        let mut dest = SimSession::new(RegionHandle(DEST_HANDLE), now);

        source.send_teleport_start(sl_proto::TeleportFlags::VIA_LOCATION, now)?;
        source.send_teleport_progress(
            "sending_dest",
            sl_proto::TeleportFlags::VIA_LOCATION,
            now,
        )?;
        pump(&mut client, &mut source, now)?;
        let client_events = drain_client(&mut client);
        assert!(
            client_events
                .iter()
                .any(|e| matches!(e, Event::TeleportStarted)),
            "a remote TeleportStart opens the teleport, got {client_events:?}"
        );
        assert!(
            client_events.iter().any(|e| matches!(
                e,
                Event::TeleportProgress { message, .. } if message == "sending_dest"
            )),
            "expected the progress key, got {client_events:?}"
        );

        source.enqueue_enable_simulator(RegionHandle(DEST_HANDLE), dest_sim_addr());
        source.enqueue_establish_agent_communication(
            dest_sim_addr(),
            "http://127.0.0.1:9001/child-seed",
        );
        let agent_id = source.agent_id().ok_or("source has no agent")?;
        source.enqueue_teleport_finish(&TeleportFinishInfo {
            agent_id,
            location_id: TELEPORT_FINISH_LOCATION_ID,
            dest: dest_sim_addr(),
            region_handle: RegionHandle(DEST_HANDLE),
            seed: "http://127.0.0.1:9001/seed".to_owned(),
            sim_access: 13,
            teleport_flags: sl_proto::TeleportFlags::VIA_LOCATION,
            region_size: (STANDARD_REGION_SIZE_METRES, STANDARD_REGION_SIZE_METRES),
        });
        let caps_events = deliver_caps(&mut client, &mut source, now)?;
        assert!(
            caps_events.iter().any(|e| matches!(
                e,
                Event::TeleportFinished { region_handle, .. }
                    if *region_handle == RegionHandle(DEST_HANDLE)
            )),
            "expected TeleportFinished with the wire handle, got {caps_events:?}"
        );
        pump_multi(
            &mut client,
            &mut [(sim_addr(), &mut source), (dest_sim_addr(), &mut dest)],
            now,
        )?;
        let dest_events = drain_server(&mut dest);
        assert!(
            dest_events
                .iter()
                .any(|e| matches!(e, ServerEvent::AgentArrived)),
            "expected the arrival on the destination, got {dest_events:?}"
        );
        assert!(dest.is_root_agent());
        let client_events = drain_client(&mut client);
        assert!(
            client_events.iter().any(|e| matches!(
                e,
                Event::RegionChanged { region_handle, .. }
                    if *region_handle == RegionHandle(DEST_HANDLE)
            )),
            "expected RegionChanged, got {client_events:?}"
        );

        // A finish with no start at all is honoured the same way: the client
        // is Active again, a second push lands it back on the source's twin.
        let mut back = SimSession::new(RegionHandle(REGION_HANDLE), now);
        let back_addr = SocketAddr::from(([127, 0, 0, 1], 9002));
        dest.enqueue_teleport_finish(&TeleportFinishInfo {
            agent_id,
            location_id: TELEPORT_FINISH_LOCATION_ID,
            dest: back_addr,
            region_handle: RegionHandle(REGION_HANDLE),
            seed: "http://127.0.0.1:9002/seed".to_owned(),
            sim_access: 13,
            teleport_flags: sl_proto::TeleportFlags::VIA_LOCATION,
            region_size: (STANDARD_REGION_SIZE_METRES, STANDARD_REGION_SIZE_METRES),
        });
        let caps_events = deliver_caps(&mut client, &mut dest, now)?;
        assert!(
            caps_events
                .iter()
                .any(|e| matches!(e, Event::TeleportStarted)),
            "a finish without a start opens the teleport, got {caps_events:?}"
        );
        pump_multi(
            &mut client,
            &mut [(dest_sim_addr(), &mut dest), (back_addr, &mut back)],
            now,
        )?;
        assert!(
            back.is_root_agent(),
            "the client should have followed the push"
        );
        Ok(())
    }

    /// A physical border crossing: the child circuit is pre-opened via the
    /// event-queue announcements, then the source's `CrossedRegion` promotes
    /// it to root with no teleport screen — the destination confirms the
    /// arrival and the client lands with a non-world-resetting
    /// `RegionChanged`.
    #[test]
    fn crossed_region_two_sims() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut source) = setup(now)?;
        drain_server(&mut source);
        let mut dest = SimSession::new(RegionHandle(DEST_HANDLE), now);

        source.enqueue_enable_simulator(RegionHandle(DEST_HANDLE), dest_sim_addr());
        source.enqueue_establish_agent_communication(
            dest_sim_addr(),
            "http://127.0.0.1:9001/child-seed",
        );
        deliver_caps(&mut client, &mut source, now)?;
        pump_multi(
            &mut client,
            &mut [(sim_addr(), &mut source), (dest_sim_addr(), &mut dest)],
            now,
        )?;
        drain_server(&mut dest);
        assert_eq!(dest.agent_presence(), AgentPresence::Child);

        let agent_id = source.agent_id().ok_or("the source has no agent")?;
        source.enqueue_crossed_region(&sl_proto::CrossedRegionInfo {
            agent_id,
            session_id: source.session_id().unwrap_or_default(),
            region_handle: RegionHandle(DEST_HANDLE),
            dest: dest_sim_addr(),
            seed: "http://127.0.0.1:9001/seed".to_owned(),
            position: sl_types::map::RegionCoordinates::new(4.0, 128.0, 26.0),
            look_at: sl_types::lsl::Vector {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            region_size: (
                sl_proto::STANDARD_REGION_SIZE_METRES,
                sl_proto::STANDARD_REGION_SIZE_METRES,
            ),
        });
        deliver_caps(&mut client, &mut source, now)?;
        pump_multi(
            &mut client,
            &mut [(sim_addr(), &mut source), (dest_sim_addr(), &mut dest)],
            now,
        )?;
        let dest_events = drain_server(&mut dest);
        assert!(
            dest_events
                .iter()
                .any(|e| matches!(e, ServerEvent::AgentArrived)),
            "expected the crossing arrival, got {dest_events:?}"
        );
        assert!(dest.is_root_agent());
        let client_events = drain_client(&mut client);
        assert!(
            client_events.iter().any(|e| matches!(
                e,
                Event::RegionChanged { region_handle, sim, world_reset: false, .. }
                    if *region_handle == RegionHandle(DEST_HANDLE) && *sim == dest_sim_addr()
            )),
            "expected a non-resetting RegionChanged, got {client_events:?}"
        );
        // The source simulator demotes itself once the handover is done: the
        // avatar is next door, but the circuit stays open as a child so the
        // region it walked out of keeps streaming.
        source.make_child_agent();
        assert_eq!(source.agent_presence(), AgentPresence::Child);
        assert!(
            !source.is_closed(),
            "a crossing's source circuit stays open as a child, unlike a teleport's"
        );
        Ok(())
    }

    /// The full task-inventory serving path: the client's
    /// `fetch_task_inventory` sends the request, the driver answers with
    /// `serve_task_inventory` (listing writer + `Xfer` registration +
    /// `ReplyTaskInventory`), and the client downloads and parses the listing
    /// back into the exact served items.
    #[test]
    fn task_inventory_fetch_round_trips() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);
        let circuit = client.root_circuit_id().ok_or("no circuit")?;

        client.fetch_task_inventory(ScopedObjectId::new(circuit, RegionLocalObjectId(77)), now)?;
        pump(&mut client, &mut sim, now)?;
        let server_events = drain_server(&mut sim);
        assert!(
            server_events.iter().any(|e| matches!(
                e,
                ServerEvent::RequestTaskInventory { local_id } if *local_id == RegionLocalObjectId(77)
            )),
            "expected RequestTaskInventory, got {server_events:?}"
        );

        let task = ObjectKey::from(uuid::Uuid::from_u128(0x2222));
        let items = vec![task_script_item(task)];
        sim.serve_task_inventory(task, 5, &items, now)?;
        pump(&mut client, &mut sim, now)?;

        let client_events = drain_client(&mut client);
        assert!(
            client_events.iter().any(|e| matches!(
                e,
                Event::TaskInventoryReply(reply)
                    if reply.task == task && reply.serial == 5 && !reply.filename.is_empty()
            )),
            "expected TaskInventoryReply, got {client_events:?}"
        );
        assert!(
            client_events.iter().any(|e| matches!(
                e,
                Event::TaskInventoryContents { task: got, serial: 5, items: parsed }
                    if *got == task && *parsed == items
            )),
            "expected the parsed task inventory, got {client_events:?}"
        );
        Ok(())
    }

    /// A full region-wide public parcel record, as a simulator would push on
    /// region entry (bitmap all ones: 64×64 blocks of a 256 m region).
    fn region_wide_parcel(sequence_id: i32) -> Result<ParcelInfo, TestError> {
        Ok(ParcelInfo {
            sequence_id,
            request_result: ParcelRequestResult::Single,
            snap_selection: false,
            self_count: 1,
            other_count: 2,
            public_count: 3,
            local_id: RegionLocalParcelId(1),
            owner: OwnerKey::Agent(AgentKey::from(uuid::Uuid::from_u128(0xA11))),
            group: Some(GroupKey::from(uuid::Uuid::from_u128(0x6))),
            auction_id: 0x8000_0001,
            claim_date: 1_700_000_000,
            claim_price: LindenAmount(10),
            rent_price: LindenAmount(20),
            aabb_min: RegionCoordinates::new(0.0, 0.0, 0.0),
            aabb_max: RegionCoordinates::new(256.0, 256.0, 0.0),
            area: LandArea(0x0001_0000),
            bitmap: vec![0xFF; 512],
            status: ParcelStatus::Leased,
            category: ParcelCategory::Residential,
            max_prims: 15_000,
            sim_wide_max_prims: 15_000,
            sim_wide_total_prims: 12,
            total_prims: 12,
            owner_prims: 10,
            group_prims: 1,
            other_prims: 1,
            selected_prims: 0,
            parcel_prim_bonus: 1.5,
            other_clean_time: 30,
            raw_parcel_flags: sl_wire::ParcelFlags::ALLOW_FLY
                .union(sl_wire::ParcelFlags::CREATE_OBJECTS)
                .union(sl_wire::ParcelFlags::FOR_SALE)
                .bits(),
            sale_price: Some(LindenAmount(4_200)),
            name: "Fake Grid Parcel".to_owned(),
            description: "Region-wide public land".to_owned(),
            music_url: Some("http://stream.example/radio".parse()?),
            media_url: None,
            media_id: Some(TextureKey::from(uuid::Uuid::from_u128(0xBEEF))),
            media_auto_scale: true,
            auth_buyer_id: None,
            snapshot_id: Some(TextureKey::from(uuid::Uuid::from_u128(0x5A9))),
            pass_price: LindenAmount(0),
            pass_hours: 0.5,
            user_location: RegionCoordinates::new(128.0, 128.0, 25.0),
            user_look_at: sl_proto::Direction::new(1.0, 0.0, 0.0),
            landing_type: LandingType::LandingPoint,
            region_push_override: false,
            region_deny_anonymous: true,
            region_deny_identified: false,
            region_deny_transacted: false,
            region_deny_age_unverified: true,
            region_allow_access_override: true,
            parcel_environment_version: 3,
            region_allow_environment_override: true,
            see_avs: None,
            any_av_sounds: None,
            group_av_sounds: None,
        })
    }

    /// A unit box prim at `position` — the minimal `Object` a full
    /// `ObjectUpdate` can carry.
    fn box_prim(local_id: u32, full_id: u128, position: sl_proto::Vector) -> sl_proto::Object {
        let zero = sl_proto::Vector {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        sl_proto::Object {
            region_handle: RegionHandle(0),
            local_id: RegionLocalObjectId(local_id),
            circuit: sl_proto::CircuitId::default(),
            full_id: ObjectKey::from(uuid::Uuid::from_u128(full_id)),
            parent_id: RegionLocalObjectId(0),
            pcode: sl_proto::pcode::PRIMITIVE,
            state: 0,
            crc: 7,
            material: 3,
            click_action: 0,
            update_flags: 0,
            scale: sl_proto::Vector {
                x: 1.0,
                y: 2.0,
                z: 3.0,
            },
            motion: sl_proto::ObjectMotion {
                position,
                velocity: zero.clone(),
                acceleration: zero.clone(),
                rotation: sl_proto::Rotation {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    s: 1.0,
                },
                angular_velocity: zero.clone(),
                collision_plane: None,
            },
            owner_id: uuid::Uuid::from_u128(0xA11),
            sound: uuid::Uuid::nil(),
            gain: 0.0,
            sound_flags: 0,
            sound_radius: 0.0,
            text: "hover".to_owned(),
            text_color: [255, 0, 0, 255],
            name_value: String::new(),
            media_url: None,
            texture_entry: Vec::new(),
            texture_anim: Vec::new(),
            texture_animation: None,
            shape: PrimShapeParams {
                path_curve: 16,
                profile_curve: 1,
                path_scale_x: 100,
                path_scale_y: 100,
                ..PrimShapeParams::default()
            },
            particle_system: Vec::new(),
            particles: None,
            data: Vec::new(),
            extra_params: Vec::new(),
            extra: ObjectExtraParams::default(),
            properties: None,
            joint_type: 0,
            joint_pivot: zero.clone(),
            joint_axis_or_anchor: zero,
        }
    }

    #[test]
    fn server_parcel_properties_round_trip_udp_and_caps() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);
        drain_client(&mut client);

        // The client's rectangle request decodes server-side.
        client.request_parcel_properties(4.0, 8.0, 12.0, 16.0, -50_000, now)?;
        pump(&mut client, &mut sim, now)?;
        let server_events = drain_server(&mut sim);
        assert!(
            server_events.iter().any(|e| matches!(
                e,
                ServerEvent::RequestParcelProperties { west, north, sequence_id: -50_000, snap_selection: false, .. }
                    if west.to_bits() == 4.0_f32.to_bits() && north.to_bits() == 16.0_f32.to_bits()
            )),
            "expected RequestParcelProperties, got {server_events:?}"
        );

        // UDP: the record survives field-for-field.
        let parcel = region_wide_parcel(-50_000)?;
        sim.send_parcel_properties(&parcel, now)?;
        pump(&mut client, &mut sim, now)?;
        let udp = drain_client(&mut client)
            .into_iter()
            .find_map(|e| match e {
                Event::ParcelProperties(info) => Some(*info),
                _ => None,
            })
            .ok_or("expected a UDP ParcelProperties event")?;
        assert_eq!(udp, parcel);

        // CAPS event queue: the same record through the long-poll body.
        let renamed = ParcelInfo {
            name: "Renamed Parcel".to_owned(),
            sequence_id: 9,
            ..parcel
        };
        sim.enqueue_parcel_properties(&renamed);
        let caps = deliver_caps(&mut client, &mut sim, now)?
            .into_iter()
            .find_map(|e| match e {
                Event::ParcelProperties(info) => Some(*info),
                _ => None,
            })
            .ok_or("expected a CAPS ParcelProperties event")?;
        assert_eq!(caps, renamed);
        Ok(())
    }

    #[test]
    fn server_parcel_overlay_chunks_reach_client() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);
        drain_client(&mut client);

        let overlay: Vec<u8> = (0..4096_u32)
            .map(|i| u8::try_from(i % 251).unwrap_or(0))
            .collect();
        sim.send_parcel_overlay(&overlay, now)?;
        pump(&mut client, &mut sim, now)?;
        let chunks: Vec<(i32, Vec<u8>)> = drain_client(&mut client)
            .into_iter()
            .filter_map(|e| match e {
                Event::ParcelOverlay(info) => Some((info.sequence_id, info.data)),
                _ => None,
            })
            .collect();
        let expected: Vec<(i32, Vec<u8>)> = overlay
            .chunks(sl_proto::PARCEL_OVERLAY_CHUNK_BYTES)
            .enumerate()
            .map(|(i, chunk)| (i32::try_from(i).unwrap_or(-1), chunk.to_vec()))
            .collect();
        assert_eq!(chunks.len(), 4);
        assert_eq!(chunks, expected);
        Ok(())
    }

    /// The edge, in cells, of a standard region's terrain patch.
    const PATCH_CELLS: u32 = 16;

    /// A flat land patch at grid (`patch_x`, `patch_y`), every cell `height`
    /// metres.
    fn land_patch(patch_x: u32, patch_y: u32, height: f32) -> TerrainPatch {
        TerrainPatch {
            region_handle: RegionHandle(0),
            layer: TerrainLayerType::Land,
            patch_x,
            patch_y,
            size: PATCH_CELLS,
            values: vec![height; 256],
        }
    }

    /// The height a test patch at (`patch_x`, `patch_y`) is flat at — distinct
    /// per patch, so a patch that arrives under the wrong coordinates shows up.
    fn patch_height(patch_x: u32, patch_y: u32) -> f32 {
        let x = u16::try_from(patch_x).unwrap_or(0);
        let y = u16::try_from(patch_y).unwrap_or(0);
        f32::from(y).mul_add(7.0, f32::from(x).mul_add(3.0, 20.0))
    }

    #[test]
    fn server_terrain_reaches_the_client_in_spiral_order() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);
        drain_client(&mut client);

        // A `LayerData` message carries no region handle: the client labels
        // each patch with the handle it learned from the circuit's first
        // object update, so one has to precede the ground.
        let prim = box_prim(
            0x20,
            0x2020,
            sl_proto::Vector {
                x: 1.0,
                y: 2.0,
                z: 3.0,
            },
        );
        sim.send_object_update(std::slice::from_ref(&prim), 0xFFFF, now)?;
        pump(&mut client, &mut sim, now)?;
        drain_client(&mut client);

        // A 4×4 patch grid, each patch flat at its own height.
        let side = 4_u32;
        let patches: Vec<TerrainPatch> = (0..side)
            .flat_map(|y| (0..side).map(move |x| land_patch(x, y, patch_height(x, y))))
            .collect();
        sim.send_terrain(&patches, now)?;

        // Count the `LayerData` messages on the way to the client.
        let mut messages = 0_usize;
        while let Some(transmit) = sim.poll_transmit() {
            if matches!(decode(&transmit)?, AnyMessage::LayerData(_)) {
                messages = messages.saturating_add(1);
            }
            client.handle_datagram(sim_addr(), &transmit.payload, now)?;
        }
        assert_eq!(
            messages, 4,
            "16 patches, {TERRAIN_PATCHES_PER_MESSAGE} to a message"
        );

        let received: Vec<TerrainPatch> = drain_client(&mut client)
            .into_iter()
            .filter_map(|e| match e {
                Event::TerrainPatch(patch) => Some(*patch),
                _ => None,
            })
            .collect();
        // The outer ring from the south-west corner (east, north, west,
        // south), then the inner ring the same way.
        let expected_order = vec![
            (0, 0),
            (1, 0),
            (2, 0),
            (3, 0),
            (3, 1),
            (3, 2),
            (3, 3),
            (2, 3),
            (1, 3),
            (0, 3),
            (0, 2),
            (0, 1),
            (1, 1),
            (2, 1),
            (2, 2),
            (1, 2),
        ];
        let order: Vec<(u32, u32)> = received
            .iter()
            .map(|patch| (patch.patch_x, patch.patch_y))
            .collect();
        assert_eq!(order, expected_order);
        for patch in &received {
            assert_eq!(patch.region_handle, RegionHandle(REGION_HANDLE));
            assert_eq!(patch.layer, TerrainLayerType::Land);
            assert_eq!(patch.size, PATCH_CELLS);
            let expected = patch_height(patch.patch_x, patch.patch_y);
            let corner = patch.value(0, 0).ok_or("a patch with no cells")?;
            let centre = patch.value(8, 8).ok_or("a patch with no centre")?;
            // The encoder quantizes to 2^10 levels across the patch's range.
            assert!(
                (corner - expected).abs() < 0.1 && (centre - expected).abs() < 0.1,
                "patch ({}, {}) decoded to {corner}/{centre}, not {expected}",
                patch.patch_x,
                patch.patch_y
            );
        }
        Ok(())
    }

    #[test]
    fn server_wind_and_cloud_layers_reach_the_client() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);
        drain_client(&mut client);

        // The wind layer is two patches — the X then the Y velocity component
        // of the same 16×16 field — in ONE message, both at position (0, 0),
        // which is why it goes out through `send_layer_data`.
        let mut east = land_patch(0, 0, 0.0);
        east.layer = TerrainLayerType::Wind;
        east.values = vec![1.5; 256];
        let mut north = east.clone();
        north.values = vec![-2.5; 256];
        sim.send_layer_data(TerrainLayerType::Wind, &[east, north], now)?;

        let mut clouds = land_patch(0, 0, 0.0);
        clouds.layer = TerrainLayerType::Cloud;
        clouds.values = vec![0.25; 256];
        sim.send_layer_data(TerrainLayerType::Cloud, std::slice::from_ref(&clouds), now)?;

        let mut messages = 0_usize;
        while let Some(transmit) = sim.poll_transmit() {
            if matches!(decode(&transmit)?, AnyMessage::LayerData(_)) {
                messages = messages.saturating_add(1);
            }
            client.handle_datagram(sim_addr(), &transmit.payload, now)?;
        }
        assert_eq!(messages, 2, "one message per layer");

        let received: Vec<TerrainPatch> = drain_client(&mut client)
            .into_iter()
            .filter_map(|e| match e {
                Event::TerrainPatch(patch) => Some(*patch),
                _ => None,
            })
            .collect();
        let layers: Vec<TerrainLayerType> = received.iter().map(|patch| patch.layer).collect();
        assert_eq!(
            layers,
            vec![
                TerrainLayerType::Wind,
                TerrainLayerType::Wind,
                TerrainLayerType::Cloud
            ]
        );
        let values: Vec<f32> = received
            .iter()
            .filter_map(|patch| patch.value(3, 5))
            .collect();
        let expected = [1.5_f32, -2.5, 0.25];
        for (got, want) in values.iter().zip(expected) {
            assert!((got - want).abs() < 0.05, "got {got}, wanted {want}");
        }
        assert_eq!(values.len(), 3);
        Ok(())
    }

    #[test]
    fn terrain_without_a_circuit_is_refused() -> Result<(), TestError> {
        let now = Instant::now();
        let mut sim = SimSession::new(RegionHandle(REGION_HANDLE), now);
        let patch = land_patch(0, 0, 21.0);
        assert!(matches!(
            sim.send_layer_data(TerrainLayerType::Land, std::slice::from_ref(&patch), now),
            Err(sl_proto::Error::NoCircuit)
        ));
        // Nothing to send is not an error, with or without a circuit.
        sim.send_terrain(&[], now)?;
        Ok(())
    }

    #[test]
    fn server_object_updates_round_trip() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);
        drain_client(&mut client);
        let circuit = client.root_circuit_id().ok_or("no circuit")?;

        // The client's cache-miss refetch decodes server-side.
        client.request_objects(
            &[
                ScopedObjectId::new(circuit, RegionLocalObjectId(0x10)),
                ScopedObjectId::new(circuit, RegionLocalObjectId(0x11)),
            ],
            now,
        )?;
        pump(&mut client, &mut sim, now)?;
        let server_events = drain_server(&mut sim);
        assert!(
            server_events.iter().any(|e| matches!(
                e,
                ServerEvent::RequestObjects { objects }
                    if objects.iter().map(|(id, _)| *id).collect::<Vec<_>>()
                        == vec![RegionLocalObjectId(0x10), RegionLocalObjectId(0x11)]
            )),
            "expected RequestObjects, got {server_events:?}"
        );

        // Full form: the prim arrives with its geometry, text, and scale.
        let position = sl_proto::Vector {
            x: 10.0,
            y: 20.0,
            z: 30.0,
        };
        let prim = box_prim(0x10, 0x1010, position.clone());
        sim.send_object_update(std::slice::from_ref(&prim), 0xFFFF, now)?;
        pump(&mut client, &mut sim, now)?;
        let added = drain_client(&mut client)
            .into_iter()
            .find_map(|e| match e {
                Event::ObjectAdded(object) => Some(*object),
                _ => None,
            })
            .ok_or("expected ObjectAdded for the full update")?;
        assert_eq!(added.local_id, prim.local_id);
        assert_eq!(added.full_id, prim.full_id);
        assert_eq!(added.motion.position, position);
        assert_eq!(added.scale, prim.scale);
        assert_eq!(added.shape, prim.shape);
        assert_eq!(added.text, "hover");
        assert_eq!(added.text_color, [255, 0, 0, 255]);
        assert_eq!(added.pcode, sl_proto::pcode::PRIMITIVE);

        // Compressed form: a second prim through the packed encoder.
        let second = box_prim(0x11, 0x1111, position.clone());
        sim.send_object_update_compressed(std::slice::from_ref(&second), 0xFFFF, now)?;
        pump(&mut client, &mut sim, now)?;
        let added = drain_client(&mut client)
            .into_iter()
            .find_map(|e| match e {
                Event::ObjectAdded(object) => Some(*object),
                _ => None,
            })
            .ok_or("expected ObjectAdded for the compressed update")?;
        assert_eq!(added.local_id, second.local_id);
        assert_eq!(added.full_id, second.full_id);
        assert_eq!(added.motion.position, position);
        assert_eq!(added.scale, second.scale);

        // KillObject removes both.
        sim.send_kill_object(&[RegionLocalObjectId(0x10), RegionLocalObjectId(0x11)], now)?;
        pump(&mut client, &mut sim, now)?;
        let removed: Vec<RegionLocalObjectId> = drain_client(&mut client)
            .into_iter()
            .filter_map(|e| match e {
                Event::ObjectRemoved { local_id, .. } => Some(local_id.id()),
                _ => None,
            })
            .collect();
        assert_eq!(
            removed,
            vec![RegionLocalObjectId(0x10), RegionLocalObjectId(0x11)]
        );
        Ok(())
    }

    /// The `Result` code the simulator's `AbortXfer` carries when it gives up on
    /// a stalled transfer (the reference's `LL_ERR_TCP_TIMEOUT`).
    const XFER_TIMEOUT_RESULT: i32 = -23016;

    /// Stands in for a live but idle client for `seconds`: steps the simulator's
    /// clock one second at a time, feeding it an inbound (unreliable)
    /// `StartPingCheck` each step so the inactivity timeout never fires, and
    /// acknowledging every reliable datagram it sends *except* those carrying a
    /// message named in `unacked` — the loss whose retransmission path a test is
    /// measuring. Returns every server event seen along the way.
    fn idle_sim(
        sim: &mut SimSession,
        now: Instant,
        seconds: u64,
        first_sequence: u32,
        unacked: &[&str],
    ) -> Result<Vec<ServerEvent>, TestError> {
        let mut events = Vec::new();
        for step in 1..=seconds {
            let at = after(now, step.saturating_mul(1000))?;
            let sequence =
                first_sequence.saturating_add(u32::try_from(step).unwrap_or(0).saturating_mul(8));
            let keepalive = AnyMessage::StartPingCheck(StartPingCheck {
                ping_id: StartPingCheckPingIDBlock {
                    ping_id: 200,
                    oldest_unacked: 0,
                },
            });
            sim.handle_datagram(
                client_addr(),
                &client_datagram(&keepalive, sequence, false)?,
                at,
            )?;
            sim.handle_timeout(at);
            let mut acks = Vec::new();
            let mut answers = Vec::new();
            while let Some(transmit) = sim.poll_transmit() {
                let parsed = parse_datagram(&transmit.payload)?;
                let message = decode(&transmit)?;
                if parsed.flags.contains(PacketFlags::RELIABLE)
                    && !unacked.contains(&message.name())
                {
                    acks.push(PacketAckPacketsBlock {
                        id: parsed.sequence.get(),
                    });
                }
                // Answer the simulator's own keep-alive pings, so its measured
                // round trip stays that of a responsive client.
                if let AnyMessage::StartPingCheck(ping) = message {
                    answers.push(AnyMessage::CompletePingCheck(CompletePingCheck {
                        ping_id: CompletePingCheckPingIDBlock {
                            ping_id: ping.ping_id.ping_id,
                        },
                    }));
                }
            }
            if !acks.is_empty() {
                answers.push(AnyMessage::PacketAck(PacketAck { packets: acks }));
            }
            for (offset, answer) in answers.iter().enumerate() {
                let seq = sequence
                    .saturating_add(u32::try_from(offset).unwrap_or(0))
                    .saturating_add(1);
                sim.handle_datagram(client_addr(), &client_datagram(answer, seq, false)?, at)?;
            }
            events.extend(drain_server(sim));
        }
        Ok(events)
    }

    /// A circuit's identity is fixed when it opens: a second `UseCircuitCode`
    /// naming a different agent, session or circuit code is refused and changes
    /// nothing, while a repeat of the same triple — the client re-sending a
    /// packet it believes was lost — is answered again.
    #[test]
    fn use_circuit_code_cannot_rebind_a_live_circuit() -> Result<(), TestError> {
        let now = Instant::now();
        let (_client, mut sim) = setup(now)?;
        drain_server(&mut sim);

        let rebind = AnyMessage::UseCircuitCode(UseCircuitCode {
            circuit_code: UseCircuitCodeCircuitCodeBlock {
                code: 0x7777_7777,
                session_id: uuid::Uuid::from_u128(0x999),
                id: uuid::Uuid::from_u128(0x888),
            },
        });
        sim.handle_datagram(client_addr(), &client_datagram(&rebind, 9400, false)?, now)?;
        let events = drain_server(&mut sim);
        assert_eq!(
            events,
            vec![ServerEvent::Rejected {
                message: Some("UseCircuitCode".to_owned()),
                reason: RejectionReason::CircuitRebind,
            }],
            "a foreign UseCircuitCode is refused and opens nothing"
        );

        // The circuit still belongs to the agent that opened it: a message
        // asserting the original session is still accepted.
        let sit = AnyMessage::AgentSit(sl_wire::messages::AgentSit {
            agent_data: sl_wire::messages::AgentSitAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(1),
                session_id: uuid::Uuid::from_u128(2),
            },
        });
        sim.handle_datagram(client_addr(), &client_datagram(&sit, 9401, false)?, now)?;
        assert!(
            drain_server(&mut sim)
                .iter()
                .any(|e| matches!(e, ServerEvent::SitConfirmed { .. })),
            "the original session's traffic is still accepted"
        );

        // The same triple again is the client retrying; it is answered.
        let repeat = AnyMessage::UseCircuitCode(UseCircuitCode {
            circuit_code: UseCircuitCodeCircuitCodeBlock {
                code: 0x0011_2233,
                session_id: uuid::Uuid::from_u128(2),
                id: uuid::Uuid::from_u128(1),
            },
        });
        sim.handle_datagram(client_addr(), &client_datagram(&repeat, 9402, false)?, now)?;
        assert!(
            drain_server(&mut sim)
                .iter()
                .any(|e| matches!(e, ServerEvent::CircuitOpened { .. })),
            "a repeat of the circuit's own triple is answered again"
        );
        Ok(())
    }

    /// The circuit's endpoint is claimed by the packet that opens it, not by
    /// whatever datagram happens to arrive first: traffic from an unrelated
    /// host before `UseCircuitCode` is refused and leaves the address unbound.
    #[test]
    fn only_the_opening_packet_claims_the_endpoint() -> Result<(), TestError> {
        let now = Instant::now();
        let mut sim = SimSession::new(RegionHandle(REGION_HANDLE), now);

        let stranger = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 41_000);
        let noise = AnyMessage::StartPingCheck(StartPingCheck {
            ping_id: StartPingCheckPingIDBlock {
                ping_id: 7,
                oldest_unacked: 0,
            },
        });
        sim.handle_datagram(stranger, &client_datagram(&noise, 1, false)?, now)?;
        assert_eq!(
            drain_server(&mut sim),
            vec![ServerEvent::Rejected {
                message: Some("StartPingCheck".to_owned()),
                reason: RejectionReason::NoCircuit,
            }],
            "a datagram before the circuit opens claims nothing"
        );
        assert!(
            sim.poll_transmit().is_none(),
            "with no endpoint bound there is nowhere to answer"
        );

        // The real client's `UseCircuitCode` then binds the circuit to *its*
        // address, and the stranger's traffic is ignored from then on.
        let open = AnyMessage::UseCircuitCode(UseCircuitCode {
            circuit_code: UseCircuitCodeCircuitCodeBlock {
                code: 0x0011_2233,
                session_id: uuid::Uuid::from_u128(2),
                id: uuid::Uuid::from_u128(1),
            },
        });
        sim.handle_datagram(client_addr(), &client_datagram(&open, 2, false)?, now)?;
        assert!(
            drain_server(&mut sim)
                .iter()
                .any(|e| matches!(e, ServerEvent::CircuitOpened { .. })),
            "the opening packet binds the circuit"
        );
        sim.handle_datagram(stranger, &client_datagram(&noise, 3, false)?, now)?;
        assert!(
            drain_server(&mut sim).is_empty(),
            "traffic from another address is not this circuit's"
        );
        Ok(())
    }

    /// A message asserting a session id other than the one the circuit was
    /// opened with is refused before it reaches a handler.
    #[test]
    fn a_foreign_session_id_is_refused() -> Result<(), TestError> {
        let now = Instant::now();
        let (_client, mut sim) = setup(now)?;
        drain_server(&mut sim);

        let stolen = AnyMessage::AgentSit(sl_wire::messages::AgentSit {
            agent_data: sl_wire::messages::AgentSitAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(1),
                session_id: uuid::Uuid::from_u128(0xDEAD),
            },
        });
        sim.handle_datagram(client_addr(), &client_datagram(&stolen, 9410, false)?, now)?;
        assert_eq!(
            drain_server(&mut sim),
            vec![ServerEvent::Rejected {
                message: Some("AgentSit".to_owned()),
                reason: RejectionReason::SessionIdMismatch,
            }],
            "a foreign session id never reaches the handler"
        );
        Ok(())
    }

    /// `CompleteAgentMovement` on a circuit that was never opened is refused,
    /// and leaves the agent a child rather than rooting it on a circuit whose
    /// keep-alive was never armed.
    #[test]
    fn complete_agent_movement_before_the_circuit_is_refused() -> Result<(), TestError> {
        let now = Instant::now();
        let mut sim = SimSession::new(RegionHandle(REGION_HANDLE), now);

        let complete = AnyMessage::CompleteAgentMovement(CompleteAgentMovement {
            agent_data: CompleteAgentMovementAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(1),
                session_id: uuid::Uuid::from_u128(2),
                circuit_code: 0x0011_2233,
            },
        });
        sim.handle_datagram(client_addr(), &client_datagram(&complete, 1, false)?, now)?;
        assert_eq!(
            drain_server(&mut sim),
            vec![ServerEvent::Rejected {
                message: Some("CompleteAgentMovement".to_owned()),
                reason: RejectionReason::NoCircuit,
            }],
        );
        assert_eq!(sim.agent_presence(), AgentPresence::Child);
        assert!(
            sim.poll_transmit().is_none(),
            "no AgentMovementComplete is sent for a circuit that is not open"
        );
        Ok(())
    }

    /// Closing frees the session's per-connection stores and refuses anything
    /// new, but the goodbye packet queued *before* the close still drains —
    /// that is how a clean logout's `LogoutReply` reaches the client.
    #[test]
    fn a_closed_session_drains_its_goodbye_and_frees_its_stores() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);
        drain_client(&mut client);

        sim.enqueue_caps_event(
            "EnableSimulator",
            enable_simulator_to_caps_llsd(REGION_HANDLE, sim_addr(), (256, 256)),
        );
        assert!(sim.has_caps_events());

        client.initiate_logout(now);
        while let Some(transmit) = client.poll_transmit() {
            sim.handle_datagram(client_addr(), &transmit.payload, now)?;
        }
        assert!(sim.is_closed(), "a LogoutRequest closes the session");
        assert!(
            !sim.has_caps_events(),
            "closing frees the queued CAPS events"
        );

        // The reply queued before the close still goes out...
        let mut replied = false;
        while let Some(transmit) = sim.poll_transmit() {
            if matches!(decode(&transmit)?, AnyMessage::LogoutReply(_)) {
                replied = true;
            }
        }
        assert!(
            replied,
            "the LogoutReply queued before the close still drains"
        );

        // ...and nothing new can be queued behind it.
        sim.send_alert_message("too late", &[], &[], now)?;
        assert!(
            sim.poll_transmit().is_none(),
            "a closed session queues nothing new"
        );
        assert_eq!(sim.poll_timeout(), None);
        Ok(())
    }

    /// An `Xfer` packet that is not the one the ordered stream expects is
    /// refused rather than concatenated into the file being assembled.
    #[test]
    fn an_out_of_order_xfer_packet_is_refused() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        client.request_region_terrain_upload("terrain.raw", vec![3_u8; 1200], now)?;
        pump(&mut client, &mut sim, now)?;
        drain_server(&mut sim);
        let xfer_id = sim.request_xfer_upload("terrain.raw", now)?;
        // Deliver the pull (and nothing back yet), so the client has queued its
        // first packet but the simulator has not seen it.
        while let Some(transmit) = sim.poll_transmit() {
            client.handle_datagram(sim_addr(), &transmit.payload, now)?;
        }

        // Packet 3 arrives while packet 0 is expected.
        let ahead = AnyMessage::SendXferPacket(SendXferPacket {
            xfer_id: SendXferPacketXferIDBlock {
                id: xfer_id.get(),
                packet: 3,
            },
            data_packet: SendXferPacketDataPacketBlock {
                data: vec![0xEE; 16],
            },
        });
        sim.handle_datagram(client_addr(), &client_datagram(&ahead, 9420, false)?, now)?;
        assert_eq!(
            drain_server(&mut sim),
            vec![ServerEvent::Rejected {
                message: Some("SendXferPacket".to_owned()),
                reason: RejectionReason::OutOfOrder,
            }],
        );
        assert!(
            sim.poll_transmit().is_none(),
            "an out-of-order packet is not confirmed"
        );

        // The pull is still live and still expecting packet 0: the client's own
        // stream completes normally.
        pump(&mut client, &mut sim, now)?;
        assert!(
            drain_server(&mut sim).iter().any(|e| matches!(
                e,
                ServerEvent::XferReceived { xfer_id: got, data, .. }
                    if *got == xfer_id && *data == vec![3_u8; 1200]
            )),
            "the refused packet left the assembled file untouched"
        );
        Ok(())
    }

    /// A `SendXferPacket` stream that would grow the assembled file past the
    /// simulator's ceiling is refused and the pull aborted, rather than letting
    /// a network-driven buffer grow without limit.
    #[test]
    fn an_oversized_xfer_upload_is_aborted() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        client.request_region_terrain_upload("terrain.raw", vec![1_u8; 16], now)?;
        pump(&mut client, &mut sim, now)?;
        drain_server(&mut sim);
        let xfer_id = sim.request_xfer_upload("terrain.raw", now)?;
        while sim.poll_transmit().is_some() {}

        // Stream 60 KiB a packet — the ceiling is 16 MiB, so ~280 packets reach
        // it — until the simulator refuses one.
        let mut refused = None;
        for packet in 0..400_u32 {
            let chunk = AnyMessage::SendXferPacket(SendXferPacket {
                xfer_id: SendXferPacketXferIDBlock {
                    id: xfer_id.get(),
                    packet,
                },
                data_packet: SendXferPacketDataPacketBlock {
                    data: vec![0x5A; 60_000],
                },
            });
            let sequence = 20_000_u32.saturating_add(packet);
            sim.handle_datagram(
                client_addr(),
                &client_datagram(&chunk, sequence, false)?,
                now,
            )?;
            while sim.poll_transmit().is_some() {}
            let events = drain_server(&mut sim);
            if let Some(event) = events.iter().find(|e| {
                matches!(
                    e,
                    ServerEvent::Rejected {
                        reason: RejectionReason::LimitExceeded,
                        ..
                    }
                )
            }) {
                refused = Some((packet, event.clone()));
                break;
            }
        }
        let (packet, _event) = refused.ok_or("the simulator refuses an oversized Xfer stream")?;
        assert!(
            packet > 250,
            "the ceiling is reached by the bytes streamed, not by an early refusal"
        );
        assert!(
            matches!(
                sim.abort_xfer(xfer_id, 0, now),
                Err(sl_proto::Error::UnknownXfer)
            ),
            "an oversized pull is dropped, not left half-assembled"
        );
        Ok(())
    }

    /// A sit offer the client never completes is withdrawn once the handshake
    /// times out, instead of leaving the machine half-done for the session's
    /// life.
    #[test]
    fn an_unanswered_sit_offer_expires() -> Result<(), TestError> {
        let now = Instant::now();
        let (_client, mut sim) = setup(now)?;
        drain_server(&mut sim);

        let target = ObjectKey::from(uuid::Uuid::from_u128(0x51D));
        let transform = SitTransform {
            autopilot: false,
            sit_position: sl_types::lsl::Vector {
                x: 0.0,
                y: 0.0,
                z: 0.5,
            },
            sit_rotation: sl_types::lsl::Rotation {
                x: 0.0,
                y: 0.0,
                z: 0.0,
                s: 1.0,
            },
            camera_eye_offset: sl_types::lsl::Vector {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            camera_at_offset: sl_types::lsl::Vector {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            force_mouselook: false,
        };
        sim.send_avatar_sit_response(target, &transform, now)?;
        while sim.poll_transmit().is_some() {}
        assert_eq!(sim.seated_on(), None, "the offer is not a seat yet");

        let events = idle_sim(&mut sim, now, 16, 9500, &[])?;
        assert!(
            events.contains(&ServerEvent::SitOfferExpired { on: target }),
            "expected SitOfferExpired, got {events:?}"
        );
        assert_eq!(sim.seated_on(), None);
        Ok(())
    }

    /// A `TransferRequest` the driver never serves is answered as unservable and
    /// dropped, rather than parked for the session's life while the client
    /// waits.
    #[test]
    fn an_unserved_transfer_request_expires() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        let transfer_id = client.fetch_task_item_asset(
            ObjectKey::from(uuid::Uuid::from_u128(0xAAAA)),
            InventoryKey::from(uuid::Uuid::from_u128(0xBBBB)),
            AssetKey::from(uuid::Uuid::from_u128(0xCCCC)),
            AssetType::ScriptText,
            now,
        )?;
        pump(&mut client, &mut sim, now)?;
        drain_server(&mut sim);

        let events = idle_sim(&mut sim, now, 61, 9600, &[])?;
        assert!(
            events.contains(&ServerEvent::TransferServeExpired { transfer_id }),
            "expected TransferServeExpired, got {events:?}"
        );
        assert!(
            matches!(
                sim.send_transfer_asset(transfer_id, b"late", now),
                Err(sl_proto::Error::UnknownTransfer)
            ),
            "an expired request is no longer awaiting an answer"
        );
        Ok(())
    }

    /// An inbound `Xfer` pull that goes quiet is abandoned and the client told,
    /// rather than holding its partial buffer forever.
    #[test]
    fn a_stalled_xfer_pull_is_abandoned() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        client.request_region_terrain_upload("terrain.raw", vec![5_u8; 2500], now)?;
        pump(&mut client, &mut sim, now)?;
        drain_server(&mut sim);
        let xfer_id = sim.request_xfer_upload("terrain.raw", now)?;
        while sim.poll_transmit().is_some() {}

        let events = idle_sim(&mut sim, now, 31, 9700, &[])?;
        assert!(
            events.contains(&ServerEvent::XferAborted {
                xfer_id,
                result: XFER_TIMEOUT_RESULT,
            }),
            "expected a timed-out XferAborted, got {events:?}"
        );
        assert!(
            matches!(
                sim.abort_xfer(xfer_id, 0, now),
                Err(sl_proto::Error::UnknownXfer)
            ),
            "the abandoned pull is gone"
        );
        Ok(())
    }

    /// A datagram still sitting in the outbound queue has not started its
    /// retransmission clock: a driver that falls behind must not turn its own
    /// backlog into a burst of retransmissions.
    #[test]
    fn a_queued_datagram_starts_its_clock_only_once_drained() -> Result<(), TestError> {
        let now = Instant::now();
        let (_client, mut sim) = setup(now)?;
        while sim.poll_transmit().is_some() {}
        drain_server(&mut sim);

        sim.send_alert_message("undrained", &[], &[], now)?;
        // Long past the timeout, but never handed to the driver.
        sim.handle_timeout(after(now, 20_000)?);
        let mut alerts = Vec::new();
        while let Some(transmit) = sim.poll_transmit() {
            if matches!(decode(&transmit)?, AnyMessage::AlertMessage(_)) {
                alerts.push(parse_datagram(&transmit.payload)?.flags);
            }
        }
        assert_eq!(alerts.len(), 1, "a queued datagram is never retransmitted");
        assert!(
            !alerts
                .first()
                .ok_or("the queued alert")?
                .contains(PacketFlags::RESENT),
            "the first transmission is not a resend"
        );

        // Now that it has left, the clock runs and it is retransmitted.
        sim.handle_timeout(after(now, 25_200)?);
        let mut resent = false;
        while let Some(transmit) = sim.poll_transmit() {
            if matches!(decode(&transmit)?, AnyMessage::AlertMessage(_)) {
                resent = parse_datagram(&transmit.payload)?
                    .flags
                    .contains(PacketFlags::RESENT);
            }
        }
        assert!(resent, "the drained datagram's clock started at the drain");
        Ok(())
    }

    /// A measured slow round trip widens the retransmission timeout past the
    /// five seconds a circuit that has measured nothing waits.
    #[test]
    fn a_slow_round_trip_widens_the_simulator_resend_timeout() -> Result<(), TestError> {
        let now = Instant::now();
        let (_client, mut sim) = setup(now)?;
        while sim.poll_transmit().is_some() {}
        drain_server(&mut sim);

        // A ping answered 1.5 s later: the average (and with it the timeout)
        // grows to match.
        let pinged = after(now, 1_000)?;
        let ping_id = sim
            .start_ping_check(pinged)?
            .ok_or("the circuit is open, so a ping goes out")?;
        while sim.poll_transmit().is_some() {}
        let answer = AnyMessage::CompletePingCheck(CompletePingCheck {
            ping_id: CompletePingCheckPingIDBlock {
                ping_id: ping_id.get(),
            },
        });
        let answered = after(now, 2_500)?;
        sim.handle_datagram(
            client_addr(),
            &client_datagram(&answer, 9800, false)?,
            answered,
        )?;

        let sent = after(now, 2_600)?;
        sim.send_alert_message("slow link", &[], &[], sent)?;
        while sim.poll_transmit().is_some() {}

        // 5.4 s after the send — past the unmeasured five-second timeout, but
        // inside the 7.5 s the measured round trip earns it.
        sim.handle_timeout(after(now, 8_000)?);
        let mut resent = false;
        while let Some(transmit) = sim.poll_transmit() {
            if matches!(decode(&transmit)?, AnyMessage::AlertMessage(_)) {
                resent = true;
            }
        }
        assert!(
            !resent,
            "a measured slow link waits longer before resending"
        );

        sim.handle_timeout(after(now, 10_200)?);
        while let Some(transmit) = sim.poll_transmit() {
            if matches!(decode(&transmit)?, AnyMessage::AlertMessage(_)) {
                resent = parse_datagram(&transmit.payload)?
                    .flags
                    .contains(PacketFlags::RESENT);
            }
        }
        assert!(resent, "past the widened timeout it is retransmitted");
        Ok(())
    }

    /// Only a packet that establishes the agent's presence is fatal: an
    /// exhausted alert is reported and leaves the session running, while an
    /// exhausted `AgentMovementComplete` closes it.
    #[test]
    fn only_a_session_critical_give_up_closes_the_session() -> Result<(), TestError> {
        let now = Instant::now();
        let (_client, mut sim) = setup(now)?;
        while sim.poll_transmit().is_some() {}
        drain_server(&mut sim);

        // An alert nobody ever acknowledges: reported, and the session lives.
        sim.send_alert_message("unacked", &[], &[], now)?;
        let events = idle_sim(&mut sim, now, 30, 9900, &["AlertMessage"])?;
        assert!(
            events.contains(&ServerEvent::ReliableGiveUp {
                message: Some("AlertMessage".to_owned()),
            }),
            "expected a give-up report for the alert, got {events:?}"
        );
        assert!(
            !sim.is_closed(),
            "one lost alert does not tear the circuit down"
        );

        // The movement completion is another matter: the client never arrives.
        let mut fresh = SimSession::new(RegionHandle(REGION_HANDLE), now);
        let open = AnyMessage::UseCircuitCode(UseCircuitCode {
            circuit_code: UseCircuitCodeCircuitCodeBlock {
                code: 0x0011_2233,
                session_id: uuid::Uuid::from_u128(2),
                id: uuid::Uuid::from_u128(1),
            },
        });
        fresh.handle_datagram(client_addr(), &client_datagram(&open, 1, false)?, now)?;
        let complete = AnyMessage::CompleteAgentMovement(CompleteAgentMovement {
            agent_data: CompleteAgentMovementAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(1),
                session_id: uuid::Uuid::from_u128(2),
                circuit_code: 0x0011_2233,
            },
        });
        fresh.handle_datagram(client_addr(), &client_datagram(&complete, 2, false)?, now)?;
        while fresh.poll_transmit().is_some() {}
        drain_server(&mut fresh);
        let events = idle_sim(&mut fresh, now, 30, 20_000, &["AgentMovementComplete"])?;
        assert!(
            events.contains(&ServerEvent::Disconnected),
            "an unacknowledged AgentMovementComplete fails the session, got {events:?}"
        );
        Ok(())
    }
}
