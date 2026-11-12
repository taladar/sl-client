// probe: avatar/sit-animation
// drive: sit, stand
//
// What a sit on a prim does to the sitter's animations, as a pose ball's
// script sees and changes them. The prim has a sit target, so the sit is
// answered from where the owner stands. Once the owner sits, the script asks
// it for TRIGGER_ANIMATION, which a seated avatar's grid grants without a
// question, and then, a second apart so the grid has stated each change
// (the runner notes every AvatarAnimation, by built-in name): reads the
// movement state and whether the built-in sit is playing; stops "sit" and
// reads again; starts "sit" again; stops it once more and starts the built-in
// dance1 as a pose ball starts its pose; and asks the owner to stand. A second
// after the stand it reads the sit and the dance once more: whether standing
// stops what the seat's script started. The movement state is read only
// while seated: before the sit and after the stand it is wherever the avatar
// was landing, falling or walking to. Only whether the list holds
// each built-in is printed, never the list, which carries whatever the
// avatar's own animation overrider plays. A timer moves on after twenty
// seconds without a sit, a stand or an answer.
string ID = "avatar/sit-animation";

key SIT = "1a5fe8ac-a804-8a5d-7cbd-56bd83184568";
key DANCE = "b68a3d7c-de9e-fc87-eec8-543d787e5b0d";

integer step;
integer granted;

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

look(string why)
{
    key owner = llGetOwner();
    list playing = llGetAnimationList(owner);
    // The state before the sit and a second after the stand is wherever the
    // avatar was landing, falling or walking to, which is timing, not the
    // grid's rule: only a seated avatar's is read.
    string state_field = "";
    if (why != "before" && why != "stood")
    {
        state_field = " state=" + q(llGetAnimation(owner));
    }
    emit("playing", " step=" + (string)step + " why=" + why
        + state_field
        + " has_sit=" + (string)(llListFindList(playing, [SIT]) != -1)
        + " has_dance=" + (string)(llListFindList(playing, [DANCE]) != -1)
        + " info_sitting=" + (string)((llGetAgentInfo(owner) & AGENT_SITTING) != 0)
        + " info_on_object=" + (string)((llGetAgentInfo(owner) & AGENT_ON_OBJECT) != 0));
}

next()
{
    step = step + 1;
    llSetTimerEvent(1.0);
    if (step == 1)
    {
        look("seated");
        llStopAnimation("sit");
        emit("did", " step=" + (string)step + " call=llStopAnimation anim=sit");
    }
    else if (step == 2)
    {
        look("sit-stopped");
        llStartAnimation("sit");
        emit("did", " step=" + (string)step + " call=llStartAnimation anim=sit");
    }
    else if (step == 3)
    {
        look("sit-started");
        llStopAnimation("sit");
        llStartAnimation("dance1");
        emit("did", " step=" + (string)step + " call=llStopAnimation anim=sit"
            + " call=llStartAnimation anim=dance1");
    }
    else if (step == 4)
    {
        look("posed");
        llSetTimerEvent(20.0);
        emit("waiting", " drive=stand");
    }
    else if (step == 5)
    {
        look("stood");
        llSetTimerEvent(0.0);
        llStopAnimation("dance1");
        emit("end", "");
    }
}

default
{
    state_entry()
    {
        begin();
        step = 0;
        llSitTarget(<0.0, 0.0, 0.5>, ZERO_ROTATION);
        look("before");
        llSetTimerEvent(20.0);
        emit("waiting", " drive=sit");
    }

    changed(integer change)
    {
        if (change & CHANGED_LINK)
        {
            key sitter = llAvatarOnSitTarget();
            if (step == 0 && sitter == llGetOwner())
            {
                llSetTimerEvent(20.0);
                emit("asked", " step=" + (string)step + " mask="
                    + (string)PERMISSION_TRIGGER_ANIMATION);
                llRequestPermissions(sitter, PERMISSION_TRIGGER_ANIMATION);
            }
            else if (step == 4 && sitter == NULL_KEY)
            {
                // A second for the grid to state what the stand did.
                llSetTimerEvent(1.0);
            }
        }
    }

    run_time_permissions(integer answer)
    {
        emit("held", " step=" + (string)step + " answer=" + (string)answer
            + " perms=" + (string)llGetPermissions());
        if (answer & PERMISSION_TRIGGER_ANIMATION)
        {
            granted = TRUE;
            // A second for the grid to state the sit.
            llSetTimerEvent(1.0);
        }
    }

    timer()
    {
        if (step == 0 && !granted)
        {
            llSetTimerEvent(0.0);
            emit("timeout", " step=" + (string)step);
            emit("end", "");
        }
        else if (step == 4 && llAvatarOnSitTarget() != NULL_KEY)
        {
            llSetTimerEvent(0.0);
            emit("timeout", " step=" + (string)step);
            emit("end", "");
        }
        else
        {
            next();
        }
    }
}
