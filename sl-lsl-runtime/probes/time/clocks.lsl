// probe: time/clocks
// vary: timestamp, date, unix, gmt, wall, time_of_day, got, then, script, real, ratio, dilation, fps
// drive: none
//
// What each clock returns and how they agree. `formats`: the timestamp's
// length and fraction digits, whether llGetDate is the timestamp's date,
// whether llGetUnixTime and llGetGMTclock are the timestamp's second,
// whether the second-of-day clocks are whole seconds, and the wall clock's
// offset from GMT in hours (Second Life: Pacific time). `get-and-reset`:
// llGetAndResetTime after half a second, and llGetTime right after it.
// `dilation`: llGetTime across five seconds of the timestamp's clock, busy
// all the while, beside the region's own dilation and frame rate.
string ID = "time/clocks";

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

        string stamp = llGetTimestamp();
        string date = llGetDate();
        integer unix = llGetUnixTime();
        float gmt = llGetGMTclock();
        float wall = llGetWallclock();
        float day = llGetTimeOfDay();
        integer dot = llSubStringIndex(stamp, ".");
        integer digits = 0;
        if (dot >= 0) digits = llStringLength(stamp) - dot - 2;
        integer second = (integer)llGetSubString(stamp, 17, 18);
        integer offset = llRound((gmt - wall) / 3600.0);
        if (offset < 0) offset += 24;
        emit("formats", " timestamp=" + q(stamp)
            + " length=" + (string)llStringLength(stamp)
            + " fraction_digits=" + (string)digits
            + " ends_z=" + (string)(llGetSubString(stamp, -1, -1) == "Z")
            + " date=" + q(date)
            + " date_is_timestamps=" + (string)(llGetSubString(stamp, 0, 9) == date)
            + " unix=" + (string)unix
            + " unix_second_is_timestamps=" + (string)(unix % 60 == second)
            + " gmt=" + (string)gmt
            + " gmt_whole=" + (string)(gmt == (float)((integer)gmt))
            + " gmt_is_unix_of_day=" + (string)((integer)gmt == unix % 86400)
            + " wall=" + (string)wall
            + " wall_whole=" + (string)(wall == (float)((integer)wall))
            + " wall_behind_gmt_hours=" + (string)offset
            + " time_of_day=" + (string)day
            + " time_of_day_under_4h=" + (string)(day < 14400.0));

        llResetTime();
        float zero = llGetTime();
        llSleep(0.5);
        float got = llGetAndResetTime();
        float then = llGetTime();
        emit("get-and-reset", " reset_reads=" + (string)zero
            + " got=" + (string)got
            + " then=" + (string)then
            + " then_is_zero=" + (string)(then == 0.0));

        integer start = micro();
        llResetTime();
        while (since(start) < 5000000)
        {
        }
        float script = llGetTime();
        float real = since(start) / 1000000.0;
        emit("dilation", " script=" + (string)script
            + " real=" + (string)real
            + " ratio=" + (string)(script / real)
            + " dilation=" + (string)llGetRegionTimeDilation()
            + " fps=" + (string)llGetRegionFPS());
        emit("end", "");
    }

    touch_start(integer count)
    {
        llResetScript();
    }
}
