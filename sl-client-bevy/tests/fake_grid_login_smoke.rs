//! Smoke tier: the real [`SlClientPlugin`] — its socket-owning network
//! thread, blocking login, retransmission and CAPS polling — in a headless
//! Bevy app against an in-process [`sl_fake_grid`] grid.
//!
//! Behaviour lives in the headless interaction/world tiers; this proves the
//! plumbing those tiers bypass is sound: login → circuit → `RegionHandshake`
//! → `SlEvent` stream → `maintain_world` region/parcel state, plus a chat
//! round-trip out, an object update in, a CAPS event through the long-poll,
//! and a clean logout.

#[cfg(test)]
mod test {
    use std::time::{Duration, Instant};

    use bevy::prelude::*;
    use pretty_assertions::{assert_eq, assert_ne};
    use sl_client_bevy::{
        ChatChannel, ChatLogConfig, ChatType, ClientDirectories, Command, InventoryCacheConfig,
        LoginParams, LoginRequest, RegionLocalObjectId, SlAgentParcel, SlCapabilities,
        SlClientPlugin, SlCommand, SlCurrentRegion, SlEvent, SlIdentity, SlParcel, SlParcelOverlay,
        SlRegion, SlRegionIdentity, SlSessionEvent, StartLocation,
    };
    use sl_fake_grid::scenario::{
        STOCK_PARCEL_LOCAL_ID, STOCK_PARCEL_NAME, STOCK_SCRIPTED_OBJECT_LOCAL_ID,
        STOCK_SCRIPTED_OBJECT_POSITION, stock_scripted_object,
    };
    use sl_fake_grid::{AccountConfig, FakeGrid, FakeGridBuilder, RegionConfig};
    use sl_proto::{ObjectKey, ServerEvent};
    use tokio::sync::broadcast;

    /// A boxed error for terse test signatures.
    type TestError = Box<dyn std::error::Error>;

    /// How long any single wait may take before the test fails with the
    /// recorded event tail.
    const WAIT: Duration = Duration::from_secs(15);

    /// How long to sleep between frames while waiting.
    const FRAME_PAUSE: Duration = Duration::from_millis(5);

    /// Every [`SlSessionEvent`] the plugin emitted, in order.
    #[derive(Resource, Default)]
    struct Recorded {
        /// The session events, oldest first.
        events: Vec<SlSessionEvent>,
        /// Every capability map the plugin published.
        capabilities: Vec<std::collections::HashMap<String, String>>,
    }

    /// Appends this frame's events and capability maps to [`Recorded`].
    fn record(
        mut events: MessageReader<SlEvent>,
        mut capabilities: MessageReader<SlCapabilities>,
        mut recorded: ResMut<Recorded>,
    ) {
        for event in events.read() {
            recorded.events.push(event.0.clone());
        }
        for caps in capabilities.read() {
            recorded.capabilities.push(caps.0.clone());
        }
    }

    /// The grid, the app logged into it, and the grid-side handles.
    struct Harness {
        /// The tokio runtime hosting the grid's tasks (must outlive the grid).
        runtime: tokio::runtime::Runtime,
        /// The grid (dropping it shuts everything down).
        grid: FakeGrid,
        /// The headless app running the real client plugin.
        app: App,
        /// Login notices, subscribed before the app starts.
        logins: broadcast::Receiver<sl_fake_grid::LoginNotice>,
    }

    impl Harness {
        /// Starts a grid with one account and builds (but does not step) a
        /// headless app logging into it.
        fn start(channel: &str) -> Result<Self, TestError> {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()?;
            let grid = runtime.block_on(
                FakeGridBuilder::new()
                    .account(AccountConfig::new("Test", "User", "password"))
                    .region(RegionConfig::default())
                    .event_queue_hold(Duration::from_secs(2))
                    .start(),
            )?;
            let logins = grid.logins();
            let params = LoginParams {
                login_uri: grid.login_uri(),
                request: LoginRequest::new(
                    "Test",
                    "User",
                    "password",
                    StartLocation::Last,
                    channel,
                    "0.0",
                ),
            };
            let mut app = App::new();
            app.add_plugins(MinimalPlugins)
                .add_plugins(SlClientPlugin {
                    params,
                    diagnostics: false,
                    chat_log_config: ChatLogConfig::default(),
                    directories: ClientDirectories::default(),
                    account_dirs: None,
                    inventory_cache_config: InventoryCacheConfig::default(),
                    background_inventory_fetch: false,
                    fetch_server_chat_history: false,
                    offline: false,
                })
                .init_resource::<Recorded>()
                // After the plugin's `(drive, maintain_world)` chain so a
                // frame's world state and its events are observed together.
                .add_systems(PostUpdate, record);
            Ok(Self {
                runtime,
                grid,
                app,
                logins,
            })
        }

        /// Steps frames until `done` returns `Some`, or fails after [`WAIT`]
        /// with the recorded event tail.
        fn step_until<T>(
            &mut self,
            what: &str,
            mut done: impl FnMut(&mut App) -> Option<T>,
        ) -> Result<T, TestError> {
            let deadline = Instant::now().checked_add(WAIT).ok_or("clock overflow")?;
            loop {
                self.app.update();
                if let Some(value) = done(&mut self.app) {
                    return Ok(value);
                }
                if Instant::now() >= deadline {
                    let tail: Vec<String> = self
                        .recorded()
                        .events
                        .iter()
                        .rev()
                        .take(12)
                        .map(|event| {
                            let text = format!("{event:?}");
                            text.chars().take(160).collect::<String>()
                        })
                        .collect();
                    return Err(
                        format!("timed out waiting for {what}; last events: {tail:#?}").into(),
                    );
                }
                std::thread::sleep(FRAME_PAUSE);
            }
        }

        /// Steps frames until an event matching `pick` has been recorded.
        fn wait_for_event<T>(
            &mut self,
            what: &str,
            mut pick: impl FnMut(&SlSessionEvent) -> Option<T>,
        ) -> Result<T, TestError> {
            self.step_until(what, |app| {
                app.world()
                    .resource::<Recorded>()
                    .events
                    .iter()
                    .find_map(&mut pick)
            })
        }

        /// Steps frames until the grid has broadcast a matching [`ServerEvent`]
        /// on `events` (drained without blocking the frame thread).
        fn wait_for_server_event<T>(
            &mut self,
            what: &str,
            events: &mut broadcast::Receiver<ServerEvent>,
            mut pick: impl FnMut(&ServerEvent) -> Option<T>,
        ) -> Result<T, TestError> {
            self.step_until(what, |_app| {
                while let Ok(event) = events.try_recv() {
                    if let Some(value) = pick(&event) {
                        return Some(value);
                    }
                }
                None
            })
        }

        /// The recorded events and capability maps.
        fn recorded(&self) -> &Recorded {
            self.app.world().resource::<Recorded>()
        }

        /// Steps frames until a login notice arrives on the broadcast.
        fn poll_login_notice(&mut self) -> Result<sl_fake_grid::LoginNotice, TestError> {
            let mut notice = None;
            let deadline = Instant::now().checked_add(WAIT).ok_or("clock overflow")?;
            while notice.is_none() {
                self.app.update();
                if let Ok(received) = self.logins.try_recv() {
                    notice = Some(received);
                } else if Instant::now() >= deadline {
                    return Err("timed out waiting for the grid's login notice".into());
                } else {
                    std::thread::sleep(FRAME_PAUSE);
                }
            }
            notice.ok_or_else(|| "no login notice".into())
        }

        /// Writes a command into the plugin's `SlCommand` message stream.
        fn command(&mut self, command: Command) {
            self.app.world_mut().write_message(SlCommand(command));
        }
    }

    /// Login, circuit, handshake, world state, chat out, object in, CAPS in,
    /// logout — the whole pipeline, in order.
    #[test]
    fn bevy_client_logs_in_and_round_trips_world_state() -> Result<(), TestError> {
        let mut harness = Harness::start("sl-fake-grid-bevy-smoke")?;
        let agent = harness.poll_login_notice().and_then(|notice| {
            harness
                .runtime
                .block_on(harness.grid.agent(&notice))
                .ok_or_else(|| "no live session for the login notice".into())
        })?;
        let mut server_events = agent.events();

        // 1. Login + circuit: the handshake lands, the identity is stamped.
        harness.wait_for_event("CircuitEstablished", |event| {
            matches!(event, SlSessionEvent::CircuitEstablished { .. }).then_some(())
        })?;
        harness.wait_for_event("RegionHandshakeComplete", |event| {
            matches!(event, SlSessionEvent::RegionHandshakeComplete).then_some(())
        })?;
        let identity = harness.app.world().resource::<SlIdentity>();
        assert_eq!(identity.agent_id, Some(agent.agent_id()));
        assert_eq!(
            identity.map_server_url.as_ref(),
            Some(&harness.grid.login_uri())
        );

        // 2. World state through `maintain_world`: one current region carrying
        //    the stock identity, a complete overlay, the stock parcel as the
        //    agent's parcel and as the region's child entity.
        let expected_handle = sl_proto::RegionHandle::from_grid(
            RegionConfig::default().grid_x,
            RegionConfig::default().grid_y,
        );
        harness
            .step_until("the current region's identity", |app| {
                let mut query = app
                    .world_mut()
                    .query_filtered::<(&SlRegion, &SlRegionIdentity), With<SlCurrentRegion>>();
                let regions: Vec<(sl_proto::RegionHandle, Option<String>)> = query
                    .iter(app.world())
                    .map(|(region, identity)| {
                        (
                            region.handle,
                            identity.0.sim_name.as_ref().map(ToString::to_string),
                        )
                    })
                    .collect();
                (regions.len() == 1).then_some(regions)
            })
            .map(|regions| {
                assert_eq!(
                    regions,
                    vec![(expected_handle, Some(RegionConfig::default().name))]
                );
            })?;
        harness.step_until("a complete parcel overlay", |app| {
            app.world()
                .resource::<SlParcelOverlay>()
                .is_complete()
                .then_some(())
        })?;
        let agent_parcel = harness.step_until("the agent's parcel", |app| {
            app.world().resource::<SlAgentParcel>().current.clone()
        })?;
        assert_eq!(agent_parcel.name, STOCK_PARCEL_NAME);
        assert_eq!(agent_parcel.local_id, STOCK_PARCEL_LOCAL_ID);
        assert!(agent_parcel.allow_fly());
        assert!(harness.app.world().resource::<SlAgentParcel>().can_fly);
        harness.step_until("the parcel entity under the region", |app| {
            let mut query = app.world_mut().query::<&SlParcel>();
            query
                .iter(app.world())
                .any(|parcel| parcel.0.name == STOCK_PARCEL_NAME)
                .then_some(())
        })?;

        // 3. Stock content: the greeting, the stock prim, the features fetched
        //    over the Bevy CAPS path, and a seed grant with an event queue.
        harness.wait_for_event("the arrival greeting", |event| match event {
            SlSessionEvent::ChatReceived(chat)
                if chat.message.contains("Welcome to the fake grid") =>
            {
                Some(())
            }
            _ => None,
        })?;
        let stock_prim =
            harness.wait_for_event("the stock prim's ObjectAdded", |event| match event {
                SlSessionEvent::ObjectAdded(object) | SlSessionEvent::ObjectUpdated(object)
                    if object.full_id == stock_scripted_object() =>
                {
                    Some((object.local_id, object.motion.position.clone()))
                }
                _ => None,
            })?;
        assert_eq!(
            stock_prim,
            (
                STOCK_SCRIPTED_OBJECT_LOCAL_ID,
                STOCK_SCRIPTED_OBJECT_POSITION
            )
        );
        // The region's ground: the whole spiral of land patches through the
        // real UDP path, each stamped with the region handle.
        harness.step_until("the region's 256 land patches", |app| {
            let patches: Vec<(u32, u32)> = app
                .world()
                .resource::<Recorded>()
                .events
                .iter()
                .filter_map(|event| match event {
                    SlSessionEvent::TerrainPatch(patch)
                        if patch.layer == sl_proto::TerrainLayerType::Land
                            && patch.region_handle == expected_handle =>
                    {
                        Some((patch.patch_x, patch.patch_y))
                    }
                    _ => None,
                })
                .collect();
            (patches.len() >= 256).then_some(patches)
        })?;

        let login_uri = harness.grid.login_uri();
        harness
            .wait_for_event("SimulatorFeatures over CAPS", |event| match event {
                SlSessionEvent::SimulatorFeatures(features) => Some(
                    features
                        .open_sim_extras
                        .as_ref()
                        .and_then(|extras| extras.map_server_url.clone()),
                ),
                _ => None,
            })
            .map(|map_server| {
                assert_eq!(map_server.as_ref(), Some(&login_uri));
            })?;
        harness.step_until("the capability map", |app| {
            app.world()
                .resource::<Recorded>()
                .capabilities
                .iter()
                .any(|caps| caps.contains_key("EventQueueGet"))
                .then_some(())
        })?;

        // 4. Chat out: a Bevy `SlCommand` reaches the grid as a ServerEvent.
        harness.command(Command::Chat {
            message: "hello from bevy".to_owned(),
            chat_type: ChatType::Normal,
            channel: ChatChannel(0),
        });
        harness.wait_for_server_event(
            "the chat line on the grid",
            &mut server_events,
            |event| match event {
                ServerEvent::Chat { message, .. } if message == "hello from bevy" => Some(()),
                _ => None,
            },
        )?;

        // 5. Object in: a prim pushed grid-side arrives as ObjectAdded with its
        //    key, id, position and scale; a KillObject removes it again.
        let key = ObjectKey::from(uuid::Uuid::from_u128(0x1234_5678));
        let local_id = RegionLocalObjectId(0x1234);
        let position = sl_proto::Vector {
            x: 10.0,
            y: 20.0,
            z: 30.0,
        };
        let scale = sl_proto::Vector {
            x: 1.0,
            y: 2.0,
            z: 3.0,
        };
        let prim = sl_fake_grid::box_prim(
            local_id,
            key,
            agent.agent_id(),
            position.clone(),
            scale.clone(),
        );
        harness.runtime.block_on(agent.with_sim(|sim| {
            sim.send_object_update(std::slice::from_ref(&prim), 0xFFFF, Instant::now())
        }))?;
        let added =
            harness.wait_for_event("the pushed prim's ObjectAdded", |event| match event {
                SlSessionEvent::ObjectAdded(object) if object.full_id == key => Some((
                    object.local_id,
                    object.motion.position.clone(),
                    object.scale.clone(),
                )),
                _ => None,
            })?;
        assert_eq!(added, (local_id, position, scale));
        harness
            .runtime
            .block_on(agent.with_sim(|sim| sim.send_kill_object(&[local_id], Instant::now())))?;
        harness.wait_for_event("ObjectRemoved for the pushed prim", |event| match event {
            SlSessionEvent::ObjectRemoved {
                local_id: removed, ..
            } if removed.id() == local_id => Some(()),
            _ => None,
        })?;

        // 6. A CAPS event through the real long-poll: a renamed parcel record
        //    updates the agent parcel the network thread mirrors.
        let renamed = {
            let mut record = agent_parcel.clone();
            record.name = "Renamed by the grid".to_owned();
            record
        };
        harness
            .runtime
            .block_on(agent.with_sim(|sim| sim.enqueue_parcel_properties(&renamed)));
        harness.step_until("the renamed agent parcel", |app| {
            app.world()
                .resource::<SlAgentParcel>()
                .current
                .as_ref()
                .is_some_and(|parcel| parcel.name == "Renamed by the grid")
                .then_some(())
        })?;

        // 7. Logout: a clean LoggedOut, and the grid sees the request.
        harness.command(Command::Logout);
        harness.wait_for_server_event("the logout on the grid", &mut server_events, |event| {
            matches!(event, ServerEvent::LoggedOut).then_some(())
        })?;
        harness.wait_for_event("LoggedOut", |event| {
            matches!(event, SlSessionEvent::LoggedOut).then_some(())
        })?;
        assert!(
            !harness
                .recorded()
                .events
                .iter()
                .any(|event| matches!(event, SlSessionEvent::Disconnected(_))),
            "a clean logout never reports a disconnect"
        );
        Ok(())
    }

    /// Two apps against two grids in one process: ephemeral ports and
    /// per-app network threads keep them independent.
    #[test]
    fn two_bevy_clients_run_against_two_grids() -> Result<(), TestError> {
        let mut first = Harness::start("sl-fake-grid-bevy-smoke-a")?;
        let mut second = Harness::start("sl-fake-grid-bevy-smoke-b")?;
        assert_ne!(first.grid.http_port(), second.grid.http_port());
        for harness in [&mut first, &mut second] {
            harness.poll_login_notice()?;
            harness.wait_for_event("RegionHandshakeComplete", |event| {
                matches!(event, SlSessionEvent::RegionHandshakeComplete).then_some(())
            })?;
        }
        let first_id = first.app.world().resource::<SlIdentity>().agent_id;
        let second_id = second.app.world().resource::<SlIdentity>().agent_id;
        assert!(first_id.is_some() && second_id.is_some());
        // Two grids mint two different agent ids for the same account name.
        assert_ne!(first_id, second_id);
        for harness in [&mut first, &mut second] {
            harness.command(Command::Logout);
            harness.wait_for_event("LoggedOut", |event| {
                matches!(event, SlSessionEvent::LoggedOut).then_some(())
            })?;
        }
        Ok(())
    }
}
