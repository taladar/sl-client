// probe: land/parcel
// script: Outside danger
// order: 4
// drive: none
//
// One of the Leader's neighbours in prim A (see there): it asks llScriptDanger
// of a position 10 m west of the region, once, as soon as it is saved, and
// prints the answer. It is a script of its own because OpenSim throws on some
// of these, which stops the script that asked.
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
        emit("outside", " call=llScriptDanger answer=" + (string)llScriptDanger(there));
    }
}
