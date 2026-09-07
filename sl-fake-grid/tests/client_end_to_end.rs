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
    use sl_fake_grid::{
        AccountConfig, FakeAgent, FakeGrid, FakeGridBuilder, ImitatedGrid, RegionConfig,
    };
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
        connect_configured(regions, None, ImitatedGrid::default()).await
    }

    /// [`connect_to`], optionally shortening how long the grid waits for a
    /// client to complete its movement into a handover destination — what a
    /// test of the **failure** path needs, since the real budget is tens of
    /// seconds — and naming the live grid this one imitates, for the tests
    /// that are about the divergence itself.
    async fn connect_configured(
        regions: Vec<RegionConfig>,
        handover_timeout: Option<Duration>,
        imitates: ImitatedGrid,
    ) -> Result<(FakeGrid, Client, FakeAgent), TestError> {
        let mut builder = FakeGridBuilder::new()
            .account(AccountConfig::new("Test", "User", "password"))
            .imitates(imitates)
            .event_queue_hold(Duration::from_secs(2));
        if let Some(timeout) = handover_timeout {
            builder = builder.handover_timeout(timeout);
        }
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
                // The stock grid imitates Second Life, whose SimulatorFeatures
                // has no `OpenSimExtras` key at all — and does name its voice
                // backend. Fetched over real CAPS. The tile server the extras
                // would have carried arrived in the login response instead
                // (asserted above); the OpenSim-flavoured shape is
                // `an_opensim_flavoured_grid_introduces_itself_as_opensim`.
                Event::SimulatorFeatures(features) => {
                    assert_eq!(features.open_sim_extras, None);
                    assert_eq!(features.voice_server_type.as_deref(), Some("webrtc"));
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
        start_configured(regions, None, ImitatedGrid::default()).await
    }

    /// [`start_in`] with the handover arrival budget and imitated grid of
    /// [`connect_configured`].
    async fn start_configured(
        regions: Vec<RegionConfig>,
        handover_timeout: Option<Duration>,
        imitates: ImitatedGrid,
    ) -> Result<Running, TestError> {
        let (grid, client, agent) = connect_configured(regions, handover_timeout, imitates).await?;
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

        /// Receives client events until `pick` answers `true`, with **one**
        /// deadline for the whole wait and a dump of what did arrive when it
        /// expires.
        ///
        /// [`wait_for`](Self::wait_for) restarts its timeout on every event it
        /// receives, so a session that keeps chattering — pings, coarse
        /// locations, the agent's own updates — keeps a wait for something that
        /// will never arrive alive indefinitely. That is fine for waiting on
        /// one thing that is about to happen, and useless for waiting on a set
        /// of things that may not.
        ///
        /// **A wait consumes what it reads.** Anything that arrived before it
        /// started, or that it stepped over on the way to what it was looking
        /// for, is gone. So a test that wants several things must ask for them
        /// in **one** wait, accumulating as they arrive, rather than waiting
        /// for each in turn: the wire order of independent messages is not
        /// something a test may assume — a real link reorders, and even here a
        /// second wait for something the first already stepped past hangs until
        /// the deadline. Waiting for one thing *after an action that causes it*
        /// is the case where sequencing is safe.
        async fn wait_until(
            &mut self,
            what: &str,
            mut pick: impl FnMut(&Event) -> bool,
        ) -> Result<(), TestError> {
            let mut seen: Vec<String> = Vec::new();
            let events = &mut self.events;
            let waited = tokio::time::timeout(WAIT, async {
                loop {
                    let Some(event) = events.recv().await else {
                        return Err::<(), TestError>("client event stream ended early".into());
                    };
                    seen.push(format!("{event:?}").chars().take(140).collect());
                    if pick(&event) {
                        return Ok(());
                    }
                }
            })
            .await;
            match waited {
                Ok(inner) => inner,
                Err(_elapsed) => {
                    let tail: Vec<&String> = seen.iter().rev().take(24).collect();
                    Err(format!("timed out waiting for {what}; last events: {tail:#?}").into())
                }
            }
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

    /// Waits for the item a take just filed, **whichever way this grid
    /// announces one**: the legacy UDP `UpdateCreateInventoryItem` on an
    /// OpenSim-flavoured grid, a `BulkUpdateInventory` over the event queue on
    /// a Second-Life-flavoured one (`sl_fake_grid::InventoryAnnouncement`).
    ///
    /// Waiting for only the first is how these tests hung when the flavour
    /// started deciding it — and `wait_on`'s timeout is per *event*, so a grid
    /// still sending pings never trips it. That is the same trap a viewer that
    /// only listens for the legacy message falls into, which is the whole point
    /// of the switch.
    async fn wait_for_taken_item(
        events: &mut mpsc::Receiver<Event>,
    ) -> Result<sl_client_tokio::InventoryItem, TestError> {
        wait_on(events, |event| match event {
            Event::InventoryItemCreated { item, .. }
                if i32::from(item.item_type) == sl_proto::AssetType::Object.to_code() =>
            {
                Some(item.clone())
            }
            Event::InventoryBulkUpdate { items, .. } => items
                .iter()
                .find(|item| i32::from(item.item_type) == sl_proto::AssetType::Object.to_code())
                .cloned(),
            _ => None,
        })
        .await
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

    /// The region's ground arrives as the full spiral of land patches, every
    /// one stamped with the region handle, carrying the heights the fixture
    /// declares — plus the wind layer's two patches.
    #[tokio::test]
    async fn arrival_streams_the_regions_ground() -> Result<(), TestError> {
        let terrain = sl_fake_grid::TerrainFixture {
            wind: Some([1.5, -2.5]),
            ..sl_fake_grid::TerrainFixture::default()
        }
        .with_heights(sl_fake_grid::Heightfield::Slope {
            low: 21.0,
            high: 41.0,
        });
        let region = RegionConfig {
            terrain: terrain.clone(),
            ..RegionConfig::default()
        };
        let expected_handle = sl_proto::RegionHandle::from_grid(region.grid_x, region.grid_y);
        let mut running = start_in(vec![region]).await?;

        let mut land: Vec<sl_proto::TerrainPatch> = Vec::new();
        let mut wind: Vec<sl_proto::TerrainPatch> = Vec::new();
        while land.len() < 256 || wind.len() < 2 {
            running
                .wait_for(|event| {
                    if let Event::TerrainPatch(patch) = event {
                        match patch.layer {
                            sl_proto::TerrainLayerType::Land => land.push((**patch).clone()),
                            sl_proto::TerrainLayerType::Wind => wind.push((**patch).clone()),
                            _other => {}
                        }
                    }
                    Some(())
                })
                .await?;
        }

        assert_eq!(land.len(), 256, "one patch per 16 m cell of the region");
        // Every patch position exactly once, all under the region's handle.
        let mut positions: Vec<(u32, u32)> = land.iter().map(|p| (p.patch_x, p.patch_y)).collect();
        positions.sort_unstable();
        positions.dedup();
        assert_eq!(positions.len(), 256);
        assert!(
            land.iter()
                .all(|patch| patch.region_handle == expected_handle && patch.size == 16),
            "every patch carries the region handle at the standard patch size"
        );
        // The spiral starts at the south-west corner and runs east.
        let opening: Vec<(u32, u32)> = land
            .iter()
            .take(4)
            .map(|patch| (patch.patch_x, patch.patch_y))
            .collect();
        assert_eq!(opening, vec![(0, 0), (1, 0), (2, 0), (3, 0)]);

        // The decoded heights are the fixture's, to within the encoder's
        // quantization of the patch's range.
        for patch in &land {
            let x = f32::from(u16::try_from(patch.patch_x * 16 + 5)?);
            let y = f32::from(u16::try_from(patch.patch_y * 16 + 9)?);
            let height = patch.value(5, 9).ok_or("a patch with no cell (5, 9)")?;
            let expected = terrain.height_at(x, y);
            assert!(
                (height - expected).abs() < 0.2,
                "patch ({}, {}) cell (5, 9) decoded to {height}, not {expected}",
                patch.patch_x,
                patch.patch_y
            );
        }

        // Wind: two whole-region patches, the east then the north component.
        assert_eq!(wind.len(), 2);
        let components: Vec<f32> = wind.iter().filter_map(|patch| patch.value(0, 0)).collect();
        let expected = [1.5_f32, -2.5];
        for (got, want) in components.iter().zip(expected) {
            assert!((got - want).abs() < 0.05, "wind {got}, wanted {want}");
        }
        assert_eq!(components.len(), 2);
        Ok(())
    }

    /// A region's own environment settings are what the `ExtEnvironment`
    /// capability answers with, stamped with the region's id.
    #[tokio::test]
    async fn a_regions_environment_reaches_the_client() -> Result<(), TestError> {
        let environment = sl_proto::EnvironmentSettings {
            parcel_id: -1,
            region_id: uuid::Uuid::nil(),
            day_length: 3600,
            day_offset: 1800,
            flags: 0,
            env_version: 7,
            track_altitudes: [500.0, 1500.0, 2500.0],
            day_cycle: sl_proto::DayCycle {
                name: "Short Day".to_owned(),
                water_track: Vec::new(),
                sky_tracks: Vec::new(),
                sky_frames: std::collections::BTreeMap::new(),
                water_frames: std::collections::BTreeMap::new(),
            },
        };
        let mut running = start_in(vec![RegionConfig {
            environment: Some(environment.clone()),
            ..RegionConfig::default()
        }])
        .await?;
        let region_id = running.agent.with_sim(|sim| sim.region_id()).await;

        running
            .commands
            .send(Command::RequestEnvironment { parcel_id: None })
            .await?;
        let served = running
            .wait_for(|event| match event {
                Event::Environment(settings) => Some((**settings).clone()),
                _ => None,
            })
            .await?;
        assert_eq!(
            served,
            sl_proto::EnvironmentSettings {
                region_id,
                ..environment
            }
        );
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
        // The stock scenario names no RAW file, so the download is the
        // region's own ground — the same heights the LAND patches carried.
        assert_eq!(data, sl_fake_grid::TerrainFixture::default().to_raw());

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

    /// The other flavour introduces itself the other way, through the real
    /// client: `SimulatorFeatures` carries the `OpenSimExtras` block with the
    /// grid's own URLs, and the region speaks no voice at all — no
    /// `VoiceServerType`, no `RequiredVoiceVersion` push on arrival, and a
    /// provision request refused for want of a backend. Both strings appear
    /// nowhere in OpenSim's sources, and both of its voice modules are off in
    /// a stock region.
    ///
    /// Runs the loop itself rather than going through [`start_imitating`]
    /// because the claim is about an event that must **not** arrive, and a
    /// wait that stepped over the arrival burst on its way to the handshake
    /// would have consumed the push this is looking for. Here nothing reads
    /// the stream before the loop below does.
    #[tokio::test]
    async fn an_opensim_flavoured_grid_introduces_itself_as_opensim() -> Result<(), TestError> {
        let (grid, client, agent) =
            connect_configured(vec![RegionConfig::default()], None, ImitatedGrid::OpenSim).await?;
        let login_uri = grid.login_uri();
        let mut server_events = agent.events();
        let (event_tx, mut event_rx) = mpsc::channel::<Event>(256);
        let (command_tx, command_rx) = mpsc::channel::<Command>(8);
        let (diag_tx, _diag_rx) = mpsc::channel(16);
        let run = tokio::spawn(client.run(event_tx, diag_tx, command_rx));

        // A viewer arriving anywhere asks for voice the only way this
        // workspace knows how to ask -- WebRTC, since Vivox is implemented
        // nowhere here. A silent region refuses it.
        let offer = "v=0\r\no=- 1 1 IN IP4 127.0.0.1\r\ns=-\r\nt=0 0\r\n\
            m=audio 9 UDP/TLS/RTP/SAVPF 111\r\nc=IN IP4 0.0.0.0\r\n\
            a=setup:actpass\r\na=mid:0\r\na=sendrecv\r\na=rtpmap:111 opus/48000/2\r\n";
        command_tx
            .send(Command::RequestVoiceAccount {
                request: VoiceProvisionRequest::webrtc(offer, "local", None),
            })
            .await?;

        // The features fetch is the terminating condition; the arrival push,
        // had there been one, would have been enqueued before it.
        let mut features = None;
        let mut named_a_backend = false;
        while features.is_none() {
            let event = tokio::time::timeout(WAIT, event_rx.recv())
                .await?
                .ok_or("client event stream ended early")?;
            match event {
                Event::SimulatorFeatures(reply) => features = Some(reply.as_ref().clone()),
                Event::RequiredVoiceVersion(_version) => named_a_backend = true,
                _other => {}
            }
        }
        let features = features.ok_or("no SimulatorFeatures")?;
        let extras = features
            .open_sim_extras
            .as_ref()
            .ok_or("an OpenSim-flavoured grid sent no OpenSimExtras")?;
        assert_eq!(extras.map_server_url.as_ref(), Some(&login_uri));
        assert_eq!(extras.currency_base_uri.as_ref(), Some(&login_uri));
        assert_eq!(extras.currency.as_deref(), Some("L$"));
        assert_eq!(features.voice_server_type, None);
        assert!(!named_a_backend, "OpenSim sends no RequiredVoiceVersion");

        // Grid-side, because a refusal is what the *grid* decided; the client
        // only learns that its POST failed.
        loop {
            let event = tokio::time::timeout(WAIT, server_events.recv()).await??;
            if let ServerEvent::VoiceProvisionRequested { outcome, .. } = &event {
                assert_eq!(
                    *outcome,
                    sl_proto::VoiceProvisionOutcome::Refused(
                        sl_proto::VoiceProvisionRefusal::BackendUnavailable
                    )
                );
                break;
            }
        }

        drop(command_tx);
        run.abort();
        Ok(())
    }

    /// **A grid that does not central-bake says so four times over.**
    ///
    /// Server-side baking is not one advertisement, it is four, and the reason
    /// they are asserted together is that moving fewer than all of them is
    /// *worse than moving none*: a viewer that has decided an avatar is
    /// server-baked and finds no service URL asks for no baked texture at all
    /// (`LLVOAvatar::getImageURL` answers an empty string), so every avatar
    /// stays a cloud with nothing logged. Three of the four are here — the
    /// fourth, the login field itself, is pinned on the raw response in
    /// `http_glue`, because its absence is not something the client re-exposes.
    ///
    /// Both flavours run, because "the OpenSim one withholds it" is only half a
    /// claim: a knob that answered the same on both would pass a test that
    /// checked one side and would still leave the fake grid nobody in
    /// particular.
    #[tokio::test]
    async fn a_client_baking_grid_withholds_every_bake_signal() -> Result<(), TestError> {
        for (imitates, bakes) in [
            (ImitatedGrid::SecondLife, true),
            (ImitatedGrid::OpenSim, false),
        ] {
            let (_grid, mut client, _agent) =
                connect_configured(vec![RegionConfig::default()], None, imitates).await?;
            let (caps_tx, mut caps_rx) = mpsc::channel(4);
            client.set_caps_reporter(caps_tx);
            let (event_tx, mut event_rx) = mpsc::channel::<Event>(256);
            let (command_tx, command_rx) = mpsc::channel::<Command>(8);
            let (diag_tx, _diag_rx) = mpsc::channel(16);
            let run = tokio::spawn(client.run(event_tx, diag_tx, command_rx));

            // The region handshake and the agent's own appearance both arrive
            // in the arrival burst; neither is requested, so this reads the
            // stream until it has both rather than asking for either.
            let mut protocols = None;
            let mut appearance = None;
            while protocols.is_none() || appearance.is_none() {
                let event = tokio::time::timeout(WAIT, event_rx.recv())
                    .await?
                    .ok_or("client event stream ended early")?;
                match event {
                    Event::RegionInfoHandshake(identity) => {
                        protocols = Some(identity.region_protocols);
                    }
                    Event::AvatarAppearance(record) => appearance = Some(*record),
                    _other => {}
                }
            }
            let protocols = protocols.ok_or("no region identity")?;
            let appearance = appearance.ok_or("no appearance")?;

            // (1) `RegionProtocols` bit 0, which the reference viewer reads as
            // `getCentralBakeVersion()` — what decides whether the agent bakes
            // and uploads its *own* appearance.
            assert_eq!(
                protocols & sl_fake_grid::REGION_PROTOCOL_SERVER_BAKES != 0,
                bakes,
                "a grid imitating {imitates:?} claimed central baking = {}",
                protocols & sl_fake_grid::REGION_PROTOCOL_SERVER_BAKES != 0
            );
            // The other bit in that field is Bakes on Mesh, an OpenSim
            // extension that rides along and is not the bake policy's.
            assert_eq!(
                protocols & sl_fake_grid::REGION_PROTOCOL_BAKES_ON_MESH != 0,
                imitates == ImitatedGrid::OpenSim
            );

            // (2) the per-avatar `AppearanceData` block, which is
            // `setIsUsingServerBakes(appearance_version > 0)`. Absent, not
            // zeroed: OpenSim writes no block at all.
            assert_eq!(appearance.appearance_version.is_some(), bakes);
            assert_eq!(appearance.cof_version.is_some(), bakes);
            assert_eq!(appearance.appearance_flags.is_some(), bakes);
            // What the avatar *looks* like is the same either way — both grids
            // publish baked ids, they disagree only about how to fetch them.
            assert!(!appearance.visual_params.is_empty());
            assert!(
                appearance
                    .texture_entry
                    .texture_id(sl_proto::avatar_texture::UPPER_BAKED)
                    .is_some()
            );

            // (3) the `UpdateAvatarAppearance` capability — the POST that
            // triggers a central bake. A viewer that finds none bakes locally.
            let caps = tokio::time::timeout(WAIT, caps_rx.recv())
                .await?
                .ok_or("no capability map")?;
            assert_eq!(caps.contains_key("UpdateAvatarAppearance"), bakes);
            // A neighbouring upload cap is still there on both, so the check
            // above is about this capability and not about the seed grant.
            assert!(caps.contains_key("UploadBakedTexture"));

            drop(command_tx);
            run.abort();
        }
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
                dest.now(),
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

    /// The region bordering the start region to the east: the crossing tests'
    /// destination, and — unlike [`east_region`] — close enough that the start
    /// region announces it as a neighbour.
    fn adjacent_east_region() -> RegionConfig {
        RegionConfig {
            name: "Fake Region East".to_owned(),
            grid_x: RegionConfig::default().grid_x.saturating_add(1),
            grid_y: RegionConfig::default().grid_y,
            ..RegionConfig::default()
        }
    }

    /// A second bordering region, named apart from [`adjacent_east_region`] so
    /// a grid can serve a neighbour *and* a distant region at once — the
    /// builder refuses two regions with the same name.
    fn next_door_region() -> RegionConfig {
        RegionConfig {
            name: "Fake Region Next Door".to_owned(),
            grid_x: RegionConfig::default().grid_x,
            grid_y: RegionConfig::default().grid_y.saturating_add(1),
            ..RegionConfig::default()
        }
    }

    /// **A neighbour is announced on arrival and streams its own scene.**
    ///
    /// The reason a region across a border is already drawn before you walk
    /// into it, and the precondition for a crossing being a *promotion* rather
    /// than a connection: the moment the agent is rooted, the region tells the
    /// client about the one next door (`EnableSimulator` +
    /// `EstablishAgentCommunication`), the client opens a child circuit, and
    /// that circuit is handed the neighbour's objects and ground — labelled
    /// with the neighbour's handle, not the root region's.
    #[tokio::test]
    async fn a_neighbour_is_announced_and_streams_its_scene() -> Result<(), TestError> {
        let mut running = start_in(vec![RegionConfig::default(), adjacent_east_region()]).await?;
        let east = running
            ._grid
            .region_handle("Fake Region East")
            .ok_or("no east region")?;
        assert_eq!(
            running._grid.neighbours_of("Fake Region"),
            vec!["Fake Region East".to_owned()],
            "an adjacent region is a neighbour"
        );

        // Everything in one pass. The announcement and the burst it triggers
        // are causally ordered, but the messages *within* each are not
        // something a test may assume the order of, and a second wait would
        // consume what the first stepped over.
        let mut announced = None;
        let mut seeded = None;
        let mut ground = false;
        let mut objects = false;
        let mut marked = false;
        running
            .wait_until("the east region announced, seeded and streaming", |event| {
                match event {
                    Event::NeighborDiscovered(info) if info.region_handle == east => {
                        announced = Some(info.sim);
                    }
                    Event::NeighborSeed { sim, .. } => seeded = Some(*sim),
                    Event::TerrainPatch(patch) if patch.region_handle == east => ground = true,
                    Event::ObjectAdded(object) | Event::ObjectUpdated(object)
                        if object.region_handle == east =>
                    {
                        // The neighbour's own content, and no avatar: a child
                        // agent has no body in the region it is only watching.
                        assert_ne!(object.pcode, sl_proto::pcode::AVATAR);
                        objects = true;
                    }
                    Event::GenericMessage(generic) => {
                        marked |= sl_fake_grid::neighbour_marker_region(generic).as_deref()
                            == Some("Fake Region East");
                    }
                    _other => {}
                }
                announced.is_some() && seeded.is_some() && ground && objects && marked
            })
            .await?;
        assert!(announced.is_some_and(|sim| sim.ip().is_loopback()));
        assert_eq!(seeded, announced, "the seed names the announced simulator");
        Ok(())
    }

    /// **An asset id is the whole grid's, not one region's.**
    ///
    /// A viewer fetches every asset over its **root** region's capability,
    /// including ids only another region's content references — the texture of
    /// the neighbour it can see across a border, most obviously. So a texture
    /// declared by the eastern region's fixture alone must be fetchable from
    /// the western region the agent is standing in.
    #[tokio::test]
    async fn an_asset_of_one_region_is_served_by_another() -> Result<(), TestError> {
        // The border fixture's checker is declared by the east region only.
        let marker = sl_fake_grid::fixtures::border::MARKER_TEXTURE;
        let east = sl_fake_grid::fixtures::border::border().into_region(adjacent_east_region());
        let mut running = start_in(vec![RegionConfig::default(), east]).await?;
        assert_eq!(running.region_name.as_deref(), Some("Fake Region"));

        running
            .commands
            .send(Command::FetchTexture {
                texture_id: marker,
                discard_level: sl_proto::j2c::DiscardLevel::FULL,
            })
            .await?;
        let mut bytes = None;
        running
            .wait_until("the neighbour's texture over this region's cap", |event| {
                if let Event::TextureReceived(fetched) = event
                    && fetched.id == marker
                {
                    bytes = Some(fetched.data.clone());
                }
                bytes.is_some()
            })
            .await?;
        let bytes = bytes.ok_or("no texture")?;
        let decoded = sl_texture::decode_j2c(&bytes, sl_proto::j2c::DiscardLevel::FULL)
            .map_err(|error| format!("the neighbour's checker did not decode: {error}"))?;
        assert!(decoded.width > 0 && decoded.height > 0);
        Ok(())
    }

    /// **The grid answers a sit, and the avatar rides its seat.**
    ///
    /// The prerequisite for anything about a *ridden* vehicle: the client's
    /// `AgentRequestSit`, the grid's `AvatarSitResponse`, the client's
    /// completing `AgentSit`, and then the thing that makes it visible — the
    /// avatar's own object update re-sent with the seat's region-local id as
    /// its `ParentID` and a position that is the offset from the seat rather
    /// than a place in the region.
    #[tokio::test]
    async fn the_agent_sits_on_a_prim_and_rides_it() -> Result<(), TestError> {
        let mut running = start().await?;
        let agent_id = running.agent.agent_id();
        let seat = sl_fake_grid::scenario::stock_scripted_object();
        let seat_local = sl_fake_grid::scenario::STOCK_SCRIPTED_OBJECT_LOCAL_ID;

        running
            .commands
            .send(Command::Sit {
                target: seat,
                offset: Vector {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
            })
            .await?;
        // The response and the avatar's re-rez in one wait: the grid sends them
        // in that order, but nothing about the transport guarantees a client
        // sees them in it, and a second wait would consume the first's.
        let mut answered = None;
        let mut seated = None;
        running
            .wait_until("the sit answered and the avatar aboard", |event| {
                match event {
                    Event::SitResult {
                        sit_object,
                        sit_position,
                        ..
                    } => answered = Some((*sit_object, sit_position.clone())),
                    Event::ObjectAdded(object) | Event::ObjectUpdated(object)
                        if object.full_id.uuid() == agent_id.uuid()
                            && object.parent_id == seat_local =>
                    {
                        seated = Some(object.motion.position.clone());
                    }
                    _other => {}
                }
                answered.is_some() && seated.is_some()
            })
            .await?;
        let answered = answered.ok_or("no sit response")?;
        assert_eq!(answered.0, seat);
        assert_eq!(
            answered.1,
            sl_fake_grid::world::SIT_TARGET_OFFSET,
            "the seat has a sit target, so the click point is not honoured"
        );
        assert_eq!(seated, Some(sl_fake_grid::world::SIT_TARGET_OFFSET));
        assert_eq!(
            running.agent.with_sim(|sim| sim.seated_on()).await,
            Some(seat),
            "the grid knows the agent is on the seat"
        );
        Ok(())
    }

    /// **A border crossing promotes the child circuit, and keeps the world.**
    ///
    /// The whole point of the crossing path next to the teleport one: no
    /// teleport screen, a `RegionChanged` that does *not* reset the world, the
    /// destination rooting the agent where it was told, and — unlike a teleport
    /// — the source circuit still open, now as the child of the region walked
    /// out of.
    #[tokio::test]
    async fn a_border_crossing_promotes_the_child_circuit() -> Result<(), TestError> {
        let mut running = start_in(vec![RegionConfig::default(), adjacent_east_region()]).await?;
        let east = running
            ._grid
            .region_handle("Fake Region East")
            .ok_or("no east region")?;
        let source = running.agent.clone();
        let source_seq = source.session_seq().await;
        let mut crossings = running._grid.crossings();

        // Wait for the neighbour's circuit to be up before crossing, so the
        // test exercises the reuse path rather than the announce-on-the-spot
        // fallback.
        running
            .wait_until("the east region's child circuit", |event| match event {
                Event::GenericMessage(generic) => {
                    sl_fake_grid::neighbour_marker_region(generic).as_deref()
                        == Some("Fake Region East")
                }
                _ => false,
            })
            .await?;

        let landing = RegionCoordinates::new(2.0, 128.0, 26.0);
        let walking = Vector {
            x: 3.2,
            y: 0.0,
            z: 0.0,
        };
        let destination = running
            ._grid
            .cross_agent(&source, "Fake Region East", landing, walking)
            .await?;

        let mut changed = None;
        running
            .wait_until("the arrival in the east region", |event| {
                if let Event::RegionChanged {
                    region_handle,
                    world_reset,
                    ..
                } = event
                {
                    changed = Some((*region_handle, *world_reset));
                }
                changed.is_some()
            })
            .await?;
        let (changed, world_reset) = changed.ok_or("no region change")?;
        assert_eq!(changed, east);
        assert!(
            !world_reset,
            "a crossing re-bases the scene it has; it does not throw it away"
        );

        let notice = tokio::time::timeout(WAIT, crossings.recv()).await??;
        assert_eq!(notice.from_seq, source_seq);
        assert_eq!(notice.region_name, "Fake Region East");
        assert_eq!(notice.agent_id, source.agent_id());
        assert_eq!(notice.to_seq, destination.session_seq().await);

        assert!(destination.with_sim(|sim| sim.is_root_agent()).await);
        assert_eq!(
            destination
                .with_sim(|sim| sim.arrival_position().position)
                .await,
            landing
        );
        assert!(
            !source.is_closed(),
            "the region walked out of keeps streaming as a child, unlike a teleport's source"
        );
        assert!(
            !source.with_sim(|sim| sim.is_root_agent()).await,
            "the source demoted itself to a child agent"
        );

        // The promoted circuit talks: a chat line from the destination reaches
        // the client, which is what "the client is rooted there now" means.
        destination
            .with_sim(|sim| {
                sim.send_chat_from_simulator(
                    "East",
                    sl_proto::ChatSource::System,
                    sl_proto::Uuid::nil(),
                    ChatType::Normal,
                    1,
                    sl_proto::Camera::region_center().center,
                    "walked in",
                    destination.now(),
                )
            })
            .await?;
        running
            .wait_until("the destination's chat line", |event| {
                matches!(event, Event::ChatReceived(message) if message.message == "walked in")
            })
            .await?;
        Ok(())
    }

    /// The two-region border grid a ridden crossing runs on: the same scene
    /// either side, with the vehicle **numbered differently** in each.
    ///
    /// That renumbering is the point. A handover keeps an object's grid-wide
    /// id and gives it the destination's own region-local one, so a rider's
    /// seat has to be re-found rather than merely kept.
    fn ridden_border_grid(ridden: bool) -> Vec<RegionConfig> {
        use sl_fake_grid::fixtures::border::{BorderSide, border_with_vehicle};
        vec![
            border_with_vehicle(BorderSide::Leaving, ridden).into_region(RegionConfig::default()),
            border_with_vehicle(BorderSide::Arriving, ridden).into_region(adjacent_east_region()),
        ]
    }

    /// Sits the agent on the border scene's vehicle and waits for the wire to
    /// show it riding: the sit response, then its own avatar object re-sent
    /// with the vehicle's `ParentID`.
    async fn ride_the_vehicle(running: &mut Running) -> Result<(), TestError> {
        let agent_id = running.agent.agent_id();
        running
            .commands
            .send(Command::Sit {
                target: sl_fake_grid::fixtures::border::VEHICLE_OBJECT,
                offset: Vector {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
            })
            .await?;
        running
            .wait_until("the agent aboard the vehicle", |event| match event {
                Event::ObjectAdded(object) | Event::ObjectUpdated(object) => {
                    object.full_id.uuid() == agent_id.uuid()
                        && object.parent_id
                            == sl_fake_grid::fixtures::border::BorderSide::Leaving
                                .vehicle_local_id()
                }
                _ => false,
            })
            .await?;
        Ok(())
    }

    /// Hands the vehicle over the border the way a simulator does: the region
    /// being left kills it, and the agent is seated on the destination's own
    /// copy — which carries the same full id under a different local id.
    ///
    /// The destination already streams its copy (it is in that region's
    /// fixtures, and its child circuit burst it long ago), so the handover is
    /// the *source* forgetting the object and the *destination* claiming the
    /// rider.
    async fn hand_the_vehicle_over(
        source: &FakeAgent,
        destination: &FakeAgent,
        riders: &[sl_proto::RegionLocalObjectId],
    ) -> Result<(), TestError> {
        use sl_fake_grid::fixtures::border;
        let mut leaving = vec![border::BorderSide::Leaving.vehicle_local_id()];
        leaving.extend_from_slice(riders);
        source
            .with_world(|world, sim| {
                world
                    .objects
                    .retain(|object| object.full_id != border::VEHICLE_OBJECT);
                world.npcs.clear();
                sim.send_kill_object(&leaving, sim_now())
            })
            .await?;
        destination
            .seat_on(
                border::BorderSide::Arriving.vehicle_local_id(),
                sl_fake_grid::world::SIT_TARGET_OFFSET,
            )
            .await;
        Ok(())
    }

    /// The instant a grid-side send is stamped with. The sessions here run on
    /// the system clock, so this is that clock.
    fn sim_now() -> std::time::Instant {
        std::time::Instant::now()
    }

    /// Waits for both halves of a handover: the region being left forgetting
    /// the vehicle, and the destination showing the agent aboard its own copy.
    ///
    /// **In one pass, because the two halves come from two different
    /// simulators.** UDP is ordered per circuit and says nothing across
    /// circuits, so the kill on the source and the re-seat on the destination
    /// may be seen in either order — and a wait for one would consume the
    /// other on its way past.
    async fn wait_for_the_handover(
        running: &mut Running,
        destination_region: sl_client_tokio::RegionHandle,
    ) -> Result<(), TestError> {
        use sl_fake_grid::fixtures::border;
        let agent_id = running.agent.agent_id();
        let mut left = false;
        let mut arrived = false;
        running
            .wait_until("the vehicle handed over the border", |event| {
                match event {
                    Event::ObjectRemoved { local_id, .. } => {
                        left |= local_id.id() == border::BorderSide::Leaving.vehicle_local_id();
                    }
                    Event::ObjectAdded(object) | Event::ObjectUpdated(object) => {
                        arrived |= object.full_id.uuid() == agent_id.uuid()
                            && object.region_handle == destination_region
                            && object.parent_id == border::BorderSide::Arriving.vehicle_local_id();
                    }
                    _other => {}
                }
                left && arrived
            })
            .await?;
        Ok(())
    }

    /// **An avatar riding a vehicle crosses the border with it.**
    ///
    /// The case that actually breaks in the wild. Everything about the seat is
    /// renumbered at the border — the vehicle's region-local id is the
    /// destination's, not the source's — while the client's own seat, which is
    /// keyed by the object's grid-wide id, must come through untouched, along
    /// with the sit-implied script permissions a stand would drop.
    #[tokio::test]
    async fn a_seated_avatar_crosses_with_its_vehicle() -> Result<(), TestError> {
        use sl_fake_grid::fixtures::border;
        let mut running = start_in(ridden_border_grid(false)).await?;
        let east = running
            ._grid
            .region_handle("Fake Region East")
            .ok_or("no east region")?;
        running
            .wait_until("the east region's child circuit", |event| match event {
                Event::GenericMessage(generic) => {
                    sl_fake_grid::neighbour_marker_region(generic).as_deref()
                        == Some("Fake Region East")
                }
                _ => false,
            })
            .await?;
        ride_the_vehicle(&mut running).await?;

        let source = running.agent.clone();
        let destination = running
            ._grid
            .cross_agent(
                &source,
                "Fake Region East",
                RegionCoordinates::new(border::MARKER_X, border::MARKER_Y, border::MARKER_Z),
                Vector {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
            )
            .await?;
        hand_the_vehicle_over(&source, &destination, &[]).await?;

        // The vehicle left one region and the rider is aboard the other's copy:
        // same object, the other region's local id.
        wait_for_the_handover(&mut running, east).await?;
        assert_ne!(
            border::BorderSide::Arriving.vehicle_local_id(),
            border::BorderSide::Leaving.vehicle_local_id(),
            "the seat is renumbered at the border, or this proves nothing"
        );
        assert_eq!(
            destination.with_sim(|sim| sim.seated_on()).await,
            Some(border::VEHICLE_OBJECT),
            "the destination inherited the sit rather than believing the agent stood up"
        );
        assert!(
            !source.is_closed(),
            "the region the vehicle left is still a neighbour"
        );
        Ok(())
    }

    /// **The other riders come too, and are re-seated on the same vehicle.**
    ///
    /// One rider proves a seat was re-found; two prove the *right* one was.
    #[tokio::test]
    async fn other_riders_cross_on_the_same_vehicle() -> Result<(), TestError> {
        use sl_fake_grid::fixtures::border;
        let mut running = start_in(ridden_border_grid(true)).await?;
        let east = running
            ._grid
            .region_handle("Fake Region East")
            .ok_or("no east region")?;
        // Both in one pass: the scripted rider's own update comes *inside* the
        // child burst and the marker closes it, so waiting for the marker
        // first would consume the rider's update on the way past.
        let mut rider_aboard = false;
        let mut burst_done = false;
        running
            .wait_until("the east region's scene, rider aboard", |event| {
                match event {
                    Event::ObjectAdded(object) | Event::ObjectUpdated(object) => {
                        rider_aboard |= object.local_id
                            == border::BorderSide::Arriving.rider_local_id()
                            && object.region_handle == east
                            && object.parent_id == border::BorderSide::Arriving.vehicle_local_id();
                    }
                    Event::GenericMessage(generic) => {
                        burst_done |= sl_fake_grid::neighbour_marker_region(generic).as_deref()
                            == Some("Fake Region East");
                    }
                    _other => {}
                }
                rider_aboard && burst_done
            })
            .await?;
        ride_the_vehicle(&mut running).await?;

        let source = running.agent.clone();
        let destination = running
            ._grid
            .cross_agent(
                &source,
                "Fake Region East",
                RegionCoordinates::new(border::MARKER_X, border::MARKER_Y, border::MARKER_Z),
                Vector {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
            )
            .await?;
        hand_the_vehicle_over(
            &source,
            &destination,
            &[border::BorderSide::Leaving.rider_local_id()],
        )
        .await?;

        wait_for_the_handover(&mut running, east).await?;
        Ok(())
    }

    /// An avatar walks over a border; it does not walk across a grid. A region
    /// the agent does not border is refused, and a region policy of
    /// [`NeighbourPolicy::None`] borders nothing at all.
    #[tokio::test]
    async fn a_crossing_is_refused_where_there_is_no_border() -> Result<(), TestError> {
        let running = start_in(vec![RegionConfig::default(), east_region()]).await?;
        assert!(
            running._grid.neighbours_of("Fake Region").is_empty(),
            "ten regions east is not next door"
        );
        let error = running
            ._grid
            .cross_agent(
                &running.agent,
                "Fake Region East",
                RegionCoordinates::new(2.0, 128.0, 26.0),
                Vector {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
            )
            .await
            .err()
            .ok_or("a distant region is not a border")?;
        assert!(matches!(error, sl_fake_grid::Error::NotAdjacent { .. }));
        assert!(
            !running.agent.is_closed(),
            "the refusal costs the session nothing"
        );

        let hermit = RegionConfig {
            neighbours: sl_fake_grid::NeighbourPolicy::None,
            ..RegionConfig::default()
        };
        let alone = start_in(vec![hermit, adjacent_east_region()]).await?;
        assert!(
            alone._grid.neighbours_of("Fake Region").is_empty(),
            "a region that announces no neighbours has none to cross into"
        );
        Ok(())
    }

    /// How long the failure tests let the grid wait for an arrival that is
    /// never coming.
    ///
    /// Long enough that it is a *timeout* and not a race with the announcement
    /// that precedes it, short enough that four tests of the failure half cost
    /// less than a second between them. `TELEPORT_ARRIVAL_TIMEOUT` is thirty
    /// seconds, which is right for a viewer on a bad link and wrong for a
    /// suite.
    const SHORT_HANDOVER: Duration = Duration::from_millis(250);

    /// **A teleport into a region the agent already borders reuses its child
    /// circuit.**
    ///
    /// The third shape of a teleport destination, and the one with no test of
    /// its own: not the same region (answered in place) and not a stranger
    /// (opened on the spot), but a neighbour the client is *already* holding a
    /// circuit to. Opening a second session there would hand the client two
    /// simulators for one region handle and stream the destination's scene
    /// twice.
    #[tokio::test]
    async fn a_teleport_to_a_neighbour_reuses_its_child_session() -> Result<(), TestError> {
        let mut running = start_in(vec![RegionConfig::default(), adjacent_east_region()]).await?;
        let east = running
            ._grid
            .region_handle("Fake Region East")
            .ok_or("no east region")?;
        let mut teleports = running._grid.teleports();
        running
            .wait_until("the east region's child circuit", |event| match event {
                Event::GenericMessage(generic) => {
                    sl_fake_grid::neighbour_marker_region(generic).as_deref()
                        == Some("Fake Region East")
                }
                _ => false,
            })
            .await?;

        // The session the *announcement* opened, before any teleport.
        let announced = running._grid.sessions_in("Fake Region East").await;
        assert_eq!(announced.len(), 1, "the neighbour was announced once");
        let announced_seq = announced.first().ok_or("no child")?.session_seq().await;

        let landing = RegionCoordinates::new(70.0, 200.0, 26.0);
        running
            .commands
            .send(Command::Teleport {
                region_handle: east,
                position: landing,
                look_at: Vector {
                    x: 0.0,
                    y: 1.0,
                    z: 0.0,
                },
            })
            .await?;
        running
            .wait_until("the arrival in the east region", |event| {
                matches!(event, Event::RegionChanged { region_handle, .. } if *region_handle == east)
            })
            .await?;

        let notice = tokio::time::timeout(WAIT, teleports.recv()).await??;
        assert_eq!(
            notice.to_seq, announced_seq,
            "the teleport landed in the session the neighbour announcement opened, not a new one"
        );
        let after = running._grid.sessions_in("Fake Region East").await;
        assert_eq!(
            after.len(),
            1,
            "one session in the destination region, not two"
        );
        let destination = after.first().ok_or("no destination session")?;
        assert!(destination.with_sim(|sim| sim.is_root_agent()).await);
        assert_eq!(
            destination
                .with_sim(|sim| sim.arrival_position().position)
                .await,
            landing,
            "the arrival is where the request asked, not where the child was built"
        );
        Ok(())
    }

    /// The teleport screen's phases must reach the client **in order**, and the
    /// neighbour teleport is where that is hardest: the destination session is
    /// already open, so the grid has almost no work to do between the UDP
    /// `TeleportStart` and the CAPS `TeleportFinish` — and those two travel on
    /// different transports, which the client's driver reads with an unbiased
    /// select. Announce the destination too soon and the client can decode the
    /// finish first, surfacing `TeleportFinished` ahead of `TeleportStarted`
    /// (see `sl-proto`'s `a_caps_finish_ahead_of_the_udp_start_reorders_the_phases`
    /// for exactly what that produces).
    ///
    /// The grid closes that window by waiting for the client's acknowledgement
    /// of the start before it announces the destination, so the whole sequence
    /// is collected here in **one** wait and asserted as an order, not a set.
    #[tokio::test]
    async fn a_neighbour_teleport_starts_before_it_finishes() -> Result<(), TestError> {
        let mut running = start_in(vec![RegionConfig::default(), adjacent_east_region()]).await?;
        let east = running
            ._grid
            .region_handle("Fake Region East")
            .ok_or("no east region")?;
        running
            .wait_until("the east region's child circuit", |event| match event {
                Event::GenericMessage(generic) => {
                    sl_fake_grid::neighbour_marker_region(generic).as_deref()
                        == Some("Fake Region East")
                }
                _ => false,
            })
            .await?;

        running
            .commands
            .send(Command::Teleport {
                region_handle: east,
                position: RegionCoordinates::new(128.0, 128.0, 26.0),
                look_at: Vector {
                    x: 0.0,
                    y: 1.0,
                    z: 0.0,
                },
            })
            .await?;

        let mut phases: Vec<&'static str> = Vec::new();
        running
            .wait_until("the arrival in the east region", |event| match event {
                Event::TeleportStarted => {
                    phases.push("started");
                    false
                }
                Event::TeleportFinished { .. } => {
                    phases.push("finished");
                    false
                }
                Event::RegionChanged { region_handle, .. } if *region_handle == east => {
                    phases.push("region-changed");
                    true
                }
                _ => false,
            })
            .await?;

        assert_eq!(
            phases,
            vec!["started", "finished", "region-changed"],
            "the teleport screen goes up before the handover is announced"
        );
        Ok(())
    }

    /// A same-region request opens no second session at all — the shape of the
    /// matrix that has nothing to hand over, and so nothing to time out.
    #[tokio::test]
    async fn a_local_teleport_opens_no_second_session() -> Result<(), TestError> {
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
        running
            .wait_until("the local teleport", |event| {
                matches!(event, Event::TeleportLocal { .. })
            })
            .await?;
        assert_eq!(
            running._grid.sessions_in("Fake Region").await.len(),
            1,
            "a local hop stays in the session it started in"
        );
        Ok(())
    }

    /// **A teleport the client never completes takes its own destination back
    /// down.**
    ///
    /// The failure half of the distant shape. The client is stopped so its
    /// movement can never complete, which is the one way to hold a handover
    /// open deterministically; what is asserted is the grid-side cleanup, since
    /// a stopped client cannot report what it was told. That the refusal
    /// *reaches* a live client is
    /// [`teleport_to_unknown_region_is_refused`]'s claim.
    #[tokio::test]
    async fn a_teleport_that_never_arrives_abandons_a_fresh_destination() -> Result<(), TestError> {
        let running = start_configured(
            vec![RegionConfig::default(), east_region()],
            Some(SHORT_HANDOVER),
            ImitatedGrid::default(),
        )
        .await?;
        assert!(
            running
                ._grid
                .sessions_in("Fake Region East")
                .await
                .is_empty(),
            "ten regions east is no neighbour, so nothing is open there yet"
        );

        // Stop the client: from here nothing can answer the destination.
        running.run.abort();
        let error = running
            ._grid
            .teleport_agent(
                &running.agent,
                "Fake Region East",
                RegionCoordinates::new(64.0, 32.0, 25.0),
                Vector {
                    x: 1.0,
                    y: 0.0,
                    z: 0.0,
                },
            )
            .await
            .err()
            .ok_or("a teleport nobody completes cannot succeed")?;
        assert!(matches!(error, sl_fake_grid::Error::TeleportTimedOut));
        assert!(
            running
                ._grid
                .sessions_in("Fake Region East")
                .await
                .is_empty(),
            "the session this teleport opened is abandoned with it"
        );
        assert!(
            !running.agent.is_closed() && running.agent.with_sim(|sim| sim.is_root_agent()).await,
            "the agent stays the root agent of the region it never left"
        );
        Ok(())
    }

    /// **A failed teleport leaves a *neighbour's* circuit alone.**
    ///
    /// The same failure, one destination shape over, and the opposite cleanup:
    /// the session was not this teleport's to abandon. It is still a
    /// neighbour, the client still holds its circuit, and tearing it down would
    /// punish it for a handover that failed elsewhere.
    #[tokio::test]
    async fn a_teleport_that_never_arrives_leaves_a_neighbour_child_alone() -> Result<(), TestError>
    {
        let mut running = start_configured(
            vec![RegionConfig::default(), adjacent_east_region()],
            Some(SHORT_HANDOVER),
            ImitatedGrid::default(),
        )
        .await?;
        running
            .wait_until("the east region's child circuit", |event| match event {
                Event::GenericMessage(generic) => {
                    sl_fake_grid::neighbour_marker_region(generic).as_deref()
                        == Some("Fake Region East")
                }
                _ => false,
            })
            .await?;
        let before = running._grid.sessions_in("Fake Region East").await;
        let before_seq = before.first().ok_or("no child")?.session_seq().await;

        running.run.abort();
        let error = running
            ._grid
            .teleport_agent(
                &running.agent,
                "Fake Region East",
                RegionCoordinates::new(64.0, 32.0, 25.0),
                Vector {
                    x: 1.0,
                    y: 0.0,
                    z: 0.0,
                },
            )
            .await
            .err()
            .ok_or("a teleport nobody completes cannot succeed")?;
        assert!(matches!(error, sl_fake_grid::Error::TeleportTimedOut));

        let after = running._grid.sessions_in("Fake Region East").await;
        assert_eq!(after.len(), 1, "the neighbour's circuit is still there");
        let survivor = after.first().ok_or("no child")?;
        assert_eq!(
            survivor.session_seq().await,
            before_seq,
            "and it is the same session, not a replacement"
        );
        assert!(!survivor.is_closed());
        assert!(
            !running.agent.is_closed() && running.agent.with_sim(|sim| sim.is_root_agent()).await,
            "the agent stays the root agent of the region it never left"
        );
        Ok(())
    }

    /// **A teleport retires the children of the region it left, and the
    /// destination announces its own.**
    ///
    /// A crossing has always done the first half. A teleport used to do
    /// neither, so an agent that hopped across the grid left one open circuit
    /// per region it had ever bordered, each still streaming to a client that
    /// is now nowhere near it.
    #[tokio::test]
    async fn a_teleport_retires_the_children_of_the_region_left_behind() -> Result<(), TestError> {
        let mut running = start_in(vec![
            RegionConfig::default(),
            next_door_region(),
            east_region(),
        ])
        .await?;
        let far = running
            ._grid
            .region_handle("Fake Region East")
            .ok_or("no far region")?;
        let mut teleports = running._grid.teleports();
        running
            .wait_until("the neighbour's child circuit", |event| match event {
                Event::GenericMessage(generic) => {
                    sl_fake_grid::neighbour_marker_region(generic).as_deref()
                        == Some("Fake Region Next Door")
                }
                _ => false,
            })
            .await?;
        assert_eq!(
            running
                ._grid
                .sessions_in("Fake Region Next Door")
                .await
                .len(),
            1,
            "the neighbour is open before the teleport"
        );

        running
            .commands
            .send(Command::Teleport {
                region_handle: far,
                position: RegionCoordinates::new(64.0, 32.0, 25.0),
                look_at: Vector {
                    x: 1.0,
                    y: 0.0,
                    z: 0.0,
                },
            })
            .await?;
        let notice = tokio::time::timeout(WAIT, teleports.recv()).await??;
        assert_eq!(notice.region_name, "Fake Region East");

        assert!(
            running
                ._grid
                .sessions_in("Fake Region Next Door")
                .await
                .is_empty(),
            "the region the agent left bordered this one; the destination does not"
        );
        assert!(
            running._grid.sessions_in("Fake Region").await.is_empty(),
            "and the source itself was retired"
        );
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
                    running.agent.now(),
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

    /// Every builder method of `PrimFixture` survives the round trip through
    /// the real wire: the catalogue's prims are seeded on the grid, the real
    /// client decodes the `ObjectUpdate`s they travel in, and the typed
    /// `extra` / particle / texture-animation views plus the raw
    /// `texture_entry` blob it comes back with are exactly what the fixture
    /// encoded.
    ///
    /// The comparison is against the fixture's own **blob**, not its typed
    /// view: the wire is the contract, and some blocks (the flexi floats) are
    /// quantized on the way out, so what a viewer can possibly see is what
    /// decoding the blob gives.
    #[tokio::test]
    async fn the_catalogue_reaches_the_client_field_for_field() -> Result<(), TestError> {
        let fixture = sl_fake_grid::catalogue();
        let seeded: Vec<sl_proto::Object> = fixture.world.objects.clone();
        assert!(
            seeded.len() >= sl_fake_grid::fixtures::catalogue::NAMES.len(),
            "the catalogue seeded only {} objects",
            seeded.len()
        );
        let region = fixture.into_region(RegionConfig::default());
        let mut running = start_in(vec![region]).await?;

        // Collect the client's view of every seeded prim; the arrival burst
        // sends them all in one `ObjectUpdate`, but the client surfaces them
        // one event each and may re-send.
        let wanted: std::collections::HashSet<sl_proto::RegionLocalObjectId> =
            seeded.iter().map(|object| object.local_id).collect();
        let mut seen: std::collections::HashMap<sl_proto::RegionLocalObjectId, sl_proto::Object> =
            std::collections::HashMap::new();
        while seen.len() < wanted.len() {
            running
                .wait_for(|event| {
                    if let Event::ObjectAdded(object) | Event::ObjectUpdated(object) = event
                        && wanted.contains(&object.local_id)
                    {
                        let _previous = seen.insert(object.local_id, (**object).clone());
                    }
                    Some(())
                })
                .await?;
        }

        for fixture_object in &seeded {
            let name = sl_fake_grid::fixtures::catalogue::entries()
                .into_iter()
                .find(|entry| entry.local_id == fixture_object.local_id)
                .map_or_else(|| "linkset-child".to_owned(), |entry| entry.name.to_owned());
            let received = seen
                .get(&fixture_object.local_id)
                .ok_or_else(|| format!("{name} never reached the client"))?;

            assert_eq!(
                received.full_id, fixture_object.full_id,
                "{name} arrived with a different full id"
            );
            assert_eq!(
                received.texture_entry, fixture_object.texture_entry,
                "{name}'s texture entry changed on the wire"
            );
            assert_eq!(
                received.extra,
                sl_proto::decode_extra_params(&fixture_object.extra_params),
                "{name}'s extra params changed on the wire"
            );
            assert_eq!(
                received.particles,
                sl_proto::decode_particle_system(&fixture_object.particle_system),
                "{name}'s particle system changed on the wire"
            );
            assert_eq!(
                received.texture_animation,
                sl_proto::decode_texture_anim(&fixture_object.texture_anim),
                "{name}'s texture animation changed on the wire"
            );
            assert_eq!(
                received.parent_id, fixture_object.parent_id,
                "{name} arrived under a different parent"
            );
            assert_eq!(
                received.text, fixture_object.text,
                "{name}'s hover text changed on the wire"
            );
            assert_eq!(
                received.motion.position, fixture_object.motion.position,
                "{name} arrived somewhere else"
            );
        }
        Ok(())
    }

    /// The catalogue's NPC arrives as another avatar: its body, then the
    /// appearance naming its bakes, then the animations it plays, then the
    /// attachment parented to it — and the bakes the appearance names are
    /// fetchable over `GetTexture`, the way a viewer gets them on OpenSim.
    #[tokio::test]
    async fn the_catalogue_npc_arrives_with_appearance_and_attachment() -> Result<(), TestError> {
        use sl_fake_grid::fixtures::catalogue::{
            NPC_AGENT, NPC_ANIMATION, NPC_ATTACHMENT_LOCAL_ID, NPC_ATTACHMENT_POINT, NPC_LOCAL_ID,
            npc,
        };

        let npc = npc();
        let region = sl_fake_grid::catalogue().into_region(RegionConfig::default());
        let mut running = start_in(vec![region]).await?;

        // 1. The body: an avatar-pcode object under the NPC's own id, wearing
        //    the name-values the viewer labels it with.
        let body = running
            .wait_for(|event| match event {
                Event::ObjectAdded(object) | Event::ObjectUpdated(object)
                    if object.local_id == NPC_LOCAL_ID =>
                {
                    Some((**object).clone())
                }
                _ => None,
            })
            .await?;
        assert_eq!(body.pcode, sl_proto::pcode::AVATAR);
        assert_eq!(body.full_id.uuid(), NPC_AGENT);
        assert!(
            body.name_values()
                .iter()
                .any(|pair| pair.name == "FirstName" && pair.value == "Catalogue"),
            "the NPC arrived unnamed: {:?}",
            body.name_value
        );

        // 2. The appearance: the bakes in their `avatar_texture` slots.
        let appearance = running
            .wait_for(|event| match event {
                Event::AvatarAppearance(appearance) if appearance.avatar_id.uuid() == NPC_AGENT => {
                    Some((**appearance).clone())
                }
                _ => None,
            })
            .await?;
        for bake in &npc.appearance.bakes {
            assert_eq!(
                appearance.texture_entry.texture_id(bake.slot),
                Some(bake.texture),
                "the bake for slot {} did not survive",
                bake.slot
            );
        }
        assert_eq!(
            appearance.visual_params.len(),
            npc.appearance.visual_params.len(),
            "the visual params were truncated on the wire"
        );

        // 3. The animations it is playing.
        let animations = running
            .wait_for(|event| match event {
                Event::AvatarAnimation {
                    avatar_id,
                    animations,
                    ..
                } if avatar_id.uuid() == NPC_AGENT => Some(animations.clone()),
                _ => None,
            })
            .await?;
        assert!(
            animations
                .iter()
                .any(|animation| animation.anim_id == NPC_ANIMATION),
            "the NPC is not playing its animation: {animations:?}"
        );

        // 4. The attachment: a child of the body, on the point it is worn at.
        let attachment = running
            .wait_for(|event| match event {
                Event::ObjectAdded(object) | Event::ObjectUpdated(object)
                    if object.local_id == NPC_ATTACHMENT_LOCAL_ID =>
                {
                    Some((**object).clone())
                }
                _ => None,
            })
            .await?;
        assert_eq!(attachment.parent_id, NPC_LOCAL_ID);
        assert_eq!(attachment.attachment_point_id(), Some(NPC_ATTACHMENT_POINT));
        assert!(
            appearance
                .attachments
                .iter()
                .any(|worn| worn.id == attachment.full_id),
            "the appearance does not list the attachment"
        );

        // 5. The bakes are served: the appearance names ids a viewer can
        //    actually fetch.
        let head = npc
            .appearance
            .bakes
            .first()
            .ok_or("the catalogue NPC has no bakes")?
            .texture;
        running
            .commands
            .send(Command::FetchTexture {
                texture_id: head,
                discard_level: sl_proto::j2c::DiscardLevel::FULL,
            })
            .await?;
        let bake = running
            .wait_for(|event| match event {
                Event::TextureReceived(fetched) if fetched.id == head => Some(fetched.data.clone()),
                _ => None,
            })
            .await?;
        assert!(!bake.is_empty(), "the NPC's head bake came back empty");
        Ok(())
    }

    /// Every built-in library texture a viewer falls back to on arrival — the
    /// sun and moon discs, the cloud noise, the two sky overlays, the star
    /// bloom, the wave normal and the blank plywood — is served under its real
    /// Linden id, and what comes back decodes.
    ///
    /// Without them a stock arrival is eight fetches that each burn six
    /// retries before giving up, drowning the arrival log; and the sky draws no
    /// sun at all. Decoding rather than only length-checking is the point: an
    /// unfetchable id and an id serving eight bytes of nothing fail the same
    /// way in a renderer.
    #[tokio::test]
    async fn every_built_in_library_texture_is_fetchable() -> Result<(), TestError> {
        let mut running = start().await?;
        let wanted: Vec<uuid::Uuid> = sl_proto::BUILTIN_ENVIRONMENT_TEXTURES
            .into_iter()
            .chain(core::iter::once(sl_proto::DEFAULT_PRIM_TEXTURE))
            .collect();
        for id in wanted {
            let texture_id = sl_client_tokio::TextureKey::from(id);
            running
                .commands
                .send(Command::FetchTexture {
                    texture_id,
                    discard_level: sl_proto::j2c::DiscardLevel::FULL,
                })
                .await?;
            let bytes = running
                .wait_for(|event| match event {
                    Event::TextureReceived(fetched) if fetched.id == texture_id => {
                        Some(fetched.data.clone())
                    }
                    _ => None,
                })
                .await?;
            let decoded = sl_texture::decode_j2c(&bytes, sl_proto::j2c::DiscardLevel::FULL)
                .map_err(|error| format!("the built-in texture {id} did not decode: {error}"))?;
            assert!(
                decoded.width > 0 && decoded.height > 0,
                "the built-in texture {id} decoded to nothing"
            );
        }
        Ok(())
    }

    /// The arriving agent is sent its **own** `AvatarAppearance`, and the
    /// bakes it names are fetchable.
    ///
    /// Without one, a viewer has no visual params and no texture entry for
    /// itself: it spawns the avatar, poses its skeleton, and draws no body at
    /// all — which is exactly what a fake-grid login looked like, a name tag
    /// hanging in mid-air over nothing.
    #[tokio::test]
    async fn the_arriving_agent_gets_its_own_appearance() -> Result<(), TestError> {
        let mut running = start().await?;
        let me = running.agent.agent_id();
        let appearance = running
            .wait_for(|event| match event {
                Event::AvatarAppearance(appearance) if appearance.avatar_id == me => {
                    Some((**appearance).clone())
                }
                _ => None,
            })
            .await?;
        assert_eq!(
            appearance.visual_params.len(),
            sl_fake_grid::fixtures::npcs::DEFAULT_VISUAL_PARAMS.len(),
            "the own avatar's visual params were truncated on the wire"
        );

        // The agent is also told what it is *playing*, in the same arrival
        // burst. A real simulator always has an answer — OpenSim stands an
        // arriving agent up before it has moved — and an avatar the grid
        // signals nothing for is one no motion drives: the reference viewer
        // draws it folded forwards in its raw rest pose, which is what a
        // fake-grid arrival looked like once the body became visible at all.
        //
        // Asserted here rather than after the fetches below, because
        // `wait_for` discards what it does not match: a later wait would throw
        // this event away while draining the texture replies.
        let animations = running
            .wait_for(|event| match event {
                Event::AvatarAnimation {
                    avatar_id,
                    animations,
                    ..
                } if *avatar_id == me => Some(animations.clone()),
                _ => None,
            })
            .await?;
        assert!(
            animations.iter().any(|animation| {
                sl_anim::builtin_animation(animation.anim_id)
                    .is_some_and(|builtin| builtin.name == "stand")
            }),
            "the arriving agent is not standing: {animations:?}"
        );

        // Every baked slot the entry names is served, so the body is painted
        // rather than left with seven failed texture fetches.
        let mut baked = 0_usize;
        for slot in [
            sl_proto::avatar_texture::HEAD_BAKED,
            sl_proto::avatar_texture::UPPER_BAKED,
            sl_proto::avatar_texture::LOWER_BAKED,
        ] {
            let texture = appearance
                .texture_entry
                .texture_id(slot)
                .ok_or("a baked slot with no texture")?;
            assert_ne!(
                texture.uuid(),
                sl_proto::avatar_texture::IMG_DEFAULT_AVATAR,
                "slot {slot} is still the un-baked sentinel"
            );
            running
                .commands
                .send(Command::FetchTexture {
                    texture_id: texture,
                    discard_level: sl_proto::j2c::DiscardLevel::FULL,
                })
                .await?;
            let bytes = running
                .wait_for(|event| match event {
                    Event::TextureReceived(fetched) if fetched.id == texture => {
                        Some(fetched.data.clone())
                    }
                    _ => None,
                })
                .await?;
            assert!(!bytes.is_empty(), "slot {slot}'s bake came back empty");
            // A bake is a five-component (`R G B alpha mask`) codestream, and
            // for these three slots in particular that is not cosmetic: the
            // reference viewer asks its fetcher for the fifth plane as the
            // avatar's morph mask, and when the read fails it discards the
            // colour decode along with it and marks the texture missing — so a
            // three-component bake that fetches, decodes and looks perfect
            // still leaves the agent's own avatar a cloud forever.
            let header = sl_proto::j2c::parse_header_unvalidated(&bytes)
                .ok_or("slot {slot}'s bake is not a J2C codestream")?;
            assert_eq!(
                header.components, 5,
                "slot {slot}'s bake is not a five-component baked avatar texture"
            );
            baked += 1;
        }
        assert_eq!(baked, 3, "one bake per body region");
        Ok(())
    }

    /// The catalogue's assets are actually served: the checker texture comes
    /// back over `GetTexture` and the mesh over `GetMesh2`, so a prim naming
    /// one is not pointing at a 404.
    #[tokio::test]
    async fn the_catalogue_assets_are_fetchable() -> Result<(), TestError> {
        use sl_fake_grid::fixtures::catalogue::{CHECKER_TEXTURE, MESH_ASSET};

        let region = sl_fake_grid::catalogue().into_region(RegionConfig::default());
        let mut running = start_in(vec![region]).await?;
        running
            .commands
            .send(Command::FetchTexture {
                texture_id: CHECKER_TEXTURE,
                discard_level: sl_proto::j2c::DiscardLevel::FULL,
            })
            .await?;
        let texture = running
            .wait_for(|event| match event {
                Event::TextureReceived(fetched) if fetched.id == CHECKER_TEXTURE => {
                    Some(fetched.data.clone())
                }
                _ => None,
            })
            .await?;
        assert!(!texture.is_empty(), "the checker texture came back empty");

        running
            .commands
            .send(Command::FetchMesh {
                mesh_id: MESH_ASSET,
                byte_range: None,
            })
            .await?;
        let mesh = running
            .wait_for(|event| match event {
                Event::AssetReceived(fetched) if fetched.id == MESH_ASSET.uuid() => {
                    Some(fetched.data.clone())
                }
                _ => None,
            })
            .await?;
        // What comes back is the mesh asset the fixture wrote: its header
        // parses and names every level of detail.
        assert_eq!(mesh, sl_test_assets::mesh::unit_cube_mesh_asset()?);
        Ok(())
    }

    /// The animation the catalogue's NPC plays is fetchable over
    /// `ViewerAsset` and decodes as a keyframe motion — so an avatar the
    /// viewer records as animating has a motion it can actually play, rather
    /// than falling back to its own idle.
    #[tokio::test]
    async fn the_npc_animation_asset_is_fetchable_and_decodes() -> Result<(), TestError> {
        use sl_fake_grid::fixtures::catalogue::NPC_ANIMATION;

        let region = sl_fake_grid::catalogue().into_region(RegionConfig::default());
        let mut running = start_in(vec![region]).await?;
        running
            .commands
            .send(Command::FetchAsset {
                asset_id: sl_client_tokio::AssetKey::from(NPC_ANIMATION),
                asset_type: sl_proto::AssetType::Animation,
                byte_range: None,
            })
            .await?;
        let bytes = running
            .wait_for(|event| match event {
                Event::AssetReceived(fetched) if fetched.id == NPC_ANIMATION => {
                    Some(fetched.data.clone())
                }
                _ => None,
            })
            .await?;
        assert_eq!(
            bytes,
            sl_test_assets::anim::chest_twist_animation_asset(),
            "the served animation is not the fixture's"
        );
        // The viewer's own decoder is the contract: a motion with a joint that
        // has keyframes to interpolate between.
        let motion = sl_anim::Motion::from_bytes(&bytes)?;
        let joint = motion.joints.first().ok_or("the motion animates nothing")?;
        assert!(joint.rotation_keys.len() >= 2);
        assert!(motion.duration > 0.0);
        Ok(())
    }

    /// A **sound** asset a region was handed is fetchable under whatever id the
    /// fixture filed it under, and arrives byte-identical.
    ///
    /// `AssetType::Sound` had no producer at all until `sl-test-assets::sound`,
    /// so no fixture could put one on a grid; this is the proof that the class
    /// travels the same `ViewerAsset` path every other asset does. That the
    /// bytes are a *playable* Ogg Vorbis tone is pinned where they are written
    /// (`sl-test-assets` decodes them through `symphonium`, the decoder
    /// `sl-audio` plays clips with), so this test does not repeat it.
    #[tokio::test]
    async fn a_sound_asset_is_fetchable_under_its_own_id() -> Result<(), TestError> {
        let sound_id = uuid::Uuid::from_u128(0x50_0000_0001);
        let sound = sl_test_assets::sound::marker_tone(sl_test_assets::sound::tones::MID)?;

        let mut fixture = sl_fake_grid::fixtures::RegionFixture::new();
        let _previous = fixture
            .assets
            .insert(sl_proto::AssetKey::from(sound_id), sound.clone());
        let region = fixture.into_region(RegionConfig::default());

        let mut running = start_in(vec![region]).await?;
        running
            .commands
            .send(Command::FetchAsset {
                asset_id: sl_client_tokio::AssetKey::from(sound_id),
                asset_type: sl_proto::AssetType::Sound,
                byte_range: None,
            })
            .await?;
        let bytes = running
            .wait_for(|event| match event {
                Event::AssetReceived(fetched) if fetched.id == sound_id => {
                    Some(fetched.data.clone())
                }
                _ => None,
            })
            .await?;
        assert_eq!(bytes, sound, "the served sound is not the fixture's");
        assert_eq!(
            bytes.get(0..4),
            Some(b"OggS".as_slice()),
            "the served sound is not an Ogg stream"
        );
        Ok(())
    }

    /// An EEP **settings** asset a region was handed is fetchable under its own
    /// id, and what comes back decodes into the day cycle the fixture wrote.
    ///
    /// This is the half of EEP the `ExtEnvironment` capability cannot reach:
    /// that path carries a *typed* `EnvironmentSettings` and never produces
    /// asset bytes, so until `sl-test-assets::environment` no fixture could put
    /// an environment in an inventory, offer one, or serve one by id.
    #[tokio::test]
    async fn a_settings_asset_is_fetchable_under_its_own_id() -> Result<(), TestError> {
        let settings_id = uuid::Uuid::from_u128(0x5E_0000_0001);
        let asset = sl_test_assets::environment::day_cycle_asset();

        let mut fixture = sl_fake_grid::fixtures::RegionFixture::new();
        let _previous = fixture
            .assets
            .insert(sl_proto::AssetKey::from(settings_id), asset.clone());
        let region = fixture.into_region(RegionConfig::default());

        let mut running = start_in(vec![region]).await?;
        running
            .commands
            .send(Command::FetchAsset {
                asset_id: sl_client_tokio::AssetKey::from(settings_id),
                asset_type: sl_proto::AssetType::Settings,
                byte_range: None,
            })
            .await?;
        let bytes = running
            .wait_for(|event| match event {
                Event::AssetReceived(fetched) if fetched.id == settings_id => {
                    Some(fetched.data.clone())
                }
                _ => None,
            })
            .await?;
        assert_eq!(
            bytes, asset,
            "the served settings asset is not the fixture's"
        );
        let decoded = sl_proto::environment_asset_from_bytes(
            sl_test_assets::environment::DAY_CYCLE_NAME,
            &bytes,
        )
        .ok_or("the served bytes are not a settings asset")?;
        assert_eq!(
            decoded,
            sl_proto::EnvironmentAsset::DayCycle(
                Box::new(sl_test_assets::environment::day_cycle())
            )
        );
        Ok(())
    }

    /// One avatar's session, for a test that needs two of them on one grid and
    /// so cannot let each own it (a [`Running`] drops its own grid).
    struct Joined {
        /// The client's root circuit id (for scoped ids).
        circuit: sl_client_tokio::CircuitId,
        /// The client event stream.
        events: mpsc::Receiver<Event>,
        /// The client command channel.
        commands: mpsc::Sender<Command>,
        /// This avatar's grid-side session handle, for the fixtures a test
        /// reads off the grid rather than over the wire (the folder a take
        /// files into).
        agent: FakeAgent,
        /// The run-loop task (aborted on teardown).
        run: tokio::task::JoinHandle<Result<(), sl_client_tokio::Error>>,
    }

    impl Drop for Joined {
        fn drop(&mut self) {
            self.run.abort();
        }
    }

    /// Logs `first_name` in against an already-started grid, starts its run
    /// loop and waits for the region handshake.
    async fn join(grid: &FakeGrid, first_name: &str) -> Result<Joined, TestError> {
        // Subscribed before the login, or the notice this login raises is one
        // nobody was listening for.
        let mut logins = grid.logins();
        let request = LoginRequest::new(
            first_name,
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
        let circuit = client.root_circuit_id().ok_or("no root circuit")?;
        let (event_tx, mut events) = mpsc::channel::<Event>(256);
        let (commands, command_rx) = mpsc::channel::<Command>(8);
        let (diag_tx, _diag_rx) = mpsc::channel(16);
        let run = tokio::spawn(client.run(event_tx, diag_tx, command_rx));
        wait_on(&mut events, |event| {
            matches!(
                event,
                Event::RegionHandshakeComplete | Event::RegionChanged { .. }
            )
            .then_some(())
        })
        .await?;
        Ok(Joined {
            circuit,
            events,
            commands,
            agent,
            run,
        })
    }

    /// Rezzes a cube at `position` from `avatar`'s client and returns the
    /// object the simulator minted for it — the ids the client could not have
    /// known before it asked.
    async fn rez_cube(
        avatar: &mut Joined,
        position: &Vector,
    ) -> Result<sl_client_tokio::Object, TestError> {
        avatar
            .commands
            .send(Command::RezObject {
                shape: sl_client_tokio::PrimShape::cube(position.clone()),
                group_id: None,
            })
            .await?;
        wait_on(&mut avatar.events, |event| match event {
            Event::ObjectAdded(object) if object.motion.position == *position => {
                Some((**object).clone())
            }
            _ => None,
        })
        .await
    }

    /// Selects `local_id` from `avatar`'s client and returns the full
    /// properties record the simulator answers the selection with.
    ///
    /// A selection is a subscription, not a lock: it is what makes this client
    /// one of the ones told when somebody *else* changes the object.
    async fn select(
        avatar: &mut Joined,
        local_id: sl_client_tokio::RegionLocalObjectId,
        object_id: sl_client_tokio::ObjectKey,
    ) -> Result<sl_client_tokio::ObjectProperties, TestError> {
        avatar
            .commands
            .send(Command::RequestObjectProperties {
                local_ids: vec![sl_client_tokio::ScopedObjectId::new(
                    avatar.circuit,
                    local_id,
                )],
            })
            .await?;
        properties_of(avatar, object_id).await
    }

    /// The next full properties record `avatar` is sent for `object_id`,
    /// however it was prompted.
    async fn properties_of(
        avatar: &mut Joined,
        object_id: sl_client_tokio::ObjectKey,
    ) -> Result<sl_client_tokio::ObjectProperties, TestError> {
        wait_on(&mut avatar.events, |event| match event {
            Event::ObjectProperties(properties) if properties.object_id == object_id => {
                Some((**properties).clone())
            }
            _ => None,
        })
        .await
    }

    /// How long a test waits before concluding that a push it must **not**
    /// receive is not coming.
    ///
    /// Every push in these tests is published inside the writing session's own
    /// flush and forwarded by a watcher task on the same loopback grid, so the
    /// real latency is sub-millisecond; the margin is three orders of magnitude
    /// above it and is only there so a loaded machine cannot turn a working
    /// subscription into a passing test.
    const SILENCE: Duration = Duration::from_secs(2);

    /// Asserts `avatar` is sent no properties for `object_id` within
    /// [`SILENCE`] — that it is not on the subscription.
    async fn expect_no_properties(
        avatar: &mut Joined,
        object_id: sl_client_tokio::ObjectKey,
    ) -> Result<(), TestError> {
        let quiet = tokio::time::timeout(SILENCE, async {
            loop {
                let Some(event) = avatar.events.recv().await else {
                    return Ok(());
                };
                if matches!(&event, Event::ObjectProperties(properties)
                    if properties.object_id == object_id)
                {
                    return Err("a deselected client was still sent the object's properties");
                }
            }
        })
        .await;
        match quiet {
            // The wait ran out with nothing matching, which is the pass.
            Err(_elapsed) => Ok(()),
            Ok(result) => result.map_err(Into::into),
        }
    }

    /// A rez is a change to the **region**, not to the rezzing circuit's view
    /// of it: the other avatar standing in the same region is shown the new
    /// object without asking for it, and the derez that takes it away kills it
    /// for both.
    ///
    /// This is what the region-scoped world buys. With a world cloned per
    /// session the first avatar would rez into its own copy and the second
    /// would never hear of it — and neither would the *next* avatar to arrive,
    /// whose arrival burst is read from the same store.
    #[tokio::test]
    async fn a_rez_reaches_the_regions_other_avatar() -> Result<(), TestError> {
        let grid = FakeGridBuilder::new()
            .account(AccountConfig::new("First", "User", "password"))
            .account(AccountConfig::new("Second", "User", "password"))
            .event_queue_hold(Duration::from_secs(2))
            .region(RegionConfig::default())
            .start()
            .await?;
        let mut first = join(&grid, "First").await?;
        let mut second = join(&grid, "Second").await?;

        let position = Vector {
            x: 140.0,
            y: 128.0,
            z: 26.0,
        };
        // The rezzing client is told directly, in the same breath as the
        // mutation, because it cannot use the object until it knows the ids
        // the simulator chose.
        let rezzed = rez_cube(&mut first, &position).await?;

        // The other avatar is told by the region.
        let seen = wait_on(&mut second.events, |event| match event {
            Event::ObjectAdded(object) if object.full_id == rezzed.full_id => {
                Some((**object).clone())
            }
            _ => None,
        })
        .await?;
        assert_eq!(seen.local_id, rezzed.local_id);
        assert_eq!(seen.motion.position, position);
        assert_eq!(seen.scale, rezzed.scale);

        // A return mints no inventory item, so the requester gets a `DeRezAck`
        // rather than an `UpdateCreateInventoryItem` -- and both avatars get
        // the kill.
        let expected_transaction =
            sl_client_tokio::TransactionId::from(uuid::Uuid::from_u128(0x0DE5));
        first
            .commands
            .send(Command::DerezObjects {
                local_ids: vec![sl_client_tokio::ScopedObjectId::new(
                    first.circuit,
                    rezzed.local_id,
                )],
                destination: sl_client_tokio::DeRezDestination::ReturnToOwner,
                transaction_id: expected_transaction,
                group_id: None,
            })
            .await?;
        let acked = wait_on(&mut first.events, |event| match event {
            Event::DeRezAck {
                transaction,
                success,
            } if *transaction == expected_transaction => Some(*success),
            _ => None,
        })
        .await?;
        assert!(acked, "a return of an object the region has is refused");
        for (name, avatar) in [("the rezzer", &mut first), ("the bystander", &mut second)] {
            let removed = wait_on(&mut avatar.events, |event| match event {
                Event::ObjectRemoved { local_id, .. } if local_id.id() == rezzed.local_id => {
                    Some(*local_id)
                }
                _ => None,
            })
            .await;
            assert!(
                removed.is_ok(),
                "{name} was never told the returned object went away"
            );
        }

        // And the region itself has forgotten it, so the next avatar to arrive
        // is not shown a ghost.
        let held = grid
            .sessions_in(&grid.region_names().first().ok_or("no regions")?.clone())
            .await;
        let session = held.first().ok_or("no live session")?;
        let still_there = session
            .with_world(|world, _sim| world.object_by_local_id(rezzed.local_id))
            .await;
        assert_eq!(
            still_there, None,
            "the region still holds the returned object"
        );
        Ok(())
    }

    /// A grid serving two accounts in one region.
    async fn two_avatar_grid() -> Result<FakeGrid, TestError> {
        Ok(FakeGridBuilder::new()
            .account(AccountConfig::new("First", "User", "password").estate_manager())
            .account(AccountConfig::new("Second", "User", "password"))
            .event_queue_hold(Duration::from_secs(2))
            .region(RegionConfig::default())
            .start()
            .await?)
    }

    /// Takes a freshly rezzed cube into `avatar`'s Objects folder and returns
    /// the inventory item the simulator minted — a donor a task-inventory
    /// write can drop into something else.
    async fn take_a_donor_item(
        avatar: &mut Joined,
        position: &Vector,
    ) -> Result<sl_client_tokio::InventoryItem, TestError> {
        let donor = rez_cube(avatar, position).await?;
        let objects_folder = avatar
            .agent
            .with_sim(|sim| {
                sim.agent_inventory()
                    .folders()
                    .find(|folder| {
                        folder.folder_type == sl_client_tokio::FolderType::Object.to_code()
                    })
                    .map(|folder| folder.folder_id)
            })
            .await
            .ok_or("the seeded account has no Objects folder")?;
        avatar
            .commands
            .send(Command::DerezObjects {
                local_ids: vec![sl_client_tokio::ScopedObjectId::new(
                    avatar.circuit,
                    donor.local_id,
                )],
                destination: sl_client_tokio::DeRezDestination::TakeIntoAgentInventory(
                    objects_folder,
                ),
                transaction_id: sl_client_tokio::TransactionId::from(uuid::Uuid::from_u128(0x0D02)),
                group_id: None,
            })
            .await?;
        wait_for_taken_item(&mut avatar.events).await
    }

    /// The `UpdateTaskInventory` payload that drops `item` into a prim. The
    /// simulator resolves the item by id from the agent's own inventory, so the
    /// masks and CRC carried here are along for the ride.
    fn task_item(item: &sl_client_tokio::InventoryItem) -> sl_client_tokio::RestoreItem {
        sl_client_tokio::RestoreItem {
            item_id: item.item_id,
            folder_id: item.folder_id,
            creator_id: item.creator_id,
            owner: item.owner,
            group: item.group,
            permissions: item.permissions,
            transaction_id: uuid::Uuid::from_u128(0x0D03),
            asset_type: item.item_type,
            inv_type: item.inv_type,
            flags: item.flags,
            sale_type: sl_client_tokio::SaleType::from_code(item.sale_type),
            sale_price: item.sale_price.clone(),
            name: item.name.clone(),
            description: item.description.clone(),
            creation_date: item.creation_date,
            crc: 0,
        }
    }

    /// Two residents with the same prim selected: one writes into its task
    /// inventory and the other is told, unasked, by the contents serial on the
    /// properties record — and stops being told the moment it deselects.
    ///
    /// This is the collision that has no arbitration behind it. Second Life has
    /// no edit lock: selection is a subscription, both residents may hold the
    /// prim indefinitely, and the loser of a race is simply the one who wrote
    /// second. What makes that survivable is that the loser is *told* — the
    /// contents serial is a prim's whole freshness marker, it travels in the
    /// properties record rather than in an `ObjectUpdate`, and a viewer that
    /// never receives it keeps a listing the region no longer holds and cannot
    /// tell the difference.
    #[tokio::test]
    async fn a_prims_contents_change_reaches_the_other_selector() -> Result<(), TestError> {
        let grid = two_avatar_grid().await?;
        let mut first = join(&grid, "First").await?;
        let mut second = join(&grid, "Second").await?;

        let container = rez_cube(
            &mut first,
            &Vector {
                x: 141.0,
                y: 129.0,
                z: 26.0,
            },
        )
        .await?;
        // The region told the second avatar about the rez; it can select what
        // it was shown.
        let seen = wait_on(&mut second.events, |event| match event {
            Event::ObjectAdded(object) if object.full_id == container.full_id => {
                Some((**object).clone())
            }
            _ => None,
        })
        .await?;

        // Both look. A fresh prim holds nothing, so both read serial zero.
        let mine = select(&mut first, container.local_id, container.full_id).await?;
        let theirs = select(&mut second, seen.local_id, container.full_id).await?;
        assert_eq!(
            mine.inventory_serial, 0,
            "a freshly rezzed prim reported a non-zero contents serial"
        );
        assert_eq!(
            theirs.inventory_serial, mine.inventory_serial,
            "the two selectors were answered different contents serials"
        );

        // One of them writes.
        let donor = take_a_donor_item(
            &mut first,
            &Vector {
                x: 143.0,
                y: 129.0,
                z: 26.0,
            },
        )
        .await?;
        first
            .commands
            .send(Command::UpdateTaskInventory {
                target: sl_client_tokio::ScopedObjectId::new(first.circuit, container.local_id),
                key: sl_client_tokio::TaskInventoryKey::Item,
                item: Box::new(task_item(&donor)),
            })
            .await?;

        // The writer is told in the same breath as the mutation...
        let written = properties_of(&mut first, container.full_id).await?;
        assert!(
            written.inventory_serial > mine.inventory_serial,
            "the writer's own contents serial did not advance ({} then {})",
            mine.inventory_serial,
            written.inventory_serial
        );
        // ...and the other resident, who asked for nothing, by the region.
        let told = properties_of(&mut second, container.full_id).await?;
        assert_eq!(
            told.inventory_serial, written.inventory_serial,
            "the other selector was not told the prim's contents changed"
        );

        // A read from either now returns the surviving record, which is the
        // whole of what "converged" means when nothing arbitrates.
        let reread = select(&mut second, seen.local_id, container.full_id).await?;
        assert_eq!(
            reread.inventory_serial, written.inventory_serial,
            "a re-read returned a serial neither write left behind"
        );

        // Deselecting ends the subscription: a resident who has stopped looking
        // is not sent the next change, exactly as a simulator stops sending it.
        second
            .commands
            .send(Command::DeselectObjects {
                local_ids: vec![sl_client_tokio::ScopedObjectId::new(
                    second.circuit,
                    seen.local_id,
                )],
            })
            .await?;
        first
            .commands
            .send(Command::UpdateTaskInventory {
                target: sl_client_tokio::ScopedObjectId::new(first.circuit, container.local_id),
                key: sl_client_tokio::TaskInventoryKey::Item,
                item: Box::new(task_item(&donor)),
            })
            .await?;
        let again = properties_of(&mut first, container.full_id).await?;
        assert!(
            again.inventory_serial > written.inventory_serial,
            "the second write did not advance the contents serial"
        );
        expect_no_properties(&mut second, container.full_id).await?;
        Ok(())
    }

    /// Two residents standing on the same parcel: one saves About Land and the
    /// other is sent the whole record unasked — and a save built from that push
    /// carries the first resident's change forward, where a save built from the
    /// record read at open silently reverts it.
    ///
    /// The revert is the sharp end of the whole task.
    /// `ParcelPropertiesUpdate` carries the *entire* record, not the field that
    /// changed, so a floater populated once and never re-read asserts every
    /// other field as it stood minutes ago. No grid stops it — there is nothing
    /// on the grid side that could tell a re-asserted field from an unchanged
    /// one — which makes converging the viewer's job and the unsolicited push
    /// the only material it has to do it with.
    #[tokio::test]
    async fn an_about_land_save_reaches_the_parcels_other_occupant() -> Result<(), TestError> {
        let grid = two_avatar_grid().await?;
        let mut first = join(&grid, "First").await?;
        let mut second = join(&grid, "Second").await?;

        // What each avatar's About Land floater would have been seeded from:
        // the parcel it was pushed on arrival.
        let opened_first = next_parcel(&mut first).await?;
        let opened_second = next_parcel(&mut second).await?;
        assert_eq!(
            opened_second.local_id, opened_first.local_id,
            "the two avatars did not arrive on the same parcel"
        );

        // The first resident renames the parcel.
        let mut renamed = opened_first.to_update();
        renamed.name = "First's Land".to_owned();
        first.commands.send(Command::UpdateParcel(renamed)).await?;
        let echoed = next_parcel(&mut first).await?;
        assert_eq!(echoed.name, "First's Land");

        // The second resident, who asked for nothing, is sent the whole record
        // under the sequence id of an unsolicited push.
        let pushed = next_parcel(&mut second).await?;
        assert_eq!(
            pushed.name, "First's Land",
            "the parcel's other occupant was not told about the About Land save"
        );
        assert_eq!(
            pushed.sequence_id, 0,
            "a pushed parcel record carried a request's sequence id"
        );

        // The hazard, staged: a save built from the record the second resident
        // read at open re-asserts the old name and undoes the rename.
        let mut stale = opened_second.to_update();
        stale.description = "Second's description".to_owned();
        second.commands.send(Command::UpdateParcel(stale)).await?;
        let reverted = next_parcel(&mut second).await?;
        assert_eq!(
            reverted.description, "Second's description",
            "the second resident's own edit did not take"
        );
        assert_eq!(
            reverted.name, opened_second.name,
            "a save from a stale record was expected to revert the rename"
        );

        // And the property that matters: a save built from the *pushed* record
        // changes the one field the resident touched and leaves the other
        // resident's change standing.
        let restored = next_parcel(&mut first).await?;
        let mut converged = restored.to_update();
        converged.name = "First's Land".to_owned();
        first
            .commands
            .send(Command::UpdateParcel(converged))
            .await?;
        let settled = next_parcel(&mut first).await?;
        assert_eq!(settled.name, "First's Land");
        assert_eq!(
            settled.description, "Second's description",
            "a save from the pushed record reverted a field it never touched"
        );
        Ok(())
    }

    /// The next parcel record `avatar` is sent, however it was prompted.
    async fn next_parcel(avatar: &mut Joined) -> Result<sl_client_tokio::ParcelInfo, TestError> {
        wait_on(&mut avatar.events, |event| match event {
            Event::ParcelProperties(parcel) => Some((**parcel).clone()),
            _ => None,
        })
        .await
    }

    /// An estate manager saves the Region tab and every avatar in the region is
    /// sent the new `RegionInfo`, not only the one who saved it.
    ///
    /// A region's configuration has no subscription to belong to — every avatar
    /// in the region is standing in it — so this is the one push of the three
    /// that goes to everybody.
    #[tokio::test]
    async fn a_region_info_save_reaches_the_regions_other_avatar() -> Result<(), TestError> {
        let grid = two_avatar_grid().await?;
        let mut first = join(&grid, "First").await?;
        let mut second = join(&grid, "Second").await?;

        second.commands.send(Command::RequestRegionInfo).await?;
        let before = next_region_limits(&mut second).await?;
        let raised = before.max_agents.saturating_add(7);

        // The save re-asserts the whole tab, so the two fields this case does
        // not touch are carried over from what it just read -- the same shape
        // as the About Land form, and the same hazard if they were not.
        let update = sl_client_tokio::RegionInfoUpdate {
            agent_limit: i32::try_from(raised)?,
            object_bonus: before.object_bonus_factor,
            maturity: before.maturity,
            ..sl_client_tokio::RegionInfoUpdate::default()
        };
        first.commands.send(Command::SetRegionInfo(update)).await?;

        let saved = next_region_limits(&mut first).await?;
        assert_eq!(
            saved.max_agents, raised,
            "the estate manager's own region save did not take"
        );
        let told = next_region_limits(&mut second).await?;
        assert_eq!(
            told.max_agents, raised,
            "the region's other avatar was not told its configuration changed"
        );
        Ok(())
    }

    /// The next region-limits record `avatar` is sent, however it was prompted.
    async fn next_region_limits(
        avatar: &mut Joined,
    ) -> Result<sl_client_tokio::RegionLimits, TestError> {
        wait_on(&mut avatar.events, |event| match event {
            Event::RegionLimits(limits) => Some(limits.clone()),
            _ => None,
        })
        .await
    }

    /// An estate manager applies new ground textures and every avatar in the
    /// region is re-handshaked, not only the one who applied them.
    ///
    /// The commit is the only estate command whose whole purpose is that
    /// *everybody* sees it — a `RegionHandshake` is the one message a terrain
    /// composition travels in, and a viewer that never receives it keeps
    /// drawing the ground it arrived on. The handshake is stamped with the
    /// receiving session's own identity, so what the region publishes is the
    /// composition and each watcher builds its own message.
    #[tokio::test]
    async fn a_terrain_texture_commit_reaches_the_regions_other_avatar() -> Result<(), TestError> {
        let grid = two_avatar_grid().await?;
        let mut first = join(&grid, "First").await?;
        let mut second = join(&grid, "Second").await?;

        let ground = uuid::Uuid::from_u128(0xF0_0DBEEF);
        let terrain = sl_client_tokio::RegionTerrainUpdate {
            detail_textures: [ground; 4],
            ..sl_client_tokio::RegionTerrainUpdate::default()
        };
        first
            .commands
            .send(Command::SetRegionTerrain(terrain))
            .await?;

        for (name, avatar) in [
            ("the estate manager", &mut first),
            ("the bystander", &mut second),
        ] {
            let handshake = wait_on(&mut avatar.events, |event| match event {
                Event::RegionInfoHandshake(identity)
                    if identity.terrain.detail_textures[0] == ground =>
                {
                    Some((**identity).clone())
                }
                _ => None,
            })
            .await;
            assert!(
                handshake.is_ok(),
                "{name} was never re-handshaked after the terrain commit"
            );
        }
        Ok(())
    }

    /// A **linkset** survives a take and comes back whole.
    ///
    /// Two things are being asserted, and neither holds by accident. A take
    /// names only the root — that is what a viewer selects — so the child has
    /// to be gathered on the grid side or it stays behind in the region with a
    /// parent nothing holds any more. And what comes back is rebuilt out of the
    /// asset text: the region mints four fresh ids (two local, two keys), reads
    /// the root's placement from `pos` and the child's from `childpos`, and
    /// re-parents the child to the root's *new* local id. A child rezzed at its
    /// stored offset but parented to nothing would stand a metre from the root
    /// instead of on it, and the region would still look right in the log.
    #[tokio::test]
    async fn a_taken_linkset_rezzes_back_whole() -> Result<(), TestError> {
        let grid = two_avatar_grid().await?;
        let mut avatar = join(&grid, "First").await?;

        let base = Vector {
            x: 140.0,
            y: 130.0,
            z: 26.0,
        };
        let above = Vector {
            z: base.z + 1.0,
            ..base.clone()
        };
        let root = rez_cube(&mut avatar, &base).await?;
        let child = rez_cube(&mut avatar, &above).await?;
        avatar
            .commands
            .send(Command::LinkObjects {
                local_ids: vec![
                    sl_client_tokio::ScopedObjectId::new(avatar.circuit, root.local_id),
                    sl_client_tokio::ScopedObjectId::new(avatar.circuit, child.local_id),
                ],
            })
            .await?;
        // The child re-sent with the root's local id as its `ParentID`: the
        // link took, and its position is now the offset a take will serialise.
        let child_key = child.full_id;
        let root_local_id = root.local_id;
        let linked = wait_on(&mut avatar.events, |event| match event {
            Event::ObjectUpdated(object)
                if object.full_id == child_key && object.parent_id == root_local_id =>
            {
                Some((**object).clone())
            }
            _ => None,
        })
        .await?;

        let objects_folder = avatar
            .agent
            .with_sim(|sim| {
                sim.agent_inventory()
                    .folders()
                    .find(|folder| {
                        folder.folder_type == sl_client_tokio::FolderType::Object.to_code()
                    })
                    .map(|folder| folder.folder_id)
            })
            .await
            .ok_or("the seeded account has no Objects folder")?;
        // The take names the root alone, exactly as a viewer's selection does.
        avatar
            .commands
            .send(Command::DerezObjects {
                local_ids: vec![sl_client_tokio::ScopedObjectId::new(
                    avatar.circuit,
                    root.local_id,
                )],
                destination: sl_client_tokio::DeRezDestination::TakeIntoAgentInventory(
                    objects_folder,
                ),
                transaction_id: sl_client_tokio::TransactionId::from(uuid::Uuid::from_u128(0x11E5)),
                group_id: None,
            })
            .await?;
        // The filed item and both kills in **one** pass, because their order is
        // the grid's and not this test's to assume. The kills go out over UDP
        // straight away; a Second-Life-flavoured take's item rides the event
        // queue and lands on the next long-poll, so it arrives *after* them.
        // Draining for the item first is how this test hung once the flavour
        // started deciding the announcement — the wait for the item ate both
        // kills on its way past, and the wait for the kills then never ended.
        let mut standing: Vec<_> = [root.local_id, child.local_id]
            .into_iter()
            .map(|local_id| sl_client_tokio::ScopedObjectId::new(avatar.circuit, local_id))
            .collect();
        let mut taken = None;
        while taken.is_none() || !standing.is_empty() {
            let event = tokio::time::timeout(WAIT, avatar.events.recv())
                .await?
                .ok_or("client event stream ended early")?;
            match event {
                Event::InventoryItemCreated { item, .. }
                    if i32::from(item.item_type) == sl_proto::AssetType::Object.to_code() =>
                {
                    taken = Some(item);
                }
                Event::InventoryBulkUpdate { items, .. } => {
                    taken = items
                        .into_iter()
                        .find(|item| {
                            i32::from(item.item_type) == sl_proto::AssetType::Object.to_code()
                        })
                        .or(taken);
                }
                Event::ObjectRemoved { local_id, .. } => {
                    standing.retain(|waiting| *waiting != local_id);
                }
                _other => {}
            }
        }
        let item = taken.ok_or("the take filed no object item")?;

        let landing = Vector {
            x: 150.0,
            y: 150.0,
            z: 30.0,
        };
        avatar
            .commands
            .send(Command::RezObjectFromInventory {
                params: Box::new(rez_params(&item, &landing)),
            })
            .await?;
        let mut rezzed = Vec::new();
        while rezzed.len() < 2 {
            let object = wait_on(&mut avatar.events, |event| match event {
                Event::ObjectAdded(object) => Some((**object).clone()),
                _ => None,
            })
            .await?;
            rezzed.push(object);
        }
        let (rezzed_root, rezzed_child) = match rezzed.as_slice() {
            [first, second] if first.parent_id == sl_client_tokio::RegionLocalObjectId(0) => {
                (first, second)
            }
            [first, second] => (second, first),
            _unreachable => return Err("the rez streamed something other than two objects".into()),
        };
        assert_eq!(
            rezzed_child.parent_id, rezzed_root.local_id,
            "the rezzed child was not parented to the rezzed root"
        );
        assert_ne!(
            rezzed_root.full_id, root.full_id,
            "the rez reused the taken object's key instead of minting one"
        );
        assert_eq!(
            rezzed_root.motion.position, landing,
            "the root did not land where the ray ended"
        );
        assert_eq!(
            rezzed_child.motion.position, linked.motion.position,
            "the child came back at something other than its offset from the root"
        );
        assert_eq!(rezzed_root.scale, root.scale);
        assert_eq!(rezzed_child.scale, child.scale);
        Ok(())
    }

    /// **A take tells a viewer nothing about where the object lives**, and the
    /// rez works anyway.
    ///
    /// This is the fake grid imitating Second Life, which is what it does unless
    /// asked otherwise ([`sl_fake_grid::ObjectAssetPolicy`]). Measured on aditi
    /// 2026-09-06: eleven of eleven object inventory items answered with a nil
    /// `asset_id`, in the AIS3 listing and in the per-item fetch, every one of
    /// them full-perm to its owner — so the withholding is about the class, not
    /// about permissions, and no viewer can open an object asset there at all.
    ///
    /// The second half is the half that is easy to get wrong. Withholding the id
    /// must not cost the resident the object: on Second Life a taken object
    /// drags back out of inventory like any other, because the *simulator*
    /// resolves the body and the viewer never needed it. A fake grid that
    /// withheld the id by simply not writing the body would pass the first
    /// assertion here and fail a resident.
    #[tokio::test]
    async fn a_take_withholds_the_object_asset_and_the_rez_still_works() -> Result<(), TestError> {
        let grid = two_avatar_grid().await?;
        let mut avatar = join(&grid, "First").await?;

        let item = take_a_donor_item(
            &mut avatar,
            &Vector {
                x: 143.0,
                y: 137.0,
                z: 26.0,
            },
        )
        .await?;
        assert!(
            item.asset_id.is_nil(),
            "the take named an asset ({}); Second Life names none",
            item.asset_id
        );

        let landing = Vector {
            x: 144.0,
            y: 138.0,
            z: 28.0,
        };
        avatar
            .commands
            .send(Command::RezObjectFromInventory {
                params: Box::new(rez_params(&item, &landing)),
            })
            .await?;
        let rezzed = wait_on(&mut avatar.events, |event| match event {
            Event::ObjectAdded(object) if object.motion.position == landing => {
                Some((**object).clone())
            }
            _ => None,
        })
        .await?;
        assert_eq!(
            rezzed.motion.position, landing,
            "an item with no asset id did not come back into the world"
        );
        Ok(())
    }

    /// The other grid: a take **names** the object's asset, and it is fetchable
    /// and describes the object that was taken.
    ///
    /// OpenSim's side of the same divergence, asked for explicitly
    /// ([`sl_fake_grid::ObjectAssetPolicy::Served`]). Two things are asserted
    /// together because either alone would pass a grid that is broken in the
    /// other way: an id nothing serves is the failure the whole asset-id family
    /// exists to catch, and bytes that describe some *other* prim would be a
    /// take that filed the wrong object.
    #[tokio::test]
    async fn an_opensim_flavoured_take_names_a_fetchable_object_asset() -> Result<(), TestError> {
        let grid = FakeGridBuilder::new()
            .account(AccountConfig::new("First", "User", "password"))
            .event_queue_hold(Duration::from_secs(2))
            .object_assets(sl_fake_grid::ObjectAssetPolicy::Served)
            .region(RegionConfig::default())
            .start()
            .await?;
        let mut avatar = join(&grid, "First").await?;

        let position = Vector {
            x: 147.0,
            y: 139.0,
            z: 26.0,
        };
        // The object's own key, kept from the rez: what the asset has to name
        // back if the take serialised the prim it was pointed at.
        let taken = rez_cube(&mut avatar, &position).await?;
        let objects_folder = avatar
            .agent
            .with_sim(|sim| {
                sim.agent_inventory()
                    .folders()
                    .find(|folder| {
                        folder.folder_type == sl_client_tokio::FolderType::Object.to_code()
                    })
                    .map(|folder| folder.folder_id)
            })
            .await
            .ok_or("the seeded account has no Objects folder")?;
        avatar
            .commands
            .send(Command::DerezObjects {
                local_ids: vec![sl_client_tokio::ScopedObjectId::new(
                    avatar.circuit,
                    taken.local_id,
                )],
                destination: sl_client_tokio::DeRezDestination::TakeIntoAgentInventory(
                    objects_folder,
                ),
                transaction_id: sl_client_tokio::TransactionId::from(uuid::Uuid::from_u128(0x0B1E)),
                group_id: None,
            })
            .await?;
        let item = wait_for_taken_item(&mut avatar.events).await?;
        assert!(
            !item.asset_id.is_nil(),
            "an OpenSim-flavoured take filed an item with no asset id"
        );

        avatar
            .commands
            .send(Command::FetchAsset {
                asset_id: sl_client_tokio::AssetKey::from(item.asset_id),
                asset_type: sl_proto::AssetType::Object,
                byte_range: None,
            })
            .await?;
        let fetched_id = item.asset_id;
        let bytes = wait_on(&mut avatar.events, |event| match event {
            Event::AssetReceived(fetched) if fetched.id == fetched_id => Some(fetched.data.clone()),
            _ => None,
        })
        .await?;
        let asset = sl_object_asset::ObjectAsset::decode(&bytes)?;
        let prim = asset.root().ok_or("the taken object's asset has no prim")?;
        assert_eq!(
            prim.task_id,
            taken.full_id.uuid(),
            "the asset names a different prim than the one taken"
        );
        assert_eq!(prim.name, item.name);
        Ok(())
    }

    /// A **no-copy** item is consumed by the rez: the object goes into the
    /// world and the item is gone, and the client is told so.
    ///
    /// This is OpenSim's rule (`DoPostRezWhenFromItem`) and it is decided by the
    /// item's own owner mask, not by the `remove_item` flag the client sent —
    /// the flag is what the viewer *expected*, and a viewer that dragged out a
    /// no-copy object and kept the item would be showing a copy the grid does
    /// not have. Everything the fake grid mints is full-permission, so the copy
    /// bit is cleared on the grid side here to reach the arm at all.
    #[tokio::test]
    async fn rezzing_a_no_copy_item_consumes_it() -> Result<(), TestError> {
        let grid = two_avatar_grid().await?;
        let mut avatar = join(&grid, "First").await?;

        let item = take_a_donor_item(
            &mut avatar,
            &Vector {
                x: 145.0,
                y: 135.0,
                z: 26.0,
            },
        )
        .await?;
        let item_id = item.item_id;
        let stripped = avatar
            .agent
            .with_sim(|sim| {
                let mut held = sim.agent_inventory().item(item_id).cloned()?;
                held.permissions.owner =
                    held.permissions.owner & !sl_client_tokio::Permissions::COPY;
                sim.agent_inventory_mut().insert_item(held.clone());
                Some(held)
            })
            .await
            .ok_or("the take's item was not in the agent's inventory")?;

        avatar
            .commands
            .send(Command::RezObjectFromInventory {
                params: Box::new(rez_params(
                    &stripped,
                    &Vector {
                        x: 146.0,
                        y: 136.0,
                        z: 27.0,
                    },
                )),
            })
            .await?;
        // The object still rezzes -- consuming the item is what happens
        // *after* the rez, not instead of it.
        let rezzed = wait_on(&mut avatar.events, |event| match event {
            Event::ObjectAdded(object) => Some((**object).clone()),
            _ => None,
        })
        .await?;
        assert_eq!(
            rezzed.motion.position,
            Vector {
                x: 146.0,
                y: 136.0,
                z: 27.0
            }
        );
        let removed = wait_on(&mut avatar.events, |event| match event {
            Event::InventoryItemsRemoved { items } if items.contains(&item_id) => Some(()),
            _ => None,
        })
        .await;
        assert!(
            removed.is_ok(),
            "a no-copy item survived the rez that consumed it"
        );
        let still_held = avatar
            .agent
            .with_sim(|sim| sim.agent_inventory().item(item_id).is_some())
            .await;
        assert!(
            !still_held,
            "the grid kept a no-copy item it told the client it had removed"
        );
        Ok(())
    }

    /// The [`RezObjectParams`](sl_client_tokio::RezObjectParams) that rez `item`
    /// at `position`, as a viewer's drag-out builds them: the ray is bypassed so
    /// the object lands exactly there, and the masks and CRC are what the client
    /// believes about the item rather than anything the grid checks.
    fn rez_params(
        item: &sl_client_tokio::InventoryItem,
        position: &Vector,
    ) -> sl_client_tokio::RezObjectParams {
        sl_client_tokio::RezObjectParams {
            group_id: None,
            from_task_id: None,
            bypass_raycast: true,
            ray_start: position.clone(),
            ray_end: position.clone(),
            ray_target_id: None,
            ray_end_is_intersection: false,
            rez_selected: false,
            remove_item: false,
            item_flags: item.flags,
            group_mask: item.permissions.group.bits(),
            everyone_mask: item.permissions.everyone.bits(),
            next_owner_mask: item.permissions.next_owner.bits(),
            item: task_item(item),
        }
    }
}
