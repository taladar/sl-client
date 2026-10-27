// probe: errors/bounds
// script: Controller
// order: 19
// drive: none
// unordered: answer
//
// Whether an index or a stride out of range is an error. Eighteen
// cases, "Bounds 1" to "Bounds 18", are each sent their case in turn,
// two seconds apart: reading a list past either end and as another
// type, the list and string functions' ranges past either end and
// reversed, inserting past either end, strides of zero, negative and
// larger than the list, statistics of an empty list and an unknown
// operation, finding an empty list, too many separators and an empty
// one, and llOrd and llChar out of range. Then every case's
// llGetScriptState is read and every case pinged.
string ID = "errors/bounds";
list NAMES = ["Bounds 1", "Bounds 2", "Bounds 3", "Bounds 4", "Bounds 5", "Bounds 6", "Bounds 7", "Bounds 8", "Bounds 9", "Bounds 10", "Bounds 11", "Bounds 12", "Bounds 13", "Bounds 14", "Bounds 15", "Bounds 16", "Bounds 17", "Bounds 18"];
integer N = 18;

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
