// A child of the three-prim linkset: answers its root's ping.
default
{
    link_message(integer sender, integer number, string message, key id)
    {
        if (message == "ping")
        {
            llMessageLinked(LINK_ROOT, 0, "pong", id);
        }
    }
}
