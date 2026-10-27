// probe: errors/bounds
// script: Bounds 3
// order: 3
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
    string r;
    emit("case", " script=" + q(llGetScriptName()) + " case=list-read-other-type step=before");
    list m = ["<1,2,3>", "<1,2,3,4>", "0x10", 1.9, "1e3", 5, <1.0, 2.0, 3.0>, " 7 apples"];
    r = (string)llList2Vector(m, 0) + "|" + (string)llList2Rot(m, 1)
        + "|" + (string)llList2Integer(m, 2) + "|" + (string)llList2Integer(m, 3)
        + "|" + (string)llList2Float(m, 4) + "|" + (string)llList2Key(m, 5)
        + "|" + (string)llList2Integer(m, 6) + "|" + (string)llList2Integer(m, 7);
    emit("case", " script=" + q(llGetScriptName()) + " case=list-read-other-type step=after result=" + q(r));
}

default
{
    link_message(integer sender, integer num, string msg, key id)
    {
        if (num == 3) run();
        else if (num == 1000) emit("answer", " script=" + q(llGetScriptName()));
    }
}
