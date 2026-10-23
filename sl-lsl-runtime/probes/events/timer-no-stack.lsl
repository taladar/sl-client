// probe: events/timer-no-stack
// drive: none
//
// Whether a timer that falls due while the script is busy stacks: a 0.1 s
// timer is set, then the script sleeps two seconds (twenty periods). The
// gaps between the first timer events after the sleep say whether the
// missed periods ran back to back (stacked) or once (Second Life: once).
string ID = "events/timer-no-stack";
float PERIOD = 0.1;
integer KEEP = 8;

float woke;
float last;
integer fired;
list gaps;

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
        llResetTime();
        llSetTimerEvent(PERIOD);
        llSleep(2.0);
        woke = llGetTime();
        last = woke;
    }

    timer()
    {
        float now = llGetTime();
        gaps += [now - last];
        last = now;
        ++fired;
        if (fired == KEEP)
        {
            llSetTimerEvent(0.0);
            emit("timers", " period=" + (string)PERIOD
                + " slept_until=" + (string)woke
                + " first_after_wake=" + (string)llList2Float(gaps, 0)
                + " gaps=" + q(llList2CSV(gaps)));
            emit("end", "");
        }
    }

    touch_start(integer count)
    {
        llResetScript();
    }
}
