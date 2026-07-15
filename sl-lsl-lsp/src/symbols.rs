//! Turning a parsed LSL script into LSP **symbols** — the outline a client shows
//! in its breadcrumb bar, its `documentSymbol` tree view and its
//! `workspace/symbol` fuzzy picker.
//!
//! Every symbol here comes from the **user's own parse tree**
//! ([`sl_lsl::ast::Script`]): the global variables and functions, the states,
//! and — nested under each function or event handler — its parameters and local
//! variables. The grid's library symbols (the `ll*` functions and the `PI` /
//! `TRUE` constants from the `LSLSyntax` capability) are deliberately *not* here:
//! a `documentSymbol` describes constructs that *live in the document*, and a
//! `workspace/symbol` needs a source [`Location`] the library symbols do not
//! have. The library table earns its keep in the language-intelligence half
//! (the `viewer-lsl-lsp-diagnostics-nav` task) — completion, hover and signature help —
//! not in the outline.
//!
//! Two shapes are produced:
//!
//! - [`document_symbols`] — the **hierarchical** [`DocumentSymbol`] tree for one
//!   buffer, nesting parameters and locals under their function/handler and
//!   handlers under their state. This is what a breadcrumb and a collapsible
//!   outline read.
//! - [`workspace_symbols`] — the **flat**, query-filtered top-level symbols of
//!   one buffer as [`SymbolInformation`] with a [`Location`], for the
//!   workspace-wide "go to symbol" picker (the server concatenates these across
//!   every open document).
//!
//! LSL has no classes, so a **state** maps to [`SymbolKind::CLASS`] (the closest
//! "named group of handlers" the protocol offers) and an **event handler** to
//! [`SymbolKind::EVENT`]; functions are [`SymbolKind::FUNCTION`] and every
//! variable-shaped thing (global, parameter, local) is [`SymbolKind::VARIABLE`].

use lsp_types::{DocumentSymbol, Location, SymbolInformation, SymbolKind};

use sl_lsl::ast::{
    Block, EventHandler, FunctionDef, GlobalItem, Param, StateDef, StateName, Stmt, TypeRef,
};

use crate::document::Document;
use crate::position::PositionEncoding;

/// Build the hierarchical [`DocumentSymbol`] outline for one document: globals
/// then states, in source order, each with its parameters and locals nested
/// beneath it.
#[expect(
    clippy::module_name_repetitions,
    reason = "`document_symbols` names the LSP `textDocument/documentSymbol` request it answers; \
              the protocol term is what a caller searches for"
)]
#[must_use]
pub fn document_symbols(doc: &Document, encoding: PositionEncoding) -> Vec<DocumentSymbol> {
    let script = &doc.parse().script;
    let mut symbols = Vec::new();
    for item in &script.globals {
        symbols.push(global_symbol(doc, item, encoding));
    }
    for state in &script.states {
        symbols.push(state_symbol(doc, state, encoding));
    }
    symbols
}

/// The outline entry for a top-level global — a variable (a leaf) or a function
/// (with its parameters and locals as children).
#[must_use]
fn global_symbol(doc: &Document, item: &GlobalItem, encoding: PositionEncoding) -> DocumentSymbol {
    match item {
        GlobalItem::Variable(var) => make_symbol(
            var.name.name.clone(),
            Some(type_detail(&var.ty)),
            SymbolKind::VARIABLE,
            doc.range(var.span.clone(), encoding),
            doc.range(var.name.span.clone(), encoding),
            None,
        ),
        GlobalItem::Function(func) => make_symbol(
            func.name.name.clone(),
            Some(function_signature(func)),
            SymbolKind::FUNCTION,
            doc.range(func.span.clone(), encoding),
            doc.range(func.name.span.clone(), encoding),
            Some(body_members(doc, &func.params, &func.body, encoding)),
        ),
    }
}

/// The outline entry for a state: a [`SymbolKind::CLASS`] whose children are its
/// event handlers.
#[must_use]
fn state_symbol(doc: &Document, state: &StateDef, encoding: PositionEncoding) -> DocumentSymbol {
    let (name, name_span) = state_name(&state.name);
    let children = state
        .events
        .iter()
        .map(|handler| event_symbol(doc, handler, encoding))
        .collect();
    make_symbol(
        name.to_owned(),
        None,
        SymbolKind::CLASS,
        doc.range(state.span.clone(), encoding),
        doc.range(name_span, encoding),
        Some(children),
    )
}

/// The outline entry for an event handler: a [`SymbolKind::EVENT`] with its
/// parameters and locals as children.
#[must_use]
fn event_symbol(
    doc: &Document,
    handler: &EventHandler,
    encoding: PositionEncoding,
) -> DocumentSymbol {
    make_symbol(
        handler.name.name.clone(),
        Some(param_signature(&handler.name.name, &handler.params)),
        SymbolKind::EVENT,
        doc.range(handler.span.clone(), encoding),
        doc.range(handler.name.span.clone(), encoding),
        Some(body_members(doc, &handler.params, &handler.body, encoding)),
    )
}

/// The child symbols of a function or event handler: its parameters (in order)
/// followed by the local variables declared anywhere in its body (in source
/// order), each a [`SymbolKind::VARIABLE`] leaf.
#[must_use]
fn body_members(
    doc: &Document,
    params: &[Param],
    body: &Block,
    encoding: PositionEncoding,
) -> Vec<DocumentSymbol> {
    let mut members = Vec::new();
    for param in params {
        members.push(make_symbol(
            param.name.name.clone(),
            Some(type_detail(&param.ty)),
            SymbolKind::VARIABLE,
            doc.range(param.span.clone(), encoding),
            doc.range(param.name.span.clone(), encoding),
            None,
        ));
    }
    let mut locals = Vec::new();
    collect_locals(body, &mut locals);
    for local in locals {
        members.push(make_symbol(
            local.name.clone(),
            Some(local.detail.clone()),
            SymbolKind::VARIABLE,
            doc.range(local.range.clone(), encoding),
            doc.range(local.name_span.clone(), encoding),
            None,
        ));
    }
    members
}

/// One local variable found while walking a body: its name, its type-keyword
/// detail, and the byte spans of the whole declaration and of the name.
struct LocalVar {
    /// The local's declared name.
    name: String,
    /// The type-keyword detail string (e.g. `"integer"`).
    detail: String,
    /// The byte span of the whole `type name [= init]` declaration.
    range: core::ops::Range<usize>,
    /// The byte span of just the name, for the selection range.
    name_span: core::ops::Range<usize>,
}

/// Collect every local declaration in `block` (recursing into nested blocks and
/// the bodies of control-flow statements) into `out`, in source order. Labels,
/// jumps and expressions declare nothing and are skipped.
fn collect_locals(block: &Block, out: &mut Vec<LocalVar>) {
    for stmt in &block.statements {
        collect_locals_stmt(stmt, out);
    }
}

/// Collect the locals one statement contributes (itself if it is a declaration,
/// plus any in its nested bodies).
fn collect_locals_stmt(stmt: &Stmt, out: &mut Vec<LocalVar>) {
    match stmt {
        Stmt::Local { ty, name, span, .. } => out.push(LocalVar {
            name: name.name.clone(),
            detail: type_detail(ty),
            range: span.clone(),
            name_span: name.span.clone(),
        }),
        Stmt::Block(block) => collect_locals(block, out),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            collect_locals_stmt(then_branch, out);
            if let Some(else_branch) = else_branch {
                collect_locals_stmt(else_branch, out);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::For { body, .. } => {
            collect_locals_stmt(body, out);
        }
        Stmt::Empty(_)
        | Stmt::Error(_)
        | Stmt::Expr { .. }
        | Stmt::Return { .. }
        | Stmt::Jump { .. }
        | Stmt::Label { .. }
        | Stmt::StateChange { .. } => {}
    }
}

/// Build the flat, query-filtered top-level symbols of one document for
/// `workspace/symbol`: its globals, functions, states and event handlers as
/// [`SymbolInformation`], each located in `doc`. A non-empty `query` keeps only
/// symbols whose name contains it (ASCII-case-insensitive); an empty query keeps
/// them all.
#[expect(
    clippy::module_name_repetitions,
    reason = "`workspace_symbols` names the LSP `workspace/symbol` request it answers; the \
              protocol term is what a caller searches for"
)]
#[must_use]
pub fn workspace_symbols(
    doc: &Document,
    query: &str,
    encoding: PositionEncoding,
) -> Vec<SymbolInformation> {
    let script = &doc.parse().script;
    let mut symbols = Vec::new();
    for item in &script.globals {
        match item {
            GlobalItem::Variable(var) => push_workspace_symbol(
                &mut symbols,
                doc,
                query,
                &var.name.name,
                SymbolKind::VARIABLE,
                var.name.span.clone(),
                None,
                encoding,
            ),
            GlobalItem::Function(func) => push_workspace_symbol(
                &mut symbols,
                doc,
                query,
                &func.name.name,
                SymbolKind::FUNCTION,
                func.name.span.clone(),
                None,
                encoding,
            ),
        }
    }
    for state in &script.states {
        let (state_ident, name_span) = state_name(&state.name);
        push_workspace_symbol(
            &mut symbols,
            doc,
            query,
            state_ident,
            SymbolKind::CLASS,
            name_span,
            None,
            encoding,
        );
        for handler in &state.events {
            push_workspace_symbol(
                &mut symbols,
                doc,
                query,
                &handler.name.name,
                SymbolKind::EVENT,
                handler.name.span.clone(),
                Some(state_ident.to_owned()),
                encoding,
            );
        }
    }
    symbols
}

/// Push one workspace symbol if its name matches `query`, resolving its byte
/// span to a [`Location`] in `doc`.
#[expect(
    clippy::too_many_arguments,
    reason = "a workspace symbol genuinely needs its name, kind, span, container and the \
              document/encoding to resolve a location; grouping them into a struct would only \
              move the argument list to the call sites"
)]
fn push_workspace_symbol(
    out: &mut Vec<SymbolInformation>,
    doc: &Document,
    query: &str,
    name: &str,
    kind: SymbolKind,
    span: core::ops::Range<usize>,
    container_name: Option<String>,
    encoding: PositionEncoding,
) {
    if !name_matches(name, query) {
        return;
    }
    out.push(make_information(
        name.to_owned(),
        kind,
        Location {
            uri: doc.uri().clone(),
            range: doc.range(span, encoding),
        },
        container_name,
    ));
}

/// Whether `name` matches a workspace-symbol `query`: an empty query matches
/// everything, otherwise a match is an ASCII-case-insensitive substring.
#[must_use]
fn name_matches(name: &str, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    name.to_ascii_lowercase()
        .contains(&query.to_ascii_lowercase())
}

/// The name and name-span of a state header (`default` or a user-named state).
#[must_use]
fn state_name(name: &StateName) -> (&str, core::ops::Range<usize>) {
    match name {
        StateName::Default(span) => ("default", span.clone()),
        StateName::Named(id) => (id.name.as_str(), id.span.clone()),
    }
}

/// The one-line detail string for a variable/parameter/local: just its type
/// keyword (e.g. `"vector"`), which an editor shows dimmed after the name.
#[must_use]
fn type_detail(ty: &TypeRef) -> String {
    ty.kind.keyword().to_owned()
}

/// The signature detail string for a function: `[ret ]name(type p, …)`.
#[must_use]
fn function_signature(func: &FunctionDef) -> String {
    let mut signature = String::new();
    if let Some(ret) = &func.ret {
        signature.push_str(ret.kind.keyword());
        signature.push(' ');
    }
    signature.push_str(&param_signature(&func.name.name, &func.params));
    signature
}

/// The `name(type p, …)` part shared by a function and an event-handler
/// signature.
#[must_use]
fn param_signature(name: &str, params: &[Param]) -> String {
    let joined = params
        .iter()
        .map(|param| format!("{} {}", param.ty.kind.keyword(), param.name.name))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{name}({joined})")
}

/// Construct a [`DocumentSymbol`], naming the `deprecated` field that lsp-types
/// still requires even though the protocol has replaced it with tags.
#[expect(
    deprecated,
    reason = "lsp-types marks DocumentSymbol::deprecated deprecated but the struct still requires \
              the field to be set; we always set it to None"
)]
#[must_use]
const fn make_symbol(
    name: String,
    detail: Option<String>,
    kind: SymbolKind,
    range: lsp_types::Range,
    selection_range: lsp_types::Range,
    children: Option<Vec<DocumentSymbol>>,
) -> DocumentSymbol {
    DocumentSymbol {
        name,
        detail,
        kind,
        tags: None,
        deprecated: None,
        range,
        selection_range,
        children,
    }
}

/// Construct a [`SymbolInformation`], naming the `deprecated` field lsp-types
/// still requires (see [`make_symbol`]).
#[expect(
    deprecated,
    reason = "lsp-types marks SymbolInformation::deprecated deprecated but the struct still \
              requires the field to be set; we always set it to None"
)]
#[must_use]
const fn make_information(
    name: String,
    kind: SymbolKind,
    location: Location,
    container_name: Option<String>,
) -> SymbolInformation {
    SymbolInformation {
        name,
        kind,
        tags: None,
        deprecated: None,
        location,
        container_name,
    }
}

#[cfg(test)]
mod tests {
    use core::str::FromStr as _;

    use pretty_assertions::assert_eq;

    use super::{document_symbols, workspace_symbols};
    use crate::document::Document;
    use crate::position::PositionEncoding;
    use lsp_types::{SymbolKind, Uri};

    /// A representative script exercising a global variable, a function with a
    /// parameter and a local, and a `default` state with an event handler.
    const SCRIPT: &str = "integer counter;\n\
        integer add(integer a)\n\
        {\n\
        integer sum = a;\n\
        return sum;\n\
        }\n\
        default\n\
        {\n\
        state_entry()\n\
        {\n\
        llSay(0, \"hi\");\n\
        }\n\
        }\n";

    /// Open `SCRIPT` as a document, surfacing a URI-parse failure as an `Err`.
    fn open() -> Result<Document, String> {
        let uri = Uri::from_str("file:///test.lsl").map_err(|err| err.to_string())?;
        Ok(Document::open(uri, 1, SCRIPT.to_owned()))
    }

    /// The document outline nests the function's parameter and local under it
    /// and the event handler under the `default` state, with the right kinds.
    #[test]
    fn document_outline_nests_members() -> Result<(), String> {
        let doc = open()?;
        let symbols = document_symbols(&doc, PositionEncoding::Utf16);
        // Three top-level symbols: the global, the function, the state.
        assert_eq!(symbols.len(), 3);

        let counter = symbols.first().ok_or("missing global")?;
        assert_eq!(counter.name, "counter");
        assert_eq!(counter.kind, SymbolKind::VARIABLE);
        assert_eq!(counter.detail.as_deref(), Some("integer"));

        let add = symbols.get(1).ok_or("missing function")?;
        assert_eq!(add.name, "add");
        assert_eq!(add.kind, SymbolKind::FUNCTION);
        assert_eq!(add.detail.as_deref(), Some("integer add(integer a)"));
        let add_children = add.children.as_ref().ok_or("function has no children")?;
        // The parameter `a` then the local `sum`.
        assert_eq!(add_children.len(), 2);
        assert_eq!(
            add_children.first().ok_or("missing param")?.name.as_str(),
            "a"
        );
        assert_eq!(
            add_children.get(1).ok_or("missing local")?.name.as_str(),
            "sum"
        );

        let default = symbols.get(2).ok_or("missing state")?;
        assert_eq!(default.name, "default");
        assert_eq!(default.kind, SymbolKind::CLASS);
        let events = default.children.as_ref().ok_or("state has no children")?;
        let entry = events.first().ok_or("missing event")?;
        assert_eq!(entry.name, "state_entry");
        assert_eq!(entry.kind, SymbolKind::EVENT);
        Ok(())
    }

    /// A selection range is contained within the enclosing range, as the
    /// protocol requires.
    #[test]
    fn selection_range_within_range() -> Result<(), String> {
        let doc = open()?;
        let symbols = document_symbols(&doc, PositionEncoding::Utf16);
        let add = symbols.get(1).ok_or("missing function")?;
        assert!(add.range.start <= add.selection_range.start);
        assert!(add.selection_range.end <= add.range.end);
        Ok(())
    }

    /// A workspace-symbol query filters case-insensitively across the document's
    /// top-level and event symbols; the event carries its state as container.
    #[test]
    fn workspace_query_filters() -> Result<(), String> {
        let doc = open()?;
        // Empty query returns everything: global, function, state, event.
        let all = workspace_symbols(&doc, "", PositionEncoding::Utf16);
        assert_eq!(all.len(), 4);

        // A substring query, case-insensitively, keeps only the matches.
        let hits = workspace_symbols(&doc, "ENTRY", PositionEncoding::Utf16);
        assert_eq!(hits.len(), 1);
        let entry = hits.first().ok_or("missing event")?;
        assert_eq!(entry.name, "state_entry");
        assert_eq!(entry.kind, SymbolKind::EVENT);
        assert_eq!(entry.container_name.as_deref(), Some("default"));
        Ok(())
    }
}
