// probe: events/merging
// script: Helper
// prim: A
// order: 2
// drive: none
//
// The merging probe's hands in its own prim: on the probe's cue (a link
// message numbered 100 + the round) it changes the prim, or sends link
// messages, as the round asks. Prints nothing itself.
integer GO = 100;

default
{
    link_message(integer sender, integer num, string msg, key id)
    {
        if (num == GO + 1)
        {
            llSetColor(<1.0, 0.0, 0.0>, ALL_SIDES);
            llSetColor(<0.0, 1.0, 0.0>, ALL_SIDES);
            llSetColor(<0.0, 0.0, 1.0>, ALL_SIDES);
            llSetColor(<1.0, 1.0, 0.0>, ALL_SIDES);
            llSetColor(<0.0, 1.0, 1.0>, ALL_SIDES);
        }
        else if (num == GO + 2)
        {
            llSetColor(<0.0, 1.0, 1.0>, ALL_SIDES);
            llSetColor(<0.0, 1.0, 1.0>, ALL_SIDES);
            llSetColor(<0.0, 1.0, 1.0>, ALL_SIDES);
        }
        else if (num == GO + 3)
        {
            llSetColor(<1.0, 0.0, 1.0>, ALL_SIDES);
            llSetScale(<0.6, 0.6, 0.6>);
            llSetTexture(TEXTURE_BLANK, ALL_SIDES);
        }
        else if (num == GO + 4)
        {
            llSetColor(<1.0, 0.0, 0.0>, ALL_SIDES);
            llSleep(0.5);
            llSetColor(<0.0, 1.0, 0.0>, ALL_SIDES);
            llSleep(0.5);
            llSetColor(<0.0, 0.0, 1.0>, ALL_SIDES);
        }
        else if (num == GO + 5)
        {
            integer i;
            for (i = 0; i < 5; ++i)
            {
                llMessageLinked(LINK_THIS, 7, "same", "");
            }
        }
    }
}
