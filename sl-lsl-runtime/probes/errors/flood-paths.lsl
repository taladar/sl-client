// probe: errors/flood-paths
// drive: none
// vary: t, cost
// deadline: 480
//
// Which complaints Second Life's error flood limit counts, what a missing
// sound costs, and whether the limit is a window or a rate. errors/flood-kinds
// saw nine llSetPrimitiveParams complaints 0.2 seconds apart pass with no
// notice, where eight missing sounds and a fault ended in one, and ten
// missing sounds four seconds apart all pass. Every line carries its own
// name or number, so the transcript says which passed. Phases are 35 seconds
// of quiet apart, more than any limit measured so far lasts:
//
// T: what one missing sound to llTriggerSound and one missing texture to
//    llSetTexture cost the script (the "cost" fields), then a
//    PRIM_MATERIAL of 12 and of 15 alone: whether they complain at all
//    (errors/flood-kinds' fifth material, 12, said nothing).
// S: twelve llSetPrimitiveParams rule lists ending in an unknown rule, the
//    rule number 1 to 12: whether these complaints have a limit of their
//    own.
// X1 to X5: seven missing sounds, then one complaint of another path, then
//    one more missing sound: when the other complaint is counted, the last
//    sound is the ninth line and the notice; when it is not, the eighth.
//    X1 llSetPrimitiveParams' unknown rule, X2 llParticleSystem's wrong
//    type, X3 llSetPrimMediaParams' unknown rule, X4 llGetPrimitiveParams'
//    unknown parameter, X5 llSetPrimMediaParams on a face the prim has not.
// W: eight missing sounds, ten seconds of quiet, then a ninth: a count over
//    a window of twenty seconds stops it, a refill of a line every ten
//    seconds or faster lets it through.
string ID = "errors/flood-paths";

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

phase(string name)
{
    emit("phase", " name=" + name + " t=" + (string)llGetTime());
}

sounds(string prefix, integer from, integer to)
{
    integer i;
    for (i = from; i <= to; ++i) llTriggerSound(prefix + (string)i, 1.0);
}

other(integer which)
{
    if (which == 1) llSetPrimitiveParams([999]);
    else if (which == 2) llParticleSystem([PSYS_PART_MAX_AGE, "one"]);
    else if (which == 3) llSetPrimMediaParams(0, [99, 1]);
    else if (which == 4) llGetPrimitiveParams([999]);
    else llSetPrimMediaParams(9, [PRIM_MEDIA_HOME_URL, "http://example.com/"]);
    llParticleSystem([]);
}

default
{
    state_entry()
    {
        emit("start", " region=" + q(llGetRegionName())
            + " channel=" + q(llGetEnv("sim_channel"))
            + " version=" + q(llGetEnv("sim_version")));
        llResetTime();
        step = 0;
        llSetTimerEvent(3.0);
    }

    timer()
    {
        // XEngine still delivers a timer event queued before the timer
        // was stopped, so a finished sequence ignores it.
        if (step < 0) return;
        ++step;
        if (step == 1)
        {
            phase("T");
            float before = llGetTime();
            llTriggerSound("T1", 1.0);
            float middle = llGetTime();
            llSetTexture("T2", ALL_SIDES);
            float after = llGetTime();
            emit("cost", " call=sound cost=" + (string)(middle - before));
            emit("cost", " call=texture cost=" + (string)(after - middle));
            llSetPrimitiveParams([PRIM_MATERIAL, 12]);
            llSetPrimitiveParams([PRIM_MATERIAL, 15]);
            emit("material", " got=" + (string)llList2Integer(llGetPrimitiveParams([PRIM_MATERIAL]), 0));
            llSetPrimitiveParams([PRIM_MATERIAL, PRIM_MATERIAL_WOOD]);
            llSetTimerEvent(35.0);
        }
        else if (step == 2)
        {
            phase("S");
            integer i;
            for (i = 0; i < 12; ++i)
            {
                list rules;
                integer k;
                for (k = 0; k < i; ++k) rules += [PRIM_FULLBRIGHT, ALL_SIDES, FALSE];
                llSetPrimitiveParams(rules + [999]);
            }
            llSetTimerEvent(35.0);
        }
        else if (step <= 7)
        {
            integer which = step - 2;
            phase("X" + (string)which);
            string prefix = "X" + (string)which + "-";
            sounds(prefix, 1, 7);
            other(which);
            sounds(prefix, 8, 8);
            llSetTimerEvent(35.0);
        }
        else if (step == 8)
        {
            phase("W");
            sounds("W", 1, 8);
            llSetTimerEvent(10.0);
        }
        else if (step == 9)
        {
            emit("single", " name=" + q("W9") + " t=" + (string)llGetTime());
            llTriggerSound("W9", 1.0);
            llSetTimerEvent(3.0);
        }
        else
        {
            llSetTimerEvent(0.0);
            step = -1;
            emit("end", "");
        }
    }
}
