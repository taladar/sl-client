// probe: persistence/restart
// script: Probe
// order: 2
// drive: answer, restart, chat
// vary: script_time, wall
//
// What a script keeps across a restart of its region, and what runs first
// when the region is back: `changed` with CHANGED_REGION_START, a
// `state_entry` or an `on_rez`. The same holdings as `persistence/rerez`:
// its globals, the state it is in, its timer, its listen, its permissions,
// the events queued for it, a sleep it is in the middle of, its script time,
// and whether a script beside it that it stopped stays stopped.
//
// The script sets each global to a value no initialiser gives it, goes into
// the state `kept`, starts a one-second timer and a listen on channel 7 (the
// owner's only), and asks the owner for PERMISSION_TRIGGER_ANIMATION and
// PERMISSION_TRACK_CAMERA. With them granted it resets its script time,
// stops the script `Stopped` and prints what it holds (`before`). Then it
// asks for the region to be restarted in 15 seconds, sends itself three link
// messages and sleeps a minute, inside which the region goes down; once the
// sleep ends it prints `woke` with how long it slept by its script time and
// by the clock. Every event that runs after that is printed, numbered
// (`event`). On CHANGED_REGION_START it prints what it holds (`after`), asks
// `Stopped` whether it is running, and asks for a line on channel 7, which
// comes only to a listen that survived; its record says whether the timer
// ticked since `after`, which only a timer that survived does.
//
// The owner is logged out while the region is down, and an owner-say to an
// owner who is not there is lost, so each line waits in a list until the
// owner is in the region again. Once the region is back the timer may be
// gone, so CHANGED_REGION_START starts a repeating sensor for a key no
// avatar has, whose `no_sensor` sends the waiting lines and counts the
// patience too.
//
// The script marks its first start in the object's description, which the
// region keeps: a `state_entry` that finds the mark is a reset, printed as
// `restarted` with what the script holds then, after which it waits for the
// region start and the line on 7 as the kept script does. On a grid that does
// not restart the region (the runner notes why), the script waits its
// patience out and ends.
string ID = "persistence/restart";
integer CHANNEL = 7;
integer PATIENCE = 40;
integer COMEBACK = 90;
float SLEEP = 60.0;
integer SECONDS = 15;
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
list waiting_lines;

string q(string s)
{
    s = llDumpList2String(llParseStringKeepNulls(s, ["\\"], []), "\\\\");
    s = llDumpList2String(llParseStringKeepNulls(s, ["\""], []), "\\\"");
    s = llDumpList2String(llParseStringKeepNulls(s, ["\n"], []), "\\n");
    return "\"" + s + "\"";
}

integer owner_here()
{
    return llGetAgentSize(llGetOwner()) != ZERO_VECTOR;
}

flush()
{
    if (!owner_here())
    {
        return;
    }
    integer count = llGetListLength(waiting_lines);
    integer i;
    for (i = 0; i < count; ++i)
    {
        llOwnerSay(llList2String(waiting_lines, i));
    }
    waiting_lines = [];
}

emit(string rec, string fields)
{
    waiting_lines += ["PROBE " + ID + " rec=" + rec + fields];
    flush();
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
    flush();
    if (awaiting == "" || !owner_here())
    {
        return;
    }
    ++waited;
    if ((awaiting == "region_start" && waited >= COMEBACK)
        || (awaiting != "region_start" && waited >= PATIENCE))
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

changes(string in_state, integer change)
{
    saw("changed", " change=" + (string)change);
    if (!(change & CHANGED_REGION_START))
    {
        return;
    }
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
            awaiting = "region_start";
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
        saw("on_rez", " param=" + (string)param);
    }

    changed(integer change)
    {
        changes("default", change);
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
        awaiting = "region_start";
        waited = 0;
        emit("waiting", " drive=restart seconds=" + (string)SECONDS);
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
        saw("on_rez", " param=" + (string)param);
    }

    changed(integer change)
    {
        changes("kept", change);
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
