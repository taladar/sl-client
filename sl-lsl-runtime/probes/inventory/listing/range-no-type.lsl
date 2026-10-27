// probe: inventory/listing
// script: Range no type
// prim: B
// distance: 2
// order: 7
// drive: none
//
// llGetInventoryName(99, 0) in prim B, which holds only these four
// scripts: a script of its own, since OpenSim throws on an index out of
// range, which under YEngine stops the script.
string ID = "inventory/listing";

default
{
    state_entry()
    {
        string name = llGetInventoryName(99, 0);
        llOwnerSay("PROBE " + ID + " rec=name case=no-type value=\"" + name + "\"");
    }
}
