// probe: memory/object
// script: Other
// prim: B
// order: 1
// distance: 3
// drive: none
//
// The one script of another object, which answers the reader's ping on
// channel -7700 with its object's key on -7701.

default
{
    state_entry()
    {
        llListen(-7700, "", NULL_KEY, "ping");
    }

    listen(integer channel, string name, key id, string msg)
    {
        llRegionSay(-7701, (string)llGetKey());
    }
}
