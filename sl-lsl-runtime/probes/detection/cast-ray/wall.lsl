// probe: detection/cast-ray
// script: Wall
// prim: B
// order: 1
// distance: 5
// drive: none
//
// A ray target of the Caster's (see there), 5 m east of prim A. It names
// its prim "Wall", by which the Caster tells its hits apart.
default
{
    state_entry()
    {
        llSetObjectName("Wall");
    }
}
