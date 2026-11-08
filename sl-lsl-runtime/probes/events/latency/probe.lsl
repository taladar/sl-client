// probe: events/latency
// script: Probe
// prim: A
// order: 3
// drive: none
// vary: t
//
// How long after the call that raised it an event reaches the script, for
// each kind of event `events/order` finds out of place. Each trial starts
// the clock, raises one event and ends the handler, and the handler that
// runs the event stops it (`t`, in seconds of `llGetTime`):
//
// - `link-self`: a link message to this prim, which this script hears;
// - `link-echo`: a link message to the echo, a second script in this prim,
//   which answers with one of its own — two hops;
// - `chat-echo`: a line said to the talker, prim B, 3 m away, which answers
//   with one of its own — two hops;
// - `sensor`: `llSensor` for avatars within 96 m, the owner standing by
//   (`event` says whether `sensor` or `no_sensor` came);
// - `changed`: `llSetColor` of this prim, a colour other than the one before.
//
// Two more measure the first hop alone, while this script is still busy in
// the handler that raised it: the echo (`link-busy`) or the talker
// (`chat-busy`) writes the trial's mark into its prim's description, and the
// handler polls the description until the mark is there (`t` = -1 after
// three seconds without it).
//
// Two more race a sweep's answer against another event while the script
// sleeps two seconds, to tell a late answer from a low rank in the queue:
// `llSensor` right after `llSetTimerEvent(delay)` (`sensor-timer`), or right
// after asking the echo to wait `delay` and send a link message
// (`sensor-link`), for delays of 0.1 to 1.5 s. `events` is the order the two
// handlers ran in.
//
// Each kind runs six trials, 0.4 s apart. Second Life's `llGetTime` moves in
// frames (1/45 s), so there `t` is a whole number of frames.
string ID = "events/latency";
integer ASK = -730201;
integer ANSWER = -730202;
integer TRIALS = 6;
list KINDS = ["link-self", "link-echo", "chat-echo", "sensor", "changed",
    "link-busy", "chat-busy", "sensor-timer", "sensor-link"];
list DELAYS = [0.1, 0.3, 0.6, 0.9, 1.2, 1.5];

integer kind;
integer trial;
float t0;
key talker;
integer found;
integer red;
list arrived;

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

string named()
{
    return llList2String(KINDS, kind);
}

record(float t, string extra)
{
    emit("latency", " kind=" + named() + " trial=" + (string)trial
        + extra + " t=" + (string)t);
    advance();
}

collect(string what)
{
    arrived += [what];
    if (llGetListLength(arrived) == 2)
    {
        emit("race", " kind=" + named() + " trial=" + (string)trial
            + " delay=" + (string)llList2Float(DELAYS, trial)
            + " events=" + q(llDumpList2String(arrived, ",")));
        advance();
    }
}

advance()
{
    ++trial;
    if (trial == TRIALS)
    {
        trial = 0;
        ++kind;
    }
    if (kind == llGetListLength(KINDS))
    {
        emit("end", "");
        return;
    }
    llSleep(0.4);
    start();
}

got(string extra)
{
    record(llGetTime() - t0, extra);
}

start()
{
    string which = named();
    string mark = which + (string)trial;
    if (which == "link-self")
    {
        t0 = llGetTime();
        llMessageLinked(LINK_THIS, 1, mark, "");
    }
    else if (which == "link-echo")
    {
        t0 = llGetTime();
        llMessageLinked(LINK_THIS, 2, mark, "");
    }
    else if (which == "chat-echo")
    {
        t0 = llGetTime();
        llSay(ASK, "ping " + mark);
    }
    else if (which == "sensor")
    {
        t0 = llGetTime();
        llSensor("", NULL_KEY, AGENT, 96.0, PI);
    }
    else if (which == "changed")
    {
        red = !red;
        t0 = llGetTime();
        if (red)
        {
            llSetColor(<1.0, 0.0, 0.0>, ALL_SIDES);
        }
        else
        {
            llSetColor(<0.0, 1.0, 0.0>, ALL_SIDES);
        }
    }
    else if (which == "sensor-timer")
    {
        arrived = [];
        llSetTimerEvent(llList2Float(DELAYS, trial));
        llSensor("", NULL_KEY, AGENT, 96.0, PI);
        llSleep(2.0);
    }
    else if (which == "sensor-link")
    {
        arrived = [];
        llMessageLinked(LINK_THIS, 5, (string)llList2Float(DELAYS, trial), "");
        llSensor("", NULL_KEY, AGENT, 96.0, PI);
        llSleep(2.0);
    }
    else if (which == "link-busy")
    {
        t0 = llGetTime();
        llMessageLinked(LINK_THIS, 4, mark, "");
        float t = -1.0;
        while (t < 0.0 && llGetTime() - t0 < 3.0)
        {
            if (llGetObjectDesc() == mark)
            {
                t = llGetTime() - t0;
            }
        }
        record(t, "");
    }
    else if (which == "chat-busy")
    {
        t0 = llGetTime();
        llSay(ASK, "mark " + mark);
        float t = -1.0;
        while (t < 0.0 && llGetTime() - t0 < 3.0)
        {
            if (llList2String(llGetObjectDetails(talker, [OBJECT_DESC]), 0)
                == mark)
            {
                t = llGetTime() - t0;
            }
        }
        record(t, "");
    }
}

default
{
    state_entry()
    {
        begin();
        llListen(ANSWER, "", NULL_KEY, "");
        llSetTimerEvent(1.0);
    }

    timer()
    {
        if (!found)
        {
            llSay(ASK, "hello");
        }
        else if (named() == "sensor-timer")
        {
            llSetTimerEvent(0.0);
            collect("timer");
        }
    }

    listen(integer channel, string name, key id, string message)
    {
        if (message == "here")
        {
            if (!found)
            {
                found = TRUE;
                talker = id;
                llSetTimerEvent(0.0);
                llSleep(0.4);
                start();
            }
        }
        else if (named() == "chat-echo"
            && message == "pong " + named() + (string)trial)
        {
            got("");
        }
    }

    link_message(integer sender, integer num, string msg, key id)
    {
        if (num == 6 && named() == "sensor-link")
        {
            collect("link");
            return;
        }
        if (msg != named() + (string)trial)
        {
            return;
        }
        if ((num == 1 && named() == "link-self")
            || (num == 3 && named() == "link-echo"))
        {
            got("");
        }
    }

    sensor(integer count)
    {
        if (named() == "sensor")
        {
            got(" event=sensor");
        }
        else if (named() == "sensor-timer" || named() == "sensor-link")
        {
            collect("sensor");
        }
    }

    no_sensor()
    {
        if (named() == "sensor")
        {
            got(" event=no_sensor");
        }
        else if (named() == "sensor-timer" || named() == "sensor-link")
        {
            collect("no_sensor");
        }
    }

    changed(integer change)
    {
        if (named() == "changed" && (change & CHANGED_COLOR))
        {
            got("");
        }
    }
}
