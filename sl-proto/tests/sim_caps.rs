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
        AVATAR_PICKER_PAGE_SIZE, AVATAR_PICKER_SEARCH_TAG, AbuseReport, AbuseReportType, AgentKey,
        AgentPreferences, AssetKey, CAP_AGENT_EXPERIENCES, CAP_ATTACHMENT_RESOURCES,
        CAP_CHAT_SESSION_REQUEST, CAP_COPY_INVENTORY_FROM_NOTECARD, CAP_CREATE_INVENTORY_CATEGORY,
        CAP_EXPERIENCE_PREFERENCES, CAP_EXT_ENVIRONMENT, CAP_FETCH_INVENTORY,
        CAP_FETCH_INVENTORY_ITEM, CAP_FETCH_LIBRARY, CAP_FETCH_LIBRARY_ITEM,
        CAP_FIND_EXPERIENCE_BY_NAME, CAP_GET_ADMIN_EXPERIENCES, CAP_GET_CREATOR_EXPERIENCES,
        CAP_GET_EXPERIENCE_INFO, CAP_GET_EXPERIENCES, CAP_GET_OBJECT_COST,
        CAP_GET_OBJECT_PHYSICS_DATA, CAP_GET_TEXTURE, CAP_GROUP_EXPERIENCES,
        CAP_IS_EXPERIENCE_ADMIN, CAP_IS_EXPERIENCE_CONTRIBUTOR, CAP_LAND_RESOURCES, CAP_LSL_SYNTAX,
        CAP_MODIFY_MATERIAL_PARAMS, CAP_NEW_FILE_AGENT_INVENTORY, CAP_OBJECT_MEDIA,
        CAP_OBJECT_MEDIA_NAVIGATE, CAP_PARCEL_VOICE_INFO, CAP_PROVISION_VOICE_ACCOUNT,
        CAP_READ_OFFLINE_MSGS, CAP_REGION_EXPERIENCES, CAP_REMOTE_PARCEL_REQUEST,
        CAP_RENDER_MATERIALS, CAP_RESOURCE_COST_SELECTED, CAP_SIMULATOR_FEATURES,
        CAP_UPDATE_AVATAR_APPEARANCE, CAP_UPDATE_EXPERIENCE, CAP_UPDATE_NOTECARD_AGENT_INVENTORY,
        CAP_UPDATE_NOTECARD_TASK_INVENTORY, CAP_UPDATE_SCRIPT_AGENT, CAP_UPLOAD_BAKED_TEXTURE,
        CAP_VIEWER_ASSET, CAP_VOICE_SIGNALING, CHAT_SESSION_ACCEPT, CHAT_SESSION_DECLINE,
        CHAT_SESSION_DECLINE_P2P_VOICE, CHAT_SESSION_FETCH_HISTORY, CHAT_SESSION_FETCH_HISTORY_TAG,
        CHAT_SESSION_INVITE, CHAT_SESSION_START_CONFERENCE, CapsDispatch, CapsRequest,
        CapsUploadMetadata, ChatSessionKind, DayCycle, DisplayName, EnvironmentSettings,
        EnvironmentUpdate, Event, ExperienceInfo, ExperienceKey, ExperiencePermission,
        ExperienceProperties, ExperienceUpdate, FaceMaterialPut, GroupKey, ImDialog, ImSessionId,
        InMemoryAssetSource, InstantMessage, InventoryFolder, InventoryFolderKey, InventoryItem,
        InventoryKey, LAND_RESOURCE_DETAIL_TAG, LAND_RESOURCE_SUMMARY_TAG, LLSD_XML_CONTENT_TYPE,
        LegacyMaterial, LoginParams, LslKeyword, LslSyntax, MaterialOverrideUpdate, MediaEntry,
        ObjectCost, ObjectKey, ObjectMediaState, ObjectPhysicsData, OwnerKey, ParcelKey,
        ParcelScriptResources, Permissions5, PhysicsShapeType, REQUESTED_CAPABILITIES,
        RegionCoordinates, RegionHandle, RegionLocalParcelId, ResourceAmount, ResourceSummary,
        ScriptedObjectInfo, ScriptedObjectResources, SelectedCostKind, SelectedResourceCost,
        ServerEvent, ServerHistoryMessage, Session, SimCaps, SimChatSessionKind, SimParcel,
        SimSession, SimulatorFeatures, StartLocation, TextureKey, VoiceChannel,
        VoiceProvisionOutcome, VoiceProvisionRefusal, WebRtcStub, avatar_picker_search_query,
        build_environment_update_request, build_event_queue_request, build_get_object_cost_request,
        build_get_object_physics_data_request, build_land_resources_request,
        build_region_experiences_request, build_remote_parcel_request,
        build_resource_cost_selected_request, build_seed_request,
        build_set_experience_permission_request, build_update_experience_request,
        chat_session_agents_body, chat_session_request_body, copy_inventory_from_notecard_body,
        enable_simulator_to_caps_llsd, experience_id_query, experience_info_query,
        find_experience_query, forget_experience_query, group_experiences_query,
        parse_event_queue_response, parse_experience_ids, parse_experience_infos,
        parse_experience_status, parse_seed_response,
    };
    use sl_wire::PROPERTY_PRIVATE;
    use sl_wire::{
        CircuitCode, FetchItemRef, Llsd, LoginRequest, LoginResponse, LoginSuccess,
        ais_category_children_fetch_url, ais_category_url, ais_create_category_url, ais_item_url,
        build_agent_preferences_request, build_ais_create_category_body,
        build_ais_create_link_body, build_ais_move_body, build_ais_rename_category_body,
        build_ais_update_item_body, build_create_inventory_category_request,
        build_fetch_inventory_items_request, build_fetch_inventory_request,
        build_modify_material_params_request, build_new_file_agent_inventory_request,
        build_object_media_get_request, build_object_media_navigate_request,
        build_object_media_update_request, build_render_materials_put_request,
        build_render_materials_request, build_send_user_report,
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
        // Eight agent-comms/framework sim caps, the four asset-delivery caps
        // (GetTexture/GetMesh/GetMesh2/ViewerAsset), the fifteen content
        // upload/materials/MOAP caps, the seven inventory caps (the two
        // descendents fetches, the two per-item fetches, AISv3 agent +
        // Library, CreateInventoryCategory), the nine
        // region/object-information caps, the twelve experience caps, and
        // the three voice signalling caps.
        assert_eq!(granted.len(), 58);
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
        // `GroupMemberData` is still a Pending capability (no handler), so it
        // stands in here for "requested but unsupported"; `GetTexture` is now
        // served by the composed asset surface and would be granted.
        let request_body =
            build_seed_request(&["EventQueueGet", "GroupMemberData", "NoSuchCapability"]);
        let seed_path = caps.seed_url().path().to_owned();
        let (status, body) = respond(&mut caps, &mut sim, &post(&seed_path, &request_body))?;
        assert_eq!(status, 200);
        let granted = parse_seed_response(&body)?;
        assert!(granted.contains_key("EventQueueGet"));
        assert!(!granted.contains_key("GroupMemberData"));
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
            enable_simulator_to_caps_llsd(REGION_HANDLE, sim_addr(), (256, 256)),
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
            enable_simulator_to_caps_llsd(REGION_HANDLE, sim_addr(), (256, 256)),
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
            enable_simulator_to_caps_llsd(REGION_HANDLE, sim_addr(), (256, 256)),
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

    /// A `start conference` registers the ad-hoc session with its invitees,
    /// tells the driver to relay the invitations, and answers a roster the
    /// real client folds — and the grid's `ChatterBoxSessionStartReply` then
    /// moves that client's session onto the id the simulator chose.
    #[test]
    fn start_conference_registers_and_then_rekeys() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        let mut client = new_client()?;
        let now = Instant::now();

        let temp = ImSessionId::from(uuid::Uuid::from_u128(0x7301));
        let real = ImSessionId::from(uuid::Uuid::from_u128(0x7302));
        let invitee = AgentKey::from(uuid::Uuid::from_u128(0x7303));

        // The client's own optimistic half of the cap path.
        client.open_conference(temp, &[invitee], now);

        let path = granted_cap_path(&caps, "ChatSessionRequest")?;
        let body = chat_session_agents_body(CHAT_SESSION_START_CONFERENCE, temp.get(), &[invitee]);
        let (status, _reply) = respond(&mut caps, &mut sim, &post(&path, &body))?;
        assert_eq!(status, 200);
        let mut relayed = false;
        while let Some(event) = sim.poll_event() {
            if let ServerEvent::ConferenceStartRequested {
                session_id,
                invitees,
                ..
            } = event
            {
                relayed = session_id == temp && invitees == vec![invitee];
            }
        }
        assert!(relayed, "the driver is asked to relay the invitations");

        // The simulator minted its own id and says so over the event queue.
        sim.enqueue_chatterbox_session_start_reply(&Event::ChatSessionStarted {
            temp_session_id: temp,
            session_id: real,
            success: true,
            session_name: "Multi-person chat".to_owned(),
            voice_enabled: false,
            error: String::new(),
        });
        let eq_path = granted_event_queue_path(&caps)?;
        let poll = build_event_queue_request(None, false);
        let (status, body) = respond(&mut caps, &mut sim, &post(&eq_path, &poll))?;
        assert_eq!(status, 200);
        let batch = parse_event_queue_response(&body)?;
        for event in &batch.events {
            client.handle_caps_event(&event.message, &event.body, now)?;
        }

        assert!(
            client
                .participants(ChatSessionKind::Conference { id: real })
                .any(|agent| agent == invitee),
            "the client's conference moved onto the simulator's id"
        );
        Ok(())
    }

    /// An `invite` grows an open session's roster and asks the driver to relay
    /// the invitations; naming a session the simulator does not know is a
    /// `400`, since an invite (unlike a start) is about an existing session.
    #[test]
    fn invite_grows_an_open_session() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        let session = ImSessionId::from(uuid::Uuid::from_u128(0x7401));
        let member = AgentKey::from(uuid::Uuid::from_u128(0x7402));
        let invitee = AgentKey::from(uuid::Uuid::from_u128(0x7403));
        sim.open_chat_session(session, SimChatSessionKind::Conference, &[member]);

        let path = granted_cap_path(&caps, "ChatSessionRequest")?;
        let body = chat_session_agents_body(CHAT_SESSION_INVITE, session.get(), &[invitee]);
        let (status, reply) = respond(&mut caps, &mut sim, &post(&path, &body))?;
        assert_eq!(status, 200);
        assert!(
            reply.contains(&invitee.uuid().to_string()),
            "the answered roster names the newly invited agent"
        );
        let mut relayed = false;
        while let Some(event) = sim.poll_event() {
            if let ServerEvent::SessionInviteRequested {
                session_id,
                invitees,
            } = event
            {
                relayed = session_id == session && invitees == vec![invitee];
            }
        }
        assert!(relayed, "the driver is asked to relay the invitations");

        let unknown = ImSessionId::from(uuid::Uuid::from_u128(0x7404));
        let body = chat_session_agents_body(CHAT_SESSION_INVITE, unknown.get(), &[invitee]);
        let (status, _reply) = respond(&mut caps, &mut sim, &post(&path, &body))?;
        assert_eq!(status, 400);
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

    /// An `AvatarPickerSearch` GET (with the query the client's own builder
    /// mints) answers the residents whose **username, display name or legacy
    /// name** matches — the three fields the legacy UDP picker could not search
    /// — and the client folds the reply into an `AvatarPickerReply` under the
    /// query id the runtime stamped, with the modern identity intact.
    #[test]
    fn avatar_picker_search_round_trips() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        let mut client = new_client()?;
        let now = Instant::now();

        let found = AgentKey::from(uuid::Uuid::from_u128(0x7601));
        let other = AgentKey::from(uuid::Uuid::from_u128(0x7602));
        sim.set_display_name(DisplayName {
            id: found,
            username: "marina.vector".to_owned(),
            display_name: "Marina".to_owned(),
            legacy_first_name: "MarinaVector".to_owned(),
            legacy_last_name: "Resident".to_owned(),
            ..DisplayName::default()
        });
        sim.set_display_name(DisplayName {
            id: other,
            username: "someone.else".to_owned(),
            display_name: "Someone Else".to_owned(),
            legacy_first_name: "SomeoneElse".to_owned(),
            legacy_last_name: "Resident".to_owned(),
            ..DisplayName::default()
        });

        // A username match, typed the way a user types one: with the dot the
        // builder turns into a space.
        let query = avatar_picker_search_query("marina.vector", AVATAR_PICKER_PAGE_SIZE);
        let query = query.trim_start_matches('?').to_owned();
        let path = granted_cap_path(&caps, "AvatarPickerSearch")?;
        let (status, body) = respond(&mut caps, &mut sim, &get(&path, Some(&query)))?;
        assert_eq!(status, 200);

        // The runtime stamps the query id it minted before handing the reply on.
        let query_id = uuid::Uuid::from_u128(0x7603);
        let Llsd::Map(mut map) = parse_llsd_xml(&body)? else {
            return Err("expected a search reply map".into());
        };
        let _previous = map.insert("query-id".to_owned(), Llsd::Uuid(query_id));
        client.handle_caps_event(AVATAR_PICKER_SEARCH_TAG, &Llsd::Map(map), now)?;

        let reply = drain_client(&mut client)
            .into_iter()
            .find_map(|event| match event {
                Event::AvatarPickerReply { query_id, results } => Some((query_id, results)),
                _other => None,
            })
            .ok_or("expected an AvatarPickerReply on the client")?;
        assert_eq!(reply.0, query_id, "the answer routes back to its search");
        assert_eq!(
            reply
                .1
                .iter()
                .map(|result| (result.avatar_id, result.username.clone()))
                .collect::<Vec<_>>(),
            vec![(found, "marina.vector".to_owned())],
            "only the match, and it kept its modern identity"
        );
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

    // -----------------------------------------------------------------------
    // The inventory cluster: the descendents / per-item fetch caps, AISv3,
    // and CreateInventoryCategory, served from the SimInventoryTree fixtures.
    // -----------------------------------------------------------------------

    /// An [`InventoryFolderKey`] from a small integer.
    fn folder_key(id: u128) -> InventoryFolderKey {
        InventoryFolderKey::from(uuid::Uuid::from_u128(id))
    }

    /// An [`InventoryKey`] from a small integer.
    fn item_key(id: u128) -> InventoryKey {
        InventoryKey::from(uuid::Uuid::from_u128(id))
    }

    /// A minimal deterministic inventory item for the serving fixtures.
    fn sample_inventory_item(id: u128, folder: InventoryFolderKey, name: &str) -> InventoryItem {
        InventoryItem {
            item_id: item_key(id),
            folder_id: folder,
            name: name.to_owned(),
            description: String::new(),
            asset_id: uuid::Uuid::from_u128(id.wrapping_add(0x1000)),
            item_type: 0,
            inv_type: 0,
            flags: 0,
            sale_type: 0,
            sale_price: None,
            creation_date: 0,
            owner: OwnerKey::Agent(AgentKey::from(uuid::Uuid::from_u128(1))),
            last_owner_id: uuid::Uuid::nil(),
            creator_id: AgentKey::from(uuid::Uuid::from_u128(1)),
            group: None,
            permissions: Permissions5::default(),
        }
    }

    /// The seeded agent tree: root (`AGENT_ROOT`) → "Clothing"
    /// (`AGENT_CLOTHING`) → item "Hat" (`AGENT_HAT`).
    const AGENT_ROOT: u128 = 0x0A01;
    /// The "Clothing" folder under the agent root.
    const AGENT_CLOTHING: u128 = 0x0A02;
    /// The "Hat" item inside "Clothing".
    const AGENT_HAT: u128 = 0x0A11;
    /// The seeded Library root folder.
    const LIB_ROOT: u128 = 0x0B01;
    /// The "Library Texture" item inside the Library root.
    const LIB_TEXTURE: u128 = 0x0B11;

    /// Seeds both serving trees with the small deterministic fixture above.
    fn seed_inventory(sim: &mut SimSession) {
        sim.agent_inventory_mut().insert_folder(InventoryFolder {
            folder_id: folder_key(AGENT_ROOT),
            parent_id: None,
            name: "My Inventory".to_owned(),
            folder_type: 8,
            version: 5,
        });
        sim.agent_inventory_mut().insert_folder(InventoryFolder {
            folder_id: folder_key(AGENT_CLOTHING),
            parent_id: Some(folder_key(AGENT_ROOT)),
            name: "Clothing".to_owned(),
            folder_type: 5,
            version: 3,
        });
        sim.agent_inventory_mut().insert_item(sample_inventory_item(
            AGENT_HAT,
            folder_key(AGENT_CLOTHING),
            "Hat",
        ));
        sim.library_inventory_mut().insert_folder(InventoryFolder {
            folder_id: folder_key(LIB_ROOT),
            parent_id: None,
            name: "Library".to_owned(),
            folder_type: 8,
            version: 2,
        });
        sim.library_inventory_mut()
            .insert_item(sample_inventory_item(
                LIB_TEXTURE,
                folder_key(LIB_ROOT),
                "Library Texture",
            ));
    }

    /// Dispatches one AIS3 request: splits the sl-wire URL suffix into the
    /// path and query halves of a [`CapsRequest`] under the cap's own path,
    /// the same split the HTTP glue performs.
    fn respond_ais(
        caps: &mut SimCaps,
        sim: &mut SimSession,
        method: &str,
        cap_path: &str,
        suffix: &str,
        body: &str,
    ) -> Result<(u16, String), TestError> {
        let (path_part, query) = match suffix.split_once('?') {
            Some((path_part, query)) => (path_part, Some(query)),
            None => (suffix, None),
        };
        let path = format!("{cap_path}{path_part}");
        let request = CapsRequest {
            method,
            path: &path,
            query,
            range: None,
            body: body.as_bytes(),
        };
        respond(caps, sim, &request)
    }

    /// The `folders` batch fetch round-trips through the real client: known
    /// folders answer their direct children, unknown folders are skipped.
    #[test]
    fn fetch_inventory_descendents2_round_trips_through_the_real_client() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        seed_inventory(&mut sim);
        let now = Instant::now();
        let mut client = logged_in_client(now)?;

        let body = build_fetch_inventory_request(
            uuid::Uuid::from_u128(1),
            &[
                folder_key(AGENT_ROOT),
                folder_key(AGENT_CLOTHING),
                folder_key(0xdead),
            ],
        );
        let path = granted_cap_path(&caps, CAP_FETCH_INVENTORY)?;
        let (status, reply) = respond(&mut caps, &mut sim, &post(&path, &body))?;
        assert_eq!(status, 200);

        client.handle_caps_event(CAP_FETCH_INVENTORY, &parse_llsd_xml(&reply)?, now)?;
        let descendents: Vec<Event> = drain_client(&mut client)
            .into_iter()
            .filter(|event| matches!(event, Event::InventoryDescendents { .. }))
            .collect();
        // The unknown folder is skipped tolerantly, so two entries come back.
        assert_eq!(descendents.len(), 2);
        match descendents.first() {
            Some(Event::InventoryDescendents {
                folder_id,
                version,
                descendents,
                folders,
                items,
            }) => {
                assert_eq!(*folder_id, folder_key(AGENT_ROOT));
                assert_eq!(*version, 5);
                assert_eq!(*descendents, 1);
                assert_eq!(
                    folders
                        .iter()
                        .map(|folder| folder.folder_id)
                        .collect::<Vec<_>>(),
                    vec![folder_key(AGENT_CLOTHING)]
                );
                assert!(items.is_empty());
            }
            other => return Err(format!("unexpected first event: {other:?}").into()),
        }
        match descendents.get(1) {
            Some(Event::InventoryDescendents {
                folder_id,
                version,
                items,
                ..
            }) => {
                assert_eq!(*folder_id, folder_key(AGENT_CLOTHING));
                assert_eq!(*version, 3);
                assert_eq!(
                    items
                        .iter()
                        .map(|item| item.name.clone())
                        .collect::<Vec<_>>(),
                    vec!["Hat".to_owned()]
                );
            }
            other => return Err(format!("unexpected second event: {other:?}").into()),
        }
        Ok(())
    }

    /// `FetchLibDescendents2` answers from the Library tree — agent-tree
    /// folders are unknown to it and are skipped.
    #[test]
    fn fetch_lib_descendents2_serves_the_library_tree() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        seed_inventory(&mut sim);

        let body = build_fetch_inventory_request(
            uuid::Uuid::from_u128(2),
            &[folder_key(LIB_ROOT), folder_key(AGENT_CLOTHING)],
        );
        let path = granted_cap_path(&caps, CAP_FETCH_LIBRARY)?;
        let (status, reply) = respond(&mut caps, &mut sim, &post(&path, &body))?;
        assert_eq!(status, 200);

        let tree = parse_llsd_xml(&reply)?;
        let folders = tree
            .get("folders")
            .and_then(Llsd::as_array)
            .ok_or("missing folders")?;
        assert_eq!(folders.len(), 1);
        let entry = folders.first().ok_or("empty folders")?;
        assert_eq!(
            entry.get("folder_id").and_then(Llsd::as_uuid),
            Some(folder_key(LIB_ROOT).uuid())
        );
        let items = entry
            .get("items")
            .and_then(Llsd::as_array)
            .ok_or("missing items")?;
        assert_eq!(items.len(), 1);
        Ok(())
    }

    /// The per-item `FetchInventory2` / `FetchLib2` caps round-trip through
    /// the real client's new fold; unknown ids are omitted from the reply.
    #[test]
    fn fetch_inventory2_and_fetch_lib2_round_trip_per_item() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        seed_inventory(&mut sim);
        let now = Instant::now();
        let mut client = logged_in_client(now)?;

        let agent = uuid::Uuid::from_u128(1);
        let body = build_fetch_inventory_items_request(
            agent,
            &[
                FetchItemRef {
                    owner_id: agent,
                    item_id: item_key(AGENT_HAT),
                },
                FetchItemRef {
                    owner_id: agent,
                    item_id: item_key(0xdead),
                },
            ],
        );
        let path = granted_cap_path(&caps, CAP_FETCH_INVENTORY_ITEM)?;
        let (status, reply) = respond(&mut caps, &mut sim, &post(&path, &body))?;
        assert_eq!(status, 200);
        client.handle_caps_event(CAP_FETCH_INVENTORY_ITEM, &parse_llsd_xml(&reply)?, now)?;
        let items = drain_client(&mut client)
            .into_iter()
            .find_map(|event| match event {
                Event::InventoryBulkUpdate { items, .. } => Some(items),
                _ => None,
            })
            .ok_or("no bulk update from FetchInventory2")?;
        assert_eq!(
            items.iter().map(|item| item.item_id).collect::<Vec<_>>(),
            vec![item_key(AGENT_HAT)]
        );

        let body = build_fetch_inventory_items_request(
            agent,
            &[FetchItemRef {
                owner_id: agent,
                item_id: item_key(LIB_TEXTURE),
            }],
        );
        let path = granted_cap_path(&caps, CAP_FETCH_LIBRARY_ITEM)?;
        let (status, reply) = respond(&mut caps, &mut sim, &post(&path, &body))?;
        assert_eq!(status, 200);
        client.handle_caps_event(CAP_FETCH_LIBRARY_ITEM, &parse_llsd_xml(&reply)?, now)?;
        let items = drain_client(&mut client)
            .into_iter()
            .find_map(|event| match event {
                Event::InventoryBulkUpdate { items, .. } => Some(items),
                _ => None,
            })
            .ok_or("no bulk update from FetchLib2")?;
        assert_eq!(
            items
                .iter()
                .map(|item| item.name.clone())
                .collect::<Vec<_>>(),
            vec!["Library Texture".to_owned()]
        );
        Ok(())
    }

    /// `CreateInventoryCategory` applies the client-chosen folder, surfaces
    /// the server event, and a follow-up fetch sees the folder under a
    /// bumped parent version.
    #[test]
    fn create_inventory_category_round_trips() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        seed_inventory(&mut sim);
        let now = Instant::now();
        let mut client = logged_in_client(now)?;

        let new_folder = folder_key(0x0C01);
        let body =
            build_create_inventory_category_request(new_folder, folder_key(AGENT_ROOT), 8, "Toys");
        let path = granted_cap_path(&caps, CAP_CREATE_INVENTORY_CATEGORY)?;
        let (status, reply) = respond(&mut caps, &mut sim, &post(&path, &body))?;
        assert_eq!(status, 200);

        client.handle_caps_event(CAP_CREATE_INVENTORY_CATEGORY, &parse_llsd_xml(&reply)?, now)?;
        let folders = drain_client(&mut client)
            .into_iter()
            .find_map(|event| match event {
                Event::InventoryBulkUpdate { folders, .. } => Some(folders),
                _ => None,
            })
            .ok_or("no bulk update from CreateInventoryCategory")?;
        assert_eq!(
            folders
                .iter()
                .map(|folder| folder.folder_id)
                .collect::<Vec<_>>(),
            vec![new_folder]
        );

        match sim.poll_event() {
            Some(ServerEvent::InventoryCategoryCreated { folder }) => {
                assert_eq!(folder.folder_id, new_folder);
                assert_eq!(folder.name, "Toys");
            }
            other => return Err(format!("unexpected server event: {other:?}").into()),
        }

        // A follow-up descendents fetch sees the new folder and the bumped
        // parent version (5 → 6).
        let body =
            build_fetch_inventory_request(uuid::Uuid::from_u128(1), &[folder_key(AGENT_ROOT)]);
        let path = granted_cap_path(&caps, CAP_FETCH_INVENTORY)?;
        let (_, reply) = respond(&mut caps, &mut sim, &post(&path, &body))?;
        let tree = parse_llsd_xml(&reply)?;
        let entry = tree
            .get("folders")
            .and_then(Llsd::as_array)
            .and_then(<[Llsd]>::first)
            .ok_or("missing folder entry")?;
        assert_eq!(entry.get("version").and_then(Llsd::as_i32), Some(6));
        assert_eq!(entry.get("descendents").and_then(Llsd::as_i32), Some(2));
        Ok(())
    }

    /// The AIS3 category lifecycle — create, rename, move, delete — mutates
    /// the serving tree, reports `_updated_category_versions` at every step,
    /// and folds through the real client.
    #[test]
    fn ais3_category_lifecycle_round_trips() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        seed_inventory(&mut sim);
        let now = Instant::now();
        let mut client = logged_in_client(now)?;
        let cap_path = granted_cap_path(&caps, "InventoryAPIv3")?;

        // Create under the root.
        let suffix = ais_create_category_url(folder_key(AGENT_ROOT), uuid::Uuid::from_u128(0x71d));
        let body = build_ais_create_category_body(5, "Sub");
        let (status, reply) = respond_ais(&mut caps, &mut sim, "POST", &cap_path, &suffix, &body)?;
        assert_eq!(status, 200);
        let tree = parse_llsd_xml(&reply)?;
        let created = tree
            .get("_created_categories")
            .and_then(Llsd::as_array)
            .and_then(<[Llsd]>::first)
            .and_then(Llsd::as_uuid)
            .ok_or("no _created_categories")?;
        let created = InventoryFolderKey::from(created);
        // The parent's bumped version is reported for the client's re-fetch.
        assert_eq!(
            tree.get("_updated_category_versions")
                .and_then(|versions| versions.get(&folder_key(AGENT_ROOT).to_string()))
                .and_then(Llsd::as_i32),
            Some(6)
        );
        // The reply folds through the real client as a bulk update carrying
        // the embedded folder.
        client.handle_caps_event("InventoryAPIv3", &tree, now)?;
        let folders = drain_client(&mut client)
            .into_iter()
            .find_map(|event| match event {
                Event::InventoryBulkUpdate { folders, .. } => Some(folders),
                _ => None,
            })
            .ok_or("no bulk update from the AIS create")?;
        assert_eq!(
            folders
                .iter()
                .map(|folder| folder.folder_id)
                .collect::<Vec<_>>(),
            vec![created]
        );
        match sim.poll_event() {
            Some(ServerEvent::InventoryCategoryCreated { folder }) => {
                assert_eq!(folder.name, "Sub");
            }
            other => return Err(format!("unexpected server event: {other:?}").into()),
        }

        // Rename bumps the folder's own version.
        let suffix = ais_category_url(created);
        let body = build_ais_rename_category_body("Renamed");
        let (status, reply) = respond_ais(&mut caps, &mut sim, "PATCH", &cap_path, &suffix, &body)?;
        assert_eq!(status, 200);
        let tree = parse_llsd_xml(&reply)?;
        assert_eq!(
            tree.get("_updated_category_versions")
                .and_then(|versions| versions.get(&created.to_string()))
                .and_then(Llsd::as_i32),
            Some(2)
        );
        assert_eq!(
            sim.agent_inventory()
                .folder(created)
                .map(|folder| folder.name.clone()),
            Some("Renamed".to_owned())
        );

        // Move under Clothing bumps both parents.
        let body = build_ais_move_body(folder_key(AGENT_CLOTHING));
        let (status, reply) = respond_ais(&mut caps, &mut sim, "PATCH", &cap_path, &suffix, &body)?;
        assert_eq!(status, 200);
        let tree = parse_llsd_xml(&reply)?;
        let versions = tree
            .get("_updated_category_versions")
            .ok_or("no versions on move")?;
        assert_eq!(
            versions
                .get(&folder_key(AGENT_ROOT).to_string())
                .and_then(Llsd::as_i32),
            Some(7)
        );
        assert_eq!(
            versions
                .get(&folder_key(AGENT_CLOTHING).to_string())
                .and_then(Llsd::as_i32),
            Some(4)
        );
        assert_eq!(
            sim.agent_inventory()
                .folder(created)
                .and_then(|folder| folder.parent_id),
            Some(folder_key(AGENT_CLOTHING))
        );

        // Delete removes the subtree and reports it.
        let (status, reply) = respond_ais(&mut caps, &mut sim, "DELETE", &cap_path, &suffix, "")?;
        assert_eq!(status, 200);
        let tree = parse_llsd_xml(&reply)?;
        assert_eq!(
            tree.get("_categories_removed")
                .and_then(Llsd::as_array)
                .and_then(<[Llsd]>::first)
                .and_then(Llsd::as_uuid),
            Some(created.uuid())
        );
        assert!(sim.agent_inventory().folder(created).is_none());
        Ok(())
    }

    /// The AIS3 item verbs: update embeds the new item state, move re-parents
    /// it, delete removes it and reports `_removed_items`.
    #[test]
    fn ais3_item_update_move_remove_round_trips() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        seed_inventory(&mut sim);
        let cap_path = granted_cap_path(&caps, "InventoryAPIv3")?;
        let suffix = ais_item_url(item_key(AGENT_HAT));

        // Update name/description; the updated item comes back embedded.
        let body = build_ais_update_item_body("Fedora", "a fancy hat");
        let (status, reply) = respond_ais(&mut caps, &mut sim, "PATCH", &cap_path, &suffix, &body)?;
        assert_eq!(status, 200);
        let tree = parse_llsd_xml(&reply)?;
        let embedded_name = tree
            .get("_embedded")
            .and_then(|embedded| embedded.get("items"))
            .and_then(|items| items.get(&item_key(AGENT_HAT).to_string()))
            .and_then(|item| item.get("name"))
            .and_then(Llsd::as_str)
            .map(str::to_owned);
        assert_eq!(embedded_name, Some("Fedora".to_owned()));
        assert_eq!(
            tree.get("_updated_category_versions")
                .and_then(|versions| versions.get(&folder_key(AGENT_CLOTHING).to_string()))
                .and_then(Llsd::as_i32),
            Some(4)
        );

        // Move into the root bumps both folders.
        let body = build_ais_move_body(folder_key(AGENT_ROOT));
        let (status, _) = respond_ais(&mut caps, &mut sim, "PATCH", &cap_path, &suffix, &body)?;
        assert_eq!(status, 200);
        assert_eq!(
            sim.agent_inventory()
                .item(item_key(AGENT_HAT))
                .map(|item| item.folder_id),
            Some(folder_key(AGENT_ROOT))
        );

        // A GET fetches the item at the top level.
        let (status, reply) = respond_ais(&mut caps, &mut sim, "GET", &cap_path, &suffix, "")?;
        assert_eq!(status, 200);
        let tree = parse_llsd_xml(&reply)?;
        assert_eq!(tree.get("name").and_then(Llsd::as_str), Some("Fedora"));

        // Delete removes it and reports `_removed_items`.
        let (status, reply) = respond_ais(&mut caps, &mut sim, "DELETE", &cap_path, &suffix, "")?;
        assert_eq!(status, 200);
        let tree = parse_llsd_xml(&reply)?;
        assert_eq!(
            tree.get("_removed_items")
                .and_then(Llsd::as_array)
                .and_then(<[Llsd]>::first)
                .and_then(Llsd::as_uuid),
            Some(item_key(AGENT_HAT).uuid())
        );
        assert!(sim.agent_inventory().item(item_key(AGENT_HAT)).is_none());
        Ok(())
    }

    /// An AIS3 create with a `links` payload mints link items whose
    /// `asset_id` is the linked object's id, embeds them **under
    /// `_embedded.links`** (not `_embedded.items` — AIS files the two
    /// separately, and the reference viewer's `AISUpdate::parseEmbeddedLinks`
    /// and `parseDescendentCount` both depend on the distinction), and
    /// surfaces the server event.
    #[test]
    fn ais3_link_creation_round_trips() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        seed_inventory(&mut sim);
        let cap_path = granted_cap_path(&caps, "InventoryAPIv3")?;

        let linked = uuid::Uuid::from_u128(0x117c);
        let suffix =
            ais_create_category_url(folder_key(AGENT_CLOTHING), uuid::Uuid::from_u128(0x71d));
        let body = build_ais_create_link_body(linked, 24, 18, "Hat Link", "worn");
        let (status, reply) = respond_ais(&mut caps, &mut sim, "POST", &cap_path, &suffix, &body)?;
        assert_eq!(status, 200);
        let tree = parse_llsd_xml(&reply)?;
        let created = tree
            .get("_created_items")
            .and_then(Llsd::as_array)
            .and_then(<[Llsd]>::first)
            .and_then(Llsd::as_uuid)
            .ok_or("no _created_items")?;
        let embedded_asset = tree
            .get("_embedded")
            .and_then(|embedded| embedded.get("links"))
            .and_then(|links| links.get(&created.to_string()))
            .and_then(|link| link.get("asset_id"))
            .and_then(Llsd::as_uuid);
        assert_eq!(embedded_asset, Some(linked));
        // And it is filed there *instead of* under `items`, not as well as.
        assert!(
            tree.get("_embedded")
                .and_then(|embedded| embedded.get("items"))
                .and_then(|items| items.get(&created.to_string()))
                .is_none(),
            "a link must not also appear under _embedded.items"
        );
        match sim.poll_event() {
            Some(ServerEvent::InventoryLinksCreated { links }) => {
                assert_eq!(links.len(), 1);
            }
            other => return Err(format!("unexpected server event: {other:?}").into()),
        }
        assert!(
            sim.agent_inventory()
                .item(InventoryKey::from(created))
                .is_some()
        );
        Ok(())
    }

    /// The AIS3 children fetch honours the depth parameter, flattening the
    /// subtree into the top-level `_embedded` block.
    #[test]
    fn ais3_children_fetch_honours_depth() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        seed_inventory(&mut sim);
        // Deepen the tree: Clothing → "Formal" → item "Tuxedo".
        let formal = folder_key(0x0A03);
        sim.agent_inventory_mut().insert_folder(InventoryFolder {
            folder_id: formal,
            parent_id: Some(folder_key(AGENT_CLOTHING)),
            name: "Formal".to_owned(),
            folder_type: -1,
            version: 1,
        });
        sim.agent_inventory_mut()
            .insert_item(sample_inventory_item(0x0A12, formal, "Tuxedo"));
        let cap_path = granted_cap_path(&caps, "InventoryAPIv3")?;

        let embedded_counts = |reply: &str| -> Result<(usize, usize), TestError> {
            let tree = parse_llsd_xml(reply)?;
            let embedded = tree.get("_embedded");
            let categories = embedded
                .and_then(|embedded| embedded.get("categories"))
                .and_then(Llsd::as_map)
                .map_or(0, std::collections::HashMap::len);
            let items = embedded
                .and_then(|embedded| embedded.get("items"))
                .and_then(Llsd::as_map)
                .map_or(0, std::collections::HashMap::len);
            Ok((categories, items))
        };

        // Depth 0: the category alone, no children.
        let suffix = ais_category_children_fetch_url(folder_key(AGENT_ROOT), 0);
        let (status, reply) = respond_ais(&mut caps, &mut sim, "GET", &cap_path, &suffix, "")?;
        assert_eq!(status, 200);
        let tree = parse_llsd_xml(&reply)?;
        assert_eq!(
            tree.get("category_id").and_then(Llsd::as_uuid),
            Some(folder_key(AGENT_ROOT).uuid())
        );
        assert!(tree.get("_embedded").is_none());

        // Depth 1: only Clothing.
        let suffix = ais_category_children_fetch_url(folder_key(AGENT_ROOT), 1);
        let (_, reply) = respond_ais(&mut caps, &mut sim, "GET", &cap_path, &suffix, "")?;
        assert_eq!(embedded_counts(&reply)?, (1, 0));

        // Depth 50: the whole flattened subtree (Clothing + Formal, Hat +
        // Tuxedo).
        let suffix = ais_category_children_fetch_url(folder_key(AGENT_ROOT), 50);
        let (_, reply) = respond_ais(&mut caps, &mut sim, "GET", &cap_path, &suffix, "")?;
        assert_eq!(embedded_counts(&reply)?, (2, 2));
        Ok(())
    }

    /// `LibraryAPIv3` serves reads from the Library tree and rejects every
    /// mutating verb.
    #[test]
    fn library_api_v3_is_read_only() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        seed_inventory(&mut sim);
        let cap_path = granted_cap_path(&caps, "LibraryAPIv3")?;

        let suffix = ais_category_children_fetch_url(folder_key(LIB_ROOT), 1);
        let (status, reply) = respond_ais(&mut caps, &mut sim, "GET", &cap_path, &suffix, "")?;
        assert_eq!(status, 200);
        let tree = parse_llsd_xml(&reply)?;
        let items = tree
            .get("_embedded")
            .and_then(|embedded| embedded.get("items"))
            .and_then(Llsd::as_map)
            .map_or(0, std::collections::HashMap::len);
        assert_eq!(items, 1);

        // Every mutating verb answers 405.
        let rename = build_ais_rename_category_body("Nope");
        let suffix = ais_category_url(folder_key(LIB_ROOT));
        let (status, _) = respond_ais(&mut caps, &mut sim, "PATCH", &cap_path, &suffix, &rename)?;
        assert_eq!(status, 405);
        let (status, _) = respond_ais(&mut caps, &mut sim, "DELETE", &cap_path, &suffix, "")?;
        assert_eq!(status, 405);
        let create_suffix =
            ais_create_category_url(folder_key(LIB_ROOT), uuid::Uuid::from_u128(0x71d));
        let create = build_ais_create_category_body(-1, "Nope");
        let (status, _) = respond_ais(
            &mut caps,
            &mut sim,
            "POST",
            &cap_path,
            &create_suffix,
            &create,
        )?;
        assert_eq!(status, 405);
        Ok(())
    }

    /// The inventory handlers' status contract: wrong methods 405, malformed
    /// bodies and unroutable sub-paths 400, unknown AIS targets 404, and
    /// invalid moves 400.
    #[test]
    fn inventory_caps_reject_bad_requests() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        seed_inventory(&mut sim);

        // The fetch caps are POST-only and reject garbage bodies.
        for name in [
            CAP_FETCH_INVENTORY,
            CAP_FETCH_LIBRARY,
            CAP_FETCH_INVENTORY_ITEM,
            CAP_FETCH_LIBRARY_ITEM,
            CAP_CREATE_INVENTORY_CATEGORY,
        ] {
            let path = granted_cap_path(&caps, name)?;
            let (status, _) = respond(&mut caps, &mut sim, &get(&path, None))?;
            assert_eq!(status, 405, "GET on {name}");
            let (status, _) = respond(&mut caps, &mut sim, &post(&path, "not xml <"))?;
            assert_eq!(status, 400, "garbage body on {name}");
        }

        let cap_path = granted_cap_path(&caps, "InventoryAPIv3")?;
        // An unknown AIS target is 404 (unlike the tolerant batch fetches).
        let suffix = ais_category_url(folder_key(0xdead));
        let rename = build_ais_rename_category_body("Ghost");
        let (status, _) = respond_ais(&mut caps, &mut sim, "PATCH", &cap_path, &suffix, &rename)?;
        assert_eq!(status, 404);
        let suffix = ais_item_url(item_key(0xdead));
        let (status, _) = respond_ais(&mut caps, &mut sim, "GET", &cap_path, &suffix, "")?;
        assert_eq!(status, 404);
        // An unknown parent on the plain create cap is 404 too.
        let path = granted_cap_path(&caps, CAP_CREATE_INVENTORY_CATEGORY)?;
        let body = build_create_inventory_category_request(
            folder_key(0x0C02),
            folder_key(0xdead),
            -1,
            "Orphan",
        );
        let (status, _) = respond(&mut caps, &mut sim, &post(&path, &body))?;
        assert_eq!(status, 404);

        // A cycle-creating move is a bad request.
        let suffix = ais_category_url(folder_key(AGENT_ROOT));
        let into_child = build_ais_move_body(folder_key(AGENT_CLOTHING));
        let (status, _) = respond_ais(
            &mut caps,
            &mut sim,
            "PATCH",
            &cap_path,
            &suffix,
            &into_child,
        )?;
        assert_eq!(status, 400);

        // Garbage bodies and unroutable sub-paths are bad requests; unknown
        // verbs are 405.
        let suffix = ais_category_url(folder_key(AGENT_ROOT));
        let (status, _) = respond_ais(
            &mut caps,
            &mut sim,
            "PATCH",
            &cap_path,
            &suffix,
            "not xml <",
        )?;
        assert_eq!(status, 400);
        let (status, _) = respond_ais(&mut caps, &mut sim, "GET", &cap_path, "/bogus", "")?;
        assert_eq!(status, 400);
        let (status, _) = respond_ais(&mut caps, &mut sim, "PUT", &cap_path, &suffix, "")?;
        assert_eq!(status, 405);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // The region/object-information cluster.
    // -----------------------------------------------------------------------

    /// A `PUT` [`CapsRequest`] carrying an LLSD-XML body and a query string.
    fn put<'a>(path: &'a str, query: Option<&'a str>, body: &'a str) -> CapsRequest<'a> {
        CapsRequest {
            method: "PUT",
            path,
            query,
            range: None,
            body: body.as_bytes(),
        }
    }

    /// The syntax id `seed_region_info` advertises for its LSL document.
    const SYNTAX_ID: u128 = 0x5b7a;

    /// Two seeded objects with cost/physics/selection records, and one id
    /// deliberately never seeded.
    const OBJECT_A: u128 = 0x0C0A;
    /// The second seeded object.
    const OBJECT_B: u128 = 0x0C0B;
    /// An object id no fixture knows — the "no such object" probe.
    const OBJECT_UNKNOWN: u128 = 0x0CFF;

    /// A deterministic [`ObjectCost`] whose four costs derive from `base`.
    fn sample_object_cost(base: f32) -> ObjectCost {
        ObjectCost {
            linked_set_resource_cost: base,
            resource_cost: base / 2.0,
            physics_cost: base / 4.0,
            linked_set_physics_cost: base / 8.0,
            resource_limiting_type: "legacy".to_owned(),
        }
    }

    /// A deterministic scripted-object entry for the resource reports.
    fn sample_scripted_object(id: u128, name: &str) -> ScriptedObjectInfo {
        ScriptedObjectInfo {
            id: uuid::Uuid::from_u128(id),
            location: RegionCoordinates::new(10.0, 20.0, 30.0),
            name: name.to_owned(),
            owner: OwnerKey::Agent(AgentKey::from(uuid::Uuid::from_u128(1))),
            resources: ScriptedObjectResources {
                memory: Some(0x0001_0000),
                urls: Some(2),
            },
        }
    }

    /// Seeds every region/object-information fixture: the feature + syntax
    /// documents (with the shared syntax id), a parcel environment override,
    /// two costed objects, the parcel-cover rectangles, and the resource
    /// reports.
    fn seed_region_info(sim: &mut SimSession) {
        sim.set_simulator_features(SimulatorFeatures {
            mesh_rez_enabled: Some(true),
            mesh_upload_enabled: Some(true),
            max_agent_attachments: Some(38),
            ..SimulatorFeatures::default()
        });
        let mut syntax = LslSyntax::default();
        syntax.controls.insert(
            "if".to_owned(),
            LslKeyword {
                tooltip: Some("Conditional.".to_owned()),
                deprecated: false,
                god_mode: false,
            },
        );
        sim.set_lsl_syntax(uuid::Uuid::from_u128(SYNTAX_ID), syntax);

        sim.set_region_id(uuid::Uuid::from_u128(0x1e6));
        sim.set_environment(EnvironmentSettings {
            parcel_id: 3,
            region_id: uuid::Uuid::from_u128(0x1e6),
            day_length: 7200,
            day_offset: 57600,
            flags: 0,
            env_version: 1,
            track_altitudes: [1000.0, 2000.0, 3000.0],
            day_cycle: DayCycle {
                name: "Parcel Cycle".to_owned(),
                water_track: Vec::new(),
                sky_tracks: Vec::new(),
                sky_frames: std::collections::BTreeMap::new(),
                water_frames: std::collections::BTreeMap::new(),
            },
        });

        sim.set_object_cost(
            ObjectKey::from(uuid::Uuid::from_u128(OBJECT_A)),
            sample_object_cost(16.0),
        );
        sim.set_object_cost(
            ObjectKey::from(uuid::Uuid::from_u128(OBJECT_B)),
            sample_object_cost(8.0),
        );
        sim.set_object_physics(
            ObjectKey::from(uuid::Uuid::from_u128(OBJECT_A)),
            ObjectPhysicsData {
                physics_shape_type: PhysicsShapeType::Prim,
                density: 1000.0,
                friction: 0.6,
                restitution: 0.5,
                gravity_multiplier: 1.0,
            },
        );
        sim.set_object_physics(
            ObjectKey::from(uuid::Uuid::from_u128(OBJECT_B)),
            ObjectPhysicsData {
                physics_shape_type: PhysicsShapeType::ConvexHull,
                density: 500.0,
                friction: 0.3,
                restitution: 0.25,
                gravity_multiplier: 2.0,
            },
        );
        sim.set_selection_cost(
            ObjectKey::from(uuid::Uuid::from_u128(OBJECT_A)),
            SelectedResourceCost {
                physics: 1.0,
                streaming: 2.0,
                simulation: 3.0,
            },
        );
        sim.set_selection_cost(
            ObjectKey::from(uuid::Uuid::from_u128(OBJECT_B)),
            SelectedResourceCost {
                physics: 0.5,
                streaming: 0.25,
                simulation: 0.125,
            },
        );

        sim.add_parcel(SimParcel {
            parcel_id: ParcelKey::from(uuid::Uuid::from_u128(0xACE1)),
            west: 0.0,
            south: 0.0,
            east: 128.0,
            north: 256.0,
        });
        sim.add_parcel(SimParcel {
            parcel_id: ParcelKey::from(uuid::Uuid::from_u128(0xACE2)),
            west: 128.0,
            south: 0.0,
            east: 256.0,
            north: 256.0,
        });

        sim.set_attachment_resources(sl_proto::AttachmentResourcesReport {
            attachments: vec![sl_proto::AttachmentLocation {
                location: "Skull".to_owned(),
                objects: vec![sample_scripted_object(0xA771, "HUD")],
            }],
            summary: ResourceSummary {
                available: vec![ResourceAmount {
                    resource_type: "memory".to_owned(),
                    amount: 0x0010_0000,
                }],
                used: vec![ResourceAmount {
                    resource_type: "memory".to_owned(),
                    amount: 0x0001_0000,
                }],
            },
        });
        sim.set_land_resource_summary(ResourceSummary {
            available: vec![ResourceAmount {
                resource_type: "urls".to_owned(),
                amount: 38,
            }],
            used: vec![ResourceAmount {
                resource_type: "urls".to_owned(),
                amount: 2,
            }],
        });
        sim.set_land_resource_details(vec![ParcelScriptResources {
            name: "Test Parcel".to_owned(),
            id: uuid::Uuid::from_u128(0xACE1),
            local_id: RegionLocalParcelId(3),
            objects: vec![sample_scripted_object(0xA772, "Greeter")],
        }]);
    }

    /// Dispatches a request and folds the LLSD reply into the client under
    /// `tag`, returning the client events it produced. Asserts the reply is a
    /// 200.
    fn fold_into_client(
        caps: &mut SimCaps,
        sim: &mut SimSession,
        client: &mut Session,
        request: &CapsRequest<'_>,
        tag: &str,
        now: Instant,
    ) -> Result<Vec<Event>, TestError> {
        let (status, body) = respond(caps, sim, request)?;
        assert_eq!(status, 200);
        client.handle_caps_event(tag, &parse_llsd_xml(&body)?, now)?;
        Ok(drain_client(client))
    }

    /// The `SimulatorFeatures` GET serves the stored document through the
    /// client's own parser, and its `lsl_syntax_id` matches the id
    /// `set_lsl_syntax` advertised — the cross-cap consistency invariant.
    #[test]
    fn simulator_features_serve_the_stored_features() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        seed_region_info(&mut sim);
        let now = Instant::now();
        let mut client = new_client()?;
        let path = granted_cap_path(&caps, CAP_SIMULATOR_FEATURES)?;
        let events = fold_into_client(
            &mut caps,
            &mut sim,
            &mut client,
            &get(&path, None),
            CAP_SIMULATOR_FEATURES,
            now,
        )?;
        match events.as_slice() {
            [Event::SimulatorFeatures(features)] => {
                assert_eq!(features.mesh_rez_enabled, Some(true));
                assert_eq!(features.max_agent_attachments, Some(38));
                assert_eq!(
                    features.lsl_syntax_id,
                    Some(uuid::Uuid::from_u128(SYNTAX_ID))
                );
            }
            other => return Err(format!("expected SimulatorFeatures, got {other:?}").into()),
        }
        Ok(())
    }

    /// The `LSLSyntax` GET serves the stored document (with the version the
    /// client's parser insists on) through the client's own parser.
    #[test]
    fn lsl_syntax_serves_the_stored_document() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        seed_region_info(&mut sim);
        let now = Instant::now();
        let mut client = new_client()?;
        let path = granted_cap_path(&caps, CAP_LSL_SYNTAX)?;
        let events = fold_into_client(
            &mut caps,
            &mut sim,
            &mut client,
            &get(&path, None),
            CAP_LSL_SYNTAX,
            now,
        )?;
        match events.as_slice() {
            [Event::LslSyntax(syntax)] => {
                assert_eq!(
                    syntax
                        .controls
                        .get("if")
                        .and_then(|kw| kw.tooltip.as_deref()),
                    Some("Conditional.")
                );
            }
            other => return Err(format!("expected LslSyntax, got {other:?}").into()),
        }
        Ok(())
    }

    /// The `ExtEnvironment` GET serves the region entry (`?parcelid=-1` and
    /// no query at all), a stored parcel override, and falls back to the
    /// region entry for a parcel without one.
    #[test]
    fn environment_get_serves_region_and_parcel_with_fallback() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        seed_region_info(&mut sim);
        let now = Instant::now();
        let mut client = new_client()?;
        let path = granted_cap_path(&caps, CAP_EXT_ENVIRONMENT)?;
        for (query, expected_parcel, expected_day_length) in [
            (None, -1, 14400),
            (Some("parcelid=-1"), -1, 14400),
            (Some("parcelid=3"), 3, 7200),
            // Parcel 9 has no override: it inherits the region entry.
            (Some("parcelid=9"), -1, 14400),
        ] {
            let events = fold_into_client(
                &mut caps,
                &mut sim,
                &mut client,
                &get(&path, query),
                CAP_EXT_ENVIRONMENT,
                now,
            )?;
            match events.as_slice() {
                [Event::Environment(environment)] => {
                    assert_eq!(environment.parcel_id, expected_parcel, "query {query:?}");
                    assert_eq!(
                        environment.day_length, expected_day_length,
                        "query {query:?}"
                    );
                }
                other => return Err(format!("expected Environment, got {other:?}").into()),
            }
        }
        Ok(())
    }

    /// The region environment a fresh sim serves **determines the sky**: it
    /// carries a real sky and water frame, and resolves to the same frame at
    /// every day position.
    ///
    /// Both halves are the point of the fixture. An environment with an empty
    /// day cycle says nothing about the sky, so each client renders its own
    /// built-in default and two viewers pointed at the same region disagree for
    /// reasons that have nothing to do with either renderer. And a cycle with
    /// more than one keyframe would make the answer depend on the region clock,
    /// so two captures minutes apart would not be comparable either.
    #[test]
    fn the_served_region_environment_pins_the_sky_at_every_day_position() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        seed_region_info(&mut sim);
        let now = Instant::now();
        let mut client = new_client()?;
        let path = granted_cap_path(&caps, CAP_EXT_ENVIRONMENT)?;
        let events = fold_into_client(
            &mut caps,
            &mut sim,
            &mut client,
            &get(&path, Some("parcelid=-1")),
            CAP_EXT_ENVIRONMENT,
            now,
        )?;
        let [Event::Environment(environment)] = events.as_slice() else {
            return Err(format!("expected Environment, got {events:?}").into());
        };
        assert_eq!(environment.day_cycle.sky_frames.len(), 1);
        assert_eq!(environment.day_cycle.water_frames.len(), 1);
        let mut skies = Vec::new();
        for position in [0.0_f32, 0.25, 0.5, 0.75, 1.0] {
            skies.push(
                environment
                    .blended_sky_settings(0.0, position)
                    .ok_or("the served environment resolves to no sky")?,
            );
        }
        let first = skies.first().ok_or("no sky sampled")?;
        for sky in &skies {
            assert_eq!(sky, first, "the served sky depends on the day position");
        }
        Ok(())
    }

    /// The `ExtEnvironment` PUT merges the update into the store (bumping
    /// `env_version`), echoes the stored result through the client fold,
    /// surfaces [`ServerEvent::EnvironmentUpdated`] for the driver, and the
    /// driver's follow-up `WindLightRefresh` push tells the client to
    /// re-fetch — which then observes the update.
    #[test]
    fn environment_put_updates_the_stored_environment() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        seed_region_info(&mut sim);
        let now = Instant::now();
        let mut client = new_client()?;
        let path = granted_cap_path(&caps, CAP_EXT_ENVIRONMENT)?;

        let update_body = build_environment_update_request(&EnvironmentUpdate {
            day_length: Some(28800),
            day_offset: Some(0),
            track_altitudes: Some([800.0, 1600.0, 2400.0]),
            flags: 1,
            ..EnvironmentUpdate::default()
        });
        let events = fold_into_client(
            &mut caps,
            &mut sim,
            &mut client,
            &put(&path, Some("parcelid=-1&trackno=1"), &update_body),
            CAP_EXT_ENVIRONMENT,
            now,
        )?;
        match events.as_slice() {
            [Event::Environment(environment)] => {
                assert_eq!(environment.parcel_id, -1);
                assert_eq!(environment.day_length, 28800);
                assert_eq!(environment.day_offset, 0);
                assert_eq!(environment.flags, 1);
                // The seeded region entry was version 1; the PUT bumps it.
                assert_eq!(environment.env_version, 2);
            }
            other => return Err(format!("expected Environment, got {other:?}").into()),
        }
        match sim.poll_event() {
            Some(ServerEvent::EnvironmentUpdated {
                parcel_id,
                track_no,
                update,
            }) => {
                assert_eq!(parcel_id, -1);
                assert_eq!(track_no, Some(1));
                assert_eq!(update.day_length, Some(28800));
            }
            other => return Err(format!("expected EnvironmentUpdated, got {other:?}").into()),
        }

        // The driver notifies other clients over the event queue; the client
        // re-fetches on the refresh and observes the stored update.
        sim.enqueue_windlight_refresh(true);
        let eq_path = granted_event_queue_path(&caps)?;
        let poll = build_event_queue_request(None, false);
        let (status, body) = respond(&mut caps, &mut sim, &post(&eq_path, &poll))?;
        assert_eq!(status, 200);
        let batch = parse_event_queue_response(&body)?;
        let refresh = batch
            .events
            .first()
            .ok_or("expected a queued WindLightRefresh event")?;
        assert_eq!(refresh.message, "WindLightRefresh");
        client.handle_caps_event(&refresh.message, &refresh.body, now)?;
        assert!(matches!(
            drain_client(&mut client).as_slice(),
            [Event::WindLightRefresh { .. }]
        ));
        let events = fold_into_client(
            &mut caps,
            &mut sim,
            &mut client,
            &get(&path, Some("parcelid=-1")),
            CAP_EXT_ENVIRONMENT,
            now,
        )?;
        match events.as_slice() {
            [Event::Environment(environment)] => assert_eq!(environment.day_length, 28800),
            other => return Err(format!("expected Environment, got {other:?}").into()),
        }
        Ok(())
    }

    /// A `day_asset`-only PUT answers the reference's graceful failure —
    /// `200 { success: false, message }` — because the fixture has no
    /// settings-asset store; nothing is stored and no client event fires.
    #[test]
    fn environment_put_without_day_cycle_asset_store_fails_gracefully() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        seed_region_info(&mut sim);
        let now = Instant::now();
        let mut client = new_client()?;
        let path = granted_cap_path(&caps, CAP_EXT_ENVIRONMENT)?;
        let update_body = build_environment_update_request(&EnvironmentUpdate {
            day_asset: Some(uuid::Uuid::from_u128(0xda)),
            day_name: Some("A Preset".to_owned()),
            flags: 0,
            ..EnvironmentUpdate::default()
        });
        let (status, body) = respond(&mut caps, &mut sim, &put(&path, None, &update_body))?;
        assert_eq!(status, 200);
        let reply = parse_llsd_xml(&body)?;
        assert_eq!(reply.get("success").and_then(Llsd::as_bool), Some(false));
        assert!(
            reply
                .get("message")
                .and_then(Llsd::as_str)
                .is_some_and(|message| !message.is_empty())
        );
        // The failure reply carries no `environment` envelope: the client
        // fold surfaces nothing (its decode-failed diagnostic path).
        client.handle_caps_event(CAP_EXT_ENVIRONMENT, &reply, now)?;
        assert!(
            drain_client(&mut client)
                .iter()
                .all(|event| !matches!(event, Event::Environment(..)))
        );
        assert!(sim.poll_event().is_none());
        Ok(())
    }

    /// The `RemoteParcelRequest` lookup resolves a covered location by region
    /// id and by region handle; a foreign region or an uncovered location
    /// answers the empty "could not resolve" map (no client event).
    #[test]
    fn remote_parcel_request_resolves_the_covering_parcel() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        seed_region_info(&mut sim);
        let now = Instant::now();
        let mut client = new_client()?;
        let path = granted_cap_path(&caps, CAP_REMOTE_PARCEL_REQUEST)?;

        // By region id: (64, 100) falls in the first (western) rectangle.
        let body = build_remote_parcel_request(
            RegionCoordinates::new(64.0, 100.0, 0.0),
            uuid::Uuid::from_u128(0x1e6),
            RegionHandle(0),
        );
        let events = fold_into_client(
            &mut caps,
            &mut sim,
            &mut client,
            &post(&path, &body),
            CAP_REMOTE_PARCEL_REQUEST,
            now,
        )?;
        assert_eq!(
            events,
            vec![Event::RemoteParcelId(ParcelKey::from(
                uuid::Uuid::from_u128(0xACE1)
            ))]
        );

        // By region handle: (200, 10) falls in the second (eastern) one.
        let body = build_remote_parcel_request(
            RegionCoordinates::new(200.0, 10.0, 0.0),
            uuid::Uuid::nil(),
            RegionHandle(REGION_HANDLE),
        );
        let events = fold_into_client(
            &mut caps,
            &mut sim,
            &mut client,
            &post(&path, &body),
            CAP_REMOTE_PARCEL_REQUEST,
            now,
        )?;
        assert_eq!(
            events,
            vec![Event::RemoteParcelId(ParcelKey::from(
                uuid::Uuid::from_u128(0xACE2)
            ))]
        );

        // A foreign region and an uncovered location both answer `{}`; the
        // client's fold treats that as a failed resolve and surfaces no
        // typed event.
        for body in [
            build_remote_parcel_request(
                RegionCoordinates::new(64.0, 100.0, 0.0),
                uuid::Uuid::from_u128(0xbad),
                RegionHandle(0),
            ),
            build_remote_parcel_request(
                RegionCoordinates::new(64.0, 300.0, 0.0),
                uuid::Uuid::from_u128(0x1e6),
                RegionHandle(0),
            ),
        ] {
            let (status, reply) = respond(&mut caps, &mut sim, &post(&path, &body))?;
            assert_eq!(status, 200);
            client.handle_caps_event(CAP_REMOTE_PARCEL_REQUEST, &parse_llsd_xml(&reply)?, now)?;
            assert!(
                drain_client(&mut client)
                    .iter()
                    .all(|event| !matches!(event, Event::RemoteParcelId(..)))
            );
        }
        Ok(())
    }

    /// The `GetObjectCost` POST serves the stored costs of the requested
    /// objects through the client's own parser; an unknown id is omitted (the
    /// "no such object" signal).
    #[test]
    fn object_cost_serves_the_stored_costs() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        seed_region_info(&mut sim);
        let now = Instant::now();
        let mut client = new_client()?;
        let path = granted_cap_path(&caps, CAP_GET_OBJECT_COST)?;
        let body = build_get_object_cost_request(&[
            ObjectKey::from(uuid::Uuid::from_u128(OBJECT_A)),
            ObjectKey::from(uuid::Uuid::from_u128(OBJECT_UNKNOWN)),
            ObjectKey::from(uuid::Uuid::from_u128(OBJECT_B)),
        ]);
        let events = fold_into_client(
            &mut caps,
            &mut sim,
            &mut client,
            &post(&path, &body),
            CAP_GET_OBJECT_COST,
            now,
        )?;
        match events.as_slice() {
            [Event::ObjectCosts(costs)] => {
                let mut expected = vec![
                    (
                        ObjectKey::from(uuid::Uuid::from_u128(OBJECT_A)),
                        sample_object_cost(16.0),
                    ),
                    (
                        ObjectKey::from(uuid::Uuid::from_u128(OBJECT_B)),
                        sample_object_cost(8.0),
                    ),
                ];
                expected.sort_by_key(|(id, _cost)| id.uuid());
                assert_eq!(costs, &expected);
            }
            other => return Err(format!("expected ObjectCosts, got {other:?}").into()),
        }
        Ok(())
    }

    /// The `GetObjectPhysicsData` POST serves the stored physics data of the
    /// requested objects; an unknown id is omitted.
    #[test]
    fn object_physics_data_serves_the_stored_data() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        seed_region_info(&mut sim);
        let now = Instant::now();
        let mut client = new_client()?;
        let path = granted_cap_path(&caps, CAP_GET_OBJECT_PHYSICS_DATA)?;
        let body = build_get_object_physics_data_request(&[
            ObjectKey::from(uuid::Uuid::from_u128(OBJECT_B)),
            ObjectKey::from(uuid::Uuid::from_u128(OBJECT_UNKNOWN)),
        ]);
        let events = fold_into_client(
            &mut caps,
            &mut sim,
            &mut client,
            &post(&path, &body),
            CAP_GET_OBJECT_PHYSICS_DATA,
            now,
        )?;
        match events.as_slice() {
            [Event::ObjectPhysicsData(data)] => match data.as_slice() {
                [(id, physics)] => {
                    assert_eq!(*id, ObjectKey::from(uuid::Uuid::from_u128(OBJECT_B)));
                    assert_eq!(physics.physics_shape_type, PhysicsShapeType::ConvexHull);
                    assert_eq!(physics.gravity_multiplier.to_bits(), 2.0_f32.to_bits());
                }
                other => return Err(format!("expected one record, got {other:?}").into()),
            },
            other => return Err(format!("expected ObjectPhysicsData, got {other:?}").into()),
        }
        Ok(())
    }

    /// The `ResourceCostSelected` POST sums the stored selection costs of the
    /// requested objects — in both the roots and prims request forms —
    /// with unknown ids contributing zero.
    #[test]
    fn resource_cost_selected_sums_the_selection() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        seed_region_info(&mut sim);
        let now = Instant::now();
        let mut client = new_client()?;
        let path = granted_cap_path(&caps, CAP_RESOURCE_COST_SELECTED)?;
        for kind in [SelectedCostKind::Roots, SelectedCostKind::Prims] {
            let body = build_resource_cost_selected_request(
                kind,
                &[
                    ObjectKey::from(uuid::Uuid::from_u128(OBJECT_A)),
                    ObjectKey::from(uuid::Uuid::from_u128(OBJECT_B)),
                    ObjectKey::from(uuid::Uuid::from_u128(OBJECT_UNKNOWN)),
                ],
            );
            let events = fold_into_client(
                &mut caps,
                &mut sim,
                &mut client,
                &post(&path, &body),
                CAP_RESOURCE_COST_SELECTED,
                now,
            )?;
            match events.as_slice() {
                [Event::SelectedResourceCost(cost)] => {
                    assert_eq!(cost.physics.to_bits(), 1.5_f32.to_bits());
                    assert_eq!(cost.streaming.to_bits(), 2.25_f32.to_bits());
                    assert_eq!(cost.simulation.to_bits(), 3.125_f32.to_bits());
                }
                other => return Err(format!("expected SelectedResourceCost, got {other:?}").into()),
            }
        }
        Ok(())
    }

    /// The `AttachmentResources` GET serves the stored report through the
    /// client's own parser.
    #[test]
    fn attachment_resources_serve_the_stored_report() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        seed_region_info(&mut sim);
        let now = Instant::now();
        let mut client = new_client()?;
        let path = granted_cap_path(&caps, CAP_ATTACHMENT_RESOURCES)?;
        let events = fold_into_client(
            &mut caps,
            &mut sim,
            &mut client,
            &get(&path, None),
            CAP_ATTACHMENT_RESOURCES,
            now,
        )?;
        match events.as_slice() {
            [Event::AttachmentResources(report)] => {
                assert_eq!(report.attachments.len(), 1);
                assert_eq!(
                    report
                        .attachments
                        .first()
                        .map(|location| location.location.as_str()),
                    Some("Skull")
                );
                assert_eq!(
                    report.summary.used.first().map(|amount| amount.amount),
                    Some(0x0001_0000)
                );
            }
            other => return Err(format!("expected AttachmentResources, got {other:?}").into()),
        }
        Ok(())
    }

    /// The two-stage `LandResources` flow: the POST answers the two follow-up
    /// URLs (sub-paths of the cap's own URL), and GETting each serves the
    /// stored summary/detail reports through the client's own parsers under
    /// the runtime's `LAND_RESOURCE_*_TAG` pseudo-cap names.
    #[test]
    fn land_resources_serves_the_summary_and_detail_reports() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        seed_region_info(&mut sim);
        let now = Instant::now();
        let mut client = new_client()?;
        let path = granted_cap_path(&caps, CAP_LAND_RESOURCES)?;
        let body = build_land_resources_request(ParcelKey::from(uuid::Uuid::from_u128(0xACE1)));
        let events = fold_into_client(
            &mut caps,
            &mut sim,
            &mut client,
            &post(&path, &body),
            CAP_LAND_RESOURCES,
            now,
        )?;
        let urls = match events.as_slice() {
            [Event::LandResourcesUrls(urls)] => urls.clone(),
            other => return Err(format!("expected LandResourcesUrls, got {other:?}").into()),
        };
        let summary_path = urls
            .script_resource_summary
            .ok_or("expected a summary URL")?
            .path()
            .to_owned();
        let detail_path = urls
            .script_resource_details
            .ok_or("expected a details URL")?
            .path()
            .to_owned();

        let events = fold_into_client(
            &mut caps,
            &mut sim,
            &mut client,
            &get(&summary_path, None),
            LAND_RESOURCE_SUMMARY_TAG,
            now,
        )?;
        match events.as_slice() {
            [Event::LandResourceSummary(summary)] => {
                assert_eq!(
                    summary.available.first().map(|amount| amount.amount),
                    Some(38)
                );
            }
            other => return Err(format!("expected LandResourceSummary, got {other:?}").into()),
        }

        let events = fold_into_client(
            &mut caps,
            &mut sim,
            &mut client,
            &get(&detail_path, None),
            LAND_RESOURCE_DETAIL_TAG,
            now,
        )?;
        match events.as_slice() {
            [Event::LandResourceDetail(parcels)] => {
                assert_eq!(
                    parcels.first().map(|parcel| parcel.name.as_str()),
                    Some("Test Parcel")
                );
                assert_eq!(
                    parcels
                        .first()
                        .and_then(|parcel| parcel.objects.first())
                        .map(|object| object.name.as_str()),
                    Some("Greeter")
                );
            }
            other => return Err(format!("expected LandResourceDetail, got {other:?}").into()),
        }
        Ok(())
    }

    /// The region/object-information handlers' status contract: wrong
    /// methods 405 (including the reference's DELETE reset, out of scope),
    /// malformed queries and bodies 400, unknown `LandResources` sub-paths
    /// 404.
    #[test]
    fn region_info_handlers_reject_wrong_methods_and_bad_bodies() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        seed_region_info(&mut sim);

        // The bodyless GETs are GET-only.
        for name in [
            CAP_SIMULATOR_FEATURES,
            CAP_LSL_SYNTAX,
            CAP_ATTACHMENT_RESOURCES,
        ] {
            let path = granted_cap_path(&caps, name)?;
            let (status, _) = respond(&mut caps, &mut sim, &post(&path, ""))?;
            assert_eq!(status, 405, "POST on {name}");
        }

        // The POST caps are POST-only and reject garbage bodies.
        for name in [
            CAP_REMOTE_PARCEL_REQUEST,
            CAP_GET_OBJECT_COST,
            CAP_GET_OBJECT_PHYSICS_DATA,
            CAP_RESOURCE_COST_SELECTED,
            CAP_LAND_RESOURCES,
        ] {
            let path = granted_cap_path(&caps, name)?;
            let (status, _) = respond(&mut caps, &mut sim, &get(&path, None))?;
            assert_eq!(status, 405, "GET on {name}");
            let (status, _) = respond(&mut caps, &mut sim, &post(&path, "not xml <"))?;
            assert_eq!(status, 400, "garbage body on {name}");
        }

        // ExtEnvironment: a malformed `parcelid` is a bad request, a PUT
        // without the `environment` envelope is a bad request, and any other
        // method (the DELETE reset stays unimplemented) is 405.
        let path = granted_cap_path(&caps, CAP_EXT_ENVIRONMENT)?;
        let (status, _) = respond(&mut caps, &mut sim, &get(&path, Some("parcelid=abc")))?;
        assert_eq!(status, 400);
        let (status, _) = respond(
            &mut caps,
            &mut sim,
            &put(&path, None, "<llsd><map/></llsd>"),
        )?;
        assert_eq!(status, 400);
        let (status, _) = respond(
            &mut caps,
            &mut sim,
            &CapsRequest {
                method: "DELETE",
                path: &path,
                query: Some("parcelid=-1"),
                range: None,
                body: b"",
            },
        )?;
        assert_eq!(status, 405);

        // LandResources: an unknown sub-path is 404; the follow-up GETs are
        // GET-only.
        let path = granted_cap_path(&caps, CAP_LAND_RESOURCES)?;
        let bogus = format!("{path}/bogus");
        let (status, _) = respond(&mut caps, &mut sim, &get(&bogus, None))?;
        assert_eq!(status, 404);
        let summary = format!("{path}/summary");
        let (status, _) = respond(&mut caps, &mut sim, &post(&summary, ""))?;
        assert_eq!(status, 405);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // The experience cluster.
    // -----------------------------------------------------------------------

    /// A `DELETE` [`CapsRequest`] carrying a query string (no body).
    fn delete<'a>(path: &'a str, query: Option<&'a str>) -> CapsRequest<'a> {
        CapsRequest {
            method: "DELETE",
            path,
            query,
            range: None,
            body: b"",
        }
    }

    /// Splits a client-built URL suffix (`/sub/?query` or `?query`) into the
    /// full request path under the granted cap path and the bare query
    /// string — what the runtime's HTTP layer does before the request
    /// reaches [`SimCaps::dispatch`].
    fn split_suffix(cap_path: &str, suffix: &str) -> (String, Option<String>) {
        match suffix.split_once('?') {
            Some((sub_path, query)) => (format!("{cap_path}{sub_path}"), Some(query.to_owned())),
            None => (format!("{cap_path}{suffix}"), None),
        }
    }

    /// A public experience the agent owns, admins and has allowed.
    const EXP_A: u128 = 0x0E0A;
    /// A private (`PROPERTY_PRIVATE`) experience the agent has blocked.
    const EXP_B: u128 = 0x0E0B;
    /// A group-owned experience the agent admins and created.
    const EXP_C: u128 = 0x0E0C;
    /// An experience id no fixture knows — the "could not resolve" probe.
    const EXP_UNKNOWN: u128 = 0x0EFF;
    /// The group that owns [`EXP_C`].
    const EXP_GROUP: u128 = 0x0E60;

    /// The [`ExperienceKey`] for one of the `EXP_*` constants.
    fn exp_key(id: u128) -> ExperienceKey {
        ExperienceKey::from(uuid::Uuid::from_u128(id))
    }

    /// Seeds the experience fixture set: three records (one private), the
    /// agent's permission/owned/admin/creator lists, one group list, and
    /// the region triple.
    fn seed_experiences(sim: &mut SimSession) {
        let experiences = sim.experiences_mut();
        experiences.insert(ExperienceInfo {
            public_id: exp_key(EXP_A),
            name: "Magic Quest".to_owned(),
            owner: Some(OwnerKey::Agent(AgentKey::from(uuid::Uuid::from_u128(1)))),
            description: "A quest of magic".to_owned(),
            quota: 128,
            maturity: 13,
            ..ExperienceInfo::default()
        });
        experiences.insert(ExperienceInfo {
            public_id: exp_key(EXP_B),
            name: "Magic Dungeon".to_owned(),
            properties: ExperienceProperties(PROPERTY_PRIVATE),
            ..ExperienceInfo::default()
        });
        experiences.insert(ExperienceInfo {
            public_id: exp_key(EXP_C),
            name: "Tour Guide".to_owned(),
            owner: Some(OwnerKey::Group(GroupKey::from(uuid::Uuid::from_u128(
                EXP_GROUP,
            )))),
            ..ExperienceInfo::default()
        });
        experiences.set_agent_permissions(vec![exp_key(EXP_A)], vec![exp_key(EXP_B)]);
        experiences.set_owned(vec![exp_key(EXP_A)]);
        experiences.set_admin(vec![exp_key(EXP_A), exp_key(EXP_C)]);
        experiences.set_creator(vec![exp_key(EXP_C)]);
        experiences.set_group(uuid::Uuid::from_u128(EXP_GROUP), vec![exp_key(EXP_C)]);
        experiences.set_region_lists(
            vec![exp_key(EXP_A)],
            vec![exp_key(EXP_B)],
            vec![exp_key(EXP_C)],
        );
    }

    /// The `GetExperienceInfo` GET serves the stored records through the
    /// client's own query builder and reply parser; an unknown id comes
    /// back as an `error_ids` entry the client folds into a `missing`
    /// placeholder.
    #[test]
    fn get_experience_info_serves_records_and_error_ids() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        seed_experiences(&mut sim);
        let now = Instant::now();
        let mut client = new_client()?;
        let cap_path = granted_cap_path(&caps, CAP_GET_EXPERIENCE_INFO)?;
        let suffix = experience_info_query(&[exp_key(EXP_A), exp_key(EXP_UNKNOWN)]);
        let (path, query) = split_suffix(&cap_path, &suffix);
        let events = fold_into_client(
            &mut caps,
            &mut sim,
            &mut client,
            &get(&path, query.as_deref()),
            CAP_GET_EXPERIENCE_INFO,
            now,
        )?;
        match events.as_slice() {
            [Event::ExperienceInfo(infos)] => {
                assert_eq!(
                    infos
                        .iter()
                        .map(|info| (info.public_id, info.missing))
                        .collect::<Vec<_>>(),
                    vec![(exp_key(EXP_A), false), (exp_key(EXP_UNKNOWN), true)]
                );
                assert_eq!(
                    infos.first().map(|info| info.name.as_str()),
                    Some("Magic Quest")
                );
                assert_eq!(
                    infos.first().and_then(|info| info.owner),
                    Some(OwnerKey::Agent(AgentKey::from(uuid::Uuid::from_u128(1))))
                );
            }
            other => return Err(format!("expected ExperienceInfo, got {other:?}").into()),
        }
        Ok(())
    }

    /// The `FindExperienceByName` GET matches case-insensitively, hides the
    /// private record, and answers an empty second page.
    #[test]
    fn find_experience_by_name_pages_and_hides_private() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        seed_experiences(&mut sim);
        let now = Instant::now();
        let mut client = new_client()?;
        let cap_path = granted_cap_path(&caps, CAP_FIND_EXPERIENCE_BY_NAME)?;
        for (page, expected) in [(1, vec![exp_key(EXP_A)]), (2, Vec::new())] {
            let suffix = find_experience_query("mAgIc", page);
            let (path, query) = split_suffix(&cap_path, &suffix);
            let events = fold_into_client(
                &mut caps,
                &mut sim,
                &mut client,
                &get(&path, query.as_deref()),
                CAP_FIND_EXPERIENCE_BY_NAME,
                now,
            )?;
            match events.as_slice() {
                [Event::ExperienceSearchResults(infos)] => {
                    assert_eq!(
                        infos.iter().map(|info| info.public_id).collect::<Vec<_>>(),
                        expected,
                        "page {page}"
                    );
                }
                other => {
                    return Err(format!("expected ExperienceSearchResults, got {other:?}").into());
                }
            }
        }
        Ok(())
    }

    /// The bodyless `GetExperiences` GET serves the agent's allowed /
    /// blocked lists through the client fold.
    #[test]
    fn get_experiences_serves_the_permission_lists() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        seed_experiences(&mut sim);
        let now = Instant::now();
        let mut client = new_client()?;
        let path = granted_cap_path(&caps, CAP_GET_EXPERIENCES)?;
        let events = fold_into_client(
            &mut caps,
            &mut sim,
            &mut client,
            &get(&path, None),
            CAP_GET_EXPERIENCES,
            now,
        )?;
        match events.as_slice() {
            [Event::ExperiencePermissions { allowed, blocked }] => {
                assert_eq!(allowed, &vec![exp_key(EXP_A)]);
                assert_eq!(blocked, &vec![exp_key(EXP_B)]);
            }
            other => return Err(format!("expected ExperiencePermissions, got {other:?}").into()),
        }
        Ok(())
    }

    /// The bodyless `AgentExperiences` GET serves the owned list.
    #[test]
    fn agent_experiences_serves_owned_ids() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        seed_experiences(&mut sim);
        let now = Instant::now();
        let mut client = new_client()?;
        let path = granted_cap_path(&caps, CAP_AGENT_EXPERIENCES)?;
        let events = fold_into_client(
            &mut caps,
            &mut sim,
            &mut client,
            &get(&path, None),
            CAP_AGENT_EXPERIENCES,
            now,
        )?;
        match events.as_slice() {
            [Event::OwnedExperiences(ids)] => assert_eq!(ids, &vec![exp_key(EXP_A)]),
            other => return Err(format!("expected OwnedExperiences, got {other:?}").into()),
        }
        Ok(())
    }

    /// The bodyless `GetAdminExperiences` / `GetCreatorExperiences` GETs
    /// serve their respective lists, name-routed through one handler.
    #[test]
    fn admin_and_creator_experiences_serve_their_lists() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        seed_experiences(&mut sim);
        let now = Instant::now();
        let mut client = new_client()?;
        let admin_path = granted_cap_path(&caps, CAP_GET_ADMIN_EXPERIENCES)?;
        let events = fold_into_client(
            &mut caps,
            &mut sim,
            &mut client,
            &get(&admin_path, None),
            CAP_GET_ADMIN_EXPERIENCES,
            now,
        )?;
        match events.as_slice() {
            [Event::AdminExperiences(ids)] => {
                assert_eq!(ids, &vec![exp_key(EXP_A), exp_key(EXP_C)]);
            }
            other => return Err(format!("expected AdminExperiences, got {other:?}").into()),
        }
        let creator_path = granted_cap_path(&caps, CAP_GET_CREATOR_EXPERIENCES)?;
        let events = fold_into_client(
            &mut caps,
            &mut sim,
            &mut client,
            &get(&creator_path, None),
            CAP_GET_CREATOR_EXPERIENCES,
            now,
        )?;
        match events.as_slice() {
            [Event::CreatorExperiences(ids)] => assert_eq!(ids, &vec![exp_key(EXP_C)]),
            other => return Err(format!("expected CreatorExperiences, got {other:?}").into()),
        }
        Ok(())
    }

    /// The `GroupExperiences` GET serves the queried group's list (an
    /// unknown group answers empty). The reply does not echo the group id,
    /// so the runtimes parse it out-of-band — this test parses the raw
    /// reply exactly as they do.
    #[test]
    fn group_experiences_serves_the_group_list() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        seed_experiences(&mut sim);
        let cap_path = granted_cap_path(&caps, CAP_GROUP_EXPERIENCES)?;
        let suffix = group_experiences_query(uuid::Uuid::from_u128(EXP_GROUP));
        let (path, query) = split_suffix(&cap_path, &suffix);
        let (status, body) = respond(&mut caps, &mut sim, &get(&path, query.as_deref()))?;
        assert_eq!(status, 200);
        assert_eq!(
            parse_experience_ids(&parse_llsd_xml(&body)?)?,
            vec![exp_key(EXP_C)]
        );
        // An unknown group answers an empty list, not an error.
        let suffix = group_experiences_query(uuid::Uuid::from_u128(0xDEAD));
        let (path, query) = split_suffix(&cap_path, &suffix);
        let (status, body) = respond(&mut caps, &mut sim, &get(&path, query.as_deref()))?;
        assert_eq!(status, 200);
        assert_eq!(parse_experience_ids(&parse_llsd_xml(&body)?)?, Vec::new());
        Ok(())
    }

    /// The `IsExperienceAdmin` / `IsExperienceContributor` GETs answer the
    /// store's admin / creator membership; unknown ids answer `false`. The
    /// reply does not echo the queried id, so the runtimes parse it
    /// out-of-band — this test parses the raw replies exactly as they do.
    #[test]
    fn is_experience_admin_and_contributor_answer_from_the_lists() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        seed_experiences(&mut sim);
        for (name, id, expected) in [
            (CAP_IS_EXPERIENCE_ADMIN, EXP_A, true),
            (CAP_IS_EXPERIENCE_ADMIN, EXP_UNKNOWN, false),
            (CAP_IS_EXPERIENCE_CONTRIBUTOR, EXP_C, true),
            (CAP_IS_EXPERIENCE_CONTRIBUTOR, EXP_A, false),
        ] {
            let cap_path = granted_cap_path(&caps, name)?;
            let suffix = experience_id_query(exp_key(id));
            let (path, query) = split_suffix(&cap_path, &suffix);
            let (status, body) = respond(&mut caps, &mut sim, &get(&path, query.as_deref()))?;
            assert_eq!(status, 200, "{name} for {id:#x}");
            assert_eq!(
                parse_experience_status(&parse_llsd_xml(&body)?)?,
                expected,
                "{name} for {id:#x}"
            );
        }
        Ok(())
    }

    /// The `ExperiencePreferences` PUT moves an id between the allowed /
    /// blocked lists, echoes the full post-mutation lists through the
    /// client fold, surfaces [`ServerEvent::ExperiencePermissionSet`], and
    /// a follow-up `GetExperiences` observes the move.
    #[test]
    fn experience_preferences_put_moves_between_lists() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        seed_experiences(&mut sim);
        let now = Instant::now();
        let mut client = new_client()?;
        let path = granted_cap_path(&caps, CAP_EXPERIENCE_PREFERENCES)?;
        let body =
            build_set_experience_permission_request(exp_key(EXP_B), ExperiencePermission::Allow);
        let events = fold_into_client(
            &mut caps,
            &mut sim,
            &mut client,
            &put(&path, None, &body),
            CAP_EXPERIENCE_PREFERENCES,
            now,
        )?;
        match events.as_slice() {
            [Event::ExperiencePermissions { allowed, blocked }] => {
                assert_eq!(allowed, &vec![exp_key(EXP_A), exp_key(EXP_B)]);
                assert_eq!(blocked, &Vec::new());
            }
            other => return Err(format!("expected ExperiencePermissions, got {other:?}").into()),
        }
        match sim.poll_event() {
            Some(ServerEvent::ExperiencePermissionSet {
                experience_id,
                permission,
            }) => {
                assert_eq!(experience_id, exp_key(EXP_B));
                assert_eq!(permission, ExperiencePermission::Allow);
            }
            other => {
                return Err(format!("expected ExperiencePermissionSet, got {other:?}").into());
            }
        }
        // The mutation is fixture state: a follow-up read observes it.
        let get_path = granted_cap_path(&caps, CAP_GET_EXPERIENCES)?;
        let events = fold_into_client(
            &mut caps,
            &mut sim,
            &mut client,
            &get(&get_path, None),
            CAP_GET_EXPERIENCES,
            now,
        )?;
        match events.as_slice() {
            [Event::ExperiencePermissions { allowed, blocked }] => {
                assert_eq!(allowed, &vec![exp_key(EXP_A), exp_key(EXP_B)]);
                assert_eq!(blocked, &Vec::new());
            }
            other => return Err(format!("expected ExperiencePermissions, got {other:?}").into()),
        }
        Ok(())
    }

    /// The `ExperiencePreferences` DELETE (the client's `Forget` form)
    /// removes the id from both lists and surfaces the `Forget` event.
    #[test]
    fn experience_preferences_delete_forgets() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        seed_experiences(&mut sim);
        let now = Instant::now();
        let mut client = new_client()?;
        let cap_path = granted_cap_path(&caps, CAP_EXPERIENCE_PREFERENCES)?;
        let suffix = forget_experience_query(exp_key(EXP_A));
        let (path, query) = split_suffix(&cap_path, &suffix);
        let events = fold_into_client(
            &mut caps,
            &mut sim,
            &mut client,
            &delete(&path, query.as_deref()),
            CAP_EXPERIENCE_PREFERENCES,
            now,
        )?;
        match events.as_slice() {
            [Event::ExperiencePermissions { allowed, blocked }] => {
                assert_eq!(allowed, &Vec::new());
                assert_eq!(blocked, &vec![exp_key(EXP_B)]);
            }
            other => return Err(format!("expected ExperiencePermissions, got {other:?}").into()),
        }
        match sim.poll_event() {
            Some(ServerEvent::ExperiencePermissionSet {
                experience_id,
                permission,
            }) => {
                assert_eq!(experience_id, exp_key(EXP_A));
                assert_eq!(permission, ExperiencePermission::Forget);
            }
            other => {
                return Err(format!("expected ExperiencePermissionSet, got {other:?}").into());
            }
        }
        Ok(())
    }

    /// The `UpdateExperience` POST applies the editable fields (preserving
    /// the server-controlled quota), echoes the updated record through the
    /// client fold, surfaces [`ServerEvent::ExperienceUpdated`], and a
    /// follow-up `GetExperienceInfo` observes the edit.
    #[test]
    fn update_experience_applies_and_echoes() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        seed_experiences(&mut sim);
        let now = Instant::now();
        let mut client = new_client()?;
        let path = granted_cap_path(&caps, CAP_UPDATE_EXPERIENCE)?;
        let body = build_update_experience_request(&ExperienceUpdate {
            public_id: exp_key(EXP_A),
            name: "Magic Quest II".to_owned(),
            description: "Now with more magic".to_owned(),
            maturity: 21,
            properties: 0,
            slurl: None,
            extended_metadata: String::new(),
        });
        let events = fold_into_client(
            &mut caps,
            &mut sim,
            &mut client,
            &post(&path, &body),
            CAP_UPDATE_EXPERIENCE,
            now,
        )?;
        match events.as_slice() {
            [Event::ExperienceUpdated(info)] => {
                assert_eq!(info.public_id, exp_key(EXP_A));
                assert_eq!(info.name, "Magic Quest II");
                assert_eq!(info.description, "Now with more magic");
                assert_eq!(info.maturity, 21);
                // Server-controlled fields survive the edit untouched.
                assert_eq!(info.quota, 128);
                assert_eq!(
                    info.owner,
                    Some(OwnerKey::Agent(AgentKey::from(uuid::Uuid::from_u128(1))))
                );
            }
            other => return Err(format!("expected ExperienceUpdated, got {other:?}").into()),
        }
        match sim.poll_event() {
            Some(ServerEvent::ExperienceUpdated { update }) => {
                assert_eq!(update.public_id, exp_key(EXP_A));
                assert_eq!(update.name, "Magic Quest II");
            }
            other => return Err(format!("expected ExperienceUpdated, got {other:?}").into()),
        }
        // The edit is fixture state: a follow-up lookup observes it.
        let info_path = granted_cap_path(&caps, CAP_GET_EXPERIENCE_INFO)?;
        let suffix = experience_info_query(&[exp_key(EXP_A)]);
        let (path, query) = split_suffix(&info_path, &suffix);
        let events = fold_into_client(
            &mut caps,
            &mut sim,
            &mut client,
            &get(&path, query.as_deref()),
            CAP_GET_EXPERIENCE_INFO,
            now,
        )?;
        match events.as_slice() {
            [Event::ExperienceInfo(infos)] => {
                assert_eq!(
                    infos.first().map(|info| info.name.as_str()),
                    Some("Magic Quest II")
                );
            }
            other => return Err(format!("expected ExperienceInfo, got {other:?}").into()),
        }
        Ok(())
    }

    /// The `RegionExperiences` GET serves the seeded triple; the POST
    /// replaces it wholesale, echoes the stored lists, surfaces
    /// [`ServerEvent::RegionExperiencesSet`], and a follow-up GET observes
    /// the replacement.
    #[test]
    fn region_experiences_get_and_post_round_trip() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        seed_experiences(&mut sim);
        let now = Instant::now();
        let mut client = new_client()?;
        let path = granted_cap_path(&caps, CAP_REGION_EXPERIENCES)?;
        let events = fold_into_client(
            &mut caps,
            &mut sim,
            &mut client,
            &get(&path, None),
            CAP_REGION_EXPERIENCES,
            now,
        )?;
        match events.as_slice() {
            [
                Event::RegionExperiences {
                    allowed,
                    blocked,
                    trusted,
                },
            ] => {
                assert_eq!(allowed, &vec![exp_key(EXP_A)]);
                assert_eq!(blocked, &vec![exp_key(EXP_B)]);
                assert_eq!(trusted, &vec![exp_key(EXP_C)]);
            }
            other => return Err(format!("expected RegionExperiences, got {other:?}").into()),
        }
        let body = build_region_experiences_request(
            &[exp_key(EXP_C)],
            &[],
            &[exp_key(EXP_A), exp_key(EXP_B)],
        );
        let events = fold_into_client(
            &mut caps,
            &mut sim,
            &mut client,
            &post(&path, &body),
            CAP_REGION_EXPERIENCES,
            now,
        )?;
        match events.as_slice() {
            [
                Event::RegionExperiences {
                    allowed,
                    blocked,
                    trusted,
                },
            ] => {
                assert_eq!(allowed, &vec![exp_key(EXP_C)]);
                assert_eq!(blocked, &Vec::new());
                assert_eq!(trusted, &vec![exp_key(EXP_A), exp_key(EXP_B)]);
            }
            other => return Err(format!("expected RegionExperiences, got {other:?}").into()),
        }
        match sim.poll_event() {
            Some(ServerEvent::RegionExperiencesSet {
                allowed,
                blocked,
                trusted,
            }) => {
                assert_eq!(allowed, vec![exp_key(EXP_C)]);
                assert_eq!(blocked, Vec::new());
                assert_eq!(trusted, vec![exp_key(EXP_A), exp_key(EXP_B)]);
            }
            other => return Err(format!("expected RegionExperiencesSet, got {other:?}").into()),
        }
        // The replacement is fixture state: a follow-up GET observes it.
        let events = fold_into_client(
            &mut caps,
            &mut sim,
            &mut client,
            &get(&path, None),
            CAP_REGION_EXPERIENCES,
            now,
        )?;
        match events.as_slice() {
            [Event::RegionExperiences { allowed, .. }] => {
                assert_eq!(allowed, &vec![exp_key(EXP_C)]);
            }
            other => return Err(format!("expected RegionExperiences, got {other:?}").into()),
        }
        Ok(())
    }

    /// The experience handlers' status contract: wrong verbs answer `405`,
    /// malformed queries/bodies `400`, and an `UpdateExperience` targeting
    /// an unknown record `404`. (`GetExperienceInfo` with no query is the
    /// documented lenient exception — `200` with no records.)
    #[test]
    fn experience_handlers_reject_wrong_methods_and_bad_requests() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        seed_experiences(&mut sim);

        // The GET-only caps are GET-only.
        for name in [
            CAP_GET_EXPERIENCE_INFO,
            CAP_FIND_EXPERIENCE_BY_NAME,
            CAP_GET_EXPERIENCES,
            CAP_AGENT_EXPERIENCES,
            CAP_GET_ADMIN_EXPERIENCES,
            CAP_GET_CREATOR_EXPERIENCES,
            CAP_GROUP_EXPERIENCES,
            CAP_IS_EXPERIENCE_ADMIN,
            CAP_IS_EXPERIENCE_CONTRIBUTOR,
        ] {
            let path = granted_cap_path(&caps, name)?;
            let (status, _) = respond(&mut caps, &mut sim, &post(&path, ""))?;
            assert_eq!(status, 405, "POST on {name}");
        }

        // The query-carrying GETs reject a missing or malformed query.
        let path = granted_cap_path(&caps, CAP_FIND_EXPERIENCE_BY_NAME)?;
        let (status, _) = respond(&mut caps, &mut sim, &get(&path, None))?;
        assert_eq!(status, 400, "FindExperienceByName without a query");
        let (status, _) = respond(&mut caps, &mut sim, &get(&path, Some("query=magic")))?;
        assert_eq!(status, 400, "FindExperienceByName without a page");
        let path = granted_cap_path(&caps, CAP_GROUP_EXPERIENCES)?;
        let (status, _) = respond(&mut caps, &mut sim, &get(&path, None))?;
        assert_eq!(status, 400, "GroupExperiences without a query");
        let (status, _) = respond(&mut caps, &mut sim, &get(&path, Some("not-a-uuid")))?;
        assert_eq!(status, 400, "GroupExperiences with a malformed id");
        let path = granted_cap_path(&caps, CAP_IS_EXPERIENCE_ADMIN)?;
        let (status, _) = respond(&mut caps, &mut sim, &get(&path, None))?;
        assert_eq!(status, 400, "IsExperienceAdmin without a query");

        // The lenient exception: an id-less GetExperienceInfo answers 200
        // with no records rather than an error.
        let path = granted_cap_path(&caps, CAP_GET_EXPERIENCE_INFO)?;
        let (status, body) = respond(&mut caps, &mut sim, &get(&path, None))?;
        assert_eq!(status, 200);
        assert_eq!(parse_experience_infos(&parse_llsd_xml(&body)?)?, Vec::new());

        // ExperiencePreferences: POST is 405; a malformed PUT body, a
        // well-formed non-permission PUT body, and a query-less DELETE are
        // all 400.
        let path = granted_cap_path(&caps, CAP_EXPERIENCE_PREFERENCES)?;
        let (status, _) = respond(&mut caps, &mut sim, &post(&path, ""))?;
        assert_eq!(status, 405);
        let (status, _) = respond(&mut caps, &mut sim, &put(&path, None, "not xml <"))?;
        assert_eq!(status, 400);
        let (status, _) = respond(
            &mut caps,
            &mut sim,
            &put(&path, None, "<llsd><map/></llsd>"),
        )?;
        assert_eq!(status, 400);
        let (status, _) = respond(&mut caps, &mut sim, &delete(&path, None))?;
        assert_eq!(status, 400);

        // UpdateExperience: GET is 405, a malformed body 400, an unknown
        // target 404.
        let path = granted_cap_path(&caps, CAP_UPDATE_EXPERIENCE)?;
        let (status, _) = respond(&mut caps, &mut sim, &get(&path, None))?;
        assert_eq!(status, 405);
        let (status, _) = respond(&mut caps, &mut sim, &post(&path, "not xml <"))?;
        assert_eq!(status, 400);
        let unknown = build_update_experience_request(&ExperienceUpdate {
            public_id: exp_key(EXP_UNKNOWN),
            name: "Ghost".to_owned(),
            description: String::new(),
            maturity: 13,
            properties: 0,
            slurl: None,
            extended_metadata: String::new(),
        });
        let (status, _) = respond(&mut caps, &mut sim, &post(&path, &unknown))?;
        assert_eq!(status, 404);

        // RegionExperiences: PUT is 405, a malformed POST body 400.
        let path = granted_cap_path(&caps, CAP_REGION_EXPERIENCES)?;
        let (status, _) = respond(&mut caps, &mut sim, &put(&path, None, ""))?;
        assert_eq!(status, 405);
        let (status, _) = respond(&mut caps, &mut sim, &post(&path, "not xml <"))?;
        assert_eq!(status, 400);

        // No mutation above landed: the server-event queue stays empty.
        assert!(sim.poll_event().is_none());
        Ok(())
    }

    // -----------------------------------------------------------------------
    // The voice signalling cluster.
    // -----------------------------------------------------------------------

    /// A Firestorm-shaped JSEP offer with one bundled audio section.
    const VOICE_OFFER: &str = "v=0\r\n\
        o=- 42 2 IN IP4 127.0.0.1\r\n\
        s=-\r\n\
        t=0 0\r\n\
        a=group:BUNDLE 0\r\n\
        m=audio 9 UDP/TLS/RTP/SAVPF 111\r\n\
        c=IN IP4 0.0.0.0\r\n\
        a=ice-ufrag:viewer\r\n\
        a=ice-pwd:viewerviewerviewerviewer\r\n\
        a=setup:actpass\r\n\
        a=mid:0\r\n\
        a=sendrecv\r\n\
        a=rtcp-mux\r\n\
        a=rtpmap:111 opus/48000/2\r\n";

    /// A fresh sim with the WebRTC stub enabled.
    fn webrtc_sim() -> SimSession {
        let mut sim = new_sim();
        sim.voice_mut().enable_webrtc(WebRtcStub::default());
        sim
    }

    /// Drives one provision POST through the cap and the real client's fold,
    /// returning the provisioned account the client surfaced.
    fn provision_through_client(
        caps: &mut SimCaps,
        sim: &mut SimSession,
        client: &mut Session,
        request: &sl_wire::VoiceProvisionRequest,
        now: Instant,
    ) -> Result<sl_wire::VoiceAccountInfo, TestError> {
        let path = granted_cap_path(caps, CAP_PROVISION_VOICE_ACCOUNT)?;
        let body = sl_wire::build_provision_voice_account_request(request);
        let events = fold_into_client(
            caps,
            sim,
            client,
            &post(&path, &body),
            CAP_PROVISION_VOICE_ACCOUNT,
            now,
        )?;
        match events.as_slice() {
            [Event::VoiceAccountProvisioned(info)] => Ok(info.clone()),
            other => Err(format!("expected VoiceAccountProvisioned, got {other:?}").into()),
        }
    }

    /// A WebRTC spatial offer is answered with a JSEP answer the real client
    /// decodes (`Event::VoiceAccountProvisioned`), the stub records the
    /// connection, and the driver sees `WebRtcOpened` with the parcel.
    #[test]
    fn webrtc_spatial_provision_round_trips_through_the_real_client() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = webrtc_sim();
        let mut client = new_client()?;
        let now = Instant::now();

        let request = sl_wire::VoiceProvisionRequest::webrtc(
            VOICE_OFFER,
            sl_wire::VOICE_CHANNEL_TYPE_LOCAL,
            Some(RegionLocalParcelId(7)),
        );
        let info = provision_through_client(&mut caps, &mut sim, &mut client, &request, now)?;
        assert!(info.is_webrtc());
        assert_eq!(info.jsep_type.as_deref(), Some("answer"));
        let session = info
            .viewer_session
            .clone()
            .ok_or("expected a viewer session")?;
        let answer = info.jsep_sdp.clone().ok_or("expected an answer sdp")?;
        // XML normalises the SDP's CRLF line ends to LF on the way through
        // the LLSD body, so compare line-wise.
        let answer_lines: Vec<&str> = answer.lines().collect();
        assert!(answer_lines.contains(&"a=setup:passive"));
        assert!(answer_lines.contains(&"a=ice-ufrag:fakegrid"));
        assert!(answer_lines.contains(&"a=rtpmap:111 opus/48000/2"));
        assert!(!answer.contains("viewer"));

        let connection = sim
            .voice()
            .connection(&session)
            .ok_or("expected the live connection")?;
        assert_eq!(
            connection.offer_sdp.lines().collect::<Vec<_>>(),
            VOICE_OFFER.lines().collect::<Vec<_>>()
        );
        assert_eq!(
            connection.answer_sdp.lines().collect::<Vec<_>>(),
            answer_lines
        );
        assert_eq!(
            connection.channel,
            VoiceChannel::Spatial {
                parcel_local_id: Some(RegionLocalParcelId(7)),
            }
        );
        match sim.poll_event() {
            Some(ServerEvent::VoiceProvisionRequested {
                request: seen,
                outcome,
            }) => {
                assert_eq!(seen.channel_type, request.channel_type);
                assert_eq!(seen.parcel_local_id, request.parcel_local_id);
                assert_eq!(
                    seen.jsep_offer_sdp
                        .as_deref()
                        .map(|sdp| sdp.lines().collect::<Vec<_>>()),
                    Some(VOICE_OFFER.lines().collect::<Vec<_>>())
                );
                assert_eq!(
                    outcome,
                    VoiceProvisionOutcome::WebRtcOpened {
                        viewer_session: session,
                        channel: VoiceChannel::Spatial {
                            parcel_local_id: Some(RegionLocalParcelId(7)),
                        },
                    }
                );
            }
            other => return Err(format!("expected VoiceProvisionRequested, got {other:?}").into()),
        }
        assert!(sim.poll_event().is_none());
        Ok(())
    }

    /// The ICE trickle lands on its connection (candidates, then the
    /// end-of-gathering flag), each batch surfacing a `ServerEvent`; an
    /// unknown `viewer_session` answers `404` and surfaces `known: false`.
    #[test]
    fn voice_signaling_trickles_onto_the_connection() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = webrtc_sim();
        let mut client = new_client()?;
        let now = Instant::now();
        let request = sl_wire::VoiceProvisionRequest::webrtc(
            VOICE_OFFER,
            sl_wire::VOICE_CHANNEL_TYPE_LOCAL,
            None,
        );
        let info = provision_through_client(&mut caps, &mut sim, &mut client, &request, now)?;
        let session = info.viewer_session.ok_or("expected a viewer session")?;
        let _provisioned = sim.poll_event();

        let path = granted_cap_path(&caps, CAP_VOICE_SIGNALING)?;
        let candidates = vec![
            sl_wire::IceCandidate {
                sdp_mid: "0".to_owned(),
                sdp_mline_index: 0,
                candidate: "candidate:1 1 udp 2122260223 192.168.1.10 51234 typ host".to_owned(),
            },
            sl_wire::IceCandidate {
                sdp_mid: "0".to_owned(),
                sdp_mline_index: 0,
                candidate: "candidate:2 1 udp 1686052607 203.0.113.5 51234 typ srflx".to_owned(),
            },
        ];
        let body = sl_wire::build_voice_signaling_request(&session, &candidates, false);
        let (status, reply) = respond(&mut caps, &mut sim, &post(&path, &body))?;
        assert_eq!((status, reply.as_str()), (200, "<llsd><undef /></llsd>"));
        let done = sl_wire::build_voice_signaling_request(&session, &[], true);
        let (status, _reply) = respond(&mut caps, &mut sim, &post(&path, &done))?;
        assert_eq!(status, 200);

        let connection = sim
            .voice()
            .connection(&session)
            .ok_or("expected the live connection")?;
        assert_eq!(connection.ice_candidates, candidates);
        assert!(connection.ice_completed);
        let events: Vec<ServerEvent> = std::iter::from_fn(|| sim.poll_event()).collect();
        assert_eq!(
            events,
            vec![
                ServerEvent::VoiceSignalingReceived {
                    viewer_session: session.clone(),
                    candidates,
                    completed: false,
                    known: true,
                },
                ServerEvent::VoiceSignalingReceived {
                    viewer_session: session,
                    candidates: Vec::new(),
                    completed: true,
                    known: true,
                },
            ]
        );

        let stray = sl_wire::build_voice_signaling_request("nope", &[], true);
        let (status, _reply) = respond(&mut caps, &mut sim, &post(&path, &stray))?;
        assert_eq!(status, 404);
        assert!(matches!(
            sim.poll_event(),
            Some(ServerEvent::VoiceSignalingReceived { known: false, .. })
        ));
        Ok(())
    }

    /// A WebRTC logout tears the connection down (`WebRtcClosed`); a second
    /// logout for the same session is an unknown session (`404`).
    #[test]
    fn webrtc_logout_closes_the_connection() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = webrtc_sim();
        let mut client = new_client()?;
        let now = Instant::now();
        let open = sl_wire::VoiceProvisionRequest::webrtc(
            VOICE_OFFER,
            sl_wire::VOICE_CHANNEL_TYPE_LOCAL,
            None,
        );
        let info = provision_through_client(&mut caps, &mut sim, &mut client, &open, now)?;
        let session = info.viewer_session.ok_or("expected a viewer session")?;
        let _provisioned = sim.poll_event();

        let logout = sl_wire::VoiceProvisionRequest::webrtc_logout(session.clone());
        let info = provision_through_client(&mut caps, &mut sim, &mut client, &logout, now)?;
        assert_eq!(info.viewer_session.as_deref(), Some(session.as_str()));
        assert!(sim.voice().connection(&session).is_none());
        assert!(matches!(
            sim.poll_event(),
            Some(ServerEvent::VoiceProvisionRequested {
                outcome: VoiceProvisionOutcome::WebRtcClosed { viewer_session },
                ..
            }) if viewer_session == session
        ));

        let path = granted_cap_path(&caps, CAP_PROVISION_VOICE_ACCOUNT)?;
        let body = sl_wire::build_provision_voice_account_request(&logout);
        let (status, _reply) = respond(&mut caps, &mut sim, &post(&path, &body))?;
        assert_eq!(status, 404);
        assert!(matches!(
            sim.poll_event(),
            Some(ServerEvent::VoiceProvisionRequested {
                outcome: VoiceProvisionOutcome::Refused(VoiceProvisionRefusal::UnknownSession),
                ..
            })
        ));
        Ok(())
    }

    /// A chat session's channel (`multiagent`) is gated by its registered
    /// credentials: a match opens the connection, a mismatch answers `401`
    /// (the viewer's "channel locked"), an unknown channel type `400`.
    #[test]
    fn multiagent_provision_checks_credentials() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = webrtc_sim();
        let mut client = new_client()?;
        let now = Instant::now();
        sim.voice_mut().set_channel_credentials("room-1", "secret");
        let path = granted_cap_path(&caps, CAP_PROVISION_VOICE_ACCOUNT)?;

        let good = sl_wire::VoiceProvisionRequest::webrtc_channel(
            VOICE_OFFER,
            "room-1",
            Some("secret".to_owned()),
        );
        let info = provision_through_client(&mut caps, &mut sim, &mut client, &good, now)?;
        let session = info.viewer_session.ok_or("expected a viewer session")?;
        assert_eq!(
            sim.voice()
                .connection(&session)
                .map(|connection| connection.channel.clone()),
            Some(VoiceChannel::MultiAgent {
                channel: "room-1".to_owned(),
            })
        );
        let _opened = sim.poll_event();

        let bad = sl_wire::VoiceProvisionRequest::webrtc_channel(
            VOICE_OFFER,
            "room-1",
            Some("wrong".to_owned()),
        );
        let body = sl_wire::build_provision_voice_account_request(&bad);
        let (status, _reply) = respond(&mut caps, &mut sim, &post(&path, &body))?;
        assert_eq!(status, 401);
        assert!(matches!(
            sim.poll_event(),
            Some(ServerEvent::VoiceProvisionRequested {
                outcome: VoiceProvisionOutcome::Refused(VoiceProvisionRefusal::BadCredentials),
                ..
            })
        ));

        let odd = sl_wire::VoiceProvisionRequest::webrtc(VOICE_OFFER, "estate", None);
        let body = sl_wire::build_provision_voice_account_request(&odd);
        let (status, _reply) = respond(&mut caps, &mut sim, &post(&path, &body))?;
        assert_eq!(status, 400);
        assert!(matches!(
            sim.poll_event(),
            Some(ServerEvent::VoiceProvisionRequested {
                outcome: VoiceProvisionOutcome::Refused(VoiceProvisionRefusal::UnknownChannel),
                ..
            })
        ));
        Ok(())
    }

    /// The Vivox fixture is handed out to a Vivox request (and the real
    /// client decodes the SIP account); without a fixture — or for a WebRTC
    /// request on a sim without the stub — the backend is unavailable
    /// (`400`).
    #[test]
    fn vivox_provision_serves_the_fixture() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        let mut client = new_client()?;
        let now = Instant::now();
        let path = granted_cap_path(&caps, CAP_PROVISION_VOICE_ACCOUNT)?;

        let vivox = sl_wire::VoiceProvisionRequest::vivox();
        let body = sl_wire::build_provision_voice_account_request(&vivox);
        let (status, _reply) = respond(&mut caps, &mut sim, &post(&path, &body))?;
        assert_eq!(status, 400);
        let webrtc = sl_wire::VoiceProvisionRequest::webrtc(
            VOICE_OFFER,
            sl_wire::VOICE_CHANNEL_TYPE_LOCAL,
            None,
        );
        let body = sl_wire::build_provision_voice_account_request(&webrtc);
        let (status, _reply) = respond(&mut caps, &mut sim, &post(&path, &body))?;
        assert_eq!(status, 400);
        let refused: Vec<ServerEvent> = std::iter::from_fn(|| sim.poll_event()).collect();
        assert_eq!(refused.len(), 2);
        assert!(refused.iter().all(|event| matches!(
            event,
            ServerEvent::VoiceProvisionRequested {
                outcome: VoiceProvisionOutcome::Refused(VoiceProvisionRefusal::BackendUnavailable),
                ..
            }
        )));

        let account = sl_wire::VoiceAccountInfo {
            voice_server_type: Some(sl_wire::VOICE_SERVER_TYPE_VIVOX.to_owned()),
            username: Some("xAgent".to_owned()),
            password: Some("hunter2".to_owned()),
            sip_uri_hostname: Some("sip.example.com".to_owned()),
            account_server_name: Some("https://vivox.example/api".parse()?),
            ..sl_wire::VoiceAccountInfo::default()
        };
        sim.voice_mut().set_vivox_account(account.clone());
        let info = provision_through_client(&mut caps, &mut sim, &mut client, &vivox, now)?;
        assert_eq!(info, account);
        assert!(matches!(
            sim.poll_event(),
            Some(ServerEvent::VoiceProvisionRequested {
                outcome: VoiceProvisionOutcome::VivoxAccount,
                ..
            })
        ));
        Ok(())
    }

    /// `ParcelVoiceInfoRequest` describes the agent's recorded parcel: a
    /// seeded WebRTC channel (a bare region UUID, decoded by the real client
    /// as `VoiceChannelUri::Id`), or the "no voice here" form elsewhere — and
    /// the `RequiredVoiceVersion` push reaches the client through the real
    /// event-queue poll.
    #[test]
    fn parcel_voice_info_follows_the_agent_parcel() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        let mut client = new_client()?;
        let now = Instant::now();
        let region = uuid::Uuid::from_u128(0x1e6);
        sim.set_region_id(region);
        sim.voice_mut().enable_webrtc(WebRtcStub::default());
        sim.voice_mut()
            .set_parcel_voice_info(sl_wire::ParcelVoiceInfo {
                parcel_local_id: RegionLocalParcelId(7),
                region_name: sl_wire::region_name_from_wire("region_name", "Fake Region")?,
                channel_uri: Some(sl_wire::VoiceChannelUri::Id(region)),
                channel_credentials: None,
            });
        sim.voice_mut()
            .set_agent_parcel(Some(RegionLocalParcelId(7)));

        let path = granted_cap_path(&caps, CAP_PARCEL_VOICE_INFO)?;
        let body = sl_wire::build_parcel_voice_info_request();
        let events = fold_into_client(
            &mut caps,
            &mut sim,
            &mut client,
            &post(&path, &body),
            CAP_PARCEL_VOICE_INFO,
            now,
        )?;
        match events.as_slice() {
            [Event::ParcelVoiceInfo(info)] => {
                assert_eq!(info.parcel_local_id, RegionLocalParcelId(7));
                assert_eq!(
                    info.region_name,
                    sl_wire::region_name_from_wire("region_name", "Fake Region")?
                );
                assert_eq!(info.channel_uri, Some(sl_wire::VoiceChannelUri::Id(region)));
            }
            other => return Err(format!("expected ParcelVoiceInfo, got {other:?}").into()),
        }
        assert_eq!(
            sim.poll_event(),
            Some(ServerEvent::ParcelVoiceInfoRequested {
                parcel_local_id: RegionLocalParcelId(7),
                enabled: true,
            })
        );

        sim.voice_mut()
            .set_agent_parcel(Some(RegionLocalParcelId(8)));
        let events = fold_into_client(
            &mut caps,
            &mut sim,
            &mut client,
            &post(&path, &body),
            CAP_PARCEL_VOICE_INFO,
            now,
        )?;
        match events.as_slice() {
            [Event::ParcelVoiceInfo(info)] => {
                assert_eq!(info.parcel_local_id, RegionLocalParcelId(8));
                assert_eq!(info.channel_uri, None);
            }
            other => return Err(format!("expected ParcelVoiceInfo, got {other:?}").into()),
        }
        assert_eq!(
            sim.poll_event(),
            Some(ServerEvent::ParcelVoiceInfoRequested {
                parcel_local_id: RegionLocalParcelId(8),
                enabled: false,
            })
        );

        sim.enqueue_required_voice_version(&sl_proto::RequiredVoiceVersion {
            major_version: 1,
            region_name: "Fake Region".to_owned(),
            voice_server_type: Some(sl_wire::VOICE_SERVER_TYPE_WEBRTC.to_owned()),
        });
        let eq_path = granted_event_queue_path(&caps)?;
        let poll = build_event_queue_request(None, false);
        let (status, body) = respond(&mut caps, &mut sim, &post(&eq_path, &poll))?;
        assert_eq!(status, 200);
        let batch = parse_event_queue_response(&body)?;
        for event in &batch.events {
            client.handle_caps_event(&event.message, &event.body, now)?;
        }
        assert!(matches!(
            drain_client(&mut client).as_slice(),
            [Event::RequiredVoiceVersion(version)]
                if version.major_version == 1
                    && version.voice_server_type.as_deref() == Some("webrtc")
        ));
        Ok(())
    }

    /// The three voice caps reject the wrong method with `405` and a
    /// malformed body with `400`, without touching the stub.
    #[test]
    fn voice_handlers_reject_wrong_methods_and_bad_bodies() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = webrtc_sim();
        for name in [
            CAP_PROVISION_VOICE_ACCOUNT,
            CAP_PARCEL_VOICE_INFO,
            CAP_VOICE_SIGNALING,
        ] {
            let path = granted_cap_path(&caps, name)?;
            let (status, _body) = respond(&mut caps, &mut sim, &get(&path, None))?;
            assert_eq!(status, 405, "{name} GET");
        }
        for name in [CAP_PROVISION_VOICE_ACCOUNT, CAP_VOICE_SIGNALING] {
            let path = granted_cap_path(&caps, name)?;
            let (status, _body) = respond(&mut caps, &mut sim, &post(&path, "<llsd><map>"))?;
            assert_eq!(status, 400, "{name} garbage");
        }
        assert!(sim.voice().connections().is_empty());
        assert!(sim.poll_event().is_none());
        Ok(())
    }
}
