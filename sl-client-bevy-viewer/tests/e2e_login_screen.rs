//! End-to-end logins through the **login screen** a viewer started without
//! credentials shows, on both backends: the grid added by its login URI (the
//! fake grid answers `get_grid_info` with its name, as OpenSim does), the
//! avatar's name and password typed in, and the window handed to the world
//! once the avatar arrives.
//!
//! - **A login.** The page covers the world and hides the permanent chrome
//!   until the agent arrives; then the page is gone and the menu bar is back.
//! - **A wrong password.** The grid's refusal is said on the panel, the
//!   window stays open, and the next try with the right password gets in.
//! - **A second factor.** A challenge shows the token row; the token typed
//!   there is the same login again, and it gets in.
//!
//! The credentials-file road that skips the screen is `e2e_login`'s.

#[cfg(test)]
mod test {
    use core::time::Duration;

    use sl_e2e::{BodyError, Stage, StageBuilder};
    use sl_fake_grid::{ImitatedGrid, RegionConfig};
    use sl_viewer_driver::Viewer;

    /// A failed stage, or a test that could not set one up.
    type TestError = Box<dyn core::error::Error>;

    /// The viewer binary cargo built for this test.
    const VIEWER: &str = env!("CARGO_BIN_EXE_sl-client-bevy-viewer");

    /// How long something the grid has to answer may take.
    const WAIT: Duration = Duration::from_secs(60);

    /// A stage for `name` whose one viewer `Alpha` starts on its login screen,
    /// on a grid imitating Second Life.
    fn stage(name: &str) -> StageBuilder {
        StageBuilder::new(name)
            .viewer_binary(VIEWER)
            .viewer("Alpha")
            .login_screen("Alpha")
            .region(RegionConfig::default())
            .configure_grid(|grid| grid.imitates(ImitatedGrid::SecondLife))
    }

    /// Add the stage's grid on `alpha`'s login screen by its login URI, and
    /// wait until the panel says it was added under the grid's own name.
    async fn add_the_grid(stage: &Stage, alpha: &Viewer) -> Result<(), BodyError> {
        let details = stage.login_details("Alpha")?;
        let _page = alpha
            .expect(&alpha.ui().test_id("login-page"))
            .timeout(WAIT)
            .to_be_visible()
            .await?;
        let _typed = alpha
            .ui()
            .test_id("login-grid-uri:field")
            .fill(&details.login_uri)
            .await?;
        let _added = alpha.ui().test_id("login-add-grid").click().await?;
        let _named = alpha
            .expect(&alpha.ui().test_id("login-notice"))
            .timeout(WAIT)
            .to_contain_text("Fake Grid")
            .await?;
        Ok(())
    }

    /// Type the stage account's name and `password` into `alpha`'s panel and
    /// press Log In.
    async fn log_in(stage: &Stage, alpha: &Viewer, password: &str) -> Result<(), BodyError> {
        let details = stage.login_details("Alpha")?;
        let _name = alpha
            .ui()
            .test_id("login-avatar:field")
            .fill(&details.name)
            .await?;
        let _password = alpha
            .ui()
            .test_id("login-password:field")
            .fill(password)
            .await?;
        let _pressed = alpha.ui().test_id("login-connect").click().await?;
        Ok(())
    }

    /// Wait until `Alpha` has arrived, and the page has given the window to
    /// the world: gone, with the menu bar back.
    async fn arrives(stage: &Stage, alpha: &Viewer) -> Result<(), BodyError> {
        stage.arrive("Alpha").await?;
        let _gone = alpha
            .expect(&alpha.ui().test_id("login-page"))
            .timeout(WAIT)
            .to_be_hidden()
            .await?;
        let _menu = alpha
            .expect(&alpha.ui().test_id("menu-button:menu-bar-avatar"))
            .timeout(WAIT)
            .to_be_visible()
            .await?;
        Ok(())
    }

    /// **A login through the screen**: the grid added by its URI, the account
    /// typed in, the window handed to the world.
    #[test]
    fn the_login_screen_logs_in_and_gives_the_window_to_the_world() -> Result<(), TestError> {
        stage("login_screen").run(async |stage: &Stage| {
            let alpha = &stage.viewer("Alpha")?;
            add_the_grid(stage, alpha).await?;
            // The world's chrome is hidden while the page is up.
            let _hidden = alpha
                .expect(&alpha.ui().test_id("menu-button:menu-bar-avatar"))
                .to_be_hidden()
                .await?;
            log_in(stage, alpha, &stage.login_details("Alpha")?.password).await?;
            arrives(stage, alpha).await
        })?;
        Ok(())
    }

    /// **A wrong password**: refused on the panel, and the next try gets in.
    #[test]
    fn a_wrong_password_is_said_on_the_panel_and_the_next_try_gets_in() -> Result<(), TestError> {
        stage("login_screen_wrong_password").run(async |stage: &Stage| {
            let alpha = &stage.viewer("Alpha")?;
            add_the_grid(stage, alpha).await?;
            log_in(stage, alpha, "not the password").await?;
            let _refused = alpha
                .expect(&alpha.ui().test_id("login-notice"))
                .timeout(WAIT)
                .to_contain_text("The grid refused the login")
                .await?;
            let _still = alpha
                .expect(&alpha.ui().test_id("login-page"))
                .to_be_visible()
                .await?;
            log_in(stage, alpha, &stage.login_details("Alpha")?.password).await?;
            arrives(stage, alpha).await
        })?;
        Ok(())
    }

    /// **A second factor on the screen**: the token row asks, the token gets
    /// in.
    #[test]
    fn a_second_factor_is_asked_for_on_the_panel() -> Result<(), TestError> {
        stage("login_screen_mfa")
            .mfa("Alpha", "314159")
            .run(async |stage: &Stage| {
                let alpha = &stage.viewer("Alpha")?;
                add_the_grid(stage, alpha).await?;
                log_in(stage, alpha, &stage.login_details("Alpha")?.password).await?;
                let token = alpha.ui().test_id("login-token:field");
                let _asked = alpha.expect(&token).timeout(WAIT).to_be_visible().await?;
                let _typed = token.fill("314159").await?;
                let _sent = alpha.ui().test_id("login-submit-token").click().await?;
                arrives(stage, alpha).await
            })?;
        Ok(())
    }
}
