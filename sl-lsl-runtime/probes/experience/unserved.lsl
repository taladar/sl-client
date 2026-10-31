// probe: experience/unserved
// drive: answer, touch
//
// What the two experience functions no served document lists —
// llGetExperienceList and llClearExperiencePermissions — do for a script
// compiled into no experience (see experience/no-experience). Second Life
// compiles both; OpenSim compiles neither. In order: llGetExperienceList of
// the owner and of NULL_KEY; llClearExperiencePermissions of the owner and of
// NULL_KEY, each followed by three seconds in which the script prints any
// event it gets and then what it holds; then the owner grants
// PERMISSION_TRIGGER_ANIMATION and PERMISSION_TELEPORT, and the script clears
// the owner's experience permissions again, to see whether an ordinary grant
// is touched; then the other avatar touches, so the script learns its key,
// and the script asks llGetExperienceList of it and clears its experience
// permissions. The runner notes any question and any complaint on
// DEBUG_CHANNEL.
string ID = "experience/unserved";

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

held(string rec)
{
    emit(rec, " step=" + (string)step
        + " perms=" + (string)llGetPermissions()
        + " granter=" + who(llGetPermissionsKey()));
}

// Prints what llGetExperienceList answers for `id`.
listed(key id)
{
    list got = llGetExperienceList(id);
    integer n = llGetListLength(got);
    list types;
    integer i;
    for (i = 0; i < n; ++i)
    {
        types += [llGetListEntryType(got, i)];
    }
    emit("list", " step=" + (string)step + " of=" + who(id)
        + " length=" + (string)n
        + " types=" + q(llList2CSV(types))
        + " items=" + q(llList2CSV(got)));
}

// Clears `id`'s experience permissions and waits three seconds for an event.
clear(key id)
{
    emit("clear", " step=" + (string)step + " of=" + who(id));
    llClearExperiencePermissions(id);
    held("cleared");
    llSetTimerEvent(3.0);
}

next()
{
    step = step + 1;
    if (step == 1)
    {
        listed(llGetOwner());
        next();
    }
    else if (step == 2)
    {
        listed(NULL_KEY);
        next();
    }
    else if (step == 3)
    {
        clear(llGetOwner());
    }
    else if (step == 4)
    {
        clear(NULL_KEY);
    }
    else if (step == 5)
    {
        emit("waiting", " drive=answer grant="
            + (string)(PERMISSION_TRIGGER_ANIMATION | PERMISSION_TELEPORT));
        llRequestPermissions(llGetOwner(),
            PERMISSION_TRIGGER_ANIMATION | PERMISSION_TELEPORT);
        llSetTimerEvent(15.0);
    }
    else if (step == 6)
    {
        clear(llGetOwner());
    }
    else if (step == 7)
    {
        listed(llGetOwner());
        llSetTimerEvent(0.0);
        emit("waiting", " drive=touch by=other");
    }
    else if (step == 8)
    {
        listed(other);
        clear(other);
    }
    else
    {
        llSetTimerEvent(0.0);
        held("end-held");
        emit("end", "");
    }
}

default
{
    state_entry()
    {
        begin();
        step = 0;
        next();
    }

    timer()
    {
        llSetTimerEvent(0.0);
        if (step == 5)
        {
            emit("no-answer", " step=" + (string)step);
        }
        else
        {
            held("after");
        }
        next();
    }

    run_time_permissions(integer answer)
    {
        emit("run-time-permissions", " step=" + (string)step
            + " answer=" + (string)answer);
        if (step == 5)
        {
            llSetTimerEvent(0.0);
            held("held");
            next();
        }
    }

    experience_permissions(key agent)
    {
        emit("granted", " step=" + (string)step + " agent=" + who(agent));
    }

    experience_permissions_denied(key agent, integer reason)
    {
        emit("denied", " step=" + (string)step + " agent=" + who(agent)
            + " reason=" + (string)reason);
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
