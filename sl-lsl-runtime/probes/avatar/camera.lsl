// probe: avatar/camera
// drive: answer, sit, stand
// vary: rot_identity
//
// llSetCameraParams, llClearCameraParams and camera tracking. The script
// first asks the owner, standing, for CONTROL_CAMERA and TRACK_CAMERA, which
// the runner grants if a question comes (Second Life allows camera control
// only to an attachment or a seat). Then the owner sits on the prim's sit
// target and is asked again, which a seat is granted without a question. The
// script sets a follow camera of every kind of rule (a flag, a float, a
// vector), then one with values far out of their documented ranges, then
// clears it, a second apart; the runner notes what the grid sent the viewer
// for each (the SetFollowCamProperties parameters, ClearFollowCamProperties).
// Then it reads the camera back — only whether llGetCameraPos is the zero
// vector and llGetCameraRot the identity, since where the runner's camera is
// is no grid's behaviour — and the owner stands, and the script says what it
// holds. A timer moves on after fifteen seconds without an answer or a
// change.
string ID = "avatar/camera";

integer MASK;
integer step;

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

held(string why, integer answer)
{
    emit("held", " step=" + (string)step + " why=" + why
        + " answer=" + (string)answer
        + " perms=" + (string)llGetPermissions());
}

next()
{
    step = step + 1;
    llSetTimerEvent(15.0);
    if (step == 1)
    {
        emit("waiting", " drive=answer grant=" + (string)MASK);
        emit("asked", " step=" + (string)step + " seated=0 mask=" + (string)MASK);
        llRequestPermissions(llGetOwner(), MASK);
    }
    else if (step == 2)
    {
        emit("waiting", " drive=sit");
    }
    else if (step == 3)
    {
        emit("asked", " step=" + (string)step + " seated=1 mask=" + (string)MASK);
        llRequestPermissions(llGetOwner(), MASK);
    }
    else if (step == 4)
    {
        llSetTimerEvent(1.0);
        llSetCameraParams([
            CAMERA_ACTIVE, TRUE,
            CAMERA_DISTANCE, 5.0,
            CAMERA_PITCH, 20.0,
            CAMERA_BEHINDNESS_ANGLE, 30.0,
            CAMERA_BEHINDNESS_LAG, 0.5,
            CAMERA_FOCUS_OFFSET, <0.0, 0.0, 1.0>,
            CAMERA_POSITION_LOCKED, TRUE,
            CAMERA_FOCUS, llGetPos() + <0.0, 0.0, 2.0>
        ]);
        emit("set", " step=" + (string)step + " which=in-range");
    }
    else if (step == 5)
    {
        llSetTimerEvent(1.0);
        llSetCameraParams([
            CAMERA_DISTANCE, 100.0,
            CAMERA_PITCH, -200.0,
            CAMERA_BEHINDNESS_ANGLE, 400.0,
            CAMERA_FOCUS_LAG, 9.0,
            CAMERA_POSITION_THRESHOLD, -1.0
        ]);
        emit("set", " step=" + (string)step + " which=out-of-range");
    }
    else if (step == 6)
    {
        llSetTimerEvent(1.0);
        llClearCameraParams();
        emit("cleared", " step=" + (string)step);
    }
    else if (step == 7)
    {
        emit("track", " step=" + (string)step
            + " pos_zero=" + (string)(llGetCameraPos() == ZERO_VECTOR)
            + " rot_identity=" + (string)(llGetCameraRot() == ZERO_ROTATION));
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
        MASK = PERMISSION_CONTROL_CAMERA | PERMISSION_TRACK_CAMERA;
        begin();
        llSitTarget(<0.0, 0.0, 0.5>, ZERO_ROTATION);
        step = 0;
        next();
    }

    run_time_permissions(integer answer)
    {
        if (step == 1 || step == 3)
        {
            llSetTimerEvent(0.0);
            held("answer", answer);
            next();
        }
    }

    changed(integer change)
    {
        if ((change & CHANGED_LINK) && (step == 2 || step == 7))
        {
            llSetTimerEvent(0.0);
            emit("seat", " step=" + (string)step
                + " seated=" + (string)(llAvatarOnSitTarget() == llGetOwner()));
            if (step == 7)
            {
                held("after-stand", -1);
            }
            next();
        }
    }

    timer()
    {
        if (step < 4 || step == 7)
        {
            llSetTimerEvent(0.0);
            emit("timeout", " step=" + (string)step);
            if (step == 1 || step == 3)
            {
                held("no-answer", -1);
            }
        }
        next();
    }
}
