// A spread of event handlers with their correct grid arity, so a wrong
// event-name or wrong event-arity regression would surface as a diff.
default
{
    state_entry()
    {
        llListen(0, "", NULL_KEY, "");
    }

    listen(integer channel, string name, key id, string message)
    {
        llSay(channel, message);
    }

    touch_start(integer total_number)
    {
        llSay(0, (string)total_number);
    }

    timer()
    {
        llSetTimerEvent(0.0);
    }

    changed(integer change)
    {
        if (change & CHANGED_OWNER)
        {
            llResetScript();
        }
    }
}
