// probe: strings/functions
// drive: none
//
// The string and list library on text and lists a grid may treat
// differently: characters outside ASCII and outside the Basic Multilingual
// Plane (length, cutting, case), the whitespace llStringTrim removes, URL
// escaping and unescaping, Base64 of text and of bytes that are not UTF-8,
// llXorBase64 with keys shorter and longer than the text, the hashes,
// llParseString2List and llParseStringKeepNulls with separators and spacers
// that overlap, llListSort of mixed types and of vectors, llListStatistics
// of mixed types, llListFindList across types, and llSubStringIndex of
// empty strings. Indices out of range are errors/bounds'. What is cut out
// of text outside the Basic Multilingual Plane is printed as the Base64 of
// its UTF-8: OpenSim counts UTF-16 units and may hand back half a surrogate
// pair, which breaks the chat line it is said in.
string ID = "strings/functions";

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

// A list with each element's type, so "1" and 1 read apart.
string typed(list l)
{
    string s;
    integer i;
    for (i = 0; i < llGetListLength(l); ++i)
    {
        if (i) s += "|";
        s += (string)llGetListEntryType(l, i) + ":" + llList2String(l, i);
    }
    return s;
}

default
{
    state_entry()
    {
        begin();
        string accented = "aäßéÿǅ";
        string astral = "a𝄞b😀";
        out("length", "aäßéÿǅ", (string)llStringLength(accented));
        out("length", "a-clef-b-smile", (string)llStringLength(astral));
        out("substring", "a-clef-b-smile 1..1", llStringToBase64(llGetSubString(astral, 1, 1)));
        out("substring", "a-clef-b-smile 1..2", llStringToBase64(llGetSubString(astral, 1, 2)));
        out("substring", "a-clef-b-smile -1..-1", llStringToBase64(llGetSubString(astral, -1, -1)));
        out("index", "b in a-clef-b-smile", (string)llSubStringIndex(astral, "b"));
        out("upper", "aäßéÿǅ", llToUpper(accented));
        out("lower", "AÄÉŸǅİ", llToLower("AÄÉŸǅİ"));
        out("upper", "a-clef-b-smile", llStringToBase64(llToUpper(astral)));
        string ws = "\t \n" + llUnescapeURL("%0B%0C%C2%A0%E3%80%80") + " a b " + llUnescapeURL("%0D%0B");
        out("trim", "both", llEscapeURL(llStringTrim(ws, STRING_TRIM)));
        out("trim", "head", llEscapeURL(llStringTrim(ws, STRING_TRIM_HEAD)));
        out("trim", "tail", llEscapeURL(llStringTrim(ws, STRING_TRIM_TAIL)));
        out("trim", "mode-7", llEscapeURL(llStringTrim(ws, 7)));
        out("escape", "ascii", llEscapeURL("a b/c?d=e&f~g*h-i_j.k!l'm(n)o+p,q;r:s@t#u$v%w"));
        out("escape", "accented", llEscapeURL("é😀"));
        out("unescape", "plus", llUnescapeURL("a+b%20c"));
        out("unescape", "bad", llEscapeURL(llUnescapeURL("%zz%4%e9%C3%A9%")));
        out("unescape", "lowercase", llUnescapeURL("%c3%a9"));
        out("base64", "accented", llStringToBase64("é😀"));
        out("base64", "empty", llStringToBase64(""));
        out("from-base64", "not-utf8", llEscapeURL(llBase64ToString("/w==")));
        out("from-base64", "invalid", llEscapeURL(llBase64ToString("a!b")));
        out("from-base64", "unpadded", llBase64ToString("YWJj"));
        out("from-base64", "truncated", llBase64ToString("YWJjZA"));
        out("from-base64", "nul", llEscapeURL(llBase64ToString("YQBi")));
        out("integer-base64", "-1", llIntegerToBase64(-1));
        out("integer-base64", "1", llIntegerToBase64(1));
        out("base64-integer", "AAAAAQ==", (string)llBase64ToInteger("AAAAAQ=="));
        out("base64-integer", "AQ==", (string)llBase64ToInteger("AQ=="));
        out("base64-integer", "AQAAAAAA", (string)llBase64ToInteger("AQAAAAAA"));
        out("base64-integer", "junk", (string)llBase64ToInteger("!!!!"));
        string text = llStringToBase64("hello world");
        out("xor", "short-key", llXorBase64(text, llStringToBase64("ab")));
        out("xor", "long-key", llXorBase64(text, llStringToBase64("a much longer key than text")));
        out("xor", "empty-key", llXorBase64(text, ""));
        out("xor", "padded-key", llXorBase64(text, "YQ=="));
        out("md5", "abc", llMD5String("abc", 0));
        out("md5", "abc nonce 5", llMD5String("abc", 5));
        out("md5", "accented", llMD5String("é", 0));
        out("sha1", "abc", llSHA1String("abc"));
        out("sha1", "accented", llSHA1String("é"));
        out("parse", "keep-spacers", typed(llParseString2List("a,b;;c", [","], [";"])));
        out("parse", "overlap", typed(llParseString2List("a--b-c", ["-"], ["--"])));
        out("parse", "overlap-reversed", typed(llParseString2List("a--b-c", ["--"], ["-"])));
        out("parse", "longer-first", typed(llParseString2List("abcabc", ["b", "bc"], [])));
        out("parse", "nulls", typed(llParseStringKeepNulls(",a,,b,", [","], [])));
        out("parse", "nulls-empty", typed(llParseStringKeepNulls("", [","], [])));
        out("parse", "drop-empty", typed(llParseString2List(",a,,b,", [","], [])));
        out("parse", "spaces", typed(llParseString2List(" a  b ", [" "], [])));
        out("parse", "numbers-stay-strings", typed(llParseString2List("1,2.5", [","], [])));
        out("csv", "commas-in-strings", llList2CSV(["a,b", " c ", "<1,2>"]));
        out("csv", "brackets", typed(llCSV2List("<1,2>,<a,<b>,c>,d")));
        out("csv", "quotes", typed(llCSV2List("\"a,b\",c\"d")));
        list mixed = [3, "b", 1.5, "a", 2, <1.0, 0.0, 0.0>, "B", -1, <0.0, 0.0, 2.0>];
        out("sort", "mixed-ascending", typed(llListSort(mixed, 1, TRUE)));
        out("sort", "mixed-descending", typed(llListSort(mixed, 1, FALSE)));
        out("sort", "strings", typed(llListSort(["b", "B", "a", "A", "é", "10", "9", ""], 1, TRUE)));
        out("sort", "vectors", typed(llListSort([<3.0, 0.0, 0.0>, <0.0, 1.0, 0.0>, <1.0, 1.0, 1.0>, <0.0, 0.0, -2.0>], 1, TRUE)));
        out("sort", "rotations", typed(llListSort([<0.0, 0.0, 0.0, 2.0>, <1.0, 0.0, 0.0, 0.0>, <0.0, 0.0, 0.0, 1.0>], 1, TRUE)));
        out("sort", "keys-and-strings", typed(llListSort([(key)"b", "a", (key)"A", "c"], 1, TRUE)));
        out("sort", "stride-2", typed(llListSort([2, "x", 1, "y", 2, "a", 1, "b"], 2, TRUE)));
        out("sort", "integers-and-floats", typed(llListSort([2, 1.5, 1, 2.5, -1.0, 0], 1, TRUE)));
        list stats = [1, 2.5, "3", (key)"4", <1.0, 2.0, 3.0>, 4];
        list ops = [LIST_STAT_RANGE, LIST_STAT_MIN, LIST_STAT_MAX, LIST_STAT_MEAN,
            LIST_STAT_STD_DEV, LIST_STAT_SUM, LIST_STAT_SUM_SQUARES,
            LIST_STAT_NUM_COUNT, LIST_STAT_GEOMETRIC_MEAN];
        // No median: OpenSim's throws on a list with anything not a number
        // in it (errors/bounds).
        integer i;
        for (i = 0; i < llGetListLength(ops); ++i)
        {
            out("statistics", "mixed op " + (string)llList2Integer(ops, i),
                (string)llListStatistics(llList2Integer(ops, i), stats));
        }
        out("statistics", "geometric-mean-with-negative",
            (string)llListStatistics(LIST_STAT_GEOMETRIC_MEAN, [-1, 4]));
        out("statistics", "median-even", (string)llListStatistics(LIST_STAT_MEDIAN, [4, 1, 3, 2]));
        out("find", "1-in-floats", (string)llListFindList([1.0, 1], [1]));
        out("find", "1.0-in-ints", (string)llListFindList([1, 1.0], [1.0]));
        out("find", "string-1-in-ints", (string)llListFindList([1, "1"], ["1"]));
        out("find", "key-as-string", (string)llListFindList([(key)NULL_KEY, NULL_KEY], [NULL_KEY]));
        out("find", "string-as-key", (string)llListFindList([NULL_KEY, (key)NULL_KEY], [(key)NULL_KEY]));
        out("find", "sublist", (string)llListFindList([1, 2, 3, 2, 3], [2, 3]));
        out("find", "minus-zero", (string)llListFindList([0.0], [-0.0]));
        out("index", "empty-in-empty", (string)llSubStringIndex("", ""));
        out("index", "empty-in-abc", (string)llSubStringIndex("abc", ""));
        out("index", "abc-in-empty", (string)llSubStringIndex("", "abc"));
        out("dump", "nested-types", llDumpList2String([1, 1.5, "s", <1.0, 2.0, 3.0>, NULL_KEY], "|"));
        out("insert", "accented", llStringToBase64(llInsertString("aé", 1, "😀")));
        out("delete", "astral", llStringToBase64(llDeleteSubString(astral, 1, 1)));
        out("random", "length-and-sum", (string)llGetListLength(llListRandomize([1, 2, 3, 4, 5, 6], 2))
            + " " + (string)llListStatistics(LIST_STAT_SUM, llListRandomize([1, 2, 3, 4, 5, 6], 2)));
        emit("end", "");
    }
}
