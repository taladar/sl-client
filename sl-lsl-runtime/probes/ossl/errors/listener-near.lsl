// probe: ossl/errors
// unordered: heard
// script: Listener near
// prim: C
// order: 9
// distance: 10
// drive: none
//
// In prim C, a separate object 10 m from prim A (inside say range), hears
// DEBUG_CHANNEL: the speaker's name, whether it is prim A's owner's, how
// far away it is, and the message verbatim (newlines as \n). Its twin tells
// a range apart from a message no script hears at all.
string ID = "ossl/errors";

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
