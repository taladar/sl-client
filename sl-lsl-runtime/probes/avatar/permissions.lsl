// probe: avatar/permissions
// drive: answer, touch
//
// How a permission question is put and answered, and what the script holds
// after each answer. Before anything is granted it calls what needs a
// permission (llTakeControls, llStartAnimation, llSetCameraParams,
// llGetCameraPos) and prints what came back; the runner notes any
// DEBUG_CHANNEL complaint. Then it asks the owner for three avatar-control
// permissions (TAKE_CONTROLS, TRIGGER_ANIMATION, TRACK_CAMERA) and the runner
// grants all three; asks again and the runner grants one of them; asks again
// and the runner denies; asks for TRIGGER_ANIMATION alone and the runner
// grants all three, more than was asked; asks with a mask of 0; asks for
// CONTROL_CAMERA alone, which Second Life allows only an attachment or a seat
// (avatar/camera asks a seated avatar). Then the other avatar touches, so the
// script learns its key, and the script asks it for the three, which the
// runner grants as that avatar. The runner notes every question: the mask
// asked, the object's name, and how the owner's name was put, or that none
// came. A timer moves on after fifteen seconds without an answer (five for
// the mask of 0).
string ID = "avatar/permissions";

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
    if (id == other)
    {
        return "other";
    }
    return "someone";
}

held(string why, integer answer)
{
    emit("held", " step=" + (string)step + " why=" + why
        + " answer=" + (string)answer
        + " perms=" + (string)llGetPermissions()
        + " granter=" + who(llGetPermissionsKey()));
}

string by(key agent)
{
    if (agent == llGetOwner())
    {
        return "";
    }
    return " by=other";
}

ask(key agent, integer grant, integer mask)
{
    emit("waiting", " drive=answer" + by(agent) + " grant=" + (string)grant);
    emit("asked", " step=" + (string)step + " of=" + who(agent) + " mask=" + (string)mask);
    llRequestPermissions(agent, mask);
}

next()
{
    step = step + 1;
    llSetTimerEvent(15.0);
    if (step == 1)
    {
        ask(llGetOwner(), MASK, MASK);
    }
    else if (step == 2)
    {
        ask(llGetOwner(), PERMISSION_TRIGGER_ANIMATION, MASK);
    }
    else if (step == 3)
    {
        ask(llGetOwner(), 0, MASK);
    }
    else if (step == 4)
    {
        ask(llGetOwner(), MASK, PERMISSION_TRIGGER_ANIMATION);
    }
    else if (step == 5)
    {
        llSetTimerEvent(5.0);
        emit("asked", " step=" + (string)step + " of=owner mask=0");
        llRequestPermissions(llGetOwner(), 0);
    }
    else if (step == 6)
    {
        ask(llGetOwner(), PERMISSION_CONTROL_CAMERA, PERMISSION_CONTROL_CAMERA);
    }
    else if (step == 7)
    {
        emit("waiting", " drive=touch by=other");
    }
    else if (step == 8)
    {
        ask(other, MASK, MASK);
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
            | PERMISSION_TRACK_CAMERA;
        begin();
        held("fresh", -1);
        llTakeControls(CONTROL_FWD, TRUE, FALSE);
        emit("unpermitted", " call=llTakeControls");
        llStartAnimation("dance1");
        emit("unpermitted", " call=llStartAnimation");
        llSetCameraParams([CAMERA_ACTIVE, TRUE]);
        emit("unpermitted", " call=llSetCameraParams");
        emit("unpermitted", " call=llGetCameraPos zero="
            + (string)(llGetCameraPos() == ZERO_VECTOR));
        step = 0;
        next();
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
        held("no-answer", -1);
        next();
    }

    touch_start(integer count)
    {
        if (step == 7)
        {
            other = llDetectedKey(0);
            emit("touch", " step=" + (string)step + " by=" + who(other));
            next();
        }
    }
}
