// probe: errors/runtime-errors
// unordered: heard
// script: Listener far
// prim: B
// order: 5
// distance: 50
// drive: none
//
// In prim B, a separate object 50 m from prim A (past say range, inside
// shout range), hears DEBUG_CHANNEL: the speaker's name (an object's),
// whether it is prim A's owner's, how far away it is, and the message
// verbatim (newlines as \n). Its twin "Listener near" does the same from
// prim C, 10 m away; `dist` tells their lines apart.
string ID = "errors/runtime-errors";

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
        emit("listening", " channel=" + (string)DEBUG_CHANNEL);
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
