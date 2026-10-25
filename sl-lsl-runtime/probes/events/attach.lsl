// probe: events/attach
// drive: attach, drop, answer, chat, rerez
//
// The events a script runs when its object is attached, dropped, and taken
// and rezzed again, and what of its running state each keeps. Every event
// of those kinds — `attach`, `on_rez`, `changed`, `run_time_permissions`,
// `listen` — is printed as it runs, numbered, with the step it came in.
//
// Step 1: the runner attaches the prim to the owner's left hand from the
// ground. Step 2: the owner says a line on channel 7, which the script has
// listened to (the owner's only) since it started. Step 3: the runner drops
// it. Step 4: the script asks for PERMISSION_ATTACH and, granted, attaches
// itself with `llAttachToAvatar`. Step 5: dropped again. Step 6: the runner
// takes the prim into the owner's inventory and rezzes it back where it
// stood. Step 7: the owner says a line on 7 again. Each step's record says
// whether the one-second timer the script started ticked since the step
// before (`timer`), the prim's key is the one it started with (`key_same`),
// and what `llGetAttached` answers.
//
// A step ends when its event comes, or after twelve timer ticks. Once
// rezzed again the timer may be gone, so a repeating sensor that finds
// nothing (a key no avatar has) then counts too. The script marks its first
// start in the object's description, which a take keeps: a `state_entry`
// that finds the mark is a reset the grid did, and is printed as
// `restarted`, after which the script waits for `on_rez` and ends.
string ID = "events/attach";
integer CHANNEL = 7;
integer PATIENCE = 12;

key first;
integer restarted;
integer step;
integer seq;
integer ticks;
integer stepped;
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

begin()
{
    emit("start", " region=" + q(llGetRegionName())
        + " channel=" + q(llGetEnv("sim_channel"))
        + " version=" + q(llGetEnv("sim_version")));
}

string where()
{
    return " attached=" + (string)llGetAttached()
        + " key_same=" + (string)(llGetKey() == first);
}

saw(string type, string fields)
{
    ++seq;
    emit("event", " step=" + (string)step + " seq=" + (string)seq
        + " type=" + type + fields + where());
    if (type == awaiting)
    {
        next();
    }
}

ask(string kind, string fields, string event_type)
{
    awaiting = event_type;
    waited = 0;
    emit("waiting", " drive=" + kind + fields);
}

next()
{
    ++step;
    if (step == 7 && restarted)
    {
        step = 8;
    }
    awaiting = "";
    emit("step", " step=" + (string)step
        + " timer=" + (string)(ticks != stepped) + where());
    stepped = ticks;
    if (step == 1)
    {
        ask("attach", " point=" + (string)ATTACH_LHAND, "attach");
    }
    else if (step == 2 || step == 7)
    {
        ask("chat", " channel=" + (string)CHANNEL + " text=\"step "
            + (string)step + "\"", "listen");
    }
    else if (step == 3 || step == 5)
    {
        ask("drop", "", "attach");
    }
    else if (step == 4)
    {
        ask("answer", " grant=" + (string)PERMISSION_ATTACH, "attach");
        llRequestPermissions(llGetOwner(), PERMISSION_ATTACH);
    }
    else if (step == 6)
    {
        ask("rerez", "", "on_rez");
    }
    else
    {
        llSetTimerEvent(0.0);
        llSensorRemove();
        emit("end", "");
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
        emit("timeout", " step=" + (string)step + " awaited=" + awaiting);
        next();
    }
}

default
{
    state_entry()
    {
        if (llGetObjectDesc() == ID)
        {
            restarted = TRUE;
            first = NULL_KEY;
            step = 6;
            awaiting = "on_rez";
            emit("restarted", where());
            llSetTimerEvent(1.0);
            return;
        }
        begin();
        llSetObjectDesc(ID);
        first = llGetKey();
        step = 0;
        llListen(CHANNEL, "", llGetOwner(), "");
        llSetTimerEvent(1.0);
        next();
    }

    attach(key id)
    {
        saw("attach", " id_is_owner=" + (string)(id == llGetOwner())
            + " id_is_null=" + (string)(id == NULL_KEY));
    }

    on_rez(integer param)
    {
        llSensorRepeat("", "00000000-0000-0000-0000-00000000dead", AGENT, 1.0,
            PI, 1.0);
        saw("on_rez", " param=" + (string)param);
    }

    changed(integer change)
    {
        saw("changed", " change=" + (string)change);
    }

    run_time_permissions(integer perms)
    {
        saw("run_time_permissions", " perms=" + (string)perms);
        if (perms & PERMISSION_ATTACH)
        {
            llAttachToAvatar(ATTACH_LHAND);
        }
    }

    listen(integer channel, string name, key id, string message)
    {
        saw("listen", " message=" + q(message));
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
