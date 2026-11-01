// probe: ossl/strings
// script: Parser
// prim: A
// order: 2
// drive: none
//
// The two ll* functions OpenSim also hands to .NET's culture-sensitive
// search: llSubStringIndex (String.IndexOf) and the parsers, which search
// for each delimiter with String.IndexOf(delimiter, start) and go on a
// delimiter's length after where it matched — where an expansion matched
// (ae in the one unit of æ), that can be past the end of the string.
// Plain LSL: on Second Life these are the unit-by-unit answers.
//
// Link message 4: the searches and splits. Link message 5: a split whose
// next search would start past the end; `call` before, `returned` after.
// Link message 100: answers the leader's ping.
string ID = "ossl/strings";

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

string u(string escaped)
{
    return llUnescapeURL(escaped);
}

index(string label, string s, string v)
{
    emit("sub-string-index", " case=" + label
        + " index=" + (string)llSubStringIndex(s, v));
}

split(string label, list parts)
{
    emit("parse", " case=" + label + " count=" + (string)llGetListLength(parts)
        + " parts=" + q(llEscapeURL(llDumpList2String(parts, "|"))));
}

default
{
    link_message(integer sender, integer num, string msg, key id)
    {
        if (num == 4)
        {
            index("ae", u("%C3%A6on"), "aeon");
            index("ae-after-ascii", u("x%C3%A6on"), "aeon");
            index("ae-after-ascii-short", u("x%C3%A6"), "ae");
            index("ae-back", "xaeon", u("%C3%A6"));
            index("combining", u("cafe%CC%81"), u("caf%C3%A9"));
            index("base-of-combining", u("cafe%CC%81"), "e");
            index("sharp-s", u("Stra%C3%9Fe"), "ss");
            index("noncharacter", "abc", u("%EF%B7%90"));
            index("case", "ABC", "b");
            split("ae", llParseString2List(u("a%C3%A6b"), ["ae"], []));
            split("ae-kept", llParseStringKeepNulls(u("%C3%A6x%C3%A6y"),
                ["ae"], []));
            split("ae-spacer", llParseString2List(u("1%C3%A62"), [], ["ae"]));
            split("noncharacter", llParseStringKeepNulls("abc",
                [u("%EF%B7%90")], []));
            split("combining", llParseString2List(u("cafe%CC%81 caf%C3%A9 x"),
                [u("caf%C3%A9")], []));
            split("first-wins", llParseString2List(u("a%C3%A6b"),
                [u("%C3%A6"), "ae"], []));
            emit("parsed", "");
        }
        else if (num == 5)
        {
            emit("call", " fn=llParseString2List");
            list r = llParseString2List(u("%C3%A6"), ["ae"], []);
            emit("returned", " fn=llParseString2List count="
                + (string)llGetListLength(r));
        }
        else if (num == 100)
            emit("answer", " script=" + q(llGetScriptName()));
    }
}
