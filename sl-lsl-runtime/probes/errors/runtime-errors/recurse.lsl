// probe: errors/runtime-errors
// script: Recurse
// prim: A
// order: 4
// drive: none
// skip-on: opensim-xengine
//
// Recurses without bound until the stack meets the heap. Left out on
// XEngine: there the recursion overflows the simulator's own native stack
// and the whole OpenSim process aborts (measured 2026-10-23), so the region
// goes down and nothing else in the probe is heard.
string ID = "errors/runtime-errors";

integer deep(integer depth)
{
    return deep(depth + 1) + 1;
}

emit(string rec, string fields)
{
    llOwnerSay("PROBE " + ID + " rec=" + rec + fields);
}

default
{
    link_message(integer sender, integer num, string msg, key id)
    {
        if (num == 5) emit("fault", " script=recurse result=" + (string)deep(0));
    }
}
