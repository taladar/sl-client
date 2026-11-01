// probe: ossl/strings
// script: Strings
// prim: A
// order: 1
// drive: none
// library: opensim
//
// OSSL's culture-sensitive string search and its in-place list sort, which
// OpenSim hands to .NET: osStringIndexOf, osStringLastIndexOf,
// osStringStartsWith and osStringEndsWith compare as the en-US culture does
// (StringComparison.CurrentCulture, and CurrentCultureIgnoreCase), not unit
// by unit; osListSortInPlace sorts the list object it is given.
//
// Link message 1: every search, both case modes, over ASCII, case, accents
// precomposed and combining, the expansions (ae, fi, ss), ignorable units
// (a noncharacter, U+0000, a soft hyphen), the Turkish and Greek letters
// whose case is special, a surrogate pair, kana; the strings are built
// with llUnescapeURL so this source and the transcript stay ASCII, and each
// record prints the lengths OpenSim counted. Then the overloads with a
// start and a count — osStringLastIndexOf reads OpenSim's start as .NET's
// end, so only a start at or past the middle of the string returns.
// Link message 2: osListSortInPlace of a local, a global, mixed types,
// strides, descending orders, a copy of a list, a parameter, and a list
// that is no variable at all.
// Link message 3: osStringLastIndexOf from a start of 0 in a longer string,
// which .NET refuses; `call` before, `returned` after.
// Link message 100: answers the leader's ping, which a script the error
// stopped does not.
string ID = "ossl/strings";
list SORTED_GLOBAL = [3, 1, 2];

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

search(string label, string s, string v)
{
    emit("search", " case=" + label
        + " len=" + (string)llStringLength(s) + "," + (string)llStringLength(v)
        + " index=" + (string)osStringIndexOf(s, v, FALSE) + ","
            + (string)osStringIndexOf(s, v, TRUE)
        + " last=" + (string)osStringLastIndexOf(s, v, FALSE) + ","
            + (string)osStringLastIndexOf(s, v, TRUE)
        + " starts=" + (string)osStringStartsWith(s, v, FALSE) + ","
            + (string)osStringStartsWith(s, v, TRUE)
        + " ends=" + (string)osStringEndsWith(s, v, FALSE) + ","
            + (string)osStringEndsWith(s, v, TRUE));
}

ranged(string label, string s, string v, integer start, integer count)
{
    emit("index-range", " case=" + label + " start=" + (string)start
        + " count=" + (string)count
        + " index=" + (string)osStringIndexOf(s, v, start, count, FALSE) + ","
            + (string)osStringIndexOf(s, v, start, count, TRUE));
}

ranged_last(string label, string s, string v, integer start, integer count)
{
    emit("last-range", " case=" + label + " start=" + (string)start
        + " count=" + (string)count
        + " last=" + (string)osStringLastIndexOf(s, v, start, count, FALSE) + ","
            + (string)osStringLastIndexOf(s, v, start, count, TRUE));
}

string types(list l)
{
    string out;
    integer i;
    for (i = 0; i < llGetListLength(l); ++i)
        out += (string)llGetListEntryType(l, i);
    return out;
}

sorted(string label, list l)
{
    emit("sort", " case=" + label + " list=" + q(llList2CSV(l))
        + " types=" + types(l));
}

sort_parameter(list p)
{
    osListSortInPlace(p, 1, TRUE);
    sorted("parameter-inside", p);
}

searches()
{
    search("ascii", "Hello World", "World");
    search("ascii-case", "Hello World", "world");
    search("repeat", "abcabc", "bc");
    search("absent", "abc", "x");
    search("empty-source", "", "a");
    search("empty-value", "abc", "");
    search("whole", "abc", "abc");
    search("longer", "ab", "abc");
    search("accent", u("caf%C3%A9"), "cafe");
    search("accent-back", "cafe", u("caf%C3%A9"));
    search("combining", u("cafe%CC%81"), u("caf%C3%A9"));
    search("combining-back", u("caf%C3%A9"), u("cafe%CC%81"));
    search("accent-case", u("%C3%89COLE"), u("%C3%A9cole"));
    search("ae", u("%C3%A6on"), "aeon");
    search("ae-back", "aeon", u("%C3%A6on"));
    search("ae-half", u("%C3%A6"), "a");
    search("fi", u("%EF%AC%81le"), "file");
    search("fi-back", "file", u("%EF%AC%81le"));
    search("ff-inside", "affix", u("%EF%AC%80"));
    search("sharp-s", u("Stra%C3%9Fe"), "STRASSE");
    search("sharp-s-back", "Strasse", u("stra%C3%9Fe"));
    search("capital-sharp-s", u("%E1%BA%9E"), u("%C3%9F"));
    search("dz", u("%C7%84"), "DZ");
    search("noncharacter", u("a%EF%B7%90b"), "ab");
    search("noncharacter-back", "ab", u("a%EF%B7%90b"));
    search("noncharacter-only", "abc", u("%EF%B7%90"));
    search("noncharacter-tail", u("abc%EF%B7%90"), "c");
    search("nul", u("a%00b"), "ab");
    search("soft-hyphen", u("a%C2%ADb"), "ab");
    search("hyphen", "co-op", "coop");
    search("dotted-capital-i", u("%C4%B0"), "i");
    search("dotless-i", u("%C4%B1"), "I");
    search("final-sigma", u("%CE%A3"), u("%CF%82"));
    search("sigma", u("%CE%A3"), u("%CF%83"));
    search("cyrillic", u("%D0%9F%D1%80%D0%B8"), u("%D0%BF%D1%80%D0%B8"));
    search("emoji", u("x%F0%9F%98%80y"), u("%F0%9F%98%80"));
    search("emoji-twice", u("%F0%9F%98%80%F0%9F%98%80"), u("%F0%9F%98%80"));
    search("fullwidth", u("%EF%BC%A1"), "A");
    search("hiragana-katakana", u("%E3%81%82"), u("%E3%82%A2"));
    search("half-width-kana", u("%EF%BD%B1"), u("%E3%82%A2"));
    search("prolonged", u("%E3%81%8B%E3%81%82"), u("%E3%81%8B%E3%83%BC"));
    search("iteration", u("%E3%81%8B%E3%81%8B"), u("%E3%81%8B%E3%82%9D"));
    search("hangul", u("%EA%B0%80"), u("%EA%B0%80"));

    string nine = "abcabcabc";
    ranged("ascii", nine, "bc", 0, 0);
    ranged("ascii", nine, "bc", 2, 3);
    ranged("ascii", nine, "bc", 3, 0);
    ranged("ascii", nine, "bc", -5, 4);
    ranged("ascii", nine, "bc", 8, 5);
    ranged("ascii", nine, "bc", 9, 1);
    ranged("ascii", nine, "BC", 4, 20);
    ranged("ascii", nine, "bc", 5, 2);
    string accents = u("caf%C3%A9 caf%C3%A9");
    ranged("accents", accents, u("CAF%C3%89"), 1, 0);
    ranged("accents", accents, u("cafe%CC%81"), 2, 6);
    ranged_last("ascii", nine, "bc", 4, 0);
    ranged_last("ascii", nine, "bc", 5, 2);
    ranged_last("ascii", nine, "bc", 6, 0);
    ranged_last("ascii", nine, "BC", 8, 3);
    ranged_last("ascii", nine, "bc", 7, 1);
    ranged_last("ascii", nine, "bc", 9, 0);
    ranged_last("ascii", nine, "bc", 12, 1);
    ranged_last("one", "a", "a", 0, 0);
    ranged_last("one", "a", "a", -3, 0);
    ranged_last("accents", accents, u("CAF%C3%89"), 6, 0);
    emit("searched", "");
}

sorts()
{
    list local = [3, 1, 2];
    osListSortInPlace(local, 1, TRUE);
    sorted("local", local);

    osListSortInPlace(SORTED_GLOBAL, 1, TRUE);
    sorted("global", SORTED_GLOBAL);

    list down = [3, 1, 2];
    osListSortInPlace(down, 1, FALSE);
    sorted("descending", down);

    list two = [3, 1, 2];
    osListSortInPlace(two, 1, 2);
    sorted("ascending-two", two);

    list mixed = [3, "b", 1.5, "a", 2, <0.0, 0.0, 2.0>, "B", -1, <1.0, 0.0, 0.0>];
    osListSortInPlace(mixed, 1, TRUE);
    sorted("mixed", mixed);

    list mixed_down = [3, "b", 1.5, "a", 2, <0.0, 0.0, 2.0>, "B", -1, <1.0, 0.0, 0.0>];
    osListSortInPlace(mixed_down, 1, FALSE);
    sorted("mixed-descending", mixed_down);

    list keyed = [(key)"b", "a", (key)"c"];
    osListSortInPlace(keyed, 1, TRUE);
    sorted("keys", keyed);

    list strided = ["b", 1, "c", 2, "a", 3];
    osListSortInPlace(strided, 2, TRUE);
    sorted("stride", strided);

    list uneven = ["b", 1, "a"];
    osListSortInPlace(uneven, 2, TRUE);
    sorted("stride-uneven", uneven);

    list pair = [2, 1];
    osListSortInPlace(pair, 2, TRUE);
    sorted("stride-whole", pair);

    list zero = [2, 1];
    osListSortInPlace(zero, 0, TRUE);
    sorted("stride-zero", zero);

    list empty = [];
    osListSortInPlace(empty, 1, TRUE);
    sorted("empty", empty);

    list original = [3, 1, 2];
    list copy = original;
    osListSortInPlace(original, 1, TRUE);
    sorted("copied-original", original);
    sorted("copied-copy", copy);

    list copied_back = [3, 1, 2];
    list other = copied_back;
    osListSortInPlace(other, 1, TRUE);
    sorted("copy-sorted-original", copied_back);
    sorted("copy-sorted-copy", other);

    list passed = [3, 1, 2];
    sort_parameter(passed);
    sorted("parameter-after", passed);

    list summed = [3, 1, 2];
    osListSortInPlace(summed + [], 1, TRUE);
    sorted("expression", summed);

    osListSortInPlace([3, 1, 2], 1, TRUE);
    emit("sorted", "");
}

default
{
    link_message(integer sender, integer num, string msg, key id)
    {
        if (num == 1) searches();
        else if (num == 2) sorts();
        else if (num == 3)
        {
            emit("call", " fn=osStringLastIndexOf start=0");
            integer r = osStringLastIndexOf("abcabc", "b", 0, 0, FALSE);
            emit("returned", " fn=osStringLastIndexOf value=" + (string)r);
        }
        else if (num == 100)
            emit("answer", " script=" + q(llGetScriptName()));
    }
}
