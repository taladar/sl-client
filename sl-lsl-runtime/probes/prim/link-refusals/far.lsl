// probe: prim/link-refusals
// script: Far
// prim: C
// order: 1
// height: 70
// drive: none
//
// Prim C, seventy metres above prim A (see the Leader): further than Second
// Life's linksets reach (64 m). It answers "who" on channel -7317 with
// "C <its key>" on -7318, and prints any `changed` event it gets, which a link
// that went through would raise.
string ID = "prim/link-refusals";

default
{
    state_entry()
    {
        llListen(-7317, "", NULL_KEY, "who");
    }

    listen(integer channel, string name, key id, string message)
    {
        if (llGetOwnerKey(id) == llGetOwner())
        {
            llRegionSay(-7318, "C " + (string)llGetKey());
        }
    }

    changed(integer change)
    {
        llOwnerSay("PROBE " + ID + " rec=far-changed bits=" + (string)change
            + " link=" + (string)llGetLinkNumber()
            + " prims=" + (string)llGetNumberOfPrims());
    }
}
