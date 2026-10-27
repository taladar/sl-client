// probe: prim/link
// script: Child
// prim: B
// order: 1
// distance: 2
// drive: none
//
// The Leader's other prim, two metres east of prim A (see there). It answers
// "who" on channel -7317 with its key on -7318, so the Leader can link it;
// prints every `changed` event it gets with its link number then; and does
// what the Leader asks by link message: "report" prints its link number, its
// position from the root's, its local position and its local rotation, and
// "setpos <x>" calls llSetPos with a local offset of x metres east of the
// root and prints the local position it then reads.
string ID = "prim/link";

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

string cm(vector v)
{
    return "<" + (string)(llRound(v.x * 100.0) / 100.0) + ","
        + (string)(llRound(v.y * 100.0) / 100.0) + ","
        + (string)(llRound(v.z * 100.0) / 100.0) + ">";
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
        llListen(-7317, "", NULL_KEY, "who");
    }

    listen(integer channel, string name, key id, string message)
    {
        if (llGetOwnerKey(id) == llGetOwner())
        {
            llRegionSay(-7318, (string)llGetKey());
        }
    }

    changed(integer change)
    {
        emit("child-changed", " bits=" + names(change) + " link=" + (string)llGetLinkNumber()
            + " prims=" + (string)llGetNumberOfPrims());
    }

    link_message(integer sender, integer num, string str, key id)
    {
        if (str == "report")
        {
            vector root = llGetRootPosition();
            vector here = llGetPos();
            // A root's or a single prim's local position is its region
            // position, which differs from grid to grid: print it from there.
            string local = cm(llGetLocalPos());
            if (llGetLinkNumber() <= 1)
            {
                local = "region" + cm(llGetLocalPos() - here);
            }
            emit("child", " label=" + (string)id + " link=" + (string)llGetLinkNumber()
                + " from_root=" + cm(here - root) + " local=" + local
                + " prims=" + (string)llGetNumberOfPrims() + " sender=" + (string)sender);
        }
        else if (llGetSubString(str, 0, 6) == "setpos ")
        {
            float x = (float)llGetSubString(str, 7, -1);
            llSetPos(<x, 0.0, 0.0>);
            emit("child-setpos", " asked=" + (string)llRound(x) + " local=" + cm(llGetLocalPos()));
        }
    }
}
