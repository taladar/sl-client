// corpus: a setter that changes the prim raises changed in its script: on
// OpenSim in the call, ahead of what the handler does next; on Second Life
// two ticks on (`events/latency`)
// ticks: 4
// at 3: changed 4
default
{
    state_entry()
    {
        llSetColor(<1, 0, 0>, ALL_SIDES);
        print("colour set");
    }

    changed(integer change)
    {
        print("changed " + (string)change);
    }
}
