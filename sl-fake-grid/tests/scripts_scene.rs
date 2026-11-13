//! The scripts scene played unattended: the grid logs in the scene's visitor,
//! which walks past the sensor and works along the row — touch, menu, chat,
//! pay, rez, read, drop, ping — and ends seated on the pose ball, each prim's
//! script saying what it saw. What the region hears is the whole check: every
//! behaviour of the row answers in local chat, nothing faults, nothing calls a
//! stubbed function, and two runs of one seed hear the same lines.

#[cfg(test)]
mod test {
    use std::time::Duration;

    use pretty_assertions::assert_eq;
    use sl_fake_grid::chat::Line;
    use sl_fake_grid::fixtures::scenarios;
    use sl_fake_grid::fixtures::scripts_scene::{
        DIALOG, LINKSET, LISTENER, NOTECARD_READER, PHYSICAL, POSE_BALL, REZZER, SENSOR, TOUCH,
        VENDOR, visitor, visitor_on,
    };
    use sl_fake_grid::{
        FakeGrid, FakeGridBuilder, HeartbeatConfig, ImitatedGrid, Progress, RegionConfig,
    };
    use sl_proto::ChatType;

    type TestError = Box<dyn core::error::Error>;

    /// How long any single wait in these tests may take.
    const WAIT: Duration = Duration::from_secs(120);

    /// The stock region's name.
    const REGION: &str = "Fake Region";

    /// The seed every run shares.
    const SEED: u64 = 0x5C_5EED;

    /// A grid imitating `imitated`, seeded, its one region the scripts scene
    /// under a heartbeat stepped by hand, holding the visitor for that grid —
    /// the `scripts-avatars` scene itself on Second Life.
    async fn grid(imitated: ImitatedGrid) -> Result<FakeGrid, TestError> {
        let scene = scenarios::scenario("scripts").ok_or("no scripts scene")?;
        let mut config = scene.dress(RegionConfig {
            heartbeat: HeartbeatConfig::stepped(Duration::from_millis(100)),
            ..RegionConfig::default()
        });
        config.scripted_avatars.push(visitor_on(imitated));
        Ok(FakeGridBuilder::new()
            .deterministic(SEED)
            .imitates(imitated)
            .region(config)
            .start()
            .await?)
    }

    /// What one run heard: every line said in the region by a prim, as
    /// `name: message`, and the faults and stubbed calls of its scripts.
    #[derive(Debug, PartialEq)]
    struct Heard {
        /// The prims' lines, in order.
        said: Vec<String>,
        /// Every run-time error, rendered.
        faults: Vec<String>,
        /// Every stubbed call, rendered.
        stubbed: Vec<String>,
    }

    /// Plays the scene on a grid imitating `imitated` until its visitor has
    /// done everything, and a few ticks more for the last lines to land.
    async fn play(imitated: ImitatedGrid) -> Result<Heard, TestError> {
        let grid = grid(imitated).await?;
        let mut said = grid.overhear(REGION)?;
        let mut faults = grid.script_faults();
        let mut stubbed = grid.stubbed_calls();
        let visitor = grid
            .scripted_avatars()
            .first()
            .ok_or("the grid logged in no avatar")?
            .clone();
        let arrived = tokio::time::timeout(WAIT, visitor.arrived()).await?;
        if arrived != Progress::Running {
            return Err(format!("the visitor did not arrive: {arrived:?}").into());
        }
        tokio::time::timeout(WAIT, async {
            loop {
                match visitor.progress() {
                    Progress::Finished => return Ok(()),
                    Progress::Stopped(reason) => return Err(reason),
                    _other => {}
                }
                grid.advance_one_tick(REGION)
                    .await
                    .map_err(|error| error.to_string())?;
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .map_err(|_elapsed| {
            format!(
                "the visitor never finished; it got as far as {:#?}",
                visitor.transcript()
            )
        })??;
        for _ in 0..5 {
            let _tick = grid.advance_one_tick(REGION).await?;
        }
        let mut heard = Heard {
            said: Vec::new(),
            faults: Vec::new(),
            stubbed: Vec::new(),
        };
        while let Ok(line) = said.try_recv() {
            if is_a_prims(&line) {
                heard
                    .said
                    .push(format!("{}: {}", line.from_name, line.message));
            }
        }
        while let Ok(fault) = faults.try_recv() {
            heard.faults.push(format!("{fault:?}"));
        }
        while let Ok(call) = stubbed.try_recv() {
            heard.stubbed.push(format!("{call:?}"));
        }
        grid.shutdown();
        Ok(heard)
    }

    /// Whether `line` is a prim's local chat, rather than an avatar's or a
    /// typing notice.
    fn is_a_prims(line: &Line) -> bool {
        matches!(line.source, sl_proto::ChatSource::Object(_))
            && !matches!(line.chat_type, ChatType::StartTyping | ChatType::StopTyping)
    }

    /// The lines each behaviour of the row answers the visitor with, in the
    /// order it visits them: each is what that prim's script says, so a
    /// behaviour that stopped working is missing here by name.
    fn expected(imitated: ImitatedGrid) -> Vec<String> {
        let visitor = "Scripted Visitor";
        let paid = imitated == ImitatedGrid::SecondLife;
        [
            (SENSOR, format!("{visitor} came near")),
            (SENSOR, format!("{visitor} went away")),
            (DIALOG, format!("{visitor} chose Green")),
            (LISTENER, format!("Hello, {visitor}!")),
            (LISTENER, format!("Spinning for {visitor}")),
            (VENDOR, format!("Thank you {visitor} for L$50")),
            ("Spark", "Spark 1 lit".to_owned()),
            (
                NOTECARD_READER,
                "Line 1: Welcome to the scripts scene.".to_owned(),
            ),
            (
                NOTECARD_READER,
                "Line 2: Every prim here runs a script.".to_owned(),
            ),
            (
                NOTECARD_READER,
                "Line 3: Touch them, sit on them, pay them.".to_owned(),
            ),
            (NOTECARD_READER, "End of Greetings".to_owned()),
            (PHYSICAL, "Bumped Plinth".to_owned()),
            (LINKSET, format!("Link 2 answered for {visitor}")),
            (LINKSET, format!("Link 3 answered for {visitor}")),
            (POSE_BALL, format!("Posing {visitor}")),
        ]
        .into_iter()
        .filter(|(name, _message)| paid || *name != VENDOR)
        .map(|(name, message)| format!("{name}: {message}"))
        .collect()
    }

    /// Every behaviour of the row answered, in the order the visitor visited
    /// them, with no fault and no stubbed call; the touch prim's line names
    /// the visitor, the rezzer heard of its spark (before or after the spark
    /// spoke, which is each grid's own ordering), and the physical box had
    /// already landed once before the visitor dropped it again.
    fn check(heard: &Heard, imitated: ImitatedGrid) {
        assert_eq!(heard.faults, Vec::<String>::new(), "a script faulted");
        assert_eq!(heard.stubbed, Vec::<String>::new(), "a stub was called");
        let mut wanted = expected(imitated).into_iter().peekable();
        for line in &heard.said {
            if wanted.peek() == Some(line) {
                let _matched = wanted.next();
            }
        }
        let missing: Vec<String> = wanted.collect();
        assert_eq!(
            missing,
            Vec::<String>::new(),
            "lines not heard in order; the region heard {:#?}",
            heard.said
        );
        assert!(
            heard
                .said
                .iter()
                .any(|line| line.starts_with(&format!("{TOUCH}: Scripted Visitor touched face"))),
            "the touch prim did not name its toucher: {:#?}",
            heard.said
        );
        assert!(
            heard.said.contains(&format!("{REZZER}: Rezzed Spark")),
            "the rezzer did not hear of its spark: {:#?}",
            heard.said
        );
        assert!(
            heard
                .said
                .iter()
                .filter(|line| *line == &format!("{PHYSICAL}: Bumped Plinth"))
                .count()
                >= 2,
            "the physical box did not land both times: {:#?}",
            heard.said
        );
    }

    /// The row, on a grid imitating Second Life, and the same lines from a
    /// second run of the seed.
    #[tokio::test]
    async fn the_visitor_exercises_every_behaviour_on_second_life() -> Result<(), TestError> {
        let first = play(ImitatedGrid::SecondLife).await?;
        check(&first, ImitatedGrid::SecondLife);
        let second = play(ImitatedGrid::SecondLife).await?;
        assert_eq!(first, second, "two runs of one seed heard different lines");
        Ok(())
    }

    /// The row, on a grid imitating OpenSim.
    #[tokio::test]
    async fn the_visitor_exercises_every_behaviour_on_opensim() -> Result<(), TestError> {
        check(&play(ImitatedGrid::OpenSim).await?, ImitatedGrid::OpenSim);
        Ok(())
    }

    /// The named `scripts-avatars` scene is the scripts scene and its visitor:
    /// the region the tests above play, as the binary and a harness start it.
    #[test]
    fn the_named_scene_brings_the_visitor() -> Result<(), TestError> {
        let scene = scenarios::scenario("scripts-avatars").ok_or("no scripts-avatars scene")?;
        let config = scene.dress(RegionConfig::default());
        assert_eq!(
            format!("{:?}", config.scripted_avatars),
            format!("{:?}", vec![visitor()])
        );
        Ok(())
    }

    /// The prims come up and run with nobody there: the `scripts` scene logs
    /// in no avatar, and the physical box falls on to its plinth by itself.
    #[tokio::test]
    async fn the_scene_runs_with_nobody_in_it() -> Result<(), TestError> {
        let scene = scenarios::scenario("scripts").ok_or("no scripts scene")?;
        let config = scene.dress(RegionConfig {
            heartbeat: HeartbeatConfig::stepped(Duration::from_millis(100)),
            ..RegionConfig::default()
        });
        let grid = FakeGridBuilder::new()
            .deterministic(SEED)
            .region(config)
            .start()
            .await?;
        assert!(
            grid.scripted_avatars().is_empty(),
            "an avatar was logged in"
        );
        let mut said = grid.overhear(REGION)?;
        let mut landed = false;
        for _ in 0..60 {
            let _tick = grid.advance_one_tick(REGION).await?;
            while let Ok(line) = said.try_recv() {
                landed |= is_a_prims(&line) && line.message == "Bumped Plinth";
            }
            if landed {
                break;
            }
        }
        grid.shutdown();
        assert!(landed, "the physical box never landed on its plinth");
        Ok(())
    }
}
