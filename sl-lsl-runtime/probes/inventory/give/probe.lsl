// probe: inventory/give
// script: Probe
// order: 4
// drive: touch
// vary: delay
//
// Giving from prim A's inventory, which holds a notecard (Gift), the script
// Passed and the script Loaded. Prim B (Receiver, 2 m east, the same owner)
// prints its inventory on every `changed` (see there). Two seconds apart:
// llGiveInventory of Gift to prim B, and of Gift again (a name B holds
// already); of the script Passed (whether it runs there); of a name that is
// not there; of Gift to the owner, and llGiveInventoryList of Gift and
// Passed to the owner as a folder and to prim B; llRemoteLoadScriptPin of
// Loaded into prim B with its pin, running, start parameter 77; with a
// wrong pin; and with the pin again, not running, 78. Then the other avatar
// touches prim A and is given Gift. An offer to an avatar is the runner's
// `# im` note. What a grid says about a call is its debug-channel chat,
// which the runner notes beside the `try` record printed before it. `delay`
// is how long the call itself took.
string ID = "inventory/give";

key receiver;
integer step;
float before;

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
    before = llGetTime();
}

done(string label)
{
    emit("called", " label=" + label + " delay=" + (string)(llGetTime() - before));
}

default
{
    state_entry()
    {
        begin();
        // Prim B names itself when its script starts, which is before this
        // one is saved.
        llSensor("Receiver", NULL_KEY, PASSIVE | ACTIVE | SCRIPTED, 10.0, PI);
    }

    sensor(integer n)
    {
        receiver = llDetectedKey(0);
        emit("found", " n=" + (string)n);
        llSetTimerEvent(2.0);
    }

    no_sensor()
    {
        emit("found", " n=0");
        emit("end", "");
    }

    timer()
    {
        // XEngine runs a timer event queued before the timer was stopped:
        // the wait for the touch, and the end, take none.
        if (step == 11 || step > 12) return;
        ++step;
        if (step == 1)
        {
            try("give-notecard");
            llGiveInventory(receiver, "Gift");
            done("give-notecard");
        }
        else if (step == 2)
        {
            try("give-notecard-again");
            llGiveInventory(receiver, "Gift");
            done("give-notecard-again");
        }
        else if (step == 3)
        {
            try("give-script");
            llGiveInventory(receiver, "Passed");
            done("give-script");
        }
        else if (step == 4)
        {
            try("give-none");
            llGiveInventory(receiver, "Nope");
            done("give-none");
        }
        else if (step == 5)
        {
            try("give-owner");
            llGiveInventory(llGetOwner(), "Gift");
            done("give-owner");
        }
        else if (step == 6)
        {
            try("give-list-owner");
            llGiveInventoryList(llGetOwner(), "Probe folder", ["Gift", "Passed"]);
            done("give-list-owner");
        }
        else if (step == 7)
        {
            try("give-list-prim");
            llGiveInventoryList(receiver, "Probe folder", ["Gift"]);
            done("give-list-prim");
        }
        else if (step == 8)
        {
            try("remote-load");
            llRemoteLoadScriptPin(receiver, "Loaded", 1234, TRUE, 77);
            done("remote-load");
        }
        else if (step == 9)
        {
            try("remote-load-wrong-pin");
            llRemoteLoadScriptPin(receiver, "Loaded", 999, TRUE, 77);
            done("remote-load-wrong-pin");
        }
        else if (step == 10)
        {
            try("remote-load-stopped");
            llRemoteLoadScriptPin(receiver, "Loaded", 1234, FALSE, 78);
            done("remote-load-stopped");
        }
        else if (step == 11)
        {
            llSetTimerEvent(0.0);
            emit("waiting", " drive=touch by=other");
        }
        else if (step == 13)
        {
            llSetTimerEvent(0.0);
            emit("end", "");
        }
    }

    touch_start(integer n)
    {
        if (step != 11) return;
        step = 12;
        try("give-other");
        llGiveInventory(llDetectedKey(0), "Gift");
        done("give-other");
        llSetTimerEvent(3.0);
    }

    changed(integer change)
    {
        emit("changed", " change=" + (string)change + " step=" + (string)step);
    }
}
