// probe: events/detected-cleared
// drive: touch
//
// Whether the detected block outlives its event: on a touch the script
// reads llDetected* in the touch itself, queues a link message and starts a
// short timer, and reads llDetected* again in each (Second Life: any other
// event clears the block). Touch the prim once; the owner's own touch, so
// `key_is_owner` should be 1 in the touch. Names are never printed.
string ID = "events/detected-cleared";

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

detected(string where)
{
    emit(where, " key_is_owner=" + (string)(llDetectedKey(0) == llGetOwner())
        + " key_is_null=" + (string)(llDetectedKey(0) == NULL_KEY)
        + " name_empty=" + (string)(llDetectedName(0) == "")
        + " name_is_null_key=" + (string)(llDetectedName(0) == (string)NULL_KEY)
        + " type=" + (string)llDetectedType(0)
        + " face=" + (string)llDetectedTouchFace(0)
        + " link=" + (string)llDetectedLinkNumber(0));
}

default
{
    state_entry()
    {
        begin();
        emit("waiting", " drive=touch");
    }

    touch_start(integer count)
    {
        detected("touch");
        llMessageLinked(LINK_THIS, 0, "", "");
        llSetTimerEvent(0.5);
    }

    link_message(integer sender, integer num, string msg, key id)
    {
        detected("link_message");
    }

    timer()
    {
        llSetTimerEvent(0.0);
        detected("timer");
        emit("end", "");
    }
}
