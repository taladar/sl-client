//! End-to-end test for the dialogs a script puts to the avatar (`llDialog`,
//! `llTextBox`), answered on the viewer's dialog card, against each flavour
//! of the fake grid ([[gridspec-lsl-comms]]).
//!
//! Both live grids put a dialog alike (`book/src/gridspec/lsl.md` § Chat,
//! listens, dialogs and link messages): a `ScriptDialog` naming the object
//! and its owner, the message, the hidden channel and the buttons in the
//! script's order, and a text box as the one button `!!llTextBox!!`. Where
//! they differ a viewer is shown something else: a dialog with no buttons
//! comes with the one button `OK` from Second Life and `Ok` from OpenSim,
//! and OpenSim cuts a label to 23 characters where Second Life refuses one
//! over 24. So each flavour is run through the same steps, each dialog as
//! that grid sends it: three buttons, the second pressed; the empty list's
//! one button, pressed; and a text box, typed into and submitted — and the
//! grid has each reply from the viewer as the button's index and label, or
//! as the text on index 0.

#[cfg(test)]
mod test {
    use core::time::Duration;

    use pretty_assertions::assert_eq;
    use sl_e2e::{BodyError, Stage, StageBuilder};
    use sl_fake_grid::ImitatedGrid;
    use sl_proto::{ChatChannel, ObjectKey, ScriptDialog, ScriptDialogAnswer, TextureKey, Uuid};

    /// A failed stage, or a test that could not set one up.
    type TestError = Box<dyn core::error::Error>;

    /// The viewer binary cargo built for this test.
    const VIEWER: &str = env!("CARGO_BIN_EXE_sl-client-bevy-viewer");

    /// How long the viewer is given to show what the grid sent, and the grid
    /// to hear the reply.
    const WAIT: Duration = Duration::from_secs(60);

    /// The notification a dialog raises.
    const DIALOG: &str = "ScriptDialog";

    /// The channel the `comms/dialog` probe listens on.
    const CHANNEL: ChatChannel = ChatChannel(-731_001);

    /// The scripted object asking.
    fn task() -> ObjectKey {
        ObjectKey::from(Uuid::from_u128(0xD1A1_0B1E))
    }

    /// The one button each grid puts on a dialog the script gave none.
    const fn no_buttons_label(flavour: ImitatedGrid) -> &'static str {
        match flavour {
            ImitatedGrid::SecondLife => "OK",
            ImitatedGrid::OpenSim => "Ok",
        }
    }

    /// Put to viewer `label` a dialog of `message` with `buttons`, as both
    /// grids put it: the object's name, the owner's names and id.
    async fn put(
        stage: &Stage,
        label: &str,
        message: &str,
        buttons: &[&str],
    ) -> Result<(), BodyError> {
        let owner = stage.account_name(label)?.to_owned();
        let (first, last) = owner.split_once(' ').unwrap_or((owner.as_str(), ""));
        let agent = stage.agent(label).await?;
        let now = agent.now();
        let dialog = ScriptDialog {
            object_id: task(),
            object_name: "Object".to_owned(),
            owner_first_name: first.to_owned(),
            owner_last_name: last.to_owned(),
            owner_id: Some(Uuid::from_u128(0x0E1)),
            message: message.to_owned(),
            chat_channel: CHANNEL,
            image_id: TextureKey::from(Uuid::nil()),
            buttons: buttons.iter().map(|button| (*button).to_owned()).collect(),
        };
        agent
            .with_sim(|sim| sim.send_script_dialog(&dialog, now))
            .await
            .map_err(|error| format!("putting the dialog: {error}"))?;
        Ok(())
    }

    /// The reply the grid has from viewer `label`, once it has one other
    /// than `before`.
    async fn replied(
        stage: &Stage,
        label: &str,
        before: Option<&ScriptDialogAnswer>,
    ) -> Result<ScriptDialogAnswer, BodyError> {
        let agent = stage.agent(label).await?;
        let started = std::time::Instant::now();
        loop {
            let reply = agent
                .with_sim(|sim| sim.script_dialog_reply(task()).cloned())
                .await;
            if let Some(reply) = reply
                && Some(&reply) != before
            {
                return Ok(reply);
            }
            if started.elapsed() >= WAIT {
                return Err("the grid never had a reply from the viewer".into());
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// The reply `label` is told to give, as the grid should have it.
    fn reply(button_index: i32, button_label: &str) -> ScriptDialogAnswer {
        ScriptDialogAnswer {
            chat_channel: CHANNEL,
            button_index,
            button_label: button_label.to_owned(),
        }
    }

    /// **The dialog card on each flavour**: a button press replies with its
    /// index and label, the empty list's one button likewise, and a text
    /// box's Submit with the text on index 0.
    #[test]
    fn a_dialog_and_a_text_box_are_answered_on_the_card() -> Result<(), TestError> {
        for (flavour, name) in [
            (ImitatedGrid::SecondLife, "script_dialog_second_life"),
            (ImitatedGrid::OpenSim, "script_dialog_open_sim"),
        ] {
            StageBuilder::new(name)
                .viewer_binary(VIEWER)
                .viewer("Alpha")
                .configure_grid(move |grid| grid.imitates(flavour))
                .run(async |stage: &Stage| {
                    let alpha = &stage.viewer("Alpha")?;

                    put(stage, "Alpha", "Pick one", &["One", "Two", "Three"]).await?;
                    let _shown = alpha
                        .expect_notification()
                        .timeout(WAIT)
                        .to_show(DIALOG)
                        .await?;
                    let _two = alpha
                        .ui()
                        .test_id("script-dialog-button:Two")
                        .click()
                        .await?;
                    let first = replied(stage, "Alpha", None).await?;
                    assert_eq!(first, reply(1, "Two"), "{flavour:?}: a button press");

                    let only = no_buttons_label(flavour);
                    put(stage, "Alpha", "No buttons", &[only]).await?;
                    let _shown = alpha
                        .expect_notification()
                        .timeout(WAIT)
                        .to_show(DIALOG)
                        .await?;
                    let _only = alpha
                        .ui()
                        .test_id(&format!("script-dialog-button:{only}"))
                        .click()
                        .await?;
                    let second = replied(stage, "Alpha", Some(&first)).await?;
                    assert_eq!(
                        second,
                        reply(0, only),
                        "{flavour:?}: the empty list's one button"
                    );

                    put(
                        stage,
                        "Alpha",
                        "Type something",
                        &[ScriptDialog::TEXT_BOX_BUTTON],
                    )
                    .await?;
                    let _shown = alpha
                        .expect_notification()
                        .timeout(WAIT)
                        .to_show(DIALOG)
                        .await?;
                    let _typed = alpha
                        .ui()
                        .test_id("script-dialog-textbox:field")
                        .fill("a few typed words")
                        .await?;
                    let _submit = alpha
                        .ui()
                        .test_id("script-dialog-action:Submit")
                        .click()
                        .await?;
                    assert_eq!(
                        replied(stage, "Alpha", Some(&second)).await?,
                        reply(0, "a few typed words"),
                        "{flavour:?}: a text box's Submit"
                    );
                    Ok(())
                })?;
        }
        Ok(())
    }
}
