// probe: prim/link-refusals
// script: Child
// prim: B
// order: 2
// distance: 2
// drive: none
//
// Prim B, two metres east of prim A (see the Leader). It answers "who" on
// channel -7317 with "B <its key>" on -7318, so the Leader can link it, and
// prints every `changed` event it gets with its link number then.
string ID = "prim/link-refusals";

string names(integer b)
{
    list NAMES = ["INVENTORY", "COLOR", "SHAPE", "SCALE", "TEXTURE", "LINK", "ALLOWED_DROP",
        "OWNER", "REGION", "TELEPORT", "REGION_START", "MEDIA", "RENDER_MATERIAL"];
    string out = "";
    integer i;
    for (i = 0; i < 13; ++i)
    {
        if (b & (1 << i))
        {
            if (out != "")
            {
                out += "|";
            }
            out += llList2String(NAMES, i);
        }
    }
    integer rest = b & ~0x1FFF;
    if (rest)
    {
        if (out != "")
        {
            out += "|";
        }
        out += (string)rest;
    }
    if (out == "")
    {
        out = "none";
    }
    return out;
}

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
            llRegionSay(-7318, "B " + (string)llGetKey());
        }
    }

    changed(integer change)
    {
        llOwnerSay("PROBE " + ID + " rec=child-changed bits=" + names(change)
            + " link=" + (string)llGetLinkNumber()
            + " prims=" + (string)llGetNumberOfPrims());
    }
}
