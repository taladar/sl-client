// probe: events/timer-reset
// script: Probe
// prim: A
// order: 2
// drive: none
// vary: took
//
// What happens to a timer that falls due while the script is busy, when
// the script sets its timer again or stops it before it is free; where a
// `changed` raised meanwhile queues against it; and how long the media
// setters keep a script busy.
//
// Busy in another handler: each round sets a short timer from the round's
// link message handler, keeps the script busy past it, and leaves the timer
// be, sets a three-second one or stops it. Rounds `sleep-keep`,
// `sleep-reset`, `sleep-stop` (a 0.2 s timer, a one-second llSleep);
// `color-keep` (a 0.3 s timer, a colour change, a one-second llSleep);
// `media-keep`, `media-reset` (a 0.3 s timer, llSetPrimMediaParams);
// `clear-keep` (a 0.3 s timer, llClearPrimMedia); `media-get`
// (llGetPrimMediaParams, no timer).
//
// Busy in the timer's own handler: the round sets a 0.3 s timer, and the
// first `timer` keeps the script busy. Rounds `in-timer-keep` (a one-second
// llSleep), `in-timer-reset` (the same, then a three-second timer),
// `in-timer-media` (llSetPrimMediaParams, then a one-second timer: the
// shape of `prim/changed`) and `in-timer-media-keep` (llSetPrimMediaParams
// alone).
//
// `events` lists what ran after the busy handler returned, each with the
// tenths of a second since: a timer still queued runs at once (`timer@0`;
// anything within a quarter of a second counts as at once, since Second
// Life's frames put it one or a few frames late), one replaced or stopped
// runs later or not at all. Every timer after the
// busy one stops the timer, so a round shows one at most. `took` is how long
// the busy call kept the script, `tenths` the same in tenths of a second.
// Each round watches four seconds, timed by the clock script, after a
// second's gap in which anything late is counted as `stray` against the
// round that follows.
string ID = "events/timer-reset";
integer WAIT = 800;
integer DONE = 801;

list ROUNDS = ["sleep-keep", "sleep-reset", "sleep-stop", "color-keep", "media-keep",
    "media-reset", "clear-keep", "media-get", "in-timer-keep", "in-timer-reset",
    "in-timer-media", "in-timer-media-keep"];

integer round = -1;
integer watching;
integer pending;
float t0;
float took;
list got;
integer stray;

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
    if (watching)
    {
        float since = llGetTime() - t0;
        integer tenths = llRound(since * 10.0);
        if (since < 0.25)
        {
            tenths = 0;
        }
        got += [what + "@" + (string)tenths];
    }
    else
    {
        ++stray;
    }
}

watch()
{
    watching = TRUE;
    t0 = llGetTime();
    llMessageLinked(LINK_THIS, WAIT, "4.0", "");
}

act(string label)
{
    float t;
    if (label == "sleep-keep" || label == "sleep-reset" || label == "sleep-stop")
    {
        llSetTimerEvent(0.2);
        t = llGetTime();
        llSleep(1.0);
        took = llGetTime() - t;
        if (label == "sleep-reset")
            llSetTimerEvent(3.0);
        else if (label == "sleep-stop")
            llSetTimerEvent(0.0);
    }
    else if (label == "color-keep")
    {
        llSetTimerEvent(0.3);
        t = llGetTime();
        llSetColor(<0.0, 1.0, 0.0>, ALL_SIDES);
        llSleep(1.0);
        took = llGetTime() - t;
    }
    else if (label == "media-keep" || label == "media-reset")
    {
        llSetTimerEvent(0.3);
        t = llGetTime();
        llSetPrimMediaParams(0, [PRIM_MEDIA_HOME_URL, "http://example.com/" + label]);
        took = llGetTime() - t;
        if (label == "media-reset")
            llSetTimerEvent(3.0);
    }
    else if (label == "clear-keep")
    {
        llSetTimerEvent(0.3);
        t = llGetTime();
        llClearPrimMedia(0);
        took = llGetTime() - t;
    }
    else if (label == "media-get")
    {
        t = llGetTime();
        llGetPrimMediaParams(0, [PRIM_MEDIA_HOME_URL]);
        took = llGetTime() - t;
    }
}

// The busy part of an `in-timer-*` round, run by its first timer.
busy(string label)
{
    float t = llGetTime();
    if (label == "in-timer-keep" || label == "in-timer-reset")
    {
        llSleep(1.0);
        took = llGetTime() - t;
        if (label == "in-timer-reset")
            llSetTimerEvent(3.0);
    }
    else
    {
        llSetPrimMediaParams(0, [PRIM_MEDIA_HOME_URL, "http://example.com/" + label]);
        took = llGetTime() - t;
        if (label == "in-timer-media")
            llSetTimerEvent(1.0);
    }
}

gap()
{
    watching = FALSE;
    pending = FALSE;
    stray = 0;
    llSetTimerEvent(0.0);
    llMessageLinked(LINK_THIS, WAIT, "1.0", "");
}

default
{
    state_entry()
    {
        begin();
        llSetColor(<1.0, 1.0, 1.0>, ALL_SIDES);
        llClearPrimMedia(0);
        gap();
    }

    link_message(integer sender, integer num, string msg, key id)
    {
        if (num != DONE)
        {
            return;
        }
        if (watching || pending)
        {
            emit("round", " label=" + llList2String(ROUNDS, round)
                + " events=" + q(llDumpList2String(got, ","))
                + " tenths=" + (string)llRound(took * 10.0)
                + " took=" + (string)took
                + " stray=" + (string)stray);
            gap();
            return;
        }
        ++round;
        if (round >= llGetListLength(ROUNDS))
        {
            emit("end", "");
            return;
        }
        got = [];
        took = 0.0;
        string label = llList2String(ROUNDS, round);
        if (llGetSubString(label, 0, 8) == "in-timer-")
        {
            pending = TRUE;
            llSetTimerEvent(0.3);
            return;
        }
        act(label);
        watch();
    }

    timer()
    {
        if (pending && !watching)
        {
            busy(llList2String(ROUNDS, round));
            watch();
            return;
        }
        llSetTimerEvent(0.0);
        note("timer");
    }

    changed(integer change)
    {
        note("changed" + (string)change);
    }
}
