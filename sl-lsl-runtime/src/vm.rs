//! The **VM**: running a compiled [`Program`](crate::bytecode::Program) in
//! bounded slices, so a region runs many scripts on one thread and none of
//! them can stall it.
//!
//! The defining property is not speed but that **execution can stop anywhere
//! and resume later**. Everything about where a script is — its program
//! counter, call frames and operand stack — is plain data in its
//! [`Instance`], so suspending is returning from [`Instance::run_slice`] and
//! resuming is calling it again. `llSleep`, a library function's forced
//! delay, an exhausted budget, a state change and `llResetScript` all work
//! that way.
//!
//! - [`Instance`] — one script: a shared program plus its own globals,
//!   state, event queue, run flag and the body in progress;
//!   [`Instance::run_slice`] advances it by at most a budget of instructions
//!   and says how it stopped ([`Outcome`]).
//! - [`Engine`] — a region's instances, served round-robin each
//!   [`Engine::tick`] with a per-script and a region-wide instruction budget
//!   ([`EngineConfig`]).
//! - [`Host`] — the world, as the library reaches it; [`ScriptCtx`] — the
//!   calling script, as a library function sees it.
//! - [`memory`] — a script's memory as its grid counts it: what the memory
//!   functions answer and where the script runs out.
//! - [`comms`] — how scripts talk: a script's listens and the lines it
//!   hears, the dialogs, instant messages and prompts it hands the host, and
//!   who a link message reaches.
//! - [`linkset_data`] — the key/value store a linkset owns, which the host
//!   keeps and lends to the library, and the `linkset_data` event a change
//!   raises in every script of the linkset.
//! - [`RuntimeError`] / [`Fault`] — what stops a script. An error stops the
//!   one script and never panics the region; the engine says it on
//!   [`DEBUG_CHANNEL`] in the words of the grid it imitates
//!   ([`EngineFlavour`]) through [`Host::say`].
//!
//! Time is counted in [`Tick`]s, never read from a clock, and budgets in
//! instructions, never in elapsed time: a script's observable behaviour must
//! not depend on the machine it runs on.

pub mod comms;
mod context;
mod cost;
mod detected;
mod engine;
mod fault;
mod flavour;
mod host;
mod instance;
pub mod linkset_data;
pub mod memory;
mod report;
#[cfg(test)]
mod tests;

pub use comms::{
    Dialog, Heard, InstantMessage, LinkMessage, LinkedPrim, Links, ListenFilter, Listens, LoadUrl,
    MapDestination, OPENSIM_LISTENS, SECOND_LIFE_LISTENS, TEXT_BOX_BUTTON, TooManyListens,
};
#[cfg(test)]
pub(crate) use context::{Peers, ScriptData};
pub use context::{Raised, ScriptCtx, Tick};
pub use detected::{DETECTION_EVENTS, Detected, MAX_DETECTED, Touch, is_detection_event};
pub use engine::{
    Engine, EngineConfig, REGION_SCRIPT_SHARES, SCRIPT_INSTRUCTIONS_PER_SECOND, ScriptUsage,
    TickReport,
};
pub use fault::{Fault, RuntimeError};
pub use flavour::EngineFlavour;
pub use host::{CallerId, Host};
pub use instance::{
    Arrival, BACKWARD_JUMP_COST, BUILTIN_CALL_COST, Coalescing, Instance, LOAD_WINDOW_SECONDS,
    MAX_CALL_DEPTH, MAX_QUEUED, OPENSIM_SCRIPT_MEMORY, Outcome, PostError, Posted, Slice,
};
pub use linkset_data::{Answer, LinksetAction, LinksetChange, LinksetData, LinksetId};
#[cfg(test)]
pub(crate) use memory::{Memory, Stack};
pub use report::{
    DEBUG_CHANNEL, FLOOD_LINES, FLOOD_NOTICE, FLOOD_QUIET_SECONDS, Speech, Whereabouts,
};
