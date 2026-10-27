// probe: inventory/newer
// script: Probe
// order: 2
// drive: none
//
// The task-inventory functions Second Life added since OpenSim's library was
// written, which OpenSim does not compile (its compile errors are the
// measurement there). Prim A holds a notecard (Lines: one, two, three) and
// an object (Child, see there). llGetInventoryDesc and
// llGetInventoryAcquireTime of the notecard, the object, this script and a
// name that is not there; llGetNotecardLineSync of line 0 before any read,
// then after llGetNotecardLine's answer lines 0, 2, 3 and -1, and of a
// notecard that is not there; llRezObjectWithParams with a start parameter,
// a start string, a relative position and REZ_FLAG_TEMP, then with a start
// string longer than its 1024 bytes and with an unknown option;
// llDerezObject of the child, of it again, and of NULL_KEY. What a grid
// says about a call is its debug-channel chat, which the runner notes beside
// the `try` record printed before it.
string ID = "inventory/newer";

integer CHANNEL = -7342;
integer step;
key child;

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

try(string label)
{
    emit("try", " label=" + label);
}

string sync(string data)
{
    if (data == EOF) return "EOF";
    if (data == NAK) return "NAK";
    return q(data);
}

default
{
    state_entry()
    {
        begin();
        llListen(CHANNEL, "", NULL_KEY, "");
        list names = ["Lines", "Child", "Probe", "nope"];
        integer i;
        for (i = 0; i < 4; ++i)
        {
            string name = llList2String(names, i);
            string acquired = llGetInventoryAcquireTime(name);
            // The time itself changes with every run; its shape does not.
            emit("item", " name=" + name + " desc=" + q(llGetInventoryDesc(name))
                + " acquired_length=" + (string)llStringLength(acquired)
                + " acquired_shape=" + q(llGetSubString(acquired, 4, 4) + llGetSubString(acquired, 10, 10)
                    + llGetSubString(acquired, -1, -1)));
        }
        try("sync-before");
        emit("sync", " case=before line=0 value=" + sync(llGetNotecardLineSync("Lines", 0)));
        llGetNotecardLine("Lines", 0);
    }

    dataserver(key id, string data)
    {
        emit("async", " value=" + q(data));
        list lines = [0, 2, 3, -1];
        integer i;
        for (i = 0; i < 4; ++i)
        {
            integer line = llList2Integer(lines, i);
            try("sync-" + (string)line);
            emit("sync", " case=after line=" + (string)line
                + " value=" + sync(llGetNotecardLineSync("Lines", line)));
        }
        try("sync-none");
        emit("sync", " case=none line=0 value=" + sync(llGetNotecardLineSync("Nope", 0)));
        llSetTimerEvent(3.0);
        try("rez-with-params");
        llRezObjectWithParams("Child", [REZ_POS, <0.0, 0.0, 1.0>, TRUE, FALSE, REZ_PARAM, 42,
            REZ_PARAM_STRING, "hello", REZ_FLAGS, REZ_FLAG_TEMP]);
    }

    timer()
    {
        if (step > 4) return;
        ++step;
        if (step == 1)
        {
            try("rez-long-string");
            string long = "x";
            while (llStringLength(long) < 1100) long += long;
            llRezObjectWithParams("Child", [REZ_POS, <0.0, 0.0, 1.0>, TRUE, FALSE,
                REZ_PARAM_STRING, long, REZ_FLAGS, REZ_FLAG_TEMP]);
        }
        else if (step == 2)
        {
            try("rez-unknown-option");
            llRezObjectWithParams("Child", [REZ_POS, <0.0, 0.0, 1.0>, TRUE, FALSE, 999, 1,
                REZ_FLAGS, REZ_FLAG_TEMP]);
        }
        else if (step == 3)
        {
            try("derez");
            emit("derez", " case=child result=" + (string)llDerezObject(child, DEREZ_DIE));
        }
        else if (step == 4)
        {
            try("derez-again");
            emit("derez", " case=again result=" + (string)llDerezObject(child, DEREZ_DIE));
            try("derez-null");
            emit("derez", " case=null result=" + (string)llDerezObject(NULL_KEY, DEREZ_DIE));
        }
        else
        {
            llSetTimerEvent(0.0);
            emit("end", "");
        }
    }

    object_rez(key id)
    {
        emit("object-rez", " step=" + (string)step);
    }

    listen(integer channel, string name, key id, string text)
    {
        list f = llParseStringKeepNulls(text, ["|"], []);
        if (step == 0) child = id;
        string start = llList2String(f, 2);
        emit("on-rez", " step=" + (string)step + " param=" + llList2String(f, 1)
            + " start_length=" + (string)llStringLength(start)
            + " start_head=" + q(llGetSubString(start, 0, 7)));
    }
}
