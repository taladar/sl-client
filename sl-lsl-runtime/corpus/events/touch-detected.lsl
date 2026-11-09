// corpus: a touch reaches touch_start and touch_end, with the toucher in the
// detected block, and llDetected* reads only inside the event: outside it
// a name is NULL_KEY as text on Second Life and "" on OpenSim, as past the
// end of the block (detection/sensor on both engines)
// ticks: 5
// at 2: touch
string outside = "unread";

default
{
    state_entry()
    {
        outside = llDetectedName(0);
    }

    touch_start(integer count)
    {
        print("touch_start " + (string)count + " " + llDetectedName(0) + " " + (string)llDetectedKey(0));
        print("type " + (string)llDetectedType(0) + " owner? " + (string)(llDetectedKey(0) == llGetOwner()));
    }

    touch_end(integer count)
    {
        print("touch_end " + (string)count);
    }
}
