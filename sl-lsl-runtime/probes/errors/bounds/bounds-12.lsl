// probe: errors/bounds
// script: Bounds 12
// order: 12
// drive: none
//
// One of the bounds cases: a script of its own per case, since a
// fault stops a script on Second Life. It runs its case when sent
// the number its name ends in as a link message and says what it
// got, or nothing more when the case faults. Link message 1000
// pings it.
string ID = "errors/bounds";

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

run()
{
    list l = ["a", "b", "c", "d", "e", "f"];
    string r;
    emit("case", " script=" + q(llGetScriptName()) + " case=randomize-odd step=before");
    r = (string)llGetListLength(llListRandomize(l, 0)) + "|"
        + (string)llGetListLength(llListRandomize(l, -1)) + "|"
        + (string)llGetListLength(llListRandomize(l, 4));
    emit("case", " script=" + q(llGetScriptName()) + " case=randomize-odd step=after result=" + q(r));
}

default
{
    link_message(integer sender, integer num, string msg, key id)
    {
        if (num == 12) run();
        else if (num == 1000) emit("answer", " script=" + q(llGetScriptName()));
    }
}
