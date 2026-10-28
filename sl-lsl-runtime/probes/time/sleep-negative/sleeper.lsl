// probe: time/sleep-negative
// vary: took
// script: Sleeper
// prim: A
// order: 1
// drive: none
//
// Sleeps -1.0 seconds when the leader asks (link message 1) and says how
// long it took by the timestamp, in milliseconds, then that it carried on
// (XEngine throws on a negative sleep, which ends the event).
string ID = "time/sleep-negative";

emit(string rec, string fields)
{
    llOwnerSay("PROBE " + ID + " rec=" + rec + fields);
}

// Microseconds within the current ten minutes, by the timestamp.
integer micro()
{
    string t = llGetTimestamp();
    integer m = (integer)llGetSubString(t, 15, 15);
    integer s = (integer)llGetSubString(t, 17, 18);
    string f = "000000";
    integer dot = llSubStringIndex(t, ".");
    if (dot >= 0) f = llGetSubString(t, dot + 1, -2) + "000000";
    return (m * 60 + s) * 1000000 + (integer)llGetSubString(f, 0, 5);
}

default
{
    link_message(integer sender, integer num, string msg, key id)
    {
        if (num != 1) return;
        integer start = micro();
        llSleep(-1.0);
        integer took = micro() - start;
        if (took < 0) took += 600000000;
        emit("slept", " asked=-1.0 took=" + (string)(took / 1000.0) + " unit=ms");
        emit("carried-on", "");
    }
}
