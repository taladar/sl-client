// probe: experience/sit-on-link
// drive: touch
//
// What llSitOnLink answers a script compiled into no experience — the only
// kind our avatars can save (see experience/no-experience). The probe's prim
// is a single prim, so its link numbers are 0 and LINK_THIS; 1, LINK_ROOT,
// LINK_SET, LINK_ALL_OTHERS, LINK_ALL_CHILDREN and 2 name either it or no
// prim at all. With no sit target, it asks to sit the owner on each of those
// link numbers, and then NULL_KEY, a key no avatar has and the prim's own key
// on link 0; it sets a sit target and asks for the owner on links 0 and 2
// again. Then the other avatar touches, so the script learns its key, and the
// script asks for it on link 0. After each call it prints what the call
// returned and, a second later, who sits on the sit target and whether the
// avatar asked for reads as sitting; the runner notes any sit the viewers are
// sent and any complaint on DEBUG_CHANNEL. Whoever sits is stood up again.
string ID = "experience/sit-on-link";

key NOBODY = "5d1f3c7a-0e4b-4c8a-9a2e-7b6f1d0c3e21";

integer step;
key other;
key agent;
string label;

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
    if (id == NOBODY)
    {
        return "nobody";
    }
    if (id == llGetKey())
    {
        return "prim";
    }
    return "someone";
}

// Asks llSitOnLink to sit `id` on `link`, prints its answer and looks again
// a second later.
sit(string name, key id, integer link)
{
    label = name;
    agent = id;
    integer code = llSitOnLink(id, link);
    emit("call", " step=" + (string)step + " case=" + name
        + " agent=" + who(id) + " link=" + (string)link
        + " code=" + (string)code);
    llSetTimerEvent(1.0);
}

// Prints who sits on the sit target and whether the avatar asked for sits.
look()
{
    key seated = llAvatarOnSitTarget();
    integer sitting = -1;
    if (agent != NULL_KEY && llGetAgentSize(agent) != ZERO_VECTOR)
    {
        sitting = (llGetAgentInfo(agent) & AGENT_SITTING) != 0;
    }
    emit("after", " step=" + (string)step + " case=" + label
        + " on_target=" + who(seated)
        + " sitting=" + (string)sitting
        + " links=" + (string)llGetNumberOfPrims());
    if (seated != NULL_KEY)
    {
        llUnSit(seated);
    }
}

next()
{
    step = step + 1;
    if (step == 1)
    {
        sit("link-0", llGetOwner(), 0);
    }
    else if (step == 2)
    {
        sit("link-this", llGetOwner(), LINK_THIS);
    }
    else if (step == 3)
    {
        sit("link-1", llGetOwner(), 1);
    }
    else if (step == 4)
    {
        sit("link-root", llGetOwner(), LINK_ROOT);
    }
    else if (step == 5)
    {
        sit("link-set", llGetOwner(), LINK_SET);
    }
    else if (step == 6)
    {
        sit("link-all-others", llGetOwner(), LINK_ALL_OTHERS);
    }
    else if (step == 7)
    {
        sit("link-all-children", llGetOwner(), LINK_ALL_CHILDREN);
    }
    else if (step == 8)
    {
        sit("link-2", llGetOwner(), 2);
    }
    else if (step == 9)
    {
        sit("null-agent", NULL_KEY, 0);
    }
    else if (step == 10)
    {
        sit("unknown-agent", NOBODY, 0);
    }
    else if (step == 11)
    {
        sit("prim-as-agent", llGetKey(), 0);
    }
    else if (step == 12)
    {
        llSitTarget(<0.0, 0.0, 0.5>, ZERO_ROTATION);
        emit("sit-target", " step=" + (string)step + " set=1");
        sit("target-link-0", llGetOwner(), 0);
    }
    else if (step == 13)
    {
        sit("target-link-2", llGetOwner(), 2);
    }
    else if (step == 14)
    {
        llSetTimerEvent(0.0);
        emit("waiting", " drive=touch by=other");
    }
    else if (step == 15)
    {
        sit("other-link-0", other, 0);
    }
    else
    {
        llSetTimerEvent(0.0);
        llSitTarget(ZERO_VECTOR, ZERO_ROTATION);
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
        look();
        next();
    }

    changed(integer change)
    {
        if (change & CHANGED_LINK)
        {
            emit("changed", " step=" + (string)step + " link=1"
                + " on_target=" + who(llAvatarOnSitTarget()));
        }
    }

    touch_start(integer count)
    {
        if (step == 14)
        {
            other = llDetectedKey(0);
            emit("touch", " step=" + (string)step + " by=" + who(other));
            next();
        }
    }
}
