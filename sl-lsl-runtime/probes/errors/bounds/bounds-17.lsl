// probe: errors/bounds
// script: Bounds 17
// order: 17
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

string d(list x)
{
    return "[" + llList2CSV(x) + "]";
}

run()
{
    string r;
    emit("case", " script=" + q(llGetScriptName()) + " case=parse-limits step=before");
    r = d(llParseString2List("a1b2c3d4e5f6g7h8i9j", ["1", "2", "3", "4", "5", "6", "7", "8", "9"], []))
        + d(llParseString2List("abc", [""], [])) + d(llParseString2List("a,b", [","], [""]));
    emit("case", " script=" + q(llGetScriptName()) + " case=parse-limits step=after result=" + q(r));
}

default
{
    link_message(integer sender, integer num, string msg, key id)
    {
        if (num == 17) run();
        else if (num == 1000) emit("answer", " script=" + q(llGetScriptName()));
    }
}
