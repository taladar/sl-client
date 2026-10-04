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

#[cfg(test)]
mod test {
    use core::time::Duration;

    use serde_json::json;
    use sl_automation_proto::{InventoryRoot, Locator, Probe, Role};
    use sl_e2e::{BodyError, Stage, StageBuilder};
    use sl_fake_grid::{ImitatedGrid, RegionConfig};
    use sl_proto::LoginGates;
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
}
