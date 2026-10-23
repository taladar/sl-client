// probe: time/sleep-while-stopped
// script: Sleeper
// prim: A
// order: 1
// drive: none
//
// Sleeps five seconds whenever the controller asks (link message 1) and
// reports how long the sleep took by its own clock.
string ID = "time/sleep-while-stopped";

emit(string rec, string fields)
{
    llOwnerSay("PROBE " + ID + " rec=" + rec + fields);
}

default
{
    link_message(integer sender, integer num, string msg, key id)
    {
        if (num == 1)
        {
            llResetTime();
            emit("sleeping", " run=" + msg + " seconds=5.0");
            llSleep(5.0);
            emit("woke", " run=" + msg + " after=" + (string)llGetTime());
        }
    }
}
