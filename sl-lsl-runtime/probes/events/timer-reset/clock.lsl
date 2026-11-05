// probe: events/timer-reset
// script: Clock
// prim: A
// order: 1
// drive: none
//
// The timer probe's clock, in its own prim: on a link message numbered
// 800 it sleeps the seconds the message names, then sends 801, which ends
// the probe's watch. The probe cannot time its watch with its own timer,
// the thing it measures. Prints nothing itself.
integer WAIT = 800;
integer DONE = 801;

default
{
    link_message(integer sender, integer num, string msg, key id)
    {
        if (num == WAIT)
        {
            llSleep((float)msg);
            llMessageLinked(LINK_THIS, DONE, "", "");
        }
    }
}
