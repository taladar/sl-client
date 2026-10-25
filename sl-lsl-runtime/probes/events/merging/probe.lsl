// probe: events/merging
// script: Probe
// prim: A
// order: 3
// drive: touch
// vary: touches
//
// Whether events of one type raised while a script is busy arrive one by
// one or merged into fewer. In each round the script cues the round's
// stimuli and sleeps three seconds; the handlers that then run are printed
// in order as the round's `events`, a `changed` with its flags.
//
// 1 `color-burst`: the helper (a second script in this prim) changes the
//   colour five times in one handler.
// 2 `color-same`: three times to the colour it already has.
// 3 `mixed-change`: colour, scale and texture (from the default plywood to
//   blank) in one handler.
// 4 `color-spaced`: three colours half a second apart.
// 5 `link-burst`: five identical link messages in one handler.
// 6 `listen-same`: the talker (prim B) says one line five times in one
//   handler.
// 7 `listen-distinct`: five different lines.
// 8 `sensor-burst`: this script calls `llSensor` three times.
// 9 `touch-burst`: three touches, a second or so apart, while it sleeps
//   seven seconds.
//
// A round ends at a fence (as in `events/order`). `touches` is how many
// `touch` events came, a measurement of the frame rate.
string ID = "events/merging";
integer TALK = -730201;
integer CUE = -730202;
integer GO = 100;
integer FENCE = 900;
integer FENCE2 = 901;
list NAMES = ["color-burst", "color-same", "mixed-change", "color-spaced",
    "link-burst", "listen-same", "listen-distinct", "sensor-burst",
    "touch-burst"];

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
    float rest = 3.0;
    if (which <= 5)
    {
        llMessageLinked(LINK_THIS, GO + which, "", "");
    }
    else if (which <= 7)
    {
        llSay(CUE, (string)which);
    }
    else if (which == 8)
    {
        llSensor("", NULL_KEY, AGENT, 30.0, PI);
        llSensor("", NULL_KEY, AGENT, 30.0, PI);
        llSensor("", NULL_KEY, AGENT, 30.0, PI);
    }
    else
    {
        emit("waiting", " drive=touch");
        emit("waiting", " drive=touch");
        emit("waiting", " drive=touch");
        rest = 7.0;
    }
    llSleep(rest);
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
        go(1);
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
            emit("merged", " round=" + (string)round
                + " name=" + llList2String(NAMES, round - 1)
                + " events=" + q(llDumpList2String(got, ","))
                + " touches=" + (string)touches);
            if (round < llGetListLength(NAMES))
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
        note("listen:" + message);
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
        note("touch_start" + (string)count);
    }

    touch(integer count)
    {
        ++touches;
    }

    touch_end(integer count)
    {
        note("touch_end" + (string)count);
    }
}
