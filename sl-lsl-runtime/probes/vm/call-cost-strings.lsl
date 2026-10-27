// probe: vm/call-cost-strings
// vary: calls, seconds, per_second
// drive: none
//
// What a string function costs as its string grows: each function timed at
// strings of 1, 10, 100 and 1000 characters ("ab," repeated), for a second
// and a half each, as calls per second, against a loop with an empty body.
// The clock is read every ten calls, as in vm/call-cost-lists.
string ID = "vm/call-cost-strings";

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

// What each measurement runs, in order.
list FNS = ["none", "llStringLength", "llGetSubString", "llSubStringIndex", "llParseString2List", "llToUpper", "llStringToBase64", "llMD5String", "llSHA1String", "llEscapeURL", "string-plus", "llInsertString", "llDeleteSubString", "llStringTrim"];
list SIZES = [1, 10, 100, 1000];
// The seconds each measurement runs for, and the calls between two looks
// at the clock.
float T = 1.5;
integer K = 10;
string S;
integer step;
integer warm;

// A string of `size` characters: "ab," repeated, cut to length, so a
// parse has separators to find and a search for "#" finds nothing.
fill(integer size)
{
    S = "";
    while (llStringLength(S) < size)
    {
        S += "ab,";
    }
    S = llGetSubString(S, 0, size - 1);
}

// Runs function `fn` in batches of K until T seconds have passed and
// reports the rate.
measure(integer fn, integer size, float limit)
{
    integer calls;
    integer j;
    integer n;
    list r;
    string s;
    llResetTime();
    if (fn == 0)
    {
        do
        {
            for (j = 0; j < K; ++j)
            {
            }
            calls += K;
        }
        while (llGetTime() < limit);
    }
    else if (fn == 1)
    {
        do
        {
            for (j = 0; j < K; ++j)
            {
                n = llStringLength(S);
            }
            calls += K;
        }
        while (llGetTime() < limit);
    }
    else if (fn == 2)
    {
        do
        {
            for (j = 0; j < K; ++j)
            {
                s = llGetSubString(S, -1, -1);
            }
            calls += K;
        }
        while (llGetTime() < limit);
    }
    else if (fn == 3)
    {
        do
        {
            for (j = 0; j < K; ++j)
            {
                n = llSubStringIndex(S, "#");
            }
            calls += K;
        }
        while (llGetTime() < limit);
    }
    else if (fn == 4)
    {
        do
        {
            for (j = 0; j < K; ++j)
            {
                r = llParseString2List(S, [","], []);
            }
            calls += K;
        }
        while (llGetTime() < limit);
    }
    else if (fn == 5)
    {
        do
        {
            for (j = 0; j < K; ++j)
            {
                s = llToUpper(S);
            }
            calls += K;
        }
        while (llGetTime() < limit);
    }
    else if (fn == 6)
    {
        do
        {
            for (j = 0; j < K; ++j)
            {
                s = llStringToBase64(S);
            }
            calls += K;
        }
        while (llGetTime() < limit);
    }
    else if (fn == 7)
    {
        do
        {
            for (j = 0; j < K; ++j)
            {
                s = llMD5String(S, 0);
            }
            calls += K;
        }
        while (llGetTime() < limit);
    }
    else if (fn == 8)
    {
        do
        {
            for (j = 0; j < K; ++j)
            {
                s = llSHA1String(S);
            }
            calls += K;
        }
        while (llGetTime() < limit);
    }
    else if (fn == 9)
    {
        do
        {
            for (j = 0; j < K; ++j)
            {
                s = llEscapeURL(S);
            }
            calls += K;
        }
        while (llGetTime() < limit);
    }
    else if (fn == 10)
    {
        do
        {
            for (j = 0; j < K; ++j)
            {
                s = S + "x";
            }
            calls += K;
        }
        while (llGetTime() < limit);
    }
    else if (fn == 11)
    {
        do
        {
            for (j = 0; j < K; ++j)
            {
                s = llInsertString(S, 0, "x");
            }
            calls += K;
        }
        while (llGetTime() < limit);
    }
    else if (fn == 12)
    {
        do
        {
            for (j = 0; j < K; ++j)
            {
                s = llDeleteSubString(S, 0, 0);
            }
            calls += K;
        }
        while (llGetTime() < limit);
    }
    else if (fn == 13)
    {
        do
        {
            for (j = 0; j < K; ++j)
            {
                s = llStringTrim(S, STRING_TRIM);
            }
            calls += K;
        }
        while (llGetTime() < limit);
    }
    float seconds = llGetTime();
    if (warm)
    {
        emit("rate", " fn=" + llList2String(FNS, fn) + " size=" + (string)size
            + " calls=" + (string)calls + " seconds=" + (string)seconds
            + " per_second=" + (string)llRound((float)calls / seconds));
    }
}

default
{
    state_entry()
    {
        begin();
        // A warm-up pass at the second size first: the first run after a
        // save is slower while Mono compiles.
        fill(llList2Integer(SIZES, 1));
        integer fn;
        for (fn = 0; fn < llGetListLength(FNS); ++fn)
        {
            measure(fn, llList2Integer(SIZES, 1), 0.1);
        }
        warm = TRUE;
        step = 0;
        llMessageLinked(LINK_THIS, 0, "", "");
    }

    // One measurement per event, so no event runs long.
    link_message(integer sender, integer num, string msg, key id)
    {
        integer per = llGetListLength(FNS);
        integer count = per * llGetListLength(SIZES);
        if (step >= count)
        {
            // The empty loop again, to see whether the rate drifted.
            fill(1);
            measure(0, 0, T);
            emit("end", "");
            return;
        }
        integer size = llList2Integer(SIZES, step / per);
        integer fn = step % per;
        if (fn == 0) fill(size);
        measure(fn, size, T);
        ++step;
        llMessageLinked(LINK_THIS, 0, "", "");
    }
}
