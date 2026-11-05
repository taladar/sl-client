//! Scripts run on the fake grid: the catalogue's scripted box runs its script
//! from the start, answers a line said on its channel, stops and starts with
//! the viewer's Running checkbox, restarts on a reset, survives a script that
//! throws, and takes its object out of the region with `llDie`; a script
//! keeps what it holds through a region restart and through a take and a rez;
//! an experience once accepted is granted to its every script, refused where
//! its standing stops it, and a question refused or left unanswered is
//! answered as Second Life answers it; and a region's `[OSSL]` settings gate
//! a function to gods; and a viewer's edits and contents changes raise
//! `changed` as each grid raises it — each step driven by a real client, the
//! region ticked one step at a time.

#[cfg(test)]
mod test {
    use std::time::Duration;

    use pretty_assertions::{assert_eq, assert_ne};
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
        /// A sit was answered: the seat's position and turn as stated.
        Sat(sl_types::lsl::Vector, sl_types::lsl::Rotation),
        /// An object the client already had was sent again.
        Updated(Box<sl_client_tokio::Object>),
        /// An alert's text.
        Alert(String),
        /// A message of the object-update stream: its form and the objects
        /// it named.
        Stream(
            sl_proto::ObjectUpdateForm,
            Vec<sl_proto::RegionLocalObjectId>,
        ),
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
                    Event::ObjectUpdated(object) => Seen::Updated(object),
                    Event::SitResult {
                        sit_position,
                        sit_rotation,
                        ..
                    } => Seen::Sat(sit_position, sit_rotation),
                    Event::ScriptPermissionRequest(question) => Seen::Question(question),
                    Event::AlertMessage { message, .. } => Seen::Alert(message),
                    Event::ObjectStreamBatch(batch) => Seen::Stream(
                        batch.form,
                        batch.entries.iter().map(|entry| entry.local_id).collect(),
                    ),
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

        /// Asks to sit on [`SEAT`] and returns what the response states, with
        /// the avatar's own update that follows: its parent and position.
        async fn sit(
            &mut self,
        ) -> Result<
            (
                sl_types::lsl::Vector,
                sl_proto::RegionLocalObjectId,
                sl_types::lsl::Vector,
            ),
            TestError,
        > {
            self.commands
                .send(Command::Sit {
                    target: seat_key(),
                    offset: at(0.0, 0.0, 0.0),
                })
                .await?;
            let stated = self
                .next("the sit response", |seen| match seen {
                    Seen::Sat(position, _rotation) => Some(position),
                    _other => None,
                })
                .await?;
            let (parent, offset) = self.own_avatar().await?;
            Ok((stated, parent, offset))
        }

        /// The next update of the client's own avatar: its parent and
        /// position.
        async fn own_avatar(
            &mut self,
        ) -> Result<(sl_proto::RegionLocalObjectId, sl_types::lsl::Vector), TestError> {
            let me = self.agent.agent_id().uuid();
            self.next("the avatar's own update", |seen| match seen {
                Seen::Updated(object) if object.full_id.uuid() == me => {
                    Some((object.parent_id, object.motion.position.clone()))
                }
                _other => None,
            })
            .await
        }

        /// Says `message` on channel 7.
        async fn say_seven(&mut self, message: &str) -> Result<(), TestError> {
            self.shout(message).await
        }

        /// The next `count` lines of the worn object's.
        async fn lines(&mut self, count: usize) -> Result<Vec<String>, TestError> {
            let mut lines = Vec::with_capacity(count);
            for _ in 0..count {
                let line = self
                    .next("a line of the worn object's", |seen| match seen {
                        Seen::Chat(_, message)
                            if message.starts_with("changed ")
                                || message.starts_with("attach ") =>
                        {
                            Some(message)
                        }
                        _other => None,
                    })
                    .await
                    .map_err(|error| format!("{error}, after {lines:?}"))?;
                lines.push(line);
            }
            Ok(lines)
        }

        /// Presses on the prim `local_id`, and waits for the session to take it.
        async fn press(
            &mut self,
            local_id: sl_proto::RegionLocalObjectId,
        ) -> Result<(), TestError> {
            let target = sl_client_tokio::ScopedObjectId::new(self.circuit, local_id);
            self.send(
                Command::GrabObject {
                    local_id: target,
                    grab_offset: OFFSET,
                    surface: None,
                },
                |event| matches!(event, ServerEvent::ObjectGrabbed(_)),
            )
            .await
        }

        /// Releases the prim `local_id`, and waits for the session to take it.
        async fn release(
            &mut self,
            local_id: sl_proto::RegionLocalObjectId,
        ) -> Result<(), TestError> {
            let target = sl_client_tokio::ScopedObjectId::new(self.circuit, local_id);
            self.send(
                Command::DegrabObject {
                    local_id: target,
                    surface: None,
                },
                |event| matches!(event, ServerEvent::ObjectReleased(_)),
            )
            .await
        }

        /// Every touch line said until the region falls quiet.
        async fn touch_lines(&mut self) -> Vec<String> {
            let mut lines = Vec::new();
            while let Ok(Some(seen)) = tokio::time::timeout(QUIET, self.seen.recv()).await {
                if let Seen::Chat(_, message) = seen
                    && ["start ", "touch ", "end"]
                        .iter()
                        .any(|prefix| message.starts_with(prefix))
                {
                    lines.push(message);
                }
            }
            lines
        }

        /// Sends `command`, which the session takes as the event `taken`
        /// picks, ticks twice, and returns everything said after.
        async fn edit(
            &mut self,
            command: Command,
            taken: impl Fn(&ServerEvent) -> bool + Send + Sync,
        ) -> Result<Vec<String>, TestError> {
            self.send(command, taken).await?;
            self.tick(2).await?;
            Ok(said_lines(self).await)
        }

        /// Moves, turns or resizes the prim `local_id` with `transform`.
        async fn transform(
            &mut self,
            local_id: sl_proto::RegionLocalObjectId,
            transform: sl_client_tokio::ObjectTransform,
        ) -> Result<Vec<String>, TestError> {
            let local_id = sl_client_tokio::ScopedObjectId::new(self.circuit, local_id);
            self.edit(
                Command::UpdateObject {
                    local_id,
                    transform,
                },
                |event| matches!(event, ServerEvent::ObjectTransformSet { .. }),
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

    // ---- Sitting and wearing ---------------------------------------------

    /// A seat whose script sets a sit target, says on a link change how many
    /// links the object has, who is on its target and whose the last link
    /// is, and on channel 7 clears its target ("clear") or stands the
    /// speaker up ("unsit").
    const SEAT_SOURCE: &str = "default
{
    state_entry()
    {
        llSitTarget(<0.0, 0.0, 0.5>, llEuler2Rot(<0.0, 0.0, PI_BY_TWO>));
        llListen(7, \"\", NULL_KEY, \"\");
    }

    changed(integer change)
    {
        if (change & CHANGED_LINK)
        {
            integer links = llGetNumberOfPrims();
            llShout(0, \"link \" + (string)links + \" \" + (string)llAvatarOnSitTarget()
                + \" \" + (string)llGetLinkKey(links));
        }
    }

    listen(integer channel, string name, key id, string message)
    {
        if (message == \"clear\")
        {
            llSitTarget(ZERO_VECTOR, ZERO_ROTATION);
        }
        else if (message == \"unsit\")
        {
            llUnSit(id);
        }
    }
}
";

    /// The seat's region-local id.
    const SEAT: sl_proto::RegionLocalObjectId = sl_proto::RegionLocalObjectId(0x700);

    /// A grid imitating `imitated` with [`SEAT`] beside the arrival point,
    /// running `source`, the client's account owning it.
    async fn seat_scene(imitated: ImitatedGrid, source: &str) -> Result<Scene, TestError> {
        let region = region_of(&[ScriptedBox {
            local_id: SEAT,
            source: source.to_owned(),
            experience: None,
        }])?;
        let scene = scene_with(
            imitated,
            region,
            RegionConfig {
                heartbeat: HeartbeatConfig::stepped(Duration::from_millis(100)),
                ..RegionConfig::default()
            },
            AccountConfig {
                agent_id: Some(box_owner()),
                ..AccountConfig::new("Test", "User", "password")
            },
        )
        .await?;
        scene.tick(2).await?;
        Ok(scene)
    }

    /// The seat's full id, as [`region_of`] mints it.
    fn seat_key() -> sl_types::key::ObjectKey {
        sl_types::key::ObjectKey::from(uuid::Uuid::from_u128((u128::from(SEAT.0) << 8) | 0x5))
    }

    /// A vector of three components.
    const fn at(x: f32, y: f32, z: f32) -> sl_types::lsl::Vector {
        sl_types::lsl::Vector { x, y, z }
    }

    /// Whether two vectors are the same within a centimetre.
    fn near(a: &sl_types::lsl::Vector, b: &sl_types::lsl::Vector) -> bool {
        (a.x - b.x).abs() < 0.01 && (a.y - b.y).abs() < 0.01 && (a.z - b.z).abs() < 0.01
    }

    /// The key a script prints for nobody.
    const NULL_KEY: &str = "00000000-0000-0000-0000-000000000000";

    /// The whole of a scripted seat on `imitated`: a sit on its target (the
    /// response stating the seated position on Second Life and the target on
    /// OpenSim, the avatar 0.35 m above the target either way), the script
    /// counting the sitter as its last link; a stand; a sit with the target
    /// cleared, placed as the grid places a sit on a prim without one and
    /// nobody on the target; and `llUnSit` standing the avatar.
    async fn a_scripted_seat_seats_and_counts_its_sitter(
        imitated: ImitatedGrid,
    ) -> Result<(), TestError> {
        let mut scene = seat_scene(imitated, SEAT_SOURCE).await?;
        let me = scene.agent.agent_id().uuid().to_string();

        let (stated, parent, offset) = scene.sit().await?;
        let target = if imitated == ImitatedGrid::SecondLife {
            0.85
        } else {
            0.5
        };
        assert!(
            near(&stated, &at(0.0, 0.0, target)),
            "{imitated:?} stated {stated:?}"
        );
        assert_eq!(parent, SEAT);
        assert!(
            near(&offset, &at(0.0, 0.0, 0.85)),
            "{imitated:?} seated at {offset:?}"
        );
        assert_eq!(
            scene.agent.with_sim(|sim| sim.seated_on()).await,
            Some(seat_key()),
            "the request alone seats the avatar"
        );
        scene.tick(2).await?;
        assert_eq!(scene.line("link ").await?, format!("link 2 {me} {me}"));

        scene.commands.send(Command::Stand).await?;
        let (parent, _standing) = scene.own_avatar().await?;
        assert_eq!(parent, sl_proto::RegionLocalObjectId(0));
        scene.tick(2).await?;
        let lone = if imitated == ImitatedGrid::SecondLife {
            NULL_KEY.to_owned()
        } else {
            seat_key().uuid().to_string()
        };
        assert_eq!(
            scene.line("link ").await?,
            format!("link 1 {NULL_KEY} {lone}")
        );

        scene.say_seven("clear").await?;
        scene.tick(2).await?;
        let (stated, parent, offset) = scene.sit().await?;
        let policy = imitated.sit_policy().plain_seat;
        let placed = at(policy.off_centre_m, 0.0, policy.above_m);
        assert_eq!(parent, SEAT);
        assert!(near(&offset, &placed), "{imitated:?} put it at {offset:?}");
        assert!(
            near(
                &stated,
                &at(placed.x, 0.0, placed.z - policy.stated_below_m)
            ),
            "{imitated:?} stated {stated:?}"
        );
        scene.tick(2).await?;
        assert_eq!(
            scene.line("link ").await?,
            format!("link 2 {NULL_KEY} {me}")
        );

        scene.say_seven("unsit").await?;
        scene.tick(2).await?;
        let (parent, _standing) = scene.own_avatar().await?;
        assert_eq!(
            parent,
            sl_proto::RegionLocalObjectId(0),
            "llUnSit left it seated"
        );
        assert_eq!(scene.agent.with_sim(|sim| sim.seated_on()).await, None);
        scene.tick(2).await?;
        assert_eq!(
            scene.line("link ").await?,
            format!("link 1 {NULL_KEY} {lone}")
        );
        scene.stop();
        Ok(())
    }

    /// On a grid imitating Second Life.
    #[tokio::test]
    async fn a_scripted_seat_seats_and_counts_its_sitter_on_second_life() -> Result<(), TestError> {
        a_scripted_seat_seats_and_counts_its_sitter(ImitatedGrid::SecondLife).await
    }

    /// On a grid imitating OpenSim.
    #[tokio::test]
    async fn a_scripted_seat_seats_and_counts_its_sitter_on_opensim() -> Result<(), TestError> {
        a_scripted_seat_seats_and_counts_its_sitter(ImitatedGrid::OpenSim).await
    }

    /// A target somebody took seats the next avatar as a prim without one
    /// does — 0.54 m higher on Second Life, where it would have sat alone on
    /// OpenSim — and `llAvatarOnSitTarget` goes on naming the first.
    #[tokio::test]
    async fn a_taken_target_seats_the_next_avatar_beside_it() -> Result<(), TestError> {
        for imitated in [ImitatedGrid::SecondLife, ImitatedGrid::OpenSim] {
            let mut scene = seat_scene(imitated, SEAT_SOURCE).await?;
            let first = sl_types::key::AgentKey::from(uuid::Uuid::from_u128(0xF125));
            scene
                .agent
                .with_world(|world, _sim| {
                    world.objects.sit(
                        first,
                        SEAT,
                        (
                            at(0.0, 0.0, 0.85),
                            sl_types::lsl::Rotation {
                                x: 0.0,
                                y: 0.0,
                                z: 0.0,
                                s: 1.0,
                            },
                        ),
                        true,
                    )
                })
                .await
                .ok_or("the seat is not in the region")?;
            let (_stated, _parent, offset) = scene.sit().await?;
            let policy = imitated.sit_policy();
            let placed = at(
                policy.plain_seat.off_centre_m,
                0.0,
                policy.plain_seat.above_m + policy.shared_seat_raise_m,
            );
            assert!(near(&offset, &placed), "{imitated:?} put it at {offset:?}");
            scene.tick(2).await?;
            let me = scene.agent.agent_id().uuid();
            assert_eq!(
                scene.line("link ").await?,
                format!("link 3 {} {me}", first.uuid()),
                "{imitated:?}"
            );
            scene.stop();
        }
        Ok(())
    }

    /// A seat without a target is answered from within reach alone on Second
    /// Life — no response, no alert, from 22 m — and from anywhere on
    /// OpenSim.
    #[tokio::test]
    async fn a_far_seat_without_a_target_is_answered_on_opensim_alone() -> Result<(), TestError> {
        for imitated in [ImitatedGrid::SecondLife, ImitatedGrid::OpenSim] {
            let mut scene = seat_scene(imitated, "default { state_entry() { } }").await?;
            scene
                .agent
                .with_world(|world, _sim| {
                    if let Some(mut seat) = world.objects.get_mut(SEAT) {
                        seat.motion.position.x = 150.0;
                    }
                })
                .await;
            scene
                .commands
                .send(Command::Sit {
                    target: seat_key(),
                    offset: at(0.0, 0.0, 0.0),
                })
                .await?;
            let answered = tokio::time::timeout(QUIET, async {
                while let Some(seen) = scene.seen.recv().await {
                    if matches!(seen, Seen::Sat(..)) {
                        return true;
                    }
                }
                false
            })
            .await
            .unwrap_or(false);
            assert_eq!(answered, imitated == ImitatedGrid::OpenSim, "{imitated:?}");
            scene.stop();
        }
        Ok(())
    }

    /// An object that says what it is told of being worn: every `changed`,
    /// every `attach` with whether it names somebody and the point it is on,
    /// and on "wear" asks its owner to let it attach itself to the left
    /// hand.
    const WORN_SOURCE: &str = "default
{
    state_entry()
    {
        llListen(7, \"\", NULL_KEY, \"\");
    }

    changed(integer change)
    {
        llShout(0, \"changed \" + (string)change);
    }

    attach(key id)
    {
        llShout(0, \"attach \" + (string)(id != NULL_KEY) + \" \" + (string)llGetAttached());
    }

    listen(integer channel, string name, key id, string message)
    {
        if (message == \"wear\")
        {
            llRequestPermissions(llGetOwner(), PERMISSION_ATTACH);
        }
        else if (message == \"where\")
        {
            llShout(0, \"where \" + (string)(llGetPos() == llGetRootPosition())
                + \" \" + (string)llGetLocalPos() + \" \" + (string)llGetPos());
        }
    }

    run_time_permissions(integer granted)
    {
        if (granted & PERMISSION_ATTACH)
        {
            llAttachToAvatar(ATTACH_LHAND);
        }
    }
}
";

    /// The lines a wear and a drop raise on `imitated`: OpenSim's own
    /// position change twice before a wear and once before a drop, then
    /// `attach` (`events/attach`).
    fn worn_lines(imitated: ImitatedGrid, worn: bool) -> Vec<String> {
        let moves = match (imitated, worn) {
            (ImitatedGrid::SecondLife, _) => 0,
            (ImitatedGrid::OpenSim, true) => 2,
            (ImitatedGrid::OpenSim, false) => 1,
        };
        let mut lines = vec!["changed 32768".to_owned(); moves];
        lines.push(if worn { "attach 1 5" } else { "attach 0 0" }.to_owned());
        lines
    }

    /// Worn from the ground, dropped, and worn again by its own script: the
    /// same object each time, the client shown it on its avatar's left hand,
    /// the script told as each grid tells it.
    async fn an_object_is_worn_dropped_and_worn_again(
        imitated: ImitatedGrid,
    ) -> Result<(), TestError> {
        let mut scene = seat_scene(imitated, WORN_SOURCE).await?;
        scene
            .commands
            .send(Command::AttachObject {
                local_id: sl_client_tokio::ScopedObjectId::new(scene.circuit, SEAT),
                attachment_point: sl_proto::AttachmentPoint::LeftHand,
                mode: sl_proto::AttachmentMode::Add,
                rotation: sl_types::lsl::Rotation {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    s: 1.0,
                },
            })
            .await?;
        // The seat was sent once already, flagged scripted as its script
        // started: the update that matters is the one that wears it.
        let worn = scene
            .next("the worn object", |seen| match seen {
                Seen::Updated(object)
                    if object.local_id == SEAT && object.attachment_point_id().is_some() =>
                {
                    Some(object)
                }
                _other => None,
            })
            .await?;
        assert_eq!(worn.attachment_point_id(), Some(5));
        assert_ne!(worn.parent_id, sl_proto::RegionLocalObjectId(0), "{worn:?}");
        scene.tick(6).await?;
        assert_eq!(
            scene.lines(worn_lines(imitated, true).len()).await?,
            worn_lines(imitated, true)
        );
        scene.say_seven("where").await?;
        scene.tick(2).await?;
        let standing = scene
            .agent
            .with_sim(|sim| sim.arrival_position().position)
            .await;
        // Worn, it stands where its wearer does, on its point's own origin.
        assert_eq!(
            scene.line("where ").await?,
            format!(
                "where 1 <0.00000, 0.00000, 0.00000> <{:.5}, {:.5}, {:.5}>",
                standing.x(),
                standing.y(),
                standing.z()
            )
        );

        scene
            .commands
            .send(Command::DropAttachments {
                local_ids: vec![sl_client_tokio::ScopedObjectId::new(scene.circuit, SEAT)],
            })
            .await?;
        let dropped = scene
            .next("the dropped object", |seen| match seen {
                Seen::Updated(object) if object.local_id == SEAT => Some(object),
                _other => None,
            })
            .await?;
        assert_eq!(dropped.parent_id, sl_proto::RegionLocalObjectId(0));
        assert_eq!(dropped.attachment_point_id(), None);
        scene.tick(6).await?;
        assert_eq!(
            scene.lines(worn_lines(imitated, false).len()).await?,
            worn_lines(imitated, false)
        );

        let question = scene.ask("wear").await?;
        scene.answer(&question, question.permissions).await?;
        scene.tick(2).await?;
        let worn = scene
            .next("the object worn by its script", |seen| match seen {
                Seen::Updated(object) if object.local_id == SEAT => Some(object),
                _other => None,
            })
            .await?;
        assert_eq!(worn.attachment_point_id(), Some(5));
        scene.tick(6).await?;
        assert_eq!(
            scene.lines(worn_lines(imitated, true).len()).await?,
            worn_lines(imitated, true)
        );
        scene.stop();
        Ok(())
    }

    /// On a grid imitating Second Life.
    #[tokio::test]
    async fn an_object_is_worn_dropped_and_worn_again_on_second_life() -> Result<(), TestError> {
        an_object_is_worn_dropped_and_worn_again(ImitatedGrid::SecondLife).await
    }

    /// On a grid imitating OpenSim.
    #[tokio::test]
    async fn an_object_is_worn_dropped_and_worn_again_on_opensim() -> Result<(), TestError> {
        an_object_is_worn_dropped_and_worn_again(ImitatedGrid::OpenSim).await
    }

    /// A script that shouts every touch event it is handed, with the link
    /// number, the grab offset and the face it detects.
    const TOUCH_SOURCE: &str = "default
{
    touch_start(integer n)
    {
        llShout(0, \"start \" + (string)n + \" link \" + (string)llDetectedLinkNumber(0)
            + \" grab \" + (string)llDetectedGrab(0) + \" face \" + (string)llDetectedTouchFace(0));
    }
    touch(integer n)
    {
        llShout(0, \"touch \" + (string)llDetectedGrab(0));
    }
    touch_end(integer n)
    {
        llShout(0, \"end\");
    }
}
";

    /// A script that takes no touch, and keeps its prim's touches from the
    /// root.
    const NEVER_PASS_SOURCE: &str = "default { state_entry() { llPassTouches(PASS_NEVER); } listen(integer c, string n, key i, string m) { } }";

    /// The touched box's region-local id.
    const TOUCHED: sl_proto::RegionLocalObjectId = sl_proto::RegionLocalObjectId(0x600);

    /// The child linked to it in [`linked_region`].
    const CHILD: sl_proto::RegionLocalObjectId = sl_proto::RegionLocalObjectId(0x601);

    /// The grab offset every press states.
    const OFFSET: sl_types::lsl::Vector = sl_types::lsl::Vector {
        x: 0.11,
        y: 0.22,
        z: 0.33,
    };

    /// The zero vector as a script says it.
    const ZERO: &str = "<0.00000, 0.00000, 0.00000>";

    /// The press's offset as a script says it.
    const SAID_OFFSET: &str = "<0.11000, 0.22000, 0.33000>";

    /// A region holding one box beside the arrival point running
    /// [`TOUCH_SOURCE`].
    fn touch_region() -> Result<sl_fake_grid::RegionFixture, TestError> {
        region_of(&[ScriptedBox {
            local_id: TOUCHED,
            source: TOUCH_SOURCE.to_owned(),
            experience: None,
        }])
    }

    /// A region holding the touch box as the root of a linkset whose child
    /// runs [`NEVER_PASS_SOURCE`].
    fn linked_region() -> Result<sl_fake_grid::RegionFixture, TestError> {
        let mut fixture = region_of(&[
            ScriptedBox {
                local_id: TOUCHED,
                source: TOUCH_SOURCE.to_owned(),
                experience: None,
            },
            ScriptedBox {
                local_id: CHILD,
                source: NEVER_PASS_SOURCE.to_owned(),
                experience: None,
            },
        ])?;
        {
            let mut child = fixture
                .world
                .object_mut(CHILD)
                .ok_or("the child is not in the region")?;
            child.parent_id = TOUCHED;
            child.motion.position = sl_types::lsl::Vector {
                x: 2.0,
                y: 0.0,
                z: 0.0,
            };
        }
        Ok(fixture)
    }

    /// A button held five ticks and a click: Second Life raises `touch` on
    /// the region's clock, once a tick here, and once for the click; OpenSim
    /// raises none with no update between. The prim carries the touch flag
    /// once its script runs.
    async fn a_held_touch(imitated: ImitatedGrid) -> Result<(), TestError> {
        let region = touch_region()?;
        let mut scene = scene_of(imitated, region).await?;
        scene.tick(2).await?;
        let flagged = scene
            .next("the touch box flagged", |seen| match seen {
                Seen::Updated(object) if object.local_id == TOUCHED => Some(object.update_flags),
                _other => None,
            })
            .await?;
        assert_eq!(
            flagged & (sl_proto::prim_flags::SCRIPTED | sl_proto::prim_flags::HANDLE_TOUCH),
            sl_proto::prim_flags::SCRIPTED | sl_proto::prim_flags::HANDLE_TOUCH
        );

        let second_life = imitated == ImitatedGrid::SecondLife;
        let start = if second_life {
            format!("start 1 link 0 grab {ZERO} face -1")
        } else {
            format!("start 1 link 0 grab {SAID_OFFSET} face -1")
        };
        scene.press(TOUCHED).await?;
        scene.tick(5).await?;
        scene.release(TOUCHED).await?;
        scene.tick(2).await?;
        let mut held = vec![start.clone()];
        if second_life {
            held.extend(std::iter::repeat_n(format!("touch {ZERO}"), 5));
        }
        held.push("end".to_owned());
        assert_eq!(scene.touch_lines().await, held);

        scene.press(TOUCHED).await?;
        scene.release(TOUCHED).await?;
        scene.tick(2).await?;
        let mut click = vec![start];
        if second_life {
            click.push(format!("touch {ZERO}"));
        }
        click.push("end".to_owned());
        assert_eq!(scene.touch_lines().await, click);
        scene.stop();
        Ok(())
    }

    /// On a grid imitating Second Life.
    #[tokio::test]
    async fn a_held_touch_on_second_life() -> Result<(), TestError> {
        a_held_touch(ImitatedGrid::SecondLife).await
    }

    /// On a grid imitating OpenSim.
    #[tokio::test]
    async fn a_held_touch_on_opensim() -> Result<(), TestError> {
        a_held_touch(ImitatedGrid::OpenSim).await
    }

    /// A click on a child whose script takes no touch and said `PASS_NEVER`
    /// reaches nobody on Second Life and the root on OpenSim, as link 2; a
    /// click on the root reaches it as link 1 on both.
    async fn a_touch_on_a_child(imitated: ImitatedGrid) -> Result<(), TestError> {
        let region = linked_region()?;
        let mut scene = scene_of(imitated, region).await?;
        scene.tick(2).await?;
        let grab = if imitated == ImitatedGrid::SecondLife {
            ZERO
        } else {
            SAID_OFFSET
        };
        let touch = if imitated == ImitatedGrid::SecondLife {
            vec![format!("touch {ZERO}")]
        } else {
            Vec::new()
        };
        scene.press(CHILD).await?;
        scene.release(CHILD).await?;
        scene.tick(2).await?;
        let mut told = Vec::new();
        if imitated == ImitatedGrid::OpenSim {
            told.push(format!("start 1 link 2 grab {grab} face -1"));
            told.push("end".to_owned());
        }
        assert_eq!(scene.touch_lines().await, told, "a click on the child");

        scene.press(TOUCHED).await?;
        scene.release(TOUCHED).await?;
        scene.tick(2).await?;
        let mut told = vec![format!("start 1 link 1 grab {grab} face -1")];
        told.extend(touch);
        told.push("end".to_owned());
        assert_eq!(scene.touch_lines().await, told, "a click on the root");
        scene.stop();
        Ok(())
    }

    /// On a grid imitating Second Life.
    #[tokio::test]
    async fn a_touch_on_a_child_on_second_life() -> Result<(), TestError> {
        a_touch_on_a_child(ImitatedGrid::SecondLife).await
    }

    /// On a grid imitating OpenSim.
    #[tokio::test]
    async fn a_touch_on_a_child_on_opensim() -> Result<(), TestError> {
        a_touch_on_a_child(ImitatedGrid::OpenSim).await
    }

    /// The root of [`linkset_region`], named `A`.
    const LINK_ROOT: sl_proto::RegionLocalObjectId = sl_proto::RegionLocalObjectId(0x800);

    /// Its first child, named `B`.
    const LINK_B: sl_proto::RegionLocalObjectId = sl_proto::RegionLocalObjectId(0x801);

    /// Its second child, named `C`.
    const LINK_C: sl_proto::RegionLocalObjectId = sl_proto::RegionLocalObjectId(0x802);

    /// The key [`region_of`] mints for the prim `local_id`.
    fn key_of(local_id: sl_proto::RegionLocalObjectId) -> uuid::Uuid {
        uuid::Uuid::from_u128((u128::from(local_id.0) << 8) | 0x5)
    }

    /// What the root's script does: on channel 7, `report` says its link
    /// number, the prim count and the names in link order and pings the other
    /// prims; `ask` asks the owner for `PERMISSION_CHANGE_LINKS`; `break 2`,
    /// `break all` and `link B` link and delink; and every `CHANGED_LINK`
    /// is reported.
    fn leader_source() -> String {
        format!(
            "report(string label)
{{
    integer prims = llGetNumberOfPrims();
    string said = label + \" \" + (string)llGetLinkNumber() + \"/\" + (string)prims + \":\";
    if (prims < 2)
    {{
        said += \" \" + llGetObjectName();
    }}
    else
    {{
        integer link;
        for (link = 1; link <= prims; ++link)
        {{
            said += \" \" + llGetLinkName(link);
        }}
    }}
    llShout(0, said);
}}

default
{{
    state_entry()
    {{
        llListen(7, \"\", NULL_KEY, \"\");
    }}

    listen(integer channel, string name, key id, string message)
    {{
        if (message == \"report\")
        {{
            report(\"report\");
            llMessageLinked(LINK_ALL_OTHERS, 0, \"ping\", \"\");
        }}
        else if (message == \"ask\")
        {{
            llRequestPermissions(llGetOwner(), PERMISSION_CHANGE_LINKS);
        }}
        else if (message == \"break 2\")
        {{
            llBreakLink(2);
        }}
        else if (message == \"break all\")
        {{
            llBreakAllLinks();
        }}
        else if (message == \"link B\")
        {{
            llCreateLink(\"{}\", TRUE);
        }}
    }}

    run_time_permissions(integer granted)
    {{
        llShout(0, \"granted \" + (string)granted);
    }}

    changed(integer change)
    {{
        if (change & CHANGED_LINK)
        {{
            report(\"changed\");
        }}
    }}
}}
",
            key_of(LINK_B)
        )
    }

    /// What each child's script does: says whom a link message came from,
    /// and its link number and the prim count at every `CHANGED_LINK`.
    const FOLLOWER_SOURCE: &str = "default
{
    link_message(integer sender, integer number, string message, key id)
    {
        llShout(0, \"pinged \" + llGetObjectName() + \" by \" + (string)sender);
    }

    changed(integer change)
    {
        if (change & CHANGED_LINK)
        {
            llShout(0, \"child \" + llGetObjectName() + \" \" + (string)llGetLinkNumber() + \"/\"
                + (string)llGetNumberOfPrims());
        }
    }
}
";

    /// A region holding a linkset of three cubes, `A` the root and `B` and `C`
    /// its children in that order, owned by the logged-in user: the root
    /// running [`leader_source`], the children [`FOLLOWER_SOURCE`].
    fn linkset_region() -> Result<sl_fake_grid::RegionFixture, TestError> {
        let follower = |local_id| ScriptedBox {
            local_id,
            source: FOLLOWER_SOURCE.to_owned(),
            experience: None,
        };
        let mut fixture = region_of(&[
            ScriptedBox {
                local_id: LINK_ROOT,
                source: leader_source(),
                experience: None,
            },
            follower(LINK_B),
            follower(LINK_C),
        ])?;
        for (local_id, name, along) in [
            (LINK_ROOT, "A", 0.0),
            (LINK_B, "B", 1.0),
            (LINK_C, "C", 2.0),
        ] {
            let mut prim = fixture
                .world
                .object_mut(local_id)
                .ok_or("a cube is not in the region")?;
            let mut record = sl_fake_grid::world::default_object_properties(&prim);
            record.name = name.to_owned();
            prim.properties = Some(record);
            if local_id != LINK_ROOT {
                prim.parent_id = LINK_ROOT;
                prim.motion.position = sl_types::lsl::Vector {
                    x: along,
                    y: 0.0,
                    z: 0.0,
                };
            }
        }
        Ok(fixture)
    }

    /// Every line said until the region falls quiet, sorted: what one step
    /// said, whichever script spoke first.
    async fn said_lines(scene: &mut Scene) -> Vec<String> {
        let mut lines = Vec::new();
        while let Ok(Some(seen)) = tokio::time::timeout(QUIET, scene.seen.recv()).await {
            if let Seen::Chat(ChatType::Shout, message) = seen {
                lines.push(message);
            }
        }
        lines.sort();
        lines
    }

    /// `message` shouted on channel 7, the region ticked `ticks` times, and
    /// everything said after.
    async fn step(scene: &mut Scene, message: &str, ticks: u32) -> Result<Vec<String>, TestError> {
        scene.shout(message).await?;
        scene.tick(ticks).await?;
        Ok(said_lines(scene).await)
    }

    /// Sorted owned lines.
    fn lines(said: &[&str]) -> Vec<String> {
        let mut lines: Vec<String> = said.iter().map(|line| (*line).to_owned()).collect();
        lines.sort();
        lines
    }

    /// A scripted linkset of three reads its link numbers, names and prim
    /// count as the grids do; `LINK_ALL_OTHERS` from the root reaches exactly
    /// the two children; and, granted `PERMISSION_CHANGE_LINKS`, the root
    /// delinks a child, links it back and breaks every link, each prim
    /// hearing `CHANGED_LINK` as its grid tells it (`prim/link`) and the
    /// returning child numbered where its grid numbers a newcomer.
    async fn a_scripted_linkset(imitated: ImitatedGrid) -> Result<(), TestError> {
        let region = linkset_region()?;
        let mut scene = scene_with(
            imitated,
            region,
            RegionConfig {
                heartbeat: HeartbeatConfig::stepped(Duration::from_millis(100)),
                ..RegionConfig::default()
            },
            AccountConfig {
                agent_id: Some(box_owner()),
                ..AccountConfig::new("Test", "User", "password")
            },
        )
        .await?;
        scene.tick(2).await?;
        let _arrival = said_lines(&mut scene).await;
        let opensim = imitated == ImitatedGrid::OpenSim;

        assert_eq!(
            step(&mut scene, "report", 2).await?,
            lines(&["report 1/3: A B C", "pinged B by 1", "pinged C by 1"])
        );

        scene.shout("ask").await?;
        scene.tick(2).await?;
        let question = scene
            .next("the link question", |seen| match seen {
                Seen::Question(question) => Some(question),
                _other => None,
            })
            .await?;
        scene
            .answer(
                &question,
                sl_types::lsl::ScriptPermissions(sl_types::lsl::ScriptPermissions::CHANGE_LINKS),
            )
            .await?;
        scene.tick(2).await?;
        assert_eq!(said_lines(&mut scene).await, lines(&["granted 128"]));

        // Second Life tells the whole set; OpenSim the child it let go alone.
        let broken = step(&mut scene, "break 2", 3).await?;
        let mut told = vec!["child B 0/1"];
        if !opensim {
            told.extend(["changed 1/2: A C", "child C 2/2"]);
        }
        assert_eq!(broken, lines(&told), "llBreakLink(2)");

        // The child comes back as link 2 on Second Life and last on OpenSim;
        // the link's forced second of sleep is over within fifteen ticks.
        let relinked = step(&mut scene, "link B", 15).await?;
        let told = if opensim {
            ["changed 1/3: A C B", "child B 3/3", "child C 2/3"]
        } else {
            ["changed 1/3: A B C", "child B 2/3", "child C 3/3"]
        };
        assert_eq!(relinked, lines(&told), "llCreateLink");

        // OpenSim's root hears it once for each child it let go.
        let all = step(&mut scene, "break all", 3).await?;
        let mut told = vec!["changed 0/1: A", "child B 0/1", "child C 0/1"];
        if opensim {
            told.push("changed 0/1: A");
        }
        assert_eq!(all, lines(&told), "llBreakAllLinks");
        assert_eq!(
            step(&mut scene, "report", 2).await?,
            lines(&["report 0/1: A"])
        );
        scene.stop();
        Ok(())
    }

    /// On a grid imitating Second Life.
    #[tokio::test]
    async fn a_scripted_linkset_on_second_life() -> Result<(), TestError> {
        a_scripted_linkset(ImitatedGrid::SecondLife).await
    }

    /// On a grid imitating OpenSim.
    #[tokio::test]
    async fn a_scripted_linkset_on_opensim() -> Result<(), TestError> {
        a_scripted_linkset(ImitatedGrid::OpenSim).await
    }

    /// What every prim of [`changes_region`] runs: each `changed` shouted as
    /// the prim's name and the bits, and on channel 7 `allow` lets anybody
    /// drop an item into the prim.
    const REPORTER_SOURCE: &str = "default
{
    state_entry()
    {
        llListen(7, \"\", NULL_KEY, \"\");
    }

    listen(integer channel, string name, key id, string message)
    {
        if (message == \"allow\")
        {
            llAllowInventoryDrop(TRUE);
        }
    }

    changed(integer change)
    {
        llShout(0, llGetObjectName() + \" \" + (string)change);
    }
}
";

    /// The box of [`changes_region`] somebody else owns.
    const STRANGERS: sl_proto::RegionLocalObjectId = sl_proto::RegionLocalObjectId(0x810);

    /// The box of [`changes_region`] the client takes, to drop it in again.
    const SPARE: sl_proto::RegionLocalObjectId = sl_proto::RegionLocalObjectId(0x811);

    /// A region holding the linkset of [`linkset_region`] — `A`, `B` and `C`,
    /// owned by the logged-in user — a box `D` owned by somebody else and a box
    /// `E` of the user's, every prim running [`REPORTER_SOURCE`].
    fn changes_region() -> Result<sl_fake_grid::RegionFixture, TestError> {
        let reporter = |local_id| ScriptedBox {
            local_id,
            source: REPORTER_SOURCE.to_owned(),
            experience: None,
        };
        let mut fixture = region_of(&[
            reporter(LINK_ROOT),
            reporter(LINK_B),
            reporter(LINK_C),
            reporter(STRANGERS),
            reporter(SPARE),
        ])?;
        let stranger = uuid::Uuid::from_u128(0x5_7A2A);
        for (local_id, name, along) in [
            (LINK_ROOT, "A", 0.0),
            (LINK_B, "B", 1.0),
            (LINK_C, "C", 2.0),
            (STRANGERS, "D", 0.0),
            (SPARE, "E", 0.0),
        ] {
            let mut prim = fixture
                .world
                .object_mut(local_id)
                .ok_or("a cube is not in the region")?;
            if local_id == STRANGERS {
                prim.owner_id = stranger;
            }
            let mut record = sl_fake_grid::world::default_object_properties(&prim);
            record.name = name.to_owned();
            prim.properties = Some(record);
            if matches!(local_id, LINK_B | LINK_C) {
                prim.parent_id = LINK_ROOT;
                prim.motion.position = sl_types::lsl::Vector {
                    x: along,
                    y: 0.0,
                    z: 0.0,
                };
            }
        }
        Ok(fixture)
    }

    /// Every face of a box, the default texture, tinted `color` and glowing
    /// `glow`.
    fn faces(color: [u8; 4], glow: f32) -> sl_client_tokio::TextureEntry {
        let face = sl_client_tokio::TextureFace {
            color,
            glow,
            ..sl_client_tokio::TextureFace::new(sl_types::key::TextureKey::from(
                sl_proto::DEFAULT_PRIM_TEXTURE,
            ))
        };
        sl_client_tokio::TextureEntry {
            faces: vec![face; 6],
        }
    }

    /// Everything shouted and every alert the client is told until the
    /// session falls quiet, the shouts sorted.
    async fn shouts_and_alerts(scene: &mut Scene) -> (Vec<String>, Vec<String>) {
        let mut shouts = Vec::new();
        let mut alerts = Vec::new();
        while let Ok(Some(seen)) = tokio::time::timeout(QUIET, scene.seen.recv()).await {
            match seen {
                Seen::Chat(ChatType::Shout, message) => shouts.push(message),
                Seen::Alert(message) => alerts.push(message),
                _other => {}
            }
        }
        shouts.sort();
        (shouts, alerts)
    }

    /// A viewer's edits and contents changes raise `changed` in the prims each
    /// grid raises it in, with the bits it raises (`events/changed-edits`,
    /// `events/changed-contents`): a face's colour `CHANGED_COLOR` and its
    /// glow `CHANGED_TEXTURE`, each only when it changed; a resize
    /// `CHANGED_SCALE` every time, a child's heard by the root on Second Life;
    /// a move and a turn OpenSim's own 32768 alone; a shape `CHANGED_SHAPE`;
    /// a drop and a new script `CHANGED_INVENTORY`; and a drop into somebody
    /// else's prim refused — Second Life saying so — until a script there
    /// allows drops, when it raises `CHANGED_ALLOWED_DROP`.
    #[expect(
        clippy::too_many_lines,
        reason = "one step per row of the probes' tables, each a few lines"
    )]
    async fn a_viewers_changes_raise_changed(imitated: ImitatedGrid) -> Result<(), TestError> {
        let region = changes_region()?;
        let mut scene = scene_with(
            imitated,
            region,
            RegionConfig {
                heartbeat: HeartbeatConfig::stepped(Duration::from_millis(100)),
                ..RegionConfig::default()
            },
            AccountConfig {
                agent_id: Some(box_owner()),
                ..AccountConfig::new("Test", "User", "password")
            },
        )
        .await?;
        scene.tick(2).await?;
        let _arrival = said_lines(&mut scene).await;
        let opensim = imitated == ImitatedGrid::OpenSim;
        let root = sl_client_tokio::ScopedObjectId::new(scene.circuit, LINK_ROOT);
        let retexture = |texture_entry| Command::SetObjectImage {
            local_id: root,
            media_url: None,
            texture_entry,
        };
        let retextured = |event: &ServerEvent| matches!(event, ServerEvent::ObjectImageSet { .. });

        let red = scene
            .edit(retexture(faces([255, 0, 0, 255], 0.0)), retextured)
            .await?;
        assert_eq!(red, lines(&["A 2"]), "a colour");
        let again = scene
            .edit(retexture(faces([255, 0, 0, 255], 0.0)), retextured)
            .await?;
        assert_eq!(again, lines(&[]), "the same colour again");
        let glowing = scene
            .edit(retexture(faces([255, 0, 0, 255], 0.2)), retextured)
            .await?;
        assert_eq!(glowing, lines(&["A 16"]), "a glow");

        let size = |x: f32| sl_client_tokio::ObjectTransform {
            scale: Some(sl_types::lsl::Vector { x, y: x, z: x }),
            ..sl_client_tokio::ObjectTransform::default()
        };
        assert_eq!(
            scene.transform(LINK_ROOT, size(0.6)).await?,
            lines(&["A 8"]),
            "a resize"
        );
        assert_eq!(
            scene.transform(LINK_ROOT, size(0.6)).await?,
            lines(&["A 8"]),
            "a resize to the same size"
        );
        let resized_child = scene.transform(LINK_B, size(0.4)).await?;
        let heard = if opensim { "B 8" } else { "A 8" };
        assert_eq!(resized_child, lines(&[heard]), "a child's resize");

        let moved = scene
            .transform(
                LINK_ROOT,
                sl_client_tokio::ObjectTransform {
                    position: Some(sl_types::lsl::Vector {
                        x: 132.0,
                        y: 128.0,
                        z: 25.8,
                    }),
                    ..sl_client_tokio::ObjectTransform::default()
                },
            )
            .await?;
        let told: &[&str] = if opensim {
            &["A 32768", "B 32768", "B 32768", "C 32768", "C 32768"]
        } else {
            &[]
        };
        assert_eq!(moved, lines(told), "a move of the root");
        let turned = scene
            .transform(
                LINK_ROOT,
                sl_client_tokio::ObjectTransform {
                    rotation: Some(sl_types::lsl::Rotation {
                        x: 0.0,
                        y: 0.0,
                        z: 0.383,
                        s: 0.924,
                    }),
                    ..sl_client_tokio::ObjectTransform::default()
                },
            )
            .await?;
        let told: &[&str] = if opensim {
            &["B 32768", "C 32768"]
        } else {
            &[]
        };
        assert_eq!(turned, lines(told), "a turn of the root");

        let shape = sl_client_tokio::PrimShapeParams {
            path_curve: 16,
            profile_curve: 1,
            path_begin: 12_500,
            path_scale_x: 100,
            path_scale_y: 100,
            ..sl_client_tokio::PrimShapeParams::default()
        };
        let reshaped = scene
            .edit(
                Command::SetObjectShape {
                    local_id: sl_client_tokio::ScopedObjectId::new(scene.circuit, LINK_B),
                    shape,
                },
                |event| matches!(event, ServerEvent::ObjectShapeSet { .. }),
            )
            .await?;
        assert_eq!(reshaped, lines(&["B 4"]), "a shape");

        let made = scene
            .edit(
                Command::RezScript {
                    target: sl_client_tokio::ScopedObjectId::new(scene.circuit, LINK_C),
                    params: Box::new(sl_client_tokio::RezScriptParams {
                        group_id: None,
                        enabled: false,
                        item: sl_client_tokio::RestoreItem::new_script(
                            box_owner(),
                            sl_types::key::ObjectKey::from(key_of(LINK_C)),
                            "New Script",
                            uuid::Uuid::from_u128(0x5C),
                        ),
                    }),
                },
                |event| matches!(event, ServerEvent::RezScript { .. }),
            )
            .await?;
        assert_eq!(made, lines(&["C 1"]), "a new script");

        // The spare box taken, to be dropped in again.
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
                local_ids: vec![sl_client_tokio::ScopedObjectId::new(scene.circuit, SPARE)],
                destination: sl_client_tokio::DeRezDestination::TakeIntoAgentInventory(objects),
                transaction_id: sl_client_tokio::TransactionId::from(uuid::Uuid::from_u128(0x7B)),
                group_id: None,
            })
            .await?;
        let spare = scene
            .next("the taken item", |seen| match seen {
                Seen::Filed(item) => Some(item),
                _other => None,
            })
            .await?;
        scene.tick(2).await?;
        let _taken = said_lines(&mut scene).await;
        let drop_into = |local_id: sl_proto::RegionLocalObjectId, circuit| {
            let task = sl_types::key::ObjectKey::from(key_of(local_id));
            sl_client_tokio::RestoreItem::for_task_drop(&spare, task, uuid::Uuid::from_u128(0xD0))
                .map(|item| Command::UpdateTaskInventory {
                    target: sl_client_tokio::ScopedObjectId::new(circuit, local_id),
                    key: sl_client_tokio::TaskInventoryKey::Item,
                    item: Box::new(item),
                })
        };
        let dropped =
            |event: &ServerEvent| matches!(event, ServerEvent::UpdateTaskInventory { .. });
        let own = scene
            .edit(drop_into(LINK_ROOT, scene.circuit)?, dropped)
            .await?;
        assert_eq!(own, lines(&["A 1"]), "a drop into the owner's prim");

        scene
            .send(drop_into(STRANGERS, scene.circuit)?, dropped)
            .await?;
        scene.tick(2).await?;
        let (refused, told) = shouts_and_alerts(&mut scene).await;
        assert_eq!(refused, lines(&[]), "a drop into somebody else's prim");
        let alerts: &[&str] = if opensim {
            &[]
        } else {
            &["Unable to edit this!"]
        };
        assert_eq!(told, lines(alerts), "what a refused drop is told");

        scene.shout("allow").await?;
        scene.tick(2).await?;
        let _allowed = said_lines(&mut scene).await;
        let let_in = scene
            .edit(drop_into(STRANGERS, scene.circuit)?, dropped)
            .await?;
        assert_eq!(let_in, lines(&["D 64"]), "a drop it allows");
        scene.stop();
        Ok(())
    }

    /// On a grid imitating Second Life.
    #[tokio::test]
    async fn a_viewers_changes_raise_changed_on_second_life() -> Result<(), TestError> {
        a_viewers_changes_raise_changed(ImitatedGrid::SecondLife).await
    }

    /// On a grid imitating OpenSim.
    #[tokio::test]
    async fn a_viewers_changes_raise_changed_on_opensim() -> Result<(), TestError> {
        a_viewers_changes_raise_changed(ImitatedGrid::OpenSim).await
    }

    /// A script that lets anything be dropped into its prim and takes
    /// touches: two of the prim's flags, both set in the tick it starts.
    const DROP_AND_TOUCH_SOURCE: &str =
        "default { state_entry() { llAllowInventoryDrop(TRUE); } touch_start(integer n) { } }";

    /// The box [`DROP_AND_TOUCH_SOURCE`] runs in.
    const FLAGGED: sl_proto::RegionLocalObjectId = sl_proto::RegionLocalObjectId(0x900);

    /// What a tick changed in a prim reaches the viewer as one update: the
    /// script's drop flag and the region's touch and scripted flags, set in
    /// the same tick, arrive in a single full `ObjectUpdate` rather than one
    /// per change.
    async fn one_tick_is_one_update(imitated: ImitatedGrid) -> Result<(), TestError> {
        let region = region_of(&[ScriptedBox {
            local_id: FLAGGED,
            source: DROP_AND_TOUCH_SOURCE.to_owned(),
            experience: None,
        }])?;
        let mut scene = scene_of(imitated, region).await?;
        scene
            .next("the box", |seen| match seen {
                Seen::Added(local_id) if local_id == FLAGGED => Some(()),
                _other => None,
            })
            .await?;
        scene.tick(3).await?;
        let mut told = Vec::new();
        let mut flags = None;
        let _quiet = tokio::time::timeout(QUIET, async {
            while let Some(seen) = scene.seen.recv().await {
                match seen {
                    Seen::Stream(form, ids) if ids.contains(&FLAGGED) => told.push(form),
                    Seen::Updated(object) if object.local_id == FLAGGED => {
                        flags = Some(object.update_flags);
                    }
                    _other => {}
                }
            }
        })
        .await;
        assert_eq!(told, vec![sl_proto::ObjectUpdateForm::Full]);
        let wanted = sl_proto::prim_flags::ALLOW_INVENTORY_DROP
            | sl_proto::prim_flags::SCRIPTED
            | sl_proto::prim_flags::HANDLE_TOUCH;
        assert_eq!(flags.map(|flags| flags & wanted), Some(wanted));
        scene.stop();
        Ok(())
    }

    /// On a grid imitating Second Life.
    #[tokio::test]
    async fn one_tick_is_one_update_on_second_life() -> Result<(), TestError> {
        one_tick_is_one_update(ImitatedGrid::SecondLife).await
    }

    /// On a grid imitating OpenSim.
    #[tokio::test]
    async fn one_tick_is_one_update_on_opensim() -> Result<(), TestError> {
        one_tick_is_one_update(ImitatedGrid::OpenSim).await
    }

    /// A script that, told `go` on the echo channel, moves its prim half a
    /// metre east five times, and told `sink`, tries to take it 20 m down
    /// and shouts where it ended up.
    fn mover_source() -> String {
        format!(
            "default {{
    state_entry() {{ llListen({ECHO_SCRIPT_CHANNEL}, \"\", NULL_KEY, \"\"); }}
    listen(integer channel, string name, key id, string message) {{
        if (message == \"go\") {{
            integer i;
            for (i = 0; i < 5; ++i) {{ llSetPos(llGetPos() + <0.5, 0.0, 0.0>); }}
        }} else if (message == \"sink\") {{
            llSetPos(llGetPos() - <0.0, 0.0, 20.0>);
            llShout(0, \"at \" + (string)llGetPos());
        }}
    }}
}}"
        )
    }

    /// The box [`mover_source`] runs in.
    const MOVER: sl_proto::RegionLocalObjectId = sl_proto::RegionLocalObjectId(0x901);

    /// A script moving its prim every tick — `llSetPos` costs it 0.2 s, the
    /// region's step here — reaches the viewer as one terse update a tick
    /// and no full update, ending where the moves took it; and a move into
    /// the ground stops on it.
    async fn a_moving_prim_goes_out_terse(imitated: ImitatedGrid) -> Result<(), TestError> {
        let region = region_of(&[ScriptedBox {
            local_id: MOVER,
            source: mover_source(),
            experience: None,
        }])?;
        let mut scene = scene_stepped(imitated, region, Duration::from_millis(200)).await?;
        scene
            .next("the box", |seen| match seen {
                Seen::Added(local_id) if local_id == MOVER => Some(()),
                _other => None,
            })
            .await?;
        // The script starting flags its prim: that full update first.
        scene.tick(3).await?;
        while let Ok(Some(_seen)) = tokio::time::timeout(QUIET, scene.seen.recv()).await {}

        scene.shout("go").await?;
        scene.tick(7).await?;
        let mut told = Vec::new();
        let mut at = None;
        while let Ok(Some(seen)) = tokio::time::timeout(QUIET, scene.seen.recv()).await {
            match seen {
                Seen::Stream(form, ids) if ids.contains(&MOVER) => told.push(form),
                Seen::Updated(object) if object.local_id == MOVER => {
                    let position = &object.motion.position;
                    at = Some(format!(
                        "{:.2} {:.2} {:.2}",
                        position.x, position.y, position.z
                    ));
                }
                _other => {}
            }
        }
        assert_eq!(told, vec![sl_proto::ObjectUpdateForm::Terse; 5]);
        assert_eq!(at.as_deref(), Some("134.50 128.00 25.50"));

        scene.shout("sink").await?;
        scene.tick(3).await?;
        let ended = scene
            .next("where the sunk box ended", |seen| match seen {
                Seen::Chat(ChatType::Shout, message) if message.starts_with("at ") => Some(message),
                _other => None,
            })
            .await?;
        assert_eq!(ended, "at <134.50000, 128.00000, 25.00000>");
        scene.stop();
        Ok(())
    }

    /// On a grid imitating Second Life.
    #[tokio::test]
    async fn a_moving_prim_goes_out_terse_on_second_life() -> Result<(), TestError> {
        a_moving_prim_goes_out_terse(ImitatedGrid::SecondLife).await
    }

    /// On a grid imitating OpenSim.
    #[tokio::test]
    async fn a_moving_prim_goes_out_terse_on_opensim() -> Result<(), TestError> {
        a_moving_prim_goes_out_terse(ImitatedGrid::OpenSim).await
    }

    /// A script that, told `dress` on the echo channel, sets its prim's
    /// colour, hover text, size, light, particles and texture animation in
    /// one rule list; told `read`, shouts its first face's colour and its
    /// size as `llGetPrimitiveParams` reads them; and shouts every `changed`.
    fn dresser_source() -> String {
        format!(
            "default {{
    state_entry() {{ llListen({ECHO_SCRIPT_CHANNEL}, \"\", NULL_KEY, \"\"); }}
    listen(integer channel, string name, key id, string message) {{
        if (message == \"dress\") {{
            llSetLinkPrimitiveParamsFast(LINK_THIS, [PRIM_COLOR, 0, <0.5, 0.0, 0.0>, 1.0,
                PRIM_TEXT, \"hello\", <1.0, 1.0, 1.0>, 1.0, PRIM_SIZE, <2.0, 1.0, 1.0>,
                PRIM_POINT_LIGHT, TRUE, <1.0, 1.0, 0.0>, 1.0, 10.0, 0.5]);
            llParticleSystem([PSYS_PART_MAX_AGE, 2.0, PSYS_SRC_BURST_RATE, 0.5]);
            llSetTextureAnim(ANIM_ON | LOOP, ALL_SIDES, 2, 2, 0.0, 4.0, 1.0);
        }} else if (message == \"read\") {{
            llShout(0, \"color \" + llList2CSV(llGetPrimitiveParams([PRIM_COLOR, 0, PRIM_SIZE])));
        }}
    }}
    changed(integer change) {{ llShout(0, \"changed \" + (string)change); }}
}}"
        )
    }

    /// The box [`dresser_source`] runs in.
    const DRESSER: sl_proto::RegionLocalObjectId = sl_proto::RegionLocalObjectId(0x902);

    /// A script's rule list reaches the viewer as the prim's update — its
    /// first face tinted, its text, its size, its light, its particles and
    /// its texture animation — and raises `changed` as the grid does: one
    /// event for colour and size on Second Life, one each on OpenSim. The
    /// script reads its colour back as the grid stores it (0.5, a byte's
    /// 0.498039 on OpenSim); a viewer's retexture since is what it reads
    /// next, the record no longer what the script wrote.
    async fn a_script_dresses_its_prim(imitated: ImitatedGrid) -> Result<(), TestError> {
        let region = region_of(&[ScriptedBox {
            local_id: DRESSER,
            source: dresser_source(),
            experience: None,
        }])?;
        let mut scene = scene_with(
            imitated,
            region,
            RegionConfig {
                heartbeat: HeartbeatConfig::stepped(Duration::from_millis(100)),
                ..RegionConfig::default()
            },
            AccountConfig {
                agent_id: Some(box_owner()),
                ..AccountConfig::new("Test", "User", "password")
            },
        )
        .await?;
        scene
            .next("the box", |seen| match seen {
                Seen::Added(local_id) if local_id == DRESSER => Some(()),
                _other => None,
            })
            .await?;
        scene.tick(2).await?;
        while let Ok(Some(_seen)) = tokio::time::timeout(QUIET, scene.seen.recv()).await {}
        let opensim = imitated == ImitatedGrid::OpenSim;

        scene.shout("dress").await?;
        scene.tick(3).await?;
        let mut dressed = None;
        let mut said = Vec::new();
        while let Ok(Some(seen)) = tokio::time::timeout(QUIET, scene.seen.recv()).await {
            match seen {
                Seen::Updated(object) if object.local_id == DRESSER => dressed = Some(object),
                Seen::Chat(ChatType::Shout, message) => said.push(message),
                _other => {}
            }
        }
        let dressed = dressed.ok_or("the dressed box was never sent")?;
        let entry = sl_proto::decode_texture_entry(&dressed.texture_entry, 6);
        let first = entry.faces.first().map(|face| face.color);
        let red = if opensim { 127 } else { 128 };
        assert_eq!(first, Some([red, 0, 0, 255]), "the first face's colour");
        assert_eq!(dressed.text, "hello");
        assert_eq!(dressed.text_color, [255, 255, 255, 0], "opaque white text");
        assert_eq!(
            dressed.scale,
            sl_types::lsl::Vector {
                x: 2.0,
                y: 1.0,
                z: 1.0
            }
        );
        assert!(dressed.extra.light.is_some(), "the light");
        assert!(!dressed.particle_system.is_empty(), "the particles");
        assert_eq!(
            dressed
                .texture_animation
                .map(|anim| (anim.mode, anim.size_x, anim.size_y)),
            Some((3, 2, 2)),
            "the texture animation"
        );
        let changed = if opensim {
            lines(&["changed 2", "changed 8"])
        } else {
            lines(&["changed 10"])
        };
        said.sort();
        assert_eq!(said, changed, "what the rule list raised");

        // `llGetPrimitiveParams` costs 0.2 s: its shout comes on the third tick.
        let read = step(&mut scene, "read", 4).await?;
        let colour = if opensim {
            "color <0.498039, 0.000000, 0.000000>, 1.000000, <2.000000, 1.000000, 1.000000>"
        } else {
            "color <0.500000, 0.000000, 0.000000>, 1.000000, <2.000000, 1.000000, 1.000000>"
        };
        assert_eq!(read, lines(&[colour]));

        let local_id = sl_client_tokio::ScopedObjectId::new(scene.circuit, DRESSER);
        let _retextured = scene
            .edit(
                Command::SetObjectImage {
                    local_id,
                    media_url: None,
                    texture_entry: faces([0, 0, 255, 255], 0.0),
                },
                |event| matches!(event, ServerEvent::ObjectImageSet { .. }),
            )
            .await?;
        // `llGetPrimitiveParams` costs 0.2 s: its shout comes on the third tick.
        let read = step(&mut scene, "read", 4).await?;
        assert_eq!(
            read,
            lines(&[
                "color <0.000000, 0.000000, 1.000000>, 1.000000, <2.000000, 1.000000, 1.000000>"
            ])
        );
        scene.stop();
        Ok(())
    }

    /// On a grid imitating Second Life.
    #[tokio::test]
    async fn a_script_dresses_its_prim_on_second_life() -> Result<(), TestError> {
        a_script_dresses_its_prim(ImitatedGrid::SecondLife).await
    }

    /// On a grid imitating OpenSim.
    #[tokio::test]
    async fn a_script_dresses_its_prim_on_opensim() -> Result<(), TestError> {
        a_script_dresses_its_prim(ImitatedGrid::OpenSim).await
    }
}
