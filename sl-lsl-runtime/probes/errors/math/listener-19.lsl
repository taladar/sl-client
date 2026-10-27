// probe: errors/math
// unordered: heard
// script: Listener 19
// prim: B
// order: 16
// distance: 19
// drive: none
//
// In prim B, 19 m east of prim A, just inside say range, hears
// DEBUG_CHANNEL: who spoke, whether its owner is prim A's, how far
// away it is and the message verbatim (newlines as \n).
string ID = "errors/math";

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
        llListen(DEBUG_CHANNEL, "", NULL_KEY, "");
        emit("listening", " channel=" + (string)DEBUG_CHANNEL + " dist=19");
    }

    listen(integer channel, string name, key id, string message)
    {
        vector at = llList2Vector(llGetObjectDetails(id, [OBJECT_POS]), 0);
        emit("heard", " name=" + q(name)
            + " same_owner=" + (string)(llGetOwnerKey(id) == llGetOwner())
            + " dist=" + (string)llVecDist(at, llGetPos())
            + " len=" + (string)llStringLength(message)
            + " msg=" + q(message));
    }
}
