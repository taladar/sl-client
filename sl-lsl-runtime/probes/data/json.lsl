// probe: data/json
// drive: none
//
// What the llJson* functions make of lists and JSON text on each grid. The
// script builds JSON with llList2Json from lists of every LSL type, of strings
// that look like JSON, numbers, escapes and non-ASCII text, and from the
// object lists that have no JSON (an odd count, a key that is not a string);
// reads JSON back with llJson2List, valid and broken, printing each element's
// LSL type; reads values with llJsonGetValue and llJsonValueType along paths
// that exist, run past an array, mix keys and indices or are empty; and
// writes values with llJsonSetValue: new keys, nested paths, the end of an
// array and past it, JSON_APPEND, JSON_DELETE, the sentinels as values and
// into broken JSON. A sentinel is printed by name (TRUE, INVALID, …), never
// as its character; a value with control or non-ASCII characters is printed
// URL-escaped as well (`<field>_esc`). First it prints what llSubStringIndex,
// llParseStringKeepNulls, == and llListFindList make of a sentinel inside
// "abc" (`sentinel-search`); last, whether the llJson* functions change the
// list they are given (`shared-list`).
string ID = "data/json";

list SENTINELS;
list NAMES = ["INVALID", "OBJECT", "ARRAY", "NUMBER", "STRING", "NULL",
    "TRUE", "FALSE", "DELETE"];

string q(string s)
{
    s = llDumpList2String(llParseStringKeepNulls(s, ["\\"], []), "\\\\");
    s = llDumpList2String(llParseStringKeepNulls(s, ["\""], []), "\\\"");
    s = llDumpList2String(llParseStringKeepNulls(s, ["\n"], []), "\\n");
    return "\"" + s + "\"";
}

emit(string rec, string fields)
{
    llOwnerSay("PROBE " + ID + " rec=" + rec + fields);
    llSleep(0.05);
}

begin()
{
    emit("start", " region=" + q(llGetRegionName())
        + " channel=" + q(llGetEnv("sim_channel"))
        + " version=" + q(llGetEnv("sim_version")));
}

// A value as the record shows it, as the field `name`: a sentinel by name,
// anything else quoted with every sentinel inside it as <NAME>, and
// URL-escaped as well (`<name>_esc`) when it holds a character outside
// printable ASCII.
string field(string name, string s)
{
    integer at = llListFindList(SENTINELS, [s]);
    if (at >= 0)
    {
        return " " + name + "=" + llList2String(NAMES, at);
    }
    // Character by character, compared whole: OpenSim finds a sentinel
    // anywhere with llSubStringIndex and splits at every character on one
    // (the `sentinel-search` records).
    // A control character, or one llEscapeURL cannot encode (a lone
    // surrogate), is shown as "?" and only `<name>_esc` has it.
    string out = "";
    integer plain = TRUE;
    integer i;
    for (i = 0; i < llStringLength(s); ++i)
    {
        string c = llGetSubString(s, i, i);
        integer sentinel = llListFindList(SENTINELS, [c]);
        string e = llEscapeURL(c);
        integer width = llStringLength(e);
        if (sentinel >= 0)
        {
            out += "<" + llList2String(NAMES, sentinel) + ">";
        }
        else if (width == 1 || punctuation(c))
        {
            out += c;
        }
        else if (width == 3 || llGetSubString(e, 0, 0) != "%")
        {
            out += "?";
            plain = FALSE;
        }
        else
        {
            out += c;
            plain = FALSE;
        }
    }
    if (plain)
    {
        return " " + name + "=" + q(out);
    }
    return " " + name + "=" + q(out) + " " + name + "_esc="
        + q(llGetSubString(llEscapeURL(s), 0, 199));
}

// A list of its own with the elements of `l`: OpenSim adds to the list a
// llJson* function is given (the `shared-list` records), and its empty list
// literal is one list.
list copy(list l)
{
    return llDeleteSubList([0] + l, 0, 0);
}

// Whether `c` is printable ASCII punctuation, a space or a newline (which q()
// escapes).
integer punctuation(string c)
{
    list marks = [" ", "!", "\"", "#", "$", "%", "&", "'", "(", ")", "*", "+",
        ",", "-", ".", "/", ":", ";", "<", "=", ">", "?", "@", "[", "\\", "]",
        "^", "_", "`", "{", "|", "}", "~", "\n"];
    return llListFindList(marks, [c]) >= 0;
}

// A list element by element: each one's LSL type (i, f, s, k, v, r) and value.
string elements(list l)
{
    integer n = llGetListLength(l);
    string out = " count=" + (string)n;
    integer i;
    for (i = 0; i < n; ++i)
    {
        string type = llGetSubString("?ifskvr", llGetListEntryType(l, i),
            llGetListEntryType(l, i));
        out += " t" + (string)i + "=" + type
            + field("e" + (string)i, llList2String(l, i));
    }
    return out;
}

string path(list specifiers)
{
    integer n = llGetListLength(specifiers);
    string out = "";
    integer i;
    for (i = 0; i < n; ++i)
    {
        if (i > 0)
        {
            out += ",";
        }
        integer type = llGetListEntryType(specifiers, i);
        if (type == TYPE_STRING)
        {
            out += "s:" + llList2String(specifiers, i);
        }
        else
        {
            out += llGetSubString("?ifskvr", type, type) + ":"
                + llList2String(specifiers, i);
        }
    }
    return q(out);
}

to_json(string label, string type, list l)
{
    emit("list2json", " case=" + label + field("type", type)
        + field("result", llList2Json(type, l)));
}

from_json(string label, string json)
{
    emit("json2list", " case=" + label + " json=" + q(json)
        + elements(llJson2List(json)));
}

get(string label, string json, list specifiers)
{
    string shown = path(specifiers);
    string value = llJsonGetValue(json, copy(specifiers));
    string type = llJsonValueType(json, copy(specifiers));
    emit("get", " case=" + label + " path=" + shown
        + field("value", value) + field("type", type));
}

set(string label, string json, list specifiers, string value)
{
    string shown = path(specifiers);
    string result = llJsonSetValue(json, copy(specifiers), value);
    emit("set", " case=" + label + field("json", json)
        + " path=" + shown + field("value", value)
        + field("result", result));
}

// Whether a llJson* function changes the list it is given: a list variable
// first, then the empty list literal, which is last because it may change
// every `[]` in the script after it.
shared_list()
{
    list p = copy([]);
    string value = llJsonGetValue("7", p);
    emit("shared-list", " what=get-variable length=" + (string)llGetListLength(p)
        + field("value", value));
    p = copy([]);
    value = llJsonValueType("7", p);
    emit("shared-list", " what=type-variable length=" + (string)llGetListLength(p)
        + field("value", value));
    p = copy([]);
    value = llJsonSetValue("[1]", p, "2");
    emit("shared-list", " what=set-variable length=" + (string)llGetListLength(p)
        + field("value", value));
    value = llJsonGetValue("7", []);
    emit("shared-list", " what=get-literal length=" + (string)llGetListLength([])
        + field("value", value));
}

// How the string functions treat a sentinel: what a script that looks for one
// inside a string gets.
sentinel_search()
{
    emit("sentinel-search", " what=length result="
        + (string)llStringLength(JSON_INVALID));
    emit("sentinel-search", " what=escaped result="
        + llEscapeURL(JSON_INVALID));
    emit("sentinel-search", " what=index-in-abc result="
        + (string)llSubStringIndex("abc", JSON_INVALID));
    emit("sentinel-search", " what=index-in-empty result="
        + (string)llSubStringIndex("", JSON_INVALID));
    emit("sentinel-search", " what=index-after-a result="
        + (string)llSubStringIndex("a" + JSON_INVALID, JSON_INVALID));
    list parts = llParseStringKeepNulls("abc", [JSON_INVALID], []);
    emit("sentinel-search", " what=split-abc count="
        + (string)llGetListLength(parts)
        + " parts=" + q(llDumpList2String(parts, "|")));
    emit("sentinel-search", " what=equal-abc result="
        + (string)("abc" == JSON_INVALID));
    emit("sentinel-search", " what=list-find-abc result="
        + (string)llListFindList(["abc"], [JSON_INVALID]));
}

lists()
{
    to_json("types", JSON_ARRAY, [1, 2.5, "a", <1.0, 2.0, 3.0>,
        <0.0, 0.0, 0.0, 1.0>, (key)NULL_KEY]);
    to_json("words", JSON_ARRAY, ["true", "false", "null", JSON_TRUE,
        JSON_FALSE, JSON_NULL]);
    to_json("looks-like-json", JSON_ARRAY, ["\"q\"", "[1]", "{\"a\":1}",
        " [2] ", "{bad", "[1,]"]);
    to_json("looks-like-numbers", JSON_ARRAY, ["7", " 7 ", "-0", "1e3", "01",
        "1.", ".5", "0x10", "+1", "1.50"]);
    to_json("strings", JSON_ARRAY, ["", "a\nb", "a\\b", "a/b",
        llUnescapeURL("%01%7F"), llUnescapeURL("%C3%A9%E2%82%AC"),
        llUnescapeURL("%F0%9F%98%80")]);
    to_json("floats", JSON_ARRAY, [3.14159265, 1.0e20, -0.0, 0.0000001,
        1.0e-20, (float)"1e39", -(float)"1e39"]);
    to_json("integers", JSON_ARRAY, [-2147483648, 2147483647, 0, -1]);
    to_json("empty", JSON_ARRAY, []);
    to_json("sentinels", JSON_ARRAY, [JSON_INVALID, JSON_DELETE, JSON_OBJECT,
        JSON_STRING]);
    to_json("pairs", JSON_OBJECT, ["a", 1, "b", "x", "c", "[1,2]", "d", 2.5]);
    to_json("duplicate", JSON_OBJECT, ["a", 1, "a", 2]);
    to_json("odd", JSON_OBJECT, ["a"]);
    to_json("integer-key", JSON_OBJECT, [1, 2]);
    to_json("vector-key", JSON_OBJECT, [<1.0, 2.0, 3.0>, 1]);
    to_json("key-key", JSON_OBJECT, [(key)NULL_KEY, 1]);
    to_json("empty-key", JSON_OBJECT, ["", 1]);
    to_json("empty", JSON_OBJECT, []);
    to_json("other-type", JSON_STRING, ["a"]);
    to_json("word-type", "x", ["a"]);
}

parses()
{
    from_json("array", "[1,2.5,\"a\",true,false,null,[1,2],{\"a\":1}]");
    from_json("object", "{\"a\":1,\"b\":[2,3],\"c\":{\"d\":null}}");
    from_json("spaces", "  [ 1 , 2 ]  ");
    from_json("number", "1");
    from_json("string", "\"s\"");
    from_json("true", "true");
    from_json("empty", "");
    from_json("word", "garbage");
    from_json("unclosed", "[1,");
    from_json("trailing-comma", "[1,]");
    from_json("no-value", "{\"a\"}");
    from_json("leading-zero", "[01]");
    from_json("bare-point", "[1.]");
    from_json("leading-point", "[.5]");
    from_json("exponents", "[1e5,1E-2,-0,2.50]");
    from_json("escapes", "[\"\\u0041\\n\\/\\\"\\\\\"]");
    from_json("no-comma", "[\"a\" \"b\"]");
    from_json("big-numbers", "[12345678901234567890,2147483648,1.5e300,-1e400]");
    from_json("duplicate", "{\"a\":1,\"a\":2}");
    from_json("nested-empty", "[[],{}]");
    from_json("empty-object", "{}");
    from_json("empty-array", "[]");
    from_json("surrogates", "[\"\\ud83d\\ude00\"]");
    from_json("lone-surrogate", "[\"\\ud83d\"]");
    from_json("literal-utf8", "[\"" + llUnescapeURL("%C3%A9") + "\"]");
    from_json("partial-word", "[truth]");
    from_json("single-quotes", "{'a':1}");
    from_json("trailing-garbage", "[1] x");
    from_json("control-in-string", "[\"a" + llUnescapeURL("%01") + "b\"]");
    from_json("newline-in-string", "[\"a\nb\"]");
}

gets()
{
    string j = "{\"a\":1,\"b\":[10,\"x\",true,null,{\"c\":2.50}],"
        + "\"s\":\"t\\\"u\",\"n\":-1.0e2}";
    emit("document", " json=" + q(j));
    get("whole", j, []);
    get("a", j, ["a"]);
    get("b", j, ["b"]);
    get("b0", j, ["b", 0]);
    get("b1", j, ["b", 1]);
    get("b2", j, ["b", 2]);
    get("b3", j, ["b", 3]);
    get("b4c", j, ["b", 4, "c"]);
    get("b5", j, ["b", 5]);
    get("b-1", j, ["b", -1]);
    get("b-append", j, ["b", JSON_APPEND]);
    get("b-string-index", j, ["b", "0"]);
    get("b-float-index", j, ["b", 1.0]);
    get("a-index", j, ["a", 0]);
    get("b0-index", j, ["b", 0, 0]);
    get("missing", j, ["z"]);
    get("index-on-object", j, [0]);
    get("s", j, ["s"]);
    get("n", j, ["n"]);
    get("broken", "[1,2", [0]);
    get("broken-whole", "[1,2", []);
    get("string-whole", "\"str\"", []);
    get("number-whole", "  7 ", []);
    get("word-whole", "abc", []);
    get("empty-whole", "", []);
    get("duplicate", "{\"a\":1,\"a\":2}", ["a"]);
    get("later-broken", "[1,2,}", [0]);
}

sets()
{
    string o = "{\"a\":1}";
    set("replace", o, ["a"], "2");
    set("word", o, ["b"], "x");
    set("true-word", o, ["b"], "true");
    set("number-text", o, ["b"], "1.50");
    set("array-text", o, ["b"], "[1, 2]");
    set("quoted-text", o, ["b"], "\"q\"");
    set("broken-text", o, ["b"], "{bad");
    set("spaced-number", o, ["b"], "  7 ");
    set("minus-zero", o, ["b"], "-0");
    set("leading-zero", o, ["b"], "01");
    set("empty", o, ["b"], "");
    set("true", o, ["b"], JSON_TRUE);
    set("null", o, ["b"], JSON_NULL);
    set("invalid", o, ["b"], JSON_INVALID);
    set("delete", o, ["a"], JSON_DELETE);
    set("delete-missing", o, ["z"], JSON_DELETE);
    set("nested-new", o, ["x", "y"], "1");
    set("nested-array-new", o, ["x", 0], "1");
    set("nested-array-past", o, ["x", 1], "1");
    set("whole", o, [], "5");
    set("index-on-object", o, [0], "1");
    set("a-nested", o, ["a", "b"], "1");
    string a = "[1,2]";
    set("index", a, [0], "9");
    set("at-end", a, [2], "3");
    set("past-end", a, [3], "4");
    set("append", a, [JSON_APPEND], "5");
    set("delete", a, [1], JSON_DELETE);
    set("delete-past", a, [5], JSON_DELETE);
    set("key-on-array", a, ["k"], "1");
    set("minus-two", a, [-2], "x");
    set("nested-past", a, [5, 0], "x");
    set("nested-at-end", a, [2, 0], "x");
    set("broken", "[1", [0], "2");
    set("empty-document", "", ["a"], "1");
    set("empty-document-index", "", [0], "1");
    set("word-document", "abc", ["a"], "1");
    set("spaced", "  {\"a\" : 1 }  ", ["b"], "2");
    set("spaced-replace", "  {\"a\" : 1 , \"c\" : [ 1 ] }  ", ["a"], "2");
    set("duplicate", "{\"a\":1,\"a\":2}", ["a"], "3");
}

default
{
    state_entry()
    {
        SENTINELS = [JSON_INVALID, JSON_OBJECT, JSON_ARRAY, JSON_NUMBER,
            JSON_STRING, JSON_NULL, JSON_TRUE, JSON_FALSE, JSON_DELETE];
        begin();
        sentinel_search();
        lists();
        parses();
        gets();
        sets();
        shared_list();
        emit("end", "");
    }
}
