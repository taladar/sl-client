// probe: physics/targets
// drive: none
// vary: at, not_at, seconds, dz
//
// Position and rotation targets, llMoveToTarget and the look-at functions, on
// each grid. Each phase runs for a second and then reports. On a prim that is
// not physical: a target at the prim and one ten metres above it (the
// handles, which raise at_target and how often, how often not_at_target
// runs, and whether at_target's positions are the target's and the prim's);
// ten targets at the prim at once (which of them still raise at_target: a
// script's limit); a range of 0 and of -1 at the prim; the same for rotation
// targets; and whether llMoveToTarget, llLookAt and llRotLookAt move or turn
// a prim that is not physical. Then, physical and weightless: how long
// llMoveToTarget with a tau of 0.3 takes to reach a target a metre up, by
// at_target with a range of 0.1; whether it moves towards one 70 metres up
// (past the documented 65 m) or with a tau of 0; and how long llRotLookAt
// takes to turn the prim a quarter round by at_rot_target. Counts and times
// are measurements. The prim is made non-physical and put back after.
string ID = "physics/targets";

vector home;
rotation home_rot;
integer step;
float started;

list counts;      // handle, at_target count, pairs
integer not_at;
list rot_counts;  // handle, at_rot_target count, pairs
integer not_at_rot;
list handles;
string first_at;
string first_rot;
integer goal;
integer done;

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

list bump(list pairs, integer handle)
{
    integer at = llListFindList(pairs, [handle]);
    if (at < 0 || at % 2)
    {
        return pairs + [handle, 1];
    }
    return llListReplaceList(pairs, [llList2Integer(pairs, at + 1) + 1], at + 1, at + 1);
}

integer count_of(list pairs, integer handle)
{
    integer at = llListFindList(pairs, [handle]);
    if (at < 0 || at % 2)
    {
        return 0;
    }
    return llList2Integer(pairs, at + 1);
}

// The positions in `handles` whose handle raised an event, as "0,1,…".
string fired(list pairs)
{
    string out = "";
    integer n = llGetListLength(handles);
    integer i;
    for (i = 0; i < n; ++i)
    {
        if (count_of(pairs, llList2Integer(handles, i)) > 0)
        {
            if (out != "")
            {
                out += ",";
            }
            out += (string)i;
        }
    }
    return out;
}

// The differences between successive handles, as "1,1,…".
string steps_between()
{
    string out = "";
    integer n = llGetListLength(handles);
    integer i;
    for (i = 1; i < n; ++i)
    {
        if (out != "")
        {
            out += ",";
        }
        out += (string)(llList2Integer(handles, i) - llList2Integer(handles, i - 1));
    }
    return out;
}

reset_counts()
{
    counts = [];
    not_at = 0;
    rot_counts = [];
    not_at_rot = 0;
    first_at = "";
    first_rot = "";
}

remove_all()
{
    integer n = llGetListLength(handles);
    integer i;
    for (i = 0; i < n; ++i)
    {
        llTargetRemove(llList2Integer(handles, i));
        llRotTargetRemove(llList2Integer(handles, i));
    }
    handles = [];
}

restore()
{
    llStopMoveToTarget();
    llStopLookAt();
    llSetStatus(STATUS_PHYSICS, FALSE);
    llSetBuoyancy(0.0);
    llSetRegionPos(home);
    llSetRot(home_rot);
}

string moved()
{
    return " moved=" + (string)(llVecDist(llGetPos(), home) > 0.01)
        + " turned=" + (string)(llAngleBetween(llGetRot(), home_rot) > 0.01);
}

next(integer s, float seconds)
{
    step = s;
    reset_counts();
    started = llGetTime();
    llSetTimerEvent(seconds);
}

default
{
    state_entry()
    {
        begin();
        home = llGetPos();
        home_rot = llGetRot();
        handles = [llTarget(home, 1.0), llTarget(home + <0.0, 0.0, 10.0>, 1.0)];
        next(1, 1.0);
    }

    at_target(integer handle, vector target, vector ours)
    {
        counts = bump(counts, handle);
        if (first_at == "")
        {
            first_at = " first_handle_is=" + (string)llListFindList(handles, [handle])
                + " target_given=" + (string)(llVecDist(target, home) < 0.01
                    || llVecDist(target, home + <0.0, 0.0, 1.0>) < 0.01)
                + " ours_is_pos=" + (string)(llVecDist(ours, llGetPos()) < 0.01);
        }
        if (step == 9 && handle == goal && !done)
        {
            done = TRUE;
            emit("move-to-target", " tau=0.3 reached=1 seconds="
                + (string)(llGetTime() - started)
                + " dz=" + (string)rise());
        }
    }

    not_at_target()
    {
        ++not_at;
    }

    at_rot_target(integer handle, rotation target, rotation ours)
    {
        rot_counts = bump(rot_counts, handle);
        if (first_rot == "")
        {
            first_rot = " first_handle_is=" + (string)llListFindList(handles, [handle])
                + " ours_is_rot=" + (string)(llAngleBetween(ours, llGetRot()) < 0.01);
        }
        if (step == 12 && handle == goal && !done)
        {
            done = TRUE;
            emit("rot-look-at", " reached=1 seconds=" + (string)(llGetTime() - started));
        }
    }

    not_at_rot_target()
    {
        ++not_at_rot;
    }

    timer()
    {
        llSetTimerEvent(0.0);
        if (step == 1)
        {
            integer near = llList2Integer(handles, 0);
            integer far = llList2Integer(handles, 1);
            emit("targets", " first_positive=" + (string)(near > 0)
                + " steps=" + q(steps_between())
                + " fired=" + q(fired(counts))
                + " at=" + (string)count_of(counts, near)
                + " far_fired=" + (string)(count_of(counts, far) > 0)
                + " not_at=" + (string)not_at + first_at);
            remove_all();
            integer i;
            for (i = 0; i < 10; ++i)
            {
                handles += [llTarget(home, 1.0)];
            }
            next(2, 1.0);
        }
        else if (step == 2)
        {
            emit("ten-targets", " steps=" + q(steps_between())
                + " fired=" + q(fired(counts)) + " not_at=" + (string)not_at);
            remove_all();
            llTargetRemove(999999);
            llTargetRemove(-1);
            handles = [llTarget(home, 0.0), llTarget(home, -1.0)];
            next(3, 1.0);
        }
        else if (step == 3)
        {
            emit("ranges", " ranges=\"0,-1\" fired=" + q(fired(counts))
                + " not_at=" + (string)not_at);
            remove_all();
            integer last_pos = llTarget(home, 1.0);
            llTargetRemove(last_pos);
            handles = [llRotTarget(home_rot, 0.1),
                llRotTarget(llEuler2Rot(<0.0, 0.0, PI_BY_TWO>) * home_rot, 0.1)];
            emit("rot-handles", " after_position_target="
                + (string)(llList2Integer(handles, 0) - last_pos));
            next(4, 1.0);
        }
        else if (step == 4)
        {
            emit("rot-targets", " steps=" + q(steps_between())
                + " fired=" + q(fired(rot_counts))
                + " at=" + (string)count_of(rot_counts, llList2Integer(handles, 0))
                + " not_at=" + (string)not_at_rot
                + " position_events=" + (string)(llGetListLength(counts) / 2 + not_at)
                + first_rot);
            remove_all();
            integer i;
            for (i = 0; i < 10; ++i)
            {
                handles += [llRotTarget(home_rot, 0.1)];
            }
            next(5, 1.0);
        }
        else if (step == 5)
        {
            emit("ten-rot-targets", " fired=" + q(fired(rot_counts))
                + " not_at=" + (string)not_at_rot);
            remove_all();
            llMoveToTarget(home + <0.0, 0.0, 1.0>, 0.5);
            next(6, 1.0);
        }
        else if (step == 6)
        {
            emit("non-physical", " call=llMoveToTarget" + moved());
            restore();
            llLookAt(home + <5.0, 0.0, 0.0>, 1.0, 0.5);
            next(7, 1.0);
        }
        else if (step == 7)
        {
            emit("non-physical", " call=llLookAt" + moved());
            restore();
            llRotLookAt(llEuler2Rot(<0.0, 0.0, PI_BY_TWO>) * home_rot, 1.0, 0.5);
            next(8, 1.0);
        }
        else if (step == 8)
        {
            emit("non-physical", " call=llRotLookAt" + moved());
            restore();
            llSetBuoyancy(1.0);
            llSetStatus(STATUS_PHYSICS, TRUE);
            goal = llTarget(home + <0.0, 0.0, 1.0>, 0.1);
            handles = [goal];
            done = FALSE;
            llMoveToTarget(home + <0.0, 0.0, 1.0>, 0.3);
            next(9, 5.0);
        }
        else if (step == 9)
        {
            if (!done)
            {
                emit("move-to-target", " tau=0.3 reached=0 dz="
                    + (string)rise());
            }
            remove_all();
            restore();
            llSetBuoyancy(1.0);
            llSetStatus(STATUS_PHYSICS, TRUE);
            llMoveToTarget(home + <0.0, 0.0, 70.0>, 0.3);
            next(10, 0.5);
        }
        else if (step == 10)
        {
            emit("move-to-target", " target=70m-up dz=" + (string)rise()
                + " rose=" + (string)(rise() > 0.5));
            restore();
            llSetBuoyancy(1.0);
            llSetStatus(STATUS_PHYSICS, TRUE);
            llMoveToTarget(home + <0.0, 0.0, 1.0>, 0.0);
            next(11, 1.0);
        }
        else if (step == 11)
        {
            emit("move-to-target", " tau=0 dz=" + (string)rise()
                + " rose=" + (string)(rise() > 0.25));
            restore();
            llSetBuoyancy(1.0);
            llSetStatus(STATUS_PHYSICS, TRUE);
            rotation turned = llEuler2Rot(<0.0, 0.0, PI_BY_TWO>) * home_rot;
            goal = llRotTarget(turned, 0.1);
            handles = [goal];
            done = FALSE;
            llRotLookAt(turned, 1.0, 0.5);
            next(12, 5.0);
        }
        else
        {
            if (!done)
            {
                emit("rot-look-at", " reached=0" + moved());
            }
            remove_all();
            restore();
            emit("after", " physics=" + (string)llGetStatus(STATUS_PHYSICS) + moved());
            emit("end", "");
        }
    }
}
