// probe: inventory/listing
// script: Range all 100
// prim: B
// distance: 2
// order: 6
// drive: none
//
// llGetInventoryName(INVENTORY_ALL, 100) in prim B, which holds only these four
// scripts: a script of its own, since OpenSim throws on an index out of
// range, which under YEngine stops the script.
string ID = "inventory/listing";

default
{
    state_entry()
    {
        string name = llGetInventoryName(INVENTORY_ALL, 100);
        llOwnerSay("PROBE " + ID + " rec=name case=all-100 value=\"" + name + "\"");
    }
}
