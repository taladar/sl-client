// probe: money/pay-price
// script: Mixed
// prim: C
// distance: 6
// order: 2
// drive: none
//
// Prim C's only script: llSetPayPrice with buttons that are not all
// integers — a string, a float, a vector and an integer. It has a prim of its
// own because a grid may stop the script at the call (OpenSim reads each
// button as an integer), which would end any probe that went on after it;
// the leader has the runner ask prim C what the call left.
string ID = "money/pay-price";

emit(string rec, string fields)
{
    llOwnerSay("PROBE " + ID + " rec=" + rec + fields);
}

default
{
    state_entry()
    {
        emit("mixed", " prim=C call=before");
        llSetPayPrice(7, ["5", 2.5, <1.0, 2.0, 3.0>, 8]);
        emit("mixed", " prim=C call=after");
    }

    money(key id, integer amount)
    {
    }
}
