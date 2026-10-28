// probe: time/timer-rearm
// vary: at
// drive: none
//
// What a second llSetTimerEvent does to a timer already running. Each case
// sets a timer, sleeps, sets it again in the same event, and reports when
// the first timer event came, counted from the first set:
// `same` 2.0, a second in 2.0 again (3.0 if the set restarts the count);
// `shorter` 2.0, a second in 1.5 (2.5 if it restarts); `longer` 1.0, half a
// second in 2.0 (2.5 if it restarts); `zero-then-set` 1.0, 0.0, 1.0 at
// once; `due-in-sleep` 1.0, a two-second sleep in which it falls due, then
// 3.0 — does the event already due still run (2.0) or is it dropped (5.0)?
// Then `due-in-sleep-cancel`: 1.0, the same sleep, then 0.0 — does the
// event already due still run? (XEngine: yes, measured by
// memory/recursion.) Seen at a fence two seconds on.
string ID = "time/timer-rearm";

list CASES = ["same", "shorter", "longer", "zero-then-set", "due-in-sleep"];

integer index = -1;
integer active;
integer cancelled;
integer late;

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

next()
{
    ++index;
    string name = llList2String(CASES, index);
    llResetTime();
    if (name == "same")
    {
        active = TRUE;
        llSetTimerEvent(2.0);
        llSleep(1.0);
        llSetTimerEvent(2.0);
    }
    else if (name == "shorter")
    {
        active = TRUE;
        llSetTimerEvent(2.0);
        llSleep(1.0);
        llSetTimerEvent(1.5);
    }
    else if (name == "longer")
    {
        active = TRUE;
        llSetTimerEvent(1.0);
        llSleep(0.5);
        llSetTimerEvent(2.0);
    }
    else if (name == "zero-then-set")
    {
        active = TRUE;
        llSetTimerEvent(1.0);
        llSetTimerEvent(0.0);
        llSetTimerEvent(1.0);
    }
    else if (name == "due-in-sleep")
    {
        active = TRUE;
        llSetTimerEvent(1.0);
        llSleep(2.0);
        llSetTimerEvent(3.0);
    }
    else
    {
        cancelled = TRUE;
        late = 0;
        llSetTimerEvent(1.0);
        llSleep(2.0);
        llSetTimerEvent(0.0);
        llMessageLinked(LINK_THIS, 1, "fence", "");
    }
}

default
{
    state_entry()
    {
        begin();
        next();
    }

    timer()
    {
        if (cancelled)
        {
            ++late;
            return;
        }
        if (!active) return;
        active = FALSE;
        float at = llGetTime();
        llSetTimerEvent(0.0);
        emit("first", " case=" + llList2String(CASES, index) + " at=" + (string)at);
        llMessageLinked(LINK_THIS, 0, "next", "");
    }

    link_message(integer sender, integer num, string msg, key id)
    {
        if (num == 0)
        {
            next();
        }
        else if (num == 1)
        {
            llSleep(2.0);
            llMessageLinked(LINK_THIS, 2, "fence", "");
        }
        else if (num == 2)
        {
            emit("cancelled", " case=due-in-sleep-cancel ran=" + (string)late);
            emit("end", "");
        }
    }

    touch_start(integer count)
    {
        llResetScript();
    }
}
