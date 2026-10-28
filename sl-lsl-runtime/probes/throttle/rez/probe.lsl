// probe: throttle/rez
// script: Probe
// order: 2
// drive: none
// vary: seconds, slowest, per_second, so_far
//
// Whether a grid throttles a script that rezzes without pause. Prim A holds
// an object, Spark, whose script dies as it is rezzed (see there). The probe
// rezzes it a metre above prim A in six waves of fifty llRezObject calls in
// a row, three seconds apart, so that no more object_rez events queue than
// the 64 a queue holds. For each wave it prints how long the fifty calls
// took, the slowest single call, and how many object_rez events have come
// by the time the next wave starts; at the end, how many came in all, three
// hundred if nothing was refused. What a grid says about a refused rez goes
// to DEBUG_CHANNEL, which the runner notes.
string ID = "throttle/rez";

integer WAVES = 6;
integer PER_WAVE = 50;
integer wave;
integer rezzed;

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

string ms(float seconds)
{
    return (string)(llRound(seconds * 1000.0) / 1000.0);
}

default
{
    state_entry()
    {
        begin();
        llSetTimerEvent(1.0);
    }

    timer()
    {
        llSetTimerEvent(0.0);
        if (wave > 0)
        {
            emit("seen", " after_wave=" + (string)(wave - 1) + " so_far=" + (string)rezzed);
        }
        if (wave == WAVES)
        {
            emit("total", " calls=" + (string)(WAVES * PER_WAVE) + " rezzed=" + (string)rezzed);
            emit("end", "");
            return;
        }
        float slowest = 0.0;
        float start = llGetTime();
        integer i;
        for (i = 0; i < PER_WAVE; ++i)
        {
            float before = llGetTime();
            llRezObject("Spark", llGetPos() + <0.0, 0.0, 1.0>, ZERO_VECTOR, ZERO_ROTATION, i);
            float took = llGetTime() - before;
            if (took > slowest)
            {
                slowest = took;
            }
        }
        float seconds = llGetTime() - start;
        emit("wave", " wave=" + (string)wave + " calls=" + (string)PER_WAVE
            + " seconds=" + ms(seconds) + " slowest=" + ms(slowest)
            + " per_second=" + (string)llRound(PER_WAVE / seconds));
        ++wave;
        llSetTimerEvent(3.0);
    }

    object_rez(key id)
    {
        ++rezzed;
    }
}
