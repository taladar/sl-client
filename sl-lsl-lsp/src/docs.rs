//! Rendering the grid's library symbols into **human-readable documentation** —
//! the one-line signatures and the Markdown tooltips hover and signature help
//! both show.
//!
//! The `LSLSyntax` capability is what makes ours the language server nobody else
//! can build: the description text, the per-argument tooltips, and the energy /
//! sleep costs all come from the *connected grid*, so a hover over `llSay` shows
//! Linden Lab's own words (and an `os*` call shows OpenSim's), current for the
//! grid the script will run on rather than scraped from a wiki. This module owns
//! turning a decoded [`LslFunction`] / [`LslConstant`] / [`LslEvent`] into the
//! string forms the LSP requests need:
//!
//! - a **signature label** (`float llFrand(float mag)`) for the first line of a
//!   hover and the header of a signature-help popup;
//! - the **parameter labels** (`integer channel`, `string msg`) a signature-help
//!   popup highlights one at a time as the user types past each comma;
//! - a **Markdown body** folding in the description, the costs, and the
//!   `deprecated` / `god-mode` flags.

use core::fmt::Write as _;

use sl_lsl::ast::TypeName;
use sl_lsl::{LslArgument, LslConstant, LslEvent, LslFunction};

/// The type keyword for an argument's declared type, or `void` when the grid
/// served a type keyword that is not one of LSL's seven (decoded to [`None`]).
#[must_use]
fn arg_type_keyword(arg: &LslArgument) -> &'static str {
    arg.arg_type.map_or("void", TypeName::keyword)
}

/// One `type name` parameter label (`integer channel`), the unit a
/// signature-help popup highlights.
#[must_use]
pub fn parameter_label(arg: &LslArgument) -> String {
    format!("{} {}", arg_type_keyword(arg), arg.name)
}

/// The parameter labels of an argument list, in order.
#[must_use]
pub fn parameter_labels(arguments: &[LslArgument]) -> Vec<String> {
    arguments.iter().map(parameter_label).collect()
}

/// The one-line signature label for a library function: `[ret ]name(params)`,
/// with `ret` omitted for a `void` function.
#[must_use]
pub fn function_label(name: &str, func: &LslFunction) -> String {
    let mut label = String::new();
    if let Some(ret) = func.return_type {
        label.push_str(ret.keyword());
        label.push(' ');
    }
    label.push_str(name);
    label.push('(');
    label.push_str(&parameter_labels(&func.arguments).join(", "));
    label.push(')');
    label
}

/// The one-line signature label for a library event: `name(params)`.
#[must_use]
pub fn event_label(name: &str, event: &LslEvent) -> String {
    format!("{name}({})", parameter_labels(&event.arguments).join(", "))
}

/// The one-line label for a library constant: `type name[ = value]`.
#[must_use]
pub fn constant_label(name: &str, constant: &LslConstant) -> String {
    let ty = constant.constant_type.map_or("", TypeName::keyword);
    let mut label = if ty.is_empty() {
        name.to_owned()
    } else {
        format!("{ty} {name}")
    };
    if let Some(value) = &constant.value {
        let _ignored = write!(label, " = {value}");
    }
    label
}

/// The Markdown hover body for a library function: the signature in a fenced
/// `lsl` block, the flag notes, the description, and the costs.
#[must_use]
pub fn function_markdown(name: &str, func: &LslFunction) -> String {
    let mut body = code_block(&function_label(name, func));
    push_flags(&mut body, func.deprecated, func.god_mode);
    if let Some(tooltip) = &func.tooltip {
        push_paragraph(&mut body, tooltip);
    }
    push_costs(&mut body, func.energy, func.sleep);
    body
}

/// The Markdown hover body for a library constant: its `type name = value` line
/// and any description.
#[must_use]
pub fn constant_markdown(name: &str, constant: &LslConstant) -> String {
    let mut body = code_block(&constant_label(name, constant));
    push_flags(&mut body, constant.deprecated, constant.god_mode);
    if let Some(tooltip) = &constant.tooltip {
        push_paragraph(&mut body, tooltip);
    }
    body
}

/// The Markdown hover body for a library event: its signature and any
/// description.
#[must_use]
pub fn event_markdown(name: &str, event: &LslEvent) -> String {
    let mut body = code_block(&event_label(name, event));
    push_flags(&mut body, event.deprecated, event.god_mode);
    if let Some(tooltip) = &event.tooltip {
        push_paragraph(&mut body, tooltip);
    }
    body
}

/// The Markdown hover body for a user symbol: its one-line `detail` (a variable
/// type or a function signature) in a fenced `lsl` block.
#[must_use]
pub fn user_markdown(detail: &str) -> String {
    code_block(detail)
}

/// Wrap `code` in a fenced ```lsl block``` (the language tag an editor
/// highlights the hover snippet with), ending with a newline.
#[must_use]
fn code_block(code: &str) -> String {
    format!("```lsl\n{code}\n```")
}

/// Append the `deprecated` / `god-mode` notes for a symbol, each on its own
/// paragraph, when the grid sets the corresponding flag.
fn push_flags(body: &mut String, deprecated: bool, god_mode: bool) {
    if deprecated {
        push_paragraph(body, "**Deprecated.**");
    }
    if god_mode {
        push_paragraph(body, "**God-mode only** — usable only by an estate god.");
    }
}

/// Append the energy and forced-sleep costs, when the grid advertises them (Second
/// Life ships both; OpenSim omits them).
fn push_costs(body: &mut String, energy: Option<f32>, sleep: Option<f32>) {
    if let Some(energy) = energy {
        push_paragraph(body, &format!("Energy: {energy}"));
    }
    if let Some(sleep) = sleep {
        push_paragraph(body, &format!("Sleep: {sleep}s"));
    }
}

/// Append `text` as a new Markdown paragraph (blank line before it).
fn push_paragraph(body: &mut String, text: &str) {
    body.push_str("\n\n");
    body.push_str(text);
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{constant_label, event_label, function_label, function_markdown, parameter_labels};
    use sl_lsl::ast::TypeName;
    use sl_lsl::{LslArgument, LslConstant, LslEvent, LslFunction};

    /// An argument with the given name and type.
    fn arg(name: &str, ty: TypeName) -> LslArgument {
        LslArgument {
            name: name.to_owned(),
            arg_type: Some(ty),
            tooltip: None,
        }
    }

    /// A function label prints the return type, name and typed parameters.
    #[test]
    fn function_label_full() {
        let func = LslFunction {
            return_type: Some(TypeName::Float),
            arguments: vec![arg("mag", TypeName::Float)],
            ..LslFunction::default()
        };
        assert_eq!(function_label("llFrand", &func), "float llFrand(float mag)");
    }

    /// A void function omits the return type.
    #[test]
    fn function_label_void() {
        let func = LslFunction {
            arguments: vec![
                arg("channel", TypeName::Integer),
                arg("msg", TypeName::String),
            ],
            ..LslFunction::default()
        };
        assert_eq!(
            function_label("llSay", &func),
            "llSay(integer channel, string msg)"
        );
        assert_eq!(
            parameter_labels(&func.arguments),
            vec!["integer channel", "string msg"]
        );
    }

    /// An event label prints its name and parameters.
    #[test]
    fn event_label_params() {
        let event = LslEvent {
            arguments: vec![arg("num_detected", TypeName::Integer)],
            ..LslEvent::default()
        };
        assert_eq!(
            event_label("touch_start", &event),
            "touch_start(integer num_detected)"
        );
    }

    /// A constant label prints its type, name and value.
    #[test]
    fn constant_label_value() {
        let constant = LslConstant {
            constant_type: Some(TypeName::Float),
            value: Some("3.14159".to_owned()),
            ..LslConstant::default()
        };
        assert_eq!(constant_label("PI", &constant), "float PI = 3.14159");
    }

    /// The Markdown body folds in the description, deprecated flag and costs.
    #[test]
    fn markdown_includes_description_and_costs() {
        let func = LslFunction {
            return_type: Some(TypeName::Integer),
            arguments: vec![arg("id", TypeName::Key)],
            energy: Some(10.0),
            sleep: Some(0.2),
            tooltip: Some("Does a thing.".to_owned()),
            deprecated: true,
            ..LslFunction::default()
        };
        let md = function_markdown("llThing", &func);
        assert!(md.contains("```lsl\ninteger llThing(key id)\n```"), "{md}");
        assert!(md.contains("Deprecated"), "{md}");
        assert!(md.contains("Does a thing."), "{md}");
        assert!(md.contains("Energy: 10"), "{md}");
        assert!(md.contains("Sleep: 0.2s"), "{md}");
    }
}
