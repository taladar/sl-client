// probe: vm/region-budget
// script: Worker
// object: Worker
// order: 1
// drive: none
//
// What the object in prim A's inventory carries. Rezzed with its number
// (from 1) as the start parameter, it waits for the probe's "go" on channel
// GO, then runs an empty loop for five seconds as fast as the region lets
// it, reading the clock every hundred iterations. Then it says on channel
// REPORT how many iterations it ran, in how many seconds of llGetTime and in
// how many of real time (UTC, from llGetTimestamp, which a region's time
// dilation does not slow), and dies. It waits a twentieth of a second per
// number before it says so, so that a hundred workers' lines do not
// overflow the probe's event queue at once. "die" on GO kills a worker that
// never heard "go", and so does a timer two minutes after the rez, so that a
// run cut short leaves no workers to answer the next run's "go". Where it was
// saved (start parameter 0) it does nothing.
integer GO = -73105;
integer REPORT = -73106;
float SECONDS = 5.0;
integer K = 100;
integer me;

// Seconds since midnight UTC, to the microsecond llGetTimestamp gives.
float real()
{
    string t = llGetTimestamp();
    return (float)llGetSubString(t, 11, 12) * 3600.0
        + (float)llGetSubString(t, 14, 15) * 60.0
        + (float)llGetSubString(t, 17, -2);
}

default
{
    on_rez(integer param)
    {
        me = param;
        if (param != 0)
        {
            llListen(GO, "", NULL_KEY, "");
            llSetTimerEvent(120.0);
        }
    }

    timer()
    {
        llDie();
    }

    listen(integer channel, string name, key id, string msg)
    {
        if (msg == "die")
        {
            llDie();
        }
        if (msg != "go")
        {
            return;
        }
        integer n;
        integer j;
        float began = real();
        llResetTime();
        do
        {
            for (j = 0; j < K; ++j)
            {
            }
            n += K;
        }
        while (llGetTime() < SECONDS);
        float seconds = llGetTime();
        float elapsed = real() - began;
        if (elapsed < 0.0)
        {
            elapsed += 86400.0;
        }
        llSleep((float)me * 0.05);
        llRegionSay(REPORT, (string)me + " " + (string)n + " " + (string)seconds
            + " " + (string)elapsed);
        llDie();
    }
}
