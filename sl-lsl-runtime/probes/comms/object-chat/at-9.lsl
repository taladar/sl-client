// probe: comms/object-chat
// script: At9
// prim: B
// order: 3
// distance: 9
// drive: none
//
// A listener of the Speaker's (see there), 9 m east, also on channel 9
// for the message "match" alone. It prints every line it hears, with
// whether it came from its own prim and how long it was.
// It listens on channel 0 once the Speaker says "go".
string ID = "comms/object-chat";

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
        llListen(5, "", NULL_KEY, "");
        llListen(6, "", NULL_KEY, "go");
        llListen(9, "", NULL_KEY, "match");
        llRegionSay(6, "ready At9");
    }

    listen(integer channel, string name, key id, string message)
    {
        if (channel == 6)
        {
            llListen(0, "", NULL_KEY, "");
            return;
        }
        emit("heard", " listener=At9" + " on=" + (string)channel
            + " from_own_prim=" + (string)(id == llGetKey())
            + " from_agent=" + (string)(llGetAgentSize(id) != ZERO_VECTOR)
            + " length=" + (string)llStringLength(message)
            + " message=" + q(llGetSubString(message, 0, 39)));
    }
}
