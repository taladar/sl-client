// probe: errors/flood
// unordered: heard, asked
// script: Listener
// prim: B
// order: 1
// distance: 10
// drive: none
//
// In prim B, a separate object 10 m from prim A (inside say range), hears
// DEBUG_CHANNEL: whether a script hears every error of a flood, or only
// what the grid passes on to the owner. Asked on channel -7 in the middle
// of the flood, it makes an error of its own: whether another object of
// the same owner's is let through.
string ID = "errors/flood";

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
        llListen(-7, "", NULL_KEY, "");
        emit("listening", " channel=" + (string)DEBUG_CHANNEL);
    }

    listen(integer channel, string name, key id, string message)
    {
        if (channel == -7)
        {
            emit("asked", " script=listener");
            llGetScriptState("Listener asks");
            return;
        }
        vector at = llList2Vector(llGetObjectDetails(id, [OBJECT_POS]), 0);
        emit("heard", " name=" + q(name)
            + " same_owner=" + (string)(llGetOwnerKey(id) == llGetOwner())
            + " dist=" + (string)llVecDist(at, llGetPos())
            + " len=" + (string)llStringLength(message)
            + " msg=" + q(message));
    }
}
