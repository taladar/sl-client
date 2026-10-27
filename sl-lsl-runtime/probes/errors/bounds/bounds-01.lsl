// probe: errors/bounds
// script: Bounds 1
// order: 1
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
    emit("case", " script=" + q(llGetScriptName()) + " case=list-read-past-end step=before");
    r = llList2String(l, 99) + "|" + (string)llList2Integer(l, 99)
        + "|" + (string)llList2Float(l, 99) + "|" + (string)llList2Key(l, 99)
        + "|" + (string)llList2Vector(l, 99) + "|" + (string)llList2Rot(l, 99)
        + "|" + (string)llGetListEntryType(l, 99);
    emit("case", " script=" + q(llGetScriptName()) + " case=list-read-past-end step=after result=" + q(r));
}

default
{
    link_message(integer sender, integer num, string msg, key id)
    {
        if (num == 1) run();
        else if (num == 1000) emit("answer", " script=" + q(llGetScriptName()));
    }
}
