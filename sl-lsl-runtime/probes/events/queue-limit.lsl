// probe: events/queue-limit
// drive: none
//
// How many events a script's queue holds: send a hundred link messages to
// the script itself in one handler, then count what arrives (Second Life:
// 0-63, the 65th and later are dropped). The timer that reports is set
// after the queue has drained, so a full queue cannot drop it.
string ID = "events/queue-limit";
integer SENT = 100;

integer got;
integer lowest = -1;
integer highest = -1;

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

default
{
    state_entry()
    {
        begin();
        integer i;
        for (i = 0; i < SENT; ++i)
        {
            llMessageLinked(LINK_THIS, i, "", "");
        }
    }

    link_message(integer sender, integer num, string msg, key id)
    {
        if (got == 0)
        {
            lowest = num;
            llSetTimerEvent(3.0);
        }
        ++got;
        highest = num;
    }

    timer()
    {
        llSetTimerEvent(0.0);
        emit("received", " sent=" + (string)SENT + " got=" + (string)got
            + " lowest=" + (string)lowest + " highest=" + (string)highest);
        emit("end", "");
    }

    touch_start(integer count)
    {
        llResetScript();
    }
}
