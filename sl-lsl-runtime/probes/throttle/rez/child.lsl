// probe: throttle/rez
// script: Spark
// object: Spark
// order: 1
// drive: none
//
// What the object in prim A's inventory carries: rezzed, it dies at once.
// It says nothing, so the burst leaves no chat and no debris; its
// state_entry, which a grid that starts the script afresh on a rez runs too,
// does nothing (where it was saved, it must not die).
default
{
    on_rez(integer param)
    {
        llDie();
    }
}
