// probe: detection/cast-ray
// script: Ghost
// prim: D
// order: 2
// distance: 7.5
// drive: none
//
// A ray target of the Caster's (see there), 7.5 m east of prim A: a
// phantom prim named "Ghost", which a ray finds only with
// RC_DETECT_PHANTOM.
default
{
    state_entry()
    {
        llSetObjectName("Ghost");
        llSetStatus(STATUS_PHANTOM, TRUE);
    }
}
