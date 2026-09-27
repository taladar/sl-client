//! The **compiled form** of a script: a stack bytecode, one [`Body`] per user
//! function, event handler and the global initialisers, with every
//! instruction carrying the source span it came from.
//!
//! The [`compiler`](crate::compiler) produces a [`Program`]; the VM runs it.
//! The book chapter `simulator/lsl-engine.md` records why the form is a stack
//! bytecode. What the lowering guarantees, so that the VM does not have to:
//!
//! - **Every name is resolved.** Locals and parameters are [`LocalSlot`]s,
//!   globals [`GlobalSlot`]s, user functions [`FunctionId`]s, library calls
//!   [`BuiltinId`]s, states [`StateId`]s. The VM never looks up a name.
//! - **Every implicit conversion is an instruction.** An `integer` passed
//!   where a `float` is wanted, a `string` assigned to a `key`, the integer
//!   side of `1 + 2.5`: each is an explicit [`Instr::Cast`]. The VM's only
//!   type rules are the ones [`crate::cast()`] and [`crate::binary()`] apply
//!   to the values they are handed.
//! - **Every program is well typed.** The operand stack holds exactly the
//!   types the instructions expect, and is empty at every statement boundary,
//!   so a jump between any two statements is sound.
//!
//! Operands of a binary operator are evaluated **right to left** — `a - b`
//! pushes `b` and then `a` — as Linden's compilers do, so [`Instr::Binary`]
//! finds its left operand on top of the stack. Call arguments, list elements
//! and vector components are evaluated left to right.
//!
//! [`Program`]'s `Display` is a disassembler, which is what the tests assert
//! on.

use core::fmt;
use core::ops::Range;

use sl_lsl::ast::{BinaryOp, PrefixOp, TypeName};

use crate::format;
use crate::library::{BuiltinId, Event};
use crate::value::Value;

/// A local variable or parameter of one [`Body`]: an index into
/// [`Body::locals`]. Parameters come first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LocalSlot(pub u32);

/// A global variable: an index into [`Program::globals`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GlobalSlot(pub u32);

/// A user-defined function: an index into [`Program::functions`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FunctionId(pub u32);

/// A state: an index into [`Program::states`]. `default` is always
/// [`StateId::DEFAULT`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StateId(pub u32);

impl StateId {
    /// The `default` state, which the grammar puts first.
    pub const DEFAULT: Self = Self(0);
}

/// A literal: an index into [`Program::constants`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ConstId(pub u32);

/// A jump target: an index into the [`Body::code`] of the body the jump is
/// in. Always a valid index — every body ends in an instruction that leaves
/// it, so no jump targets the end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CodeOffset(pub u32);

/// A component of a vector (`x`, `y`, `z`) or rotation (also `s`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Component {
    /// `.x`
    X,
    /// `.y`
    Y,
    /// `.z`
    Z,
    /// `.s` — rotations only.
    S,
}

impl Component {
    /// The component named `name` (`x`, `y`, `z` or `s`), if it is one.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "x" => Some(Self::X),
            "y" => Some(Self::Y),
            "z" => Some(Self::Z),
            "s" => Some(Self::S),
            _ => None,
        }
    }

    /// The component's name as the source spells it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::X => "x",
            Self::Y => "y",
            Self::Z => "z",
            Self::S => "s",
        }
    }
}

/// One instruction, with its effect on the operand stack. "Top" is the most
/// recently pushed value; a stack written `a b` has `b` on top.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Instr {
    /// `→ c`: push a copy of a literal.
    Const(ConstId),
    /// `v →`: discard the top value.
    Pop,
    /// `v → v v`: duplicate the top value.
    Dup,
    /// `→ v`: push a local's value.
    LoadLocal(LocalSlot),
    /// `v →`: store into a local.
    StoreLocal(LocalSlot),
    /// `→ v`: push a global's value.
    LoadGlobal(GlobalSlot),
    /// `v →`: store into a global.
    StoreGlobal(GlobalSlot),
    /// `aggregate → float`: one component of a vector or rotation.
    GetMember(Component),
    /// `float aggregate → aggregate'`: the aggregate on top with one component
    /// replaced by the float beneath it.
    SetMember(Component),
    /// `v → (type)v`, by [`crate::cast()`]. Emitted for every explicit cast and
    /// every implicit conversion.
    Cast(TypeName),
    /// `right left → left op right`, by [`crate::binary()`]. The left operand
    /// is on top: operands are evaluated right to left.
    Binary(BinaryOp),
    /// `v → op v`, by [`crate::prefix()`]. `PreInc` and `PreDec` mean "plus
    /// one" and "minus one"; the lowering does the load and the store.
    Prefix(PrefixOp),
    /// `x y z → vector`; the components are floats.
    BuildVector,
    /// `x y z s → rotation`; the components are floats.
    BuildRotation,
    /// `e1 … en → list`, `e1` deepest; no element is a list.
    BuildList(u32),
    /// `a1 … an → [r]`: call a library function, `a1` deepest, each argument
    /// already of the type the table states. Pushes the return value unless
    /// the function returns nothing.
    CallBuiltin(BuiltinId),
    /// `a1 … an → [r]`: call a user function, as [`Self::CallBuiltin`].
    CallFunction(FunctionId),
    /// Leave a body that yields no value: an event handler, a function
    /// without a return type, or the global initialisers.
    Return,
    /// `v →`: leave a function, yielding `v`.
    ReturnValue,
    /// Continue at the target.
    Jump(CodeOffset),
    /// `v →`: continue at the target if `v` is false in a condition
    /// ([`Value::is_true`]) — any type may stand in a condition.
    JumpIfFalse(CodeOffset),
    /// `v →`: continue at the target if `v` is true in a condition.
    JumpIfTrue(CodeOffset),
    /// `state name;`: request the transition, then leave the body as
    /// [`Self::Return`] does — in a function (legal only inside an `if`, the
    /// reference's "state change hack") yielding the default of its return
    /// type. The transition itself happens when the event handler ends.
    StateChange(StateId),
    /// `string →`: LSL's legacy `print`.
    Print,
    /// Raise the run-time error the reference raises when Mono refuses to
    /// load the body at all (`System.InvalidProgramException`). Only ever
    /// the first instruction of a body, so the body faults before it does
    /// anything; its span is the construct that caused it — today the use of
    /// an `integer *= float`'s value, which Linden's compiler emits as IL
    /// that fails verification.
    InvalidProgram,
}

impl Instr {
    /// Whether control never continues to the next instruction.
    #[must_use]
    pub const fn ends_flow(self) -> bool {
        matches!(
            self,
            Self::Return
                | Self::ReturnValue
                | Self::Jump(_)
                | Self::StateChange(_)
                | Self::InvalidProgram
        )
    }
}

/// One local variable or parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Local {
    /// The name the source declares it under (locals of one body may share a
    /// name when they live in different blocks).
    pub name: String,
    /// Its type.
    pub ty: TypeName,
}

/// The code of one function, event handler or the global initialisers.
///
/// A frame holds one value per entry of [`Self::locals`], each starting at
/// its type's default ([`Value::default_of`]); the first [`Self::params`] are
/// filled from the arguments.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Body {
    /// How many of [`Self::locals`] are parameters.
    pub params: u32,
    /// Every parameter and local of the body, one slot per declaration.
    pub locals: Vec<Local>,
    /// The instructions.
    pub code: Vec<Instr>,
    /// The source map: the byte span of the source each instruction of
    /// [`Self::code`] came from, index for index.
    pub spans: Vec<Range<usize>>,
}

impl Body {
    /// The local in `slot`, if there is one.
    #[must_use]
    pub fn local(&self, slot: LocalSlot) -> Option<&Local> {
        self.locals.get(usize::try_from(slot.0).ok()?)
    }
}

/// One global variable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Global {
    /// Its name.
    pub name: String,
    /// Its type.
    pub ty: TypeName,
}

/// One user-defined function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Function {
    /// Its name.
    pub name: String,
    /// Its return type; [`None`] for a function that returns nothing.
    pub ret: Option<TypeName>,
    /// Its code; the parameters are the first locals.
    pub body: Body,
}

/// One event handler of a state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Handler {
    /// The event it handles; its parameters are the body's first locals.
    pub event: &'static Event,
    /// Its code.
    pub body: Body,
}

/// One state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    /// Its name (`default` for [`StateId::DEFAULT`]).
    pub name: String,
    /// Its event handlers, in source order.
    pub handlers: Vec<Handler>,
}

impl State {
    /// The handler for the event called `event`, if the state has one.
    #[must_use]
    pub fn handler(&self, event: &str) -> Option<&Handler> {
        self.handlers
            .iter()
            .find(|handler| handler.event.name == event)
    }
}

/// A 1-based line and column in the source, the column counted in
/// characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Position {
    /// The line, from 1.
    pub line: u32,
    /// The column, from 1.
    pub column: u32,
}

/// The source a program was compiled from, with the line index that turns a
/// byte span of the source map into a [`Position`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    /// The script's text.
    text: String,
    /// The byte offset each line starts at.
    line_starts: Vec<usize>,
}

impl Source {
    /// Index `text`.
    #[must_use]
    pub fn new(text: &str) -> Self {
        let line_starts = core::iter::once(0)
            .chain(
                text.match_indices('\n')
                    .map(|(offset, _)| offset.saturating_add(1)),
            )
            .collect();
        Self {
            text: text.to_owned(),
            line_starts,
        }
    }

    /// The script's text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The line and column of a byte offset; an offset past the end is the
    /// end.
    #[must_use]
    pub fn position(&self, offset: usize) -> Position {
        let offset = offset.min(self.text.len());
        let line_index = self
            .line_starts
            .partition_point(|&start| start <= offset)
            .saturating_sub(1);
        let line_start = self.line_starts.get(line_index).copied().unwrap_or(0);
        let column = self
            .text
            .get(line_start..offset)
            .map_or(0, |prefix| prefix.chars().count());
        Position {
            line: u32::try_from(line_index.saturating_add(1)).unwrap_or(u32::MAX),
            column: u32::try_from(column.saturating_add(1)).unwrap_or(u32::MAX),
        }
    }
}

/// A compiled script.
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    /// The global variables, in declaration order. Each starts at its type's
    /// default; [`Self::init`] then runs the initialisers.
    pub globals: Vec<Global>,
    /// The literal pool [`Instr::Const`] indexes.
    pub constants: Vec<Value>,
    /// The global initialisers, in source order — run once when the script
    /// starts and again on every reset, before `default`'s `state_entry`.
    pub init: Body,
    /// The user functions, in declaration order.
    pub functions: Vec<Function>,
    /// The states, `default` first.
    pub states: Vec<State>,
    /// The source, for turning the source map into lines.
    pub source: Source,
}

impl Program {
    /// The literal `id` names.
    #[must_use]
    pub fn constant(&self, id: ConstId) -> Option<&Value> {
        self.constants.get(usize::try_from(id.0).ok()?)
    }

    /// The global in `slot`.
    #[must_use]
    pub fn global(&self, slot: GlobalSlot) -> Option<&Global> {
        self.globals.get(usize::try_from(slot.0).ok()?)
    }

    /// The function `id` names.
    #[must_use]
    pub fn function(&self, id: FunctionId) -> Option<&Function> {
        self.functions.get(usize::try_from(id.0).ok()?)
    }

    /// The state `id` names.
    #[must_use]
    pub fn state(&self, id: StateId) -> Option<&State> {
        self.states.get(usize::try_from(id.0).ok()?)
    }

    /// Where the instruction at `pc` of `body` came from.
    #[must_use]
    pub fn position(&self, body: &Body, pc: CodeOffset) -> Option<Position> {
        let span = body.spans.get(usize::try_from(pc.0).ok()?)?;
        Some(self.source.position(span.start))
    }

    /// The disassembly of one instruction, operands resolved to names.
    fn describe(&self, body: &Body, instr: Instr) -> String {
        let local = |slot: LocalSlot| {
            body.local(slot)
                .map_or_else(|| format!("{}?", slot.0), |local| local.name.clone())
        };
        match instr {
            Instr::Const(id) => format!(
                "Const {}",
                self.constant(id)
                    .map_or_else(|| format!("#{}?", id.0), literal)
            ),
            Instr::Pop => "Pop".to_owned(),
            Instr::Dup => "Dup".to_owned(),
            Instr::LoadLocal(slot) => format!("LoadLocal {}", local(slot)),
            Instr::StoreLocal(slot) => format!("StoreLocal {}", local(slot)),
            Instr::LoadGlobal(slot) => format!("LoadGlobal {}", self.global_name(slot)),
            Instr::StoreGlobal(slot) => format!("StoreGlobal {}", self.global_name(slot)),
            Instr::GetMember(component) => format!("GetMember .{}", component.name()),
            Instr::SetMember(component) => format!("SetMember .{}", component.name()),
            Instr::Cast(ty) => format!("Cast {}", ty.keyword()),
            Instr::Binary(op) => format!("Binary {op:?}"),
            Instr::Prefix(op) => format!("Prefix {op:?}"),
            Instr::BuildVector => "BuildVector".to_owned(),
            Instr::BuildRotation => "BuildRotation".to_owned(),
            Instr::BuildList(count) => format!("BuildList {count}"),
            Instr::CallBuiltin(id) => format!("CallBuiltin {}", id.descriptor().name),
            Instr::CallFunction(id) => format!(
                "CallFunction {}",
                self.function(id)
                    .map_or_else(|| format!("{}?", id.0), |function| function.name.clone())
            ),
            Instr::Return => "Return".to_owned(),
            Instr::ReturnValue => "ReturnValue".to_owned(),
            Instr::Jump(target) => format!("Jump {}", target.0),
            Instr::JumpIfFalse(target) => format!("JumpIfFalse {}", target.0),
            Instr::JumpIfTrue(target) => format!("JumpIfTrue {}", target.0),
            Instr::StateChange(id) => format!(
                "StateChange {}",
                self.state(id)
                    .map_or_else(|| format!("{}?", id.0), |state| state.name.clone())
            ),
            Instr::Print => "Print".to_owned(),
            Instr::InvalidProgram => "InvalidProgram".to_owned(),
        }
    }

    /// A global's name for the disassembly.
    fn global_name(&self, slot: GlobalSlot) -> String {
        self.global(slot)
            .map_or_else(|| format!("{}?", slot.0), |global| global.name.clone())
    }

    /// Write one body's locals and code.
    fn write_body(&self, out: &mut fmt::Formatter<'_>, body: &Body) -> fmt::Result {
        for (index, local) in body.locals.iter().enumerate() {
            let kind = if u32::try_from(index).is_ok_and(|index| index < body.params) {
                "param"
            } else {
                "local"
            };
            writeln!(out, "  {kind} {} {}", local.ty.keyword(), local.name)?;
        }
        for (pc, instr) in body.code.iter().enumerate() {
            let at = body.spans.get(pc).map_or_else(
                || "?".to_owned(),
                |span| {
                    let position = self.source.position(span.start);
                    format!("{}:{}", position.line, position.column)
                },
            );
            writeln!(out, "  {pc:>4} {at:<7} {}", self.describe(body, *instr))?;
        }
        Ok(())
    }
}

/// A parameter list for the disassembly: `type name, …`.
fn params(body: &Body) -> String {
    body.locals
        .iter()
        .take(usize::try_from(body.params).unwrap_or(usize::MAX))
        .map(|local| format!("{} {}", local.ty.keyword(), local.name))
        .collect::<Vec<_>>()
        .join(", ")
}

/// A value written the way a script would write it as a literal.
fn literal(value: &Value) -> String {
    match value {
        Value::Integer(integer) => integer.to_string(),
        Value::Float(float) => format::float(*float, format::FLOAT_DECIMALS),
        Value::String(text) => format!("{text:?}"),
        Value::Key(text) => format!("(key){text:?}"),
        Value::Vector(vector) => format::vector(vector, format::FLOAT_DECIMALS),
        Value::Rotation(rotation) => format::rotation(rotation, format::FLOAT_DECIMALS),
        Value::List(elements) => format!(
            "[{}]",
            elements
                .iter()
                .map(|element| literal(&Value::from(element.clone())))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

impl fmt::Display for Program {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let out = f;
        for global in &self.globals {
            writeln!(out, "global {} {}", global.ty.keyword(), global.name)?;
        }
        if self.init.code != [Instr::Return] {
            writeln!(out, "init")?;
            self.write_body(out, &self.init)?;
        }
        for function in &self.functions {
            let ret = function
                .ret
                .map_or_else(String::new, |ty| format!(" -> {}", ty.keyword()));
            writeln!(
                out,
                "function {}({}){ret}",
                function.name,
                params(&function.body)
            )?;
            self.write_body(out, &function.body)?;
        }
        for state in &self.states {
            writeln!(out, "state {}", state.name)?;
            for handler in &state.handlers {
                writeln!(
                    out,
                    " event {}({})",
                    handler.event.name,
                    params(&handler.body)
                )?;
                self.write_body(out, &handler.body)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::value::Element;

    #[test]
    fn positions_count_lines_and_characters_from_one() {
        let source = Source::new("ab\n\u{e9}xy\n\nz");
        let at = |offset| {
            let position = source.position(offset);
            (position.line, position.column)
        };
        assert_eq!(at(0), (1, 1));
        assert_eq!(at(2), (1, 3));
        assert_eq!(at(3), (2, 1));
        // `é` is two bytes and one column.
        assert_eq!(at(5), (2, 2));
        assert_eq!(at(9), (4, 1));
        assert_eq!(at(99), (4, 2));
    }

    #[test]
    fn literals_print_as_a_script_would_write_them() {
        assert_eq!(literal(&Value::Float(1.5)), "1.500000");
        assert_eq!(literal(&Value::String("a\"b".to_owned())), "\"a\\\"b\"");
        assert_eq!(
            literal(&Value::List(vec![
                Element::Integer(1),
                Element::Key("k".to_owned())
            ])),
            "[1, (key)\"k\"]"
        );
    }
}
