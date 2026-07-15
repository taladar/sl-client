//! Answering `textDocument/inlayHint` — the small annotations rendered inline in
//! the editor, here surfacing the **runtime cost of a library call**.
//!
//! LSL performance is dominated by exactly two things the grid documents
//! per-function: the **forced sleep** a call imposes (`llSleep`, `llGiveMoney`,
//! the `ll*Email` family — each parks the script for a fixed time) and its
//! **energy** cost. Both are in the `LSLSyntax` document, so the client can show
//! them *inline* at each call site — a scripter sees that a loop of `llSleep`s
//! costs real seconds without opening the wiki. Only library calls the grid
//! actually annotates get a hint; a user function, or a library call with no
//! advertised cost (as on OpenSim, whose document omits both), gets none.
//!
//! The client asks for hints in a visible [`lsp_types::Range`]; this
//! module resolves that to a byte span and emits one hint, placed just after the
//! closing parenthesis, for each qualifying call within it.

use core::fmt::Write as _;

use lsp_types::{InlayHint, InlayHintLabel, Position, Range};

use sl_lsl::ast::{Block, Expr, GlobalItem, Script, Stmt};

use crate::document::Document;
use crate::position::PositionEncoding;

/// The inlay hints for the visible `range` of `document`, against the grid
/// library `syntax`: a cost annotation after each library call in range that the
/// grid gives an energy or sleep cost.
#[expect(
    clippy::module_name_repetitions,
    reason = "`inlay_hints` names the LSP `textDocument/inlayHint` request it answers; the \
              protocol term is what a caller searches for"
)]
#[must_use]
pub fn inlay_hints(
    document: &Document,
    range: Range,
    syntax: &sl_lsl::LslSyntax,
    encoding: PositionEncoding,
) -> Vec<InlayHint> {
    let start = document.offset(range.start, encoding);
    let end = document.offset(range.end, encoding);
    let script = &document.parse().script;
    let mut calls = Vec::new();
    collect_calls_script(script, &mut calls);

    let mut hints = Vec::new();
    for call in calls {
        if let Expr::Call { callee, span, .. } = call {
            // Place the hint by the call's end; skip calls outside the window.
            if span.end < start || span.end > end {
                continue;
            }
            let Some(func) = syntax.function(callee.name.as_str()) else {
                continue;
            };
            let Some(label) = cost_label(func.energy, func.sleep) else {
                continue;
            };
            hints.push(hint_at(document, span.end, label, encoding));
        }
    }
    hints
}

/// The cost label for a function's energy and sleep, or [`None`] when the grid
/// advertises neither (nothing to annotate).
#[must_use]
fn cost_label(energy: Option<f32>, sleep: Option<f32>) -> Option<String> {
    let mut label = String::new();
    if let Some(sleep) = sleep {
        let _ignored = write!(label, "sleep {sleep}s");
    }
    if let Some(energy) = energy {
        if !label.is_empty() {
            label.push_str(", ");
        }
        let _ignored = write!(label, "energy {energy}");
    }
    if label.is_empty() { None } else { Some(label) }
}

/// Build an inlay hint with the given label at the byte `offset`, padded on the
/// left so it does not abut the source token.
#[must_use]
fn hint_at(
    document: &Document,
    offset: usize,
    label: String,
    encoding: PositionEncoding,
) -> InlayHint {
    let position: Position = document.range(offset..offset, encoding).start;
    InlayHint {
        position,
        label: InlayHintLabel::String(label),
        kind: None,
        text_edits: None,
        tooltip: None,
        padding_left: Some(true),
        padding_right: Some(false),
        data: None,
    }
}

/// Collect every call expression in the script.
fn collect_calls_script<'a>(script: &'a Script, out: &mut Vec<&'a Expr>) {
    for item in &script.globals {
        match item {
            GlobalItem::Variable(var) => {
                if let Some(init) = &var.init {
                    collect_calls_expr(init, out);
                }
            }
            GlobalItem::Function(func) => collect_calls_block(&func.body, out),
        }
    }
    for state in &script.states {
        for handler in &state.events {
            collect_calls_block(&handler.body, out);
        }
    }
}

/// Collect the calls in a block's statements.
fn collect_calls_block<'a>(block: &'a Block, out: &mut Vec<&'a Expr>) {
    for stmt in &block.statements {
        collect_calls_stmt(stmt, out);
    }
}

/// Collect the calls in one statement (and its nested statements/expressions).
fn collect_calls_stmt<'a>(stmt: &'a Stmt, out: &mut Vec<&'a Expr>) {
    match stmt {
        Stmt::Local { init, .. } => {
            if let Some(init) = init {
                collect_calls_expr(init, out);
            }
        }
        Stmt::Expr { expr, .. } => collect_calls_expr(expr, out),
        Stmt::Block(block) => collect_calls_block(block, out),
        Stmt::If {
            cond,
            then_branch,
            else_branch,
            ..
        } => {
            collect_calls_expr(cond, out);
            collect_calls_stmt(then_branch, out);
            if let Some(else_branch) = else_branch {
                collect_calls_stmt(else_branch, out);
            }
        }
        Stmt::While { cond, body, .. } => {
            collect_calls_expr(cond, out);
            collect_calls_stmt(body, out);
        }
        Stmt::DoWhile { body, cond, .. } => {
            collect_calls_stmt(body, out);
            collect_calls_expr(cond, out);
        }
        Stmt::For {
            init,
            cond,
            incr,
            body,
            ..
        } => {
            for expr in init {
                collect_calls_expr(expr, out);
            }
            if let Some(cond) = cond {
                collect_calls_expr(cond, out);
            }
            for expr in incr {
                collect_calls_expr(expr, out);
            }
            collect_calls_stmt(body, out);
        }
        Stmt::Return { value, .. } => {
            if let Some(value) = value {
                collect_calls_expr(value, out);
            }
        }
        Stmt::Empty(_)
        | Stmt::Error(_)
        | Stmt::Jump { .. }
        | Stmt::Label { .. }
        | Stmt::StateChange { .. } => {}
    }
}

/// Collect the calls in one expression (and its sub-expressions).
fn collect_calls_expr<'a>(expr: &'a Expr, out: &mut Vec<&'a Expr>) {
    match expr {
        Expr::Call {
            callee: _, args, ..
        } => {
            out.push(expr);
            for arg in args {
                collect_calls_expr(arg, out);
            }
        }
        Expr::List { elements, .. } => {
            for element in elements {
                collect_calls_expr(element, out);
            }
        }
        Expr::Vector { x, y, z, .. } => {
            collect_calls_expr(x, out);
            collect_calls_expr(y, out);
            collect_calls_expr(z, out);
        }
        Expr::Rotation { x, y, z, s, .. } => {
            collect_calls_expr(x, out);
            collect_calls_expr(y, out);
            collect_calls_expr(z, out);
            collect_calls_expr(s, out);
        }
        Expr::Prefix { operand, .. }
        | Expr::Postfix { operand, .. }
        | Expr::Cast { operand, .. } => {
            collect_calls_expr(operand, out);
        }
        Expr::Binary { lhs, rhs, .. } => {
            collect_calls_expr(lhs, out);
            collect_calls_expr(rhs, out);
        }
        Expr::Assign { target, value, .. } => {
            collect_calls_expr(target, out);
            collect_calls_expr(value, out);
        }
        Expr::Paren { inner, .. } => collect_calls_expr(inner, out),
        Expr::Integer { .. }
        | Expr::Float { .. }
        | Expr::Str { .. }
        | Expr::Variable(_)
        | Expr::Member { .. }
        | Expr::Error(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use core::str::FromStr as _;

    use pretty_assertions::assert_eq;

    use super::inlay_hints;
    use crate::document::Document;
    use crate::position::PositionEncoding;
    use lsp_types::{InlayHintLabel, Position, Range, Uri};
    use sl_lsl::{LslFunction, LslSyntax};

    /// A library with a costly `llSleep` (0.2s sleep, 10 energy) and a free
    /// `llSay`.
    fn library() -> LslSyntax {
        let mut syntax = LslSyntax::default();
        let _prev = syntax.functions.insert(
            "llSleep".to_owned(),
            LslFunction {
                sleep: Some(0.2),
                energy: Some(10.0),
                ..LslFunction::default()
            },
        );
        let _prev = syntax
            .functions
            .insert("llSay".to_owned(), LslFunction::default());
        syntax
    }

    /// Open `source`, surfacing a URI-parse failure as an `Err`.
    fn open(source: &str) -> Result<Document, String> {
        let uri = Uri::from_str("file:///test.lsl").map_err(|err| err.to_string())?;
        Ok(Document::open(uri, 1, source.to_owned()))
    }

    /// The whole document as an inlay-hint range (line 0 to a generous end).
    const fn whole() -> Range {
        Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: 1000,
                character: 0,
            },
        }
    }

    /// A costly library call gets a cost hint; a free one does not.
    #[test]
    fn cost_hint_on_costly_call() -> Result<(), String> {
        let source = "default { state_entry() { llSleep(0.5); llSay(0, \"\"); } }\n";
        let doc = open(source)?;
        let hints = inlay_hints(&doc, whole(), &library(), PositionEncoding::Utf16);
        // Exactly one hint — for `llSleep`, not `llSay`.
        assert_eq!(hints.len(), 1);
        let hint = hints.first().ok_or("no hint")?;
        match &hint.label {
            InlayHintLabel::String(text) => {
                assert!(text.contains("sleep 0.2s"), "{text}");
                assert!(text.contains("energy 10"), "{text}");
            }
            InlayHintLabel::LabelParts(_) => return Err("expected string label".to_owned()),
        }
        Ok(())
    }

    /// A hint outside the requested range is omitted.
    #[test]
    fn hint_respects_range() -> Result<(), String> {
        let source = "default { state_entry() { llSleep(0.5); } }\n";
        let doc = open(source)?;
        // A zero-width range at the start of the document excludes the call.
        let narrow = Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: 0,
                character: 1,
            },
        };
        let hints = inlay_hints(&doc, narrow, &library(), PositionEncoding::Utf16);
        assert!(hints.is_empty(), "expected no hints, got {hints:?}");
        Ok(())
    }
}
