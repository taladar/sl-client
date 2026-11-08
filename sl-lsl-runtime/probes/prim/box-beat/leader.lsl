// probe: prim/box-beat
// script: Leader
// order: 2
// drive: none
// vary: rez, called, stamp, set, seen, wall, t, fps, dilation, polls
// unordered: beat, first, rezzed
// deadline: 180
//
// When llGetBoundingBox catches up with a new scale on Second Life, and
// whether each prim keeps its own clock for it. prim/placement-settle found
// the box catching up about once a second; this asks what that second counts
// from. Prim A first flips its own x between 1 and 2 m twenty times, at a
// different moment of the second each time, reading its box every frame
// until it shows the new x and printing the wall time and its script's clock
// at the set and where it saw it (label L). Then it rezzes the Child three
// times from its inventory (see there), A at once, B half a second later and
// C straight after B; each rescales itself once soon after its rez (`first`)
// and, on prim A's "go", flips its x twenty times the same way, its rounds
// overlapping the others'. Prim A prints what each tells it. Prims that
// share one clock for the region hold each other's boxes back; prims with
// their own do not. The region's frame rate at the start and the end says
// whether a second is 45 frames.
string ID = "prim/box-beat";

integer CHANNEL = -7351;
integer ready;
integer done;

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

frame(string rec)
{
    emit(rec, " fps=" + (string)llGetRegionFPS()
        + " dilation=" + (string)llGetRegionTimeDilation());
}

rez(string label, vector offset, integer param)
{
    llRezObject("Child", llGetPos() + offset, ZERO_VECTOR, ZERO_ROTATION, param);
    emit("rezzed", " label=" + label + " called=" + llGetTimestamp());
}

default
{
    state_entry()
    {
        begin();
        emit("home", " rez=" + llList2String(llGetObjectDetails(llGetKey(), [36]), 0)
            + " stamp=" + llGetTimestamp() + " t=" + (string)llGetTime());
        float x = 1.0;
        integer round;
        for (round = 0; round < 20; ++round)
        {
            llSleep(0.37 + 0.11 * (float)(round % 9));
            x = 3.0 - x;
            llSetScale(<x, 1.0, 1.0>);
            float set = llGetTime();
            string set_wall = llGetTimestamp();
            float seen = -1.0;
            string wall = "";
            integer polls = 0;
            while (seen < 0.0 && polls < 200)
            {
                vector top = llList2Vector(llGetBoundingBox(llGetKey()), 1);
                if (llFabs(top.x - x / 2.0) < 0.001)
                {
                    seen = llGetTime();
                    wall = llGetTimestamp();
                }
                else
                {
                    llSleep(0.01);
                    ++polls;
                }
            }
            emit("beat", " label=L round=" + (string)round + " polls=" + (string)polls
                + " set=" + set_wall + " wall=" + wall
                + " t=" + (string)set + " seen=" + (string)seen);
        }
        llSetScale(<0.5, 0.5, 0.5>);
        llListen(CHANNEL, "", NULL_KEY, "");
        rez("A", <1.0, 0.0, 1.0>, 1);
        llSleep(0.4);
        rez("B", <2.0, 0.0, 1.0>, 2);
        rez("C", <3.0, 0.0, 1.0>, 3);
        llSetTimerEvent(15.0);
    }

    listen(integer channel, string name, key id, string text)
    {
        if (text == "ready")
        {
            ++ready;
            if (ready == 3)
            {
                frame("go");
                llRegionSay(CHANNEL, "go");
                llSetTimerEvent(60.0);
            }
        }
        else if (text == "done")
        {
            ++done;
            if (done == 3)
            {
                llSetTimerEvent(0.0);
                frame("frames");
                emit("end", "");
            }
        }
        else if (llSubStringIndex(text, "rec=") == 0)
        {
            llOwnerSay("PROBE " + ID + " " + text);
        }
    }

    timer()
    {
        llSetTimerEvent(0.0);
        emit("timeout", " ready=" + (string)ready + " done=" + (string)done);
        emit("end", "");
    }
}
