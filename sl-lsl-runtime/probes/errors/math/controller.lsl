// probe: errors/math
// script: Controller
// order: 18
// drive: none
// unordered: answer
//
// Which arithmetic faults, and what each grid says. Fifteen faulters,
// "Math 1" to "Math 15", are each sent their case in turn, two seconds
// apart: integer division and modulo by zero, float, vector and rotation
// division by zero, the most negative integer divided by and modulo -1,
// llSqrt of a negative, a float multiplication past the largest float,
// llModPow modulo zero, llLog of zero, llAcos out of range, llPow giving
// NaN, casts of infinity and NaN, and normalising a zero vector. The
// prim is renamed "Faulty prim" first, to see whose name an error
// carries. Then every faulter's llGetScriptState is read, and two ways
// back are tried: Math 1 reset and then set running, Math 2 set running
// and then reset, after which every faulter is pinged. Listeners 19 m
// and 21 m away, either side of say range, hear DEBUG_CHANNEL. The
// sequence pauses 35 seconds after the fourth and the ninth case,
// since Second Life drops an object's errors past the eighth until it
// has been quiet a while (errors/flood).
string ID = "errors/math";
list NAMES = ["Math 1", "Math 2", "Math 3", "Math 4", "Math 5", "Math 6", "Math 7", "Math 8", "Math 9", "Math 10", "Math 11", "Math 12", "Math 13", "Math 14", "Math 15"];
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
        llSetObjectName("Faulty prim");
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
            emit("revive", " script=" + q("Math 1") + " how=reset-then-run");
            llResetOtherScript("Math 1");
            llSetScriptState("Math 1", TRUE);
            emit("revive", " script=" + q("Math 2") + " how=run-then-reset");
            llSetScriptState("Math 2", TRUE);
            llResetOtherScript("Math 2");
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
