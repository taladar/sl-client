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
        AbuseReport, AbuseReportType, AgentKey, AlertInfo, AttachmentMode, AttachmentPoint,
        AvatarName, AvatarPickerResult, ChatChannel, ChatSource, ChatType, ClassifiedCategory,
        ClassifiedKey, CoarseLocation, ControlFlags, DetachOrder, DirClassifiedResult,
        DirEventResult, DirFindFlags, DirGroupResult, DirLandResult, DirPeopleResult,
        DirPlaceResult, EstateCovenant, Event, EventId, EventInfo, FollowCamProperty,
        FollowCamPropertyValue, GestureActivation, GlobalCoordinates, GridCoordinates,
        GridRectangle, GroupAccountDetails, GroupAccountDetailsEntry, GroupAccountSummary,
        GroupAccountTransaction, GroupAccountTransactions, GroupActiveProposalItem, GroupKey,
        GroupName, GroupVote, GroupVoteHistoryItem, ImDialog, InventoryFolderKey, InventoryKey,
        LandArea, LandSearchType, LandStatItem, LandStatReportType, LindenAmount, LindenBalance,
        LoginParams, MapItem, MapItemType, MapLayer, MapRegionInfo, MapRequestFlags, Maturity,
        MeanCollision, MeanCollisionType, MovementMode, NotecardRez, ObjectBuyItem, ObjectKey,
        ObjectPropertiesFamily, ParcelCategory, ParcelDetails, ParcelKey, ParcelObjectOwner,
        ParcelReturnType, Permissions5, PingId, PlacesResult, PointAtType, Postcard, ProductType,
        RegionCoordinates, RegionHandle, RegionIdentity, RegionLocalObjectId, RegionLocalParcelId,
        RestoreItem, RezAttachment, SaleType, ScopedObjectId, ScopedParcelId, ScriptControl,
        ScriptControlAction, ServerEvent, Session, SimSession, TelehubInfo, TextureKey, Throttle,
        Transmit, ViewerEffect, ViewerEffectData, ViewerEffectType, enable_simulator_to_caps_llsd,
        parse_event_queue_response,
    };
    use sl_wire::messages::{StartPingCheck, StartPingCheckPingIDBlock};
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

    /// Delivers all queued datagrams between the client and simulator (in both
    /// directions) until neither has anything more to send.
    fn pump(client: &mut Session, sim: &mut SimSession, now: Instant) -> Result<(), TestError> {
        loop {
            let mut moved = false;
            while let Some(transmit) = client.poll_transmit() {
                sim.handle_datagram(client_addr(), &transmit.payload, now)?;
                moved = true;
            }
            while let Some(transmit) = sim.poll_transmit() {
                client.handle_datagram(sim_addr(), &transmit.payload, now)?;
                moved = true;
            }
            if !moved {
                break;
            }
        }
        Ok(())
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

    /// Logs a client in and drives both peers through circuit setup and arrival,
    /// returning the active pair.
    fn setup(now: Instant) -> Result<(Session, SimSession), TestError> {
        let mut client = new_client()?;
        client.handle_login_response(success()?, now)?;
        let mut sim = SimSession::new(RegionHandle(REGION_HANDLE), now);
        pump(&mut client, &mut sim, now)?;
        Ok((client, sim))
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
        client.remove_attachment(AttachmentPoint::Skull, item, now)?;
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
        client.rez_attachments(compound, DetachOrder::Keep, &attachments, now)?;
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
        client.find_agent(hunter, prey, now)?;
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
            qid,
            "alice",
            DirFindFlags::PEOPLE.union(DirFindFlags::ONLINE),
            0,
            now,
        )?;
        client.dir_places_query(
            qid,
            "sandbox",
            DirFindFlags::INC_PG,
            ParcelCategory::Commercial,
            "Region",
            10,
            now,
        )?;
        client.dir_land_query(
            qid,
            DirFindFlags::FOR_SALE.union(DirFindFlags::LIMIT_BY_PRICE),
            LandSearchType::MAINLAND,
            5000,
            512,
            0,
            now,
        )?;
        client.dir_classified_query(
            qid,
            "shoes",
            DirFindFlags::INC_MATURE,
            ClassifiedCategory::PropertyRental,
            0,
            now,
        )?;
        client.avatar_picker_request(qid, "bob", now)?;
        client.places_query(
            qid,
            txn,
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
            uuid::Uuid::from_u128(0x17E),
            uuid::Uuid::nil(),
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
            &[uuid::Uuid::from_u128(0x99)],
            &[ObjectKey::from(uuid::Uuid::from_u128(0xAB))],
            now,
        )?;
        client.request_parcel_info(ParcelKey::from(uuid::Uuid::from_u128(0x00C0_FFEE)), now)?;
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
        client.request_script_running(object_id, item_id, now)?;
        client.set_script_running(object_id, item_id, true, now)?;
        client.reset_script(object_id, item_id, now)?;
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
        client.request_group_account_summary(GroupKey::from(group_id), request_id, 60, 0, now)?;
        client.request_group_account_details(GroupKey::from(group_id), request_id, 60, 0, now)?;
        client.request_group_account_transactions(
            GroupKey::from(group_id),
            request_id,
            60,
            0,
            now,
        )?;
        client.request_group_active_proposals(GroupKey::from(group_id), transaction_id, now)?;
        client.request_group_vote_history(GroupKey::from(group_id), transaction_id, now)?;
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
        client.deactivate_gestures(&[item_b], now)?;
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
            enable_simulator_to_caps_llsd(REGION_HANDLE, sim_addr()),
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
        client.request_avatar_names(&[alice], now)?;
        client.request_group_names(&[club], now)?;
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
}
