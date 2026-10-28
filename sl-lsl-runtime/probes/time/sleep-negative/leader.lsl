// probe: time/sleep-negative
// script: Leader
// prim: A
// order: 2
// drive: none
//
// What llSleep(-1.0) does: the Sleeper sleeps -1.0 seconds and says how
// long that took, and whether its event went on after. Then the Sleeper is
// asked once more, to see whether it still runs. The leader ends the probe
// five seconds on, whatever the Sleeper said.
string ID = "time/sleep-negative";

integer tick;

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
        llSetTimerEvent(1.0);
    }

    timer()
    {
        ++tick;
        if (tick == 1 || tick == 3)
        {
            emit("asked", " time=" + (string)tick);
            llMessageLinked(LINK_THIS, 1, "", "");
        }
        else if (tick == 5)
        {
            llSetTimerEvent(0.0);
            emit("end", "");
        }
    }

    touch_start(integer count)
    {
        llResetScript();
    }
}
