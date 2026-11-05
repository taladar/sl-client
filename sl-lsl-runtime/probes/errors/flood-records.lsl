// probe: errors/flood-records
// drive: none
// vary: t
// deadline: 900
//
// Whether a script's own llOwnerSay lines change how Second Life's error
// flood limit counts. errors/flood-epoch, which printed a record before
// every complaint, passed ten complaints 1.5 seconds apart, where bursts
// with no records between them pass nine; prim/params, which prints two
// records per try, passed eleven complaints in twenty seconds and dropped
// one 18 seconds after the last. Each pair of phases differs only in the
// records; every complaint carries its own number. Phases are 70 seconds of
// quiet apart, more than a flood that ends lasts:
//
// Q: twelve PRIM_MATERIAL complaints 1.5 seconds apart, nothing between.
// R: the same with one record before each complaint, as errors/flood-epoch.
// X: the same with three records before each.
// Q, R and X run in the order Q R X Q R.
// P0: eleven complaints two seconds apart, then one more 18 seconds after
//    the eleventh: prim/params' pace, nothing between.
// P4: the same with four records before each complaint.
string ID = "errors/flood-records";

list PHASES = ["Q", "R", "X", "Q", "R", "P0", "P4"];

integer phase_at;
integer sub;
integer records;

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

// Complaint `n` of the phase at `at`, told the owner alone ("bounds error;
// <number> is not in (0, 7)"), after `records` records.
complain(integer at, integer n)
{
    integer i;
    for (i = 1; i <= records; ++i)
        emit("record", " n=" + (string)n + " i=" + (string)i + " t=" + (string)llGetTime());
    llSetPrimitiveParams([PRIM_MATERIAL, (at + 1) * 100 + n]);
}

next()
{
    ++phase_at;
    sub = 0;
    llSetTimerEvent(70.0);
}

default
{
    state_entry()
    {
        emit("start", " region=" + q(llGetRegionName())
            + " channel=" + q(llGetEnv("sim_channel"))
            + " version=" + q(llGetEnv("sim_version")));
        llResetTime();
        phase_at = 0;
        sub = 0;
        llSetTimerEvent(3.0);
    }

    timer()
    {
        // XEngine still delivers a timer event queued before the timer
        // was stopped, so a finished sequence ignores it.
        if (phase_at < 0) return;
        if (phase_at >= llGetListLength(PHASES))
        {
            llSetTimerEvent(0.0);
            phase_at = -1;
            emit("end", "");
            return;
        }
        string name = llList2String(PHASES, phase_at);
        string kind = llGetSubString(name, 0, 0);
        if (sub == 0)
        {
            records = 0;
            if (kind == "R") records = 1;
            else if (kind == "X") records = 3;
            else if (name == "P4") records = 4;
            emit("phase", " name=" + name + " t=" + (string)llGetTime());
        }
        ++sub;
        complain(phase_at, sub);
        if (kind == "P")
        {
            if (sub < 11) llSetTimerEvent(2.0);
            else if (sub == 11) llSetTimerEvent(18.0);
            else next();
        }
        else if (sub < 12) llSetTimerEvent(1.5);
        else next();
    }
}
