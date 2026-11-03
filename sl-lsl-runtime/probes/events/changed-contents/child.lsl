// probe: events/changed-contents
// script: Child
// prim: B
// order: 1
// distance: 2
// drive: none
//
// The Leader's other prim, two metres east of prim A (see there). It answers
// "who" on channel -7333 with its key on -7334, so the Leader can link it,
// keeps the step the Leader last told it by link message, and prints every
// `changed` event it gets with that step and how many items it holds.
string ID = "events/changed-contents";

integer step;

emit(string rec, string fields)
{
    llOwnerSay("PROBE " + ID + " rec=" + rec + fields);
}

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
        llListen(-7333, "", NULL_KEY, "who");
    }

    listen(integer channel, string name, key id, string message)
    {
        if (llGetOwnerKey(id) == llGetOwner())
        {
            llRegionSay(-7334, (string)llGetKey());
        }
    }

    link_message(integer sender, integer num, string str, key id)
    {
        if (str == "step")
        {
            step = num;
        }
    }

    changed(integer change)
    {
        emit("changed", " prim=B step=" + (string)step + " bits=" + names(change)
            + " items=" + (string)llGetInventoryNumber(INVENTORY_ALL));
    }
}
