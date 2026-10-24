// probe: time/get-time-frames
// vary: samples, distinct, smallest, largest, first, timestamp, unix, gmt, wall, time_of_day
// drive: none
//
// How llGetTime moves: sample it in a tight loop for about a second and
// report the steps between distinct values (Second Life: whole 1/45 s
// frames). Then one sample of each clock's format.
string ID = "time/get-time-frames";

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

default
{
    state_entry()
    {
        begin();
        llResetTime();
        float last = llGetTime();
        float now = last;
        integer samples = 0;
        integer distinct = 0;
        float smallest = 1000.0;
        float largest = 0.0;
        list first = [];
        while (now < 1.0)
        {
            now = llGetTime();
            ++samples;
            if (now != last)
            {
                float step = now - last;
                if (step < smallest) smallest = step;
                if (step > largest) largest = step;
                if (distinct < 10) first += [step];
                ++distinct;
                last = now;
            }
        }
        emit("steps", " samples=" + (string)samples
            + " distinct=" + (string)distinct
            + " smallest=" + (string)smallest
            + " largest=" + (string)largest
            + " first=" + q(llList2CSV(first)));
        emit("clocks", " timestamp=" + q(llGetTimestamp())
            + " unix=" + (string)llGetUnixTime()
            + " gmt=" + (string)llGetGMTclock()
            + " wall=" + (string)llGetWallclock()
            + " time_of_day=" + (string)llGetTimeOfDay());
        emit("end", "");
    }

    touch_start(integer count)
    {
        llResetScript();
    }
}
