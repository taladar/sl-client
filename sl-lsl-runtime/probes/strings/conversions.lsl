// probe: strings/conversions
// drive: none
//
// Casts between strings and the other types, both ways: how a float, a
// vector and a rotation are written by (string), by llList2String, by
// llList2CSV and by llDumpList2String (large, small, negative zero, many
// digits, Infinity and NaN where the grid can make them); what (integer),
// (float), (vector), (rotation) and (key) read from strings with a sign,
// spaces, hexadecimal, an exponent, trailing text, too few or too many
// components; and the type llGetListEntryType reports for each.
string ID = "strings/conversions";

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

out(string rec, string name, string value)
{
    emit(rec, " case=" + q(name) + " value=" + q(value));
}

default
{
    state_entry()
    {
        begin();
        list fs = [1.0e30, -1.0e30, 1.0e-7, 0.0000005, 0.0000015, 123456789.0,
            1234567.875, -0.0, 0.1, 3.4e38, 1.0e-30, 99999.99, 0.000001];
        list names = ["1e30", "-1e30", "1e-7", "5e-7", "1.5e-6", "123456789",
            "1234567.875", "-0", "0.1", "3.4e38", "1e-30", "99999.99", "1e-6"];
        integer i;
        for (i = 0; i < llGetListLength(fs); ++i)
        {
            float x = llList2Float(fs, i);
            string n = llList2String(names, i);
            out("float", n + " cast", (string)x);
            out("float", n + " list2string", llList2String(fs, i));
            out("float", n + " csv", llList2CSV([x]));
            out("float", n + " dump", llDumpList2String([x], ","));
        }
        float inf = (float)"inf";
        float nan = (float)"nan";
        out("float", "inf cast", (string)inf);
        out("float", "nan cast", (string)nan);
        out("float", "inf csv", llList2CSV([inf, nan]));
        float big = llList2Float(fs, 9);
        float huge = big * 10.0;
        out("float", "overflow cast", (string)huge);
        out("float", "overflow negated", (string)(-huge));
        vector v = <1.0, -0.0000015, 123456.7>;
        rotation r = <0.5, -0.25, 1.0e-7, 1.0e10>;
        out("vector", "cast", (string)v);
        out("vector", "list2string", llList2String([v], 0));
        out("vector", "csv", llList2CSV([v]));
        out("vector", "dump", llDumpList2String([v], ","));
        out("vector", "concat", "" + (string)v);
        out("rotation", "cast", (string)r);
        out("rotation", "csv", llList2CSV([r]));
        out("key", "csv", llList2CSV([NULL_KEY, (key)"x"]));
        out("integer", "csv", llList2CSV([-2147483648, 0, 7]));
        list ints = ["0x1F", "0X1f", "-0x1", " +5", "+-5", "5e2", "2147483648",
            "4294967296", "-2147483649", "99999999999", "  12abc", "abc", "", "0x",
            "0xFFFFFFFF", "0x100000000", "\t\n7", "1.9", "-1.9", "٣", "０７"];
        for (i = 0; i < llGetListLength(ints); ++i)
        {
            string s = llList2String(ints, i);
            out("to-integer", s, (string)((integer)s));
        }
        list floats = ["1e3", "1E3", ".5", "-.5e1", "5.", "0x1p4", "0x10", "  1.5x",
            "1e39", "-1e39", "1e-50", "infinity", "-inf", "NaN", "1,5", "", "+2",
            "1.0000001", "3.4028235e38", "1e", "e5", "1.5e+2", "1.5e-2"];
        for (i = 0; i < llGetListLength(floats); ++i)
        {
            string s = llList2String(floats, i);
            out("to-float", s, (string)((float)s));
        }
        list vecs = ["<1,2,3>", "<1, 2, 3>", " <1,2,3>", "<1,2>", "<1,2,3,4>",
            "<1,2,3> tail", "<1,2,x>", "1,2,3", "<1e1,0x10,-.5>", "<1 2 3>",
            "<,,>", "<1,2,3"];
        for (i = 0; i < llGetListLength(vecs); ++i)
        {
            string s = llList2String(vecs, i);
            out("to-vector", s, (string)((vector)s));
            out("to-rotation", s, (string)((rotation)s));
        }
        list keys = ["00000000-0000-0000-0000-000000000001", "junk", "",
            "00000000-0000-0000-0000-00000000000", "00000000000000000000000000000001",
            " 00000000-0000-0000-0000-000000000001", "AAAAAAAA-0000-0000-0000-000000000001"];
        for (i = 0; i < llGetListLength(keys); ++i)
        {
            string s = llList2String(keys, i);
            key k = (key)s;
            integer truth;
            if (k) truth = 1;
            out("to-key", s, (string)k + " true=" + (string)truth);
        }
        list typed = [1, 1.0, "s", (key)"k", <1.0, 2.0, 3.0>, <0.0, 0.0, 0.0, 1.0>,
            (string)((key)"x"), (key)NULL_KEY];
        string types;
        for (i = 0; i < llGetListLength(typed); ++i)
        {
            types += (string)llGetListEntryType(typed, i) + " ";
        }
        out("entry-type", "1 1.0 s key vector rotation string-of-key NULL_KEY", types);
        list csv = llCSV2List("1,1.5,<1,2,3>,<1,2,3,4>,a b, c ,\"q,r\",,x");
        types = "";
        for (i = 0; i < llGetListLength(csv); ++i)
        {
            types += (string)llGetListEntryType(csv, i) + ":" + llList2String(csv, i) + "|";
        }
        out("csv2list", "1,1.5,<1,2,3>,<1,2,3,4>,a b, c ,\"q,r\",,x", types);
        out("list2string", "float-in-list", llList2String([1.5], 0));
        out("list2string", "integer-as-float", (string)llList2Float([7], 0));
        out("list2string", "key-as-string", llList2String([NULL_KEY], 0));
        out("list2key", "string", (string)llList2Key(["00000000-0000-0000-0000-000000000001"], 0));
        out("list2key", "key", (string)llList2Key([NULL_KEY], 0));
        emit("end", "");
    }
}
