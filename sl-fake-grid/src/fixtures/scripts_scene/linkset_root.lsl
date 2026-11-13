// The root of a three-prim linkset: touched (on any of its prims), it pings
// its children by link message and says which link answered, for whom.
default
{
    state_entry()
    {
        llSetText("Touch to ping the links", <1.0, 1.0, 1.0>, 1.0);
    }

    touch_start(integer count)
    {
        llMessageLinked(LINK_ALL_CHILDREN, 0, "ping", llDetectedKey(0));
    }

    link_message(integer sender, integer number, string message, key id)
    {
        if (message == "pong")
        {
            llSay(0, "Link " + (string)sender + " answered for "
                + llKey2Name(id));
        }
    }
}
