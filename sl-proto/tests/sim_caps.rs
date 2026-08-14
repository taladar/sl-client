//! In-memory loopback tests of the server-side CAPS core: the client's own
//! seed/event-queue builders and parsers driven against [`SimCaps::dispatch`]
//! and a [`SimSession`]'s event buffer — the CAPS mirror of the UDP loopback
//! in `tests/sim_session.rs`, with no HTTP transport involved.

#[cfg(test)]
mod test {
    use std::error::Error;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::time::{Duration, Instant};

    use pretty_assertions::assert_eq;
    use sl_proto::{
        AbuseReport, AbuseReportType, AgentKey, AgentPreferences, CAP_CHAT_SESSION_REQUEST,
        CAP_READ_OFFLINE_MSGS, CHAT_SESSION_ACCEPT, CHAT_SESSION_DECLINE,
        CHAT_SESSION_DECLINE_P2P_VOICE, CHAT_SESSION_FETCH_HISTORY, CHAT_SESSION_FETCH_HISTORY_TAG,
        CapsDispatch, CapsRequest, ChatSessionKind, DisplayName, Event, ImDialog, ImSessionId,
        InstantMessage, LLSD_XML_CONTENT_TYPE, LoginParams, REQUESTED_CAPABILITIES,
        RegionCoordinates, RegionHandle, ServerEvent, ServerHistoryMessage, Session, SimCaps,
        SimChatSessionKind, SimSession, StartLocation, build_event_queue_request,
        build_seed_request, chat_session_request_body, enable_simulator_to_caps_llsd,
        parse_event_queue_response, parse_seed_response,
    };
    use sl_wire::{
        CircuitCode, Llsd, LoginRequest, LoginResponse, LoginSuccess,
        build_agent_preferences_request, build_send_user_report, display_names_query,
        parse_agent_preferences, parse_asset_upload_response, parse_display_names, parse_llsd_xml,
    };

    /// A boxed test error.
    type TestError = Box<dyn Error>;

    /// The region handle the simulator serves throughout these tests.
    const REGION_HANDLE: u64 = 0x0000_03e8_0000_03e8;

    /// The simulator's UDP address (for event bodies that carry one).
    fn sim_addr() -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 9000)
    }

    /// A fresh simulator session (the event-queue buffer needs no circuit).
    fn new_sim() -> SimSession {
        SimSession::new(RegionHandle(REGION_HANDLE), Instant::now())
    }

    /// A [`SimCaps`] with a deterministic token mint.
    fn new_caps() -> Result<SimCaps, TestError> {
        let base: url::Url = "http://127.0.0.1:9001/".parse()?;
        let mut next: u128 = 0;
        let mint = move || {
            next = next.wrapping_add(1);
            uuid::Uuid::from_u128(next)
        };
        Ok(SimCaps::new(base, uuid::Uuid::from_u128(0x5eed), mint))
    }

    /// A `POST` [`CapsRequest`] carrying an LLSD-XML body.
    fn post<'a>(path: &'a str, body: &'a str) -> CapsRequest<'a> {
        CapsRequest {
            method: "POST",
            path,
            query: None,
            body: body.as_bytes(),
        }
    }

    /// Dispatches and unwraps an immediate response, failing on would-block.
    fn respond(
        caps: &mut SimCaps,
        sim: &mut SimSession,
        request: &CapsRequest<'_>,
    ) -> Result<(u16, String), TestError> {
        match caps.dispatch(sim, request) {
            CapsDispatch::Response(response) => {
                Ok((response.status, String::from_utf8(response.body.clone())?))
            }
            CapsDispatch::EventQueueWouldBlock => Err("unexpected would-block".into()),
        }
    }

    #[test]
    fn seed_round_trips_against_the_client_builders() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();

        // The exact request the client runtime POSTs to the seed URL.
        let request_body = build_seed_request(REQUESTED_CAPABILITIES);
        let seed_path = caps.seed_url().path().to_owned();
        let (status, body) = respond(&mut caps, &mut sim, &post(&seed_path, &request_body))?;
        assert_eq!(status, 200);

        // The client's own parser reads the grant; only the served
        // capabilities come back, with the URLs `grant` mints.
        let granted = parse_seed_response(&body)?;
        let requested: Vec<String> = REQUESTED_CAPABILITIES
            .iter()
            .map(|name| (*name).to_owned())
            .collect();
        let expected = caps.grant(&requested);
        assert_eq!(granted, expected);
        assert_eq!(granted.len(), 7);
        Ok(())
    }

    #[test]
    fn seed_grant_is_idempotent_across_retries() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        let request_body = build_seed_request(REQUESTED_CAPABILITIES);
        let seed_path = caps.seed_url().path().to_owned();
        let first = respond(&mut caps, &mut sim, &post(&seed_path, &request_body))?;
        // The reference viewer retries the seed POST up to 30 times; every
        // retry must receive a byte-identical grant.
        let second = respond(&mut caps, &mut sim, &post(&seed_path, &request_body))?;
        assert_eq!(first, second);
        Ok(())
    }

    #[test]
    fn unsupported_caps_are_omitted_from_the_grant() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        let request_body = build_seed_request(&["EventQueueGet", "GetTexture", "NoSuchCapability"]);
        let seed_path = caps.seed_url().path().to_owned();
        let (status, body) = respond(&mut caps, &mut sim, &post(&seed_path, &request_body))?;
        assert_eq!(status, 200);
        let granted = parse_seed_response(&body)?;
        assert!(granted.contains_key("EventQueueGet"));
        assert!(!granted.contains_key("GetTexture"));
        assert!(!granted.contains_key("NoSuchCapability"));
        Ok(())
    }

    /// Grants the named capability and returns its URL path.
    fn granted_cap_path(caps: &SimCaps, name: &str) -> Result<String, TestError> {
        let granted = caps.grant(&[name.to_owned()]);
        let url: url::Url = granted
            .get(name)
            .ok_or_else(|| format!("{name} not granted"))?
            .parse()?;
        Ok(url.path().to_owned())
    }

    /// Grants the event queue and returns its URL path.
    fn granted_event_queue_path(caps: &SimCaps) -> Result<String, TestError> {
        granted_cap_path(caps, "EventQueueGet")
    }

    /// A `GET` [`CapsRequest`] carrying a query string (no body).
    fn get<'a>(path: &'a str, query: Option<&'a str>) -> CapsRequest<'a> {
        CapsRequest {
            method: "GET",
            path,
            query,
            body: b"",
        }
    }

    /// A fresh client [`Session`] whose CAPS folds the loopback tests drive
    /// (never logged in — [`Session::handle_caps_event`] needs no circuit).
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

    /// Drains the client's pending events.
    fn drain_client(client: &mut Session) -> Vec<Event> {
        let mut events = Vec::new();
        while let Some(event) = client.poll_event() {
            events.push(event);
        }
        events
    }

    /// A client [`Session`] that has processed a successful login (so it
    /// knows its own agent id — some CAPS folds resolve sessions through it)
    /// but has no live circuit.
    fn logged_in_client(now: Instant) -> Result<Session, TestError> {
        let mut client = new_client()?;
        client.handle_login_response(
            LoginResponse::Success(Box::new(LoginSuccess::minimal(
                AgentKey::from(uuid::Uuid::from_u128(1)),
                uuid::Uuid::from_u128(2),
                uuid::Uuid::from_u128(3),
                CircuitCode(0x0011_2233),
                Ipv4Addr::new(127, 0, 0, 1),
                9000,
                "http://127.0.0.1:9000/seed".parse()?,
            ))),
            now,
        )?;
        drain_client(&mut client);
        Ok(client)
    }

    #[test]
    fn event_queue_full_poll_cycle() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        let eq_path = granted_event_queue_path(&caps)?;

        // A first poll (ack undef) with one queued event delivers batch 1.
        sim.enqueue_caps_event(
            "EnableSimulator",
            enable_simulator_to_caps_llsd(REGION_HANDLE, sim_addr()),
        );
        let poll = build_event_queue_request(None, false);
        let (status, body) = respond(&mut caps, &mut sim, &post(&eq_path, &poll))?;
        assert_eq!(status, 200);
        let batch = parse_event_queue_response(&body)?;
        assert_eq!(batch.id, 1);
        assert_eq!(
            batch.events.first().map(|event| event.message.as_str()),
            Some("EnableSimulator")
        );

        // The client re-polls acking batch 1; nothing is queued, so the
        // long-poll would block (the runtime holds it open).
        let ack_poll = build_event_queue_request(Some(batch.id), false);
        assert_eq!(
            caps.dispatch(&mut sim, &post(&eq_path, &ack_poll)),
            CapsDispatch::EventQueueWouldBlock
        );

        // A later event releases the next poll as batch 2.
        sim.enqueue_caps_event(
            "EnableSimulator",
            enable_simulator_to_caps_llsd(REGION_HANDLE, sim_addr()),
        );
        let (status, body) = respond(&mut caps, &mut sim, &post(&eq_path, &ack_poll))?;
        assert_eq!(status, 200);
        assert_eq!(parse_event_queue_response(&body)?.id, 2);
        Ok(())
    }

    #[test]
    fn empty_poll_would_block_and_times_out_as_502() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        let eq_path = granted_event_queue_path(&caps)?;
        let poll = build_event_queue_request(None, false);
        assert_eq!(
            caps.dispatch(&mut sim, &post(&eq_path, &poll)),
            CapsDispatch::EventQueueWouldBlock
        );
        // The runtime's hold expires: the 502 is what the reference viewer
        // treats as "nothing yet, re-poll".
        assert_eq!(caps.event_queue_timeout().status, 502);
        Ok(())
    }

    #[test]
    fn done_poll_tears_the_queue_down() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        let eq_path = granted_event_queue_path(&caps)?;

        let teardown = build_event_queue_request(Some(1), true);
        let (status, _) = respond(&mut caps, &mut sim, &post(&eq_path, &teardown))?;
        assert_eq!(status, 200);

        // Every later poll answers 404 — the client's "stop polling" signal
        // — even with events queued.
        sim.enqueue_caps_event(
            "EnableSimulator",
            enable_simulator_to_caps_llsd(REGION_HANDLE, sim_addr()),
        );
        let poll = build_event_queue_request(None, false);
        let (status, _) = respond(&mut caps, &mut sim, &post(&eq_path, &poll))?;
        assert_eq!(status, 404);
        Ok(())
    }

    #[test]
    fn closed_session_polls_are_gone() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let now = Instant::now();
        let mut sim = SimSession::new(RegionHandle(REGION_HANDLE), now);
        // Let the inactivity timeout close the session (45 s in SimSession).
        let later = now
            .checked_add(Duration::from_secs(60))
            .ok_or("clock overflow")?;
        sim.handle_timeout(later);
        assert!(sim.is_closed());

        let eq_path = granted_event_queue_path(&caps)?;
        let poll = build_event_queue_request(None, false);
        let (status, _) = respond(&mut caps, &mut sim, &post(&eq_path, &poll))?;
        assert_eq!(status, 404);
        Ok(())
    }

    #[test]
    fn wrong_method_and_unknown_paths_are_rejected() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        let seed_path = caps.seed_url().path().to_owned();

        // GET on the seed: known URL, wrong method.
        let get = CapsRequest {
            method: "GET",
            path: &seed_path,
            query: None,
            body: b"",
        };
        let (status, _) = respond(&mut caps, &mut sim, &get)?;
        assert_eq!(status, 405);

        // An unminted token and a non-capability path: not found.
        let unknown = post("/cap/00000000-0000-0000-0000-0000000000ff", "");
        let (status, _) = respond(&mut caps, &mut sim, &unknown)?;
        assert_eq!(status, 404);
        let elsewhere = post("/somewhere/else", "");
        let (status, _) = respond(&mut caps, &mut sim, &elsewhere)?;
        assert_eq!(status, 404);

        // A seed body that is not LLSD-XML: bad request.
        let (status, _) = respond(&mut caps, &mut sim, &post(&seed_path, "not xml <"))?;
        assert_eq!(status, 400);
        Ok(())
    }

    #[test]
    fn responses_carry_the_llsd_content_type() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        let request_body = build_seed_request(REQUESTED_CAPABILITIES);
        let seed_path = caps.seed_url().path().to_owned();
        match caps.dispatch(&mut sim, &post(&seed_path, &request_body)) {
            CapsDispatch::Response(response) => {
                assert_eq!(response.content_type, LLSD_XML_CONTENT_TYPE);
            }
            CapsDispatch::EventQueueWouldBlock => return Err("unexpected would-block".into()),
        }
        Ok(())
    }

    /// A `ChatSessionRequest` accept answers the session's roster, which the
    /// real client folds into its own chat-session registry (with the
    /// `session-id` / `from_group` stamp the HTTP glue applies to the reply).
    #[test]
    fn chat_session_accept_round_trips_the_roster() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        let mut client = new_client()?;
        let now = Instant::now();

        let session_uuid = uuid::Uuid::from_u128(0x7001);
        let member_a = AgentKey::from(uuid::Uuid::from_u128(0x7002));
        let member_b = AgentKey::from(uuid::Uuid::from_u128(0x7003));
        sim.open_chat_session(
            ImSessionId::from(session_uuid),
            SimChatSessionKind::Conference,
            &[member_a, member_b],
        );

        let path = granted_cap_path(&caps, "ChatSessionRequest")?;
        let body = chat_session_request_body(CHAT_SESSION_ACCEPT, session_uuid);
        let (status, reply) = respond(&mut caps, &mut sim, &post(&path, &body))?;
        assert_eq!(status, 200);

        // The client runtime stamps the answered invitation's identity into
        // the reply map before forwarding (`post_chat_session_request`).
        let Llsd::Map(mut map) = parse_llsd_xml(&reply)? else {
            return Err("expected a roster map".into());
        };
        let _previous = map.insert("session-id".to_owned(), Llsd::Uuid(session_uuid));
        let _previous = map.insert("from_group".to_owned(), Llsd::Boolean(false));
        client.handle_caps_event(CAP_CHAT_SESSION_REQUEST, &Llsd::Map(map), now)?;

        let kind = ChatSessionKind::Conference {
            id: ImSessionId::from(session_uuid),
        };
        let info = client
            .chat_sessions_info()
            .find(|info| info.kind == kind)
            .ok_or("expected the accepted conference session on the client")?;
        assert_eq!(info.participants, vec![member_a, member_b]);
        Ok(())
    }

    /// Declines (invitation and p2p voice) are data-less `200` acks; an
    /// unknown method is a `400`.
    #[test]
    fn chat_session_decline_and_p2p_voice_ack() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        let session_uuid = uuid::Uuid::from_u128(0x7101);
        let path = granted_cap_path(&caps, "ChatSessionRequest")?;

        let decline = chat_session_request_body(CHAT_SESSION_DECLINE, session_uuid);
        let (status, body) = respond(&mut caps, &mut sim, &post(&path, &decline))?;
        assert_eq!((status, body.as_str()), (200, "<llsd><undef /></llsd>"));

        let voice = chat_session_request_body(CHAT_SESSION_DECLINE_P2P_VOICE, session_uuid);
        let (status, body) = respond(&mut caps, &mut sim, &post(&path, &voice))?;
        assert_eq!((status, body.as_str()), (200, "<llsd><undef /></llsd>"));

        let unknown = chat_session_request_body("mute update", session_uuid);
        let (status, _) = respond(&mut caps, &mut sim, &post(&path, &unknown))?;
        assert_eq!(status, 400);
        Ok(())
    }

    /// A `fetch history` answers the session's server-side backlog, which the
    /// real client (via the runtime's `{ history, session-id, from_group }`
    /// wrapper) surfaces as `Event::SessionServerHistory`.
    #[test]
    fn chat_session_fetch_history_round_trips() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        let mut client = new_client()?;
        let now = Instant::now();

        let session_uuid = uuid::Uuid::from_u128(0x7201);
        let session_id = ImSessionId::from(session_uuid);
        let speaker = AgentKey::from(uuid::Uuid::from_u128(0x7202));
        sim.open_chat_session(session_id, SimChatSessionKind::Conference, &[speaker]);
        let backlog = ServerHistoryMessage {
            sender: speaker,
            sender_name: "Speaker".to_owned(),
            text: "stored on the server".to_owned(),
            timestamp: Some(1_700_003_000),
        };
        sim.record_session_history(session_id, backlog.clone());

        let path = granted_cap_path(&caps, "ChatSessionRequest")?;
        let body = chat_session_request_body(CHAT_SESSION_FETCH_HISTORY, session_uuid);
        let (status, reply) = respond(&mut caps, &mut sim, &post(&path, &body))?;
        assert_eq!(status, 200);

        // The client runtime wraps the bare-array reply with the session
        // identity (`post_chat_session_fetch_history`).
        let history = parse_llsd_xml(&reply)?;
        let wrapped = Llsd::Map(
            [
                ("history".to_owned(), history),
                ("session-id".to_owned(), Llsd::Uuid(session_uuid)),
                ("from_group".to_owned(), Llsd::Boolean(false)),
            ]
            .into_iter()
            .collect(),
        );
        client.handle_caps_event(CHAT_SESSION_FETCH_HISTORY_TAG, &wrapped, now)?;

        let kind = ChatSessionKind::Conference { id: session_id };
        let history_event = drain_client(&mut client)
            .into_iter()
            .find_map(|event| match event {
                Event::SessionServerHistory { kind: k, messages } if k == kind => Some(messages),
                _ => None,
            })
            .ok_or("expected Event::SessionServerHistory")?;
        assert_eq!(history_event, vec![backlog]);
        Ok(())
    }

    /// The `ChatterBoxInvitation` and `ChatterBoxSessionAgentListUpdates`
    /// event-queue pushes reach the real client through one poll: the
    /// invitation records the pending session, the agent-list update folds
    /// the voice membership.
    #[test]
    fn chatterbox_eq_events_reach_the_client() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        let now = Instant::now();
        // The agent-list fold resolves the session through the client's own
        // agent id, so this client must have processed a login.
        let mut client = logged_in_client(now)?;

        let conference = uuid::Uuid::from_u128(0x7301);
        let inviter = AgentKey::from(uuid::Uuid::from_u128(0x7302));
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
        sim.enqueue_chatterbox_invitation(&invitation);
        sim.enqueue_chatterbox_agent_list_updates(conference, &[(inviter, true)]);

        let eq_path = granted_event_queue_path(&caps)?;
        let poll = build_event_queue_request(None, false);
        let (status, body) = respond(&mut caps, &mut sim, &post(&eq_path, &poll))?;
        assert_eq!(status, 200);
        let batch = parse_event_queue_response(&body)?;
        assert_eq!(batch.events.len(), 2);
        for event in &batch.events {
            client.handle_caps_event(&event.message, &event.body, now)?;
        }

        assert!(
            drain_client(&mut client)
                .iter()
                .any(|event| matches!(event, Event::ConferenceInvited { .. })),
            "the invitation surfaced on the client"
        );
        let kind = ChatSessionKind::Conference {
            id: ImSessionId::from(conference),
        };
        let info = client
            .chat_sessions_info()
            .find(|info| info.kind == kind)
            .ok_or("expected the invited conference session on the client")?;
        assert!(info.has_voice);
        assert_eq!(info.voice_members, vec![inviter]);
        Ok(())
    }

    /// A `ReadOfflineMsgs` GET serves the stored messages to the real client
    /// (surfaced as offline `Event::InstantMessageReceived`) exactly once — a
    /// repeated fetch answers an empty batch.
    #[test]
    fn read_offline_msgs_round_trips_and_delivers_once() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        let mut client = new_client()?;
        let now = Instant::now();

        let stored = InstantMessage {
            from_agent_id: AgentKey::from(uuid::Uuid::from_u128(0x7401)),
            from_agent_name: "Sender Resident".to_owned(),
            to_agent_id: AgentKey::from(uuid::Uuid::from_u128(0x7402)),
            dialog: ImDialog::Message,
            from_group: false,
            region_id: Some(uuid::Uuid::from_u128(0x7403)),
            position: RegionCoordinates::new(128.0, 64.0, 32.0),
            offline: true,
            timestamp: Some(1_700_004_000),
            id: uuid::Uuid::from_u128(0x7404),
            parent_estate_id: 1,
            message: "stored while offline".to_owned(),
            binary_bucket: Vec::new(),
        };
        sim.store_offline_message(stored.clone());

        let path = granted_cap_path(&caps, "ReadOfflineMsgs")?;
        let (status, body) = respond(&mut caps, &mut sim, &get(&path, None))?;
        assert_eq!(status, 200);
        client.handle_caps_event(CAP_READ_OFFLINE_MSGS, &parse_llsd_xml(&body)?, now)?;
        let received = drain_client(&mut client)
            .into_iter()
            .find_map(|event| match event {
                Event::InstantMessageReceived(im) => Some(*im),
                _ => None,
            })
            .ok_or("expected the offline IM on the client")?;
        assert_eq!(received, stored);

        // Deliver-once: the fetch drained the store.
        let (status, body) = respond(&mut caps, &mut sim, &get(&path, None))?;
        assert_eq!(status, 200);
        assert_eq!(parse_llsd_xml(&body)?, Llsd::Array(Vec::new()));
        Ok(())
    }

    /// A `GetDisplayNames` GET (with the query the client's own builder
    /// mints) answers stored records as `agents` and unknown ids as
    /// `bad_ids`, decoded by the client's own parser.
    #[test]
    fn get_display_names_round_trips() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();

        let known = AgentKey::from(uuid::Uuid::from_u128(0x7501));
        let unknown = AgentKey::from(uuid::Uuid::from_u128(0x7502));
        let record = DisplayName {
            id: known,
            username: "avatar.tester".to_owned(),
            display_name: "Avatar Tester".to_owned(),
            legacy_first_name: "Avatar".to_owned(),
            legacy_last_name: "Tester".to_owned(),
            is_display_name_default: false,
            display_name_expires: "2026-09-01T00:00:00Z".to_owned(),
            display_name_next_update: "2026-08-15T00:00:00Z".to_owned(),
            missing: false,
        };
        sim.set_display_name(record.clone());

        // The client's query builder mints `?ids=…&ids=…`; `CapsRequest`
        // carries the query without the leading `?`.
        let query = display_names_query(&[known.uuid(), unknown.uuid()]);
        let query = query.trim_start_matches('?').to_owned();
        let path = granted_cap_path(&caps, "GetDisplayNames")?;
        let (status, body) = respond(&mut caps, &mut sim, &get(&path, Some(&query)))?;
        assert_eq!(status, 200);

        let names = parse_display_names(&parse_llsd_xml(&body)?)?;
        let resolved = names
            .iter()
            .find(|name| name.id == known)
            .ok_or("expected the stored record")?;
        assert_eq!(resolved, &record);
        let missing = names
            .iter()
            .find(|name| name.id == unknown)
            .ok_or("expected the bad_ids record")?;
        assert!(missing.missing);
        Ok(())
    }

    /// `AgentPreferences` merges the POSTed fields into the stored set and
    /// echoes the full set; an empty-body POST is the pure "get"; `god_level`
    /// cannot be set by the client.
    #[test]
    fn agent_preferences_merge_and_echo() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        let path = granted_cap_path(&caps, "AgentPreferences")?;

        let update = AgentPreferences {
            hover_height: Some(0.5),
            god_level: Some(200),
            ..AgentPreferences::default()
        };
        let body = build_agent_preferences_request(&update);
        let (status, reply) = respond(&mut caps, &mut sim, &post(&path, &body))?;
        assert_eq!(status, 200);
        let stored = parse_agent_preferences(&parse_llsd_xml(&reply)?)?;
        // The merged hover height over the OpenSim defaults; the request's
        // god_level was ignored.
        assert_eq!(stored.hover_height, Some(0.5));
        assert_eq!(stored.max_access_pref.as_deref(), Some("M"));
        assert_eq!(stored.language.as_deref(), Some("en-us"));
        assert_eq!(stored.language_is_public, Some(true));
        assert_eq!(stored.god_level, Some(0));

        // An empty-body POST (the client's "get") echoes the same stored set.
        let (status, reply) = respond(&mut caps, &mut sim, &post(&path, "<llsd><map /></llsd>"))?;
        assert_eq!(status, 200);
        assert_eq!(parse_agent_preferences(&parse_llsd_xml(&reply)?)?, stored);
        Ok(())
    }

    /// A `SendUserReport` POST (built by the client's own builder) surfaces
    /// the parsed report as `ServerEvent::AbuseReportReceived`.
    #[test]
    fn send_user_report_round_trips() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        let report = AbuseReport {
            report_type: AbuseReportType::Complaint,
            category: 31,
            abuser_id: uuid::Uuid::from_u128(0x7601),
            summary: "summary line".to_owned(),
            details: "the details".to_owned(),
            version_string: "MyViewer 1.2.3".to_owned(),
            ..AbuseReport::default()
        };

        let path = granted_cap_path(&caps, "SendUserReport")?;
        let body = build_send_user_report(&report);
        let (status, reply) = respond(&mut caps, &mut sim, &post(&path, &body))?;
        assert_eq!((status, reply.as_str()), (200, "<llsd><undef /></llsd>"));

        match sim.poll_event() {
            Some(ServerEvent::AbuseReportReceived(received)) => assert_eq!(*received, report),
            other => return Err(format!("expected AbuseReportReceived, got {other:?}").into()),
        }
        Ok(())
    }

    /// The two-step `SendUserReportWithScreenshot` uploader: the report POST
    /// answers an uploader URL under the cap's own token (parsed by the
    /// client's own uploader-response parser), the raw-bytes POST completes
    /// it, and both halves surface together as
    /// `ServerEvent::AbuseReportWithScreenshotReceived`.
    #[test]
    fn send_user_report_with_screenshot_two_steps() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        let report = AbuseReport {
            report_type: AbuseReportType::Bug,
            summary: "with screenshot".to_owned(),
            ..AbuseReport::default()
        };
        let path = granted_cap_path(&caps, "SendUserReportWithScreenshot")?;

        // A bytes-POST with no parked report is rejected.
        let premature = CapsRequest {
            method: "POST",
            path: &format!("{path}/screenshot"),
            query: None,
            body: b"jp2 bytes",
        };
        let (status, _) = respond(&mut caps, &mut sim, &premature)?;
        assert_eq!(status, 400);

        // Step 1: the report POST answers the uploader URL.
        let body = build_send_user_report(&report);
        let (status, reply) = respond(&mut caps, &mut sim, &post(&path, &body))?;
        assert_eq!(status, 200);
        let step1 = parse_asset_upload_response(&reply)?;
        assert_eq!(step1.state, "upload");
        let uploader: url::Url = step1.uploader.ok_or("expected an uploader URL")?.parse()?;
        assert_eq!(uploader.path(), format!("{path}/screenshot"));

        // Step 2: the raw screenshot bytes complete the upload.
        let screenshot = CapsRequest {
            method: "POST",
            path: uploader.path(),
            query: None,
            body: b"jp2 bytes",
        };
        let (status, reply) = respond(&mut caps, &mut sim, &screenshot)?;
        assert_eq!(status, 200);
        assert_eq!(parse_asset_upload_response(&reply)?.state, "complete");

        match sim.poll_event() {
            Some(ServerEvent::AbuseReportWithScreenshotReceived {
                report: received,
                screenshot,
            }) => {
                assert_eq!(*received, report);
                assert_eq!(screenshot, b"jp2 bytes");
            }
            other => {
                return Err(
                    format!("expected AbuseReportWithScreenshotReceived, got {other:?}").into(),
                );
            }
        }
        Ok(())
    }

    /// The agent-communication handlers reject wrong methods and malformed
    /// bodies with the framework's status contract.
    #[test]
    fn agent_comms_methods_and_bodies_are_validated() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();

        // POST-only capabilities reject a GET.
        for name in ["ChatSessionRequest", "AgentPreferences", "SendUserReport"] {
            let path = granted_cap_path(&caps, name)?;
            let (status, _) = respond(&mut caps, &mut sim, &get(&path, None))?;
            assert_eq!(status, 405, "GET on {name}");
        }
        // GET-only capabilities reject a POST.
        for name in ["ReadOfflineMsgs", "GetDisplayNames"] {
            let path = granted_cap_path(&caps, name)?;
            let (status, _) = respond(&mut caps, &mut sim, &post(&path, ""))?;
            assert_eq!(status, 405, "POST on {name}");
        }
        // Garbage LLSD on the POST handlers is a bad request.
        for name in ["ChatSessionRequest", "AgentPreferences", "SendUserReport"] {
            let path = granted_cap_path(&caps, name)?;
            let (status, _) = respond(&mut caps, &mut sim, &post(&path, "not xml <"))?;
            assert_eq!(status, 400, "garbage body on {name}");
        }
        // A method-less ChatSessionRequest body is a bad request too.
        let path = granted_cap_path(&caps, "ChatSessionRequest")?;
        let (status, _) = respond(&mut caps, &mut sim, &post(&path, "<llsd><map /></llsd>"))?;
        assert_eq!(status, 400);
        Ok(())
    }
}
