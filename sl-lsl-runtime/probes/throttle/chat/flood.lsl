// probe: throttle/chat
// script: Flood
// order: 6
// drive: none
// vary: seconds, slowest, delays
//
// Whether a grid throttles an object's chat, and how. Prim A finds prim B,
// five metres east, by a hello on channel -7420 (the Tally answers), and
// then sends four kinds of chat, which the four Ears in prim B count between
// them (see there), each message `f|<kind>|<wave>|<index>`:
//
// - kind 0: 25 llSay on channel 0, in a row, timing each call — OpenSim's
//   source sleeps a script two seconds when it says more than eleven lines
//   on channel 0 within a second;
// - kind 1: llRegionSay, kind 2: llSay and kind 3: llRegionSayTo prim B, on
//   channels 10–13, 20–23 and 30–33 in turn (one per Ear): first a burst of
//   200 in a row (wave 0), then three seconds later ten waves of 40, a second
//   apart (waves 1–10), 600 messages in all.
//
// For each kind it prints how long the calls took and the slowest call (for
// kind 0, every call's time, to a tenth of a second, and how many took half
// a second or more); three seconds after the last, the Tally prints how
// many messages of each wave arrived.
string ID = "throttle/chat";

integer CHANNEL = -7420;
key ears;
integer phase;
integer kind;
integer waited;

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

string ms(float seconds)
{
    return (string)(llRound(seconds * 1000.0) / 1000.0);
}

float slowest;

// Sends `n` messages of one kind and wave in a row; returns how long it took.
float wave(integer which, integer n)
{
    float start = llGetTime();
    integer i;
    for (i = 0; i < n; ++i)
    {
        string text = "f|" + (string)kind + "|" + (string)which + "|" + (string)i;
        float before = llGetTime();
        if (kind == 1)
        {
            llRegionSay(10 + i % 4, text);
        }
        else if (kind == 2)
        {
            llSay(20 + i % 4, text);
        }
        else
        {
            llRegionSayTo(ears, 30 + i % 4, text);
        }
        float took = llGetTime() - before;
        if (took > slowest)
        {
            slowest = took;
        }
    }
    return llGetTime() - start;
}

say_on_zero()
{
    list delays = [];
    integer slow = 0;
    float start = llGetTime();
    integer i;
    for (i = 0; i < 25; ++i)
    {
        float before = llGetTime();
        llSay(0, "f|0|0|" + (string)i);
        float took = llGetTime() - before;
        delays += [llRound(took * 10.0) / 10.0];
        if (took >= 0.5)
        {
            ++slow;
        }
    }
    emit("flood", " kind=0 fn=llSay channel=0 calls=25 seconds=" + ms(llGetTime() - start)
        + " slow=" + (string)slow + " delays=" + q(llDumpList2String(delays, ",")));
}

flood()
{
    string fn = "llRegionSayTo";
    if (kind == 1)
    {
        fn = "llRegionSay";
    }
    else if (kind == 2)
    {
        fn = "llSay";
    }
    slowest = 0.0;
    float seconds = wave(0, 200);
    emit("flood", " kind=" + (string)kind + " fn=" + fn + " wave=0 calls=200 seconds="
        + ms(seconds) + " slowest=" + ms(slowest));
    llSleep(3.0);
    slowest = 0.0;
    seconds = 0.0;
    integer w;
    for (w = 1; w <= 10; ++w)
    {
        seconds += wave(w, 40);
        llSleep(1.0);
    }
    emit("flood", " kind=" + (string)kind + " fn=" + fn + " waves=1-10 calls=400 seconds="
        + ms(seconds) + " slowest=" + ms(slowest));
}

default
{
    state_entry()
    {
        begin();
        llListen(CHANNEL, "", NULL_KEY, "");
        llRegionSay(CHANNEL, "hello");
        llSetTimerEvent(2.0);
    }

    listen(integer channel, string name, key id, string text)
    {
        if (phase == 0 && llGetSubString(text, 0, 4) == "here|")
        {
            ears = (key)llGetSubString(text, 5, -1);
            emit("ears", " found=1");
            phase = 1;
            llSetTimerEvent(1.0);
        }
        else if (phase == 2 && text == "done|" + (string)kind)
        {
            ++kind;
            phase = 1;
            llSetTimerEvent(1.0);
        }
    }

    timer()
    {
        llSetTimerEvent(0.0);
        if (phase == 0)
        {
            ++waited;
            if (waited < 10)
            {
                llRegionSay(CHANNEL, "hello");
                llSetTimerEvent(2.0);
                return;
            }
            emit("ears", " found=0");
            emit("end", "");
            phase = 9;
            return;
        }
        if (phase == 1)
        {
            if (kind > 3)
            {
                emit("end", "");
                phase = 9;
                return;
            }
            if (kind == 0)
            {
                say_on_zero();
            }
            else
            {
                flood();
            }
            phase = 3;
            llSetTimerEvent(3.0);
            return;
        }
        if (phase == 3)
        {
            llRegionSayTo(ears, CHANNEL, "report|" + (string)kind);
            phase = 2;
            llSetTimerEvent(10.0);
            return;
        }
        if (phase == 2)
        {
            emit("heard", " kind=" + (string)kind + " report=none");
            ++kind;
            phase = 1;
            llSetTimerEvent(1.0);
        }
    }
}
