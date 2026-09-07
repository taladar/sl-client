//! Smoke tier: a scripted object on a real grid talking `@` to a real viewer.
//!
//! Every other RLV test in the workspace stops at a function boundary — the
//! parser is handed a string, the state machine is handed a command, the intake
//! is handed a line. This tier hands nothing to anything: an object on an
//! in-process [`sl_fake_grid`] grid says `llOwnerSay("@detach=n")` over real
//! UDP, and the assertion is what comes back out of the socket on the other
//! side. It is the only place the *whole* surface is exercised —
//! `ChatFromSimulator` → the owner-say gate → the state machine → the reply
//! queue → `ChatFromViewer` → the grid — and the only one that would notice if
//! any seam between them were left unwired.
//!
//! It runs the real [`SlClientPlugin`] (its socket-owning network thread,
//! blocking login, retransmission) and the real [`RlvIntakePlugin`], with no
//! stand-ins between them. What it does *not* run is the UI: the four RLVa
//! windows need the whole widget scaffold, and what they draw is already pinned
//! by the crate's own unit tests. The state they draw is asserted here directly.

#[cfg(test)]
mod test {
    use std::time::{Duration, Instant};

    use bevy::prelude::*;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{
        ChatChannel, ChatLogConfig, ChatSource, ChatType, ClientDirectories, InventoryCacheConfig,
        LoginParams, LoginRequest, ObjectKey, SlClientPlugin, SlEvent, SlSessionEvent,
        StartLocation, Vector,
    };
    use sl_fake_grid::{AccountConfig, FakeGrid, FakeGridBuilder, RegionConfig};
    use sl_proto::ServerEvent;
    use sl_rlv::RlvBehaviour;
    use sl_settings::{Scope, SettingValue, SettingsStore};
    use sl_viewer_notifications::ShowNotification;
    use sl_viewer_rlv::intake::{NOTIFY_TOGGLED_OFF, NOTIFY_TOGGLED_ON, RlvIntakePlugin};
    use sl_viewer_settings::ViewerSettings;
    use sl_viewer_world_api::rlv::{
        RlvConsoleKind, RlvSession, SETTING_DEBUG, SETTING_MAIN, register_settings,
        swallows_owner_say,
    };
    use tokio::sync::broadcast;
    use uuid::Uuid;

    /// A boxed error for terse test signatures.
    type TestError = Box<dyn std::error::Error>;

    /// How long any single wait may take before the test fails.
    const WAIT: Duration = Duration::from_secs(15);

    /// How long to sleep between frames while waiting.
    const FRAME_PAUSE: Duration = Duration::from_millis(5);

    /// How many frames [`Harness::settle`] gives the intake to act. Generous,
    /// because the assertion it precedes is that it did *not*.
    const SETTLE_FRAMES: u32 = 30;

    /// The channel the test's queries and subscriptions name. Any positive
    /// channel that is not the debug channel would do; a script picks one the
    /// same way.
    const REPLY_CHANNEL: i32 = 2222;

    /// The collar — the object that owner-says at the viewer.
    const COLLAR: Uuid = Uuid::from_u128(0xc0_11a2);

    /// Every [`SlSessionEvent`] the plugin emitted and every notification the
    /// intake raised, in order — the "did the wire message actually arrive" and
    /// "was the user told" halves of each assertion.
    #[derive(Resource, Default)]
    struct Recorded {
        /// The session events, oldest first.
        events: Vec<SlSessionEvent>,
        /// The catalogue names of the notifications raised, oldest first.
        notifications: Vec<&'static str>,
    }

    /// Appends this frame's events and notifications to [`Recorded`].
    fn record(
        mut events: MessageReader<SlEvent>,
        mut notifications: MessageReader<ShowNotification>,
        mut recorded: ResMut<Recorded>,
    ) {
        for event in events.read() {
            recorded.events.push(event.0.clone());
        }
        for notification in notifications.read() {
            recorded.notifications.push(notification.template);
        }
    }

    /// The grid, the app logged into it, and the grid-side handles.
    struct Harness {
        /// The tokio runtime hosting the grid's tasks (must outlive the grid).
        runtime: tokio::runtime::Runtime,
        /// The grid (dropping it shuts everything down).
        grid: FakeGrid,
        /// The headless app running the client plugin and the RLV intake.
        app: App,
        /// Login notices, subscribed before the app starts.
        logins: broadcast::Receiver<sl_fake_grid::LoginNotice>,
    }

    impl Harness {
        /// Starts a grid with one account and builds (but does not step) a
        /// headless app logging into it with RLV `enabled` or not.
        ///
        /// The master switch is set *before* the first frame on purpose: this
        /// is the state a user who turned RLV on is in every session after,
        /// and it is the state the gate has to be right in.
        fn start(enabled: bool) -> Result<Self, TestError> {
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
                    "sl-fake-grid-rlv",
                    "0.0",
                ),
            };
            let mut settings = ViewerSettings::from_store_for_test(SettingsStore::new());
            register_settings(&mut settings);
            settings.set(Scope::Global, SETTING_MAIN, SettingValue::Bool(enabled));
            // The console echo is the only window into what the intake did that
            // does not require the UI, so this tier always wants it.
            settings.set(Scope::Global, SETTING_DEBUG, SettingValue::Bool(true));

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
                .insert_resource(settings)
                .init_resource::<RlvSession>()
                .add_plugins(RlvIntakePlugin)
                .init_resource::<Recorded>()
                .add_systems(PostUpdate, record);
            Ok(Self {
                runtime,
                grid,
                app,
                logins,
            })
        }

        /// Steps frames until `done` returns `Some`, or fails after [`WAIT`].
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
                    let console: Vec<String> = self
                        .app
                        .world()
                        .resource::<RlvSession>()
                        .console()
                        .iter()
                        .rev()
                        .take(12)
                        .map(|line| format!("{}{}", line.kind.prefix(), line.text))
                        .collect();
                    return Err(format!(
                        "timed out waiting for {what}; last console lines: {console:#?}"
                    )
                    .into());
                }
                std::thread::sleep(FRAME_PAUSE);
            }
        }

        /// Steps frames until the grid has broadcast a matching [`ServerEvent`].
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

        /// Steps frames until a login notice arrives, then resolves the grid's
        /// live agent handle for it.
        fn logged_in_agent(&mut self) -> Result<sl_fake_grid::FakeAgent, TestError> {
            let deadline = Instant::now().checked_add(WAIT).ok_or("clock overflow")?;
            let notice = loop {
                self.app.update();
                if let Ok(received) = self.logins.try_recv() {
                    break received;
                }
                if Instant::now() >= deadline {
                    return Err("timed out waiting for the grid's login notice".into());
                }
                std::thread::sleep(FRAME_PAUSE);
            };
            self.runtime
                .block_on(self.grid.agent(&notice))
                .ok_or_else(|| "no live session for the login notice".into())
        }

        /// Steps frames until the circuit and the region handshake are up, so
        /// the grid can push chat at a client that will hear it.
        fn wait_until_in_world(&mut self) -> Result<(), TestError> {
            type Matcher = fn(&SlSessionEvent) -> bool;
            const MILESTONES: &[(&str, Matcher)] = &[
                ("CircuitEstablished", |event| {
                    matches!(event, SlSessionEvent::CircuitEstablished { .. })
                }),
                ("RegionHandshakeComplete", |event| {
                    matches!(event, SlSessionEvent::RegionHandshakeComplete)
                }),
            ];
            for &(what, matches_event) in MILESTONES {
                self.step_until(what, |app| {
                    app.world()
                        .resource::<Recorded>()
                        .events
                        .iter()
                        .any(&matches_event)
                        .then_some(())
                })?;
            }
            Ok(())
        }

        /// Move the `RestrainedLove` master switch, as the RLVa menu does, and
        /// let the frame that notices it run.
        fn set_rlv(&mut self, enabled: bool) {
            self.app.world_mut().resource_mut::<ViewerSettings>().set(
                Scope::Global,
                SETTING_MAIN,
                SettingValue::Bool(enabled),
            );
            self.settle();
        }

        /// Step enough frames for anything the intake was going to do to have
        /// happened — the wait an assertion that *nothing* happened needs, since
        /// there is no event to key it off.
        fn settle(&mut self) {
            for _frame in 0..SETTLE_FRAMES {
                self.app.update();
                std::thread::sleep(FRAME_PAUSE);
            }
        }

        /// The RLV state machine the intake fills.
        fn session(&self) -> &RlvSession {
            self.app.world().resource::<RlvSession>()
        }
    }

    /// Anything the viewer has said on [`REPLY_CHANNEL`] so far, or `None` if
    /// it has said nothing there — the shape "no answer was chatted back" needs.
    fn drain_reply_channel(events: &mut broadcast::Receiver<ServerEvent>) -> Option<String> {
        while let Ok(event) = events.try_recv() {
            if let ServerEvent::Chat {
                message, channel, ..
            } = event
                && channel == ChatChannel(REPLY_CHANNEL)
            {
                return Some(message);
            }
        }
        None
    }

    /// Say `line` at the viewer as `COLLAR` would: `llOwnerSay` is
    /// `CHAT_TYPE_OWNER` from an object source, which is the exact shape the
    /// gate admits and the only one it does.
    fn owner_say(
        harness: &Harness,
        agent: &sl_fake_grid::FakeAgent,
        line: &str,
    ) -> Result<(), TestError> {
        harness.runtime.block_on(agent.with_sim(|sim| {
            sim.send_chat_from_simulator(
                "Collar",
                ChatSource::Object(ObjectKey::from(COLLAR)),
                agent.agent_id().uuid(),
                ChatType::Owner,
                1,
                Vector {
                    x: 128.0,
                    y: 128.0,
                    z: 25.0,
                },
                line,
                agent.now(),
            )
        }))?;
        Ok(())
    }

    /// The whole surface in one session: a collar restrains the viewer, is
    /// answered, is heard by a `@notify` subscriber, is refused an answer it
    /// cannot honestly give, and finally lets go.
    ///
    /// One test rather than six because each of these costs a login, and
    /// because the *order* is itself the thing worth proving: a real device
    /// handshakes, subscribes, restrains and releases over one session, and a
    /// seam that only works from a clean state would pass six isolated tests.
    #[test]
    fn a_collar_commands_the_viewer_and_is_answered() -> Result<(), TestError> {
        let mut harness = Harness::start(true)?;
        let agent = harness.logged_in_agent()?;
        let mut server_events = agent.events();
        harness.wait_until_in_world()?;

        // 1. The handshake. Every RLV device opens with it, and a viewer that
        //    does not answer is one every device concludes is not RLV at all.
        owner_say(&harness, &agent, &format!("@version={REPLY_CHANNEL}"))?;
        let version = harness.wait_for_server_event(
            "the @version answer shouted back",
            &mut server_events,
            |event| match event {
                ServerEvent::Chat {
                    message,
                    channel,
                    chat_type,
                } if *channel == ChatChannel(REPLY_CHANNEL) => Some((message.clone(), *chat_type)),
                _ => None,
            },
        )?;
        assert!(
            version.0.contains("RestrainedLife viewer"),
            "the handshake should name the viewer: {version:?}"
        );
        // The reference shouts its replies (`RlvUtil::sendChatReply` sends
        // `CHAT_TYPE_SHOUT`) so the answer carries the full 100 m.
        assert_eq!(version.1, ChatType::Shout);

        // 2. The line the viewer took never reached the chat surfaces. It did
        //    arrive on the wire — the recorded event proves the grid really
        //    sent it — and the one predicate every display surface asks says to
        //    swallow it.
        let settings = harness.app.world().resource::<ViewerSettings>();
        let arrived = harness
            .app
            .world()
            .resource::<Recorded>()
            .events
            .iter()
            .filter_map(|event| match event {
                SlSessionEvent::ChatReceived(chat) if chat.message.starts_with('@') => {
                    Some(chat.clone())
                }
                _ => None,
            })
            .next_back()
            .ok_or("the owner-say never arrived as a ChatReceived")?;
        assert_eq!(arrived.chat_type, ChatType::Owner);
        assert!(swallows_owner_say(
            Some(settings),
            arrived.chat_type,
            &arrived.message
        ));

        // 3. A subscription, then a restriction: the subscriber is told, and
        //    the state machine holds what it was told about.
        owner_say(&harness, &agent, &format!("@notify:{REPLY_CHANNEL}=n"))?;
        owner_say(&harness, &agent, "@fly=n,detach=n")?;
        harness.step_until("the restrictions to be held", |app| {
            let session = app.world().resource::<RlvSession>();
            (session.state().has_behaviour(RlvBehaviour::Fly)
                && session
                    .state()
                    .has_behaviour_from(COLLAR, RlvBehaviour::Detach, ""))
            .then_some(())
        })?;
        // Attributed to the collar, which is what makes taking it off lift
        // exactly what it held.
        assert!(
            harness
                .session()
                .state()
                .restricting_objects()
                .any(|object| object == COLLAR)
        );
        harness.wait_for_server_event(
            "the @notify report of @fly=n",
            &mut server_events,
            |event| match event {
                ServerEvent::Chat {
                    message, channel, ..
                } if *channel == ChatChannel(REPLY_CHANNEL) && message == "/fly=n" => Some(()),
                _ => None,
            },
        )?;

        // 4. `@getstatus` reads nothing outside the state machine, so it is
        //    answered in full — with the restrictions step 3 just added.
        owner_say(&harness, &agent, &format!("@getstatus={REPLY_CHANNEL}"))?;
        let status = harness.wait_for_server_event(
            "the @getstatus answer",
            &mut server_events,
            |event| match event {
                ServerEvent::Chat {
                    message, channel, ..
                } if *channel == ChatChannel(REPLY_CHANNEL) && message.contains("fly") => {
                    Some(message.clone())
                }
                _ => None,
            },
        )?;
        assert!(status.contains("/fly"), "{status:?}");
        assert!(status.contains("/detach"), "{status:?}");

        // 5. A query that reads the avatar is *not* answered: this viewer has
        //    no query source wired to its appearance yet, and `@getattach`
        //    answered all-zeros for a dressed avatar is a lie the script acts
        //    on. The console says so instead of the grid hearing a wrong answer.
        owner_say(&harness, &agent, &format!("@getattach={REPLY_CHANNEL}"))?;
        harness.step_until("the console to record the unanswered query", |app| {
            app.world()
                .resource::<RlvSession>()
                .console()
                .iter()
                .any(|line| {
                    line.kind == RlvConsoleKind::Error && line.text.contains("not answered")
                })
                .then_some(())
        })?;

        // 6. Letting go. `@fly=y` lifts what the collar held, and the
        //    subscriber hears that too — the whole point of `@notify` being on
        //    the command path rather than on each enforcement family.
        owner_say(&harness, &agent, "@fly=y")?;
        harness.step_until("the restriction to be lifted", |app| {
            (!app
                .world()
                .resource::<RlvSession>()
                .state()
                .has_behaviour(RlvBehaviour::Fly))
            .then_some(())
        })?;
        harness.wait_for_server_event(
            "the @notify report of @fly=y",
            &mut server_events,
            |event| match event {
                ServerEvent::Chat {
                    message, channel, ..
                } if *channel == ChatChannel(REPLY_CHANNEL) && message == "/fly=y" => Some(()),
                _ => None,
            },
        )?;

        // 7. Nothing told the user their switch moved, because it did not. A
        //    session that logs in with RLV already on must not be greeted by a
        //    card saying it was just turned on.
        assert_eq!(
            harness.app.world().resource::<Recorded>().notifications,
            Vec::<&'static str>::new()
        );
        Ok(())
    }

    /// With the master switch off the very same line does nothing: it is not
    /// applied, nothing is shouted back, and it is *not* swallowed — seeing an
    /// object try to restrain a viewer that does not obey RLV is the point of
    /// leaving it in the chat.
    ///
    /// Then the switch is flipped **on mid-session**, which is what makes this
    /// a test of the gate rather than of an absent intake: the line said while
    /// it was off stays unheard (an object cannot bank commands against a
    /// switch it hopes will move), and the very next line is obeyed.
    ///
    /// And flipped back **off**, which releases everything — the state the
    /// reference reaches by restarting, since a restart is what its own switch
    /// demands. Both moves tell the user, because both leave them somewhere
    /// they will otherwise meet as a bug.
    #[test]
    fn the_master_switch_is_what_decides() -> Result<(), TestError> {
        let mut harness = Harness::start(false)?;
        let agent = harness.logged_in_agent()?;
        let mut server_events = agent.events();
        harness.wait_until_in_world()?;

        // 1. Off: the line arrives and nothing comes of it.
        owner_say(&harness, &agent, &format!("@fly=n,version={REPLY_CHANNEL}"))?;
        // Wait for the line to have arrived, so "nothing happened" is a
        // statement about a message the viewer really received.
        harness.step_until("the owner-say to arrive", |app| {
            app.world()
                .resource::<Recorded>()
                .events
                .iter()
                .any(|event| {
                    matches!(event, SlSessionEvent::ChatReceived(chat)
                        if chat.message.starts_with('@'))
                })
                .then_some(())
        })?;
        harness.settle();

        assert!(
            !harness.session().state().has_behaviour(RlvBehaviour::Fly),
            "a switched-off viewer must not be restrained"
        );
        assert_eq!(harness.session().state().restricting_objects().count(), 0);
        // The chat surfaces show it, because the viewer did not take it.
        let settings = harness.app.world().resource::<ViewerSettings>();
        assert!(!swallows_owner_say(
            Some(settings),
            ChatType::Owner,
            "@fly=n"
        ));
        assert_eq!(drain_reply_channel(&mut server_events), None);
        assert_eq!(
            harness.app.world().resource::<Recorded>().notifications,
            Vec::<&'static str>::new(),
            "logging in with RLV off is not a toggle"
        );

        // 2. On: the user is told why devices worn before now may not work, and
        //    the line said while it was off is not banked and replayed.
        harness.set_rlv(true);
        assert!(
            !harness.session().state().has_behaviour(RlvBehaviour::Fly),
            "a line said while the switch was off must not be replayed when it moves"
        );
        assert_eq!(
            harness.app.world().resource::<Recorded>().notifications,
            vec![NOTIFY_TOGGLED_ON]
        );

        // 3. The next line *is* obeyed and answered, which is what proves the
        //    intake was running all along and the gate is what refused step 1.
        owner_say(&harness, &agent, &format!("@fly=n,version={REPLY_CHANNEL}"))?;
        harness.step_until("the restriction to be held once RLV is on", |app| {
            app.world()
                .resource::<RlvSession>()
                .state()
                .has_behaviour(RlvBehaviour::Fly)
                .then_some(())
        })?;
        harness.wait_for_server_event(
            "the @version answer, now that RLV is on",
            &mut server_events,
            |event| match event {
                ServerEvent::Chat {
                    message, channel, ..
                } if *channel == ChatChannel(REPLY_CHANNEL)
                    && message.contains("RestrainedLife viewer") =>
                {
                    Some(())
                }
                _ => None,
            },
        )?;

        // 4. Off again: **off means off**. Every restriction the collar had
        //    placed is released, not merely un-enforced — the reference reaches
        //    exactly this state by restarting, and leaving them held would
        //    invent one it cannot: restrained, with the windows that could show
        //    it greyed out because RLV is off.
        harness.set_rlv(false);
        assert!(
            !harness.session().state().has_behaviour(RlvBehaviour::Fly),
            "turning RLV off must release what it was holding"
        );
        assert_eq!(harness.session().state().restricting_objects().count(), 0);
        assert_eq!(
            harness.app.world().resource::<Recorded>().notifications,
            vec![NOTIFY_TOGGLED_ON, NOTIFY_TOGGLED_OFF]
        );
        Ok(())
    }
}
