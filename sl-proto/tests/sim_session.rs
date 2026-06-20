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
        AttachmentPoint, AvatarName, ChatType, CoarseLocation, Event, GroupName, ImDialog,
        LoginParams, MapItem, MapItemType, MapRegionInfo, Maturity, PointAtType, ProductType,
        RegionIdentity, RezAttachment, ServerEvent, Session, SimSession, Throttle, Transmit,
        ViewerEffect, ViewerEffectData, ViewerEffectType, enable_simulator_to_caps_llsd,
        grid_to_handle, parse_event_queue_response,
    };
    use sl_wire::messages::{StartPingCheck, StartPingCheckPingIDBlock};
    use sl_wire::{
        AnyMessage, LoginRequest, LoginResponse, LoginSuccess, MessageId, PacketFlags, Reader,
        Writer, encode_datagram, parse_datagram,
    };

    /// A boxed test error.
    type TestError = Box<dyn Error>;

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
    fn new_client() -> Session {
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
        }))
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
        Ok(encode_datagram(flags, sequence, &writer.into_bytes()))
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
        let mut client = new_client();
        client.handle_login_response(success(), now)?;
        let mut sim = SimSession::new(REGION_HANDLE, now);
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
                } if *agent_id == uuid::Uuid::from_u128(1)
                    && *session_id == uuid::Uuid::from_u128(2)
                    && *circuit_code == 0x0011_2233
            )),
            "expected CircuitOpened, got {server_events:?}"
        );
        assert!(
            server_events
                .iter()
                .any(|e| matches!(e, ServerEvent::AgentArrived)),
            "expected AgentArrived, got {server_events:?}"
        );
        assert_eq!(sim.agent_id(), Some(uuid::Uuid::from_u128(1)));
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

        client.say("hello sim", ChatType::Shout, 7, now)?;
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
        assert_eq!(chat, ("hello sim".to_owned(), 7, ChatType::Shout));
        Ok(())
    }

    #[test]
    fn client_attach_object_reaches_simulator() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);

        client.attach_object(
            55,
            AttachmentPoint::RightHand,
            true,
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
                    add,
                    ..
                } => Some((*local_id, *attachment_point, *add)),
                _ => None,
            })
            .ok_or("expected an AttachObject server event")?;
        assert_eq!(attach, (55, AttachmentPoint::RightHand, true));
        Ok(())
    }

    #[test]
    fn client_detach_objects_reaches_simulator() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);

        client.detach_objects(&[3, 4], now)?;
        pump(&mut client, &mut sim, now)?;

        let events = drain_server(&mut sim);
        let ids = events
            .iter()
            .find_map(|e| match e {
                ServerEvent::DetachObjects(ids) => Some(ids.clone()),
                _ => None,
            })
            .ok_or("expected a DetachObjects server event")?;
        assert_eq!(ids, vec![3, 4]);
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
            item_id: uuid::Uuid::from_u128(0x9002),
            owner_id: uuid::Uuid::from_u128(0x9000),
            attachment_point: AttachmentPoint::LeftHand,
            add: true,
            name: String::new(),
            description: String::new(),
        }];
        client.rez_attachments(compound, false, &attachments, now)?;
        pump(&mut client, &mut sim, now)?;

        let events = drain_server(&mut sim);
        let rez = events
            .iter()
            .find_map(|e| match e {
                ServerEvent::RezAttachments {
                    compound_id,
                    first_detach_all,
                    attachments,
                } => Some((*compound_id, *first_detach_all, attachments.clone())),
                _ => None,
            })
            .ok_or("expected a RezAttachments server event")?;
        assert_eq!(rez.0, compound);
        assert!(!rez.1);
        let first = rez.2.first().ok_or("first attachment")?;
        assert_eq!(first.attachment_point, AttachmentPoint::LeftHand);
        assert!(first.add);
        assert_eq!(first.item_id, uuid::Uuid::from_u128(0x9002));
        Ok(())
    }

    #[test]
    fn client_viewer_effect_round_trips() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_server(&mut sim);

        let source = uuid::Uuid::from_u128(0xA00);
        let data = ViewerEffectData::PointAt {
            source,
            target: uuid::Uuid::from_u128(0xA01),
            target_position: [1.0, 2.0, 3.0],
            point_at_type: PointAtType::Grab,
        };
        client.send_viewer_effect(
            &[ViewerEffect {
                id: uuid::Uuid::from_u128(0xA0F),
                agent_id: source,
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
        client.track_agent(prey, now)?;
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
        assert_eq!(tracked, prey);
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
                    agent_id: me,
                    x: 100,
                    y: 50,
                    z: 80, // sent as 80/4 = 20 on the wire, decoded back to 80
                },
                CoarseLocation {
                    agent_id: other,
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
        assert_eq!(first.agent_id, me);
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
    fn simulator_chat_reaches_client() -> Result<(), TestError> {
        let now = Instant::now();
        let (mut client, mut sim) = setup(now)?;
        drain_client(&mut client);

        sim.send_chat_from_simulator(
            "Region",
            uuid::Uuid::nil(),
            uuid::Uuid::nil(),
            0,
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
        client.send_instant_message(target, "psst", now)?;
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
        assert_eq!(im.to_agent_id, target);
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
            events
                .iter()
                .any(|e| matches!(e, ServerEvent::PingRequested { ping_id: 0x2A })),
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
        let mut sim = SimSession::new(REGION_HANDLE, now);
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

        // A MapBlockRequest is a client message with no dedicated ServerEvent
        // variant; it must be surfaced verbatim as ClientMessage.
        let request = AnyMessage::MapBlockRequest(sl_wire::messages::MapBlockRequest {
            agent_data: sl_wire::messages::MapBlockRequestAgentDataBlock {
                agent_id: uuid::Uuid::from_u128(1),
                session_id: uuid::Uuid::from_u128(2),
                flags: 0,
                estate_id: 0,
                godlike: false,
            },
            position_data: sl_wire::messages::MapBlockRequestPositionDataBlock {
                min_x: 1000,
                max_x: 1001,
                min_y: 1000,
                max_y: 1001,
            },
        });
        let datagram = client_datagram(&request, 600, false)?;
        sim.handle_datagram(client_addr(), &datagram, now)?;

        let events = drain_server(&mut sim);
        assert!(
            events.iter().any(|e| matches!(
                e,
                ServerEvent::ClientMessage(message) if matches!(**message, AnyMessage::MapBlockRequest(_))
            )),
            "expected a ClientMessage(MapBlockRequest), got {events:?}"
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
                name: "Standard".to_owned(),
                grid_x: 1000,
                grid_y: 1000,
                region_handle: grid_to_handle(1000, 1000),
                maturity: Maturity::Mature,
                region_flags: 0x0000_0345,
                size_x: 256,
                size_y: 256,
                agents: 3,
                water_height: 20,
                map_image_id: uuid::Uuid::from_u128(0xABCD),
            },
            MapRegionInfo {
                name: "Variable".to_owned(),
                grid_x: 1100,
                grid_y: 1200,
                region_handle: grid_to_handle(1100, 1200),
                maturity: Maturity::Adult,
                region_flags: 0x0000_0007,
                size_x: 512,
                size_y: 512,
                agents: 0,
                water_height: 25,
                map_image_id: uuid::Uuid::from_u128(0x1234),
            },
        ];
        sim.send_map_block_reply(2, &regions, now)?;
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
                global_x: 256_000,
                global_y: 256_128,
                id: uuid::Uuid::nil(),
                extra: 4,
                extra2: 0,
                name: "dots".to_owned(),
            },
            MapItem {
                global_x: 257_000,
                global_y: 256_200,
                id: uuid::Uuid::from_u128(0x55AA),
                extra: 1024,
                extra2: 250,
                name: "Parcel For Sale".to_owned(),
            },
        ];
        sim.send_map_item_reply(2, MapItemType::AgentLocations, &items, now)?;
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
    fn send_region_handshake_encodes_the_identity() -> Result<(), TestError> {
        let now = Instant::now();
        let (_client, mut sim) = setup(now)?;

        let identity = RegionIdentity {
            sim_name: "Server Region".to_owned(),
            region_id: uuid::Uuid::from_u128(0xBEEF),
            // Grid coordinates / handle are not wire fields of the handshake.
            region_handle: 0,
            grid_x: 0,
            grid_y: 0,
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
                id: alice,
                first_name: "Alice".to_owned(),
                last_name: "Liddell".to_owned(),
            }],
            now,
        )?;
        sim.send_group_names(
            &[GroupName {
                id: club,
                name: "The Club".to_owned(),
            }],
            now,
        )?;
        pump(&mut client, &mut sim, now)?;

        let client_events = drain_client(&mut client);
        let avatar = client_events
            .iter()
            .find_map(|event| match event {
                Event::AvatarNames(names) => names.iter().find(|name| name.id == alice),
                _ => None,
            })
            .ok_or("expected the avatar name on the client")?;
        assert_eq!(avatar.legacy_name(), "Alice Liddell");
        let group = client_events
            .iter()
            .find_map(|event| match event {
                Event::GroupNames(names) => names.iter().find(|name| name.id == club),
                _ => None,
            })
            .ok_or("expected the group name on the client")?;
        assert_eq!(group.name, "The Club");
        Ok(())
    }
}
