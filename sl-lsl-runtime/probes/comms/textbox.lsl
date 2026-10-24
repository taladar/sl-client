// probe: comms/textbox
// drive: dialog, touch
//
// What llTextBox puts to an avatar and what the script hears back. The
// script listens on its channel to anybody. A text box the runner answers
// is announced with a `waiting` record and the text it should type, and
// the step moves on when the text is heard (or after fifteen seconds); one
// that may be refused is put unannounced, and the step moves on after four
// seconds. The runner notes what arrives (a text box is a dialog with the
// one button "!!llTextBox!!"). The steps: a text box answered with a few
// words; one answered with nothing; one answered with 300 characters; an
// empty message; a message of 513 characters; then the other avatar
// touches, and a text box to it is answered by it.
string ID = "comms/textbox";
integer CHANNEL = -731002;

integer step;
key other;

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

string who(key id)
{
    if (id == llGetOwner())
    {
        return "owner";
    }
    if (id == other)
    {
        return "other";
    }
    return "someone";
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

put(key agent, string message, string how)
{
    emit("put", " step=" + (string)step + " to=" + who(agent) + " how=" + how
        + " message_length=" + (string)llStringLength(message));
    llTextBox(agent, message, CHANNEL);
}

answered(key agent, string message, string reply)
{
    string by;
    if (agent != llGetOwner())
    {
        by = " by=other";
    }
    emit("waiting", " drive=dialog" + by + " reply=" + q(reply));
    put(agent, message, "answered");
    llSetTimerEvent(15.0);
}

unanswered(key agent, string message)
{
    put(agent, message, "unanswered");
    llSetTimerEvent(4.0);
}

next()
{
    step = step + 1;
    key owner = llGetOwner();
    if (step == 1)
    {
        answered(owner, "Type something", "a few typed words");
    }
    else if (step == 2)
    {
        answered(owner, "Type nothing", "");
    }
    else if (step == 3)
    {
        answered(owner, "Type a lot", repeat("x", 300));
    }
    else if (step == 4)
    {
        unanswered(owner, "");
    }
    else if (step == 5)
    {
        unanswered(owner, repeat("m", 513));
    }
    else if (step == 6)
    {
        llSetTimerEvent(15.0);
        emit("waiting", " drive=touch by=other");
    }
    else if (step == 7)
    {
        answered(other, "For the other", "typed by the other");
    }
    else
    {
        llSetTimerEvent(0.0);
        emit("end", "");
    }
}

default
{
    state_entry()
    {
        begin();
        llListen(CHANNEL, "", NULL_KEY, "");
        next();
    }

    listen(integer channel, string name, key id, string message)
    {
        emit("heard", " step=" + (string)step + " from=" + who(id)
            + " length=" + (string)llStringLength(message)
            + " message=" + q(llGetSubString(message, 0, 39)));
        llSetTimerEvent(0.0);
        next();
    }

    touch_start(integer count)
    {
        if (step == 6)
        {
            other = llDetectedKey(0);
            emit("touch", " step=" + (string)step + " by=" + who(other));
            llSetTimerEvent(0.0);
            next();
        }
    }

    timer()
    {
        llSetTimerEvent(0.0);
        emit("moved-on", " step=" + (string)step);
        next();
    }
}
