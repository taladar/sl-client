// probe: comms/dialog
// drive: dialog, touch
//
// What llDialog puts to an avatar, what comes back, and what it refuses.
// The script listens on its dialog channel to anybody and goes through its
// steps. A dialog the runner should answer is announced with a `waiting`
// record and the reply it should send, and the step moves on when the reply
// is heard (or after fifteen seconds); a dialog that may be refused is put
// without a `waiting` record, and the step moves on after four seconds. The
// runner notes every dialog that arrives: the message, the buttons in the
// order the grid sent them, and the channel. The steps: three buttons,
// answered with the second; no buttons at all (Second Life shows "OK");
// two buttons answered with a label the dialog does not have; twelve
// buttons; thirteen; a label of 24 characters, answered; one of 25; an
// empty label; a button that is an integer; an empty message; a message of
// 511 characters, of 512, and of 513; a dialog to NULL_KEY; then the other
// avatar touches, and a dialog to it is answered by it.
string ID = "comms/dialog";
integer CHANNEL = -731001;

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

list numbered(integer count)
{
    list out;
    integer i;
    for (i = 1; i <= count; ++i)
    {
        out += ["B" + (string)i];
    }
    return out;
}

// A dialog the runner answers with `reply`, as `by` (the owner or the
// other avatar).
answered(key agent, string message, list buttons, string reply)
{
    string by;
    if (agent != llGetOwner())
    {
        by = " by=other";
    }
    emit("waiting", " drive=dialog" + by + " reply=" + q(reply));
    put(agent, message, buttons, "answered");
    llSetTimerEvent(15.0);
}

// A dialog that may be refused: no reply asked for.
unanswered(key agent, string message, list buttons)
{
    put(agent, message, buttons, "unanswered");
    llSetTimerEvent(4.0);
}

put(key agent, string message, list buttons, string how)
{
    emit("put", " step=" + (string)step + " to=" + who(agent) + " how=" + how
        + " buttons=" + (string)llGetListLength(buttons)
        + " message_length=" + (string)llStringLength(message));
    llDialog(agent, message, buttons, CHANNEL);
}

next()
{
    step = step + 1;
    key owner = llGetOwner();
    if (step == 1)
    {
        answered(owner, "Pick one", ["One", "Two", "Three"], "Two");
    }
    else if (step == 2)
    {
        answered(owner, "No buttons", [], "OK");
    }
    else if (step == 3)
    {
        answered(owner, "Two buttons", ["One", "Two"], "Not a button");
    }
    else if (step == 4)
    {
        answered(owner, "Twelve", numbered(12), "B12");
    }
    else if (step == 5)
    {
        unanswered(owner, "Thirteen", numbered(13));
    }
    else if (step == 6)
    {
        answered(owner, "Long label", [repeat("x", 24)], repeat("x", 24));
    }
    else if (step == 7)
    {
        unanswered(owner, "Too long a label", [repeat("y", 25)]);
    }
    else if (step == 8)
    {
        unanswered(owner, "Empty label", ["One", ""]);
    }
    else if (step == 9)
    {
        unanswered(owner, "An integer button", ["One", 2]);
    }
    else if (step == 10)
    {
        unanswered(owner, "", ["One"]);
    }
    else if (step == 11)
    {
        unanswered(owner, repeat("m", 511), ["One"]);
    }
    else if (step == 12)
    {
        unanswered(owner, repeat("m", 512), ["One"]);
    }
    else if (step == 13)
    {
        unanswered(owner, repeat("m", 513), ["One"]);
    }
    else if (step == 14)
    {
        unanswered(NULL_KEY, "To nobody", ["One"]);
    }
    else if (step == 15)
    {
        llSetTimerEvent(15.0);
        emit("waiting", " drive=touch by=other");
    }
    else if (step == 16)
    {
        answered(other, "For the other", ["Yes", "No"], "No");
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
            + " name_is_key2name=" + (string)(name == llKey2Name(id))
            + " message=" + q(message));
        llSetTimerEvent(0.0);
        next();
    }

    touch_start(integer count)
    {
        if (step == 15)
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
