// probe: inventory/listing
// script: aaa Helper
// order: 2
// drive: none
//
// A second script beside the leader, for it to stop, start and reset: it
// prints a line each time it starts and answers a link message while it
// runs.
string ID = "inventory/listing";

default
{
    state_entry()
    {
        llOwnerSay("PROBE " + ID + " rec=helper-entry param=" + (string)llGetStartParameter());
    }

    link_message(integer sender, integer num, string text, key id)
    {
        if (text == "ping")
        {
            llOwnerSay("PROBE " + ID + " rec=helper-pong num=" + (string)num);
        }
    }
}
