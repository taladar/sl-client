// probe: events/changed-worn
// drive: attach, away, drop
// deadline: 300
//
// What a worn object's script hears when its wearer teleports to another
// region and back. The runner attaches the prim to the owner's left hand from
// the ground; once `attach` comes, the owner goes away for fifteen seconds
// and comes back; then the runner drops the prim. Every `changed` event is
// kept with the step it came in and whether the script was then in the
// region it started in (`home`), and printed once the owner has been back in
// that region for five seconds — an owner-say may not reach an owner on the
// way — or after a minute and a half if it never comes back. The home region
// is kept in the object's description too: a grid that runs `state_entry`
// again in a worn object arriving in a region (XEngine) loses the script's
// globals, and the script then says `restarted`, with the region it is in,
// and carries on waiting for the owner to come home.
string ID = "events/changed-worn";

string home;
integer step;
integer ticks;
integer back;
list held;

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

flush()
{
    integer i;
    for (i = 0; i < llGetListLength(held); ++i)
    {
        llOwnerSay("PROBE " + ID + " rec=" + llList2String(held, i));
    }
    held = [];
}

default
{
    state_entry()
    {
        if (llGetAttached() != 0)
        {
            home = llGetObjectDesc();
            step = 2;
            ticks = 21;
            held += ["restarted home=" + (string)(llGetRegionName() == home)];
            llSetTimerEvent(1.0);
            return;
        }
        home = llGetRegionName();
        llSetObjectDesc(home);
        emit("start", " region=" + q(home)
            + " channel=" + q(llGetEnv("sim_channel"))
            + " version=" + q(llGetEnv("sim_version")));
        step = 1;
        emit("waiting", " drive=attach point=" + (string)ATTACH_LHAND);
        llSetTimerEvent(1.0);
    }

    attach(key id)
    {
        if (step == 1 && id != NULL_KEY)
        {
            emit("attached", " attached=" + (string)llGetAttached());
            step = 2;
            ticks = 0;
            emit("waiting", " drive=away seconds=15");
        }
        else if (step == 3 && id == NULL_KEY)
        {
            flush();
            llSetTimerEvent(0.0);
            emit("end", "");
        }
    }

    changed(integer change)
    {
        held += ["changed step=" + (string)step + " bits=" + names(change)
            + " home=" + (string)(llGetRegionName() == home)];
        if (step != 2)
        {
            flush();
        }
    }

    timer()
    {
        ++ticks;
        if (step != 2)
        {
            if (ticks > 60)
            {
                flush();
                llSetTimerEvent(0.0);
                emit("timeout", " step=" + (string)step);
                emit("end", "");
            }
            return;
        }
        // Back home with the owner there, for five seconds running.
        if (ticks > 20 && llGetRegionName() == home
            && llGetAgentSize(llGetOwner()) != ZERO_VECTOR)
        {
            ++back;
        }
        else
        {
            back = 0;
        }
        if (back >= 5 || ticks > 90)
        {
            flush();
            emit("back", " home=" + (string)(llGetRegionName() == home)
                + " seconds=" + (string)ticks);
            step = 3;
            ticks = 0;
            emit("waiting", " drive=drop");
        }
    }
}
