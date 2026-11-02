//! Scripts run on the fake grid: the catalogue's scripted box runs its script
//! from the start, answers a line said on its channel, stops and starts with
//! the viewer's Running checkbox, restarts on a reset, survives a script that
//! throws, and takes its object out of the region with `llDie`; a script
//! keeps what it holds through a region restart and through a take and a rez;
//! an experience once accepted is granted to its every script, refused where
//! its standing stops it, and a question refused or left unanswered is
//! answered as Second Life answers it; and a region's `[OSSL]` settings gate
//! a function to gods — each step driven by a real client, the region ticked
//! one step at a time.

#[cfg(test)]
mod test {
    use std::time::Duration;

    use pretty_assertions::assert_eq;
    use sl_client_tokio::{Client, Command, Event, LoginParams, LoginRequest, StartLocation};
    use sl_fake_grid::fixtures::catalogue::{ECHO_SCRIPT_CHANNEL, echo_script_item, entry};
    use sl_fake_grid::{
        AccountConfig, FakeGrid, FakeGridBuilder, HeartbeatConfig, ImitatedGrid, RegionConfig,
    };
    use sl_proto::{ChatType, ServerEvent};
    use sl_types::chat::ChatChannel;
    use tokio::sync::{broadcast, mpsc};

    type TestError = Box<dyn core::error::Error>;

    /// How long any single wait in these tests may take.
    const WAIT: Duration = Duration::from_secs(20);

    /// How long to listen for something that must not arrive.
    const QUIET: Duration = Duration::from_millis(500);

    /// The stock region's name.
    const REGION: &str = "Fake Region";

    /// What the client is told that these tests watch for.
    #[derive(Debug)]
    enum Seen {
        /// A line of chat.
        Chat(ChatType, String),
        /// A script's run state.
        Running(bool),
        /// An object left the client's view.
        Removed(sl_proto::RegionLocalObjectId),
        /// An object came into it.
        Added(sl_proto::RegionLocalObjectId),
        /// An object item was filed in the agent's inventory.
        Filed(Box<sl_client_tokio::InventoryItem>),
        /// A script asked for permissions.
        Question(Box<sl_proto::ScriptPermissionRequest>),
    }

    /// A logged-in client in a stepped catalogue region.
    struct Scene {
        /// The grid.
        grid: FakeGrid,
        /// The client's commands.
        commands: mpsc::Sender<Command>,
        /// What the client was told, filtered.
        seen: mpsc::UnboundedReceiver<Seen>,
        /// What the session received from the client.
        server: broadcast::Receiver<ServerEvent>,
        /// The client's run loop and the forwarder.
        tasks: Vec<tokio::task::JoinHandle<()>>,
        /// The agent's session on the grid.
        agent: sl_fake_grid::FakeAgent,
        /// The client's circuit, which its object ids are scoped to.
        circuit: sl_client_tokio::CircuitId,
    }

    /// A grid imitating `imitated`, its one region the catalogue under a
    /// stepped heartbeat, and a client logged into it and arrived.
    async fn scene(imitated: ImitatedGrid) -> Result<Scene, TestError> {
        scene_of(imitated, sl_fake_grid::catalogue()).await
    }

    /// A grid imitating `imitated`, its one region `fixture` under a stepped
    /// heartbeat, and a client logged into it and arrived.
    async fn scene_of(
        imitated: ImitatedGrid,
        fixture: sl_fake_grid::RegionFixture,
    ) -> Result<Scene, TestError> {
        scene_stepped(imitated, fixture, Duration::from_millis(100)).await
    }

    /// A grid imitating `imitated`, its one region `fixture` under a
    /// heartbeat stepped by hand in steps of `step`, and a client logged into
    /// it and arrived.
    async fn scene_stepped(
        imitated: ImitatedGrid,
        fixture: sl_fake_grid::RegionFixture,
        step: Duration,
    ) -> Result<Scene, TestError> {
        scene_with(
            imitated,
            fixture,
            RegionConfig {
                heartbeat: HeartbeatConfig::stepped(step),
                ..RegionConfig::default()
            },
            AccountConfig::new("Test", "User", "password"),
        )
        .await
    }

    /// A grid imitating `imitated`, its one region `fixture` configured as
    /// `config` (whose heartbeat must be stepped), and a client logged into it
    /// as `account` and arrived.
    async fn scene_with(
        imitated: ImitatedGrid,
        fixture: sl_fake_grid::RegionFixture,
        config: RegionConfig,
        account: AccountConfig,
    ) -> Result<Scene, TestError> {
        let region = fixture.into_region(config);
        let grid = FakeGridBuilder::new()
            .imitates(imitated)
            .account(account)
            .region(region)
            .start()
            .await?;
        let mut logins = grid.logins();
        let client = Client::connect(LoginParams {
            login_uri: grid.login_uri(),
            request: LoginRequest::new(
                "Test",
                "User",
                "password",
                StartLocation::Last,
                "sl-fake-grid-scripts",
                "0.0",
            ),
        })
        .await?;
        let circuit = client.root_circuit_id().ok_or("no root circuit")?;
        let notice = tokio::time::timeout(WAIT, logins.recv()).await??;
        let agent = grid.agent(&notice).await.ok_or("no live session")?;
        let mut server = agent.events();
        let (event_tx, mut event_rx) = mpsc::channel::<Event>(256);
        let (command_tx, command_rx) = mpsc::channel::<Command>(8);
        let (diag_tx, _diag_rx) = mpsc::channel(16);
        let run = tokio::spawn(async move {
            let _ended = client.run(event_tx, diag_tx, command_rx).await;
        });
        // Forward only what the tests watch: the client's run loop stops at
        // the 256th unread event, which the catalogue's arrival outnumbers.
        let (seen_tx, seen) = mpsc::unbounded_channel();
        let forward = tokio::spawn(async move {
            while let Some(event) = event_rx.recv().await {
                let seen = match event {
                    Event::ChatReceived(chat) => Seen::Chat(chat.chat_type, chat.message),
                    Event::ScriptRunning { running, .. } => Seen::Running(running),
                    Event::ObjectRemoved { local_id, .. } => Seen::Removed(local_id.id()),
                    Event::ObjectAdded(object) => Seen::Added(object.local_id),
                    Event::ScriptPermissionRequest(question) => Seen::Question(question),
                    Event::InventoryItemCreated { item, .. }
                        if i32::from(item.item_type) == sl_proto::AssetType::Object.to_code() =>
                    {
                        Seen::Filed(Box::new(item))
                    }
                    Event::InventoryBulkUpdate { items, .. } => {
                        let Some(item) = items.into_iter().find(|item| {
                            i32::from(item.item_type) == sl_proto::AssetType::Object.to_code()
                        }) else {
                            continue;
                        };
                        Seen::Filed(Box::new(item))
                    }
                    _other => continue,
                };
                if seen_tx.send(seen).is_err() {
                    break;
                }
            }
        });
        tokio::time::timeout(WAIT, async {
            loop {
                if matches!(server.recv().await?, ServerEvent::AgentArrived) {
                    return Ok::<(), broadcast::error::RecvError>(());
                }
            }
        })
        .await??;
        Ok(Scene {
            grid,
            commands: command_tx,
            seen,
            server,
            tasks: vec![run, forward],
            agent,
            circuit,
        })
    }

    impl Scene {
        /// Sends `command` and waits until the session has taken the event
        /// `taken` recognises: what the region does next tick then follows it.
        async fn send(
            &mut self,
            command: Command,
            taken: impl Fn(&ServerEvent) -> bool + Send + Sync,
        ) -> Result<(), TestError> {
            let sent = format!("{command:?}");
            self.commands.send(command).await?;
            tokio::time::timeout(WAIT, async {
                loop {
                    if taken(&self.server.recv().await?) {
                        return Ok::<(), broadcast::error::RecvError>(());
                    }
                }
            })
            .await
            .map_err(|_elapsed| format!("the session never took {sent}"))??;
            Ok(())
        }

        /// Shouts `message` on the scripted box's channel; the row stands some
        /// 80 m from the arrival point, inside a shout's reach.
        async fn shout(&mut self, message: &str) -> Result<(), TestError> {
            self.send(
                Command::Chat {
                    message: message.to_owned(),
                    chat_type: ChatType::Shout,
                    channel: ChatChannel(ECHO_SCRIPT_CHANNEL),
                },
                |event| matches!(event, ServerEvent::Chat { message: said, .. } if said == message),
            )
            .await
        }

        /// Runs `ticks` ticks of the region.
        async fn tick(&self, ticks: u32) -> Result<(), TestError> {
            for _ in 0..ticks {
                let _tick = self.grid.advance_one_tick(REGION).await?;
            }
            Ok(())
        }

        /// The next thing seen that `wanted` picks, skipping the rest.
        async fn next<T: Send>(
            &mut self,
            what: &str,
            wanted: impl Fn(Seen) -> Option<T> + Send + Sync,
        ) -> Result<T, TestError> {
            let found = tokio::time::timeout(WAIT, async {
                while let Some(seen) = self.seen.recv().await {
                    if let Some(found) = wanted(seen) {
                        return Some(found);
                    }
                }
                None
            })
            .await
            .map_err(|_elapsed| format!("the client was never told {what}"))?;
            Ok(found.ok_or("the client stopped")?)
        }

        /// The next echo the client is told, or `None` when none comes.
        async fn echo(&mut self) -> Option<String> {
            tokio::time::timeout(QUIET, async {
                while let Some(seen) = self.seen.recv().await {
                    if let Seen::Chat(_, message) = seen
                        && message.starts_with("Echo: ")
                    {
                        return Some(message);
                    }
                }
                None
            })
            .await
            .ok()
            .flatten()
        }

        /// Whether the scripted box's script reads as running, as the
        /// viewer's Contents floater asks it.
        async fn running(&mut self) -> Result<bool, TestError> {
            let (object_id, item_id) = script()?;
            self.commands
                .send(Command::RequestScriptRunning { object_id, item_id })
                .await?;
            self.next("a script's run state", |seen| match seen {
                Seen::Running(running) => Some(running),
                _other => None,
            })
            .await
        }

        /// The next line the client is told that starts with `prefix`.
        async fn line(&mut self, prefix: &str) -> Result<String, TestError> {
            self.next(prefix, |seen| match seen {
                Seen::Chat(_, message) if message.starts_with(prefix) => Some(message),
                _other => None,
            })
            .await
        }

        /// Shouts on channel 7, ticks, and returns the box's answer.
        async fn count(&mut self, message: &str) -> Result<String, TestError> {
            self.shout(message).await?;
            self.tick(2).await?;
            self.line("heard ").await
        }

        /// Says `name` on channel 7 and waits for the question it raises.
        async fn ask(
            &mut self,
            name: &str,
        ) -> Result<sl_proto::ScriptPermissionRequest, TestError> {
            self.shout(name).await?;
            self.tick(2).await?;
            let question = self
                .next("an experience's question", |seen| match seen {
                    Seen::Question(question) => Some(question),
                    _other => None,
                })
                .await?;
            Ok(*question)
        }

        /// Answers `question` with `permissions`, and waits for the session
        /// to take the answer.
        async fn answer(
            &mut self,
            question: &sl_proto::ScriptPermissionRequest,
            permissions: sl_types::lsl::ScriptPermissions,
        ) -> Result<(), TestError> {
            self.send(
                Command::AnswerScriptPermissions {
                    task_id: question.task_id,
                    item_id: question.item_id,
                    permissions,
                    experience_id: question.experience_id,
                },
                |event| matches!(event, ServerEvent::ScriptPermissionAnswer { .. }),
            )
            .await
        }

        /// Ends the run.
        fn stop(self) {
            for task in self.tasks {
                task.abort();
            }
            self.grid.shutdown();
        }
    }

    /// The scripted box and its script item.
    fn script() -> Result<(sl_types::key::ObjectKey, sl_types::key::InventoryKey), TestError> {
        let entry = entry("scripted-box").ok_or("the catalogue has no scripted box")?;
        Ok((entry.full_id, echo_script_item().item_id))
    }

    /// The whole life of the catalogue's scripted box on a grid imitating
    /// `imitated`.
    async fn a_scripted_box_lives(imitated: ImitatedGrid) -> Result<(), TestError> {
        let mut scene = scene(imitated).await?;
        let (object_id, item_id) = script()?;

        // It runs from the start: its `state_entry` opened the listen.
        scene.tick(2).await?;
        assert!(
            scene.running().await?,
            "the fixture's script is not running"
        );
        scene.shout("hello").await?;
        scene.tick(2).await?;
        assert_eq!(scene.echo().await.as_deref(), Some("Echo: hello"));

        // The Running checkbox stops it, and a stopped script hears nothing.
        scene
            .send(
                Command::SetScriptRunning {
                    object_id,
                    item_id,
                    running: false,
                },
                |event| matches!(event, ServerEvent::SetScriptRunning { running: false, .. }),
            )
            .await?;
        assert!(!scene.running().await?, "the script still reads running");
        scene.shout("unheard").await?;
        scene.tick(2).await?;
        assert_eq!(scene.echo().await, None, "a stopped script answered");
        scene
            .send(
                Command::SetScriptRunning {
                    object_id,
                    item_id,
                    running: true,
                },
                |event| matches!(event, ServerEvent::SetScriptRunning { running: true, .. }),
            )
            .await?;
        scene.shout("again").await?;
        scene.tick(2).await?;
        assert_eq!(scene.echo().await.as_deref(), Some("Echo: again"));

        // A script that throws leaves the region ticking; how it stands
        // after is its engine's: Second Life stops it, YEngine leaves it
        // reading as running and answering nothing.
        scene.shout("throw").await?;
        scene.tick(2).await?;
        if imitated == ImitatedGrid::OpenSim {
            // YEngine's error line reaches every avatar in the region;
            // Second Life's carries a say's 20 m, short of the arrival point.
            let error = scene
                .next("the run-time error", |seen| match seen {
                    Seen::Chat(ChatType::DebugChannel, message) => Some(message),
                    _other => None,
                })
                .await?;
            assert!(!error.is_empty());
        }
        assert_eq!(
            scene.running().await?,
            imitated == ImitatedGrid::OpenSim,
            "the faulted script's run state"
        );
        scene.shout("held").await?;
        scene.tick(2).await?;
        assert_eq!(scene.echo().await, None, "a faulted script answered");

        // A reset releases it, and it runs from the top: its listen again.
        scene
            .send(Command::ResetScript { object_id, item_id }, |event| {
                matches!(event, ServerEvent::ResetScript { .. })
            })
            .await?;
        if imitated == ImitatedGrid::SecondLife {
            scene
                .send(
                    Command::SetScriptRunning {
                        object_id,
                        item_id,
                        running: true,
                    },
                    |event| matches!(event, ServerEvent::SetScriptRunning { running: true, .. }),
                )
                .await?;
        }
        scene.tick(2).await?;
        scene.shout("reset").await?;
        scene.tick(2).await?;
        assert_eq!(scene.echo().await.as_deref(), Some("Echo: reset"));

        // `llDie` takes the object out of the region, and the viewer is told.
        scene.shout("die").await?;
        scene.tick(2).await?;
        let gone = entry("scripted-box")
            .ok_or("the catalogue has no scripted box")?
            .local_id;
        let removed = scene
            .next("the scripted box's removal", |seen| match seen {
                Seen::Removed(local_id) if local_id == gone => Some(local_id),
                _other => None,
            })
            .await?;
        assert_eq!(removed, gone);
        scene.tick(1).await?;
        scene.stop();
        Ok(())
    }

    /// On a grid imitating Second Life.
    #[tokio::test]
    async fn a_scripted_box_lives_on_second_life() -> Result<(), TestError> {
        a_scripted_box_lives(ImitatedGrid::SecondLife).await
    }

    /// On a grid imitating OpenSim, whose engine is YEngine.
    #[tokio::test]
    async fn a_scripted_box_lives_on_opensim() -> Result<(), TestError> {
        a_scripted_box_lives(ImitatedGrid::OpenSim).await
    }

    /// The counting box's script: it counts the lines said to it on channel
    /// 7 and answers each with the count, and shouts the count when its object
    /// is rezzed and when its region starts again.
    const COUNTER_SOURCE: &str = "integer heard;

default
{
    state_entry()
    {
        llListen(7, \"\", NULL_KEY, \"\");
    }

    on_rez(integer start)
    {
        llShout(0, \"rezzed after \" + (string)heard);
    }

    changed(integer change)
    {
        if (change & CHANGED_REGION_START)
        {
            llShout(0, \"restarted after \" + (string)heard);
        }
    }

    listen(integer channel, string name, key id, string message)
    {
        heard += 1;
        llRegionSayTo(id, 0, \"heard \" + (string)heard);
    }
}
";

    /// The counting box's region-local id.
    const COUNTER: sl_proto::RegionLocalObjectId = sl_proto::RegionLocalObjectId(0x500);

    /// Who owns every box [`region_of`] builds.
    fn box_owner() -> sl_types::key::AgentKey {
        sl_types::key::AgentKey::from(uuid::Uuid::from_u128(0xB0C0))
    }

    /// One scripted box of a test region: its region-local id, its script's
    /// source and the experience the script was compiled under.
    struct ScriptedBox {
        /// Its region-local id; its keys are minted from it.
        local_id: sl_proto::RegionLocalObjectId,
        /// Its script's source, running.
        source: String,
        /// The script's experience.
        experience: Option<sl_types::key::ExperienceKey>,
    }

    /// A region holding `boxes` in a row beside the arrival point, each with
    /// its one script running.
    fn region_of(boxes: &[ScriptedBox]) -> Result<sl_fake_grid::RegionFixture, TestError> {
        let owner = box_owner();
        let mut fixture = sl_fake_grid::RegionFixture::new();
        for (index, scripted) in (0_u16..).zip(boxes) {
            let id = u128::from(scripted.local_id.0) << 8;
            let key = sl_types::key::ObjectKey::from(uuid::Uuid::from_u128(id | 0x5));
            let asset = sl_proto::AssetKey::from(uuid::Uuid::from_u128(id | 0x6));
            fixture.world.objects.push(sl_fake_grid::world::box_prim(
                scripted.local_id,
                key,
                owner,
                sl_types::lsl::Vector {
                    x: 132.0 + 2.0 * f32::from(index),
                    y: 128.0,
                    z: 25.5,
                },
                sl_types::lsl::Vector {
                    x: 1.0,
                    y: 1.0,
                    z: 1.0,
                },
            ));
            let item = sl_proto::TaskInventoryItem {
                item_id: sl_types::key::InventoryKey::from(uuid::Uuid::from_u128(id | 0x7)),
                parent_task: key,
                asset_id: Some(asset),
                name: "Script".to_owned(),
                ..sl_fake_grid::scenario::stock_script_item()
            };
            let mut contents = sl_fake_grid::world::TaskInventory::stated(1, vec![item.clone()]);
            let _running = contents.running.insert(item.item_id);
            if let Some(experience) = scripted.experience {
                let _previous = contents.experiences.insert(item.item_id, experience);
            }
            fixture
                .world
                .add_task_inventory(scripted.local_id, contents)
                .map_err(|_contents| "the box is not in the region")?;
            let _previous = fixture
                .assets
                .insert(asset, scripted.source.as_bytes().to_vec());
        }
        Ok(fixture)
    }

    /// A region holding one box beside the arrival point, its script
    /// [`COUNTER_SOURCE`] running.
    fn counting_region() -> Result<sl_fake_grid::RegionFixture, TestError> {
        region_of(&[ScriptedBox {
            local_id: COUNTER,
            source: COUNTER_SOURCE.to_owned(),
            experience: None,
        }])
    }

    /// A script keeps its globals and its listen through a region restart,
    /// and through a take into inventory and a rez back out, told of each by
    /// `changed(CHANGED_REGION_START)` and by `on_rez`.
    async fn a_script_keeps_its_state(imitated: ImitatedGrid) -> Result<(), TestError> {
        let region = counting_region()?;
        let mut scene = scene_of(imitated, region).await?;
        scene.tick(2).await?;
        assert_eq!(scene.count("one").await?, "heard 1");
        assert_eq!(scene.count("two").await?, "heard 2");

        scene.grid.restart_region(REGION, Duration::from_secs(30))?;
        scene.tick(2).await?;
        assert_eq!(scene.line("restarted after ").await?, "restarted after 2");
        assert_eq!(scene.count("three").await?, "heard 3");

        let objects = scene
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
            .ok_or("the account has no Objects folder")?;
        scene
            .commands
            .send(Command::DerezObjects {
                local_ids: vec![sl_client_tokio::ScopedObjectId::new(scene.circuit, COUNTER)],
                destination: sl_client_tokio::DeRezDestination::TakeIntoAgentInventory(objects),
                transaction_id: sl_client_tokio::TransactionId::from(uuid::Uuid::from_u128(0x7A)),
                group_id: None,
            })
            .await?;
        let item = scene
            .next("the taken item", |seen| match seen {
                Seen::Filed(item) => Some(item),
                _other => None,
            })
            .await?;
        // Gone from the region, the script with it: nothing answers.
        scene.tick(2).await?;
        scene.shout("away").await?;
        scene.tick(2).await?;
        assert_eq!(scene.echo().await, None);

        let landing = sl_types::lsl::Vector {
            x: 134.0,
            y: 128.0,
            z: 25.5,
        };
        scene
            .commands
            .send(Command::RezObjectFromInventory {
                params: Box::new(sl_client_tokio::RezObjectParams {
                    group_id: None,
                    from_task_id: None,
                    bypass_raycast: true,
                    ray_start: landing.clone(),
                    ray_end: landing,
                    ray_target_id: None,
                    ray_end_is_intersection: false,
                    rez_selected: false,
                    remove_item: false,
                    item_flags: item.flags,
                    group_mask: item.permissions.group.bits(),
                    everyone_mask: item.permissions.everyone.bits(),
                    next_owner_mask: item.permissions.next_owner.bits(),
                    item: sl_client_tokio::RestoreItem {
                        item_id: item.item_id,
                        folder_id: item.folder_id,
                        creator_id: item.creator_id,
                        owner: item.owner,
                        group: item.group,
                        permissions: item.permissions,
                        transaction_id: uuid::Uuid::from_u128(0x7B),
                        asset_type: item.item_type,
                        inv_type: item.inv_type,
                        flags: item.flags,
                        sale_type: sl_client_tokio::SaleType::from_code(item.sale_type),
                        sale_price: item.sale_price.clone(),
                        name: item.name.clone(),
                        description: item.description.clone(),
                        creation_date: item.creation_date,
                        crc: 0,
                    },
                }),
            })
            .await?;
        let _rezzed = scene
            .next("the rezzed box", |seen| match seen {
                Seen::Added(local_id) => Some(local_id),
                _other => None,
            })
            .await?;
        scene.tick(2).await?;
        assert_eq!(scene.line("rezzed after ").await?, "rezzed after 3");
        assert_eq!(scene.count("four").await?, "heard 4");
        scene.stop();
        Ok(())
    }

    /// On a grid imitating Second Life.
    #[tokio::test]
    async fn a_script_keeps_its_state_on_second_life() -> Result<(), TestError> {
        a_script_keeps_its_state(ImitatedGrid::SecondLife).await
    }

    /// On a grid imitating OpenSim.
    #[tokio::test]
    async fn a_script_keeps_its_state_on_opensim() -> Result<(), TestError> {
        a_script_keeps_its_state(ImitatedGrid::OpenSim).await
    }

    /// A script of an experience that asks the avatar who says `name` on
    /// channel 7 for the experience's permissions, and tells it the answer.
    fn asking_source(name: &str) -> String {
        format!(
            "default
{{
    state_entry()
    {{
        llListen(7, \"\", NULL_KEY, \"{name}\");
    }}

    listen(integer channel, string speaker, key id, string message)
    {{
        llRequestExperiencePermissions(id, \"\");
    }}

    experience_permissions(key agent)
    {{
        llRegionSayTo(agent, 0, \"{name} granted\");
    }}

    experience_permissions_denied(key agent, integer reason)
    {{
        llRegionSayTo(agent, 0, \"{name} denied \" + (string)reason);
    }}
}}
"
        )
    }

    /// A box at `local_id` whose script, of `experience`, answers to `name`.
    fn asking_box(
        local_id: u32,
        name: &str,
        experience: sl_types::key::ExperienceKey,
    ) -> ScriptedBox {
        ScriptedBox {
            local_id: sl_proto::RegionLocalObjectId(local_id),
            source: asking_source(name),
            experience: Some(experience),
        }
    }

    /// The experience no fixture list mentions: the agent has answered
    /// nothing of it, and it is grid-wide, so it may run anywhere.
    fn stranger() -> sl_types::key::ExperienceKey {
        sl_fake_grid::experiences::stranger_experience()
    }

    /// An experience script asks; accepting it allows the experience for the
    /// agent, so a second script of it is granted without a question; and a
    /// suspended experience, or one the estate blocks, is refused before
    /// anybody is asked.
    #[tokio::test]
    async fn an_experience_once_allowed_is_allowed_for_its_every_script() -> Result<(), TestError> {
        let workshop = sl_fake_grid::experiences::catalogue()
            .into_iter()
            .find(|record| record.name == "Fake Grid Workshop")
            .ok_or("the catalogue has no workshop")?
            .public_id;
        let blocked = sl_fake_grid::experiences::ExperienceLand::default()
            .blocked
            .first()
            .copied()
            .ok_or("the estate blocks no experience")?;
        let region = region_of(&[
            asking_box(0x600, "A", stranger()),
            asking_box(0x601, "B", stranger()),
            asking_box(0x602, "C", workshop),
            asking_box(0x603, "D", blocked),
        ])?;
        let mut scene = scene_of(ImitatedGrid::SecondLife, region).await?;
        scene.tick(2).await?;

        let question = scene.ask("A").await?;
        assert_eq!(question.experience_id, Some(stranger()));
        scene.answer(&question, question.permissions).await?;
        scene.tick(2).await?;
        assert_eq!(scene.line("A ").await?, "A granted");
        let allowed = scene
            .agent
            .with_sim(|sim| sim.experiences().agent_permissions().0)
            .await;
        assert!(
            allowed.contains(&stranger()),
            "accepting did not allow the experience: {allowed:?}"
        );

        scene.shout("B").await?;
        scene.tick(2).await?;
        assert_eq!(scene.line("B ").await?, "B granted");

        scene.shout("C").await?;
        scene.tick(2).await?;
        assert_eq!(scene.line("C ").await?, "C denied 9");

        scene.shout("D").await?;
        scene.tick(2).await?;
        assert_eq!(scene.line("D ").await?, "D denied 17");
        scene.stop();
        Ok(())
    }

    /// A refusal is the script's `XP_ERROR_NOT_PERMITTED` and records
    /// nothing, so the next request asks again; a question nobody answers
    /// times out after five minutes as `XP_ERROR_REQUEST_PERM_TIMEOUT`.
    #[tokio::test]
    async fn an_experience_question_refused_or_unanswered() -> Result<(), TestError> {
        let region = region_of(&[asking_box(0x600, "A", stranger())])?;
        let step = Duration::from_secs(10);
        let mut scene = scene_stepped(ImitatedGrid::SecondLife, region, step).await?;
        scene.tick(2).await?;

        let question = scene.ask("A").await?;
        scene
            .answer(&question, sl_types::lsl::ScriptPermissions(0))
            .await?;
        scene.tick(2).await?;
        assert_eq!(scene.line("A ").await?, "A denied 4");

        let _again = scene.ask("A").await?;
        // Five minutes of ten-second ticks, and the one the answer runs in.
        scene.tick(30).await?;
        assert_eq!(scene.line("A ").await?, "A denied 18");
        let (allowed, blocked) = scene
            .agent
            .with_sim(|sim| sim.experiences().agent_permissions())
            .await;
        assert!(!allowed.contains(&stranger()) && !blocked.contains(&stranger()));
        scene.stop();
        Ok(())
    }

    /// A script asking the simulator's version on channel 7 (a function
    /// OpenSim gates at threat level `High`), and saying it.
    const VERSION_SOURCE: &str = "default
{
    state_entry()
    {
        llListen(7, \"\", NULL_KEY, \"name\");
    }

    listen(integer channel, string speaker, key id, string message)
    {
        llShout(0, \"version: \" + osGetSimulatorVersion());
    }
}
";

    /// Whether the script of a box owned by an account of `god_level`, in a
    /// region whose `[OSSL]` allows `osGetSimulatorVersion` to gods alone, is
    /// let call it: the version, or YEngine's refusal on the debug channel.
    async fn version_for(god_level: u8) -> Result<String, TestError> {
        let region = region_of(&[ScriptedBox {
            local_id: sl_proto::RegionLocalObjectId(0x700),
            source: VERSION_SOURCE.to_owned(),
            experience: None,
        }])?;
        let ossl = sl_lsl_runtime::vm::ossl::Settings::from_ini(
            "[OSSL]\nAllow_osGetSimulatorVersion = GOD\n",
        );
        assert_eq!(ossl.ignored, Vec::<String>::new());
        let mut scene = scene_with(
            ImitatedGrid::OpenSim,
            region,
            RegionConfig {
                heartbeat: HeartbeatConfig::stepped(Duration::from_millis(100)),
                ossl: Some(ossl),
                ..RegionConfig::default()
            },
            AccountConfig {
                agent_id: Some(box_owner()),
                god_level,
                ..AccountConfig::new("Test", "User", "password")
            },
        )
        .await?;
        scene.tick(2).await?;
        scene.shout("name").await?;
        scene.tick(2).await?;
        let said = scene
            .next("the version or the refusal", |seen| match seen {
                Seen::Chat(ChatType::DebugChannel, message) => Some(message),
                Seen::Chat(_, message) if message.starts_with("version: ") => Some(message),
                _other => None,
            })
            .await?;
        scene.stop();
        Ok(said)
    }

    /// The region's `[OSSL]` settings are the engine's, and a god — an
    /// account of god level 200 or more — is what `GOD` admits.
    #[tokio::test]
    async fn the_region_ossl_settings_admit_a_god_alone() -> Result<(), TestError> {
        let god = version_for(200).await?;
        assert!(god.starts_with("version: "), "a god was refused: {god}");
        let resident = version_for(0).await?;
        assert!(
            resident.contains("osGetSimulatorVersion permission denied"),
            "a resident was let through: {resident}"
        );
        Ok(())
    }
}
