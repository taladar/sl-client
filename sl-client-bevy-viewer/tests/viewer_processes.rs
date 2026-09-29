//! Two real viewer processes, launched and stopped the way the end-to-end
//! stage will launch and stop them (`sl-viewer-launch`): each confined to a
//! directory of its own, both logged into one in-process fake grid as two
//! accounts, then stopped together — `SIGTERM`, the logout grace, and `SIGKILL`
//! only if that fails — and the grid asked afterwards whether either left a
//! session behind.
//!
//! A test here because only the built binary, run as a process, answers it:
//! the question is whether the viewer's own signal handling turns the stop
//! into a logout, and a process that is killed rather than asked strands a
//! session the *next* login trips over.

#[cfg(test)]
mod test {
    use core::time::Duration;
    use std::path::{Path, PathBuf};
    use std::time::Instant;

    use pretty_assertions::assert_eq;
    use sl_fake_grid::fixtures::scenarios;
    use sl_fake_grid::{AccountConfig, FakeAgent, FakeGrid, FakeGridBuilder, RegionConfig};
    use sl_proto::ServerEvent;
    use sl_viewer_launch::{Ending, LOGOUT_GRACE, Launch, RunningViewer, ViewerDir, stop_all};
    use tokio::sync::broadcast;

    /// A boxed error so tests can use `?` instead of the disallowed
    /// `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// The shared first name of the grid's accounts.
    const FIRST: &str = "Launch";
    /// The password every account shares on the loopback grid.
    const PASSWORD: &str = "password";
    /// The `[avatars.<key>]` each viewer's credentials file names.
    const AVATAR_KEY: &str = "e2e";
    /// Long enough for two cold starts and logins on a loaded machine.
    const ARRIVAL: Duration = Duration::from_secs(240);

    /// The directory this test's viewers are confined to, fresh.
    fn run_root() -> Result<PathBuf, TestError> {
        let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("viewer_processes");
        if fs_err::metadata(&root).is_ok() {
            fs_err::remove_dir_all(&root)?;
        }
        fs_err::create_dir_all(&root)?;
        Ok(root)
    }

    /// The last `lines` lines of `log`, for a failure message.
    fn tail(log: &Path, lines: usize) -> String {
        fs_err::read_to_string(log).map_or_else(
            |error| format!("(no log: {error})"),
            |text| {
                let all: Vec<&str> = text.lines().collect();
                all.get(all.len().saturating_sub(lines)..)
                    .unwrap_or_default()
                    .join("\n")
            },
        )
    }

    /// Launch the viewer headless in `dir`, logging into `grid` as
    /// `FIRST last` from a credentials file of its own.
    fn launch(grid: &FakeGrid, dir: &ViewerDir, last: &str) -> Result<Launch, TestError> {
        dir.create()?;
        let credentials = dir.root.join("credentials.toml");
        fs_err::write(
            &credentials,
            format!(
                "default_avatar = \"{AVATAR_KEY}\"\n\n[avatars.{AVATAR_KEY}]\nfirst = \
                 \"{FIRST}\"\nlast = \"{last}\"\npassword = \"{PASSWORD}\"\nlogin_uri = \
                 \"{uri}\"\n",
                uri = grid.login_uri()
            ),
        )?;
        Ok(
            Launch::in_dir(last, env!("CARGO_BIN_EXE_sl-client-bevy-viewer"), dir).args([
                "--credentials".to_owned(),
                credentials.display().to_string(),
                "--avatar".to_owned(),
                AVATAR_KEY.to_owned(),
                "--login-uri".to_owned(),
                grid.login_uri().to_string(),
                "--headless".to_owned(),
                "--disable-web-media".to_owned(),
            ]),
        )
    }

    /// Wait until every one of `agents` holds a session in `region`, failing
    /// early — with its log — when a viewer exits instead.
    fn wait_arrived(
        runtime: &tokio::runtime::Runtime,
        grid: &FakeGrid,
        region: &str,
        viewers: &mut [(RunningViewer, PathBuf)],
        agents: &[sl_proto::AgentKey],
    ) -> Result<Vec<FakeAgent>, TestError> {
        let since = Instant::now();
        loop {
            let here = runtime.block_on(grid.sessions_in(region));
            let arrived: Vec<FakeAgent> = agents
                .iter()
                .filter_map(|agent| {
                    here.iter()
                        .find(|session| session.agent_id() == *agent)
                        .cloned()
                })
                .collect();
            if arrived.len() == agents.len() {
                return Ok(arrived);
            }
            for (viewer, log) in viewers.iter_mut() {
                if let Some(ending) = viewer.try_ending()? {
                    return Err(format!(
                        "{} ended ({ending:?}) before it arrived; its log ends:\n{}",
                        viewer.name(),
                        tail(log, 40)
                    )
                    .into());
                }
            }
            if since.elapsed() > ARRIVAL {
                return Err(format!(
                    "only {} of {} viewers arrived in {region} within {} s",
                    arrived.len(),
                    agents.len(),
                    ARRIVAL.as_secs()
                )
                .into());
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }

    /// Whether `events` carried the session's clean logout.
    fn logged_out(events: &mut broadcast::Receiver<ServerEvent>) -> bool {
        loop {
            match events.try_recv() {
                Ok(ServerEvent::LoggedOut) => return true,
                Ok(_) | Err(broadcast::error::TryRecvError::Lagged(_)) => {}
                Err(_empty_or_closed) => return false,
            }
        }
    }

    /// Two viewer processes log in, are stopped together, and log out: each
    /// ended because it was asked, each session closed by a `LogoutRequest`,
    /// and the region holds no session afterwards.
    #[test]
    fn two_viewer_processes_are_stopped_without_stranding_a_session() -> Result<(), TestError> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;
        let lasts = ["Alpha", "Beta"];
        let scene = scenarios::scenario(scenarios::DEFAULT).ok_or("no stock scenario")?;
        let region = scene.dress(RegionConfig::default());
        let region_name = region.name.clone();
        let mut builder = FakeGridBuilder::new()
            .event_queue_hold(Duration::from_secs(2))
            .region(region);
        for last in lasts {
            builder = builder.account(AccountConfig::new(FIRST, last, PASSWORD));
        }
        let grid = runtime.block_on(builder.start())?;
        let agents = lasts
            .iter()
            .map(|last| {
                grid.account_agent_id(FIRST, last)
                    .ok_or_else(|| format!("no account {FIRST} {last}"))
            })
            .collect::<Result<Vec<_>, _>>()?;

        let root = run_root()?;
        let mut viewers = Vec::new();
        for last in lasts {
            let dir = ViewerDir::new(root.join(last))?;
            let viewer = RunningViewer::spawn(&launch(&grid, &dir, last)?)?;
            viewers.push((viewer, dir.log()));
        }

        let sessions = wait_arrived(&runtime, &grid, &region_name, &mut viewers, &agents)?;
        let mut events: Vec<_> = sessions.iter().map(FakeAgent::events).collect();

        let (running, logs): (Vec<RunningViewer>, Vec<PathBuf>) = viewers.into_iter().unzip();
        let ran = stop_all(running, LOGOUT_GRACE);
        for ((result, log), last) in ran.into_iter().zip(&logs).zip(lasts) {
            let ending = result?.ending;
            assert_eq!(
                ending,
                Ending::AskedToQuit,
                "{last} did not quit when asked; its log ends:\n{}",
                tail(log, 40)
            );
        }
        for ((session, events), last) in sessions.iter().zip(&mut events).zip(lasts) {
            assert!(session.is_closed(), "{last}'s session is still open");
            assert!(
                logged_out(events),
                "{last}'s session closed without a LogoutRequest"
            );
        }
        assert!(
            runtime.block_on(grid.sessions_in(&region_name)).is_empty(),
            "the region still holds a session"
        );
        grid.shutdown();
        Ok(())
    }
}
