// probe: time/timer-floor
// vary: fired, first, smallest, largest, mean
// drive: none
//
// The shortest interval a timer keeps: each interval runs for three
// seconds, and the probe reports how many timer events came, how long the
// first took and the smallest, largest and mean gap after it (Second
// Life: nothing comes faster than a frame; XEngine: the 0.5 s
// MinTimerInterval; YEngine: 0.1 s). Then a negative interval, -1.0: does
// the timer run, and how often? A repeating sensor that finds nothing is
// the second clock that ends that run, three seconds or so on.
string ID = "time/timer-floor";
list INTERVALS = [0.001, 0.01, 0.02, 0.05, 0.1, 0.2, 0.5];
float SPAN = 3.0;

integer index = -1;
integer active;
integer negative;
integer fired;
float first;
float last;
float smallest;
float largest;

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

next()
{
    ++index;
    if (index < llGetListLength(INTERVALS))
    {
        fired = 0;
        first = 0.0;
        smallest = 1000.0;
        largest = 0.0;
        active = TRUE;
        llResetTime();
        last = 0.0;
        llSetTimerEvent(llList2Float(INTERVALS, index));
        return;
    }
    negative = TRUE;
    fired = 0;
    first = 0.0;
    smallest = 1000.0;
    largest = 0.0;
    active = TRUE;
    llResetTime();
    last = 0.0;
    llSetTimerEvent(-1.0);
    llSensorRepeat("probe no such avatar", NULL_KEY, AGENT, 1.0, PI, 1.0);
}

report(float asked)
{
    float mean = 0.0;
    if (fired > 1) mean = (last - first) / (fired - 1);
    emit("interval", " asked=" + (string)asked
        + " fired=" + (string)fired
        + " first=" + (string)first
        + " smallest=" + (string)smallest
        + " largest=" + (string)largest
        + " mean=" + (string)mean);
}

default
{
    state_entry()
    {
        begin();
        next();
    }

    timer()
    {
        if (!active) return;
        float now = llGetTime();
        if (fired == 0)
        {
            first = now;
        }
        else
        {
            float gap = now - last;
            if (gap < smallest) smallest = gap;
            if (gap > largest) largest = gap;
        }
        ++fired;
        last = now;
        if (now >= SPAN && !negative)
        {
            active = FALSE;
            llSetTimerEvent(0.0);
            report(llList2Float(INTERVALS, index));
            llMessageLinked(LINK_THIS, 0, "next", "");
        }
    }

    no_sensor()
    {
        if (!active || llGetTime() < SPAN) return;
        active = FALSE;
        llSensorRemove();
        llSetTimerEvent(0.0);
        report(-1.0);
        emit("end", "");
    }

    link_message(integer sender, integer num, string msg, key id)
    {
        next();
    }

    touch_start(integer count)
    {
        llResetScript();
    }
}
