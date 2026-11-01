// probe: ossl/strings
// unordered: answer
// script: Leader
// prim: A
// order: 3
// drive: none
//
// Drives "Strings" and "Parser" one step every three seconds, by link
// message: the OSSL searches (1), the in-place sorts (2), the one OSSL
// search .NET refuses (3) — each script's throw is its last step, since an
// engine may stop the script for it — then the ll* searches and splits
// (4) and the split .NET refuses (5). Then it reads both scripts'
// llGetScriptState and pings them (link message 100: each answers if it
// still runs).
//
// The leader itself is plain LSL. On Second Life "Strings" does not
// compile, and that compile error is the measurement.
string ID = "ossl/strings";

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

default
{
    state_entry()
    {
        begin();
        step = 0;
        llSetTimerEvent(3.0);
    }

    timer()
    {
        ++step;
        if (step <= 5)
        {
            emit("trigger", " step=" + (string)step);
            llMessageLinked(LINK_THIS, step, "", "");
        }
        else if (step == 6)
        {
            emit("script-state", " script=" + q("Strings")
                + " running=" + (string)llGetScriptState("Strings"));
            emit("script-state", " script=" + q("Parser")
                + " running=" + (string)llGetScriptState("Parser"));
            emit("ping", "");
            llMessageLinked(LINK_THIS, 100, "", "");
        }
        else if (step == 7)
        {
            llSetTimerEvent(0.0);
            emit("end", "");
        }
    }
}
