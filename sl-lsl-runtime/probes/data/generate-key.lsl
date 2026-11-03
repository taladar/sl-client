// probe: data/generate-key
// drive: none
//
// What llGenerateKey hands out, measured by shape so the transcript is the
// same from run to run: 200 keys drawn in one event, their length, how many
// carry an upper-case digit, how many repeat one drawn before, which version
// and variant digits (the 15th and 20th characters) appear among them, and
// which of the 36 positions hold the same character in every key (`fixed`,
// `.` where they differ) -- a key built from a clock or a host keeps whole
// fields fixed from one draw to the next, a random one only its dashes and
// its version digit. Last, whether two calls in one expression differ.
string ID = "data/generate-key";

integer COUNT = 200;

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
    llSleep(0.05);
}

begin()
{
    emit("start", " region=" + q(llGetRegionName())
        + " channel=" + q(llGetEnv("sim_channel"))
        + " version=" + q(llGetEnv("sim_version")));
}

// The distinct characters of `s`, sorted.
string sorted_chars(string s)
{
    list chars = [];
    integer i;
    integer n = llStringLength(s);
    for (i = 0; i < n; ++i)
    {
        chars += [llGetSubString(s, i, i)];
    }
    return llDumpList2String(llListSort(chars, 1, TRUE), "");
}

default
{
    state_entry()
    {
        begin();
        list keys = [];
        integer i;
        for (i = 0; i < COUNT; ++i)
        {
            keys += [(string)llGenerateKey()];
        }

        string first = llList2String(keys, 0);
        integer upper = 0;
        string versions = "";
        string variants = "";
        integer invalid = 0;
        for (i = 0; i < COUNT; ++i)
        {
            string k = llList2String(keys, i);
            if (k != llToLower(k))
            {
                ++upper;
            }
            // `!` takes no key on Second Life (a type mismatch); `if` does.
            if ((key)k)
            {
            }
            else
            {
                ++invalid;
            }
            string version = llGetSubString(k, 14, 14);
            if (llSubStringIndex(versions, version) == -1)
            {
                versions += version;
            }
            string variant = llGetSubString(k, 19, 19);
            if (llSubStringIndex(variants, variant) == -1)
            {
                variants += variant;
            }
        }

        list ordered = llListSort(keys, 1, TRUE);
        integer repeats = 0;
        for (i = 1; i < COUNT; ++i)
        {
            if (llList2String(ordered, i) == llList2String(ordered, i - 1))
            {
                ++repeats;
            }
        }
        emit("shape", " count=" + (string)COUNT
            + " length=" + (string)llStringLength(first)
            + " upper=" + (string)upper
            + " invalid=" + (string)invalid
            + " repeats=" + (string)repeats);
        emit("digits", " version=" + q(sorted_chars(versions))
            + " variant=" + q(sorted_chars(variants)));

        string fixed = "";
        integer p;
        for (p = 0; p < 36; ++p)
        {
            string c = llGetSubString(first, p, p);
            integer same = TRUE;
            for (i = 1; i < COUNT && same; ++i)
            {
                if (llGetSubString(llList2String(keys, i), p, p) != c)
                {
                    same = FALSE;
                }
            }
            if (same)
            {
                fixed += c;
            }
            else
            {
                fixed += ".";
            }
        }
        emit("fixed", " mask=" + q(fixed));

        emit("pair", " differ=" + (string)(llGenerateKey() != llGenerateKey()));
        emit("end", "");
    }
}
