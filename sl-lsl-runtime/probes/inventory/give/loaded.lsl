// probe: inventory/give
// script: Loaded
// order: 2
// drive: none
//
// The script prim A loads into prim B with llRemoteLoadScriptPin. In prim A,
// where it starts with no start parameter, it says nothing; loaded, it says
// what it was started with and where it runs.
string ID = "inventory/give";

default
{
    state_entry()
    {
        integer param = llGetStartParameter();
        if (param)
        {
            llOwnerSay("PROBE " + ID + " rec=loaded param=" + (string)param
                + " in=" + llGetObjectName());
        }
    }
}
