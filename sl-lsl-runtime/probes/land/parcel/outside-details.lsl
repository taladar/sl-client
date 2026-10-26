// probe: land/parcel
// script: Outside details
// order: 2
// drive: none
//
// One of the Leader's neighbours in prim A (see there): it asks
// llGetParcelDetails (the area and the parcel id) of a position 10 m west of
// the region, once, as soon as it is saved, and prints the answer. It is a
// script of its own because OpenSim throws on some of these, which stops the
// script that asked.
string ID = "land/parcel";

string q(string s)
{
    s = llDumpList2String(llParseStringKeepNulls(s, ["\\"], []), "\\\\");
    s = llDumpList2String(llParseStringKeepNulls(s, ["\""], []), "\\\"");
    s = llDumpList2String(llParseStringKeepNulls(s, ["\n"], []), "\\n");
    return "\"" + s + "\"";
}

emit(string rec, string fields)
{
    llOwnerSay("PROBE " + ID + " rec=" + rec + fields);
}

default
{
    state_entry()
    {
        vector here = llGetPos();
        vector there = <-10.0, here.y, here.z>;
        list got = llGetParcelDetails(there, [4, 5]);
        emit("outside", " call=llGetParcelDetails length=" + (string)llGetListLength(got)
            + " area=" + q(llList2String(got, 0))
            + " id_set=" + (string)(llList2Key(got, 1) != NULL_KEY)
            + " same_as_west_edge=" + (string)(llList2Key(got, 1)
                == llList2Key(llGetParcelDetails(<0.0, here.y, here.z>, [5]), 0)));
    }
}
