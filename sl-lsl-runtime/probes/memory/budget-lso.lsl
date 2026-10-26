// probe: memory/budget-lso
// drive: none
// target: lsl2
//
// What the memory functions answer, and how they move as a script holds
// more. Reads llGetFreeMemory, llGetUsedMemory and llGetMemoryLimit at
// start; holds 0 to 64 values of each type in a global list, reading after
// each count, then clears it; grows a global string of ASCII, two-byte and
// three-byte characters; runs the memory profiler over a growth; and asks
// llSetMemoryLimit for limits above, at and below what the script uses,
// reading what it answered and what the limit became.
//
// The two- and three-byte characters are U+00E9 and U+20AC, made with
// llUnescapeURL so the source stays ASCII. This is memory/budget compiled
// to LSO on Second Life, line for line but the id; OpenSim has no LSO and
// runs it as it runs every script.
//
// Every read takes the three numbers into locals first, in statements of
// their own: Second Life evaluates an expression right to left, and
// building the line allocates.
string ID = "memory/budget-lso";

list held;
string text;

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

read(string rec, string fields)
{
    integer free = llGetFreeMemory();
    integer used = llGetUsedMemory();
    integer limit = llGetMemoryLimit();
    emit(rec, fields + " used=" + (string)used + " free=" + (string)free
        + " limit=" + (string)limit);
}

// Holds 0, 1, 2, 4, 16 and 64 values of one kind, reading after each.
hold(string kind)
{
    held = [];
    read("held", " kind=" + kind + " count=0");
    integer n = 0;
    integer target = 1;
    while (target <= 64)
    {
        while (n < target)
        {
            if (kind == "integer") held += [n];
            else if (kind == "float") held += [(float)n];
            else if (kind == "string") held += ["a"];
            else if (kind == "key") held += [llGetKey()];
            else if (kind == "vector") held += [<1.0, 2.0, 3.0>];
            else if (kind == "rotation") held += [<0.0, 0.0, 0.0, 1.0>];
            ++n;
        }
        read("held", " kind=" + kind + " count=" + (string)n);
        if (target == 2) target = 4;
        else if (target == 4) target = 16;
        else if (target == 16) target = 64;
        else if (target == 64) target = 128;
        else target = target + 1;
    }
    held = [];
    read("held", " kind=" + kind + " count=cleared");
}

// Grows the global string by one character at a time to each length.
grow(string kind, string c)
{
    text = "";
    read("text", " kind=" + kind + " length=0");
    integer n = 0;
    list lengths = [1, 16, 256, 1024];
    integer i;
    for (i = 0; i < llGetListLength(lengths); ++i)
    {
        integer target = llList2Integer(lengths, i);
        while (n < target)
        {
            text += c;
            ++n;
        }
        read("text", " kind=" + kind + " length=" + (string)llStringLength(text));
    }
    text = "";
    read("text", " kind=" + kind + " length=cleared");
}

ask_limit(string asked_as, integer asked)
{
    integer result = llSetMemoryLimit(asked);
    read("set-limit", " asked=" + asked_as + " value=" + (string)asked
        + " result=" + (string)result);
}

default
{
    state_entry()
    {
        emit("start", " region=" + q(llGetRegionName())
            + " channel=" + q(llGetEnv("sim_channel"))
            + " version=" + q(llGetEnv("sim_version")));
        read("read", " when=entry");
        read("read", " when=again");

        hold("integer");
        hold("float");
        hold("string");
        hold("key");
        hold("vector");
        hold("rotation");

        grow("ascii", "a");
        grow("two-byte", llUnescapeURL("%C3%A9"));
        grow("three-byte", llUnescapeURL("%E2%82%AC"));

        integer before = llGetSPMaxMemory();
        llScriptProfiler(PROFILE_SCRIPT_MEMORY);
        hold("integer");
        llScriptProfiler(PROFILE_NONE);
        integer after = llGetSPMaxMemory();
        read("profiler", " sp_max_before=" + (string)before
            + " sp_max_after=" + (string)after);

        integer used = llGetUsedMemory();
        ask_limit("65536", 65536);
        ask_limit("65537", 65537);
        ask_limit("100000", 100000);
        ask_limit("32768", 32768);
        ask_limit("used+1024", used + 1024);
        ask_limit("used", used);
        ask_limit("used-1024", used - 1024);
        ask_limit("1", 1);
        ask_limit("0", 0);
        ask_limit("-1", -1);
        ask_limit("65536", 65536);
        read("read", " when=last");
        emit("end", "");
    }
}
