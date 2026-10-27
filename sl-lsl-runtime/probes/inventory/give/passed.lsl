// probe: inventory/give
// script: Passed
// order: 3
// drive: none
//
// The script prim A gives prim B with llGiveInventory: it says so if it runs
// there.
string ID = "inventory/give";

default
{
    state_entry()
    {
        if (llGetObjectName() == "Receiver")
        {
            llOwnerSay("PROBE " + ID + " rec=passed-runs param=" + (string)llGetStartParameter());
        }
    }
}
