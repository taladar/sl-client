// probe: errors/flood-repeat
// drive: none
// vary: t
// deadline: 1260
//
// Whether errors/flood-window's turn between 16 and 17 seconds holds, and
// what ends a flood. There, eight owner-only complaints in a row passed a
// ninth 16 seconds after the first and dropped the two after it, and passed
// all three at 17; eight missing sounds a second apart did the opposite.
// errors/flood-epoch's complaint every 1.5 seconds flooded for good, where
// one every 3.2 seconds passed again after about 34. Every complaint
// carries its own number, so the transcript says which passed. Phases are
// 40 seconds of quiet apart, more than a flood of quiet lasts:
//
// B16, B17: eight PRIM_MATERIAL complaints in a row (the call's 0.2 s
//    apart), then three more D seconds after the first (D the phase's
//    number): the ninth, tenth and eleventh lines.
// S16, S17: the same eight owner-only complaints spread a second apart,
//    as missing sounds come: whether the spread or the audience turns it.
// N16, N17: eight missing sounds (a second apart, the call's delay), said
//    in the open, then three more D seconds after the first.
// Each of the six runs twice, in the order B16 S16 N16 B17 S17 N17.
// G2, G25, G3: twelve PRIM_MATERIAL complaints in a row, then one every 2,
//    2.5 and 3 seconds for 60 seconds: whether the flood ends while they
//    come, and when.
string ID = "errors/flood-repeat";

list PHASES = ["B16", "S16", "N16", "B17", "S17", "N17",
               "B16", "S16", "N16", "B17", "S17", "N17",
               "G2", "G25", "G3"];

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

// The complaint numbered `n` of phase `name`, said in the open: "Could not
// find sound".
open(string name, integer n)
{
    llTriggerSound(name + "-" + (string)n, 1.0);
}

// The complaint numbered `n` of phase `name`, run `run`, in its kind.
line(string name, integer run, integer n)
{
    string kind = llGetSubString(name, 0, 0);
    if (kind == "N") open(name + "r" + (string)run, n);
    else owner(run * 10000 + (integer)llGetSubString(name, 1, -1) * 100 + n);
}

// Phase `name`'s number: B16 is 16, G25 is 25.
integer number(string name)
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
        integer run = 1 + phase_at / 6;
        integer i;
        if (sub == 0)
        {
            first = llGetTime();
            emit("phase", " name=" + name + " run=" + (string)run + " t=" + (string)first);
        }
        if (kind == "G")
        {
            float gap = number(name) / 10.0;
            if (gap < 1.0) gap = number(name);
            if (sub == 0)
            {
                for (i = 1; i <= 12; ++i) owner(30000 + number(name) * 100 + i);
            }
            else
            {
                emit("single", " n=" + (string)sub + " t=" + (string)llGetTime());
                owner(31000 + number(name) * 100 + sub);
            }
            ++sub;
            if (llGetTime() - first > 60.0) next();
            else llSetTimerEvent(gap);
        }
        else if (sub == 0)
        {
            for (i = 1; i <= 8; ++i)
            {
                line(name, run, i);
                if (kind == "S") llSleep(0.85);
            }
            sub = 1;
            llSetTimerEvent(number(name) - (llGetTime() - first));
        }
        else
        {
            emit("single", " n=9 t=" + (string)llGetTime());
            for (i = 9; i <= 11; ++i) line(name, run, i);
            next();
        }
    }
}
