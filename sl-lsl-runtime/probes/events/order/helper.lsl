// probe: events/order
// script: Helper
// prim: A
// order: 2
// drive: none
//
// The order probe's hands in its own prim: on the probe's cue (a link
// message numbered 100 + the round) it sends link messages numbered for
// their place and changes the prim's colour, as the round asks. Every
// colour differs from the one before it, so that each change is one.
// Prints nothing itself.
integer GO = 100;

default
{
    link_message(integer sender, integer num, string msg, key id)
    {
        if (num == GO + 1)
        {
            llMessageLinked(LINK_THIS, 1, "", "");
            llSetColor(<1.0, 0.0, 0.0>, ALL_SIDES);
        }
        else if (num == GO + 2)
        {
            llSetColor(<0.0, 1.0, 0.0>, ALL_SIDES);
            llMessageLinked(LINK_THIS, 2, "", "");
        }
        else if (num == GO + 3)
        {
            llMessageLinked(LINK_THIS, 3, "", "");
            llSleep(0.3);
            llSetColor(<0.0, 0.0, 1.0>, ALL_SIDES);
            llSleep(0.3);
            llMessageLinked(LINK_THIS, 4, "", "");
            llSleep(0.3);
            llSetColor(<1.0, 1.0, 1.0>, ALL_SIDES);
        }
        else if (num == GO + 4)
        {
            llMessageLinked(LINK_THIS, 5, "", "");
            llSetColor(<1.0, 0.0, 0.0>, ALL_SIDES);
        }
        else if (num == GO + 5)
        {
            llSleep(2.5);
            llMessageLinked(LINK_THIS, 6, "", "");
            llSetColor(<0.0, 1.0, 0.0>, ALL_SIDES);
        }
    }
}
