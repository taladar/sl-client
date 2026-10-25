// probe: events/merging
// script: Talker
// prim: B
// order: 1
// distance: 3
// drive: none
//
// The merging probe's voice: on the cue `6` it says the line `same` five
// times in one handler, on `7` the lines `1` to `5`. Prints nothing itself.
integer TALK = -730201;
integer CUE = -730202;

default
{
    state_entry()
    {
        llListen(CUE, "", NULL_KEY, "");
    }

    listen(integer channel, string name, key id, string message)
    {
        integer i;
        if (message == "6")
        {
            for (i = 0; i < 5; ++i)
            {
                llSay(TALK, "same");
            }
        }
        else if (message == "7")
        {
            for (i = 1; i <= 5; ++i)
            {
                llSay(TALK, (string)i);
            }
        }
    }
}
