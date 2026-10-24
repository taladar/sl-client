// probe: events/other-avatar
// drive: chat, touch
// vary: dist
//
// What a script is told about who spoke to it and who touched it, the owner
// against another avatar. It listens on channel 7 to anybody and on channel
// 8 to its owner only, then asks, in turn: the owner to say a line on 7,
// the other avatar to say one on 8 (the owner-only listener must not hear
// it; a timer moves on after three seconds), the other avatar to say one on
// 7, and the other avatar to touch. Names are never printed: whether the key
// is the owner's, whether it is an avatar, and whether the name the event
// carries is the key's own. Nor are groups: whether the object's group is
// NULL_KEY, beside llDetectedGroup; the runner notes whether the toucher
// has an active group, which no script can read (OBJECT_GROUP is NULL_KEY
// for any avatar on OpenSim).
string ID = "events/other-avatar";

integer step;

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

string who(key id, string name)
{
    vector at = llList2Vector(llGetObjectDetails(id, [OBJECT_POS]), 0);
    return " key_is_owner=" + (string)(id == llGetOwner())
        + " is_agent=" + (string)(llGetAgentSize(id) != ZERO_VECTOR)
        + " name_is_key2name=" + (string)(name == llKey2Name(id))
        + " dist=" + (string)llRound(llVecDist(at, llGetPos()));
}

key group_of(key id)
{
    return llList2Key(llGetObjectDetails(id, [OBJECT_GROUP]), 0);
}

default
{
    state_entry()
    {
        begin();
        llListen(7, "", NULL_KEY, "");
        llListen(8, "", llGetOwner(), "");
        step = 1;
        emit("waiting", " drive=chat channel=7 text=" + q("from the owner"));
    }

    listen(integer channel, string name, key id, string message)
    {
        emit("heard", " step=" + (string)step + " on=" + (string)channel
            + who(id, name) + " message=" + q(message));
        if (step == 1)
        {
            step = 2;
            emit("waiting", " drive=chat by=other channel=8 text=" + q("to the owner's listener"));
            llSetTimerEvent(3.0);
        }
        else if (step == 3)
        {
            step = 4;
            emit("waiting", " drive=touch by=other");
        }
    }

    timer()
    {
        llSetTimerEvent(0.0);
        if (step == 2)
        {
            step = 3;
            emit("waiting", " drive=chat by=other channel=7 text=" + q("from another avatar"));
        }
    }

    touch_start(integer count)
    {
        emit("touch", " step=" + (string)step + " count=" + (string)count
            + who(llDetectedKey(0), llDetectedName(0))
            + " type=" + (string)llDetectedType(0)
            + " group=" + (string)llDetectedGroup(0)
            + " object_group_null=" + (string)(group_of(llGetKey()) == NULL_KEY));
        if (step == 4)
        {
            emit("end", "");
        }
    }
}
