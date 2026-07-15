//! Turning a document's parse and semantic analysis into LSP **diagnostics** —
//! the squiggles an editor shows and the entries in its problems panel.
//!
//! Three sources feed one list, all *local* (no grid round-trip), which is the
//! whole point: **SL has no compile-without-save**, so the only way to tell a
//! user their script is wrong before they overwrite the in-world copy is to
//! check it here.
//!
//! - **parse errors** ([`sl_lsl::Parse::errors`]) — the recovered syntax errors,
//!   always [`ERROR`](lsp_types::DiagnosticSeverity::ERROR);
//! - **semantic findings** ([`sl_lsl::analyze`]) — undefined symbols, arity and
//!   type mismatches, return correctness, duplicates and reachability, mapped
//!   from the pass's own [`sl_lsl::Severity`];
//! - **library lints** — a *use* of a symbol the grid flags `deprecated` (a
//!   warning carrying the [`DEPRECATED`](lsp_types::DiagnosticTag::DEPRECATED)
//!   tag, so the editor strikes it through) or `god-mode` (an informational note
//!   that only an estate god may call it). These read the grid's own flags via
//!   the navigation pass, so they cost nothing extra and never fire on a name
//!   the connected grid does not actually deprecate.
//!
//! The grid's own authoritative `ScriptCompileError` is *not* here: it arrives
//! only on an explicit save/compile, is rendered by
//! [`sl_lsl::render_grid_error`], and belongs to the editor-save task, not to
//! the keystroke-time push this module drives.

use lsp_types::{
    Diagnostic, DiagnosticSeverity, DiagnosticTag, NumberOrString, PublishDiagnosticsParams,
};

use sl_lsl::{Severity, analyze};

use crate::document::Document;
use crate::navigation::{Binding, SymbolClass, resolve};
use crate::position::PositionEncoding;

/// The diagnostic `source` label every finding carries, so an editor showing
/// several servers' diagnostics attributes ours.
const SOURCE: &str = "sl-lsl";

/// Compute the full diagnostic list for `document` against the grid library
/// `syntax`, under `encoding`: parse errors, semantic findings, and the
/// deprecated/god-mode library lints, in source order.
#[must_use]
pub fn diagnostics(
    document: &Document,
    syntax: &sl_lsl::LslSyntax,
    encoding: PositionEncoding,
) -> Vec<Diagnostic> {
    let parse = document.parse();
    let mut out = Vec::new();

    for error in &parse.errors {
        out.push(Diagnostic {
            range: document.range(error.span.clone(), encoding),
            severity: Some(DiagnosticSeverity::ERROR),
            source: Some(SOURCE.to_owned()),
            message: error.message.clone(),
            ..Diagnostic::default()
        });
    }

    for finding in analyze(&parse.script, syntax) {
        out.push(Diagnostic {
            range: document.range(finding.span.clone(), encoding),
            severity: Some(severity_of(finding.severity)),
            source: Some(SOURCE.to_owned()),
            message: finding.message.clone(),
            ..Diagnostic::default()
        });
    }

    lint_library_use(document, syntax, encoding, &mut out);

    out.sort_by(|a, b| {
        a.range
            .start
            .cmp(&b.range.start)
            .then_with(|| a.range.end.cmp(&b.range.end))
    });
    out
}

/// The full `publishDiagnostics` payload for `document`: its diagnostics tagged
/// with the document's URI and current version (the LSP guard that lets a client
/// drop a report computed against text it has since edited past).
#[must_use]
pub fn publish_params(
    document: &Document,
    syntax: &sl_lsl::LslSyntax,
    encoding: PositionEncoding,
) -> PublishDiagnosticsParams {
    PublishDiagnosticsParams {
        uri: document.uri().clone(),
        diagnostics: diagnostics(document, syntax, encoding),
        version: Some(document.version()),
    }
}

/// The `publishDiagnostics` payload that **clears** all diagnostics for `uri`
/// (an empty list) — sent when a document is closed so its squiggles do not
/// linger in the editor's problems panel.
#[must_use]
pub fn clear_params(uri: &lsp_types::Uri) -> PublishDiagnosticsParams {
    PublishDiagnosticsParams {
        uri: uri.clone(),
        diagnostics: Vec::new(),
        version: None,
    }
}

/// Map the semantic pass's [`Severity`] to the LSP one.
#[must_use]
const fn severity_of(severity: Severity) -> DiagnosticSeverity {
    match severity {
        Severity::Error => DiagnosticSeverity::ERROR,
        Severity::Warning => DiagnosticSeverity::WARNING,
    }
}

/// Emit the library-use lints: for every resolved *use* of a library function,
/// constant or event the grid flags `deprecated` or `god-mode`, push a
/// diagnostic. Declarations and user symbols are skipped — only a call/reference
/// to a *library* symbol carries a grid flag.
fn lint_library_use(
    document: &Document,
    syntax: &sl_lsl::LslSyntax,
    encoding: PositionEncoding,
    out: &mut Vec<Diagnostic>,
) {
    for occ in resolve(&document.parse().script, syntax) {
        if occ.binding != Binding::Library {
            continue;
        }
        let (deprecated, god_mode) = library_flags(syntax, &occ.name, occ.class);
        if deprecated {
            out.push(Diagnostic {
                range: document.range(occ.span.clone(), encoding),
                severity: Some(DiagnosticSeverity::WARNING),
                code: Some(NumberOrString::String("deprecated".to_owned())),
                source: Some(SOURCE.to_owned()),
                message: format!("`{}` is deprecated", occ.name),
                tags: Some(vec![DiagnosticTag::DEPRECATED]),
                ..Diagnostic::default()
            });
        }
        if god_mode {
            out.push(Diagnostic {
                range: document.range(occ.span.clone(), encoding),
                severity: Some(DiagnosticSeverity::INFORMATION),
                code: Some(NumberOrString::String("god-mode".to_owned())),
                source: Some(SOURCE.to_owned()),
                message: format!("`{}` is only usable by an estate god", occ.name),
                ..Diagnostic::default()
            });
        }
    }
}

/// The `(deprecated, god_mode)` flags the grid attaches to a library symbol of
/// the given class, or `(false, false)` for a class the grid does not flag or a
/// name it does not know.
#[must_use]
fn library_flags(syntax: &sl_lsl::LslSyntax, name: &str, class: SymbolClass) -> (bool, bool) {
    match class {
        SymbolClass::Function => syntax
            .function(name)
            .map_or((false, false), |func| (func.deprecated, func.god_mode)),
        SymbolClass::Constant => syntax
            .constant(name)
            .map_or((false, false), |c| (c.deprecated, c.god_mode)),
        SymbolClass::Event => syntax
            .event(name)
            .map_or((false, false), |event| (event.deprecated, event.god_mode)),
        SymbolClass::Variable | SymbolClass::State | SymbolClass::Label => (false, false),
    }
}

#[cfg(test)]
mod tests {
    use core::str::FromStr as _;

    use pretty_assertions::assert_eq;

    use super::diagnostics;
    use crate::document::Document;
    use crate::position::PositionEncoding;
    use lsp_types::{DiagnosticSeverity, DiagnosticTag, Uri};
    use sl_lsl::ast::TypeName;
    use sl_lsl::{LslArgument, LslFunction, LslSyntax};

    /// A library with `llSay` (two args) and a deprecated `llSleep`, enough to
    /// exercise a semantic error and the deprecated lint.
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
        let _prev = syntax.functions.insert(
            "llSleep".to_owned(),
            LslFunction {
                arguments: vec![LslArgument {
                    name: "sec".to_owned(),
                    arg_type: Some(TypeName::Float),
                    tooltip: None,
                }],
                deprecated: true,
                ..LslFunction::default()
            },
        );
        syntax
    }

    /// Open `source` as a document, surfacing a URI-parse failure as an `Err`.
    fn open(source: &str) -> Result<Document, String> {
        let uri = Uri::from_str("file:///test.lsl").map_err(|err| err.to_string())?;
        Ok(Document::open(uri, 1, source.to_owned()))
    }

    /// A syntax error surfaces as an ERROR diagnostic.
    #[test]
    fn parse_error_is_error() -> Result<(), String> {
        let doc = open("integer x = ;\ndefault { state_entry() {} }\n")?;
        let diags = diagnostics(&doc, &library(), PositionEncoding::Utf16);
        assert!(
            diags
                .iter()
                .any(|d| d.severity == Some(DiagnosticSeverity::ERROR)),
            "expected a parse error, got {diags:?}"
        );
        Ok(())
    }

    /// A call with the wrong argument count surfaces as a semantic ERROR.
    #[test]
    fn wrong_arg_count_is_error() -> Result<(), String> {
        let doc = open("default { state_entry() { llSay(0); } }\n")?;
        let diags = diagnostics(&doc, &library(), PositionEncoding::Utf16);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("llSay") && d.message.contains("argument")),
            "expected an arity error, got {diags:?}"
        );
        Ok(())
    }

    /// A use of a deprecated library function surfaces as a WARNING carrying the
    /// DEPRECATED tag.
    #[test]
    fn deprecated_use_is_tagged_warning() -> Result<(), String> {
        let doc = open("default { state_entry() { llSleep(1.0); } }\n")?;
        let diags = diagnostics(&doc, &library(), PositionEncoding::Utf16);
        let deprecated = diags
            .iter()
            .find(|d| d.message.contains("deprecated"))
            .ok_or("expected a deprecated warning")?;
        assert_eq!(deprecated.severity, Some(DiagnosticSeverity::WARNING));
        assert_eq!(
            deprecated.tags.as_deref(),
            Some([DiagnosticTag::DEPRECATED].as_slice())
        );
        Ok(())
    }

    /// A clean script produces no diagnostics.
    #[test]
    fn clean_script_is_quiet() -> Result<(), String> {
        let doc = open("default { state_entry() { llSay(0, \"hi\"); } }\n")?;
        let diags = diagnostics(&doc, &library(), PositionEncoding::Utf16);
        assert_eq!(diags, vec![]);
        Ok(())
    }
}
