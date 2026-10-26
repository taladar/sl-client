// probe: money/pay-price
// script: No money
// prim: B
// distance: 3
// order: 1
// drive: none
//
// Prim B's only script, which has no money event: it sets a pay price, so
// that the leader can have the runner ask prim B for its buttons and see
// whether a grid answers what a script without a money event set.
string ID = "money/pay-price";

emit(string rec, string fields)
{
    llOwnerSay("PROBE " + ID + " rec=" + rec + fields);
}

default
{
    state_entry()
    {
        llSetPayPrice(9, [1, 2, 3, 4]);
        emit("set", " prim=B price=9 buttons=1,2,3,4");
    }
}
