// probe: comms/listen-limit
// script: Leader
// order: 1
// drive: none
//
// How many listens one script may hold, and what the one too many does. The
// Filler, saved after this script, opens listens on channels of their own
// until it has seventy or the grid stops it, and tells this script by link
// message where it got to (Second Life documents 65, and a script error
// beyond). This script then asks whether the Filler still runs
// (llGetScriptState) and pings it, since a faulted script may be reported
// running and answer nothing (errors/runtime-errors).
string ID = "comms/listen-limit";

integer step;

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

check()
{
    step = 1;
    emit("filler", " running=" + (string)llGetScriptState("Filler"));
    llMessageLinked(LINK_THIS, 2, "ping", "");
    llSetTimerEvent(3.0);
}

default
{
    state_entry()
    {
        begin();
        llSetTimerEvent(20.0);
    }

    link_message(integer sender, integer num, string text, key id)
    {
        if (num == 1 && step == 0)
        {
            check();
        }
        else if (num == 3)
        {
            emit("pong", " text=" + q(text));
        }
    }

    timer()
    {
        if (step == 0)
        {
            emit("filler", " reported=0");
            check();
        }
        else
        {
            llSetTimerEvent(0.0);
            emit("end", "");
        }
    }
}
