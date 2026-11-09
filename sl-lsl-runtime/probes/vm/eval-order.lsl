// probe: vm/eval-order
// drive: none
//
// The order each grid evaluates the parts of an expression in, read from a
// function that notes its argument and hands it back: the operands of a
// binary operator, of a chain of them and of a comparison; `&&` and `||`;
// the elements of a list, the components of a vector and of a rotation; the
// arguments of a function of the script's own and of a library function;
// the parts of a string concatenation; the right side of a compound
// assignment against its variable; and an index into a list built inline.
string ID = "vm/eval-order";

list seen;

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

begin()
{
    emit("start", " region=" + q(llGetRegionName())
        + " channel=" + q(llGetEnv("sim_channel"))
        + " version=" + q(llGetEnv("sim_version")));
}

integer f(integer n)
{
    seen += [n];
    return n;
}

string s(integer n)
{
    seen += [n];
    return (string)n;
}

integer two(integer a, integer b)
{
    return a * 10 + b;
}

integer three(integer a, integer b, integer c)
{
    return a * 100 + b * 10 + c;
}

report(string label, string value)
{
    emit("order", " case=" + label + " seen=" + llDumpList2String(seen, ",")
        + " value=" + q(value));
    seen = [];
}

default
{
    state_entry()
    {
        begin();
        seen = [];
        integer i;

        i = f(1) - f(2);
        report("minus", (string)i);
        i = f(1) - f(2) + f(3);
        report("chain", (string)i);
        i = f(1) * f(2) + f(3) * f(4);
        report("products", (string)i);
        i = f(1) < f(2);
        report("less", (string)i);
        i = f(0) && f(1);
        report("and", (string)i);
        i = f(1) || f(0);
        report("or", (string)i);

        list l = [f(1), f(2), f(3)];
        report("list", llList2CSV(l));
        l = [f(1)] + [f(2)];
        report("list-plus", llList2CSV(l));
        vector v = <f(1), f(2), f(3)>;
        report("vector", (string)v);
        rotation r = <f(1), f(2), f(3), f(4)>;
        report("rotation", (string)r);

        i = two(f(1), f(2));
        report("user-args", (string)i);
        i = three(f(1), f(2), f(3));
        report("user-args-three", (string)i);
        string t = llGetSubString("abcdef", f(1), f(2));
        report("library-args", t);
        t = llDumpList2String([f(1), f(2)], s(3));
        report("library-list-arg", t);

        t = s(1) + s(2) + s(3);
        report("concat", t);

        i = f(5);
        seen = [];
        i += f(1) * f(2);
        report("compound", (string)i);

        i = llList2Integer([f(10), f(20), f(30)], f(1));
        report("index-inline", (string)i);

        emit("end", "");
    }
}
