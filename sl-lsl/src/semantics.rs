//! The **semantic pass** over the [`crate::ast`] tree — the checks LSL's type
//! rules make decidable *locally*, without a grid round-trip.
//!
//! [`analyze`] walks a parsed [`Script`] against the grid's library
//! ([`LslSyntax`], from the `LSLSyntax` capability) and returns a list of
//! [`Diagnostic`]s: **undefined symbols** (calls, variables, states, labels,
//! events), **call arity and type** at the call site, **return** correctness
//! (a value where none is wanted, a missing value, a function that can fall off
//! its end), **duplicate definitions**, and **state reachability**.
//!
//! This earns its keep because **SL has no compile-without-save**: compilation
//! happens *as part of* the upload, so every "did I typo that function name?"
//! is a network round-trip that mutates the world (the in-world script is
//! replaced and its state resets). Local checking is the only way to type-check
//! without touching the grid.
//!
//! ## The no-false-positive bar
//!
//! A false *error* on code the grid would happily compile is worse than no
//! error at all, so the pass is deliberately conservative:
//!
//! - Checks that would false-positive without the library table are **gated on
//!   a non-empty table** — an empty [`LslSyntax`] (the grid data not yet
//!   fetched) suppresses undefined-symbol reporting rather than flagging every
//!   `ll*` call and every `PI`.
//! - Type inference returns "unknown" generously — for arithmetic whose result
//!   type is operand-dependent, for a void call used as a value, for anything it
//!   cannot pin down — and an unknown operand **skips** the type check rather
//!   than guessing.
//! - Symbol resolution is **order-insensitive**: a name defined anywhere in the
//!   right scope counts as defined, so LSL's single-pass "defined before use"
//!   rule can only ever cause a *missed* error here, never a false one.
//! - Missing-return-on-a-path is a **warning**, not an error, because the grid's
//!   own tolerance for falling off the end of a value function is not something
//!   this pass claims to speak for authoritatively.
//!
//! Three things stay authoritative on the server and the pass does not claim to
//! speak for them: the Mono/CIL path is only *semantically equivalent* to the
//! legacy bytecode; OpenSim compiles LSL to C# with its own quirks; and several
//! failures are not front-end errors at all (script too large, no modify
//! permission, upload failure). Meeting the bar is *proven* — not asserted — by
//! the differential-testing oracle that diffs this pass against `tailslide`.

use core::ops::Range;
use std::collections::HashMap;
use std::collections::HashSet;

use crate::ast::{
    Block, Expr, FunctionDef, GlobalItem, Script, StateDef, StateName, Stmt, TypeName,
};
use crate::syntax::LslSyntax;

/// How seriously to take a [`Diagnostic`]: a definite compile error the grid
/// would reject, or a warning about dubious-but-legal code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Severity {
    /// The grid's front-end would reject this — a definite compile failure.
    Error,
    /// Legal, but very likely a mistake (dead code, a value function that can
    /// fall off its end). The pass never claims the grid rejects it.
    Warning,
}

/// The machine-readable kind of a [`Diagnostic`], carrying the identifiers,
/// types and counts a richer renderer (spans, carets, "did you mean…?") needs
/// without re-deriving them from the message text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticKind {
    /// A call to a name that is neither a user function nor a library function.
    UndefinedFunction {
        /// The called name.
        name: String,
    },
    /// A reference to a variable that resolves to no local, parameter, global
    /// or library constant.
    UndefinedVariable {
        /// The referenced name.
        name: String,
    },
    /// A `state name;` whose target is not a defined state.
    UndefinedState {
        /// The target state name.
        name: String,
    },
    /// A `jump label;` whose target label is not defined in the same function or
    /// event handler.
    UndefinedLabel {
        /// The target label name.
        name: String,
    },
    /// An event handler whose name the grid's event table does not know.
    UnknownEvent {
        /// The handler name.
        name: String,
    },
    /// A script with states but no `default` state (LSL requires one).
    MissingDefaultState,
    /// A state no reachable `state name;` can ever transition to (dead code).
    UnreachableState {
        /// The unreachable state's name.
        name: String,
    },
    /// A call with the wrong number of arguments for its (user or library)
    /// signature. LSL has no overloading, defaults or varargs, so this is exact.
    WrongArgCount {
        /// The called name.
        callee: String,
        /// The number of arguments the signature declares.
        expected: usize,
        /// The number of arguments passed.
        found: usize,
    },
    /// An argument whose type is incompatible with the parameter it fills,
    /// accounting for LSL's implicit conversions (`integer`→`float`,
    /// `string`↔`key`).
    ArgTypeMismatch {
        /// The called name.
        callee: String,
        /// The zero-based argument position.
        index: usize,
        /// The type the parameter declares.
        expected: TypeName,
        /// The type the argument evaluates to.
        found: TypeName,
    },
    /// An event handler declared with the wrong number of parameters for its
    /// grid signature.
    WrongEventArgCount {
        /// The event name.
        event: String,
        /// The number of parameters the grid signature declares.
        expected: usize,
        /// The number of parameters declared.
        found: usize,
    },
    /// An event-handler parameter whose declared type does not match the grid's
    /// event signature (event parameters require an exact type, no implicit
    /// conversion).
    EventArgTypeMismatch {
        /// The event name.
        event: String,
        /// The zero-based parameter position.
        index: usize,
        /// The type the grid signature declares.
        expected: TypeName,
        /// The type the handler declared.
        found: TypeName,
    },
    /// A `return value;` inside a `void` function or an event handler, neither
    /// of which yields a value.
    ReturnValueInVoid,
    /// A bare `return;` inside a function declared with a return type.
    MissingReturnValue {
        /// The return type the function declares.
        expected: TypeName,
    },
    /// A returned expression whose type is incompatible with the function's
    /// declared return type.
    ReturnTypeMismatch {
        /// The declared return type.
        expected: TypeName,
        /// The returned expression's type.
        found: TypeName,
    },
    /// A value function whose body can reach its end without returning a value
    /// on every path (a warning — see the module's no-false-positive note).
    MissingReturn {
        /// The function name.
        function: String,
        /// The return type it fails to always produce.
        expected: TypeName,
    },
    /// A second function defined with a name already taken by another function.
    DuplicateFunction {
        /// The repeated name.
        name: String,
    },
    /// A second global variable declared with an already-used name.
    DuplicateGlobal {
        /// The repeated name.
        name: String,
    },
    /// A second `state` block with an already-used name.
    DuplicateState {
        /// The repeated name.
        name: String,
    },
    /// Two parameters of one function or event handler sharing a name.
    DuplicateParam {
        /// The repeated name.
        name: String,
    },
    /// Two handlers for the same event within one state.
    DuplicateEvent {
        /// The repeated event name.
        name: String,
    },
    /// A local variable redeclared in the same block scope.
    DuplicateLocal {
        /// The repeated name.
        name: String,
    },
    /// Two jump labels with the same name in one function or event handler.
    DuplicateLabel {
        /// The repeated label name.
        name: String,
    },
    /// An assignment whose target is a read-only library constant.
    AssignToConstant {
        /// The constant's name.
        name: String,
    },
}

impl DiagnosticKind {
    /// The severity this kind carries. Everything the grid's front-end would
    /// reject is an [`Severity::Error`]; the two "legal but suspicious" kinds
    /// ([`Self::MissingReturn`], [`Self::UnreachableState`]) are
    /// [`Severity::Warning`].
    #[must_use]
    pub const fn severity(&self) -> Severity {
        match self {
            Self::MissingReturn { .. } | Self::UnreachableState { .. } => Severity::Warning,
            _ => Severity::Error,
        }
    }

    /// A default human-readable message. A richer renderer (the diagnostics
    /// task) may re-render from the structured fields instead; this is the
    /// plain fallback and what the unit tests read.
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Self::UndefinedFunction { name } => {
                format!("call to undefined function `{name}`")
            }
            Self::UndefinedVariable { name } => format!("undefined variable `{name}`"),
            Self::UndefinedState { name } => format!("undefined state `{name}`"),
            Self::UndefinedLabel { name } => format!("undefined label `{name}`"),
            Self::UnknownEvent { name } => format!("unknown event handler `{name}`"),
            Self::MissingDefaultState => "script has no `default` state".to_owned(),
            Self::UnreachableState { name } => format!("state `{name}` is never reached"),
            Self::WrongArgCount {
                callee,
                expected,
                found,
            } => format!("`{callee}` takes {expected} argument(s) but {found} were supplied"),
            Self::ArgTypeMismatch {
                callee,
                index,
                expected,
                found,
            } => format!(
                "argument {} of `{callee}` expects `{}`, got `{}`",
                index.saturating_add(1),
                expected.keyword(),
                found.keyword()
            ),
            Self::WrongEventArgCount {
                event,
                expected,
                found,
            } => format!("event `{event}` takes {expected} parameter(s) but {found} were declared"),
            Self::EventArgTypeMismatch {
                event,
                index,
                expected,
                found,
            } => format!(
                "parameter {} of event `{event}` must be `{}`, not `{}`",
                index.saturating_add(1),
                expected.keyword(),
                found.keyword()
            ),
            Self::ReturnValueInVoid => {
                "returning a value from a function or event that yields none".to_owned()
            }
            Self::MissingReturnValue { expected } => {
                format!(
                    "`return;` in a function that must return `{}`",
                    expected.keyword()
                )
            }
            Self::ReturnTypeMismatch { expected, found } => format!(
                "returning `{}` from a function declared to return `{}`",
                found.keyword(),
                expected.keyword()
            ),
            Self::MissingReturn { function, expected } => format!(
                "function `{function}` may reach its end without returning `{}`",
                expected.keyword()
            ),
            Self::DuplicateFunction { name } => format!("function `{name}` is already defined"),
            Self::DuplicateGlobal { name } => {
                format!("global variable `{name}` is already defined")
            }
            Self::DuplicateState { name } => format!("state `{name}` is already defined"),
            Self::DuplicateParam { name } => format!("parameter `{name}` is already declared"),
            Self::DuplicateEvent { name } => {
                format!("event `{name}` is already handled in this state")
            }
            Self::DuplicateLocal { name } => {
                format!("variable `{name}` is already declared in this scope")
            }
            Self::DuplicateLabel { name } => format!("label `{name}` is already defined"),
            Self::AssignToConstant { name } => {
                format!("cannot assign to the constant `{name}`")
            }
        }
    }
}

/// One semantic finding: its severity, a human-readable message, the machine
/// readable [`DiagnosticKind`], and the byte span it points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// How seriously to take the finding.
    pub severity: Severity,
    /// The default human-readable message (`kind.message()`).
    pub message: String,
    /// The structured kind, for a richer renderer.
    pub kind: DiagnosticKind,
    /// The byte range in the source the finding points at.
    pub span: Range<usize>,
}

/// Run the semantic pass over `script`, checking it against the grid library
/// `syntax`, and return the findings sorted by source position.
///
/// Total and side-effect-free: it never panics and never touches the grid. Pass
/// the [`LslSyntax`] the grid served for the region the script will run on; an
/// empty table (not yet fetched) suppresses the checks that would otherwise
/// false-positive on library symbols (see the module note).
#[must_use]
pub fn analyze(script: &Script, syntax: &LslSyntax) -> Vec<Diagnostic> {
    let mut analyzer = Analyzer::new(script, syntax);
    analyzer.run(script);
    analyzer.diagnostics.sort_by(|a, b| {
        a.span
            .start
            .cmp(&b.span.start)
            .then_with(|| a.span.end.cmp(&b.span.end))
    });
    analyzer.diagnostics
}

/// The walker's mutable state: the collected top-level symbol tables, the
/// current lexical scope stack, the return-type context of the body being
/// walked, and the accumulating diagnostics.
struct Analyzer<'a> {
    /// The grid library the script is checked against.
    syntax: &'a LslSyntax,
    /// User functions by name, for call arity/type resolution.
    functions: HashMap<&'a str, &'a FunctionDef>,
    /// Global variable types by name, for reference resolution and typing.
    globals: HashMap<&'a str, TypeName>,
    /// Defined state names (including `default`), for `state name;` targets.
    states: HashSet<&'a str>,
    /// The lexical scope stack: innermost block last. Each frame maps a
    /// variable name to its declared type.
    scopes: Vec<HashMap<String, TypeName>>,
    /// The labels defined in the function or event body currently being walked,
    /// for `jump` resolution.
    labels: HashSet<String>,
    /// Labels *encountered so far* while walking the current body, to report a
    /// second `@label` with a name already defined ([`Self::labels`] is the full
    /// deduplicated set and cannot itself detect the repeat).
    seen_labels: HashSet<String>,
    /// The declared return type of the function currently being walked, or
    /// [`None`] inside a `void` function or an event handler.
    current_ret: Option<TypeName>,
    /// The accumulating findings.
    diagnostics: Vec<Diagnostic>,
}

impl<'a> Analyzer<'a> {
    /// Build an analyzer, collecting the top-level symbol tables (functions,
    /// globals, states) and reporting duplicates among them up front so the
    /// later reference-resolution walk sees a complete, order-insensitive view.
    fn new(script: &'a Script, syntax: &'a LslSyntax) -> Self {
        let mut analyzer = Self {
            syntax,
            functions: HashMap::new(),
            globals: HashMap::new(),
            states: HashSet::new(),
            scopes: Vec::new(),
            labels: HashSet::new(),
            seen_labels: HashSet::new(),
            current_ret: None,
            diagnostics: Vec::new(),
        };
        analyzer.collect_symbols(script);
        analyzer
    }

    /// Populate [`Self::functions`], [`Self::globals`] and [`Self::states`],
    /// reporting a duplicate the moment a name recurs within its own namespace.
    fn collect_symbols(&mut self, script: &'a Script) {
        for item in &script.globals {
            match item {
                GlobalItem::Function(func) => {
                    if self.functions.contains_key(func.name.name.as_str()) {
                        self.report(
                            DiagnosticKind::DuplicateFunction {
                                name: func.name.name.clone(),
                            },
                            func.name.span.clone(),
                        );
                    } else {
                        let _prev = self.functions.insert(func.name.name.as_str(), func);
                    }
                }
                GlobalItem::Variable(var) => {
                    if self.globals.contains_key(var.name.name.as_str()) {
                        self.report(
                            DiagnosticKind::DuplicateGlobal {
                                name: var.name.name.clone(),
                            },
                            var.name.span.clone(),
                        );
                    } else {
                        let _prev = self.globals.insert(var.name.name.as_str(), var.ty.kind);
                    }
                }
            }
        }
        for state in &script.states {
            let (name, span) = state_name(&state.name);
            if self.states.contains(name) {
                self.report(
                    DiagnosticKind::DuplicateState {
                        name: name.to_owned(),
                    },
                    span,
                );
            } else {
                let _present = self.states.insert(name);
            }
        }
    }

    /// Walk the whole script: global initialisers, function bodies, state event
    /// handlers, then the cross-state reachability check.
    fn run(&mut self, script: &'a Script) {
        for item in &script.globals {
            match item {
                GlobalItem::Variable(var) => {
                    if let Some(init) = &var.init {
                        // A global initialiser sees only globals and constants
                        // (no locals); an empty scope stack models that.
                        self.analyze_expr(init);
                    }
                }
                GlobalItem::Function(func) => self.analyze_function(func),
            }
        }
        for state in &script.states {
            self.analyze_state(state);
        }
        self.check_state_reachability(script);
    }

    /// Walk one user function: its parameter scope, its body (with the return
    /// context set), and the fall-off-the-end check for a value function.
    fn analyze_function(&mut self, func: &'a FunctionDef) {
        self.current_ret = func.ret.as_ref().map(|ty| ty.kind);
        self.labels = collect_labels(&func.body);
        self.seen_labels.clear();
        let params = self.param_scope(&func.params);
        self.scopes = vec![params];
        self.analyze_block(&func.body);
        self.scopes.clear();
        if let Some(ret) = self.current_ret
            && !block_diverges(&func.body)
        {
            self.report(
                DiagnosticKind::MissingReturn {
                    function: func.name.name.clone(),
                    expected: ret,
                },
                func.name.span.clone(),
            );
        }
        self.current_ret = None;
    }

    /// Walk one state's event handlers: checking each handler's name and
    /// signature against the grid's event table, and its body as a `void`
    /// context (an event yields no value).
    fn analyze_state(&mut self, state: &'a StateDef) {
        let mut seen_events: HashSet<&str> = HashSet::new();
        for handler in &state.events {
            let name = handler.name.name.as_str();
            if !seen_events.insert(name) {
                self.report(
                    DiagnosticKind::DuplicateEvent {
                        name: name.to_owned(),
                    },
                    handler.name.span.clone(),
                );
            }
            self.check_event_signature(handler);

            self.current_ret = None;
            self.labels = collect_labels(&handler.body);
            self.seen_labels.clear();
            let params = self.param_scope(&handler.params);
            self.scopes = vec![params];
            self.analyze_block(&handler.body);
            self.scopes.clear();
        }
    }

    /// Check an event handler's name and declared parameters against the grid's
    /// event table. Gated on a non-empty event table (an unknown-event report
    /// needs a table that lists the events); the signature check runs only when
    /// the grid actually knows the event.
    fn check_event_signature(&mut self, handler: &'a crate::ast::EventHandler) {
        let name = handler.name.name.as_str();
        let Some(event) = self.syntax.event(name) else {
            if !self.syntax.events.is_empty() {
                self.report(
                    DiagnosticKind::UnknownEvent {
                        name: name.to_owned(),
                    },
                    handler.name.span.clone(),
                );
            }
            return;
        };
        if handler.params.len() != event.arguments.len() {
            self.report(
                DiagnosticKind::WrongEventArgCount {
                    event: name.to_owned(),
                    expected: event.arguments.len(),
                    found: handler.params.len(),
                },
                handler.name.span.clone(),
            );
            return;
        }
        for (index, (param, arg)) in handler.params.iter().zip(&event.arguments).enumerate() {
            if let Some(expected) = arg.arg_type
                && param.ty.kind != expected
            {
                self.report(
                    DiagnosticKind::EventArgTypeMismatch {
                        event: name.to_owned(),
                        index,
                        expected,
                        found: param.ty.kind,
                    },
                    param.ty.span.clone(),
                );
            }
        }
    }

    /// Build a fresh scope frame from a parameter list, reporting a duplicate
    /// parameter name within the one list.
    fn param_scope(&mut self, params: &[crate::ast::Param]) -> HashMap<String, TypeName> {
        let mut frame = HashMap::new();
        for param in params {
            if frame
                .insert(param.name.name.clone(), param.ty.kind)
                .is_some()
            {
                self.report(
                    DiagnosticKind::DuplicateParam {
                        name: param.name.name.clone(),
                    },
                    param.name.span.clone(),
                );
            }
        }
        frame
    }

    /// Walk a braced block in its own lexical scope.
    fn analyze_block(&mut self, block: &Block) {
        self.scopes.push(HashMap::new());
        for stmt in &block.statements {
            self.analyze_stmt(stmt);
        }
        let _frame = self.scopes.pop();
    }

    /// Walk one statement, resolving references, checking `return`, `jump` and
    /// `state` targets, and recording locals into the current scope.
    fn analyze_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Empty(_) | Stmt::Error(_) => {}
            Stmt::Label { name, .. } => {
                if !self.seen_labels.insert(name.name.clone()) {
                    self.report(
                        DiagnosticKind::DuplicateLabel {
                            name: name.name.clone(),
                        },
                        name.span.clone(),
                    );
                }
            }
            Stmt::Local { ty, name, init, .. } => {
                // Insert the name before analysing the initialiser so a
                // self-reference (`integer x = x;`) resolves rather than being
                // flagged; a genuinely undefined name on the right still is.
                if let Some(frame) = self.scopes.last()
                    && frame.contains_key(name.name.as_str())
                {
                    self.report(
                        DiagnosticKind::DuplicateLocal {
                            name: name.name.clone(),
                        },
                        name.span.clone(),
                    );
                }
                if let Some(frame) = self.scopes.last_mut() {
                    let _prev = frame.insert(name.name.clone(), ty.kind);
                }
                if let Some(init) = init {
                    self.analyze_expr(init);
                }
            }
            Stmt::Expr { expr, .. } => self.analyze_expr(expr),
            Stmt::Block(block) => self.analyze_block(block),
            Stmt::If {
                cond,
                then_branch,
                else_branch,
                ..
            } => {
                self.analyze_expr(cond);
                self.analyze_stmt(then_branch);
                if let Some(else_branch) = else_branch {
                    self.analyze_stmt(else_branch);
                }
            }
            Stmt::While { cond, body, .. } => {
                self.analyze_expr(cond);
                self.analyze_stmt(body);
            }
            Stmt::DoWhile { body, cond, .. } => {
                self.analyze_stmt(body);
                self.analyze_expr(cond);
            }
            Stmt::For {
                init,
                cond,
                incr,
                body,
                ..
            } => {
                for expr in init {
                    self.analyze_expr(expr);
                }
                if let Some(cond) = cond {
                    self.analyze_expr(cond);
                }
                for expr in incr {
                    self.analyze_expr(expr);
                }
                self.analyze_stmt(body);
            }
            Stmt::Return { value, span } => self.analyze_return(value.as_ref(), span.clone()),
            Stmt::Jump { label, .. } => {
                if !self.labels.contains(label.name.as_str()) {
                    self.report(
                        DiagnosticKind::UndefinedLabel {
                            name: label.name.clone(),
                        },
                        label.span.clone(),
                    );
                }
            }
            Stmt::StateChange { target, .. } => {
                if let StateName::Named(id) = target
                    && !self.states.contains(id.name.as_str())
                {
                    self.report(
                        DiagnosticKind::UndefinedState {
                            name: id.name.clone(),
                        },
                        id.span.clone(),
                    );
                }
            }
        }
    }

    /// Check a `return` against the current return context: a value where none
    /// is wanted, a bare `return;` where a value is required, or a returned
    /// expression whose type is incompatible with the declared return type.
    fn analyze_return(&mut self, value: Option<&Expr>, span: Range<usize>) {
        match (self.current_ret, value) {
            (None, Some(expr)) => {
                self.analyze_expr(expr);
                self.report(DiagnosticKind::ReturnValueInVoid, span);
            }
            (Some(expected), None) => {
                self.report(DiagnosticKind::MissingReturnValue { expected }, span);
            }
            (Some(expected), Some(expr)) => {
                self.analyze_expr(expr);
                if let Some(found) = self.expr_type(expr)
                    && !compatible(expected, found)
                {
                    self.report(
                        DiagnosticKind::ReturnTypeMismatch { expected, found },
                        expr.span(),
                    );
                }
            }
            (None, None) => {}
        }
    }

    /// Walk an expression, resolving every referenced name and checking each
    /// call site's arity and argument types.
    fn analyze_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Integer { .. } | Expr::Float { .. } | Expr::Str { .. } | Expr::Error(_) => {}
            Expr::Variable(id) => self.resolve_variable(&id.name, id.span.clone()),
            Expr::Member { base, .. } => self.resolve_variable(&base.name, base.span.clone()),
            Expr::Call { callee, args, span } => {
                for arg in args {
                    self.analyze_expr(arg);
                }
                self.check_call(callee, args, span.clone());
            }
            Expr::List { elements, .. } => {
                for element in elements {
                    self.analyze_expr(element);
                }
            }
            Expr::Vector { x, y, z, .. } => {
                self.analyze_expr(x);
                self.analyze_expr(y);
                self.analyze_expr(z);
            }
            Expr::Rotation { x, y, z, s, .. } => {
                self.analyze_expr(x);
                self.analyze_expr(y);
                self.analyze_expr(z);
                self.analyze_expr(s);
            }
            Expr::Prefix { operand, .. } | Expr::Postfix { operand, .. } => {
                self.analyze_expr(operand);
            }
            Expr::Binary { lhs, rhs, .. } => {
                self.analyze_expr(lhs);
                self.analyze_expr(rhs);
            }
            Expr::Assign { target, value, .. } => {
                self.analyze_expr(value);
                self.analyze_expr(target);
                if let Expr::Variable(id) = target.as_ref()
                    && self.is_only_constant(&id.name)
                {
                    self.report(
                        DiagnosticKind::AssignToConstant {
                            name: id.name.clone(),
                        },
                        id.span.clone(),
                    );
                }
            }
            Expr::Cast { operand, .. } => self.analyze_expr(operand),
            Expr::Paren { inner, .. } => self.analyze_expr(inner),
        }
    }

    /// Resolve a bare identifier used as a value. Flags it only when the library
    /// constant table is populated (so `PI`/`TRUE` are not mistaken for
    /// undefined) and the name is neither an in-scope variable, a global, a
    /// constant, nor a callable name (a bare function name is the grid's error
    /// to report, not ours).
    fn resolve_variable(&mut self, name: &str, span: Range<usize>) {
        if self.is_defined_var(name) || self.is_callable(name) {
            return;
        }
        if !self.syntax.constants.is_empty() {
            self.report(
                DiagnosticKind::UndefinedVariable {
                    name: name.to_owned(),
                },
                span,
            );
        }
    }

    /// Check a call site: resolve the callee to a user or library signature and
    /// check arity and argument types against it, or flag it undefined (gated
    /// on a non-empty function table).
    fn check_call(&mut self, callee: &crate::ast::Ident, args: &[Expr], span: Range<usize>) {
        let name = callee.name.as_str();
        if let Some(func) = self.functions.get(name) {
            let expected: Vec<Option<TypeName>> = func
                .params
                .iter()
                .map(|param| Some(param.ty.kind))
                .collect();
            self.check_args(name, &expected, args, span);
        } else if let Some(func) = self.syntax.function(name) {
            let expected: Vec<Option<TypeName>> =
                func.arguments.iter().map(|arg| arg.arg_type).collect();
            self.check_args(name, &expected, args, span);
        } else if !self.syntax.functions.is_empty() {
            self.report(
                DiagnosticKind::UndefinedFunction {
                    name: name.to_owned(),
                },
                callee.span.clone(),
            );
        }
    }

    /// Check `args` against a resolved parameter-type list: arity first, then —
    /// only when the arity matches — each argument's inferred type against its
    /// parameter, skipping any argument or parameter whose type is unknown.
    fn check_args(
        &mut self,
        callee: &str,
        expected: &[Option<TypeName>],
        args: &[Expr],
        span: Range<usize>,
    ) {
        if args.len() != expected.len() {
            self.report(
                DiagnosticKind::WrongArgCount {
                    callee: callee.to_owned(),
                    expected: expected.len(),
                    found: args.len(),
                },
                span,
            );
            return;
        }
        for (index, (arg, expected_ty)) in args.iter().zip(expected).enumerate() {
            let (Some(expected_ty), Some(found)) = (*expected_ty, self.expr_type(arg)) else {
                continue;
            };
            if !compatible(expected_ty, found) {
                self.report(
                    DiagnosticKind::ArgTypeMismatch {
                        callee: callee.to_owned(),
                        index,
                        expected: expected_ty,
                        found,
                    },
                    arg.span(),
                );
            }
        }
    }

    /// The type an expression evaluates to, or [`None`] when it cannot be pinned
    /// down confidently. Unknown is the safe answer: a caller skips the check
    /// rather than guessing, so this never manufactures a false type error.
    fn expr_type(&self, expr: &Expr) -> Option<TypeName> {
        match expr {
            Expr::Integer { .. } => Some(TypeName::Integer),
            // A float literal and a vector/rotation component (`v.x`, always a
            // float) share the same inferred type.
            Expr::Float { .. } | Expr::Member { .. } => Some(TypeName::Float),
            Expr::Str { .. } => Some(TypeName::String),
            Expr::List { .. } => Some(TypeName::List),
            Expr::Vector { .. } => Some(TypeName::Vector),
            Expr::Rotation { .. } => Some(TypeName::Rotation),
            Expr::Cast { ty, .. } => Some(ty.kind),
            Expr::Paren { inner, .. } => self.expr_type(inner),
            Expr::Variable(id) => self.var_type(&id.name),
            Expr::Call { callee, .. } => self.call_type(&callee.name),
            Expr::Assign { target, .. } => self.expr_type(target),
            // `!x` is always an integer; other prefixes and both postfixes keep
            // the operand's type (`-v` is a vector, `++i` an integer).
            Expr::Prefix { op, operand, .. } => match op {
                crate::ast::PrefixOp::Not => Some(TypeName::Integer),
                _ => self.expr_type(operand),
            },
            Expr::Postfix { operand, .. } => self.expr_type(operand),
            // Only the operators whose result type is fixed regardless of
            // operand types are inferred; arithmetic (`+ - * / %`) is
            // operand-polymorphic in LSL (`%` is vector cross product too), so
            // it stays unknown rather than risk a wrong guess.
            Expr::Binary { op, .. } => binary_result_type(*op),
            Expr::Error(_) => None,
        }
    }

    /// The return type of a call to `name` (user function or library function),
    /// or [`None`] if the function is `void` or unknown.
    fn call_type(&self, name: &str) -> Option<TypeName> {
        if let Some(func) = self.functions.get(name) {
            func.ret.as_ref().map(|ty| ty.kind)
        } else {
            self.syntax.function(name).and_then(|func| func.return_type)
        }
    }

    /// The declared type of a variable name: an in-scope local/parameter first
    /// (innermost frame wins), then a global, then a library constant.
    fn var_type(&self, name: &str) -> Option<TypeName> {
        for frame in self.scopes.iter().rev() {
            if let Some(ty) = frame.get(name) {
                return Some(*ty);
            }
        }
        if let Some(ty) = self.globals.get(name) {
            return Some(*ty);
        }
        self.syntax.constant(name).and_then(|c| c.constant_type)
    }

    /// Whether a name resolves to a variable: an in-scope local/parameter, a
    /// global, or a library constant.
    fn is_defined_var(&self, name: &str) -> bool {
        self.scopes.iter().any(|frame| frame.contains_key(name))
            || self.globals.contains_key(name)
            || self.syntax.constant(name).is_some()
    }

    /// Whether a name resolves to a callable: a user function or a library
    /// function.
    fn is_callable(&self, name: &str) -> bool {
        self.functions.contains_key(name) || self.syntax.function(name).is_some()
    }

    /// Whether a name resolves *only* to a library constant — a read-only
    /// symbol that cannot be an assignment target (a local or global of the
    /// same name would shadow it and make the assignment legal).
    fn is_only_constant(&self, name: &str) -> bool {
        let shadowed = self.scopes.iter().any(|frame| frame.contains_key(name))
            || self.globals.contains_key(name);
        !shadowed && self.syntax.constant(name).is_some()
    }

    /// After every state is collected, warn on any state no reachable
    /// `state name;` can transition to (BFS from `default`), and error if a
    /// script with states lacks a `default`.
    fn check_state_reachability(&mut self, script: &'a Script) {
        if script.states.is_empty() {
            return;
        }
        if !self.states.contains("default") {
            self.report(
                DiagnosticKind::MissingDefaultState,
                script.span.start..script.span.start,
            );
            return;
        }
        // Adjacency: each state to the set of states it can switch to.
        let mut edges: HashMap<String, HashSet<String>> = HashMap::new();
        for state in &script.states {
            let (name, _span) = state_name(&state.name);
            let mut targets = HashSet::new();
            for handler in &state.events {
                collect_state_targets(&handler.body, &mut targets);
            }
            let _prev = edges.insert(name.to_owned(), targets);
        }
        let mut reachable: HashSet<String> = HashSet::new();
        let mut worklist: Vec<String> = vec!["default".to_owned()];
        while let Some(state) = worklist.pop() {
            if !reachable.insert(state.clone()) {
                continue;
            }
            if let Some(targets) = edges.get(&state) {
                for target in targets {
                    // Only follow edges to states that actually exist; a
                    // `state name;` to an undefined state is reported
                    // separately and contributes no reachability.
                    if self.states.contains(target.as_str()) {
                        worklist.push(target.clone());
                    }
                }
            }
        }
        for state in &script.states {
            let (name, span) = state_name(&state.name);
            if !reachable.contains(name) {
                self.report(
                    DiagnosticKind::UnreachableState {
                        name: name.to_owned(),
                    },
                    span,
                );
            }
        }
    }

    /// Record a finding, deriving its severity and message from the kind.
    fn report(&mut self, kind: DiagnosticKind, span: Range<usize>) {
        self.diagnostics.push(Diagnostic {
            severity: kind.severity(),
            message: kind.message(),
            kind,
            span,
        });
    }
}

/// The name and span of a state header (`default` or a user state).
fn state_name(name: &StateName) -> (&str, Range<usize>) {
    match name {
        StateName::Default(span) => ("default", span.clone()),
        StateName::Named(id) => (id.name.as_str(), id.span.clone()),
    }
}

/// The result type of a binary operator whose result type is *fixed*
/// independent of its operands (comparisons, logical and bitwise/shift ops,
/// which all yield `integer`), or [`None`] for the operand-polymorphic
/// arithmetic operators.
const fn binary_result_type(op: crate::ast::BinaryOp) -> Option<TypeName> {
    use crate::ast::BinaryOp;
    match op {
        BinaryOp::Eq
        | BinaryOp::Ne
        | BinaryOp::Lt
        | BinaryOp::Le
        | BinaryOp::Gt
        | BinaryOp::Ge
        | BinaryOp::And
        | BinaryOp::Or
        | BinaryOp::Shl
        | BinaryOp::Shr
        | BinaryOp::BitAnd
        | BinaryOp::BitOr
        | BinaryOp::BitXor => Some(TypeName::Integer),
        BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod => None,
    }
}

/// Whether a value of type `found` may fill a parameter (or return slot) of type
/// `expected`, honouring LSL's two implicit conversions: `integer`→`float`
/// (widening) and `string`↔`key` (freely interchangeable). Every other pairing
/// needs an explicit cast, so this is deliberately narrow.
fn compatible(expected: TypeName, found: TypeName) -> bool {
    if expected == found {
        return true;
    }
    matches!(
        (expected, found),
        (TypeName::Float, TypeName::Integer)
            | (TypeName::Key, TypeName::String)
            | (TypeName::String, TypeName::Key)
    )
}

/// Collect every jump-label name defined anywhere in a body (labels are
/// function/event-scoped, so a `jump` may target one in a sibling or enclosing
/// block).
fn collect_labels(block: &Block) -> HashSet<String> {
    let mut labels = HashSet::new();
    collect_labels_into(block, &mut labels);
    labels
}

/// Recurse into `block`'s statements, inserting each `@label` name into `out`.
fn collect_labels_into(block: &Block, out: &mut HashSet<String>) {
    for stmt in &block.statements {
        collect_labels_stmt(stmt, out);
    }
}

/// Insert the labels defined by one statement (and its nested statements).
fn collect_labels_stmt(stmt: &Stmt, out: &mut HashSet<String>) {
    match stmt {
        Stmt::Label { name, .. } => {
            let _present = out.insert(name.name.clone());
        }
        Stmt::Block(block) => collect_labels_into(block, out),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            collect_labels_stmt(then_branch, out);
            if let Some(else_branch) = else_branch {
                collect_labels_stmt(else_branch, out);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::For { body, .. } => {
            collect_labels_stmt(body, out);
        }
        Stmt::Empty(_)
        | Stmt::Error(_)
        | Stmt::Local { .. }
        | Stmt::Expr { .. }
        | Stmt::Return { .. }
        | Stmt::Jump { .. }
        | Stmt::StateChange { .. } => {}
    }
}

/// Collect the target state names of every `state name;` reachable in a body
/// (used to build the state-reachability graph).
fn collect_state_targets(block: &Block, out: &mut HashSet<String>) {
    for stmt in &block.statements {
        collect_state_targets_stmt(stmt, out);
    }
}

/// Insert the state-change targets of one statement (and its nested statements).
fn collect_state_targets_stmt(stmt: &Stmt, out: &mut HashSet<String>) {
    match stmt {
        Stmt::StateChange { target, .. } => {
            let (name, _span) = state_name(target);
            let _present = out.insert(name.to_owned());
        }
        Stmt::Block(block) => collect_state_targets(block, out),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            collect_state_targets_stmt(then_branch, out);
            if let Some(else_branch) = else_branch {
                collect_state_targets_stmt(else_branch, out);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::For { body, .. } => {
            collect_state_targets_stmt(body, out);
        }
        Stmt::Empty(_)
        | Stmt::Error(_)
        | Stmt::Local { .. }
        | Stmt::Expr { .. }
        | Stmt::Return { .. }
        | Stmt::Jump { .. }
        | Stmt::Label { .. } => {}
    }
}

/// Whether a block *always* diverges before its end — every path leaves through
/// a `return`, a `state` change, an unconditional `jump`, or an infinite loop.
/// The first statement that always diverges makes the rest of the block
/// unreachable, so the block diverges. Used only for the missing-return warning,
/// where a *false* "diverges" merely suppresses the warning (conservative).
fn block_diverges(block: &Block) -> bool {
    block.statements.iter().any(stmt_diverges)
}

/// Whether one statement always diverges (never falls through to the statement
/// after it). Conservative on the side of "diverges" — a `jump` or `state`
/// change is treated as diverging so the missing-return warning never fires on
/// code that in fact never falls off the end.
fn stmt_diverges(stmt: &Stmt) -> bool {
    match stmt {
        // A return leaves the function; a state change and an unconditional
        // jump divert control away and are treated as diverging so we never
        // warn on a function that ends in one.
        Stmt::Return { .. } | Stmt::StateChange { .. } | Stmt::Jump { .. } => true,
        Stmt::Block(block) => block_diverges(block),
        // An `if` diverges only when it has an `else` and *both* arms diverge.
        Stmt::If {
            then_branch,
            else_branch: Some(else_branch),
            ..
        } => stmt_diverges(then_branch) && stmt_diverges(else_branch),
        // A loop with a constant-true condition and no way to fall out (LSL has
        // no `break`) never reaches its end. `do` also diverges if its body
        // does (the body always runs at least once).
        Stmt::While { cond, .. } => cond_is_true(cond),
        Stmt::DoWhile { body, cond, .. } => stmt_diverges(body) || cond_is_true(cond),
        Stmt::For { cond, .. } => cond.as_ref().is_none_or(cond_is_true),
        // An `if` without an `else`, and every non-control-flow statement, can
        // fall through to the next statement.
        Stmt::If {
            else_branch: None, ..
        }
        | Stmt::Empty(_)
        | Stmt::Error(_)
        | Stmt::Local { .. }
        | Stmt::Expr { .. }
        | Stmt::Label { .. } => false,
    }
}

/// Whether a loop condition is a compile-time constant truth — a non-zero
/// integer literal or the universal `TRUE` constant (always `1`), through any
/// parentheses. Anything else is treated as possibly-false so a loop over it can
/// fall through.
fn cond_is_true(cond: &Expr) -> bool {
    match cond {
        Expr::Integer { raw, .. } => parse_int_literal(raw).is_some_and(|value| value != 0),
        Expr::Variable(id) => id.name == "TRUE",
        Expr::Paren { inner, .. } => cond_is_true(inner),
        _ => false,
    }
}

/// Parse an LSL integer literal's raw text (decimal or `0x`/`0X` hexadecimal)
/// into a value, or [`None`] if it does not parse.
fn parse_int_literal(raw: &str) -> Option<i64> {
    if let Some(hex) = raw.strip_prefix("0x").or_else(|| raw.strip_prefix("0X")) {
        i64::from_str_radix(hex, 16).ok()
    } else {
        raw.parse().ok()
    }
}
