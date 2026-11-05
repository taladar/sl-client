// probe: errors/owner-lines
// script: Speaker
// order: 2
// drive: none
//
// Six complaints two seconds apart, few enough that no flood limit cuts
// them: a missing sound (which every avatar near hears), then the five
// complaints Second Life sent the owner alone in errors/flood-paths —
// llSetPrimitiveParams' unknown rule, llParticleSystem's wrong type,
// llSetPrimMediaParams' unknown rule and a face the prim has not, and
// llGetPrimitiveParams' unknown parameter. "Listener" in prim B says
// which of them a script hears.
string ID = "errors/owner-lines";

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

default
{
    state_entry()
    {
        emit("start", " region=" + q(llGetRegionName())
            + " channel=" + q(llGetEnv("sim_channel"))
            + " version=" + q(llGetEnv("sim_version")));
        step = 0;
        llSetTimerEvent(3.0);
    }

    timer()
    {
        // XEngine still delivers a timer event queued before the timer
        // was stopped, so a finished sequence ignores it.
        if (step < 0) return;
        ++step;
        if (step == 1) llTriggerSound("no such sound", 1.0);
        else if (step == 2) llSetPrimitiveParams([999]);
        else if (step == 3) llParticleSystem([PSYS_PART_MAX_AGE, "one"]);
        else if (step == 4) llSetPrimMediaParams(0, [99, 1]);
        else if (step == 5) llSetPrimMediaParams(9, [PRIM_MEDIA_HOME_URL, "http://example.com/"]);
        else if (step == 6) llGetPrimitiveParams([999]);
        else if (step == 7)
        {
            llParticleSystem([]);
            llSetTimerEvent(5.0);
            return;
        }
        else
        {
            llSetTimerEvent(0.0);
            step = -1;
            emit("end", "");
            return;
        }
        emit("said", " step=" + (string)step);
        llSetTimerEvent(2.0);
    }
}
