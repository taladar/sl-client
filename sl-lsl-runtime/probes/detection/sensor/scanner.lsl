// probe: detection/sensor
// script: Scanner
// order: 6
// drive: none
// bare: C@22, D@21, E@20, F@19, G@18, H@17, I@16, J@15, K@14, L@13, M@12, N@11, O@10, P@9, Q@8, R@7, S@6, T@5
// vary: agent_dist, t, events, per_second
// unordered: agent
//
// What llSensor and llSensorRepeat find, and in what order. Prim A holds
// this script and faces east (the runner rezzes it unrotated). East of it
// stand, every metre from 2 m to 22 m, a row of prims: scripted "Target B"
// at 2 m, "Target V" at 3 m and "Target U" at 4 m (see Named, TargetV,
// TargetU), then eighteen bare prims with no script, rezzed from the far
// end inwards (C at 22 m first, T at 5 m last), so that an answer in rez
// order and one in distance order differ. "Target W" stands 3.5 m
// straight above prim A (outside a narrow arc about the forward axis; the
// near face of its 0.5 m cube is 3.25 m away), and "Target X" 100 m above
// it (see Far), beyond the documented 96 m range. Both avatars stand near.
// A sweep the grid answers with neither sensor nor no_sensor within five
// seconds is printed as event=none.
//
// One sweep at a time, half a second apart, the Scanner sweeps with a
// mask, a name or key filter, a range and an arc, and prints what each
// sweep found in the order the grid listed it: the distance from prim A's
// centre to the detected one's, its type, whether the owner owns it,
// which of the row it is by name ("ours" is the default name the bare
// prims share with prim A), and whether it is prim A itself. An avatar is
// printed as whether it is the owner, never by name. Then the repeating
// sensor: how often it fires at 0.5 s, whether no_sensor repeats, whether
// anything comes after llSensorRemove, how fast it fires asked for 0.01 s
// and 0.0 s, and what an llSensor called while a repeat runs does to it.
string ID = "detection/sensor";

// The sweeps, stride 6: label, name filter, key filter, type mask, range,
// arc. A name of "@ours" is prim A's own (default) name, "@owner" the
// owner's legacy name, "@username" its username; a key of "@B" is
// Target B's, learned by the "name" sweep.
list sweeps;
integer sweep;
integer phase;
key target_b;
string ours;
float t0;
integer events;
string label;
integer unanswered;

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

string num(float f)
{
    return (string)(llRound(f * 1000.0) / 1000.0);
}

string vec(vector v)
{
    return "<" + num(v.x) + "," + num(v.y) + "," + num(v.z) + ">";
}

string rot(rotation r)
{
    return "<" + num(r.x) + "," + num(r.y) + "," + num(r.z) + "," + num(r.s) + ">";
}

string kind(string name)
{
    if (name == ours)
    {
        return "ours";
    }
    if (llGetSubString(name, 0, 6) == "Target " && llStringLength(name) == 8)
    {
        return "target-" + llToLower(llGetSubString(name, 7, 7));
    }
    return "other";
}

string elapsed()
{
    return num(llGetTime() - t0);
}

next_sweep()
{
    if (sweep * 6 >= llGetListLength(sweeps))
    {
        phase = 1;
        next_phase();
        return;
    }
    integer at = sweep * 6;
    label = llList2String(sweeps, at);
    string name = llList2String(sweeps, at + 1);
    string id = llList2String(sweeps, at + 2);
    integer type = llList2Integer(sweeps, at + 3);
    float range = llList2Float(sweeps, at + 4);
    float arc = llList2Float(sweeps, at + 5);
    emit("sweep", " step=" + label + " name=" + q(name) + " key=" + q(id)
        + " type=" + (string)type + " range=" + num(range) + " arc=" + num(arc));
    string filter = name;
    if (name == "@ours")
    {
        filter = ours;
    }
    else if (name == "@owner")
    {
        filter = llKey2Name(llGetOwner());
    }
    else if (name == "@username")
    {
        filter = llGetUsername(llGetOwner());
    }
    key with = NULL_KEY;
    if (id == "@B")
    {
        with = target_b;
    }
    unanswered = TRUE;
    llSensor(filter, with, type, range, arc);
    llSetTimerEvent(5.0);
}

swept(string how, integer count)
{
    unanswered = FALSE;
    emit("swept", " step=" + label + " event=" + how + " num=" + (string)count);
    integer i;
    for (i = 0; i < count; ++i)
    {
        key id = llDetectedKey(i);
        if (llGetAgentSize(id) != ZERO_VECTOR)
        {
            emit("agent", " step=" + label + " key_is_owner=" + (string)(id == llGetOwner())
                + " type=" + (string)llDetectedType(i)
                + " owner_is_self=" + (string)(llDetectedOwner(i) == id)
                + " name_is_key2name=" + (string)(llDetectedName(i) == llKey2Name(id))
                + " agent_dist=" + num(llVecDist(llDetectedPos(i), llGetPos())));
        }
        else
        {
            emit("found", " step=" + label + " i=" + (string)i
                + " dist=" + num(llVecDist(llDetectedPos(i), llGetPos()))
                + " type=" + (string)llDetectedType(i)
                + " owned=" + (string)(llDetectedOwner(i) == llGetOwner())
                + " name=" + kind(llDetectedName(i))
                + " self=" + (string)(id == llGetKey()));
        }
    }
    if (label == "name" && count > 0)
    {
        target_b = llDetectedKey(0);
        emit("detail", " step=" + label
            + " offset=" + vec(llDetectedPos(0) - llGetPos())
            + " rot=" + rot(llDetectedRot(0))
            + " vel=" + vec(llDetectedVel(0))
            + " owned=" + (string)(llDetectedOwner(0) == llGetOwner())
            + " group=" + (string)llDetectedGroup(0)
            + " link=" + (string)llDetectedLinkNumber(0)
            + " grab=" + vec(llDetectedGrab(0))
            + " touch_face=" + (string)llDetectedTouchFace(0)
            + " touch_uv=" + vec(llDetectedTouchUV(0))
            + " touch_st=" + vec(llDetectedTouchST(0))
            + " touch_pos=" + vec(llDetectedTouchPos(0))
            + " touch_normal=" + vec(llDetectedTouchNormal(0))
            + " touch_binormal=" + vec(llDetectedTouchBinormal(0))
            + " past_end_key_null=" + (string)(llDetectedKey(count) == NULL_KEY));
    }
    sweep = sweep + 1;
    llSetTimerEvent(0.5);
}

// The repeating sensor's phases, each ended by the timer.
next_phase()
{
    events = 0;
    t0 = llGetTime();
    if (phase == 1)
    {
        emit("repeat", " phase=1 call=" + q("llSensorRepeat Target B every 0.5 s"));
        llSensorRepeat("Target B", NULL_KEY, SCRIPTED, 10.0, PI, 0.5);
        llSetTimerEvent(2.6);
    }
    else if (phase == 3)
    {
        emit("repeat", " phase=3 call=" + q("llSensorRepeat nothing every 0.5 s"));
        llSensorRepeat("Nobody here", NULL_KEY, AGENT | ACTIVE | PASSIVE | SCRIPTED, 10.0, PI, 0.5);
        llSetTimerEvent(2.6);
    }
    else if (phase == 5)
    {
        emit("repeat", " phase=5 call=" + q("llSensorRepeat Target B every 0.01 s"));
        llSensorRepeat("Target B", NULL_KEY, SCRIPTED, 10.0, PI, 0.01);
        llSetTimerEvent(2.0);
    }
    else if (phase == 7)
    {
        emit("repeat", " phase=7 call=" + q("llSensorRepeat Target B every 1 s, then llSensor Target X"));
        llSensorRepeat("Target B", NULL_KEY, SCRIPTED, 10.0, PI, 1.0);
        llSensor("Target X", NULL_KEY, SCRIPTED, 200.0, PI);
        llSetTimerEvent(3.6);
    }
    else if (phase == 9)
    {
        emit("repeat", " phase=9 call=" + q("llSensorRepeat Target B every 0.0 s"));
        llSensorRepeat("Target B", NULL_KEY, SCRIPTED, 10.0, PI, 0.0);
        llSetTimerEvent(1.6);
    }
    else
    {
        // An even phase: the repeat was removed; anything that still comes
        // is counted.
        emit("removed", " after_phase=" + (string)(phase - 1));
        llSensorRemove();
        llSetTimerEvent(1.6);
    }
}

repeated(integer count)
{
    events = events + 1;
    string found = "none";
    if (count > 0)
    {
        found = kind(llDetectedName(0));
    }
    // The fast phase prints only its count.
    if (phase != 5)
    {
        emit("fired", " phase=" + (string)phase + " event=" + (string)events
            + " num=" + (string)count + " found=" + found + " t=" + elapsed());
    }
}

default
{
    state_entry()
    {
        begin();
        ours = llGetObjectName();
        integer objects = ACTIVE | PASSIVE | SCRIPTED;
        sweeps = [
            "all", "", "", objects, 96.0, PI,
            "ours", "@ours", "", objects, 96.0, PI,
            "agents", "", "", AGENT, 96.0, PI,
            "owner-legacy", "@owner", "", AGENT_BY_LEGACY_NAME, 96.0, PI,
            "owner-username", "@username", "", AGENT_BY_USERNAME, 96.0, PI,
            "scripted", "", "", SCRIPTED, 96.0, PI,
            "passive", "@ours", "", PASSIVE, 96.0, PI,
            "passive-scripted", "Target B", "", PASSIVE, 96.0, PI,
            "active", "", "", ACTIVE, 96.0, PI,
            "name", "Target B", "", objects, 96.0, PI,
            "key", "", "@B", objects, 96.0, PI,
            "key-wrong-name", "Nope", "@B", objects, 96.0, PI,
            "nothing", "Nobody here", "", AGENT | objects, 96.0, PI,
            "range-3", "", "", objects, 3.0, PI,
            "range-3.2", "", "", objects, 3.2, PI,
            "range-3.4", "", "", objects, 3.4, PI,
            "range-4.1", "", "", objects, 4.1, PI,
            "arc-quarter", "", "", objects, 4.1, PI / 4.0,
            "arc-zero", "", "", objects, 4.1, 0.0,
            "arc-negative", "", "", objects, 4.1, -1.0,
            "arc-wide", "", "", objects, 4.1, 4.0,
            "range-zero", "", "", objects, 0.0, PI,
            "range-negative", "", "", objects, -1.0, PI,
            "far-96", "Target X", "", objects, 96.0, PI,
            "far-200", "Target X", "", objects, 200.0, PI
        ];
        // Give the targets' scripts time to name their prims.
        llSetTimerEvent(2.0);
    }

    timer()
    {
        llSetTimerEvent(0.0);
        if (phase == 0)
        {
            if (unanswered)
            {
                swept("none", 0);
            }
            else
            {
                next_sweep();
            }
            return;
        }
        if (phase == 5)
        {
            emit("rate", " phase=5 events=" + (string)events
                + " per_second=" + num(events / (llGetTime() - t0)));
        }
        else
        {
            emit("phase", " phase=" + (string)phase + " events=" + (string)events);
        }
        if (phase == 10)
        {
            llSensorRemove();
            emit("end", "");
            return;
        }
        phase = phase + 1;
        next_phase();
    }

    sensor(integer count)
    {
        if (phase == 0)
        {
            swept("sensor", count);
        }
        else
        {
            repeated(count);
        }
    }

    no_sensor()
    {
        if (phase == 0)
        {
            swept("no_sensor", 0);
        }
        else
        {
            repeated(0);
        }
    }
}
