// probe: ossl/errors
// unordered: answer
// script: Leader
// prim: A
// order: 11
// drive: none
//
// What an OSSL function that is denied, or fails, does to the script that
// called it, and what is said about it. Every three seconds the leader has
// one of the eight callers in prim A make its call (a link message of its
// own number): denied by threat level ("Threat"), by an Allow_ list
// ("Listed"), disabled ("Disabled"); a run-time error ("Runtime"); a
// deprecated function's warning ("Deprecated"); an error shouted rather
// than thrown ("Shout"); and one function each of the LightShare and MOD
// families. Then it reads every caller's llGetScriptState, pings them all
// (link message 100: the ones still running answer), resets "Threat",
// pings again, has "Threat" call a second time — whether a reset script is
// denied again — and reads the states once more.
//
// The leader itself is plain LSL. On Second Life the first caller does not
// compile, and that compile error is the measurement.
string ID = "ossl/errors";
list NAMES = ["Threat", "Listed", "Disabled", "Runtime", "Deprecated",
    "Shout", "LightShare", "Mod"];

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
}

default
{
    state_entry()
    {
        begin();
        step = 0;
        llSetTimerEvent(3.0);
    }

    timer()
    {
        ++step;
        integer callers = llGetListLength(NAMES);
        if (step <= callers)
        {
            emit("trigger", " script=" + q(llList2String(NAMES, step - 1)));
            llMessageLinked(LINK_THIS, step, "", "");
        }
        else if (step == callers + 1) report("after-calls");
        else if (step == callers + 2)
        {
            emit("ping", " when=after-calls");
            llMessageLinked(LINK_THIS, 100, "", "");
        }
        else if (step == callers + 3)
        {
            emit("reset", " script=" + q("Threat"));
            llResetOtherScript("Threat");
        }
        else if (step == callers + 4)
        {
            emit("ping", " when=after-reset");
            llMessageLinked(LINK_THIS, 100, "", "");
        }
        else if (step == callers + 5)
        {
            emit("trigger", " script=" + q("Threat") + " again=1");
            llMessageLinked(LINK_THIS, 1, "", "");
        }
        else if (step == callers + 6)
        {
            report("after-again");
            llSetTimerEvent(0.0);
            emit("end", "");
        }
    }
}
