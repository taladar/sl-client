// Sweeps four metres around itself once a second and says who came near and
// who went away.
list gNear;

default
{
    state_entry()
    {
        llSetText("Come near", <1.0, 1.0, 1.0>, 1.0);
        llSensorRepeat("", NULL_KEY, AGENT, 4.0, PI, 1.0);
    }

    sensor(integer count)
    {
        list now;
        integer i;
        for (i = 0; i < count; ++i)
        {
            string name = llDetectedName(i);
            now += [name];
            if (llListFindList(gNear, [name]) == -1)
            {
                llSay(0, name + " came near");
            }
        }
        for (i = 0; i < llGetListLength(gNear); ++i)
        {
            string name = llList2String(gNear, i);
            if (llListFindList(now, [name]) == -1)
            {
                llSay(0, name + " went away");
            }
        }
        gNear = now;
    }

    no_sensor()
    {
        integer i;
        for (i = 0; i < llGetListLength(gNear); ++i)
        {
            llSay(0, llList2String(gNear, i) + " went away");
        }
        gNear = [];
    }
}
