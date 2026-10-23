// probe: time/sleep-while-stopped
// script: Controller
// prim: A
// order: 2
// drive: none
//
// Whether a stopped script's llSleep counts on while it is stopped. Two
// runs: each starts the Sleeper's five-second sleep, stops the Sleeper one
// second in, and restarts it — at eight seconds in run `late` (after the
// sleep would have ended; Second Life: it wakes within the restart's frame)
// and at three seconds in run `early` (before; Second Life: it wakes at
// five). Save the Sleeper first.
string ID = "time/sleep-while-stopped";
string SLEEPER = "Sleeper";

integer tick;

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

mark(string run, string what)
{
    emit("control", " run=" + run + " " + what + "=" + (string)llGetTime());
}

default
{
    state_entry()
    {
        begin();
        llSetScriptState(SLEEPER, TRUE);
        llResetOtherScript(SLEEPER);
        llSetTimerEvent(1.0);
    }

    timer()
    {
        // One tick a second; run `late` on ticks 2-10, run `early` on 18-24.
        ++tick;
        if (tick == 2)
        {
            llResetTime();
            llMessageLinked(LINK_THIS, 1, "late", "");
        }
        else if (tick == 3)
        {
            llSetScriptState(SLEEPER, FALSE);
            mark("late", "stopped_at");
        }
        else if (tick == 10)
        {
            llSetScriptState(SLEEPER, TRUE);
            mark("late", "restarted_at");
        }
        else if (tick == 18)
        {
            llResetTime();
            llMessageLinked(LINK_THIS, 1, "early", "");
        }
        else if (tick == 19)
        {
            llSetScriptState(SLEEPER, FALSE);
            mark("early", "stopped_at");
        }
        else if (tick == 21)
        {
            llSetScriptState(SLEEPER, TRUE);
            mark("early", "restarted_at");
        }
        else if (tick == 30)
        {
            llSetTimerEvent(0.0);
            emit("end", "");
        }
    }

    touch_start(integer count)
    {
        llResetScript();
    }
}
