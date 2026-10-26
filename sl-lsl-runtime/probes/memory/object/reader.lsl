// probe: memory/object
// script: Reader
// order: 6
// drive: none
// vary: script_time
// stats: active-scripts
//
// What llGetObjectDetails says of an object's scripts: OBJECT_SCRIPT_MEMORY,
// OBJECT_RUNNING_SCRIPT_COUNT, OBJECT_TOTAL_SCRIPT_COUNT and
// OBJECT_SCRIPT_TIME, read of prim A as its scripts settle, after "Idle" is
// stopped, after it is started again and "Limited" has raised its limit;
// of prim B, another object with one script; of the owner, whose
// attachments are what an avatar's scripts are; and of NULL_KEY and a key
// that names nothing. The runner notes the region's active-script count
// as it changes, beside what the scripts say.
//
// Prim A holds this script, "Idle", "Limited", "Stopped" and "Idle LSO"
// (LSO on Second Life); prim B, 3 m east, holds "Other". Save them first;
// the sequence takes about fifteen seconds.
string ID = "memory/object";
list NAMES = ["Idle", "Limited", "Stopped", "Idle LSO", "Reader"];

integer step;
key other = NULL_KEY;

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

details(string of, string when, key k)
{
    list d = llGetObjectDetails(k, [OBJECT_SCRIPT_MEMORY,
        OBJECT_RUNNING_SCRIPT_COUNT, OBJECT_TOTAL_SCRIPT_COUNT,
        OBJECT_SCRIPT_TIME]);
    list names = ["script_memory", "running", "total", "script_time"];
    string fields = " of=" + of + " when=" + when
        + " length=" + (string)llGetListLength(d);
    integer i;
    for (i = 0; i < llGetListLength(d); ++i)
    {
        fields += " " + llList2String(names, i) + "=" + llList2String(d, i);
    }
    emit("details", fields);
}

states(string when)
{
    integer i;
    for (i = 0; i < llGetListLength(NAMES); ++i)
    {
        string n = llList2String(NAMES, i);
        emit("script-state", " when=" + when + " script=" + q(n)
            + " running=" + (string)llGetScriptState(n));
    }
}

default
{
    state_entry()
    {
        emit("start", " region=" + q(llGetRegionName())
            + " channel=" + q(llGetEnv("sim_channel"))
            + " version=" + q(llGetEnv("sim_version")));
        integer used = llGetUsedMemory();
        integer limit = llGetMemoryLimit();
        emit("self", " used=" + (string)used + " limit=" + (string)limit);
        llListen(-7701, "", NULL_KEY, "");
        step = 0;
        llSetTimerEvent(3.0);
    }

    timer()
    {
        ++step;
        if (step == 1)
        {
            states("settled");
            details("self", "settled", llGetKey());
            llSetScriptState("Idle", FALSE);
        }
        else if (step == 2)
        {
            states("idle-stopped");
            details("self", "idle-stopped", llGetKey());
            llSetScriptState("Idle", TRUE);
            llMessageLinked(LINK_THIS, 2, "", "");
        }
        else if (step == 3)
        {
            states("idle-started");
            details("self", "idle-started", llGetKey());
            llRegionSay(-7700, "ping");
        }
        else if (step == 4)
        {
            emit("other", " answered=" + (string)(other != NULL_KEY));
            if (other != NULL_KEY) details("other", "settled", other);
            details("owner", "settled", llGetOwner());
            details("null-key", "settled", NULL_KEY);
            details("nothing", "settled", "6a4f2c1e-0b7d-4e3a-9c58-2d1f0e9b7a63");
            llSetTimerEvent(0.0);
            emit("end", "");
        }
    }

    listen(integer channel, string name, key id, string msg)
    {
        other = (key)msg;
    }
}
