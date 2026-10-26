// probe: memory/collision
// script: Controller
// order: 5
// drive: none
//
// What a runaway allocation meets on each grid. Each growing script in
// turn is sent its link message; once it says it is done (link message
// 100), or has stopped (llGetScriptState), or the prim's description,
// where it writes its progress, has not changed for ten seconds, or a
// minute has passed, the description holds the last step or count it
// wrote, which this script reads with the grower's state. The description
// is cleared before each. A grower that stopped at a stack-heap collision
// says nothing more, and on YEngine a faulted script still reads as
// running, so there the description is what tells it has stopped.
//
// Prim A holds this script and the four growers, named exactly "Doubling",
// "Doubling LSO", "Full list" and "Limited list". Save them first; the
// sequence starts three seconds after this script starts and takes one to
// three minutes.
string ID = "memory/collision";
list NAMES = ["Doubling", "Doubling LSO", "Full list", "Limited list"];

integer step;
integer done;
integer waited;
integer still;
string last;

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

report(integer i)
{
    string n = llList2String(NAMES, i);
    emit("reached", " script=" + q(n) + " last=" + q(llGetObjectDesc())
        + " running=" + (string)llGetScriptState(n));
}

default
{
    state_entry()
    {
        emit("start", " region=" + q(llGetRegionName())
            + " channel=" + q(llGetEnv("sim_channel"))
            + " version=" + q(llGetEnv("sim_version")));
        llSetObjectDesc("");
        step = 0;
        llSetTimerEvent(3.0);
    }

    timer()
    {
        // XEngine still delivers a timer event queued before the timer
        // was stopped, so a finished sequence ignores it.
        if (step > llGetListLength(NAMES)) return;
        if (step > 0)
        {
            ++waited;
            string now = llGetObjectDesc();
            if (now == last) ++still;
            else still = 0;
            last = now;
            integer running = llGetScriptState(llList2String(NAMES, step - 1));
            if (!done && running && still < 10 && waited < 60) return;
            report(step - 1);
        }
        if (step < llGetListLength(NAMES))
        {
            llSetObjectDesc("");
            done = FALSE;
            waited = 0;
            still = 0;
            last = "";
            emit("grow", " script=" + q(llList2String(NAMES, step)));
            llMessageLinked(LINK_THIS, step + 1, "", "");
            ++step;
            llSetTimerEvent(1.0);
        }
        else
        {
            llSetTimerEvent(0.0);
            ++step;
            llSetObjectDesc("");
            emit("end", "");
        }
    }

    link_message(integer sender, integer num, string msg, key id)
    {
        if (num == 100) done = TRUE;
    }
}
