// probe: experience/powers
// drive: answer, touch
// vary: off, dz
//
// The powers an experience widens — teleporting an agent and a temporary
// attachment — as a script in no experience has them. Each teleport aims
// three metres east of where the agent stands, inside the region, and the
// script reads where the agent is five seconds later (`landed` is within a
// metre of the aim, `moved` more than a metre from where it stood, `off` how
// far it is from the aim across the ground and `dz` how far above it); the
// runner notes the teleport messages each viewer is sent. In order: the
// owner, with no permission granted (llTeleportAgent); the owner, granted
// PERMISSION_TELEPORT, then back where it stood; then the other avatar
// touches, so the script learns its key, and is teleported with no
// permission, then asked for PERMISSION_TELEPORT (the runner grants it as
// that avatar, or notes that no question came) and teleported again; then
// the owner is asked for PERMISSION_TELEPORT and PERMISSION_ATTACH together,
// teleported with llTeleportAgentGlobalCoords and back, and the prim attaches
// itself to the owner's left hand with llAttachToAvatarTemp. It prints what
// llGetAttached says, ends while worn, and the runner detaches it.
string ID = "experience/powers";

integer step;
key other;
key agent;
vector from;
vector aim;

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

// `f` to one decimal, without the six a float's cast to string prints.
string num(float f)
{
    integer d = llRound(f * 10.0);
    string sign = "";
    if (d < 0)
    {
        sign = "-";
        d = -d;
    }
    return sign + (string)(d / 10) + "." + (string)(d % 10);
}

string by(key id)
{
    if (id == llGetOwner())
    {
        return "";
    }
    return " by=other";
}

vector where(key id)
{
    return llList2Vector(llGetObjectDetails(id, [OBJECT_POS]), 0);
}

// Teleports `id` three metres east (`back` FALSE) or back where it stood
// (`back` TRUE) with `call`, and reads where it is five seconds later.
teleport(key id, string call, integer back)
{
    agent = id;
    if (back)
    {
        aim = from;
    }
    else
    {
        from = where(id);
        aim = from + <3.0, 0.0, 0.0>;
    }
    emit("teleport", " step=" + (string)step + " call=" + call + " of=" + who(id)
        + " back=" + (string)back
        + " perms=" + (string)llGetPermissions()
        + " granter=" + who(llGetPermissionsKey()));
    if (call == "llTeleportAgentGlobalCoords")
    {
        llTeleportAgentGlobalCoords(id, llGetRegionCorner(), aim, aim + <1.0, 0.0, 0.0>);
    }
    else
    {
        llTeleportAgent(id, "", aim, aim + <1.0, 0.0, 0.0>);
    }
    llSetTimerEvent(5.0);
}

ask(key id, integer grant, integer mask)
{
    emit("waiting", " drive=answer" + by(id) + " grant=" + (string)grant);
    emit("asked", " step=" + (string)step + " of=" + who(id) + " mask=" + (string)mask);
    llRequestPermissions(id, mask);
    llSetTimerEvent(15.0);
}

next()
{
    step = step + 1;
    if (step == 1)
    {
        teleport(llGetOwner(), "llTeleportAgent", FALSE);
    }
    else if (step == 2)
    {
        ask(llGetOwner(), PERMISSION_TELEPORT, PERMISSION_TELEPORT);
    }
    else if (step == 3)
    {
        teleport(llGetOwner(), "llTeleportAgent", FALSE);
    }
    else if (step == 4)
    {
        teleport(llGetOwner(), "llTeleportAgent", TRUE);
    }
    else if (step == 5)
    {
        emit("waiting", " drive=touch by=other");
    }
    else if (step == 6)
    {
        teleport(other, "llTeleportAgent", FALSE);
    }
    else if (step == 7)
    {
        ask(other, PERMISSION_TELEPORT, PERMISSION_TELEPORT);
    }
    else if (step == 8)
    {
        teleport(other, "llTeleportAgent", FALSE);
    }
    else if (step == 9)
    {
        ask(llGetOwner(), PERMISSION_TELEPORT | PERMISSION_ATTACH,
            PERMISSION_TELEPORT | PERMISSION_ATTACH);
    }
    else if (step == 10)
    {
        teleport(llGetOwner(), "llTeleportAgentGlobalCoords", FALSE);
    }
    else if (step == 11)
    {
        teleport(llGetOwner(), "llTeleportAgent", TRUE);
    }
    else if (step == 12)
    {
        emit("attach-temp", " step=" + (string)step
            + " perms=" + (string)llGetPermissions()
            + " granter=" + who(llGetPermissionsKey()));
        llAttachToAvatarTemp(ATTACH_LHAND);
        llSetTimerEvent(10.0);
    }
    else
    {
        llSetTimerEvent(0.0);
        emit("end", " attached=" + (string)llGetAttached());
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

    run_time_permissions(integer answer)
    {
        llSetTimerEvent(0.0);
        emit("held", " step=" + (string)step + " answer=" + (string)answer
            + " perms=" + (string)llGetPermissions()
            + " granter=" + who(llGetPermissionsKey()));
        next();
    }

    timer()
    {
        llSetTimerEvent(0.0);
        if (step == 2 || step == 7 || step == 9)
        {
            emit("no-answer", " step=" + (string)step);
        }
        else if (step == 12)
        {
            emit("not-attached", " step=" + (string)step
                + " attached=" + (string)llGetAttached());
        }
        else
        {
            vector now = where(agent);
            emit("after", " step=" + (string)step + " of=" + who(agent)
                + " landed=" + (string)(llVecDist(now, aim) < 1.0)
                + " moved=" + (string)(llVecDist(now, from) > 1.0)
                + " off=" + num(llVecDist(<now.x, now.y, 0.0>, <aim.x, aim.y, 0.0>))
                + " dz=" + num(now.z - aim.z));
        }
        next();
    }

    attach(key id)
    {
        if (step == 12)
        {
            llSetTimerEvent(0.0);
            emit("attached", " step=" + (string)step + " to=" + who(id)
                + " point=" + (string)llGetAttached()
                + " owner=" + who(llGetOwner())
                + " perms=" + (string)llGetPermissions());
            next();
        }
    }

    touch_start(integer count)
    {
        if (step == 5)
        {
            other = llDetectedKey(0);
            emit("touch", " step=" + (string)step + " by=" + who(other));
            next();
        }
    }
}
