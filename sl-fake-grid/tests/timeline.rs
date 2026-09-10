//! Scripted timelines against the real client: what a scenario makes happen
//! because time passed, seen from the far end of a live circuit.
//!
//! Every check here drives `sl-client-tokio` rather than reading the grid's own
//! state, because a script that only moved a fixture is a script that did
//! nothing: the claim is that the *client* saw the move, in the order the script
//! wrote it.

#[cfg(test)]
mod test {
    use std::time::Duration;

    use pretty_assertions::assert_eq;
    use sl_client_tokio::{
        Client, Command, EnvironmentPushAction, Event, ExperienceEnvironmentPush, ExperienceKey,
        Llsd, LoginParams, LoginRequest, StartLocation,
    };
    use sl_fake_grid::scenario::STOCK_SCRIPTED_OBJECT_LOCAL_ID;
    use sl_fake_grid::{
        AccountConfig, Action, At, FakeAgent, FakeGrid, FakeGridBuilder, RegionConfig, Scenario,
        Timeline,
    };
    use sl_types::lsl::Vector;
    use sl_types::map::RegionCoordinates;
    use tokio::sync::mpsc;

    /// A boxed error for terse test signatures.
    type TestError = Box<dyn std::error::Error>;

    /// How long any single wait in these tests may take.
    const WAIT: Duration = Duration::from_secs(20);

    /// How long a script waits before its first step. Short enough not to slow
    /// a test down, long enough that the arrival burst — which streams the very
    /// object these scripts then move — has been sent first.
    const LEAD_IN: Duration = Duration::from_millis(50);

    /// Where the stock scripted object is moved to: a metre up and well clear of
    /// where it was rezzed, so no rounding could confuse the two.
    fn moved_to() -> Vector {
        Vector {
            x: 100.0,
            y: 100.0,
            z: 40.0,
        }
    }

    /// The region the eastern half of the teleport test serves.
    const EAST_REGION: &str = "Fake Region East";

    /// Starts a grid whose start region runs `timeline`, plus any `extra`
    /// regions, connects the real client to it, and hands back the grid-side
    /// handle on the session the script will run in.
    async fn start(
        timeline: Timeline,
        extra: Vec<RegionConfig>,
    ) -> Result<(FakeGrid, Client, FakeAgent), TestError> {
        let start_region = RegionConfig {
            scenario: Some(Scenario {
                timeline,
                ..Scenario::default()
            }),
            ..RegionConfig::default()
        };
        let mut builder = FakeGridBuilder::new()
            .account(AccountConfig::new("Test", "User", "password"))
            .event_queue_hold(Duration::from_secs(2))
            .region(start_region);
        for region in extra {
            builder = builder.region(region);
        }
        let grid = builder.start().await?;
        // Subscribed before the login, or the notice can slip past.
        let mut logins = grid.logins();
        let client = Client::connect(LoginParams {
            login_uri: grid.login_uri(),
            request: LoginRequest::new(
                "Test",
                "User",
                "password",
                StartLocation::Last,
                "sl-fake-grid-timeline",
                "0.0",
            ),
        })
        .await?;
        let notice = tokio::time::timeout(WAIT, logins.recv()).await??;
        let agent = grid.agent(&notice).await.ok_or("no live session")?;
        Ok((grid, client, agent))
    }

    /// **A script moves an object, marks it, and kills it — and the client sees
    /// all three, in that order.**
    ///
    /// The three `at` kinds in one script: a duration from the arrival, a
    /// duration from the previous step, and the client's own acknowledgement.
    /// The last is the one with teeth — the kill is scheduled behind the
    /// *client's* receipt of the marker, so a client that sees the removal
    /// without having seen the move first would be a real ordering bug rather
    /// than a race in the test.
    #[tokio::test]
    async fn a_script_moves_marks_and_kills_an_object_in_order() -> Result<(), TestError> {
        let timeline = Timeline::new()
            .then(
                At::AfterArrival(LEAD_IN),
                Action::MoveObject {
                    local_id: STOCK_SCRIPTED_OBJECT_LOCAL_ID,
                    to: moved_to(),
                },
            )
            .after(Duration::ZERO, Action::Marker("moved".to_owned()))
            .then(
                At::OnMarkerAck,
                Action::KillObject(STOCK_SCRIPTED_OBJECT_LOCAL_ID),
            );
        let (grid, client, _agent) = start(timeline, Vec::new()).await?;

        let (event_tx, mut event_rx) = mpsc::channel::<Event>(256);
        let (command_tx, command_rx) = mpsc::channel::<Command>(8);
        let (diag_tx, _diag_rx) = mpsc::channel(16);
        let run = tokio::spawn(client.run(event_tx, diag_tx, command_rx));

        let mut moved = false;
        let mut marked = false;
        loop {
            let event = tokio::time::timeout(WAIT, event_rx.recv())
                .await?
                .ok_or("client event stream ended early")?;
            match event {
                Event::ObjectUpdated(object)
                    if object.local_id == STOCK_SCRIPTED_OBJECT_LOCAL_ID
                        && object.motion.position == moved_to() =>
                {
                    moved = true;
                }
                Event::GenericMessage(generic)
                    if sl_fake_grid::marker_name(&generic).as_deref() == Some("moved") =>
                {
                    assert!(
                        moved,
                        "the marker arrived before the move it was written after"
                    );
                    marked = true;
                }
                Event::ObjectRemoved { local_id, .. }
                    if local_id.id() == STOCK_SCRIPTED_OBJECT_LOCAL_ID =>
                {
                    assert!(
                        marked,
                        "the kill arrived before the marker its step waited on: the \
                         `OnMarkerAck` wait did not wait"
                    );
                    break;
                }
                _ => {}
            }
        }

        drop(command_tx);
        run.abort();
        grid.shutdown();
        Ok(())
    }

    /// **A script pushes an experience environment, and takes it away again.**
    ///
    /// The push is the one live environment change in the protocol, and the
    /// claim here is that both halves of it reach the *client* as typed events:
    /// the partial injection with the keys it named, and the release naming the
    /// same experience. Ordering matters as much as arrival — a release the
    /// client saw before the push it releases would leave the sky changed
    /// forever.
    #[tokio::test]
    async fn a_script_pushes_and_releases_an_experience_environment() -> Result<(), TestError> {
        let experience = ExperienceKey::from(uuid::Uuid::from_u128(0xE_1234));
        let injected = ExperienceEnvironmentPush {
            experience_id: experience,
            action: EnvironmentPushAction::Partial {
                sky: Some(Llsd::Map(std::collections::HashMap::from([(
                    "cloud_shadow".to_owned(),
                    Llsd::Real(0.75),
                )]))),
                water: None,
            },
            transition_time: 2.0,
            owner_id: uuid::Uuid::from_u128(0x00AA),
            object_name: "Weather Machine".to_owned(),
            parcel_name: "The Back Forty".to_owned(),
        };
        let released = ExperienceEnvironmentPush {
            action: EnvironmentPushAction::Clear,
            ..injected.clone()
        };
        let timeline = Timeline::new()
            .then(
                At::AfterArrival(LEAD_IN),
                Action::PushExperienceEnvironment(Box::new(injected.clone())),
            )
            .after(
                Duration::ZERO,
                Action::PushExperienceEnvironment(Box::new(released.clone())),
            );
        let (grid, client, _agent) = start(timeline, Vec::new()).await?;

        let (event_tx, mut event_rx) = mpsc::channel::<Event>(256);
        let (command_tx, command_rx) = mpsc::channel::<Command>(8);
        let (diag_tx, _diag_rx) = mpsc::channel(16);
        let run = tokio::spawn(client.run(event_tx, diag_tx, command_rx));

        let mut seen = Vec::new();
        while seen.len() < 2 {
            let event = tokio::time::timeout(WAIT, event_rx.recv())
                .await?
                .ok_or("client event stream ended early")?;
            if let Event::ExperienceEnvironmentPush(push) = event {
                seen.push(*push);
            }
        }
        assert_eq!(
            seen,
            vec![injected, released],
            "both halves of the push must reach the client, in the order the script wrote them"
        );

        drop(command_tx);
        run.abort();
        grid.shutdown();
        Ok(())
    }

    /// **A script survives the teleport it asked for.**
    ///
    /// The step after the `Teleport` runs in the *destination* session, which is
    /// a second `SimSession` on a second socket: nothing of the first one is
    /// still there to run it. The marker names the region so a script that
    /// somehow ran on the source would be a different message, not a missing
    /// one.
    #[tokio::test]
    async fn a_script_continues_after_the_teleport_it_asked_for() -> Result<(), TestError> {
        let east = RegionConfig {
            name: EAST_REGION.to_owned(),
            // Ten regions away, so the destination is not a neighbour whose
            // child circuit the login already opened: what is being checked is
            // a script continuing in a session that did not exist when it
            // started.
            grid_x: RegionConfig::default().grid_x.saturating_add(10),
            ..RegionConfig::default()
        };
        let timeline = Timeline::new()
            .then(
                At::AfterArrival(LEAD_IN),
                Action::Teleport {
                    region: EAST_REGION.to_owned(),
                    position: RegionCoordinates::new(128.0, 128.0, 26.0),
                    look_at: Vector {
                        x: 1.0,
                        y: 0.0,
                        z: 0.0,
                    },
                },
            )
            .then(At::AfterArrival(LEAD_IN), Action::Marker("east".to_owned()));
        let (grid, client, _agent) = start(timeline, vec![east]).await?;

        let (event_tx, mut event_rx) = mpsc::channel::<Event>(256);
        let (command_tx, command_rx) = mpsc::channel::<Command>(8);
        let (diag_tx, _diag_rx) = mpsc::channel(16);
        let run = tokio::spawn(client.run(event_tx, diag_tx, command_rx));

        let east_handle = grid.region_handle(EAST_REGION).ok_or("no eastern region")?;
        let mut arrived = false;
        loop {
            let event = tokio::time::timeout(WAIT, event_rx.recv())
                .await?
                .ok_or("client event stream ended early")?;
            match event {
                Event::RegionChanged { region_handle, .. } if region_handle == east_handle => {
                    arrived = true;
                }
                Event::GenericMessage(generic)
                    if sl_fake_grid::marker_name(&generic).as_deref() == Some("east") =>
                {
                    assert!(
                        arrived,
                        "the destination's step ran before the client got there"
                    );
                    break;
                }
                _ => {}
            }
        }

        drop(command_tx);
        run.abort();
        grid.shutdown();
        Ok(())
    }

    /// How long the scripted-wait check makes its step wait for. Long enough
    /// that no amount of loopback jitter could account for it, short enough to
    /// be a rounding error in a test run.
    const SCRIPTED_WAIT: Duration = Duration::from_millis(500);

    /// **A scripted wait is actually waited out.**
    ///
    /// Not a claim about the *clock* — that is `clock.rs`'s question, and this
    /// timer is the same one every other deadline in the crate is armed on. The
    /// claim here is that an `at` is a wait at all: the marker must not arrive
    /// before its half-second is up, which is what a runner that read the `at`
    /// and ignored it would do.
    ///
    /// The baseline is the **grid's** own `AgentArrived`, which is the instant
    /// the step is timed from. The client's handshake is not: it reaches the
    /// test through a channel, and it arrived a few milliseconds *after* the
    /// grid's own arrival often enough to fail this by four.
    ///
    /// Real time rather than a paused timer, deliberately: everything in this
    /// tier is real I/O, and tokio's paused clock auto-advances whenever every
    /// task is idle — which is exactly what a socket waiting on a peer looks
    /// like, so every network timeout in the stack would fire at once.
    #[tokio::test]
    async fn a_scripted_wait_is_actually_waited_out() -> Result<(), TestError> {
        let timeline = Timeline::new().then(
            At::AfterArrival(SCRIPTED_WAIT),
            Action::Marker("late".to_owned()),
        );
        let (grid, client, agent) = start(timeline, Vec::new()).await?;
        // The grid's own arrival, which is the instant the step is timed from.
        // Subscribed before the client runs, so it cannot slip past.
        let mut server_events = agent.events();

        let (event_tx, mut event_rx) = mpsc::channel::<Event>(256);
        let (command_tx, command_rx) = mpsc::channel::<Command>(8);
        let (diag_tx, _diag_rx) = mpsc::channel(16);
        let run = tokio::spawn(client.run(event_tx, diag_tx, command_rx));

        loop {
            let event = tokio::time::timeout(WAIT, server_events.recv()).await??;
            if matches!(event, sl_proto::ServerEvent::AgentArrived) {
                break;
            }
        }
        let arrived = std::time::Instant::now();

        loop {
            let event = tokio::time::timeout(WAIT, event_rx.recv())
                .await?
                .ok_or("client event stream ended early")?;
            if let Event::GenericMessage(generic) = &event
                && sl_fake_grid::marker_name(generic).as_deref() == Some("late")
            {
                break;
            }
        }
        let waited = arrived.elapsed();
        assert!(
            waited >= SCRIPTED_WAIT,
            "the scripted marker arrived {waited:?} after the grid recorded the arrival, sooner \
             than the {SCRIPTED_WAIT:?} its step asked for"
        );

        drop(command_tx);
        run.abort();
        grid.shutdown();
        Ok(())
    }

    /// A scenario that says nothing about time carries an empty timeline, which
    /// is what keeps every other test on this grid unchanged.
    #[tokio::test]
    async fn the_stock_scenario_scripts_nothing() -> Result<(), TestError> {
        assert_eq!(Scenario::default().timeline.steps.len(), 0);
        assert!(Scenario::empty().timeline.is_empty());
        Ok(())
    }
}
