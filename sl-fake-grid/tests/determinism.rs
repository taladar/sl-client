//! Two grids built from the same seed and the same content mint the same
//! identifiers and answer a scripted session the same way — the property
//! that makes an offline scenario's records comparable run to run. And once
//! the region runs scripts — a timer, `llFrand`, `llGenerateKey`,
//! `llListRandomize` and two scripts talking — what a client sees of them is
//! the same twice from one seed, and different from another
//! (`book/src/simulator/determinism.md`).

#[cfg(test)]
mod test {
    use std::time::Duration;

    use pretty_assertions::{assert_eq, assert_ne};
    use sl_client_tokio::{
        ChatChannel, ChatType, Client, Command, Event, LoginParams, LoginRequest, StartLocation,
    };
    use sl_fake_grid::{
        AccountConfig, FakeGrid, FakeGridBuilder, HeartbeatConfig, ImitatedGrid, RegionConfig,
    };
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

    /// When a wait for one thing among other events gives up: three `WAIT`s
    /// from now, however many of those others arrive meanwhile.
    fn whole_wait() -> tokio::time::Instant {
        let now = tokio::time::Instant::now();
        now.checked_add(WAIT.saturating_mul(3)).unwrap_or(now)
    }

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

        // One deadline for the whole wait: a timeout on each event is
        // never reached on a stream that keeps sending something else.
        let deadline = whole_wait();
        loop {
            let event = tokio::time::timeout_at(deadline, event_rx.recv())
                .await?
                .ok_or("client event stream ended early")?;
            if matches!(
                event,
                Event::RegionHandshakeComplete | Event::RegionChanged { .. }
            ) {
                break;
            }
        }
        // Keep reading: the client's run loop waits on this channel, so a
        // session whose events go unread stops at the 256th — which the
        // region's ground alone outnumbers — and never sends the chat.
        let drain = tokio::spawn(async move { while event_rx.recv().await.is_some() {} });
        command_tx
            .send(Command::Chat {
                message: "hello twice".to_owned(),
                chat_type: ChatType::Normal,
                channel: ChatChannel(0),
            })
            .await?;
        let mut events = Vec::new();
        let deadline = whole_wait();
        loop {
            let event = tokio::time::timeout_at(deadline, server_events.recv()).await??;
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
        drain.abort();
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

    /// The ticking box's script: every 0.3 s, five times, a line to the
    /// avatars with a roll of `llFrand` and a key of `llGenerateKey`, the
    /// roll said on to the answering box on a hidden channel, and the prim's
    /// allow-drop flag flipped — an object update.
    const TICKER_SOURCE: &str = r#"integer n;

default
{
    state_entry()
    {
        llSetTimerEvent(0.3);
    }

    timer()
    {
        ++n;
        float roll = llFrand(100.0);
        llSay(0, "tick " + (string)n + " roll " + (string)roll
            + " key " + (string)llGenerateKey());
        llSay(-42, (string)n + " " + (string)roll);
        llAllowInventoryDrop(n % 2);
        if (n == 5)
        {
            llSetTimerEvent(0.0);
        }
    }
}
"#;

    /// The answering box's script: what it hears on the hidden channel, said
    /// on with a shuffle of `llListRandomize` and a roll of its own, and
    /// `done` after the fifth.
    const ANSWER_SOURCE: &str = r#"default
{
    state_entry()
    {
        llListen(-42, "", NULL_KEY, "");
    }

    listen(integer channel, string name, key id, string message)
    {
        list shuffled = llListRandomize(["a", "b", "c", "d", "e"], 1);
        llSay(0, "heard " + message + " from " + name
            + " shuffled " + llDumpList2String(shuffled, "")
            + " roll " + (string)llFrand(1.0));
        if (llList2Integer(llParseString2List(message, [" "], []), 0) == 5)
        {
            llSay(0, "done");
        }
    }
}
"#;

    /// The boxes' region-local ids: the ticking one, then the answering one.
    const BOXES: [sl_proto::RegionLocalObjectId; 2] = [
        sl_proto::RegionLocalObjectId(0x600),
        sl_proto::RegionLocalObjectId(0x601),
    ];

    /// The full id of the box `local_id`.
    fn box_key(local_id: sl_proto::RegionLocalObjectId) -> sl_types::key::ObjectKey {
        sl_types::key::ObjectKey::from(uuid::Uuid::from_u128((u128::from(local_id.0) << 8) | 0x5))
    }

    /// A region holding the two boxes beside the arrival point, 2 m apart,
    /// each with its one script running.
    fn talking_region() -> Result<sl_fake_grid::RegionFixture, TestError> {
        let owner = sl_types::key::AgentKey::from(uuid::Uuid::from_u128(0xB0C0));
        let mut fixture = sl_fake_grid::RegionFixture::new();
        for ((index, local_id), source) in (0_u16..).zip(BOXES).zip([TICKER_SOURCE, ANSWER_SOURCE])
        {
            let id = u128::from(local_id.0) << 8;
            let key = box_key(local_id);
            let asset = sl_proto::AssetKey::from(uuid::Uuid::from_u128(id | 0x6));
            fixture.world.objects.push(sl_fake_grid::world::box_prim(
                local_id,
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
            fixture
                .world
                .add_task_inventory(local_id, contents)
                .map_err(|_contents| "the box is not in the region")?;
            let _previous = fixture.assets.insert(asset, source.as_bytes().to_vec());
        }
        Ok(fixture)
    }

    /// The region's name in [`talking_stream`]'s grid.
    const TALKING_REGION: &str = "Fake Region";

    /// How many ticks [`talking_stream`] runs: the ticker's five 0.3 s
    /// timers are fifteen 0.1 s ticks, the answer's last line one more.
    const TALKING_TICKS: u32 = 24;

    /// How long the client must hear nothing more before a stream counts as
    /// whole.
    const QUIET: Duration = Duration::from_millis(500);

    /// Everything a client logged into a grid imitating `imitated` and seeded
    /// with `seed` is told of the two talking boxes over [`TALKING_TICKS`]
    /// stepped ticks: every line of chat and every object update of either
    /// box, in the order it arrived, each as its whole decoded value.
    async fn talking_stream(imitated: ImitatedGrid, seed: u64) -> Result<Vec<String>, TestError> {
        let region = talking_region()?.into_region(RegionConfig {
            heartbeat: HeartbeatConfig::stepped(Duration::from_millis(100)),
            ..RegionConfig::default()
        });
        let grid = FakeGridBuilder::new()
            .imitates(imitated)
            .account(AccountConfig::new("Test", "User", "password"))
            .region(region)
            .deterministic(seed)
            .start()
            .await?;
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
        // Read every event, or the client stops at the 256th unread one; keep
        // what is said and what is sent of the boxes.
        let boxes = BOXES.map(box_key);
        let (seen_tx, mut seen) = mpsc::unbounded_channel::<String>();
        let forward = tokio::spawn(async move {
            while let Some(event) = event_rx.recv().await {
                let kept = match event {
                    Event::ChatReceived(chat) => format!("chat {chat:?}"),
                    Event::ObjectAdded(object) | Event::ObjectUpdated(object)
                        if boxes.contains(&object.full_id) =>
                    {
                        format!("object {object:?}")
                    }
                    _other => continue,
                };
                if seen_tx.send(kept).is_err() {
                    break;
                }
            }
        });
        tokio::time::timeout(WAIT, async {
            loop {
                if matches!(server_events.recv().await?, ServerEvent::AgentArrived) {
                    return Ok::<(), tokio::sync::broadcast::error::RecvError>(());
                }
            }
        })
        .await??;
        let mut stream = Vec::new();
        // Both boxes are in view before the region runs a tick, so nothing a
        // tick sends can overtake the arrival's own updates of them.
        let deadline = whole_wait();
        while stream
            .iter()
            .filter(|kept: &&String| kept.starts_with("object "))
            .count()
            < BOXES.len()
        {
            let kept = tokio::time::timeout_at(deadline, seen.recv())
                .await?
                .ok_or("the client stopped")?;
            stream.push(kept);
        }
        for _ in 0..TALKING_TICKS {
            let _tick = grid.advance_one_tick(TALKING_REGION).await?;
        }
        let deadline = whole_wait();
        loop {
            match tokio::time::timeout(QUIET, seen.recv()).await {
                Ok(Some(kept)) => stream.push(kept),
                Ok(None) => return Err("the client stopped".into()),
                Err(_quiet) if stream.iter().any(|kept| kept.contains("message: \"done\"")) => {
                    break;
                }
                Err(_quiet) if tokio::time::Instant::now() >= deadline => {
                    return Err(format!("the boxes never said done: {stream:#?}").into());
                }
                Err(_quiet) => {}
            }
        }
        drop(command_tx);
        run.abort();
        forward.abort();
        grid.shutdown();
        Ok(stream)
    }

    /// The contract's guard: a region running a timer, `llFrand`,
    /// `llGenerateKey`, `llListRandomize` and two scripts talking tells a
    /// client the same lines and the same object updates, in the same order,
    /// on two runs from one seed — on both flavours — and other rolls from
    /// another seed, in a stream of the same shape.
    #[tokio::test]
    async fn scripts_tell_a_client_the_same_twice_from_one_seed() -> Result<(), TestError> {
        for imitated in [ImitatedGrid::SecondLife, ImitatedGrid::OpenSim] {
            let first = talking_stream(imitated, 1).await?;
            let second = talking_stream(imitated, 1).await?;
            assert_eq!(first, second, "{imitated:?}: one seed, one stream");
            let said = |prefix: &str| {
                first
                    .iter()
                    .filter(|kept| kept.contains(&format!("message: \"{prefix}")))
                    .count()
            };
            assert_eq!(said("tick "), 5, "{imitated:?}: {first:#?}");
            assert_eq!(said("heard "), 5, "{imitated:?}: {first:#?}");
            assert_eq!(said("done"), 1, "{imitated:?}: {first:#?}");
            // The flipped flag reached the client: an update of the ticker
            // after the arrival's.
            let [ticking, _answering] = BOXES;
            let ticker = format!("{:?}", box_key(ticking));
            assert!(
                first
                    .iter()
                    .filter(|kept| kept.starts_with("object ") && kept.contains(&ticker))
                    .count()
                    > 1,
                "{imitated:?}: {first:#?}"
            );
        }
        let one = talking_stream(ImitatedGrid::SecondLife, 1).await?;
        let other = talking_stream(ImitatedGrid::SecondLife, 2).await?;
        assert_ne!(one, other, "another seed, other rolls");
        assert_eq!(one.len(), other.len(), "the same shape from either seed");
        Ok(())
    }
}
