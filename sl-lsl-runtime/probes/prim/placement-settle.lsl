// probe: prim/placement-settle
// drive: none
// vary: polls, set, seen, after, rez, stamp, wall, t
//
// What prim/placement left open about where a move into the ground stops
// and when llGetBoundingBox follows a new scale, measured finely enough to
// tell the candidate rules apart.
//
// The ground: llSetRegionPos 30 m down from above a point, for prims of
// several sizes, shapes and rotations and at points between the terrain's
// whole-metre samples, printing in millimetres where the centre stopped
// above what llGround gives under it, and the ground at the four samples
// around it, so that a fixed lift, one that grows with the prim's size or
// its height, and a ground read another way than llGround all come out
// different. A move straight after a scale change shows which shape a stop
// is measured with.
//
// The box: llGetBoundingBox of a settled prim at small scales (whether the
// box has a least size of its own), and of one whose scale changed in the
// same event — straight after llSetScale, after llSetLinkPrimitiveParamsFast,
// after llSetPrimitiveParams with its forced delay, after sleeps of a frame
// and less, and in the next event, a link message to itself.
string ID = "prim/placement-settle";

vector home;
rotation home_rot;
vector home_scale;
integer stage;

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

// A number in millimetres.
string mm1(float f)
{
    return (string)(llRound(f * 1000.0) / 1000.0);
}

string mm(vector v)
{
    return "<" + mm1(v.x) + "," + mm1(v.y) + "," + mm1(v.z) + ">";
}

// The box of this prim, its corners in millimetres.
string box_now()
{
    list b = llGetBoundingBox(llGetKey());
    return " min=" + mm(llList2Vector(b, 0)) + " max=" + mm(llList2Vector(b, 1));
}

// Back home, at the home scale and rotation, settled for a few frames.
reset()
{
    llSetRegionPos(home);
    llSetRot(home_rot);
    llSetScale(home_scale);
    llSleep(0.5);
}

// Settle `s` at `r`, then move 30 m down from above `offset` (east and
// north of home), and print where the centre stopped above the ground.
drop(string label, vector s, rotation r, vector offset)
{
    llSetScale(s);
    llSetRot(r);
    llSetRegionPos(home + offset);
    llSleep(0.5);
    integer ok = llSetRegionPos(home + offset - <0.0, 0.0, 30.0>);
    report(label, ok);
}

report(string label, integer ok)
{
    vector p = llGetPos();
    float x0 = (float)llFloor(p.x);
    float y0 = (float)llFloor(p.y);
    float g = llGround(ZERO_VECTOR);
    emit("drop", " label=" + label + " answer=" + (string)ok
        + " scale=" + mm(llGetScale())
        + " at=" + mm(<p.x - home.x, p.y - home.y, 0.0>)
        + " above=" + mm1(p.z - g)
        + " ground=" + mm1(g - home.z)
        + " g00=" + mm1(llGround(<x0 - p.x, y0 - p.y, 0.0>) - home.z)
        + " g10=" + mm1(llGround(<x0 + 1.0 - p.x, y0 - p.y, 0.0>) - home.z)
        + " g01=" + mm1(llGround(<x0 - p.x, y0 + 1.0 - p.y, 0.0>) - home.z)
        + " g11=" + mm1(llGround(<x0 + 1.0 - p.x, y0 + 1.0 - p.y, 0.0>) - home.z));
    reset();
}

settled(string label, vector s)
{
    llSetScale(s);
    llSleep(0.5);
    emit("settled", " label=" + label + " scale=" + mm(llGetScale()) + box_now());
}

default
{
    state_entry()
    {
        begin();
        home = llGetPos();
        home_rot = llGetRot();
        home_scale = llGetScale();
        emit("home", " scale=" + mm(home_scale)
            + " ground=" + mm1(llGround(ZERO_VECTOR) - home.z)
            + " frac=" + mm(<home.x - llFloor(home.x), home.y - llFloor(home.y), 0.0>)
            // OBJECT_REZ_TIME, which XEngine does not name.
            + " rez=" + llList2String(llGetObjectDetails(llGetKey(), [36]), 0)
            + " stamp=" + llGetTimestamp() + " t=" + (string)llGetTime());

        // Where a move into the ground stops.
        rotation lying = llEuler2Rot(<PI_BY_TWO, 0.0, 0.0>);
        rotation tilted = llEuler2Rot(<PI / 4.0, 0.0, 0.0>);
        // From home to the terrain sample south-west of it. (XEngine's
        // parser refuses a vector literal less a variable in an argument.)
        vector frac = <home.x - llFloor(home.x), home.y - llFloor(home.y), 0.0>;
        drop("cube-0.5", <0.5, 0.5, 0.5>, home_rot, ZERO_VECTOR);
        drop("cube-0.1", <0.1, 0.1, 0.1>, home_rot, ZERO_VECTOR);
        drop("cube-0.01", <0.01, 0.01, 0.01>, home_rot, ZERO_VECTOR);
        drop("cube-2", <2.0, 2.0, 2.0>, home_rot, ZERO_VECTOR);
        drop("cube-8", <8.0, 8.0, 8.0>, home_rot, ZERO_VECTOR);
        drop("flat", <2.0, 2.0, 0.1>, home_rot, ZERO_VECTOR);
        drop("tall", <0.1, 0.1, 4.0>, home_rot, ZERO_VECTOR);
        drop("tall-lying", <0.1, 0.1, 4.0>, lying, ZERO_VECTOR);
        drop("cube-2-tilted", <2.0, 2.0, 2.0>, tilted, ZERO_VECTOR);
        drop("cube-0.5-at-0.5", <0.5, 0.5, 0.5>, home_rot, <0.5 - frac.x, 0.5 - frac.y, 0.0>);
        drop("cube-0.5-at-0.25-0.75", <0.5, 0.5, 0.5>, home_rot, <0.25 - frac.x, 0.75 - frac.y, 0.0>);
        drop("cube-0.5-at-0.9-0.1", <0.5, 0.5, 0.5>, home_rot, <3.9 - frac.x, 1.1 - frac.y, 0.0>);

        // A move straight after a scale change: which shape is the stop
        // measured with?
        llSetScale(<4.0, 4.0, 4.0>);
        llSleep(0.5);
        llSetScale(<0.1, 0.1, 0.1>);
        integer ok = llSetRegionPos(home - <0.0, 0.0, 30.0>);
        report("rescaled-4-to-0.1", ok);
        llSetScale(<0.1, 0.1, 0.1>);
        llSleep(0.5);
        llSetScale(<4.0, 4.0, 4.0>);
        ok = llSetRegionPos(home - <0.0, 0.0, 30.0>);
        report("rescaled-0.1-to-4", ok);

        // llSetPos into the ground, capped at 10 m, from 5 m above it.
        llSetScale(<2.0, 2.0, 2.0>);
        llSetRegionPos(<home.x, home.y, llGround(ZERO_VECTOR) + 5.0>);
        llSleep(0.5);
        llSetPos(llGetPos() - <0.0, 0.0, 9.0>);
        report("setpos-cube-2", 1);

        // The box of a settled prim at small scales.
        settled("x-0.01", <0.01, 1.0, 1.0>);
        settled("x-0.02", <0.02, 1.0, 1.0>);
        settled("x-0.04", <0.04, 1.0, 1.0>);
        settled("x-0.05", <0.05, 1.0, 1.0>);
        settled("x-0.06", <0.06, 1.0, 1.0>);
        settled("x-0.1", <0.1, 1.0, 1.0>);
        settled("cube-0.01", <0.01, 0.01, 0.01>);
        settled("cube-1", <1.0, 1.0, 1.0>);

        // The box straight after a scale change.
        llSetScale(<1.0, 1.0, 1.0>);
        llSleep(0.5);
        llSetScale(<2.0, 1.0, 1.0>);
        emit("lag", " label=set-scale scale=" + mm(llGetScale()) + box_now());
        llSetScale(<3.0, 1.0, 1.0>);
        emit("lag", " label=set-scale-again scale=" + mm(llGetScale()) + box_now());
        llSleep(0.01);
        emit("lag", " label=sleep-0.01 scale=" + mm(llGetScale()) + box_now());
        llSleep(0.05);
        emit("lag", " label=sleep-0.05 scale=" + mm(llGetScale()) + box_now());
        llSetLinkPrimitiveParamsFast(LINK_THIS, [PRIM_SIZE, <1.0, 2.0, 1.0>]);
        emit("lag", " label=params-fast scale=" + mm(llGetScale()) + box_now());
        llSleep(0.5);
        llSetPrimitiveParams([PRIM_SIZE, <1.0, 3.0, 1.0>]);
        emit("lag", " label=params-delayed scale=" + mm(llGetScale()) + box_now());
        llSleep(0.5);

        // When the box catches up: set a scale at a different moment of the
        // second each round, then read the box every frame until it shows
        // the new scale's x, and print both clocks.
        float x = 1.0;
        integer round;
        for (round = 0; round < 8; ++round)
        {
            llSleep(0.13 * (float)round);
            x = x + 0.5;
            llSetScale(<x, 1.0, 1.0>);
            float set_at = llGetTime();
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
            emit("cadence", " round=" + (string)round + " polls=" + (string)polls
                + " set=" + (string)set_at + " seen=" + (string)seen
                + " after=" + (string)(seen - set_at) + " wall=" + wall);
        }
        llSetScale(<1.0, 1.0, 1.0>);
        llSleep(0.5);

        llSetScale(<1.0, 1.0, 2.0>);
        stage = 1;
        llMessageLinked(LINK_THIS, 0, "", NULL_KEY);
    }

    link_message(integer sender, integer num, string str, key id)
    {
        if (stage != 1)
        {
            return;
        }
        stage = 2;
        emit("lag", " label=next-event scale=" + mm(llGetScale()) + box_now());
        reset();
        emit("after", " moved=" + mm(llGetPos() - home) + " scale=" + mm(llGetScale()));
        emit("end", "");
    }
}
