//! Every region has a clock of its own that ticks with nobody in it, a test
//! can step it one tick at a time, and the region's statistics go out on its
//! ticks rather than on a timer of each session's.

#[cfg(test)]
mod test {
    use std::time::Duration;

    use pretty_assertions::assert_eq;
    use sl_client_tokio::{Client, Command, Event, LoginParams, LoginRequest, StartLocation};
    use sl_fake_grid::{
        AccountConfig, FakeGrid, FakeGridBuilder, HeartbeatConfig, RegionConfig, TickStats,
    };
    use sl_proto::{ServerEvent, SimStatId, TimeDilation};
    use tokio::sync::mpsc;

    type TestError = Box<dyn core::error::Error>;

    /// How long any single wait in these tests may take.
    const WAIT: Duration = Duration::from_secs(20);

    /// The stock region's name.
    const REGION: &str = "Fake Region";

    /// A grid of the stock region with its heartbeat as given.
    async fn grid_with(heartbeat: HeartbeatConfig) -> Result<FakeGrid, TestError> {
        Ok(FakeGridBuilder::new()
            .account(AccountConfig::new("Test", "User", "password"))
            .region(RegionConfig {
                heartbeat,
                ..RegionConfig::default()
            })
            .start()
            .await?)
    }

    /// A region with no client connected ticks by itself, its clock always
    /// a whole number of steps.
    #[tokio::test]
    async fn an_empty_region_ticks() -> Result<(), TestError> {
        let step = Duration::from_millis(10);
        let grid = grid_with(HeartbeatConfig::running(step)).await?;
        let mut ticks = grid.ticks(REGION)?;
        let tick = tokio::time::timeout(WAIT, async {
            loop {
                if ticks.changed().await.is_err() {
                    return None;
                }
                let tick = *ticks.borrow_and_update();
                if tick.number >= 3 {
                    return Some(tick);
                }
            }
        })
        .await?
        .ok_or("the heartbeat stopped")?;
        assert_eq!(
            tick.region_time,
            step.saturating_mul(u32::try_from(tick.number)?)
        );
        grid.shutdown();
        Ok(())
    }

    /// A zero step is refused when the grid starts, and an unknown region
    /// cannot be ticked.
    #[tokio::test]
    async fn a_zero_step_and_an_unknown_region_are_refused() -> Result<(), TestError> {
        let refused = grid_with(HeartbeatConfig::stepped(Duration::ZERO)).await;
        assert!(
            refused.as_ref().is_err_and(|error| matches!(
                error.downcast_ref::<sl_fake_grid::Error>(),
                Some(sl_fake_grid::Error::ZeroHeartbeatStep { .. })
            )),
            "a zero step was accepted: {refused:?}"
        );
        let grid = grid_with(HeartbeatConfig::stepped(Duration::from_secs(1))).await?;
        assert!(matches!(
            grid.advance_one_tick("Nowhere").await,
            Err(sl_fake_grid::Error::UnknownRegion { .. })
        ));
        grid.shutdown();
        Ok(())
    }

    /// A stepped region never ticks by itself, and each tick a test runs
    /// sends the agent standing in it one `SimStats` stating a region that
    /// kept up — none before the first tick, one after each.
    #[tokio::test]
    async fn each_stepped_tick_sends_the_region_statistics() -> Result<(), TestError> {
        // Longer than either imitated grid's statistics interval, so every
        // tick is a statistics tick.
        let step = Duration::from_secs(5);
        let grid = grid_with(HeartbeatConfig::stepped(step)).await?;
        let mut logins = grid.logins();
        let client = Client::connect(LoginParams {
            login_uri: grid.login_uri(),
            request: LoginRequest::new(
                "Test",
                "User",
                "password",
                StartLocation::Last,
                "sl-fake-grid-heartbeat",
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

        // Forward only the statistics: the client's run loop stops at the
        // 256th unread event, which the region's ground alone outnumbers.
        let (stats_tx, mut stats_rx) = mpsc::unbounded_channel();
        let forward = tokio::spawn(async move {
            while let Some(event) = event_rx.recv().await {
                if let Event::SimStats(stats) = event
                    && stats_tx.send(stats).is_err()
                {
                    break;
                }
            }
        });
        tokio::time::timeout(WAIT, async {
            loop {
                match server_events.recv().await {
                    Ok(ServerEvent::AgentArrived) => return Ok(()),
                    Ok(_) => {}
                    Err(error) => return Err(error),
                }
            }
        })
        .await??;

        // A session used to send its first a second after the arrival; a
        // stepped region sends none until it is ticked.
        let early = tokio::time::timeout(Duration::from_millis(1_500), stats_rx.recv()).await;
        assert!(early.is_err(), "statistics arrived with no tick");
        assert_eq!(grid.ticks(REGION)?.borrow().number, 0);

        for number in 1..=3_u64 {
            let tick = grid.advance_one_tick(REGION).await?;
            assert_eq!(tick.number, number);
            assert_eq!(
                tick.region_time,
                step.saturating_mul(u32::try_from(number)?)
            );
            assert_eq!(
                tick.stats,
                Some(TickStats {
                    dilation: TimeDilation::NORMAL,
                    sent_to: 1,
                })
            );
            let stats = tokio::time::timeout(WAIT, stats_rx.recv())
                .await?
                .ok_or("the client stopped")?;
            let dilation = stats
                .stats
                .iter()
                .find(|(id, _)| matches!(id, SimStatId::TimeDilation))
                .map(|(_, value)| *value);
            assert_eq!(dilation, Some(1.0), "tick {number}");
        }
        let extra = tokio::time::timeout(Duration::from_millis(500), stats_rx.recv()).await;
        assert!(extra.is_err(), "a tick sent more than one SimStats");

        drop(command_tx);
        run.abort();
        forward.abort();
        grid.shutdown();
        Ok(())
    }
}
