// probe: prim/params-rules
// script: FaceMinus2
// prim: B
// order: 3
// distance: 2
// drive: none
//
// The Probe's neighbour in prim B, two metres east of prim A (see there): as
// soon as it is saved it asks llGetPrimitiveParams for the glow of face -2,
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
        emit("try", " label=face-minus-2");
        list got = llGetPrimitiveParams([PRIM_GLOW, -2, PRIM_NAME]);
        emit("get", " label=face-minus-2 count=" + (string)llGetListLength(got)
            + " got=" + q(llList2CSV(got)));
    }
}
