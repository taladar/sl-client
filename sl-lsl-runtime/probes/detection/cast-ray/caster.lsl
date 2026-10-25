// probe: detection/cast-ray
// script: Caster
// order: 4
// drive: none
// bare: C@10
// vary: land_at, agent_at, ok, failed, first_fail, elapsed, per_second, after_status
//
// What llCastRay returns, and how fast it may be called. Prim A holds this
// script; east of it stand "Wall" at 5 m (see Wall), the phantom "Ghost" at
// 7.5 m (see Ghost) and a bare prim at 10 m, each a 0.5 m cube, so a ray
// along the row meets their near faces at 4.75 m, 7.25 m and 9.75 m. Both
// avatars stand near.
//
// Each ray is printed with its options, then every element of the list it
// returned in order: its type and its value — a key as what it is (self,
// wall, ghost, ours for the bare prim, owner, agent, land for NULL_KEY,
// other), a vector as the offset from prim A's centre, an integer as it is.
// A vector that comes after the hit's position (a vector, or a string the
// grid put it as) and before the next key is the hit's normal, printed as
// it is; a position put as a string is printed as the offset it reads as. The last element is the
// status: the hit count, or an RCERR_* code. A hit on land or an avatar is
// put as land_at or agent_at, which depend on where the grid's ground and
// the avatar are. Then the throttle: 500 rays in a row, how many
// succeeded, how many failed and with which code, the first that failed,
// and the status of one more a second later. Last, the Odd script (saved before this one) casts
// with an option that has no value (see there), which may stop it.
string ID = "detection/cast-ray";

string ours;
string last_role;

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

string role(key id)
{
    if (id == NULL_KEY)
    {
        return "land";
    }
    if (id == llGetKey())
    {
        return "self";
    }
    if (id == llGetOwner())
    {
        return "owner";
    }
    if (llGetAgentSize(id) != ZERO_VECTOR)
    {
        return "agent";
    }
    string name = llKey2Name(id);
    if (name == "Wall")
    {
        return "wall";
    }
    if (name == "Ghost")
    {
        return "ghost";
    }
    if (name == ours)
    {
        return "ours";
    }
    return "other";
}

string type_name(integer type)
{
    if (type == TYPE_INTEGER)
    {
        return "integer";
    }
    if (type == TYPE_FLOAT)
    {
        return "float";
    }
    if (type == TYPE_STRING)
    {
        return "string";
    }
    if (type == TYPE_KEY)
    {
        return "key";
    }
    if (type == TYPE_VECTOR)
    {
        return "vector";
    }
    if (type == TYPE_ROTATION)
    {
        return "rotation";
    }
    return "invalid";
}

cast(string step, vector from, vector to, list options, string described)
{
    vector here = llGetPos();
    list result = llCastRay(from, to, options);
    integer length = llGetListLength(result);
    emit("ray", " step=" + step + " from=" + vec(from - here) + " to=" + vec(to - here)
        + " options=" + q(described) + " length=" + (string)length
        + " status=" + (string)llList2Integer(result, -1));
    last_role = "";
    integer positioned = FALSE;
    integer i;
    for (i = 0; i < length; ++i)
    {
        integer type = llGetListEntryType(result, i);
        string value;
        string field = "value";
        if (type == TYPE_KEY)
        {
            last_role = role(llList2Key(result, i));
            value = last_role;
            positioned = FALSE;
        }
        else if (type == TYPE_VECTOR && positioned)
        {
            field = "normal";
            value = vec(llList2Vector(result, i));
        }
        else if (type == TYPE_VECTOR || type == TYPE_STRING)
        {
            positioned = TRUE;
            value = vec((vector)llList2String(result, i) - here);
            if (last_role == "land")
            {
                field = "land_at";
            }
            else if (last_role == "owner" || last_role == "agent")
            {
                field = "agent_at";
            }
        }
        else
        {
            value = llList2String(result, i);
        }
        emit("item", " step=" + step + " i=" + (string)i + " type=" + type_name(type)
            + " " + field + "=" + value);
    }
}

throttle()
{
    vector here = llGetPos();
    vector from = here + <0.5, 0.0, 0.0>;
    vector to = here + <15.0, 0.0, 0.0>;
    integer ok;
    integer failed;
    integer first_fail = -1;
    integer unknown;
    integer perf_low;
    integer time_exceeded;
    integer i;
    float t0 = llGetTime();
    for (i = 0; i < 500; ++i)
    {
        integer status = llList2Integer(llCastRay(from, to, []), -1);
        if (status >= 0)
        {
            ok = ok + 1;
        }
        else
        {
            failed = failed + 1;
            if (first_fail < 0)
            {
                first_fail = i;
            }
            if (status == RCERR_UNKNOWN)
            {
                unknown = unknown + 1;
            }
            else if (status == RCERR_SIM_PERF_LOW)
            {
                perf_low = perf_low + 1;
            }
            else if (status == RCERR_CAST_TIME_EXCEEDED)
            {
                time_exceeded = time_exceeded + 1;
            }
        }
    }
    float elapsed = llGetTime() - t0;
    if (elapsed < 0.001)
    {
        elapsed = 0.001;
    }
    emit("throttle", " calls=500 ok=" + (string)ok + " failed=" + (string)failed
        + " first_fail=" + (string)first_fail
        + " unknown=" + (string)unknown + " sim_perf_low=" + (string)perf_low
        + " cast_time_exceeded=" + (string)time_exceeded
        + " elapsed=" + num(elapsed) + " per_second=" + (string)llRound(500 / elapsed));
    llSleep(1.0);
    emit("after", " after_status=" + (string)llList2Integer(llCastRay(from, to, []), -1));
}

default
{
    state_entry()
    {
        begin();
        ours = llGetObjectName();
        // Give the targets' scripts time to name their prims.
        llSleep(2.0);
        vector here = llGetPos();
        vector front = here + <0.5, 0.0, 0.0>;
        vector far = here + <15.0, 0.0, 0.0>;
        cast("default", front, far, [], "");
        cast("max-hits-4", front, far, [RC_MAX_HITS, 4], "RC_MAX_HITS 4");
        cast("phantom", front, far, [RC_MAX_HITS, 4, RC_DETECT_PHANTOM, TRUE],
            "RC_MAX_HITS 4, RC_DETECT_PHANTOM TRUE");
        cast("data-flags", front, far,
            [RC_MAX_HITS, 4, RC_DATA_FLAGS, RC_GET_NORMAL | RC_GET_ROOT_KEY | RC_GET_LINK_NUM],
            "RC_MAX_HITS 4, RC_DATA_FLAGS normal|root key|link num");
        cast("normal-only", front, far, [RC_DATA_FLAGS, RC_GET_NORMAL],
            "RC_DATA_FLAGS RC_GET_NORMAL");
        cast("link-only", front, far, [RC_DATA_FLAGS, RC_GET_LINK_NUM],
            "RC_DATA_FLAGS RC_GET_LINK_NUM");
        cast("reject-nonphysical", front, far,
            [RC_REJECT_TYPES, RC_REJECT_NONPHYSICAL, RC_MAX_HITS, 4],
            "RC_REJECT_TYPES RC_REJECT_NONPHYSICAL, RC_MAX_HITS 4");
        cast("reject-physical", front, far,
            [RC_REJECT_TYPES, RC_REJECT_PHYSICAL, RC_MAX_HITS, 4],
            "RC_REJECT_TYPES RC_REJECT_PHYSICAL, RC_MAX_HITS 4");
        cast("from-inside", here, far, [RC_MAX_HITS, 4], "RC_MAX_HITS 4");
        // RC_REJECT_HOST, written as its value: the linter predates it.
        cast("from-inside-reject-host", here, far,
            [RC_REJECT_TYPES, 0x20000000, RC_MAX_HITS, 4],
            "RC_REJECT_TYPES RC_REJECT_HOST, RC_MAX_HITS 4");
        cast("inside-wall", here + <5.0, 0.0, 0.0>, far, [RC_MAX_HITS, 4], "RC_MAX_HITS 4");
        cast("reverse", far, front, [RC_MAX_HITS, 4], "RC_MAX_HITS 4");
        vector above = here + <2.5, 0.0, 0.0>;
        vector ground = <above.x, above.y, 0.0>;
        cast("down", above, ground, [RC_MAX_HITS, 4], "RC_MAX_HITS 4");
        cast("down-reject-land", above, ground, [RC_REJECT_TYPES, RC_REJECT_LAND, RC_MAX_HITS, 4],
            "RC_REJECT_TYPES RC_REJECT_LAND, RC_MAX_HITS 4");
        cast("zero-length", front, front, [], "");
        cast("off-region", front, <300.0, front.y, front.z>, [RC_MAX_HITS, 4], "RC_MAX_HITS 4");
        cast("max-hits-0", front, far, [RC_MAX_HITS, 0], "RC_MAX_HITS 0");
        cast("max-hits-negative", front, far, [RC_MAX_HITS, -1], "RC_MAX_HITS -1");
        cast("max-hits-300", front, far, [RC_MAX_HITS, 300], "RC_MAX_HITS 300");
        cast("bad-option", front, far, [99, 1], "99 1");
        vector owner = llList2Vector(llGetObjectDetails(llGetOwner(), [OBJECT_POS]), 0);
        vector top = here + <0.0, 0.0, 0.6>;
        vector beyond = owner + llVecNorm(owner - top) * 2.0;
        cast("owner", top, beyond, [RC_MAX_HITS, 4], "RC_MAX_HITS 4");
        cast("owner-reject-agents", top, beyond, [RC_REJECT_TYPES, RC_REJECT_AGENTS, RC_MAX_HITS, 4],
            "RC_REJECT_TYPES RC_REJECT_AGENTS, RC_MAX_HITS 4");
        throttle();
        llMessageLinked(LINK_THIS, 1, "odd", NULL_KEY);
        llSetTimerEvent(3.0);
    }

    timer()
    {
        llSetTimerEvent(0.0);
        emit("end", "");
    }
}
