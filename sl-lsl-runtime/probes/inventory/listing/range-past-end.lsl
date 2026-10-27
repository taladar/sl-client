// probe: inventory/listing
// script: Range past end
// prim: B
// distance: 2
// order: 5
// drive: none
//
// llGetInventoryName(INVENTORY_SCRIPT, llGetInventoryNumber(INVENTORY_SCRIPT)) in prim B, which holds only these four
// scripts: a script of its own, since OpenSim throws on an index out of
// range, which under YEngine stops the script.
string ID = "inventory/listing";

default
{
    state_entry()
    {
        string name = llGetInventoryName(INVENTORY_SCRIPT, llGetInventoryNumber(INVENTORY_SCRIPT));
        llOwnerSay("PROBE " + ID + " rec=name case=past-end value=\"" + name + "\"");
    }
}
