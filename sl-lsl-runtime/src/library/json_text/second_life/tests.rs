//! Second Life's `llJson*` held to LL's own test script — the wiki's
//! `Json_usage_in_LSL/TestScript`, as LSL PyOptimizer carries it in
//! `unit_tests/json.py` — case by case, with the one case it disagrees with
//! named. The probe `data/json`'s aditi rows are held by the VM replay
//! (`vm/tests/json.rs`).

use pretty_assertions::assert_eq;

use super::*;

/// A string element.
fn s(text: &str) -> Element {
    Element::String(text.to_owned())
}

/// An integer element.
const fn i(value: i32) -> Element {
    Element::Integer(value)
}

/// `test_types`: `llJsonValueType` of documents and along paths.
#[test]
fn types() {
    for (json, expected) in [
        ("\"test\"", JSON_STRING),
        ("test", JSON_INVALID),
        ("12", JSON_NUMBER),
        ("12.300000", JSON_NUMBER),
        ("Inf", JSON_INVALID),
        ("NaN", JSON_INVALID),
        ("-123.4e-5", JSON_NUMBER),
        ("{\"a\":\"b\"}", JSON_OBJECT),
        // Not checked inside: an object, whatever its keys and values.
        ("{a:\"b\"}", JSON_OBJECT),
        ("{\"a\":b}", JSON_OBJECT),
        ("[1,2,3]", JSON_ARRAY),
        (" [1,2,3]", JSON_ARRAY),
        ("[1,2,3] ", JSON_ARRAY),
        (" [1,2,3] ", JSON_ARRAY),
        ("true", JSON_TRUE),
        ("false", JSON_FALSE),
        ("null", JSON_NULL),
    ] {
        assert_eq!(value_type(json, &[]), expected, "{json}");
    }
    let json = "[[1,2,3],{\"a\":3,\"b\":[true,\"test\",6],\"c\":\"true\",\"d\":false}]";
    for (path, expected) in [
        (vec![i(0)], JSON_ARRAY),
        (vec![i(0), i(1)], JSON_NUMBER),
        (vec![i(1)], JSON_OBJECT),
        (vec![i(1), s("b")], JSON_ARRAY),
        (vec![i(1), s("b"), i(0)], JSON_TRUE),
        (vec![i(1), s("b"), i(1)], JSON_STRING),
        (vec![i(1), s("b"), i(2)], JSON_NUMBER),
        (vec![i(1), s("c")], JSON_STRING),
        (vec![i(1), s("d")], JSON_FALSE),
        (vec![i(3)], JSON_INVALID),
        (vec![i(-1)], JSON_INVALID),
        // MAINT-2670.
        (vec![i(1), s("c"), i(3)], JSON_INVALID),
        (vec![i(1), s("c"), i(0)], JSON_INVALID),
        (vec![i(1), s("b"), i(3)], JSON_INVALID),
        (vec![i(1), s("b"), i(2), i(0), i(1)], JSON_INVALID),
    ] {
        assert_eq!(value_type(json, &path), expected, "{path:?}");
    }
    // Unquoted keys are invalid on the level the walk reads.
    let json = "[[1,2,3],{a:3,b:[true,\"test\",6],c:\"true\",\"d\":false}]";
    for key in ["a", "b", "c"] {
        assert_eq!(value_type(json, &[i(1), s(key)]), JSON_INVALID, "{key}");
    }
}

/// `test_get_value`.
#[test]
fn get_values() {
    let json = "[[1,2,3,4.0],{\"a\":3,\"b\":[true,\"test\",6]}]";
    for (path, expected) in [
        (vec![i(0)], "[1,2,3,4.0]"),
        (vec![i(0), i(1)], "2"),
        (vec![i(1)], "{\"a\":3,\"b\":[true,\"test\",6]}"),
        (vec![i(1), s("b")], "[true,\"test\",6]"),
        (vec![i(1), s("b"), i(0)], JSON_TRUE),
        (vec![i(1), s("b"), i(1)], "test"),
        (vec![i(1), s("b"), i(2)], "6"),
        (vec![i(0), i(3)], "4.0"),
        (vec![i(2)], JSON_INVALID),
        (vec![i(-1)], JSON_INVALID),
        (vec![i(0), i(4)], JSON_INVALID),
        // MAINT-2671.
        (vec![s("f")], JSON_INVALID),
        (vec![i(0), s("f")], JSON_INVALID),
        (vec![i(1), i(2)], JSON_INVALID),
    ] {
        assert_eq!(get_value(json, &path), expected, "{path:?}");
    }
    let json = "[[1,2,3,4.0],{a:3,\"b\":[true,test,6]}]";
    assert_eq!(get_value(json, &[i(1), s("b"), i(1)]), JSON_INVALID);
    assert_eq!(get_value(json, &[i(1), s("a")]), JSON_INVALID);
}

/// `test_set_value`: building, replacing, appending and deleting.
#[test]
fn set_values() {
    let mut json = String::new();
    for (path, value, expected) in [
        (vec![i(0), i(0)], "1", "[[1]]"),
        (vec![i(0), i(1)], "2", "[[1,2]]"),
        (vec![i(0), i(2)], "3", "[[1,2,3]]"),
        (vec![i(1), s("a")], "3", "[[1,2,3],{\"a\":3}]"),
        (
            vec![i(1), s("b"), i(0)],
            JSON_TRUE,
            "[[1,2,3],{\"a\":3,\"b\":[true]}]",
        ),
        (
            vec![i(1), s("b"), i(1)],
            "test",
            "[[1,2,3],{\"a\":3,\"b\":[true,\"test\"]}]",
        ),
        (
            vec![i(1), s("b"), i(2)],
            "6",
            "[[1,2,3],{\"a\":3,\"b\":[true,\"test\",6]}]",
        ),
        (
            vec![i(1), s("b"), i(1)],
            "foo",
            "[[1,2,3],{\"a\":3,\"b\":[true,\"foo\",6]}]",
        ),
        (
            vec![i(1), s("b")],
            JSON_TRUE,
            "[[1,2,3],{\"a\":3,\"b\":true}]",
        ),
        (vec![i(1), s("b")], "true", "[[1,2,3],{\"a\":3,\"b\":true}]"),
        (vec![i(1), i(0), i(0)], JSON_FALSE, "[[1,2,3],[[false]]]"),
    ] {
        json = set_value(&json, &path, value);
        assert_eq!(json, expected, "{path:?} {value}");
    }
    // Appending.
    let json = set_value(
        "[[1,2,3],{\"a\":3,\"b\":[true,\"test\",6]}]",
        &[i(0), i(-1)],
        "4.0",
    );
    assert_eq!(json, "[[1,2,3,4.0],{\"a\":3,\"b\":[true,\"test\",6]}]");
    let json = set_value(&json, &[i(1), s("b"), i(-1)], "5.0");
    assert_eq!(json, "[[1,2,3,4.0],{\"a\":3,\"b\":[true,\"test\",6,5.0]}]");
    let json = set_value(&json, &[i(-1)], "6.0");
    assert_eq!(
        json,
        "[[1,2,3,4.0],{\"a\":3,\"b\":[true,\"test\",6,5.0]},6.0]"
    );
    // MAINT-2684.
    assert_eq!(set_value("[]", &[i(-1)], "\"alone\""), "[\"alone\"]");
    assert_eq!(set_value("[]", &[i(1)], "\"alone\""), JSON_INVALID);
    assert_eq!(set_value("[]", &[i(0)], "\"alone\""), "[\"alone\"]");
    // Deleting.
    let mut json = "[[1,2,3],{\"a\":3,\"b\":[true,\"test\",6,null]}]".to_owned();
    for (path, expected) in [
        (
            vec![i(1), s("b"), i(1)],
            "[[1,2,3],{\"a\":3,\"b\":[true,6,null]}]",
        ),
        (
            vec![i(1), s("b"), i(2)],
            "[[1,2,3],{\"a\":3,\"b\":[true,6]}]",
        ),
        (vec![i(1), s("b")], "[[1,2,3],{\"a\":3}]"),
        (vec![i(1)], "[[1,2,3]]"),
    ] {
        json = set_value(&json, &path, JSON_DELETE);
        assert_eq!(json, expected, "{path:?}");
    }
    let json = set_value("[[1,2,3],4]", &[i(0)], JSON_DELETE);
    assert_eq!(json, "[4]");
    assert_eq!(set_value(&json, &[i(0)], JSON_DELETE), "[]");
    let json = set_value("[[1]]", &[i(0), i(0)], JSON_DELETE);
    assert_eq!(json, "[[]]");
    assert_eq!(set_value(&json, &[i(0)], JSON_DELETE), "[]");
    // What cannot be deleted.
    let json = "[[1,2,3],{\"a\":3,\"b\":[true,\"test\",6,null]}]";
    for path in [
        vec![i(1), s("d")],
        vec![i(2)],
        vec![i(1), s("a"), s("unicorn")],
        vec![i(0), i(1), i(1)],
    ] {
        assert_eq!(
            set_value(json, &path, JSON_DELETE),
            JSON_INVALID,
            "{path:?}"
        );
    }
    // MAINT-2675: the one failure of a write.
    assert_eq!(
        set_value(
            "[[1,2,3],{\"a\":3,\"b\":[true,\"foo\",6]}]",
            &[i(3)],
            JSON_FALSE
        ),
        JSON_INVALID
    );
}

/// `test_json_to_list`.
#[test]
fn json_to_list() {
    let list = json2_list("[[1,2,3],{\"a\":3,\"b\":[true,\"test\",6]}]");
    assert_eq!(
        list,
        vec![s("[1,2,3]"), s("{\"a\":3,\"b\":[true,\"test\",6]}")]
    );
    assert_eq!(json2_list("[1,2,3]"), vec![i(1), i(2), i(3)]);
    let object = json2_list("{\"a\":3,\"b\":[true,\"test\",6]}");
    assert_eq!(object, vec![s("a"), i(3), s("b"), s("[true,\"test\",6]")]);
    assert_eq!(
        json2_list("[true,\"test\",6]"),
        vec![s(JSON_TRUE), s("test"), i(6)]
    );
    assert_eq!(json2_list("test"), vec![s("test")]);
    assert_eq!(json2_list(""), Vec::<Element>::new());
    // MAINT-2678.
    assert_eq!(json2_list("[]"), Vec::<Element>::new());
    assert_eq!(json2_list("{}"), Vec::<Element>::new());
    assert_eq!(
        json2_list("Non-JSON string, with comma"),
        vec![s("Non-JSON string, with comma")]
    );
    assert_eq!(json2_list("[malformed}"), vec![s("[malformed}")]);
}

/// `test_list_to_json`.
#[test]
fn list_to_json() {
    let pairs = [
        s("a"),
        i(1),
        s("b"),
        Element::Float(2.5),
        s("c"),
        s("test"),
        s("d"),
        s("true"),
        s("e"),
        s("[1,2,3]"),
    ];
    assert_eq!(
        list2_json(JSON_OBJECT, &pairs),
        "{\"a\":1,\"b\":2.500000,\"c\":\"test\",\"d\":true,\"e\":[1,2,3]}"
    );
    let values = [
        i(1),
        Element::Float(2.5),
        s("test"),
        s("true"),
        s("[1,2,3]"),
    ];
    assert_eq!(
        list2_json(JSON_ARRAY, &values),
        "[1,2.500000,\"test\",true,[1,2,3]]"
    );
    let values = [
        i(1),
        Element::Float(2.5),
        s("test"),
        s(JSON_TRUE),
        s("[1,2,3]"),
    ];
    assert_eq!(
        list2_json(JSON_ARRAY, &values),
        "[1,2.500000,\"test\",true,[1,2,3]]"
    );
    // MAINT-2681.
    assert_eq!(list2_json(JSON_OBJECT, &[]), "{}");
    assert_eq!(list2_json(JSON_ARRAY, &[]), "[]");
    let truncated = pairs.get(..9).unwrap_or_default();
    assert_eq!(list2_json(JSON_OBJECT, truncated), JSON_INVALID);
    let mut keyed = pairs.to_vec();
    if let Some(key) = keyed.get_mut(2) {
        *key = i(1);
    }
    assert_eq!(list2_json(JSON_OBJECT, &keyed), JSON_INVALID);
    assert_eq!(list2_json("foo", &pairs), JSON_INVALID);
}

/// `test_strings_with_escaped_chars` (MAINT-2698): every string that needs
/// an escape survives each function, in and out.
#[test]
fn strings_with_escaped_characters() {
    for (funky, escaped) in [
        ("funky\"string", "funky\\\"string"),
        ("funkystr\"ing", "funkystr\\\"ing"),
        ("funky\\string", "funky\\\\string"),
        ("\\funkystring", "\\\\funkystring"),
        ("funky\nstring", "funky\\nstring"),
        ("funky/string", "funky\\/string"),
        ("funky\tstring", "funky\\tstring"),
        ("funky\u{08}string", "funky\\bstring"),
        ("funky\u{0C}string", "funky\\fstring"),
        ("funky\rstring", "funky\\rstring"),
        ("\"funkystring", "\\\"funkystring"),
        ("vanilla string", "vanilla string"),
    ] {
        assert_eq!(value_type(&format!("\"{escaped}\""), &[]), JSON_STRING);
        let json = format!("[[1,2,3,4.0],{{\"{escaped}\":3,\"b\":\"{escaped}value\"}}]");
        assert_eq!(get_value(&json, &[i(1), s(funky)]), "3", "{escaped}");
        assert_eq!(
            get_value(&json, &[i(1), s("b")]),
            format!("{funky}value"),
            "{escaped}"
        );
        let object = format!("{{\"{escaped}\":3,\"b\":\"{escaped}value\"}}");
        let json = set_value(&json, &[i(0)], funky);
        assert_eq!(json, format!("[\"{escaped}\",{object}]"));
        let json = set_value(&json, &[i(0)], funky);
        assert_eq!(json, format!("[\"{escaped}\",{object}]"));
        let json = set_value(&json, &[i(0), s(funky)], &format!("{funky}value"));
        let pair = format!("{{\"{escaped}\":\"{escaped}value\"}}");
        assert_eq!(json, format!("[{pair},{object}]"));
        assert_eq!(json2_list(&json), vec![s(&pair), s(&object)]);
        let list = json2_list(&pair);
        assert_eq!(list, vec![s(funky), s(&format!("{funky}value"))]);
        assert_eq!(
            list2_json(JSON_ARRAY, &list),
            format!("[\"{escaped}\",\"{escaped}value\"]")
        );
        assert_eq!(list2_json(JSON_OBJECT, &list), pair);
    }
}

/// MAINT-3070, MAINT-3053, MAINT-3081 and MAINT-4187.
#[test]
fn jira_fixes() {
    // MAINT-3070: brackets inside a string are no array.
    assert_eq!(
        set_value("", &[s("toast")], "messa[g]e"),
        "{\"toast\":\"messa[g]e\"}"
    );
    assert_eq!(
        set_value("", &[s("toast")], "label[x]"),
        "{\"toast\":\"label[x]\"}"
    );
    assert_eq!(
        set_value("", &[s("toast")], "label\\[x\\]"),
        "{\"toast\":\"label\\\\[x\\\\]\"}"
    );
    // MAINT-3053.
    let array = "[1, 2]";
    assert_eq!(set_value(array, &[i(2)], "t"), "[1,2,\"t\"]");
    assert_eq!(set_value(array, &[i(3)], "t"), JSON_INVALID);
    assert_eq!(set_value(array, &[i(0), i(0)], "t"), "[[\"t\"],2]");
    let deep = [i(0), i(0), i(2), s("t"), i(75)];
    assert_eq!(set_value(array, &deep, "t"), JSON_INVALID);
    assert_eq!(set_value(array, &[i(0), i(1)], "t"), JSON_INVALID);
    let deep = [i(0), i(1), i(2), s("t"), i(75)];
    assert_eq!(set_value(array, &deep, "t"), JSON_INVALID);
    let nested = "[ [\"A\", \"B\", \"C\"], 2]";
    assert_eq!(
        set_value(nested, &[i(0), i(3)], "t"),
        "[[\"A\",\"B\",\"C\",\"t\"],2]"
    );
    assert_eq!(set_value(nested, &[i(0), i(4)], "t"), JSON_INVALID);
    assert_eq!(
        set_value(nested, &[i(0), i(1), i(0)], "t"),
        "[[\"A\",[\"t\"],\"C\"],2]"
    );
    assert_eq!(set_value(nested, &[i(0), i(1), i(1)], "t"), JSON_INVALID);
    let keyed = "{\"1\":2}";
    assert_eq!(set_value(keyed, &[s("1")], "t"), "{\"1\":\"t\"}");
    assert_eq!(set_value(keyed, &[s("1"), i(0)], "t"), "{\"1\":[\"t\"]}");
    assert_eq!(set_value(keyed, &[s("1"), i(1)], "t"), JSON_INVALID);
    assert_eq!(value_type("[null, 2]", &[i(0)]), JSON_NULL);
    assert_eq!(value_type("[null, 2]", &[i(0), i(0)]), JSON_INVALID);
    assert_eq!(value_type("[, 2]", &[i(0)]), JSON_INVALID);
    let deep = [i(0), i(0), i(2), s("t"), i(75)];
    assert_eq!(value_type("[, 2]", &deep), JSON_INVALID);
    let deep = [i(1), i(0), i(2), s("t"), i(75)];
    assert_eq!(get_value("[, 2]", &deep), JSON_INVALID);
    // MAINT-3081.
    assert_eq!(set_value("", &[s("test")], ""), "{\"test\":\"\"}");
    assert_eq!(set_value("", &[s("test")], JSON_NULL), "{\"test\":null}");
    assert_eq!(get_value("{\"test\":\"\"}", &[s("test")]), "");
    assert_eq!(get_value("{\"test\":null}", &[s("test")]), JSON_NULL);
    let set = set_value("", &[s("test")], "");
    assert_eq!(get_value(&set, &[s("test")]), "");
    let set = set_value("", &[s("test")], JSON_NULL);
    assert_eq!(get_value(&set, &[s("test")]), JSON_NULL);
    // MAINT-4187, whose numbers PyOptimizer tests only with BUG-6466 fixed;
    // aditi refuses a needless zero (`data/json`'s `leading-zero`), as the
    // fix does.
    for (json, expected) in [
        ("1.0e-1", JSON_NUMBER),
        ("-1.0e-1", JSON_NUMBER),
        ("1.0e1", JSON_NUMBER),
        ("+1.0e1", JSON_INVALID),
        ("e1", JSON_INVALID),
        ("01", JSON_INVALID),
        ("-01", JSON_INVALID),
        ("0.01", JSON_NUMBER),
        ("-0.01", JSON_NUMBER),
    ] {
        assert_eq!(value_type(json, &[]), expected, "{json}");
    }
    // The one deviation: with BUG-6466 fixed `1.0e+1` is a number. Today's
    // grid has it back (PyOptimizer: "they've fixed BUG-6657 by bringing
    // BUG-6466 back to life"), so an exponent takes no `+` here.
    assert_eq!(value_type("1.0e+1", &[]), JSON_INVALID);
}

/// The scanner's own rules: where a string ends, what a number is, and that
/// a word ends at `:`.
#[test]
fn the_scanner_reads_as_aditi_does() {
    // A string ends at the first quote not preceded by a backslash.
    assert_eq!(
        json2_list("[\"\\u0041\\n\\/\\\"\\\\\"]"),
        vec![s("\"\\u0041\\n\\/\\\"\\\\\"]")]
    );
    assert_eq!(json2_list("[\"\\ud83d\\ude00\"]"), vec![s("ud83dude00")]);
    assert_eq!(json2_list("[01]"), vec![s("01")]);
    assert_eq!(json2_list("[1.]"), vec![Element::Float(1.0)]);
    assert_eq!(json2_list("[.5]"), vec![Element::Float(0.5)]);
    assert_eq!(
        json2_list("[1e5,1E-2,-0,2.50]"),
        vec![
            Element::Float(100_000.0),
            Element::Float(0.01),
            i(0),
            Element::Float(2.5)
        ]
    );
    assert_eq!(
        json2_list("[12345678901234567890,2147483648,-99999999999]"),
        vec![i(i32::MAX), i(i32::MAX), i(i32::MIN)]
    );
    assert_eq!(json2_list("{'a':1}"), vec![s(JSON_INVALID)]);
    assert_eq!(json2_list("[\"a\" \"b\"]"), vec![s("b")]);
    assert_eq!(json2_list("[1,]"), vec![i(1), s("")]);
    assert_eq!(json2_list("{\"a\"}"), vec![s(""), s("a")]);
    assert_eq!(json2_list("[truth]"), vec![s("truth")]);
}
