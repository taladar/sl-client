// probe: errors/flood-epoch
// drive: none
// vary: t
// deadline: 720
//
// When Second Life clears an object's count of error lines. errors/flood-window
// fits no window that slides with the lines: eight missing sounds passed a
// ninth 16 seconds after the first, and eight more 60 seconds later made a
// ninth 17 seconds after the first the notice, where owner lines did the
// opposite. A count the region clears at fixed times, about every 20 to 24
// seconds, fits every line of it. Here one complaint told the owner alone
// every 1.5 seconds for ten minutes, each with its own number and a record
// of the clock: the ninth line of a count passes and starts a flood that
// drops what follows for about 34 seconds, so where each run of passed
// lines ends says when the count was last cleared.
string ID = "errors/flood-epoch";

integer n;

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

default
{
    state_entry()
    {
        emit("start", " region=" + q(llGetRegionName())
            + " channel=" + q(llGetEnv("sim_channel"))
            + " version=" + q(llGetEnv("sim_version")));
        llResetTime();
        n = 0;
        llSetTimerEvent(1.5);
    }

    timer()
    {
        // XEngine still delivers a timer event queued before the timer
        // was stopped, so a finished sequence ignores it.
        if (n < 0) return;
        ++n;
        if (n > 400)
        {
            llSetTimerEvent(0.0);
            n = -1;
            emit("end", "");
            return;
        }
        emit("line", " n=" + (string)n + " t=" + (string)llGetTime());
        llSetPrimitiveParams([PRIM_MATERIAL, 1000 + n]);
    }
}
