// probe: errors/bounds-newer
// script: Controller
// order: 5
// drive: none
// unordered: answer
//
// errors/bounds for the newer list and string functions, which OpenSim
// compiles none of: llReplaceSubString with an empty pattern and a count
// out of range, and a stride of zero to llList2ListSlice,
// llListFindStrided and llListSortStrided ("Bounds 1" to "Bounds 4").
// Second Life's column is the measurement; OpenSim's is the compile
// error.
string ID = "errors/bounds-newer";
list NAMES = ["Bounds 1", "Bounds 2", "Bounds 3", "Bounds 4"];
integer N = 4;

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
        }
        else if (step == N + 1) report("after-faults");
        else if (step == N + 2)
        {
            emit("ping", " when=after-faults");
            llMessageLinked(LINK_THIS, 1000, "", "");
        }
        else if (step == N + 4)
        {
            llSetTimerEvent(0.0);
            step = -1;
            emit("end", "");
        }
    }
}
