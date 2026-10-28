// probe: time/reset-mid-sleep
// script: Sleeper
// prim: A
// order: 1
// drive: none
//
// Sleeps five seconds when the controller asks (link message 1), resets
// its clock on 4 and reports its clock on 5, and answers a ping (3). It
// tells the controller of everything it does (link message 2), so that
// the controller's one clock times it all.
string ID = "time/reset-mid-sleep";

tell(string what)
{
    llMessageLinked(LINK_THIS, 2, what, "");
}

default
{
    state_entry()
    {
        tell("entry");
    }

    link_message(integer sender, integer num, string msg, key id)
    {
        if (num == 1)
        {
            tell("sleeping");
            llSleep(5.0);
            tell("woke");
        }
        else if (num == 3)
        {
            tell("pong");
        }
        else if (num == 4)
        {
            llResetTime();
        }
        else if (num == 5)
        {
            tell("clock:" + (string)llGetTime());
        }
    }
}
