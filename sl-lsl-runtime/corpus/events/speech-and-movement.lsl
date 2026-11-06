// corpus: what a script says and does reaches the host as it was asked: the
// four chat volumes, a line to the owner, and a move kept out of the ground
// ticks: 8
default
{
    state_entry()
    {
        llWhisper(0, "whispered");
        llSay(5, "said");
        llShout(0, "shouted");
        llRegionSay(9, "region");
        llOwnerSay("to the owner");
        llSetPos(llGetPos() + <1, 0, 0>);
        llSetPos(<128, 128, 0>);
        print((string)llGetPos());
    }
}
