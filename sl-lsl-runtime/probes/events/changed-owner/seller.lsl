// probe: events/changed-owner
// script: Seller
// order: 1
// drive: none
//
// The prim the other avatar buys (see the Relay). Once the Relay has asked
// for it ("who" on channel -7335) it answers with its key on -7336, and from
// then on says there every `changed` event it gets and whether the owner it
// then has is the one it started with, since what it would print to its
// owner after the sale goes to the buyer. Told "die", it deletes itself, sold
// or not, so that the run need not clear away a prim it no longer owns.
key first;

default
{
    state_entry()
    {
        first = llGetOwner();
        llListen(-7335, "", NULL_KEY, "");
    }

    listen(integer channel, string name, key id, string message)
    {
        if (message == "who")
        {
            llRegionSay(-7336, "key " + (string)llGetKey());
        }
        else if (message == "die")
        {
            llDie();
        }
    }

    changed(integer change)
    {
        llRegionSay(-7336, "changed " + (string)change + " " + (string)(llGetOwner() == first));
    }
}
