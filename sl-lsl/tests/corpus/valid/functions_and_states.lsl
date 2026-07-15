// A user function, a global, two states and a reachable transition between
// them — the shape the semantic pass's central "valid script is clean" case
// exercises, kept in step with the grid's own front-end via tailslide.
integer gCounter;

string greet(key who)
{
    return "hello " + llKey2Name(who);
}

default
{
    state_entry()
    {
        llSetTimerEvent(5.0);
        llSay(0, greet(llGetOwner()));
    }

    touch_start(integer total)
    {
        integer i;
        for (i = 0; i < total; ++i)
        {
            gCounter += 1;
            llSay(0, "touched");
        }
        state waiting;
    }
}

state waiting
{
    state_entry()
    {
        state default;
    }
}
