//! **Locators in an `&mut App`**: the cheap test tiers — `InteractionTest`,
//! the fixture world, the full-stack `ViewerHarness` — address widgets the way
//! the end-to-end tier does, so a test moves down a tier without being
//! rewritten.
//!
//! Every function here is a request to the app's own executor
//! ([`AutomationPlugin`], installed by [`install`]): it is submitted to the
//! [`AutomationQueue`] and the app is stepped until the answer comes. So the
//! locator is resolved by the same engine, strictly, the action waits for the
//! same actionability checks, the input goes through the same synthetic input,
//! and a failure is the driver's own [`DriverError::Failed`] — the viewer's
//! error and its report, under the action's name, printed exactly as a
//! `sl-viewer-driver` failure is. An app is not a viewer process, so there is
//! no artifact directory; the report is in the error instead.
//!
//! The waits are counted in **frames** by default ([`Options`]): a
//! fixture app is stepped by hand, as fast as it runs, and a test that fails
//! on a slow machine's wall clock is a flaky one. A harness whose frames cost
//! real time (the full-stack one renders) states a wall-clock deadline
//! instead.

use std::time::{Duration, Instant};

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use sl_automation_proto::{
    Deadline, Locator, NodeValue, PointerButton, Request, RequestBody, RequestId, ResponseBody,
    UiNode, WaitCondition,
};
use sl_viewer_driver::{Artifacts, DriverError, Failure};
use sl_viewer_ui_core::synthetic_input::{ActionStatus, InputAction, SyntheticInput};

use crate::executor::{AutomationPlugin, AutomationQueue};

/// Where the ids of in-app requests start: below every transport's base
/// ([`crate::IN_PROCESS_ID_BASE`], [`crate::REMOTE_ID_BASE`]), so a test that
/// also hosts a transport never sees its ids collide.
pub const IN_APP_ID_BASE: u64 = 1 << 46;

/// The frames an in-app request waits for its node by default.
pub const IN_APP_DEADLINE_FRAMES: u32 = 600;

/// The frames stepped past a request's own frame deadline before it counts as
/// unanswered — the executor answers a timeout in the frame it expires, so
/// this is slack, not a second wait.
const GRACE_FRAMES: u32 = 60;

/// The wall-clock time allowed past a request's own wall deadline before it
/// counts as unanswered.
const GRACE: Duration = Duration::from_secs(30);

/// How a test's app is driven: what its failures call it, and how long a
/// request waits.
///
/// Absent, the defaults: the label `app`, and [`IN_APP_DEADLINE_FRAMES`]
/// frames with no wall-clock limit to speak of.
#[derive(Resource, Debug, Clone)]
pub struct Options {
    /// The name a failure gives the app, where a driver's names its viewer.
    pub label: String,
    /// The deadline every request is sent with.
    pub deadline: Deadline,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            label: "app".to_owned(),
            deadline: Deadline {
                frames: Some(IN_APP_DEADLINE_FRAMES),
                // Frames decide; the wall clock only stops a runaway.
                millis: Some(600_000),
            },
        }
    }
}

/// The next in-app request id.
#[derive(Resource, Debug)]
struct NextInAppId(u64);

impl Default for NextInAppId {
    fn default() -> Self {
        Self(IN_APP_ID_BASE)
    }
}

/// Install the executor in `app` unless it is there already. The functions
/// here install it themselves; a test calls this only to have it before the
/// first frame it steps by hand.
pub fn install(app: &mut App) {
    if !app.is_plugin_added::<AutomationPlugin>() {
        app.add_plugins(AutomationPlugin);
    }
    app.init_resource::<NextInAppId>();
}

/// The one node `locator` names, with its children, once one is attached.
///
/// # Errors
///
/// [`DriverError::Failed`] when none is attached within the deadline, or
/// several are.
pub fn locate(app: &mut App, locator: &Locator) -> Result<UiNode, DriverError> {
    let _attached = wait_for(app, locator, WaitCondition::Attached, "read")?;
    let action = format!("read {locator}");
    match ask(
        app,
        RequestBody::Snapshot {
            within: Some(locator.clone()),
        },
        &action,
    )? {
        ResponseBody::Snapshot { roots } => {
            let mut roots = roots.into_iter();
            match (roots.next(), roots.next()) {
                (Some(node), None) => Ok(node),
                (first, second) => Err(unexpected(
                    app,
                    &action,
                    &ResponseBody::Snapshot {
                        roots: first.into_iter().chain(second).collect(),
                    },
                )),
            }
        }
        other => Err(unexpected(app, &action, &other)),
    }
}

/// Every node `locator` names now, without waiting, without children.
///
/// # Errors
///
/// [`DriverError::Failed`] when a scope it names matches several nodes.
pub fn find(app: &mut App, locator: &Locator) -> Result<Vec<UiNode>, DriverError> {
    let action = format!("find {locator}");
    match ask(
        app,
        RequestBody::Find {
            locator: locator.clone(),
        },
        &action,
    )? {
        ResponseBody::Found { nodes } => Ok(nodes),
        other => Err(unexpected(app, &action, &other)),
    }
}

/// The text of the one node `locator` names: a field's or a label's value,
/// else its accessible name.
///
/// # Errors
///
/// As [`locate`].
pub fn text(app: &mut App, locator: &Locator) -> Result<Option<String>, DriverError> {
    let node = locate(app, locator)?;
    Ok(match node.value {
        Some(NodeValue::Text(text)) => Some(text),
        Some(NodeValue::Number(_)) | None => node.name,
    })
}

/// Click the one node `locator` names with the left button, once it is
/// actionable: attached, visible, stable, **enabled** and not covered.
///
/// # Errors
///
/// [`DriverError::Failed`] when it matches several nodes or never becomes
/// actionable — a disabled button times out on the `enabled` check.
pub fn click(app: &mut App, locator: &Locator) -> Result<UiNode, DriverError> {
    act(
        app,
        "click",
        locator,
        RequestBody::Click {
            locator: locator.clone(),
            button: PointerButton::Left,
            double: false,
            deadline: deadline(app),
        },
    )
}

/// Rest the pointer on the one node `locator` names; a disabled node may be
/// hovered.
///
/// # Errors
///
/// As [`click`], but for the `enabled` check.
pub fn hover(app: &mut App, locator: &Locator) -> Result<UiNode, DriverError> {
    act(
        app,
        "hover",
        locator,
        RequestBody::Hover {
            locator: locator.clone(),
            deadline: deadline(app),
        },
    )
}

/// Click the one node `locator` names **while it is disabled**, with the real
/// pointer: the way to show that a disabled control does nothing.
///
/// [`click`] never presses a disabled node — it waits for it to be enabled —
/// so a test of "disabled does nothing" needs a click that goes there anyway,
/// as a user's does. This waits for the node to be disabled, brings the
/// pointer onto it as a hover does (every check but `enabled`), then presses
/// and releases the left button there. Whatever the click reaches is what a
/// user's would; the test asserts that nothing came of it.
///
/// # Errors
///
/// [`DriverError::Failed`] when the node does not come to be disabled, or
/// cannot be hovered; [`DriverError::Unexpected`] when the app has no cursor
/// or no synthetic input to click with.
pub fn click_while_disabled(app: &mut App, locator: &Locator) -> Result<UiNode, DriverError> {
    let verb = "click while disabled";
    let _disabled = wait_for(app, locator, WaitCondition::Disabled, verb)?;
    let node = hover(app, locator)?;
    let action = format!("{verb} {locator}");
    let mut windows = app
        .world_mut()
        .query_filtered::<&Window, With<PrimaryWindow>>();
    let at = windows
        .single(app.world())
        .ok()
        .and_then(Window::cursor_position)
        .ok_or_else(|| {
            missing(
                app,
                &action,
                "no cursor in the primary window after the hover",
            )
        })?;
    let Some(mut input) = app.world_mut().get_resource_mut::<SyntheticInput>() else {
        return Err(missing(app, &action, "no synthetic input to click with"));
    };
    let id = input.enqueue(InputAction::click(at, MouseButton::Left));
    let (limit, _wall) = limits(deadline(app));
    for _frame in 0..limit {
        app.update();
        let status = app.world().resource::<SyntheticInput>().status(id);
        if matches!(
            status,
            ActionStatus::Done { .. } | ActionStatus::Expired | ActionStatus::Unknown
        ) {
            return Ok(node);
        }
    }
    Err(missing(app, &action, "the click was never played"))
}

/// Replace the text of the one text field `locator` names by typing — click
/// into it, select all, delete, type — and answer once it holds `text`.
///
/// # Errors
///
/// As [`click`]; also when the node is not an editable field or does not
/// hold the text afterwards.
pub fn fill(app: &mut App, locator: &Locator, text: &str) -> Result<UiNode, DriverError> {
    act(
        app,
        "fill",
        locator,
        RequestBody::Fill {
            locator: locator.clone(),
            text: text.to_owned(),
            deadline: deadline(app),
        },
    )
}

/// Press `keys` — `Enter`, `Ctrl+A` — on whatever holds the focus.
///
/// # Errors
///
/// [`DriverError::Failed`] for keys the executor cannot parse.
pub fn press(app: &mut App, keys: &str) -> Result<(), DriverError> {
    let action = format!("press {keys}");
    match ask(
        app,
        RequestBody::Press {
            keys: keys.to_owned(),
        },
        &action,
    )? {
        ResponseBody::Pressed => Ok(()),
        other => Err(unexpected(app, &action, &other)),
    }
}

/// Wait for the nodes `locator` names to satisfy `condition`, and return
/// them — an expectation, evaluated every frame in the app until it holds or
/// the deadline passes.
///
/// # Errors
///
/// [`DriverError::Failed`] when it does not come to hold, with the last nodes
/// observed.
pub fn expect(
    app: &mut App,
    locator: &Locator,
    condition: WaitCondition,
) -> Result<Vec<UiNode>, DriverError> {
    let action = format!("expect {locator} to be {condition}");
    wait_as(app, locator, condition, &action)
}

/// Expect some node `locator` names, and every one of them, to be disabled.
///
/// # Errors
///
/// As [`expect`].
pub fn expect_disabled(app: &mut App, locator: &Locator) -> Result<Vec<UiNode>, DriverError> {
    expect(app, locator, WaitCondition::Disabled)
}

/// Expect some node `locator` names, and none disabled.
///
/// # Errors
///
/// As [`expect`].
pub fn expect_enabled(app: &mut App, locator: &Locator) -> Result<Vec<UiNode>, DriverError> {
    expect(app, locator, WaitCondition::Enabled)
}

/// Expect no node `locator` names to be seen (none matching counts).
///
/// # Errors
///
/// As [`expect`].
pub fn expect_hidden(app: &mut App, locator: &Locator) -> Result<Vec<UiNode>, DriverError> {
    expect(app, locator, WaitCondition::Hidden)
}

/// Expect some node `locator` names to be seen.
///
/// # Errors
///
/// As [`expect`].
pub fn expect_visible(app: &mut App, locator: &Locator) -> Result<Vec<UiNode>, DriverError> {
    expect(app, locator, WaitCondition::Visible)
}

/// An action on the one node `locator` names, answered with that node.
fn act(
    app: &mut App,
    verb: &str,
    locator: &Locator,
    body: RequestBody,
) -> Result<UiNode, DriverError> {
    let action = format!("{verb} {locator}");
    match ask(app, body, &action)? {
        ResponseBody::Done { node } => Ok(node),
        other => Err(unexpected(app, &action, &other)),
    }
}

/// The wait an action makes first, named as the driver names it.
fn wait_for(
    app: &mut App,
    locator: &Locator,
    condition: WaitCondition,
    verb: &str,
) -> Result<Vec<UiNode>, DriverError> {
    let action = format!("{verb}: wait for {locator} to be {condition}");
    wait_as(app, locator, condition, &action)
}

/// Wait for `locator`'s matches to satisfy `condition`, failing as `action`.
fn wait_as(
    app: &mut App,
    locator: &Locator,
    condition: WaitCondition,
    action: &str,
) -> Result<Vec<UiNode>, DriverError> {
    let body = RequestBody::WaitFor {
        locator: locator.clone(),
        condition,
        deadline: deadline(app),
    };
    match ask(app, body, action)? {
        ResponseBody::Satisfied { nodes } => Ok(nodes),
        other => Err(unexpected(app, action, &other)),
    }
}

/// Submit `body` to the app's executor and step the app until it answers;
/// a failure comes back as the driver's, named `action`.
fn ask(app: &mut App, body: RequestBody, action: &str) -> Result<ResponseBody, DriverError> {
    install(app);
    let id = {
        let mut next = app.world_mut().resource_mut::<NextInAppId>();
        let id = RequestId(next.0);
        next.0 = next.0.saturating_add(1);
        id
    };
    let (frame_limit, wall_limit) = limits(deadline(app));
    app.world_mut()
        .resource_mut::<AutomationQueue>()
        .submit(Request { id, body });
    let started = Instant::now();
    let mut frames = 0_u32;
    loop {
        app.update();
        frames = frames.saturating_add(1);
        let response = app
            .world_mut()
            .resource_mut::<AutomationQueue>()
            .take_response(id);
        if let Some(response) = response {
            return response.result.map_err(|error| {
                DriverError::Failed(Box::new(Failure {
                    viewer: label(app),
                    action: action.to_owned(),
                    error,
                    report: response.report,
                    artifacts: Artifacts::default(),
                }))
            });
        }
        let waited = started.elapsed();
        if frames >= frame_limit || waited >= wall_limit {
            return Err(DriverError::NoAnswer {
                viewer: label(app),
                what: action.to_owned(),
                waited,
            });
        }
    }
}

/// The most frames and wall-clock time to step for a request sent with
/// `deadline`, slack included.
fn limits(deadline: Deadline) -> (u32, Duration) {
    let frames = deadline
        .frames
        .unwrap_or(crate::DEFAULT_DEADLINE_FRAMES)
        .saturating_add(GRACE_FRAMES);
    let wall = deadline
        .millis
        .map_or(crate::DEFAULT_DEADLINE, Duration::from_millis)
        .saturating_add(GRACE);
    (frames, wall)
}

/// The deadline the app's requests are sent with.
fn deadline(app: &App) -> Deadline {
    app.world()
        .get_resource::<Options>()
        .cloned()
        .unwrap_or_default()
        .deadline
}

/// What the app's failures call it.
fn label(app: &App) -> String {
    app.world()
        .get_resource::<Options>()
        .cloned()
        .unwrap_or_default()
        .label
}

/// The error for an answer of the wrong kind to `action`.
fn unexpected(app: &App, action: &str, got: &ResponseBody) -> DriverError {
    DriverError::Unexpected {
        viewer: label(app),
        what: action.to_owned(),
        got: format!("{got:?}"),
    }
}

/// The error for something `action` needed that the app does not have.
fn missing(app: &App, action: &str, what: &str) -> DriverError {
    DriverError::Unexpected {
        viewer: label(app),
        what: action.to_owned(),
        got: what.to_owned(),
    }
}

#[cfg(test)]
mod tests;
