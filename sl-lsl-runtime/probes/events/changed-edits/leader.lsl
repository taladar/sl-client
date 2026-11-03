// probe: events/changed-edits
// script: Leader
// order: 2
// drive: answer, edit
//
// Which `changed` bits an edit made with the build floater raises, and in
// which prims. Prim A holds this script, prim B the Child (see there); once
// the owner grants PERMISSION_CHANGE_LINKS, B is linked to A as its child.
// Then the runner makes one edit every three seconds — to every face of
// prim A, its size, place, rotation, shape, light, name, description,
// phantom flag and face media, then to prim B — and each prim prints every
// `changed` event it gets with the step it came in. An edit to the value a
// prim already has is made twice in a row, so that the second says whether a
// change to nothing raises anything. Last, the link is broken again.
string ID = "events/changed-edits";

// Each step: what is changed, to what, and in which prim. A value that is
// text is quoted as the record carries it.
list STEPS = [
    "color", "255,0,0", "A",
    "color", "255,0,0", "A",
    "alpha", "128", "A",
    "alpha", "128", "A",
    "texture", "5748decc-f629-461c-9a36-a35a221fe21f", "A",
    "texture", "5748decc-f629-461c-9a36-a35a221fe21f", "A",
    "repeats", "2", "A",
    "glow", "0.2", "A",
    "fullbright", "1", "A",
    "scale", "0.6,0.6,0.6", "A",
    "scale", "0.6,0.6,0.6", "A",
    "move", "0,0,0.3", "A",
    "turn", "45", "A",
    "cut", "25", "A",
    "light", "1", "A",
    "name", "\"Edited\"", "A",
    "description", "\"edited\"", "A",
    "phantom", "1", "A",
    "phantom", "0", "A",
    "media", "\"https://example.com/\"", "A",
    "media", "\"\"", "A",
    "color", "0,255,0", "B",
    "scale", "0.4,0.4,0.4", "B",
    "texture", "5748decc-f629-461c-9a36-a35a221fe21f", "B",
    "move", "0,0,0.3", "B"
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
        llListen(-7332, "", NULL_KEY, "");
        llRegionSay(-7331, "who");
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
        llSetTimerEvent(3.0);
    }

    changed(integer change)
    {
        emit("changed", " prim=A step=" + (string)step + " bits=" + names(change));
    }

    timer()
    {
        if (!found)
        {
            // The child may not have been listening yet.
            llRegionSay(-7331, "who");
            return;
        }
        if (!linked)
        {
            return;
        }
        integer count = llGetListLength(STEPS) / 3;
        if (step < count)
        {
            ++step;
            llMessageLinked(LINK_ALL_OTHERS, step, "step", "");
            integer at = (step - 1) * 3;
            string what = llList2String(STEPS, at);
            string value = llList2String(STEPS, at + 1);
            string prim = llList2String(STEPS, at + 2);
            emit("step", " n=" + (string)step + " what=" + what + " prim=" + prim
                + " prims=" + (string)llGetNumberOfPrims());
            emit("waiting", " drive=edit what=" + what + " value=" + value + " prim=" + prim);
        }
        else if (step == count)
        {
            ++step;
            llMessageLinked(LINK_ALL_OTHERS, step, "step", "");
            emit("step", " n=" + (string)step + " what=unlink");
            llBreakAllLinks();
        }
        else
        {
            llSetTimerEvent(0.0);
            emit("end", " prims=" + (string)llGetNumberOfPrims());
        }
    }
}
