// probe: comms/owner-away
// drive: away
// vary: after_polls
//
// What reaches the owner while it is in another region. The script sends
// llOwnerSay, llRegionSayTo on channel 0 and llInstantMessage to the owner
// once while it is here, as a baseline; then asks the runner to take the
// owner away for twenty seconds (a teleport to a neighbour on OpenSim; on
// aditi, where Mauve has none on the map, to a region nearby). It watches
// for the owner to leave (llGetAgentSize and OBJECT_POS through
// llGetObjectDetails), sends the same three while it is gone, and once the
// owner is back prints what it saw. Its own probe lines while the owner is
// away are kept back, since llOwnerSay is what is being measured; the
// runner notes the three messages that do reach the owner, wherever it is.
string ID = "comms/owner-away";

integer phase;
integer polls;
list kept;

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

key owner()
{
    return llGetOwner();
}

string presence()
{
    list details = llGetObjectDetails(owner(), [OBJECT_POS]);
    return " agent_size_zero=" + (string)(llGetAgentSize(owner()) == ZERO_VECTOR)
        + " details=" + (string)llGetListLength(details);
}

send(string when)
{
    llOwnerSay("owner-say " + when);
    llRegionSayTo(owner(), 0, "region-say-to " + when);
    llInstantMessage(owner(), "im " + when);
}

default
{
    state_entry()
    {
        begin();
        emit("here", presence());
        send("here");
        phase = 1;
        emit("waiting", " drive=away seconds=20");
        llSetTimerEvent(1.0);
    }

    timer()
    {
        polls = polls + 1;
        integer gone = llGetAgentSize(owner()) == ZERO_VECTOR;
        if (phase == 1)
        {
            if (gone)
            {
                phase = 2;
                kept += ["gone", " after_polls=" + (string)polls + presence()];
                send("away");
                kept += ["sent-away", ""];
            }
            else if (polls > 30)
            {
                phase = 3;
                kept += ["never-gone", presence()];
            }
        }
        else if (phase == 2)
        {
            if (!gone)
            {
                phase = 3;
                kept += ["returned", presence()];
            }
        }
        if (phase == 3 || polls > 90)
        {
            llSetTimerEvent(0.0);
            integer i;
            for (i = 0; i < llGetListLength(kept); i += 2)
            {
                emit(llList2String(kept, i), llList2String(kept, i + 1));
            }
            emit("end", "");
        }
    }
}
