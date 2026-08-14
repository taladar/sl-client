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
        AbuseReport, AbuseReportType, AgentKey, AgentPreferences, AssetKey,
        CAP_CHAT_SESSION_REQUEST, CAP_COPY_INVENTORY_FROM_NOTECARD, CAP_GET_TEXTURE,
        CAP_MODIFY_MATERIAL_PARAMS, CAP_NEW_FILE_AGENT_INVENTORY, CAP_OBJECT_MEDIA,
        CAP_OBJECT_MEDIA_NAVIGATE, CAP_READ_OFFLINE_MSGS, CAP_RENDER_MATERIALS,
        CAP_UPDATE_AVATAR_APPEARANCE, CAP_UPDATE_NOTECARD_AGENT_INVENTORY,
        CAP_UPDATE_NOTECARD_TASK_INVENTORY, CAP_UPDATE_SCRIPT_AGENT, CAP_UPLOAD_BAKED_TEXTURE,
        CAP_VIEWER_ASSET, CHAT_SESSION_ACCEPT, CHAT_SESSION_DECLINE,
        CHAT_SESSION_DECLINE_P2P_VOICE, CHAT_SESSION_FETCH_HISTORY, CHAT_SESSION_FETCH_HISTORY_TAG,
        CapsDispatch, CapsRequest, CapsUploadMetadata, ChatSessionKind, DisplayName, Event,
        FaceMaterialPut, ImDialog, ImSessionId, InMemoryAssetSource, InstantMessage,
        InventoryFolderKey, InventoryKey, LLSD_XML_CONTENT_TYPE, LegacyMaterial, LoginParams,
        MaterialOverrideUpdate, MediaEntry, ObjectKey, ObjectMediaState, REQUESTED_CAPABILITIES,
        RegionCoordinates, RegionHandle, ServerEvent, ServerHistoryMessage, Session, SimCaps,
        SimChatSessionKind, SimSession, StartLocation, TextureKey, build_event_queue_request,
        build_seed_request, chat_session_request_body, copy_inventory_from_notecard_body,
        enable_simulator_to_caps_llsd, parse_event_queue_response, parse_seed_response,
    };
    use sl_wire::{
        CircuitCode, Llsd, LoginRequest, LoginResponse, LoginSuccess,
        build_agent_preferences_request, build_modify_material_params_request,
        build_new_file_agent_inventory_request, build_object_media_get_request,
        build_object_media_navigate_request, build_object_media_update_request,
        build_render_materials_put_request, build_render_materials_request, build_send_user_report,
        build_update_avatar_appearance_request, build_update_item_asset_request,
        build_update_script_agent_request, build_update_task_item_asset_request,
        build_upload_baked_texture_request, display_names_query, parse_agent_preferences,
        parse_asset_upload_response, parse_display_names, parse_llsd_xml,
        parse_render_materials_response,
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
            range: None,
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
        // Seven agent-comms/framework sim caps, the four asset-delivery caps
        // (GetTexture/GetMesh/GetMesh2/ViewerAsset), and the fifteen content
        // upload/materials/MOAP caps.
        assert_eq!(granted.len(), 26);
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
        // `GetObjectCost` is still a Pending capability (no handler), so it
        // stands in here for "requested but unsupported"; `GetTexture` is now
        // served by the composed asset surface and would be granted.
        let request_body =
            build_seed_request(&["EventQueueGet", "GetObjectCost", "NoSuchCapability"]);
        let seed_path = caps.seed_url().path().to_owned();
        let (status, body) = respond(&mut caps, &mut sim, &post(&seed_path, &request_body))?;
        assert_eq!(status, 200);
        let granted = parse_seed_response(&body)?;
        assert!(granted.contains_key("EventQueueGet"));
        assert!(!granted.contains_key("GetObjectCost"));
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
            range: None,
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
            range: None,
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
            range: None,
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
            range: None,
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

    /// The chunk size the progressive-fetch loop below pulls the asset in —
    /// small, so a modest fixture spans several `206` responses.
    const ASSET_CHUNK: usize = 64;

    /// An asset `GET` request against a granted cap URL, with an optional
    /// `Range` header. The asset caps dispatch on
    /// [`SimCaps::assets`](sl_proto::SimCaps::assets), not `SimCaps::dispatch`.
    fn asset_get<'a>(path: &'a str, query: &'a str, range: Option<&'a str>) -> CapsRequest<'a> {
        CapsRequest {
            method: "GET",
            path,
            query: Some(query),
            range,
            body: b"",
        }
    }

    /// The seed grant advertises the four asset-delivery caps, and a
    /// `GetTexture` fetch round-trips: a whole `200`, a hand-rolled
    /// progressive `206` loop that reassembles the exact bytes the store
    /// holds, an out-of-range `416`, and a missing-asset `404` — the client's
    /// byte-range contract, driven with no HTTP.
    #[test]
    fn asset_caps_round_trip() -> Result<(), TestError> {
        let caps = new_caps()?;

        // The seed grant carries the asset caps alongside the sim caps.
        let granted = caps.grant(
            &REQUESTED_CAPABILITIES
                .iter()
                .map(|name| (*name).to_owned())
                .collect::<Vec<String>>(),
        );
        for name in [CAP_GET_TEXTURE, "GetMesh", "GetMesh2", CAP_VIEWER_ASSET] {
            assert!(
                granted.contains_key(name),
                "{name} not advertised in the grant"
            );
        }

        // A deterministic multi-chunk texture: 3.5 chunks, so the loop makes
        // four requests, the last one short.
        let id = uuid::Uuid::from_u128(0xa55e7);
        let total = ASSET_CHUNK * 3 + ASSET_CHUNK / 2;
        let bytes = (0..total)
            .map(|byte| u8::try_from(byte % 256).unwrap_or(0))
            .collect::<Vec<u8>>();
        let source = InMemoryAssetSource::new().with_asset(AssetKey::from(id), bytes.clone());

        let texture_path: url::Url = granted
            .get(CAP_GET_TEXTURE)
            .ok_or("GetTexture not granted")?
            .parse()?;
        let path = texture_path.path().to_owned();
        let query = format!("texture_id={id}");

        // Whole fetch: no `Range` → 200 with the full codestream.
        let whole = caps
            .assets()
            .dispatch(&source, &asset_get(&path, &query, None));
        assert_eq!(whole.status, 200);
        assert_eq!(whole.content_type, "image/x-j2c");
        assert_eq!(whole.content_range, None);
        assert_eq!(whole.body, bytes);

        // Progressive fetch: pull `ASSET_CHUNK` at a time, reassembling.
        let mut reassembled: Vec<u8> = Vec::new();
        let mut start = 0_usize;
        let mut requests = 0_usize;
        while start < total {
            let last = (start + ASSET_CHUNK - 1).min(total - 1);
            let range = format!("bytes={start}-{last}");
            let chunk = caps
                .assets()
                .dispatch(&source, &asset_get(&path, &query, Some(&range)));
            assert_eq!(chunk.status, 206, "chunk at {start}");
            assert_eq!(
                chunk.content_range.as_deref(),
                Some(format!("bytes {start}-{last}/{total}").as_str())
            );
            reassembled.extend_from_slice(&chunk.body);
            start = last + 1;
            requests += 1;
        }
        assert_eq!(reassembled, bytes);
        // ceil(3.5 chunks) = 4 requests.
        assert_eq!(requests, 4);

        // A range past the end of the existing asset → 416 (the client turns
        // it into an empty chunk and stops).
        let over = format!("bytes={total}-{}", total + 10);
        let response = caps
            .assets()
            .dispatch(&source, &asset_get(&path, &query, Some(&over)));
        assert_eq!(response.status, 416);
        assert_eq!(
            response.content_range.as_deref(),
            Some(format!("bytes */{total}").as_str())
        );
        assert!(response.body.is_empty());

        // A UUID not in the store → 404 (the client's hard NotFound).
        let missing = format!("texture_id={}", uuid::Uuid::from_u128(0xdead));
        let response = caps
            .assets()
            .dispatch(&source, &asset_get(&path, &missing, None));
        assert_eq!(response.status, 404);
        Ok(())
    }

    // -- The content upload / materials / MOAP cluster -------------------------

    /// A `POST` request carrying a raw (non-LLSD) body — the second step of a
    /// two-stage upload POSTs the asset bytes verbatim.
    fn raw_post<'a>(path: &'a str, body: &'a [u8]) -> CapsRequest<'a> {
        CapsRequest {
            method: "POST",
            path,
            query: None,
            range: None,
            body,
        }
    }

    /// Drives both steps of a two-stage upload for `cap` against the client's
    /// own builders/parsers: POST `metadata`, check the
    /// `{ state: "upload", uploader }` reply, POST `bytes` to the uploader
    /// sub-path, and return the parsed completion reply.
    fn run_two_stage_upload(
        caps: &mut SimCaps,
        sim: &mut SimSession,
        cap: &str,
        metadata: &str,
        bytes: &[u8],
    ) -> Result<sl_wire::AssetUploadResponse, TestError> {
        let path = granted_cap_path(caps, cap)?;
        let (status, reply) = respond(caps, sim, &post(&path, metadata))?;
        assert_eq!(status, 200, "step 1 for {cap}");
        let step1 = parse_asset_upload_response(&reply)?;
        assert_eq!(step1.state, "upload", "step 1 state for {cap}");
        let uploader: url::Url = step1.uploader.ok_or("no uploader url")?.parse()?;
        assert_eq!(uploader.path(), format!("{path}/upload"));
        let (status, reply) = respond(caps, sim, &raw_post(uploader.path(), bytes))?;
        assert_eq!(status, 200, "step 2 for {cap}");
        Ok(parse_asset_upload_response(&reply)?)
    }

    /// A `LegacyMaterial` sample whose fixed-point fields round-trip cleanly
    /// through the `RenderMaterials` codec.
    fn sample_material() -> LegacyMaterial {
        LegacyMaterial {
            normal_map: TextureKey::from(uuid::Uuid::from_u128(0x1234)),
            normal_offset: (0.5, -0.25),
            normal_repeat: (2.0, 4.0),
            normal_rotation: 1.5,
            specular_map: TextureKey::from(uuid::Uuid::from_u128(0x5678)),
            specular_offset: (0.1, 0.2),
            specular_repeat: (1.0, 1.0),
            specular_rotation: 0.0,
            specular_color: [10, 20, 30, 255],
            specular_exponent: 51,
            environment_intensity: 7,
            diffuse_alpha_mode: 1,
            alpha_mask_cutoff: 128,
        }
    }

    /// `NewFileAgentInventory`: the two-stage uploader parks the metadata,
    /// answers an uploader URL, then completes into
    /// `ServerEvent::CapsAssetUploaded` with a fresh asset and inventory item.
    /// A premature bytes-POST (no parked upload) is rejected.
    #[test]
    fn new_file_agent_inventory_two_stage() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        let folder = InventoryFolderKey::from(uuid::Uuid::from_u128(0x0f01_de11));
        let metadata = build_new_file_agent_inventory_request(
            folder,
            "texture",
            "texture",
            "My Texture",
            "a note",
            0x0008_e000,
            0,
            0,
            10,
        );

        // A bytes-POST before any step 1 is a bad request.
        let path = granted_cap_path(&caps, CAP_NEW_FILE_AGENT_INVENTORY)?;
        let upload_path = format!("{path}/upload");
        let (status, _) = respond(&mut caps, &mut sim, &raw_post(&upload_path, b"early"))?;
        assert_eq!(status, 400);

        let bytes = b"j2c-texture-bytes";
        let completion = run_two_stage_upload(
            &mut caps,
            &mut sim,
            CAP_NEW_FILE_AGENT_INVENTORY,
            &metadata,
            bytes,
        )?;
        assert_eq!(completion.state, "complete");
        let new_asset = completion.new_asset.ok_or("no new_asset")?;
        assert!(completion.new_inventory_item.is_some());

        match sim.poll_event() {
            Some(ServerEvent::CapsAssetUploaded {
                metadata,
                new_asset: event_asset,
                new_inventory_item,
                data,
            }) => {
                assert_eq!(event_asset, AssetKey::from(new_asset));
                assert!(new_inventory_item.is_some());
                assert_eq!(data, bytes);
                match *metadata {
                    CapsUploadMetadata::NewFileInventory(request) => {
                        assert_eq!(request.name, "My Texture");
                        assert_eq!(request.folder_id, folder);
                        assert_eq!(request.asset_type, "texture");
                    }
                    other => return Err(format!("expected NewFileInventory, got {other:?}").into()),
                }
            }
            other => return Err(format!("expected CapsAssetUploaded, got {other:?}").into()),
        }
        Ok(())
    }

    /// `UploadBakedTexture`: a temporary bake completes with **no** inventory
    /// item, and the metadata is `BakedTexture`.
    #[test]
    fn upload_baked_texture_has_no_inventory_item() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        let completion = run_two_stage_upload(
            &mut caps,
            &mut sim,
            CAP_UPLOAD_BAKED_TEXTURE,
            &build_upload_baked_texture_request(),
            b"baked-bytes",
        )?;
        assert_eq!(completion.state, "complete");
        assert!(completion.new_asset.is_some());
        assert_eq!(completion.new_inventory_item, None);
        match sim.poll_event() {
            Some(ServerEvent::CapsAssetUploaded {
                metadata,
                new_inventory_item,
                ..
            }) => {
                assert_eq!(new_inventory_item, None);
                assert!(matches!(*metadata, CapsUploadMetadata::BakedTexture));
            }
            other => return Err(format!("expected CapsAssetUploaded, got {other:?}").into()),
        }
        Ok(())
    }

    /// An `Update*AgentInventory` replacement carries the cap name and the item
    /// being updated through to the completion event.
    #[test]
    fn update_agent_item_replaces_asset() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        let item = InventoryKey::from(uuid::Uuid::from_u128(0x7e11));
        let completion = run_two_stage_upload(
            &mut caps,
            &mut sim,
            CAP_UPDATE_NOTECARD_AGENT_INVENTORY,
            &build_update_item_asset_request(item),
            b"notecard-text",
        )?;
        assert!(completion.new_asset.is_some());
        assert!(completion.new_inventory_item.is_some());
        match sim.poll_event() {
            Some(ServerEvent::CapsAssetUploaded { metadata, .. }) => match *metadata {
                CapsUploadMetadata::UpdateAgentItem { cap, item_id } => {
                    assert_eq!(cap, CAP_UPDATE_NOTECARD_AGENT_INVENTORY);
                    assert_eq!(item_id, item);
                }
                other => return Err(format!("expected UpdateAgentItem, got {other:?}").into()),
            },
            other => return Err(format!("expected CapsAssetUploaded, got {other:?}").into()),
        }
        Ok(())
    }

    /// `UpdateNotecardTaskInventory` carries the holding object and item.
    #[test]
    fn update_task_item_replaces_asset() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        let task = ObjectKey::from(uuid::Uuid::from_u128(0x7a5c));
        let item = InventoryKey::from(uuid::Uuid::from_u128(0x7e12));
        run_two_stage_upload(
            &mut caps,
            &mut sim,
            CAP_UPDATE_NOTECARD_TASK_INVENTORY,
            &build_update_task_item_asset_request(task, item),
            b"task-notecard",
        )?;
        match sim.poll_event() {
            Some(ServerEvent::CapsAssetUploaded { metadata, .. }) => match *metadata {
                CapsUploadMetadata::UpdateTaskItem {
                    cap,
                    task_id,
                    item_id,
                } => {
                    assert_eq!(cap, CAP_UPDATE_NOTECARD_TASK_INVENTORY);
                    assert_eq!(task_id, task);
                    assert_eq!(item_id, item);
                }
                other => return Err(format!("expected UpdateTaskItem, got {other:?}").into()),
            },
            other => return Err(format!("expected CapsAssetUploaded, got {other:?}").into()),
        }
        Ok(())
    }

    /// `UpdateScriptAgent` completes with a `compiled` result (the script
    /// family's extra completion field) and the script metadata.
    #[test]
    fn update_script_agent_reports_compiled() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        let item = InventoryKey::from(uuid::Uuid::from_u128(0x5c11));
        let completion = run_two_stage_upload(
            &mut caps,
            &mut sim,
            CAP_UPDATE_SCRIPT_AGENT,
            &build_update_script_agent_request(item, "mono"),
            b"default { state_entry() {} }",
        )?;
        assert_eq!(completion.compiled, Some(true));
        assert!(completion.errors.is_empty());
        match sim.poll_event() {
            Some(ServerEvent::CapsAssetUploaded { metadata, .. }) => match *metadata {
                CapsUploadMetadata::UpdateScriptAgent(request) => {
                    assert_eq!(request.item_id, item);
                    assert_eq!(request.target, "mono");
                }
                other => return Err(format!("expected UpdateScriptAgent, got {other:?}").into()),
            },
            other => return Err(format!("expected CapsAssetUploaded, got {other:?}").into()),
        }
        Ok(())
    }

    /// `UpdateAvatarAppearance`: the client's bake trigger is surfaced as
    /// `ServerAppearanceRequested`, and the accept reply folds into
    /// `Event::ServerAppearanceUpdate { success: true }`.
    #[test]
    fn update_avatar_appearance_round_trips() -> Result<(), TestError> {
        let now = Instant::now();
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        let mut client = new_client()?;
        let path = granted_cap_path(&caps, CAP_UPDATE_AVATAR_APPEARANCE)?;
        let (status, reply) = respond(
            &mut caps,
            &mut sim,
            &post(&path, &build_update_avatar_appearance_request(42)),
        )?;
        assert_eq!(status, 200);
        client.handle_caps_event(CAP_UPDATE_AVATAR_APPEARANCE, &parse_llsd_xml(&reply)?, now)?;
        assert!(
            drain_client(&mut client)
                .iter()
                .any(|event| matches!(event, Event::ServerAppearanceUpdate { success: true, .. }))
        );
        assert!(matches!(
            sim.poll_event(),
            Some(ServerEvent::ServerAppearanceRequested { cof_version: 42 })
        ));
        Ok(())
    }

    /// `CopyInventoryFromNotecard`: the one-way POST acks with an undefined body
    /// and surfaces the copy request (nil object/folder ids fold to `None`).
    #[test]
    fn copy_inventory_from_notecard_round_trips() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        let notecard = InventoryKey::from(uuid::Uuid::from_u128(0x0ca1));
        let object = ObjectKey::from(uuid::Uuid::from_u128(0x0ca2));
        let item = InventoryKey::from(uuid::Uuid::from_u128(0x0ca3));
        let body = copy_inventory_from_notecard_body(notecard, Some(object), item, None);
        let path = granted_cap_path(&caps, CAP_COPY_INVENTORY_FROM_NOTECARD)?;
        let (status, _) = respond(&mut caps, &mut sim, &post(&path, &body))?;
        assert_eq!(status, 200);
        match sim.poll_event() {
            Some(ServerEvent::CopyInventoryFromNotecardRequested {
                notecard_id,
                object_id,
                item_id,
                folder_id,
            }) => {
                assert_eq!(notecard_id, notecard);
                assert_eq!(object_id, Some(object));
                assert_eq!(item_id, item);
                assert_eq!(folder_id, None);
            }
            other => {
                return Err(
                    format!("expected CopyInventoryFromNotecardRequested, got {other:?}").into(),
                );
            }
        }
        Ok(())
    }

    /// `RenderMaterials`: a POST query round-trips a stored material through the
    /// client's response parser; a PUT surfaces the face assignments as
    /// `ServerEvent::RenderMaterialsSet`.
    #[test]
    fn render_materials_query_and_put() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        let material_id = uuid::Uuid::from_u128(0x0a1a_0001);
        let material = sample_material();
        sim.set_region_material(material_id, material.clone());

        let path = granted_cap_path(&caps, CAP_RENDER_MATERIALS)?;
        let (status, reply) = respond(
            &mut caps,
            &mut sim,
            &post(&path, &build_render_materials_request(&[material_id])),
        )?;
        assert_eq!(status, 200);
        let entries = parse_render_materials_response(&reply);
        assert_eq!(entries.len(), 1);
        let entry = entries.first().ok_or("no material entry")?;
        assert_eq!(entry.material_id, material_id);
        assert_eq!(entry.material, material);

        // A PUT sets legacy materials on faces → RenderMaterialsSet.
        let updates = vec![FaceMaterialPut {
            local_id: 0x00ab_cdef,
            face: 2,
            material: Some(sample_material()),
        }];
        let put_body = build_render_materials_put_request(&updates);
        let put = CapsRequest {
            method: "PUT",
            path: &path,
            query: None,
            range: None,
            body: put_body.as_bytes(),
        };
        let (status, _) = respond(&mut caps, &mut sim, &put)?;
        assert_eq!(status, 200);
        match sim.poll_event() {
            Some(ServerEvent::RenderMaterialsSet { updates: set }) => {
                assert_eq!(set, updates);
            }
            other => return Err(format!("expected RenderMaterialsSet, got {other:?}").into()),
        }
        Ok(())
    }

    /// `ModifyMaterialParams`: the POST folds into
    /// `Event::MaterialParamsResult { success: true }` and surfaces
    /// `ServerEvent::MaterialParamsModified`.
    #[test]
    fn modify_material_params_round_trips() -> Result<(), TestError> {
        let now = Instant::now();
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        let mut client = new_client()?;
        let updates = vec![MaterialOverrideUpdate {
            object_id: ObjectKey::from(uuid::Uuid::from_u128(0x0b1b)),
            side: 1,
            gltf_json: Some("{}".to_owned()),
            asset_id: None,
        }];
        let path = granted_cap_path(&caps, CAP_MODIFY_MATERIAL_PARAMS)?;
        let (status, reply) = respond(
            &mut caps,
            &mut sim,
            &post(&path, &build_modify_material_params_request(&updates)),
        )?;
        assert_eq!(status, 200);
        client.handle_caps_event(CAP_MODIFY_MATERIAL_PARAMS, &parse_llsd_xml(&reply)?, now)?;
        assert!(
            drain_client(&mut client)
                .iter()
                .any(|event| matches!(event, Event::MaterialParamsResult { success: true, .. }))
        );
        match sim.poll_event() {
            Some(ServerEvent::MaterialParamsModified { updates: modified }) => {
                assert_eq!(modified, updates);
            }
            other => return Err(format!("expected MaterialParamsModified, got {other:?}").into()),
        }
        Ok(())
    }

    /// `ObjectMedia`: a GET round-trips the stored per-face media through the
    /// client fold; an UPDATE records new media and surfaces
    /// `ServerEvent::ObjectMediaSet`.
    #[test]
    fn object_media_get_and_update() -> Result<(), TestError> {
        let now = Instant::now();
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        let mut client = new_client()?;
        let object = ObjectKey::from(uuid::Uuid::from_u128(0xed1a));
        let entry = MediaEntry {
            current_url: Some("http://example.com/".parse()?),
            home_url: Some("http://example.com/".parse()?),
            ..MediaEntry::default()
        };
        sim.set_object_media(
            object,
            ObjectMediaState {
                version: "x-mv:0000000001/init".to_owned(),
                faces: vec![Some(entry.clone()), None],
            },
        );

        let path = granted_cap_path(&caps, CAP_OBJECT_MEDIA)?;
        let (status, reply) = respond(
            &mut caps,
            &mut sim,
            &post(&path, &build_object_media_get_request(object)),
        )?;
        assert_eq!(status, 200);
        client.handle_caps_event(CAP_OBJECT_MEDIA, &parse_llsd_xml(&reply)?, now)?;
        let media = drain_client(&mut client)
            .into_iter()
            .find_map(|event| match event {
                Event::ObjectMedia {
                    object_id, faces, ..
                } if object_id == object => Some(faces),
                _ => None,
            })
            .ok_or("expected an ObjectMedia event")?;
        assert_eq!(media, vec![Some(entry.clone()), None]);

        // An UPDATE verb records the new media and advances the version.
        let (status, _) = respond(
            &mut caps,
            &mut sim,
            &post(
                &path,
                &build_object_media_update_request(object, &[Some(entry.clone())]),
            ),
        )?;
        assert_eq!(status, 200);
        match sim.poll_event() {
            Some(ServerEvent::ObjectMediaSet { object_id, faces }) => {
                assert_eq!(object_id, object);
                assert_eq!(faces, vec![Some(entry)]);
            }
            other => return Err(format!("expected ObjectMediaSet, got {other:?}").into()),
        }
        Ok(())
    }

    /// `ObjectMediaNavigate`: the POST surfaces `ServerEvent::ObjectMediaNavigated`.
    #[test]
    fn object_media_navigate_round_trips() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        let object = ObjectKey::from(uuid::Uuid::from_u128(0xed2a));
        let path = granted_cap_path(&caps, CAP_OBJECT_MEDIA_NAVIGATE)?;
        let (status, _) = respond(
            &mut caps,
            &mut sim,
            &post(
                &path,
                &build_object_media_navigate_request(object, 3, "http://example.net/"),
            ),
        )?;
        assert_eq!(status, 200);
        match sim.poll_event() {
            Some(ServerEvent::ObjectMediaNavigated {
                object_id,
                face,
                url,
            }) => {
                assert_eq!(object_id, object);
                assert_eq!(face, 3);
                assert_eq!(url, "http://example.net/");
            }
            other => return Err(format!("expected ObjectMediaNavigated, got {other:?}").into()),
        }
        Ok(())
    }

    /// The content handlers reject wrong methods and malformed bodies with the
    /// framework's status contract.
    #[test]
    fn content_caps_methods_and_bodies_are_validated() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();

        // POST-only content caps reject a GET.
        for name in [
            CAP_NEW_FILE_AGENT_INVENTORY,
            CAP_UPDATE_AVATAR_APPEARANCE,
            CAP_COPY_INVENTORY_FROM_NOTECARD,
            CAP_MODIFY_MATERIAL_PARAMS,
            CAP_OBJECT_MEDIA,
            CAP_OBJECT_MEDIA_NAVIGATE,
        ] {
            let path = granted_cap_path(&caps, name)?;
            let (status, _) = respond(&mut caps, &mut sim, &get(&path, None))?;
            assert_eq!(status, 405, "GET on {name}");
        }

        // Garbage LLSD on the LLSD-bodied content POST handlers is a bad request.
        for name in [
            CAP_UPDATE_AVATAR_APPEARANCE,
            CAP_COPY_INVENTORY_FROM_NOTECARD,
            CAP_MODIFY_MATERIAL_PARAMS,
            CAP_OBJECT_MEDIA,
            CAP_OBJECT_MEDIA_NAVIGATE,
        ] {
            let path = granted_cap_path(&caps, name)?;
            let (status, _) = respond(&mut caps, &mut sim, &post(&path, "not xml <"))?;
            assert_eq!(status, 400, "garbage body on {name}");
        }

        // A method-less ObjectMedia body is unroutable → 400.
        let path = granted_cap_path(&caps, CAP_OBJECT_MEDIA)?;
        let (status, _) = respond(&mut caps, &mut sim, &post(&path, "<llsd><map /></llsd>"))?;
        assert_eq!(status, 400);
        Ok(())
    }
}
