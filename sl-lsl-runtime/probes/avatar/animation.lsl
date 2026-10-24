// probe: avatar/animation
// drive: answer, touch
//
// llStartAnimation, llStopAnimation, llGetAnimation, llGetAnimationList and
// the animation overrides, on the owner and then on the other avatar. Every
// step is a second apart, so the grid has sent its AvatarAnimation (which the
// runner notes, by built-in name, and whether the probe's prim is the source)
// before the script reads the list back. Only whether the list holds the
// built-in dance1 is printed, never the list, which carries whatever the
// avatar's own state animation is. The owner is asked for TRIGGER_ANIMATION
// and OVERRIDE_ANIMATIONS on an object that is not attached, which the grid
// may refuse to grant or to ask; the override is of the standing state with a
// built-in animation's name, as the prim holds no animation. Then the other
// avatar touches, so the script learns its key, is asked for
// TRIGGER_ANIMATION, and is danced and stopped.
string ID = "avatar/animation";

key DANCE = "b68a3d7c-de9e-fc87-eec8-543d787e5b0d";

integer step;
key agent;
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

string whose(key id)
{
    if (id == llGetOwner())
    {
        return "owner";
    }
    return "other";
}

look(string why)
{
    emit("playing", " step=" + (string)step + " of=" + whose(agent) + " why=" + why
        + " state=" + q(llGetAnimation(agent))
        + " has_dance=" + (string)(llListFindList(llGetAnimationList(agent), [DANCE]) != -1));
}

next()
{
    step = step + 1;
    llSetTimerEvent(1.0);
    if (step == 1)
    {
        llStartAnimation("dance1");
        emit("did", " step=" + (string)step + " call=llStartAnimation anim=dance1");
    }
    else if (step == 2)
    {
        look("started");
        llStopAnimation("dance1");
        emit("did", " step=" + (string)step + " call=llStopAnimation anim=dance1");
    }
    else if (step == 3)
    {
        look("stopped");
        llStartAnimation("no such animation");
        emit("did", " step=" + (string)step + " call=llStartAnimation anim=missing");
    }
    else if (step == 4)
    {
        look("missing");
        llSetAnimationOverride("Standing", "dance1");
        emit("did", " step=" + (string)step + " call=llSetAnimationOverride state=Standing anim=dance1"
            + " override=" + q(llGetAnimationOverride("Standing")));
    }
    else if (step == 5)
    {
        look("overridden");
        llResetAnimationOverride("ALL");
        emit("did", " step=" + (string)step + " call=llResetAnimationOverride state=ALL"
            + " override=" + q(llGetAnimationOverride("Standing")));
    }
    else if (step == 6)
    {
        look("reset");
        llSetTimerEvent(15.0);
        emit("waiting", " drive=touch by=other");
    }
    else if (step == 7)
    {
        llSetTimerEvent(15.0);
        emit("waiting", " drive=answer by=other grant=" + (string)PERMISSION_TRIGGER_ANIMATION);
        llRequestPermissions(other, PERMISSION_TRIGGER_ANIMATION);
    }
    else if (step == 8)
    {
        agent = other;
        llStartAnimation("dance1");
        emit("did", " step=" + (string)step + " call=llStartAnimation anim=dance1");
    }
    else if (step == 9)
    {
        look("started");
        llStopAnimation("dance1");
        emit("did", " step=" + (string)step + " call=llStopAnimation anim=dance1");
    }
    else if (step == 10)
    {
        look("stopped");
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
        begin();
        agent = llGetOwner();
        step = 0;
        look("before");
        integer mask = PERMISSION_TRIGGER_ANIMATION | PERMISSION_OVERRIDE_ANIMATIONS;
        llSetTimerEvent(15.0);
        emit("waiting", " drive=answer grant=" + (string)mask);
        llRequestPermissions(agent, mask);
    }

    run_time_permissions(integer answer)
    {
        llSetTimerEvent(0.0);
        emit("held", " step=" + (string)step + " answer=" + (string)answer
            + " perms=" + (string)llGetPermissions());
        next();
    }

    touch_start(integer count)
    {
        if (step == 6)
        {
            llSetTimerEvent(0.0);
            other = llDetectedKey(0);
            emit("touch", " step=" + (string)step
                + " key_is_owner=" + (string)(other == llGetOwner()));
            next();
        }
    }

    timer()
    {
        if (step == 0 || step == 6 || step == 7)
        {
            llSetTimerEvent(0.0);
            emit("timeout", " step=" + (string)step);
        }
        next();
    }
}
