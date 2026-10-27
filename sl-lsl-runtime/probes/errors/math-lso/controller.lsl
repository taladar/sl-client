// probe: errors/math-lso
// script: Controller
// order: 16
// drive: none
// unordered: answer
//
// errors/math's fifteen cases with the faulters compiled to LSO on
// Second Life ("LSO 1" to "LSO 15"): the old engine's words for the
// same faults, and whether it faults on the same ones. OpenSim ignores
// the target, so there it is errors/math again. The same pauses keep
// Second Life's flood limit from dropping a fault's words.
string ID = "errors/math-lso";
list NAMES = ["LSO 1", "LSO 2", "LSO 3", "LSO 4", "LSO 5", "LSO 6", "LSO 7", "LSO 8", "LSO 9", "LSO 10", "LSO 11", "LSO 12", "LSO 13", "LSO 14", "LSO 15"];
integer N = 15;

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

report(string when)
{
    integer i;
    for (i = 0; i < N; ++i)
    {
        string n = llList2String(NAMES, i);
        emit("script-state", " when=" + when + " script=" + q(n)
            + " running=" + (string)llGetScriptState(n));
    }
}

default
{
    state_entry()
    {
        emit("start", " region=" + q(llGetRegionName())
            + " channel=" + q(llGetEnv("sim_channel"))
            + " version=" + q(llGetEnv("sim_version")));
        integer i;
        for (i = 0; i < N; ++i)
        {
            llSetScriptState(llList2String(NAMES, i), TRUE);
            llResetOtherScript(llList2String(NAMES, i));
        }
        step = 0;
        llSetTimerEvent(2.0);
    }

    timer()
    {
        // XEngine still delivers a timer event queued before the timer
        // was stopped, so a finished sequence ignores it.
        if (step < 0) return;
        ++step;
        if (step <= N)
        {
            llMessageLinked(LINK_THIS, step, "0", "");
            // Second Life passes an object eight error messages, then
            // drops them until it has been quiet for 10 to 30 seconds
            // (errors/flood): no more than four faults between pauses.
            if (step == 4 || step == 9) llSetTimerEvent(35.0);
            else llSetTimerEvent(2.0);
        }
        else if (step == N + 1) report("after-faults");
        else if (step == N + 2)
        {
            // Two ways back for a faulted script: reset, then set running;
            // set running, then reset.
            emit("revive", " script=" + q("LSO 1") + " how=reset-then-run");
            llResetOtherScript("LSO 1");
            llSetScriptState("LSO 1", TRUE);
            emit("revive", " script=" + q("LSO 2") + " how=run-then-reset");
            llSetScriptState("LSO 2", TRUE);
            llResetOtherScript("LSO 2");
        }
        else if (step == N + 4)
        {
            emit("ping", " when=after-revive");
            llMessageLinked(LINK_THIS, 1000, "", "");
        }
        else if (step == N + 6)
        {
            report("after-revive");
            llSetTimerEvent(0.0);
            step = -1;
            emit("end", "");
        }
    }
}
