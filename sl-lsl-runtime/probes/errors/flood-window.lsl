// probe: errors/flood-window
// drive: none
// vary: t
// deadline: 720
//
// How long Second Life's error flood window is, and what happens to the
// line told the owner alone that reaches the limit. prim/params passed nine
// llSetPrimitiveParams complaints (told the owner alone) in 17.7 seconds
// and dropped the next three, 18 seconds after the ninth: as if the ninth
// passed and started a silent flood, over a window longer than 17.7
// seconds; errors/flood-paths' W passed a ninth missing sound 18.1 seconds
// after the first of eight, so the window is shorter than that.
// Every complaint carries its own number, so the transcript says which
// passed. Phases are 40 seconds of quiet apart, more than a flood lasts:
//
// P: PRIM_PROJECTOR set, read and reset alone: whether those complain at
//    all (prim/params' last three complaints were these).
// O16 to O20: eight PRIM_MATERIAL complaints in a row (0.2 seconds apart,
//    the call's delay), then, D seconds after the first (D the phase's
//    number), three more: the ninth, tenth and eleventh lines. Within the window the ninth reaches
//    the limit: it passes and the two after it do not when it starts a
//    flood, or it is dropped with them. Past the window the first line has
//    left it, and the ninth passes as the eighth.
// N16, N17: the same with missing sounds, said in the open (a second
//    apart, the call's delay): the ninth is the notice within the window.
// F: twelve PRIM_MATERIAL complaints in a row, then one every three
//    seconds for 45: when the flood they started ends.
string ID = "errors/flood-window";

list PHASES = ["P", "O16", "O17", "O18", "O19", "O20", "N16", "N17", "F"];

integer phase_at;
integer sub;
float first;

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

// The complaint numbered `n`, told the owner alone: "bounds error; n is
// not in (0, 7)".
owner(integer n)
{
    llSetPrimitiveParams([PRIM_MATERIAL, n]);
}

// The complaint numbered `n`, said in the open: "Could not find sound".
open(string name, integer n)
{
    llTriggerSound(name + "-" + (string)n, 1.0);
}

// Phase `name`'s window in seconds, from its name: O16 is 16.
integer delta(string name)
{
    return (integer)llGetSubString(name, 1, -1);
}

next()
{
    ++phase_at;
    sub = 0;
    llSetTimerEvent(40.0);
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
        integer i;
        if (sub == 0)
        {
            first = llGetTime();
            emit("phase", " name=" + name + " t=" + (string)first);
        }
        if (kind == "P")
        {
            llSetPrimitiveParams([42, TEXTURE_PLYWOOD, 1.0, 0.0, 0.5]);
            emit("got", " got=" + q(llList2CSV(llGetPrimitiveParams([42]))));
            llSetPrimitiveParams([42, NULL_KEY, 0.0, 0.0, 0.0]);
            next();
        }
        else if (kind == "F")
        {
            if (sub == 0)
            {
                for (i = 1; i <= 12; ++i) owner(2000 + i);
            }
            else
            {
                emit("single", " n=" + (string)(2100 + sub) + " t=" + (string)llGetTime());
                owner(2100 + sub);
            }
            ++sub;
            if (sub > 15) next();
            else llSetTimerEvent(3.0);
        }
        else if (sub == 0)
        {
            integer base = delta(name) * 100;
            for (i = 1; i <= 8; ++i)
            {
                if (kind == "O") owner(base + i);
                else open(name, i);
            }
            sub = 1;
            llSetTimerEvent(delta(name) - (llGetTime() - first));
        }
        else
        {
            emit("single", " n=9 t=" + (string)llGetTime());
            integer base = delta(name) * 100;
            for (i = 9; i <= 11; ++i)
            {
                if (kind == "O") owner(base + i);
                else open(name, i);
            }
            next();
        }
    }
}
