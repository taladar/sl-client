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
//! - [`RuntimeError`] / [`Fault`] — what stops a script. An error stops the
//!   one script and never panics the region.
//!
//! Time is counted in [`Tick`]s, never read from a clock, and budgets in
//! instructions, never in elapsed time: a script's observable behaviour must
//! not depend on the machine it runs on.

mod context;
mod detected;
mod engine;
mod fault;
mod host;
mod instance;
#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) use context::{Peers, ScriptData};
pub use context::{ScriptCtx, Tick};
pub use detected::{DETECTION_EVENTS, Detected, MAX_DETECTED, Touch, is_detection_event};
pub use engine::{
    Engine, EngineConfig, REGION_SCRIPT_SHARES, SCRIPT_INSTRUCTIONS_PER_SECOND, TickReport,
};
pub use fault::{Fault, RuntimeError};
pub use host::{CallerId, Host};
pub use instance::{
    Arrival, BACKWARD_JUMP_COST, BUILTIN_CALL_COST, Coalescing, Instance, MAX_CALL_DEPTH,
    MAX_QUEUED, Outcome, PostError, Posted, Slice,
};
