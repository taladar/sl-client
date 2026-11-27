//! End-to-end logins through the **login screen** a viewer started without
//! credentials shows, on both backends: the grid added by its login URI (the
//! fake grid answers `get_grid_info` with its name, as OpenSim does), the
//! avatar's name and password typed in, and the window handed to the world
//! once the avatar arrives.
//!
//! - **A login.** The page covers the world and hides the permanent chrome
//!   until the agent arrives; then the page is gone and the menu bar is back.
//!   Before it, the pre-login menu opens Preferences over the page and its
//!   Close Window closes it again.
//! - **A wrong password.** The grid's refusal is said on the panel, in the
//!   viewer's own string for the localization key Second Life sent, with the
//!   refusal's incident id beside it; the window stays open, and the next try
//!   with the right password gets in.
//! - **A second factor.** A challenge shows the token row; the token typed
//!   there is the same login again, and it gets in.
//! - **The keyboard alone.** The name field has the keyboard when the page
//!   opens and Log In is greyed until there is a name and a password; `Enter`
//!   logs in, a refused password comes back selected, so one `Backspace`
//!   clears it, and the next `Enter` gets in.
//! - **A remembered password.** With Remember password ticked, the login that
//!   arrives keeps its password in the stage's in-memory secret store; the
//!   next session fills the field from it, and Log In alone gets in.
//!
//! The credentials-file road that skips the screen is `e2e_login`'s.

#[cfg(test)]
mod test {
    use core::time::Duration;

    use sl_e2e::{Backend, BodyError, Stage, StageBuilder};
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
            // The pre-login menu: Preferences over the page, and Close Window
            // closing it.
            let preferences = alpha.ui().test_id("floater:preferences");
            let _opened = alpha
                .menu_path(&["login-menu-viewer", "menu-bar-preferences"])
                .await?;
            let _shown = alpha.expect(&preferences).to_be_visible().await?;
            let _closed = alpha
                .menu_path(&["login-menu-viewer", "login-menu-close-window"])
                .await?;
            let _gone = alpha.expect(&preferences).to_be_hidden().await?;
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
            // Said from the viewer's own string for Second Life's
            // `LoginFailedAuthenticationFailed`, with the refusal's incident
            // id beside it to copy. (Fluent isolates the inserted text, so the
            // two halves are matched apart.)
            let _refused = alpha
                .expect(&alpha.ui().test_id("login-notice"))
                .timeout(WAIT)
                .to_contain_text("The grid refused the login")
                .await?;
            let _worded = alpha
                .expect(&alpha.ui().test_id("login-notice"))
                .to_contain_text("Please check to make sure you entered the right")
                .await?;
            let _incident = alpha
                .expect(&alpha.ui().test_id("login-incident"))
                .to_contain_text("1-")
                .await?;
            let _row = alpha
                .expect(&alpha.ui().test_id("login-incident-row"))
                .to_be_visible()
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

    /// **The keyboard alone**: the panel opens with the keyboard in the name
    /// field and Log In greyed, `Enter` logs in, and a refused password comes
    /// back selected for the next try.
    #[test]
    fn the_panel_is_worked_from_the_keyboard() -> Result<(), TestError> {
        stage("login_screen_keyboard").run(async |stage: &Stage| {
            let alpha = &stage.viewer("Alpha")?;
            let details = stage.login_details("Alpha")?;
            let name = alpha.ui().test_id("login-avatar:field");
            let password = alpha.ui().test_id("login-password:field");
            let connect = alpha.ui().test_id("login-connect");
            let _page = alpha
                .expect(&alpha.ui().test_id("login-page"))
                .timeout(WAIT)
                .to_be_visible()
                .await?;
            let _keyboard = alpha.expect(&name).to_be_focused().await?;
            let _greyed = alpha.expect(&connect).to_be_disabled().await?;
            add_the_grid(stage, alpha).await?;
            let _named = name.fill(&details.name).await?;
            let _still_greyed = alpha.expect(&connect).to_be_disabled().await?;
            let _typed = password.fill("not the password").await?;
            let _ready = alpha.expect(&connect).to_be_enabled().await?;
            alpha.press("Enter").await?;
            let _refused = alpha
                .expect(&alpha.ui().test_id("login-notice"))
                .timeout(WAIT)
                .to_contain_text("The grid refused the login")
                .await?;
            // The refused password has the keyboard, selected: one Backspace
            // empties it, which greys Log In again.
            let _back = alpha
                .expect(&password)
                .timeout(WAIT)
                .to_be_focused()
                .await?;
            alpha.press("Backspace").await?;
            let _cleared = alpha.expect(&password).to_have_text("").await?;
            let _greyed_again = alpha.expect(&connect).to_be_disabled().await?;
            let _right = password.fill(&details.password).await?;
            alpha.press("Enter").await?;
            arrives(stage, alpha).await
        })?;
        Ok(())
    }

    /// **A second factor on the screen**: the token row asks, with the
    /// keyboard in it, and the token gets in.
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
                let _keyboard = alpha.expect(&token).to_be_focused().await?;
                let _typed = token.fill("314159").await?;
                let _sent = alpha.ui().test_id("login-submit-token").click().await?;
                arrives(stage, alpha).await
            })?;
        Ok(())
    }

    /// **A remembered password**: ticked, the login that arrives keeps it,
    /// and the next session fills it in, so Log In alone gets in. In process
    /// only: a process viewer's in-memory store ends with its process, while
    /// the in-process stage's outlives each session, as a keychain does.
    #[test]
    fn a_remembered_password_logs_in_without_typing_it() -> Result<(), TestError> {
        stage("login_screen_remember")
            .backends([Backend::InProcess])
            .run(async |stage: &Stage| {
                let alpha = &stage.viewer("Alpha")?;
                let details = stage.login_details("Alpha")?;
                add_the_grid(stage, alpha).await?;
                let remember = alpha.ui().test_id("login-remember:checkbox");
                let _ticked = remember.click().await?;
                let _checked = alpha.expect(&remember).to_be_checked().await?;
                log_in(stage, alpha, &details.password).await?;
                arrives(stage, alpha).await?;

                stage.log_out_viewer("Alpha").await?;
                let alpha = &stage.log_in_again("Alpha").await?;
                let bullets: String = details.password.chars().map(|_| '•').collect();
                let _filled = alpha
                    .expect(&alpha.ui().test_id("login-password:field"))
                    .timeout(WAIT)
                    .to_have_text(&bullets)
                    .await?;
                let _pressed = alpha.ui().test_id("login-connect").click().await?;
                arrives(stage, alpha).await
            })?;
        Ok(())
    }
}
