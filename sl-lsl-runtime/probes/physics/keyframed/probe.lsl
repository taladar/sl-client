// probe: physics/keyframed
// script: Probe
// order: 3
// drive: none
// vary: mid_dz
//
// llSetKeyframedMotion's constraints and errors, on each grid. Each try
// prints a `try` record, makes one call and, a second and a half later, an
// `outcome`: how far the prim rose and whether it turned, and how many
// moving_start and moving_end events it ran; then the motion is stopped and
// the prim put back. What a grid says about a call is its debug-channel
// chat, which the runner notes beside the try. First a valid motion on the
// fresh prim's physics shape, then the same once it is convex; a frame
// shorter than a tenth of a second, a list one element short, two frames
// (whether they add up), translation-only and rotation-only data, a rise and
// a turn in one frame, an unknown mode and option, reverse, a
// negative and an integer time, and a fast frame. Then a loop paused, played
// and stopped (where a stop leaves the prim), llSetPos and physics during a
// motion, a motion on a physical prim, and the commands with no motion.
// A position in the middle of a motion is a measurement. The elements'
// types swapped and a list the wrong shape for translation-only data are
// tried by the scripts in prims B and C (see there), because OpenSim throws
// on them.
string ID = "physics/keyframed";

vector home;
rotation home_rot;
integer step;
integer starts;
integer ends;
vector paused_at;

list UP = [<0.0, 0.0, 0.5>, ZERO_ROTATION, 1.0];
list LABELS = ["fresh-shape", "forward", "short-frame", "one-short", "two-frames",
    "translation-only", "rotation-only", "up-and-turn", "unknown-mode",
    "unknown-option", "reverse", "negative-time", "integer-time", "fast"];

// How far the prim stands above where it started.
float rise()
{
    vector p = llGetPos();
    return p.z - home.z;
}

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

string r3(float f)
{
    return (string)((float)llRound(f * 1000.0) / 1000.0);
}

string where()
{
    vector p = llGetPos();
    return " dz=" + r3(p.z - home.z)
        + " dx=" + r3(p.x - home.x)
        + " turned=" + (string)(llAngleBetween(llGetRot(), home_rot) > 0.01);
}

stop_and_restore()
{
    llSetKeyframedMotion([], [KFM_COMMAND, KFM_CMD_STOP]);
    llSetStatus(STATUS_PHYSICS, FALSE);
    llSetBuoyancy(0.0);
    llSetRegionPos(home);
    llSetRot(home_rot);
}

try(string label, list frames, list options)
{
    starts = 0;
    ends = 0;
    emit("try", " label=" + label);
    llSetKeyframedMotion(frames, options);
}

outcome(string label)
{
    emit("outcome", " label=" + label + where()
        + " moving_start=" + (string)starts + " moving_end=" + (string)ends);
}

// An outcome in the middle of a three-second motion: how high the prim
// stands then is a measurement; whether it rose, fell or went east is the
// reading.
midway(string label)
{
    vector p = llGetPos();
    emit("outcome", " label=" + label + " mid_dz=" + (string)(p.z - home.z)
        + " rose=" + (string)(p.z - home.z > 0.1)
        + " fell=" + (string)(p.z - home.z < -0.1)
        + " east=" + (string)(p.x - home.x > 0.5)
        + " moving_start=" + (string)starts + " moving_end=" + (string)ends);
}

act(integer s)
{
    rotation quarter = llEuler2Rot(<0.0, 0.0, PI_BY_TWO>);
    if (s == 0)
    {
        list shape = llGetPrimitiveParams([PRIM_PHYSICS_SHAPE_TYPE]);
        emit("shape", " shape_type=" + llList2String(shape, 0));
        try("fresh-shape", UP, []);
    }
    else if (s == 1)
    {
        llSetPrimitiveParams([PRIM_PHYSICS_SHAPE_TYPE, PRIM_PHYSICS_SHAPE_CONVEX]);
        try("forward", UP, []);
    }
    else if (s == 2)
    {
        try("short-frame", [<0.0, 0.0, 0.5>, ZERO_ROTATION, 0.05], []);
    }
    else if (s == 3)
    {
        try("one-short", [<0.0, 0.0, 0.5>, ZERO_ROTATION], []);
    }
    else if (s == 4)
    {
        try("two-frames", [<0.0, 0.0, 0.25>, ZERO_ROTATION, 0.5,
            <0.0, 0.0, 0.25>, ZERO_ROTATION, 0.5], []);
    }
    else if (s == 5)
    {
        try("translation-only", [<0.0, 0.0, 0.5>, 1.0], [KFM_DATA, KFM_TRANSLATION]);
    }
    else if (s == 6)
    {
        try("rotation-only", [quarter, 1.0], [KFM_DATA, KFM_ROTATION]);
    }
    else if (s == 7)
    {
        try("up-and-turn", [<0.0, 0.0, 0.5>, quarter, 1.0], []);
    }
    else if (s == 8)
    {
        try("unknown-mode", UP, [KFM_MODE, 7]);
    }
    else if (s == 9)
    {
        try("unknown-option", UP, [99, 1]);
    }
    else if (s == 10)
    {
        try("reverse", UP, [KFM_MODE, KFM_REVERSE]);
    }
    else if (s == 11)
    {
        try("negative-time", [<0.0, 0.0, 0.5>, ZERO_ROTATION, -1.0], []);
    }
    else if (s == 12)
    {
        try("integer-time", [<0.0, 0.0, 0.5>, ZERO_ROTATION, 1], []);
    }
    else if (s == 13)
    {
        try("fast", [<0.0, 0.0, 20.0>, ZERO_ROTATION, 0.2], []);
    }
    else if (s == 14)
    {
        try("loop", [<0.0, 0.0, 0.3>, ZERO_ROTATION, 0.5,
            <0.0, 0.0, -0.3>, ZERO_ROTATION, 0.5], [KFM_MODE, KFM_LOOP]);
    }
    else if (s == 15)
    {
        // The loop runs on; pause it where it is.
        emit("loop", " step=running mid_dz=" + (string)rise());
        llSetKeyframedMotion([], [KFM_COMMAND, KFM_CMD_PAUSE]);
        paused_at = llGetPos();
    }
    else if (s == 16)
    {
        emit("loop", " step=paused still=" + (string)(llVecDist(llGetPos(), paused_at) < 0.01)
            + " mid_dz=" + (string)(paused_at.z - home.z));
        paused_at = llGetPos();
        llSetKeyframedMotion([], [KFM_COMMAND, KFM_CMD_PLAY]);
    }
    else if (s == 17)
    {
        emit("loop", " step=played moved_on=" + (string)(llVecDist(llGetPos(), paused_at) > 0.01));
        llSetKeyframedMotion([], [KFM_COMMAND, KFM_CMD_STOP]);
        paused_at = llGetPos();
    }
    else if (s == 18)
    {
        emit("loop", " step=stopped mid_dz=" + (string)rise()
            + " at_start=" + (string)(llVecDist(llGetPos(), home) < 0.01)
            + " moved_after_stop=" + (string)(llVecDist(llGetPos(), paused_at) > 0.01));
        stop_and_restore();
        try("set-pos-during", [<0.0, 0.0, 1.0>, ZERO_ROTATION, 3.0], []);
        llSetPos(home + <1.0, 0.0, 0.0>);
    }
    else if (s == 19)
    {
        try("physics-during", [<0.0, 0.0, 1.0>, ZERO_ROTATION, 3.0], []);
        llSetStatus(STATUS_PHYSICS, TRUE);
        emit("physics-during", " physics=" + (string)llGetStatus(STATUS_PHYSICS));
    }
    else if (s == 20)
    {
        llSetBuoyancy(1.0);
        llSetStatus(STATUS_PHYSICS, TRUE);
        try("on-physical", UP, []);
    }
    else if (s == 21)
    {
        try("play-with-none", [], [KFM_COMMAND, KFM_CMD_PLAY]);
    }
    else if (s == 22)
    {
        try("empty-with-none", [], []);
    }
}

default
{
    state_entry()
    {
        begin();
        home = llGetPos();
        home_rot = llGetRot();
        step = 0;
        act(step);
        llSetTimerEvent(1.5);
    }

    moving_start()
    {
        ++starts;
    }

    moving_end()
    {
        ++ends;
    }

    timer()
    {
        if (step < 14)
        {
            outcome(llList2String(LABELS, step));
            stop_and_restore();
        }
        else if (step == 18)
        {
            midway("set-pos-during");
            stop_and_restore();
        }
        else if (step == 19)
        {
            midway("physics-during");
            stop_and_restore();
        }
        else if (step == 20)
        {
            outcome("on-physical");
            stop_and_restore();
        }
        else if (step == 21)
        {
            outcome("play-with-none");
            stop_and_restore();
        }
        else if (step == 22)
        {
            outcome("empty-with-none");
            stop_and_restore();
            llSetPrimitiveParams([PRIM_PHYSICS_SHAPE_TYPE, PRIM_PHYSICS_SHAPE_PRIM]);
            llSetTimerEvent(0.0);
            emit("end", "");
            return;
        }
        ++step;
        act(step);
    }
}
