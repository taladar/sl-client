//! Integration tests for the `sl-lsl` semantic pass over its public API.
//!
//! Each test parses a whole script, runs [`analyze`] against a small hand-built
//! library table modelled on the grid's `LSLSyntax` document, and asserts on the
//! resulting [`DiagnosticKind`]s. Findings are sorted by source span, so an
//! exact-vector assertion is deterministic.
//!
//! The overriding property under test is the **no-false-positive bar**: every
//! "valid script" case must produce *zero* diagnostics, and the gating cases
//! prove an empty table suppresses the checks that would otherwise misfire.

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use sl_lsl::ast::TypeName;
    use sl_lsl::parse;
    use sl_lsl::semantics::{DiagnosticKind, Severity, analyze};
    use sl_lsl::syntax::{LslArgument, LslConstant, LslEvent, LslFunction, LslKeyword, LslSyntax};

    /// A library argument with a known type and no tooltip.
    fn arg(name: &str, ty: TypeName) -> LslArgument {
        LslArgument {
            name: name.to_owned(),
            arg_type: Some(ty),
            tooltip: None,
        }
    }

    /// A representative slice of the grid library: a handful of functions,
    /// constants and events with real signatures, plus the keyword groups. Big
    /// enough to exercise arity, typing and gating; small enough to read.
    fn library() -> LslSyntax {
        let mut syntax = LslSyntax::default();

        let _prev = syntax.functions.insert(
            "llSay".to_owned(),
            LslFunction {
                return_type: None,
                arguments: vec![
                    arg("channel", TypeName::Integer),
                    arg("msg", TypeName::String),
                ],
                ..LslFunction::default()
            },
        );
        let _prev = syntax.functions.insert(
            "llSetTimerEvent".to_owned(),
            LslFunction {
                return_type: None,
                arguments: vec![arg("rate", TypeName::Float)],
                ..LslFunction::default()
            },
        );
        let _prev = syntax.functions.insert(
            "llGetOwner".to_owned(),
            LslFunction {
                return_type: Some(TypeName::Key),
                arguments: vec![],
                ..LslFunction::default()
            },
        );
        let _prev = syntax.functions.insert(
            "llKey2Name".to_owned(),
            LslFunction {
                return_type: Some(TypeName::String),
                arguments: vec![arg("id", TypeName::Key)],
                ..LslFunction::default()
            },
        );

        let _prev = syntax.constants.insert(
            "TRUE".to_owned(),
            LslConstant {
                constant_type: Some(TypeName::Integer),
                value: Some("1".to_owned()),
                ..LslConstant::default()
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

        let _prev = syntax
            .events
            .insert("state_entry".to_owned(), LslEvent::default());
        let _prev = syntax.events.insert(
            "touch_start".to_owned(),
            LslEvent {
                arguments: vec![arg("num_detected", TypeName::Integer)],
                ..LslEvent::default()
            },
        );

        for control in ["if", "else", "while", "for", "return", "jump", "state"] {
            let _prev = syntax
                .controls
                .insert(control.to_owned(), LslKeyword::default());
        }
        for ty in [
            "integer", "float", "string", "key", "vector", "rotation", "list",
        ] {
            let _prev = syntax.types.insert(ty.to_owned(), LslKeyword::default());
        }

        syntax
    }

    /// Parse `src` and return the semantic diagnostics' kinds, against the full
    /// library table.
    fn kinds(src: &str) -> Vec<DiagnosticKind> {
        let script = parse(src).script;
        analyze(&script, &library())
            .into_iter()
            .map(|d| d.kind)
            .collect()
    }

    /// Parse `src` and return the diagnostics' kinds against an *empty* library
    /// table (the grid data not yet fetched), to prove gating.
    fn kinds_no_library(src: &str) -> Vec<DiagnosticKind> {
        let script = parse(src).script;
        analyze(&script, &LslSyntax::default())
            .into_iter()
            .map(|d| d.kind)
            .collect()
    }

    /// A realistic, valid script produces no diagnostics at all — the central
    /// no-false-positive property.
    #[test]
    fn valid_script_is_clean() {
        let src = "\
integer gCounter;
string greet(key who) {
    return \"hello \" + llKey2Name(who);
}
default {
    state_entry() {
        llSetTimerEvent(5);
        llSay(0, greet(llGetOwner()));
    }
    touch_start(integer total) {
        integer i;
        for (i = 0; i < total; ++i) {
            llSay(0, \"touched\");
        }
        state waiting;
    }
}
state waiting {
    state_entry() {
        state default;
    }
}";
        assert_eq!(kinds(src), vec![]);
    }

    #[test]
    fn undefined_function_call() {
        assert_eq!(
            kinds("default { state_entry() { doesNotExist(); } }"),
            vec![DiagnosticKind::UndefinedFunction {
                name: "doesNotExist".to_owned()
            }]
        );
    }

    #[test]
    fn undefined_variable_reference() {
        assert_eq!(
            kinds("default { state_entry() { llSay(0, missingVar); } }"),
            vec![DiagnosticKind::UndefinedVariable {
                name: "missingVar".to_owned()
            }]
        );
    }

    #[test]
    fn library_call_wrong_arity() {
        assert_eq!(
            kinds("default { state_entry() { llSay(0); } }"),
            vec![DiagnosticKind::WrongArgCount {
                callee: "llSay".to_owned(),
                expected: 2,
                found: 1,
            }]
        );
    }

    #[test]
    fn user_function_wrong_arity_and_order_insensitive() {
        // `helper` is called before it is defined; order-insensitive resolution
        // must not flag it undefined, only its arity.
        let src = "\
default { state_entry() { helper(1); } }
integer helper(integer a, integer b) { return a; }";
        assert_eq!(
            kinds(src),
            vec![DiagnosticKind::WrongArgCount {
                callee: "helper".to_owned(),
                expected: 2,
                found: 1,
            }]
        );
    }

    #[test]
    fn arg_type_mismatch_is_flagged() {
        assert_eq!(
            kinds("default { state_entry() { llSetTimerEvent(\"fast\"); } }"),
            vec![DiagnosticKind::ArgTypeMismatch {
                callee: "llSetTimerEvent".to_owned(),
                index: 0,
                expected: TypeName::Float,
                found: TypeName::String,
            }]
        );
    }

    #[test]
    fn implicit_conversions_are_accepted() {
        // integer -> float (llSetTimerEvent), string -> key (llKey2Name),
        // key -> string (llGetOwner into llSay's msg). None may be flagged.
        let src = "\
default {
    state_entry() {
        llSetTimerEvent(3);
        llKey2Name(\"00000000-0000-0000-0000-000000000000\");
        llSay(0, llGetOwner());
    }
}";
        assert_eq!(kinds(src), vec![]);
    }

    #[test]
    fn undefined_state_target() {
        assert_eq!(
            kinds("default { state_entry() { state nowhere; } }"),
            vec![DiagnosticKind::UndefinedState {
                name: "nowhere".to_owned()
            }]
        );
    }

    #[test]
    fn missing_default_state() {
        assert_eq!(
            kinds("state lonely { state_entry() { } }"),
            vec![DiagnosticKind::MissingDefaultState]
        );
    }

    #[test]
    fn unreachable_state_warns() {
        let src = "\
default { state_entry() { } }
state orphan { state_entry() { } }";
        assert_eq!(
            kinds(src),
            vec![DiagnosticKind::UnreachableState {
                name: "orphan".to_owned()
            }]
        );
    }

    #[test]
    fn reachable_state_does_not_warn() {
        let src = "\
default { touch_start(integer n) { state active; } state_entry() { } }
state active { state_entry() { } }";
        assert_eq!(kinds(src), vec![]);
    }

    #[test]
    fn return_value_in_void_function() {
        assert_eq!(
            kinds("nothing() { return 5; } default { state_entry() { } }"),
            vec![DiagnosticKind::ReturnValueInVoid]
        );
    }

    #[test]
    fn return_value_in_event() {
        assert_eq!(
            kinds("default { state_entry() { return 5; } }"),
            vec![DiagnosticKind::ReturnValueInVoid]
        );
    }

    #[test]
    fn missing_return_value() {
        assert_eq!(
            kinds("integer f() { return; } default { state_entry() { } }"),
            vec![DiagnosticKind::MissingReturnValue {
                expected: TypeName::Integer
            }]
        );
    }

    #[test]
    fn return_type_mismatch() {
        assert_eq!(
            kinds("integer f() { return \"nope\"; } default { state_entry() { } }"),
            vec![DiagnosticKind::ReturnTypeMismatch {
                expected: TypeName::Integer,
                found: TypeName::String,
            }]
        );
    }

    #[test]
    fn return_string_where_key_expected_is_accepted() {
        // string -> key is implicit, so this must not be a return-type error.
        assert_eq!(
            kinds("key f() { return \"abc\"; } default { state_entry() { } }"),
            vec![]
        );
    }

    #[test]
    fn missing_return_warns() {
        let diagnostics = kinds("integer f() { llSay(0, \"\"); } default { state_entry() { } }");
        assert_eq!(
            diagnostics,
            vec![DiagnosticKind::MissingReturn {
                function: "f".to_owned(),
                expected: TypeName::Integer,
            }]
        );
    }

    #[test]
    fn missing_return_severity_is_warning() {
        let script = parse("integer f() { } default { state_entry() { } }").script;
        let diagnostics = analyze(&script, &library());
        assert_eq!(
            diagnostics.iter().map(|d| d.severity).collect::<Vec<_>>(),
            vec![Severity::Warning]
        );
    }

    #[test]
    fn if_else_both_return_no_warning() {
        let src = "\
integer f() { if (TRUE) { return 1; } else { return 2; } }
default { state_entry() { } }";
        assert_eq!(kinds(src), vec![]);
    }

    #[test]
    fn while_true_loop_no_warning() {
        let src = "\
integer f() { while (TRUE) { llSay(0, \"spin\"); } }
default { state_entry() { } }";
        assert_eq!(kinds(src), vec![]);
    }

    #[test]
    fn duplicate_function() {
        let src = "foo() { } foo() { } default { state_entry() { } }";
        assert_eq!(
            kinds(src),
            vec![DiagnosticKind::DuplicateFunction {
                name: "foo".to_owned()
            }]
        );
    }

    #[test]
    fn duplicate_global() {
        let src = "integer x; integer x; default { state_entry() { } }";
        assert_eq!(
            kinds(src),
            vec![DiagnosticKind::DuplicateGlobal {
                name: "x".to_owned()
            }]
        );
    }

    #[test]
    fn duplicate_param() {
        let src = "foo(integer a, integer a) { } default { state_entry() { } }";
        assert_eq!(
            kinds(src),
            vec![DiagnosticKind::DuplicateParam {
                name: "a".to_owned()
            }]
        );
    }

    #[test]
    fn duplicate_event() {
        let src = "default { state_entry() { } state_entry() { } }";
        assert_eq!(
            kinds(src),
            vec![DiagnosticKind::DuplicateEvent {
                name: "state_entry".to_owned()
            }]
        );
    }

    #[test]
    fn duplicate_local() {
        let src = "default { state_entry() { integer x; integer x; } }";
        assert_eq!(
            kinds(src),
            vec![DiagnosticKind::DuplicateLocal {
                name: "x".to_owned()
            }]
        );
    }

    #[test]
    fn shadowing_in_nested_block_is_allowed() {
        // A local of the same name in an inner block shadows, not redeclares.
        let src = "default { state_entry() { integer x; { integer x; } } }";
        assert_eq!(kinds(src), vec![]);
    }

    #[test]
    fn duplicate_label() {
        let src = "default { state_entry() { @again; @again; } }";
        assert_eq!(
            kinds(src),
            vec![DiagnosticKind::DuplicateLabel {
                name: "again".to_owned()
            }]
        );
    }

    #[test]
    fn undefined_label_jump() {
        assert_eq!(
            kinds("default { state_entry() { jump missing; } }"),
            vec![DiagnosticKind::UndefinedLabel {
                name: "missing".to_owned()
            }]
        );
    }

    #[test]
    fn valid_jump_to_later_label() {
        // A jump may target a label defined later in the same body.
        assert_eq!(
            kinds("default { state_entry() { jump done; @done; } }"),
            vec![]
        );
    }

    #[test]
    fn assign_to_constant() {
        assert_eq!(
            kinds("default { state_entry() { TRUE = 0; } }"),
            vec![DiagnosticKind::AssignToConstant {
                name: "TRUE".to_owned()
            }]
        );
    }

    #[test]
    fn local_shadowing_constant_may_be_assigned() {
        // A local named `TRUE` shadows the constant, so assigning to it is fine.
        let src = "default { state_entry() { integer TRUE; TRUE = 0; } }";
        assert_eq!(kinds(src), vec![]);
    }

    #[test]
    fn unknown_event() {
        assert_eq!(
            kinds("default { not_an_event() { } state_entry() { } }"),
            vec![DiagnosticKind::UnknownEvent {
                name: "not_an_event".to_owned()
            }]
        );
    }

    #[test]
    fn wrong_event_arity() {
        assert_eq!(
            kinds("default { state_entry(integer x) { } }"),
            vec![DiagnosticKind::WrongEventArgCount {
                event: "state_entry".to_owned(),
                expected: 0,
                found: 1,
            }]
        );
    }

    #[test]
    fn event_arg_type_mismatch() {
        assert_eq!(
            kinds("default { touch_start(string num) { } }"),
            vec![DiagnosticKind::EventArgTypeMismatch {
                event: "touch_start".to_owned(),
                index: 0,
                expected: TypeName::Integer,
                found: TypeName::String,
            }]
        );
    }

    #[test]
    fn empty_library_suppresses_symbol_checks() {
        // With no grid table, an unknown call, unknown variable and unknown
        // event must all stay silent rather than misfire.
        let src = "default { state_entry() { mysteryFunc(mysteryVar); } }";
        assert_eq!(kinds_no_library(src), vec![]);
    }

    #[test]
    fn self_referential_initialiser_is_not_undefined() {
        // `integer x = x;` — the name is in scope for its own initialiser, so
        // it must not be reported undefined (the grid, not us, judges it).
        let src = "default { state_entry() { integer x = x; } }";
        assert_eq!(kinds(src), vec![]);
    }
}
