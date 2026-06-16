//! Scripted-peer, simulated-clock tests for the full session lifecycle:
//! login -> circuit -> handshake -> keep-alive -> logout.

#[cfg(test)]
mod test {
    use std::error::Error;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::time::{Duration, Instant};

    use pretty_assertions::{assert_eq, assert_ne};
    use sl_proto::{
        DisconnectReason, Event, LoginParams, Maturity, ProductType, Reliability, Session, Transmit,
    };
    use sl_types::lsl::Vector;
    use sl_wire::messages::{
        LogoutRequest, LogoutRequestAgentDataBlock, MapBlockReply, MapBlockReplyAgentDataBlock,
        MapBlockReplyDataBlock, MapBlockReplySizeBlock, ParcelProperties,
        ParcelPropertiesAgeVerificationBlockBlock, ParcelPropertiesParcelDataBlock,
        ParcelPropertiesParcelEnvironmentBlockBlock, ParcelPropertiesRegionAllowAccessBlockBlock,
        RegionHandshake, RegionHandshakeRegionInfo2Block, RegionHandshakeRegionInfo3Block,
        RegionHandshakeRegionInfoBlock, RegionInfo, RegionInfoAgentDataBlock,
        RegionInfoRegionInfo2Block, RegionInfoRegionInfoBlock, TeleportFailed,
        TeleportFailedInfoBlock,
    };
    use sl_wire::{
        AnyMessage, LoginFailure, LoginRequest, LoginResponse, LoginSuccess, MessageId,
        PacketFlags, Reader, Writer, encode_datagram, parse_datagram,
    };

    /// A boxed test error.
    type TestError = Box<dyn Error>;

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

        // A ParcelProperties event as delivered over the CAPS event queue.
        let xml = "<llsd><map><key>ParcelData</key><array><map>\
            <key>LocalID</key><integer>3</integer>\
            <key>SequenceID</key><integer>9</integer>\
            <key>Area</key><integer>2048</integer>\
            <key>ParcelFlags</key><integer>64</integer>\
            <key>MaxPrims</key><integer>750</integer>\
            <key>AABBMax</key><array><real>32</real><real>16</real><real>0</real></array>\
            <key>Bitmap</key><binary>AQID</binary>\
            </map></array></map></llsd>";
        let body = sl_proto::parse_llsd_xml(xml)?;
        session.handle_caps_event("ParcelProperties", &body);

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
        // ParcelFlags 64 = CREATE_OBJECTS.
        assert!(parcel.create_objects());
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
}
