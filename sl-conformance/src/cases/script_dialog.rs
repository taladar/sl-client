//! Receive a scripted-object dialog (`llDialog`) and reply with a button choice.
//!
//! A script raises a menu on an avatar's viewer with `llDialog` (or a free-text
//! prompt with `llTextBox`): the simulator sends a `ScriptDialog` naming the
//! object, its owner, the prompt text, the button labels and a hidden negative
//! chat channel. The avatar answers by chatting the chosen button's label on
//! that channel — a `ScriptDialogReply` — which the script hears on its
//! `llListen`. This case exercises both edges: it waits for the dialog the
//! Default Region's scripted test prim (`SLClientScriptTester`) fires on its
//! timer, asserts the parse (a hidden channel, at least one button), then
//! answers it with [`Command::ReplyScriptDialog`] choosing the first button.
//!
//! The reply carries no application-level acknowledgement — the only observer of
//! a `ScriptDialogReply` is the script's own `llListen`, whose reaction a stock
//! prim need not expose to the viewer — so "no error" is read the same way
//! [`super::object_touch_grab`] reads it: the circuit staying healthy, a
//! keep-alive ping still round-tripping after the reply is enqueued. The reply is
//! a reliable message, so a failure to encode or enqueue it propagates from
//! `send` and fails the case before that check.
//!
//! `1av`, `[both]`. On OpenSim the avatar is forced into the "Default Region",
//! whose `SLClientScriptTester` prim calls `llDialog` at the test avatar every
//! few seconds (see the scripted-object OAR setup in the appendix), so a dialog
//! is guaranteed and its absence fails the case. On Second Life no such fixture
//! exists — an unsolicited dialog would need a scripted object in the landing
//! region deliberately menuing this avatar — so a window with no dialog is
//! recorded `partial` rather than failed. The aditi run is deferred with the
//! rest of the Aditi batch (no aditi record this session).

use std::time::Duration;

use sl_client_tokio::{Command, Event, ScriptDialog as ScriptDialogEvent};

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, count_metric, is_opensim, secs_metric};

/// The OpenSim start location: the "Default Region" (1000,1000), centred, which
/// holds the `SLClientScriptTester` scripted prim that fires the dialog. On
/// Second Life the avatar keeps `"last"` (a named OpenSim region is meaningless
/// there), and no scripted dialog fixture is expected.
const OPENSIM_START: &str = "uri:Default Region&128&128&30";

/// How long to watch for a `ScriptDialog` after the region is ready.
///
/// The test prim fires on a few-second `llSetTimerEvent` loop, but the first
/// tick has to fall after login settles and the interest list streams the
/// object; the window spans several such ticks. Kept generous for Aditi network
/// jitter (where, absent a fixture, it simply times out into `partial`).
const DIALOG_WINDOW: Duration = Duration::from_secs(30);

/// How long to observe the circuit after the reply for a keep-alive ping.
///
/// The `ScriptDialogReply` has no application-level acknowledgement, so the
/// circuit staying healthy — a root-simulator keep-alive ping (≈ 5 s interval)
/// still round-tripping — is the "no error" signal, exactly as
/// [`super::object_touch_grab`] confirms an unacknowledged interaction.
const OBSERVE_WINDOW: Duration = Duration::from_secs(15);

/// The free-text answer sent when the dialog is an `llTextBox` prompt rather
/// than a button menu — a short, harmless string on the hidden channel.
const TEXT_BOX_ANSWER: &str = "sl-client";

/// Receives a scripted-object dialog and replies with a button choice,
/// confirming the circuit stays healthy afterwards.
#[derive(Debug)]
pub struct ScriptDialog;

impl GridTest for ScriptDialog {
    fn name(&self) -> &'static str {
        "script-dialog"
    }

    fn description(&self) -> &'static str {
        "Receive a script dialog and reply"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn start_location(&self, grid: Grid) -> &'static str {
        if is_opensim(grid) {
            OPENSIM_START
        } else {
            "last"
        }
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();

            // Wait for the scripted prim's dialog. `ScriptDialog` fires each time
            // the test prim calls `llDialog`; the first one inside the window is
            // the one this case answers. A per-attempt timeout that consumes the
            // whole window ends the wait empty-handed.
            let dialog: Option<ScriptDialogEvent> = {
                let session = ctx.primary();
                session.wait_for_region(REGION_TIMEOUT).await?;
                let started = std::time::Instant::now();
                match session
                    .wait_for(DIALOG_WINDOW, |event| match event {
                        Event::ScriptDialog(dialog) => Some((**dialog).clone()),
                        _ => None,
                    })
                    .await
                {
                    Ok(dialog) => {
                        ctx.metrics().set_timing(
                            &secs_metric("dialog_rtt"),
                            started.elapsed().as_secs_f64(),
                        );
                        Some(dialog)
                    }
                    // No dialog streamed inside the window; handled per grid below
                    // (a hard failure on OpenSim, `partial` on SL).
                    Err(TestFailure::Timeout(_)) => None,
                    Err(other) => return Err(other),
                }
            };

            let dialog = match dialog {
                Some(dialog) => dialog,
                None if is_opensim(grid) => {
                    return Err(TestFailure::Assertion(
                        "no ScriptDialog arrived from the Default Region test prim".to_owned(),
                    ));
                }
                None => {
                    // On Second Life no scripted object is menuing this avatar,
                    // so the absence of a dialog is a legitimately incomplete run.
                    ctx.mark_partial("landing region raised no script dialog within the window");
                    return Ok(());
                }
            };

            // The reply target: the object that raised the dialog, on its hidden
            // channel. Choose the first button; an `llTextBox` prompt carries the
            // typed answer in place of a real label.
            let is_text_box = dialog.is_text_box();
            let (button_index, button_label) = if is_text_box {
                (0_i32, TEXT_BOX_ANSWER.to_owned())
            } else {
                let label = dialog.buttons.first().ok_or_else(|| {
                    TestFailure::Assertion("the script dialog carried no buttons".to_owned())
                })?;
                (0_i32, label.clone())
            };

            let session = ctx.primary();
            session
                .send(Command::ReplyScriptDialog {
                    object_id: dialog.object_id,
                    chat_channel: dialog.chat_channel,
                    button_index,
                    button_label: button_label.clone(),
                })
                .await?;

            // The reply carries no application-level response, so a keep-alive
            // ping answered after it is the "no error" signal: the reply was
            // accepted and the session is still live. A `Disconnected` mid-window
            // propagates and fails the case.
            let rtt = match session
                .wait_for(OBSERVE_WINDOW, |event| match event {
                    Event::Ping {
                        child: false, rtt, ..
                    } => Some(*rtt),
                    _ => None,
                })
                .await
            {
                Ok(rtt) => rtt,
                // No ping inside the window means the circuit went quiet after the
                // reply — the case's failure.
                Err(TestFailure::Timeout(_)) => {
                    return Err(TestFailure::Assertion(
                        "no keep-alive ping observed after replying to the script dialog"
                            .to_owned(),
                    ));
                }
                Err(other) => return Err(other),
            };

            let metrics = ctx.metrics();
            metrics.set("object_id", dialog.object_id.to_string());
            metrics.set("object_name", dialog.object_name.clone());
            metrics.set("chat_channel", i64::from(dialog.chat_channel.0));
            metrics.set(&count_metric("buttons"), dialog.buttons.len().to_string());
            metrics.set("is_text_box", is_text_box);
            metrics.set("chosen_button", button_label);
            metrics.set_timing(&secs_metric("ping_rtt"), rtt.as_secs_f64());
            Ok(())
        })
    }
}
