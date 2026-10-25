// probe: events/order
// script: Talker
// prim: B
// order: 1
// distance: 3
// drive: none
//
// The order probe's voice: a separate object, since a prim does not hear
// its own chat. On the cue `now` it says a line on the probe's channel at
// once; on `late`, three seconds later. Prints nothing itself.
integer TALK = -730101;
integer CUE = -730102;

default
{
    state_entry()
    {
        llListen(CUE, "", NULL_KEY, "");
    }

    listen(integer channel, string name, key id, string message)
    {
        if (message == "now")
        {
            llSay(TALK, "now");
        }
        else if (message == "late")
        {
            llSleep(3.0);
            llSay(TALK, "late");
        }
    }
}
