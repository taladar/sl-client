//! End-to-end tests: the real `sl-client-tokio` stack — login POST, UDP
//! circuit, seed fetch, event-queue long-poll — against the fake grid.

#[cfg(test)]
mod test {
    use std::time::Duration;

    use pretty_assertions::{assert_eq, assert_ne};
    use sl_client_tokio::{
        ChatChannel, ChatType, Client, Command, Event, LoginParams, LoginRequest, StartLocation,
        VoiceProvisionRequest,
    };
    use sl_fake_grid::{AccountConfig, FakeAgent, FakeGrid, FakeGridBuilder, RegionConfig};
    use sl_proto::{ServerEvent, VoiceChannelUri};
    use sl_types::lsl::Vector;
    use sl_types::map::RegionCoordinates;
    use tokio::sync::mpsc;

    /// A boxed error for terse test signatures.
    type TestError = Box<dyn std::error::Error>;

    /// How long any single wait in these tests may take.
    const WAIT: Duration = Duration::from_secs(10);

    /// Starts a grid, connects the real client, and returns both plus the
    /// grid-side agent handle.
    async fn connect() -> Result<(FakeGrid, Client, FakeAgent), TestError> {
        connect_to(vec![RegionConfig::default()]).await
    }

    /// [`connect`] against a grid serving `regions` (the first is the start
    /// region).
    async fn connect_to(
        regions: Vec<RegionConfig>,
    ) -> Result<(FakeGrid, Client, FakeAgent), TestError> {
        let mut builder = FakeGridBuilder::new()
            .account(AccountConfig::new("Test", "User", "password"))
            .event_queue_hold(Duration::from_secs(2));
        for region in regions {
            builder = builder.region(region);
        }
        let grid = builder.start().await?;
        let mut logins = grid.logins();
        let request = LoginRequest::new(
            "Test",
            "User",
            "password",
            StartLocation::Last,
            "sl-fake-grid-e2e",
            "0.0",
        );
        let client = Client::connect(LoginParams {
            login_uri: grid.login_uri(),
            request,
        })
        .await?;
        let notice = tokio::time::timeout(WAIT, logins.recv()).await??;
        let agent = grid.agent(&notice).await.ok_or("no live session")?;
        Ok((grid, client, agent))
    }

    #[tokio::test]
    async fn full_stack_login_chat_and_event_queue() -> Result<(), TestError> {
        let (grid, client, agent) = connect().await?;
        let expected_agent = agent.agent_id();
        assert_eq!(client.agent_id(), Some(expected_agent));
        // The login response advertises the grid itself as the tile server.
        assert_eq!(client.map_server_url(), Some(&grid.login_uri()));

        let server_events = agent.events();
        let (event_tx, mut event_rx) = mpsc::channel::<Event>(256);
        let (command_tx, command_rx) = mpsc::channel::<Command>(8);
        let (diag_tx, _diag_rx) = mpsc::channel(16);
        let run = tokio::spawn(client.run(event_tx, diag_tx, command_rx));

        // The circuit comes up: the grid sees the arrival, the client sees the
        // region handshake and the scenario's greeting line.
        let mut greeted = false;
        let mut handshaken = false;
        let mut features_seen = false;
        while !(greeted && handshaken && features_seen) {
            let event = tokio::time::timeout(WAIT, event_rx.recv())
                .await?
                .ok_or("client event stream ended early")?;
            match event {
                Event::RegionHandshakeComplete | Event::RegionChanged { .. } => handshaken = true,
                Event::ChatReceived(message)
                    if message.message.contains("Welcome to the fake grid") =>
                {
                    greeted = true;
                }
                // The stock SimulatorFeatures carry the grid's OpenSimExtras
                // URLs (tile server, currency helper) — fetched over real CAPS.
                Event::SimulatorFeatures(features) => {
                    let extras = features
                        .open_sim_extras
                        .as_ref()
                        .ok_or("SimulatorFeatures without OpenSimExtras")?;
                    assert_eq!(extras.map_server_url.as_ref(), Some(&grid.login_uri()));
                    assert_eq!(extras.currency_base_uri.as_ref(), Some(&grid.login_uri()));
                    assert_eq!(extras.currency.as_deref(), Some("L$"));
                    features_seen = true;
                }
                _other => {}
            }
        }

        // Client chat reaches the grid side as a decoded ServerEvent.
        command_tx
            .send(Command::Chat {
                message: "hello grid".to_owned(),
                chat_type: ChatType::Normal,
                channel: ChatChannel(0),
            })
            .await?;
        let mut events = server_events;
        loop {
            let event = tokio::time::timeout(WAIT, events.recv()).await??;
            if let ServerEvent::Chat { message, .. } = &event
                && message == "hello grid"
            {
                break;
            }
        }

        // A CAPS event enqueued grid-side arrives through the real long-poll.
        agent
            .with_sim(|sim| {
                sim.enqueue_display_name_update(&sl_proto::DisplayNameUpdate {
                    old_display_name: "Test User".to_owned(),
                    name: sl_wire::DisplayName {
                        id: expected_agent,
                        username: "test.user".to_owned(),
                        display_name: "Test Resident".to_owned(),
                        legacy_first_name: "Test".to_owned(),
                        legacy_last_name: "User".to_owned(),
                        is_display_name_default: false,
                        ..sl_wire::DisplayName::default()
                    },
                });
            })
            .await;
        loop {
            let event = tokio::time::timeout(WAIT, event_rx.recv())
                .await?
                .ok_or("client event stream ended early")?;
            if let Event::DisplayNameUpdate(_) = event {
                break;
            }
        }

        drop(command_tx);
        run.abort();
        Ok(())
    }

    /// A connected client whose run loop is live and whose circuit has
    /// completed the region handshake.
    struct Running {
        /// The grid (dropping it shuts everything down).
        _grid: FakeGrid,
        /// The grid-side handle onto the session.
        agent: FakeAgent,
        /// The client's root circuit id (for scoped ids).
        circuit: sl_client_tokio::CircuitId,
        /// The client event stream.
        events: mpsc::Receiver<Event>,
        /// The client command channel.
        commands: mpsc::Sender<Command>,
        /// The run-loop task (aborted on teardown).
        run: tokio::task::JoinHandle<Result<(), sl_client_tokio::Error>>,
        /// The region name the `RegionHandshake` carried (it precedes
        /// `RegionHandshakeComplete`, so `start` captures it).
        region_name: Option<String>,
    }

    impl Drop for Running {
        fn drop(&mut self) {
            self.run.abort();
        }
    }

    /// Connects, starts the run loop, and waits for the region handshake.
    async fn start() -> Result<Running, TestError> {
        start_in(vec![RegionConfig::default()]).await
    }

    /// [`start`] against a grid serving `regions`.
    async fn start_in(regions: Vec<RegionConfig>) -> Result<Running, TestError> {
        let (grid, client, agent) = connect_to(regions).await?;
        let circuit = client.root_circuit_id().ok_or("no root circuit")?;
        let (event_tx, event_rx) = mpsc::channel::<Event>(256);
        let (command_tx, command_rx) = mpsc::channel::<Command>(8);
        let (diag_tx, _diag_rx) = mpsc::channel(16);
        let run = tokio::spawn(client.run(event_tx, diag_tx, command_rx));
        let mut running = Running {
            _grid: grid,
            agent,
            circuit,
            events: event_rx,
            commands: command_tx,
            run,
            region_name: None,
        };
        let mut region_name = None;
        running
            .wait_for(|event| match event {
                Event::RegionInfoHandshake(identity) => {
                    region_name = identity.sim_name.as_ref().map(ToString::to_string);
                    None
                }
                Event::RegionHandshakeComplete | Event::RegionChanged { .. } => Some(()),
                _ => None,
            })
            .await?;
        running.region_name = region_name;
        Ok(running)
    }

    impl Running {
        /// Receives client events until `pick` returns a value.
        async fn wait_for<T>(
            &mut self,
            pick: impl FnMut(&Event) -> Option<T>,
        ) -> Result<T, TestError> {
            wait_on(&mut self.events, pick).await
        }
    }

    /// Receives events from `events` until `pick` returns a value (the
    /// field-level form, for borrowing the grid alongside).
    async fn wait_on<T>(
        events: &mut mpsc::Receiver<Event>,
        mut pick: impl FnMut(&Event) -> Option<T>,
    ) -> Result<T, TestError> {
        loop {
            let event = tokio::time::timeout(WAIT, events.recv())
                .await?
                .ok_or("client event stream ended early")?;
            if let Some(value) = pick(&event) {
                return Ok(value);
            }
        }
    }

    #[tokio::test]
    async fn arrival_world_burst_reaches_client() -> Result<(), TestError> {
        let mut running = start().await?;
        let agent_id = running.agent.agent_id();

        // The stock world burst: own avatar, the overlay (four chunks), the
        // region-wide parcel, and the scripted object as a visible box.
        let mut avatar_seen = false;
        let mut box_seen = false;
        let mut overlay_chunks = 0_usize;
        let mut parcel = None;
        // The handshake goes out on `UseCircuitCode`, so the client (which
        // drops one arriving after its movement completed) decoded the
        // region's identity before the movement completed.
        assert_eq!(running.region_name.as_deref(), Some("Fake Region"));
        while !(avatar_seen && box_seen && overlay_chunks >= 4 && parcel.is_some()) {
            running
                .wait_for(|event| {
                    match event {
                        Event::ObjectAdded(object) | Event::ObjectUpdated(object)
                            if object.full_id.uuid() == agent_id.uuid() =>
                        {
                            avatar_seen = true;
                        }
                        Event::ObjectAdded(object) | Event::ObjectUpdated(object)
                            if object.full_id
                                == sl_fake_grid::scenario::stock_scripted_object() =>
                        {
                            assert_eq!(
                                object.local_id,
                                sl_fake_grid::scenario::STOCK_SCRIPTED_OBJECT_LOCAL_ID
                            );
                            assert_eq!(
                                object.motion.position,
                                sl_fake_grid::scenario::STOCK_SCRIPTED_OBJECT_POSITION
                            );
                            box_seen = true;
                        }
                        Event::ParcelOverlay(_) => {
                            overlay_chunks = overlay_chunks.saturating_add(1);
                        }
                        Event::ParcelProperties(info) => parcel = Some((**info).clone()),
                        _other => {}
                    }
                    Some(())
                })
                .await?;
        }
        let parcel = parcel.ok_or("no parcel")?;
        assert_eq!(parcel.name, sl_fake_grid::scenario::STOCK_PARCEL_NAME);
        assert_eq!(
            parcel.local_id,
            sl_fake_grid::scenario::STOCK_PARCEL_LOCAL_ID
        );
        assert!(parcel.allow_fly());

        // A rectangle request is answered from the same fixture, echoing the
        // sequence id; a refetch re-sends the box.
        running
            .commands
            .send(Command::RequestParcelProperties {
                west: 10.0,
                south: 10.0,
                east: 11.0,
                north: 11.0,
                sequence_id: -50_000,
            })
            .await?;
        let echoed = running
            .wait_for(|event| match event {
                Event::ParcelProperties(info) if info.sequence_id == -50_000 => {
                    Some(info.name.clone())
                }
                _ => None,
            })
            .await?;
        assert_eq!(echoed, sl_fake_grid::scenario::STOCK_PARCEL_NAME);

        let circuit = running.circuit;
        running
            .commands
            .send(Command::RequestObjects {
                local_ids: vec![sl_client_tokio::ScopedObjectId::new(
                    circuit,
                    sl_fake_grid::scenario::STOCK_SCRIPTED_OBJECT_LOCAL_ID,
                )],
            })
            .await?;
        running
            .wait_for(|event| match event {
                Event::ObjectAdded(object) | Event::ObjectUpdated(object)
                    if object.full_id == sl_fake_grid::scenario::stock_scripted_object() =>
                {
                    Some(())
                }
                _ => None,
            })
            .await?;
        Ok(())
    }

    #[tokio::test]
    async fn named_xfer_file_downloads_and_re_arms() -> Result<(), TestError> {
        let mut running = start().await?;
        for _round in 0..2 {
            running
                .commands
                .send(Command::RequestXfer {
                    filename: sl_fake_grid::scenario::STOCK_XFER_FILE.to_owned(),
                })
                .await?;
            let data = running
                .wait_for(|event| match event {
                    Event::XferDownloaded { data, .. } => Some(data.clone()),
                    _ => None,
                })
                .await?;
            assert_eq!(data, sl_fake_grid::scenario::STOCK_XFER_FILE_BODY);
        }
        // An unknown name is refused with an abort, not a hang.
        running
            .commands
            .send(Command::RequestXfer {
                filename: "missing.txt".to_owned(),
            })
            .await?;
        let result = running
            .wait_for(|event| match event {
                Event::XferAborted { result, .. } => Some(*result),
                _ => None,
            })
            .await?;
        assert_eq!(result, -1);
        Ok(())
    }

    #[tokio::test]
    async fn task_inventory_and_item_asset_round_trip() -> Result<(), TestError> {
        let mut running = start().await?;
        let task = sl_fake_grid::scenario::stock_scripted_object();
        let script = sl_fake_grid::scenario::stock_script_item();

        running
            .commands
            .send(Command::FetchTaskInventory {
                target: sl_client_tokio::ScopedObjectId::new(
                    running.circuit,
                    sl_fake_grid::scenario::STOCK_SCRIPTED_OBJECT_LOCAL_ID,
                ),
            })
            .await?;
        let (serial, items) = running
            .wait_for(|event| match event {
                Event::TaskInventoryContents {
                    task: got,
                    serial,
                    items,
                } if *got == task => Some((*serial, items.clone())),
                _ => None,
            })
            .await?;
        assert_eq!(serial, 1);
        assert_eq!(items, vec![script.clone()]);

        let asset_id = script.asset_id.ok_or("stock script has no asset id")?;
        running
            .commands
            .send(Command::FetchTaskItemAsset {
                task,
                item_id: script.item_id,
                asset_id,
                asset_type: script.asset_type,
            })
            .await?;
        let data = running
            .wait_for(|event| match event {
                Event::TaskItemAssetReceived {
                    task: got_task,
                    item,
                    data,
                    ..
                } if *got_task == task && *item == script.item_id => Some(data.clone()),
                _ => None,
            })
            .await?;
        assert_eq!(data, sl_fake_grid::scenario::STOCK_SCRIPT_BODY);

        // An item the fixtures do not hold is refused as an unknown source.
        running
            .commands
            .send(Command::FetchTaskItemAsset {
                task,
                item_id: sl_types::key::InventoryKey::from(uuid::Uuid::from_u128(0xBAD)),
                asset_id,
                asset_type: script.asset_type,
            })
            .await?;
        let status = running
            .wait_for(|event| match event {
                Event::TransferFailed { status, .. } => Some(*status),
                _ => None,
            })
            .await?;
        assert_eq!(status, sl_proto::TransferStatus::UnknownSource);
        Ok(())
    }

    #[tokio::test]
    async fn estate_covenant_round_trips() -> Result<(), TestError> {
        let mut running = start().await?;
        running
            .commands
            .send(Command::FetchEstateCovenantAsset)
            .await?;
        let data = running
            .wait_for(|event| match event {
                Event::EstateCovenantAssetReceived { data, .. } => Some(data.clone()),
                _ => None,
            })
            .await?;
        assert_eq!(data, sl_fake_grid::scenario::STOCK_COVENANT_BODY);
        Ok(())
    }

    #[tokio::test]
    async fn terrain_raw_download_and_upload_round_trip() -> Result<(), TestError> {
        let mut running = start().await?;
        let mut server_events = running.agent.events();

        running
            .commands
            .send(Command::RequestRegionTerrainDownload {
                viewer_filename: "terrain.raw".to_owned(),
            })
            .await?;
        let data = running
            .wait_for(|event| match event {
                Event::ServerFileDownloaded {
                    viewer_filename,
                    data,
                } if viewer_filename == "terrain.raw" => Some(data.clone()),
                _ => None,
            })
            .await?;
        assert_eq!(
            data,
            sl_fake_grid::flat_terrain_raw(sl_fake_grid::scenario::STOCK_TERRAIN_HEIGHT_M)
        );

        // Upload a different heightmap; the grid pulls it and keeps it.
        let uploaded = sl_fake_grid::flat_terrain_raw(42);
        running
            .commands
            .send(Command::RequestRegionTerrainUpload {
                viewer_filename: "new.raw".to_owned(),
                data: uploaded.clone(),
            })
            .await?;
        let byte_count = running
            .wait_for(|event| match event {
                Event::XferUploaded {
                    viewer_filename,
                    byte_count,
                    ..
                } if viewer_filename == "new.raw" => Some(*byte_count),
                _ => None,
            })
            .await?;
        assert_eq!(byte_count, uploaded.len());
        loop {
            let event = tokio::time::timeout(WAIT, server_events.recv()).await??;
            if let ServerEvent::XferReceived { filename, data, .. } = &event
                && filename == "new.raw"
            {
                assert_eq!(*data, uploaded);
                break;
            }
        }

        // A following download returns the uploaded bytes.
        running
            .commands
            .send(Command::RequestRegionTerrainDownload {
                viewer_filename: "again.raw".to_owned(),
            })
            .await?;
        let data = running
            .wait_for(|event| match event {
                Event::ServerFileDownloaded {
                    viewer_filename,
                    data,
                } if viewer_filename == "again.raw" => Some(data.clone()),
                _ => None,
            })
            .await?;
        assert_eq!(data, uploaded);
        Ok(())
    }

    /// The WebRTC voice signalling path end to end through the real client:
    /// the region advertises WebRTC (`SimulatorFeatures.VoiceServerType`,
    /// the arrival `RequiredVoiceVersion` push), a spatial offer is answered
    /// (`Event::VoiceAccountProvisioned`), the ICE trickle lands grid-side,
    /// the parcel channel is the region id, and logout closes the session.
    #[tokio::test]
    async fn voice_signalling_round_trips_through_the_real_client() -> Result<(), TestError> {
        let mut running = start().await?;
        let region_id = running.agent.with_sim(|sim| sim.region_id()).await;
        let mut server_events = running.agent.events();

        // The backend advertisement reaches the client both ways.
        let mut features_seen = false;
        let mut version_seen = false;
        while !(features_seen && version_seen) {
            running
                .wait_for(|event| match event {
                    Event::SimulatorFeatures(features)
                        if features.voice_server_type.as_deref() == Some("webrtc") =>
                    {
                        Some(true)
                    }
                    Event::RequiredVoiceVersion(version)
                        if version.voice_server_type.as_deref() == Some("webrtc") =>
                    {
                        Some(false)
                    }
                    _ => None,
                })
                .await
                .map(|is_features| {
                    if is_features {
                        features_seen = true;
                    } else {
                        version_seen = true;
                    }
                })?;
        }

        // Offer in → answer out.
        let offer = "v=0\r\no=- 1 1 IN IP4 127.0.0.1\r\ns=-\r\nt=0 0\r\n\
            m=audio 9 UDP/TLS/RTP/SAVPF 111\r\nc=IN IP4 0.0.0.0\r\n\
            a=setup:actpass\r\na=mid:0\r\na=sendrecv\r\na=rtpmap:111 opus/48000/2\r\n";
        running
            .commands
            .send(Command::RequestVoiceAccount {
                request: VoiceProvisionRequest::webrtc(offer, "local", None),
            })
            .await?;
        let info = running
            .wait_for(|event| match event {
                Event::VoiceAccountProvisioned(info) => Some(info.clone()),
                _ => None,
            })
            .await?;
        assert!(info.is_webrtc());
        assert_eq!(info.jsep_type.as_deref(), Some("answer"));
        let viewer_session = info.viewer_session.clone().ok_or("no viewer session")?;
        let answer = info.jsep_sdp.clone().ok_or("no answer sdp")?;
        assert!(answer.lines().any(|line| line == "a=setup:passive"));
        assert!(answer.lines().any(|line| line == "a=ice-ufrag:fakegrid"));

        // The ICE trickle is recorded on the grid-side connection.
        let candidate = sl_client_tokio::IceCandidate {
            sdp_mid: "0".to_owned(),
            sdp_mline_index: 0,
            candidate: "candidate:1 1 udp 2122260223 192.168.1.10 51234 typ host".to_owned(),
        };
        running
            .commands
            .send(Command::SendVoiceSignaling {
                viewer_session: viewer_session.clone(),
                candidates: vec![candidate.clone()],
                completed: false,
            })
            .await?;
        loop {
            let event = tokio::time::timeout(WAIT, server_events.recv()).await??;
            if let ServerEvent::VoiceSignalingReceived {
                viewer_session: seen,
                candidates,
                known,
                ..
            } = &event
            {
                assert_eq!(seen, &viewer_session);
                assert_eq!(candidates, &vec![candidate.clone()]);
                assert!(known);
                break;
            }
        }
        let recorded = running
            .agent
            .with_sim(|sim| {
                sim.voice()
                    .connection(&viewer_session)
                    .map(|connection| connection.ice_candidates.clone())
            })
            .await;
        assert_eq!(recorded, Some(vec![candidate]));

        // The parcel channel is the region id (SL's estate-wide WebRTC
        // channel form).
        running
            .commands
            .send(Command::RequestParcelVoiceInfo)
            .await?;
        let parcel = running
            .wait_for(|event| match event {
                Event::ParcelVoiceInfo(info) => Some(info.clone()),
                _ => None,
            })
            .await?;
        assert_eq!(parcel.channel_uri, Some(VoiceChannelUri::Id(region_id)));

        // Logout tears the connection down.
        running
            .commands
            .send(Command::RequestVoiceAccount {
                request: VoiceProvisionRequest::webrtc_logout(viewer_session.clone()),
            })
            .await?;
        running
            .wait_for(|event| match event {
                Event::VoiceAccountProvisioned(info)
                    if info.viewer_session.as_deref() == Some(viewer_session.as_str())
                        && info.jsep_sdp.is_none() =>
                {
                    Some(())
                }
                _ => None,
            })
            .await?;
        let closed = running
            .agent
            .with_sim(|sim| sim.voice().connection(&viewer_session).is_none())
            .await;
        assert!(closed);
        Ok(())
    }

    #[tokio::test]
    async fn two_grids_run_in_parallel() -> Result<(), TestError> {
        let (first_grid, first_client, _first_agent) = connect().await?;
        let (second_grid, second_client, _second_agent) = connect().await?;
        assert_ne!(first_grid.http_port(), second_grid.http_port());
        assert!(first_client.agent_id().is_some());
        assert!(second_client.agent_id().is_some());
        Ok(())
    }

    /// A distant second region for the teleport tests: ten regions east, so
    /// the hop is a genuine teleport (no neighbour adjacency) rather than a
    /// crossing.
    fn east_region() -> RegionConfig {
        RegionConfig {
            name: "Fake Region East".to_owned(),
            grid_x: 1010,
            grid_y: 1000,
            ..RegionConfig::default()
        }
    }

    /// The full inter-region teleport over the loopback: the client's own
    /// `TeleportLocationRequest` is answered with the teleport screen, the
    /// progress keys, the destination's child seed, a `TeleportFinish` naming
    /// the destination handle, and the arrival — the client lands with a
    /// world-resetting `RegionChanged`, the destination's handshake and world
    /// burst follow, and the source session is retired.
    #[tokio::test]
    async fn inter_region_teleport_over_loopback() -> Result<(), TestError> {
        let mut running = start_in(vec![RegionConfig::default(), east_region()]).await?;
        let dest_handle = running
            ._grid
            .region_handle("Fake Region East")
            .ok_or("no east region")?;
        let source = running.agent.clone();
        let mut teleports = running._grid.teleports();
        let source_seq = source.session_seq().await;

        running
            .commands
            .send(Command::Teleport {
                region_handle: dest_handle,
                position: RegionCoordinates::new(64.0, 32.0, 25.0),
                look_at: Vector {
                    x: 0.0,
                    y: 1.0,
                    z: 0.0,
                },
            })
            .await?;

        // The teleport screen, then the progress keys the viewer localises.
        running
            .wait_for(|event| matches!(event, Event::TeleportStarted).then_some(()))
            .await?;
        // The destination's `RegionHandshake` arrives on the child circuit
        // (before the finish), so its name is collected across the sequence.
        let mut handshakes = Vec::new();
        let mut progress = Vec::new();
        let dest_sim = running
            .wait_for(|event| match event {
                Event::TeleportProgress { message, .. } => {
                    progress.push(message.clone());
                    None
                }
                Event::RegionInfoHandshake(identity) => {
                    handshakes.push(identity.sim_name.as_ref().map(ToString::to_string));
                    None
                }
                Event::NeighborSeed { sim, .. } => Some(*sim),
                _ => None,
            })
            .await?;
        // UDP progress lines may outrun the CAPS seed; the keys are what
        // matters, in order.
        assert_eq!(progress.first().map(String::as_str), Some("resolving"));
        assert_eq!(progress.get(1).map(String::as_str), Some("sending_dest"));
        assert!(dest_sim.ip().is_loopback());

        let (finished_handle, finished_sim) = running
            .wait_for(|event| match event {
                Event::TeleportFinished {
                    region_handle, sim, ..
                } => Some((*region_handle, *sim)),
                Event::RegionInfoHandshake(identity) => {
                    handshakes.push(identity.sim_name.as_ref().map(ToString::to_string));
                    None
                }
                _ => None,
            })
            .await?;
        assert_eq!(finished_handle, dest_handle);
        assert_eq!(finished_sim, dest_sim);

        let (changed_handle, world_reset) = running
            .wait_for(|event| match event {
                Event::RegionChanged {
                    region_handle,
                    world_reset,
                    ..
                } => Some((*region_handle, *world_reset)),
                Event::RegionInfoHandshake(identity) => {
                    handshakes.push(identity.sim_name.as_ref().map(ToString::to_string));
                    None
                }
                _ => None,
            })
            .await?;
        assert_eq!(changed_handle, dest_handle);
        // The destination was announced (`EnableSimulator`) before the
        // finish, so the client already held it as a child circuit and keeps
        // its scene — distance alone does not reset the world.
        assert!(!world_reset, "a pre-announced destination keeps the scene");

        // The destination greeted the child circuit with its own handshake,
        // and the arrival world burst follows the promotion.
        if !handshakes.contains(&Some("Fake Region East".to_owned())) {
            running
                .wait_for(|event| match event {
                    Event::RegionInfoHandshake(identity)
                        if identity.sim_name.as_ref().map(ToString::to_string)
                            == Some("Fake Region East".to_owned()) =>
                    {
                        Some(())
                    }
                    _ => None,
                })
                .await?;
        }
        running
            .wait_for(|event| match event {
                Event::ParcelProperties(parcel) if parcel.local_id.0 == 1 => Some(()),
                _ => None,
            })
            .await?;

        // The grid reports the move; the source is retired, the destination
        // handle is live and hosts the root agent.
        let notice = tokio::time::timeout(WAIT, teleports.recv()).await??;
        assert_eq!(notice.from_seq, source_seq);
        assert_eq!(notice.region_name, "Fake Region East");
        assert_eq!(notice.agent_id, source.agent_id());
        assert!(source.is_closed(), "the source session should be retired");
        let dest = running
            ._grid
            .agent_by_seq(notice.to_seq)
            .await
            .ok_or("destination session missing")?;
        assert!(dest.with_sim(|sim| sim.is_root_agent()).await);
        assert_eq!(
            dest.with_sim(|sim| sim.arrival_position().position).await,
            RegionCoordinates::new(64.0, 32.0, 25.0)
        );

        // The new session talks: a simulator chat line reaches the client
        // through the destination circuit.
        dest.with_sim(|sim| {
            sim.send_chat_from_simulator(
                "East",
                sl_proto::ChatSource::System,
                sl_proto::Uuid::nil(),
                ChatType::Normal,
                1,
                sl_proto::Camera::region_center().center,
                "welcome east",
                std::time::Instant::now(),
            )
        })
        .await?;
        running
            .wait_for(|event| match event {
                Event::ChatReceived(message) if message.message == "welcome east" => Some(()),
                _ => None,
            })
            .await?;
        Ok(())
    }

    /// A request naming a region the grid does not serve is refused with the
    /// `invalid_tport` key, and the client returns to its region intact.
    #[tokio::test]
    async fn teleport_to_unknown_region_is_refused() -> Result<(), TestError> {
        let mut running = start().await?;
        running
            .commands
            .send(Command::Teleport {
                region_handle: sl_client_tokio::RegionHandle::from_grid(2000, 2000),
                position: RegionCoordinates::new(128.0, 128.0, 25.0),
                look_at: Vector {
                    x: 1.0,
                    y: 0.0,
                    z: 0.0,
                },
            })
            .await?;
        let reason = running
            .wait_for(|event| match event {
                Event::TeleportFailed { reason, .. } => Some(reason.clone()),
                _ => None,
            })
            .await?;
        assert_eq!(reason, "invalid_tport");
        // Still in the source region: a chat line from it arrives.
        running
            .agent
            .with_sim(|sim| {
                sim.send_chat_from_simulator(
                    "Home",
                    sl_proto::ChatSource::System,
                    sl_proto::Uuid::nil(),
                    ChatType::Normal,
                    1,
                    sl_proto::Camera::region_center().center,
                    "still here",
                    std::time::Instant::now(),
                )
            })
            .await?;
        running
            .wait_for(|event| match event {
                Event::ChatReceived(message) if message.message == "still here" => Some(()),
                _ => None,
            })
            .await?;
        assert!(!running.agent.is_closed());
        Ok(())
    }

    /// A logout does not merely close the session machine: the grid forgets
    /// the session, so the socket, the scenario clone and the terrain it held
    /// are freed instead of leaking for the life of the process.
    #[tokio::test]
    async fn a_logged_out_session_is_pruned() -> Result<(), TestError> {
        let running = start().await?;
        let seq = running.agent.session_seq().await;
        assert!(
            running._grid.agent_by_seq(seq).await.is_some(),
            "the live session is in the grid's table"
        );

        running.commands.send(Command::Logout).await?;
        tokio::time::timeout(WAIT, async {
            while running._grid.agent_by_seq(seq).await.is_some() {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await?;
        assert!(
            running.agent.is_closed(),
            "the session machine closed on the logout"
        );
        Ok(())
    }

    /// A request for the agent's own region finishes as a `TeleportLocal`
    /// at the requested position — no new session.
    #[tokio::test]
    async fn same_region_teleport_is_local() -> Result<(), TestError> {
        let mut running = start().await?;
        let handle = running
            ._grid
            .region_handle("Fake Region")
            .ok_or("no region")?;
        running
            .commands
            .send(Command::Teleport {
                region_handle: handle,
                position: RegionCoordinates::new(10.0, 20.0, 30.0),
                look_at: Vector {
                    x: 1.0,
                    y: 0.0,
                    z: 0.0,
                },
            })
            .await?;
        let position = running
            .wait_for(|event| match event {
                Event::TeleportLocal { position } => Some(*position),
                _ => None,
            })
            .await?;
        assert_eq!(position, RegionCoordinates::new(10.0, 20.0, 30.0));
        assert!(!running.agent.is_closed());
        Ok(())
    }

    /// The grid-initiated helper (what a lure or a scripted push does) moves
    /// the client without any request of its own and hands back the
    /// destination session.
    #[tokio::test]
    async fn grid_initiated_teleport_lands_the_client() -> Result<(), TestError> {
        let mut running = start_in(vec![RegionConfig::default(), east_region()]).await?;
        let source = running.agent.clone();
        let grid = &running._grid;
        let teleport = grid.teleport_agent(
            &source,
            "Fake Region East",
            RegionCoordinates::new(100.0, 100.0, 21.0),
            Vector {
                x: 1.0,
                y: 0.0,
                z: 0.0,
            },
        );
        // The helper resolves only once the client arrived, so the client's
        // events are drained concurrently.
        let (dest, landed) = tokio::join!(
            teleport,
            wait_on(&mut running.events, |event| match event {
                Event::RegionChanged { region_handle, .. } => Some(*region_handle),
                _ => None,
            })
        );
        let dest = dest?;
        let landed = landed?;
        assert_eq!(
            Some(landed),
            running._grid.region_handle("Fake Region East")
        );
        assert!(source.is_closed());
        assert!(dest.with_sim(|sim| sim.is_root_agent()).await);
        assert_ne!(dest.session_seq().await, source.session_seq().await);
        Ok(())
    }

    /// An unknown region name is refused by the helper before anything goes
    /// on the wire.
    #[tokio::test]
    async fn grid_initiated_teleport_to_unknown_region_errors() -> Result<(), TestError> {
        let running = start().await?;
        let result = running
            ._grid
            .teleport_agent(
                &running.agent,
                "Nowhere",
                RegionCoordinates::new(1.0, 1.0, 1.0),
                Vector {
                    x: 1.0,
                    y: 0.0,
                    z: 0.0,
                },
            )
            .await;
        assert!(matches!(
            result,
            Err(sl_fake_grid::Error::UnknownRegion { .. })
        ));
        Ok(())
    }

    /// A landmark teleport: the landmark asset (a `Landmark version 2` body
    /// naming the east region's fixed id) is served from the scenario's asset
    /// store, resolved to the region, and the client lands at the landmark's
    /// position with the `sending_landmark` progress key.
    #[tokio::test]
    async fn landmark_teleport_resolves_the_region_id() -> Result<(), TestError> {
        let east_id = sl_proto::Uuid::from_u128(0xea57);
        let landmark_key = sl_client_tokio::AssetKey::from(sl_proto::Uuid::from_u128(0x1a5d));
        let mut scenario = sl_fake_grid::Scenario::default();
        scenario.assets.insert(
            landmark_key,
            sl_proto::landmark_to_wire(east_id, RegionCoordinates::new(12.0, 34.0, 56.0)),
        );
        let east = RegionConfig {
            region_id: Some(east_id),
            ..east_region()
        };
        let mut running = start_in(vec![
            RegionConfig {
                scenario: Some(scenario),
                ..RegionConfig::default()
            },
            east,
        ])
        .await?;
        let mut teleports = running._grid.teleports();
        running
            .commands
            .send(Command::TeleportViaLandmark {
                landmark: Some(landmark_key),
            })
            .await?;
        let mut progress = Vec::new();
        let landed = running
            .wait_for(|event| match event {
                Event::TeleportProgress { message, .. } => {
                    progress.push(message.clone());
                    None
                }
                Event::RegionChanged { region_handle, .. } => Some(*region_handle),
                _ => None,
            })
            .await?;
        assert_eq!(
            Some(landed),
            running._grid.region_handle("Fake Region East")
        );
        assert!(
            progress.contains(&"sending_landmark".to_owned()),
            "{progress:?}"
        );
        let notice = tokio::time::timeout(WAIT, teleports.recv()).await??;
        let dest = running
            ._grid
            .agent_by_seq(notice.to_seq)
            .await
            .ok_or("destination session missing")?;
        assert_eq!(
            dest.with_sim(|sim| sim.arrival_position().position).await,
            RegionCoordinates::new(12.0, 34.0, 56.0)
        );
        Ok(())
    }

    /// A landmark the asset store does not hold is refused with
    /// `nolandmark_tport`.
    #[tokio::test]
    async fn unknown_landmark_is_refused() -> Result<(), TestError> {
        let mut running = start().await?;
        running
            .commands
            .send(Command::TeleportViaLandmark {
                landmark: Some(sl_client_tokio::AssetKey::from(sl_proto::Uuid::from_u128(
                    0xbad,
                ))),
            })
            .await?;
        let reason = running
            .wait_for(|event| match event {
                Event::TeleportFailed { reason, .. } => Some(reason.clone()),
                _ => None,
            })
            .await?;
        assert_eq!(reason, "nolandmark_tport");
        Ok(())
    }

    /// "Teleport home" (a landmark request with no landmark) lands the agent
    /// in its start region — from the east region that is a real move back.
    #[tokio::test]
    async fn teleport_home_returns_to_the_start_region() -> Result<(), TestError> {
        let mut running = start_in(vec![RegionConfig::default(), east_region()]).await?;
        let mut teleports = running._grid.teleports();
        let east = running
            ._grid
            .region_handle("Fake Region East")
            .ok_or("no east region")?;
        running
            .commands
            .send(Command::Teleport {
                region_handle: east,
                position: RegionCoordinates::new(1.0, 2.0, 3.0),
                look_at: Vector {
                    x: 1.0,
                    y: 0.0,
                    z: 0.0,
                },
            })
            .await?;
        running
            .wait_for(|event| match event {
                Event::RegionChanged { region_handle, .. } if *region_handle == east => Some(()),
                _ => None,
            })
            .await?;
        let _first = tokio::time::timeout(WAIT, teleports.recv()).await??;

        running
            .commands
            .send(Command::TeleportViaLandmark { landmark: None })
            .await?;
        let mut progress = Vec::new();
        let landed = running
            .wait_for(|event| match event {
                Event::TeleportProgress { message, .. } => {
                    progress.push(message.clone());
                    None
                }
                Event::RegionChanged { region_handle, .. } => Some(*region_handle),
                _ => None,
            })
            .await?;
        assert_eq!(Some(landed), running._grid.region_handle("Fake Region"));
        assert!(
            progress.contains(&"sending_home".to_owned()),
            "{progress:?}"
        );
        let notice = tokio::time::timeout(WAIT, teleports.recv()).await??;
        assert_eq!(notice.region_name, "Fake Region");
        Ok(())
    }

    /// Accepting a lure whose id packs the destination the OpenSim way (a
    /// fake parcel id: handle + position) lands the agent there, with the
    /// lure flag echoed; an opaque lure id naming nobody online is refused
    /// with `no_host`.
    #[tokio::test]
    async fn lure_acceptance_decodes_the_fake_parcel_id() -> Result<(), TestError> {
        let mut running = start_in(vec![RegionConfig::default(), east_region()]).await?;
        let mut teleports = running._grid.teleports();
        let east = running
            ._grid
            .region_handle("Fake Region East")
            .ok_or("no east region")?;

        let opaque = sl_client_tokio::LureId::from(sl_proto::Uuid::from_u128(
            0x3b6b_7c62_8f8f_4e34_9c1a_79c2_e2ba_0fd1,
        ));
        running
            .commands
            .send(Command::AcceptTeleportLure { lure_id: opaque })
            .await?;
        let reason = running
            .wait_for(|event| match event {
                Event::TeleportFailed { reason, .. } => Some(reason.clone()),
                _ => None,
            })
            .await?;
        assert_eq!(reason, "no_host");

        let place = sl_proto::FakeParcelId {
            region_handle: east,
            x: 40,
            y: 50,
            z: 60,
        };
        running
            .commands
            .send(Command::AcceptTeleportLure {
                lure_id: sl_client_tokio::LureId::from(place.to_uuid()),
            })
            .await?;
        let (landed, flags) = running
            .wait_for(|event| match event {
                Event::TeleportFinished {
                    region_handle,
                    flags,
                    ..
                } => Some((*region_handle, *flags)),
                _ => None,
            })
            .await?;
        assert_eq!(landed, east);
        assert_ne!(flags.0 & sl_proto::TeleportFlags::VIA_LURE, 0);
        running
            .wait_for(|event| match event {
                Event::RegionChanged { region_handle, .. } if *region_handle == east => Some(()),
                _ => None,
            })
            .await?;
        let notice = tokio::time::timeout(WAIT, teleports.recv()).await??;
        let dest = running
            ._grid
            .agent_by_seq(notice.to_seq)
            .await
            .ok_or("destination session missing")?;
        assert_eq!(
            dest.with_sim(|sim| sim.arrival_position().position).await,
            RegionCoordinates::new(40.0, 50.0, 60.0)
        );
        Ok(())
    }
}
