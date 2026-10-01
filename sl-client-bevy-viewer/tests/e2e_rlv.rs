//! End-to-end tests for the RLVa console and windows
//! ([[test-e2e-sweep-rlv]]): the console runs a typed `@`-command exactly as a
//! worn object's would, so each test types commands into it on one fake-grid
//! viewer and reads back the console's report line and the state the command
//! changed — through the windows, the menu, the environment and the agent.
//!
//! - the RLVa menu is greyed until the master switch turns RLV on; the console
//!   reports each command of a line; the Restrictions and Locks windows list
//!   what the console holds; closing the console lifts all of it;
//! - the Strings window edits a string, keeps the edit across picks and puts
//!   the default back;
//! - `@setenv_ambient` turns the sky red and `@getenv_ambient` reads it back,
//!   World ▸ Environment ▸ Use Shared Environment takes it back, and `@setrot`
//!   turns the avatar;
//! - `@setenv=n` greys the user's environment menu, and turning
//!   `RestrainedLoveNoSetEnv` on in the debug-settings editor releases it and
//!   refuses the next one while `@setenv_*` still applies.

#[cfg(test)]
mod test {
    use core::time::Duration;

    use serde_json::json;
    use sl_automation_proto::{Locator, Probe, Role};
    use sl_e2e::{BodyError, Stage, StageBuilder};
    use sl_viewer_driver::{UiLocator, Viewer};

    /// A failed stage, or a test that could not set one up.
    type TestError = Box<dyn core::error::Error>;

    /// The viewer binary cargo built for this test.
    const VIEWER: &str = env!("CARGO_BIN_EXE_sl-client-bevy-viewer");

    /// How long a command's effect may take to reach the state it changes.
    const WAIT: Duration = Duration::from_secs(30);

    /// The RLVa console's floater id.
    const CONSOLE: &str = "rlv-console";

    /// The Restrictions window's floater id.
    const BEHAVIOURS: &str = "rlv-behaviours";

    /// The Locks window's floater id.
    const LOCKS: &str = "rlv-locks";

    /// The Strings window's floater id.
    const STRINGS: &str = "rlv-strings";

    /// The debug-settings editor's floater id.
    const DEBUG_SETTINGS: &str = "debug_settings";

    /// The notification the master switch raises when it turns RLV on.
    const RLV_TOGGLED_ON: &str = "RLVaToggledOn";

    /// A one-viewer stage for `name`.
    fn stage(name: &str) -> StageBuilder {
        StageBuilder::new(name)
            .viewer_binary(VIEWER)
            .viewer("Alpha")
    }

    /// `text` as a Fluent placeable prints it: wrapped in the Unicode
    /// isolation marks that keep its direction its own.
    fn isolated(text: &str) -> String {
        format!("\u{2068}{text}\u{2069}")
    }

    /// A plural select placeable's text: `count noun`, with the count
    /// isolated inside the isolated selection.
    fn counted(count: u32, noun: &str) -> String {
        isolated(&format!("{} {noun}", isolated(&count.to_string())))
    }

    /// The RLVa menu's entry whose caption is `key`, while the menu is open.
    fn rlva_entry(alpha: &Viewer, key: &str) -> UiLocator {
        alpha
            .ui()
            .locator(Locator::role(Role::MenuItem).name_key(key))
    }

    /// Open the RLVa menu, let `check` judge it, and close it again.
    async fn with_rlva_menu<F>(alpha: &Viewer, check: F) -> Result<(), BodyError>
    where
        F: AsyncFnOnce(&Viewer) -> Result<(), BodyError>,
    {
        let _menu = alpha
            .ui()
            .locator(Locator::role(Role::MenuItem).name_key("menu-bar-rlva"))
            .click()
            .await?;
        check(alpha).await?;
        alpha.press("Escape").await?;
        let _closed = alpha
            .expect(&rlva_entry(alpha, "menu-bar-enable-rlva"))
            .to_be_detached()
            .await?;
        Ok(())
    }

    /// Turn RLV on with the RLVa menu's master switch, and answer the notice
    /// that says it took effect.
    async fn enable_rlv(alpha: &Viewer) -> Result<(), BodyError> {
        let _on = alpha
            .menu_path(&["menu-bar-rlva", "menu-bar-enable-rlva"])
            .await?;
        let _shown = alpha
            .expect_notification()
            .timeout(WAIT)
            .to_show(RLV_TOGGLED_ON)
            .await?;
        let _answered = alpha.ui().test_id("toast-button:OK").click().await?;
        Ok(())
    }

    /// Open `floater` by the menu `path` that reaches it, and wait for it.
    async fn open_rlva_window(
        alpha: &Viewer,
        path: &[&str],
        floater: &str,
    ) -> Result<(), BodyError> {
        let _opened = alpha.menu_path(path).await?;
        let _shown = alpha
            .expect(&alpha.ui().window(floater))
            .to_be_visible()
            .await?;
        Ok(())
    }

    /// Close `floater` with its own close button, and wait for it to go.
    async fn close_window(alpha: &Viewer, floater: &str) -> Result<(), BodyError> {
        let window = alpha.ui().window(floater);
        let _closed = window.button_key("floater-chrome-close").click().await?;
        let _hidden = alpha.expect(&window).to_be_hidden().await?;
        Ok(())
    }

    /// Open the console from the RLVa menu.
    async fn open_console(alpha: &Viewer) -> Result<(), BodyError> {
        open_rlva_window(alpha, &["menu-bar-rlva", "menu-bar-console"], CONSOLE).await
    }

    /// Type `line` into the console and press Enter.
    async fn run(alpha: &Viewer, line: &str) -> Result<(), BodyError> {
        let field = alpha
            .ui()
            .window(CONSOLE)
            .test_id("rlv-console-input:field");
        let _typed = field.fill(line).await?;
        field.press("Enter").await?;
        Ok(())
    }

    /// Wait until the console's transcript shows a line that is exactly
    /// `text`.
    async fn expect_line(alpha: &Viewer, text: &str) -> Result<(), BodyError> {
        let line = alpha
            .ui()
            .window(CONSOLE)
            .get(Locator::test_id("rlv-console-line").named(text));
        let _shown = alpha.expect(&line).timeout(WAIT).to_be_visible().await?;
        Ok(())
    }

    // ---- The console, the menu, Restrictions and Locks -------------------

    /// **The RLVa menu and the windows onto the held state**: every entry
    /// below the master switch is greyed while RLV is off and live once it is
    /// on; the console reports each command of a line on its own line, the
    /// moment it is typed; the Restrictions window lists the restrictions and
    /// the Locks window the locks they make, each with a count; and closing
    /// the console lifts everything typed into it.
    #[test]
    fn the_rlva_windows_show_what_the_console_holds_until_it_closes() -> Result<(), TestError> {
        stage("rlva_windows").run(async |stage: &Stage| {
            let alpha = &stage.viewer("Alpha")?;
            with_rlva_menu(alpha, async |alpha: &Viewer| {
                let _off = alpha
                    .expect(&rlva_entry(alpha, "menu-bar-enable-rlva"))
                    .to_be_unchecked()
                    .await?;
                for key in [
                    "menu-bar-console",
                    "menu-bar-restrictions",
                    "menu-bar-strings",
                ] {
                    let _greyed = alpha
                        .expect(&rlva_entry(alpha, key))
                        .to_be_disabled()
                        .await?;
                }
                Ok(())
            })
            .await?;
            enable_rlv(alpha).await?;
            with_rlva_menu(alpha, async |alpha: &Viewer| {
                let _on = alpha
                    .expect(&rlva_entry(alpha, "menu-bar-enable-rlva"))
                    .to_be_checked()
                    .await?;
                for key in [
                    "menu-bar-console",
                    "menu-bar-restrictions",
                    "menu-bar-strings",
                ] {
                    let _live = alpha
                        .expect(&rlva_entry(alpha, key))
                        .to_be_enabled()
                        .await?;
                }
                Ok(())
            })
            .await?;

            open_console(alpha).await?;
            run(alpha, "@fly=n").await?;
            expect_line(alpha, "> @fly=n").await?;
            expect_line(alpha, "INFO: @fly=n").await?;
            run(alpha, "@notacommand=n").await?;
            expect_line(alpha, "ERR: @notacommand=n (unknown command)").await?;
            run(alpha, "@remattach:chest=n,addoutfit:gloves=n").await?;
            expect_line(alpha, "INFO: @remattach:chest=n").await?;
            expect_line(alpha, "INFO: @addoutfit:gloves=n").await?;

            // The windows open over the console's input, so the commands
            // come first and the windows after.
            open_rlva_window(
                alpha,
                &["menu-bar-rlva", "menu-bar-restrictions"],
                BEHAVIOURS,
            )
            .await?;
            let behaviours = alpha.ui().window(BEHAVIOURS);
            let restriction =
                |part: &str| behaviours.get(Locator::role(Role::ListItem).name_containing(part));
            for held in ["fly", "remattach:chest", "addoutfit:gloves"] {
                let _listed = alpha.expect(&restriction(held)).to_be_visible().await?;
            }
            let count = behaviours.test_id("rlv-behaviours-count");
            let _counted = alpha
                .expect(&count)
                .to_have_text(&format!(
                    "{}, {}, from {}",
                    counted(3, "restrictions"),
                    counted(0, "exceptions"),
                    counted(1, "object")
                ))
                .await?;
            close_window(alpha, BEHAVIOURS).await?;

            open_rlva_window(
                alpha,
                &["menu-bar-rlva", "menu-bar-debug", "menu-bar-locks"],
                LOCKS,
            )
            .await?;
            let locks = alpha.ui().window(LOCKS);
            for (kind, target) in [("Attachment point", "chest"), ("Wearable layer", "gloves")] {
                let row =
                    locks.get(Locator::role(Role::ListItem).name_containing(format!("{kind} ")));
                let _locked = alpha.expect(&row).to_contain_text(target).await?;
            }
            let _counted = alpha
                .expect(&locks.test_id("rlv-locks-count"))
                // The select is the whole message, so it is not isolated
                // again.
                .to_have_text(&format!("{} locks in force", isolated("2")))
                .await?;
            close_window(alpha, LOCKS).await?;

            // The console issues as the agent, so nothing else could ever lift
            // what it holds: closing it does.
            close_window(alpha, CONSOLE).await?;
            open_rlva_window(
                alpha,
                &["menu-bar-rlva", "menu-bar-restrictions"],
                BEHAVIOURS,
            )
            .await?;
            let _lifted = alpha
                .expect(&count)
                .to_have_text(&format!(
                    "{}, {}, from {}",
                    counted(0, "restrictions"),
                    counted(0, "exceptions"),
                    counted(0, "objects")
                ))
                .await?;
            let _gone = alpha.expect(&restriction("fly")).to_be_detached().await?;
            Ok(())
        })?;
        Ok(())
    }

    // ---- Strings ----------------------------------------------------------

    /// The string the Strings window shows first, and its default wording.
    const FIRST_STRING: (&str, &str) = (
        "Blocked incoming IM message (local)",
        "*** IM blocked by your viewer",
    );

    /// The string after it in the picker.
    const SECOND_STRING: &str = "Blocked incoming IM message (remote)";

    /// **Strings**: an edit to a string is kept — picking another string and
    /// coming back shows the edit, not the default — and Restore default puts
    /// the reference's wording back.
    #[test]
    fn the_strings_window_keeps_an_edit_and_restores_the_default() -> Result<(), TestError> {
        stage("rlva_strings").run(async |stage: &Stage| {
            let alpha = &stage.viewer("Alpha")?;
            enable_rlv(alpha).await?;
            open_rlva_window(alpha, &["menu-bar-rlva", "menu-bar-strings"], STRINGS).await?;
            let strings = alpha.ui().window(STRINGS);
            let picker = strings.test_id("rlv-strings-picker:combo");
            let value = strings.test_id("rlv-strings-value:field");
            let (label, default) = FIRST_STRING;
            let _first = alpha.expect(&picker).to_have_text(label).await?;
            let _default = alpha.expect(&value).to_have_text(default).await?;

            let edited = "*** Not reading IMs right now";
            let _typed = value.fill(edited).await?;
            let option = |label: &str| Locator::role(Role::ListItem).named(label);
            let _other = picker.select_option(option(SECOND_STRING)).await?;
            let _moved = alpha.expect(&picker).to_have_text(SECOND_STRING).await?;
            let _other_value = alpha
                .expect(&value)
                .to_contain_text("prevented from reading your instant messages")
                .await?;
            let _back = picker.select_option(option(label)).await?;
            let _kept = alpha.expect(&value).to_have_text(edited).await?;

            let _restored = strings.button_key("rlv-strings-restore").click().await?;
            let _default_again = alpha.expect(&value).to_have_text(default).await?;
            Ok(())
        })?;
        Ok(())
    }

    // ---- Environment and @setrot ------------------------------------------

    /// The World ▸ Environment entry that drops the local sky.
    const USE_SHARED_ENVIRONMENT: [&str; 3] = [
        "menu-bar-world",
        "menu-bar-environment",
        "menu-bar-use-shared-environment",
    ];

    /// Whether a colour channel is off.
    fn none(channel: f32) -> bool {
        channel.abs() < f32::EPSILON
    }

    /// The ambient colour of the sky being drawn.
    async fn drawn_ambient(alpha: &Viewer) -> Result<[f32; 3], BodyError> {
        Ok(alpha
            .environment()
            .await?
            .sky
            .ok_or("the viewer draws no sky")?
            .ambient)
    }

    /// **Environment and heading commands**: `@setenv_ambient:1/0/0=force`
    /// puts a red local sky over the shared one and `@getenv_ambient` reads it
    /// back; Use Shared Environment takes the local sky away again; and
    /// `@setrot` turns the avatar to the heading it names (north for `0`).
    #[test]
    fn the_console_turns_the_sky_red_and_the_avatar_round() -> Result<(), TestError> {
        stage("rlv_environment").run(async |stage: &Stage| {
            let alpha = &stage.viewer("Alpha")?;
            enable_rlv(alpha).await?;
            open_console(alpha).await?;

            let local_sky = alpha.expect_state(Probe::Environment).at("/local_sky");
            let _shared = local_sky.clone().to_equal(json!(false)).await?;
            run(alpha, "@getenv_ambient=2222").await?;
            expect_line(alpha, "INFO: @getenv_ambient=2222").await?;

            run(alpha, "@setenv_ambient:1/0/0=force").await?;
            expect_line(alpha, "INFO: @setenv_ambient:1/0/0=force").await?;
            let _local = local_sky
                .clone()
                .timeout(WAIT)
                .to_equal(json!(true))
                .await?;
            let red = drawn_ambient(alpha).await?;
            assert!(
                red[0] > 0.0 && none(red[1]) && none(red[2]),
                "the sky's ambient is pure red: {red:?}"
            );
            run(alpha, "@getenv_ambient=2222").await?;
            expect_line(alpha, "2222: 1.000000/0.000000/0.000000").await?;

            let heading = alpha.expect_state(Probe::Agent).at("/heading");
            run(alpha, "@setrot:0=force").await?;
            let _north = heading
                .clone()
                .timeout(WAIT)
                .to_equal(json!(core::f32::consts::FRAC_PI_2))
                .await?;
            // The option is the heading clockwise from north, so a quarter turn
            // faces east: a heading of zero.
            run(alpha, "@setrot:1.5707964=force").await?;
            let _east = heading.timeout(WAIT).to_equal(json!(0.0_f32)).await?;

            let _shared_again = alpha.menu_path(&USE_SHARED_ENVIRONMENT).await?;
            let _dropped = local_sky.timeout(WAIT).to_equal(json!(false)).await?;
            let shared = drawn_ambient(alpha).await?;
            assert!(
                shared[1] > 0.0 || shared[2] > 0.0,
                "the shared sky is drawn again, not the script's red: {shared:?}"
            );
            Ok(())
        })?;
        Ok(())
    }

    // ---- @setenv=n and RestrainedLoveNoSetEnv ----------------------------

    /// The debug setting that takes `@setenv=n` out of the language.
    const NO_SET_ENV: &str = "RestrainedLoveNoSetEnv";

    /// Open World ▸ Environment, see Use Shared Environment `enabled` or not,
    /// and close the menus again.
    async fn expect_environment_menu(alpha: &Viewer, enabled: bool) -> Result<(), BodyError> {
        let _menu = alpha
            .menu_path(&["menu-bar-world", "menu-bar-environment"])
            .await?;
        let entry = alpha
            .ui()
            .locator(Locator::role(Role::MenuItem).name_key("menu-bar-use-shared-environment"));
        let _state = if enabled {
            alpha.expect(&entry).to_be_enabled().await?
        } else {
            alpha.expect(&entry).to_be_disabled().await?
        };
        alpha.press("Escape").await?;
        alpha.press("Escape").await?;
        let _closed = alpha.expect(&entry).to_be_detached().await?;
        Ok(())
    }

    /// **`@setenv=n` and `RestrainedLoveNoSetEnv`**: a held `@setenv=n` greys
    /// the user's own environment menu; turning the setting on in the
    /// debug-settings editor releases it and says so, the next `@setenv=n` is
    /// refused as turned off in the user's settings, `@getdebug_*` reads the
    /// setting back, and a `@setenv_*` force command still applies.
    #[test]
    fn no_set_env_releases_and_refuses_setenv_but_not_the_force_commands() -> Result<(), TestError>
    {
        stage("rlv_no_set_env").run(async |stage: &Stage| {
            let alpha = &stage.viewer("Alpha")?;
            enable_rlv(alpha).await?;
            open_console(alpha).await?;
            run(alpha, "@getdebug_restrainedlovenosetenv=2222").await?;
            expect_line(alpha, "2222: 0").await?;
            run(alpha, "@setenv=n").await?;
            expect_line(alpha, "INFO: @setenv=n").await?;
            expect_environment_menu(alpha, false).await?;

            alpha.press("Ctrl+Alt+Shift+S").await?;
            let editor = alpha.ui().window(DEBUG_SETTINGS);
            let _open = alpha.expect(&editor).to_be_visible().await?;
            let _searched = editor
                .test_id("debug-settings:field")
                .fill("nosetenv")
                .await?;
            let _picked = editor
                .get(Locator::role(Role::ListItem).named(NO_SET_ENV))
                .click()
                .await?;
            let value =
                editor.get(Locator::role(Role::Checkbox).name_key("debug-settings-value-name"));
            value.check().await?;
            alpha.press("Ctrl+Alt+Shift+S").await?;
            let _closed = alpha.expect(&editor).to_be_hidden().await?;

            let released = alpha.ui().window(CONSOLE).get(
                Locator::test_id("rlv-console-line").name_containing("@setenv=n is now refused"),
            );
            let _said = alpha
                .expect(&released)
                .timeout(WAIT)
                .to_be_visible()
                .await?;
            expect_environment_menu(alpha, true).await?;
            run(alpha, "@setenv=n").await?;
            expect_line(alpha, "ERR: @setenv=n (turned off in your settings)").await?;
            run(alpha, "@getdebug_restrainedlovenosetenv=2222").await?;
            expect_line(alpha, "2222: 1").await?;
            run(alpha, "@setenv_ambient:0/1/0=force").await?;
            expect_line(alpha, "INFO: @setenv_ambient:0/1/0=force").await?;
            let _local = alpha
                .expect_state(Probe::Environment)
                .at("/local_sky")
                .timeout(WAIT)
                .to_equal(json!(true))
                .await?;
            let green = drawn_ambient(alpha).await?;
            assert!(
                none(green[0]) && green[1] > 0.0 && none(green[2]),
                "the force command still reached the sky: {green:?}"
            );
            Ok(())
        })?;
        Ok(())
    }
}
