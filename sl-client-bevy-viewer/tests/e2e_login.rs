//! End-to-end logins on the fake grid, through the automation driver alone:
//! what a viewer has to get through between a password and a region, on both
//! backends.
//!
//! - **Both flavours.** The two answer a login differently in ways the viewer
//!   has to survive (`book/src/gridspec/login.md`): a Second-Life-flavoured
//!   grid trims the response to the `options` list, sends a benefits package
//!   and a maturity preference and no `home`; an OpenSim-flavoured one ignores
//!   the list, sends `home` and no benefits. Both name the Library's owner,
//!   which is who the viewer fetches the Library's contents as — so the test
//!   opens the Library and waits for its item.
//! - **A second factor.** An account behind MFA is answered with a challenge
//!   first; the viewer answers it with the token and logs in.
//! - **Terms of service and a critical message.** A grid that holds the login
//!   for either is passed, because the viewer's request already carries
//!   `agree_to_tos` and `read_critical`. That is today's behaviour, not the
//!   reference's — Firestorm sends both `false` and shows the text first — and
//!   [[viewer-login-tos]] changes it, at which point this test has to show and
//!   accept the dialogs instead.
//! - **A refusal.** A login the grid declines ends the run with the grid's own
//!   reason and text — each flavour's measured words for an avatar it believes
//!   is already logged in.
//! - **A second login from elsewhere.** Both live grids kick the session an
//!   avatar already had when its account logs in again, and differ in what they
//!   tell the newcomer: Second Life admits it, OpenSim refuses it. The viewer
//!   that was kicked exits cleanly either way. That too is today's behaviour:
//!   [[viewer-disconnect-screen]] keeps the window open and shows the reason.

#[cfg(test)]
mod test {
    use core::time::Duration;

    use serde_json::json;
    use sl_automation_proto::{InventoryRoot, Locator, Probe, Role};
    use sl_client_tokio::{Client, LoginParams, LoginRequest, StartLocation};
    use sl_e2e::{BodyError, Stage, StageBuilder, StageError};
    use sl_fake_grid::{ImitatedGrid, RegionConfig, SecondLogin};
    use sl_proto::{LoginGates, ServerEvent};
    use sl_viewer_driver::Viewer;

    /// A failed stage, or a test that could not set one up.
    type TestError = Box<dyn core::error::Error>;

    /// The viewer binary cargo built for this test.
    const VIEWER: &str = env!("CARGO_BIN_EXE_sl-client-bevy-viewer");

    /// How long something the grid has to answer may take.
    const WAIT: Duration = Duration::from_secs(60);

    /// A stage for `name` with the one viewer `Alpha` on a grid imitating
    /// `flavour`.
    fn stage(name: &str, flavour: ImitatedGrid) -> StageBuilder {
        StageBuilder::new(name)
            .viewer_binary(VIEWER)
            .viewer("Alpha")
            .region(RegionConfig::default())
            .configure_grid(move |grid| grid.imitates(flavour))
    }

    /// Wait until `alpha` stands in a region.
    async fn arrived(alpha: &Viewer) -> Result<(), BodyError> {
        let _arrived = alpha
            .expect_state(Probe::Agent)
            .at("/region")
            .timeout(WAIT)
            .to_be_present()
            .await?;
        Ok(())
    }

    /// Open the inventory, expand the Library and wait until it lists its
    /// item: the login named the Library, its owner and its skeleton, and the
    /// viewer fetched its contents.
    async fn library_loads(alpha: &Viewer) -> Result<(), BodyError> {
        let (_wearable_type, library_item, _asset) = sl_test_assets::builtin::DEFAULT_BODY_PARTS
            .first()
            .copied()
            .ok_or("no library body part")?;
        let inventory = alpha.ui().window("inventory");
        alpha.press("Ctrl+I").await?;
        let _shown = alpha.expect(&inventory).to_be_visible().await?;
        let _expanded = inventory
            .get(Locator::role(Role::TreeItem).named("Library"))
            .timeout(WAIT)
            .double_click()
            .await?;
        let _listed = alpha
            .expect_state(Probe::Inventory {
                root: InventoryRoot::Library,
                path: Vec::new(),
            })
            .at("/items")
            .timeout(WAIT)
            .to_include(json!([{ "name": library_item }]))
            .await?;
        Ok(())
    }

    /// **A Second-Life-flavoured login**: options honoured, no `home`.
    #[test]
    fn a_second_life_flavoured_login_loads_the_library() -> Result<(), TestError> {
        stage("login_second_life", ImitatedGrid::SecondLife).run(async |stage: &Stage| {
            let alpha = &stage.viewer("Alpha")?;
            arrived(alpha).await?;
            library_loads(alpha).await
        })?;
        Ok(())
    }

    /// **An OpenSim-flavoured login**: everything sent, no benefits package.
    #[test]
    fn an_opensim_flavoured_login_loads_the_library() -> Result<(), TestError> {
        stage("login_opensim", ImitatedGrid::OpenSim).run(async |stage: &Stage| {
            let alpha = &stage.viewer("Alpha")?;
            arrived(alpha).await?;
            library_loads(alpha).await
        })?;
        Ok(())
    }

    /// **A second factor**: the account is challenged, the viewer answers
    /// with the token and is let in. Second Life only — OpenSim has no MFA.
    #[test]
    fn a_login_behind_a_second_factor_answers_the_challenge() -> Result<(), TestError> {
        stage("login_mfa", ImitatedGrid::SecondLife)
            .mfa("Alpha", "314159")
            .run(async |stage: &Stage| arrived(&stage.viewer("Alpha")?).await)?;
        Ok(())
    }

    /// **Terms of service and a critical message**: a grid that holds the
    /// login for both lets this viewer in, because it agrees to both up front
    /// (see the module docs for why that is a stand-in).
    #[test]
    fn a_login_held_for_terms_and_a_critical_message_gets_through() -> Result<(), TestError> {
        StageBuilder::new("login_tos_critical")
            .viewer_binary(VIEWER)
            .viewer("Alpha")
            .region(RegionConfig::default())
            .configure_grid(|grid| {
                grid.imitates(ImitatedGrid::SecondLife).gates(LoginGates {
                    tos_message: Some("The terms of service changed.".to_owned()),
                    critical_message: Some("A critical message.".to_owned()),
                    ..LoginGates::default()
                })
            })
            .run(async |stage: &Stage| arrived(&stage.viewer("Alpha")?).await)?;
        Ok(())
    }

    /// A stage whose one account the grid believes is already logged in, on a
    /// grid imitating `flavour`: the run must end in that flavour's refusal,
    /// reason and text.
    fn refused_as_already_logged_in(name: &str, flavour: ImitatedGrid) -> Result<(), TestError> {
        let outcome = StageBuilder::new(name)
            .viewer_binary(VIEWER)
            .viewer("Alpha")
            .region(RegionConfig::default())
            .configure_grid(move |grid| grid.imitates(flavour).stale_presence())
            .run(async |_stage: &Stage| Ok(()));
        let Err(StageError::Login { reason, .. }) = outcome else {
            return Err(format!("the login was not refused: {outcome:?}").into());
        };
        let expected = format!(
            "presence ({})",
            flavour.login_refusals().already_logged_in_message
        );
        if reason != expected {
            return Err(format!("refused with {reason:?}, not {expected:?}").into());
        }
        Ok(())
    }

    /// **A refusal on Second Life's flavour** reaches the run's outcome word
    /// for word.
    #[test]
    fn a_second_life_flavoured_refusal_is_reported_verbatim() -> Result<(), TestError> {
        refused_as_already_logged_in("login_refused_second_life", ImitatedGrid::SecondLife)
    }

    /// **A refusal on OpenSim's flavour**, in OpenSim's words.
    #[test]
    fn an_opensim_flavoured_refusal_is_reported_verbatim() -> Result<(), TestError> {
        refused_as_already_logged_in("login_refused_opensim", ImitatedGrid::OpenSim)
    }

    /// Log `Alpha`'s account in a second time, from a client of the test's
    /// own, while the viewer is in world; hold the grid to its flavour's answer
    /// and the viewer to leaving cleanly.
    async fn log_in_from_elsewhere(stage: &Stage, flavour: ImitatedGrid) -> Result<(), BodyError> {
        arrived(&stage.viewer("Alpha")?).await?;
        let mut heard = stage.agent("Alpha").await?.events();
        // The viewer is about to be thrown out and exit of its own accord.
        stage.expect_quit("Alpha")?;
        let second = Client::connect(LoginParams {
            login_uri: stage.grid()?.login_uri(),
            request: LoginRequest::new(
                sl_e2e::FIRST_NAME,
                "Alpha",
                sl_e2e::PASSWORD,
                StartLocation::Last,
                "sl-e2e-elsewhere",
                "0.0",
            ),
        })
        .await;
        match (flavour.login_refusals().second_login, second) {
            (SecondLogin::Admitted, Ok(client)) => {
                // The newcomer must not outlive the stage: log it out again.
                let (events, mut drained) = tokio::sync::mpsc::channel(256);
                let (diagnostics, _unread) = tokio::sync::mpsc::channel(1);
                let (commands, inbox) = tokio::sync::mpsc::channel(4);
                let run = tokio::spawn(client.run(events, diagnostics, inbox));
                commands.send(sl_client_tokio::Command::Logout).await?;
                drop(commands);
                while drained.recv().await.is_some() {}
                run.await??;
            }
            (SecondLogin::Refused, Err(sl_client_tokio::Error::LoginRejected { failure, .. }))
                if failure.reason == "presence" => {}
            (expected, got) => {
                return Err(format!(
                    "the second login should have been {expected:?}, and was {:?}",
                    got.map(|_client| "admitted")
                )
                .into());
            }
        }
        // The grid ended the viewer's session rather than the viewer leaving.
        let ended = tokio::time::timeout(WAIT, async {
            loop {
                match heard.recv().await {
                    Ok(ServerEvent::Disconnected) => break true,
                    Ok(ServerEvent::LoggedOut) | Err(_) => break false,
                    Ok(_other) => {}
                }
            }
        })
        .await?;
        if !ended {
            return Err("the session the viewer had was not kicked".into());
        }
        Ok(())
    }

    /// **A second login on Second Life's flavour** is admitted, and the viewer
    /// already in world is kicked and exits.
    #[test]
    fn a_second_life_flavoured_second_login_kicks_the_viewer() -> Result<(), TestError> {
        stage("login_elsewhere_second_life", ImitatedGrid::SecondLife).run(
            async |stage: &Stage| log_in_from_elsewhere(stage, ImitatedGrid::SecondLife).await,
        )?;
        Ok(())
    }

    /// **A second login on OpenSim's flavour** is refused, and kicks the
    /// viewer all the same.
    #[test]
    fn an_opensim_flavoured_second_login_kicks_the_viewer() -> Result<(), TestError> {
        stage("login_elsewhere_opensim", ImitatedGrid::OpenSim)
            .run(async |stage: &Stage| log_in_from_elsewhere(stage, ImitatedGrid::OpenSim).await)?;
        Ok(())
    }
}
