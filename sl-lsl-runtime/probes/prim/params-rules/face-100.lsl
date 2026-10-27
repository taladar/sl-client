// probe: prim/params-rules
// script: Face100
// prim: B
// order: 2
// distance: 2
// drive: none
//
// The Probe's neighbour in prim B, two metres east of prim A (see there): as
// soon as it is saved it asks llGetPrimitiveParams for a colour of face 100,
// then PRIM_NAME, and prints what came back. It is a script of its own because
// YEngine throws on a face number past the ones it can index, which stops the
// script that asked.
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
        emit("try", " label=face-100");
        list got = llGetPrimitiveParams([PRIM_COLOR, 100, PRIM_NAME]);
        emit("get", " label=face-100 count=" + (string)llGetListLength(got)
            + " got=" + q(llList2CSV(got)));
    }
}
