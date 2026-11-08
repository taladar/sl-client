// probe: prim/box-beat
// script: Child
// object: Child
// order: 1
// drive: none
//
// What the object in prim A's inventory does once rezzed (see the Leader).
// Its start parameter names it: 1 is A, 2 is B, 3 is C. It rescales itself
// once soon after the rez (0.2 s for A, 0.6 s for B, 1.5 s for C) and reads
// its box every frame until it shows the new x; says "ready" on a channel;
// and on the Leader's "go" flips its own x between 1 and 2 m twenty times the
// same way. Each record goes to the Leader on the channel, which prints it,
// since only prim A's lines are the probe's. Then it says "done" and dies.
integer CHANNEL = -7351;
string label;

// Set x and read the box every frame until it shows it; tell the Leader
// when.
string flip(float x)
{
    llSetScale(<x, 1.0, 1.0>);
    float set = llGetTime();
    string set_wall = llGetTimestamp();
    float seen = -1.0;
    string wall = "";
    integer polls = 0;
    while (seen < 0.0 && polls < 200)
    {
        vector top = llList2Vector(llGetBoundingBox(llGetKey()), 1);
        if (llFabs(top.x - x / 2.0) < 0.001)
        {
            seen = llGetTime();
            wall = llGetTimestamp();
        }
        else
        {
            llSleep(0.01);
            ++polls;
        }
    }
    return " polls=" + (string)polls + " set=" + set_wall + " wall=" + wall
        + " t=" + (string)set + " seen=" + (string)seen;
}

default
{
    on_rez(integer param)
    {
        if (param == 0)
        {
            return;
        }
        label = llList2String(["", "A", "B", "C"], param);
        string rez = llList2String(llGetObjectDetails(llGetKey(), [36]), 0);
        string stamp = llGetTimestamp();
        llSleep(llList2Float([0.0, 0.2, 0.6, 1.5], param));
        string first = flip(2.0);
        llRegionSay(CHANNEL, "rec=first label=" + label + " rez=" + rez + " stamp=" + stamp + first);
        llListen(CHANNEL, "", NULL_KEY, "go");
        llRegionSay(CHANNEL, "ready");
    }

    listen(integer channel, string name, key id, string text)
    {
        float x = 2.0;
        integer round;
        for (round = 0; round < 20; ++round)
        {
            llSleep(0.37 + 0.11 * (float)(round % 9));
            x = 3.0 - x;
            llRegionSay(CHANNEL, "rec=beat label=" + label + " round=" + (string)round + flip(x));
        }
        llRegionSay(CHANNEL, "done");
        llDie();
    }
}
