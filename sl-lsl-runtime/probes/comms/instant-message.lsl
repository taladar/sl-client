// probe: comms/instant-message
// drive: touch
// vary: seconds
//
// What llInstantMessage costs the script and what it sends. The other
// avatar touches, so the script learns its key; then the script sends an
// instant message to the owner, to the other avatar, to NULL_KEY, to a key
// that is nobody's, one of 1100 characters to the owner, and one to the
// owner with a line break, timing each call with llGetTime (Second Life
// documents a two-second delay). The runner notes every instant message
// from an object that either avatar is sent: the dialog, who the grid says
// sent it and what its id is, the name and the text. llOwnerSay and
// llRegionSayTo to the owner are timed beside it.
string ID = "comms/instant-message";

key other;

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

string repeat(string unit, integer count)
{
    string out;
    integer i;
    for (i = 0; i < count; ++i)
    {
        out += unit;
    }
    return out;
}

sent(string to, string what, string message)
{
    float started = llGetTime();
    if (what == "im")
    {
        if (to == "owner")
        {
            llInstantMessage(llGetOwner(), message);
        }
        else if (to == "other")
        {
            llInstantMessage(other, message);
        }
        else if (to == "null")
        {
            llInstantMessage(NULL_KEY, message);
        }
        else
        {
            llInstantMessage("01234567-89ab-cdef-0123-456789abcdef", message);
        }
    }
    else if (what == "owner-say")
    {
        llOwnerSay(message);
    }
    else
    {
        llRegionSayTo(llGetOwner(), 0, message);
    }
    emit("sent", " call=" + what + " to=" + to
        + " length=" + (string)llStringLength(message)
        + " seconds=" + (string)(llGetTime() - started));
}

default
{
    state_entry()
    {
        begin();
        emit("waiting", " drive=touch by=other");
    }

    touch_start(integer count)
    {
        other = llDetectedKey(0);
        emit("touch", " by_owner=" + (string)(other == llGetOwner()));
        sent("owner", "im", "to the owner");
        sent("other", "im", "to the other");
        sent("null", "im", "to nobody");
        sent("unknown", "im", "to a key that is nobody's");
        sent("owner", "im", repeat("i", 1100));
        sent("owner", "im", "two\nlines");
        sent("owner", "owner-say", "owner-say beside it");
        sent("owner", "region-say-to", "region-say-to beside it");
        llSetTimerEvent(5.0);
    }

    timer()
    {
        llSetTimerEvent(0.0);
        emit("end", "");
    }
}
