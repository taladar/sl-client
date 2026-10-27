// probe: strings/unicode
// drive: none
//
// llChar, llOrd and llHash, which both grids compile: code points across
// the planes and the surrogate range, llOrd of a character outside the
// Basic Multilingual Plane and at each index into one, a round trip of
// llChar and llOrd, and llHash of empty, ASCII, accented and astral text.
// Out-of-range indices and code points are errors/bounds-newer's.
string ID = "strings/unicode";

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

begin()
{
    emit("start", " region=" + q(llGetRegionName())
        + " channel=" + q(llGetEnv("sim_channel"))
        + " version=" + q(llGetEnv("sim_version")));
}

out(string rec, string name, string value)
{
    emit(rec, " case=" + q(name) + " value=" + q(value));
}

default
{
    state_entry()
    {
        begin();
        list points = [65, 233, 0x7FF, 0x800, 0xFFFF, 0x10000, 0x1F600, 0x10FFFF,
            0xD800, 0xDFFF, 0xFFFE];
        integer i;
        for (i = 0; i < llGetListLength(points); ++i)
        {
            integer p = llList2Integer(points, i);
            string c = llChar(p);
            out("char", (string)p, llEscapeURL(c) + " length=" + (string)llStringLength(c)
                + " back=" + (string)llOrd(c, 0));
        }
        string astral = "a😀b";
        out("ord", "a-smile-b 0", (string)llOrd(astral, 0));
        out("ord", "a-smile-b 1", (string)llOrd(astral, 1));
        out("ord", "a-smile-b 2", (string)llOrd(astral, 2));
        out("ord", "a-smile-b 3", (string)llOrd(astral, 3));
        out("ord", "a-smile-b -1", (string)llOrd(astral, -1));
        out("ord", "a-smile-b length", (string)llStringLength(astral));
        list texts = ["", "a", "abc", "é", "😀", "The quick brown fox"];
        for (i = 0; i < llGetListLength(texts); ++i)
        {
            string t = llList2String(texts, i);
            out("hash", llEscapeURL(t), (string)llHash(t));
        }
        emit("end", "");
    }
}
