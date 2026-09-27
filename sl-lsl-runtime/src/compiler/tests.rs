//! The compiler's output pinned by its disassembly, and its errors by kind
//! and position.

use pretty_assertions::assert_eq;

use super::{CompileErrorKind as Kind, compile};

/// The disassembly of `source`, or the rendered errors.
fn disassemble(source: &str) -> String {
    match compile(source) {
        Ok(program) => program.to_string(),
        Err(errors) => errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

/// `body` as the only statements of `default`'s `state_entry`, with
/// `globals` before it.
fn script(globals: &str, body: &str) -> String {
    format!("{globals}\ndefault\n{{\n    state_entry()\n    {{\n{body}\n    }}\n}}\n")
}

/// The errors of `source` as (kind, one-based line, one-based column), or
/// an empty list when it compiles.
fn errors(source: &str) -> Vec<(Kind, u32, u32)> {
    compile(source).err().map_or_else(Vec::new, |errors| {
        errors
            .iter()
            .map(|error| (error.kind, error.position.line, error.position.column))
            .collect()
    })
}

/// The kinds of the errors of `body` in `state_entry`, with `globals`.
fn kinds(globals: &str, body: &str) -> Vec<Kind> {
    errors(&script(globals, body))
        .into_iter()
        .map(|(kind, _, _)| kind)
        .collect()
}

#[test]
fn binary_operands_are_evaluated_right_to_left() {
    assert_eq!(
        disassemble(&script(
            "integer a; integer b; integer c;",
            "        a - b + c;"
        )),
        "\
global integer a
global integer b
global integer c
state default
 event state_entry()
     0 6:17    LoadGlobal c
     1 6:13    LoadGlobal b
     2 6:9     LoadGlobal a
     3 6:9     Binary Sub
     4 6:9     Binary Add
     5 6:9     Pop
     6 7:5     Return
"
    );
}

#[test]
fn implicit_conversions_are_explicit_casts() {
    let body = "        float f = 1 + 2.5;
        f = 2.5 + 1;
        key k = \"x\";
        llSetTimerEvent(5);";
    assert_eq!(
        disassemble(&script("", body)),
        "\
state default
 event state_entry()
  local float f
  local key k
     0 6:23    Const 2.500000
     1 6:19    Const 1
     2 6:19    Cast float
     3 6:19    Binary Add
     4 6:15    StoreLocal f
     5 7:19    Const 1
     6 7:19    Cast float
     7 7:13    Const 2.500000
     8 7:13    Binary Add
     9 7:9     StoreLocal f
    10 8:17    Const \"x\"
    11 8:17    Cast key
    12 8:13    StoreLocal k
    13 9:25    Const 5
    14 9:25    Cast float
    15 9:9     CallBuiltin llSetTimerEvent
    16 10:5    Return
"
    );
}

#[test]
fn integer_times_assign_float_truncates_before_it_stores() {
    // Measured on aditi: `integer i = 3; i *= 1.5;` leaves `i` at 4.
    assert_eq!(
        disassemble(&script("", "        integer i = 3;\n        i *= 1.5;")),
        "\
state default
 event state_entry()
  local integer i
     0 6:21    Const 3
     1 6:17    StoreLocal i
     2 7:14    Const 1.500000
     3 7:9     LoadLocal i
     4 7:9     Cast float
     5 7:9     Binary Mul
     6 7:9     Cast integer
     7 7:9     StoreLocal i
     8 8:5     Return
"
    );
}

#[test]
fn using_an_integer_times_assign_floats_value_faults_the_whole_body() {
    // Measured on aditi: the grid compiles this, then the event fails with
    // `System.InvalidProgramException` before its first line runs.
    let body = "        llOwnerSay(\"never\");
        integer i = 2;
        if (i) i = 3;
        float g = i *= 1.5;";
    assert_eq!(
        disassemble(&script("", body)),
        "\
state default
 event state_entry()
  local integer i
  local float g
     0 9:19    InvalidProgram
     1 6:20    Const \"never\"
     2 6:9     CallBuiltin llOwnerSay
     3 7:21    Const 2
     4 7:17    StoreLocal i
     5 8:13    LoadLocal i
     6 8:9     JumpIfFalse 9
     7 8:20    Const 3
     8 8:16    StoreLocal i
     9 9:24    Const 1.500000
    10 9:19    LoadLocal i
    11 9:19    Cast float
    12 9:19    Binary Mul
    13 9:19    Cast integer
    14 9:19    Dup
    15 9:19    StoreLocal i
    16 9:19    Cast float
    17 9:15    StoreLocal g
    18 10:5    Return
"
    );
}

#[test]
fn steps_and_members_load_and_store_their_variable() {
    let body = "        vector v;
        v.y += 2;
        float f = v.z++;
        float h = --f;";
    assert_eq!(
        disassemble(&script("", body)),
        "\
state default
 event state_entry()
  local vector v
  local float f
  local float h
     0 6:16    Const <0.000000, 0.000000, 0.000000>
     1 6:16    StoreLocal v
     2 7:16    Const 2
     3 7:16    Cast float
     4 7:9     LoadLocal v
     5 7:9     GetMember .y
     6 7:9     Binary Add
     7 7:9     LoadLocal v
     8 7:9     SetMember .y
     9 7:9     StoreLocal v
    10 8:19    LoadLocal v
    11 8:19    GetMember .z
    12 8:19    Dup
    13 8:19    Prefix PreInc
    14 8:19    LoadLocal v
    15 8:19    SetMember .z
    16 8:19    StoreLocal v
    17 8:15    StoreLocal f
    18 9:19    LoadLocal f
    19 9:19    Prefix PreDec
    20 9:19    Dup
    21 9:19    StoreLocal f
    22 9:15    StoreLocal h
    23 10:5    Return
"
    );
}

#[test]
fn loops_and_jumps_target_instructions_in_their_body() {
    let body = "        integer i;
        while (i < 3) ++i;
        do --i; while (i);
        for (;;) jump out;
        @out;";
    assert_eq!(
        disassemble(&script("", body)),
        "\
state default
 event state_entry()
  local integer i
     0 6:17    Const 0
     1 6:17    StoreLocal i
     2 7:20    Const 3
     3 7:16    LoadLocal i
     4 7:16    Binary Lt
     5 7:9     JumpIfFalse 10
     6 7:23    LoadLocal i
     7 7:23    Prefix PreInc
     8 7:23    StoreLocal i
     9 7:9     Jump 2
    10 8:12    LoadLocal i
    11 8:12    Prefix PreDec
    12 8:12    StoreLocal i
    13 8:24    LoadLocal i
    14 8:9     JumpIfTrue 10
    15 9:18    Jump 17
    16 9:9     Jump 15
    17 11:5    Return
"
    );
}

#[test]
fn a_jump_lands_on_the_last_label_of_its_name() {
    // The inner `@done` is the one in scope at the `jump`; the reference
    // jumps to the outer one, the last in the function.
    let body = "        if (TRUE) { jump done; @done; llOwnerSay(\"inner\"); }
        @done;";
    assert_eq!(
        disassemble(&script("", body)),
        "\
state default
 event state_entry()
     0 6:13    Const 1
     1 6:9     JumpIfFalse 5
     2 6:21    Jump 5
     3 6:50    Const \"inner\"
     4 6:39    CallBuiltin llOwnerSay
     5 8:5     Return
"
    );
}

#[test]
fn global_initialisers_run_in_order_in_the_init_body() {
    let globals = "float p = -PI; integer x = -2147483648; list l = [1, x]; string s;";
    assert_eq!(
        disassemble(&script(globals, "        s = (string)l;")),
        "\
global float p
global integer x
global list l
global string s
init
     0 1:12    Const 3.141593
     1 1:11    Prefix Neg
     2 1:1     StoreGlobal p
     3 1:28    Const -2147483648
     4 1:16    StoreGlobal x
     5 1:51    Const 1
     6 1:54    LoadGlobal x
     7 1:50    BuildList 2
     8 1:41    StoreGlobal l
     9 1:1     Return
state default
 event state_entry()
     0 6:21    LoadGlobal l
     1 6:13    Cast string
     2 6:9     StoreGlobal s
     3 7:5     Return
"
    );
}

#[test]
fn functions_and_state_changes() {
    let globals = "integer f(integer n) { if (n) return n; else return -n; }
h(float x) { if (x > 0) state two; }";
    let body = "        h(f(3));
    }
    touch_start(integer n)
    {
        state default;
    }
}
state two
{
    state_entry()
    {
        return;";
    assert_eq!(
        disassemble(&script(globals, body)),
        "\
function f(integer n) -> integer
  param integer n
     0 1:28    LoadLocal n
     1 1:24    JumpIfFalse 5
     2 1:38    LoadLocal n
     3 1:31    ReturnValue
     4 1:24    Jump 8
     5 1:54    LoadLocal n
     6 1:53    Prefix Neg
     7 1:46    ReturnValue
     8 1:57    Const 0
     9 1:57    ReturnValue
function h(float x)
  param float x
     0 2:22    Const 0
     1 2:22    Cast float
     2 2:18    LoadLocal x
     3 2:18    Binary Gt
     4 2:14    JumpIfFalse 6
     5 2:25    StateChange two
     6 2:36    Return
state default
 event state_entry()
     0 7:13    Const 3
     1 7:11    CallFunction f
     2 7:11    Cast float
     3 7:9     CallFunction h
     4 8:5     Return
 event touch_start(integer n)
  param integer n
     0 11:9    StateChange default
state two
 event state_entry()
     0 18:9    Return
"
    );
}

#[test]
fn an_error_is_rendered_as_the_grid_sends_it() {
    let source = script("", "        string s = [1];");
    assert_eq!(errors(&source), vec![(Kind::TypeMismatch, 6, 20)]);
    assert_eq!(disassemble(&source), "(5, 19): ERROR : Type mismatch");
}

#[test]
fn a_missing_return_is_reported_at_the_closing_brace() {
    // Second Life, measured on aditi: `(4, 0): ERROR : Not all code paths
    // return a value` for this function.
    let source = "integer g()\n{\n    while (TRUE)\n        return 1;\n}\n\ndefault\n{\n    state_entry()\n    {\n        g();\n    }\n}\n";
    assert_eq!(
        disassemble(source),
        "(4, 0): ERROR : Not all code paths return a value"
    );
}

#[test]
fn only_the_first_syntax_error_is_reported() {
    assert_eq!(
        errors(&script(
            "",
            "        llSay(0 \"a\");\n        llSay(0 \"b\");"
        )),
        vec![(Kind::Syntax, 6, 17)]
    );
}

#[test]
fn what_the_semantic_pass_misses_the_lowering_rejects() {
    let cases: &[(&str, &str, Kind)] = &[
        // operator, assignment, cast and condition typing
        ("", "        string s = \"a\" | \"b\";", Kind::TypeMismatch),
        ("", "        integer i; i += 1.0;", Kind::TypeMismatch),
        ("", "        vector v; v *= v;", Kind::TypeMismatch),
        ("", "        integer i = \"1\";", Kind::TypeMismatch),
        ("", "        key k = (key)1;", Kind::TypeMismatch),
        ("", "        if (llSleep(1.0)) ;", Kind::TypeMismatch),
        ("", "        integer i = llSleep(1.0);", Kind::TypeMismatch),
        ("", "        print(llSleep(1.0));", Kind::TypeMismatch),
        ("", "        list l = [llSleep(1.0)];", Kind::TypeMismatch),
        ("", "        vector v = <1, \"2\", 3>;", Kind::TypeMismatch),
        ("", "        llOwnerSay(1 + 1);", Kind::FunctionMismatch),
        // lists, members, lvalues
        ("", "        list a; list b = [a];", Kind::ListInList),
        ("", "        vector v; float f = v.j;", Kind::InvalidMember),
        ("", "        vector v; v.s = 1;", Kind::InvalidMember),
        ("", "        integer i; float f = i.x;", Kind::InvalidMember),
        ("", "        float f = ZERO_VECTOR.x;", Kind::InvalidMember),
        ("", "        integer i; (i) = 1;", Kind::Syntax),
        ("", "        integer i; ++(i + 1);", Kind::Syntax),
        // scopes and names
        (
            "",
            "        if (TRUE) integer i;",
            Kind::DeclarationNeedsScope,
        ),
        ("", "        integer x = x;", Kind::Undefined),
        ("", "        { @inner; } jump inner;", Kind::Undefined),
        ("", "        @a; @a;", Kind::AlreadyDefined),
        ("", "        integer a; @a;", Kind::AlreadyDefined),
        ("", "        integer touch_start;", Kind::Syntax),
        ("integer llSay;", "", Kind::AlreadyDefined),
        ("integer f; f() {}", "", Kind::AlreadyDefined),
        ("integer a = b; integer b = 1;", "", Kind::Undefined),
        // global initialisers are constants
        ("integer g = llAbs(1);", "", Kind::Syntax),
        ("integer a = 1; integer b = -a;", "", Kind::Syntax),
        ("integer t = -TRUE;", "", Kind::Syntax),
        ("integer n = (integer)1.5;", "", Kind::Syntax),
        // returns and state changes
        (
            "integer f() { while (TRUE) return 1; }",
            "",
            Kind::MissingReturn,
        ),
        ("integer f() { return 1; ; }", "", Kind::MissingReturn),
        (
            "integer f() { if (TRUE) return 1; }",
            "",
            Kind::MissingReturn,
        ),
        ("f() { state default; }", "", Kind::StateChangeInFunction),
    ];
    for (globals, body, expected) in cases {
        assert_eq!(kinds(globals, body), vec![*expected], "{globals} {body}");
    }
}

#[test]
fn what_the_grid_accepts_compiles() {
    let cases: &[(&str, &str)] = &[
        ("", "        integer llSay = 1;"),
        ("integer x = 1;", "        integer x = x;"),
        (
            "float p = -PI; vector v = <1, 2, -3.5>; list l = [TRUE, NULL_KEY, <0, 0, 0, 1>];",
            "",
        ),
        ("integer f() { if (TRUE) { return 1; } else return 2; }", ""),
        ("f() { if (TRUE) state default; }", ""),
        ("", "        if (TRUE) return llSleep(1.0);"),
        ("", "        integer i; i *= 2.5;"),
        ("", "        { integer a; } { integer a; }"),
        ("", "        string s = (key)\"k\"; key k = s;"),
        ("", "        list l = [] + 1 + [2.0] + \"s\";"),
    ];
    for (globals, body) in cases {
        assert_eq!(kinds(globals, body), Vec::<Kind>::new(), "{globals} {body}");
    }
}

#[test]
fn a_misplaced_or_missing_default_state_is_a_syntax_error() {
    assert_eq!(
        errors("state other\n{\n    state_entry() {}\n}\n"),
        vec![(Kind::Syntax, 1, 1)]
    );
    assert_eq!(
        errors("state other\n{\n    state_entry() {}\n}\ndefault\n{\n    state_entry() {}\n}\n"),
        vec![(Kind::Syntax, 1, 1)]
    );
    assert_eq!(
        errors("default\n{\n    state_entry() {}\n}\nstate empty\n{\n}\n"),
        vec![(Kind::Syntax, 5, 7)]
    );
}
