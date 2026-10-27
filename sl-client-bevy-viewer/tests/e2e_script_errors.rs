//! End-to-end test of the Script Errors window (`viewer-script-warning-window`)
//! on the fake grid: a script's `DEBUG_CHANNEL` lines, as Second Life sends
//! them for a fault (`book/src/gridspec/lsl.md` § Run-time errors — the
//! object's name and the script's, then the error), open the window on their
//! own and land on its All tab and on a tab of the object's own; the same
//! lines from an object somebody else owns are not shown; and with the
//! preference set to nearby chat a line goes to the Nearby transcript
//! instead, as a debug line.

#[cfg(test)]
mod test {
    use core::time::Duration;

    use serde_json::json;
    use sl_automation_proto::{Locator, Probe, Role};
    use sl_e2e::{BodyError, Stage, StageBuilder};
    use sl_proto::{AgentKey, ChatSource, ChatType, ObjectKey, Uuid, Vector};

    /// A failed stage, or a test that could not set one up.
    type TestError = Box<dyn core::error::Error>;

    /// The viewer binary cargo built for this test.
    const VIEWER: &str = env!("CARGO_BIN_EXE_sl-client-bevy-viewer");

    /// How long the viewer is given to show what the grid sent.
    const WAIT: Duration = Duration::from_secs(60);

    /// The window's floater id.
    const WINDOW: &str = "script-errors";

    /// A row's test id.
    const LINE: &str = "script-errors-line";

    /// The faulting object's name.
    const OBJECT: &str = "Faulty prim";

    /// The first of a fault's two lines on Second Life.
    const FAULT: &str = "Faulty prim [script:Math 1] Script run-time error";

    /// The second.
    const ERROR: &str = "Math Error";

    /// What the object somebody else owns says.
    const THEIRS: &str = "Could not find script 'Nope'.";

    /// The line said once the preference sends lines to nearby chat.
    const ROUTED: &str = "Could not find script 'Later'.";

    /// The faulting object.
    fn object() -> ObjectKey {
        ObjectKey::from(Uuid::from_u128(0x5C41_9E44))
    }

    /// Have the grid say `text` on `DEBUG_CHANNEL` to viewer `label`, from
    /// [`object`], owned by `owner`, fully audible.
    async fn say(stage: &Stage, label: &str, owner: AgentKey, text: &str) -> Result<(), BodyError> {
        let agent = stage.agent(label).await?;
        let now = agent.now();
        agent
            .with_sim(|sim| {
                sim.send_chat_from_simulator(
                    OBJECT,
                    ChatSource::Object(object()),
                    owner.uuid(),
                    ChatType::DebugChannel,
                    1,
                    Vector {
                        x: 120.0,
                        y: 128.0,
                        z: 27.5,
                    },
                    text,
                    now,
                )
            })
            .await
            .map_err(|error| format!("saying {text:?}: {error}"))?;
        Ok(())
    }

    /// **The Script Errors window**: Alpha's own object's fault opens it,
    /// with a tab for the object from the first line, and shows both lines
    /// on the All tab under the object's name; an object Beta does not own says nothing to Beta's window;
    /// and routed to nearby chat, a line is a debug line of Alpha's Nearby
    /// transcript.
    #[test]
    fn an_own_objects_errors_open_the_window_and_others_are_not_shown() -> Result<(), TestError> {
        StageBuilder::new("script_errors")
            .viewer_binary(VIEWER)
            .viewer("Alpha")
            .viewer("Beta")
            .run(async |stage: &Stage| {
                let alpha = &stage.viewer("Alpha")?;
                let beta = &stage.viewer("Beta")?;
                let owner = stage.agent_id("Alpha")?;

                // The first line opens the window, and the object has a tab
                // from it — though the window's content is built only as the
                // line opens it.
                say(stage, "Alpha", owner, FAULT).await?;
                let window = alpha.ui().window(WINDOW);
                let _opened = alpha.expect(&window).timeout(WAIT).to_be_visible().await?;
                let tab = window.get(Locator::role(Role::Tab).named(OBJECT));
                let _tabbed = alpha.expect(&tab).timeout(WAIT).to_be_visible().await?;
                say(stage, "Alpha", owner, ERROR).await?;
                for text in [FAULT, ERROR] {
                    let row = window
                        .get(Locator::test_id(LINE).name_containing(format!("] {OBJECT}: {text}")));
                    let _shown = alpha.expect(&row).timeout(WAIT).to_be_visible().await?;
                }

                // Beta is told what Alpha's object said, then what its own
                // says: the window opens on its own object's line alone.
                say(stage, "Beta", owner, THEIRS).await?;
                say(stage, "Beta", stage.agent_id("Beta")?, ERROR).await?;
                let theirs = beta.ui().window(WINDOW);
                let _opened = beta.expect(&theirs).timeout(WAIT).to_be_visible().await?;
                let own = theirs.get(Locator::test_id(LINE).name_containing(ERROR));
                let _shown = beta.expect(&own).timeout(WAIT).to_be_visible().await?;
                let other = theirs.get(Locator::test_id(LINE).name_containing(THEIRS));
                let _hidden = beta.expect(&other).to_be_hidden().await?;

                let _routed = alpha
                    .set_setting("ScriptErrorsShowIn", json!("chat"))
                    .await?;
                say(stage, "Alpha", owner, ROUTED).await?;
                let _heard = alpha
                    .expect_state(Probe::Conversations)
                    .timeout(WAIT)
                    .to_include(json!([{ "lines": [{
                        "text": ROUTED,
                        "chat_kind": "debug",
                    }] }]))
                    .await?;
                Ok(())
            })?;
        Ok(())
    }
}
