// probe: inventory/listing
// script: Range minus one
// prim: B
// distance: 2
// order: 4
// drive: none
//
// llGetInventoryName(INVENTORY_SCRIPT, -1) in prim B, which holds only these four
// scripts: a script of its own, since OpenSim throws on an index out of
// range, which under YEngine stops the script.
string ID = "inventory/listing";

default
{
    state_entry()
    {
        string name = llGetInventoryName(INVENTORY_SCRIPT, -1);
        llOwnerSay("PROBE " + ID + " rec=name case=minus-one value=\"" + name + "\"");
    }
}
