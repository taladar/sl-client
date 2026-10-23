// probe: events/state-change
// script: Probe
// prim: A
// order: 2
// drive: none
//
// What a state change keeps and drops. In `default` the script listens on
// the talker's channel and runs a one-second timer; on its third tick it
// queues ten link messages to itself and changes to `two` in the same
// handler. In `two` it counts what still arrives: the queued messages
// (Second Life: discarded), the timer (Second Life: survives), the listen
// (Second Life: dropped). After three ticks in `two` it listens again, to
// show the talker was audible all along.
//
// Prim A holds this script; prim B, a separate object a few metres away,
// holds the talker, which starts first.
string ID = "events/state-change";
integer CHANNEL = -730001;
integer QUEUED = 10;

integer ticks;
integer heard;
integer linked;
integer exits;

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
        llListen(CHANNEL, "", NULL_KEY, "");
        llSetTimerEvent(1.0);
    }

    listen(integer channel, string name, key id, string message)
    {
        ++heard;
    }

    timer()
    {
        ++ticks;
        if (ticks == 3)
        {
            emit("default", " ticks=" + (string)ticks + " heard=" + (string)heard);
            integer i;
            for (i = 0; i < QUEUED; ++i)
            {
                llMessageLinked(LINK_THIS, i, "", "");
            }
            state two;
        }
    }

    state_exit()
    {
        ++exits;
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
        ticks = 0;
        heard = 0;
        emit("entered", " exits=" + (string)exits);
    }

    link_message(integer sender, integer num, string msg, key id)
    {
        ++linked;
    }

    listen(integer channel, string name, key id, string message)
    {
        ++heard;
    }

    timer()
    {
        ++ticks;
        if (ticks == 3)
        {
            emit("two", " ticks=" + (string)ticks + " heard=" + (string)heard
                + " queued=" + (string)QUEUED + " linked=" + (string)linked);
            heard = 0;
            llListen(CHANNEL, "", NULL_KEY, "");
        }
        else if (ticks == 6)
        {
            llSetTimerEvent(0.0);
            emit("relistened", " heard=" + (string)heard);
            emit("end", "");
        }
    }

    touch_start(integer count)
    {
        llResetScript();
    }
}
