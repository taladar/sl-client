// probe: vm/call-cost-lists
// vary: calls, seconds, per_second
// drive: none
//
// What a list function costs as its list grows: each function timed at
// lists of 1, 10, 100 and 500 integers, for a second and a half each, as
// calls per second. A loop with an empty body is the baseline the cost of a
// call is read against. The clock is read every ten calls rather than after
// a fixed count, since llGetTime moves in 1/45 s frames on Second Life and
// one frame is about 1.5 % of the run.
string ID = "vm/call-cost-lists";

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
list FNS = ["none", "llGetListLength", "llList2Integer", "llListSort", "llList2CSV", "llDumpList2String", "llListFindList", "llList2List", "llDeleteSubList", "llListReplaceList", "list-plus", "llListStatistics", "llCSV2List", "llListRandomize"];
list SIZES = [1, 10, 100, 500];
// The seconds each measurement runs for, and the calls between two looks
// at the clock.
float T = 1.5;
integer K = 10;
list L;
list P;
string C;
integer step;
integer warm;

// A list of `size` integers from a fixed linear congruential sequence, so
// a sort has work to do and every run sorts the same list.
fill(integer size)
{
    L = [];
    integer x = 12345;
    integer i;
    for (i = 0; i < size; ++i)
    {
        x = (x * 1103515245 + 12345) & 0x7FFFFFFF;
        L += [x % 100000];
    }
    P = [-1];
    C = llList2CSV(L);
}

// Runs function `fn` in batches of K until T seconds have passed and
// reports the rate.
measure(integer fn, integer size, float limit)
{
    integer calls;
    integer j;
    integer n;
    float f;
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
                n = llGetListLength(L);
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
                n = llList2Integer(L, -1);
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
                r = llListSort(L, 1, TRUE);
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
                s = llList2CSV(L);
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
                s = llDumpList2String(L, ",");
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
                n = llListFindList(L, P);
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
                r = llList2List(L, 0, -1);
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
                r = llDeleteSubList(L, 0, 0);
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
                r = llListReplaceList(L, P, 0, 0);
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
                r = L + P;
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
                f = llListStatistics(LIST_STAT_SUM, L);
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
                r = llCSV2List(C);
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
                r = llListRandomize(L, 1);
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
