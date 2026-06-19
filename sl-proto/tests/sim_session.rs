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
        ChatType, Event, ImDialog, LoginParams, MapItem, MapItemType, MapRegionInfo, Maturity,
        ServerEvent, Session, SimSession, Throttle, Transmit, enable_simulator_to_caps_llsd,
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
}
