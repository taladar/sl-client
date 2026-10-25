// probe: experience/no-experience
// drive: touch
// unordered: kv-answer
//
// What the experience functions answer a script that was compiled into no
// experience — the only kind our avatars can save, since none of them owns,
// administers or contributes to one (`book/src/gridspec/experiences.md`).
// It asks whether the owner is in the script's experience
// (llAgentInExperience); reads llGetExperienceDetails for NULL_KEY (the
// script's own experience), for a public experience of a stranger's on aditi
// (Bugsly Testing) and for an id no experience has, asking again each second
// up to five times while the answer is empty, since the simulator may have to
// fetch the record first; prints llGetExperienceErrorMessage for every code
// from -1 to 20; asks the owner for experience permissions and waits ten
// seconds for experience_permissions or experience_permissions_denied (the
// runner notes any question the viewer is put); calls each of the seven
// key-value functions and prints the key each returns and what the dataserver
// answers to it within ten seconds. Then the other avatar touches, so the
// script learns its key, and the script asks it for experience permissions
// and asks llAgentInExperience of it.
string ID = "experience/no-experience";

key BUGSLY = "040979ee-0aa5-11e8-8c52-00304892026a";
key NOBODY = "5d1f3c7a-0e4b-4c8a-9a2e-7b6f1d0c3e21";

integer step;
integer tries;
key other;
list pending;
list names;

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

string role(key id)
{
    if (id == NULL_KEY)
    {
        return "nil";
    }
    if (id == BUGSLY)
    {
        return "bugsly";
    }
    if (id == NOBODY)
    {
        return "nobody";
    }
    if (id == llGetOwner())
    {
        return "owner";
    }
    if ((string)id == "")
    {
        return "empty";
    }
    return "set";
}

// Prints what llGetExperienceDetails answers for `id`; returns whether the
// answer was empty.
integer details(string of, key id)
{
    list got = llGetExperienceDetails(id);
    integer n = llGetListLength(got);
    string fields = " of=" + of + " try=" + (string)tries + " length=" + (string)n;
    if (n > 0)
    {
        fields += " name=" + q(llList2String(got, 0))
            + " owner=" + role(llList2Key(got, 1))
            + " experience=" + role(llList2Key(got, 2))
            + " state=" + q(llList2String(got, 3))
            + " message=" + q(llList2String(got, 4))
            + " group=" + role(llList2Key(got, 5));
        if (n > 6)
        {
            fields += " rest=" + q(llList2CSV(llList2List(got, 6, -1)));
        }
    }
    emit("details", fields);
    return n == 0;
}

// Keeps the request `id` a key-value function `call` returned for its
// dataserver answer, and prints the id.
kv(string call, key id)
{
    emit("kv-call", " call=" + call + " request=" + role(id));
    if (id != NULL_KEY && (string)id != "")
    {
        pending += [id];
        names += [call];
    }
}

ask(key agent)
{
    emit("asked", " step=" + (string)step + " of=" + who(agent));
    llRequestExperiencePermissions(agent, "");
    llSetTimerEvent(10.0);
}

next()
{
    step = step + 1;
    tries = 0;
    if (step == 1)
    {
        emit("in-experience", " of=owner answer="
            + (string)llAgentInExperience(llGetOwner()));
        next();
    }
    else if (step == 2)
    {
        if (details("own", NULL_KEY))
        {
            llSetTimerEvent(1.0);
        }
        else
        {
            next();
        }
    }
    else if (step == 3)
    {
        if (details("bugsly", BUGSLY))
        {
            llSetTimerEvent(1.0);
        }
        else
        {
            next();
        }
    }
    else if (step == 4)
    {
        if (details("nobody", NOBODY))
        {
            llSetTimerEvent(1.0);
        }
        else
        {
            next();
        }
    }
    else if (step == 5)
    {
        integer code;
        for (code = -1; code <= 20; ++code)
        {
            emit("error-message", " code=" + (string)code
                + " text=" + q(llGetExperienceErrorMessage(code)));
        }
        next();
    }
    else if (step == 6)
    {
        ask(llGetOwner());
    }
    else if (step == 7)
    {
        kv("llCreateKeyValue", llCreateKeyValue("probe", "one"));
        kv("llReadKeyValue", llReadKeyValue("probe"));
        kv("llUpdateKeyValue", llUpdateKeyValue("probe", "two", FALSE, ""));
        kv("llDeleteKeyValue", llDeleteKeyValue("probe"));
        kv("llKeyCountKeyValue", llKeyCountKeyValue());
        kv("llKeysKeyValue", llKeysKeyValue(0, 10));
        kv("llDataSizeKeyValue", llDataSizeKeyValue());
        llSetTimerEvent(10.0);
    }
    else if (step == 8)
    {
        emit("waiting", " drive=touch by=other");
    }
    else if (step == 9)
    {
        emit("in-experience", " of=other answer="
            + (string)llAgentInExperience(other));
        ask(other);
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
        next();
    }

    timer()
    {
        llSetTimerEvent(0.0);
        if (step >= 2 && step <= 4)
        {
            tries = tries + 1;
            string of = "own";
            key id = NULL_KEY;
            if (step == 3)
            {
                of = "bugsly";
                id = BUGSLY;
            }
            else if (step == 4)
            {
                of = "nobody";
                id = NOBODY;
            }
            if (details(of, id) && tries < 5)
            {
                llSetTimerEvent(1.0);
                return;
            }
        }
        else if (step == 6 || step == 9)
        {
            emit("no-answer", " step=" + (string)step);
        }
        else if (step == 7)
        {
            integer n = llGetListLength(names);
            integer i;
            for (i = 0; i < n; ++i)
            {
                emit("kv-unanswered", " call=" + llList2String(names, i));
            }
            pending = [];
            names = [];
        }
        next();
    }

    experience_permissions(key agent)
    {
        llSetTimerEvent(0.0);
        emit("granted", " step=" + (string)step + " agent=" + who(agent)
            + " perms=" + (string)llGetPermissions()
            + " granter=" + who(llGetPermissionsKey()));
        next();
    }

    experience_permissions_denied(key agent, integer reason)
    {
        llSetTimerEvent(0.0);
        emit("denied", " step=" + (string)step + " agent=" + who(agent)
            + " reason=" + (string)reason
            + " text=" + q(llGetExperienceErrorMessage(reason))
            + " perms=" + (string)llGetPermissions());
        next();
    }

    dataserver(key id, string data)
    {
        integer at = llListFindList(pending, [id]);
        if (at < 0)
        {
            emit("dataserver", " request=unknown data=" + q(data));
            return;
        }
        emit("kv-answer", " call=" + llList2String(names, at) + " data=" + q(data));
        pending = llDeleteSubList(pending, at, at);
        names = llDeleteSubList(names, at, at);
        if (step == 7 && pending == [])
        {
            llSetTimerEvent(0.0);
            next();
        }
    }

    touch_start(integer count)
    {
        if (step == 8)
        {
            other = llDetectedKey(0);
            emit("touch", " step=" + (string)step + " by=" + who(other));
            next();
        }
    }
}
