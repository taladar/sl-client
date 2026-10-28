// probe: time/sleep-lengths
// vary: mean, smallest, largest, script_mean
// drive: none
//
// How long llSleep really sleeps: five sleeps of each length, timed by the
// timestamp's microseconds (llGetTime moves in frames on Second Life) and,
// beside it, by llGetTime. Does a zero sleep yield a frame; does a short
// one round up to a frame? A negative sleep has a probe of its own
// (time/sleep-negative), since XEngine throws on it.
string ID = "time/sleep-lengths";
list LENGTHS = [0.0, 0.001, 0.01, 0.02, 0.05, 0.1, 0.25];
integer REPEATS = 5;

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

// Microseconds within the current ten minutes, by the timestamp.
integer micro()
{
    string t = llGetTimestamp();
    integer m = (integer)llGetSubString(t, 15, 15);
    integer s = (integer)llGetSubString(t, 17, 18);
    string f = "000000";
    integer dot = llSubStringIndex(t, ".");
    if (dot >= 0) f = llGetSubString(t, dot + 1, -2) + "000000";
    return (m * 60 + s) * 1000000 + (integer)llGetSubString(f, 0, 5);
}

integer since(integer start)
{
    integer d = micro() - start;
    if (d < 0) d += 600000000;
    return d;
}

default
{
    state_entry()
    {
        begin();
        // Warm up the clock helpers before anything is timed.
        since(micro());
        integer i;
        integer n = llGetListLength(LENGTHS);
        for (i = 0; i < n; ++i)
        {
            float length = llList2Float(LENGTHS, i);
            integer total = 0;
            integer smallest = 1000000000;
            integer largest = 0;
            float script = 0.0;
            integer r;
            for (r = 0; r < REPEATS; ++r)
            {
                llResetTime();
                integer start = micro();
                llSleep(length);
                integer took = since(start);
                script += llGetTime();
                total += took;
                if (took < smallest) smallest = took;
                if (took > largest) largest = took;
            }
            emit("sleep", " asked=" + (string)length
                + " mean=" + (string)(total / REPEATS / 1000.0)
                + " smallest=" + (string)(smallest / 1000.0)
                + " largest=" + (string)(largest / 1000.0)
                + " script_mean=" + (string)(script / REPEATS * 1000.0)
                + " unit=ms");
        }
        emit("end", "");
    }

    touch_start(integer count)
    {
        llResetScript();
    }
}
