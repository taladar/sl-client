//! The grid's stamps come from its injected clock.
//!
//! Both checks have teeth: each runs the same scenario twice, once on a
//! clock skewed an hour ahead of the system one and once on the stock
//! system clock, and the two runs must disagree. A grid that still reached
//! for `Instant::now()` behind the builder's back would make them agree.

#[cfg(test)]
mod test {
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use pretty_assertions::assert_eq;
    use sl_client_tokio::{
        ChatChannel, ChatType, Client, Command, Event, LoginParams, LoginRequest, StartLocation,
    };
    use sl_fake_grid::{
        AccountConfig, FakeGrid, FakeGridBuilder, Now, RegionConfig, Scenario, SimEventHook,
        SimHook, system_clock, tokio_clock,
    };
    use sl_proto::ServerEvent;
    use sl_wire::{
        LoginResponse, build_event_queue_request, build_login_request, build_seed_request,
        parse_login_response, parse_seed_response,
    };
    use tokio::sync::mpsc;

    /// A boxed error for terse test signatures.
    type TestError = Box<dyn std::error::Error>;

    /// How long any single wait in these tests may take.
    const WAIT: Duration = Duration::from_secs(20);

    /// How far ahead of the system clock the injected one runs. Large enough
    /// that no stamp drawn from it can be mistaken for an `Instant::now()`,
    /// and far enough out that the session machines' own deadlines simply
    /// never come due for the length of a test.
    const SKEW: Duration = Duration::from_secs(3600);

    /// A clock an hour ahead of the system one.
    fn skewed_clock() -> Now {
        Arc::new(|| {
            Instant::now()
                .checked_add(SKEW)
                .unwrap_or_else(Instant::now)
        })
    }

    /// Where a scenario's hooks record the instant they were handed.
    type Stamps = Arc<Mutex<Vec<Instant>>>;

    /// The stock scenario with every hook wrapped so it also records the
    /// instant the driver stamped it with.
    fn recording_scenario(stamps: &Stamps) -> Scenario {
        let stock = Scenario::default();
        let stock_setup = Arc::clone(&stock.setup);
        let sink = Arc::clone(stamps);
        let setup: SimHook = Arc::new(move |sim, now| {
            stock_setup(sim, now);
            record(&sink, now);
        });
        let stock_arrival = stock.on_agent_arrived.as_ref().map(Arc::clone);
        let sink = Arc::clone(stamps);
        let on_agent_arrived: SimHook = Arc::new(move |sim, now| {
            if let Some(hook) = &stock_arrival {
                hook(sim, now);
            }
            record(&sink, now);
        });
        let sink = Arc::clone(stamps);
        let on_event: SimEventHook = Arc::new(move |_sim, _event, now| record(&sink, now));
        Scenario {
            setup,
            on_agent_arrived: Some(on_agent_arrived),
            on_event: Some(on_event),
            ..stock
        }
    }

    /// Records one stamp, ignoring a poisoned lock (a panicking hook has
    /// already failed the test more loudly than this could).
    fn record(stamps: &Stamps, now: Instant) {
        if let Ok(mut guard) = stamps.lock() {
            guard.push(now);
        }
    }

    /// Reads the recorded stamps back out.
    fn recorded(stamps: &Stamps) -> Result<Vec<Instant>, TestError> {
        Ok(stamps
            .lock()
            .map_err(|_poisoned| "stamps poisoned")?
            .clone())
    }

    /// Logs a real client into `grid`, drives it to arrival and one chat
    /// line, and returns once the grid decoded that chat — by which point
    /// the setup, arrival and event hooks have all run.
    async fn drive_a_session(grid: &FakeGrid) -> Result<(), TestError> {
        let mut logins = grid.logins();
        let client = Client::connect(LoginParams {
            login_uri: grid.login_uri(),
            request: LoginRequest::new(
                "Test",
                "User",
                "password",
                StartLocation::Last,
                "sl-fake-grid-clock",
                "0.0",
            ),
        })
        .await?;
        let notice = tokio::time::timeout(WAIT, logins.recv()).await??;
        let agent = grid.agent(&notice).await.ok_or("no live session")?;
        let mut server_events = agent.events();
        let (event_tx, mut event_rx) = mpsc::channel::<Event>(256);
        let (command_tx, command_rx) = mpsc::channel::<Command>(8);
        let (diag_tx, _diag_rx) = mpsc::channel(16);
        let run = tokio::spawn(client.run(event_tx, diag_tx, command_rx));

        loop {
            let event = tokio::time::timeout(WAIT, event_rx.recv())
                .await?
                .ok_or("client event stream ended early")?;
            if matches!(
                event,
                Event::RegionHandshakeComplete | Event::RegionChanged { .. }
            ) {
                break;
            }
        }
        command_tx
            .send(Command::Chat {
                message: "what time is it".to_owned(),
                chat_type: ChatType::Normal,
                channel: ChatChannel(0),
            })
            .await?;
        loop {
            let event = tokio::time::timeout(WAIT, server_events.recv()).await??;
            if let ServerEvent::Chat { message, .. } = &event
                && message == "what time is it"
            {
                break;
            }
        }
        drop(command_tx);
        run.abort();
        Ok(())
    }

    /// Runs one session against a grid built with `clock` and returns the
    /// stamps its scenario hooks were handed.
    async fn stamps_under(clock: Option<Now>) -> Result<Vec<Instant>, TestError> {
        let stamps: Stamps = Arc::new(Mutex::new(Vec::new()));
        let mut builder = FakeGridBuilder::new()
            .account(AccountConfig::new("Test", "User", "password"))
            .region(RegionConfig::default())
            .scenario(recording_scenario(&stamps))
            .event_queue_hold(Duration::from_secs(2));
        if let Some(clock) = clock {
            builder = builder.clock(clock);
        }
        let grid = builder.start().await?;
        drive_a_session(&grid).await?;
        grid.shutdown();
        recorded(&stamps)
    }

    /// Every instant a scenario hook is handed — setup, arrival, per-event —
    /// comes from the builder's clock. On the skewed clock every stamp is
    /// most of an hour ahead of real time; on the stock clock none of them
    /// is. A grid that stamped its hooks with `Instant::now()` would make
    /// both runs look like the second.
    #[tokio::test]
    async fn every_scenario_hook_is_stamped_from_the_grid_clock() -> Result<(), TestError> {
        let skewed = stamps_under(Some(skewed_clock())).await?;
        let after_skewed = Instant::now();
        assert!(
            !skewed.is_empty(),
            "the recording scenario's hooks never ran"
        );
        let half = SKEW.checked_div(2).ok_or("halving the skew")?;
        for stamp in &skewed {
            assert!(
                stamp.saturating_duration_since(after_skewed) > half,
                "a hook was stamped {:?} after the run, not from the skewed clock",
                stamp.saturating_duration_since(after_skewed)
            );
        }

        let stock = stamps_under(None).await?;
        let after_stock = Instant::now();
        assert!(
            !stock.is_empty(),
            "the recording scenario's hooks never ran"
        );
        for stamp in &stock {
            assert!(
                stamp <= &after_stock,
                "the stock clock handed out a stamp from the future"
            );
        }
        Ok(())
    }

    /// [`tokio_clock`] reads tokio's virtual time, not the wall clock: a
    /// test that pauses and advances the timer moves it by exactly what it
    /// advanced. The stock [`system_clock`] does not move at all, which is
    /// what a paused-time test would silently get without this helper.
    #[tokio::test]
    async fn the_tokio_clock_follows_paused_virtual_time() -> Result<(), TestError> {
        let virtual_clock = tokio_clock();
        let wall_clock = system_clock();
        tokio::time::pause();
        let (virtual_before, wall_before) = (virtual_clock(), wall_clock());
        let leap = Duration::from_secs(600);
        tokio::time::advance(leap).await;
        assert!(
            virtual_clock().saturating_duration_since(virtual_before) >= leap,
            "the tokio clock must move with the virtual timer"
        );
        assert!(
            wall_clock().saturating_duration_since(wall_before) < leap,
            "the system clock must not follow the virtual timer"
        );
        Ok(())
    }

    /// Logs in over plain HTTP and returns the granted `EventQueueGet` cap.
    async fn event_queue_cap(grid: &FakeGrid) -> Result<String, TestError> {
        let http = reqwest::Client::new();
        let text = http
            .post(grid.login_uri())
            .header("Content-Type", "text/xml")
            .body(build_login_request(&LoginRequest::new(
                "Test",
                "User",
                "password",
                StartLocation::Last,
                "sl-fake-grid-clock",
                "0.0",
            )))
            .send()
            .await?
            .text()
            .await?;
        let LoginResponse::Success(success) = parse_login_response(&text)? else {
            return Err("expected a successful login".into());
        };
        let seed_reply = http
            .post(success.seed_capability.clone())
            .header("Content-Type", "application/llsd+xml")
            .body(build_seed_request(&["EventQueueGet"]))
            .send()
            .await?
            .text()
            .await?;
        parse_seed_response(&seed_reply)?
            .get("EventQueueGet")
            .cloned()
            .ok_or_else(|| "EventQueueGet not granted".into())
    }

    /// Polls an empty event queue, returning `Some(status)` if the hold
    /// expired within `budget` and `None` if it was still being held.
    async fn poll_empty_queue(cap: &str, budget: Duration) -> Result<Option<u16>, TestError> {
        let request = reqwest::Client::new()
            .post(cap)
            .header("Content-Type", "application/llsd+xml")
            .body(build_event_queue_request(None, false))
            .send();
        match tokio::time::timeout(budget, request).await {
            Ok(response) => Ok(Some(response?.status().as_u16())),
            Err(_elapsed) => Ok(None),
        }
    }

    /// The `EventQueueGet` hold deadline is measured on the grid's clock
    /// too: a 200 ms hold expires promptly on the stock clock, and a grid
    /// whose clock runs an hour ahead is still holding the same poll a
    /// second and a half later — its deadline landed an hour out.
    #[tokio::test]
    async fn the_event_queue_hold_is_measured_on_the_grid_clock() -> Result<(), TestError> {
        let hold = Duration::from_millis(200);
        let budget = Duration::from_millis(1500);

        let stock = FakeGridBuilder::new()
            .account(AccountConfig::new("Test", "User", "password"))
            .region(RegionConfig::default())
            .event_queue_hold(hold)
            .start()
            .await?;
        let cap = event_queue_cap(&stock).await?;
        assert_eq!(
            poll_empty_queue(&cap, budget).await?,
            Some(502),
            "the stock clock's 200 ms hold must expire as the 502 re-poll answer"
        );
        stock.shutdown();

        let skewed = FakeGridBuilder::new()
            .account(AccountConfig::new("Test", "User", "password"))
            .region(RegionConfig::default())
            .event_queue_hold(hold)
            .clock(skewed_clock())
            .start()
            .await?;
        let cap = event_queue_cap(&skewed).await?;
        assert_eq!(
            poll_empty_queue(&cap, budget).await?,
            None,
            "a hold deadline drawn from a clock an hour ahead must not expire"
        );
        // The shutdown releases the still-held poll.
        skewed.shutdown();
        Ok(())
    }
}
