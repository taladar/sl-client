//! End-to-end test of the fake grid's `scripts` scene driven through the
//! viewer, against each flavour ([[server-fake-grid-scripted-scenario]]).
//!
//! The scene is one scripted prim per behaviour, meant for a person to drive
//! by hand: this is that person. The viewer touches the touch prim and is told
//! who touched it; is menued by the dialog prim and picks a colour on the
//! card; says `hello` and `/42 spin` to the listener in its chat bar; has the
//! rezzer rez its spark, the notecard reader read its notecard and the
//! linkset pinged; clicks the vendor, which this viewer cannot pay yet and
//! says so; opens the broken script from the Build window's Content tab and
//! saves it to be shown why it does not compile; is seated on the pose ball by
//! a left click, which poses it; and drops the physical box. Every answer is
//! a line of local chat the viewer's transcript shows.

#[cfg(test)]
mod test {
    use core::time::Duration;

    use serde_json::json;
    use sl_automation_proto::{Locator, Probe, Role};
    use sl_e2e::{BodyError, Stage, StageBuilder};
    use sl_fake_grid::fixtures::scenarios;
    use sl_fake_grid::fixtures::scripts_scene::{
        BROKEN, DIALOG, LINKSET, LISTENER, LISTENER_CHANNEL, NOTECARD_READER, PHYSICAL, POSE_BALL,
        REZZER, TOUCH, VENDOR, key, script_item_key,
    };
    use sl_fake_grid::{ImitatedGrid, RegionConfig};
    use sl_viewer_driver::Viewer;

    /// A failed stage, or a test that could not set one up.
    type TestError = Box<dyn core::error::Error>;

    /// The viewer binary cargo built for this test.
    const VIEWER: &str = env!("CARGO_BIN_EXE_sl-client-bevy-viewer");

    /// How long the viewer is given to show what the grid sent.
    const WAIT: Duration = Duration::from_secs(60);

    /// The notification a script's dialog raises.
    const DIALOG_CARD: &str = "ScriptDialog";

    /// The notice a left click on a pay prim raises while the viewer has no
    /// pay window ([[viewer-money-economy-ui]]).
    const PAY_UNAVAILABLE: &str = "ClickActionPayUnavailable";

    /// Waits until `viewer`'s transcript has a line from `speaker` holding
    /// `text`.
    async fn heard(viewer: &Viewer, speaker: &str, text: &str) -> Result<(), BodyError> {
        let _heard = viewer
            .expect_chat()
            .from(speaker)
            .timeout(WAIT)
            .to_contain(text)
            .await?;
        Ok(())
    }

    /// Says `line` in `viewer`'s nearby-chat bar.
    async fn say(viewer: &Viewer, line: &str) -> Result<(), BodyError> {
        let bar = viewer.ui().test_id("nearby-chat-bar").role(Role::Textbox);
        let _typed = bar.fill(line).await?;
        bar.press("Enter").await?;
        Ok(())
    }

    /// Touches the physical box and waits for the grid to hear it land on its
    /// plinth again — heard at the grid rather than in the transcript, which
    /// may already hold the landing the box made as the scene started.
    async fn drop_the_box(stage: &Stage, viewer: &Viewer) -> Result<(), BodyError> {
        let mut said = stage.grid()?.overhear(stage.home_region())?;
        let _touched = viewer
            .world()
            .object_named(PHYSICAL)
            .timeout(WAIT)
            .touch()
            .await?;
        tokio::time::timeout(WAIT, async {
            while let Some(line) = said.recv().await {
                if line.from_name == PHYSICAL && line.message == "Bumped Plinth" {
                    return Ok(());
                }
            }
            Err("the region stopped hearing")
        })
        .await
        .map_err(|_elapsed| "the box never landed again")??;
        Ok(())
    }

    /// Opens the broken script from the Build window's Content tab, saves it
    /// and is shown that it did not compile, and where.
    async fn the_broken_script_is_shown_its_errors(viewer: &Viewer) -> Result<(), BodyError> {
        let prim = key(BROKEN).ok_or("no broken prim")?;
        let item = script_item_key(BROKEN).ok_or("no broken script")?;
        viewer.press("Ctrl+B").await?;
        let build = viewer.ui().window("build-tools");
        // The window opens on Create with nothing selected; a click there
        // would rez rather than select.
        let _move = build
            .get(Locator::role(Role::Radio).name_key("build-tool-move"))
            .click()
            .await?;
        let _selected = viewer
            .world()
            .object_named(BROKEN)
            .timeout(WAIT)
            .select()
            .await?;
        let _content = build
            .get(Locator::role(Role::Tab).name_key("build-tab-content"))
            .click()
            .await?;
        let row = build.get(Locator::role(Role::ListItem).name_containing("broken.lsl"));
        let _listed = viewer.expect(&row).timeout(WAIT).to_be_visible().await?;
        let _opened = row.double_click().await?;
        let editor = viewer
            .ui()
            .window(&format!("script-editor#{}/{}", prim.uuid(), item.uuid()));
        let save = editor.get(Locator::role(Role::Button).name_key("script-save"));
        let _ready = viewer.expect(&save).timeout(WAIT).to_be_visible().await?;
        let _saved = save.click().await?;
        let failed = editor.get(Locator::default().name_key("script-status-compile-failed"));
        let _failed = viewer.expect(&failed).timeout(WAIT).to_be_visible().await?;
        // The row is the position, then the compiler's message, which each
        // flavour words its own way.
        let error = editor.get(Locator::role(Role::Text).name_containing(", column"));
        let _listed = viewer.expect(&error).timeout(WAIT).to_be_visible().await?;
        // Without the isolation marks Fluent puts around each placeable.
        let said: String = error
            .text()
            .await?
            .unwrap_or_default()
            .chars()
            .filter(|character| !matches!(character, '\u{2068}' | '\u{2069}'))
            .collect();
        if !said.starts_with("Line 8, column ") {
            return Err(format!("the error is listed as {said:?}").into());
        }
        let _closed = editor.test_id("floater-button:close").click().await?;
        let _gone = viewer.expect(&editor).timeout(WAIT).to_be_hidden().await?;
        // Closing the Build window leaves build mode, so the next left click
        // touches or sits rather than selects.
        let _closed = build.test_id("floater-button:close").click().await?;
        let _gone = viewer.expect(&build).timeout(WAIT).to_be_hidden().await?;
        Ok(())
    }

    /// **The scripts scene through the viewer, on each flavour**: every prim
    /// answers what the viewer does to it, as a person driving it by hand
    /// would see.
    #[test]
    fn the_viewer_drives_every_prim_of_the_scripts_scene() -> Result<(), TestError> {
        for (flavour, name) in [
            (ImitatedGrid::SecondLife, "scripts_scene_second_life"),
            (ImitatedGrid::OpenSim, "scripts_scene_open_sim"),
        ] {
            let scene = scenarios::scenario("scripts").ok_or("no scripts scene")?;
            StageBuilder::new(name)
                .viewer_binary(VIEWER)
                .viewer("Alpha")
                .region(scene.dress(RegionConfig::default()))
                .configure_grid(move |grid| grid.imitates(flavour))
                .run(async |stage: &Stage| {
                    let alpha = &stage.viewer("Alpha")?;
                    let me = stage.account_name("Alpha")?.to_owned();
                    let world = alpha.world();

                    let _touched = world.object_named(TOUCH).timeout(WAIT).touch().await?;
                    heard(alpha, TOUCH, &format!("{me} touched face")).await?;

                    let _touched = world.object_named(DIALOG).timeout(WAIT).touch().await?;
                    let _shown = alpha
                        .expect_notification()
                        .timeout(WAIT)
                        .to_show(DIALOG_CARD)
                        .await?;
                    let _blue = alpha
                        .ui()
                        .test_id("script-dialog-button:Blue")
                        .click()
                        .await?;
                    heard(alpha, DIALOG, &format!("{me} chose Blue")).await?;

                    say(alpha, "hello").await?;
                    heard(alpha, LISTENER, &format!("Hello, {me}!")).await?;
                    say(alpha, &format!("/{LISTENER_CHANNEL} spin")).await?;
                    heard(alpha, LISTENER, &format!("Spinning for {me}")).await?;

                    let _touched = world.object_named(REZZER).timeout(WAIT).touch().await?;
                    heard(alpha, "Spark", "Spark 1 lit").await?;
                    heard(alpha, REZZER, "Rezzed Spark").await?;

                    let _touched = world
                        .object_named(NOTECARD_READER)
                        .timeout(WAIT)
                        .touch()
                        .await?;
                    heard(
                        alpha,
                        NOTECARD_READER,
                        "Line 1: Welcome to the scripts scene.",
                    )
                    .await?;
                    heard(alpha, NOTECARD_READER, "End of Greetings").await?;

                    let _touched = world.object_named(LINKSET).timeout(WAIT).touch().await?;
                    heard(alpha, LINKSET, &format!("Link 2 answered for {me}")).await?;
                    heard(alpha, LINKSET, &format!("Link 3 answered for {me}")).await?;

                    let _touched = world.object_named(VENDOR).timeout(WAIT).touch().await?;
                    let _unavailable = alpha
                        .expect_notification()
                        .timeout(WAIT)
                        .to_show(PAY_UNAVAILABLE)
                        .await?;

                    the_broken_script_is_shown_its_errors(alpha).await?;

                    let ball = key(POSE_BALL).ok_or("no pose ball")?;
                    let _clicked = world.object_named(POSE_BALL).timeout(WAIT).touch().await?;
                    let _seated = alpha
                        .expect_state(Probe::Agent)
                        .at("/seated_on")
                        .timeout(WAIT)
                        .to_equal(json!(ball.uuid().to_string()))
                        .await?;
                    heard(alpha, POSE_BALL, &format!("Posing {me}")).await?;

                    // Last: the box is in front of the props past it while
                    // it falls, and the viewer sees it fall a little after
                    // the grid does, so a click aimed past it then can land
                    // on it.
                    drop_the_box(stage, alpha).await?;
                    Ok(())
                })?;
        }
        Ok(())
    }
}
