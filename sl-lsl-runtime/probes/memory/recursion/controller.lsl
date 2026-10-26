// probe: memory/recursion
// script: Controller
// order: 6
// drive: none
// vary: depth
//
// How deep a script recurses before the stack meets the heap. Each
// recursing script in turn is sent its link message; once it has stopped
// (llGetScriptState), or the prim's description, where it writes each
// level, has not changed for ten seconds, or a minute has passed, the
// description holds the deepest level reached, which this script reads
// with the recurser's state. The description is cleared before each. The
// depth Second Life reaches differs from run to run.
//
// Prim A holds this script and the five recursers, named exactly "Narrow",
// "Wide", "Limited", "Narrow LSO" and "Wide LSO". Save them first; the
// sequence starts three seconds after this script starts and takes under
// a minute. On OpenSim the recursers are left out (see narrow.lsl) and this
// reads only what a missing script answers.
string ID = "memory/recursion";
list NAMES = ["Narrow", "Wide", "Limited", "Narrow LSO", "Wide LSO"];

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
    emit("reached", " script=" + q(n) + " depth=" + q(llGetObjectDesc())
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
            emit("recurse", " script=" + q(llList2String(NAMES, step)));
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
}
