// Says who touched it, which face and where on that face.
default
{
    state_entry()
    {
        llSetText("Touch me", <1.0, 1.0, 1.0>, 1.0);
    }

    touch_start(integer count)
    {
        integer i;
        for (i = 0; i < count; ++i)
        {
            llSay(0, llDetectedName(i) + " touched face "
                + (string)llDetectedTouchFace(i) + " at "
                + (string)llDetectedTouchST(i));
        }
    }
}
