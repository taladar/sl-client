// probe: events/state-self
// drive: none
//
// Two corners of the `state` statement. `state default;` inside `default`:
// does it run state_exit and state_entry (Second Life: neither)? And a
// `state` in a global function (the `if (TRUE)` form the compiler accepts):
// does the caller carry on after the call, and when does the change happen
// (Second Life: the caller carries on; the change happens when the handler
// ends)?
string ID = "events/state-self";

integer entries;
integer exits;
integer phase;

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

leave()
{
    emit("function", " step=before");
    if (TRUE) state two;
    emit("function", " step=after");
}

default
{
    state_entry()
    {
        ++entries;
        if (entries == 1)
        {
            begin();
            llSetTimerEvent(0.5);
        }
        else emit("reentered", " entries=" + (string)entries);
    }

    state_exit()
    {
        ++exits;
        emit("exit", " state=default exits=" + (string)exits);
    }

    timer()
    {
        ++phase;
        if (phase == 1)
        {
            emit("self", " step=before");
            state default;
        }
        else if (phase == 2)
        {
            emit("self", " step=after entries=" + (string)entries
                + " exits=" + (string)exits);
            leave();
            emit("caller", " step=after-call");
        }
    }

    touch_start(integer count)
    {
        llResetScript();
    }
}

state two
{
    state_entry()
    {
        llSetTimerEvent(0.0);
        emit("entered", " state=two exits=" + (string)exits);
        emit("end", "");
    }

    touch_start(integer count)
    {
        llResetScript();
    }
}
