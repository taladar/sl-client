// probe: errors/flood-kinds
// script: Flooder
// order: 2
// drive: none
// vary: t
// deadline: 420
//
// What Second Life's error flood limit counts. errors/flood saw an
// object's ninth refusal in a row replaced by "Too many errors...", yet
// prim/params, prim/params-rules and prim/effects passed eleven, nine and
// fifteen lines, half a second apart and less, with no more than eight of
// any one kind. Four phases, seventy seconds of quiet apart, each with a
// different name or number in every line so that the transcript says
// which line passed:
//
// A: ten missing sounds to llTriggerSound ("A1" to "A10") as fast as the
//    script runs, then four missing textures to llSetTexture ("B1" to
//    "B4"): whether another kind passes while one floods. Then one more
//    missing texture every five seconds ("C1" to "C4"), and a missing
//    sound ("A11") thirty seconds after "A10" but ten after "C4": whether
//    the quiet that ends a flood is the kind's or the object's.
// B: eight missing sounds ("D1" to "D8"), a division by zero in
//    "Faulter", then a ninth missing sound ("D9") two seconds later:
//    whether a fault's lines are counted with the refusals.
// C: five llSetPrimitiveParams rule lists ending in an unknown rule
//    (after 0 to 4 fullbright rules, so the rule number differs), then
//    five with a PRIM_MATERIAL out of range (8 to 12): whether two
//    complaints of one function are one kind, as the complaints of
//    llGetInventoryPermMask and llGetInventoryKey in inventory/listing
//    were.
// D: ten missing sounds three seconds apart ("E1" to "E10"): whether
//    spacing lets a ninth through.
string ID = "errors/flood-kinds";

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

sound(string name)
{
    llTriggerSound(name, 1.0);
}

texture(string name)
{
    llSetTexture(name, ALL_SIDES);
}

sounds(string prefix, integer from, integer to)
{
    integer i;
    for (i = from; i <= to; ++i) sound(prefix + (string)i);
}

textures(string prefix, integer from, integer to)
{
    integer i;
    for (i = from; i <= to; ++i) texture(prefix + (string)i);
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
        // Phase A: steps 1 to 6.
        if (step == 1)
        {
            phase("A");
            sounds("A", 1, 10);
            textures("B", 1, 4);
            llSetTimerEvent(5.0);
        }
        else if (step <= 5)
        {
            emit("single", " name=" + q("C" + (string)(step - 1)) + " t=" + (string)llGetTime());
            texture("C" + (string)(step - 1));
            if (step == 5) llSetTimerEvent(10.0);
        }
        else if (step == 6)
        {
            emit("single", " name=" + q("A11") + " t=" + (string)llGetTime());
            sound("A11");
            llSetTimerEvent(70.0);
        }
        // Phase B: steps 7 and 8.
        else if (step == 7)
        {
            phase("B");
            sounds("D", 1, 8);
            llMessageLinked(LINK_THIS, 1, "0", "");
            llSetTimerEvent(2.0);
        }
        else if (step == 8)
        {
            emit("single", " name=" + q("D9") + " t=" + (string)llGetTime());
            sound("D9");
            llSetTimerEvent(70.0);
        }
        // Phase C: step 9.
        else if (step == 9)
        {
            phase("C");
            integer i;
            for (i = 0; i < 5; ++i)
            {
                list rules;
                integer k;
                for (k = 0; k < i; ++k) rules += [PRIM_FULLBRIGHT, ALL_SIDES, FALSE];
                llSetPrimitiveParams(rules + [999]);
            }
            for (i = 8; i <= 12; ++i) llSetPrimitiveParams([PRIM_MATERIAL, i]);
            llSetTimerEvent(70.0);
        }
        // Phase D: steps 10 to 19.
        else if (step == 10)
        {
            phase("D");
            sound("E1");
            llSetTimerEvent(3.0);
        }
        else if (step <= 19)
        {
            emit("single", " name=" + q("E" + (string)(step - 9)) + " t=" + (string)llGetTime());
            sound("E" + (string)(step - 9));
            if (step == 19) llSetTimerEvent(3.0);
        }
        else
        {
            llSetTimerEvent(0.0);
            step = -1;
            emit("end", "");
        }
    }
}
