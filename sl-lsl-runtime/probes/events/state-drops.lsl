// probe: events/state-drops
// drive: answer, control, chat
// vary: sensors, ticks
//
// What a state change keeps of the things a script has asked for, beyond
// the queue, the timer and the listens `events/state-change` measured: a
// repeating sensor, taken controls and the permission that took them,
// position and rotation targets, and a listen opened in `state_exit`.
//
// In `default`, granted TAKE_CONTROLS, the script takes turn-left (accepted,
// not passed on), starts a half-second repeating sensor for avatars and a
// half-second timer, and listens to the owner on 7; the runner holds
// turn-left and the owner says a line on 7. After seven seconds it prints
// what it got, sets a position target and a rotation target it is already
// at, and changes to `two`; `state_exit` opens a listen to the owner on 8.
// In `two` the runner holds turn-left again and the owner says a line on 7
// and one on 8; seven seconds in, `two` prints what it got there — sensor
// events, presses, lines heard on 7 and on 8, whether `at_target` and
// `at_rot_target` ran — then sets fresh targets and a single sensor, to show
// each works in `two`, and two seconds later prints those and ends. The
// runner notes the ScriptControlChange the grid sends the viewer, if any, at
// the state change.
string ID = "events/state-drops";

integer ticks;
integer sensors;
integer presses;
integer heard7;
integer heard8;
integer at;
integer rot_at;
integer target;
integer rot_target;
integer exits;

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

string counts()
{
    return " sensed=" + (string)(sensors > 0) + " sensors=" + (string)sensors
        + " presses=" + (string)presses
        + " heard7=" + (string)heard7 + " heard8=" + (string)heard8
        + " at_target=" + (string)at + " at_rot_target=" + (string)rot_at
        + " ticks=" + (string)ticks;
}

reset()
{
    ticks = 0;
    sensors = 0;
    presses = 0;
    heard7 = 0;
    heard8 = 0;
    at = 0;
    rot_at = 0;
}

default
{
    state_entry()
    {
        begin();
        reset();
        emit("waiting", " drive=answer grant=" + (string)PERMISSION_TAKE_CONTROLS);
        llRequestPermissions(llGetOwner(), PERMISSION_TAKE_CONTROLS);
    }

    run_time_permissions(integer perms)
    {
        emit("held", " perms=" + (string)perms);
        llTakeControls(CONTROL_ROT_LEFT, TRUE, FALSE);
        llSensorRepeat("", NULL_KEY, AGENT, 30.0, PI, 0.5);
        llListen(7, "", llGetOwner(), "");
        llSetTimerEvent(0.5);
        emit("waiting", " drive=control keys=" + (string)CONTROL_ROT_LEFT);
        emit("waiting", " drive=chat channel=7 text=\"before\"");
    }

    sensor(integer count)
    {
        ++sensors;
    }

    control(key id, integer level, integer edge)
    {
        if (level & edge)
        {
            ++presses;
        }
    }

    listen(integer channel, string name, key id, string message)
    {
        ++heard7;
    }

    timer()
    {
        ++ticks;
        if (ticks == 14)
        {
            emit("default", counts());
            target = llTarget(llGetPos(), 100.0);
            rot_target = llRotTarget(llGetRot(), PI);
            state two;
        }
    }

    state_exit()
    {
        ++exits;
        llListen(8, "", llGetOwner(), "");
    }
}

state two
{
    state_entry()
    {
        reset();
        emit("entered", " exits=" + (string)exits
            + " perms=" + (string)llGetPermissions()
            + " perm_key_is_owner=" + (string)(llGetPermissionsKey() == llGetOwner()));
        emit("waiting", " drive=control keys=" + (string)CONTROL_ROT_LEFT);
        emit("waiting", " drive=chat channel=7 text=\"after\"");
        emit("waiting", " drive=chat channel=8 text=\"from state_exit\"");
    }

    sensor(integer count)
    {
        ++sensors;
    }

    no_sensor()
    {
        ++sensors;
    }

    control(key id, integer level, integer edge)
    {
        if (level & edge)
        {
            ++presses;
        }
    }

    listen(integer channel, string name, key id, string message)
    {
        if (channel == 8)
        {
            ++heard8;
        }
        else
        {
            ++heard7;
        }
    }

    at_target(integer number, vector wanted, vector now)
    {
        ++at;
        llTargetRemove(number);
    }

    at_rot_target(integer number, rotation wanted, rotation now)
    {
        ++rot_at;
        llRotTargetRemove(number);
    }

    timer()
    {
        ++ticks;
        if (ticks == 14)
        {
            emit("two", counts());
            reset();
            ticks = 14;
            target = llTarget(llGetPos(), 100.0);
            rot_target = llRotTarget(llGetRot(), PI);
            llSensor("", NULL_KEY, AGENT, 30.0, PI);
        }
        else if (ticks == 18)
        {
            emit("fresh", " sensed=" + (string)(sensors > 0)
                + " at_target=" + (string)at + " at_rot_target=" + (string)rot_at);
            llSetTimerEvent(0.0);
            llSensorRemove();
            llReleaseControls();
            emit("end", "");
        }
    }
}
