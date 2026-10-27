// probe: prim/effects
// script: Missing
// prim: B
// order: 1
// distance: 2
// drive: none
//
// The Probe's neighbour in prim B, two metres east of prim A (see there): as
// soon as it is saved it calls llParticleSystem with a list whose last rule
// has no value, then prints that it carried on. It is a script of its own
// because OpenSim reads past the end of the list and throws, which stops the
// script that asked.
string ID = "prim/effects";

emit(string rec, string fields)
{
    llOwnerSay("PROBE " + ID + " rec=" + rec + fields);
}

default
{
    state_entry()
    {
        emit("try", " label=particles-missing-value");
        llParticleSystem([PSYS_PART_MAX_AGE, 1.0, PSYS_SRC_BURST_RATE]);
        emit("carried-on", " label=particles-missing-value");
        llParticleSystem([]);
    }
}
