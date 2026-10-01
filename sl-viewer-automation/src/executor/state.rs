//! The requests on the viewer's state: reading a probe, the event log and
//! the diagnostics, waiting for a state condition, and a screenshot.

use std::path::PathBuf;

use bevy::prelude::*;
use sl_automation_proto::{
    AutomationError, Deadline, InventoryRoot, Locator, LogEntry, LogStream, PROTOCOL_VERSION,
    Probe, ProbeReadout, ResponseBody, StateCondition, StateObservation, UiNode, ViewerIdentity,
};

use super::{Answer, AutomationIdentity, Clock, Started, Step, Task};
use crate::event_log::EventLog;
use crate::locate::{find_all, shallow};
use crate::probes::{
    ProbeError, read_agent, read_conversations, read_environment, read_inventory,
    read_notifications, read_quiescence, read_selection, read_status,
};
use crate::screenshot::{ScreenshotTicket, request_screenshot, take_screenshot};
use crate::ui_model::snapshot;

/// Read `probe`.
///
/// # Errors
///
/// [`AutomationError::Unavailable`] when the viewer keeps no such model, and
/// [`AutomationError::InventoryFolderNotFound`] for an inventory path that
/// names no folder the viewer knows.
pub(super) fn read(world: &mut World, probe: &Probe) -> Result<ProbeReadout, Box<AutomationError>> {
    Ok(match probe {
        Probe::Agent => ProbeReadout::Agent(read_agent(world)),
        Probe::Status => ProbeReadout::Status(read_status(world)),
        Probe::Conversations => ProbeReadout::Conversations(
            read_conversations(world).map_err(|error| probe_error(error, InventoryRoot::Agent))?,
        ),
        Probe::Notifications => ProbeReadout::Notifications(read_notifications(world)),
        Probe::Selection => ProbeReadout::Selection(read_selection(world)),
        Probe::Inventory { root, path } => ProbeReadout::Inventory(Some(
            read_inventory(world, *root, path).map_err(|error| probe_error(error, *root))?,
        )),
        Probe::Quiescence => ProbeReadout::Quiescence(read_quiescence(world)),
        Probe::Environment => ProbeReadout::Environment(read_environment(world)),
    })
}

/// The answer to a hello: the protocol version, and who this viewer is — its
/// [`AutomationIdentity`], its process and, once logged in, its agent.
pub(super) fn hello(world: &mut World) -> ResponseBody {
    let AutomationIdentity {
        viewer,
        version,
        grid,
        agent_name,
    } = world
        .get_resource::<AutomationIdentity>()
        .cloned()
        .unwrap_or_default();
    ResponseBody::Hello {
        protocol: PROTOCOL_VERSION,
        viewer: ViewerIdentity {
            viewer,
            version,
            pid: std::process::id(),
            grid,
            agent_name,
            agent_id: read_agent(world).agent_id,
        },
    }
}

/// A probe's failure as the protocol's error; `root` is the inventory an
/// inventory path started in.
fn probe_error(error: ProbeError, root: InventoryRoot) -> Box<AutomationError> {
    Box::new(match error {
        ProbeError::Unavailable { probe } => AutomationError::Unavailable {
            what: probe.to_owned(),
        },
        ProbeError::NoSuchFolder {
            path,
            index,
            segment: _,
        } => AutomationError::InventoryFolderNotFound {
            root,
            path,
            index: u32::try_from(index).unwrap_or(u32::MAX),
        },
    })
}

/// Read the event log from `cursor`.
///
/// # Errors
///
/// [`AutomationError::Unavailable`] when the app keeps no event log.
pub(super) fn read_log(
    world: &World,
    cursor: u64,
    streams: &[LogStream],
    limit: Option<u32>,
) -> Answer {
    let log = event_log(world)?;
    let limit = limit.map_or(usize::MAX, |limit| {
        usize::try_from(limit).unwrap_or(usize::MAX)
    });
    Ok(ResponseBody::Log {
        page: log.read(cursor, streams, limit),
    })
}

/// The app's event log.
fn event_log(world: &World) -> Result<&EventLog, Box<AutomationError>> {
    world.get_resource::<EventLog>().ok_or_else(|| {
        Box::new(AutomationError::Unavailable {
            what: "event log".to_owned(),
        })
    })
}

/// A wait until `condition` holds.
pub(super) fn wait(condition: StateCondition, deadline: Deadline) -> Started {
    if let StateCondition::Probe { pointer, .. } = &condition
        && !(pointer.is_empty() || pointer.starts_with('/'))
    {
        return Started::answered(Err(Box::new(AutomationError::InvalidRequest {
            reason: format!("{pointer:?} is not a JSON Pointer: it must be empty or start with /"),
        })));
    }
    let cursor = match &condition {
        StateCondition::Logged { cursor, .. } => *cursor,
        StateCondition::Quiet | StateCondition::Probe { .. } => 0,
    };
    Started::Running(Task::state(StateTask::Wait(Box::new(StateWait {
        condition,
        clock: Clock::new(deadline),
        cursor,
        last_observed: None,
    }))))
}

/// A screenshot of the window to `path`, with `outline`'s matches outlined.
pub(super) fn screenshot(world: &mut World, path: String, outline: Option<Locator>) -> Started {
    let file = PathBuf::from(&path);
    if !file.is_absolute() {
        return Started::answered(Err(Box::new(AutomationError::InvalidRequest {
            reason: format!("the screenshot path {path:?} is not absolute"),
        })));
    }
    let outlined = match outline {
        Some(locator) => {
            let roots = match snapshot(world) {
                Ok(roots) => roots,
                Err(error) => {
                    return Started::answered(Err(Box::new(AutomationError::Unavailable {
                        what: format!("readable semantic model ({error})"),
                    })));
                }
            };
            match find_all(&roots, &locator) {
                Ok(found) => found.into_iter().map(shallow).collect(),
                Err(error) => return Started::answered(Err(error)),
            }
        }
        None => Vec::new(),
    };
    let boxes = outlined.iter().map(|node: &UiNode| node.bounds).collect();
    let ticket = request_screenshot(world, boxes);
    Started::Running(Task::state(StateTask::Screenshot(Capture {
        ticket,
        path,
        outlined,
        clock: Clock::new(Deadline::default()),
    })))
}

/// A state request under way.
pub(super) enum StateTask {
    /// A wait for a condition.
    Wait(Box<StateWait>),
    /// A screenshot being rendered.
    Screenshot(Capture),
}

impl StateTask {
    /// Advance it by a frame.
    pub(super) fn poll(&mut self, world: &mut World) -> Step {
        match self {
            Self::Wait(wait) => wait.poll(world),
            Self::Screenshot(capture) => capture.poll(world),
        }
    }
}

/// A wait until a state condition holds.
pub(super) struct StateWait {
    /// The condition.
    condition: StateCondition,
    /// Its deadline.
    clock: Clock,
    /// For a log condition, where the next read starts.
    cursor: u64,
    /// What it saw last.
    last_observed: Option<StateObservation>,
}

impl StateWait {
    /// Advance it by a frame.
    fn poll(&mut self, world: &mut World) -> Step {
        self.clock.tick();
        match self.observe(world) {
            Ok(Some(observed)) => return Step::done(ResponseBody::StateHeld { observed }),
            Ok(None) => {}
            Err(error) => return Step::fail(error),
        }
        match self.clock.expired() {
            Some((frames, millis)) => Step::fail(AutomationError::StateTimedOut {
                condition: self.condition.clone(),
                last_observed: self.last_observed.take(),
                frames,
                millis,
            }),
            None => Step::Pending,
        }
    }

    /// Look once: what made the condition hold, or `None` (keeping what was
    /// seen as the last observation).
    fn observe(
        &mut self,
        world: &mut World,
    ) -> Result<Option<StateObservation>, Box<AutomationError>> {
        match &self.condition {
            StateCondition::Quiet => {
                let readout = read_quiescence(world);
                let quiet = readout.is_quiet();
                let observed = StateObservation::Quiet { readout };
                Ok(self.judge(observed, quiet))
            }
            StateCondition::Probe {
                probe,
                pointer,
                test,
            } => {
                let readout = match read(world, probe) {
                    Ok(readout) => readout,
                    // The folder may yet arrive: its absence is a readout.
                    Err(error)
                        if matches!(*error, AutomationError::InventoryFolderNotFound { .. }) =>
                    {
                        ProbeReadout::Inventory(None)
                    }
                    Err(error) => return Err(error),
                };
                let json = serde_json::to_value(&readout).map_err(|error| {
                    Box::new(AutomationError::Unavailable {
                        what: format!("JSON form of the {probe} readout ({error})"),
                    })
                })?;
                // The readout's JSON wraps it as {"probe":…, "readout":…}.
                let value = json
                    .get("readout")
                    .and_then(|readout| readout.pointer(pointer));
                let holds = test.holds(value);
                Ok(self.judge(StateObservation::Probe { readout }, holds))
            }
            StateCondition::Logged { streams, .. } => {
                let page = event_log(world)?.read(self.cursor, streams, usize::MAX);
                self.cursor = page.next;
                let matched: Option<LogEntry> = page
                    .entries
                    .iter()
                    .find(|entry| self.condition.accepts_entry(entry))
                    .cloned();
                if let Some(entry) = matched {
                    let next = entry.seq.saturating_add(1);
                    return Ok(Some(StateObservation::Logged { entry, next }));
                }
                if let Some(entry) = page.entries.into_iter().last() {
                    self.last_observed = Some(StateObservation::Logged {
                        entry,
                        next: page.next,
                    });
                }
                Ok(None)
            }
        }
    }

    /// `observed` when the condition `holds`; else keep it as the last seen.
    fn judge(&mut self, observed: StateObservation, holds: bool) -> Option<StateObservation> {
        if holds {
            return Some(observed);
        }
        self.last_observed = Some(observed);
        None
    }
}

/// A screenshot being rendered.
pub(super) struct Capture {
    /// Its ticket.
    ticket: ScreenshotTicket,
    /// Where to write it.
    path: String,
    /// The nodes outlined on it.
    outlined: Vec<UiNode>,
    /// How long to wait for a frame.
    clock: Clock,
}

impl Capture {
    /// Advance it by a frame.
    fn poll(&mut self, world: &mut World) -> Step {
        self.clock.tick();
        let Some(frame) = take_screenshot(world, self.ticket) else {
            return match self.clock.expired() {
                Some((frames, millis)) => Step::fail(AutomationError::ScreenshotFailed {
                    reason: format!("no frame was rendered in {frames} frames ({millis} ms)"),
                }),
                None => Step::Pending,
            };
        };
        let written = frame.map_err(|error| error.to_string()).and_then(|frame| {
            let png = frame.to_png().map_err(|error| error.to_string())?;
            fs_err::write(&self.path, png).map_err(|error| error.to_string())?;
            Ok((frame.width, frame.height))
        });
        match written {
            Ok((width, height)) => Step::done(ResponseBody::Screenshot {
                path: self.path.clone(),
                width,
                height,
                outlined: core::mem::take(&mut self.outlined),
            }),
            Err(reason) => Step::fail(AutomationError::ScreenshotFailed { reason }),
        }
    }
}
