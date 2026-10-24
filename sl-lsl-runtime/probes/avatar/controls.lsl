// probe: avatar/controls
// drive: answer, control
// vary: repeats
//
// llTakeControls and the control event. Granted TAKE_CONTROLS, the script
// takes forward and turn-left, accepted and not passed on (the runner notes
// the ScriptControlChange the grid sends the viewer), and the runner holds
// each down for a second in turn, then back, which was not taken. Each
// control event that changes a key (a press or a release) is printed; the
// events between, while a key is held, are counted as `repeats`, a
// measurement of the frame rate. Then the script takes turn-left with
// accept FALSE and pass-on TRUE (taking nothing), releases its controls, and
// takes turn-left once more, each time with the runner holding turn-left
// down, and says what permissions it holds after the release. Turn-left is
// held rather than forward wherever the key might reach the avatar, so the
// probe never walks it away. A timer moves on three seconds after a hold
// with no control event.
string ID = "avatar/controls";

integer step;
integer repeats;

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

hold(integer keys)
{
    repeats = 0;
    emit("waiting", " drive=control keys=" + (string)keys);
    llSetTimerEvent(5.0);
}

next()
{
    step = step + 1;
    if (step == 1)
    {
        llTakeControls(CONTROL_FWD | CONTROL_ROT_LEFT, TRUE, FALSE);
        emit("took", " step=" + (string)step + " controls="
            + (string)(CONTROL_FWD | CONTROL_ROT_LEFT) + " accept=1 pass_on=0");
        hold(CONTROL_FWD);
    }
    else if (step == 2)
    {
        hold(CONTROL_ROT_LEFT);
    }
    else if (step == 3)
    {
        hold(CONTROL_BACK);
    }
    else if (step == 4)
    {
        llTakeControls(CONTROL_ROT_LEFT, FALSE, TRUE);
        emit("took", " step=" + (string)step + " controls="
            + (string)CONTROL_ROT_LEFT + " accept=0 pass_on=1");
        hold(CONTROL_ROT_LEFT);
    }
    else if (step == 5)
    {
        llReleaseControls();
        emit("released", " step=" + (string)step
            + " perms=" + (string)llGetPermissions());
        hold(CONTROL_ROT_LEFT);
    }
    else if (step == 6)
    {
        llTakeControls(CONTROL_ROT_LEFT, TRUE, TRUE);
        emit("took", " step=" + (string)step + " controls="
            + (string)CONTROL_ROT_LEFT + " accept=1 pass_on=1"
            + " perms=" + (string)llGetPermissions());
        hold(CONTROL_ROT_LEFT);
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
        step = 0;
        emit("waiting", " drive=answer grant=" + (string)PERMISSION_TAKE_CONTROLS);
        llRequestPermissions(llGetOwner(), PERMISSION_TAKE_CONTROLS);
    }

    run_time_permissions(integer answer)
    {
        emit("held", " answer=" + (string)answer
            + " perms=" + (string)llGetPermissions());
        next();
    }

    control(key id, integer level, integer edge)
    {
        if (edge == 0)
        {
            repeats = repeats + 1;
            return;
        }
        emit("control", " step=" + (string)step
            + " key_is_owner=" + (string)(id == llGetOwner())
            + " level=" + (string)level + " edge=" + (string)edge
            + " pressed=" + (string)(level & edge)
            + " released=" + (string)(~level & edge)
            + " repeats=" + (string)repeats);
        repeats = 0;
        if (level == 0)
        {
            llSetTimerEvent(3.0);
        }
    }

    timer()
    {
        llSetTimerEvent(0.0);
        emit("hold-over", " step=" + (string)step + " repeats=" + (string)repeats);
        next();
    }
}
