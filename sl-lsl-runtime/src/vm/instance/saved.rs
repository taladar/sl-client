//! An instance saved to a [`SavedScript`] and brought back from one.

use std::collections::VecDeque;
use std::sync::Arc;

use core::time::Duration;

use crate::bytecode::{Program, StateId};
use crate::library;
use crate::value::Value;
use crate::vm::context::{ScriptData, Tick, Timer};
use crate::vm::flavour::EngineFlavour;
use crate::vm::memory::{Memory, Stack, frame_cost};
use crate::vm::objects::ListId;
use crate::vm::persistence::{
    RestoreError, SavedActivity, SavedBody, SavedEvent, SavedFrame, SavedPhase, SavedScript,
    SavedTimer, is_of, span, ticks_in,
};

use super::{Activity, BodyRef, Frame, Instance, Phase, Queued, body_of, handler_index};

/// Save `instance` as it stands at `now`, a tick of length `step`: its
/// sleep and its timer as the time left of them.
pub(crate) fn save(instance: &Instance, now: Tick, step: Duration) -> SavedScript {
    SavedScript {
        flavour: instance.memory.flavour(),
        globals: instance.globals.clone(),
        objects: instance.objects.clone(),
        state: instance.state,
        running: instance.running,
        held: instance.held,
        queue: instance
            .queue
            .iter()
            .map(|queued| SavedEvent {
                event: queued.event.name.to_owned(),
                args: queued.args.clone(),
                detected: queued.detected.clone(),
            })
            .collect(),
        activity: instance.activity.as_ref().map(|activity| SavedActivity {
            phase: match activity.phase {
                Phase::Init => SavedPhase::Init,
                Phase::Event => SavedPhase::Event,
                Phase::Entry => SavedPhase::Entry,
                Phase::Exit(target) => SavedPhase::Exit(target),
            },
            frames: activity
                .frames
                .iter()
                .map(|frame| SavedFrame {
                    body: match frame.body {
                        BodyRef::Init => SavedBody::Init,
                        BodyRef::Function(id) => SavedBody::Function(id),
                        BodyRef::Handler { state, index } => SavedBody::Handler {
                            state,
                            index: u32::try_from(index).unwrap_or(u32::MAX),
                        },
                    },
                    pc: frame.pc,
                    locals: frame.locals.clone(),
                    objects: frame.objects.clone(),
                    costs: frame.costs.clone(),
                })
                .collect(),
            stack: activity.stack.clone(),
            slept_first: activity.slept_first,
        }),
        pending_state: instance.pending_state,
        asleep: span(instance.wake.0.saturating_sub(now.0), step),
        timer: instance.data.timer.map(|timer| SavedTimer {
            interval: timer.interval,
            next: timer
                .next
                .saturating_sub(crate::vm::context::region_time(now, step)),
        }),
        start_parameter: instance.data.start_parameter,
        script_time: span(instance.data.script_ticks, step),
        detected: instance.data.detected.clone(),
        listens: instance.data.listens.clone(),
        permissions: instance.data.permissions.clone(),
        memory: instance.memory.saved(),
    }
}

/// Bring `saved` back as an instance of `program` in a region imitating
/// `flavour`, whose clock reads `now` with ticks of length `step`: the
/// sleep and the timer go on from what was left of them.
///
/// # Errors
///
/// [`RestoreError`] when `saved` was saved under another engine, or names
/// a global, a state, a body or an event `program` does not have.
pub(crate) fn restore(
    program: Arc<Program>,
    saved: &SavedScript,
    flavour: EngineFlavour,
    now: Tick,
    step: Duration,
) -> Result<Instance, RestoreError> {
    if saved.flavour != flavour {
        return Err(RestoreError::Flavour {
            saved: saved.flavour,
            region: flavour,
        });
    }
    let globals_fit = saved.globals.len() == program.globals.len()
        && saved
            .globals
            .iter()
            .zip(&program.globals)
            .all(|(value, global)| is_of(value, global.ty))
        && objects_fit(&saved.globals, &saved.objects.globals);
    if !globals_fit {
        return Err(RestoreError::Globals);
    }
    let state = known_state(&program, saved.state)?;
    let pending_state = saved
        .pending_state
        .map(|target| known_state(&program, target))
        .transpose()?;
    // Posted before anything the region posts from now on, so nothing new
    // merges into an event saved with the script.
    let posted = Tick(now.0.saturating_sub(1));
    let queue = saved
        .queue
        .iter()
        .map(|saved_event| queued(&program, state, saved_event, posted))
        .collect::<Result<VecDeque<_>, _>>()?;
    let activity = saved
        .activity
        .as_ref()
        .map(|activity| restored_activity(&program, activity, flavour))
        .transpose()?;
    let memory = Memory::restored(&program, flavour, &saved.globals, saved.memory);
    let asleep = ticks_in(saved.asleep, step);
    Ok(Instance {
        globals: saved.globals.clone(),
        objects: saved.objects.clone(),
        state,
        running: saved.running,
        held: saved.held,
        queue,
        begun: 0,
        activity,
        pending_state,
        wake: if asleep == 0 {
            Tick::default()
        } else {
            now.after(asleep)
        },
        data: ScriptData {
            detected: saved.detected.clone(),
            timer: saved.timer.map(|timer| Timer {
                interval: timer.interval,
                next: crate::vm::context::region_time(now, step).saturating_add(timer.next),
                fired: true,
            }),
            drop_queued_timer: false,
            start_parameter: saved.start_parameter,
            start_string: String::new(),
            sweeps: Vec::new(),
            repeat: None,
            drop_queued_sensor: false,
            // Script time counts on through a take, a rez and a restart on
            // Second Life and YEngine; XEngine's reset of the script on a rez
            // and a restart starts it again (`persistence/rerez`,
            // `persistence/restart`).
            script_ticks: if flavour == EngineFlavour::XEngine {
                0
            } else {
                ticks_in(saved.script_time, step)
            },
            refusals: Vec::new(),
            raised: Vec::new(),
            listens: saved.listens.clone(),
            permissions: saved.permissions.clone(),
            // A URL is the region's, gone with a take, a rez or a restart, and
            // with it every request in flight.
            http: crate::vm::context::HttpState::default(),
            // Controls are the avatar's, taken again by a script that wants
            // them after a take, a rez or a restart.
            controls: None,
            // OpenSim's `LSL_Api` is made anew for the restored script.
            say_throttle: crate::vm::delays::SayThrottle::default(),
        },
        reset_when_idle: false,
        overdraft: 0,
        memory,
        charged: 0,
        load: 0.0,
        program,
    })
}

/// `state`, if `program` has it.
fn known_state(program: &Program, state: StateId) -> Result<StateId, RestoreError> {
    program
        .state(state)
        .map(|_found| state)
        .ok_or(RestoreError::State(state.0))
}

/// A saved event queued again in `state`, as posted at `posted`.
fn queued(
    program: &Program,
    state: StateId,
    saved: &SavedEvent,
    posted: Tick,
) -> Result<Queued, RestoreError> {
    let refused = || RestoreError::Event(saved.event.clone());
    let event = library::event(&saved.event).ok_or_else(refused)?;
    let handler = handler_index(program, state, event.name).ok_or_else(refused)?;
    let args_fit = saved.args.len() == event.args.len()
        && saved
            .args
            .iter()
            .zip(event.args)
            .all(|(value, parameter)| is_of(value, parameter.ty));
    if !args_fit {
        return Err(refused());
    }
    Ok(Queued {
        event,
        handler,
        args: saved.args.clone(),
        detected: saved.detected.clone(),
        posted,
        // A restored event is free to run: what held it back was the frame
        // it was raised in, long over.
        ready: posted,
        batch: 0,
    })
}

/// A saved body in progress, checked against `program` and costed as
/// `flavour` costs a frame.
fn restored_activity(
    program: &Program,
    saved: &SavedActivity,
    flavour: EngineFlavour,
) -> Result<Activity, RestoreError> {
    if saved.frames.is_empty() {
        return Err(RestoreError::NoFrame);
    }
    let frames = saved
        .frames
        .iter()
        .map(|frame| restored_frame(program, frame, flavour))
        .collect::<Result<Vec<_>, _>>()?;
    let held = frames.iter().fold(Stack::default(), |held, frame| Stack {
        frames: held.frames.saturating_add(frame.cost),
        locals: frame
            .costs
            .iter()
            .fold(held.locals, |locals, cost| locals.saturating_add(*cost)),
    });
    let phase = match saved.phase {
        SavedPhase::Init => Phase::Init,
        SavedPhase::Event => Phase::Event,
        SavedPhase::Entry => Phase::Entry,
        SavedPhase::Exit(target) => Phase::Exit(known_state(program, target)?),
    };
    Ok(Activity {
        phase,
        frames,
        stack: saved.stack.clone(),
        held,
        slept_first: saved.slept_first,
    })
}

/// A saved frame, checked against its body in `program`.
fn restored_frame(
    program: &Program,
    saved: &SavedFrame,
    flavour: EngineFlavour,
) -> Result<Frame, RestoreError> {
    let body = match saved.body {
        SavedBody::Init => BodyRef::Init,
        SavedBody::Function(id) => {
            program.function(id).ok_or(RestoreError::Function(id.0))?;
            BodyRef::Function(id)
        }
        SavedBody::Handler { state, index } => BodyRef::Handler {
            state,
            index: usize::try_from(index).map_err(|_overflow| RestoreError::Handler {
                state: state.0,
                index,
            })?,
        },
    };
    let code = body_of(program, body).map_err(|_missing| match saved.body {
        SavedBody::Handler { state, index } => RestoreError::Handler {
            state: state.0,
            index,
        },
        SavedBody::Init | SavedBody::Function(_) => RestoreError::Frame,
    })?;
    let fits = usize::try_from(saved.pc).is_ok_and(|pc| pc <= code.code.len())
        && saved.locals.len() == code.locals.len()
        && saved.costs.len() == code.locals.len()
        && saved
            .locals
            .iter()
            .zip(&code.locals)
            .all(|(value, local)| is_of(value, local.ty))
        && objects_fit(&saved.locals, &saved.objects);
    if !fits {
        return Err(RestoreError::Frame);
    }
    Ok(Frame {
        body,
        pc: saved.pc,
        locals: saved.locals.clone(),
        objects: saved.objects.clone(),
        costs: saved.costs.clone(),
        cost: frame_cost(code, flavour),
    })
}

/// Whether `objects` are one slot per value of `values`, an object only
/// beside a list.
fn objects_fit(values: &[Value], objects: &[Option<ListId>]) -> bool {
    values.len() == objects.len()
        && values
            .iter()
            .zip(objects)
            .all(|(value, object)| object.is_none() || matches!(value, Value::List(_)))
}
