//! Scripted-peer, simulated-clock tests for the full session lifecycle:
//! login -> circuit -> handshake -> keep-alive -> logout.

#[cfg(test)]
mod test {
    use std::error::Error;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::time::{Duration, Instant};

    use pretty_assertions::assert_eq;
    use sl_proto::{DisconnectReason, Event, LoginParams, Reliability, Session, Transmit};
    use sl_wire::messages::{LogoutRequest, LogoutRequestAgentDataBlock};
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
        assert_eq!(
            drain_events(&mut session),
            vec![Event::RegionHandshakeComplete]
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
}
