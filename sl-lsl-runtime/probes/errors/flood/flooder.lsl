// probe: errors/flood
// script: Flooder
// order: 3
// drive: none
// vary: seconds
//
// How many errors in a row a grid passes on, and for how long it stops.
// Twelve llGetScriptState calls on scripts that do not exist, each a
// different name ("Nope 1" to "Nope 12"), as fast as the script runs: a
// refused library call, one DEBUG_CHANNEL message each, which leaves the
// script running. After the tenth, "Second" (another script in this prim)
// and "Listener" (in prim B) are asked for one error each, to see whether
// a flood is the script's, the object's or the owner's. Then one more
// error after 10, 30 and 60 seconds of quiet ("Later 1" to "Later 3"), to
// see when the grid passes errors on again.
string ID = "errors/flood";

integer step;

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

burst(string prefix, integer count)
{
    float started = llGetTime();
    integer i;
    for (i = 1; i <= count; ++i)
    {
        llGetScriptState(prefix + (string)i);
        if (i == 10)
        {
            llMessageLinked(LINK_THIS, 1, "", "");
            llRegionSay(-7, "error");
        }
    }
    emit("burst", " names=" + q(prefix) + " count=" + (string)count
        + " seconds=" + (string)(llGetTime() - started));
}

single(integer n, integer quiet)
{
    emit("single", " name=" + q("Later " + (string)n) + " quiet=" + (string)quiet);
    llGetScriptState("Later " + (string)n);
}

default
{
    state_entry()
    {
        emit("start", " region=" + q(llGetRegionName())
            + " channel=" + q(llGetEnv("sim_channel"))
            + " version=" + q(llGetEnv("sim_version")));
        step = 0;
        llSetTimerEvent(3.0);
    }

    timer()
    {
        // XEngine still delivers a timer event queued before the timer
        // was stopped, so a finished sequence ignores it.
        if (step < 0) return;
        ++step;
        if (step == 1)
        {
            burst("Nope ", 12);
            llSetTimerEvent(10.0);
        }
        else if (step == 2)
        {
            single(1, 10);
            llSetTimerEvent(30.0);
        }
        else if (step == 3)
        {
            single(2, 30);
            llSetTimerEvent(60.0);
        }
        else if (step == 4)
        {
            single(3, 60);
            llSetTimerEvent(3.0);
        }
        else
        {
            llSetTimerEvent(0.0);
            step = -1;
            emit("end", "");
        }
    }
}
