// Display: shows the colour the vendor's menu was answered with, and says
// for whom.

default
{
    link_message(integer sender, integer number, string message, key id)
    {
        llSetText("Selected: " + message, <1.0, 1.0, 1.0>, 1.0);
        llSay(0, "Showing the " + message + " gadget for " + llKey2Name(id));
    }
}
