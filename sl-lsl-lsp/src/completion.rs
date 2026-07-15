//! Answering `textDocument/completion` — the identifier suggestions offered as
//! the user types.
//!
//! Completion is **context-aware**: what is offered depends on *where* the
//! cursor is, because LSL's three regions accept different constructs.
//!
//! - **Inside a function or event body** — the useful case — offers the
//!   in-scope variables (parameters and the locals declared before the cursor),
//!   the file's globals and user functions, and the whole grid library
//!   (`ll*`/`os*` functions, constants) plus the control and type keywords. The
//!   locals are scope-filtered so a sibling block's variable is not suggested.
//! - **Inside a `state` block but outside any handler** — offers the grid's
//!   **event names**, the only thing that may be declared there, so
//!   `touch_start`/`state_entry` complete in exactly the right place.
//! - **At the top level** — offers the type keywords and the `default` / `state`
//!   headers a global declaration or a state block begins with.
//!
//! Unlike diagnostics, an over-broad completion is a nuisance, not a bug, so the
//! filtering aims for *helpful* rather than *provably minimal*; the grid-served
//! symbols are the part no wiki-scraped competitor can offer for the connected
//! grid.

use lsp_types::{CompletionItem, CompletionItemKind, Documentation, Position};

use sl_lsl::ast::{Block, FunctionDef, GlobalItem, Param, Script, Stmt, TypeName};

use crate::docs;
use crate::document::Document;
use crate::position::PositionEncoding;

/// The completion items for `position` in `document`, against the grid library
/// `syntax`, chosen by the cursor's context and sorted by label.
#[must_use]
pub fn completion(
    document: &Document,
    position: Position,
    syntax: &sl_lsl::LslSyntax,
    encoding: PositionEncoding,
) -> Vec<CompletionItem> {
    let offset = document.offset(position, encoding);
    let script = &document.parse().script;
    let mut items = match context(script, offset) {
        Context::Body(scope) => body_items(script, syntax, &scope),
        Context::EventDeclaration => event_items(syntax),
        Context::Global => global_items(),
    };
    items.sort_by(|a, b| a.label.cmp(&b.label));
    items
}

/// The region the cursor is in, which decides what to offer.
enum Context {
    /// Inside a function or event body, carrying the variables visible there.
    Body(Vec<ScopeVar>),
    /// Inside a `state` block but outside every handler body.
    EventDeclaration,
    /// At the top level (a global declaration or a state header).
    Global,
}

/// A variable visible at the cursor: its name and its `type name` detail.
struct ScopeVar {
    /// The variable name.
    name: String,
    /// The `type name` detail shown after it.
    detail: String,
}

/// Classify the cursor's context: an enclosing function/event body first (most
/// specific), then an enclosing state block, else the top level.
#[must_use]
fn context(script: &Script, offset: usize) -> Context {
    for item in &script.globals {
        if let GlobalItem::Function(func) = item
            && contains(&func.body.span, offset)
        {
            return Context::Body(visible_vars(&func.params, &func.body, offset));
        }
    }
    for state in &script.states {
        for handler in &state.events {
            if contains(&handler.body.span, offset) {
                return Context::Body(visible_vars(&handler.params, &handler.body, offset));
            }
        }
        if contains(&state.span, offset) {
            // Inside the state block but not inside a handler body: an event
            // handler is the only thing declarable here.
            return Context::EventDeclaration;
        }
    }
    Context::Global
}

/// Whether `span` contains `offset`.
#[must_use]
const fn contains(span: &core::ops::Range<usize>, offset: usize) -> bool {
    span.start <= offset && offset <= span.end
}

/// The variables visible at `offset` inside a function/event body: every
/// parameter, plus the locals whose declaration begins before the cursor.
#[must_use]
fn visible_vars(params: &[Param], body: &Block, offset: usize) -> Vec<ScopeVar> {
    let mut vars = Vec::new();
    for param in params {
        vars.push(ScopeVar {
            name: param.name.name.clone(),
            detail: format!("{} {}", param.ty.kind.keyword(), param.name.name),
        });
    }
    collect_locals(body, offset, &mut vars);
    vars
}

/// Collect the locals declared before `offset` in a block (recursing into nested
/// blocks and control-flow bodies).
fn collect_locals(block: &Block, offset: usize, out: &mut Vec<ScopeVar>) {
    for stmt in &block.statements {
        collect_locals_stmt(stmt, offset, out);
    }
}

/// Collect the locals one statement declares before `offset`.
fn collect_locals_stmt(stmt: &Stmt, offset: usize, out: &mut Vec<ScopeVar>) {
    match stmt {
        Stmt::Local { ty, name, span, .. } if span.start < offset => out.push(ScopeVar {
            name: name.name.clone(),
            detail: format!("{} {}", ty.kind.keyword(), name.name),
        }),
        Stmt::Block(block) => collect_locals(block, offset, out),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            collect_locals_stmt(then_branch, offset, out);
            if let Some(else_branch) = else_branch {
                collect_locals_stmt(else_branch, offset, out);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::For { body, .. } => {
            collect_locals_stmt(body, offset, out);
        }
        Stmt::Local { .. }
        | Stmt::Empty(_)
        | Stmt::Error(_)
        | Stmt::Expr { .. }
        | Stmt::Return { .. }
        | Stmt::Jump { .. }
        | Stmt::Label { .. }
        | Stmt::StateChange { .. } => {}
    }
}

/// The completions inside a body: in-scope variables, the file's globals and
/// user functions, the whole grid library, and the control/type keywords.
#[must_use]
fn body_items(
    script: &Script,
    syntax: &sl_lsl::LslSyntax,
    scope: &[ScopeVar],
) -> Vec<CompletionItem> {
    let mut items = Vec::new();
    for var in scope {
        items.push(item(
            &var.name,
            CompletionItemKind::VARIABLE,
            Some(var.detail.clone()),
            None,
        ));
    }
    for global in &script.globals {
        match global {
            GlobalItem::Variable(var) => items.push(item(
                &var.name.name,
                CompletionItemKind::VARIABLE,
                Some(format!("{} {}", var.ty.kind.keyword(), var.name.name)),
                None,
            )),
            GlobalItem::Function(func) => items.push(item(
                &func.name.name,
                CompletionItemKind::FUNCTION,
                Some(user_function_detail(func)),
                None,
            )),
        }
    }
    for (name, func) in &syntax.functions {
        items.push(item(
            name,
            CompletionItemKind::FUNCTION,
            Some(docs::function_label(name, func)),
            func.tooltip.as_deref(),
        ));
    }
    for (name, constant) in &syntax.constants {
        items.push(item(
            name,
            CompletionItemKind::CONSTANT,
            Some(docs::constant_label(name, constant)),
            constant.tooltip.as_deref(),
        ));
    }
    for name in syntax.controls.keys() {
        items.push(item(name, CompletionItemKind::KEYWORD, None, None));
    }
    for name in syntax.types.keys() {
        items.push(item(name, CompletionItemKind::KEYWORD, None, None));
    }
    items
}

/// The completions inside a state block: the grid's event names.
#[must_use]
fn event_items(syntax: &sl_lsl::LslSyntax) -> Vec<CompletionItem> {
    syntax
        .events
        .iter()
        .map(|(name, event)| {
            item(
                name,
                CompletionItemKind::EVENT,
                Some(docs::event_label(name, event)),
                event.tooltip.as_deref(),
            )
        })
        .collect()
}

/// The completions at the top level: the type keywords and the `default` /
/// `state` block headers.
#[must_use]
fn global_items() -> Vec<CompletionItem> {
    let mut items = vec![
        item("default", CompletionItemKind::KEYWORD, None, None),
        item("state", CompletionItemKind::KEYWORD, None, None),
    ];
    for ty in [
        TypeName::Integer,
        TypeName::Float,
        TypeName::String,
        TypeName::Key,
        TypeName::Vector,
        TypeName::Rotation,
        TypeName::List,
    ] {
        items.push(item(ty.keyword(), CompletionItemKind::KEYWORD, None, None));
    }
    items
}

/// The one-line detail for a user function completion: `[ret ]name(type p, …)`.
#[must_use]
fn user_function_detail(func: &FunctionDef) -> String {
    let mut detail = String::new();
    if let Some(ret) = &func.ret {
        detail.push_str(ret.kind.keyword());
        detail.push(' ');
    }
    detail.push_str(&func.name.name);
    detail.push('(');
    let joined = func
        .params
        .iter()
        .map(|param| format!("{} {}", param.ty.kind.keyword(), param.name.name))
        .collect::<Vec<_>>()
        .join(", ");
    detail.push_str(&joined);
    detail.push(')');
    detail
}

/// Build a completion item with a label, kind, optional detail and optional
/// plain-text documentation.
#[must_use]
fn item(
    label: &str,
    kind: CompletionItemKind,
    detail: Option<String>,
    documentation: Option<&str>,
) -> CompletionItem {
    CompletionItem {
        label: label.to_owned(),
        kind: Some(kind),
        detail,
        documentation: documentation.map(|doc| Documentation::String(doc.to_owned())),
        ..CompletionItem::default()
    }
}

#[cfg(test)]
mod tests {
    use core::str::FromStr as _;

    use pretty_assertions::assert_eq;

    use super::completion;
    use crate::document::Document;
    use crate::position::PositionEncoding;
    use lsp_types::{CompletionItemKind, Position, Uri};
    use sl_lsl::ast::TypeName;
    use sl_lsl::{LslConstant, LslEvent, LslFunction, LslSyntax};

    /// A library with `llSay`, the constant `PI`, and the `touch_start` event.
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
            .insert("touch_start".to_owned(), LslEvent::default());
        syntax
    }

    /// Open `source`, surfacing a URI-parse failure as an `Err`.
    fn open(source: &str) -> Result<Document, String> {
        let uri = Uri::from_str("file:///test.lsl").map_err(|err| err.to_string())?;
        Ok(Document::open(uri, 1, source.to_owned()))
    }

    /// The completion labels at a `(line, column)` cursor.
    fn labels_at(source: &str, line: u32, column: u32) -> Result<Vec<String>, String> {
        let doc = open(source)?;
        let items = completion(
            &doc,
            Position {
                line,
                character: column,
            },
            &library(),
            PositionEncoding::Utf16,
        );
        Ok(items.into_iter().map(|item| item.label).collect())
    }

    /// Inside a body, completion offers in-scope locals, globals, user
    /// functions, library symbols and keywords — and not a sibling scope's
    /// local.
    #[test]
    fn body_offers_scope_and_library() -> Result<(), String> {
        // Line 3 is inside `state_entry`, after `integer local;`.
        let source = "integer global;\n\
            f() {}\n\
            default {\n\
            state_entry() { integer local; local = 1; }\n\
            }\n";
        // Put the cursor on line 3, just after `local = ` — column past the local decl.
        let line = 3;
        let line_text = source.lines().nth(3).ok_or("no line 3")?;
        let col = u32::try_from(line_text.find("local = 1").ok_or("no assign")? + 8)
            .map_err(|err| err.to_string())?;
        let labels = labels_at(source, line, col)?;
        assert!(labels.contains(&"global".to_owned()), "{labels:?}");
        assert!(labels.contains(&"f".to_owned()), "{labels:?}");
        assert!(labels.contains(&"local".to_owned()), "{labels:?}");
        assert!(labels.contains(&"llSay".to_owned()), "{labels:?}");
        assert!(labels.contains(&"PI".to_owned()), "{labels:?}");
        Ok(())
    }

    /// Inside a state block but outside a handler, completion offers event names.
    #[test]
    fn state_block_offers_events() -> Result<(), String> {
        // Cursor between the `{` and the first handler.
        let source = "default {\n\n}\n";
        let labels = labels_at(source, 1, 0)?;
        assert_eq!(labels, vec!["touch_start".to_owned()]);
        Ok(())
    }

    /// At the top level, completion offers the type keywords and block headers.
    #[test]
    fn top_level_offers_types() -> Result<(), String> {
        let source = "\n\ndefault { touch_start(integer n) {} }\n";
        let labels = labels_at(source, 0, 0)?;
        assert!(labels.contains(&"integer".to_owned()), "{labels:?}");
        assert!(labels.contains(&"default".to_owned()), "{labels:?}");
        // No library symbols at the top level.
        assert!(!labels.contains(&"llSay".to_owned()), "{labels:?}");
        Ok(())
    }

    /// A completion item carries its kind so the editor shows the right icon.
    #[test]
    fn items_have_kinds() -> Result<(), String> {
        let source = "default {\n\n}\n";
        let doc = open(source)?;
        let items = completion(
            &doc,
            Position {
                line: 1,
                character: 0,
            },
            &library(),
            PositionEncoding::Utf16,
        );
        let touch = items
            .iter()
            .find(|item| item.label == "touch_start")
            .ok_or("no touch_start")?;
        assert_eq!(touch.kind, Some(CompletionItemKind::EVENT));
        Ok(())
    }
}
