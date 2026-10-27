// probe: strings/newer
// drive: none
//
// The string and list functions Second Life added that OpenSim does not
// compile (its compile error is the measurement there): llReplaceSubString
// by count from either end and with overlapping matches,
// llList2ListSlice, llListFindStrided and llListSortStrided with negative
// indices and stride indices, llComputeHash by every algorithm and an
// unknown one, llSHA256String, llHMAC by algorithm, and llLinear2sRGB /
// llsRGB2Linear at their ends. Strides of zero are errors/bounds-newer's.
string ID = "strings/newer";

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

string dump(list l)
{
    return llDumpList2String(l, "|");
}

default
{
    state_entry()
    {
        begin();
        string s = "aaa-b-aaa";
        out("replace", "all", llReplaceSubString(s, "a", "x", 0));
        out("replace", "first-2", llReplaceSubString(s, "a", "x", 2));
        out("replace", "last-2", llReplaceSubString(s, "a", "x", -2));
        out("replace", "overlap-aa", llReplaceSubString(s, "aa", "y", 0));
        out("replace", "overlap-aa-from-end", llReplaceSubString(s, "aa", "y", -1));
        out("replace", "grow", llReplaceSubString(s, "-", "--", 0));
        out("replace", "to-empty", llReplaceSubString(s, "a", "", 0));
        out("replace", "astral", llReplaceSubString("a😀b😀", "😀", "c", 0));
        list l = [1, "a", 2, "b", 3, "c", 4, "d"];
        out("slice", "stride-2-index-1", dump(llList2ListSlice(l, 0, -1, 2, 1)));
        out("slice", "stride-2-index-minus-1", dump(llList2ListSlice(l, 0, -1, 2, -1)));
        out("slice", "stride-2-index-2", dump(llList2ListSlice(l, 0, -1, 2, 2)));
        out("slice", "stride-2-range-2-5", dump(llList2ListSlice(l, 2, 5, 2, 0)));
        out("slice", "stride-2-reversed", dump(llList2ListSlice(l, 5, 2, 2, 0)));
        out("slice", "stride-3-odd", dump(llList2ListSlice(l, 0, -1, 3, 0)));
        out("find-strided", "b-stride-2", (string)llListFindStrided(l, ["b"], 0, -1, 2));
        out("find-strided", "3-stride-2", (string)llListFindStrided(l, [3], 0, -1, 2));
        out("find-strided", "3-stride-2-from-5", (string)llListFindStrided(l, [3], 5, -1, 2));
        out("find-strided", "3-stride-2-start-1", (string)llListFindStrided(l, [3], 1, -1, 2));
        out("find-strided", "pair-stride-2", (string)llListFindStrided(l, [2, "b"], 0, -1, 2));
        out("find-strided", "negative-range", (string)llListFindStrided(l, [4], -4, -1, 2));
        list m = [3, "c", 1, "z", 2, "a"];
        out("sort-strided", "by-1-ascending", dump(llListSortStrided(m, 2, 1, TRUE)));
        out("sort-strided", "by-minus-1-descending", dump(llListSortStrided(m, 2, -1, FALSE)));
        out("sort-strided", "by-0", dump(llListSortStrided(m, 2, 0, TRUE)));
        out("sort-strided", "by-2-past-stride", dump(llListSortStrided(m, 2, 2, TRUE)));
        out("sort-strided", "odd-length", dump(llListSortStrided([3, "c", 1, "z", 2], 2, 0, TRUE)));
        list algs = ["md5", "sha1", "sha224", "sha256", "sha384", "sha512", "SHA1", "md4", ""];
        integer i;
        for (i = 0; i < llGetListLength(algs); ++i)
        {
            string a = llList2String(algs, i);
            out("compute-hash", a, llComputeHash("abc", a));
        }
        out("compute-hash", "sha1-accented", llComputeHash("é", "sha1"));
        out("sha256", "abc", llSHA256String("abc"));
        out("sha256", "accented", llSHA256String("é"));
        list hmacs = ["md5", "sha1", "sha224", "sha256", "sha384", "sha512", "SHA256", "md4"];
        for (i = 0; i < llGetListLength(hmacs); ++i)
        {
            string a = llList2String(hmacs, i);
            out("hmac", a, llHMAC("key", "abc", a));
        }
        out("hmac", "empty-key", llHMAC("", "abc", "sha1"));
        vector half = <0.5, 0.0, 1.0>;
        vector over = <2.0, -1.0, 0.0031308>;
        out("linear2srgb", "0.5,0,1", (string)llLinear2sRGB(half));
        out("linear2srgb", "2,-1,0.0031308", (string)llLinear2sRGB(over));
        out("srgb2linear", "0.5,0,1", (string)llsRGB2Linear(half));
        out("srgb2linear", "2,-1,0.04045", (string)llsRGB2Linear(<2.0, -1.0, 0.04045>));
        emit("end", "");
    }
}
