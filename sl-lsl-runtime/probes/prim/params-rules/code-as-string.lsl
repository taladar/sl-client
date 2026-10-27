// probe: prim/params-rules
// script: StringCode
// prim: B
// order: 1
// distance: 2
// drive: none
//
// The Probe's neighbour in prim B, two metres east of prim A (see there): as
// soon as it is saved it asks llGetPrimitiveParams for a rule whose code is
// the string "7" (PRIM_SIZE's number), then PRIM_NAME, and prints what came
// back. It is a script of its own because OpenSim may throw on a code that is
// not an integer, which would stop the script that asked.
string ID = "prim/params-rules";

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

default
{
    state_entry()
    {
        emit("try", " label=code-as-string");
        list got = llGetPrimitiveParams(["7", PRIM_NAME]);
        emit("get", " label=code-as-string count=" + (string)llGetListLength(got)
            + " got=" + q(llList2CSV(got)));
    }
}
