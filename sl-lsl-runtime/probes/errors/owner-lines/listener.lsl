// probe: errors/owner-lines
// unordered: heard
// script: Listener
// prim: B
// order: 1
// distance: 10
// drive: none
//
// In prim B, 10 m from prim A, hears DEBUG_CHANNEL: which of prim A's
// complaints a script hears, where Second Life sends some to the owner
// alone (errors/flood-paths: the other avatar heard none of them).
string ID = "errors/owner-lines";

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
        emit("heard", " name=" + q(name) + " msg=" + q(message));
    }
}
