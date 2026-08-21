//! End-to-end tests: the real `sl-client-tokio` stack — login POST, UDP
//! circuit, seed fetch, event-queue long-poll — against the fake grid.

#[cfg(test)]
mod test {
    use std::time::Duration;

    use pretty_assertions::{assert_eq, assert_ne};
    use sl_client_tokio::{
        ChatChannel, ChatType, Client, Command, Event, LoginParams, LoginRequest, StartLocation,
    };
    use sl_fake_grid::{AccountConfig, FakeAgent, FakeGrid, FakeGridBuilder, RegionConfig};
    use sl_proto::ServerEvent;
    use tokio::sync::mpsc;

    /// A boxed error for terse test signatures.
    type TestError = Box<dyn std::error::Error>;

    /// How long any single wait in these tests may take.
    const WAIT: Duration = Duration::from_secs(10);

    /// Starts a grid, connects the real client, and returns both plus the
    /// grid-side agent handle.
    async fn connect() -> Result<(FakeGrid, Client, FakeAgent), TestError> {
        let grid = FakeGridBuilder::new()
            .account(AccountConfig::new("Test", "User", "password"))
            .region(RegionConfig::default())
            .event_queue_hold(Duration::from_secs(2))
            .start()
            .await?;
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
    }

    impl Drop for Running {
        fn drop(&mut self) {
            self.run.abort();
        }
    }

    /// Connects, starts the run loop, and waits for the region handshake.
    async fn start() -> Result<Running, TestError> {
        let (grid, client, agent) = connect().await?;
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
        };
        running
            .wait_for(|event| {
                matches!(
                    event,
                    Event::RegionHandshakeComplete | Event::RegionChanged { .. }
                )
                .then_some(())
            })
            .await?;
        Ok(running)
    }

    impl Running {
        /// Receives client events until `pick` returns a value.
        async fn wait_for<T>(
            &mut self,
            mut pick: impl FnMut(&Event) -> Option<T>,
        ) -> Result<T, TestError> {
            loop {
                let event = tokio::time::timeout(WAIT, self.events.recv())
                    .await?
                    .ok_or("client event stream ended early")?;
                if let Some(value) = pick(&event) {
                    return Ok(value);
                }
            }
        }
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

    #[tokio::test]
    async fn two_grids_run_in_parallel() -> Result<(), TestError> {
        let (first_grid, first_client, _first_agent) = connect().await?;
        let (second_grid, second_client, _second_agent) = connect().await?;
        assert_ne!(first_grid.http_port(), second_grid.http_port());
        assert!(first_client.agent_id().is_some());
        assert!(second_client.agent_id().is_some());
        Ok(())
    }
}
