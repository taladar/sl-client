// probe: detection/sensor
// script: Named
// prim: B
// order: 1
// distance: 2
// drive: none
//
// A sensor target of the Scanner's (see there), 2 m east of prim A. It
// names its prim "Target B", so a sweep can filter on the name, and is
// the one scripted prim of the row.
default
{
    state_entry()
    {
        llSetObjectName("Target B");
    }
}
