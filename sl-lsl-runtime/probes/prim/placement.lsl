// probe: prim/placement
// drive: none
//
// Where llSetPos, llSetRegionPos, llSetRot and llSetScale put a single prim
// that is not physical, on each grid. Every move starts from where the prim
// was rezzed and prints where it ended as an offset from there (`moved`),
// whether it moved at all, and what llSetRegionPos answered; then the prim is
// put back. llSetPos 5, 10, 15 and 50 m east and 15 m up; the same up 100 m
// in one call; 20 m down, which is below the ground; a target past the
// region's east edge and one past 4096 m. llSetRegionPos 50 m east, 100 m
// up, 20 m down, to 4000 m and to 5000 m; not past an edge, since Second Life then moves the
// prim off a region with no neighbour there and returns it to its owner's
// inventory (seen on aditi, 2026-10-27), which ends the probe. PRIM_POS_LOCAL
// of a root prim. Then the rotation: what llGetRot reads after llSetRot of a
// rotation that is not normalised, of a zero rotation and of one with a
// negative s; llSetLocalRot of a root prim. Then the scale: llSetScale under
// 0.01, at 0.01, at 64, past 64, past 256, of a negative or zero axis, and
// PRIM_SIZE the same; what llGetBoundingBox and llGetGeometricCenter read at
// a few scales, in the event that set the scale and a second later. Last,
// llTargetOmega read back through PRIM_OMEGA.
string ID = "prim/placement";

vector home;
rotation home_rot;
vector home_scale;

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

// A vector rounded to centimetres, so that a grid's float noise does not
// read as a difference.
string cm(vector v)
{
    return "<" + (string)(llRound(v.x * 100.0) / 100.0) + ","
        + (string)(llRound(v.y * 100.0) / 100.0) + ","
        + (string)(llRound(v.z * 100.0) / 100.0) + ">";
}

// A vector rounded to millimetres, for a scale near its least.
string mm(vector v)
{
    return "<" + (string)(llRound(v.x * 1000.0) / 1000.0) + ","
        + (string)(llRound(v.y * 1000.0) / 1000.0) + ","
        + (string)(llRound(v.z * 1000.0) / 1000.0) + ">";
}

string rot4(rotation r)
{
    return "<" + (string)(llRound(r.x * 10000.0) / 10000.0) + ","
        + (string)(llRound(r.y * 10000.0) / 10000.0) + ","
        + (string)(llRound(r.z * 10000.0) / 10000.0) + ","
        + (string)(llRound(r.s * 10000.0) / 10000.0) + ">";
}

// Where the prim stands now, from home, and whether it moved.
report(string label, string extra)
{
    vector p = llGetPos();
    emit("move", " label=" + label + extra + " moved=" + cm(p - home)
        + " any=" + (string)(llVecDist(p, home) > 0.01));
    llSetRegionPos(home);
}

// The ground under home, from home's height.
float ground_below()
{
    return llGround(ZERO_VECTOR) - home.z;
}

scale(string label, vector s)
{
    llSetScale(home_scale);
    llSetScale(s);
    emit("scale", " label=" + label + " asked=" + mm(s) + " got=" + mm(llGetScale()));
}

size(string label, vector s)
{
    llSetScale(home_scale);
    llSetPrimitiveParams([PRIM_SIZE, s]);
    list got = llGetPrimitiveParams([PRIM_SIZE]);
    emit("size", " label=" + label + " asked=" + mm(s) + " got="
        + mm(llList2Vector(got, 0)));
}

// The bounding box and the geometric centre in the event that set the scale,
// and again a second later.
box(string label, vector s)
{
    llSetScale(s);
    list b = llGetBoundingBox(llGetKey());
    emit("box", " label=" + label + " when=at-once scale=" + cm(llGetScale()) + " min="
        + cm(llList2Vector(b, 0)) + " max=" + cm(llList2Vector(b, 1)) + " centre="
        + cm(llGetGeometricCenter()));
    llSleep(1.0);
    b = llGetBoundingBox(llGetKey());
    emit("box", " label=" + label + " when=later scale=" + cm(llGetScale()) + " min="
        + cm(llList2Vector(b, 0)) + " max=" + cm(llList2Vector(b, 1)) + " centre="
        + cm(llGetGeometricCenter()));
}

default
{
    state_entry()
    {
        begin();
        home = llGetPos();
        home_rot = llGetRot();
        home_scale = llGetScale();
        emit("home", " scale=" + cm(home_scale) + " rot=" + rot4(home_rot)
            + " local=" + cm(llGetLocalPos() - home)
            + " root=" + cm(llGetRootPosition() - home)
            + " ground_below=" + (string)(llRound(ground_below() * 10.0) / 10.0)
            + " link=" + (string)llGetLinkNumber());

        // llSetPos: its 10 m cap and its other limits.
        llSetPos(home + <5.0, 0.0, 0.0>);
        report("setpos-east-5", "");
        llSetPos(home + <10.0, 0.0, 0.0>);
        report("setpos-east-10", "");
        llSetPos(home + <15.0, 0.0, 0.0>);
        report("setpos-east-15", "");
        llSetPos(home + <50.0, 0.0, 0.0>);
        report("setpos-east-50", "");
        llSetPos(home + <0.0, 0.0, 15.0>);
        report("setpos-up-15", "");
        llSetPos(home + <0.0, 0.0, 100.0>);
        report("setpos-up-100", "");
        llSetPos(home + <0.0, 0.0, -20.0>);
        report("setpos-down-20", "");
        vector east = <300.0, home.y, home.z>;
        llSetPos(east);
        report("setpos-past-east-edge", "");
        llSetPos(<home.x, home.y, 5000.0>);
        report("setpos-past-4096", "");
        llSetPrimitiveParams([PRIM_POS_LOCAL, home + <3.0, 0.0, 0.0>]);
        report("pos-local-root-3", "");

        // llSetRegionPos: no cap, its own limits.
        integer ok = llSetRegionPos(home + <50.0, 0.0, 0.0>);
        report("regionpos-east-50", " answer=" + (string)ok);
        ok = llSetRegionPos(home + <0.0, 0.0, 100.0>);
        report("regionpos-up-100", " answer=" + (string)ok);
        ok = llSetRegionPos(home + <0.0, 0.0, -20.0>);
        report("regionpos-down-20", " answer=" + (string)ok);
        ok = llSetRegionPos(<home.x, home.y, 4000.0>);
        report("regionpos-4000", " answer=" + (string)ok);
        ok = llSetRegionPos(<home.x, home.y, 5000.0>);
        report("regionpos-5000", " answer=" + (string)ok);
        ok = llSetRegionPos(home);
        report("regionpos-home", " answer=" + (string)ok);

        // llSetRot.
        llSetRot(<0.0, 0.0, 2.0, 2.0>);
        emit("rot", " label=not-normalised got=" + rot4(llGetRot()));
        llSetRot(home_rot);
        llSetRot(<0.0, 0.0, 0.0, 0.0>);
        emit("rot", " label=zero got=" + rot4(llGetRot()));
        llSetRot(home_rot);
        llSetRot(<0.0, 0.0, 0.0, -1.0>);
        emit("rot", " label=negative-s got=" + rot4(llGetRot()));
        llSetRot(home_rot);
        llSetRot(llEuler2Rot(<0.0, 0.0, PI_BY_TWO>));
        emit("rot", " label=quarter-z got=" + rot4(llGetRot())
            + " local=" + rot4(llGetLocalRot()) + " root=" + rot4(llGetRootRotation()));
        llSetRot(home_rot);
        llSetLocalRot(llEuler2Rot(<0.0, 0.0, PI_BY_TWO>));
        emit("rot", " label=local-quarter-z got=" + rot4(llGetRot()));
        llSetRot(home_rot);
        llSetPrimitiveParams([PRIM_ROTATION, <0.0, 0.0, 2.0, 2.0>]);
        emit("rot", " label=prim-rotation-not-normalised got=" + rot4(llGetRot()));
        llSetRot(home_rot);

        // llSetScale and PRIM_SIZE.
        scale("under-min", <0.005, 0.005, 0.005>);
        scale("min", <0.01, 0.01, 0.01>);
        scale("max", <64.0, 64.0, 64.0>);
        scale("past-max", <65.0, 1.0, 1.0>);
        scale("past-256", <300.0, 1.0, 1.0>);
        scale("negative", <-1.0, 1.0, 1.0>);
        scale("zero", <0.0, 1.0, 1.0>);
        size("under-min", <0.005, 0.005, 0.005>);
        size("past-max", <65.0, 1.0, 1.0>);
        size("negative", <-1.0, 1.0, 1.0>);
        llSetScale(home_scale);

        // The bounding box and the geometric centre.
        box("half", <0.5, 0.5, 0.5>);
        box("flat", <2.0, 1.0, 0.1>);
        llSetRot(llEuler2Rot(<0.0, 0.0, PI_BY_TWO>));
        box("flat-turned", <2.0, 1.0, 0.1>);
        llSetRot(home_rot);
        llSetScale(home_scale);

        // llTargetOmega and PRIM_OMEGA.
        list got = llGetPrimitiveParams([PRIM_OMEGA]);
        emit("omega", " label=fresh got=" + q(llList2CSV(got)));
        llTargetOmega(<0.0, 0.0, 1.0>, 2.0, 1.0);
        got = llGetPrimitiveParams([PRIM_OMEGA]);
        emit("omega", " label=target-omega got=" + q(llList2CSV(got))
            + " rot_moved=" + (string)(llAngleBetween(llGetRot(), home_rot) > 0.001));
        llTargetOmega(ZERO_VECTOR, 0.0, 0.0);
        got = llGetPrimitiveParams([PRIM_OMEGA]);
        emit("omega", " label=stopped got=" + q(llList2CSV(got)));
        llSetPrimitiveParams([PRIM_OMEGA, <1.0, 0.0, 0.0>, 3.0, 0.5]);
        got = llGetPrimitiveParams([PRIM_OMEGA]);
        emit("omega", " label=prim-omega got=" + q(llList2CSV(got)));
        llTargetOmega(ZERO_VECTOR, 0.0, 0.0);

        llSetRegionPos(home);
        llSetRot(home_rot);
        emit("after", " moved=" + cm(llGetPos() - home) + " scale=" + cm(llGetScale()));
        emit("end", "");
    }
}
