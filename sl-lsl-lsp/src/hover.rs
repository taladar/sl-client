//! Answering `textDocument/hover` — the documentation popup when the cursor
//! rests on an identifier.
//!
//! The cursor position is resolved to a byte offset, the navigation pass finds
//! the identifier under it, and the answer depends on what it binds to: a
//! **library** function, constant or event shows the grid's own signature,
//! description and costs ([`crate::docs`]); a **user** symbol shows the one-line
//! detail (a variable's type, a function's signature) the resolution pass
//! already computed for its declaration. An identifier that resolves to nothing
//! — a keyword, a literal, an undefined name — has no hover.

use lsp_types::{Hover, HoverContents, MarkupContent, MarkupKind, Position};

use crate::docs;
use crate::document::Document;
use crate::navigation::{Binding, SymbolClass, declaration_of, occurrence_at, resolve};
use crate::position::PositionEncoding;

/// The hover for the identifier at `position` in `document`, against the grid
/// library `syntax`, or [`None`] when the cursor is not on a symbol the server
/// can document.
#[must_use]
pub fn hover(
    document: &Document,
    position: Position,
    syntax: &sl_lsl::LslSyntax,
    encoding: PositionEncoding,
) -> Option<Hover> {
    let offset = document.offset(position, encoding);
    let occurrences = resolve(&document.parse().script, syntax);
    let target = occurrence_at(&occurrences, offset)?;

    let markdown = match &target.binding {
        Binding::Library => library_markdown(syntax, &target.name, target.class)?,
        Binding::User { .. } => {
            let detail = declaration_of(&occurrences, target)?.detail.as_deref()?;
            docs::user_markdown(detail)
        }
    };

    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: markdown,
        }),
        range: Some(document.range(target.span.clone(), encoding)),
    })
}

/// The Markdown body for a library symbol of the given class, or [`None`] when
/// the grid does not actually carry that name in that group.
#[must_use]
fn library_markdown(syntax: &sl_lsl::LslSyntax, name: &str, class: SymbolClass) -> Option<String> {
    match class {
        SymbolClass::Function => syntax
            .function(name)
            .map(|func| docs::function_markdown(name, func)),
        SymbolClass::Constant => syntax
            .constant(name)
            .map(|constant| docs::constant_markdown(name, constant)),
        SymbolClass::Event => syntax
            .event(name)
            .map(|event| docs::event_markdown(name, event)),
        SymbolClass::Variable | SymbolClass::State | SymbolClass::Label => None,
    }
}

#[cfg(test)]
mod tests {
    use core::str::FromStr as _;

    use pretty_assertions::assert_eq;

    use super::hover;
    use crate::document::Document;
    use crate::position::PositionEncoding;
    use lsp_types::{HoverContents, Position, Uri};
    use sl_lsl::ast::TypeName;
    use sl_lsl::{LslArgument, LslConstant, LslFunction, LslSyntax};

    /// A library with `llSay` and the constant `PI`.
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
                tooltip: Some("Says text on a channel.".to_owned()),
                ..LslFunction::default()
            },
        );
        let _prev = syntax.constants.insert(
            "PI".to_owned(),
            LslConstant {
                constant_type: Some(TypeName::Float),
                value: Some("3.14159".to_owned()),
                ..LslConstant::default()
            },
        );
        syntax
    }

    /// Open `source`, surfacing a URI-parse failure as an `Err`.
    fn open(source: &str) -> Result<Document, String> {
        let uri = Uri::from_str("file:///test.lsl").map_err(|err| err.to_string())?;
        Ok(Document::open(uri, 1, source.to_owned()))
    }

    /// The Markdown text of a hover, or an `Err` describing what was missing.
    fn hover_text(h: Option<lsp_types::Hover>) -> Result<String, String> {
        match h.ok_or("no hover")?.contents {
            HoverContents::Markup(markup) => Ok(markup.value),
            HoverContents::Scalar(_) | HoverContents::Array(_) => {
                Err("expected markup hover".to_owned())
            }
        }
    }

    /// Hovering a library call shows its signature and description.
    #[test]
    fn hover_library_function() -> Result<(), String> {
        let source = "default { state_entry() { llSay(0, \"hi\"); } }\n";
        let doc = open(source)?;
        // Column of `llSay` on line 0 (single-line source).
        let col = u32::try_from(source.find("llSay").ok_or("no llSay")?)
            .map_err(|err| err.to_string())?;
        let h = hover(
            &doc,
            Position {
                line: 0,
                character: col,
            },
            &library(),
            PositionEncoding::Utf16,
        );
        let text = hover_text(h)?;
        assert!(
            text.contains("llSay(integer channel, string msg)"),
            "{text}"
        );
        assert!(text.contains("Says text on a channel."), "{text}");
        Ok(())
    }

    /// Hovering a user variable shows its declared type.
    #[test]
    fn hover_user_variable() -> Result<(), String> {
        let source = "integer counter;\ndefault { state_entry() { counter = 1; } }\n";
        let doc = open(source)?;
        // The `counter` use is on line 1.
        let line1 = source.lines().nth(1).ok_or("no line 1")?;
        let col = u32::try_from(line1.find("counter").ok_or("no counter use")?)
            .map_err(|err| err.to_string())?;
        let h = hover(
            &doc,
            Position {
                line: 1,
                character: col,
            },
            &library(),
            PositionEncoding::Utf16,
        );
        let text = hover_text(h)?;
        assert!(text.contains("integer counter"), "{text}");
        Ok(())
    }

    /// Hovering whitespace yields no hover.
    #[test]
    fn hover_nothing_on_blank() -> Result<(), String> {
        let doc = open("integer counter;\n")?;
        let h = hover(
            &doc,
            Position {
                line: 0,
                character: 0,
            },
            &library(),
            PositionEncoding::Utf16,
        );
        // Column 0 is the `i` of `integer` — a type keyword, not a symbol.
        assert_eq!(h.map(|_| ()), None);
        Ok(())
    }
}
