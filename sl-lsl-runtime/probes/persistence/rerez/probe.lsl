// probe: persistence/rerez
// script: Probe
// order: 2
// drive: answer, rerez, chat
// vary: script_time, wall
//
// What a script keeps when its object is taken into inventory and rezzed
// again: its globals, the state it is in, its timer, its listen, the
// permissions it holds, the events queued for it, a sleep it is in the
// middle of, its script time, and whether a script beside it that it
// stopped stays stopped.
//
// The script sets each global to a value no initialiser gives it, goes into
// the state `kept`, and there starts a one-second timer and a listen on
// channel 7 (the owner's only), and asks the owner for
// PERMISSION_TRIGGER_ANIMATION and PERMISSION_TRACK_CAMERA. With them
// granted it resets its script time, stops the script `Stopped` and prints
// what it holds (`before`). Then it asks to be taken and rezzed again, sends
// itself three link messages and sleeps eight seconds, inside which the take
// comes; once the sleep ends it prints `woke` with how long it slept by its
// script time and by the clock. The events that run after that are printed
// as they come, numbered (`event`). On `on_rez` it prints what it holds now
// (`after`), asks `Stopped` whether it is running, and asks for a line on
// channel 7, which comes only to a listen that survived; its record says
// whether the timer ticked since `after`, which only a timer that survived
// does.
//
// The script marks its first start in the object's description, which a take
// keeps: a `state_entry` that finds the mark is a reset the grid did, and is
// printed as `restarted` with what the script holds then, after which it
// waits for `on_rez` and the line on 7 as the kept script does.
//
// Once rezzed again the timer may be gone, so `on_rez` starts a repeating
// sensor for a key no avatar has, whose `no_sensor` counts the patience too.
string ID = "persistence/rerez";
integer CHANNEL = 7;
integer PATIENCE = 15;
float SLEEP = 8.0;
integer PERMS = 1040;
integer ASK_STOPPED = 99;
integer STOPPED_SAYS = 98;

integer gi;
float gf;
string gs;
key gk;
vector gv;
rotation gr;
list gl;

key first;
integer ticks;
integer stepped;
integer seq;
integer waited;
string awaiting;

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

string held(string in_state)
{
    return " state=" + in_state
        + " gi=" + (string)gi + " gf=" + (string)gf + " gs=" + q(gs)
        + " gk_is_owner=" + (string)(gk == llGetOwner())
        + " gv=" + q((string)gv) + " gr=" + q((string)gr)
        + " gl=" + q(llList2CSV(gl))
        + " perms=" + (string)llGetPermissions()
        + " perms_key_is_owner=" + (string)(llGetPermissionsKey() == llGetOwner())
        + " stopped_state=" + (string)llGetScriptState("Stopped")
        + " start_param=" + (string)llGetStartParameter()
        + " script_time=" + (string)llGetTime()
        + " timer=" + (string)(ticks != stepped)
        + " key_same=" + (string)(llGetKey() == first);
}

saw(string type, string fields)
{
    ++seq;
    emit("event", " seq=" + (string)seq + " type=" + type + fields);
    if (type == awaiting)
    {
        awaiting = "";
    }
}

patience()
{
    if (awaiting == "")
    {
        return;
    }
    ++waited;
    if (waited >= PATIENCE)
    {
        emit("timeout", " awaited=" + awaiting);
        finish();
    }
}

finish()
{
    awaiting = "";
    llSetTimerEvent(0.0);
    llSensorRemove();
    emit("end", "");
}

rezzed(string in_state, integer param)
{
    saw("on_rez", " param=" + (string)param);
    llSensorRepeat("", "00000000-0000-0000-0000-00000000dead", AGENT, 1.0, PI,
        1.0);
    emit("after", held(in_state));
    stepped = ticks;
    llMessageLinked(LINK_THIS, ASK_STOPPED, "", NULL_KEY);
    awaiting = "listen";
    waited = 0;
    emit("waiting", " drive=chat channel=" + (string)CHANNEL
        + " text=\"after\"");
}

linked(integer number, string text)
{
    if (number == STOPPED_SAYS)
    {
        saw("stopped_running", " text=" + q(text));
    }
    else if (number != ASK_STOPPED)
    {
        saw("link_message", " number=" + (string)number + " text=" + q(text));
    }
}

heard(string message)
{
    saw("listen", " message=" + q(message)
        + " timer=" + (string)(ticks != stepped));
    finish();
}

default
{
    state_entry()
    {
        if (llGetObjectDesc() == ID)
        {
            emit("restarted", held("default"));
            awaiting = "on_rez";
            llListen(CHANNEL, "", llGetOwner(), "");
            llSetTimerEvent(1.0);
            return;
        }
        emit("start", " region=" + q(llGetRegionName())
            + " channel=" + q(llGetEnv("sim_channel"))
            + " version=" + q(llGetEnv("sim_version")));
        llSetObjectDesc(ID);
        first = llGetKey();
        gi = 42;
        gf = 2.5;
        gs = "kept";
        gk = llGetOwner();
        gv = <1.0, 2.0, 3.0>;
        gr = <0.0, 0.0, 0.70710677, 0.70710677>;
        gl = [1, "two", 3.0];
        state kept;
    }

    on_rez(integer param)
    {
        rezzed("default", param);
    }

    changed(integer change)
    {
        saw("changed", " change=" + (string)change);
    }

    link_message(integer sender, integer number, string text, key id)
    {
        linked(number, text);
    }

    listen(integer channel, string name, key id, string message)
    {
        heard(message);
    }

    timer()
    {
        ++ticks;
        patience();
    }

    no_sensor()
    {
        patience();
    }
}

state kept
{
    state_entry()
    {
        llListen(CHANNEL, "", llGetOwner(), "");
        llSetTimerEvent(1.0);
        awaiting = "run_time_permissions";
        emit("waiting", " drive=answer grant=" + (string)PERMS);
        llRequestPermissions(llGetOwner(), PERMS);
    }

    run_time_permissions(integer perms)
    {
        saw("run_time_permissions", " perms=" + (string)perms);
        llResetTime();
        llSetScriptState("Stopped", FALSE);
        emit("before", held("kept"));
        stepped = ticks;
        awaiting = "on_rez";
        waited = 0;
        emit("waiting", " drive=rerez");
        llMessageLinked(LINK_THIS, 1, "queued", NULL_KEY);
        llMessageLinked(LINK_THIS, 2, "queued", NULL_KEY);
        llMessageLinked(LINK_THIS, 3, "queued", NULL_KEY);
        float slept = llGetTime();
        integer wall = llGetUnixTime();
        llSleep(SLEEP);
        emit("woke", " script_time=" + (string)(llGetTime() - slept)
            + " wall=" + (string)(llGetUnixTime() - wall)
            + " key_same=" + (string)(llGetKey() == first));
    }

    on_rez(integer param)
    {
        rezzed("kept", param);
    }

    changed(integer change)
    {
        saw("changed", " change=" + (string)change);
    }

    link_message(integer sender, integer number, string text, key id)
    {
        linked(number, text);
    }

    listen(integer channel, string name, key id, string message)
    {
        heard(message);
    }

    timer()
    {
        ++ticks;
        patience();
    }

    no_sensor()
    {
        patience();
    }
}
