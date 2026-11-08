// probe: events/latency
// script: Echo
// prim: A
// order: 2
// drive: none
//
// The latency probe's link-message echo, in the probe's own prim: a link
// message numbered 2 is answered with one numbered 3, at once; one numbered
// 4 writes its text into the prim's description, which the probe polls
// while it is still busy in the handler that sent it; one numbered 5 waits
// the seconds its text names and then sends one numbered 6. Prints nothing
// itself.
default
{
    link_message(integer sender, integer num, string msg, key id)
    {
        if (num == 2)
        {
            llMessageLinked(LINK_THIS, 3, msg, "");
        }
        else if (num == 4)
        {
            llSetObjectDesc(msg);
        }
        else if (num == 5)
        {
            llSleep((float)msg);
            llMessageLinked(LINK_THIS, 6, msg, "");
        }
    }
}
