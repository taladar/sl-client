// probe: errors/runtime-errors
// script: Controller
// prim: A
// order: 7
// drive: none
//
// What a run-time error does. Each faulter in turn is made to fail (integer
// division and modulo by zero, float division by zero, unbounded recursion);
// then every script's llGetScriptState is read, the stopped "Div int" is
// pinged (a stopped script is sent nothing), restarted, and pinged again to
// see which state and globals it carries on with.
//
// Prim A holds this script and the four faulters, named exactly "Div int",
// "Mod int", "Div float" and "Recurse"; prims B and C, separate objects 50 m
// and 10 m away, hold the two listeners. Save the faulters and the
// listeners first; the sequence starts three seconds after this script
// starts and ends after about thirty. By hand, also copy the Script Debug
// window's text.
string ID = "errors/runtime-errors";
list NAMES = ["Div int", "Mod int", "Div float", "Recurse"];

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

begin()
{
    emit("start", " region=" + q(llGetRegionName())
        + " channel=" + q(llGetEnv("sim_channel"))
        + " version=" + q(llGetEnv("sim_version")));
}

report(string when)
{
    integer i;
    for (i = 0; i < llGetListLength(NAMES); ++i)
    {
        string n = llList2String(NAMES, i);
        emit("script-state", " when=" + when + " script=" + q(n)
            + " running=" + (string)llGetScriptState(n));
    }
    emit("script-state", " when=" + when + " script=self running="
        + (string)llGetScriptState(llGetScriptName()));
    emit("script-state", " when=" + when + " script=missing running="
        + (string)llGetScriptState("Nope"));
}

default
{
    state_entry()
    {
        begin();
        integer i;
        for (i = 0; i < llGetListLength(NAMES); ++i)
        {
            llSetScriptState(llList2String(NAMES, i), TRUE);
            llResetOtherScript(llList2String(NAMES, i));
        }
        step = 0;
        llSetTimerEvent(3.0);
    }

    timer()
    {
        ++step;
        if (step == 1) llMessageLinked(LINK_THIS, 1, "0", "");      // Div int
        else if (step == 2) llMessageLinked(LINK_THIS, 3, "0", ""); // Mod int
        else if (step == 3) llMessageLinked(LINK_THIS, 4, "0", ""); // Div float
        else if (step == 4) llMessageLinked(LINK_THIS, 5, "0", ""); // Recurse
        else if (step == 6) report("after-faults");
        else if (step == 7)
        {
            emit("ping", " target=stopped");
            llMessageLinked(LINK_THIS, 100, "", "");
        }
        else if (step == 8)
        {
            emit("restart", " script=" + q("Div int"));
            llSetScriptState("Div int", TRUE);
        }
        else if (step == 9)
        {
            emit("ping", " target=restarted");
            llMessageLinked(LINK_THIS, 100, "", "");
        }
        else if (step == 10)
        {
            report("after-restart");
            llSetTimerEvent(0.0);
            emit("end", "");
        }
    }

    touch_start(integer count)
    {
        llResetScript();
    }
}
