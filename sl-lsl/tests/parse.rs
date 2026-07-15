//! Integration tests for the `sl-lsl` parser over its public API.
//!
//! Tests return `Result<(), String>` and surface extraction failures with
//! `Err` rather than `panic!`, which the workspace's clippy config denies even
//! in tests.

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use sl_lsl::ast::{
        AssignOp, BinaryOp, Expr, GlobalItem, PostfixOp, PrefixOp, Script, StateName, Stmt,
        TypeName,
    };
    use sl_lsl::parse;

    /// Parse a whole script, asserting it produced no recovered errors.
    fn parse_ok(source: &str) -> Script {
        let result = parse(source);
        assert_eq!(result.errors, vec![], "expected a clean parse, got errors");
        result.script
    }

    /// Parse `src` as a single expression by wrapping it in a global-variable
    /// initialiser, returning the initialiser expression.
    fn expr(src: &str) -> Result<Expr, String> {
        let wrapped = format!("integer x = {src};");
        let script = parse_ok(&wrapped);
        match script.globals.first() {
            Some(GlobalItem::Variable(var)) => var
                .init
                .clone()
                .ok_or_else(|| "expected an initialiser".to_owned()),
            _ => Err("expected a single global variable".to_owned()),
        }
    }

    /// Parse `src` as the single statement in an event handler body, returning
    /// that statement.
    fn stmt(src: &str) -> Result<Stmt, String> {
        let wrapped = format!("default {{ timer() {{ {src} }} }}");
        let script = parse_ok(&wrapped);
        script
            .states
            .first()
            .and_then(|state| state.events.first())
            .and_then(|event| event.body.statements.first())
            .cloned()
            .ok_or_else(|| "expected a single statement".to_owned())
    }

    #[test]
    fn empty_source_is_an_empty_script() {
        let script = parse_ok("");
        assert!(script.globals.is_empty());
        assert!(script.states.is_empty());
        assert_eq!(script.span, 0..0);
    }

    #[test]
    fn realistic_script_top_level_shape() -> Result<(), String> {
        let src = "\
integer counter = 0;
string greeting = \"hi\";

integer add(integer a, integer b)
{
    return a + b;
}

sayHello()
{
    llOwnerSay(greeting);
}

default
{
    state_entry()
    {
        llSay(0, greeting);
    }

    touch_start(integer total)
    {
        ++counter;
        state running;
    }
}

state running
{
    timer()
    {
    }
}
";
        let script = parse_ok(src);
        // Two variables and two functions, in source order.
        assert_eq!(script.globals.len(), 4);
        assert!(matches!(
            script.globals.first(),
            Some(GlobalItem::Variable(_))
        ));
        // `add` has a return type and two typed params; `sayHello` is void.
        let Some(GlobalItem::Function(add)) = script.globals.get(2) else {
            return Err("expected the `add` function".to_owned());
        };
        assert_eq!(add.name.name, "add");
        assert!(add.ret.is_some());
        assert_eq!(add.params.len(), 2);
        assert_eq!(
            add.params.first().map(|p| p.ty.kind),
            Some(TypeName::Integer)
        );
        let Some(GlobalItem::Function(say)) = script.globals.get(3) else {
            return Err("expected the `sayHello` function".to_owned());
        };
        assert_eq!(say.name.name, "sayHello");
        assert!(say.ret.is_none());
        // Two states: default then `running`.
        assert_eq!(script.states.len(), 2);
        assert!(matches!(
            script.states.first().map(|s| &s.name),
            Some(StateName::Default(_))
        ));
        let Some(running) = script.states.get(1) else {
            return Err("expected the `running` state".to_owned());
        };
        let StateName::Named(name) = &running.name else {
            return Err("expected a named state".to_owned());
        };
        assert_eq!(name.name, "running");
        // default has two event handlers.
        assert_eq!(script.states.first().map(|s| s.events.len()), Some(2));
        Ok(())
    }

    #[test]
    fn global_variable_with_and_without_initialiser() -> Result<(), String> {
        let script = parse_ok("integer a; float b = 1.5;");
        assert_eq!(script.globals.len(), 2);
        let Some(GlobalItem::Variable(a)) = script.globals.first() else {
            return Err("expected variable a".to_owned());
        };
        assert_eq!(a.ty.kind, TypeName::Integer);
        assert!(a.init.is_none());
        let Some(GlobalItem::Variable(b)) = script.globals.get(1) else {
            return Err("expected variable b".to_owned());
        };
        assert_eq!(b.ty.kind, TypeName::Float);
        assert!(b.init.is_some());
        Ok(())
    }

    #[test]
    fn arithmetic_precedence_and_left_associativity() -> Result<(), String> {
        // `a + b * c` parses as `a + (b * c)`.
        let Expr::Binary { op, rhs, .. } = expr("a + b * c")? else {
            return Err("expected a binary expression".to_owned());
        };
        assert_eq!(op, BinaryOp::Add);
        assert!(matches!(
            *rhs,
            Expr::Binary {
                op: BinaryOp::Mul,
                ..
            }
        ));

        // `a - b - c` parses as `(a - b) - c` (left-associative).
        let Expr::Binary { op, lhs, .. } = expr("a - b - c")? else {
            return Err("expected a binary expression".to_owned());
        };
        assert_eq!(op, BinaryOp::Sub);
        assert!(matches!(
            *lhs,
            Expr::Binary {
                op: BinaryOp::Sub,
                ..
            }
        ));
        Ok(())
    }

    #[test]
    fn logical_and_or_share_one_left_associative_level() -> Result<(), String> {
        // The LSL quirk: `a || b && c` parses as `(a || b) && c`, because `&&`
        // and `||` share a precedence level and associate left-to-right.
        let Expr::Binary { op, lhs, .. } = expr("a || b && c")? else {
            return Err("expected a binary expression".to_owned());
        };
        assert_eq!(op, BinaryOp::And);
        assert!(matches!(
            *lhs,
            Expr::Binary {
                op: BinaryOp::Or,
                ..
            }
        ));
        Ok(())
    }

    #[test]
    fn assignment_is_right_associative() -> Result<(), String> {
        // `a = b = c` parses as `a = (b = c)`.
        let Expr::Assign { op, value, .. } = expr("a = b = c")? else {
            return Err("expected an assignment".to_owned());
        };
        assert_eq!(op, AssignOp::Assign);
        assert!(matches!(
            *value,
            Expr::Assign {
                op: AssignOp::Assign,
                ..
            }
        ));
        Ok(())
    }

    #[test]
    fn compound_assignment_operators() -> Result<(), String> {
        let Expr::Assign { op, .. } = expr("a += 1")? else {
            return Err("expected an assignment".to_owned());
        };
        assert_eq!(op, AssignOp::AddAssign);
        Ok(())
    }

    #[test]
    fn prefix_and_postfix_operators() -> Result<(), String> {
        assert!(matches!(
            expr("-a")?,
            Expr::Prefix {
                op: PrefixOp::Neg,
                ..
            }
        ));
        assert!(matches!(
            expr("!a")?,
            Expr::Prefix {
                op: PrefixOp::Not,
                ..
            }
        ));
        assert!(matches!(
            expr("~a")?,
            Expr::Prefix {
                op: PrefixOp::BitNot,
                ..
            }
        ));
        assert!(matches!(
            expr("++a")?,
            Expr::Prefix {
                op: PrefixOp::PreInc,
                ..
            }
        ));
        assert!(matches!(
            expr("a--")?,
            Expr::Postfix {
                op: PostfixOp::PostDec,
                ..
            }
        ));
        Ok(())
    }

    #[test]
    fn vector_and_rotation_constructors() -> Result<(), String> {
        assert!(matches!(expr("<1, 2, 3>")?, Expr::Vector { .. }));
        assert!(matches!(expr("<1, 2, 3, 4>")?, Expr::Rotation { .. }));
        // A vector in operand position, then a binary operator on it.
        let Expr::Binary { op, lhs, .. } = expr("<1, 2, 3> * 2")? else {
            return Err("expected a binary expression".to_owned());
        };
        assert_eq!(op, BinaryOp::Mul);
        assert!(matches!(*lhs, Expr::Vector { .. }));
        Ok(())
    }

    #[test]
    fn less_than_in_operator_position_is_relational_not_a_vector() -> Result<(), String> {
        let Expr::Binary { op, .. } = expr("a < b")? else {
            return Err("expected a relational expression".to_owned());
        };
        assert_eq!(op, BinaryOp::Lt);
        Ok(())
    }

    #[test]
    fn parenthesised_comparison_inside_a_vector_component() -> Result<(), String> {
        // Real LSL requires the parentheses; the component then holds a `>`.
        let Expr::Vector { x, .. } = expr("<(a > b), 0, 0>")? else {
            return Err("expected a vector".to_owned());
        };
        let Expr::Paren { inner, .. } = *x else {
            return Err("expected a parenthesised first component".to_owned());
        };
        assert!(matches!(
            *inner,
            Expr::Binary {
                op: BinaryOp::Gt,
                ..
            }
        ));
        Ok(())
    }

    #[test]
    fn list_constructor_and_empty_list() -> Result<(), String> {
        let Expr::List { elements, .. } = expr("[1, 2, \"a\"]")? else {
            return Err("expected a list".to_owned());
        };
        assert_eq!(elements.len(), 3);
        let Expr::List { elements, .. } = expr("[]")? else {
            return Err("expected an empty list".to_owned());
        };
        assert!(elements.is_empty());
        Ok(())
    }

    #[test]
    fn call_member_and_cast() -> Result<(), String> {
        let Expr::Call { callee, args, .. } = expr("llSay(0, \"hi\")")? else {
            return Err("expected a call".to_owned());
        };
        assert_eq!(callee.name, "llSay");
        assert_eq!(args.len(), 2);

        let Expr::Member {
            base, component, ..
        } = expr("v.x")?
        else {
            return Err("expected a member access".to_owned());
        };
        assert_eq!(base.name, "v");
        assert_eq!(component.name, "x");

        let Expr::Cast { ty, operand, .. } = expr("(integer)a")? else {
            return Err("expected a cast".to_owned());
        };
        assert_eq!(ty.kind, TypeName::Integer);
        assert!(matches!(*operand, Expr::Variable(_)));
        Ok(())
    }

    #[test]
    fn cast_binds_tighter_than_a_binary_operator() -> Result<(), String> {
        // `(integer)a + b` is `((integer)a) + b`.
        let Expr::Binary { op, lhs, .. } = expr("(integer)a + b")? else {
            return Err("expected a binary expression".to_owned());
        };
        assert_eq!(op, BinaryOp::Add);
        assert!(matches!(*lhs, Expr::Cast { .. }));
        Ok(())
    }

    #[test]
    fn quaternion_is_a_synonym_for_rotation() -> Result<(), String> {
        let script = parse_ok("quaternion r;");
        let Some(GlobalItem::Variable(var)) = script.globals.first() else {
            return Err("expected a variable".to_owned());
        };
        assert_eq!(var.ty.kind, TypeName::Rotation);
        Ok(())
    }

    #[test]
    fn every_statement_kind_parses() -> Result<(), String> {
        assert!(matches!(stmt(";")?, Stmt::Empty(_)));
        assert!(matches!(stmt("{ }")?, Stmt::Block(_)));
        assert!(matches!(stmt("integer n = 1;")?, Stmt::Local { .. }));
        assert!(matches!(stmt("llSay(0, \"x\");")?, Stmt::Expr { .. }));
        assert!(matches!(stmt("if (a) b(); else c();")?, Stmt::If { .. }));
        assert!(matches!(stmt("while (a) b();")?, Stmt::While { .. }));
        assert!(matches!(stmt("do b(); while (a);")?, Stmt::DoWhile { .. }));
        assert!(matches!(
            stmt("for (i = 0; i < 10; ++i) b();")?,
            Stmt::For { .. }
        ));
        assert!(matches!(stmt("return;")?, Stmt::Return { .. }));
        assert!(matches!(stmt("return 1 + 2;")?, Stmt::Return { .. }));
        assert!(matches!(stmt("jump done;")?, Stmt::Jump { .. }));
        assert!(matches!(stmt("@done;")?, Stmt::Label { .. }));
        assert!(matches!(stmt("state other;")?, Stmt::StateChange { .. }));
        assert!(matches!(stmt("state default;")?, Stmt::StateChange { .. }));
        Ok(())
    }

    #[test]
    fn if_else_binds_else_to_the_nearest_if() -> Result<(), String> {
        let Stmt::If {
            then_branch,
            else_branch,
            ..
        } = stmt("if (a) if (b) c(); else d();")?
        else {
            return Err("expected an if statement".to_owned());
        };
        // The outer `if` has no else; the inner one owns the `else`.
        assert!(else_branch.is_none());
        assert!(matches!(
            *then_branch,
            Stmt::If {
                else_branch: Some(_),
                ..
            }
        ));
        Ok(())
    }

    #[test]
    fn for_loop_clauses_can_be_empty() -> Result<(), String> {
        let Stmt::For {
            init, cond, incr, ..
        } = stmt("for (;;) b();")?
        else {
            return Err("expected a for loop".to_owned());
        };
        assert!(init.is_empty());
        assert!(cond.is_none());
        assert!(incr.is_empty());
        Ok(())
    }

    #[test]
    fn for_loop_multiple_init_and_increment_expressions() -> Result<(), String> {
        let Stmt::For { init, incr, .. } = stmt("for (i = 0, j = 1; i < j; ++i, --j) b();")? else {
            return Err("expected a for loop".to_owned());
        };
        assert_eq!(init.len(), 2);
        assert_eq!(incr.len(), 2);
        Ok(())
    }

    #[test]
    fn literal_expressions_keep_their_raw_text() -> Result<(), String> {
        assert!(matches!(expr("42")?, Expr::Integer { raw, .. } if raw == "42"));
        assert!(matches!(expr("0xFF")?, Expr::Integer { raw, .. } if raw == "0xFF"));
        assert!(matches!(expr("1.5")?, Expr::Float { raw, .. } if raw == "1.5"));
        assert!(matches!(expr("\"hi\"")?, Expr::Str { raw, .. } if raw == "\"hi\""));
        Ok(())
    }

    #[test]
    fn node_spans_slice_back_out_of_the_source() -> Result<(), String> {
        let src = "integer add(integer a) { return a; }";
        let script = parse_ok(src);
        let Some(GlobalItem::Function(func)) = script.globals.first() else {
            return Err("expected a function".to_owned());
        };
        assert_eq!(src.get(func.name.span.clone()), Some("add"));
        let Some(param) = func.params.first() else {
            return Err("expected a parameter".to_owned());
        };
        assert_eq!(src.get(param.name.span.clone()), Some("a"));
        // The whole function span covers the definition.
        assert_eq!(src.get(func.span.clone()), Some(src));
        Ok(())
    }

    #[test]
    fn comments_are_skipped_by_the_parser() {
        let src = "\
// leading comment
integer x = /* inline */ 1; // trailing
";
        let script = parse_ok(src);
        assert_eq!(script.globals.len(), 1);
    }

    // -- error tolerance --------------------------------------------------

    #[test]
    fn missing_semicolon_is_recovered_and_reported() {
        let result = parse("integer x = 5\ninteger y = 6;");
        assert!(result.has_errors());
        // Both variables still parse despite the missing `;`.
        assert_eq!(result.script.globals.len(), 2);
    }

    #[test]
    fn missing_initialiser_leaves_an_error_node_but_keeps_the_variable() -> Result<(), String> {
        let result = parse("integer x = ;");
        assert!(result.has_errors());
        let Some(GlobalItem::Variable(var)) = result.script.globals.first() else {
            return Err("expected a variable".to_owned());
        };
        assert!(matches!(var.init, Some(Expr::Error(_))));
        Ok(())
    }

    #[test]
    fn half_typed_call_survives_and_the_rest_of_the_block_parses() -> Result<(), String> {
        let src = "\
default
{
    timer()
    {
        llSay(0,
        llOwnerSay(\"still here\");
    }
}
";
        let result = parse(src);
        assert!(result.has_errors());
        // The event handler and its (recovered) body still exist.
        let Some(state) = result.script.states.first() else {
            return Err("expected a state".to_owned());
        };
        let Some(event) = state.events.first() else {
            return Err("expected an event handler".to_owned());
        };
        assert!(!event.body.statements.is_empty());
        Ok(())
    }

    #[test]
    fn unclosed_brace_still_yields_the_state_and_reports_the_error() {
        let result = parse("default { timer() { ");
        assert!(result.has_errors());
        assert_eq!(result.script.states.len(), 1);
    }

    #[test]
    fn stray_top_level_tokens_are_reported_but_do_not_swallow_later_items() {
        let result = parse("@#$ integer x = 1;");
        assert!(result.has_errors());
        // The valid global after the junk still parses.
        assert!(
            result
                .script
                .globals
                .iter()
                .any(|g| matches!(g, GlobalItem::Variable(_)))
        );
    }

    #[test]
    fn parser_never_panics_on_operator_soup() {
        // Purely a "does not panic / terminates" check.
        for src in [
            "default{timer(){ a = = = ;}}",
            "integer x = <<<>>>;",
            "((((((",
            "]]]]]]",
            "if if if if",
            "integer integer integer",
            "<1,2,",
            "f(,,,)",
        ] {
            let result = parse(src);
            // The tree is always present; we only assert termination here.
            assert_eq!(result.script.span, 0..src.len());
        }
    }
}
