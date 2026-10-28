// probe: time/reset-mid-sleep
// vary: at, script_time
// script: Controller
// prim: A
// order: 2
// drive: none
//
// What a reset and a stop do to a script in the middle of an llSleep, and
// whether a stopped script's clock runs. Run `reset`: the Sleeper starts a
// five-second sleep and is reset one second in — does its state_entry come
// at once, when the sleep would have ended, or never? Run `clock`: the
// Sleeper, idle, resets its clock, is stopped for three seconds and asked
// its clock a second after its restart (5 if the clock ran while it was
// stopped, 2 if not). Run `ask`: the Sleeper starts a five-second sleep, is
// stopped a second in, restarted a second later and pinged a second after
// that — when does the pong come (XEngine: its sleeping event never wakes,
// time/sleep-while-stopped)? Ten seconds in it is reset: does that bring it
// back? Every time is the controller's, since the start of the run.
string ID = "time/reset-mid-sleep";
string SLEEPER = "Sleeper";

integer tick;
string run = "none";

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

mark(string what)
{
    emit("control", " run=" + run + " did=" + what + " at=" + (string)llGetTime());
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
        ++tick;
        if (tick == 2)
        {
            run = "reset";
            llResetTime();
            llMessageLinked(LINK_THIS, 1, "", "");
        }
        else if (tick == 3)
        {
            llResetOtherScript(SLEEPER);
            mark("reset");
        }
        else if (tick == 12)
        {
            run = "clock";
            llResetTime();
            llMessageLinked(LINK_THIS, 4, "", "");
        }
        else if (tick == 13)
        {
            llSetScriptState(SLEEPER, FALSE);
            mark("stop");
        }
        else if (tick == 16)
        {
            llSetScriptState(SLEEPER, TRUE);
            mark("restart");
        }
        else if (tick == 17)
        {
            llMessageLinked(LINK_THIS, 5, "", "");
            mark("ask-clock");
        }
        else if (tick == 20)
        {
            run = "ask";
            llResetTime();
            llMessageLinked(LINK_THIS, 1, "", "");
        }
        else if (tick == 21)
        {
            llSetScriptState(SLEEPER, FALSE);
            mark("stop");
        }
        else if (tick == 22)
        {
            llSetScriptState(SLEEPER, TRUE);
            mark("restart");
        }
        else if (tick == 23)
        {
            llMessageLinked(LINK_THIS, 3, "", "");
            mark("ping");
        }
        else if (tick == 30)
        {
            llResetOtherScript(SLEEPER);
            mark("reset");
        }
        else if (tick == 34)
        {
            llMessageLinked(LINK_THIS, 3, "", "");
            mark("ping");
        }
        else if (tick == 37)
        {
            llSetTimerEvent(0.0);
            emit("end", "");
        }
    }

    link_message(integer sender, integer num, string msg, key id)
    {
        if (num != 2) return;
        if (llGetSubString(msg, 0, 5) == "clock:")
        {
            emit("heard", " run=" + run + " what=clock at=" + (string)llGetTime()
                + " script_time=" + llGetSubString(msg, 6, -1));
            return;
        }
        emit("heard", " run=" + run + " what=" + msg + " at=" + (string)llGetTime());
    }

    touch_start(integer count)
    {
        llResetScript();
    }
}
