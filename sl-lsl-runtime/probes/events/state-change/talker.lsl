// probe: events/state-change
// script: Talker
// prim: B
// order: 1
// distance: 5
// drive: none
//
// The voice the state-change probe listens for: says a numbered line on the
// probe's channel twice a second for thirty seconds, then stops. Touch to
// start it again. Prints nothing itself.
integer CHANNEL = -730001;
integer LINES = 60;

integer said;

default
{
    state_entry()
    {
        said = 0;
        llSetTimerEvent(0.5);
    }

    timer()
    {
        ++said;
        llSay(CHANNEL, "tick " + (string)said);
        if (said >= LINES) llSetTimerEvent(0.0);
    }

    touch_start(integer count)
    {
        llResetScript();
    }
}
