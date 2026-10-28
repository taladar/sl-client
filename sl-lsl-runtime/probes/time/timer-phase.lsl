// probe: time/timer-phase
// vary: times, gaps, last, drift
// drive: none
//
// Whether a timer keeps its phase. Run `plain`: a 1.0 s timer, ten events,
// each one's time since the set — does the error add up (drift) or stay
// put? Runs `busy-short` and `busy-long`: a 0.5 s timer whose handler
// sleeps 0.3 s, then 0.7 s — is the next event due an interval after the
// last was due (gaps of 0.5 s, and 0.7 s once the handler is the longer),
// or an interval after the handler ended (0.8 s and 1.2 s)?
string ID = "time/timer-phase";

string run;
integer active;
integer fired;
integer want;
float interval;
float busy;
float last;
list times;
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

go(string name, float every, float sleep, integer count)
{
    run = name;
    interval = every;
    busy = sleep;
    want = count;
    fired = 0;
    times = [];
    gaps = [];
    last = 0.0;
    active = TRUE;
    llResetTime();
    llSetTimerEvent(interval);
}

default
{
    state_entry()
    {
        begin();
        go("plain", 1.0, 0.0, 10);
    }

    timer()
    {
        if (!active) return;
        float now = llGetTime();
        times += [now];
        gaps += [now - last];
        last = now;
        ++fired;
        if (fired >= want)
        {
            active = FALSE;
            llSetTimerEvent(0.0);
            emit("timers", " run=" + run
                + " interval=" + (string)interval
                + " handler_sleeps=" + (string)busy
                + " events=" + (string)fired
                + " last=" + (string)last
                + " drift=" + (string)(last - fired * interval)
                + " times=" + q(llList2CSV(times))
                + " gaps=" + q(llList2CSV(gaps)));
            llMessageLinked(LINK_THIS, 0, run, "");
            return;
        }
        if (busy > 0.0) llSleep(busy);
    }

    link_message(integer sender, integer num, string msg, key id)
    {
        if (msg == "plain") go("busy-short", 0.5, 0.3, 8);
        else if (msg == "busy-short") go("busy-long", 0.5, 0.7, 8);
        else if (msg == "busy-long") emit("end", "");
    }

    touch_start(integer count)
    {
        llResetScript();
    }
}
