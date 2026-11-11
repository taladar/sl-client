//! A script's memory as the script observes it: what `llGetUsedMemory`,
//! `llGetFreeMemory`, `llGetMemoryLimit` and `llGetSPMaxMemory` answer, what
//! `llSetMemoryLimit` accepts, and where the script runs out — a Mono
//! stack-heap collision, a YEngine `OutOfHeapException`.
//!
//! A Rust VM needs none of this, and that is the point: content reads its
//! memory and branches on it (`llGetFreeMemory() < 2000`), and runs out where
//! the grid it was written for runs out. Every number is the book's (chapter
//! `gridspec/lsl.md`, § Memory and limits); the per-value costs are
//! [`value::memory`](crate::value::memory)'s. Two parts are **models**
//! fitted to measurements, not measurements, and say so:
//!
//! - **Mono's code.** `llGetUsedMemory` counts the script's compiled code,
//!   which this runtime does not have — it runs its own bytecode. The code
//!   is costed as [`MONO_SCRIPT_CODE`], [`MONO_BODY_CODE`] a function or
//!   event handler and [`MONO_INSTRUCTION_CODE`] an instruction of ours,
//!   fitted by least squares to the first reading of eight probe scripts on
//!   aditi (`memory/sizes`' Globals none, `memory/recursion`'s Narrow, Wide
//!   and Limited, `memory/collision`'s Doubling, Full list and Limited list,
//!   `memory/budget`): every one within 300 bytes, about the reading's own
//!   wander on the grid, once the two call contexts each first reading
//!   carries (below) are taken off. A string literal costs nothing of its
//!   own — the eight scripts fit as well without it, and the one row that
//!   seemed to say otherwise is a global's value, which is costed as one.
//! - **Mono's stack.** Fitted to the depths aditi reached in `memory/stack`
//!   (five runs, 2026-11-11) and `memory/recursion`, it has two parts:
//!   - A frame is [`MONO_FRAME`] and the size of each of its slots
//!     ([`slot_size`]), which never show in `llGetUsedMemory`. Frame,
//!     slots, heap and locals meet in the script's 64 KB whatever its limit;
//!     against a limit `llSetMemoryLimit` lowered only the slots count.
//!   - A frame that has made a library call holds [`MONO_CALL_CONTEXT`]
//!     more on the heap until it returns, which `llGetUsedMemory` reads:
//!     Narrow's use rose 30 a level on aditi.
//!
//!   So a recursion that calls the library at every level goes half as deep
//!   as one that calls it every sixteenth: aditi's Quiet reached 2 256 to
//!   2 464 (2 272 here), Narrow 1 039 to 1 116 (1 092) and Litter, which
//!   drops a 104-character string at every level too, 1 040 to 1 280
//!   (1 072). Garbage does not cut the depth (Litter's is collected). Over
//!   32 KB of heap Heavy reached 912 to 1 712 (1 024), and Wide, five
//!   arguments and two locals, 413 to 518 (424). Under a limit lowered 4 096
//!   bytes over the use, Quiet limited reached 560 to 928 (672). There
//!   aditi's Limited, which calls the library at every level, reached 129 to
//!   253 (118 here), and the Limited list 540 elements under 8 192 (500):
//!   some 600 to 1 000 bytes more room than the model's, likely the garbage
//!   aditi's reading carried when the limit was set, collected later, which
//!   has no model here.
//!
//! What collides is checked where memory grows: a global or a local stored
//! bigger, a call made, a frame's first library call, and a value made that
//! alone could never fit. A store that makes nothing bigger cannot collide,
//! even past the limit. That is enough to match the measured cases —
//! aditi's Full list collides making 3 660 elements, as it does on the fake
//! grid (3 650 in the unit test's rewrite of the probe), although
//! appending makes a second copy of the list, so a value on its way to a
//! store is not counted beside the one it replaces.
//!
//! `llSetMemoryLimit` takes a limit down to the use `llGetUsedMemory`
//! reads, call contexts included. Asked for exactly the use it read in the
//! same expression, aditi took it; asked for a use read before a call whose
//! frame has since made its call context, it refused, as it does here
//! (`memory/budget`).
//!
//! Second Life's LSO, with its 16 KB, is not modelled: the fake grid
//! compiles every script from source, so every script is Mono's.

use sl_lsl::ast::TypeName;

use crate::bytecode::{Body, Program};
use crate::value::Value;
use crate::value::memory::{Holder, Literals, MEMORY_LIMIT, YENGINE_HEAP, global_cost, value_cost};
use crate::vm::fault::RuntimeError;
use crate::vm::flavour::EngineFlavour;
use crate::vm::persistence::SavedMemory;

/// Mono: what the code of any script costs, before its bodies and
/// instructions — the fitted constant of the code model (module docs).
pub const MONO_SCRIPT_CODE: u32 = 3_209;

/// Mono: what each function and event handler's code costs over its
/// instructions.
pub const MONO_BODY_CODE: u32 = 342;

/// Mono: what each instruction of this runtime's bytecode stands for in
/// Mono's code.
pub const MONO_INSTRUCTION_CODE: u32 = 18;

/// Mono: what a call frame costs toward a stack-heap collision in the
/// script's 64 KB, over its slots ([`slot_size`]).
pub const MONO_FRAME: u32 = 20;

/// Mono: what a frame holds on the heap from its first library call until
/// it returns — read by `llGetUsedMemory` and counted against the limit
/// (module docs).
pub const MONO_CALL_CONTEXT: u32 = 30;

/// The most any one script may hold where its grid sets no limit of its own
/// — XEngine, and a single value on YEngine before it is stored. It is not a
/// grid's figure but the host's: a runaway allocation stops the script
/// ([`RuntimeError::HostMemory`]) rather than exhaust the machine.
pub const HOST_MEMORY_CAP: u32 = 0x0400_0000;

/// `llScriptProfiler`'s `PROFILE_SCRIPT_MEMORY`.
const PROFILE_SCRIPT_MEMORY: i32 = 1;

/// Mono: what a slot of type `ty` adds to its frame — a number 4 bytes, a
/// vector 12 and a rotation 16, held in the frame, and a string, key or
/// list 8, a reference to the object.
#[must_use]
pub const fn slot_size(ty: TypeName) -> u32 {
    match ty {
        TypeName::Integer | TypeName::Float => 4,
        TypeName::Vector => 12,
        TypeName::Rotation => 16,
        TypeName::String | TypeName::Key | TypeName::List => 8,
    }
}

/// What `program`'s code costs in `flavour`'s `llGetUsedMemory`: Mono's code
/// model (module docs); nothing on OpenSim, whose engines count values only.
#[must_use]
pub fn code_cost(program: &Program, flavour: EngineFlavour) -> u32 {
    if flavour != EngineFlavour::SecondLife {
        return 0;
    }
    let bodies = program
        .functions
        .iter()
        .map(|function| &function.body)
        .chain(
            program
                .states
                .iter()
                .flat_map(|state| state.handlers.iter().map(|handler| &handler.body)),
        );
    let (count, instructions) = bodies.fold(
        (0_u32, saturate(program.init.code.len())),
        |(count, instructions), body| {
            (
                count.saturating_add(1),
                instructions.saturating_add(saturate(body.code.len())),
            )
        },
    );
    MONO_SCRIPT_CODE
        .saturating_add(MONO_BODY_CODE.saturating_mul(count))
        .saturating_add(MONO_INSTRUCTION_CODE.saturating_mul(instructions))
}

/// What a frame of `body` costs toward running out in `flavour`: Mono's stack
/// model, [`MONO_FRAME`] and its slots against the script's 64 KB and the
/// slots alone against its limit; nothing on OpenSim, whose engines keep
/// their call stack apart from the heap they count.
#[must_use]
pub(crate) fn frame_cost(body: &Body, flavour: EngineFlavour) -> Stack {
    if flavour != EngineFlavour::SecondLife {
        return Stack::default();
    }
    let slots = body
        .locals
        .iter()
        .fold(0_u32, |sum, local| sum.saturating_add(slot_size(local.ty)));
    Stack {
        frames: MONO_FRAME.saturating_add(slots),
        slots,
        locals: 0,
        contexts: 0,
    }
}

/// What a frame's first library call adds to the call stack in `flavour`:
/// Mono's call context ([`MONO_CALL_CONTEXT`]); nothing on OpenSim.
#[must_use]
pub(crate) const fn context_cost(flavour: EngineFlavour) -> Stack {
    Stack {
        frames: 0,
        slots: 0,
        locals: 0,
        contexts: match flavour {
            EngineFlavour::SecondLife => MONO_CALL_CONTEXT,
            EngineFlavour::YEngine | EngineFlavour::XEngine => 0,
        },
    }
}

/// A length as a `u32`, saturating.
fn saturate(length: usize) -> u32 {
    u32::try_from(length).unwrap_or(u32::MAX)
}

/// What a script's call stack holds toward running out: the frames, and what
/// the locals stored in them hold.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Stack {
    /// Every frame's own cost against the script's 64 KB, [`MONO_FRAME`] and
    /// its slots ([`frame_cost`]).
    pub(crate) frames: u32,
    /// Every frame's slots alone, what the frames cost against a limit
    /// `llSetMemoryLimit` lowered.
    pub(crate) slots: u32,
    /// What every frame's stored locals hold ([`Memory::local_cost`]).
    pub(crate) locals: u32,
    /// Mono: the call contexts of the frames that have made a library call
    /// ([`MONO_CALL_CONTEXT`]), which are heap.
    pub(crate) contexts: u32,
}

impl Stack {
    /// This and `frame`'s frame cost pushed on it.
    pub(crate) const fn push(self, frame: Self) -> Self {
        Self {
            frames: self.frames.saturating_add(frame.frames),
            slots: self.slots.saturating_add(frame.slots),
            locals: self.locals.saturating_add(frame.locals),
            contexts: self.contexts.saturating_add(frame.contexts),
        }
    }

    /// This with `frame`'s frame cost popped off it.
    pub(crate) const fn pop(self, frame: Self) -> Self {
        Self {
            frames: self.frames.saturating_sub(frame.frames),
            slots: self.slots.saturating_sub(frame.slots),
            locals: self.locals.saturating_sub(frame.locals),
            contexts: self.contexts.saturating_sub(frame.contexts),
        }
    }
}

/// One script's memory accounting: what its code and globals cost, its limit,
/// and the most it has used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Memory {
    /// The grid's engine the script runs under.
    flavour: EngineFlavour,
    /// The flavour whose costs are counted: the script's own, but YEngine's
    /// for XEngine, which counts nothing a script can see and is held to
    /// [`HOST_MEMORY_CAP`] by YEngine's measure.
    model: EngineFlavour,
    /// The script's string literals, for Mono's sharing of them in a list.
    literals: Literals,
    /// The code ([`code_cost`]).
    code: u32,
    /// The globals' declarations, all together ([`global_cost`]).
    declared: u32,
    /// What each global's value costs over its declaration, slot by slot.
    globals: Vec<u32>,
    /// The sum of [`Self::globals`].
    held: u32,
    /// `llGetMemoryLimit`: [`MEMORY_LIMIT`] until `llSetMemoryLimit` lowers
    /// it, on Mono; always [`MEMORY_LIMIT`] on OpenSim.
    limit: u32,
    /// Mono: the most the script has used since its limit was last set,
    /// which `llGetFreeMemory` answers from.
    peak: u32,
    /// `llScriptProfiler(PROFILE_SCRIPT_MEMORY)` is on.
    profiling: bool,
    /// `llGetSPMaxMemory`: the most used while profiling, [`None`] before
    /// any profiling.
    profiled: Option<u32>,
}

impl Memory {
    /// The accounting of a fresh instance of `program` in `flavour`, its
    /// globals at their types' defaults.
    pub(crate) fn new(program: &Program, flavour: EngineFlavour) -> Self {
        let model = match flavour {
            EngineFlavour::XEngine => EngineFlavour::YEngine,
            other => other,
        };
        let declared = program.globals.iter().fold(0_u32, |sum, global| {
            sum.saturating_add(global_cost(global.ty, model))
        });
        let mut memory = Self {
            flavour,
            model,
            literals: Literals::of(&program.constants),
            code: code_cost(program, model),
            declared,
            globals: vec![0; program.globals.len()],
            held: 0,
            limit: MEMORY_LIMIT,
            peak: 0,
            profiling: false,
            profiled: None,
        };
        memory.peak = memory.heap(Stack::default());
        memory
    }

    /// The accounting of `program` in `flavour` holding `globals`, with the
    /// limit, the low-water mark and the profile `saved` kept: a script
    /// brought back from its saved state. What the globals hold is counted
    /// again rather than saved, so it is the grid's measure of them now.
    pub(crate) fn restored(
        program: &Program,
        flavour: EngineFlavour,
        globals: &[Value],
        saved: SavedMemory,
    ) -> Self {
        let mut memory = Self::new(program, flavour);
        for (slot, value) in memory.globals.iter_mut().zip(globals) {
            *slot = value_cost(value, memory.model, Holder::Global, &memory.literals);
        }
        memory.held = memory
            .globals
            .iter()
            .fold(0_u32, |sum, cost| sum.saturating_add(*cost));
        memory.limit = saved.limit;
        memory.peak = saved.peak;
        memory.profiling = saved.profiling;
        memory.profiled = saved.profiled;
        memory
    }

    /// What of the accounting a saved script keeps: the limit, the low-water
    /// mark and the profile.
    pub(crate) const fn saved(&self) -> SavedMemory {
        SavedMemory {
            limit: self.limit,
            peak: self.peak,
            profiling: self.profiling,
            profiled: self.profiled,
        }
    }

    /// The accounting of a script with no code and no globals, on Second
    /// Life, for calling a library function outside any instance.
    #[cfg(test)]
    pub(crate) fn bare() -> Self {
        Self {
            flavour: EngineFlavour::SecondLife,
            model: EngineFlavour::SecondLife,
            literals: Literals::default(),
            code: 0,
            declared: 0,
            globals: Vec::new(),
            held: 0,
            limit: MEMORY_LIMIT,
            peak: 0,
            profiling: false,
            profiled: None,
        }
    }

    /// The grid's engine the script runs under.
    pub(crate) const fn flavour(&self) -> EngineFlavour {
        self.flavour
    }

    /// The script was reset: its globals are back at their defaults, and
    /// the most it has used starts again from what it uses now. The limit is
    /// kept (a reset does not undo `llSetMemoryLimit` — not measured; the
    /// reference's documentation says so) and so is what profiling found.
    pub(crate) fn reset(&mut self) {
        self.globals.fill(0);
        self.held = 0;
        self.peak = self.heap(Stack::default());
    }

    /// What the code and globals cost, on Mono the frames' call contexts
    /// and on YEngine the locals the stack holds: the heap as the model
    /// counts it.
    const fn heap(&self, stack: Stack) -> u32 {
        let globals = self
            .code
            .saturating_add(self.declared)
            .saturating_add(self.held);
        match self.model {
            EngineFlavour::SecondLife => globals.saturating_add(stack.contexts),
            EngineFlavour::YEngine | EngineFlavour::XEngine => globals.saturating_add(stack.locals),
        }
    }

    /// `llGetUsedMemory`, with `stack` the call stack's holding: Mono's code
    /// and globals, YEngine's heap; `65536` on XEngine.
    pub(crate) const fn used(&self, stack: Stack) -> u32 {
        match self.flavour {
            EngineFlavour::XEngine => MEMORY_LIMIT,
            EngineFlavour::SecondLife | EngineFlavour::YEngine => self.heap(stack),
        }
    }

    /// `llGetFreeMemory`: Mono's limit less the most used since it was set (a
    /// low-water mark, which never rises when a value is cleared); YEngine's
    /// heap less what is used now; `65536` on XEngine.
    pub(crate) const fn free(&self, stack: Stack) -> u32 {
        match self.flavour {
            EngineFlavour::SecondLife => self.limit.saturating_sub(self.peak),
            EngineFlavour::YEngine => YENGINE_HEAP.saturating_sub(self.heap(stack)),
            EngineFlavour::XEngine => MEMORY_LIMIT,
        }
    }

    /// `llGetMemoryLimit`.
    pub(crate) const fn limit(&self) -> u32 {
        self.limit
    }

    /// `llSetMemoryLimit(asked)`: on Mono, a limit from what the script uses
    /// now up to [`MEMORY_LIMIT`] is taken, and where it changes the limit
    /// the low-water mark restarts from the use (asked for the limit it has,
    /// aditi kept the mark, `memory/budget`); anything else is refused and
    /// the limit left as it was. Both OpenSim engines refuse every call.
    pub(crate) fn set_limit(&mut self, asked: i32, stack: Stack) -> bool {
        if self.flavour != EngineFlavour::SecondLife {
            return false;
        }
        let used = self.heap(stack);
        match u32::try_from(asked) {
            Ok(limit) if (used..=MEMORY_LIMIT).contains(&limit) => {
                if limit != self.limit {
                    self.peak = used;
                }
                self.limit = limit;
                true
            }
            Ok(_) | Err(_) => false,
        }
    }

    /// `llScriptProfiler(flags)`: `PROFILE_SCRIPT_MEMORY` starts recording
    /// the most the script uses, from what it uses now; anything else stops.
    /// Nothing on OpenSim.
    pub(crate) fn profile(&mut self, flags: i32, stack: Stack) {
        if self.flavour != EngineFlavour::SecondLife {
            return;
        }
        self.profiling = flags == PROFILE_SCRIPT_MEMORY;
        if self.profiling {
            self.profiled = Some(self.heap(stack));
        }
    }

    /// `llGetSPMaxMemory`: on Mono the most used while profiling, `0` before
    /// any; `65536` on OpenSim.
    pub(crate) fn profiled(&self) -> u32 {
        match self.flavour {
            EngineFlavour::SecondLife => self.profiled.unwrap_or(0),
            EngineFlavour::YEngine | EngineFlavour::XEngine => MEMORY_LIMIT,
        }
    }

    /// Note what the script uses now in its low-water mark and, while
    /// profiling, in the profile.
    fn observe(&mut self, stack: Stack) {
        let used = self.heap(stack);
        self.peak = self.peak.max(used);
        if self.profiling {
            self.profiled = Some(self.profiled.map_or(used, |most| most.max(used)));
        }
    }

    /// What `value` stored in a local costs toward running out: Mono's heap
    /// object (invisible to `llGetUsedMemory`, but there to collide with),
    /// YEngine's local heap use.
    pub(crate) fn local_cost(&self, value: &Value) -> u32 {
        let holder = match self.model {
            EngineFlavour::SecondLife => Holder::Global,
            EngineFlavour::YEngine | EngineFlavour::XEngine => Holder::Local,
        };
        value_cost(value, self.model, holder, &self.literals)
    }

    /// Whether `stack` and the heap as it stands fit, or the script has run
    /// out: Mono's collision is the heap, the locals and either the frames
    /// past the script's 64 KB or their slots past its limit; XEngine's
    /// backstop is the heap past [`HOST_MEMORY_CAP`].
    const fn fits(&self, stack: Stack) -> Result<(), RuntimeError> {
        match self.flavour {
            EngineFlavour::SecondLife => {
                let heap = self.heap(stack).saturating_add(stack.locals);
                if heap.saturating_add(stack.frames) > MEMORY_LIMIT
                    || heap.saturating_add(stack.slots) > self.limit
                {
                    return Err(RuntimeError::StackHeapCollision);
                }
            }
            EngineFlavour::XEngine => {
                if self.heap(stack) > HOST_MEMORY_CAP {
                    return Err(RuntimeError::HostMemory);
                }
            }
            EngineFlavour::YEngine => {}
        }
        Ok(())
    }

    /// Store `value` in the global in `slot`, with `stack` the call stack's
    /// holding, or run out doing it. On YEngine the check is the engine's
    /// own (`XMRInstArrays.PopString` and its kin): the global's growth and
    /// the globals already held past the heap.
    pub(crate) fn store_global(
        &mut self,
        slot: usize,
        value: &Value,
        stack: Stack,
    ) -> Result<(), RuntimeError> {
        let cost = value_cost(value, self.model, Holder::Global, &self.literals);
        let old = self.globals.get(slot).copied().unwrap_or(0);
        if self.flavour == EngineFlavour::YEngine {
            let globals = self.declared.saturating_add(self.held);
            if let Some(growth) = cost.checked_sub(old)
                && growth.saturating_add(globals) > YENGINE_HEAP
            {
                return Err(RuntimeError::OutOfHeap {
                    old_total: 0,
                    new_total: growth,
                    limit: YENGINE_HEAP,
                });
            }
        }
        let held = self.held.saturating_sub(old).saturating_add(cost);
        let before = core::mem::replace(&mut self.held, held);
        if let Err(error) = self.grown(cost > old, stack) {
            self.held = before;
            return Err(error);
        }
        if let Some(entry) = self.globals.get_mut(slot) {
            *entry = cost;
        }
        self.observe(stack);
        Ok(())
    }

    /// A local was stored, and the call stack now holds `stack`, its locals
    /// `before` the store holding [`Stack::locals`] of what they hold now:
    /// whether it fits. On YEngine the check is `UpdateLocalsHeapUse`'s, the
    /// locals and the globals past the heap.
    pub(crate) fn store_local(&mut self, before: u32, stack: Stack) -> Result<(), RuntimeError> {
        if self.flavour == EngineFlavour::YEngine
            && stack
                .locals
                .saturating_add(self.declared)
                .saturating_add(self.held)
                > YENGINE_HEAP
        {
            return Err(RuntimeError::OutOfHeap {
                old_total: before,
                new_total: stack.locals,
                limit: YENGINE_HEAP,
            });
        }
        self.grown(stack.locals > before, stack)?;
        self.observe(stack);
        Ok(())
    }

    /// The call stack is about to hold `stack` with a new frame pushed:
    /// whether it fits.
    pub(crate) const fn call(&self, stack: Stack) -> Result<(), RuntimeError> {
        self.fits(stack)
    }

    /// A frame made its first library call and the call stack now holds
    /// `stack`, its call context included: whether it fits.
    pub(crate) fn library_call(&mut self, stack: Stack) -> Result<(), RuntimeError> {
        self.fits(stack)?;
        self.observe(stack);
        Ok(())
    }

    /// A store left the call stack holding `stack`: whether it fits, where
    /// it `grew` what is held. A store that makes nothing bigger cannot
    /// collide, even where the script already sits past its limit — asked
    /// for exactly what it uses, aditi took the limit and stored integers
    /// under it (`memory/budget`'s `set-limit-now`).
    const fn grown(&self, grew: bool, stack: Stack) -> Result<(), RuntimeError> {
        if !grew && matches!(self.flavour, EngineFlavour::SecondLife) {
            return Ok(());
        }
        self.fits(stack)
    }

    /// A value was just made — a concatenation, a list, a cast, a library
    /// call's answer: whether it could exist at all. On Mono a value that
    /// alone is past the limit collides; anywhere, one past the host's cap
    /// stops the script.
    pub(crate) fn made(&self, value: &Value) -> Result<(), RuntimeError> {
        if !matches!(value, Value::String(_) | Value::Key(_) | Value::List(_)) {
            return Ok(());
        }
        let cost = value_cost(value, self.model, Holder::Global, &self.literals);
        match self.flavour {
            EngineFlavour::SecondLife if cost > self.limit => Err(RuntimeError::StackHeapCollision),
            _ if cost > HOST_MEMORY_CAP => Err(RuntimeError::HostMemory),
            _ => Ok(()),
        }
    }
}
