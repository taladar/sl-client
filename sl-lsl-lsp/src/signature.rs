//! Answering `textDocument/signatureHelp` — the parameter popup that appears
//! while typing the arguments of a call and highlights the argument the cursor
//! is currently on.
//!
//! The cursor is resolved to a byte offset, the innermost **enclosing call**
//! whose parentheses contain it is found in the parse tree, and its callee is
//! resolved to a signature: a **user** function's from the parse tree, a
//! **library** function's from the grid's [`LslFunction`](sl_lsl::LslFunction).
//! The active parameter is the count of arguments already completed before the
//! cursor, so the popup tracks the comma the user just typed past. A cursor that
//! is not inside any call's argument list has no signature help.

use core::ops::Range;

use lsp_types::{
    ParameterInformation, ParameterLabel, Position, SignatureHelp, SignatureInformation,
};

use sl_lsl::ast::{Block, Expr, FunctionDef, GlobalItem, Ident, Script, Stmt};

use crate::docs;
use crate::document::Document;
use crate::position::PositionEncoding;

/// The signature help for the call surrounding `position` in `document`, against
/// the grid library `syntax`, or [`None`] when the cursor is not inside a call's
/// argument list or the callee resolves to no known signature.
#[expect(
    clippy::module_name_repetitions,
    reason = "`signature_help` names the LSP `textDocument/signatureHelp` request it answers; \
              the protocol term is what a caller searches for"
)]
#[must_use]
pub fn signature_help(
    document: &Document,
    position: Position,
    syntax: &sl_lsl::LslSyntax,
    encoding: PositionEncoding,
) -> Option<SignatureHelp> {
    let offset = document.offset(position, encoding);
    let script = &document.parse().script;
    let call = innermost_call(script, offset)?;
    let signature = signature_for(&call, script, syntax)?;
    let param_count = signature.parameters.as_ref().map_or(0, Vec::len);
    let active = active_parameter(&call, offset, param_count);
    Some(SignatureHelp {
        signatures: vec![signature],
        active_signature: Some(0),
        active_parameter: Some(active),
    })
}

/// A borrowed view of a call at the cursor: its callee, its argument spans and
/// its full span.
struct CallSite<'a> {
    /// The called name.
    callee: &'a Ident,
    /// The byte spans of the argument expressions, in order.
    arg_spans: Vec<Range<usize>>,
}

/// The innermost call whose parentheses contain `offset` — the call whose
/// argument list the cursor is inside. "Inside the parentheses" means past the
/// callee's name (`offset > callee.span.end`) and within the whole call span;
/// among nested matches the one with the smallest span wins.
#[must_use]
fn innermost_call(script: &Script, offset: usize) -> Option<CallSite<'_>> {
    let mut calls = Vec::new();
    collect_calls_script(script, offset, &mut calls);
    let mut best: Option<(&Ident, Vec<Range<usize>>, usize)> = None;
    for call in calls {
        if let Expr::Call { callee, args, span } = call {
            let length = span.end.saturating_sub(span.start);
            let smaller = best
                .as_ref()
                .is_none_or(|(_, _, best_len)| length < *best_len);
            if smaller {
                best = Some((callee, args.iter().map(Expr::span).collect(), length));
            }
        }
    }
    best.map(|(callee, arg_spans, _len)| CallSite { callee, arg_spans })
}

/// Collect every call expression whose parentheses contain `offset` across the
/// whole script.
fn collect_calls_script<'a>(script: &'a Script, offset: usize, out: &mut Vec<&'a Expr>) {
    for item in &script.globals {
        match item {
            GlobalItem::Variable(var) => {
                if let Some(init) = &var.init {
                    collect_calls_expr(init, offset, out);
                }
            }
            GlobalItem::Function(func) => collect_calls_block(&func.body, offset, out),
        }
    }
    for state in &script.states {
        for handler in &state.events {
            collect_calls_block(&handler.body, offset, out);
        }
    }
}

/// Collect the calls containing `offset` in a block's statements.
fn collect_calls_block<'a>(block: &'a Block, offset: usize, out: &mut Vec<&'a Expr>) {
    for stmt in &block.statements {
        collect_calls_stmt(stmt, offset, out);
    }
}

/// Collect the calls containing `offset` in one statement (and its nested
/// statements and expressions).
fn collect_calls_stmt<'a>(stmt: &'a Stmt, offset: usize, out: &mut Vec<&'a Expr>) {
    match stmt {
        Stmt::Local { init, .. } => {
            if let Some(init) = init {
                collect_calls_expr(init, offset, out);
            }
        }
        Stmt::Expr { expr, .. } => collect_calls_expr(expr, offset, out),
        Stmt::Block(block) => collect_calls_block(block, offset, out),
        Stmt::If {
            cond,
            then_branch,
            else_branch,
            ..
        } => {
            collect_calls_expr(cond, offset, out);
            collect_calls_stmt(then_branch, offset, out);
            if let Some(else_branch) = else_branch {
                collect_calls_stmt(else_branch, offset, out);
            }
        }
        Stmt::While { cond, body, .. } => {
            collect_calls_expr(cond, offset, out);
            collect_calls_stmt(body, offset, out);
        }
        Stmt::DoWhile { body, cond, .. } => {
            collect_calls_stmt(body, offset, out);
            collect_calls_expr(cond, offset, out);
        }
        Stmt::For {
            init,
            cond,
            incr,
            body,
            ..
        } => {
            for expr in init {
                collect_calls_expr(expr, offset, out);
            }
            if let Some(cond) = cond {
                collect_calls_expr(cond, offset, out);
            }
            for expr in incr {
                collect_calls_expr(expr, offset, out);
            }
            collect_calls_stmt(body, offset, out);
        }
        Stmt::Return { value, .. } => {
            if let Some(value) = value {
                collect_calls_expr(value, offset, out);
            }
        }
        Stmt::Empty(_)
        | Stmt::Error(_)
        | Stmt::Jump { .. }
        | Stmt::Label { .. }
        | Stmt::StateChange { .. } => {}
    }
}

/// Collect the calls containing `offset` in one expression (and its
/// sub-expressions).
fn collect_calls_expr<'a>(expr: &'a Expr, offset: usize, out: &mut Vec<&'a Expr>) {
    if let Expr::Call { callee, args, span } = expr {
        if span.start <= offset && offset <= span.end && offset > callee.span.end {
            out.push(expr);
        }
        for arg in args {
            collect_calls_expr(arg, offset, out);
        }
        return;
    }
    match expr {
        Expr::List { elements, .. } => {
            for element in elements {
                collect_calls_expr(element, offset, out);
            }
        }
        Expr::Vector { x, y, z, .. } => {
            collect_calls_expr(x, offset, out);
            collect_calls_expr(y, offset, out);
            collect_calls_expr(z, offset, out);
        }
        Expr::Rotation { x, y, z, s, .. } => {
            collect_calls_expr(x, offset, out);
            collect_calls_expr(y, offset, out);
            collect_calls_expr(z, offset, out);
            collect_calls_expr(s, offset, out);
        }
        Expr::Prefix { operand, .. }
        | Expr::Postfix { operand, .. }
        | Expr::Cast { operand, .. } => {
            collect_calls_expr(operand, offset, out);
        }
        Expr::Binary { lhs, rhs, .. } => {
            collect_calls_expr(lhs, offset, out);
            collect_calls_expr(rhs, offset, out);
        }
        Expr::Assign { target, value, .. } => {
            collect_calls_expr(target, offset, out);
            collect_calls_expr(value, offset, out);
        }
        Expr::Paren { inner, .. } => collect_calls_expr(inner, offset, out),
        Expr::Call { .. }
        | Expr::Integer { .. }
        | Expr::Float { .. }
        | Expr::Str { .. }
        | Expr::Variable(_)
        | Expr::Member { .. }
        | Expr::Error(_) => {}
    }
}

/// The signature information for a call's callee: a **user** function's rebuilt
/// from the parse tree (checked first, since a user function shadows nothing but
/// is what the caller wrote), else a **library** function's from the grid table.
/// [`None`] when the name resolves to neither.
#[must_use]
fn signature_for(
    call: &CallSite<'_>,
    script: &Script,
    syntax: &sl_lsl::LslSyntax,
) -> Option<SignatureInformation> {
    let name = call.callee.name.as_str();
    if let Some(func) = user_function(script, name) {
        let parameters: Vec<String> = func
            .params
            .iter()
            .map(|param| format!("{} {}", param.ty.kind.keyword(), param.name.name))
            .collect();
        return Some(build_signature(
            user_function_label(func, &parameters),
            &parameters,
            None,
        ));
    }
    if let Some(func) = syntax.function(name) {
        let label = docs::function_label(name, func);
        let parameters = docs::parameter_labels(&func.arguments);
        return Some(build_signature(label, &parameters, func.tooltip.as_deref()));
    }
    None
}

/// The user function named `name` in the script, if one is defined.
#[must_use]
fn user_function<'a>(script: &'a Script, name: &str) -> Option<&'a FunctionDef> {
    script.globals.iter().find_map(|item| match item {
        GlobalItem::Function(func) if func.name.name == name => Some(func),
        GlobalItem::Function(_) | GlobalItem::Variable(_) => None,
    })
}

/// The signature label for a user function: `[ret ]name(params)`.
#[must_use]
fn user_function_label(func: &FunctionDef, parameters: &[String]) -> String {
    let mut label = String::new();
    if let Some(ret) = &func.ret {
        label.push_str(ret.kind.keyword());
        label.push(' ');
    }
    label.push_str(&func.name.name);
    label.push('(');
    label.push_str(&parameters.join(", "));
    label.push(')');
    label
}

/// Assemble a [`SignatureInformation`] from a label, its parameter labels and an
/// optional documentation string. Each parameter's label is the substring form
/// (`ParameterLabel::Simple`) so a client that cannot offset into the signature
/// still highlights the right token.
#[must_use]
fn build_signature(
    label: String,
    parameters: &[String],
    documentation: Option<&str>,
) -> SignatureInformation {
    SignatureInformation {
        label,
        documentation: documentation.map(|doc| lsp_types::Documentation::String(doc.to_owned())),
        parameters: Some(
            parameters
                .iter()
                .map(|param| ParameterInformation {
                    label: ParameterLabel::Simple(param.clone()),
                    documentation: None,
                })
                .collect(),
        ),
        active_parameter: None,
    }
}

/// The zero-based index of the argument the cursor is on: the count of arguments
/// wholly completed before `offset`, clamped to the last parameter so an
/// over-long call still highlights a real slot.
#[must_use]
fn active_parameter(call: &CallSite<'_>, offset: usize, param_count: usize) -> u32 {
    let mut active = call.arg_spans.len();
    for (index, span) in call.arg_spans.iter().enumerate() {
        if offset <= span.end {
            active = index;
            break;
        }
    }
    let clamped = if param_count == 0 {
        0
    } else {
        active.min(param_count.saturating_sub(1))
    };
    u32::try_from(clamped).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use core::str::FromStr as _;

    use pretty_assertions::assert_eq;

    use super::signature_help;
    use crate::document::Document;
    use crate::position::PositionEncoding;
    use lsp_types::{Position, Uri};
    use sl_lsl::ast::TypeName;
    use sl_lsl::{LslArgument, LslFunction, LslSyntax};

    /// A library with `llSay(integer channel, string msg)`.
    fn library() -> LslSyntax {
        let mut syntax = LslSyntax::default();
        let _prev = syntax.functions.insert(
            "llSay".to_owned(),
            LslFunction {
                arguments: vec![
                    LslArgument {
                        name: "channel".to_owned(),
                        arg_type: Some(TypeName::Integer),
                        tooltip: None,
                    },
                    LslArgument {
                        name: "msg".to_owned(),
                        arg_type: Some(TypeName::String),
                        tooltip: None,
                    },
                ],
                ..LslFunction::default()
            },
        );
        syntax
    }

    /// Open `source`, surfacing a URI-parse failure as an `Err`.
    fn open(source: &str) -> Result<Document, String> {
        let uri = Uri::from_str("file:///test.lsl").map_err(|err| err.to_string())?;
        Ok(Document::open(uri, 1, source.to_owned()))
    }

    /// The signature-help active parameter for a cursor at the given byte offset
    /// on line 0 of `source`.
    fn active_at(source: &str, offset: usize) -> Result<u32, String> {
        let doc = open(source)?;
        let col = u32::try_from(offset).map_err(|err| err.to_string())?;
        let help = signature_help(
            &doc,
            Position {
                line: 0,
                character: col,
            },
            &library(),
            PositionEncoding::Utf16,
        )
        .ok_or("no signature help")?;
        help.active_parameter
            .ok_or_else(|| "no active parameter".to_owned())
    }

    /// The cursor on the first argument highlights parameter 0; after the comma,
    /// parameter 1.
    #[test]
    fn active_parameter_tracks_comma() -> Result<(), String> {
        let source = "default { state_entry() { llSay(0, \"hi\"); } }\n";
        let first_arg = source.find('0').ok_or("no first arg")?;
        assert_eq!(active_at(source, first_arg)?, 0);
        // A byte just after the comma is on the second parameter.
        let comma = source.find(',').ok_or("no comma")?;
        assert_eq!(active_at(source, comma + 1)?, 1);
        Ok(())
    }

    /// The signature label and parameters come from the grid table.
    #[test]
    fn signature_label_from_grid() -> Result<(), String> {
        let source = "default { state_entry() { llSay(0, \"hi\"); } }\n";
        let doc = open(source)?;
        let col =
            u32::try_from(source.find('0').ok_or("no arg")?).map_err(|err| err.to_string())?;
        let help = signature_help(
            &doc,
            Position {
                line: 0,
                character: col,
            },
            &library(),
            PositionEncoding::Utf16,
        )
        .ok_or("no signature help")?;
        let sig = help.signatures.first().ok_or("no signature")?;
        assert_eq!(sig.label, "llSay(integer channel, string msg)");
        let params = sig.parameters.as_ref().ok_or("no parameters")?;
        assert_eq!(params.len(), 2);
        Ok(())
    }

    /// Signature help works for a user-defined function call too, built from the
    /// parse tree.
    #[test]
    fn user_function_signature() -> Result<(), String> {
        let source = "integer add(integer a, integer b) { return a + b; }\n\
                      default { state_entry() { add(1, 2); } }\n";
        let doc = open(source)?;
        // Cursor on the second argument `2` on line 1.
        let line1 = source.lines().nth(1).ok_or("no line 1")?;
        let col = u32::try_from(line1.rfind('2').ok_or("no second arg")?)
            .map_err(|err| err.to_string())?;
        let help = signature_help(
            &doc,
            Position {
                line: 1,
                character: col,
            },
            &library(),
            PositionEncoding::Utf16,
        )
        .ok_or("no signature help")?;
        let sig = help.signatures.first().ok_or("no signature")?;
        assert_eq!(sig.label, "integer add(integer a, integer b)");
        assert_eq!(help.active_parameter, Some(1));
        Ok(())
    }

    /// A cursor outside any call has no signature help.
    #[test]
    fn no_help_outside_call() -> Result<(), String> {
        let source = "integer x;\ndefault { state_entry() {} }\n";
        let doc = open(source)?;
        let help = signature_help(
            &doc,
            Position {
                line: 0,
                character: 0,
            },
            &library(),
            PositionEncoding::Utf16,
        );
        assert_eq!(help.map(|_| ()), None);
        Ok(())
    }
}
