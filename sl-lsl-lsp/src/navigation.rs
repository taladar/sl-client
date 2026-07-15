//! **Scope-aware symbol resolution** over a parsed LSL script — the one pass
//! that backs go-to-definition, find-references, rename and document highlight,
//! and feeds hover and the deprecated/god-mode lints.
//!
//! Every navigation request reduces to the same question: *which binding does
//! the identifier under the cursor refer to, and where else is that binding
//! mentioned?* Answering it needs LSL's scope rules, not plain text matching —
//! a local `x` shadows a global `x`, two functions may each have their own
//! parameter `n`, and a `jump` resolves to a label in its own function only.
//! [`resolve`] walks the tree exactly the way the semantic pass does (globals
//! and states are file-scoped and order-insensitive; parameters and locals are
//! block-scoped with the innermost frame winning; labels are function-scoped)
//! and emits one [`Occurrence`] per identifier mention, each already resolved to
//! the [`Binding`] it names.
//!
//! With that occurrence list the four navigation requests are trivial and
//! *consistent* — they all read the same resolution, so a rename touches exactly
//! the spans find-references reports and go-to-definition lands on:
//!
//! - **definition** — the [`Binding::User`] declaration span of the occurrence
//!   at the cursor;
//! - **references** — every occurrence sharing that binding
//!   ([`references_of`]);
//! - **rename** — the same set, refused for a [`Binding::Library`] symbol the
//!   editor cannot rewrite;
//! - **highlight** — the references restricted to the one document.
//!
//! A [`Binding::Library`] occurrence (an `ll*` call, a `PI`, a `touch_start`
//! handler) has no editable definition but still groups by name, so
//! find-references over `llSay` or `state_entry` works even though rename does
//! not. An identifier that resolves to nothing (an undefined name, which the
//! semantic pass reports as an error) contributes no occurrence: there is
//! nothing to navigate to.

use core::ops::Range;
use std::collections::HashMap;

use sl_lsl::LslSyntax;
use sl_lsl::ast::{
    Block, EventHandler, Expr, FunctionDef, GlobalItem, Param, Script, StateDef, StateName, Stmt,
    TypeName,
};

/// The namespace a resolved identifier lives in — what *kind* of thing the name
/// refers to, which chooses the LSP symbol icon and gates a few behaviours (only
/// a value-shaped binding is a rename candidate the same way, an event name is
/// never renameable).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolClass {
    /// A variable, parameter or global — a value binding.
    Variable,
    /// A function — a user function or a library `ll*`/`os*` call.
    Function,
    /// A state (`default` or a named `state`).
    State,
    /// A jump label (`@label` / `jump label`).
    Label,
    /// An event handler name (`state_entry`, `touch_start`, …).
    Event,
    /// A library constant (`TRUE`, `PI`, `AGENT`, …).
    Constant,
}

/// What an [`Occurrence`] resolves to: a user declaration in this document (with
/// an editable definition span) or a grid library symbol (no local definition,
/// not renameable).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Binding {
    /// A user-defined symbol, identified by the byte span of its declaring
    /// *name* — unique per declaration and shared by every occurrence that
    /// resolves to it, so it is the binding's identity for grouping references.
    User {
        /// The byte span of the declaration's name.
        decl: Range<usize>,
    },
    /// A grid library symbol (function, constant, event). Grouped by name for
    /// find-references; refused for rename.
    Library,
}

/// One resolved mention of an identifier in the source: its byte span, its name,
/// what namespace it is in, the binding it resolves to, and — on a *declaration*
/// occurrence only — a one-line detail string (a variable's type, a function's
/// signature) hover reuses so it need not re-walk the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Occurrence {
    /// The byte span of the identifier token this occurrence covers.
    pub span: Range<usize>,
    /// The identifier text.
    pub name: String,
    /// The namespace the name resolves in.
    pub class: SymbolClass,
    /// The binding the name resolves to.
    pub binding: Binding,
    /// A one-line detail (type or signature) for a user declaration occurrence;
    /// [`None`] on a use-site and on library occurrences.
    pub detail: Option<String>,
}

impl Occurrence {
    /// Whether this occurrence is the *declaration* of its binding — a
    /// [`Binding::User`] whose declaration span is this occurrence's own span.
    #[must_use]
    pub fn is_declaration(&self) -> bool {
        matches!(&self.binding, Binding::User { decl } if *decl == self.span)
    }
}

/// Resolve every identifier mention in `script` against the grid library
/// `syntax`, returning the occurrences in source order. Total and
/// side-effect-free — the single pass all navigation requests read.
#[must_use]
pub fn resolve(script: &Script, syntax: &LslSyntax) -> Vec<Occurrence> {
    let mut resolver = Resolver::new(script, syntax);
    resolver.run(script);
    resolver.occurrences
}

/// The occurrence whose span contains `offset`, if any — the identifier under
/// the cursor. Spans do not overlap, so at most one matches; a cursor just past
/// an identifier's last byte (`offset == span.end`) counts as on it, the usual
/// editor convention for a click at a word's trailing edge.
#[must_use]
pub fn occurrence_at(occurrences: &[Occurrence], offset: usize) -> Option<&Occurrence> {
    occurrences
        .iter()
        .find(|occ| occ.span.start <= offset && offset <= occ.span.end)
}

/// The declaration occurrence of `target`'s binding — the one whose span is the
/// binding's declaration — or [`None`] for a [`Binding::Library`] symbol (which
/// has no editable definition in the document). Used by go-to-definition and to
/// recover a use-site's hover detail.
#[must_use]
pub fn declaration_of<'a>(
    occurrences: &'a [Occurrence],
    target: &Occurrence,
) -> Option<&'a Occurrence> {
    match &target.binding {
        Binding::User { decl } => occurrences
            .iter()
            .find(|occ| occ.is_declaration() && occ.span == *decl),
        Binding::Library => None,
    }
}

/// Every occurrence that shares `target`'s binding: for a [`Binding::User`] the
/// occurrences with the same declaration span; for a [`Binding::Library`] the
/// occurrences of the same name and class (a library name is unique within its
/// namespace, so name-plus-class identifies it).
#[must_use]
pub fn references_of<'a>(
    occurrences: &'a [Occurrence],
    target: &Occurrence,
) -> Vec<&'a Occurrence> {
    occurrences
        .iter()
        .filter(|occ| match (&occ.binding, &target.binding) {
            (Binding::User { decl: a }, Binding::User { decl: b }) => a == b,
            (Binding::Library, Binding::Library) => {
                occ.name == target.name && occ.class == target.class
            }
            _ => false,
        })
        .collect()
}

/// The mutable state of the resolution walk: the file-scoped symbol tables, the
/// current lexical scope stack and label set, the accumulating occurrences, and
/// the grid library the calls and constants resolve against.
struct Resolver<'a> {
    /// The grid library, for classifying library functions, constants and
    /// events.
    syntax: &'a LslSyntax,
    /// User functions by name → the byte span of the declaring name.
    functions: HashMap<&'a str, Range<usize>>,
    /// Global variables by name → declaring-name span.
    globals: HashMap<&'a str, Range<usize>>,
    /// States by name (including `default`) → declaring-name span.
    states: HashMap<String, Range<usize>>,
    /// The lexical scope stack (innermost last): variable name → declaring-name
    /// span of the local/parameter it binds.
    scopes: Vec<HashMap<String, Range<usize>>>,
    /// The labels of the function/handler currently walked: name → declaring
    /// span.
    labels: HashMap<String, Range<usize>>,
    /// The occurrences collected so far, in source order.
    occurrences: Vec<Occurrence>,
}

impl<'a> Resolver<'a> {
    /// Build a resolver, collecting the file-scoped tables (functions, globals,
    /// states) and emitting their declaration occurrences up front so a later
    /// reference resolves order-insensitively.
    fn new(script: &'a Script, syntax: &'a LslSyntax) -> Self {
        let mut resolver = Self {
            syntax,
            functions: HashMap::new(),
            globals: HashMap::new(),
            states: HashMap::new(),
            scopes: Vec::new(),
            labels: HashMap::new(),
            occurrences: Vec::new(),
        };
        resolver.collect_symbols(script);
        resolver
    }

    /// Record the file-scoped declarations (globals, functions, states) into the
    /// tables and emit a declaration occurrence for each. The first declaration
    /// of a name owns the binding; a duplicate keeps the first's span (the
    /// semantic pass flags the duplication separately).
    fn collect_symbols(&mut self, script: &'a Script) {
        for item in &script.globals {
            match item {
                GlobalItem::Function(func) => {
                    let span = func.name.span.clone();
                    let _first = self
                        .functions
                        .entry(func.name.name.as_str())
                        .or_insert_with(|| span.clone());
                    self.declare(
                        &func.name.name,
                        span.clone(),
                        SymbolClass::Function,
                        span,
                        Some(function_signature(func)),
                    );
                }
                GlobalItem::Variable(var) => {
                    let span = var.name.span.clone();
                    let _first = self
                        .globals
                        .entry(var.name.name.as_str())
                        .or_insert_with(|| span.clone());
                    self.declare(
                        &var.name.name,
                        span.clone(),
                        SymbolClass::Variable,
                        span,
                        Some(variable_detail(var.ty.kind, &var.name.name)),
                    );
                }
            }
        }
        for state in &script.states {
            let (name, span) = state_name(&state.name);
            let _first = self
                .states
                .entry(name.to_owned())
                .or_insert_with(|| span.clone());
            self.declare(name, span.clone(), SymbolClass::State, span, None);
        }
    }

    /// Walk the whole script: global initialisers (globals-only scope), function
    /// bodies, then state event handlers.
    fn run(&mut self, script: &Script) {
        for item in &script.globals {
            if let GlobalItem::Variable(var) = item
                && let Some(init) = &var.init
            {
                self.walk_expr(init);
            }
        }
        for item in &script.globals {
            if let GlobalItem::Function(func) = item {
                self.walk_function(func);
            }
        }
        for state in &script.states {
            self.walk_state(state);
        }
    }

    /// Walk one user function: its parameter scope, its labels, then its body.
    fn walk_function(&mut self, func: &FunctionDef) {
        self.labels = collect_labels(&func.body);
        self.scopes = vec![self.param_scope(&func.params)];
        self.walk_block(&func.body);
        self.scopes.clear();
        self.labels.clear();
    }

    /// Walk one state's event handlers, each in its own parameter scope and
    /// label set. The handler name is a library event mention (never a user
    /// definition), so an unknown handler name simply resolves to nothing.
    fn walk_state(&mut self, state: &StateDef) {
        for handler in &state.events {
            self.walk_event(handler);
        }
    }

    /// Walk one event handler: emit its name as a library event occurrence when
    /// the grid knows the event, then its parameter scope, labels and body.
    fn walk_event(&mut self, handler: &EventHandler) {
        if self.syntax.event(handler.name.name.as_str()).is_some() {
            self.push(
                handler.name.span.clone(),
                &handler.name.name,
                SymbolClass::Event,
                Binding::Library,
                None,
            );
        }
        self.labels = collect_labels(&handler.body);
        self.scopes = vec![self.param_scope(&handler.params)];
        self.walk_block(&handler.body);
        self.scopes.clear();
        self.labels.clear();
    }

    /// Build a parameter scope frame, emitting a declaration occurrence for each
    /// parameter (the first of a duplicated name owns the binding).
    fn param_scope(&mut self, params: &[Param]) -> HashMap<String, Range<usize>> {
        let mut frame = HashMap::new();
        for param in params {
            if !frame.contains_key(param.name.name.as_str()) {
                let _absent = frame.insert(param.name.name.clone(), param.name.span.clone());
            }
            self.declare(
                &param.name.name,
                param.name.span.clone(),
                SymbolClass::Variable,
                param.name.span.clone(),
                Some(variable_detail(param.ty.kind, &param.name.name)),
            );
        }
        frame
    }

    /// Walk a braced block in its own lexical scope frame.
    fn walk_block(&mut self, block: &Block) {
        self.scopes.push(HashMap::new());
        for stmt in &block.statements {
            self.walk_stmt(stmt);
        }
        let _frame = self.scopes.pop();
    }

    /// Walk one statement, emitting occurrences for the names it mentions and
    /// recording a local declaration into the current scope.
    fn walk_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Empty(_) | Stmt::Error(_) => {}
            Stmt::Local { ty, name, init, .. } => {
                // Bind the name before walking the initialiser so `integer x = x`
                // records the second `x` as a self-reference to the new local.
                if let Some(frame) = self.scopes.last_mut()
                    && !frame.contains_key(name.name.as_str())
                {
                    let _absent = frame.insert(name.name.clone(), name.span.clone());
                }
                self.declare(
                    &name.name,
                    name.span.clone(),
                    SymbolClass::Variable,
                    name.span.clone(),
                    Some(variable_detail(ty.kind, &name.name)),
                );
                if let Some(init) = init {
                    self.walk_expr(init);
                }
            }
            Stmt::Expr { expr, .. } => self.walk_expr(expr),
            Stmt::Block(block) => self.walk_block(block),
            Stmt::If {
                cond,
                then_branch,
                else_branch,
                ..
            } => {
                self.walk_expr(cond);
                self.walk_stmt(then_branch);
                if let Some(else_branch) = else_branch {
                    self.walk_stmt(else_branch);
                }
            }
            Stmt::While { cond, body, .. } => {
                self.walk_expr(cond);
                self.walk_stmt(body);
            }
            Stmt::DoWhile { body, cond, .. } => {
                self.walk_stmt(body);
                self.walk_expr(cond);
            }
            Stmt::For {
                init,
                cond,
                incr,
                body,
                ..
            } => {
                for expr in init {
                    self.walk_expr(expr);
                }
                if let Some(cond) = cond {
                    self.walk_expr(cond);
                }
                for expr in incr {
                    self.walk_expr(expr);
                }
                self.walk_stmt(body);
            }
            Stmt::Return { value, .. } => {
                if let Some(value) = value {
                    self.walk_expr(value);
                }
            }
            Stmt::Jump { label, .. } => {
                if let Some(decl) = self.labels.get(label.name.as_str()).cloned() {
                    self.push(
                        label.span.clone(),
                        &label.name,
                        SymbolClass::Label,
                        Binding::User { decl },
                        None,
                    );
                }
            }
            Stmt::Label { name, .. } => {
                self.declare(
                    &name.name,
                    name.span.clone(),
                    SymbolClass::Label,
                    name.span.clone(),
                    None,
                );
            }
            Stmt::StateChange { target, .. } => {
                let (name, span) = state_name(target);
                if let Some(decl) = self.states.get(name).cloned() {
                    self.push(span, name, SymbolClass::State, Binding::User { decl }, None);
                }
            }
        }
    }

    /// Walk one expression, emitting an occurrence for each variable, member
    /// base and call it mentions.
    fn walk_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Integer { .. } | Expr::Float { .. } | Expr::Str { .. } | Expr::Error(_) => {}
            Expr::Variable(id) => self.resolve_variable(&id.name, id.span.clone()),
            Expr::Member { base, .. } => self.resolve_variable(&base.name, base.span.clone()),
            Expr::Call { callee, args, .. } => {
                self.resolve_call(&callee.name, callee.span.clone());
                for arg in args {
                    self.walk_expr(arg);
                }
            }
            Expr::List { elements, .. } => {
                for element in elements {
                    self.walk_expr(element);
                }
            }
            Expr::Vector { x, y, z, .. } => {
                self.walk_expr(x);
                self.walk_expr(y);
                self.walk_expr(z);
            }
            Expr::Rotation { x, y, z, s, .. } => {
                self.walk_expr(x);
                self.walk_expr(y);
                self.walk_expr(z);
                self.walk_expr(s);
            }
            Expr::Prefix { operand, .. }
            | Expr::Postfix { operand, .. }
            | Expr::Cast { operand, .. } => self.walk_expr(operand),
            Expr::Binary { lhs, rhs, .. } => {
                self.walk_expr(lhs);
                self.walk_expr(rhs);
            }
            Expr::Assign { target, value, .. } => {
                self.walk_expr(target);
                self.walk_expr(value);
            }
            Expr::Paren { inner, .. } => self.walk_expr(inner),
        }
    }

    /// Resolve a bare identifier used as a value: an in-scope local/parameter
    /// (innermost wins), then a global, then a library constant. An unresolved
    /// name emits nothing.
    fn resolve_variable(&mut self, name: &str, span: Range<usize>) {
        if let Some(decl) = self.lookup_var(name) {
            self.push(
                span,
                name,
                SymbolClass::Variable,
                Binding::User { decl },
                None,
            );
        } else if self.syntax.constant(name).is_some() {
            self.push(span, name, SymbolClass::Constant, Binding::Library, None);
        }
    }

    /// Resolve a call target: a user function first, then a library function. An
    /// unresolved callee emits nothing.
    fn resolve_call(&mut self, name: &str, span: Range<usize>) {
        if let Some(decl) = self.functions.get(name).cloned() {
            self.push(
                span,
                name,
                SymbolClass::Function,
                Binding::User { decl },
                None,
            );
        } else if self.syntax.function(name).is_some() {
            self.push(span, name, SymbolClass::Function, Binding::Library, None);
        }
    }

    /// The declaring-name span a variable name binds to: an in-scope
    /// local/parameter (searching innermost-out), else a global.
    fn lookup_var(&self, name: &str) -> Option<Range<usize>> {
        for frame in self.scopes.iter().rev() {
            if let Some(span) = frame.get(name) {
                return Some(span.clone());
            }
        }
        self.globals.get(name).cloned()
    }

    /// Emit a declaration occurrence (its binding is `User { decl }` pointing at
    /// its own name span), carrying the one-line `detail` for hover.
    fn declare(
        &mut self,
        name: &str,
        span: Range<usize>,
        class: SymbolClass,
        decl: Range<usize>,
        detail: Option<String>,
    ) {
        self.push(span, name, class, Binding::User { decl }, detail);
    }

    /// Push one occurrence.
    fn push(
        &mut self,
        span: Range<usize>,
        name: &str,
        class: SymbolClass,
        binding: Binding,
        detail: Option<String>,
    ) {
        self.occurrences.push(Occurrence {
            span,
            name: name.to_owned(),
            class,
            binding,
            detail,
        });
    }
}

/// The one-line detail for a variable/parameter/local: `type name`.
#[must_use]
fn variable_detail(ty: TypeName, name: &str) -> String {
    format!("{} {name}", ty.keyword())
}

/// The one-line signature detail for a user function: `[ret ]name(type p, …)`.
#[must_use]
fn function_signature(func: &FunctionDef) -> String {
    let mut signature = String::new();
    if let Some(ret) = &func.ret {
        signature.push_str(ret.kind.keyword());
        signature.push(' ');
    }
    signature.push_str(&func.name.name);
    signature.push('(');
    let joined = func
        .params
        .iter()
        .map(|param| format!("{} {}", param.ty.kind.keyword(), param.name.name))
        .collect::<Vec<_>>()
        .join(", ");
    signature.push_str(&joined);
    signature.push(')');
    signature
}

/// The name and declaring-name span of a state header.
#[must_use]
fn state_name(name: &StateName) -> (&str, Range<usize>) {
    match name {
        StateName::Default(span) => ("default", span.clone()),
        StateName::Named(id) => (id.name.as_str(), id.span.clone()),
    }
}

/// Collect every jump-label name in a body → its declaring span (labels are
/// function/handler-scoped, so a forward `jump` resolves too). The first
/// declaration of a repeated name wins.
#[must_use]
fn collect_labels(block: &Block) -> HashMap<String, Range<usize>> {
    let mut labels = HashMap::new();
    collect_labels_into(block, &mut labels);
    labels
}

/// Recurse into `block`, inserting each `@label`'s declaring span (keeping the
/// first of a repeated name).
fn collect_labels_into(block: &Block, out: &mut HashMap<String, Range<usize>>) {
    for stmt in &block.statements {
        collect_labels_stmt(stmt, out);
    }
}

/// Insert the labels one statement (and its nested bodies) declares.
fn collect_labels_stmt(stmt: &Stmt, out: &mut HashMap<String, Range<usize>>) {
    match stmt {
        Stmt::Label { name, .. } => {
            if !out.contains_key(name.name.as_str()) {
                let _absent = out.insert(name.name.clone(), name.span.clone());
            }
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

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{Binding, Occurrence, SymbolClass, occurrence_at, references_of, resolve};
    use sl_lsl::ast::TypeName;
    use sl_lsl::{LslConstant, LslEvent, LslFunction, LslSyntax, parse};

    /// A small library table: `llSay`, the constant `PI`, and the `state_entry`
    /// / `touch_start` events, enough to resolve library mentions in the tests.
    fn library() -> LslSyntax {
        let mut syntax = LslSyntax::default();
        let _prev = syntax
            .functions
            .insert("llSay".to_owned(), LslFunction::default());
        let _prev = syntax.constants.insert(
            "PI".to_owned(),
            LslConstant {
                constant_type: Some(TypeName::Float),
                value: Some("3.14159".to_owned()),
                ..LslConstant::default()
            },
        );
        let _prev = syntax
            .events
            .insert("state_entry".to_owned(), LslEvent::default());
        let _prev = syntax
            .events
            .insert("touch_start".to_owned(), LslEvent::default());
        syntax
    }

    /// Resolve `source` against the test library and return the occurrence at
    /// `offset`, surfacing a miss as an `Err` (the workspace denies `unwrap`).
    fn occurrence_at_offset(
        occurrences: &[Occurrence],
        offset: usize,
    ) -> Result<&Occurrence, String> {
        occurrence_at(occurrences, offset).ok_or_else(|| format!("no occurrence at byte {offset}"))
    }

    /// A local shadows a global of the same name: a use inside the function
    /// resolves to the local, and find-references does not cross into the global.
    #[test]
    fn local_shadows_global() -> Result<(), String> {
        let source = "integer x;\ninteger f()\n{\ninteger x = 1;\nreturn x;\n}\n";
        let occ = resolve(&parse(source).script, &library());
        // The `x` in `return x;` is the last occurrence.
        let use_offset = source.rfind('x').ok_or("no `x`")?;
        let target = occurrence_at_offset(&occ, use_offset)?;
        assert_eq!(target.class, SymbolClass::Variable);
        // Its binding is the local `x` (declared on line 4), not the global.
        let refs = references_of(&occ, target);
        // Two occurrences: the local declaration and the `return x` use.
        assert_eq!(refs.len(), 2);
        let body_start = source.find("integer x = 1").ok_or("no local decl")?;
        for reference in &refs {
            // Every reference is inside the function body, never the global.
            assert!(reference.span.start >= body_start);
        }
        Ok(())
    }

    /// A call to a user function resolves to the function's declaration and
    /// find-references includes the declaration and every call site.
    #[test]
    fn user_function_references() -> Result<(), String> {
        let source = "f() {}\ndefault { state_entry() { f(); f(); } }\n";
        let occ = resolve(&parse(source).script, &library());
        let decl_offset = source.find('f').ok_or("no `f`")?;
        let decl = occurrence_at_offset(&occ, decl_offset)?;
        assert!(decl.is_declaration());
        // The declaration plus two calls.
        assert_eq!(references_of(&occ, decl).len(), 3);
        Ok(())
    }

    /// A library function and constant resolve to [`Binding::Library`]; a library
    /// symbol groups its references by name.
    #[test]
    fn library_symbols_resolve() -> Result<(), String> {
        let source = "default { state_entry() { llSay(0, (string)PI); llSay(1, \"\"); } }\n";
        let occ = resolve(&parse(source).script, &library());
        let say_offset = source.find("llSay").ok_or("no `llSay`")?;
        let say = occurrence_at_offset(&occ, say_offset)?;
        assert_eq!(say.binding, Binding::Library);
        assert_eq!(say.class, SymbolClass::Function);
        // Two `llSay` calls group together.
        assert_eq!(references_of(&occ, say).len(), 2);
        // The `PI` constant resolves to a library constant.
        let pi_offset = source.find("PI").ok_or("no `PI`")?;
        let pi = occurrence_at_offset(&occ, pi_offset)?;
        assert_eq!(pi.class, SymbolClass::Constant);
        assert_eq!(pi.binding, Binding::Library);
        Ok(())
    }

    /// An event handler name resolves to a library event, and two states each
    /// handling `touch_start` group under find-references.
    #[test]
    fn event_handler_references() -> Result<(), String> {
        let source = "default { touch_start(integer n) {} }\n\
                      state other { touch_start(integer n) {} }\n";
        let occ = resolve(&parse(source).script, &library());
        let first = source.find("touch_start").ok_or("no `touch_start`")?;
        let target = occurrence_at_offset(&occ, first)?;
        assert_eq!(target.class, SymbolClass::Event);
        assert_eq!(references_of(&occ, target).len(), 2);
        Ok(())
    }

    /// A `state` change resolves to the named state's declaration.
    #[test]
    fn state_change_navigates() -> Result<(), String> {
        let source = "default { touch_start(integer n) { state other; } }\n\
                      state other { state_entry() {} }\n";
        let occ = resolve(&parse(source).script, &library());
        // The `other` in `state other;` — the change, not the declaration.
        let change_offset = source.find("state other;").ok_or("no state change")? + "state ".len();
        let target = occurrence_at_offset(&occ, change_offset)?;
        assert_eq!(target.class, SymbolClass::State);
        match &target.binding {
            Binding::User { decl } => {
                let decl_offset =
                    source.rfind("state other").ok_or("no state decl")? + "state ".len();
                assert!(decl.start <= decl_offset && decl_offset <= decl.end);
            }
            Binding::Library => return Err("state should be user-defined".to_owned()),
        }
        Ok(())
    }

    /// A `jump` resolves to its label's declaration within the same function.
    #[test]
    fn jump_resolves_to_label() -> Result<(), String> {
        let source = "f() { @top; jump top; }\n";
        let occ = resolve(&parse(source).script, &library());
        let jump_offset = source.find("jump top").ok_or("no jump")? + "jump ".len();
        let target = occurrence_at_offset(&occ, jump_offset)?;
        assert_eq!(target.class, SymbolClass::Label);
        // The label declaration plus the jump reference.
        assert_eq!(references_of(&occ, target).len(), 2);
        Ok(())
    }

    /// A declaration occurrence carries a one-line detail hover reuses.
    #[test]
    fn declaration_detail_present() -> Result<(), String> {
        let source = "integer counter;\ninteger add(integer a) { return a; }\n";
        let occ = resolve(&parse(source).script, &library());
        let counter = occurrence_at_offset(&occ, source.find("counter").ok_or("no counter")?)?;
        assert_eq!(counter.detail.as_deref(), Some("integer counter"));
        let add = occurrence_at_offset(&occ, source.find("add").ok_or("no add")?)?;
        assert_eq!(add.detail.as_deref(), Some("integer add(integer a)"));
        Ok(())
    }
}
