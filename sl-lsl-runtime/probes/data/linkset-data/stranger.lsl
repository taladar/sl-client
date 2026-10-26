// probe: data/linkset-data
// script: Stranger
// prim: B
// order: 3
// distance: 3
// drive: none
//
// The Leader's other object (see there): it writes a pair of its own,
// says it is ready on channel 7, and when it hears "look" prints what it
// sees of the store — its own pair, the one prim A wrote, and the count.
string ID = "data/linkset-data";

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
        llLinksetDataWrite("own", "by-b");
        llListen(7, "", NULL_KEY, "look");
        llRegionSay(7, "ready");
    }

    listen(integer channel, string name, key id, string message)
    {
        if (llGetOwnerKey(id) == llGetOwner())
        {
            emit("stranger", " own=" + q(llLinksetDataRead("own"))
                + " seen=" + q(llLinksetDataRead("seen"))
                + " keys=" + (string)llLinksetDataCountKeys());
        }
    }
}
