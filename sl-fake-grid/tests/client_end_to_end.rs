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
        let (_grid, client, agent) = connect().await?;
        let expected_agent = agent.agent_id();
        assert_eq!(client.agent_id(), Some(expected_agent));

        let server_events = agent.events();
        let (event_tx, mut event_rx) = mpsc::channel::<Event>(256);
        let (command_tx, command_rx) = mpsc::channel::<Command>(8);
        let (diag_tx, _diag_rx) = mpsc::channel(16);
        let run = tokio::spawn(client.run(event_tx, diag_tx, command_rx));

        // The circuit comes up: the grid sees the arrival, the client sees the
        // region handshake and the scenario's greeting line.
        let mut greeted = false;
        let mut handshaken = false;
        while !(greeted && handshaken) {
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
