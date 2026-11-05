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
//! - [`permissions`] — what a script may do to an avatar: the grant it
//!   holds, who made it and the question waiting for an answer.
//! - [`economy`] — what a script pays and is paid: the payments it hands the
//!   host, and the prim's pay layout.
//! - [`experiences`] — what a script in an experience asks of an avatar once
//!   for the experience, and the key-value store the experience keeps on the
//!   grid.
//! - [`ossl`] — OpenSim's own library families as its engines gate them:
//!   the region's `[OSSL]` configuration, what the script's owner is in the
//!   region, and the check a call passes before it runs.
//! - [`persistence`] — a script's running state saved as plain data
//!   ([`SavedScript`]) when its object is taken or its region goes down, and
//!   brought back when it is rezzed or the region is up again.
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
pub mod economy;
mod engine;
pub mod experiences;
mod fault;
mod flavour;
mod host;
mod instance;
pub mod linkset_data;
pub mod memory;
pub(crate) mod objects;
pub mod ossl;
pub mod permissions;
pub mod persistence;
pub mod prim;
mod report;
pub mod seating;
#[cfg(test)]
mod tests;

pub use comms::{
    Dialog, Heard, InstantMessage, InvalidLinkTarget, LinkMessage, Linked, LinkedAvatar,
    LinkedPrim, Links, ListenFilter, Listens, LoadUrl, MapDestination, OPENSIM_LISTENS,
    SECOND_LIFE_LISTENS, TEXT_BOX_BUTTON, TooManyListens,
};
#[cfg(test)]
pub(crate) use context::{Peers, ScriptData};
pub use context::{Raised, ScriptCtx, Tick};
pub use detected::{DETECTION_EVENTS, Detected, MAX_DETECTED, Touch, is_detection_event};
pub use economy::{
    MoneyRefusal, PAY_DEFAULT, PAY_HIDE, PAYMENT_STOPPED, PayPrice, Payment, PaymentAdmission,
    SECOND_LIFE_DEFAULT_BUTTONS, TransactionResult,
};
pub use engine::{
    Engine, EngineConfig, REGION_SCRIPT_SHARES, SCRIPT_INSTRUCTIONS_PER_SECOND, ScriptUsage,
    TickReport,
};
pub use experiences::{
    Admission, EXPERIENCE_PERMISSIONS, ExperienceDetails, ExperienceError, ExperienceQuestion,
    KeyValueStore, ScriptExperience, SitRefusal,
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
pub use ossl::{Allowed, Simulator, Standing, Thrown};
pub use permissions::{Permissions, Question};
pub use persistence::{
    CHANGED_REGION_START, DecodeError, OPENSIM_CHANGED_POSITION, RestoreError, SavedScript,
};
pub use report::{
    DEBUG_CHANNEL, FLOOD_GAP_SECONDS, FLOOD_LINES, FLOOD_NOTICE, FLOOD_SECONDS,
    FLOOD_WINDOW_SECONDS, Speech, Warning, Whereabouts,
};
