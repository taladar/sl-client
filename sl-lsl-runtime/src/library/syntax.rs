//! The library table as an [`LslSyntax`] — the shape `sl_lsl`'s semantic
//! pass checks a script against — so the compiler's first check reads the
//! same list of functions, constants and events the lowering resolves
//! against.

use sl_lsl::{LslArgument, LslConstant, LslEvent, LslFunction, LslKeyword, LslSyntax};

use crate::library::generated::{BUILTINS, CONSTANTS, EVENTS};
use crate::library::table::Argument;
use crate::value::Value;

/// The library table as an [`LslSyntax`], with LSL's seven type keywords
/// and eight control-flow keywords.
#[must_use]
pub fn lsl_syntax() -> LslSyntax {
    let mut syntax = LslSyntax::default();
    for builtin in &BUILTINS {
        let _previous = syntax.functions.insert(
            builtin.name.to_owned(),
            LslFunction {
                return_type: builtin.ret,
                arguments: arguments(builtin.args),
                energy: Some(builtin.energy),
                sleep: Some(builtin.sleep),
                tooltip: Some(builtin.tooltip.to_owned()),
                deprecated: builtin.deprecated,
                god_mode: builtin.god_mode,
            },
        );
    }
    for constant in &CONSTANTS {
        let value = constant.value.to_value();
        let text = match crate::cast(value, sl_lsl::ast::TypeName::String) {
            Ok(Value::String(text)) => Some(text),
            Ok(_) | Err(_) => None,
        };
        let _previous = syntax.constants.insert(
            constant.name.to_owned(),
            LslConstant {
                constant_type: Some(constant.value.type_name()),
                value: text,
                tooltip: Some(constant.tooltip.to_owned()),
                deprecated: false,
                god_mode: false,
            },
        );
    }
    for event in &EVENTS {
        let _previous = syntax.events.insert(
            event.name.to_owned(),
            LslEvent {
                arguments: arguments(event.args),
                tooltip: Some(event.tooltip.to_owned()),
                deprecated: false,
                god_mode: false,
            },
        );
    }
    for keyword in sl_lsl::types::ALL_TYPES.map(sl_lsl::ast::TypeName::keyword) {
        let _previous = syntax
            .types
            .insert(keyword.to_owned(), LslKeyword::default());
    }
    for keyword in [
        "do", "else", "for", "if", "jump", "return", "state", "while",
    ] {
        let _previous = syntax
            .controls
            .insert(keyword.to_owned(), LslKeyword::default());
    }
    syntax
}

/// A descriptor's parameters as [`LslArgument`]s.
fn arguments(args: &[Argument]) -> Vec<LslArgument> {
    args.iter()
        .map(|arg| LslArgument {
            name: arg.name.to_owned(),
            arg_type: Some(arg.ty),
            tooltip: None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn every_table_entry_is_in_the_syntax() {
        let syntax = lsl_syntax();
        assert_eq!(syntax.functions.len(), BUILTINS.len());
        assert_eq!(syntax.constants.len(), CONSTANTS.len());
        assert_eq!(syntax.events.len(), EVENTS.len());
        assert_eq!(syntax.function("llSay").map(|f| f.arguments.len()), Some(2));
        assert_eq!(
            syntax.constant("PI").and_then(|c| c.value.clone()),
            Some("3.141593".to_owned())
        );
        assert!(syntax.is_control("jump"));
        assert!(syntax.is_type("rotation"));
    }
}
