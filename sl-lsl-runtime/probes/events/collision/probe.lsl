// probe: events/collision
// script: Probe
// prim: A
// order: 2
// drive: none
// vary: collisions, faller_collisions
//
// Collision events, from a prim that is hit. Prim B, two metres above this
// one, turns physical on the cue `drop`, falls onto it and comes to rest;
// five seconds later the cue `stop` makes it non-physical again, puts it
// back where it started and has it report what it raised itself. Each
// phase prints the `collision_start` and `collision_end` events this prim
// ran, in order, with the count detected, whether the first detected is
// prim B and its `llDetectedType`; the `collision` events between are
// counted as `collisions`, a measurement of the frame rate.
//
// Phase 1 `solid`: this prim solid. Phase 2 `volume-detect`: this prim
// under `llVolumeDetect(TRUE)`, which prim B falls through. Phase 3
// `asleep`: solid again, and this script asleep for three seconds while
// prim B lands, so that what is raised meanwhile waits in its queue — how
// many `collision` events a busy script gets.
string ID = "events/collision";
integer CUE = -730301;
integer BACK = -730302;
list NAMES = ["solid", "volume-detect", "asleep"];

key faller;
integer phase;
list got;
integer collisions;
string first;

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

detected(string type, integer count)
{
    got += [type + (string)count];
    if (first == "")
    {
        first = " first_is_faller=" + (string)(llDetectedKey(0) == faller)
            + " first_type=" + (string)llDetectedType(0);
    }
}

drop(integer which)
{
    phase = which;
    got = [];
    collisions = 0;
    first = "";
    llVolumeDetect(which == 2);
    llSay(CUE, "drop");
    if (which == 3)
    {
        llSleep(3.0);
    }
    llSetTimerEvent(5.0);
}

default
{
    state_entry()
    {
        begin();
        phase = 0;
        llListen(BACK, "", NULL_KEY, "");
        llSay(CUE, "where");
        llSetTimerEvent(10.0);
    }

    listen(integer channel, string name, key id, string message)
    {
        list words = llParseString2List(message, [" "], []);
        string word = llList2String(words, 0);
        if (word == "here" && phase == 0)
        {
            faller = id;
            drop(1);
        }
        else if (word == "report")
        {
            emit("phase", " phase=" + (string)phase
                + " name=" + llList2String(NAMES, phase - 1)
                + " events=" + q(llDumpList2String(got, ","))
                + first
                + " collisions=" + (string)collisions
                + " faller_start=" + llList2String(words, 1)
                + " faller_end=" + llList2String(words, 2)
                + " faller_land_start=" + llList2String(words, 3)
                + " faller_land_end=" + llList2String(words, 4)
                + " faller_collisions=" + llList2String(words, 5)
                + " faller_hit_probe=" + llList2String(words, 6));
            if (phase < llGetListLength(NAMES))
            {
                drop(phase + 1);
            }
            else
            {
                llVolumeDetect(FALSE);
                emit("end", "");
            }
        }
    }

    timer()
    {
        llSetTimerEvent(0.0);
        if (phase == 0)
        {
            emit("no-faller", "");
            emit("end", "");
            return;
        }
        llSay(CUE, "stop");
    }

    collision_start(integer count)
    {
        detected("collision_start", count);
    }

    collision(integer count)
    {
        ++collisions;
    }

    collision_end(integer count)
    {
        detected("collision_end", count);
    }
}
