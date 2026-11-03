// probe: events/changed-owner
// script: Relay
// prim: B
// order: 2
// distance: 2
// drive: buy
//
// What a script hears when its object is sold. Prim A holds the Seller (see
// there), prim B this script, which the owner keeps. The runner puts prim A
// up for sale, the original for L$ 0, and the other avatar buys it; the
// Seller says every `changed` event it gets on channel -7336, which this
// script prints with whether prim A's owner was still the one it started
// with (`same_owner`). Ten seconds after it asked for the sale it prints the
// owner prim A now has as the region tells it, tells the Seller to delete
// itself, and ends.
string ID = "events/changed-owner";

key seller;
integer found;
integer asked;
integer ticks;

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
        llListen(-7336, "", NULL_KEY, "");
        llRegionSay(-7335, "who");
        llSetTimerEvent(1.0);
    }

    listen(integer channel, string name, key id, string message)
    {
        list words = llParseString2List(message, [" "], []);
        string kind = llList2String(words, 0);
        if (kind == "key" && !found && llGetOwnerKey(id) == llGetOwner())
        {
            found = TRUE;
            seller = llList2Key(words, 1);
            emit("waiting", " drive=buy by=other");
            asked = TRUE;
            ticks = 0;
        }
        else if (kind == "changed" && id == seller)
        {
            emit("changed", " prim=A bits=" + names(llList2Integer(words, 1))
                + " same_owner=" + llList2String(words, 2));
        }
    }

    timer()
    {
        if (!asked)
        {
            // The Seller may not have been listening yet.
            llRegionSay(-7335, "who");
            return;
        }
        ++ticks;
        if (ticks == 10)
        {
            key owner = llList2Key(llGetObjectDetails(seller, [OBJECT_OWNER]), 0);
            string now = "other";
            if (owner == llGetOwner())
            {
                now = "owner";
            }
            emit("sold", " owner=" + now);
            llRegionSay(-7335, "die");
        }
        else if (ticks == 12)
        {
            llSetTimerEvent(0.0);
            emit("end", "");
        }
    }
}
