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
//!   wander on the grid. A string literal costs nothing of its own — the
//!   eight scripts fit as well without it, and the one row that seemed to
//!   say otherwise is a global's value, which is costed as one.
//! - **Mono's stack.** No local, argument or call frame shows in Mono's
//!   `llGetUsedMemory`, yet a recursion collides and a lowered limit cuts
//!   its depth. A frame is costed [`MONO_FRAME`] and the size of each of its
//!   slots ([`slot_size`]) toward the collision only, fitted to the depths
//!   aditi reached at the default limit — 1 039 to 1 116 for one integer
//!   argument (`Narrow`), 413 to 518 for five arguments and two locals
//!   (`Wide`); the probes themselves, run here, reach 1 093 and 424. Under a
//!   limit lowered to 4 096 bytes over the use, aditi went deeper (129 to
//!   253) than the model does (73): the grid's garbage, which a lowered limit
//!   makes it collect, is in the default-limit depths and in no model here.
//!
//! What collides is checked where memory grows: a global or a local stored,
//! a call made, and a value made that alone could never fit. That is enough
//! to match the measured cases — aditi's Full list collides making 3 660
//! elements, as it does here, although appending makes a second copy of the
//! list, so a value on its way to a store is not counted beside the one it
//! replaces; the Limited list collides making 500 here and 540 there, the
//! grid's garbage again.
//!
//! Second Life's LSO, with its 16 KB, is not modelled: the fake grid
//! compiles every script from source, so every script is Mono's.

use sl_lsl::ast::TypeName;

use crate::bytecode::{Body, Program};
use crate::value::Value;
use crate::value::memory::{Holder, Literals, MEMORY_LIMIT, YENGINE_HEAP, global_cost, value_cost};
use crate::vm::fault::RuntimeError;
use crate::vm::flavour::EngineFlavour;

/// Mono: what the code of any script costs, before its bodies and
/// instructions — the fitted constant of the code model (module docs).
pub const MONO_SCRIPT_CODE: u32 = 3_269;

/// Mono: what each function and event handler's code costs over its
/// instructions.
pub const MONO_BODY_CODE: u32 = 342;

/// Mono: what each instruction of this runtime's bytecode stands for in
/// Mono's code.
pub const MONO_INSTRUCTION_CODE: u32 = 18;

/// Mono: what a call frame costs toward a stack-heap collision, over its
/// slots ([`slot_size`]).
pub const MONO_FRAME: u32 = 50;

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
/// model ([`MONO_FRAME`] and its slots); nothing on OpenSim, whose engines
/// keep their call stack apart from the heap they count.
#[must_use]
pub fn frame_cost(body: &Body, flavour: EngineFlavour) -> u32 {
    if flavour != EngineFlavour::SecondLife {
        return 0;
    }
    body.locals.iter().fold(MONO_FRAME, |sum, local| {
        sum.saturating_add(slot_size(local.ty))
    })
}

/// A length as a `u32`, saturating.
fn saturate(length: usize) -> u32 {
    u32::try_from(length).unwrap_or(u32::MAX)
}

/// What a script's call stack holds toward running out: the frames, and what
/// the locals stored in them hold.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Stack {
    /// Every frame's own cost ([`frame_cost`]).
    pub(crate) frames: u32,
    /// What every frame's stored locals hold ([`Memory::local_cost`]).
    pub(crate) locals: u32,
}

impl Stack {
    /// Both together.
    const fn total(self) -> u32 {
        self.frames.saturating_add(self.locals)
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

    /// What the code and globals cost, and on YEngine the locals the stack
    /// holds too: the heap as the model counts it.
    const fn heap(&self, stack: Stack) -> u32 {
        let globals = self
            .code
            .saturating_add(self.declared)
            .saturating_add(self.held);
        match self.model {
            EngineFlavour::SecondLife => globals,
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
    /// now up to [`MEMORY_LIMIT`] is taken, and the low-water mark restarts
    /// from the use; anything else is refused and the limit left as it was.
    /// Both OpenSim engines refuse every call.
    pub(crate) fn set_limit(&mut self, asked: i32, stack: Stack) -> bool {
        if self.flavour != EngineFlavour::SecondLife {
            return false;
        }
        let used = self.heap(stack);
        match u32::try_from(asked) {
            Ok(limit) if (used..=MEMORY_LIMIT).contains(&limit) => {
                self.limit = limit;
                self.peak = used;
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
    /// out: Mono's collision is the heap and the stack past the limit;
    /// XEngine's backstop is the heap past [`HOST_MEMORY_CAP`].
    const fn fits(&self, stack: Stack) -> Result<(), RuntimeError> {
        match self.flavour {
            EngineFlavour::SecondLife => {
                if self.heap(stack).saturating_add(stack.total()) > self.limit {
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
        if let Err(error) = self.fits(stack) {
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
        self.fits(stack)?;
        self.observe(stack);
        Ok(())
    }

    /// The call stack is about to hold `stack` with a new frame pushed:
    /// whether it fits.
    pub(crate) const fn call(&self, stack: Stack) -> Result<(), RuntimeError> {
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
