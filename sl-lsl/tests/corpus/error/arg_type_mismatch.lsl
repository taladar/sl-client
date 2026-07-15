// llSay's channel parameter is an integer; a vector cannot fill it.
default
{
    state_entry()
    {
        llSay(<1.0, 2.0, 3.0>, "bad channel");
    }
}
