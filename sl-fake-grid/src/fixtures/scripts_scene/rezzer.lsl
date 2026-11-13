// Touched, rezzes the "Spark" it holds a metre above itself, numbered by its
// start parameter, and says what it rezzed.
integer gCount;

default
{
    state_entry()
    {
        llSetText("Touch to rez a spark", <1.0, 1.0, 1.0>, 1.0);
    }

    touch_start(integer count)
    {
        ++gCount;
        llRezObject("Spark", llGetPos() + <0.0, 0.0, 1.0>, ZERO_VECTOR,
            ZERO_ROTATION, gCount);
    }

    object_rez(key child)
    {
        llSay(0, "Rezzed " + llKey2Name(child));
    }
}
