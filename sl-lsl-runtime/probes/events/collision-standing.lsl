// probe: events/collision-standing
// drive: none
// vary: collisions, owner_detected, other_detected, per_second
//
// Collision events from an avatar standing still on a prim: whether a grid
// raises `collision` every frame for as long as an avatar stands in contact,
// as for a body that keeps moving, or lets the contact go quiet, as Second
// Life lets a resting prim's (see events/collision). The probe makes this
// prim a platform a metre and a half square and 0.2 m thick and slides it
// under the owner's feet, its top 0.15 m above where they were, so the
// avatar stands on it. A collision filter lets the owner's collisions alone
// through, since the other avatar may stand on the platform or beside it
// depending on where its login put it (on aditi it did once and not the
// next run). Phase 1 `standing` lasts eight seconds; phase 2
// `gone` moves the platform back where it was and lasts three. Each phase
// prints the `collision_start` and `collision_end` events this prim ran, in
// order, each with whom its detected were (owner, other avatar, object), the
// `collision` events between as `collisions`, the avatars those detected as
// `owner_detected` and `other_detected`, and objects as `object_detected`,
// and how many `collision` events came in each second of the phase
// (`per_second`), which tells a contact that keeps raising them at some rate
// from one that raises a burst while the avatar settles and then goes quiet;
// phase 1 also prints whether the owner stood higher at its end than before
// (`lifted`), which says the avatar did stand on the platform.
string ID = "events/collision-standing";
list NAMES = ["standing", "gone"];

vector home;
vector before;
integer phase;
list got;
integer collisions;
integer owner_detected;
integer other_detected;
integer object_detected;
list seconds;

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

string who(integer i)
{
    if (llDetectedKey(i) == llGetOwner())
    {
        return "owner";
    }
    if (llDetectedType(i) & AGENT)
    {
        return "other";
    }
    return "object";
}

string whom(integer count)
{
    list names;
    integer i;
    for (i = 0; i < count; ++i)
    {
        names += [who(i)];
    }
    return llDumpList2String(names, "+");
}

vector owner_at()
{
    return llList2Vector(llGetObjectDetails(llGetOwner(), [OBJECT_POS]), 0);
}

start(integer which)
{
    phase = which;
    got = [];
    collisions = 0;
    owner_detected = 0;
    other_detected = 0;
    object_detected = 0;
    seconds = [];
    llResetTime();
    if (which == 1)
    {
        before = owner_at();
        vector size = llGetAgentSize(llGetOwner());
        float top = before.z - size.z / 2.0 + 0.15;
        llSetRegionPos(<before.x, before.y, top - 0.1>);
        llSetTimerEvent(8.0);
    }
    else
    {
        llSetRegionPos(home);
        llSetTimerEvent(3.0);
    }
}

default
{
    state_entry()
    {
        begin();
        home = llGetPos();
        if (llGetAgentSize(llGetOwner()) == ZERO_VECTOR)
        {
            emit("no-owner", "");
            emit("end", "");
            return;
        }
        llCollisionFilter("", llGetOwner(), TRUE);
        llSetPrimitiveParams([PRIM_SIZE, <1.5, 1.5, 0.2>, PRIM_ROTATION, ZERO_ROTATION]);
        start(1);
    }

    timer()
    {
        llSetTimerEvent(0.0);
        string lifted = "";
        if (phase == 1)
        {
            vector now = owner_at();
            lifted = " lifted=" + (string)(now.z > before.z + 0.05);
        }
        emit("phase", " phase=" + (string)phase
            + " name=" + llList2String(NAMES, phase - 1)
            + " events=" + q(llDumpList2String(got, ","))
            + " collisions=" + (string)collisions
            + " owner_detected=" + (string)owner_detected
            + " other_detected=" + (string)other_detected
            + " object_detected=" + (string)object_detected
            + " per_second=" + q(llDumpList2String(seconds, ","))
            + lifted);
        if (phase < llGetListLength(NAMES))
        {
            start(phase + 1);
        }
        else
        {
            emit("end", "");
        }
    }

    collision_start(integer count)
    {
        got += ["collision_start:" + whom(count)];
    }

    collision(integer count)
    {
        ++collisions;
        integer second = (integer)llGetTime();
        while (llGetListLength(seconds) <= second)
        {
            seconds += [0];
        }
        seconds = llListReplaceList(seconds,
            [llList2Integer(seconds, second) + 1], second, second);
        integer i;
        for (i = 0; i < count; ++i)
        {
            string one = who(i);
            if (one == "owner")
            {
                ++owner_detected;
            }
            else if (one == "other")
            {
                ++other_detected;
            }
            else
            {
                ++object_detected;
            }
        }
    }

    collision_end(integer count)
    {
        got += ["collision_end:" + whom(count)];
    }
}
