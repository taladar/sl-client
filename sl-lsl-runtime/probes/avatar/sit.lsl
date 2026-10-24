// probe: avatar/sit
// drive: touch, sit, stand
//
// Sitting on the probe's prim. It sets a sit target (half a metre up, turned
// a quarter to the left), the sit-camera eye and focus offsets and the sit and
// touch texts; the runner notes the sit response each sit gets (seat
// position and rotation, camera offsets, mouselook, autopilot). The other
// avatar touches first, so the script knows its key. Then, in turn: the
// owner sits, and the script says what changed, who is on the sit target
// (and whether llGetAgentSize knows the sitter), how many prims the object
// counts and whose the last link is, then asks the seated owner for the four
// avatar-control permissions with no `waiting` for an answer, which a seated
// avatar's grid grants without a question (the runner notes a question if
// one comes); llUnSit puts the owner off; the other avatar sits and is asked
// the same, and stands; the sit target is cleared and the owner sits on the
// bare prim and stands. A timer moves on after twenty seconds without a
// change, a touch or an answer.
string ID = "avatar/sit";

integer MASK;
integer step;
key other;

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

string who(key id)
{
    if (id == NULL_KEY)
    {
        return "none";
    }
    if (id == llGetOwner())
    {
        return "owner";
    }
    if (id == llGetKey())
    {
        return "prim";
    }
    if (id == other)
    {
        return "other";
    }
    return "someone";
}

seated(string why)
{
    integer prims = llGetNumberOfPrims();
    key sitter = llAvatarOnSitTarget();
    string size = "";
    if (sitter != NULL_KEY)
    {
        size = " sitter_size_zero=" + (string)(llGetAgentSize(sitter) == ZERO_VECTOR);
    }
    emit("seat", " step=" + (string)step + " why=" + why
        + " on_target=" + who(sitter) + size
        + " prims=" + (string)prims
        + " last_link=" + who(llGetLinkKey(prims))
        + " objects=" + (string)llGetObjectPrimCount(llGetKey()));
}

held(string why, integer answer)
{
    emit("held", " step=" + (string)step + " why=" + why
        + " answer=" + (string)answer
        + " perms=" + (string)llGetPermissions()
        + " granter=" + who(llGetPermissionsKey()));
}

next()
{
    step = step + 1;
    llSetTimerEvent(20.0);
    if (step == 1)
    {
        emit("waiting", " drive=touch by=other");
    }
    else if (step == 2)
    {
        emit("waiting", " drive=sit");
    }
    else if (step == 3)
    {
        emit("asked", " step=" + (string)step + " of=owner mask=" + (string)MASK);
        llRequestPermissions(llGetOwner(), MASK);
    }
    else if (step == 4)
    {
        emit("unsit", " step=" + (string)step + " of=owner");
        llUnSit(llGetOwner());
    }
    else if (step == 5)
    {
        emit("waiting", " drive=sit by=other");
    }
    else if (step == 6)
    {
        emit("asked", " step=" + (string)step + " of=other mask=" + (string)MASK);
        llRequestPermissions(other, MASK);
    }
    else if (step == 7)
    {
        emit("waiting", " drive=stand by=other");
    }
    else if (step == 8)
    {
        llSitTarget(ZERO_VECTOR, ZERO_ROTATION);
        emit("target-cleared", " on_target=" + who(llAvatarOnSitTarget()));
        emit("waiting", " drive=sit");
    }
    else if (step == 9)
    {
        emit("waiting", " drive=stand");
    }
    else
    {
        llSetTimerEvent(0.0);
        emit("end", "");
    }
}

default
{
    state_entry()
    {
        MASK = PERMISSION_TAKE_CONTROLS | PERMISSION_TRIGGER_ANIMATION
            | PERMISSION_TRACK_CAMERA | PERMISSION_CONTROL_CAMERA;
        begin();
        llSitTarget(<0.0, 0.0, 0.5>, llEuler2Rot(<0.0, 0.0, PI_BY_TWO>));
        llSetCameraEyeOffset(<-2.0, 0.0, 1.0>);
        llSetCameraAtOffset(<0.0, 0.0, 0.5>);
        llSetSitText("Probe sit");
        llSetTouchText("Probe touch");
        seated("before");
        step = 0;
        next();
    }

    touch_start(integer count)
    {
        if (step == 1)
        {
            llSetTimerEvent(0.0);
            other = llDetectedKey(0);
            emit("touch", " step=" + (string)step + " by=" + who(other));
            next();
        }
    }

    changed(integer change)
    {
        if (change & CHANGED_LINK)
        {
            llSetTimerEvent(0.0);
            seated("changed");
            if (step == 4)
            {
                held("after-unsit", -1);
            }
            else if (step == 7 || step == 9)
            {
                held("after-stand", -1);
            }
            next();
        }
    }

    run_time_permissions(integer answer)
    {
        llSetTimerEvent(0.0);
        held("answer", answer);
        next();
    }

    timer()
    {
        llSetTimerEvent(0.0);
        emit("timeout", " step=" + (string)step);
        if (step == 4)
        {
            held("after-unsit", -1);
        }
        next();
    }
}
