// probe: events/changed-contents
// script: Leader
// order: 2
// drive: answer, contents
//
// Which `changed` bits a change to a prim's contents made from a viewer
// raises, and in which prims. Prim A holds this script, prim B the Child
// (see there); once the owner grants PERMISSION_CHANGE_LINKS, B is linked to
// A as its child. Then, every four seconds, the owner drops a notecard into
// prim A, renames it in the contents tab, deletes it, makes a new script,
// saves it, deletes it, and drops a notecard into prim B; the other avatar,
// which owns nothing here, drops a notecard into prim A; prim A allows
// inventory drops (llAllowInventoryDrop) and the other avatar drops another,
// and then the owner does; drops are disallowed again. Each prim prints
// every `changed` event it gets with the step it came in and how many items
// it then holds, and each step's record how many prim A held before it.
// Last, the link is broken again.
string ID = "events/changed-contents";

// Each step: the record's fields after `drive=contents`, and whether prim A
// allows drops while it runs.
list STEPS = [
    "op=drop name=Dropped", FALSE,
    "op=rename name=Dropped to=Renamed", FALSE,
    "op=remove name=Renamed", FALSE,
    "op=new-script name=Fresh", FALSE,
    "op=save-script name=Fresh", FALSE,
    "op=remove name=Fresh", FALSE,
    "op=drop name=Card prim=B", FALSE,
    "by=other op=drop name=Gift", FALSE,
    "by=other op=drop name=Allowed", TRUE,
    "op=drop name=Owned", TRUE
];

key child;
integer found;
integer step;
integer linked;

string q(string s)
{
    s = llDumpList2String(llParseStringKeepNulls(s, ["\\"], []), "\\\\");
    s = llDumpList2String(llParseStringKeepNulls(s, ["\""], []), "\\\"");
    s = llDumpList2String(llParseStringKeepNulls(s, ["\n"], []), "\\n");
    return "\"" + s + "\"";
}

emit(string rec, string fields)
{
    llOwnerSay("PROBE " + ID + " rec=" + rec + fields);
}

begin()
{
    emit("start", " region=" + q(llGetRegionName())
        + " channel=" + q(llGetEnv("sim_channel"))
        + " version=" + q(llGetEnv("sim_version")));
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
        begin();
        llListen(-7334, "", NULL_KEY, "");
        llRegionSay(-7333, "who");
        llSetTimerEvent(2.0);
    }

    listen(integer channel, string name, key id, string message)
    {
        if (llGetOwnerKey(id) != llGetOwner() || found)
        {
            return;
        }
        found = TRUE;
        child = (key)message;
        emit("waiting", " drive=answer grant=128");
        llRequestPermissions(llGetOwner(), PERMISSION_CHANGE_LINKS);
    }

    run_time_permissions(integer perms)
    {
        emit("permissions", " got=" + (string)perms);
        llCreateLink(child, TRUE);
        linked = TRUE;
        step = 0;
        llSetTimerEvent(4.0);
    }

    changed(integer change)
    {
        emit("changed", " prim=A step=" + (string)step + " bits=" + names(change)
            + " items=" + (string)llGetInventoryNumber(INVENTORY_ALL));
    }

    timer()
    {
        if (!found)
        {
            // The child may not have been listening yet.
            llRegionSay(-7333, "who");
            return;
        }
        if (!linked)
        {
            return;
        }
        integer count = llGetListLength(STEPS) / 2;
        if (step < count)
        {
            ++step;
            llMessageLinked(LINK_ALL_OTHERS, step, "step", "");
            integer at = (step - 1) * 2;
            integer allow = llList2Integer(STEPS, at + 1);
            llAllowInventoryDrop(allow);
            emit("step", " n=" + (string)step + " allow_drop=" + (string)allow
                + " items=" + (string)llGetInventoryNumber(INVENTORY_ALL));
            emit("waiting", " drive=contents " + llList2String(STEPS, at));
        }
        else if (step == count)
        {
            ++step;
            llMessageLinked(LINK_ALL_OTHERS, step, "step", "");
            llAllowInventoryDrop(FALSE);
            emit("step", " n=" + (string)step + " what=unlink allow_drop=0"
                + " items=" + (string)llGetInventoryNumber(INVENTORY_ALL));
            llBreakAllLinks();
        }
        else
        {
            llSetTimerEvent(0.0);
            emit("end", " items=" + (string)llGetInventoryNumber(INVENTORY_ALL)
                + " prims=" + (string)llGetNumberOfPrims());
        }
    }
}
