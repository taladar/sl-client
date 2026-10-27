// probe: inventory/give
// script: Receiver
// prim: B
// distance: 2
// order: 1
// drive: none
//
// Prim B, which prim A gives to. It names itself Receiver for prim A's sensor
// to find, sets a remote script access pin of 1234, and on every `changed`
// prints the change and its inventory: each item's name and type, and for a
// script whether it runs.
string ID = "inventory/give";

string q(string s)
{
    s = llDumpList2String(llParseStringKeepNulls(s, ["\\"], []), "\\\\");
    s = llDumpList2String(llParseStringKeepNulls(s, ["\""], []), "\\\"");
    s = llDumpList2String(llParseStringKeepNulls(s, ["\n"], []), "\\n");
    return "\"" + s + "\"";
}

default
{
    state_entry()
    {
        llSetObjectName("Receiver");
        llSetRemoteScriptAccessPin(1234);
    }

    changed(integer change)
    {
        string items;
        integer n = llGetInventoryNumber(INVENTORY_ALL);
        integer i;
        for (i = 0; i < n; ++i)
        {
            string name = llGetInventoryName(INVENTORY_ALL, i);
            integer type = llGetInventoryType(name);
            if (i) items += "|";
            items += name + ":" + (string)type;
            if (type == INVENTORY_SCRIPT) items += ":" + (string)llGetScriptState(name);
        }
        llOwnerSay("PROBE " + ID + " rec=receiver-changed change=" + (string)change
            + " items=" + q(items));
    }
}
