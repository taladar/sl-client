// probe: prim/params
// script: Shadows
// prim: B
// order: 1
// distance: 2
// drive: none
// skip-on: opensim-xengine
//
// The Probe's neighbour in prim B, two metres east of prim A (see there): as
// soon as it is saved it sets PRIM_CAST_SHADOWS (by its number, 24) TRUE and
// prints what a get of it then reads. It is a script of its own because on
// XEngine the call never returns: in the Probe it ended the probe there, and
// in this script it held the engine up so long that the Probe's upload was
// never answered (twice, 2026-10-27). So it is left out on XEngine.
string ID = "prim/params";

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
        emit("try", " label=cast-shadows");
        llSetPrimitiveParams([24, TRUE]);
        emit("rt", " label=cast-shadows got=" + q(llList2CSV(llGetPrimitiveParams([24]))));
    }
}
