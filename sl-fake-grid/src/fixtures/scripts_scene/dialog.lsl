// Menus whoever touches it for a colour, takes the colour that comes back and
// says who chose it.
integer CHANNEL = -7072;

default
{
    state_entry()
    {
        llSetText("Touch for a menu", <1.0, 1.0, 1.0>, 1.0);
        llListen(CHANNEL, "", NULL_KEY, "");
    }

    touch_start(integer count)
    {
        llDialog(llDetectedKey(0), "Pick a colour", ["Red", "Green", "Blue"],
            CHANNEL);
    }

    listen(integer channel, string name, key id, string message)
    {
        if (message == "Red")
        {
            llSetColor(<1.0, 0.0, 0.0>, ALL_SIDES);
        }
        else if (message == "Green")
        {
            llSetColor(<0.0, 1.0, 0.0>, ALL_SIDES);
        }
        else if (message == "Blue")
        {
            llSetColor(<0.0, 0.0, 1.0>, ALL_SIDES);
        }
        llSay(0, name + " chose " + message);
    }
}
