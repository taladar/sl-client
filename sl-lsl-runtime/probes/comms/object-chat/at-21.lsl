// probe: comms/object-chat
// script: At21
// prim: E
// order: 6
// distance: 21
// drive: none
//
// A listener of the Speaker's (see there), 21 m east. It prints every
// line it hears, with whether it came from its own prim and how long it
// was.
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
        llRegionSay(6, "ready At21");
    }

    listen(integer channel, string name, key id, string message)
    {
        if (channel == 6)
        {
            llListen(0, "", NULL_KEY, "");
            return;
        }
        emit("heard", " listener=At21" + " on=" + (string)channel
            + " from_own_prim=" + (string)(id == llGetKey())
            + " from_agent=" + (string)(llGetAgentSize(id) != ZERO_VECTOR)
            + " length=" + (string)llStringLength(message)
            + " message=" + q(llGetSubString(message, 0, 39)));
    }
}
