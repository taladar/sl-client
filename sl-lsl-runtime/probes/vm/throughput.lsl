// probe: vm/throughput
// drive: none
//
// How fast a script runs: one million iterations of an empty loop, of a loop
// with three more operations in its body, and of a loop calling llAbs. Each
// loop runs twice; the first run is the warm-up (the first run after a save
// is slower while Mono compiles) and the second is the measurement. A
// million iterations, because llGetTime moves in whole 1/45 s frames and a
// short loop is off by one.
string ID = "vm/throughput";
integer N = 1000000;

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

report(string loop, integer run, float seconds)
{
    string rate = "inf";
    if (seconds > 0.0) rate = (string)llRound((float)N / seconds);
    emit("loop", " loop=" + loop + " run=" + (string)run
        + " iterations=" + (string)N
        + " seconds=" + (string)seconds + " per_second=" + rate);
}

empty(integer run)
{
    integer i;
    llResetTime();
    for (i = 0; i < N; ++i)
    {
    }
    report("empty", run, llGetTime());
}

three_ops(integer run)
{
    integer i;
    integer a = 1;
    llResetTime();
    for (i = 0; i < N; ++i)
    {
        a = a + 1;
        a = a * 1;
        a = a - 1;
    }
    report("three-ops", run, llGetTime());
}

library_call(integer run)
{
    integer i;
    integer a;
    llResetTime();
    for (i = 0; i < N; ++i)
    {
        a = llAbs(i);
    }
    report("llAbs", run, llGetTime());
}

default
{
    state_entry()
    {
        begin();
        integer run;
        for (run = 0; run < 2; ++run)
        {
            empty(run);
            three_ops(run);
            library_call(run);
        }
        emit("end", "");
    }

    touch_start(integer count)
    {
        llResetScript();
    }
}
