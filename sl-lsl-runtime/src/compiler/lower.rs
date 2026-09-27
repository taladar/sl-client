//! The **lowering**: a checked syntax tree to a [`Program`].
//!
//! One walk does three jobs, because each needs the others' answers: it
//! **types** every expression completely (with `sl_lsl::types`, the
//! compile-time half of the type rules), **resolves** every name to a slot,
//! a function, a library entry or a state, and **emits** the bytecode with
//! the implicit conversions spelled out. Whatever it cannot type or resolve
//! is a compile error.
//!
//! The scoping and checking rules are tailslide's (`symbol_resolution.cc`,
//! `type_checking.cc`, `globalexpr_validator.cc`, `final_pass.cc`), which
//! reproduce Linden's compiler:
//!
//! - **Global variables are sequential**: an initialiser sees only the
//!   globals declared before it — itself excluded, so `integer x = x;` is an
//!   error. Every global, function and state is visible from every body.
//! - **Locals are sequential too**, and one block may not declare a name
//!   twice (labels included); an inner block may shadow an outer name.
//! - **One namespace per scope, looked up by kind.** A local named like a
//!   function does not hide the function from a call, but a global may not
//!   share a name with a function or a state, and nothing may be named like
//!   a library constant or event. Only a local may take a library function's
//!   name.
//! - **A label is visible from its whole block**, but a `jump` lands on the
//!   *last* label of that name in the function — the reference's behaviour
//!   when a name is repeated in nested blocks.
//! - **A value function's last statement must return**: a `return`, or an
//!   `if`/`else` whose branches both end in one. Nothing else counts — not a
//!   loop, not an unconditional jump.

use core::ops::Range;
use std::collections::{HashMap, HashSet};

use sl_lsl::ast::{
    AssignOp, BinaryOp, Block, EventHandler, Expr, FunctionDef, GlobalItem, GlobalVar, Ident,
    Param, PostfixOp, PrefixOp, Script, StateDef, StateName, Stmt, TypeName,
};
use sl_lsl::types;

use super::{CompileError, CompileErrorKind as Kind, literal};
use crate::bytecode::{
    Body, CodeOffset, Component, ConstId, Function, FunctionId, Global, GlobalSlot, Handler, Instr,
    Local, LocalSlot, Program, Source, State, StateId,
};
use crate::library::{self, BuiltinId, ConstantValue};
use crate::value::Value;

/// Lower a parsed script that the semantic pass accepted.
pub(super) fn lower(script: &Script, source: Source) -> Result<Program, Vec<CompileError>> {
    let mut lowerer = Lowerer::new(&source);
    lowerer.script(script);
    let Lowerer {
        mut errors,
        globals,
        constants,
        init,
        functions,
        states,
        ..
    } = lowerer;
    if errors.is_empty() {
        Ok(Program {
            globals,
            constants,
            init,
            functions,
            states,
            source,
        })
    } else {
        errors.sort_by_key(|error| (error.span.start, error.span.end));
        Err(errors)
    }
}

/// An expression's type as the lowering tracks it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ty {
    /// A call to a function that returns nothing, or `print`.
    Void,
    /// A value of one of the seven types.
    Value(TypeName),
    /// Already reported: whatever uses it stays quiet rather than report the
    /// same mistake again.
    Error,
}

/// What a name in the script's global scope is.
#[derive(Debug, Clone, Copy)]
enum GlobalSymbol {
    /// A global variable.
    Variable(GlobalSlot),
    /// A user function.
    Function(FunctionId),
    /// A state.
    State(StateId),
}

/// A user function's signature, known before any body is lowered.
#[derive(Debug, Clone)]
struct Signature {
    /// The parameter types, in order.
    params: Vec<TypeName>,
    /// The return type.
    ret: Option<TypeName>,
}

/// Where a definition lives, for the rules about which names it may take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DefinedIn {
    /// A global variable or a user function.
    Global,
    /// A parameter, a local or a label.
    Local,
}

/// A name a variable reference resolves to.
#[derive(Debug, Clone, Copy)]
enum Resolved {
    /// A parameter or local.
    Local(LocalSlot, TypeName),
    /// A global variable.
    Global(GlobalSlot, TypeName),
    /// A library constant.
    Constant(ConstantValue),
}

/// A variable an assignment, `++` or `--` can store to.
#[derive(Debug, Clone, Copy)]
enum Place {
    /// A parameter or local.
    Local(LocalSlot, TypeName),
    /// A global variable.
    Global(GlobalSlot, TypeName),
}

/// A variable, or one component of a vector or rotation variable.
#[derive(Debug, Clone, Copy)]
struct LValue {
    /// The variable.
    place: Place,
    /// The component, for `v.x`.
    member: Option<Component>,
}

impl LValue {
    /// The type a load yields and a store takes.
    const fn ty(self) -> TypeName {
        match (self.member, self.place) {
            (Some(_), _) => TypeName::Float,
            (None, Place::Local(_, ty) | Place::Global(_, ty)) => ty,
        }
    }
}

/// One lexical scope: a block, or a body's parameters.
#[derive(Debug, Default)]
struct Scope {
    /// The variables declared so far.
    variables: HashMap<String, (LocalSlot, TypeName)>,
    /// The labels of the block — all of them, wherever in the block they
    /// stand, since a `jump` may go forwards.
    labels: HashSet<String>,
}

/// What kind of body is being lowered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BodyKind {
    /// The global initialisers.
    Init,
    /// A user function.
    Function,
    /// An event handler.
    Event,
}

/// The state of the body being lowered.
#[derive(Debug)]
struct BodyBuilder {
    /// What it is.
    kind: BodyKind,
    /// The function's or event's name, for messages.
    name: String,
    /// Its return type.
    ret: Option<TypeName>,
    /// The code so far.
    body: Body,
    /// The scopes, innermost last.
    scopes: Vec<Scope>,
    /// Each label name's position: the last label of that name wins.
    labels: HashMap<String, usize>,
    /// The `jump`s to patch once every label is placed: the instruction and
    /// the label it names.
    jumps: Vec<(usize, String)>,
    /// How many `if` statements enclose the current statement.
    if_depth: u32,
    /// How many `if`s and loops enclose the current statement.
    control_depth: u32,
    /// The first construct that makes the reference's runtime refuse the
    /// whole body, if any (see [`Instr::InvalidProgram`]).
    invalid_program: Option<Range<usize>>,
}

impl BodyBuilder {
    /// An empty body of a kind.
    fn new(kind: BodyKind, ret: Option<TypeName>) -> Self {
        Self {
            kind,
            name: String::new(),
            ret,
            body: Body::default(),
            scopes: Vec::new(),
            labels: HashMap::new(),
            jumps: Vec::new(),
            if_depth: 0,
            control_depth: 0,
            invalid_program: None,
        }
    }
}

/// The walk's state.
struct Lowerer<'source> {
    /// The source, for error positions.
    source: &'source Source,
    /// The errors found so far.
    errors: Vec<CompileError>,
    /// The global variables.
    globals: Vec<Global>,
    /// The literal pool.
    constants: Vec<Value>,
    /// The global scope: variables, functions and states by name.
    symbols: HashMap<String, GlobalSymbol>,
    /// Every function's signature, by [`FunctionId`].
    signatures: Vec<Signature>,
    /// The global initialisers, once lowered.
    init: Body,
    /// The lowered functions.
    functions: Vec<Function>,
    /// The lowered states.
    states: Vec<State>,
    /// The body being lowered.
    current: BodyBuilder,
}

impl<'source> Lowerer<'source> {
    /// A lowerer with nothing lowered yet.
    fn new(source: &'source Source) -> Self {
        Self {
            source,
            errors: Vec::new(),
            globals: Vec::new(),
            constants: Vec::new(),
            symbols: HashMap::new(),
            signatures: Vec::new(),
            init: Body::default(),
            functions: Vec::new(),
            states: Vec::new(),
            current: BodyBuilder::new(BodyKind::Init, None),
        }
    }

    /// Record an error: the grid's kind, and a message saying what exactly
    /// is wrong.
    fn error(&mut self, kind: Kind, span: &Range<usize>, message: String) {
        self.errors.push(CompileError {
            kind,
            message,
            span: span.clone(),
            position: self.source.position(span.start),
        });
    }

    /// A length as a bytecode index, or an error when it does not fit.
    fn index(&mut self, len: usize, span: &Range<usize>) -> u32 {
        u32::try_from(len).unwrap_or_else(|_| {
            self.error(
                Kind::TooLarge,
                span,
                "the script is too large to assemble".to_owned(),
            );
            0
        })
    }

    // -- the script -------------------------------------------------------

    /// Lower the whole script: the globals and function signatures in
    /// source order (so each initialiser sees the globals before it), then
    /// the states' names, then every function body and every handler.
    fn script(&mut self, script: &Script) {
        self.check_layout(script);

        for item in &script.globals {
            match item {
                GlobalItem::Variable(var) => self.global_variable(var),
                GlobalItem::Function(func) => self.function_signature(func),
            }
        }
        self.init = self.finish_body(&(script.span.start..script.span.start));

        for state in &script.states {
            let (name, span) = state_name(&state.name);
            let id = StateId(self.index(self.states.len(), &span));
            self.states.push(State {
                name: name.to_owned(),
                handlers: Vec::new(),
            });
            self.define_global(name, &span, GlobalSymbol::State(id));
        }

        let mut next_function = 0_u32;
        for item in &script.globals {
            if let GlobalItem::Function(func) = item {
                self.function_body(func, FunctionId(next_function));
                next_function = next_function.saturating_add(1);
            }
        }
        for (index, state) in script.states.iter().enumerate() {
            self.state_handlers(state, index);
        }
    }

    /// The structural rules Linden's grammar enforces: globals before
    /// states, `default` first, and at least one handler per state.
    fn check_layout(&mut self, script: &Script) {
        let Some(first) = script.states.first() else {
            self.error(
                Kind::Syntax,
                &(script.span.end..script.span.end),
                "a script needs a `default` state".to_owned(),
            );
            return;
        };
        if let StateName::Named(name) = &first.name {
            let has_default = script
                .states
                .iter()
                .any(|state| matches!(state.name, StateName::Default(_)));
            let message = if has_default {
                format!("`default` must be the first state, before `{}`", name.name)
            } else {
                "a script needs a `default` state, and it must come first".to_owned()
            };
            self.error(Kind::Syntax, &(first.span.start..name.span.end), message);
        }
        for item in &script.globals {
            let span = match item {
                GlobalItem::Variable(var) => &var.span,
                GlobalItem::Function(func) => &func.span,
            };
            if span.start > first.span.start {
                self.error(
                    Kind::Syntax,
                    span,
                    "global variables and functions must come before the states".to_owned(),
                );
            }
        }
        for state in &script.states {
            if state.events.is_empty() {
                let (name, span) = state_name(&state.name);
                self.error(
                    Kind::Syntax,
                    &span,
                    format!("state `{name}` has no event handlers; a state needs at least one"),
                );
            }
        }
    }

    /// Whether a name may be defined where it is: never a library constant
    /// or event (both are keywords of Linden's grammar), and a library
    /// function's name only for a local.
    fn check_definable(&mut self, name: &Ident, place: DefinedIn) {
        let what = if library::constant(&name.name).is_some() {
            Some("constant")
        } else if library::event(&name.name).is_some() {
            Some("event")
        } else {
            None
        };
        if let Some(what) = what {
            self.error(
                Kind::Syntax,
                &name.span,
                format!(
                    "`{}` is a library {what}, so nothing can be named that",
                    name.name
                ),
            );
        } else if place == DefinedIn::Global && library::builtin(&name.name).is_some() {
            self.error(
                Kind::AlreadyDefined,
                &name.span,
                format!(
                    "`{}` is a library function; only a local variable may reuse its name",
                    name.name
                ),
            );
        }
    }

    /// Bind a name in the global scope, or report it taken.
    fn define_global(&mut self, name: &str, span: &Range<usize>, symbol: GlobalSymbol) {
        if let Some(existing) = self.symbols.get(name) {
            let what = match existing {
                GlobalSymbol::Variable(_) => "a global variable",
                GlobalSymbol::Function(_) => "a function",
                GlobalSymbol::State(_) => "a state",
            };
            self.error(
                Kind::AlreadyDefined,
                span,
                format!("`{name}` is already declared as {what}"),
            );
        } else {
            let _previous = self.symbols.insert(name.to_owned(), symbol);
        }
    }

    /// A global variable: its initialiser into the init body, then the
    /// name, which the initialiser itself cannot see.
    fn global_variable(&mut self, var: &GlobalVar) {
        let ty = var.ty.kind;
        let slot = GlobalSlot(self.index(self.globals.len(), &var.span));
        if let Some(init) = &var.init {
            if simple_assignable(init, false) {
                let found = self.expr(init);
                let what = format!("the initial value of `{}`", var.name.name);
                if self.coerce(found, ty, &init.span(), Kind::TypeMismatch, &what) {
                    self.emit(Instr::StoreGlobal(slot), &var.span);
                }
            } else {
                self.error(
                    Kind::Syntax,
                    &init.span(),
                    "a global's initial value must be a constant: a literal, a library \
                     constant or an earlier global, or a list, vector or rotation of those"
                        .to_owned(),
                );
            }
        }
        self.globals.push(Global {
            name: var.name.name.clone(),
            ty,
        });
        self.check_definable(&var.name, DefinedIn::Global);
        self.define_global(&var.name.name, &var.name.span, GlobalSymbol::Variable(slot));
    }

    /// A user function's signature. Every function gets an id, a duplicate
    /// too, so every body is still checked.
    fn function_signature(&mut self, func: &FunctionDef) {
        let id = FunctionId(self.index(self.signatures.len(), &func.span));
        self.signatures.push(Signature {
            params: func.params.iter().map(|param| param.ty.kind).collect(),
            ret: func.ret.as_ref().map(|ty| ty.kind),
        });
        self.check_definable(&func.name, DefinedIn::Global);
        self.define_global(&func.name.name, &func.name.span, GlobalSymbol::Function(id));
    }

    /// A user function's body.
    fn function_body(&mut self, func: &FunctionDef, id: FunctionId) {
        let ret = self
            .signatures
            .get(usize::try_from(id.0).unwrap_or(usize::MAX))
            .and_then(|signature| signature.ret);
        self.current = BodyBuilder::new(BodyKind::Function, ret);
        self.current.name.clone_from(&func.name.name);
        self.params(&func.params);
        self.block(&func.body);
        if ret.is_some() && !func.body.statements.last().is_some_and(always_returns) {
            // Second Life points at the body's closing brace (measured on
            // aditi, 2026-09-27; tailslide points at the name instead).
            self.error(
                Kind::MissingReturn,
                &closing(&func.body.span),
                format!(
                    "`{}` must return {} on every path: its last statement has to be a \
                     `return`, or an `if`/`else` whose branches both end in one",
                    func.name.name,
                    ret.map_or_else(String::new, a)
                ),
            );
        }
        let body = self.finish_body(&closing(&func.body.span));
        self.functions.push(Function {
            name: func.name.name.clone(),
            ret,
            body,
        });
    }

    /// One state's handlers.
    fn state_handlers(&mut self, state: &StateDef, index: usize) {
        let mut handled = HashSet::new();
        let mut handlers = Vec::new();
        for handler in &state.events {
            let event = self.check_event(handler);
            if !handled.insert(handler.name.name.as_str()) {
                let (state_name, _) = state_name(&state.name);
                self.error(
                    Kind::AlreadyDefined,
                    &handler.name.span,
                    format!(
                        "state `{state_name}` already has a `{}` handler",
                        handler.name.name
                    ),
                );
            }
            self.current = BodyBuilder::new(BodyKind::Event, None);
            self.current.name.clone_from(&handler.name.name);
            self.params(&handler.params);
            self.block(&handler.body);
            let body = self.finish_body(&closing(&handler.body.span));
            if let Some(event) = event {
                handlers.push(Handler { event, body });
            }
        }
        if let Some(lowered) = self.states.get_mut(index) {
            lowered.handlers = handlers;
        }
    }

    /// The event a handler handles, if the name is an event and the
    /// parameters are exactly its signature — a grammar rule in Linden's
    /// compiler, so a mismatch is a syntax error.
    fn check_event(&mut self, handler: &EventHandler) -> Option<&'static library::Event> {
        let Some(event) = library::event(&handler.name.name) else {
            let hint = did_you_mean(
                &handler.name.name,
                library::EVENTS.iter().map(|event| event.name),
            );
            self.error(
                Kind::Syntax,
                &handler.name.span,
                format!("`{}` is not an event{hint}", handler.name.name),
            );
            return None;
        };
        let matches = handler.params.len() == event.args.len()
            && handler
                .params
                .iter()
                .zip(event.args)
                .all(|(param, arg)| param.ty.kind == arg.ty);
        if matches {
            Some(event)
        } else {
            let expected = event
                .args
                .iter()
                .map(|arg| format!("{} {}", arg.ty.keyword(), arg.name))
                .collect::<Vec<_>>()
                .join(", ");
            let given = handler
                .params
                .iter()
                .map(|param| param.ty.kind.keyword())
                .collect::<Vec<_>>()
                .join(", ");
            self.error(
                Kind::Syntax,
                &handler.name.span,
                format!(
                    "the `{}` event takes ({expected}), not ({given})",
                    event.name
                ),
            );
            None
        }
    }

    /// Open a body's parameter scope and declare the parameters.
    fn params(&mut self, params: &[Param]) {
        self.current.scopes.push(Scope::default());
        for param in params {
            self.check_definable(&param.name, DefinedIn::Local);
            if self
                .current
                .scopes
                .last()
                .is_some_and(|scope| scope.variables.contains_key(&param.name.name))
            {
                self.error(
                    Kind::AlreadyDefined,
                    &param.name.span,
                    format!("parameter `{}` is declared twice", param.name.name),
                );
            }
            let _slot = self.declare_local(&param.name, param.ty.kind);
            self.current.body.params = self.current.body.params.saturating_add(1);
        }
    }

    /// Finish the body being lowered: patch the jumps, and end it with a
    /// return unless every path already leaves it.
    fn finish_body(&mut self, end: &Range<usize>) -> Body {
        let mut builder =
            core::mem::replace(&mut self.current, BodyBuilder::new(BodyKind::Init, None));
        for (at, label) in core::mem::take(&mut builder.jumps) {
            // An unknown label was reported when the jump was lowered.
            let target = builder.labels.get(&label).copied().unwrap_or(0);
            let target = CodeOffset(self.index(target, end));
            if let Some(instr) = builder.body.code.get_mut(at) {
                *instr = retarget(*instr, target);
            }
        }
        let len = builder.body.code.len();
        let targets_end =
            builder.body.code.iter().any(|instr| {
                target(*instr).is_some_and(|target| usize::try_from(target.0) == Ok(len))
            });
        let falls_off = builder
            .body
            .code
            .last()
            .is_none_or(|instr| !instr.ends_flow());
        if targets_end || falls_off {
            self.current = builder;
            match self.current.ret {
                Some(ty) => {
                    self.push_const(Value::default_of(ty), end);
                    self.emit(Instr::ReturnValue, end);
                }
                None => self.emit(Instr::Return, end),
            }
            builder = core::mem::replace(&mut self.current, BodyBuilder::new(BodyKind::Init, None));
        }
        if let Some(span) = builder.invalid_program.take() {
            let code = &mut builder.body.code;
            for instr in code.iter_mut() {
                if let Some(to) = target(*instr) {
                    *instr = retarget(*instr, CodeOffset(to.0.saturating_add(1)));
                }
            }
            code.insert(0, Instr::InvalidProgram);
            builder.body.spans.insert(0, span);
        }
        builder.body
    }

    // -- emission ---------------------------------------------------------

    /// Append an instruction.
    fn emit(&mut self, instr: Instr, span: &Range<usize>) {
        self.current.body.code.push(instr);
        self.current.body.spans.push(span.clone());
    }

    /// The position the next instruction will have.
    const fn here(&self) -> usize {
        self.current.body.code.len()
    }

    /// Insert a conversion at an earlier position — the right operand's, which
    /// the right-to-left evaluation order has already buried under the left
    /// one by the time the left one's type is known. No jump target lies
    /// inside an expression, so nothing needs patching.
    fn insert_cast(&mut self, at: usize, ty: TypeName, span: &Range<usize>) {
        let at = at.min(self.here());
        self.current.body.code.insert(at, Instr::Cast(ty));
        self.current.body.spans.insert(at, span.clone());
    }

    /// Emit a jump whose target is patched later; its position.
    fn emit_jump(&mut self, make: fn(CodeOffset) -> Instr, span: &Range<usize>) -> usize {
        let at = self.here();
        self.emit(make(CodeOffset(0)), span);
        at
    }

    /// Point the jump at `at` to the next instruction.
    fn patch_here(&mut self, at: usize, span: &Range<usize>) {
        let target = CodeOffset(self.index(self.here(), span));
        if let Some(instr) = self.current.body.code.get_mut(at) {
            *instr = retarget(*instr, target);
        }
    }

    /// A jump back to an earlier position.
    fn jump_to(&mut self, make: fn(CodeOffset) -> Instr, to: usize, span: &Range<usize>) {
        let target = CodeOffset(self.index(to, span));
        self.emit(make(target), span);
    }

    /// Push a literal, pooled.
    fn push_const(&mut self, value: Value, span: &Range<usize>) {
        let existing = self
            .constants
            .iter()
            .position(|pooled| same_literal(pooled, &value));
        let index = existing.unwrap_or_else(|| {
            self.constants.push(value);
            self.constants.len().saturating_sub(1)
        });
        let id = ConstId(self.index(index, span));
        self.emit(Instr::Const(id), span);
    }

    /// Declare a local in the innermost scope; its slot.
    fn declare_local(&mut self, name: &Ident, ty: TypeName) -> LocalSlot {
        let slot = LocalSlot(self.index(self.current.body.locals.len(), &name.span));
        self.current.body.locals.push(Local {
            name: name.name.clone(),
            ty,
        });
        if let Some(scope) = self.current.scopes.last_mut() {
            let _shadowed = scope.variables.insert(name.name.clone(), (slot, ty));
        }
        slot
    }

    /// Convert the value just pushed to `to`, as an assignment, argument or
    /// return does: nothing, a cast for one of LSL's two implicit
    /// conversions, or `kind` reported, the message naming the value as
    /// `what`. Whether it succeeded.
    fn coerce(
        &mut self,
        found: Ty,
        to: TypeName,
        span: &Range<usize>,
        kind: Kind,
        what: &str,
    ) -> bool {
        match found {
            Ty::Error => false,
            Ty::Value(from) if from == to => true,
            Ty::Value(from) if types::implicitly_converts(from, to) => {
                self.emit(Instr::Cast(to), span);
                true
            }
            Ty::Value(from) => {
                let hint = if types::cast_legal(from, to) {
                    format!("; an explicit `({})` cast converts it", to.keyword())
                } else {
                    String::new()
                };
                self.error(
                    kind,
                    span,
                    format!(
                        "{what} must be `{}`, but this is `{}`{hint}",
                        to.keyword(),
                        from.keyword()
                    ),
                );
                false
            }
            Ty::Void => {
                self.error(
                    kind,
                    span,
                    format!(
                        "{what} must be `{}`, but this returns nothing",
                        to.keyword()
                    ),
                );
                false
            }
        }
    }

    // -- statements -------------------------------------------------------

    /// A block, in a scope of its own.
    fn block(&mut self, block: &Block) {
        let mut scope = Scope::default();
        for stmt in &block.statements {
            self.collect_labels(stmt, &mut scope.labels);
        }
        self.current.scopes.push(scope);
        for stmt in &block.statements {
            self.stmt(stmt);
        }
        let _closed = self.current.scopes.pop();
    }

    /// Add the labels a statement defines in the enclosing block — itself,
    /// or inside an unbraced `if` or loop body, which opens no scope.
    fn collect_labels(&mut self, stmt: &Stmt, labels: &mut HashSet<String>) {
        match stmt {
            Stmt::Label { name, .. } => {
                self.check_definable(name, DefinedIn::Local);
                if !labels.insert(name.name.clone()) {
                    self.error(
                        Kind::AlreadyDefined,
                        &name.span,
                        format!("label `@{}` is already defined in this block", name.name),
                    );
                }
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                self.collect_labels(then_branch, labels);
                if let Some(else_branch) = else_branch {
                    self.collect_labels(else_branch, labels);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::For { body, .. } => {
                self.collect_labels(body, labels);
            }
            Stmt::Empty(_)
            | Stmt::Block(_)
            | Stmt::Local { .. }
            | Stmt::Expr { .. }
            | Stmt::Return { .. }
            | Stmt::Jump { .. }
            | Stmt::StateChange { .. }
            | Stmt::Error(_) => {}
        }
    }

    /// The body of an `if`, `else` or loop, which may not be a bare
    /// declaration.
    fn substatement(&mut self, stmt: &Stmt) {
        if let Stmt::Local { span, .. } = stmt {
            self.error(
                Kind::DeclarationNeedsScope,
                span,
                "a declaration cannot be the whole body of an `if`, `else` or loop; \
                 wrap it in `{ }`"
                    .to_owned(),
            );
        }
        self.stmt(stmt);
    }

    /// One statement.
    fn stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Empty(_) => {}
            Stmt::Block(block) => self.block(block),
            Stmt::Local { ty, name, init, .. } => self.local(ty.kind, name, init.as_ref()),
            Stmt::Expr { expr, .. } => self.effect(expr),
            Stmt::If {
                cond,
                then_branch,
                else_branch,
                span,
            } => {
                self.condition(cond);
                let skip_then = self.emit_jump(Instr::JumpIfFalse, span);
                self.enter_control(true);
                self.substatement(then_branch);
                if let Some(else_branch) = else_branch {
                    let skip_else = self.emit_jump(Instr::Jump, span);
                    self.patch_here(skip_then, span);
                    self.substatement(else_branch);
                    self.patch_here(skip_else, span);
                } else {
                    self.patch_here(skip_then, span);
                }
                self.leave_control(true);
            }
            Stmt::While { cond, body, span } => {
                let start = self.here();
                self.condition(cond);
                let exit = self.emit_jump(Instr::JumpIfFalse, span);
                self.enter_control(false);
                self.substatement(body);
                self.leave_control(false);
                self.jump_to(Instr::Jump, start, span);
                self.patch_here(exit, span);
            }
            Stmt::DoWhile { body, cond, span } => {
                let start = self.here();
                self.enter_control(false);
                self.substatement(body);
                self.leave_control(false);
                self.condition(cond);
                self.jump_to(Instr::JumpIfTrue, start, span);
            }
            Stmt::For {
                init,
                cond,
                incr,
                body,
                span,
            } => {
                for expr in init {
                    self.effect(expr);
                }
                let start = self.here();
                let exit = cond.as_ref().map(|cond| {
                    self.condition(cond);
                    self.emit_jump(Instr::JumpIfFalse, span)
                });
                self.enter_control(false);
                self.substatement(body);
                self.leave_control(false);
                for expr in incr {
                    self.effect(expr);
                }
                self.jump_to(Instr::Jump, start, span);
                if let Some(exit) = exit {
                    self.patch_here(exit, span);
                }
            }
            Stmt::Return { value, span } => self.return_stmt(value.as_ref(), span),
            Stmt::Jump { label, span } => {
                let visible = self
                    .current
                    .scopes
                    .iter()
                    .any(|scope| scope.labels.contains(&label.name));
                if !visible {
                    let hint = did_you_mean(
                        &label.name,
                        self.current
                            .scopes
                            .iter()
                            .flat_map(|scope| scope.labels.iter().map(String::as_str)),
                    );
                    self.error(
                        Kind::Undefined,
                        &label.span,
                        format!(
                            "no label `@{}` in scope here — a label is visible only in its own \
                             block and the blocks inside it{hint}",
                            label.name
                        ),
                    );
                }
                let at = self.emit_jump(Instr::Jump, span);
                self.current.jumps.push((at, label.name.clone()));
            }
            Stmt::Label { name, .. } => {
                let here = self.here();
                let _previous = self.current.labels.insert(name.name.clone(), here);
            }
            Stmt::StateChange { target, span } => self.state_change(target, span),
            Stmt::Error(span) => {
                self.error(
                    Kind::Syntax,
                    span,
                    "this statement does not parse".to_owned(),
                );
            }
        }
    }

    /// Step into an `if` (`is_if`) or a loop.
    const fn enter_control(&mut self, is_if: bool) {
        self.current.control_depth = self.current.control_depth.saturating_add(1);
        if is_if {
            self.current.if_depth = self.current.if_depth.saturating_add(1);
        }
    }

    /// Step out of an `if` (`is_if`) or a loop.
    const fn leave_control(&mut self, is_if: bool) {
        self.current.control_depth = self.current.control_depth.saturating_sub(1);
        if is_if {
            self.current.if_depth = self.current.if_depth.saturating_sub(1);
        }
    }

    /// A local declaration: the initialiser first, which cannot see the new
    /// name, then the name.
    fn local(&mut self, ty: TypeName, name: &Ident, init: Option<&Expr>) {
        match init {
            Some(init) => {
                let found = self.expr(init);
                let what = format!("the initial value of `{}`", name.name);
                let _converted = self.coerce(found, ty, &init.span(), Kind::TypeMismatch, &what);
            }
            None => self.push_const(Value::default_of(ty), &name.span),
        }
        self.check_definable(name, DefinedIn::Local);
        let taken = self.current.scopes.last().is_some_and(|scope| {
            scope.variables.contains_key(&name.name) || scope.labels.contains(&name.name)
        });
        if taken {
            self.error(
                Kind::AlreadyDefined,
                &name.span,
                format!("`{}` is already declared in this block", name.name),
            );
        }
        let slot = self.declare_local(name, ty);
        self.emit(Instr::StoreLocal(slot), &name.span);
    }

    /// A condition: any type but no void.
    fn condition(&mut self, cond: &Expr) {
        if self.expr(cond) == Ty::Void {
            self.error(
                Kind::TypeMismatch,
                &cond.span(),
                "a condition needs a value, but this returns nothing".to_owned(),
            );
        }
    }

    /// `return;` or `return value;`.
    fn return_stmt(&mut self, value: Option<&Expr>, span: &Range<usize>) {
        match (self.current.ret, value) {
            (None, None) => self.emit(Instr::Return, span),
            (None, Some(expr)) => match self.expr(expr) {
                // tailslide: returning a void call is tolerated when the
                // `return` is nested in a control statement.
                Ty::Void if self.current.control_depth > 0 => self.emit(Instr::Return, span),
                Ty::Error => {}
                found @ (Ty::Void | Ty::Value(_)) => {
                    let whose = match self.current.kind {
                        BodyKind::Event => format!("the `{}` event", self.current.name),
                        BodyKind::Function | BodyKind::Init => {
                            format!("`{}`", self.current.name)
                        }
                    };
                    let note = if found == Ty::Void {
                        " (returning a call that returns nothing is allowed only inside an \
                         `if` or a loop)"
                    } else {
                        ""
                    };
                    self.error(
                        Kind::ReturnValueInVoid,
                        span,
                        format!("{whose} returns nothing, so its `return` takes no value{note}"),
                    );
                }
            },
            (Some(ret), None) => self.error(
                Kind::ReturnWithoutValue,
                span,
                format!(
                    "`{}` returns {}, so its `return` needs a value",
                    self.current.name,
                    a(ret)
                ),
            ),
            (Some(ret), Some(expr)) => {
                let found = self.expr(expr);
                let what = format!("the value `{}` returns", self.current.name);
                if self.coerce(found, ret, &expr.span(), Kind::TypeMismatch, &what) {
                    self.emit(Instr::ReturnValue, span);
                }
            }
        }
    }

    /// `state name;`.
    fn state_change(&mut self, target: &StateName, span: &Range<usize>) {
        let (name, name_span) = state_name(target);
        let id = match self.symbols.get(name) {
            Some(GlobalSymbol::State(id)) => *id,
            _ => {
                let hint = did_you_mean(
                    name,
                    self.names_of(|symbol| matches!(symbol, GlobalSymbol::State(_))),
                );
                self.error(
                    Kind::Undefined,
                    &name_span,
                    format!("there is no state `{name}`{hint}"),
                );
                StateId::DEFAULT
            }
        };
        if self.current.kind == BodyKind::Function && self.current.if_depth == 0 {
            self.error(
                Kind::StateChangeInFunction,
                span,
                "a function can change state only inside an `if`".to_owned(),
            );
        }
        self.emit(Instr::StateChange(id), span);
    }

    /// An expression evaluated for its effect: an assignment or step stores
    /// without keeping the value, anything else is evaluated and dropped.
    fn effect(&mut self, expr: &Expr) {
        match expr {
            Expr::Assign {
                op,
                target,
                value,
                span,
            } => {
                let _void = self.assign(*op, target, value, span, false);
            }
            Expr::Prefix {
                op: op @ (PrefixOp::PreInc | PrefixOp::PreDec),
                operand,
                span,
            } => {
                let _void = self.step(*op, operand, span, false, false);
            }
            Expr::Postfix { op, operand, span } => {
                let _void = self.step(postfix_step(*op), operand, span, true, false);
            }
            _ => {
                if let Ty::Value(_) = self.expr(expr) {
                    self.emit(Instr::Pop, &expr.span());
                }
            }
        }
    }

    // -- expressions ------------------------------------------------------

    /// An expression, leaving its value on the stack; its type.
    fn expr(&mut self, expr: &Expr) -> Ty {
        match expr {
            Expr::Integer { raw, span } => {
                self.push_const(Value::Integer(literal::integer(raw)), span);
                Ty::Value(TypeName::Integer)
            }
            Expr::Float { raw, span } => {
                self.push_const(Value::Float(literal::float(raw)), span);
                Ty::Value(TypeName::Float)
            }
            Expr::Str { raw, span } => {
                self.push_const(Value::String(literal::string(raw)), span);
                Ty::Value(TypeName::String)
            }
            Expr::Variable(id) => self.variable(id),
            Expr::Member {
                base,
                component,
                span,
            } => match self.member(base, component) {
                Some(lvalue) => {
                    self.load(lvalue, span);
                    Ty::Value(TypeName::Float)
                }
                None => Ty::Error,
            },
            Expr::Call { callee, args, span } => self.call(callee, args, span),
            Expr::List { elements, span } => self.list(elements, span),
            Expr::Vector { x, y, z, span } => {
                self.components(&[x, y, z]);
                self.emit(Instr::BuildVector, span);
                Ty::Value(TypeName::Vector)
            }
            Expr::Rotation { x, y, z, s, span } => {
                self.components(&[x, y, z, s]);
                self.emit(Instr::BuildRotation, span);
                Ty::Value(TypeName::Rotation)
            }
            Expr::Prefix { op, operand, span } => self.prefix(*op, operand, span),
            Expr::Postfix { op, operand, span } => {
                self.step(postfix_step(*op), operand, span, true, true)
            }
            Expr::Binary { op, lhs, rhs, span } => self.binary(*op, lhs, rhs, span),
            Expr::Assign {
                op,
                target,
                value,
                span,
            } => self.assign(*op, target, value, span, true),
            Expr::Cast { ty, operand, span } => self.cast(ty.kind, operand, span),
            Expr::Paren { inner, .. } => self.expr(inner),
            Expr::Print { arg, span } => {
                match self.expr(arg) {
                    Ty::Value(TypeName::String) => self.emit(Instr::Print, span),
                    Ty::Value(_) => {
                        self.emit(Instr::Cast(TypeName::String), span);
                        self.emit(Instr::Print, span);
                    }
                    Ty::Void => self.error(
                        Kind::TypeMismatch,
                        span,
                        "`print` needs a value, but this returns nothing".to_owned(),
                    ),
                    Ty::Error => {}
                }
                Ty::Void
            }
            Expr::Error(span) => {
                self.error(
                    Kind::Syntax,
                    span,
                    "this expression does not parse".to_owned(),
                );
                Ty::Error
            }
        }
    }

    /// What a variable name refers to: the innermost local, then a global
    /// (only those declared so far, inside a global initialiser), then a
    /// library constant.
    fn resolve(&self, name: &str) -> Option<Resolved> {
        for scope in self.current.scopes.iter().rev() {
            if let Some((slot, ty)) = scope.variables.get(name) {
                return Some(Resolved::Local(*slot, *ty));
            }
        }
        if let Some(GlobalSymbol::Variable(slot)) = self.symbols.get(name) {
            let ty = self
                .globals
                .get(usize::try_from(slot.0).ok()?)
                .map(|global| global.ty)?;
            return Some(Resolved::Global(*slot, ty));
        }
        library::constant(name).map(|constant| Resolved::Constant(constant.value))
    }

    /// A variable reference.
    fn variable(&mut self, id: &Ident) -> Ty {
        match self.resolve(&id.name) {
            Some(Resolved::Local(slot, ty)) => {
                self.emit(Instr::LoadLocal(slot), &id.span);
                Ty::Value(ty)
            }
            Some(Resolved::Global(slot, ty)) => {
                self.emit(Instr::LoadGlobal(slot), &id.span);
                Ty::Value(ty)
            }
            Some(Resolved::Constant(value)) => {
                self.push_const(value.to_value(), &id.span);
                Ty::Value(value.type_name())
            }
            None => {
                self.undefined_variable(id);
                Ty::Error
            }
        }
    }

    /// Report a variable name that resolves to nothing — naming what it is
    /// instead, when it is a function or a state, and suggesting a near name
    /// otherwise.
    fn undefined_variable(&mut self, id: &Ident) {
        let message = match self.symbols.get(&id.name) {
            Some(GlobalSymbol::Function(_)) => {
                format!(
                    "`{}` is a function, not a variable; call it with `()`",
                    id.name
                )
            }
            Some(GlobalSymbol::State(_)) => format!("`{}` is a state, not a variable", id.name),
            Some(GlobalSymbol::Variable(_)) => {
                format!(
                    "`{}` is declared later; a global's initial value can use only the globals above it",
                    id.name
                )
            }
            None if library::builtin(&id.name).is_some() => {
                format!(
                    "`{}` is a library function, not a variable; call it with `()`",
                    id.name
                )
            }
            None => {
                let mut candidates: Vec<&str> = self
                    .current
                    .scopes
                    .iter()
                    .flat_map(|scope| scope.variables.keys().map(String::as_str))
                    .collect();
                candidates
                    .extend(self.names_of(|symbol| matches!(symbol, GlobalSymbol::Variable(_))));
                candidates.extend(library::CONSTANTS.iter().map(|constant| constant.name));
                let hint = did_you_mean(&id.name, candidates.into_iter());
                format!("no variable `{}` in scope here{hint}", id.name)
            }
        };
        self.error(Kind::Undefined, &id.span, message);
    }

    /// The global names whose symbol matches.
    fn names_of(&self, wanted: fn(&GlobalSymbol) -> bool) -> impl Iterator<Item = &str> {
        self.symbols
            .iter()
            .filter(move |(_, symbol)| wanted(symbol))
            .map(|(name, _)| name.as_str())
    }

    /// What an assignment or step targets: a variable, or one component of a
    /// vector or rotation variable.
    fn lvalue(&mut self, target: &Expr) -> Option<LValue> {
        match target {
            Expr::Variable(id) => match self.resolve(&id.name) {
                Some(Resolved::Local(slot, ty)) => Some(LValue {
                    place: Place::Local(slot, ty),
                    member: None,
                }),
                Some(Resolved::Global(slot, ty)) => Some(LValue {
                    place: Place::Global(slot, ty),
                    member: None,
                }),
                // A constant is a keyword token in Linden's grammar.
                Some(Resolved::Constant(_)) => {
                    self.error(
                        Kind::Syntax,
                        &id.span,
                        format!("`{}` is a library constant and cannot be changed", id.name),
                    );
                    None
                }
                None => {
                    self.undefined_variable(id);
                    None
                }
            },
            Expr::Member {
                base, component, ..
            } => self.member(base, component),
            _ => {
                self.error(
                    Kind::Syntax,
                    &target.span(),
                    "only a variable or one component of a vector or rotation variable can be \
                     assigned, incremented or decremented"
                        .to_owned(),
                );
                None
            }
        }
    }

    /// `base.component`: a vector variable's `x`, `y` or `z`, or a rotation
    /// variable's `x`, `y`, `z` or `s`. Not a constant's — `ZERO_VECTOR.x`
    /// is rejected, as the reference rejects it.
    fn member(&mut self, base: &Ident, component: &Ident) -> Option<LValue> {
        let place = match self.resolve(&base.name) {
            Some(Resolved::Local(slot, ty)) => Place::Local(slot, ty),
            Some(Resolved::Global(slot, ty)) => Place::Global(slot, ty),
            Some(Resolved::Constant(_)) => {
                self.error(
                    Kind::InvalidMember,
                    &component.span,
                    format!(
                        "`{}` is a library constant; a component can be taken only from a \
                         variable",
                        base.name
                    ),
                );
                return None;
            }
            None => {
                self.undefined_variable(base);
                return None;
            }
        };
        let (Place::Local(_, ty) | Place::Global(_, ty)) = place;
        let member = Component::from_name(&component.name).filter(|member| match ty {
            TypeName::Vector => *member != Component::S,
            TypeName::Rotation => true,
            TypeName::Integer
            | TypeName::Float
            | TypeName::String
            | TypeName::Key
            | TypeName::List => false,
        });
        if member.is_none() {
            let message = match ty {
                TypeName::Vector => format!(
                    "`{}` is a vector, which has `.x`, `.y` and `.z`, not `.{}`",
                    base.name, component.name
                ),
                TypeName::Rotation => format!(
                    "`{}` is a rotation, which has `.x`, `.y`, `.z` and `.s`, not `.{}`",
                    base.name, component.name
                ),
                TypeName::Integer
                | TypeName::Float
                | TypeName::String
                | TypeName::Key
                | TypeName::List => format!(
                    "`{}` is `{}`; only vectors and rotations have components",
                    base.name,
                    ty.keyword()
                ),
            };
            self.error(Kind::InvalidMember, &component.span, message);
        }
        member.map(|member| LValue {
            place,
            member: Some(member),
        })
    }

    /// Push the value of an lvalue.
    fn load(&mut self, lvalue: LValue, span: &Range<usize>) {
        match lvalue.place {
            Place::Local(slot, _) => self.emit(Instr::LoadLocal(slot), span),
            Place::Global(slot, _) => self.emit(Instr::LoadGlobal(slot), span),
        }
        if let Some(member) = lvalue.member {
            self.emit(Instr::GetMember(member), span);
        }
    }

    /// Store the top value into an lvalue; a component is written into the
    /// variable's current value.
    fn store(&mut self, lvalue: LValue, span: &Range<usize>) {
        if let Some(member) = lvalue.member {
            match lvalue.place {
                Place::Local(slot, _) => self.emit(Instr::LoadLocal(slot), span),
                Place::Global(slot, _) => self.emit(Instr::LoadGlobal(slot), span),
            }
            self.emit(Instr::SetMember(member), span);
        }
        match lvalue.place {
            Place::Local(slot, _) => self.emit(Instr::StoreLocal(slot), span),
            Place::Global(slot, _) => self.emit(Instr::StoreGlobal(slot), span),
        }
    }

    /// A call to a user or library function. Arguments are evaluated left to
    /// right, each converted to its parameter's type.
    fn call(&mut self, callee: &Ident, args: &[Expr], span: &Range<usize>) -> Ty {
        enum Target {
            /// A user function.
            User(FunctionId),
            /// A library function.
            Builtin(BuiltinId),
        }
        let (target, params, ret) =
            if let Some(GlobalSymbol::Function(id)) = self.symbols.get(&callee.name).copied() {
                let signature = self
                    .signatures
                    .get(usize::try_from(id.0).unwrap_or(usize::MAX))
                    .cloned()
                    .unwrap_or(Signature {
                        params: Vec::new(),
                        ret: None,
                    });
                (Target::User(id), signature.params, signature.ret)
            } else if let Some(builtin) = library::builtin(&callee.name) {
                (
                    Target::Builtin(builtin.id),
                    builtin.args.iter().map(|arg| arg.ty).collect(),
                    builtin.ret,
                )
            } else {
                let message = if self.resolve(&callee.name).is_some() {
                    format!("`{}` is a variable, not a function", callee.name)
                } else {
                    let mut candidates: Vec<&str> = self
                        .names_of(|symbol| matches!(symbol, GlobalSymbol::Function(_)))
                        .collect();
                    candidates.extend(library::BUILTINS.iter().map(|builtin| builtin.name));
                    let hint = did_you_mean(&callee.name, candidates.into_iter());
                    format!("no function `{}`{hint}", callee.name)
                };
                self.error(Kind::Undefined, &callee.span, message);
                for arg in args {
                    let _checked = self.expr(arg);
                }
                return Ty::Error;
            };
        let signature = format!(
            "`{}({})`",
            callee.name,
            match &target {
                Target::Builtin(id) => id
                    .descriptor()
                    .args
                    .iter()
                    .map(|arg| format!("{} {}", arg.ty.keyword(), arg.name))
                    .collect::<Vec<_>>()
                    .join(", "),
                Target::User(_) => params
                    .iter()
                    .map(|param| param.keyword())
                    .collect::<Vec<_>>()
                    .join(", "),
            }
        );
        if args.len() == params.len() {
            for (index, (arg, param)) in args.iter().zip(params).enumerate() {
                let found = self.expr(arg);
                let what = format!("argument {} of {signature}", index.saturating_add(1));
                let _converted =
                    self.coerce(found, param, &arg.span(), Kind::FunctionMismatch, &what);
            }
        } else {
            self.error(
                Kind::FunctionMismatch,
                span,
                format!(
                    "{signature} takes {} argument{}, but {} {} given",
                    params.len(),
                    if params.len() == 1 { "" } else { "s" },
                    args.len(),
                    if args.len() == 1 { "was" } else { "were" }
                ),
            );
            for arg in args {
                let _checked = self.expr(arg);
            }
        }
        match target {
            Target::User(id) => self.emit(Instr::CallFunction(id), span),
            Target::Builtin(id) => self.emit(Instr::CallBuiltin(id), span),
        }
        ret.map_or(Ty::Void, Ty::Value)
    }

    /// `[a, b, …]`: elements left to right, none a list or void.
    fn list(&mut self, elements: &[Expr], span: &Range<usize>) -> Ty {
        for element in elements {
            match self.expr(element) {
                Ty::Value(TypeName::List) => self.error(
                    Kind::ListInList,
                    &element.span(),
                    "a list cannot contain a list; join lists with `+` instead".to_owned(),
                ),
                Ty::Void => self.error(
                    Kind::TypeMismatch,
                    &element.span(),
                    "a list element needs a value, but this returns nothing".to_owned(),
                ),
                Ty::Value(_) | Ty::Error => {}
            }
        }
        let count = self.index(elements.len(), span);
        self.emit(Instr::BuildList(count), span);
        Ty::Value(TypeName::List)
    }

    /// The components of a vector or rotation constructor: left to right,
    /// each a float or an integer converted to one.
    fn components(&mut self, components: &[&Expr]) {
        let what = if components.len() == 4 {
            "a rotation component"
        } else {
            "a vector component"
        };
        for component in components {
            let found = self.expr(component);
            let _converted = self.coerce(
                found,
                TypeName::Float,
                &component.span(),
                Kind::TypeMismatch,
                what,
            );
        }
    }

    /// A prefix operator. `-` of a numeric literal is folded into the
    /// literal, as Linden's lexer reads a negative number.
    fn prefix(&mut self, op: PrefixOp, operand: &Expr, span: &Range<usize>) -> Ty {
        match (op, operand) {
            (PrefixOp::PreInc | PrefixOp::PreDec, _) => self.step(op, operand, span, false, true),
            (PrefixOp::Neg, Expr::Integer { raw, .. }) => {
                self.push_const(Value::Integer(literal::integer(raw).wrapping_neg()), span);
                Ty::Value(TypeName::Integer)
            }
            (PrefixOp::Neg, Expr::Float { raw, .. }) => {
                self.push_const(Value::Float(-literal::float(raw)), span);
                Ty::Value(TypeName::Float)
            }
            (PrefixOp::Neg | PrefixOp::Not | PrefixOp::BitNot, _) => match self.expr(operand) {
                Ty::Value(ty) => {
                    if let Some(result) = types::prefix_result(op, ty) {
                        self.emit(Instr::Prefix(op), span);
                        Ty::Value(result)
                    } else {
                        let takes = match op {
                            PrefixOp::Neg => "an `integer`, `float`, `vector` or `rotation`",
                            PrefixOp::Not
                            | PrefixOp::BitNot
                            | PrefixOp::PreInc
                            | PrefixOp::PreDec => "an `integer`",
                        };
                        self.error(
                            Kind::TypeMismatch,
                            span,
                            format!(
                                "`{}` takes {takes}, not `{}`",
                                prefix_symbol(op),
                                ty.keyword()
                            ),
                        );
                        Ty::Error
                    }
                }
                Ty::Void => {
                    self.error(
                        Kind::TypeMismatch,
                        span,
                        format!(
                            "`{}` needs a value, but this returns nothing",
                            prefix_symbol(op)
                        ),
                    );
                    Ty::Error
                }
                Ty::Error => Ty::Error,
            },
        }
    }

    /// `++x`, `--x`, `x++` or `x--` (`post`), on an integer or float
    /// variable or component. With `want` the expression's value — the new
    /// value for a pre-, the old for a post-step — is left on the stack.
    fn step(
        &mut self,
        step: PrefixOp,
        operand: &Expr,
        span: &Range<usize>,
        post: bool,
        want: bool,
    ) -> Ty {
        let Some(lvalue) = self.lvalue(operand) else {
            return Ty::Error;
        };
        let ty = lvalue.ty();
        if types::prefix_result(step, ty).is_none() {
            self.error(
                Kind::TypeMismatch,
                span,
                format!(
                    "`{}` takes an `integer` or `float` variable, not `{}`",
                    prefix_symbol(step),
                    ty.keyword()
                ),
            );
            return Ty::Error;
        }
        self.load(lvalue, span);
        if post && want {
            self.emit(Instr::Dup, span);
        }
        self.emit(Instr::Prefix(step), span);
        if !post && want {
            self.emit(Instr::Dup, span);
        }
        self.store(lvalue, span);
        if want { Ty::Value(ty) } else { Ty::Void }
    }

    /// A binary operator: the right operand first, then the left, then the
    /// operator, with an `integer` operand of a mixed numeric operation
    /// converted to `float` explicitly.
    fn binary(&mut self, op: BinaryOp, lhs: &Expr, rhs: &Expr, span: &Range<usize>) -> Ty {
        let right = self.expr(rhs);
        let mark = self.here();
        let left = self.expr(lhs);
        let (left, right) = match (left, right) {
            (Ty::Value(left), Ty::Value(right)) => (left, right),
            (Ty::Error, _) | (_, Ty::Error) => return Ty::Error,
            (Ty::Void, _) | (_, Ty::Void) => {
                self.error(
                    Kind::TypeMismatch,
                    span,
                    format!(
                        "`{}` needs a value on both sides, but one side returns nothing",
                        binary_symbol(op)
                    ),
                );
                return Ty::Error;
            }
        };
        let Some(result) = types::binary_result(op, left, right) else {
            self.error(
                Kind::TypeMismatch,
                span,
                format!(
                    "there is no `{} {} {}`{}",
                    left.keyword(),
                    binary_symbol(op),
                    right.keyword(),
                    binary_hint(op, left, right)
                ),
            );
            return Ty::Error;
        };
        let concatenation =
            op == BinaryOp::Add && (left == TypeName::List || right == TypeName::List);
        if !concatenation {
            if left == TypeName::Integer && promotes(right) {
                self.emit(Instr::Cast(TypeName::Float), &lhs.span());
            }
            if right == TypeName::Integer && promotes(left) {
                self.insert_cast(mark, TypeName::Float, &rhs.span());
            }
        }
        self.emit(Instr::Binary(op), span);
        Ty::Value(result)
    }

    /// An assignment. `=` converts the value to the target's type; a compound
    /// assignment evaluates the value, then loads the target (right to left,
    /// as the operator would), applies the operator and stores.
    fn assign(
        &mut self,
        op: AssignOp,
        target: &Expr,
        value: &Expr,
        span: &Range<usize>,
        want: bool,
    ) -> Ty {
        let found = self.expr(value);
        let Some(operator) = compound_operator(op) else {
            let Some(lvalue) = self.lvalue(target) else {
                return Ty::Error;
            };
            let what = format!(
                "the value assigned to `{}`",
                self.source.text().get(target.span()).unwrap_or_default()
            );
            if !self.coerce(found, lvalue.ty(), span, Kind::TypeMismatch, &what) {
                return Ty::Error;
            }
            if want {
                self.emit(Instr::Dup, span);
            }
            self.store(lvalue, span);
            return if want {
                Ty::Value(lvalue.ty())
            } else {
                Ty::Void
            };
        };
        let mark = self.here();
        let Some(lvalue) = self.lvalue(target) else {
            return Ty::Error;
        };
        let ty = lvalue.ty();
        let right = match found {
            Ty::Value(right) => right,
            Ty::Error => return Ty::Error,
            Ty::Void => {
                self.error(
                    Kind::TypeMismatch,
                    span,
                    format!(
                        "`{}` needs a value on its right, but this returns nothing",
                        assign_symbol(op)
                    ),
                );
                return Ty::Error;
            }
        };
        if types::assign_result(op, ty, right).is_none() {
            let message = match types::binary_result(operator, ty, right) {
                Some(result) => format!(
                    "`{} {} {}` is {}, which `{}` cannot store back into {}",
                    ty.keyword(),
                    binary_symbol(operator),
                    right.keyword(),
                    a(result),
                    assign_symbol(op),
                    a(ty)
                ),
                None => format!(
                    "there is no `{} {} {}`{}",
                    ty.keyword(),
                    binary_symbol(operator),
                    right.keyword(),
                    binary_hint(operator, ty, right)
                ),
            };
            self.error(Kind::TypeMismatch, span, message);
            return Ty::Error;
        }
        self.load(lvalue, &target.span());
        if op == AssignOp::MulAssign && ty == TypeName::Integer && right == TypeName::Float {
            // `integer *= float`: `target = (integer)((float)target * value)`.
            // The reference leaves the stored integer as the expression's
            // value, which the type table calls a float.
            self.emit(Instr::Cast(TypeName::Float), span);
            self.emit(Instr::Binary(BinaryOp::Mul), span);
            self.emit(Instr::Cast(TypeName::Integer), span);
            if want {
                self.emit(Instr::Dup, span);
            }
            self.store(lvalue, span);
            if want {
                // Second Life compiles a use of that value, then refuses the
                // whole method: Mono's verifier finds an `int32` where the
                // IL says `float` (`InvalidProgramException`, measured on
                // aditi for a store into a float local). The body faults on
                // entry; the cast keeps the rest of it well typed.
                if self.current.invalid_program.is_none() {
                    self.current.invalid_program = Some(span.clone());
                }
                self.emit(Instr::Cast(TypeName::Float), span);
                return Ty::Value(TypeName::Float);
            }
            return Ty::Void;
        }
        let concatenation = operator == BinaryOp::Add && ty == TypeName::List;
        if !concatenation && right == TypeName::Integer && promotes(ty) {
            self.insert_cast(mark, TypeName::Float, &value.span());
        }
        self.emit(Instr::Binary(operator), span);
        if want {
            self.emit(Instr::Dup, span);
        }
        self.store(lvalue, span);
        if want { Ty::Value(ty) } else { Ty::Void }
    }

    /// `(type)operand`, legal or a type mismatch.
    fn cast(&mut self, to: TypeName, operand: &Expr, span: &Range<usize>) -> Ty {
        match self.expr(operand) {
            Ty::Value(from) if types::cast_legal(from, to) => {
                self.emit(Instr::Cast(to), span);
                Ty::Value(to)
            }
            Ty::Value(from) => {
                self.error(
                    Kind::TypeMismatch,
                    span,
                    format!("{} cannot be cast to `{}`", a(from), to.keyword()),
                );
                Ty::Error
            }
            Ty::Void => {
                self.error(
                    Kind::TypeMismatch,
                    span,
                    "a cast needs a value, but this returns nothing".to_owned(),
                );
                Ty::Error
            }
            Ty::Error => Ty::Error,
        }
    }
}

/// `; did you mean `x`?` when a candidate is a plausible typo of `name`,
/// else nothing.
fn did_you_mean<'a>(name: &str, candidates: impl Iterator<Item = &'a str>) -> String {
    sl_lsl::closest(name, candidates)
        .map_or_else(String::new, |close| format!("; did you mean `{close}`?"))
}

/// A type with its indefinite article: "an `integer`", "a `float`".
fn a(ty: TypeName) -> String {
    let article = if ty == TypeName::Integer { "an" } else { "a" };
    format!("{article} `{}`", ty.keyword())
}

/// A binary operator as the source spells it.
const fn binary_symbol(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Add => "+",
        BinaryOp::Sub => "-",
        BinaryOp::Mul => "*",
        BinaryOp::Div => "/",
        BinaryOp::Mod => "%",
        BinaryOp::Eq => "==",
        BinaryOp::Ne => "!=",
        BinaryOp::Lt => "<",
        BinaryOp::Le => "<=",
        BinaryOp::Gt => ">",
        BinaryOp::Ge => ">=",
        BinaryOp::Shl => "<<",
        BinaryOp::Shr => ">>",
        BinaryOp::BitAnd => "&",
        BinaryOp::BitOr => "|",
        BinaryOp::BitXor => "^",
        BinaryOp::And => "&&",
        BinaryOp::Or => "||",
    }
}

/// A prefix operator as the source spells it.
const fn prefix_symbol(op: PrefixOp) -> &'static str {
    match op {
        PrefixOp::Neg => "-",
        PrefixOp::Not => "!",
        PrefixOp::BitNot => "~",
        PrefixOp::PreInc => "++",
        PrefixOp::PreDec => "--",
    }
}

/// An assignment operator as the source spells it.
const fn assign_symbol(op: AssignOp) -> &'static str {
    match op {
        AssignOp::Assign => "=",
        AssignOp::AddAssign => "+=",
        AssignOp::SubAssign => "-=",
        AssignOp::MulAssign => "*=",
        AssignOp::DivAssign => "/=",
        AssignOp::ModAssign => "%=",
    }
}

/// A hint for the operand combinations people most often expect to work.
fn binary_hint(op: BinaryOp, left: TypeName, right: TypeName) -> String {
    let text = |ty| matches!(ty, TypeName::String | TypeName::Key);
    match op {
        BinaryOp::Add if text(left) && text(right) => {
            "; `+` joins two strings, not keys — cast the key with `(string)`".to_owned()
        }
        BinaryOp::Add if text(left) || text(right) => {
            "; cast the other side with `(string)` to join them".to_owned()
        }
        BinaryOp::And
        | BinaryOp::Or
        | BinaryOp::BitAnd
        | BinaryOp::BitOr
        | BinaryOp::BitXor
        | BinaryOp::Shl
        | BinaryOp::Shr => {
            format!("; `{}` works on integers only", binary_symbol(op))
        }
        _ => String::new(),
    }
}

/// The name and span of a state header or `state` target.
fn state_name(name: &StateName) -> (&str, Range<usize>) {
    match name {
        StateName::Default(span) => ("default", span.clone()),
        StateName::Named(id) => (id.name.as_str(), id.span.clone()),
    }
}

/// The span of a block's closing brace, for the implicit return.
const fn closing(block: &Range<usize>) -> Range<usize> {
    block.end.saturating_sub(1)..block.end
}

/// Whether a statement ends every path through it with a `return`, by the
/// reference's rule: a `return`; a block whose last statement does; an `if`
/// with an `else` whose branches both do. Nothing else.
fn always_returns(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Return { .. } => true,
        Stmt::Block(block) => block.statements.last().is_some_and(always_returns),
        Stmt::If {
            then_branch,
            else_branch: Some(else_branch),
            ..
        } => always_returns(then_branch) && always_returns(else_branch),
        _ => false,
    }
}

/// Whether a global initialiser has the only shape Linden's grammar allows
/// there: a literal (negative numbers included), a global or constant name
/// (negated only if it is a numeric constant other than `TRUE`/`FALSE`), or
/// a vector, rotation or list of those — a list inside no list.
fn simple_assignable(expr: &Expr, in_list: bool) -> bool {
    match expr {
        Expr::Integer { .. } | Expr::Float { .. } | Expr::Str { .. } | Expr::Variable(_) => true,
        Expr::Prefix {
            op: PrefixOp::Neg,
            operand,
            ..
        } => match operand.as_ref() {
            Expr::Integer { .. } | Expr::Float { .. } => true,
            Expr::Variable(id) => {
                id.name != "TRUE"
                    && id.name != "FALSE"
                    && library::constant(&id.name).is_some_and(|constant| {
                        matches!(
                            constant.value,
                            ConstantValue::Integer(_) | ConstantValue::Float(_)
                        )
                    })
            }
            _ => false,
        },
        Expr::Vector { x, y, z, .. } => [x, y, z]
            .iter()
            .all(|component| simple_assignable(component, false)),
        Expr::Rotation { x, y, z, s, .. } => [x, y, z, s]
            .iter()
            .all(|component| simple_assignable(component, false)),
        Expr::List { elements, .. } => {
            !in_list
                && elements
                    .iter()
                    .all(|element| simple_assignable(element, true))
        }
        _ => false,
    }
}

/// Whether an `integer` operand is promoted to `float` next to this type.
const fn promotes(other: TypeName) -> bool {
    matches!(other, TypeName::Float | TypeName::Vector)
}

/// The operator of a compound assignment; [`None`] for `=`.
const fn compound_operator(op: AssignOp) -> Option<BinaryOp> {
    match op {
        AssignOp::Assign => None,
        AssignOp::AddAssign => Some(BinaryOp::Add),
        AssignOp::SubAssign => Some(BinaryOp::Sub),
        AssignOp::MulAssign => Some(BinaryOp::Mul),
        AssignOp::DivAssign => Some(BinaryOp::Div),
        AssignOp::ModAssign => Some(BinaryOp::Mod),
    }
}

/// The step a postfix operator takes.
const fn postfix_step(op: PostfixOp) -> PrefixOp {
    match op {
        PostfixOp::PostInc => PrefixOp::PreInc,
        PostfixOp::PostDec => PrefixOp::PreDec,
    }
}

/// A jump instruction's target.
const fn target(instr: Instr) -> Option<CodeOffset> {
    match instr {
        Instr::Jump(target) | Instr::JumpIfFalse(target) | Instr::JumpIfTrue(target) => {
            Some(target)
        }
        _ => None,
    }
}

/// A jump instruction with a new target; anything else unchanged.
const fn retarget(instr: Instr, to: CodeOffset) -> Instr {
    match instr {
        Instr::Jump(_) => Instr::Jump(to),
        Instr::JumpIfFalse(_) => Instr::JumpIfFalse(to),
        Instr::JumpIfTrue(_) => Instr::JumpIfTrue(to),
        other => other,
    }
}

/// Whether two literals are the same — by bits for floats, so `-0.0` and
/// `0.0` stay apart in the pool.
fn same_literal(left: &Value, right: &Value) -> bool {
    let bits = |values: &[f32]| {
        values
            .iter()
            .map(|value| value.to_bits())
            .collect::<Vec<_>>()
    };
    match (left, right) {
        (Value::Float(a), Value::Float(b)) => a.to_bits() == b.to_bits(),
        (Value::Vector(a), Value::Vector(b)) => bits(&[a.x, a.y, a.z]) == bits(&[b.x, b.y, b.z]),
        (Value::Rotation(a), Value::Rotation(b)) => {
            bits(&[a.x, a.y, a.z, a.s]) == bits(&[b.x, b.y, b.z, b.s])
        }
        _ => left == right,
    }
}
