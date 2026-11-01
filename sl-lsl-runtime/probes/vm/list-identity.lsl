// probe: vm/list-identity
// drive: none
//
// Whether a list is a value or a shared object: what a change a library
// function or an operator makes to the list it is handed does to the
// variable passed, to the other variables holding that list, and to the
// list a function returned. OpenSim's list is a .NET object handed on by
// reference, and some of its library changes the very object it is given
// (LSL_Api.cs: llSetPayPrice pads a short button list, llDialog adds "Ok" to
// an empty one, llJsonSetValue and, for JSON that is no array or object,
// llJsonGetValue and llJsonValueType add a 0 to empty specifiers), returns
// the object it was given in some cases (llList2List through GetSublist,
// llListInsertList into an empty list, llListReplaceList of a whole list
// from 0), and XEngine compiles list + string, + integer and + float to an
// operator that appends to its left operand (LSL_Types.list.ExtendAndAdd).
//
// Each case starts from fresh locals, makes the change and says every list
// involved with its element types. llSetPayPrice is the change for a list
// of one to three elements, llJsonSetValue for an empty one; the probe is
// in the root prim.
string ID = "vm/list-identity";

list GLOBAL_TARGET;
list GLOBAL_SOURCE = [1];
list GLOBAL_PLUS = [1];

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
}

string types(list l)
{
    string out = "";
    integer i;
    for (i = 0; i < llGetListLength(l); ++i)
    {
        out += (string)llGetListEntryType(l, i);
    }
    return out;
}

// One list of a case, by the name the case gives it.
show(string label, string name, list l)
{
    emit("list", " case=" + label + " name=" + name + " value=" + q(llList2CSV(l))
        + " types=" + types(l));
}

pad(list p)
{
    llSetPayPrice(PAY_DEFAULT, p);
}

list same(list p)
{
    return p;
}

plus_parameter(list p)
{
    p += 2;
}

mutators()
{
    list set = [];
    llJsonSetValue("[]", set, "1");
    show("json-set", "passed", set);

    list set_full = [7];
    llJsonSetValue("[]", set_full, "1");
    show("json-set-full", "passed", set_full);

    list get = [];
    llJsonGetValue("5", get);
    show("json-get", "passed", get);

    list get_array = [];
    llJsonGetValue("[5]", get_array);
    show("json-get-array", "passed", get_array);

    list get_blank = [];
    llJsonGetValue(" ", get_blank);
    show("json-get-blank", "passed", get_blank);

    list value_type = [];
    llJsonValueType("5", value_type);
    show("json-type", "passed", value_type);

    list dialog = [];
    llDialog(llGetOwner(), "vm/list-identity", dialog, 7);
    show("dialog", "passed", dialog);

    list dialog_bad = [];
    llDialog((key)"not a key", "vm/list-identity", dialog_bad, 7);
    show("dialog-bad-key", "passed", dialog_bad);

    list paid = [1];
    llSetPayPrice(PAY_DEFAULT, paid);
    show("pay", "passed", paid);

    list paid_full = [1, 2, 3, 4];
    llSetPayPrice(PAY_DEFAULT, paid_full);
    show("pay-full", "passed", paid_full);
}

aliases()
{
    list a = [1];
    list b = a;
    pad(a);
    show("assign", "passed", a);
    show("assign", "other", b);

    list c = [1];
    list d = c;
    llSetPayPrice(PAY_DEFAULT, d);
    show("assign-other", "original", c);
    show("assign-other", "passed", d);

    list e = [];
    list f = e;
    llJsonSetValue("[]", e, "1");
    show("assign-json", "other", f);

    list g = [];
    list h = g;
    llDialog(llGetOwner(), "vm/list-identity", g, 7);
    show("assign-dialog", "other", h);

    list i = [1];
    pad(i);
    show("parameter", "caller", i);

    list j = [1];
    list r = same(j);
    pad(r);
    show("returned", "original", j);
    show("returned", "result", r);

    list k = [1];
    list cast = (list)k;
    pad(cast);
    show("cast", "original", k);

    list m;
    list n;
    m = n = [1];
    pad(m);
    show("chain", "other", n);

    list o = [1];
    GLOBAL_TARGET = o;
    pad(o);
    show("to-global", "global", GLOBAL_TARGET);

    list p = GLOBAL_SOURCE;
    pad(p);
    show("from-global", "global", GLOBAL_SOURCE);

    list s = [1];
    list t = s;
    s = [2];
    pad(s);
    show("rebound", "other", t);

    list u = [1];
    llSetPayPrice(PAY_DEFAULT, u + []);
    show("expression", "original", u);
}

results()
{
    list s1 = [1, 2, 3];
    list r1 = llList2List(s1, -4, -5);
    pad(r1);
    show("list2list-wrapped", "source", s1);
    show("list2list-wrapped", "result", r1);

    list s2 = [1, 2, 3];
    list r2 = llList2List(s2, 0, -1);
    pad(r2);
    show("list2list-whole", "source", s2);

    list s3 = [1];
    list r3 = llListInsertList([], s3, 0);
    pad(r3);
    show("insert-empty", "source", s3);

    list s4 = [1];
    list r4 = llListInsertList([], s4, -1);
    pad(r4);
    show("insert-empty-negative", "source", s4);

    list s5 = [1];
    list r5 = llListInsertList([], s5, 1);
    pad(r5);
    show("insert-empty-after", "source", s5);

    list s6 = [1];
    list r6 = llListInsertList([5], s6, 0);
    pad(r6);
    show("insert-front", "source", s6);

    list s7 = [1];
    list r7 = llListReplaceList([5, 6], s7, 0, 5);
    pad(r7);
    show("replace-whole", "source", s7);

    list s8 = [1];
    list r8 = llListReplaceList([5, 6, 7], s8, 0, 0);
    pad(r8);
    show("replace-front", "source", s8);
}

operators()
{
    list a1 = [1];
    list c1 = a1 + 2;
    show("plus-integer", "left", a1);
    show("plus-integer", "result", c1);

    list a2 = [1];
    list c2 = a2 + 2.5;
    show("plus-float", "left", a2);
    show("plus-float", "result", c2);

    list a3 = [1];
    list c3 = a3 + "x";
    show("plus-string", "left", a3);
    show("plus-string", "result", c3);

    list a4 = [1];
    list c4 = a4 + (key)"x";
    show("plus-key", "left", a4);
    show("plus-key", "result", c4);

    list a5 = [1];
    list c5 = a5 + <1.0, 2.0, 3.0>;
    show("plus-vector", "left", a5);
    show("plus-vector", "result", c5);

    list a6 = [1];
    list c6 = a6 + <0.0, 0.0, 0.0, 1.0>;
    show("plus-rotation", "left", a6);
    show("plus-rotation", "result", c6);

    list a7 = [1];
    list c7 = a7 + [2];
    show("plus-list", "left", a7);
    show("plus-list", "result", c7);

    list a8 = [1];
    list c8 = 2 + a8;
    show("prepend-integer", "right", a8);
    show("prepend-integer", "result", c8);

    list a9 = [1];
    list c9 = "x" + a9;
    show("prepend-string", "right", a9);
    show("prepend-string", "result", c9);

    integer two = 2;
    list a10 = [1];
    list c10 = a10 + two;
    show("plus-variable", "left", a10);
    show("plus-variable", "result", c10);

    list a11 = [1];
    list b11 = a11;
    a11 += 2;
    show("plus-assign", "other", b11);

    list a12 = [1];
    plus_parameter(a12);
    show("plus-parameter", "caller", a12);

    list c13 = GLOBAL_PLUS + 2;
    show("plus-global", "global", GLOBAL_PLUS);
    show("plus-global", "result", c13);

    list a14 = [1];
    list c14 = a14 + 2 + 3;
    show("plus-twice", "left", a14);
    show("plus-twice", "result", c14);

    list a15 = [1];
    list c15 = a15 + 2;
    c15 += 3;
    show("plus-result-later", "left", a15);
}

default
{
    state_entry()
    {
        emit("start", " region=" + q(llGetRegionName())
            + " channel=" + q(llGetEnv("sim_channel"))
            + " version=" + q(llGetEnv("sim_version")));
        mutators();
        aliases();
        results();
        operators();
        emit("end", "");
    }
}
