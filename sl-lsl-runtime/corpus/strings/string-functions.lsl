// corpus: the everyday string functions
// Oracle: the SL wiki's per-function pages.
default
{
    state_entry()
    {
        print((string)llStringLength("héllo"));
        print((string)llSubStringIndex("haystack", "st"));
        print((string)llSubStringIndex("haystack", "needle"));
        print(llToUpper("MiXeD") + " " + llToLower("MiXeD"));
        print("[" + llStringTrim("  padded  ", STRING_TRIM) + "]");
        print("[" + llStringTrim("  padded  ", STRING_TRIM_HEAD) + "]");
        print(llInsertString("held", 2, "--"));
        print(llDumpList2String(llParseString2List("a,b;;c", [","], [";"]), "|"));
        print(llDumpList2String(llParseStringKeepNulls("a,,b", [","], []), "|"));
        print(llEscapeURL("a b&c") + " " + llUnescapeURL("a%20b"));
    }
}
