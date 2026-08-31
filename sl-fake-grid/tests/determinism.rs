//! Two grids built from the same seed and the same content mint the same
//! identifiers and answer a scripted session the same way — the property
//! that makes an offline scenario's records comparable run to run.

#[cfg(test)]
mod test {
    use std::time::Duration;

    use pretty_assertions::{assert_eq, assert_ne};
    use sl_client_tokio::{
        ChatChannel, ChatType, Client, Command, Event, LoginParams, LoginRequest, StartLocation,
    };
    use sl_fake_grid::{AccountConfig, FakeGrid, FakeGridBuilder, RegionConfig};
    use sl_proto::ServerEvent;
    use sl_types::key::AgentKey;
    use sl_wire::{
        LoginResponse, build_login_request, build_seed_request, parse_login_response,
        parse_seed_response,
    };
    use tokio::sync::mpsc;

    type TestError = Box<dyn core::error::Error>;

    /// How long any single wait in these tests may take.
    const WAIT: Duration = Duration::from_secs(20);

    /// Build one seeded grid and collect its visible minted identifiers.
    async fn minted(seed: u64) -> Result<(AgentKey, uuid::Uuid), TestError> {
        let grid = FakeGridBuilder::new()
            .account(AccountConfig::new("Test", "User", "password"))
            .region(RegionConfig::default())
            .deterministic(seed)
            .http_port(0)
            .start()
            .await?;
        let agent = grid
            .account_agent_id("Test", "User")
            .ok_or("the account was not registered")?;
        let region = grid
            .region_names()
            .first()
            .and_then(|name| grid.region_id(name))
            .ok_or("the region was not registered")?;
        grid.shutdown();
        Ok((agent, region))
    }

    /// The same seed yields the same identifiers; a different seed does not.
    #[tokio::test]
    async fn the_same_seed_mints_the_same_identifiers() -> Result<(), TestError> {
        let first = minted(7).await?;
        let second = minted(7).await?;
        assert_eq!(first, second, "one seed, one identifier stream");
        let third = minted(8).await?;
        assert_ne!(first, third, "a different seed must not repeat the stream");
        Ok(())
    }

    /// What one scripted run of a seeded grid produced: every identifier the
    /// grid minted that reaches a client, and the sequence of decoded
    /// [`ServerEvent`]s a login-to-chat session drew out of it.
    #[derive(Debug, PartialEq, Eq)]
    struct Transcript {
        /// The account's minted agent id.
        agent: AgentKey,
        /// The start region's minted id.
        region: uuid::Uuid,
        /// The login-minted session id.
        session: uuid::Uuid,
        /// The login-minted secure session id.
        secure_session: uuid::Uuid,
        /// The login-minted circuit code.
        circuit_code: sl_wire::CircuitCode,
        /// The seed capability's path (its minted token; the port is the
        /// listener's and differs run to run).
        seed_path: String,
        /// The granted capability paths, by capability name (each a minted
        /// token).
        caps: Vec<(String, String)>,
        /// The decoded grid-side events, in order.
        events: Vec<String>,
    }

    /// Events driven by a client-side timer rather than by the script: the
    /// movement cadence, pings, throttle renegotiation, and the reliable
    /// give-up an unlucky resend schedule can produce. UDP resend timing is
    /// the client's, so a transcript that counted these would be testing the
    /// client's clock, not the grid's determinism.
    const CADENCE_EVENTS: [&str; 5] = [
        "AgentUpdate",
        "PingRequested",
        "Throttle",
        "ReliableGiveUp",
        "ViewerEffect",
    ];

    /// A [`ServerEvent`]'s variant name — the leading identifier of its
    /// `Debug` rendering, before any payload.
    fn variant_name(event: &ServerEvent) -> String {
        format!("{event:?}")
            .chars()
            .take_while(char::is_ascii_alphanumeric)
            .collect()
    }

    /// The stock login request for these runs.
    fn login_request() -> LoginRequest {
        LoginRequest::new(
            "Test",
            "User",
            "password",
            StartLocation::Last,
            "sl-fake-grid-determinism",
            "0.0",
        )
    }

    /// Logs in over plain HTTP and grants a fixed capability list, returning
    /// the identifiers the login and the seed grant minted.
    async fn minted_over_http(
        grid: &FakeGrid,
    ) -> Result<
        (
            uuid::Uuid,
            uuid::Uuid,
            sl_wire::CircuitCode,
            String,
            Vec<(String, String)>,
        ),
        TestError,
    > {
        let http = reqwest::Client::new();
        let text = http
            .post(grid.login_uri())
            .header("Content-Type", "text/xml")
            .body(build_login_request(&login_request()))
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
            .body(build_seed_request(&["EventQueueGet", "SimulatorFeatures"]))
            .send()
            .await?
            .text()
            .await?;
        let mut caps: Vec<(String, String)> = parse_seed_response(&seed_reply)?
            .into_iter()
            .filter_map(|(name, url)| {
                url.parse::<url::Url>()
                    .ok()
                    .map(|parsed| (name, parsed.path().to_owned()))
            })
            .collect();
        caps.sort();
        Ok((
            success.session_id,
            success.secure_session_id,
            success.circuit_code,
            success.seed_capability.path().to_owned(),
            caps,
        ))
    }

    /// Runs the real client through login, arrival and one chat line, and
    /// returns the grid-side events the script drew out, cadence events
    /// dropped.
    async fn scripted_events(grid: &FakeGrid) -> Result<Vec<String>, TestError> {
        let mut logins = grid.logins();
        let client = Client::connect(LoginParams {
            login_uri: grid.login_uri(),
            request: login_request(),
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
                message: "hello twice".to_owned(),
                chat_type: ChatType::Normal,
                channel: ChatChannel(0),
            })
            .await?;
        let mut events = Vec::new();
        loop {
            let event = tokio::time::timeout(WAIT, server_events.recv()).await??;
            let name = variant_name(&event);
            let closing =
                matches!(&event, ServerEvent::Chat { message, .. } if message == "hello twice");
            if !CADENCE_EVENTS.contains(&name.as_str()) {
                events.push(name);
            }
            if closing {
                break;
            }
        }
        drop(command_tx);
        run.abort();
        Ok(events)
    }

    /// One whole scripted run against a grid seeded with `seed`.
    async fn transcript(seed: u64) -> Result<Transcript, TestError> {
        let grid = FakeGridBuilder::new()
            .account(AccountConfig::new("Test", "User", "password"))
            .region(RegionConfig::default())
            .deterministic(seed)
            .event_queue_hold(Duration::from_secs(2))
            .start()
            .await?;
        let agent = grid
            .account_agent_id("Test", "User")
            .ok_or("the account was not registered")?;
        let region = grid
            .region_names()
            .first()
            .and_then(|name| grid.region_id(name))
            .ok_or("the region was not registered")?;
        let (session, secure_session, circuit_code, seed_path, caps) =
            minted_over_http(&grid).await?;
        let events = scripted_events(&grid).await?;
        grid.shutdown();
        Ok(Transcript {
            agent,
            region,
            session,
            secure_session,
            circuit_code,
            seed_path,
            caps,
            events,
        })
    }

    /// The acceptance property: two runs of the same scripted end-to-end
    /// session against `deterministic(1)` mint the same identifiers — down
    /// to the capability tokens in the granted URLs — and decode the same
    /// event sequence; a different seed mints a different stream while
    /// answering the same script the same way.
    #[tokio::test]
    async fn two_seeded_runs_produce_the_same_transcript() -> Result<(), TestError> {
        let first = transcript(1).await?;
        let second = transcript(1).await?;
        assert_eq!(first, second, "one seed, one transcript");
        // Not a vacuous transcript: the session really got as far as the
        // arrival burst and the chat line the script sent.
        assert!(
            first.events.contains(&"AgentArrived".to_owned()),
            "the scripted session never arrived: {:?}",
            first.events
        );
        assert!(
            first.events.contains(&"Chat".to_owned()),
            "the scripted session's chat never reached the grid: {:?}",
            first.events
        );

        let other = transcript(2).await?;
        assert_ne!(
            first.session, other.session,
            "a different seed must not repeat the session id"
        );
        assert_ne!(
            first.seed_path, other.seed_path,
            "a different seed must not repeat the capability tokens"
        );
        assert_eq!(
            first.events, other.events,
            "the script's event sequence does not depend on the seed"
        );
        Ok(())
    }
}
