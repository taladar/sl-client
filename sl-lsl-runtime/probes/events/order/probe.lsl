// probe: events/order
// script: Probe
// prim: A
// order: 3
// drive: touch
// vary: touches
//
// The order in which events of different types reach a script that was
// busy while they were raised. In each round the script starts the round's
// stimuli and sleeps; what was raised meanwhile waits in its queue, and the
// order the handlers then run in is printed as the round's `events`.
//
// Round 1: the helper (a second script in this prim) sends a link message,
// then changes the prim's colour, in one handler (one frame). Round 2: the
// colour first, then the link message. Round 3: link, colour, link, colour,
// 0.3 s apart. Round 4: this script starts a 0.3 s timer and a sensor for
// avatars, cues the talker (prim B) to say a line on the probe's channel
// and the helper to send a link message and change the colour, and asks for
// a touch — the order raised is about link and colour, then the sensor's
// answer and the talker's line, the timer at 0.3 s and the touch at about a
// second. Round 5 raises them the other way round: the touch at about a
// second, the helper's link message and colour at 2.5 s, the talker's line
// at 3 s, the timer at 3.5 s. A queue that is first in, first out prints
// each round in the order raised; one that ranks types prints rounds 4 and
// 5 alike.
//
// A round ends at a fence: a link message to itself after the sleep, whose
// handler waits a second more and sends a second, so that what is still on
// its way lands in front of it. `touches` is how many `touch` events came
// between `touch_start` and `touch_end`, a measurement of the frame rate.
string ID = "events/order";
integer TALK = -730101;
integer CUE = -730102;
integer GO = 100;
integer FENCE = 900;
integer FENCE2 = 901;

integer round;
list got;
integer touches;

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

note(string what)
{
    if (round > 0)
    {
        got += [what];
    }
}

go(integer which)
{
    round = which;
    got = [];
    touches = 0;
    if (which <= 3)
    {
        llMessageLinked(LINK_THIS, GO + which, "", "");
        llSleep(2.0);
    }
    else if (which == 4)
    {
        llSetTimerEvent(0.3);
        llSensor("", NULL_KEY, AGENT, 30.0, PI);
        llSay(CUE, "now");
        llMessageLinked(LINK_THIS, GO + which, "", "");
        emit("waiting", " drive=touch");
        llSleep(4.0);
    }
    else
    {
        emit("waiting", " drive=touch");
        llSay(CUE, "late");
        llMessageLinked(LINK_THIS, GO + which, "", "");
        llSetTimerEvent(3.5);
        llSleep(5.0);
    }
    llMessageLinked(LINK_THIS, FENCE, "", "");
}

default
{
    state_entry()
    {
        begin();
        round = 0;
        llListen(TALK, "", NULL_KEY, "");
        llSetTimerEvent(1.0);
    }

    timer()
    {
        llSetTimerEvent(0.0);
        if (round == 0)
        {
            go(1);
            return;
        }
        note("timer");
    }

    link_message(integer sender, integer num, string msg, key id)
    {
        if (num == FENCE)
        {
            llSleep(1.0);
            llMessageLinked(LINK_THIS, FENCE2, "", "");
            return;
        }
        if (num == FENCE2)
        {
            emit("order", " round=" + (string)round
                + " events=" + q(llDumpList2String(got, ","))
                + " touches=" + (string)touches);
            if (round < 5)
            {
                go(round + 1);
            }
            else
            {
                emit("end", "");
            }
            return;
        }
        if (num >= GO && num < FENCE)
        {
            return;
        }
        note("link" + (string)num);
    }

    changed(integer change)
    {
        note("changed" + (string)change);
    }

    listen(integer channel, string name, key id, string message)
    {
        note("listen");
    }

    sensor(integer count)
    {
        note("sensor");
    }

    no_sensor()
    {
        note("no_sensor");
    }

    touch_start(integer count)
    {
        note("touch_start");
    }

    touch(integer count)
    {
        ++touches;
    }

    touch_end(integer count)
    {
        note("touch_end");
    }
}
