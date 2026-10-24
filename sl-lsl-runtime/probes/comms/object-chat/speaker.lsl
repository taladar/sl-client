// probe: comms/object-chat
// script: Speaker
// order: 1
// drive: none
// unordered: heard, ready
//
// What one object hears of another's chat. Prim A holds this script and a
// listener (Self); six more prims stand east of it, each with a listener,
// one just inside and one just outside each documented range: 9 m and 11 m
// (whisper, 10 m), 19 m and 21 m (say, 20 m), and 99 m and 101 m (shout,
// 100 m), the last two straight above prim A. Each listener says it is ready on channel 6 with llRegionSay, and
// Self also by link message, so the script learns their keys (and whether
// it hears its own prim's chat), and says "go" on 6, at which they listen
// on channel 0 as well (a grid's own start-up lines on 0 are not this
// probe's). Then, a second and a half apart: a
// whisper, a say, a shout and a region-say on channel 5; llRegionSayTo the
// listener at 21 m, the one at 101 m and the script's own prim on 5, and
// the one at 21 m on 0; a region-say and a say on 0; a say of 1100
// characters on 5; llRegionSayTo NULL_KEY; and on 9, which the listener at
// 9 m hears filtered to the message "match", a "match" and a "nomatch".
// Every listener prints what it heard.
string ID = "comms/object-chat";

integer LISTENERS = 6;

integer step;
integer heard_ready;
key near;
key far;
integer self_ready;

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

string repeat(string unit, integer count)
{
    string out;
    integer i;
    for (i = 0; i < count; ++i)
    {
        out += unit;
    }
    return out;
}

said(string call, integer channel, string message)
{
    emit("said", " step=" + (string)step + " call=" + call
        + " channel=" + (string)channel
        + " length=" + (string)llStringLength(message));
}

next()
{
    step = step + 1;
    if (step == 1)
    {
        said("whisper", 5, "whisper");
        llWhisper(5, "whisper");
    }
    else if (step == 2)
    {
        said("say", 5, "say");
        llSay(5, "say");
    }
    else if (step == 3)
    {
        said("shout", 5, "shout");
        llShout(5, "shout");
    }
    else if (step == 4)
    {
        said("region-say", 5, "region");
        llRegionSay(5, "region");
    }
    else if (step == 5)
    {
        said("region-say-to-21", 5, "to 21");
        llRegionSayTo(near, 5, "to 21");
    }
    else if (step == 6)
    {
        said("region-say-to-101", 5, "to 101");
        llRegionSayTo(far, 5, "to 101");
    }
    else if (step == 7)
    {
        said("region-say-to-self", 5, "to self");
        llRegionSayTo(llGetKey(), 5, "to self");
    }
    else if (step == 8)
    {
        said("region-say-to-21", 0, "to 21 on 0");
        llRegionSayTo(near, 0, "to 21 on 0");
    }
    else if (step == 9)
    {
        said("region-say", 0, "region on 0");
        llRegionSay(0, "region on 0");
    }
    else if (step == 10)
    {
        said("say", 0, "say on 0");
        llSay(0, "say on 0");
    }
    else if (step == 11)
    {
        string long = repeat("l", 1100);
        said("say", 5, long);
        llSay(5, long);
    }
    else if (step == 12)
    {
        said("region-say-to-null", 5, "to null");
        llRegionSayTo(NULL_KEY, 5, "to null");
    }
    else if (step == 13)
    {
        said("say", 9, "match");
        llSay(9, "match");
        said("say", 9, "nomatch");
        llSay(9, "nomatch");
    }
    else if (step == 14)
    {
        // the last lines' grace
    }
    else
    {
        llSetTimerEvent(0.0);
        emit("end", "");
        return;
    }
    llSetTimerEvent(1.5);
}

begun()
{
    if (step == 0 && heard_ready == LISTENERS && self_ready)
    {
        emit("listeners", " ready=1");
        llRegionSay(6, "go");
        next();
    }
}

default
{
    state_entry()
    {
        begin();
        llListen(6, "", NULL_KEY, "");
        llSetTimerEvent(20.0);
    }

    listen(integer channel, string name, key id, string message)
    {
        emit("ready", " message=" + q(message)
            + " from_own_prim=" + (string)(id == llGetKey()));
        if (llSubStringIndex(message, "ready At") == 0)
        {
            heard_ready = heard_ready + 1;
        }
        if (message == "ready At21")
        {
            near = id;
        }
        else if (message == "ready At101")
        {
            far = id;
        }
        begun();
    }

    link_message(integer sender, integer num, string text, key id)
    {
        if (text == "ready Self")
        {
            self_ready = TRUE;
            begun();
        }
    }

    timer()
    {
        if (step == 0)
        {
            emit("listeners", " ready=0 heard=" + (string)heard_ready
                + " self=" + (string)self_ready);
            llRegionSay(6, "go");
        }
        next();
    }
}
